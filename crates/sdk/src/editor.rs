//! A soft-wrapping, multi-line text editor built on GPUI's text system.
//!
//! Used for the global input (multi-line) and for form fields (single-line,
//! optionally masked). Up/Down at the first/last visual row *propagate*, so
//! the launcher can use them to move between tools Spotlight-style.

use std::ops::Range;

use gpui::{
    App, Bounds, ClipboardItem, Context, CursorStyle, Element, ElementId, ElementInputHandler, Entity,
    EntityInputHandler, EventEmitter, FocusHandle, Focusable, GlobalElementId, Hsla, KeyBinding, LayoutId,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PaintQuad, Pixels, Point, ScrollHandle,
    SharedString, Style, TextAlign, TextRun, UTF16Selection, Window, WrappedLine, actions, div, fill, font, point,
    prelude::*, px, relative, size,
};
use unicode_segmentation::UnicodeSegmentation;

use crate::theme::theme;

actions!(
    editor,
    [
        Backspace,
        Delete,
        DeleteWordLeft,
        DeleteToLineStart,
        Left,
        Right,
        Up,
        Down,
        WordLeft,
        WordRight,
        LineStart,
        LineEnd,
        DocStart,
        DocEnd,
        SelectLeft,
        SelectRight,
        SelectUp,
        SelectDown,
        SelectWordLeft,
        SelectWordRight,
        SelectLineStart,
        SelectLineEnd,
        SelectDocStart,
        SelectDocEnd,
        SelectAll,
        Newline,
        Copy,
        Cut,
        Paste,
        Undo,
        Redo,
        ShowCharacterPalette,
    ]
);

const CONTEXT: &str = "Editor";

