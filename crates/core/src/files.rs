//! Where Delight keeps its files, and writing them safely.

use std::path::{Path, PathBuf};

/// `~/Library/Application Support/Delight`.
pub fn app_dir() -> PathBuf {
    dirs::config_dir().unwrap_or_else(std::env::temp_dir).join("Delight")
}

/// Writes `value` as pretty JSON through a temporary file and a rename, so a
/// crash mid-write never leaves a half-written file.
pub fn write_json(path: &Path, value: &impl serde::Serialize) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(value)?)?;
    std::fs::rename(tmp, path)?;
    Ok(())
}

/// Reads JSON from `path`; a missing file gives `None`, an invalid one is
/// logged and gives `None` too.
pub fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Option<T> {
    let raw = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw)
        .map_err(|e| log::warn!("{} is invalid, ignoring it: {e}", path.display()))
        .ok()
}
