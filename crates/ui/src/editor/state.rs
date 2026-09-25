//! The editor entity: content, selection, options, the editing primitives
//! every command goes through, focus, and rendering.

use std::ops::Range;

use gpui::{
    AppContext, Context, CursorStyle, Entity, EventEmitter, FocusHandle, Focusable, InteractiveElement, IntoElement,
    MouseButton, ParentElement, Pixels, Render, ScrollHandle, SharedString, StatefulInteractiveElement, Styled,
    Subscription, Window, div, point, prelude::FluentBuilder, px,
};

use super::blink::BlinkCursor;
use super::element::{Layout, TextElement, offset_for_point, position_for_offset};
use super::history::{Edit, EditKind, History, apply};
use super::{keymap, text};
use crate::ActiveTheme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorEvent {
    /// The text changed.
    Changed,
    Focus,
    Blur,
}

/// Which of the theme's fonts the editor uses.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum EditorFont {
    /// Interface text.
    #[default]
    Ui,
    /// Code.
    Mono,
    /// The launcher's input font (Lilex).
    Input,
}

pub struct TextEditor {
    pub(super) focus_handle: FocusHandle,
    pub(super) content: String,
    pub(super) placeholder: SharedString,
    pub(super) selected_range: Range<usize>,
    pub(super) selection_reversed: bool,
    /// Text an input method is still composing (underlined, not committed).
    pub(super) marked_range: Option<Range<usize>>,
    /// While composing: the edit the composition will record when committed.
    pub(super) composing: Option<Edit>,
    pub(super) multiline: bool,
    pub(super) font: EditorFont,
    pub(super) font_size: Pixels,
    pub(super) line_height: Pixels,
    pub(super) max_height: Option<Pixels>,
    /// The last painted layout: for mouse hit-testing and ↑/↓.
    pub(super) layout: Option<Layout>,
    pub(super) is_selecting: bool,
    /// The column ↑/↓ keep while moving between rows.
    pub(super) goal_x: Option<Pixels>,
    pub(super) history: History,
    pub(super) blink: Entity<BlinkCursor>,
    pub(super) scroll: ScrollHandle,
    /// Scroll the cursor into view on the next paint.
    pub(super) autoscroll: bool,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<EditorEvent> for TextEditor {}

impl TextEditor {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();
        let blink = cx.new(|_| BlinkCursor::new());
        let subscriptions = vec![
            cx.observe(&blink, |_, _, cx| cx.notify()),
            cx.on_focus(&focus_handle, window, |this, _, cx| {
                this.blink.update(cx, |b, cx| b.start(cx));
                cx.emit(EditorEvent::Focus);
            }),
            cx.on_blur(&focus_handle, window, |this, _, cx| {
                this.blink.update(cx, |b, cx| b.stop(cx));
                cx.emit(EditorEvent::Blur);
            }),
        ];
        Self {
            focus_handle,
            content: String::new(),
            placeholder: SharedString::default(),
            selected_range: 0..0,
            selection_reversed: false,
            marked_range: None,
            composing: None,
            multiline: false,
            font: EditorFont::default(),
            font_size: px(13.),
            line_height: px(18.),
            max_height: None,
            layout: None,
            is_selecting: false,
            goal_x: None,
            history: History::default(),
            blink,
            scroll: ScrollHandle::new(),
            autoscroll: false,
            _subscriptions: subscriptions,
        }
    }

    /// Several lines, soft-wrapped; scrolls past `max_height`.
    pub fn multiline(mut self, max_height: Pixels) -> Self {
        self.multiline = true;
        self.max_height = Some(max_height);
        self
    }

