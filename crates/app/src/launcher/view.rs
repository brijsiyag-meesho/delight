//! Drawing the launcher: the input bar, then (with input) the tool list, the
//! selected tool and the footer.

use delight_ui::{
    ActiveTheme, Button, Caption, Divider, Icon, IconButton, IconName, Keycap, KeycapStyle, LogoBadge, Theme, Tooltip,
    h_flex, keystroke_label, v_flex,
};
use gpui::{
    AnyElement, Context, FocusHandle, Focusable, FontWeight, IntoElement, MouseButton, ParentElement, Render, Styled, Window, div,
    prelude::*, px,
};

use delight_core::stats::human_bytes;
use delight_ui::editor::actions as editor;

use super::history;
use super::{
    BAR_HEIGHT, BAR_ICON_GAP, BAR_ICON_SIZE, BAR_PADDING_X, CONTEXT, ClearInput, Dismiss, FocusNext, FocusPrevious,
    FocusTool, FocusTools, Launcher, NewerCompletion, OlderCompletion, OpenSettings, SelectNext, SelectPrevious,
    SelectTool, TOOL_LIST_CONTEXT, hide,
};
use delight_ui::theme::INPUT_LINE_HEIGHT;

use crate::boundary::PanicBoundary;
use crate::{platform, settings_window};