pub fn bind_keys(cx: &mut App) {
    let c = Some(CONTEXT);
    cx.bind_keys([
        KeyBinding::new("backspace", Backspace, c),
        KeyBinding::new("shift-backspace", Backspace, c),
        KeyBinding::new("delete", Delete, c),
        KeyBinding::new("alt-backspace", DeleteWordLeft, c),
        KeyBinding::new("cmd-backspace", DeleteToLineStart, c),
        KeyBinding::new("left", Left, c),
        KeyBinding::new("right", Right, c),
        KeyBinding::new("up", Up, c),
        KeyBinding::new("down", Down, c),
        KeyBinding::new("alt-left", WordLeft, c),
        KeyBinding::new("alt-right", WordRight, c),
        KeyBinding::new("cmd-left", LineStart, c),
        KeyBinding::new("cmd-right", LineEnd, c),
        KeyBinding::new("home", LineStart, c),
        KeyBinding::new("end", LineEnd, c),
        KeyBinding::new("ctrl-a", LineStart, c),
        KeyBinding::new("ctrl-e", LineEnd, c),
        KeyBinding::new("cmd-up", DocStart, c),
        KeyBinding::new("cmd-down", DocEnd, c),
        KeyBinding::new("shift-left", SelectLeft, c),
        KeyBinding::new("shift-right", SelectRight, c),
        KeyBinding::new("shift-up", SelectUp, c),
        KeyBinding::new("shift-down", SelectDown, c),
        KeyBinding::new("alt-shift-left", SelectWordLeft, c),
        KeyBinding::new("alt-shift-right", SelectWordRight, c),
        KeyBinding::new("cmd-shift-left", SelectLineStart, c),
        KeyBinding::new("cmd-shift-right", SelectLineEnd, c),
        KeyBinding::new("cmd-shift-up", SelectDocStart, c),
        KeyBinding::new("cmd-shift-down", SelectDocEnd, c),
        KeyBinding::new("cmd-a", SelectAll, c),
        KeyBinding::new("shift-enter", Newline, c),
        KeyBinding::new("alt-enter", Newline, c),
        KeyBinding::new("cmd-c", Copy, c),
        KeyBinding::new("cmd-x", Cut, c),
        KeyBinding::new("cmd-v", Paste, c),
        KeyBinding::new("cmd-z", Undo, c),
        KeyBinding::new("cmd-shift-z", Redo, c),
        KeyBinding::new("ctrl-cmd-space", ShowCharacterPalette, c),
    ]);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorEvent {
    Changed,
}

impl EventEmitter<EditorEvent> for TextEditor {}

#[derive(Clone)]
struct Snapshot {
    content: String,
    selection: Range<usize>,
}

/// One logical (`\n`-separated) line after wrapping.
struct LineLayout {
    /// Byte offset of the line start in `content`.
    start: usize,
    wrapped: WrappedLine,
    y: Pixels,
}

struct Layout {
    lines: Vec<LineLayout>,
    bounds: Bounds<Pixels>,
    line_height: Pixels,
}

pub struct TextEditor {
    focus_handle: FocusHandle,
    content: String,
    placeholder: SharedString,
    selected_range: Range<usize>,
    selection_reversed: bool,
    marked_range: Option<Range<usize>>,
    multiline: bool,
    masked: bool,
    mono: bool,
    input_font: bool,
    font_size: Pixels,
    line_height: Pixels,
    max_height: Option<Pixels>,
    layout: Option<Layout>,
    is_selecting: bool,
    goal_x: Option<Pixels>,
    undo_stack: Vec<Snapshot>,
    redo_stack: Vec<Snapshot>,
    scroll: ScrollHandle,
    autoscroll: bool,
}

impl TextEditor {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            content: String::new(),
            placeholder: SharedString::default(),
            selected_range: 0..0,
            selection_reversed: false,
            marked_range: None,
            multiline: false,
            masked: false,
            mono: false,
            input_font: false,
            font_size: px(13.),
            line_height: px(18.),
            max_height: None,
            layout: None,
            is_selecting: false,
            goal_x: None,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            scroll: ScrollHandle::new(),
            autoscroll: false,
        }
    }

    pub fn multiline(mut self, max_height: Pixels) -> Self {
        self.multiline = true;
        self.max_height = Some(max_height);
        self
    }
    pub fn masked(mut self, masked: bool) -> Self {
        self.masked = masked;
        self
    }
    /// Use the theme's input font (the launcher's main input).
    pub fn input_font(mut self) -> Self {
        self.input_font = true;
        self
    }
    pub fn mono(mut self, mono: bool) -> Self {
        self.mono = mono;
        self
    }
    pub fn text_size(mut self, font_size: Pixels, line_height: Pixels) -> Self {
        self.font_size = font_size;
        self.line_height = line_height;
        self
    }
    pub fn placeholder(mut self, placeholder: impl Into<SharedString>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    pub fn text(&self) -> &str {
        &self.content
    }

    /// Replace everything (undoable), placing the cursor at the end.
    pub fn set_text(&mut self, text: impl Into<String>, cx: &mut Context<Self>) {
        let text = text.into();
        if text == self.content {
            return;
        }
        self.push_undo();
        let mut text = text;
        if !self.multiline {
            text = text.replace('\n', " ");
        }
        self.selected_range = text.len()..text.len();
        self.content = text;
        self.marked_range = None;
        self.autoscroll = true;
        cx.emit(EditorEvent::Changed);
        cx.notify();
    }

    pub fn select_all_text(&mut self, cx: &mut Context<Self>) {
        self.selected_range = 0..self.content.len();
        self.selection_reversed = false;
        cx.notify();
    }

    // ---------------------------------------------------------------------
    // Editing primitives
    // ---------------------------------------------------------------------

    fn push_undo(&mut self) {
        self.undo_stack.push(Snapshot { content: self.content.clone(), selection: self.selected_range.clone() });
        if self.undo_stack.len() > 200 {
            self.undo_stack.remove(0);
        }
        self.redo_stack.clear();
    }

    fn replace(&mut self, range: Range<usize>, new_text: &str, cx: &mut Context<Self>) {
        let range = self.clamp(range);
        let new_text = if self.multiline { new_text.replace("\r\n", "\n") } else { new_text.replace(['\r', '\n'], " ") };
        self.push_undo();
        self.content.replace_range(range.clone(), &new_text);
        let cursor = range.start + new_text.len();
        self.selected_range = cursor..cursor;
        self.selection_reversed = false;
        self.marked_range = None;
        self.goal_x = None;
        self.autoscroll = true;
        cx.emit(EditorEvent::Changed);
        cx.notify();
    }

    /// `range` limited to the content and snapped to char boundaries. Offsets
    /// arrive from the platform (IME) and mouse, so never trust them blindly.
    fn clamp(&self, range: Range<usize>) -> Range<usize> {
        let floor = |mut i: usize| {
            i = i.min(self.content.len());
            while !self.content.is_char_boundary(i) {
                i -= 1;
            }
            i
        };
        let end = floor(range.end);
        floor(range.start.min(end))..end
    }

    fn cursor(&self) -> usize {
        if self.selection_reversed { self.selected_range.start } else { self.selected_range.end }
    }

    fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.selected_range = offset..offset;
        self.selection_reversed = false;
        self.autoscroll = true;
        cx.notify();
    }

    fn select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        if self.selection_reversed {
            self.selected_range.start = offset;
        } else {
            self.selected_range.end = offset;
        }
        if self.selected_range.end < self.selected_range.start {
            self.selection_reversed = !self.selection_reversed;
            self.selected_range = self.selected_range.end..self.selected_range.start;
        }
        self.autoscroll = true;
        cx.notify();
    }

    fn prev_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .rev()
            .find_map(|(i, _)| (i < offset).then_some(i))
            .unwrap_or(0)
    }

    fn next_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .find_map(|(i, _)| (i > offset).then_some(i))
            .unwrap_or(self.content.len())
    }

    fn word_left(&self, offset: usize) -> usize {
        let before = &self.content[..offset];
        let trimmed = before.trim_end_matches(|c: char| !is_word(c));
        let start = trimmed.trim_end_matches(is_word);
        start.len()
    }

    fn word_right(&self, offset: usize) -> usize {
        let after = &self.content[offset..];
        let skipped = after.trim_start_matches(|c: char| !is_word(c));
        let rest = skipped.trim_start_matches(is_word);
        self.content.len() - rest.len()
    }

    fn line_start(&self, offset: usize) -> usize {
        self.content[..offset].rfind('\n').map(|i| i + 1).unwrap_or(0)
    }

    fn line_end(&self, offset: usize) -> usize {
        self.content[offset..].find('\n').map(|i| offset + i).unwrap_or(self.content.len())
    }

    /// Offset one visual row up/down, or `None` at the top/bottom edge.
    fn vertical(&mut self, offset: usize, down: bool) -> Option<usize> {
        let layout = self.layout.as_ref()?;
        let pos = position_for_offset(layout, &self.display_map(), offset)?;
        let goal = *self.goal_x.get_or_insert(pos.x);
        let lh = layout.line_height;
        let y = if down { pos.y + lh } else { pos.y - lh };
        let total = layout.lines.last().map(|l| l.y + l.wrapped.size(lh).height).unwrap_or(lh);
        if y < px(0.) || y >= total {
            return None;
        }
        Some(offset_for_point(layout, &self.display_map(), point(goal, y + lh / 2.)))
    }

    // ---------------------------------------------------------------------
    // Masking: secrets are displayed as bullets, one per char.
    // ---------------------------------------------------------------------

    fn display_text(&self) -> String {
        if self.masked { "•".repeat(self.content.chars().count()) } else { self.content.clone() }
    }

    fn display_map(&self) -> DisplayMap {
        DisplayMap { masked: self.masked, content: if self.masked { self.content.clone() } else { String::new() } }
    }

    // ---------------------------------------------------------------------
    // Actions
    // ---------------------------------------------------------------------

    fn backspace(&mut self, _: &Backspace, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            let start = self.prev_boundary(self.cursor());
            if start == self.cursor() {
                return;
            }
            self.replace(start..self.cursor(), "", cx);
        } else {
            self.replace(self.selected_range.clone(), "", cx);
        }
    }

    fn delete(&mut self, _: &Delete, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            let end = self.next_boundary(self.cursor());
            if end == self.cursor() {
                return;
            }
            self.replace(self.cursor()..end, "", cx);
        } else {
            self.replace(self.selected_range.clone(), "", cx);
        }
    }

    fn delete_word_left(&mut self, _: &DeleteWordLeft, _: &mut Window, cx: &mut Context<Self>) {
        let range = if self.selected_range.is_empty() {
            self.word_left(self.cursor())..self.cursor()
        } else {
            self.selected_range.clone()
        };
        if !range.is_empty() {
            self.replace(range, "", cx);
        }
    }

    fn delete_to_line_start(&mut self, _: &DeleteToLineStart, _: &mut Window, cx: &mut Context<Self>) {
        let c = self.cursor();
        let start = self.line_start(c);
        let range = if start == c { self.prev_boundary(c)..c } else { start..c };
        if !range.is_empty() {
            self.replace(range, "", cx);
        }
    }

    /// At the start (or end, for →) with nothing selected, ←/→ propagate so
    /// the launcher can switch the tool's mode.
    fn left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
        self.goal_x = None;
        if self.selected_range.is_empty() && self.cursor() == 0 {
            cx.propagate();
        } else if self.selected_range.is_empty() {
            self.move_to(self.prev_boundary(self.cursor()), cx);
        } else {
            self.move_to(self.selected_range.start, cx);
        }
    }

    fn right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
        self.goal_x = None;
        if self.selected_range.is_empty() && self.cursor() == self.content.len() {
            cx.propagate();
        } else if self.selected_range.is_empty() {
            self.move_to(self.next_boundary(self.cursor()), cx);
        } else {
            self.move_to(self.selected_range.end, cx);
        }
    }

    fn up(&mut self, _: &Up, _: &mut Window, cx: &mut Context<Self>) {
        match self.vertical(self.cursor(), false) {
            Some(o) => {
                let goal = self.goal_x;
                self.move_to(o, cx);
                self.goal_x = goal;
            }
            None => cx.propagate(),
        }
    }

    fn down(&mut self, _: &Down, _: &mut Window, cx: &mut Context<Self>) {
        match self.vertical(self.cursor(), true) {
            Some(o) => {
                let goal = self.goal_x;
                self.move_to(o, cx);
                self.goal_x = goal;
            }
            None => cx.propagate(),
        }
    }

    fn word_left_action(&mut self, _: &WordLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.word_left(self.cursor()), cx);
    }
    fn word_right_action(&mut self, _: &WordRight, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.word_right(self.cursor()), cx);
    }
    fn line_start_action(&mut self, _: &LineStart, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.line_start(self.cursor()), cx);
    }
    fn line_end_action(&mut self, _: &LineEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.line_end(self.cursor()), cx);
    }
    fn doc_start(&mut self, _: &DocStart, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
    }
    fn doc_end(&mut self, _: &DocEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.content.len(), cx);
    }
    fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.prev_boundary(self.cursor()), cx);
    }
    fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.next_boundary(self.cursor()), cx);
    }
    fn select_up(&mut self, _: &SelectUp, _: &mut Window, cx: &mut Context<Self>) {
        let target = self.vertical(self.cursor(), false).unwrap_or(0);
        let goal = self.goal_x;
        self.select_to(target, cx);
        self.goal_x = goal;
    }
    fn select_down(&mut self, _: &SelectDown, _: &mut Window, cx: &mut Context<Self>) {
        let target = self.vertical(self.cursor(), true).unwrap_or(self.content.len());
        let goal = self.goal_x;
        self.select_to(target, cx);
        self.goal_x = goal;
    }
    fn select_word_left(&mut self, _: &SelectWordLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.word_left(self.cursor()), cx);
    }
    fn select_word_right(&mut self, _: &SelectWordRight, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.word_right(self.cursor()), cx);
    }
    fn select_line_start(&mut self, _: &SelectLineStart, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.line_start(self.cursor()), cx);
    }
    fn select_line_end(&mut self, _: &SelectLineEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.line_end(self.cursor()), cx);
    }
    fn select_doc_start(&mut self, _: &SelectDocStart, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(0, cx);
    }
    fn select_doc_end(&mut self, _: &SelectDocEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.content.len(), cx);
    }
    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.select_all_text(cx);
    }

    fn newline(&mut self, _: &Newline, _: &mut Window, cx: &mut Context<Self>) {
        if !self.multiline {
            cx.propagate();
            return;
        }
        // Keep the current line's indentation.
        let start = self.line_start(self.cursor());
        let indent: String = self.content[start..].chars().take_while(|c| *c == ' ' || *c == '\t').collect();
        self.replace(self.selected_range.clone(), &format!("\n{indent}"), cx);
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() && !self.masked {
            cx.write_to_clipboard(ClipboardItem::new_string(self.content[self.selected_range.clone()].to_string()));
        }
    }

    fn cut(&mut self, _: &Cut, _: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() && !self.masked {
            cx.write_to_clipboard(ClipboardItem::new_string(self.content[self.selected_range.clone()].to_string()));
            self.replace(self.selected_range.clone(), "", cx);
        }
    }

    fn paste(&mut self, _: &Paste, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            self.replace(self.selected_range.clone(), &text, cx);
        }
    }

    fn undo(&mut self, _: &Undo, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(s) = self.undo_stack.pop() {
            self.redo_stack.push(Snapshot { content: self.content.clone(), selection: self.selected_range.clone() });
            self.content = s.content;
            self.selected_range = s.selection;
            self.autoscroll = true;
            cx.emit(EditorEvent::Changed);
            cx.notify();
        }
    }

    fn redo(&mut self, _: &Redo, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(s) = self.redo_stack.pop() {
            self.undo_stack.push(Snapshot { content: self.content.clone(), selection: self.selected_range.clone() });
            self.content = s.content;
            self.selected_range = s.selection;
            self.autoscroll = true;
            cx.emit(EditorEvent::Changed);
            cx.notify();
        }
    }

    fn show_character_palette(&mut self, _: &ShowCharacterPalette, window: &mut Window, _: &mut Context<Self>) {
        window.show_character_palette();
    }

    fn on_mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus_handle);
        let Some(offset) = self.offset_for_mouse(event.position) else { return };
        self.goal_x = None;
        match event.click_count {
            2 => {
                self.selected_range = self.word_left(offset.min(self.word_right(offset)))..self.word_right(offset);
                cx.notify();
            }
            n if n >= 3 => {
                self.selected_range = self.line_start(offset)..self.line_end(offset);
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

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.is_selecting = false;
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.is_selecting
            && let Some(offset) = self.offset_for_mouse(event.position)
        {
            self.select_to(offset, cx);
        }
    }

    fn offset_for_mouse(&self, position: Point<Pixels>) -> Option<usize> {
        let layout = self.layout.as_ref()?;
        let local = position - layout.bounds.origin;
        Some(offset_for_point(layout, &self.display_map(), local))
    }

    // UTF-16 conversions for the platform input handler.
    fn offset_from_utf16(&self, offset: usize) -> usize {
        let mut utf8 = 0;
        let mut utf16 = 0;
        for ch in self.content.chars() {
            if utf16 >= offset {
                break;
            }
            utf16 += ch.len_utf16();
            utf8 += ch.len_utf8();
        }
        utf8
    }

    fn offset_to_utf16(&self, offset: usize) -> usize {
        self.content[..offset.min(self.content.len())].chars().map(char::len_utf16).sum()
    }

    fn range_to_utf16(&self, r: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(r.start)..self.offset_to_utf16(r.end)
    }

    fn range_from_utf16(&self, r: &Range<usize>) -> Range<usize> {
        self.offset_from_utf16(r.start)..self.offset_from_utf16(r.end)
    }
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Maps content offsets to displayed-text offsets (differs only when masked).
struct DisplayMap {
    masked: bool,
    content: String,
}

impl DisplayMap {
    const BULLET: usize = '•'.len_utf8();

    fn display_offset(&self, offset: usize) -> usize {
        if self.masked { self.content[..offset].chars().count() * Self::BULLET } else { offset }
    }

    fn content_offset(&self, offset: usize) -> usize {
        if !self.masked {
            return offset;
        }
        let n = offset / Self::BULLET;
        self.content.char_indices().nth(n).map(|(i, _)| i).unwrap_or(self.content.len())
    }
}

fn position_for_offset(layout: &Layout, map: &DisplayMap, offset: usize) -> Option<Point<Pixels>> {
    let d = map.display_offset(offset);
    let line = layout.lines.iter().rev().find(|l| l.start <= d)?;
    let p = line.wrapped.position_for_index(d - line.start, layout.line_height)?;
    Some(point(p.x, p.y + line.y))
}

fn offset_for_point(layout: &Layout, map: &DisplayMap, p: Point<Pixels>) -> usize {
    let lh = layout.line_height;
    let Some(first) = layout.lines.first() else { return 0 };
    if p.y < px(0.) {
        return 0;
    }
    let line = layout
        .lines
        .iter()
        .find(|l| p.y < l.y + l.wrapped.size(lh).height)
        .unwrap_or_else(|| layout.lines.last().unwrap_or(first));
    let local = point(p.x.max(px(0.)), (p.y - line.y).max(px(0.)));
    let idx = match line.wrapped.closest_index_for_position(local, lh) {
        Ok(i) | Err(i) => i,
    };
    map.content_offset(line.start + idx.min(line.wrapped.len()))
}

impl EntityInputHandler for TextEditor {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.range_from_utf16(&range_utf16);
        actual_range.replace(self.range_to_utf16(&range));
        Some(self.content[range].to_string())
    }

    fn selected_text_range(&mut self, _: bool, _: &mut Window, _: &mut Context<Self>) -> Option<UTF16Selection> {
        Some(UTF16Selection { range: self.range_to_utf16(&self.selected_range), reversed: self.selection_reversed })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked_range.as_ref().map(|r| self.range_to_utf16(r))
    }

    fn unmark_text(&mut self, _: &mut Window, _: &mut Context<Self>) {
        self.marked_range = None;
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|r| self.range_from_utf16(r))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());
        self.replace(range, new_text, cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|r| self.range_from_utf16(r))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());
        let range = self.clamp(range);
        self.content.replace_range(range.clone(), new_text);
        self.marked_range = (!new_text.is_empty()).then(|| range.start..range.start + new_text.len());
        self.selected_range = new_selected_range_utf16
            .as_ref()
            .map(|r| self.range_from_utf16(r))
            .map(|r| r.start + range.start..r.end + range.start)
            .unwrap_or_else(|| range.start + new_text.len()..range.start + new_text.len());
        cx.emit(EditorEvent::Changed);
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        _bounds: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let layout = self.layout.as_ref()?;
        let range = self.range_from_utf16(&range_utf16);
        let map = self.display_map();
        let start = position_for_offset(layout, &map, range.start)?;
        let end = position_for_offset(layout, &map, range.end)?;
        Some(Bounds::from_corners(
            layout.bounds.origin + start,
            layout.bounds.origin + point(end.x, end.y + layout.line_height),
        ))
    }

    fn character_index_for_point(&mut self, p: Point<Pixels>, _: &mut Window, _: &mut Context<Self>) -> Option<usize> {
        let offset = self.offset_for_mouse(p)?;
        Some(self.offset_to_utf16(offset))
    }
}