    pub fn font(mut self, font: EditorFont) -> Self {
        self.font = font;
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

    /// Replaces everything (undoable), cursor at the end.
    pub fn set_text(&mut self, text: impl Into<String>, cx: &mut Context<Self>) {
        let text = text.into();
        if text != self.content {
            self.history.break_group();
            self.replace(0..self.content.len(), &text, EditKind::Other, cx);
        }
    }

    pub fn select_all_text(&mut self, cx: &mut Context<Self>) {
        self.selected_range = 0..self.content.len();
        self.selection_reversed = false;
        cx.notify();
    }

    // ---------------------------------------------------------------------
    // Editing primitives
    // ---------------------------------------------------------------------

    /// The one place text changes (apart from IME composition): replaces
    /// `range` with `new_text`, records it for undo, puts the cursor after it.
    pub(super) fn replace(&mut self, range: Range<usize>, new_text: &str, kind: EditKind, cx: &mut Context<Self>) {
        let range = text::clamp(&self.content, range);
        let new_text =
            if self.multiline { new_text.replace("\r\n", "\n") } else { new_text.replace(['\r', '\n'], " ") };
        let edit = Edit {
            start: range.start,
            old: self.content[range.clone()].to_string(),
            new: new_text,
            selection_before: self.selected_range.clone(),
        };
        apply(&mut self.content, &edit);
        let cursor = edit.start + edit.new.len();
        self.history.record(edit, kind);
        self.selected_range = cursor..cursor;
        self.selection_reversed = false;
        self.marked_range = None;
        self.changed(cx);
    }

    /// Applies an undo or redo step.
    pub(super) fn apply_history(&mut self, edit: Edit, cx: &mut Context<Self>) {
        apply(&mut self.content, &edit);
        self.selected_range = text::clamp(&self.content, edit.selection_before);
        self.selection_reversed = false;
        self.marked_range = None;
        self.changed(cx);
    }

    /// After any change to the text.
    pub(super) fn changed(&mut self, cx: &mut Context<Self>) {
        self.goal_x = None;
        self.autoscroll = true;
        self.blink.update(cx, |b, cx| b.pause(cx));
        cx.emit(EditorEvent::Changed);
        cx.notify();
    }

    pub(super) fn cursor(&self) -> usize {
        if self.selection_reversed { self.selected_range.start } else { self.selected_range.end }
    }

    pub(super) fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.selected_range = offset..offset;
        self.selection_reversed = false;
        self.moved(cx);
    }

    pub(super) fn select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        if self.selection_reversed {
            self.selected_range.start = offset;
        } else {
            self.selected_range.end = offset;
        }
        if self.selected_range.end < self.selected_range.start {
            self.selection_reversed = !self.selection_reversed;
            self.selected_range = self.selected_range.end..self.selected_range.start;
        }
        self.moved(cx);
    }

    /// After the cursor or selection moved: the next edit is a new undo step.
    fn moved(&mut self, cx: &mut Context<Self>) {
        self.history.break_group();
        self.autoscroll = true;
        self.blink.update(cx, |b, cx| b.pause(cx));
        cx.notify();
    }

    /// The offset one visual row up or down, or `None` at the first or last
    /// row — where the key then propagates (the launcher switches tools).
    pub(super) fn vertical(&mut self, offset: usize, down: bool) -> Option<usize> {
        let layout = self.layout.as_ref()?;
        let pos = position_for_offset(layout, offset)?;
        let goal = *self.goal_x.get_or_insert(pos.x);
        let lh = layout.line_height;
        let y = if down { pos.y + lh } else { pos.y - lh };
        let total = layout.lines.last().map_or(lh, |l| l.y + l.wrapped.size(lh).height);
        if y < px(0.) || y >= total {
            return None;
        }
        Some(offset_for_point(layout, point(goal, y + lh / 2.)))
    }

    pub(super) fn offset_for_mouse(&self, position: gpui::Point<Pixels>) -> Option<usize> {
        let layout = self.layout.as_ref()?;
        Some(offset_for_point(layout, position - layout.bounds.origin))
    }
}

impl Focusable for TextEditor {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for TextEditor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme();
        let element = TextElement {
            editor: cx.entity(),
            color: t.colors.label,
            placeholder_color: t.colors.tertiary_label,
            selection_color: t.colors.selection,
            cursor_color: t.colors.cursor,
            font_family: match self.font {
                EditorFont::Ui => t.text.ui_font.clone(),
                EditorFont::Mono => t.text.mono_font.clone(),
                EditorFont::Input => t.input_font.clone(),
            },
        };
        let body = div()
            .id("editor-scroll")
            .w_full()
            .when_some(self.max_height, |d, h| d.max_h(h))
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .child(element);
        let root = div()
            .key_context(keymap::CONTEXT)
            .track_focus(&self.focus_handle)
            .cursor(CursorStyle::IBeam)
            .w_full()
            .text_size(self.font_size)
            .line_height(self.line_height)
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .child(body);
        keymap::wire(root, cx)
    }
}
