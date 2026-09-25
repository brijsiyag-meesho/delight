//! What each key does ([`super::keymap`]), and the mouse. Every edit goes
//! through `replace`, every cursor move through `move_to` / `select_to`.

use gpui::{ClipboardItem, Context, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Window};

use super::history::EditKind;
use super::keymap::*;
use super::{TextEditor, text};

impl TextEditor {
    pub(super) fn backspace(&mut self, _: &Backspace, _: &mut Window, cx: &mut Context<Self>) {
        let range = if self.selected_range.is_empty() {
            text::prev_grapheme(&self.content, self.cursor())..self.cursor()
        } else {
            self.selected_range.clone()
        };
        if !range.is_empty() {
            self.replace(range, "", EditKind::Backspace, cx);
        }
    }

    pub(super) fn delete(&mut self, _: &Delete, _: &mut Window, cx: &mut Context<Self>) {
        let range = if self.selected_range.is_empty() {
            self.cursor()..text::next_grapheme(&self.content, self.cursor())
        } else {
            self.selected_range.clone()
        };
        if !range.is_empty() {
            self.replace(range, "", EditKind::DeleteForward, cx);
        }
    }

    pub(super) fn delete_word_left(&mut self, _: &DeleteWordLeft, _: &mut Window, cx: &mut Context<Self>) {
        let range = if self.selected_range.is_empty() {
            text::word_left(&self.content, self.cursor())..self.cursor()
        } else {
            self.selected_range.clone()
        };
        if !range.is_empty() {
            self.replace(range, "", EditKind::Other, cx);
        }
    }

    /// ⌘⌫: to the line start; at the line start, joins with the line above.
    pub(super) fn delete_to_line_start(&mut self, _: &DeleteToLineStart, _: &mut Window, cx: &mut Context<Self>) {
        let c = self.cursor();
        let start = text::line_start(&self.content, c);
        let range = if start == c { text::prev_grapheme(&self.content, c)..c } else { start..c };
        if !range.is_empty() {
            self.replace(range, "", EditKind::Other, cx);
        }
    }

