//! Plugins installed from source, shared by the `delight` CLI and the app.
//! Plugins are shared only as source, a git repo or a folder; the built
//! files in the plugins folder are this Delight's build of them, rebuilt
//! when an update changes its SDK.
//!
//! * `manage` — install, update, rebuild and remove.
//! * `sources` — where installed plugins come from, and their clones.
//! * `project` — reading a plugin repo.
//! * `target` — which Delight to build for, and asking it things.
//! * `kit` — building plugins inside that Delight's SDK kit.
//! * `diagnose` — which crates a plugin's build changed, when Delight won't load it.
//! * `error` — what went wrong, with a stable code.

mod diagnose;
pub mod error;
pub mod kit;
mod manage;
pub mod project;
pub mod sources;
pub mod target;

use std::path::PathBuf;

pub use error::{Code, Error, Result};
pub use manage::{
    InstallReport, Installed, Opened, Pick, RebuildFailure, RebuildReport, SourceUpdate, UpdateStatus, install, open, rebuild,
    remove, update,
};
pub use target::{Info, Target};

/// The Delight to build for, what it said about itself, and where progress
/// goes (the terminal, or the app's log).
pub struct Env {
    pub target: Target,
    pub info: Info,
    progress: Box<dyn Fn(&str)>,
}

impl Env {
    pub fn new(target: Target, progress: impl Fn(&str) + 'static) -> Result<Self> {
        let info = target.info()?;
        Ok(Self { target, info, progress: Box::new(progress) })
    }

    pub fn progress(&self, message: &str) {
        (self.progress)(message);
    }

    /// An installed plugin's file: `<plugins folder>/<name>.dylib`.
    pub fn plugin_file(&self, plugin: &str) -> PathBuf {
        self.info.plugin_dir.join(format!("{plugin}.dylib"))
    }
}
