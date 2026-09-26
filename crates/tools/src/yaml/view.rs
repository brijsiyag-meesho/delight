//! The YAML tool's view: the converted text.

use delight_sdk::{Action, ToolContext, ToolView, host};
use delight_ui::v_flex;
use gpui::{
    AnyView, App, ClipboardItem, Context, Entity, InteractiveElement, IntoElement, ParentElement, Render,
    SharedString, StatefulInteractiveElement, Styled, Task, Window, px,
};

use super::convert::{json_to_yaml, yaml_to_json};
use super::output::Output;

pub struct YamlView {
    /// YAML → JSON, or JSON → YAML.
    to_json: bool,
    output: Output,
    /// Replacing it cancels the conversion in progress.
    _task: Option<Task<()>>,
}

impl YamlView {
    pub fn new(to_json: bool) -> Self {
        Self { to_json, output: Output::default(), _task: None }
    }

    fn copy_label(&self) -> &'static str {
        if self.to_json { "Copy JSON" } else { "Copy YAML" }
    }
}

impl YamlView {
    /// Converts `input` in the background, then shows the result.
    fn convert(&mut self, input: SharedString, cx: &mut Context<Self>) {
        let to_json = self.to_json;
        self._task = Some(cx.spawn(async move |this, cx| {
            let convert = async move {
                let text = input.trim();
                let conversion = if to_json { yaml_to_json(text) } else { json_to_yaml(text) };
                Output::new(conversion, text)
            };
            let output = cx.background_executor().spawn(convert).await;
            let _ = this.update(cx, |this, cx| {
                this.output = output;
                cx.notify();
            });
        }));
    }

}

/// The view, as the host sees it.
pub struct YamlTool(pub Entity<YamlView>);

impl ToolView for YamlTool {
    fn view(&self) -> AnyView {
        self.0.clone().into()
    }

    fn update(&self, context: &ToolContext, cx: &mut App) {
        let input = context.input.text.clone();
        self.0.update(cx, |view, cx| view.convert(input, cx));
    }

    fn actions(&self, cx: &App) -> Vec<Action> {
        let view = self.0.read(cx);
        view.output.actions(view.copy_label())
    }

    fn perform(&self, action_id: &str, cx: &mut App) {
        let view = self.0.read(cx);
        let Some(text) = view.output.text().filter(|_| action_id == "copy") else { return };
        let label = view.copy_label();
        cx.write_to_clipboard(ClipboardItem::new_string(text.to_string()));
        host(cx).toast(format!("{label} — copied to clipboard").into(), cx);
    }
}

impl Render for YamlView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex().id("yaml").flex_1().overflow_y_scroll().gap(px(14.)).child(self.output.render(None, cx))
    }
}
