//! Small AppKit-flavoured building blocks: keycaps, icon badges, buttons,
//! switches, segmented controls, grouped sections, tooltips.

use gpui::{
    AnyElement, AnyView, App, AppContext, Context, Div, ElementId, FontWeight, Hsla, SharedString, Stateful, Window,
    div, prelude::*, px, svg,
};

use crate::theme::{Theme, theme};

/// A small tooltip (macOS style). Attach with
/// `element.tooltip(ui::Tooltip::text("…"))` on a stateful element.
pub struct Tooltip {
    text: SharedString,
}

impl Tooltip {
    pub fn text(text: impl Into<SharedString>) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
        let text = text.into();
        move |_, cx| cx.new(|_| Tooltip { text: text.clone() }).into()
    }
}

impl Render for Tooltip {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(window, cx);
        div()
            .px(px(8.))
            .py(px(4.))
            .rounded(px(6.))
            .bg(if t.dark { gpui::hsla(0., 0., 0.18, 1.) } else { gpui::hsla(0., 0., 0.99, 1.) })
            .border_1()
            .border_color(t.separator)
            .shadow_md()
            .font_family(t.ui_font.clone())
            .text_size(px(11.5))
            .text_color(t.label)
            .child(self.text.clone())
    }
}

pub fn icon(path: &'static str, size: f32, color: Hsla) -> impl IntoElement {
    svg().path(path).size(px(size)).flex_shrink_0().text_color(color)
}

