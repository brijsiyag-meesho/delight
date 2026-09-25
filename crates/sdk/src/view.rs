//! A tool's UI: its result pane and its Settings page.

use gpui::{AnyView, App};

use crate::{Action, Input};

/// What a [`ToolView`] is showing.
#[derive(Debug, Clone, Default, PartialEq)]
#[non_exhaustive]
pub struct ToolContext {
    pub operation_id: String,
    /// Keep it as long as needed (e.g. for background work): cloning is free.
    pub input: Input,
}

impl ToolContext {
    pub fn new(operation_id: impl Into<String>, input: Input) -> Self {
        Self { operation_id: operation_id.into(), input }
    }
}

/// A tool's result pane: its options, its output — everything the tool shows.
/// The host frames it and draws the footer [`ToolView::actions`].
pub trait ToolView {
    fn view(&self) -> AnyView;
    /// Called after creation and whenever the input changes. Keep it quick:
    /// start slow work on the background executor.
    fn update(&self, context: &ToolContext, cx: &mut App);
    /// Footer actions; the primary one (or the first) is bound to ↵. Delight
    /// renders them; [`crate::ActionKind::Custom`] ones come back to
    /// [`ToolView::perform`].
    fn actions(&self, _cx: &App) -> Vec<Action> {
        Vec::new()
    }
    /// Runs one of this view's [`crate::ActionKind::Custom`] actions.
    fn perform(&self, _action_id: &str, _cx: &mut App) {}
}

/// A plugin's page in Settings. Persist its values through [`crate::host`]. The
/// settings page and the tool views come from the same [`crate::Plugin`], so
/// they can share in-memory state to stay in sync.
pub trait SettingsView {
    fn view(&self) -> AnyView;
}
