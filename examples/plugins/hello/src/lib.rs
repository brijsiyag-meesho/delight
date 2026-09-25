//! A minimal Delight plugin: a custom GPUI view that greets the input and
//! counts clicks. Build with `cargo build -p delight-plugin-hello` and copy
//! the dylib into Delight's plugins folder (`scripts/install-plugin.sh`).

use delight_sdk::{Action, DetectRule, OperationSpec, Plugin, PluginManifest, ToolContext, ToolView};
use gpui::{AnyView, App, AppContext, Context, Entity, Window, div, prelude::*, px};

pub struct HelloPlugin {
    manifest: PluginManifest,
}

impl HelloPlugin {
    pub fn new() -> Self {
        Self {
            manifest: PluginManifest::new("example.hello", "Hello")
                .version(env!("CARGO_PKG_VERSION"))
                .description("Example plugin with a custom GPUI view.")
                .icon("👋")
                .accent("#34C759")
                .tags(["example"])
                .operations([OperationSpec::new("greet", "Say hello")
                    .description("Greets the input")
                    .detect([DetectRule::prefix("hello", 0.9)])]),
        }
    }
}

impl Default for HelloPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl Plugin for HelloPlugin {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    fn tool_view(&self, _operation_id: &str, cx: &mut App) -> Option<Box<dyn ToolView>> {
        Some(Box::new(Greeter(cx.new(|_| GreeterView { name: String::new(), clicks: 0 }))))
    }
}

struct GreeterView {
    name: String,
    clicks: usize,
}

impl Render for GreeterView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(div().text_size(px(20.)).child(format!("Hello, {}!", self.name)))
            .child(
                div()
                    .id("count")
                    .px(px(10.))
                    .py(px(4.))
                    .rounded(px(6.))
                    .bg(gpui::hsla(0.33, 0.6, 0.45, 1.))
                    .text_color(gpui::white())
                    .cursor_pointer()
                    .child(format!("Clicked {} times", self.clicks))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.clicks += 1;
                        cx.notify();
                    })),
            )
    }
}

struct Greeter(Entity<GreeterView>);

impl ToolView for Greeter {
    fn view(&self) -> AnyView {
        self.0.clone().into()
    }

    fn update(&self, context: &ToolContext, cx: &mut App) {
        let name = context.input.trim().trim_start_matches("hello").trim().to_string();
        self.0.update(cx, |v, cx| {
            v.name = if name.is_empty() { "world".into() } else { name };
            cx.notify();
        });
    }

    fn actions(&self, cx: &App) -> Vec<Action> {
        let text = format!("Hello, {}!", self.0.read(cx).name);
        vec![Action::copy("copy", "Copy greeting", text).primary()]
    }
}

delight_sdk::export_plugin!(HelloPlugin::new);
