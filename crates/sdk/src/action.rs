//! Footer commands a tool view lists (see [`crate::ToolView::actions`]); the
//! host draws them and binds their keys, and runs them by calling
//! [`crate::ToolView::perform`].

#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Action {
    pub id: String,
    pub label: String,
    /// The key that runs the action, as a GPUI keystroke (`"enter"`,
    /// `"cmd-enter"`, `"alt-k"`); `None`: it's only clicked. A key Delight's
    /// keymap already binds (Esc, ⌘K…), or an earlier action's, is ignored.
    pub shortcut: Option<String>,
}

impl Action {
    /// A button labelled `label`; pressing it (or its key) calls
    /// [`crate::ToolView::perform`] with `id`.
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self { id: id.into(), label: label.into(), shortcut: None }
    }
    /// Runs the action on `keystroke` (e.g. `"enter"`, `"cmd-enter"`).
    pub fn shortcut(mut self, keystroke: impl Into<String>) -> Self {
        self.shortcut = Some(keystroke.into());
        self
    }
}
