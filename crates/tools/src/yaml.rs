//! YAML ⇄ JSON: paste YAML to get JSON, paste JSON to get YAML.
//!
//! `yaml-rust2` parses and writes YAML (1.2, pure Rust); the tree is mapped
//! to and from `serde_json::Value` here, so the edge cases are explicit:
//! scalar keys become strings, `.inf`/`.nan` (no JSON number for them)
//! become strings, and several documents (`---`) become a JSON array.

use delight_sdk::{Action, Detection, Input, OperationSpec, Plugin, PluginManifest, ToolView};
use delight_ui::{Language, v_flex};
use gpui::{App, AppContext, Context, IntoElement, ParentElement, Render, SharedString, Styled, Task, Window, px};
use serde_json::{Map, Number, Value};
use yaml_rust2::yaml::Hash;
use yaml_rust2::{Yaml, YamlEmitter, YamlLoader};

use crate::json::{Indent, pretty};
use crate::output::{Converter, ConverterView, Output};

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
        let manifest = crate::manifest("delight.yaml", "YAML", include_bytes!("../assets/yaml.svg"))
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
        // One `key: value` line could be prose ("Note: …").
        let confidence = if structured >= 2 { 0.85 } else { 0.35 };
        if text.len() > DETECT_PARSE_LIMIT {
            return vec![Detection::new(YAML_TO_JSON, confidence * 0.9)];
        }
        match load(text) {
            Ok(docs) if matches!(docs.first(), Some(Yaml::Hash(_) | Yaml::Array(_))) => {
                vec![Detection::new(YAML_TO_JSON, confidence)]
            }
            _ => Vec::new(),
        }
    }

    fn tool_view(&self, operation_id: &str, cx: &mut App) -> Box<dyn ToolView> {
        let to_json = operation_id == YAML_TO_JSON;
        Box::new(ConverterView(cx.new(|_| YamlView { to_json, ..YamlView::default() })))
    }
}

#[derive(Default)]
struct YamlView {
    /// YAML → JSON, or JSON → YAML.
    to_json: bool,
    input: SharedString,
    output: Output,
    /// Replacing it cancels the conversion in progress.
    _task: Option<Task<()>>,
}

impl Converter for YamlView {
    fn set_input(&mut self, input: SharedString, cx: &mut Context<Self>) {
        self.input = input.clone();
        let to_json = self.to_json;
        self._task = Some(cx.spawn(async move |this, cx| {
            let convert = async move { if to_json { yaml_to_json(input.trim()) } else { json_to_yaml(input.trim()) } };
            let output = cx.background_executor().spawn(convert).await;
            let _ = this.update(cx, |this, cx| {
                this.output = output;
                cx.notify();
            });
        }));
    }

    fn actions(&self) -> Vec<Action> {
        self.output.actions(if self.to_json { "Copy JSON" } else { "Copy YAML" })
    }
}

impl Render for YamlView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex().gap(px(14.)).child(self.output.render(None, cx))
    }
}

// ---------------------------------------------------------------------------
// Detection
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Conversion
// ---------------------------------------------------------------------------

/// A parse error with its 1-based line and column.
struct ParseError {
    message: String,
    position: (usize, usize),
}

fn load(text: &str) -> Result<Vec<Yaml>, ParseError> {
    YamlLoader::load_from_str(text).map_err(|e| {
        let marker = e.marker();
        let position = (marker.line(), marker.col() + 1);
        ParseError { message: format!("line {}, column {}: {}", position.0, position.1, e.info()), position }
    })
}

fn yaml_to_json(text: &str) -> Output {
    if text.is_empty() {
        return Output::Empty;
    }
    let docs = match load(text) {
        Ok(docs) => docs,
        Err(e) => return Output::error(format!("Invalid YAML: {}", e.message), text, Some(e.position)),
    };
    let values = match docs.iter().map(to_json).collect::<Result<Vec<_>, _>>() {
        Ok(values) => values,
        Err(e) => return Output::error(format!("Can't convert to JSON: {e}"), text, None),
    };
    let count = values.len();
    let value = match count {
        0 => Value::Null,
        1 => values.into_iter().next().expect("one document"),
        _ => Value::Array(values),
    };
    let output = Output::code("JSON", Language::Json, pretty(&value, Indent::Two));
    if count > 1 { output.with_note(&format!("{count} YAML documents → a JSON array")) } else { output }
}

fn json_to_yaml(text: &str) -> Output {
    if text.is_empty() {
        return Output::Empty;
    }
    let value: Value = match serde_json::from_str(text) {
        Ok(value) => value,
        Err(e) => {
            let message = format!("Invalid JSON: line {}, column {}: {e}", e.line(), e.column());
            return Output::error(message, text, Some((e.line(), e.column())));
        }
    };
    match emit(&to_yaml(&value)) {
        Ok(yaml) => Output::code("YAML", Language::Yaml, yaml),
        Err(e) => Output::error(format!("Can't write YAML: {e}"), text, None),
    }
}

