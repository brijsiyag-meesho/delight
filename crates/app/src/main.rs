//! Delight — a Spotlight-style developer toolbox for macOS.
//!
//! ⌘⇧Space opens a floating input; every tool is a plugin written against
//! `delight-sdk` — built in, or a plugin file loaded from the plugins folder.

mod boundary;
mod hotkey;
mod launcher;
mod lifecycle;
mod login;
mod platform;
mod state;
mod tray;

use delight_core::Settings;
use futures::StreamExt;
use futures::channel::mpsc::UnboundedReceiver;
use global_hotkey::GlobalHotKeyManager;
use gpui::{App, Application, Global, KeyBinding, actions};
use tray_icon::TrayIcon;

use crate::state::AppState;
use crate::tray::TrayCommand;

actions!(delight, [Quit]);

/// Keeps the menu bar icon and the global shortcut alive for the life of
/// the app.
struct Native {
    _icon: Option<TrayIcon>,
    _hotkey: Option<GlobalHotKeyManager>,
}

impl Global for Native {}

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

    Application::new().with_assets(delight_ui::Assets).run(move |cx: &mut App| {
        platform::set_accessory_app();
        delight_ui::init(cx, state::theme_mode(settings.appearance));
        launcher::bind_keys(cx);
        cx.bind_keys([KeyBinding::new("cmd-q", Quit, None)]);
        cx.on_action(|_: &Quit, cx| lifecycle::quit(cx));

        state::init(cx, settings);
        match launcher::open(cx) {
            Ok(handle) => cx.global_mut::<AppState>().launcher = Some(handle),
            Err(e) => {
                log::error!("opening the launcher window: {e:#}");
                cx.quit();
                return;
            }
        }

        let shortcut = hotkey::register(&state::settings(cx).launcher_shortcut)
            .map_err(|e| log::error!("no launcher shortcut: {e:#}"))
            .ok();
        let shortcut_label = shortcut.as_ref().map(|s| hotkey::label(s.hotkey)).unwrap_or_default();
        let icon = tray::create(&shortcut_label).map_err(|e| log::error!("menu bar icon unavailable: {e:#}")).ok();
        run_on_main_thread(cx, tray::clicks(), handle_menu_click);
        let manager = shortcut.map(|shortcut| {
            run_on_main_thread(cx, shortcut.presses, |(), cx| launcher::toggle(cx));
            shortcut.manager
        });
        cx.set_global(Native { _icon: icon, _hotkey: manager });
        cx.activate(true);
    });
}

/// Delivers each event from a background channel (menu clicks, shortcut
/// presses) to `handle` on the main thread, where app code runs, as it
/// arrives.
fn run_on_main_thread<T: 'static>(cx: &mut App, mut events: UnboundedReceiver<T>, handle: fn(T, &mut App)) {
    cx.spawn(async move |cx| {
        while let Some(event) = events.next().await {
            if cx.update(|cx| handle(event, cx)).is_err() {
                break;
            }
        }
    })
    .detach();
}

fn handle_menu_click(command: TrayCommand, cx: &mut App) {
    match command {
        TrayCommand::Open => launcher::show(cx),
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
