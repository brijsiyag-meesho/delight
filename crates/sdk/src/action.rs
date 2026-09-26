//! Footer commands a tool view lists (see [`crate::ToolView::actions`]); the
//! host draws them and binds their keys, and runs them by calling
//! [`crate::ToolView::perform`].

#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Action {
    pub id: String,
    pub label: String,
    pub primary: bool,
    /// The action's own key, as a GPUI keystroke (`"cmd-enter"`,
    /// `"cmd-shift-enter"`, `"alt-k"`). Use it for destructive actions: ↵ is
    /// only ever given to an action *without* a shortcut (the primary one, or
    /// the first), and ⌥2… to the rest. `None`: a key is assigned. A key
    /// Delight's keymap already binds (⌘K, Esc…) is ignored: a key is
    /// assigned instead.
    pub shortcut: Option<String>,
}

impl Action {
    /// A button labelled `label`; pressing it (or its key) calls
    /// [`crate::ToolView::perform`] with `id`.
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self { id: id.into(), label: label.into(), primary: false, shortcut: None }
    }
    /// Binds the action to `keystroke` (e.g. `"cmd-enter"`) instead of ↵ / ⌥N.
    pub fn shortcut(mut self, keystroke: impl Into<String>) -> Self {
        self.shortcut = Some(keystroke.into());
        self
    }
    pub fn primary(mut self) -> Self {
        self.primary = true;
        self
    }
}
