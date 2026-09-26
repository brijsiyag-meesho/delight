//! Plugin files: `<plugin dir>/*.dylib` (`.so` on Linux) — one compiled
//! plugin per file, built against `delight-sdk` with
//! `delight_sdk::export_plugin!`.
//!
//! A plugin file is loaded once and never unloaded — its code backs GPUI views
//! and vtables that can outlive any reload — so picking up a rebuilt plugin
//! needs a restart.

use std::any::TypeId;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use delight_sdk::{BUILD_ID, Plugin, SDK_TYPE_ID, SDK_VERSION};

use crate::guard;

type Constructor = fn() -> Box<dyn Plugin>;

/// Why a plugin file (or a plugin) didn't load: shown in Settings, and
/// copied for whoever fixes it.
#[derive(Debug, Clone)]
pub struct LoadError {
    /// The plugin file's name (`lucide.dylib`), or `built-in`.
    pub name: String,
    /// The plugin file, for an installed plugin.
    pub path: Option<PathBuf>,
    /// For people: what's wrong, in a few words.
    pub summary: String,
    /// For fixing it: the full reason (both SDK builds, the system's message).
    pub detail: String,
    /// Built for another Delight: building it again for this one fixes it.
    pub needs_rebuild: bool,
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.summary, self.detail)
    }
}

impl LoadError {
    pub fn new(path: &Path, summary: &str, detail: impl Into<String>) -> Self {
        let name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
        Self { name, path: Some(path.to_path_buf()), summary: summary.into(), detail: detail.into(), needs_rebuild: false }
    }

    fn rebuild(path: &Path, detail: impl Into<String>) -> Self {
        Self { needs_rebuild: true, ..Self::new(path, "Built for a different version of Delight", detail) }
    }
}

/// Why a plugin built for `plugin_build` (its `DELIGHT_SDK_BUILD`) can't load.
fn mismatch(plugin_build: &str) -> String {
    // `Delight SDK <version> (<rustc>) src <hash>`
    let version = plugin_build.strip_prefix("Delight SDK ").and_then(|s| s.split(' ').next()).unwrap_or("?");
    let why = if version != SDK_VERSION {
        format!("built for Delight SDK {version}, this Delight has SDK {SDK_VERSION}")
    } else if !plugin_build.contains(delight_sdk::RUSTC_VERSION) {
        format!("built with a different Rust compiler than SDK {SDK_VERSION}")
    } else {
        format!("built from different sources of SDK {SDK_VERSION}")
    };
    format!("{why} (the plugin: {plugin_build}; this Delight: {BUILD_ID})")
}

pub fn load(path: &Path) -> Result<Arc<dyn Plugin>, LoadError> {
    // SAFETY: a plugin runs arbitrary code in-process by design (plugins are
    // trusted). Binding every symbol up front (RTLD_NOW) and the two checks
    // below guard the Rust-ABI calls that follow: only a plugin compiled
    // against this exact SDK build is called into.
    unsafe {
        let flags = libloading::os::unix::RTLD_NOW | libloading::os::unix::RTLD_LOCAL;
        let lib: libloading::Library = libloading::os::unix::Library::open(Some(path), flags)
            .map_err(|e| match e.to_string() {
                // Gatekeeper: a downloaded (quarantined) plugin file.
                msg if msg.contains("disallowed by system policy") => LoadError::new(
                    path,
                    "Blocked by macOS because it was downloaded",
                    format!(
                        "install plugins from source with `delight install`, or run `xattr -d com.apple.quarantine` \
                         on the file ({msg})"
                    ),
                ),
                // A function of the SDK it was built against isn't in this one.
                msg if msg.contains("Symbol not found") => {
                    LoadError::rebuild(path, format!("built against a different Delight SDK than {SDK_VERSION} ({msg})"))
                }
                msg => LoadError::new(path, "Couldn't be opened", msg),
            })?
            .into();
        let not_a_plugin =
            |what: &str| LoadError::new(path, "Not a Delight plugin", format!("no `{what}` (a plugin ends with `export_plugin!`)"));
        let build = lib.get::<*const &'static str>(b"DELIGHT_SDK_BUILD\0").map_err(|_| not_a_plugin("DELIGHT_SDK_BUILD"))?;
        let build: &str = **build;
        if build != BUILD_ID {
            return Err(LoadError::rebuild(path, mismatch(build)));
        }
        let type_id = **lib
            .get::<*const TypeId>(b"DELIGHT_SDK_TYPE_ID\0")
            .map_err(|_| LoadError::rebuild(path, "built with an older Delight SDK"))?;
        if type_id != SDK_TYPE_ID {
            let detail =
                format!("built against a different build of SDK {SDK_VERSION}: same sources, but another Cargo.lock, features or flags");
            return Err(LoadError::rebuild(path, detail));
        }
        let constructor = *lib.get::<Constructor>(b"delight_plugin\0").map_err(|_| not_a_plugin("delight_plugin"))?;
        // Never unloaded — also when the constructor panics, since it may
        // have left behind code the process still references.
        std::mem::forget(lib);
        let plugin = guard::catch(constructor).map_err(|message| LoadError::new(path, "Crashed while loading", message))?;
        Ok(Arc::from(plugin))
    }
}

/// A loaded plugin and the plugin file it came from.
pub type Loaded = (Arc<dyn Plugin>, PathBuf);

/// Loads every plugin file (`.dylib`) directly under `dir`, in name order.
/// Returns the plugins, and why each plugin file that failed didn't load.
pub fn discover(dir: &Path) -> (Vec<Loaded>, Vec<LoadError>) {
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
            Err(e) => errors.push(e),
        }
    }
    (plugins, errors)
}
