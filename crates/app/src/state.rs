//! Process-wide state shared by the launcher, settings window and tray, and
//! the [`Host`] services plugins call back into.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use delight_core::registry::PluginSource;
use delight_core::settings::ClassifierMode;
use delight_core::{Registry, Router, Settings, secrets};
use delight_sdk::{FieldKind, Host, HostHandle, Params};
use gpui::{App, Global, SharedString, Task, WindowHandle};

use crate::launcher::{self, Launcher};
use crate::settings_window::{self, SettingsWindow};

pub struct AppState {
    pub settings: Settings,
    pub registry: Arc<Registry>,
    pub router: Arc<Router>,
    pub launcher: Option<WindowHandle<Launcher>>,
    pub settings_window: Option<WindowHandle<SettingsWindow>>,
    /// Keychain secrets by plugin id, read once per plugin. Behind a RefCell
    /// so reading settings never mutates the global — observers of `AppState`
    /// (the launcher re-classifies) would otherwise loop.
    secrets: RefCell<HashMap<String, Params>>,
    /// Pending debounced Keychain writes by `plugin/key`.
    secret_writes: HashMap<String, Task<()>>,
}

impl Global for AppState {}

impl AppState {
    pub fn new(settings: Settings) -> Self {
        let registry = Arc::new(load_registry(&settings));
        let router = Arc::new(build_router(&settings));
        Self {
            settings,
            registry,
            router,
            launcher: None,
            settings_window: None,
            secrets: RefCell::default(),
            secret_writes: HashMap::new(),
        }
    }
}

pub fn init(cx: &mut App, settings: Settings) {
    cx.set_global(AppState::new(settings));
    cx.set_global(HostHandle(Rc::new(AppHost)));
}

fn load_registry(settings: &Settings) -> Registry {
    let dir = settings.effective_plugin_dir();
    let _ = std::fs::create_dir_all(&dir);
    let registry = Registry::load_all(bundled_plugin_dir().as_deref(), Some(&dir));
    for e in &registry.load_errors {
        log::warn!("plugin load error: {e}");
    }
    for p in registry.plugins() {
        if let PluginSource::Bundled { path } | PluginSource::Dylib { path } = &p.source {
            log::info!("loaded plugin {} from {}", p.manifest().id, path.display());
        }
    }
    registry
}

/// `Delight.app/Contents/PlugIns` — plugins built and shipped with the app —
/// when running from the bundle.
fn bundled_plugin_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?.parent()?.join("PlugIns");
    dir.is_dir().then_some(dir)
}

/// Classifier pipeline. `Hybrid` is where a model backend (e.g. Jev) is
/// pushed as an extra stage; until one is configured it equals rules-only.
fn build_router(settings: &Settings) -> Router {
    match settings.classifier {
        ClassifierMode::Deterministic | ClassifierMode::Hybrid => Router::deterministic(),
    }
}

pub fn settings(cx: &App) -> &Settings {
    &cx.global::<AppState>().settings
}

/// Mutate + persist settings, then refresh every window.
pub fn update_settings(cx: &mut App, f: impl FnOnce(&mut Settings)) {
    let state = cx.global_mut::<AppState>();
    let before = state.settings.clone();
    f(&mut state.settings);
    if state.settings.classifier != before.classifier {
        state.router = Arc::new(build_router(&state.settings));
    }
    if state.settings.plugin_dir != before.plugin_dir {
        state.registry = Arc::new(load_registry(&state.settings));
    }
    if state.settings.open_at_login != before.open_at_login {
        crate::login::apply(state.settings.open_at_login);
    }
    if let Err(e) = state.settings.save() {
        log::error!("saving settings failed: {e:#}");
    }
    cx.refresh_windows();
}

/// A plugin's settings: stored values over manifest defaults, secrets from
/// the Keychain.
pub fn plugin_settings(cx: &App, plugin_id: &str) -> Params {
    let state = cx.global::<AppState>();
    let Some(plugin) = state.registry.get(plugin_id) else { return Params::new() };
    let manifest = plugin.manifest();
    let mut values = state.settings.plugin_values(manifest);
    let mut cache = state.secrets.borrow_mut();
    let secrets = cache.entry(plugin_id.to_string()).or_insert_with(|| {
        manifest
            .settings
            .iter()
            .filter(|f| f.kind == FieldKind::Secret)
            .map(|f| (f.key.clone(), secrets::get(plugin_id, &f.key).unwrap_or_default()))
            .collect()
    });
    values.extend(secrets.clone());
    values
}

