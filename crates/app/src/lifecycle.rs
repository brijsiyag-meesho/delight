//! Quitting and restarting — cleanly, or after a plugin crash that left GPUI
//! unusable.

use std::fs::{File, OpenOptions, TryLockError};
use std::io::Write as _;

use anyhow::bail;
use delight_core::Settings;
use gpui::App;

/// Makes this the only Delight running: locks `delight.lock` in Delight's
/// folder while the process runs (the system releases it when the process
/// ends, however it ends). Two would both answer the launcher shortcut and
/// fight over whose launcher is in front, so it never closes. The error
/// says which process is running already.
pub fn claim_single_instance() -> anyhow::Result<File> {
    let path = Settings::instance_lock_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut file = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(&path)?;
    match file.try_lock() {
        Ok(()) => {
            file.set_len(0)?;
            write!(file, "{}", std::process::id())?;
            Ok(file)
        }
        Err(TryLockError::WouldBlock) => {
            let pid = std::fs::read_to_string(&path).unwrap_or_default();
            bail!("Delight is already running (process {})", pid.trim())
        }
        Err(TryLockError::Error(e)) => Err(e.into()),
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

/// Restarts the app — the only way to pick up rebuilt plugin files. GPUI
/// does it as for Zed: it quits, waits until this process has exited, then
/// opens the app again.
pub fn restart(cx: &mut App) {
    crate::launcher::hide(cx);
    delight_core::guard::clear_crash_note();
    cx.restart();
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