    /// At the start with nothing selected, ← propagates (the launcher uses it
    /// to switch the tool's mode); likewise → at the end.
    pub(super) fn left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
        self.goal_x = None;
        if !self.selected_range.is_empty() {
            self.move_to(self.selected_range.start, cx);
        } else if self.cursor() == 0 {
            cx.propagate();
        } else {
            self.move_to(text::prev_grapheme(&self.content, self.cursor()), cx);
        }
    }

    pub(super) fn right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
        self.goal_x = None;
        if !self.selected_range.is_empty() {
            self.move_to(self.selected_range.end, cx);
        } else if self.cursor() == self.content.len() {
            cx.propagate();
        } else {
            self.move_to(text::next_grapheme(&self.content, self.cursor()), cx);
        }
    }

    /// ↑ on the first row propagates (the launcher moves between tools).
    pub(super) fn up(&mut self, _: &Up, _: &mut Window, cx: &mut Context<Self>) {
        self.move_vertically(false, cx);
    }

    pub(super) fn down(&mut self, _: &Down, _: &mut Window, cx: &mut Context<Self>) {
        self.move_vertically(true, cx);
    }

    fn move_vertically(&mut self, down: bool, cx: &mut Context<Self>) {
        match self.vertical(self.cursor(), down) {
            Some(offset) => {
                let goal = self.goal_x;
                self.move_to(offset, cx);
                self.goal_x = goal;
            }
            None => cx.propagate(),
        }
    }

    pub(super) fn word_left(&mut self, _: &WordLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(text::word_left(&self.content, self.cursor()), cx);
    }

    pub(super) fn word_right(&mut self, _: &WordRight, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(text::word_right(&self.content, self.cursor()), cx);
    }

    pub(super) fn line_start(&mut self, _: &LineStart, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(text::line_start(&self.content, self.cursor()), cx);
    }

    pub(super) fn line_end(&mut self, _: &LineEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(text::line_end(&self.content, self.cursor()), cx);
    }

    pub(super) fn doc_start(&mut self, _: &DocStart, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
    }

    pub(super) fn doc_end(&mut self, _: &DocEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.content.len(), cx);
    }

    pub(super) fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(text::prev_grapheme(&self.content, self.cursor()), cx);
    }

    pub(super) fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(text::next_grapheme(&self.content, self.cursor()), cx);
    }

    pub(super) fn select_up(&mut self, _: &SelectUp, _: &mut Window, cx: &mut Context<Self>) {
        self.select_vertically(false, cx);
    }

    pub(super) fn select_down(&mut self, _: &SelectDown, _: &mut Window, cx: &mut Context<Self>) {
        self.select_vertically(true, cx);
    }

    /// Past the first / last row, extends to the start / end.
    fn select_vertically(&mut self, down: bool, cx: &mut Context<Self>) {
        let edge = if down { self.content.len() } else { 0 };
        let target = self.vertical(self.cursor(), down).unwrap_or(edge);
        let goal = self.goal_x;
        self.select_to(target, cx);
        self.goal_x = goal;
    }

    pub(super) fn select_word_left(&mut self, _: &SelectWordLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(text::word_left(&self.content, self.cursor()), cx);
    }

    pub(super) fn select_word_right(&mut self, _: &SelectWordRight, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(text::word_right(&self.content, self.cursor()), cx);
    }

    pub(super) fn select_line_start(&mut self, _: &SelectLineStart, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(text::line_start(&self.content, self.cursor()), cx);
    }

    pub(super) fn select_line_end(&mut self, _: &SelectLineEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(text::line_end(&self.content, self.cursor()), cx);
    }

    pub(super) fn select_doc_start(&mut self, _: &SelectDocStart, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(0, cx);
    }

    pub(super) fn select_doc_end(&mut self, _: &SelectDocEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.content.len(), cx);
    }

    pub(super) fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.select_all_text(cx);
    }

    /// Tab: inserts the completion shown after the cursor; without one, the
    /// key propagates.
    pub(super) fn accept_completion(&mut self, _: &AcceptCompletion, _: &mut Window, cx: &mut Context<Self>) {
        match self.visible_completion() {
            Some(completion) => {
                let end = self.content.len();
                cx.emit(super::EditorEvent::CompletionAccepted);
                self.replace(end..end, &completion, EditKind::Other, cx);
            }
            None => cx.propagate(),
        }
    }

    /// ⇧↵ / ⌥↵ in a multi-line editor, keeping the line's indentation. A
    /// single-line editor lets it propagate.
    pub(super) fn newline(&mut self, _: &Newline, _: &mut Window, cx: &mut Context<Self>) {
        if !self.multiline {
            cx.propagate();
            return;
        }
        let start = text::line_start(&self.content, self.cursor());
        let indent: String = self.content[start..].chars().take_while(|c| *c == ' ' || *c == '\t').collect();
        self.replace(self.selected_range.clone(), &format!("\n{indent}"), EditKind::Other, cx);
    }

    pub(super) fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(self.content[self.selected_range.clone()].to_string()));
        }
    }

    pub(super) fn cut(&mut self, _: &Cut, _: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(self.content[self.selected_range.clone()].to_string()));
            self.replace(self.selected_range.clone(), "", EditKind::Other, cx);
        }
    }

    pub(super) fn paste(&mut self, _: &Paste, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            self.replace(self.selected_range.clone(), &text, EditKind::Other, cx);
        }
    }

    pub(super) fn undo(&mut self, _: &Undo, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(edit) = self.history.undo() {
            self.apply_history(edit, cx);
        }
    }

    pub(super) fn redo(&mut self, _: &Redo, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(edit) = self.history.redo() {
            self.apply_history(edit, cx);
        }
    }

    pub(super) fn show_character_palette(&mut self, _: &ShowCharacterPalette, window: &mut Window, _: &mut Context<Self>) {
        window.show_character_palette();
    }

    // ---------------------------------------------------------------------
    // Mouse: click places the cursor, drag selects, ⇧-click extends;
    // double-click selects a word, triple-click a line.
    // ---------------------------------------------------------------------

    pub(super) fn on_mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus_handle);
        let Some(offset) = self.offset_for_mouse(event.position) else { return };
        self.goal_x = None;
        match event.click_count {
            2 => {
                self.selected_range = text::word_at(&self.content, offset);
                cx.notify();
            }
            n if n >= 3 => {
                self.selected_range = text::line_start(&self.content, offset)..text::line_end(&self.content, offset);
                cx.notify();
            }
            _ => {
                self.is_selecting = true;
                if event.modifiers.shift {
                    self.select_to(offset, cx);
                } else {
                    self.move_to(offset, cx);
                }
            }
        }
    }

    pub(super) fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.is_selecting = false;
    }

    pub(super) fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.is_selecting
            && let Some(offset) = self.offset_for_mouse(event.position)
        {
            self.select_to(offset, cx);
        }
    }
}
