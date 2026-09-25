//! A field that records a keyboard shortcut: click it and press the keys.
//! While recording, the modifiers being held show as they're pressed.
//!
//! Like Zed's keystroke recorder, it intercepts keystrokes before the keymap
//! sees them, so a shortcut Delight binds (⌘W, ⌘,) is recorded rather than
//! run. Escape alone stops recording.

use delight_ui::{ActiveTheme, Keycap, KeycapStyle, h_flex, keystroke_keys, modifier_keys, v_flex};
use gpui::{
    App, Context, EventEmitter, FocusHandle, Focusable, InteractiveElement, IntoElement, KeystrokeEvent, Keystroke,
    Modifiers, ModifiersChangedEvent, ParentElement, Render, SharedString, StatefulInteractiveElement, Styled,
    Subscription, Window, div, prelude::FluentBuilder, px,
};

/// A shortcut was pressed while recording.
pub struct Recorded(pub Keystroke);

pub struct ShortcutRecorder {
    focus_handle: FocusHandle,
    /// The shortcut shown when not recording.
    current: Option<Keystroke>,
    /// The modifiers held right now, while recording.
    held: Modifiers,
    /// Why the last pressed keys can't be the shortcut.
    error: Option<SharedString>,
    _intercept: Subscription,
}

impl EventEmitter<Recorded> for ShortcutRecorder {}

impl ShortcutRecorder {
    pub fn new(current: Option<Keystroke>, cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();
        let this = cx.weak_entity();
        let recording = focus_handle.clone();
        let intercept = cx.intercept_keystrokes(move |event, window, cx| {
            if recording.is_focused(window) {
                cx.stop_propagation();
                let _ = this.update(cx, |this, cx| this.on_keystroke(event, window, cx));
            }
        });
        Self { focus_handle, current, held: Modifiers::default(), error: None, _intercept: intercept }
    }

    /// Shows `shortcut` and stops recording.
    pub fn set_current(&mut self, shortcut: Keystroke, window: &mut Window, cx: &mut Context<Self>) {
        self.current = Some(shortcut);
        self.stop(window, cx);
    }

    /// Shows why the recorded shortcut can't be used; keeps recording.
    pub fn set_error(&mut self, error: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.error = Some(error.into());
        cx.notify();
    }

    fn stop(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.error = None;
        self.held = Modifiers::default();
        window.blur();
        cx.notify();
    }

    fn on_keystroke(&mut self, event: &KeystrokeEvent, window: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        // A modifier pressed and released on its own isn't a shortcut.
        if matches!(keystroke.key.as_str(), "shift" | "control" | "alt" | "platform" | "function") {
            return;
        }
        if keystroke.key == "escape" && !keystroke.modifiers.modified() {
            self.stop(window, cx);
            return;
        }
        let keystroke = Keystroke { key_char: None, ..keystroke.clone() };
        cx.emit(Recorded(keystroke));
    }

    fn on_modifiers_changed(&mut self, event: &ModifiersChangedEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.held = event.modifiers;
        cx.notify();
    }
}

impl Focusable for ShortcutRecorder {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for ShortcutRecorder {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let recording = self.focus_handle.is_focused(window);
        let t = cx.theme();
        let k = &t.colors;
        let shown = if recording {
            modifier_keys(&self.held)
        } else {
            self.current.as_ref().map(keystroke_keys).unwrap_or_default()
        };
        let style = if recording { KeycapStyle::Accent } else { KeycapStyle::Plain };
        let field = h_flex()
            .id("shortcut-recorder")
            .key_context("ShortcutRecorder")
            .track_focus(&self.focus_handle)
            .on_modifiers_changed(cx.listener(Self::on_modifiers_changed))
            .on_click(cx.listener(|this, _, window, cx| {
                this.error = None;
                window.focus(&this.focus_handle);
                cx.notify();
            }))
            .w(px(150.))
            .h(px(28.))
            .px(px(6.))
            .gap(px(3.))
            .justify_center()
            .rounded(px(7.))
            .border_1()
            .border_color(if recording { k.focus_ring } else { k.border })
            .bg(if recording { k.selection } else { k.fill })
            .cursor_pointer()
            .children(shown.into_iter().map(|key| Keycap::new(key).style(style)))
            .when(recording && !self.held.modified(), |field| {
                field.text_size(t.text.size_sm).text_color(k.secondary_label).child("Press a shortcut")
            });
        v_flex().items_end().gap(px(4.)).child(field).when_some(self.error.clone(), |d, error| {
            d.child(div().max_w(px(220.)).text_size(px(11.)).text_color(t.status.error.fg).child(error))
        })
    }
}
