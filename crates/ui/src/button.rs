//! [`Button`] (push, primary and borderless text buttons) and
//! [`IconButton`] (toolbar style).

use std::rc::Rc;

use gpui::{
    App, ClickEvent, ElementId, FontWeight, Hsla, InteractiveElement, IntoElement, MouseButton, ParentElement,
    RenderOnce, SharedString, StatefulInteractiveElement, Styled, Window, div, prelude::FluentBuilder, px,
};

use crate::{ActiveTheme, Disableable, Icon, IconName, Keycap, KeycapStyle, Size, Sizable, Theme};

type ClickHandler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ButtonVariant {
    /// Filled with the accent colour: the window's default action.
    Primary,
    /// AppKit "bezel" push button.
    #[default]
    Secondary,
    /// Borderless, accent-coloured label (AppKit "borderless" / link style).
    Text,
}

/// A button's colours for its variant.
struct Colors {
    bg: Option<Hsla>,
    fg: Hsla,
    hover: Hsla,
    keycap: KeycapStyle,
}

impl ButtonVariant {
    fn colors(self, t: &Theme) -> Colors {
        let k = &t.colors;
        match self {
            ButtonVariant::Primary => {
                Colors { bg: Some(k.accent), fg: k.accent_text, hover: k.accent.opacity(0.85), keycap: KeycapStyle::OnAccent }
            }
            ButtonVariant::Secondary => {
                Colors { bg: Some(k.fill_strong), fg: k.label, hover: k.fill_strong.opacity(1.5), keycap: KeycapStyle::Plain }
            }
            ButtonVariant::Text => Colors {
                bg: None,
                fg: k.accent,
                hover: k.accent.opacity(if t.dark { 0.18 } else { 0.1 }),
                keycap: KeycapStyle::Accent,
            },
        }
    }
}

/// Stops a click on a disabled control from reaching its parents.
fn swallow_clicks<E: InteractiveElement>(element: E) -> E {
    element.on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
}

#[derive(IntoElement)]
pub struct Button {
    id: ElementId,
    label: SharedString,
    icon: Option<IconName>,
    shortcut: Option<SharedString>,
    variant: ButtonVariant,
    emphasized: bool,
    size: Size,
    disabled: bool,
    on_click: Option<ClickHandler>,
}

impl Button {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            icon: None,
            shortcut: None,
            variant: ButtonVariant::default(),
            emphasized: false,
            size: Size::default(),
            disabled: false,
            on_click: None,
        }
    }

    pub fn variant(mut self, variant: ButtonVariant) -> Self {
        self.variant = variant;
        self
    }

    pub fn primary(self) -> Self {
        self.variant(ButtonVariant::Primary)
    }

    pub fn text(self) -> Self {
        self.variant(ButtonVariant::Text)
    }

    pub fn icon(mut self, icon: IconName) -> Self {
        self.icon = Some(icon);
        self
    }

    /// A keycap after the label, e.g. `"↵"` or `"⌥2"`.
    pub fn shortcut(mut self, shortcut: impl Into<SharedString>) -> Self {
        self.shortcut = Some(shortcut.into());
        self
    }

    /// Medium-weight label: the default action among text buttons.
    pub fn emphasized(mut self, emphasized: bool) -> Self {
        self.emphasized = emphasized;
        self
    }

    pub fn on_click(mut self, handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }
}

impl Sizable for Button {
    fn with_size(mut self, size: Size) -> Self {
        self.size = size;
        self
    }
}

impl Disableable for Button {
    fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

impl RenderOnce for Button {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let c = self.variant.colors(cx.theme());
        let text = self.variant == ButtonVariant::Text;
        let padding = if text { self.size.padding_x() - px(2.) } else { self.size.padding_x() };
        let weight = if !text || self.emphasized { FontWeight::MEDIUM } else { FontWeight::NORMAL };
        div()
            .id(self.id)
            .flex_shrink_0()
            .h(self.size.control_height())
            .pl(padding)
            .pr(if self.shortcut.is_some() && !text { px(5.) } else { padding })
            .flex()
            .items_center()
            .gap(px(6.))
            .rounded(px(6.))
            .when_some(c.bg, |d, bg| d.bg(bg))
            .when(self.variant == ButtonVariant::Primary, |d| d.shadow_sm())
            .text_color(c.fg)
            .text_size(self.size.text_size())
            .font_weight(weight)
            .when_some(self.icon, |d, icon| d.child(Icon::new(icon).size(self.size.icon_size() - px(2.))))
            .child(self.label)
            .when_some(self.shortcut, |d, s| d.child(Keycap::new(s).style(c.keycap)))
            .map(|d| {
                if self.disabled {
                    swallow_clicks(d.opacity(0.5))
                } else {
                    let hover = c.hover;
                    d.cursor_pointer()
                        .hover(move |s| s.bg(hover))
                        .when_some(self.on_click, |d, f| d.on_click(move |e, window, cx| f(e, window, cx)))
                }
            })
    }
}

#[derive(IntoElement)]
pub struct IconButton {
    id: ElementId,
    icon: IconName,
    size: Size,
    disabled: bool,
    on_click: Option<ClickHandler>,
}

impl IconButton {
    pub fn new(id: impl Into<ElementId>, icon: IconName) -> Self {
        Self { id: id.into(), icon, size: Size::default(), disabled: false, on_click: None }
    }

    pub fn on_click(mut self, handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }
}

impl Sizable for IconButton {
    fn with_size(mut self, size: Size) -> Self {
        self.size = size;
        self
    }
}

impl Disableable for IconButton {
    fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

impl RenderOnce for IconButton {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let k = &cx.theme().colors;
        let hover = k.hover;
        div()
            .id(self.id)
            .flex_shrink_0()
            .size(self.size.control_height())
            .rounded(px(6.))
            .flex()
            .items_center()
            .justify_center()
            .child(Icon::new(self.icon).size(self.size.icon_size()).color(k.secondary_label))
            .map(|d| {
                if self.disabled {
                    swallow_clicks(d.opacity(0.5))
                } else {
                    d.cursor_pointer()
                        .hover(move |s| s.bg(hover))
                        .when_some(self.on_click, |d, f| d.on_click(move |e, window, cx| f(e, window, cx)))
                }
            })
    }
}
