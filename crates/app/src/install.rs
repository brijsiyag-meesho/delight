//! Installing a plugin the user picked: a `.zip` (as `delight package` makes)
//! holding one plugin file, or a bare `.dylib`.

use std::io::Read;
use std::path::Path;

use anyhow::{Context, bail, ensure};
use gpui::App;

use crate::state::{self, AppState};

/// Copies the plugin into the plugins folder and loads it. A new plugin
/// works at once; a replaced one after a restart (loaded code is never
/// unloaded). Returns what to tell the user.
pub fn install_plugin(cx: &mut App, picked: &Path) -> anyhow::Result<String> {
    let (name, bytes) = match picked.extension().and_then(|e| e.to_str()) {
        Some("zip") => plugin_in_zip(picked)?,
        Some("dylib") => {
            let name = picked.file_stem().and_then(|s| s.to_str()).unwrap_or("plugin").to_string();
            (name, std::fs::read(picked)?)
        }
        _ => bail!("choose a .zip made by `delight package`, or a .dylib"),
    };
    let dir = state::settings(cx).effective_plugin_dir();
    std::fs::create_dir_all(&dir)?;
    let file_name = format!("{name}.dylib");
    let dest = dir.join(&file_name);
    let replacing = dest.exists();
    // Replace, never overwrite in place: macOS caches a loaded library's
    // signature per file, and rewriting it gets Delight killed.
    if replacing {
        std::fs::remove_file(&dest)?;
    }
    // Written afresh, the file isn't marked as downloaded (quarantined), so
    // macOS lets Delight load it: the user chose to install it.
    std::fs::write(&dest, bytes).with_context(|| format!("writing {}", dest.display()))?;

    state::reload_plugins(cx);
    let registry = cx.global::<AppState>().registry.clone();
    if let Some(error) = registry.load_errors.iter().find_map(|e| e.strip_prefix(&format!("{file_name}: "))) {
        let error = error.to_string();
        let _ = std::fs::remove_file(&dest);
        state::reload_plugins(cx);
        bail!("{error}");
    }
    let installed = registry.plugins().iter().find(|p| {
        matches!(&p.source, delight_core::registry::PluginSource::Installed { path } if *path == dest)
    });
    let plugin_name = installed.map_or(name, |p| p.manifest().name.clone());
    Ok(if replacing {
        format!("{plugin_name} updated — restart Delight to use the new version")
    } else {
        format!("{plugin_name} installed")
    })
}

/// The one plugin file in a `.zip`: its name (without `.dylib`) and bytes.
fn plugin_in_zip(zip: &Path) -> anyhow::Result<(String, Vec<u8>)> {
    let mut archive = zip::ZipArchive::new(std::fs::File::open(zip)?).context("not a valid .zip")?;
    let plugins: Vec<String> = archive.file_names().filter(|n| n.ends_with(".dylib")).map(String::from).collect();
    ensure!(plugins.len() == 1, "{} should hold one plugin (.dylib), not {}", zip.display(), plugins.len());
    let mut entry = archive.by_name(&plugins[0])?;
    let mut bytes = Vec::new();
    entry.read_to_end(&mut bytes)?;
    let name = Path::new(&plugins[0]).file_stem().and_then(|s| s.to_str()).unwrap_or("plugin").to_string();
    Ok((name, bytes))
}