// -------------------------------------------------------------------------
// Element
// -------------------------------------------------------------------------

struct TextElement {
    editor: Entity<TextEditor>,
    color: Hsla,
    placeholder_color: Hsla,
    selection_color: Hsla,
    cursor_color: Hsla,
    font_family: SharedString,
}

struct Prepaint {
    lines: Vec<LineLayout>,
    /// The lines show the placeholder, not content.
    placeholder: bool,
    selections: Vec<PaintQuad>,
    cursor: Option<PaintQuad>,
}

impl TextElement {
    fn run(&self, len: usize, color: Hsla) -> TextRun {
        TextRun {
            len,
            font: font(self.font_family.clone()),
            color,
            background_color: None,
            underline: None,
            strikethrough: None,
        }
    }

    /// Shape every logical line; returns lines and total height.
    fn shape(
        &self,
        text: &str,
        color: Hsla,
        font_size: Pixels,
        lh: Pixels,
        wrap: Option<Pixels>,
        window: &mut Window,
    ) -> (Vec<LineLayout>, Pixels) {
        let mut out = Vec::new();
        let mut y = px(0.);
        let mut start = 0;
        for line in text.split('\n') {
            let runs = [self.run(line.len(), color)];
            let shaped = window
                .text_system()
                .shape_text(SharedString::from(line.to_string()), font_size, &runs, wrap, None)
                .ok()
                .and_then(|mut v| (!v.is_empty()).then(|| v.remove(0)))
                .unwrap_or_default();
            let h = shaped.size(lh).height.max(lh);
            out.push(LineLayout { start, wrapped: shaped, y });
            y += h;
            start += line.len() + 1;
        }
        (out, y.max(lh))
    }
}

