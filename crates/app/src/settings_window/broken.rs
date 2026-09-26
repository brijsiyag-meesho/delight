//! Plugins that don't load, in the Plugins tab: what's wrong in a line, and
//! everything an agent needs to fix it (Copy for Agent).

use std::path::{Path, PathBuf};

use askama::Template;
use delight_core::native::LoadError;
use delight_plugin_manager::Error;
use delight_ui::{ActiveTheme, Icon, IconButton, IconName, h_flex, v_flex};
use gpui::{AnyElement, ClipboardItem, Context, IntoElement, ParentElement, PromptLevel, Styled, Window, div, prelude::*, px};

use super::SettingsWindow;
use crate::plugin_rebuild::Rebuild;
use crate::state::{self, AppState};

impl SettingsWindow {
    /// ⚠, the plugin's file name and what's wrong, then Copy for Agent and 🗑.
    pub(super) fn broken_row(&self, index: usize, error: &LoadError, cx: &mut Context<Self>) -> AnyElement {
        let state = cx.global::<AppState>();
        let plugin = error.name.trim_end_matches(".dylib").to_string();
        let problem = problem(error, &state.plugin_rebuild);
        let report = agent_report(error, &state.plugin_rebuild, &state.settings.effective_plugin_dir());
        let t = cx.theme().clone();
        h_flex()
            .gap(px(10.))
            .px(px(12.))
            .py(px(8.))
            .child(
                div().flex().justify_center().w(px(26.)).child(Icon::new(IconName::TriangleAlert).size(px(16.)).color(t.status.warning.fg)),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w(px(0.))
                    .child(div().child(plugin.clone()))
                    .child(div().text_size(px(11.)).text_color(t.colors.secondary_label).truncate().child(problem)),
            )
            .child(
                IconButton::new(("broken-copy", index), IconName::Copy)
                    .tooltip("Copy for Agent")
                    .on_click(move |_, _, cx| cx.write_to_clipboard(ClipboardItem::new_string(report.clone()))),
            )
            .when_some(error.path.clone(), |row, path| {
                row.child(IconButton::new(("broken-delete", index), IconName::Trash).on_click(cx.listener(
                    move |this, _, window, cx| this.confirm_delete_file(plugin.clone(), path.clone(), window, cx),
                )))
            })
            .into_any_element()
    }

    fn confirm_delete_file(&self, plugin: String, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let answer = window.prompt(PromptLevel::Warning, &format!("Delete “{plugin}”?"), None, &["Delete", "Cancel"], cx);
        cx.spawn(async move |_, cx| {
            if answer.await == Ok(0) {
                let _ = cx.update(|cx| {
                    if let Err(e) = std::fs::remove_file(&path) {
                        log::error!("deleting {}: {e}", path.display());
                    }
                    state::reload_plugins(cx);
                });
            }
        })
        .detach();
    }
}

/// What's wrong, for people: the problem, and where rebuilding it is at.
fn problem(error: &LoadError, rebuild: &Rebuild) -> String {
    let plugin = error.name.trim_end_matches(".dylib");
    if !error.needs_rebuild {
        return error.summary.clone();
    }
    if rebuild.running {
        format!("{} · rebuilding…", error.summary)
    } else if rebuild.unmanaged.iter().any(|p| p == plugin) {
        format!("{} · not installed from source: install it again", error.summary)
    } else if let Some(failure) = rebuild.failure(plugin) {
        format!("{} · the rebuild failed: {}", error.summary, failure.message)
    } else {
        error.summary.clone()
    }
}

/// Everything an agent needs to fix the plugin: what's wrong, where its code
/// is, the rebuild's error and log, and the commands that reproduce it
/// (templates/agent_report.txt).
#[derive(Template)]
#[template(path = "agent_report.txt")]
struct AgentReport<'a> {
    plugin: &'a str,
    error: &'a LoadError,
    source: Option<Source>,
    failure: Option<&'a Error>,
}

fn agent_report(error: &LoadError, rebuild: &Rebuild, plugin_dir: &Path) -> String {
    let plugin = error.name.trim_end_matches(".dylib");
    let report = AgentReport { plugin, error, source: source_of(plugin, plugin_dir), failure: rebuild.failure(plugin) };
    report.render().unwrap_or_else(|e| format!("{error}\n(the report couldn't be written: {e})"))
}

