//! The launcher's input history, when [`crate::Settings::input_history`] is
//! on. Stored in `input-history.json`:
//!
//! * **Remembered inputs** — inputs *plugins* chose to remember
//!   (`Host::remember_input`), each tagged with the plugin's id: a log search
//!   remembers its queries, a JSON formatter remembers nothing. As the input
//!   is typed, the launcher shows how a remembered input would complete it
//!   (terminal-style autosuggestions); accepting the completion brings that
//!   plugin's tool up.
//! * **The input to restore** — the launcher's input when it last closed,
//!   brought back on launch.
//!
//! It's kept apart from the settings: inputs often contain tokens and other
//! secrets, and turning the history off erases the file.

use std::path::{Path, PathBuf};

use anyhow::ensure;
use serde::{Deserialize, Serialize};

use crate::files;
use crate::plugin_store::valid_plugin_id;

/// Remembered inputs kept; the oldest go first.
const MAX_REMEMBERED_INPUTS: usize = 10_000;
/// Inputs with more characters than this (big pastes) are neither remembered
/// nor restored.
const MAX_INPUT_CHARS: usize = 1729;
/// All remembered text stays under this, oldest dropped first, so the file
/// stays quick to load and to rewrite however long the inputs are.
const MAX_TOTAL_TEXT_BYTES: usize = 4 * 1024 * 1024;

/// An input a plugin chose to remember.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RememberedInput {
    pub plugin_id: String,
    pub text: String,
}

/// How a remembered input would complete what's typed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Completion<'a> {
    /// The rest of the remembered input, after what's typed.
    pub remainder: &'a str,
    /// The plugin that remembered it.
    pub plugin_id: &'a str,
}

/// The file's contents.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct HistoryFile {
    input_to_restore: String,
    /// Most recent last.
    remembered: Vec<RememberedInput>,
}

pub struct InputHistory {
    path: PathBuf,
    file: HistoryFile,
}

fn too_long(text: &str) -> bool {
    text.chars().count() > MAX_INPUT_CHARS
}

impl InputHistory {
    /// The history in Delight's app folder.
    pub fn load() -> Self {
        Self::open(files::app_dir().join("input-history.json"))
    }

    /// The history stored at `path` (tests use a temporary file).
    pub fn open(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let file = files::read_json(&path).unwrap_or_default();
        Self { path, file }
    }

    /// The input to bring back on launch.
    pub fn input_to_restore(&self) -> Option<&str> {
        Some(self.file.input_to_restore.as_str()).filter(|s| !s.is_empty())
    }

    /// Saves the launcher's current input to restore on the next launch. An
    /// input that's too long isn't kept (nor is the previous one).
    pub fn set_input_to_restore(&mut self, input: &str) -> anyhow::Result<()> {
        let input = if too_long(input) { "" } else { input };
        if input == self.file.input_to_restore {
            return Ok(());
        }
        self.file.input_to_restore = input.to_string();
        self.save()
    }

