//! Plugin dylibs: `<plugin dir>/*.dylib` (`.so` on Linux), built against `delight-sdk` with
//! `delight_sdk::export_plugin!`.
//!
//! A library is loaded once and never unloaded — its code backs GPUI views
//! and vtables that can outlive any reload — so picking up a rebuilt plugin
//! needs a restart.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, anyhow, ensure};
use std::any::TypeId;

use delight_sdk::{BUILD_ID, Plugin, SDK_TYPE_ID, SDK_VERSION};

use crate::guard;

type Constructor = fn() -> Box<dyn Plugin>;

const REBUILD: &str = "it needs a rebuild against this Delight's SDK";

/// Why a plugin built for `plugin_build` (its `DELIGHT_SDK_BUILD`) can't load.
fn mismatch(plugin_build: &str) -> String {
    // `Delight SDK <version> (<rustc>) src <hash>`
    let version = plugin_build.strip_prefix("Delight SDK ").and_then(|s| s.split(' ').next()).unwrap_or("?");
    if version != SDK_VERSION {
        format!("built for Delight SDK {version}, this Delight has SDK {SDK_VERSION} — {REBUILD}")
    } else if !plugin_build.contains(delight_sdk::RUSTC_VERSION) {
        format!("built with a different Rust compiler than SDK {SDK_VERSION} ({plugin_build}) — {REBUILD}")
    } else {
        format!("built against a different build of SDK {SDK_VERSION} — {REBUILD}")
    }
}

pub fn load(path: &Path) -> anyhow::Result<Arc<dyn Plugin>> {
    // SAFETY: a plugin runs arbitrary code in-process by design (plugins are
    // trusted). Binding every symbol up front (RTLD_NOW) and the two checks
    // below guard the Rust-ABI calls that follow: only a plugin compiled
    // against this exact SDK build is called into.
    unsafe {
        let flags = libloading::os::unix::RTLD_NOW | libloading::os::unix::RTLD_LOCAL;
        let lib: libloading::Library = libloading::os::unix::Library::open(Some(path), flags)
            .map_err(|e| match e.to_string() {
                // The full dyld message (a mangled symbol) is only useful in the log.
                // Gatekeeper: a downloaded (quarantined) library.
                msg if msg.contains("disallowed by system policy") => {
                    log::debug!("{}: {msg}", path.display());
                    anyhow!(
                        "macOS blocked it because it was downloaded — install it with Settings → Plugins → Install…, \
                         or run `xattr -d com.apple.quarantine` on the file"
                    )
                }
                msg if msg.contains("Symbol not found") => {
                    log::debug!("{}: {msg}", path.display());
                    anyhow!("built against a different Delight SDK than {SDK_VERSION} — {REBUILD}")
                }
                msg => anyhow!("{msg}"),
            })?
            .into();
        let build = lib
            .get::<*const &'static str>(b"DELIGHT_SDK_BUILD\0")
            .context("not a Delight plugin (no `export_plugin!`)")?;
        let build: &str = **build;
        ensure!(build == BUILD_ID, "{}", mismatch(build));
        let type_id = **lib
            .get::<*const TypeId>(b"DELIGHT_SDK_TYPE_ID\0")
            .map_err(|_| anyhow!("built with an older Delight SDK — {REBUILD}"))?;
        ensure!(type_id == SDK_TYPE_ID, "built against a different build of SDK {SDK_VERSION} — {REBUILD}");
        let constructor = *lib.get::<Constructor>(b"delight_plugin\0").context("missing `delight_plugin`")?;
        // Never unloaded — also when the constructor panics, since it may
        // have left behind code the process still references.
        std::mem::forget(lib);
        let plugin = guard::catch(constructor).map_err(|message| anyhow!("panicked while loading: {message}"))?;
        Ok(Arc::from(plugin))
    }
}

/// A loaded plugin and the library it came from.
pub type Loaded = (Arc<dyn Plugin>, PathBuf);

/// Loads every dynamic library directly under `dir`, in name order. Returns
/// the plugins and one message per library that failed to load.
pub fn discover(dir: &Path) -> (Vec<Loaded>, Vec<String>) {
    let mut plugins = Vec::new();
    let mut errors = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return (plugins, errors);
    };
    let mut paths: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == std::env::consts::DLL_EXTENSION))
        .collect();
    paths.sort();
    for path in paths {
        match load(&path) {
            Ok(p) => plugins.push((p, path)),
            Err(e) => errors.push(format!("{}: {e:#}", path.file_name().unwrap_or_default().to_string_lossy())),
        }
    }
    (plugins, errors)
}
