//! JSON: format, minify, escape and unescape.
//!
//! * this file — the plugin: its manifest and detection.
//! * `view` — the tool's view: mode tabs, formatting buttons, the result.
//! * `convert` — the conversions themselves.
//! * `output` — a conversion's result, and how it's drawn.

mod convert;
mod output;
mod view;

use delight_sdk::{Detection, Input, OperationSpec, Plugin, PluginManifest, ToolView};
use gpui::{App, AppContext};
use serde_json::Value;

/// Larger inputs are detected by their first character only (parsing them
/// on every keystroke would be slow); the view still parses them.
const DETECT_PARSE_LIMIT: usize = 256 * 1024;

pub struct JsonPlugin {
    manifest: PluginManifest,
}

impl JsonPlugin {
    pub fn new() -> Self {
        let operation = OperationSpec::new("json", "JSON")
            .description("Format, minify, escape or unescape JSON")
            .tags(["json", "format", "pretty", "minify", "escape", "unescape", "validate"]);
        let manifest = crate::manifest("delight.json", "JSON", include_bytes!("icon.svg"))
            .description("Format, minify, escape and unescape JSON.")
            .tags(["json", "format", "pretty print"])
            .operations([operation]);
        Self { manifest }
    }
}

impl Plugin for JsonPlugin {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    fn detect(&self, input: &Input) -> Vec<Detection> {
        let text = input.text.trim();
        let starts_container = matches!(text.as_bytes().first(), Some(b'{' | b'['));
        if !starts_container && !text.starts_with('"') {
            return Vec::new();
        }
        if text.len() > DETECT_PARSE_LIMIT {
            return if starts_container { vec![Detection::new("json", 0.8)] } else { Vec::new() };
        }
        let confidence = match serde_json::from_str::<Value>(text) {
            Ok(Value::Object(_) | Value::Array(_)) => 0.92,
            // A JSON string holding JSON (often copied from logs).
            Ok(Value::String(s)) if convert::is_container(&s) => 0.9,
            Ok(_) => return Vec::new(),
            // Looks like JSON but doesn't parse: show where it breaks.
            Err(_) if starts_container => 0.6,
            Err(_) => return Vec::new(),
        };
        vec![Detection::new("json", confidence)]
    }

    fn tool_view(&self, _operation_id: &str, cx: &mut App) -> Box<dyn ToolView> {
        Box::new(view::JsonTool(cx.new(view::JsonView::new)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_json_and_escaped_json() {
        let plugin = JsonPlugin::new();
        let confidence = |text: &str| plugin.detect(&Input::new(text.to_string())).first().map(|d| d.confidence);
        assert_eq!(confidence(r#"{"a": 1}"#), Some(0.92));
        assert_eq!(confidence(r#""{\"a\":1}""#), Some(0.9));
        assert_eq!(confidence("{\"a\": "), Some(0.6));
        assert_eq!(confidence("hello"), None);
        assert_eq!(confidence(r#""just a string""#), None);
    }
}