/// Where a plugin was installed from (as the CLI's `Origin`).
enum Source {
    /// A git repo, maybe pinned to a rev, and the clone of it.
    Git { url: String, rev: Option<String>, clone: PathBuf },
    /// A folder on this Mac, used where it is.
    Folder(PathBuf),
}

impl Source {
    /// The repo link (with `@rev`), or the folder.
    fn describe(&self) -> String {
        match self {
            Source::Git { url, rev: Some(rev), .. } => format!("{url}@{rev}"),
            Source::Git { url, rev: None, .. } => url.clone(),
            Source::Folder(folder) => folder.display().to_string(),
        }
    }

    /// Where its code is.
    fn code(&self) -> &Path {
        match self {
            Source::Git { clone, .. } => clone,
            Source::Folder(folder) => folder,
        }
    }
}

/// From the `delight` CLI's record, `<plugins folder>/sources.json`.
fn source_of(plugin: &str, plugin_dir: &Path) -> Option<Source> {
    let record: serde_json::Value = serde_json::from_slice(&std::fs::read(plugin_dir.join("sources.json")).ok()?).ok()?;
    let installs = |s: &&serde_json::Value| s["plugins"].as_array().is_some_and(|p| p.iter().any(|p| p == plugin));
    let source = record["sources"].as_array()?.iter().find(installs)?;
    match source["kind"].as_str()? {
        "git" => Some(Source::Git {
            url: source["url"].as_str()?.to_string(),
            rev: source["rev"].as_str().map(String::from),
            clone: plugin_dir.join("sources").join(source["name"].as_str()?),
        }),
        "folder" => Some(Source::Folder(PathBuf::from(source["path"].as_str()?))),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stale(path: &str) -> LoadError {
        LoadError::new(Path::new(path), "Built for a different version of Delight", "built for Delight SDK 0.0.9")
    }

    #[test]
    fn reports_a_git_source_and_its_failed_rebuild() {
        let error = stale("/p/lucide.dylib");
        let failure = Error::new(delight_plugin_manager::Code::BuildFailed, "cargo build failed").log("/c/logs/repo.log".into());
        let source = Source::Git { url: "https://github.com/a/plugins".into(), rev: None, clone: "/p/sources/plugins".into() };
        let report = AgentReport { plugin: "lucide", error: &error, source: Some(source), failure: Some(&failure) }.render().unwrap();
        assert!(report.starts_with("The Delight plugin `lucide` doesn't load.\n\nProblem: Built for"), "{report}");
        assert!(report.contains("\nPlugin file: /p/lucide.dylib\nSource: https://github.com/a/plugins (code in /p/sources/plugins)\n"));
        assert!(report.contains("\nRebuild error: cargo build failed\nBuild log: /c/logs/repo.log\n\nReproduce"), "{report}");
        assert!(report.contains("  delight update lucide --json --restart\nThe Delight and SDK"), "{report}");
    }

    #[test]
    fn reports_a_folder_source() {
        let error = stale("/p/lucide.dylib");
        let source = Source::Folder("/code/plugins".into());
        let report = AgentReport { plugin: "lucide", error: &error, source: Some(source), failure: None }.render().unwrap();
        let expected = "Source: /code/plugins (code in /code/plugins)\n\n\
            Reproduce, and check a fix (prints JSON with an error code, message and hint):\n\
            \x20 cd \"/code/plugins\" && delight build -p lucide --json\n\
            Once it builds and loads, install it:\n\
            \x20 cd \"/code/plugins\" && delight install -p lucide --json --restart\n\
            The Delight and SDK it must be built for: `delight info --json`.";
        assert!(report.ends_with(expected), "{report}");
    }

    #[test]
    fn reports_a_plugin_without_a_source() {
        let error = stale("/p/old.dylib");
        let report = AgentReport { plugin: "old", error: &error, source: None, failure: None }.render().unwrap();
        assert!(report.contains("Source: none recorded (it wasn't installed with `delight install`)\n\nInstall it from its source"), "{report}");
    }
}