/// `⌘1`-style shortcut hint.
pub fn keycap(t: &Theme, label: impl Into<SharedString>, on_accent: bool) -> Div {
    let (bg, fg) = if on_accent {
        (gpui::hsla(0., 0., 1., 0.22), gpui::hsla(0., 0., 1., 0.9))
    } else {
        (t.fill_strong, t.secondary_label)
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
        .child(label.into())
}

/// Rounded-square tool badge (like a tiny app icon).
pub fn badge(t: &Theme, glyph: &str, color: Hsla, size: f32) -> Div {
    let text_size = match glyph.chars().count() {
        0..=2 => size * 0.42,
        _ => size * 0.32,
    };
    let top = color.opacity(0.95);
    div()
        .flex_shrink_0()
        .size(px(size))
        .rounded(px(size * 0.26))
        .bg(top)
        .border_1()
        .border_color(gpui::hsla(0., 0., 1., if t.dark { 0.12 } else { 0.25 }))
        .flex()
        .items_center()
        .justify_center()
        .text_color(gpui::white())
        .text_size(px(text_size))
        .font_weight(FontWeight::BOLD)
        .font_family(t.mono_font.clone())
        .child(glyph.to_string())
}

#[derive(Clone, Copy, PartialEq)]
pub enum ButtonStyle {
    Primary,
    Secondary,
}

/// Push button, AppKit "bezel" style. Returns a stateful div for `on_click`.
pub fn button(
    t: &Theme,
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    icon_path: Option<&'static str>,
    shortcut: Option<SharedString>,
    style: ButtonStyle,
) -> Stateful<Div> {
    let primary = style == ButtonStyle::Primary;
    let (bg, fg, hover) = if primary {
        (t.accent, t.accent_text, t.accent.opacity(0.85))
    } else {
        (t.fill_strong, t.label, if t.dark { gpui::hsla(0., 0., 1., 0.16) } else { gpui::hsla(0., 0., 0., 0.12) })
    };
    div()
        .id(id.into())
        .flex_shrink_0()
        .h(px(26.))
        .pl(px(10.))
        .pr(px(if shortcut.is_some() { 5. } else { 10. }))
        .flex()
        .items_center()
        .gap(px(6.))
        .rounded(px(6.))
        .bg(bg)
        .when(primary, |d| d.shadow_sm())
        .text_color(fg)
        .text_size(px(12.))
        .font_weight(FontWeight::MEDIUM)
        .cursor_pointer()
        .hover(move |s| s.bg(hover))
        .when_some(icon_path, |d, p| d.child(icon(p, 12., fg)))
        .child(label.into())
        .when_some(shortcut, |d, s| d.child(keycap(t, s, primary)))
}

/// Borderless text action (AppKit "borderless" / link style): accent-coloured
/// label plus a tinted shortcut keycap, highlighted on hover. The primary
/// action is medium weight.
pub fn text_button(
    t: &Theme,
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    shortcut: impl Into<SharedString>,
    primary: bool,
) -> Stateful<Div> {
    let hover = t.accent.opacity(if t.dark { 0.18 } else { 0.1 });
    div()
        .id(id.into())
        .flex_shrink_0()
        .h(px(26.))
        .px(px(8.))
        .flex()
        .items_center()
        .gap(px(6.))
        .rounded(px(6.))
        .text_size(px(12.))
        .font_weight(if primary { FontWeight::MEDIUM } else { FontWeight::NORMAL })
        .text_color(t.accent)
        .cursor_pointer()
        .hover(move |s| s.bg(hover))
        .child(label.into())
        .child(keycap(t, shortcut, false).bg(t.accent.opacity(if t.dark { 0.2 } else { 0.12 })).text_color(t.accent))
}

/// Short vertical divider between inline items.
pub fn divider(t: &Theme) -> Div {
    div().flex_shrink_0().w(px(1.)).h(px(14.)).bg(t.separator)
}

/// Borderless 26pt icon button (toolbar style).
pub fn icon_button(t: &Theme, id: impl Into<ElementId>, icon_path: &'static str) -> Stateful<Div> {
    div()
        .id(id.into())
        .flex_shrink_0()
        .size(px(26.))
        .rounded(px(6.))
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .hover(|s| s.bg(t.hover))
        .child(icon(icon_path, 14., t.secondary_label))
}

/// NSSwitch look-alike.
pub fn switch(t: &Theme, id: impl Into<ElementId>, on: bool) -> Stateful<Div> {
    let track = if on { t.accent } else { t.fill_strong };
    div()
        .id(id.into())
        .flex_shrink_0()
        .w(px(32.))
        .h(px(19.))
        .rounded_full()
        .bg(track)
        .p(px(2.))
        .flex()
        .when(on, |d| d.justify_end())
        .cursor_pointer()
        .child(div().size(px(15.)).rounded_full().bg(gpui::white()).shadow_sm())
}

/// NSSegmentedControl look-alike. `on_pick` wiring is done by the caller via
/// the returned segment ids (`{id}-{index}`).
pub fn segmented(
    t: &Theme,
    id: &str,
    options: &[(SharedString, bool)],
    mut on_segment: impl FnMut(usize, Stateful<Div>) -> Stateful<Div>,
) -> Div {
    let mut row = div().flex().flex_shrink_0().p(px(2.)).gap(px(2.)).rounded(px(7.)).bg(t.fill_strong);
    for (i, (label, selected)) in options.iter().enumerate() {
        let seg = div()
            .id(SharedString::from(format!("{id}-{i}")))
            .px(px(10.))
            .h(px(20.))
            .flex()
            .items_center()
            .rounded(px(5.))
            .text_size(px(12.))
            .cursor_pointer()
            .text_color(if *selected { t.label } else { t.secondary_label })
            .when(*selected, |d| {
                d.bg(if t.dark { gpui::hsla(0., 0., 1., 0.2) } else { gpui::white() }).shadow_sm()
            })
            .when(!*selected, |d| d.hover(|s| s.text_color(t.label)))
            .child(label.clone());
        row = row.child(on_segment(i, seg));
    }
    row
}

/// Small uppercase section caption (System Settings style).
pub fn caption(t: &Theme, text: impl Into<SharedString>) -> Div {
    div()
        .text_size(px(11.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(t.secondary_label)
        .child(text.into())
}

/// Grouped inset container (rounded, filled, hairline rows).
pub fn group(t: &Theme, rows: Vec<AnyElement>) -> Div {
    let n = rows.len();
    let mut g = div().flex().flex_col().rounded(px(8.)).bg(t.fill).border_1().border_color(t.separator);
    for (i, row) in rows.into_iter().enumerate() {
        g = g.child(row);
        if i + 1 < n {
            g = g.child(div().h(px(1.)).mx(px(10.)).bg(t.separator));
        }
    }
    g
}

pub fn hairline(t: &Theme) -> Div {
    div().h(px(1.)).w_full().flex_shrink_0().bg(t.separator)
}
