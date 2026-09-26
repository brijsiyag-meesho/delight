//! The JSON tool's view: mode tabs, formatting buttons, the result.

use delight_sdk::{Action, ToolContext, ToolView, host};
use delight_ui::{IconButton, IconName, SegmentedControl, Selectable, h_flex, v_flex};
use gpui::{
    AnyView, App, ClipboardItem, Context, Entity, FocusHandle, InteractiveElement, IntoElement, ParentElement, Render,
    SharedString, StatefulInteractiveElement, Styled, Task, Window, px,
};

use super::convert::{Indent, Mode, Options, convert};
use super::output::Output;

const MODES: [(Mode, &str); 4] =
    [(Mode::Format, "Format"), (Mode::Minify, "Minify"), (Mode::Escape, "Escape"), (Mode::Unescape, "Unescape")];

impl Indent {
    fn toggled(self) -> Self {
        match self {
            Indent::Two => Indent::Four,
            Indent::Four => Indent::Two,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Indent::Two => "2",
            Indent::Four => "4",
        }
    }
}

pub struct JsonView {
    /// The mode tabs' focus: a Tab stop, where ← / → switch modes.
    modes_focus: FocusHandle,
    input: SharedString,
    options: Options,
    output: Output,
    /// Minified, for "Copy minified" next to a formatted result.
    minified: Option<String>,
    /// Replacing it cancels the conversion in progress.
    _task: Option<Task<()>>,
}

impl JsonView {
    fn copy_label(&self) -> &'static str {
        match self.options.mode {
            Mode::Format => "Copy formatted",
            Mode::Minify => "Copy minified",
            Mode::Escape => "Copy escaped",
            Mode::Unescape => "Copy unescaped",
        }
    }

    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            modes_focus: cx.focus_handle().tab_stop(true),
            input: SharedString::default(),
            options: Options::default(),
            output: Output::default(),
            minified: None,
            _task: None,
        }
    }

    fn set_options(&mut self, options: Options, cx: &mut Context<Self>) {
        self.options = options;
        self.convert(cx);
    }

    /// Converts the input in the background, then shows the result.
    fn convert(&mut self, cx: &mut Context<Self>) {
        let (input, options) = (self.input.clone(), self.options);
        self._task = Some(cx.spawn(async move |this, cx| {
            let (output, minified) = cx
                .background_executor()
                .spawn(async move {
                    let (conversion, minified) = convert(&input, options);
                    (Output::new(conversion, input.trim()), minified)
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.output = output;
                this.minified = minified;
                cx.notify();
            });
        }));
    }
}

/// The view, as the host sees it.
pub struct JsonTool(pub Entity<JsonView>);

impl ToolView for JsonTool {
    fn view(&self) -> AnyView {
        self.0.clone().into()
    }

    fn update(&self, context: &ToolContext, cx: &mut App) {
        let input = context.input.text.clone();
        self.0.update(cx, |view, cx| {
            view.input = input;
            view.convert(cx);
        });
    }

    fn actions(&self, cx: &App) -> Vec<Action> {
        let view = self.0.read(cx);
        let mut actions = view.output.actions(view.copy_label());
        if view.minified.is_some() {
            actions.insert(1, Action::new("copy_minified", "Copy minified").shortcut("cmd-enter"));
        }
        actions
    }

    fn perform(&self, action_id: &str, cx: &mut App) {
        let view = self.0.read(cx);
        let (label, text) = match action_id {
            "copy" if let Some(text) = view.output.text() => (view.copy_label(), text.to_string()),
            "copy_minified" if let Some(minified) = &view.minified => ("Copy minified", minified.clone()),
            _ => return,
        };
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        host(cx).toast(format!("{label} — copied to clipboard").into(), cx);
    }
}

impl Render for JsonView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let options = self.options;
        let modes = SegmentedControl::new("json-mode")
            .focus(&self.modes_focus)
            .options(MODES.map(|(_, label)| label))
            .selected(MODES.iter().position(|(m, _)| *m == options.mode).unwrap_or(0))
            .on_change(cx.listener(move |this, index: &usize, _, cx| {
                this.set_options(Options { mode: MODES[*index].0, ..options }, cx)
            }));
        // Formatting buttons at the right of the result's title.
        let indent = IconButton::new("json-indent", IconName::IndentIncrease)
            .label(options.indent.label())
            .tooltip(format!("Indent: {} spaces", options.indent.label()))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.set_options(Options { indent: options.indent.toggled(), ..options }, cx)
            }));
        let sort_keys = IconButton::new("json-sort-keys", IconName::ArrowDownAZ)
            .selected(options.sort_keys)
            .tooltip(if options.sort_keys { "Keys sorted A→Z" } else { "Sort keys A→Z" })
            .on_click(cx.listener(move |this, _, _, cx| {
                this.set_options(Options { sort_keys: !options.sort_keys, ..options }, cx)
            }));
        let mut accessory = h_flex().gap(px(2.));
        if matches!(options.mode, Mode::Format | Mode::Unescape) {
            accessory = accessory.child(indent);
        }
        if options.mode == Mode::Format {
            accessory = accessory.child(sort_keys);
        }
        v_flex()
            .id("json")
            .flex_1()
            .overflow_y_scroll()
            .gap(px(14.))
            .child(h_flex().child(modes))
            .child(self.output.render(Some(accessory.into_any_element()), cx))
    }
}
