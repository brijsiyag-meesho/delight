//! Quitting and restarting — cleanly, or after a plugin crash that left GPUI
//! unusable.

use delight_core::Settings;
use gpui::App;

/// A clean exit: the next launch shows no crash note.
pub fn quit(cx: &mut App) {
    delight_core::guard::clear_crash_note();
    cx.quit();
}

/// Restarts the app — the only way to pick up rebuilt plugin files. GPUI
/// does it as for Zed: it quits, waits until this process has exited, then
/// opens the app again.
pub fn restart(cx: &mut App) {
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
