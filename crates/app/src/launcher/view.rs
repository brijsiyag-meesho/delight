//! Drawing the launcher: the input bar, then (with input) the tool list, the
//! selected tool and the footer.

use delight_ui::{
    ActiveTheme, Button, Caption, Divider, Icon, IconButton, IconName, Keycap, KeycapStyle, LogoBadge, Theme, h_flex,
    v_flex,
};
use gpui::{
    AnyElement, Context, FocusHandle, Focusable, FontWeight, IntoElement, MouseButton, ParentElement, Render, Styled, Window, div,
    prelude::*, px,
};

use super::footer::ActionKey;
use super::{
    BAR_HEIGHT, BAR_ICON_GAP, BAR_ICON_SIZE, BAR_PADDING_X, CONTEXT, ClearInput, Dismiss, Launcher, RunAltAction, RunPrimaryAction, SelectNext, SelectPrevious,
    SelectTool, hide,
};
use delight_ui::theme::INPUT_LINE_HEIGHT;

use crate::boundary::PanicBoundary;
use crate::platform;

const LIST_WIDTH: f32 = 200.;
/// ⌘1…⌘9.
const NUMBERED_TOOLS: usize = 9;

impl Focusable for Launcher {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for Launcher {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_window_size(window, cx);
        let t = cx.theme().clone();
        let k = &t.colors;
        let root = v_flex()
            .key_context(CONTEXT)
            .track_focus(&self.focus_handle)
            .size_full()
            .overflow_hidden()
            .rounded(px(self.corner_radius()))
            .border_1()
            .map(|root| {
                if platform::uses_liquid_glass() {
                    // On glass: no fill, and Spotlight's light rim.
                    root.border_color(gpui::hsla(0., 0., 1., if t.dark { 0.2 } else { 0.6 }))
                } else {
                    root.border_color(k.border).bg(t.window_tint)
                }
            })
            .font_family(t.text.ui_font.clone())
            .text_color(k.label)
            .text_size(t.text.size_base)
            .on_action(cx.listener(|_, _: &Dismiss, _, cx| cx.defer(hide)))
            .on_action(cx.listener(|this, _: &RunPrimaryAction, _, cx| this.run_action_with_key(ActionKey::Enter, cx)))
            .on_action(cx.listener(|this, RunAltAction(n): &RunAltAction, _, cx| {
                this.run_action_with_key(ActionKey::Alt(*n), cx)
            }))
            .on_action(cx.listener(|this, _: &SelectPrevious, _, cx| this.select_previous(cx)))
            .on_action(cx.listener(|this, _: &SelectNext, _, cx| this.select_next(cx)))
            .on_action(cx.listener(|this, SelectTool(index): &SelectTool, _, cx| this.select(*index, cx)))
            .on_action(cx.listener(|this, _: &ClearInput, window, cx| this.clear_input(window, cx)))
            .on_key_down(cx.listener(Self::on_key_down))
            .child(self.render_bar(cx));
        if !self.is_expanded(cx) {
            return root;
        }
        root.child(Divider::horizontal())
            .child(
                h_flex()
                    .flex_1()
                    .min_h(px(0.))
                    .child(self.render_list(&t, cx))
                    .child(Divider::vertical())
                    .child(self.render_detail(&t, cx)),
            )
            .child(Divider::horizontal())
            .child(self.render_footer(&t, cx))
    }
}

