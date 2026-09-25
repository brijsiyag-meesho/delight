//! Delight — a Spotlight-style developer toolbox for macOS.
//!
//! ⌘⇧Space opens a floating input; every tool is a plugin written against
//! `delight-sdk` — built in, or a plugin file loaded from the plugins folder.

mod lifecycle;
mod login;
mod platform;
mod tray;

use delight_core::Settings;
use futures::StreamExt;
use gpui::{App, Application, Global};
use tray_icon::TrayIcon;

use crate::tray::TrayCommand;

/// Keeps the menu bar icon alive for the life of the app.
struct MenuBarIcon {
    _icon: Option<TrayIcon>,
}

impl Global for MenuBarIcon {}

fn main() {
    // Delight's warnings and errors on stderr; e.g. `DELIGHT_LOG=delight=debug`
    // for more, `DELIGHT_LOG=debug` to include GPUI's.
    env_logger::Builder::from_env(env_logger::Env::new().filter_or("DELIGHT_LOG", "delight=warn")).init();
    if let Some(exit_code) = command_line() {
        std::process::exit(exit_code);
    }
    // Plugin panics are caught (delight_core::guard); one that leaves GPUI
    // unusable turns the plugin off and quits the app.
    delight_core::guard::install(Some(Settings::crash_note_path()));
    delight_core::guard::set_fatal_handler(lifecycle::fatal_plugin_crash);
    let settings = Settings::load();
    login::apply(settings.open_at_login);

    Application::new().run(|cx: &mut App| {
        platform::set_accessory_app();
        let icon = tray::create().map_err(|e| log::error!("menu bar icon unavailable: {e:#}")).ok();
        cx.set_global(MenuBarIcon { _icon: icon });

        // Waits for menu clicks and handles each on the main thread.
        let mut clicks = tray::clicks();
        cx.spawn(async move |cx| {
            while let Some(command) = clicks.next().await {
                if cx.update(|cx| handle_menu_click(command, cx)).is_err() {
                    break;
                }
            }
        })
        .detach();
    });
}

fn handle_menu_click(command: TrayCommand, cx: &mut App) {
    match command {
        TrayCommand::Restart => lifecycle::restart(cx),
        TrayCommand::Quit => lifecycle::quit(cx),
    }
}

/// Commands for plugin tooling (the `delight` CLI), answered by this exact
/// app build without starting the UI:
///
/// * `--sdk-build-id` — prints the SDK build plugins must match.
/// * `--check-plugin <dylib>` — loads a plugin like the app does and prints
///   its id, or why it can't load.
///
/// Returns the exit code, or `None` to start the app.
fn command_line() -> Option<i32> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match args[..] {
        ["--sdk-build-id"] => {
            println!("{}", delight_sdk::BUILD_ID);
            Some(0)
        }
        ["--check-plugin", path] => Some(check_plugin(path)),
        _ => None,
    }
}

fn check_plugin(path: &str) -> i32 {
    // Absolute: the hardened runtime refuses to load from a relative path.
    let loaded = std::fs::canonicalize(path).map_err(anyhow::Error::from).and_then(|path| delight_core::native::load(&path));
    match loaded {
        Ok(plugin) => {
            println!("ok: {} {}", plugin.manifest().id, plugin.manifest().version);
            0
        }
        Err(e) => {
            eprintln!("error: {e:#}");
            1
        }
    }
}