const LIST_WIDTH: f32 = 200.;

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
            // The theme's background decides the contrast; the native glass
            // or blur underneath adds the blur, and glass its shadow.
            .bg(t.window_tint)
            .border_1()
            .border_color(if platform::uses_liquid_glass() {
                // Spotlight's light rim.
                gpui::hsla(0., 0., 1., if t.dark { 0.2 } else { 0.6 })
            } else {
                k.border
            })
            .font_family(t.text.ui_font.clone())
            .text_color(k.label)
            .text_size(t.text.size_base)
            .on_action(cx.listener(|_, _: &Dismiss, _, cx| cx.defer(hide)))
            .on_action(cx.listener(|this, _: &ClearInput, window, cx| this.clear_input(window, cx)))
            .on_action(cx.listener(|_, _: &OpenSettings, _, cx| cx.defer(settings_window::open)))
            .on_action(cx.listener(|_, _: &FocusNext, window, _| window.focus_next()))
            .on_action(cx.listener(|_, _: &FocusPrevious, window, _| window.focus_prev()))
            .on_action(cx.listener(|this, _: &FocusTools, window, cx| this.focus_tools(window, cx)))
            .on_action(cx.listener(|this, _: &FocusTool, window, cx| this.focus_tool(window, cx)))
            .on_action(cx.listener(|this, _: &SelectPrevious, window, cx| this.select_previous(window, cx)))
            .on_action(cx.listener(|this, _: &SelectNext, _, cx| this.select_next(cx)))
            .on_action(cx.listener(|this, _: &OlderCompletion, _, cx| this.step_completion(1, cx)))
            .on_action(cx.listener(|this, _: &NewerCompletion, _, cx| this.step_completion(-1, cx)))
            .on_action(cx.listener(|this, SelectTool(n): &SelectTool, _, cx| {
                if let Some(index) = n.checked_sub(1) {
                    this.select(index, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &history::Search, window, cx| this.open_history(window, cx)))
            .on_action(cx.listener(|this, _: &history::SelectNext, _, cx| this.move_in_history(1, cx)))
            .on_action(cx.listener(|this, _: &history::SelectPrevious, _, cx| this.move_in_history(-1, cx)))
            .on_action(cx.listener(|this, _: &history::Confirm, window, cx| this.confirm_history(None, window, cx)))
            .on_action(cx.listener(|this, _: &history::Cancel, window, cx| this.close_history(window, cx)))
            // Before the input sees them: ⌘V pastes files, Backspace removes one.
            .capture_action(cx.listener(|this, _: &editor::Paste, window, cx| {
                if this.paste_files(window, cx) {
                    cx.stop_propagation();
                }
            }))
            .capture_action(cx.listener(|this, _: &editor::Backspace, window, cx| {
                if this.backspace_file(window, cx) {
                    cx.stop_propagation();
                }
            }))
            .on_key_down(cx.listener(Self::on_key_down))
            .child(self.render_bar(cx));
        if let Some(history) = &self.history {
            return root.child(Divider::horizontal()).child(self.render_history(history, &t, cx));
        }
        if !self.is_expanded(cx) {
            return root;
        }
        root.child(Divider::horizontal())
            .child(
                h_flex()
                    .flex_1()
                    .min_h(px(0.))
                    .child(self.render_list(&t, window, cx))
                    .child(Divider::vertical())
                    .child(self.render_detail(&t, cx)),
            )
            .child(Divider::horizontal())
            .child(self.render_footer(&t, window, cx))
    }
}

impl Launcher {
    fn render_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let expanded = self.is_expanded(cx);
        // The first input line sits centred in the bar; more lines grow it
        // down. Icon and clear button stay centred on that first line.
        let line = px(INPUT_LINE_HEIGHT);
        let on_first_line = |element: AnyElement| h_flex().h(line).flex_shrink_0().child(element);
        // While searching the history, its search input takes the input's place.
        let (icon, input) = match &self.history {
            Some(history) => (history::icon(cx), history.query.clone()),
            None => (Icon::new(IconName::Zap).color(cx.theme().colors.secondary_label), self.input.clone()),
        };
        let icon = icon.size(px(BAR_ICON_SIZE));
        h_flex()
            .when(self.history.is_some(), |bar| bar.key_context(history::CONTEXT))
            .flex_shrink_0()
            .items_start()
            .min_h(px(BAR_HEIGHT))
            .py((px(BAR_HEIGHT) - line) / 2.)
            .px(px(BAR_PADDING_X))
            .gap(px(BAR_ICON_GAP))
            // The bolt is a handle for moving the window.
            .child(
                on_first_line(icon.into_any_element())
                    .on_mouse_down(MouseButton::Left, |_, window, _| platform::drag_window(window)),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w(px(0.))
                    .gap(px(8.))
                    .child(input)
                    .when(self.history.is_none() && !self.files.is_empty(), |column| {
                        column.child(self.render_files(cx))
                    }),
            )
            .when(expanded && self.history.is_none(), |bar| {
                let clear = IconButton::new("clear", IconName::CircleX)
                    .on_click(cx.listener(|this, _, window, cx| this.clear_input(window, cx)));
                bar.child(on_first_line(clear.into_any_element()))
            })
    }

    /// The pasted files, as tags: icon, name, and ✕ to remove it.
    fn render_files(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();
        let tags = self.files.iter().enumerate().map(|(i, path)| {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.display().to_string());
            let icon = if path.is_dir() { IconName::Folder } else { IconName::File };
            let hover = t.colors.hover;
            h_flex()
                .id(("file", i))
                .h(px(26.))
                .pl(px(8.))
                .pr(px(4.))
                .gap(px(6.))
                .rounded(t.metrics.radius_sm)
                .bg(t.colors.fill)
                .text_size(t.text.size_sm)
                .tooltip(Tooltip::text(path.display().to_string()))
                .child(Icon::new(icon).size(px(14.)).color(t.colors.secondary_label))
                .child(div().max_w(px(220.)).truncate().child(name))
                .child(
                    div()
                        .id(("remove-file", i))
                        .p(px(3.))
                        .rounded(t.metrics.radius_sm)
                        .cursor_pointer()
                        .hover(move |s| s.bg(hover))
                        .on_click(cx.listener(move |this, _, _, cx| this.remove_file(i, cx)))
                        .child(Icon::new(IconName::X).size(px(12.)).color(t.colors.secondary_label)),
                )
        });
        h_flex().flex_wrap().gap(px(6.)).children(tags)
    }

    /// Recommended tools, then the other tools that fit less well. The
    /// selection is the accent colour while the list has focus, grey
    /// otherwise (as in macOS lists).
    fn render_list(&self, t: &Theme, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focused = self.list_focus.is_focused(window);
        let section = |label: &'static str| div().px(px(14.)).py(px(4.)).child(Caption::new(label));
        let recommended = self.candidates.iter().take_while(|c| c.recommended()).count();
        let mut list = v_flex()
            .id("tools")
            .key_context(TOOL_LIST_CONTEXT)
            .track_focus(&self.list_focus)
            .on_key_down(cx.listener(Self::on_list_key_down))
            .w(px(LIST_WIDTH))
            .flex_shrink_0()
            .h_full()
            .overflow_y_scroll()
            .track_scroll(&self.list_scroll)
            .py(px(6.));
        list = if recommended == 0 {
            list.child(
                div()
                    .px(px(14.))
                    .py(px(6.))
                    .text_size(t.text.size_sm)
                    .text_color(t.colors.tertiary_label)
                    .child("No recommendations for this input"),
            )
        } else {
            list.child(section("Recommended"))
        };
        for (i, candidate) in self.candidates.iter().enumerate() {
            if i == recommended {
                list = list
                    .child(div().mx(px(14.)).my(px(6.)).child(Divider::horizontal()))
                    .child(section("Other Matches"));
            }
            let selected = self.selected == Some(i);
            let on_accent = selected && focused;
            let hover = t.colors.hover;
            let keystroke = delight_ui::keystroke_for(&SelectTool(i + 1), window);
            list = list.child(
                h_flex()
                    .id(("tool", i))
                    .mx(px(6.))
                    .px(px(8.))
                    .py(px(5.))
                    .gap(px(8.))
                    .rounded(t.metrics.radius_sm)
                    .cursor_pointer()
                    .when(on_accent, |row| row.bg(t.colors.accent))
                    .when(selected && !focused, |row| row.bg(t.colors.fill_strong))
                    .when(!selected, |row| row.hover(move |s| s.bg(hover)))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.select(i, cx);
                        window.focus(&this.list_focus);
                    }))
                    .child(LogoBadge::new(candidate.icon_svg).size(px(20.)))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .text_color(if on_accent { t.colors.accent_text } else { t.colors.label })
                            .truncate()
                            .child(candidate.title.clone()),
                    )
                    .when_some(keystroke, |row, keystroke| {
                        let style = if on_accent { KeycapStyle::OnAccent } else { KeycapStyle::Plain };
                        row.child(Keycap::new(keystroke_label(&keystroke)).style(style))
                    }),
            );
        }
        list
    }

    /// The selected tool: its name, then its own view.
    fn render_detail(&mut self, t: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let pane = v_flex().id("detail").flex_1().min_w(px(0.)).h_full().gap(px(14.)).px(px(18.)).py(px(14.));
        let Some(candidate) = self.selected_candidate().cloned() else {
            return pane.child(self.render_empty(t));
        };
        // The plugin's name, unless the tool's title already says it.
        let plugin_name = (candidate.plugin_name != candidate.title).then(|| candidate.plugin_name.clone());
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
                    .children(plugin_name),
            )
            .when(self.has_settings(&candidate.plugin_id, cx), |header| {
                let plugin_id = candidate.plugin_id.clone();
                header.child(IconButton::new("tool-settings", IconName::Settings).on_click(move |_, _, cx| {
                    let plugin_id = plugin_id.clone();
                    cx.defer(move |cx| settings_window::open_plugin(cx, &plugin_id));
                }))
            });
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
            // The rest of the pane's height; the view scrolls itself.
            Some(view) => pane.child(v_flex().flex_1().min_h(px(0.)).overflow_hidden().child(PanicBoundary::new(
                view.view().into_any_element(),
                candidate.plugin_id.clone(),
                candidate.plugin_name.clone(),
            ))),
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
    fn render_footer(&self, t: &Theme, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let status: AnyElement = match &self.toast {
            Some(message) => h_flex()
                .gap(px(6.))
                .text_color(t.status.success.fg)
                .child(Icon::new(IconName::Check).size(px(12.)))
                .child(message.clone())
                .into_any_element(),
            None => {
                let s = &self.stats;
                let mut parts = Vec::new();
                if !self.files.is_empty() {
                    let size = human_bytes(usize::try_from(self.files_bytes).unwrap_or(usize::MAX));
                    parts.extend([plural(self.files.len(), "file"), size]);
                }
                if s.chars > 0 || self.files.is_empty() {
                    parts.extend([
                        s.human_size(),
                        plural(s.lines, "line"),
                        plural(s.chars, "char"),
                        plural(s.words, "word"),
                    ]);
                }
                div().text_color(t.colors.tertiary_label).child(parts.join("  ·  ")).into_any_element()
            }
        };
        // Buttons are clicked, not dragged: keep their mouse-downs from the footer.
        let mut actions = h_flex().gap(px(2.)).on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation());
        for (i, (action, key)) in self.keyed_actions(window, cx).into_iter().take(4).enumerate() {
            if i > 0 {
                actions = actions.child(Divider::vertical());
            }
            let mut button = Button::new(("action", i), action.label.clone())
                .text()
                .on_click(cx.listener(move |this, _, _, cx| this.perform(action.clone(), cx)));
            if let Some(keystroke) = &key {
                button = button.shortcut(keystroke_label(keystroke));
            }
            actions = actions.child(button);
        }
        let actions = actions.child(
            IconButton::new("settings", IconName::Settings).on_click(|_, _, cx| cx.defer(settings_window::open)),
        );
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
