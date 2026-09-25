//! [`Keycap`]: a `⌘1`-style shortcut hint.

use gpui::{App, FontWeight, IntoElement, ParentElement, RenderOnce, SharedString, Styled, Window, div, px};

use crate::ActiveTheme;

/// Where the keycap sits, which decides its colours.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum KeycapStyle {
    /// On ordinary content.
    #[default]
    Plain,
    /// On the accent colour (a selected row, a primary button).
    OnAccent,
    /// Tinted with the accent colour (a text button).
    Accent,
}

#[derive(IntoElement)]
pub struct Keycap {
    label: SharedString,
    style: KeycapStyle,
}

impl Keycap {
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self { label: label.into(), style: KeycapStyle::Plain }
    }

    pub fn style(mut self, style: KeycapStyle) -> Self {
        self.style = style;
        self
    }
}

impl RenderOnce for Keycap {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = cx.theme();
        let k = &t.colors;
        let (bg, fg) = match self.style {
            KeycapStyle::Plain => (k.fill_strong, k.secondary_label),
            KeycapStyle::OnAccent => (k.accent_text.opacity(0.22), k.accent_text.opacity(0.9)),
            KeycapStyle::Accent => (k.accent.opacity(if t.dark { 0.2 } else { 0.12 }), k.accent),
        };
        div()
            .flex_shrink_0()
            .px(px(5.))
            .h(px(18.))
            .min_w(px(18.))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(4.))
            .bg(bg)
            .text_color(fg)
            .text_size(px(11.))
            .font_weight(FontWeight::MEDIUM)
            .child(self.label)
    }
}
