//! What Delight stores for plugins (see `delight_sdk::Host`): each plugin's
//! settings — one JSON value — in `plugin-settings.json`, and a data folder
//! per plugin under `plugin-data/<id>/` for everything else. Delight never
//! interprets either. (Secrets go to the Keychain: [`crate::secrets`].)

use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::ensure;
use serde_json::Value;

use crate::files;

/// Plugin ids are reverse-DNS (`delight.json`, `acme.ab-lookup`). Only these
/// are accepted where an id names a file or folder.
pub fn valid_plugin_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && !id.starts_with('.')
        && id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
}

pub struct PluginStore {
    root: PathBuf,
    settings: BTreeMap<String, Value>,
}

impl PluginStore {
    /// The store in Delight's app folder.
    pub fn load() -> Self {
        Self::open(files::app_dir())
    }

    /// The store in `root` (tests use a temporary folder).
    pub fn open(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        let settings = files::read_json(&root.join("plugin-settings.json")).unwrap_or_default();
        Self { root, settings }
    }

    /// The plugin's settings: what it last saved, or `Null`.
    pub fn settings(&self, plugin_id: &str) -> Value {
        self.settings.get(plugin_id).cloned().unwrap_or(Value::Null)
    }

    /// Replaces the plugin's settings and saves; `Null` removes them.
    pub fn set_settings(&mut self, plugin_id: &str, value: Value) -> anyhow::Result<()> {
        ensure!(valid_plugin_id(plugin_id), "invalid plugin id {plugin_id:?}");
        if value.is_null() {
            self.settings.remove(plugin_id);
        } else {
            self.settings.insert(plugin_id.to_string(), value);
        }
        self.save()
    }

    /// The plugin's data folder, created if needed.
    pub fn data_dir(&self, plugin_id: &str) -> anyhow::Result<PathBuf> {
        ensure!(valid_plugin_id(plugin_id), "invalid plugin id {plugin_id:?}");
        let dir = self.data_root().join(plugin_id);
        std::fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    /// Forgets a plugin: its settings and its data folder.
    pub fn remove(&mut self, plugin_id: &str) -> anyhow::Result<()> {
        ensure!(valid_plugin_id(plugin_id), "invalid plugin id {plugin_id:?}");
        if self.settings.remove(plugin_id).is_some() {
            self.save()?;
        }
        let dir = self.data_root().join(plugin_id);
        if dir.exists() {
            std::fs::remove_dir_all(dir)?;
        }
        Ok(())
    }

    fn data_root(&self) -> PathBuf {
        self.root.join("plugin-data")
    }

    fn save(&self) -> anyhow::Result<()> {
        files::write_json(&self.path(), &self.settings)
    }

    fn path(&self) -> PathBuf {
        self.root.join("plugin-settings.json")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn temp_root(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("delight-store-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        root
    }

    #[test]
    fn settings_persist_and_null_removes_them() {
        let root = temp_root("settings");
        let mut store = PluginStore::open(&root);
        assert_eq!(store.settings("acme.ab"), Value::Null);
        store.set_settings("acme.ab", json!({"env": "stg"})).unwrap();

        let mut reopened = PluginStore::open(&root);
        assert_eq!(reopened.settings("acme.ab"), json!({"env": "stg"}));
        reopened.set_settings("acme.ab", Value::Null).unwrap();
        assert_eq!(PluginStore::open(&root).settings("acme.ab"), Value::Null);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn remove_deletes_settings_and_data() {
        let root = temp_root("remove");
        let mut store = PluginStore::open(&root);
        store.set_settings("acme.ab", json!(1)).unwrap();
        let dir = store.data_dir("acme.ab").unwrap();
        std::fs::write(dir.join("history.json"), "[]").unwrap();
        store.remove("acme.ab").unwrap();
        assert!(!dir.exists());
        assert_eq!(PluginStore::open(&root).settings("acme.ab"), Value::Null);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn ids_that_could_escape_the_folder_are_rejected() {
        for id in ["", "../evil", "a/b", ".hidden", "sp ace"] {
            assert!(!valid_plugin_id(id), "{id:?}");
        }
        assert!(valid_plugin_id("delight.json") && valid_plugin_id("acme.ab-lookup_2"));
        let store = PluginStore::open(temp_root("ids"));
        assert!(store.data_dir("../evil").is_err());
    }
}
