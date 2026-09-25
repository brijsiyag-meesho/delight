//! The loaded plugins. Reloading builds a new registry, so background work
//! can hold an `Arc<Registry>` without locking.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use delight_sdk::{OperationSpec, Plugin, PluginManifest};

use crate::guard::GuardedPlugin;
use crate::native;
use crate::plugin_store::valid_plugin_id;

/// Where a plugin came from — shown in Settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginSource {
    /// Compiled into Delight.
    Builtin,
    /// A plugin file (`.dylib`) the user installed into the plugins folder.
    Installed { path: PathBuf },
}

pub struct LoadedPlugin {
    /// The plugin behind panic guards (see [`crate::guard`]).
    pub plugin: Arc<GuardedPlugin>,
    pub source: PluginSource,
}

impl LoadedPlugin {
    pub fn manifest(&self) -> &PluginManifest {
        self.plugin.manifest()
    }
}

#[derive(Default)]
pub struct Registry {
    /// In load order: later entries replaced earlier ones with the same id.
    plugins: Vec<LoadedPlugin>,
    /// Plugins that failed to load, with why — shown in Settings.
    pub load_errors: Vec<String>,
}

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }

    /// The plugins installed in `dir` — each can replace an earlier one
    /// (a built-in) with the same id.
    pub fn load(&mut self, dir: &Path) {
        let (plugins, errors) = native::discover(dir);
        self.load_errors.extend(errors);
        for (plugin, path) in plugins {
            self.register(plugin, PluginSource::Installed { path });
        }
    }

    /// Registers a plugin behind panic guards. A later plugin with the same
    /// id replaces the earlier one, so plugins can override built-ins. A
    /// plugin whose manifest panics, or whose id can't name its storage
    /// (see [`valid_plugin_id`]), becomes a load error.
    pub fn register(&mut self, plugin: Arc<dyn Plugin>, source: PluginSource) {
        let name = match &source {
            PluginSource::Builtin => "built-in".to_string(),
            PluginSource::Installed { path } => path.file_name().unwrap_or_default().to_string_lossy().into_owned(),
        };
        let plugin = match GuardedPlugin::new(plugin) {
            Ok(plugin) => plugin,
            Err(message) => {
                self.load_errors.push(format!("{name}: panicked while loading: {message}"));
                return;
            }
        };
        let id = plugin.manifest().id.clone();
        if !valid_plugin_id(&id) {
            self.load_errors.push(format!("{name}: invalid plugin id {id:?} (use letters, digits, `.`, `-`, `_`)"));
            return;
        }
        self.plugins.retain(|p| p.manifest().id != id);
        self.plugins.push(LoadedPlugin { plugin: Arc::new(plugin), source });
    }

    pub fn plugins(&self) -> &[LoadedPlugin] {
        &self.plugins
    }

    pub fn get(&self, id: &str) -> Option<&LoadedPlugin> {
        self.plugins.iter().find(|p| p.manifest().id == id)
    }

    pub fn operation(&self, plugin_id: &str, operation_id: &str) -> Option<(&LoadedPlugin, &OperationSpec)> {
        let plugin = self.get(plugin_id)?;
        Some((plugin, plugin.manifest().operation(operation_id)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestPlugin;

    #[test]
    fn a_later_plugin_replaces_one_with_the_same_id() {
        let mut registry = Registry::new();
        registry.register(Arc::new(TestPlugin::new("acme.x", "old")), PluginSource::Builtin);
        registry.register(Arc::new(TestPlugin::new("acme.y", "other")), PluginSource::Builtin);
        let path = PathBuf::from("/plugins/x.dylib");
        registry.register(Arc::new(TestPlugin::new("acme.x", "new")), PluginSource::Installed { path: path.clone() });

        assert_eq!(registry.plugins().len(), 2);
        let (plugin, op) = registry.operation("acme.x", "new").expect("the replacement's operation");
        assert_eq!((plugin.source.clone(), op.title.as_str()), (PluginSource::Installed { path }, "NEW"));
        assert!(registry.operation("acme.x", "old").is_none());
    }

    #[test]
    fn an_invalid_id_is_a_load_error() {
        let mut registry = Registry::new();
        registry.register(Arc::new(TestPlugin::new("../escape", "op")), PluginSource::Builtin);
        assert!(registry.plugins().is_empty());
        assert!(registry.load_errors[0].contains("invalid plugin id"), "{:?}", registry.load_errors);
    }

    #[test]
    fn a_missing_folder_loads_nothing() {
        let mut registry = Registry::new();
        registry.load(Path::new("/nonexistent/plugins"));
        assert!(registry.plugins().is_empty() && registry.load_errors.is_empty());
    }
}
