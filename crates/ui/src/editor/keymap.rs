//! The editor's keys. One table lists each action with its macOS shortcuts
//! and handler; it generates the action types, the key bindings and the
//! handler wiring, so the three can't drift apart.

use gpui::{App, Context, Div, InteractiveElement, KeyBinding, actions};

use super::TextEditor;

/// Key context the bindings apply in (only while an editor has focus).
pub const CONTEXT: &str = "Editor";

macro_rules! keymap {
    ($($action:ident [$($key:literal),*] => $handler:ident),* $(,)?) => {
        actions!(editor, [$($action),*]);

        pub(super) fn bind_keys(cx: &mut App) {
            let mut bindings = Vec::new();
            $($(bindings.push(KeyBinding::new($key, $action, Some(CONTEXT)));)*)*
            cx.bind_keys(bindings);
        }

        /// Routes every action to its handler on `editor`.
        pub(super) fn wire(element: Div, cx: &mut Context<TextEditor>) -> Div {
            element $(.on_action(cx.listener(TextEditor::$handler)))*
        }
    };
}

keymap! {
    Backspace ["backspace", "shift-backspace"] => backspace,
    Delete ["delete"] => delete,
    DeleteWordLeft ["alt-backspace"] => delete_word_left,
    DeleteToLineStart ["cmd-backspace"] => delete_to_line_start,
    Left ["left"] => left,
    Right ["right"] => right,
    // ⌃P / ⌃N (Emacs) behave like ↑ / ↓: at the first / last row they
    // propagate, so the launcher moves between tools.
    Up ["up", "ctrl-p"] => up,
    Down ["down", "ctrl-n"] => down,
    WordLeft ["alt-left"] => word_left,
    WordRight ["alt-right"] => word_right,
    LineStart ["cmd-left", "home", "ctrl-a"] => line_start,
    LineEnd ["cmd-right", "end", "ctrl-e"] => line_end,
    DocStart ["cmd-up"] => doc_start,
    DocEnd ["cmd-down"] => doc_end,
    SelectLeft ["shift-left"] => select_left,
    SelectRight ["shift-right"] => select_right,
    SelectUp ["shift-up"] => select_up,
    SelectDown ["shift-down"] => select_down,
    SelectWordLeft ["alt-shift-left"] => select_word_left,
    SelectWordRight ["alt-shift-right"] => select_word_right,
    SelectLineStart ["cmd-shift-left"] => select_line_start,
    SelectLineEnd ["cmd-shift-right"] => select_line_end,
    SelectDocStart ["cmd-shift-up"] => select_doc_start,
    SelectDocEnd ["cmd-shift-down"] => select_doc_end,
    SelectAll ["cmd-a"] => select_all,
    // Accepts the greyed completion; without one, Tab propagates (the
    // launcher moves focus to the tool's fields).
    AcceptCompletion ["tab"] => accept_completion,
    // Plain ↵ is left to the launcher (it runs the primary action).
    Newline ["shift-enter", "alt-enter"] => newline,
    Copy ["cmd-c"] => copy,
    Cut ["cmd-x"] => cut,
    Paste ["cmd-v"] => paste,
    Undo ["cmd-z"] => undo,
    Redo ["cmd-shift-z"] => redo,
    ShowCharacterPalette ["ctrl-cmd-space"] => show_character_palette,
}