impl Launcher {
    fn render_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let expanded = self.is_expanded(cx);
        // The first input line sits centred in the bar; more lines grow it
        // down. Icon and clear button stay centred on that first line.
        let line = px(INPUT_LINE_HEIGHT);
        let on_first_line = |element: AnyElement| h_flex().h(line).flex_shrink_0().child(element);
        let icon = Icon::new(IconName::Zap).size(px(BAR_ICON_SIZE)).color(cx.theme().colors.secondary_label);
        h_flex()
            .flex_shrink_0()
            .items_start()
            .min_h(px(BAR_HEIGHT))
            .py((px(BAR_HEIGHT) - line) / 2.)
            .px(px(BAR_PADDING_X))
            .gap(px(BAR_ICON_GAP))
            // The bolt is a handle for moving the window.
            .child(on_first_line(icon.into_any_element()).on_mouse_down(MouseButton::Left, |_, window, _| {
                platform::drag_window(window)
            }))
            .child(div().flex_1().min_w(px(0.)).child(self.input.clone()))
            .when(expanded, |bar| {
                let clear = IconButton::new("clear", IconName::CircleX)
                    .on_click(cx.listener(|this, _, window, cx| this.clear_input(window, cx)));
                bar.child(on_first_line(clear.into_any_element()))
            })
    }

    /// Suggested tools, then the other tools that fit less well.
    fn render_list(&self, t: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let section = |label: &'static str| div().px(px(14.)).py(px(4.)).child(Caption::new(label));
        let suggested = self.candidates.iter().take_while(|c| c.suggested()).count();
        let mut list = v_flex()
            .id("tools")
            .w(px(LIST_WIDTH))
            .flex_shrink_0()
            .h_full()
            .overflow_y_scroll()
            .track_scroll(&self.list_scroll)
            .py(px(6.));
        list = if suggested == 0 {
            list.child(
                div().px(px(14.)).py(px(6.)).text_size(t.text.size_sm).text_color(t.colors.tertiary_label).child("No suggestions for this input"),
            )
        } else {
            list.child(section("Suggested"))
        };
        for (i, candidate) in self.candidates.iter().enumerate() {
            if i == suggested {
                list = list.child(div().mx(px(14.)).my(px(6.)).child(Divider::horizontal())).child(section("Other Tools"));
            }
            let selected = self.selected == Some(i);
            let hover = t.colors.hover;
            list = list.child(
                h_flex()
                    .id(("tool", i))
                    .mx(px(6.))
                    .px(px(8.))
                    .py(px(5.))
                    .gap(px(8.))
                    .rounded(t.metrics.radius_sm)
                    .cursor_pointer()
                    .when(selected, |row| row.bg(t.colors.accent))
                    .when(!selected, |row| row.hover(move |s| s.bg(hover)))
                    .on_click(cx.listener(move |this, _, _, cx| this.select(i, cx)))
                    .child(LogoBadge::new(candidate.icon_svg).size(px(20.)))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .text_color(if selected { t.colors.accent_text } else { t.colors.label })
                            .truncate()
                            .child(candidate.title.clone()),
                    )
                    .when(i < NUMBERED_TOOLS, |row| {
                        let style = if selected { KeycapStyle::OnAccent } else { KeycapStyle::Plain };
                        row.child(Keycap::new(format!("⌘{}", i + 1)).style(style))
                    }),
            );
        }
        list
    }

    /// The selected tool: its name, then its own view.
    fn render_detail(&mut self, t: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let pane = v_flex()
            .id("detail")
            .flex_1()
            .min_w(px(0.))
            .h_full()
            .overflow_y_scroll()
            .track_scroll(&self.detail_scroll)
            .gap(px(14.))
            .px(px(18.))
            .py(px(14.));
        let Some(candidate) = self.selected_candidate().cloned() else {
            return pane.child(self.render_empty(t));
        };
        let header = h_flex()
            .h(px(24.))
            .gap(px(8.))
            .child(LogoBadge::new(candidate.icon_svg).size(px(20.)))
            .child(div().flex_shrink_0().font_weight(FontWeight::SEMIBOLD).child(candidate.title.clone()))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .text_size(t.text.size_sm)
                    .text_color(t.colors.tertiary_label)
                    .truncate()
                    .child(candidate.plugin_name.clone()),
            );
        let pane = pane.child(header);
        // A crashed plugin is never called again: say so instead.
        if let Some(message) = self.crash_of(&candidate.plugin_id, cx) {
            let text = format!(
                "{} crashed: {message}\n\nIt's off until Delight restarts (menu bar → Restart Delight).",
                candidate.plugin_name
            );
            return pane.child(error_notice(t, text));
        }
        match self.selected_view(cx) {
            Some(view) => pane.child(PanicBoundary::new(
                view.view().into_any_element(),
                candidate.plugin_id.clone(),
                candidate.plugin_name.clone(),
            )),
            None => pane,
        }
    }

    fn render_empty(&self, t: &Theme) -> impl IntoElement {
        let (title, hint) = if self.candidates.is_empty() {
            ("No tool fits this input", "Plugins in the plugins folder add tools.")
        } else {
            ("No strong match", "Pick a tool on the left, or keep typing.")
        };
        v_flex()
            .flex_1()
            .items_center()
            .justify_center()
            .gap(px(6.))
            .pt(px(120.))
            .child(Icon::new(IconName::Sparkles).size(px(28.)).color(t.colors.tertiary_label))
            .child(div().text_size(t.text.size_lg).font_weight(FontWeight::MEDIUM).child(title))
            .child(div().text_size(t.text.size_sm).text_color(t.colors.secondary_label).child(hint))
    }

    /// Input statistics (or a message), then the selected tool's actions.
    fn render_footer(&self, t: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let status: AnyElement = match &self.toast {
            Some(message) => h_flex()
                .gap(px(6.))
                .text_color(t.status.success.fg)
                .child(Icon::new(IconName::Check).size(px(12.)))
                .child(message.clone())
                .into_any_element(),
            None => {
                let s = &self.stats;
                let text = format!(
                    "{}  ·  {}  ·  {}  ·  {}",
                    s.human_size(),
                    plural(s.lines, "line"),
                    plural(s.chars, "char"),
                    plural(s.words, "word")
                );
                div().text_color(t.colors.tertiary_label).child(text).into_any_element()
            }
        };
        // Buttons are clicked, not dragged: keep their mouse-downs from the footer.
        let mut actions = h_flex().gap(px(2.)).on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation());
        for (i, (action, key)) in self.keyed_actions(cx).into_iter().take(4).enumerate() {
            if i > 0 {
                actions = actions.child(Divider::vertical());
            }
            let button = Button::new(("action", i), action.label.clone())
                .text()
                .shortcut(key.label())
                .emphasized(key == ActionKey::Enter)
                .on_click(cx.listener(move |this, _, _, cx| this.perform(action.clone(), cx)));
            actions = actions.child(button);
        }
        // The footer is a handle for moving the window.
        h_flex()
            .on_mouse_down(MouseButton::Left, |_, window, _| platform::drag_window(window))
            .flex_shrink_0()
            .h(px(44.))
            .px(px(14.))
            .justify_between()
            .gap(px(12.))
            .text_size(t.text.size_sm)
            .child(div().min_w(px(0.)).truncate().child(status))
            .child(actions)
    }
}

fn error_notice(t: &Theme, text: String) -> impl IntoElement {
    let tint = &t.status.error;
    h_flex()
        .items_start()
        .gap(px(8.))
        .px(px(12.))
        .py(px(9.))
        .rounded(t.metrics.radius_md)
        .bg(tint.bg)
        .border_1()
        .border_color(tint.fg.opacity(0.25))
        .child(div().pt(px(1.)).child(Icon::new(IconName::CircleX).size(px(14.)).color(tint.fg)))
        .child(div().flex_1().min_w(px(0.)).child(text))
}

fn plural(n: usize, word: &str) -> String {
    if n == 1 { format!("1 {word}") } else { format!("{n} {word}s") }
}
