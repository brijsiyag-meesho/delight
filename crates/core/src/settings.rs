//! Persisted preferences: `~/Library/Application Support/Delight/settings.json`.

use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;

pub use delight_sdk::theme::ThemeMode;
use delight_sdk::{FieldKind, Params, PluginManifest};
use serde::{Deserialize, Serialize};

/// Which classifier pipeline ranks tools.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ClassifierMode {
    /// Plugin detection rules only.
    #[default]
    Deterministic,
    /// Rules + a model backend (Jev). Falls back to rules when no backend is available.
    Hybrid,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub theme: ThemeMode,
    /// Hide the launcher when it loses focus (Spotlight behaviour).
    pub hide_on_blur: bool,
    /// Hide the launcher after ↵ runs a copy action.
    pub hide_after_copy: bool,
    /// Keep the input between launches (the "global input").
    pub remember_input: bool,
    /// Put the clipboard's text into the input when the launcher opens (only
    /// when it changed since the last auto-paste).
    pub paste_clipboard_on_open: bool,
    /// Start Delight when the user logs in (a LaunchAgent).
    pub open_at_login: bool,
    pub classifier: ClassifierMode,
    /// Folder scanned for plugin dylibs. `None` → default folder.
    pub plugin_dir: Option<PathBuf>,
    pub disabled_plugins: HashSet<String>,
    /// Plugins turned off because they crashed Delight's UI, with the panic
    /// message. Cleared when the user turns the plugin back on.
    pub crashed_plugins: BTreeMap<String, String>,
    /// Non-secret plugin settings by plugin id. Secrets live in the Keychain.
    pub plugin_settings: BTreeMap<String, Params>,
    /// Last input, restored on launch when `remember_input` is on.
    pub last_input: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: ThemeMode::System,
            hide_on_blur: true,
            hide_after_copy: false,
            remember_input: true,
            paste_clipboard_on_open: false,
            open_at_login: false,
            classifier: ClassifierMode::Deterministic,
            plugin_dir: None,
            disabled_plugins: HashSet::new(),
            crashed_plugins: BTreeMap::new(),
            plugin_settings: BTreeMap::new(),
            last_input: String::new(),
        }
    }
}

impl Settings {
    pub fn app_dir() -> PathBuf {
        dirs::config_dir().unwrap_or_else(std::env::temp_dir).join("Delight")
    }

    pub fn path() -> PathBuf {
        Self::app_dir().join("settings.json")
    }

    /// Written by the panic hook when Delight is about to die; shown and
    /// removed by the next launch (see [`crate::guard`]).
    pub fn crash_note_path() -> PathBuf {
        Self::app_dir().join("last-crash.txt")
    }

    /// Plugin dylibs must match the app's build profile, so debug builds use
    /// their own folder.
    pub fn default_plugin_dir() -> PathBuf {
        Self::app_dir().join(if cfg!(debug_assertions) { "plugins-debug" } else { "plugins" })
    }

    pub fn effective_plugin_dir(&self) -> PathBuf {
        self.plugin_dir.clone().unwrap_or_else(Self::default_plugin_dir)
    }

    /// A plugin's non-secret settings over the manifest defaults.
    pub fn plugin_values(&self, manifest: &PluginManifest) -> Params {
        let stored = self.plugin_settings.get(&manifest.id);
        manifest
            .settings
            .iter()
            .filter(|f| f.kind != FieldKind::Secret)
            .map(|f| {
                let value = stored.and_then(|s| s.get(&f.key)).or(f.default.as_ref());
                (f.key.clone(), value.cloned().unwrap_or_default())
            })
            .collect()
    }

    /// One-time move from the app's former name, DevLight: its folder
    /// (settings, plugins) becomes `Delight`, and the built-in tools' ids
    /// (`devlight.*` → `delight.*`) are renamed in the settings. Run before
    /// anything reads the app folder.
    pub fn migrate_legacy() {
        let old = dirs::config_dir().unwrap_or_else(std::env::temp_dir).join("DevLight");
        let new = Self::app_dir();
        if old.is_dir() && !new.exists() {
            match std::fs::rename(&old, &new) {
                Ok(()) => log::info!("moved {} to {}", old.display(), new.display()),
                Err(e) => log::error!("moving {} to {}: {e}", old.display(), new.display()),
            }
        }
        let mut settings = Self::load();
        let rename = |id: &str| match id.strip_prefix("devlight.") {
            Some(rest) => format!("delight.{rest}"),
            None => id.to_string(),
        };
        let before = settings.clone();
        settings.disabled_plugins = settings.disabled_plugins.iter().map(|id| rename(id)).collect();
        settings.plugin_settings = std::mem::take(&mut settings.plugin_settings).into_iter().map(|(k, v)| (rename(&k), v)).collect();
        settings.crashed_plugins = std::mem::take(&mut settings.crashed_plugins).into_iter().map(|(k, v)| (rename(&k), v)).collect();
        if settings != before
            && let Err(e) = settings.save()
        {
            log::error!("saving migrated settings failed: {e:#}");
        }
    }

    pub fn load() -> Self {
        match std::fs::read_to_string(Self::path()) {
            Ok(raw) => serde_json::from_str(&raw).unwrap_or_else(|e| {
                log::warn!("settings.json is invalid, using defaults: {e}");
                Self::default()
            }),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let path = Self::path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(tmp, path)?;
        Ok(())
    }
}
