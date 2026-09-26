//! A minimal Delight plugin: type `hello <name>` and its view greets you, with
//! a button that counts clicks and an action that copies the greeting.
//!
//! A plugin is a `dylib` crate that implements [`Plugin`] and ends with
//! `export_plugin!`. Its view draws everything itself with GPUI, in the
//! colours and fonts of the host's theme (`host(cx).theme(window, cx)`).

use delight_sdk::{
    Action, Detection, Input, OperationSpec, Plugin, PluginManifest, ToolContext, ToolView, export_plugin, host,
};
use gpui::{AnyView, App, AppContext, ClipboardItem, Context, Entity, SharedString, Window, div, prelude::*, px};

export_plugin!(HelloPlugin::new);

pub struct HelloPlugin {
    manifest: PluginManifest,
}

impl HelloPlugin {
    pub fn new() -> Self {
        let greet = OperationSpec::new("greet", "Say hello").description("Greets the name after `hello`");
        let manifest = PluginManifest::new("example.hello", "Hello", include_bytes!("icon.svg"))
            .version(env!("CARGO_PKG_VERSION"))
            .description("An example plugin with its own GPUI view.")
            .tags(["example"])
            .operations([greet]);
        Self { manifest }
    }
}

impl Plugin for HelloPlugin {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    /// Cheap: a prefix check. The view works out the name itself.
    fn detect(&self, input: &Input) -> Vec<Detection> {
        if input.text.trim_start().to_lowercase().starts_with("hello") {
            vec![Detection::new("greet", 0.9)]
        } else {
            Vec::new()
        }
    }

    fn tool_view(&self, _operation_id: &str, cx: &mut App) -> Box<dyn ToolView> {
        Box::new(Greeter(cx.new(|_| GreeterView::default())))
    }
}

#[derive(Default)]
struct GreeterView {
    name: SharedString,
    clicks: usize,
}

impl GreeterView {
    fn greeting(&self) -> String {
        format!("Hello, {}!", if self.name.is_empty() { "you" } else { &self.name })
    }
}

impl Render for GreeterView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = host(cx).theme(window, cx);
        let button = div()
            .id("count")
            .px(px(10.))
            .py(px(4.))
            .rounded(theme.metrics.radius_sm)
            .bg(theme.colors.accent)
            .text_color(theme.colors.accent_text)
            .cursor_pointer()
            .child(format!("Clicked {} times", self.clicks))
            .on_click(cx.listener(|this, _, _, cx| {
                this.clicks += 1;
                cx.notify();
            }));
        // The view fills the pane and scrolls itself.
        div()
            .id("greeter")
            .flex_1()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .items_start()
            .gap(px(10.))
            .child(div().text_size(px(20.)).text_color(theme.colors.label).child(self.greeting()))
            .child(button)
    }
}

/// What the host holds: the view, fed the input.
struct Greeter(Entity<GreeterView>);

impl ToolView for Greeter {
    fn view(&self) -> AnyView {
        self.0.clone().into()
    }

    fn update(&self, context: &ToolContext, cx: &mut App) {
        let text = context.input.text.trim();
        let name = text.get("hello".len()..).unwrap_or_default().trim().to_string();
        self.0.update(cx, |view, cx| {
            view.name = name.into();
            cx.notify();
        });
    }

    fn actions(&self, _cx: &App) -> Vec<Action> {
        vec![Action::new("copy", "Copy greeting").shortcut("enter")]
    }

    /// Runs an action: copies the greeting, says so, and hides Delight.
    fn perform(&self, action_id: &str, cx: &mut App) {
        if action_id == "copy" {
            cx.write_to_clipboard(ClipboardItem::new_string(self.0.read(cx).greeting()));
            host(cx).toast("Greeting copied".into(), cx);
            host(cx).hide(cx);
        }
    }
}
