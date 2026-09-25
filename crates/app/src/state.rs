//! Process-wide state — settings, plugins, what's stored for them, the input
//! history — and the [`Host`] services plugins call.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use delight_core::{InputHistory, PluginStore, Registry, Settings, secrets};
use delight_sdk::{Host, HostHandle, Theme};
use delight_core::settings::Appearance;
use delight_ui::{ActiveTheme, ThemeMode};
use gpui::{App, Global, SharedString, Window, WindowHandle};

use crate::launcher::{self, Launcher};

pub struct AppState {
    pub settings: Settings,
    pub registry: Arc<Registry>,
    pub plugin_store: PluginStore,
    pub input_history: InputHistory,
    pub launcher: Option<WindowHandle<Launcher>>,
    /// Keychain secrets read so far, by `(plugin id, key)`. Reading the
    /// Keychain is slow (and can ask for a password), so each is read once.
    /// A `RefCell` because plugins read secrets with a shared `&App`.
    secrets: RefCell<HashMap<(String, String), Option<String>>>,
}

impl Global for AppState {}

/// Loads the plugins and installs the state and the plugins' [`Host`].
pub fn init(cx: &mut App, settings: Settings) {
    let registry = Arc::new(load_plugins(&settings));
    cx.set_global(AppState {
        settings,
        registry,
        plugin_store: PluginStore::load(),
        input_history: InputHistory::load(),
        launcher: None,
        secrets: RefCell::default(),
    });
    cx.set_global(HostHandle(Rc::new(AppHost)));
}

/// The plugins installed in the plugins folder.
fn load_plugins(settings: &Settings) -> Registry {
    let dir = settings.effective_plugin_dir();
    let _ = std::fs::create_dir_all(&dir);
    let mut registry = Registry::new();
    registry.load(&dir);
    for error in &registry.load_errors {
        log::warn!("plugin load error: {error}");
    }
    registry
}

/// The saved appearance, as the UI kit's theme takes it. (Core stores it
/// without depending on the UI kit, hence two enums.)
pub fn theme_mode(appearance: Appearance) -> ThemeMode {
    match appearance {
        Appearance::System => ThemeMode::System,
        Appearance::Dark => ThemeMode::Dark,
        Appearance::Light => ThemeMode::Light,
    }
}

pub fn settings(cx: &App) -> &Settings {
    &cx.global::<AppState>().settings
}

/// What plugins call (through `delight_sdk::host`).
struct AppHost;

impl Host for AppHost {
    fn settings(&self, plugin_id: &str, cx: &App) -> delight_sdk::serde_json::Value {
        cx.global::<AppState>().plugin_store.settings(plugin_id)
    }

    fn set_settings(&self, plugin_id: &str, value: delight_sdk::serde_json::Value, cx: &mut App) {
        if let Err(e) = cx.global_mut::<AppState>().plugin_store.set_settings(plugin_id, value) {
            log::error!("saving {plugin_id}'s settings: {e:#}");
        }
    }

    fn data_dir(&self, plugin_id: &str, cx: &App) -> std::io::Result<PathBuf> {
        cx.global::<AppState>().plugin_store.data_dir(plugin_id).map_err(std::io::Error::other)
    }

    fn secret(&self, plugin_id: &str, key: &str, cx: &App) -> Option<String> {
        let mut cache = cx.global::<AppState>().secrets.borrow_mut();
        let cached = cache.entry((plugin_id.to_string(), key.to_string())).or_insert_with(|| {
            secrets::get(plugin_id, key).unwrap_or_else(|e| {
                log::error!("reading secret {plugin_id}/{key}: {e:#}");
                None
            })
        });
        cached.clone()
    }

    fn set_secret(&self, plugin_id: &str, key: &str, value: String, cx: &mut App) {
        let stored = Some(value.clone()).filter(|v| !v.is_empty());
        cx.global::<AppState>().secrets.borrow_mut().insert((plugin_id.to_string(), key.to_string()), stored);
        // The Keychain is slow: write in the background.
        let (plugin_id, key) = (plugin_id.to_string(), key.to_string());
        cx.background_executor()
            .spawn(async move {
                if let Err(e) = secrets::set(&plugin_id, &key, &value) {
                    log::error!("saving secret {plugin_id}/{key}: {e:#}");
                }
            })
            .detach();
    }

    fn theme(&self, _: &Window, cx: &App) -> Theme {
        cx.theme().sdk.clone()
    }

    fn remember_input(&self, plugin_id: &str, text: SharedString, cx: &mut App) {
        let state = cx.global_mut::<AppState>();
        if state.settings.input_history
            && let Err(e) = state.input_history.remember(plugin_id, &text)
        {
            log::error!("remembering {plugin_id}'s input: {e:#}");
        }
    }

    fn set_input(&self, text: SharedString, cx: &mut App) {
        launcher::set_input(cx, text);
    }

    fn toast(&self, message: SharedString, cx: &mut App) {
        launcher::toast(cx, message);
    }

    fn open_settings(&self, _plugin_id: &str, _cx: &mut App) {
        // The Settings window arrives in the next block.
    }
}
