//! Test fixture: panics on purpose, so the host's panic guards can be checked
//! end to end — across the dylib boundary, as with any installed plugin.
//!
//! Type one of these into the launcher (after `scripts/install-plugin.sh
//! delight-plugin-panicky`):
//!
//! * `panic detect` — `detect` panics (background): caught, the plugin is
//!   turned off for this run, Delight keeps going.
//! * `panic run` — `run` panics (background): caught, shown as an error.
//! * `panic render` — the custom view's `render` panics (main thread):
//!   caught, the plugin is turned off and Delight relaunches.
//! * `panic update` — `ToolView::update` panics (main thread): same.

use delight_sdk::{
    DetectRule, Detection, Input, OperationSpec, Plugin, PluginError, PluginManifest, RunRequest, ToolContext, ToolOutput,
    ToolView,
};
use gpui::{AnyView, App, AppContext, Context, Entity, Window, div, prelude::*};

pub struct PanickyPlugin {
    manifest: PluginManifest,
}

impl PanickyPlugin {
    pub fn new() -> Self {
        Self {
            manifest: PluginManifest::new("test.panicky", "Panicky")
                .version(env!("CARGO_PKG_VERSION"))
                .description("Panics on purpose (test fixture).")
                .icon("💥")
                .operations([
                    OperationSpec::new("declarative", "Panic in run").detect([DetectRule::prefix("panic run", 0.95)]),
                    OperationSpec::new("view", "Panic in a view")
                        .detect([DetectRule::prefix("panic render", 0.95), DetectRule::prefix("panic update", 0.95)]),
                ]),
        }
    }
}

impl Default for PanickyPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl Plugin for PanickyPlugin {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    fn detect(&self, input: &Input) -> Vec<Detection> {
        if input.trimmed == "panic detect" {
            let empty: Vec<u8> = Vec::new();
            let _ = empty[input.trimmed.len()]; // index out of bounds
        }
        delight_sdk::rules::evaluate(&self.manifest, input)
    }

    fn run(&self, _: &RunRequest) -> Result<ToolOutput, PluginError> {
        panic!("panicky: run exploded");
    }

    fn tool_view(&self, operation_id: &str, cx: &mut App) -> Option<Box<dyn ToolView>> {
        (operation_id == "view").then(|| Box::new(Panicking(cx.new(|_| PanickingView { input: String::new() }))) as Box<dyn ToolView>)
    }
}

struct PanickingView {
    input: String,
}

impl Render for PanickingView {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        if self.input.trim() == "panic render" {
            panic!("panicky: render exploded");
        }
        div().child(format!("Waiting to panic: {}", self.input))
    }
}

struct Panicking(Entity<PanickingView>);

impl ToolView for Panicking {
    fn view(&self) -> AnyView {
        self.0.clone().into()
    }

    fn update(&self, context: &ToolContext, cx: &mut App) {
        self.0.update(cx, |view, cx| {
            if context.input.trim() == "panic update" {
                panic!("panicky: update exploded");
            }
            view.input = context.input.clone();
            cx.notify();
        });
    }
}

delight_sdk::export_plugin!(PanickyPlugin::new);