pub fn set_plugin_setting(cx: &mut App, plugin_id: &str, key: &str, value: String) {
    let state = cx.global::<AppState>();
    let Some(plugin) = state.registry.get(plugin_id) else { return };
    let secret = plugin.manifest().settings.iter().any(|f| f.key == key && f.kind == FieldKind::Secret);
    if !secret {
        update_settings(cx, |s| {
            s.plugin_settings.entry(plugin_id.to_string()).or_default().insert(key.to_string(), value);
        });
        return;
    }
    // Secrets: update the cache now, write the Keychain once typing pauses.
    plugin_settings(cx, plugin_id);
    let (pid, k, v) = (plugin_id.to_string(), key.to_string(), value.clone());
    let write = cx.spawn(async move |cx| {
        cx.background_executor().timer(Duration::from_millis(400)).await;
        let result = cx.background_executor().spawn(async move { secrets::set(&pid, &k, &v) }).await;
        if let Err(e) = result {
            log::error!("saving secret failed: {e:#}");
        }
    });
    let state = cx.global_mut::<AppState>();
    if let Some(cached) = state.secrets.get_mut().get_mut(plugin_id) {
        cached.insert(key.to_string(), value);
    }
    state.secret_writes.insert(format!("{plugin_id}/{key}"), write);
    cx.refresh_windows();
}

/// Deletes an installed plugin completely: its dylib, its settings and its
/// Keychain secrets. Its code stays mapped until the next restart (libraries
/// are never unloaded), but it's gone from every list immediately.
pub fn delete_plugin(cx: &mut App, plugin_id: &str) -> anyhow::Result<()> {
    let state = cx.global::<AppState>();
    let plugin = state.registry.get(plugin_id).ok_or_else(|| anyhow::anyhow!("{plugin_id} is not loaded"))?;
    let PluginSource::Dylib { path } = &plugin.source else {
        anyhow::bail!("tools that come with Delight can be turned off, not deleted");
    };
    let secret_keys: Vec<String> = plugin
        .manifest()
        .settings
        .iter()
        .filter(|f| f.kind == FieldKind::Secret)
        .map(|f| f.key.clone())
        .collect();
    std::fs::remove_file(path).map_err(|e| anyhow::anyhow!("removing {}: {e}", path.display()))?;

    let pid = plugin_id.to_string();
    cx.background_executor()
        .spawn(async move {
            for key in secret_keys {
                if let Err(e) = secrets::set(&pid, &key, "") {
                    log::error!("removing secret {pid}/{key}: {e:#}");
                }
            }
        })
        .detach();
    let state = cx.global_mut::<AppState>();
    state.secrets.get_mut().remove(plugin_id);
    state.secret_writes.retain(|k, _| !k.starts_with(&format!("{plugin_id}/")));
    state.registry = Arc::new(load_registry(&state.settings));
    update_settings(cx, |s| {
        s.plugin_settings.remove(plugin_id);
        s.disabled_plugins.remove(plugin_id);
        s.crashed_plugins.remove(plugin_id);
    });
    Ok(())
}