    /// How the most recently remembered input starting with `typed` would
    /// complete it. `None` when no longer remembered input starts with it.
    pub fn completion_for(&self, typed: &str) -> Option<Completion<'_>> {
        if typed.trim().is_empty() {
            return None;
        }
        self.file
            .remembered
            .iter()
            .rev()
            .find(|r| r.text.len() > typed.len() && r.text.starts_with(typed))
            .map(|r| Completion { remainder: &r.text[typed.len()..], plugin_id: &r.plugin_id })
    }

    /// Remembers `text` for `plugin_id` as the most recent input and saves.
    /// The plugin's older inputs that are just its beginning — partial inputs
    /// like `con` before `config-manager` — are dropped, so they don't crowd
    /// out the full one. Blank and too-long inputs aren't remembered.
    pub fn remember(&mut self, plugin_id: &str, text: &str) -> anyhow::Result<()> {
        ensure!(valid_plugin_id(plugin_id), "invalid plugin id {plugin_id:?}");
        if text.trim().is_empty() || too_long(text) {
            return Ok(());
        }
        let remembered = &mut self.file.remembered;
        if remembered.last().is_some_and(|r| r.plugin_id == plugin_id && r.text == text) {
            return Ok(());
        }
        remembered.retain(|r| r.plugin_id != plugin_id || !text.starts_with(r.text.as_str()));
        remembered.push(RememberedInput { plugin_id: plugin_id.to_string(), text: text.to_string() });
        let mut oldest_kept = remembered.len().saturating_sub(MAX_REMEMBERED_INPUTS);
        let mut total: usize = remembered[oldest_kept..].iter().map(|r| r.text.len()).sum();
        while total > MAX_TOTAL_TEXT_BYTES {
            total -= remembered[oldest_kept].text.len();
            oldest_kept += 1;
        }
        remembered.drain(..oldest_kept);
        self.save()
    }

    /// Forgets everything a plugin remembered (the plugin was deleted).
    pub fn forget_plugin(&mut self, plugin_id: &str) -> anyhow::Result<()> {
        let before = self.file.remembered.len();
        self.file.remembered.retain(|r| r.plugin_id != plugin_id);
        if self.file.remembered.len() == before { Ok(()) } else { self.save() }
    }

    /// Forgets everything and deletes the file (the history was turned off).
    pub fn erase(&mut self) -> anyhow::Result<()> {
        self.file = HistoryFile::default();
        match std::fs::remove_file(&self.path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
            _ => Ok(()),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn save(&self) -> anyhow::Result<()> {
        files::write_json(&self.path, &self.file)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOGS: &str = "acme.logs";
    const AB: &str = "acme.ab";

    fn temp_history(name: &str) -> InputHistory {
        let path = std::env::temp_dir().join(format!("delight-history-{name}-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&path);
        InputHistory::open(path)
    }

    fn texts(h: &InputHistory) -> Vec<&str> {
        h.file.remembered.iter().map(|r| r.text.as_str()).collect()
    }

    #[test]
    fn completes_with_the_most_recent_match_and_its_plugin() {
        let mut h = temp_history("complete");
        h.remember(LOGS, r#"application_name: "billing""#).unwrap();
        h.remember(LOGS, r#"application_name: "config-manager""#).unwrap();
        h.remember(AB, "apple").unwrap();
        assert_eq!(
            h.completion_for("application"),
            Some(Completion { remainder: r#"_name: "config-manager""#, plugin_id: LOGS })
        );
        assert_eq!(h.completion_for("app"), Some(Completion { remainder: "le", plugin_id: AB }), "most recent first");
        assert_eq!(h.completion_for("apple"), None, "nothing longer");
        assert_eq!(h.completion_for("  "), None);
        h.erase().unwrap();
    }

    #[test]
    fn a_full_input_replaces_the_same_plugins_partial_inputs() {
        let mut h = temp_history("partials");
        h.remember(AB, "con").unwrap();
        for input in ["con", "cargo", "config", "config-manager"] {
            h.remember(LOGS, input).unwrap();
        }
        assert_eq!(texts(&h), ["con", "cargo", "config-manager"], "another plugin's `con` stays");
        h.remember(LOGS, "con").unwrap(); // a partial input used later is kept, as the newest
        assert_eq!(texts(&h), ["con", "cargo", "config-manager", "con"]);
        h.erase().unwrap();
    }

    #[test]
    fn remembers_within_limits_persists_and_forgets() {
        let mut h = temp_history("remember");
        for input in ["a", "b", "a", "  ", "b"] {
            h.remember(LOGS, input).unwrap();
        }
        assert_eq!(texts(&h), ["a", "b"]);
        h.remember(LOGS, &"é".repeat(MAX_INPUT_CHARS + 1)).unwrap();
        assert_eq!(texts(&h), ["a", "b"], "too-long inputs aren't remembered");
        h.remember(LOGS, &"é".repeat(MAX_INPUT_CHARS)).unwrap();
        assert_eq!(h.file.remembered.len(), 3, "the limit counts characters, not bytes");
        assert!(h.remember("../evil", "x").is_err());

        h.remember(AB, "keep").unwrap();
        h.forget_plugin(LOGS).unwrap();
        assert_eq!(texts(&InputHistory::open(h.path().to_path_buf())), ["keep"]);
        h.erase().unwrap();
        assert!(!h.path().exists());
    }

    #[test]
    fn restores_the_last_input() {
        let mut h = temp_history("restore");
        assert_eq!(h.input_to_restore(), None);
        h.set_input_to_restore("jwt eyJ…").unwrap();
        assert_eq!(InputHistory::open(h.path().to_path_buf()).input_to_restore(), Some("jwt eyJ…"));
        h.set_input_to_restore(&"x".repeat(MAX_INPUT_CHARS + 1)).unwrap();
        assert_eq!(h.input_to_restore(), None, "too long: not kept");
        h.erase().unwrap();
    }

    #[test]
    fn keeps_the_newest_within_count_and_size() {
        let mut h = temp_history("limits");
        let input = |text: String| RememberedInput { plugin_id: LOGS.into(), text };
        h.file.remembered = (0..MAX_REMEMBERED_INPUTS + 5).map(|i| input(i.to_string())).collect();
        h.remember(AB, "last").unwrap();
        assert_eq!(h.file.remembered.len(), MAX_REMEMBERED_INPUTS);
        assert_eq!(h.file.remembered[0].text, "6");

        let long = |i: usize| format!("{i:05}{}", "x".repeat(MAX_INPUT_CHARS - 10));
        h.file.remembered = (0..MAX_TOTAL_TEXT_BYTES / MAX_INPUT_CHARS + 20).map(|i| input(long(i))).collect();
        h.remember(AB, "newest").unwrap();
        let total: usize = h.file.remembered.iter().map(|r| r.text.len()).sum();
        assert!(total <= MAX_TOTAL_TEXT_BYTES, "{total}");
        assert_eq!(h.file.remembered.last().map(|r| r.text.as_str()), Some("newest"));
        h.erase().unwrap();
    }

    #[test]
    fn completing_from_a_full_history_is_fast() {
        let mut h = temp_history("speed");
        h.file.remembered = (0..MAX_REMEMBERED_INPUTS)
            .map(|i| RememberedInput { plugin_id: LOGS.into(), text: format!("remembered input {i} of a typical length") })
            .collect();
        let started = std::time::Instant::now();
        for _ in 0..100 {
            assert_eq!(h.completion_for("no match anywhere"), None);
        }
        let per_keystroke = started.elapsed() / 100;
        assert!(per_keystroke < std::time::Duration::from_millis(5), "{per_keystroke:?}");
    }
}
