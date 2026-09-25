//! [`TextEditor`]: a soft-wrapping, single- or multi-line text field built on
//! GPUI's text system (GPUI has no text input of its own; this follows the
//! pattern of GPUI's `examples/input.rs`).
//!
//! Used for the launcher's input and the Settings window's fields. ↑/↓ on the
//! first/last row and ←/→ at the ends propagate, so the launcher can use them
//! to move between tools and modes.
//!
//! * [`text`] — pure navigation: graphemes, words, lines, UTF-16.
//! * [`history`] — undo/redo of edits, merging consecutive typing.
//! * `blink` — the blinking cursor.
//! * `keymap` — actions and their macOS shortcuts.
//! * `state` — the editor entity and its editing primitives.
//! * `commands` — what each key and mouse gesture does.
//! * `ime` — the platform's text input.
//! * `element` — layout and painting.

mod blink;
mod commands;
mod element;
pub mod history;
mod ime;
mod keymap;
mod state;
pub mod text;

pub use state::{EditorEvent, EditorFont, TextEditor};

pub(crate) fn init(cx: &mut gpui::App) {
    keymap::bind_keys(cx);
}
