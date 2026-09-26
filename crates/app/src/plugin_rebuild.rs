//! Rebuilding installed plugins for this Delight. Plugins are installed from
//! source (`delight install`); after an update that changes the SDK build,
//! their built files no longer load. At start, Delight then rebuilds them
//! (`delight-plugin-manager`, as `delight rebuild` does), and loads them.

use delight_plugin_manager::{Code, Env, Error, RebuildReport, Target};
use gpui::App;

use crate::state::{self, AppState};

/// How the rebuild went: shown with each plugin that doesn't load.
#[derive(Default)]
pub struct Rebuild {
    pub running: bool,
    /// Plugins (by file name, without `.dylib`) whose rebuild failed, and why.
    pub failed: Vec<(Vec<String>, Error)>,
    /// Plugin files not installed from a source: they can't be rebuilt.
    pub unmanaged: Vec<String>,
    /// Why nothing could be rebuilt.
    pub error: Option<Error>,
}

impl Rebuild {
    /// Why `plugin`'s rebuild failed, if it did.
    pub fn failure(&self, plugin: &str) -> Option<&Error> {
        let failed = self.failed.iter().find(|(plugins, _)| plugins.iter().any(|p| p == plugin));
        failed.map(|(_, error)| error).or(self.error.as_ref())
    }
}

/// Rebuilds in the background if a plugin file needs it.
pub fn start_if_needed(cx: &mut App) {
    if !cx.global::<AppState>().registry.load_errors.iter().any(|e| e.needs_rebuild) {
        return;
    }
    cx.global_mut::<AppState>().plugin_rebuild = Rebuild { running: true, ..Rebuild::default() };
    let rebuild = cx.background_executor().spawn(async move { run() });
    cx.spawn(async move |cx| {
        let result = rebuild.await;
        let _ = cx.update(|cx| {
            cx.global_mut::<AppState>().plugin_rebuild = match result {
                Ok(report) => Rebuild {
                    failed: report.failed.into_iter().map(|f| (f.plugins, f.error)).collect(),
                    unmanaged: report.unmanaged,
                    ..Rebuild::default()
                },
                Err(error) => Rebuild { error: Some(error), ..Rebuild::default() },
            };
            state::reload_plugins(cx);
        });
    })
    .detach();
}

/// Rebuilds for this app binary. Slow (it runs cargo); a panic in it is a
/// failed rebuild, not a crash.
fn run() -> Result<RebuildReport, Error> {
    std::panic::catch_unwind(|| {
        let target = Target::resolve(Some(std::env::current_exe()?))?;
        let env = Env::new(target, |line| log::debug!("rebuild: {line}"))?;
        delight_plugin_manager::rebuild(&env)
    })
    .unwrap_or_else(|_| Err(Error::new(Code::Failed, "the rebuild panicked")))
}
