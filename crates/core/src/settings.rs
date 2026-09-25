//! Delight's own preferences: `~/Library/Application Support/Delight/settings.json`.
//! (What plugins store lives elsewhere — see [`crate::plugin_store`].)

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::files;

/// Light/dark preference; `System` follows the macOS appearance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Appearance {
    #[default]
    System,
    Dark,
    Light,
}

/// ⌘⇧Space.
pub const DEFAULT_LAUNCHER_SHORTCUT: &str = "cmd-shift-space";

/// Missing fields take their defaults, so the file only needs what changed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub appearance: Appearance,
    /// The system-wide shortcut that shows and hides the launcher, written
    /// like every key in Delight's keymaps: `cmd-shift-space`, `alt-k`.
    pub launcher_shortcut: String,
    /// Hide the launcher when it loses focus (Spotlight behaviour).
    pub hide_on_blur: bool,
    /// Hide the launcher after a copy action.
    pub hide_after_copy: bool,
    /// Keep an input history ([`crate::InputHistory`]): the last input comes
    /// back on launch, and inputs plugins remembered complete what's typed
    /// (Tab accepts). Turning it off erases the history.
    pub input_history: bool,
    /// Put the clipboard's text into the input when the launcher opens (only
    /// when it changed since the last auto-paste).
    pub paste_clipboard_on_open: bool,
    /// Start Delight when the user logs in (a LaunchAgent).
    pub open_at_login: bool,
    /// Folder scanned for plugin dylibs. `None` → [`Settings::default_plugin_dir`].
    pub plugin_dir: Option<PathBuf>,
    pub disabled_plugins: BTreeSet<String>,
    /// Plugins turned off because they crashed Delight's UI, with the panic
    /// message. Cleared when the user turns the plugin back on.
    pub crashed_plugins: BTreeMap<String, String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            appearance: Appearance::System,
            launcher_shortcut: DEFAULT_LAUNCHER_SHORTCUT.to_string(),
            hide_on_blur: true,
            hide_after_copy: false,
            input_history: true,
            paste_clipboard_on_open: false,
            open_at_login: false,
            plugin_dir: None,
            disabled_plugins: BTreeSet::new(),
            crashed_plugins: BTreeMap::new(),
        }
    }
}

impl Settings {
    pub fn path() -> PathBuf {
        files::app_dir().join("settings.json")
    }

    /// The user's key bindings, over the app's default keymap.
    pub fn keymap_path() -> PathBuf {
        files::app_dir().join("keymap.json")
    }

    /// Written by the panic hook when Delight is about to die; shown and
    /// removed by the next launch.
    pub fn crash_note_path() -> PathBuf {
        files::app_dir().join("last-crash.txt")
    }

    /// Plugin dylibs must match the app's build profile, so debug builds use
    /// their own folder.
    pub fn default_plugin_dir() -> PathBuf {
        files::app_dir().join(if cfg!(debug_assertions) { "plugins-debug" } else { "plugins" })
    }

    pub fn effective_plugin_dir(&self) -> PathBuf {
        self.plugin_dir.clone().unwrap_or_else(Self::default_plugin_dir)
    }

    /// The saved settings; defaults when there are none or the file is invalid.
    pub fn load() -> Self {
        files::read_json(&Self::path()).unwrap_or_default()
    }

    pub fn save(&self) -> anyhow::Result<()> {
        files::write_json(&Self::path(), self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_and_unknown_fields_are_fine() {
        let s: Settings = serde_json::from_str(r#"{"hide_on_blur": false, "no_longer_a_setting": 1}"#).unwrap();
        assert_eq!(s, Settings { hide_on_blur: false, ..Settings::default() });
        let round: Settings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(round, s);
    }
}
