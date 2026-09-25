//! Delight — a Spotlight-style developer toolbox for macOS.
//!
//! ⌘⇧Space opens a floating input; every tool is a plugin written against
//! `delight-sdk` — built in, or a dylib loaded from the plugins folder.

mod boundary;
mod highlight;
mod launcher;
mod login;
mod logger;
mod platform;
mod settings_window;
mod state;
mod tray;

use std::time::Duration;

use delight_core::Settings;
use delight_sdk::{assets, editor, theme};
use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use gpui::{App, Application, Global, KeyBinding, actions};

use crate::state::AppState;
use crate::tray::{Tray, TrayCommand};

actions!(delight, [Quit]);

/// Native handles that must stay alive for the life of the app.
struct Native {
    tray: Option<Tray>,
    _hotkeys: Option<GlobalHotKeyManager>,
    hotkey_id: u32,
}

impl Global for Native {}

fn main() {
    logger::init();
    if let Some(code) = command_line() {
        std::process::exit(code);
    }
    Settings::migrate_legacy();
    // Panic guards: plugin panics are caught (delight_core::guard); ones that
    // leave GPUI unusable turn the plugin off and relaunch the app.
    delight_core::guard::install(Some(Settings::crash_note_path()));
    delight_core::guard::set_fatal_handler(state::fatal_plugin_crash);
    let settings = Settings::load();
    // Keeps the login item pointing at this copy of the app (or removes it).
    login::apply(settings.open_at_login);
    Application::new().with_assets(assets::Assets).run(move |cx: &mut App| {
        platform::set_accessory_app();
        theme::init(cx, settings.theme);
        editor::bind_keys(cx);
        launcher::bind_keys(cx);
        settings_window::bind_keys(cx);
        cx.bind_keys([KeyBinding::new("cmd-q", Quit, None)]);
        cx.on_action(|_: &Quit, cx| quit(cx));

        state::init(cx, settings);

        match launcher::open(cx) {
            Ok(handle) => cx.global_mut::<AppState>().launcher = Some(handle),
            Err(e) => {
                log::error!("failed to open launcher window: {e:#}");
                cx.quit();
                return;
            }
        }

        let tray = Tray::new().map_err(|e| log::error!("menu bar icon unavailable: {e:#}")).ok();
        let hotkey = HotKey::new(Some(Modifiers::SUPER | Modifiers::SHIFT), Code::Space);
        let hotkeys = GlobalHotKeyManager::new()
            .and_then(|m| m.register(hotkey).map(|_| m))
            .map_err(|e| log::error!("could not register ⌘⇧Space: {e}"))
            .ok();
        cx.set_global(Native { tray, _hotkeys: hotkeys, hotkey_id: hotkey.id() });

        // tray-icon and global-hotkey deliver events on channels; drain them
        // on the main thread.
        cx.spawn(async move |cx| {
            loop {
                cx.background_executor().timer(Duration::from_millis(40)).await;
                if cx.update(pump_native_events).is_err() {
                    break;
                }
            }
        })
        .detach();

        cx.activate(true);
    });
}

/// Non-GUI commands for plugin tooling (`scripts/check-kit.sh`, the `delight`
/// CLI), run against this exact app build:
///
/// * `--sdk-build-id` — prints the SDK build plugins must match.
/// * `--check-plugin <dylib>` — loads a plugin like the app does; prints its
///   id, or why it can't load (exit code 1).
///
/// Returns the exit code, or `None` to start the app.
fn command_line() -> Option<i32> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["--sdk-build-id"] => {
            println!("{}", delight_sdk::BUILD_ID);
            Some(0)
        }
        // Absolute: the hardened runtime refuses to load from a relative path.
        ["--check-plugin", path] => match std::fs::canonicalize(path)
            .map_err(anyhow::Error::from)
            .and_then(|path| delight_core::native::load(&path))
        {
            Ok(plugin) => {
                println!("ok: {} {}", plugin.manifest().id, plugin.manifest().version);
                Some(0)
            }
            Err(e) => {
                eprintln!("error: {e:#}");
                Some(1)
            }
        },
        _ => None,
    }
}

fn pump_native_events(cx: &mut App) {
    let native = cx.global::<Native>();
    let hotkey_id = native.hotkey_id;
    let commands = native.tray.as_ref().map(Tray::poll).unwrap_or_default();
    let mut toggle = false;
    while let Ok(ev) = GlobalHotKeyEvent::receiver().try_recv() {
        if ev.id == hotkey_id && ev.state == HotKeyState::Pressed {
            toggle = !toggle;
        }
    }
    if toggle {
        launcher::toggle(cx);
    }
    for cmd in commands {
        match cmd {
            TrayCommand::Open => launcher::show(cx),
            TrayCommand::Settings => settings_window::open(cx),
            TrayCommand::Restart => state::restart(cx),
            TrayCommand::Quit => quit(cx),
        }
    }
}

pub(crate) fn quit(cx: &mut App) {
    launcher::hide(cx); // persists the global input
    delight_core::guard::clear_crash_note(); // a clean exit
    cx.quit();
}

