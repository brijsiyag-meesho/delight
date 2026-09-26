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
use gpui::{App, Global, Keystroke, SharedString, Window, WindowHandle};

use anyhow::Context as _;
use delight_core::registry::PluginSource;

use crate::hotkey::LauncherShortcut;
use crate::launcher::{self, Launcher};
use crate::settings_window::{self, SettingsWindow};
use crate::tray::Tray;

pub struct AppState {
    pub settings: Settings,
    pub registry: Arc<Registry>,
    pub plugin_store: PluginStore,
    pub input_history: InputHistory,
    pub launcher: Option<WindowHandle<Launcher>>,
    pub settings_window: Option<WindowHandle<SettingsWindow>>,
    /// How rebuilding the plugins for this Delight went: shown in Settings.
    pub plugin_rebuild: crate::plugin_rebuild::Rebuild,
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
        settings_window: None,
        plugin_rebuild: Default::default(),
        secrets: RefCell::default(),
    });
    cx.set_global(HostHandle(Rc::new(AppHost)));
}

/// The built-in tools, then the plugins installed in the plugins folder.
fn load_plugins(settings: &Settings) -> Registry {
    let dir = settings.effective_plugin_dir();
    let _ = std::fs::create_dir_all(&dir);
    let mut registry = Registry::new();
    for tool in delight_tools::all() {
        registry.register(tool, PluginSource::Builtin);
    }
    registry.load(&dir);
    for error in &registry.load_errors {
        log::warn!("plugin load error: {error}");
    }
    registry
}

/// Applies the appearance setting: the UI kit's theme, and macOS's own
/// drawing of Delight's windows (blur, glass, title bar), so both match.
pub fn apply_appearance(cx: &mut App, appearance: Appearance) {
    // Core stores the setting without depending on the UI kit: two enums.
    let (mode, dark) = match appearance {
        Appearance::System => (ThemeMode::System, None),
        Appearance::Dark => (ThemeMode::Dark, Some(true)),
        Appearance::Light => (ThemeMode::Light, Some(false)),
    };
    crate::platform::set_app_appearance(dark);
    delight_ui::theme::set_mode(cx, mode);
}

pub fn settings(cx: &App) -> &Settings {
    &cx.global::<AppState>().settings
}

/// Changes Delight's settings, saves them, and applies what changed.
pub fn update_settings(cx: &mut App, change: impl FnOnce(&mut Settings)) {
    let state = cx.global_mut::<AppState>();
    let before = state.settings.clone();
    change(&mut state.settings);
    let after = state.settings.clone();
    if let Err(e) = after.save() {
        log::error!("saving settings: {e:#}");
    }
    if after.appearance != before.appearance {
        apply_appearance(cx, after.appearance);
    }
    if after.open_at_login != before.open_at_login {
        crate::login::apply(after.open_at_login);
    }
    if before.input_history && !after.input_history
        && let Err(e) = cx.global_mut::<AppState>().input_history.erase()
    {
        log::error!("erasing the input history: {e:#}");
    }
    if after.plugin_dir != before.plugin_dir {
        reload_plugins(cx);
    } else if after.disabled_plugins != before.disabled_plugins {
        launcher::refresh(cx);
    }
    cx.refresh_windows();
}

/// Switches the launcher shortcut and saves it; the error says why it
/// can't be used (invalid, or refused by macOS), and the old one stays.
pub fn set_launcher_shortcut(cx: &mut App, keystroke: &Keystroke) -> anyhow::Result<()> {
    anyhow::ensure!(cx.has_global::<LauncherShortcut>(), "the launcher shortcut isn't available");
    cx.global_mut::<LauncherShortcut>().change(keystroke)?;
    if let Some(tray) = cx.try_global::<Tray>() {
        tray.set_shortcut(keystroke);
    }
    update_settings(cx, |s| s.launcher_shortcut = keystroke.unparse());
    Ok(())
}

/// Loads the plugins folder again: newly installed plugins appear. (A
/// plugin already loaded keeps its old code until Delight restarts: loaded
/// libraries are never unloaded.)
pub fn reload_plugins(cx: &mut App) {
    let state = cx.global_mut::<AppState>();
    state.registry = Arc::new(load_plugins(&state.settings));
    launcher::plugins_reloaded(cx);
    cx.refresh_windows();
}

/// Deletes an installed plugin: its file, its settings and data, its
/// remembered inputs and its Keychain secrets. Its code stays loaded until
/// Delight restarts, but it's gone from every list at once.
pub fn delete_plugin(cx: &mut App, plugin_id: &str) -> anyhow::Result<()> {
    let state = cx.global_mut::<AppState>();
    let plugin = state.registry.get(plugin_id).with_context(|| format!("{plugin_id} isn't loaded"))?;
    let PluginSource::Installed { path } = &plugin.source else {
        anyhow::bail!("tools that come with Delight can be turned off, not deleted");
    };
    std::fs::remove_file(path).with_context(|| format!("removing {}", path.display()))?;
    state.plugin_store.remove(plugin_id)?;
    state.input_history.forget_plugin(plugin_id)?;
    state.secrets.get_mut().retain(|(id, _), _| id != plugin_id);
    let id = plugin_id.to_string();
    cx.background_executor()
        .spawn(async move {
            if let Err(e) = secrets::forget_plugin(&id) {
                log::error!("deleting {id}'s secrets: {e:#}");
            }
        })
        .detach();
    update_settings(cx, |s| {
        s.disabled_plugins.remove(plugin_id);
        s.crashed_plugins.remove(plugin_id);
    });
    reload_plugins(cx);
    Ok(())
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

    fn remember_input(&self, plugin_id: &str, operation_id: &str, text: SharedString, cx: &mut App) {
        let state = cx.global_mut::<AppState>();
        if state.settings.input_history
            && let Err(e) = state.input_history.remember(plugin_id, operation_id, &text)
        {
            log::error!("remembering {plugin_id}'s input: {e:#}");
        }
    }

    // Deferred, like `open_settings`: a plugin usually calls these while
    // the launcher is handling a key or click, and the launcher can't be
    // updated again until that's done.
    fn set_input(&self, text: SharedString, cx: &mut App) {
        cx.defer(move |cx| launcher::set_input(cx, text));
    }

    fn toast(&self, message: SharedString, cx: &mut App) {
        cx.defer(move |cx| launcher::toast(cx, message));
    }

    fn hide(&self, cx: &mut App) {
        cx.defer(launcher::hide);
    }

    fn copy_file(&self, name: &str, bytes: &[u8], _: &mut App) -> std::io::Result<()> {
        crate::clipboard::copy_file(name, bytes).map_err(|e| std::io::Error::other(format!("{e:#}")))
    }

    fn open_settings(&self, plugin_id: &str, cx: &mut App) {
        let plugin_id = plugin_id.to_string();
        cx.defer(move |cx| settings_window::open_plugin(cx, &plugin_id));
    }
}
