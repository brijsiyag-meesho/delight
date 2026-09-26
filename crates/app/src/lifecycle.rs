//! Quitting and restarting — cleanly, or after a plugin crash that left GPUI
//! unusable.

use std::fs::{File, OpenOptions, TryLockError};
use std::io::Write as _;
use std::time::{Duration, Instant};

use anyhow::bail;
use delight_core::Settings;
use gpui::App;

/// The argument a restarted Delight is started with (outside a bundle): it
/// waits for the one that started it to exit.
pub const RESTARTED: &str = "--restarted";

/// How long a restarted Delight waits for the previous one to exit.
const RESTART_WAIT: Duration = Duration::from_secs(10);

/// Makes this the only Delight running: locks `delight.lock` in Delight's
/// folder while the process runs (the system releases it when the process
/// ends, however it ends). Two would both answer the launcher shortcut and
/// fight over whose launcher is in front, so it never closes. A restarted
/// Delight (`wait`) waits for the previous one to let go. The error says
/// which process is running already.
pub fn claim_single_instance(wait: bool) -> anyhow::Result<File> {
    let path = Settings::instance_lock_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut file = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(&path)?;
    let deadline = Instant::now() + RESTART_WAIT;
    loop {
        match file.try_lock() {
            Ok(()) => {
                file.set_len(0)?;
                write!(file, "{}", std::process::id())?;
                return Ok(file);
            }
            Err(TryLockError::WouldBlock) if wait && Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(TryLockError::WouldBlock) => {
                let pid = std::fs::read_to_string(&path).unwrap_or_default();
                bail!("Delight is already running (process {})", pid.trim())
            }
            Err(TryLockError::Error(e)) => return Err(e.into()),
        }
    }
}

/// The process id of the Delight holding `delight.lock`, if one is running.
pub fn running_instance() -> Option<u32> {
    let path = Settings::instance_lock_path();
    let file = File::open(&path).ok()?;
    match file.try_lock() {
        // Free: nobody's running (the lock goes with `file`).
        Ok(()) => None,
        Err(_) => std::fs::read_to_string(&path).ok()?.trim().parse().ok(),
    }
}

/// A clean exit: the input is kept for the next launch (if the history is
/// on), and the next launch shows no crash note.
pub fn quit(cx: &mut App) {
    crate::launcher::hide(cx);
    delight_core::guard::clear_crash_note();
    cx.quit();
}

/// Restarts the app — the only way to pick up rebuilt plugin files. In
/// Delight.app, GPUI does it as for Zed: it quits, waits until this process
/// has exited, then opens the app again. A binary outside a bundle (`cargo
/// run`) starts itself instead, with this process's environment (`open`
/// would start it in Terminal, without the path to Rust's `libstd` that
/// `cargo run` set), and quits; the new one waits for this one to exit.
/// Replacing the process (exec) doesn't work: macOS keeps the process's menu
/// bar icon and windows tied to the old program.
pub fn restart(cx: &mut App) {
    let exe = std::env::current_exe().unwrap_or_default();
    if exe.to_string_lossy().contains(".app/Contents/MacOS/") {
        crate::launcher::hide(cx);
        delight_core::guard::clear_crash_note();
        cx.restart();
        return;
    }
    match std::process::Command::new(&exe).arg(RESTARTED).spawn() {
        Ok(_) => quit(cx),
        Err(e) => log::error!("restarting {}: {e}", exe.display()),
    }
}

/// `delight_core::guard`'s fatal handler: a plugin panicked on the main
/// thread, so GPUI's state can't be trusted anymore. Turns the plugin off,
/// leaves a note for the next launch and exits — without touching GPUI,
/// which is why settings are read from and written to disk directly.
pub fn fatal_plugin_crash(plugin_id: &str, plugin_name: &str, message: &str) {
    let mut settings = Settings::load();
    settings.disabled_plugins.insert(plugin_id.to_string());
    settings.crashed_plugins.insert(plugin_id.to_string(), message.to_string());
    if let Err(e) = settings.save() {
        log::error!("saving settings failed: {e:#}");
    }
    delight_core::guard::write_crash_note(&format!(
        "{plugin_name} crashed and was turned off: {message}. Turn it back on in Settings → Plugins."
    ));
    std::process::exit(1);
}
