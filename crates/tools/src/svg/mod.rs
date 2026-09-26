//! SVG: shows an SVG document, and copies it as a PNG or a data URI.
//!
//! * this file — the plugin: its manifest and detection.
//! * `view` — the tool's view: the preview and its actions.
//! * `render` — the preview and the PNG from the input's SVG.
//! * `document` — an SVG document rendered at any scale.
//! * `checkerboard` — the squares behind the preview.

mod checkerboard;
mod document;
mod render;
mod view;

use delight_sdk::{Detection, Input, OperationSpec, Plugin, PluginManifest, ToolView};
use gpui::{App, AppContext};

/// How much of the input `detect` searches for `<svg`.
const DETECT_HEAD: usize = 1024;

pub struct SvgPlugin {
    manifest: PluginManifest,
}

impl SvgPlugin {
    pub fn new() -> Self {
        let operation = OperationSpec::new("svg", "SVG Preview")
            .description("Show an SVG image; copy it as PNG or a data URI")
            .tags(["svg", "image", "preview", "png", "vector"]);
        let manifest = crate::manifest("delight.svg", "SVG", include_bytes!("icon.svg"))
            .description("Preview SVG images and copy them as PNG or a data URI.")
            .tags(["svg", "image", "preview"])
            .operations([operation]);
        Self { manifest }
    }
}

impl Plugin for SvgPlugin {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    fn detect(&self, input: &Input) -> Vec<Detection> {
        let text = input.text.trim_start();
        let confidence = if text.starts_with("<svg") {
            0.95
        } else if text.starts_with('<') && text.chars().take(DETECT_HEAD).collect::<String>().contains("<svg") {
            // An XML declaration, a doctype or a comment before `<svg`.
            0.9
        } else {
            return Vec::new();
        };
        vec![Detection::new("svg", confidence)]
    }

    fn tool_view(&self, _operation_id: &str, cx: &mut App) -> Box<dyn ToolView> {
        Box::new(view::SvgTool(cx.new(|_| view::SvgView::default())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_svg() {
        let plugin = SvgPlugin::new();
        let confidence = |text: &str| plugin.detect(&Input::new(text.to_string())).first().map(|d| d.confidence);
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="1" height="1"/>"#;
        assert_eq!(confidence(svg), Some(0.95));
        assert_eq!(confidence(&format!("<?xml version=\"1.0\"?>\n{svg}")), Some(0.9));
        assert_eq!(confidence("<div>not svg</div>"), None);
        assert_eq!(confidence("hello"), None);
    }
}