fn scalar_key(y: &Yaml) -> Result<String, String> {
    Ok(match y {
        Yaml::String(s) | Yaml::Real(s) => s.clone(),
        Yaml::Integer(i) => i.to_string(),
        Yaml::Boolean(b) => b.to_string(),
        Yaml::Null => "null".into(),
        _ => return Err("a mapping key that isn't a plain value (JSON keys are strings)".into()),
    })
}

fn to_json(y: &Yaml) -> Result<Value, String> {
    Ok(match y {
        Yaml::Null => Value::Null,
        Yaml::Boolean(b) => Value::Bool(*b),
        Yaml::Integer(i) => Value::from(*i),
        Yaml::Real(s) => match y.as_f64().and_then(Number::from_f64) {
            Some(n) => Value::Number(n),
            // `.inf`, `.nan`: no JSON number for them.
            None => Value::String(s.clone()),
        },
        Yaml::String(s) => Value::String(s.clone()),
        Yaml::Array(items) => Value::Array(items.iter().map(to_json).collect::<Result<_, _>>()?),
        Yaml::Hash(map) => {
            let mut out = Map::new();
            for (k, v) in map {
                out.insert(scalar_key(k)?, to_json(v)?);
            }
            Value::Object(out)
        }
        Yaml::Alias(_) => return Err("an unresolved alias".into()),
        Yaml::BadValue => return Err("an invalid value".into()),
    })
}

fn to_yaml(v: &Value) -> Yaml {
    match v {
        Value::Null => Yaml::Null,
        Value::Bool(b) => Yaml::Boolean(*b),
        Value::Number(n) => match n.as_i64() {
            Some(i) => Yaml::Integer(i),
            None => Yaml::Real(n.to_string()),
        },
        Value::String(s) => Yaml::String(s.clone()),
        Value::Array(items) => Yaml::Array(items.iter().map(to_yaml).collect()),
        Value::Object(map) => {
            let mut out = Hash::new();
            for (k, v) in map {
                out.insert(Yaml::String(k.clone()), to_yaml(v));
            }
            Yaml::Hash(out)
        }
    }
}

fn emit(y: &Yaml) -> Result<String, String> {
    let mut out = String::new();
    let mut emitter = YamlEmitter::new(&mut out);
    emitter.multiline_strings(true);
    emitter.dump(y).map_err(|e| e.to_string())?;
    // The emitter starts every document with `---`.
    let body = out.strip_prefix("---\n").or_else(|| out.strip_prefix("---")).unwrap_or(&out);
    Ok(body.trim_start().to_string() + "\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn detect(s: &str) -> Vec<(String, f32)> {
        YamlPlugin::new().detect(&Input::new(s.to_string())).into_iter().map(|d| (d.operation_id, d.confidence)).collect()
    }

    fn text(output: Output) -> String {
        match output {
            Output::Code { text, .. } => text.to_string(),
            Output::Error { message, .. } => panic!("unexpected error: {message}"),
            Output::Empty => panic!("unexpected empty output"),
        }
    }

    const K8S: &str = "apiVersion: v1\nkind: Service\nmetadata:\n  name: api\n  labels:\n    app: api\nspec:\n  ports:\n    - port: 80\n      targetPort: 8080\n";

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
        // A single key line might be prose: offered, but low.
        assert!(detect("name: api")[0].1 < 0.5);
    }

    #[test]
    fn yaml_to_json_and_back() {
        let json = text(yaml_to_json(K8S));
        let v: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["spec"]["ports"][0]["targetPort"], 8080);
        assert_eq!(v["metadata"]["labels"]["app"], "api");
        // Key order is kept.
        assert!(json.find("apiVersion").unwrap() < json.find("spec").unwrap());

        let yaml = text(json_to_yaml(&json));
        let back = to_json(&load(&yaml).ok().unwrap()[0]).unwrap();
        assert_eq!(back, v);
        assert!(!yaml.starts_with("---"));
    }

    #[test]
    fn edge_cases() {
        // Several documents → an array, with a note.
        let output = yaml_to_json("a: 1\n---\nb: 2\n");
        assert!(matches!(&output, Output::Code { note: Some(note), .. } if note.contains("2 YAML documents")));
        assert_eq!(serde_json::from_str::<Value>(&text(output)).unwrap(), serde_json::json!([{"a": 1}, {"b": 2}]));
        // Non-string keys, specials, anchors.
        let v: Value =
            serde_json::from_str(&text(yaml_to_json("1: one\ntrue: yes\ninf: .inf\nbase: &b {x: 1}\nuse: *b\n"))).unwrap();
        assert_eq!(v["1"], "one");
        assert_eq!(v["true"], "yes");
        assert_eq!(v["inf"], ".inf");
        assert_eq!(v["use"]["x"], 1);
        // Errors point at the line.
        assert!(matches!(yaml_to_json("a: [1, 2\nb: 3"), Output::Error { message, near: Some(_) } if message.contains("line")));
        // Strings that look like other types stay strings in YAML.
        let yaml = text(json_to_yaml(r#"{"s": "true", "n": "123", "e": ""}"#));
        let back = to_json(&load(&yaml).ok().unwrap()[0]).unwrap();
        assert_eq!(back, serde_json::json!({"s": "true", "n": "123", "e": ""}));
    }
}
