//! YAML ⇄ JSON: paste YAML to get JSON, paste JSON to get YAML.
//!
//! * this file — the plugin: its manifest and detection.
//! * `view` — the tool's view: the converted text.
//! * `convert` — the conversions, and the parsing detection uses.
//! * `output` — a conversion's result, and how it's drawn.

mod convert;
mod output;
mod view;

use delight_sdk::{Detection, Input, OperationSpec, Plugin, PluginManifest, ToolView};
use gpui::{App, AppContext};
use serde_json::Value;
use yaml_rust2::Yaml;

/// Lines the cheap shape check in `detect` looks at.
const DETECT_LINES: usize = 400;
/// Inputs up to this size are parsed to confirm a detection; larger ones
/// rely on the shape check (the view parses them).
const DETECT_PARSE_LIMIT: usize = 256 * 1024;

const YAML_TO_JSON: &str = "yaml_to_json";
const JSON_TO_YAML: &str = "json_to_yaml";

pub struct YamlPlugin {
    manifest: PluginManifest,
}

impl YamlPlugin {
    pub fn new() -> Self {
        let operations = [
            OperationSpec::new(YAML_TO_JSON, "YAML → JSON").description("Convert YAML to JSON").tags(["yaml", "json", "convert"]),
            OperationSpec::new(JSON_TO_YAML, "JSON → YAML").description("Convert JSON to YAML").tags(["json", "yaml", "convert"]),
        ];
        let manifest = crate::manifest("delight.yaml", "YAML", include_bytes!("icon.svg"))
            .description("Convert YAML to JSON and JSON to YAML.")
            .tags(["yaml", "yml", "json", "convert"])
            .operations(operations);
        Self { manifest }
    }
}

impl Plugin for YamlPlugin {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    fn detect(&self, input: &Input) -> Vec<Detection> {
        let text = input.text.trim();
        if text.is_empty() {
            return Vec::new();
        }
        // JSON is also valid YAML: offer the JSON → YAML direction.
        if matches!(text.as_bytes().first(), Some(b'{' | b'[')) {
            let is_json = text.len() > DETECT_PARSE_LIMIT
                || matches!(serde_json::from_str::<Value>(text), Ok(Value::Object(_) | Value::Array(_)));
            return if is_json { vec![Detection::new(JSON_TO_YAML, 0.5)] } else { Vec::new() };
        }
        // Cheap shape check first: plain text is valid YAML too (a scalar).
        let (structured, total) = yaml_shape(text);
        if structured == 0 || (structured as f32) < 0.8 * total as f32 {
            return Vec::new();
        }
        // One `key: value` line could be prose ("Note: call me later"):
        // recommended only when its value is one word or quoted.
        let confidence = match structured {
            2.. => 0.85,
            _ if single_value(text) => 0.6,
            _ => 0.35,
        };
        if text.len() > DETECT_PARSE_LIMIT {
            return vec![Detection::new(YAML_TO_JSON, confidence * 0.9)];
        }
        match convert::load(text) {
            Ok(docs) if matches!(docs.first(), Some(Yaml::Hash(_) | Yaml::Array(_))) => {
                vec![Detection::new(YAML_TO_JSON, confidence)]
            }
            _ => Vec::new(),
        }
    }

    fn tool_view(&self, operation_id: &str, cx: &mut App) -> Box<dyn ToolView> {
        let to_json = operation_id == YAML_TO_JSON;
        Box::new(view::YamlTool(cx.new(|_| view::YamlView::new(to_json))))
    }
}

/// Whether a one-line `key: value`'s value is a single word (`api`, `8080`,
/// `true`) or quoted, not a sentence.
fn single_value(line: &str) -> bool {
    let value = line.split_once(':').map_or("", |(_, value)| value.trim());
    let quoted = value.len() >= 2 && [('"', '"'), ('\'', '\''), ('[', ']'), ('{', '}')].iter().any(|&(open, close)| {
        value.starts_with(open) && value.ends_with(close)
    });
    quoted || !value.contains(char::is_whitespace)
}

/// How many of the first lines are YAML structure (`key: value`, `key:`,
/// `- item`, `---`), out of the meaningful ones.
fn yaml_shape(text: &str) -> (usize, usize) {
    let (mut structured, mut total) = (0, 0);
    for line in text.lines().take(DETECT_LINES) {
        let l = line.trim();
        if l.is_empty() || l.starts_with('#') {
            continue;
        }
        total += 1;
        let is_key = l.split_once(':').is_some_and(|(key, rest)| {
            let key = key.trim_matches(|c| c == '"' || c == '\'');
            !key.is_empty()
                && !key.contains(char::is_whitespace)
                && (rest.is_empty() || rest.starts_with(' '))
                && !key.contains("//")
        });
        let is_item = l == "-" || l.starts_with("- ");
        // Continues a block scalar or a nested value.
        let is_nested = line.starts_with(' ') && total > 1;
        if is_key || is_item || l == "---" || is_nested {
            structured += 1;
        }
    }
    (structured, total)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn detect(s: &str) -> Vec<(String, f32)> {
        YamlPlugin::new().detect(&Input::new(s.to_string())).into_iter().map(|d| (d.operation_id, d.confidence)).collect()
    }

    const K8S: &str = "apiVersion: v1\nkind: Service\nmetadata:\n  name: api\n";

    #[test]
    fn detects_yaml_but_not_prose_json_or_env() {
        assert_eq!(detect(K8S)[0].0, YAML_TO_JSON);
        assert!(detect(K8S)[0].1 > 0.8);
        assert_eq!(detect(r#"{"a": 1}"#), [(JSON_TO_YAML.to_string(), 0.5)]);
        assert!(detect("hello there").is_empty());
        assert!(detect("DB_HOST=localhost\nDB_PORT=5432").is_empty());
        assert!(detect("Note: this is a sentence, not config.\nIt goes on for a while here.").is_empty());
        assert!(detect("curl https://x/y").is_empty());
        assert!(detect("{\"broken\": ").is_empty());
        // A single key line: recommended when its value is a word or
        // quoted; offered, but low, when it reads like a sentence.
        assert!(detect("name: api")[0].1 >= 0.5);
        assert!(detect("key: Val")[0].1 >= 0.5);
        assert!(detect("title: \"Hello there\"")[0].1 >= 0.5);
        assert!(detect("Note: call me later")[0].1 < 0.5);
    }
}
