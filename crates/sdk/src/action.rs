//! Footer commands a tool view lists (see [`crate::ToolView::actions`]); the
//! host draws them and binds their keys.

#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Action {
    pub id: String,
    pub label: String,
    pub primary: bool,
    /// The action's own key, as a GPUI keystroke (`"cmd-enter"`,
    /// `"cmd-shift-enter"`, `"alt-k"`). Use it for destructive actions: ↵ is
    /// only ever given to an action *without* a shortcut (the primary one, or
    /// the first), and ⌥2… to the rest. `None`: a key is assigned.
    pub shortcut: Option<String>,
    pub kind: ActionKind,
}

impl Action {
    pub fn new(id: impl Into<String>, label: impl Into<String>, kind: ActionKind) -> Self {
        Self { id: id.into(), label: label.into(), primary: false, shortcut: None, kind }
    }
    pub fn copy(id: impl Into<String>, label: impl Into<String>, text: impl Into<String>) -> Self {
        Self::new(id, label, ActionKind::Copy { text: text.into() })
    }
    pub fn open_url(id: impl Into<String>, label: impl Into<String>, url: impl Into<String>) -> Self {
        Self::new(id, label, ActionKind::OpenUrl { url: url.into() })
    }
    /// An action the tool view handles itself (see [`ActionKind::Custom`]).
    pub fn custom(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self::new(id, label, ActionKind::Custom)
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

/// What the host does when an action is triggered.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum ActionKind {
    /// Copy `text` to the clipboard, confirm it in the status bar and — if the
    /// user turned on "Hide after copy" — hide Delight.
    Copy { text: String },
    /// Open `url` in the default browser.
    OpenUrl { url: String },
    /// Handled by the tool: Delight renders the button (and its ↵/⌥
    /// shortcut) and calls [`crate::ToolView::perform`] with the action's id.
    Custom,
}
