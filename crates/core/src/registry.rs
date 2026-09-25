use std::path::Path;
use std::sync::Arc;

use delight_sdk::{OperationSpec, Plugin, PluginManifest};

use crate::guard::{CrashCell, Guarded};
use crate::{builtin, native};

/// Where a plugin came from — shown in settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginSource {
    Builtin,
    /// Shipped inside Delight.app (`Contents/PlugIns`), built with it.
    Bundled { path: std::path::PathBuf },
    /// Installed by the user into the plugins folder.
    Dylib { path: std::path::PathBuf },
}

pub struct LoadedPlugin {
    /// The plugin behind panic guards (see [`crate::guard`]).
    pub plugin: Arc<dyn Plugin>,
    pub source: PluginSource,
    crash: CrashCell,
}

impl LoadedPlugin {
    pub fn manifest(&self) -> &PluginManifest {
        self.plugin.manifest()
    }

    /// Why the plugin crashed in this process, if it did. A crashed plugin
    /// is never called again until Delight restarts.
    pub fn crash(&self) -> Option<String> {
        self.crash.lock().ok()?.clone()
    }
}

/// Immutable snapshot of loaded plugins. Reloading builds a new registry, so
/// background work can hold an `Arc<Registry>` without locking.
#[derive(Default)]
pub struct Registry {
    plugins: Vec<LoadedPlugin>,
    /// Errors from loading plugin dylibs, surfaced in settings.
    pub load_errors: Vec<String>,
}

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_builtins() -> Self {
        let mut r = Self::new();
        for plugin in builtin::all() {
            r.register(plugin, PluginSource::Builtin);
        }
        r
    }

    /// Built-ins plus every plugin dylib in `dir`.
    pub fn load(plugin_dir: Option<&Path>) -> Self {
        Self::load_all(None, plugin_dir)
    }

    /// Built-ins, then the plugins bundled with the app, then the user's —
    /// each can replace an earlier one with the same id.
    pub fn load_all(bundled_dir: Option<&Path>, plugin_dir: Option<&Path>) -> Self {
        let mut r = Self::with_builtins();
        if let Some(dir) = bundled_dir {
            let (plugins, errors) = native::discover(dir);
            r.load_errors.extend(errors);
            for (plugin, path) in plugins {
                r.register(plugin, PluginSource::Bundled { path });
            }
        }
        if let Some(dir) = plugin_dir {
            let (plugins, errors) = native::discover(dir);
            r.load_errors.extend(errors);
            for (plugin, path) in plugins {
                r.register(plugin, PluginSource::Dylib { path });
            }
        }
        r
    }

    /// Registers a plugin behind panic guards. A later plugin with the same
    /// id replaces the earlier one, so plugins can override built-ins. A
    /// plugin whose manifest panics becomes a load error.
    pub fn register(&mut self, plugin: Arc<dyn Plugin>, source: PluginSource) {
        let guarded = match Guarded::new(plugin) {
            Ok(guarded) => guarded,
            Err(message) => {
                let name = match &source {
                    PluginSource::Builtin => "built-in".to_string(),
                    PluginSource::Bundled { path } | PluginSource::Dylib { path } => {
                        path.file_name().unwrap_or_default().to_string_lossy().into_owned()
                    }
                };
                self.load_errors.push(format!("{name}: panicked while loading: {message}"));
                return;
            }
        };
        let id = guarded.manifest().id.clone();
        let crash = guarded.crash_cell();
        self.plugins.retain(|p| p.manifest().id != id);
        self.plugins.push(LoadedPlugin { plugin: Arc::new(guarded), source, crash });
    }

    pub fn plugins(&self) -> &[LoadedPlugin] {
        &self.plugins
    }

    pub fn get(&self, id: &str) -> Option<&LoadedPlugin> {
        self.plugins.iter().find(|p| p.manifest().id == id)
    }

    pub fn operation(&self, plugin_id: &str, op_id: &str) -> Option<(&LoadedPlugin, &OperationSpec)> {
        let p = self.get(plugin_id)?;
        let op = p.manifest().operation(op_id)?;
        Some((p, op))
    }
}
