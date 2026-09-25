//! [`SegmentedControl`]: an NSSegmentedControl look-alike (tabs). Controlled:
//! pass the selected index, get the picked one in `on_change`.

use std::rc::Rc;

use gpui::{
    App, ElementId, InteractiveElement, IntoElement, MouseButton, ParentElement, RenderOnce, SharedString,
    StatefulInteractiveElement, Styled, Window, div, prelude::FluentBuilder, px, white,
};

use crate::{ActiveTheme, Disableable};

#[derive(IntoElement)]
pub struct SegmentedControl {
    id: ElementId,
    options: Vec<SharedString>,
    selected: usize,
    disabled: bool,
    on_change: Option<Rc<dyn Fn(&usize, &mut Window, &mut App)>>,
}

impl SegmentedControl {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self { id: id.into(), options: Vec::new(), selected: 0, disabled: false, on_change: None }
    }

    pub fn options<S: Into<SharedString>>(mut self, options: impl IntoIterator<Item = S>) -> Self {
        self.options = options.into_iter().map(Into::into).collect();
        self
    }

    pub fn selected(mut self, index: usize) -> Self {
        self.selected = index;
        self
    }

    /// Called with the segment's index when the user picks one.
    pub fn on_change(mut self, handler: impl Fn(&usize, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(handler));
        self
    }
}

impl Disableable for SegmentedControl {
    fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

impl RenderOnce for SegmentedControl {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = cx.theme();
        let (k, dark) = (&t.colors, t.dark);
        // The selected segment is a raised white chip (translucent in dark mode).
        let chip = if dark { white().opacity(0.2) } else { white() };
        let row = div()
            .id(self.id.clone())
            .flex()
            .flex_shrink_0()
            .p(px(2.))
            .gap(px(2.))
            .rounded(px(7.))
            .bg(k.fill_strong)
            .when(self.disabled, |d| d.opacity(0.5).on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation()));
        let (label, secondary) = (k.label, k.secondary_label);
        row.children(self.options.into_iter().enumerate().map(|(i, option)| {
            let selected = i == self.selected;
            div()
                .id(ElementId::NamedInteger(SharedString::from(format!("{}-segment", self.id)), i as u64))
                .px(px(10.))
                .h(px(20.))
                .flex()
                .items_center()
                .rounded(px(5.))
                .text_size(px(12.))
                .text_color(if selected { label } else { secondary })
                .when(selected, |d| d.bg(chip).shadow_sm())
                .when(!selected && !self.disabled, |d| d.cursor_pointer().hover(move |s| s.text_color(label)))
                .when_some(self.on_change.clone().filter(|_| !self.disabled && !selected), |d, f| {
                    d.on_click(move |_, window, cx| f(&i, window, cx))
                })
                .child(option)
        }))
    }
}