/// Installs a plugin the user picked: a `.zip` made by `delight package`, or
/// a bare `.dylib`. It's copied into the plugins folder and loaded right away;
/// a new plugin works immediately, a replaced one after a restart (loaded
/// libraries are never unloaded). Returns what to tell the user.
pub fn install_plugin(cx: &mut App, picked: &Path) -> anyhow::Result<String> {
    let staging = std::env::temp_dir().join(format!("delight-install-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging)?;
    let result = install_from(cx, picked, &staging);
    let _ = std::fs::remove_dir_all(&staging);
    result
}

fn install_from(cx: &mut App, picked: &Path, staging: &Path) -> anyhow::Result<String> {
    let extension = picked.extension().and_then(|e| e.to_str()).unwrap_or_default();
    let dylib = match extension {
        "zip" => {
            let status = std::process::Command::new("/usr/bin/unzip")
                .args(["-q", "-o", "-j"])
                .arg(picked)
                .arg("-d")
                .arg(staging)
                .status()?;
            anyhow::ensure!(status.success(), "couldn't unpack {}", picked.display());
            let files: Vec<PathBuf> = std::fs::read_dir(staging)?.flatten().map(|e| e.path()).collect();
            let dylibs: Vec<&PathBuf> = files.iter().filter(|p| p.extension().is_some_and(|e| e == "dylib")).collect();
            let [dylib] = dylibs.as_slice() else {
                anyhow::bail!("{} should contain one plugin (.dylib), not {}", picked.display(), dylibs.len());
            };
            // `delight package` records what it was built for: say so before
            // trying to load it.
            let meta = files.iter().find(|p| p.to_string_lossy().ends_with(".plugin.toml"));
            if let Some(meta) = meta.and_then(|p| std::fs::read_to_string(p).ok()) {
                let value = |key: &str| {
                    meta.lines().find_map(|l| l.strip_prefix(key)?.trim_start().strip_prefix('=')).map(|v| v.trim().trim_matches('"').to_string())
                };
                if let Some(target) = value("target") {
                    anyhow::ensure!(target.starts_with(std::env::consts::ARCH), "it's built for {target}, not this Mac");
                }
                if let Some(build) = value("sdk_build_id") {
                    anyhow::ensure!(
                        build == delight_sdk::BUILD_ID,
                        "it's built for {build}, but this Delight has {} — ask its author for a build for this version",
                        delight_sdk::BUILD_ID
                    );
                }
            }
            (*dylib).clone()
        }
        "dylib" => picked.to_path_buf(),
        _ => anyhow::bail!("choose a .zip made by `delight package`, or a .dylib"),
    };
    let name = dylib.file_stem().and_then(|s| s.to_str()).unwrap_or("plugin").to_string();

    let dir = settings(cx).effective_plugin_dir();
    std::fs::create_dir_all(&dir)?;
    let dest = dir.join(format!("{name}.dylib"));
    let replacing = dest.exists();
    // Replace, never overwrite in place: macOS caches a loaded dylib's code
    // signature per file, and a rewritten one gets the app killed.
    if replacing {
        std::fs::remove_file(&dest)?;
    }
    std::fs::copy(&dylib, &dest)?;
    // Downloads are quarantined, and a quarantined library won't load; the
    // user chose to install this one.
    let _ = std::process::Command::new("/usr/bin/xattr").args(["-d", "com.apple.quarantine"]).arg(&dest).output();

    let state = cx.global_mut::<AppState>();
    state.registry = Arc::new(load_registry(&state.settings));
    let file = format!("{name}.dylib:");
    if let Some(error) = state.registry.load_errors.iter().find(|e| e.starts_with(&file)).cloned() {
        let _ = std::fs::remove_file(&dest);
        state.registry = Arc::new(load_registry(&state.settings));
        cx.refresh_windows();
        anyhow::bail!("{}", error.trim_start_matches(&file).trim());
    }
    let plugin_name = state
        .registry
        .plugins()
        .iter()
        .find(|p| matches!(&p.source, PluginSource::Dylib { path } if *path == dest))
        .map(|p| p.manifest().name.clone())
        .unwrap_or(name);
    cx.refresh_windows();
    Ok(if replacing {
        format!("{plugin_name} updated — restart Delight to use the new version")
    } else {
        format!("{plugin_name} installed")
    })
}

/// Relaunches the app — the only way to pick up rebuilt plugin dylibs.
pub fn restart(cx: &mut App) {
    spawn_relaunch();
    crate::quit(cx);
}

/// Starts a new instance once this one has exited (and released the hotkey).
fn spawn_relaunch() {
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::process::Command::new("/bin/sh").arg("-c").arg("sleep 0.5; exec \"$0\"").arg(exe).spawn();
    }
}

/// `delight_core::guard`'s fatal handler: a plugin panicked on the main
/// thread, so GPUI's state can't be trusted anymore. Turns the plugin off,
/// leaves a note for the next launch and relaunches — without touching GPUI,
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
    spawn_relaunch();
    std::process::exit(0);
}

struct AppHost;

impl Host for AppHost {
    fn settings(&self, plugin_id: &str, cx: &App) -> Params {
        plugin_settings(cx, plugin_id)
    }

    fn set_setting(&self, plugin_id: &str, key: &str, value: String, cx: &mut App) {
        set_plugin_setting(cx, plugin_id, key, value);
    }

    fn toast(&self, message: SharedString, cx: &mut App) {
        launcher::toast(cx, message);
    }

    fn open_settings(&self, plugin_id: &str, cx: &mut App) {
        let plugin_id = plugin_id.to_string();
        cx.defer(move |cx| settings_window::open_plugin(cx, &plugin_id));
    }
}

