//! The SDK kit this build carries: the files a plugin build needs to get
//! exactly this app's SDK build — the workspace's manifest, lock file,
//! toolchain and flags, and the SDK's crates (embedded by build.rs).
//! `delight-plugin-manager` (the CLI's, and the app's rebuild) asks for it
//! (`--write-sdk-kit`) and builds plugins in it.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::Context as _;

const FILES: &[(&str, &[u8])] = include!(concat!(env!("OUT_DIR"), "/sdk_kit.rs"));

/// Writes the kit into `dir`, rewriting only files that changed (so Cargo
/// doesn't rebuild what didn't), and removes files under `crates/` the kit
/// no longer has (the SDK's build id hashes every file of its sources).
pub fn write(dir: &Path) -> anyhow::Result<()> {
    for (path, contents) in FILES {
        let file = dir.join(path);
        if std::fs::read(&file).ok().as_deref() != Some(*contents) {
            if let Some(parent) = file.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&file, contents).with_context(|| format!("writing {}", file.display()))?;
        }
    }
    let wanted: HashSet<PathBuf> = FILES.iter().map(|(path, _)| dir.join(path)).collect();
    for file in files_under(&dir.join("crates")) {
        if !wanted.contains(&file) {
            std::fs::remove_file(&file)?;
        }
    }
    Ok(())
}

fn files_under(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    entries
        .flatten()
        .flat_map(|e| if e.path().is_dir() { files_under(&e.path()) } else { vec![e.path()] })
        .collect()
}
