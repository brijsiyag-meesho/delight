//! The trait every tool implements.

use crate::{Detection, Input, PluginManifest, SettingsView, ToolView};

/// Implemented by every tool, built-in or loaded from a plugin dylib.
///
/// `detect` is called on a background thread; the view methods on the main
/// thread. A view does slow work (network, processes, big parses) on GPUI's
/// background executor, never in [`ToolView::update`].
pub trait Plugin: Send + Sync {
    fn manifest(&self) -> &PluginManifest;

    /// Which of the plugin's operations fit `input`, and how well — nothing
    /// more. Called on every input change, for every plugin: keep it simple
    /// and cheap (see [`Input`]). Views derive what they need from the input
    /// themselves ([`crate::ToolContext::input`]).
    fn detect(&self, input: &Input) -> Vec<Detection>;

    /// The operation's view for the result pane — options, output, all of it.
    /// Created once per operation and kept while the launcher lives; fed the
    /// input through [`ToolView::update`].
    fn tool_view(&self, operation_id: &str, cx: &mut gpui::App) -> Box<dyn ToolView>;

    /// Whether the plugin has a Settings page ([`Plugin::settings_view`]),
    /// shown as ⚙ in the tool page and the plugin list.
    fn has_settings(&self) -> bool {
        false
    }

    /// The plugin's page in Settings. Read and write values through [`crate::host`]
    /// so they are persisted.
    fn settings_view(&self, _cx: &mut gpui::App) -> Option<Box<dyn SettingsView>> {
        None
    }
}