impl IntoElement for TextElement {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for TextElement {
    type RequestLayoutState = ();
    type PrepaintState = Prepaint;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let editor = self.editor.read(cx);
        let (font_size, lh, multiline) = (editor.font_size, editor.line_height, editor.multiline);
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        if !multiline {
            style.size.height = lh.into();
            return (window.request_layout(style, [], cx), ());
        }
        let text = editor.display_text();
        let text = if text.is_empty() { editor.placeholder.to_string() } else { text };
        let this = TextElement {
            editor: self.editor.clone(),
            color: self.color,
            placeholder_color: self.placeholder_color,
            selection_color: self.selection_color,
            cursor_color: self.cursor_color,
            font_family: self.font_family.clone(),
        };
        let id = window.request_measured_layout(style, move |known, available, window, _cx| {
            let wrap = known.width.or(match available.width {
                gpui::AvailableSpace::Definite(w) => Some(w),
                _ => None,
            });
            let (_, height) = this.shape(&text, this.color, font_size, lh, wrap, window);
            size(wrap.unwrap_or(px(400.)), height)
        });
        (id, ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> Prepaint {
        let editor = self.editor.read(cx);
        let (font_size, lh) = (editor.font_size, editor.line_height);
        let display = editor.display_text();
        let empty = display.is_empty();
        let (text, color) =
            if empty { (editor.placeholder.to_string(), self.placeholder_color) } else { (display, self.color) };
        let wrap = editor.multiline.then_some(bounds.size.width);
        let (lines, _) = self.shape(&text, color, font_size, lh, wrap, window);

        let map = editor.display_map();
        let selected = editor.selected_range.clone();
        let cursor_offset = editor.cursor();
        let tmp = Layout { lines, bounds, line_height: lh };

        let mut selections = Vec::new();
        if !selected.is_empty() && !empty {
            let (ds, de) = (map.display_offset(selected.start), map.display_offset(selected.end));
            for line in &tmp.lines {
                let line_end = line.start + line.wrapped.len();
                if line_end < ds || line.start > de {
                    continue;
                }
                let s = ds.max(line.start) - line.start;
                let e = de.min(line_end) - line.start;
                let (Some(sp), Some(ep)) =
                    (line.wrapped.position_for_index(s, lh), line.wrapped.position_for_index(e, lh))
                else {
                    continue;
                };
                // Selections that continue past a newline extend a little to show it.
                let tail = if de > line_end { px(6.) } else { px(0.) };
                let width = line.wrapped.width().max(bounds.size.width.min(line.wrapped.width() + tail));
                let mut row_y = sp.y;
                while row_y <= ep.y {
                    let x0 = if row_y == sp.y { sp.x } else { px(0.) };
                    let x1 = if row_y == ep.y { ep.x + tail } else { width };
                    let origin = bounds.origin + point(x0, line.y + row_y);
                    selections.push(fill(Bounds::new(origin, size((x1 - x0).max(px(1.)), lh)), self.selection_color));
                    row_y += lh;
                }
            }
        }

        let cursor = if selected.is_empty() {
            let pos = if empty { Some(point(px(0.), px(0.))) } else { position_for_offset(&tmp, &map, cursor_offset) };
            pos.map(|p| {
                let inset = (lh - font_size * 1.2).max(px(0.)) / 2.;
                fill(
                    Bounds::new(bounds.origin + point(p.x, p.y + inset), size(px(2.), lh - inset * 2.)),
                    self.cursor_color,
                )
            })
        } else {
            None
        };

        Prepaint { lines: tmp.lines, placeholder: empty, selections, cursor }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        prepaint: &mut Prepaint,
        window: &mut Window,
        cx: &mut App,
    ) {
        let (focus, lh, autoscroll, scroll) = {
            let e = self.editor.read(cx);
            (e.focus_handle.clone(), e.line_height, e.autoscroll, e.scroll.clone())
        };
        window.handle_input(&focus, ElementInputHandler::new(bounds, self.editor.clone()), cx);
        for quad in prepaint.selections.drain(..) {
            window.paint_quad(quad);
        }
        for line in &prepaint.lines {
            let _ = line.wrapped.paint(bounds.origin + point(px(0.), line.y), lh, TextAlign::Left, None, window, cx);
        }
        let focused = focus.is_focused(window);
        let cursor_bounds = prepaint.cursor.as_ref().map(|q| q.bounds);
        if focused && let Some(cursor) = prepaint.cursor.take() {
            window.paint_quad(cursor);
        }

        // Keep the cursor in view inside the scroll container.
        if autoscroll && let Some(cb) = cursor_bounds {
            let viewport = scroll.bounds();
            let mut offset = scroll.offset();
            if cb.bottom() > viewport.bottom() {
                offset.y -= cb.bottom() - viewport.bottom();
                scroll.set_offset(offset);
                window.refresh();
            } else if cb.top() < viewport.top() {
                offset.y += viewport.top() - cb.top();
                scroll.set_offset(offset);
                window.refresh();
            }
        }

        // The placeholder's layout must not drive mouse/IME offsets: with no
        // content, every position is offset 0.
        let lines = if prepaint.placeholder { Vec::new() } else { std::mem::take(&mut prepaint.lines) };
        self.editor.update(cx, |e, _| {
            e.layout = Some(Layout { lines, bounds, line_height: lh });
            e.autoscroll = false;
        });
    }
}

impl Focusable for TextEditor {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for TextEditor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(window, cx);
        let element = TextElement {
            editor: cx.entity(),
            color: t.label,
            placeholder_color: t.tertiary_label,
            selection_color: t.selection,
            cursor_color: t.accent,
            font_family: match (self.input_font, self.mono) {
                (true, _) => t.input_font.clone(),
                (_, true) => t.mono_font.clone(),
                _ => t.ui_font.clone(),
            },
        };
        let body = div()
            .id("editor-scroll")
            .w_full()
            .when_some(self.max_height, |d, h| d.max_h(h))
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .child(element);
        div()
            .key_context(CONTEXT)
            .track_focus(&self.focus_handle)
            .cursor(CursorStyle::IBeam)
            .w_full()
            .text_size(self.font_size)
            .line_height(self.line_height)
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::delete_word_left))
            .on_action(cx.listener(Self::delete_to_line_start))
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::up))
            .on_action(cx.listener(Self::down))
            .on_action(cx.listener(Self::word_left_action))
            .on_action(cx.listener(Self::word_right_action))
            .on_action(cx.listener(Self::line_start_action))
            .on_action(cx.listener(Self::line_end_action))
            .on_action(cx.listener(Self::doc_start))
            .on_action(cx.listener(Self::doc_end))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::select_up))
            .on_action(cx.listener(Self::select_down))
            .on_action(cx.listener(Self::select_word_left))
            .on_action(cx.listener(Self::select_word_right))
            .on_action(cx.listener(Self::select_line_start))
            .on_action(cx.listener(Self::select_line_end))
            .on_action(cx.listener(Self::select_doc_start))
            .on_action(cx.listener(Self::select_doc_end))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::newline))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::undo))
            .on_action(cx.listener(Self::redo))
            .on_action(cx.listener(Self::show_character_palette))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .child(body)
    }
}

