//! YAML ⇄ JSON. Paste YAML to get JSON, paste JSON to get YAML.
//!
//! Parsing and emitting is `yaml-rust2` (YAML 1.2, pure Rust); the tree is
//! mapped to/from `serde_json::Value` here so edge cases are explicit:
//! scalar keys become strings, `.inf`/`.nan` (not representable in JSON)
//! become strings, and several documents (`---`) become a JSON array.

use delight_sdk::{
    Action, Block, Detection, Input, NoticeLevel, Plugin, PluginError, PluginManifest, RunRequest, ToolOutput,
};
use serde_json::{Map, Number, Value};
use yaml_rust2::{Yaml, YamlEmitter, YamlLoader, yaml::Hash};

use super::json::pretty;
use super::{manifest, op};

/// Lines looked at by the cheap shape check in `detect`.
const DETECT_LINES: usize = 400;
/// Inputs up to this size are fully parsed to confirm a detection; larger ones
/// rely on the shape check (and are parsed on run).
const DETECT_PARSE_LIMIT: usize = 256 * 1024;

pub struct YamlPlugin {
    manifest: PluginManifest,
}

impl YamlPlugin {
    pub fn new() -> Self {
        Self {
            manifest: manifest(
                "delight.yaml",
                "YAML",
                "Convert YAML to JSON and JSON to YAML.",
                "YML",
                "#FF2D55",
                &["yaml", "yml", "json", "convert"],
                vec![
                    op("yaml_to_json", "YAML → JSON", "Convert YAML to JSON", &["yaml", "json", "convert"], vec![]),
                    op("json_to_yaml", "JSON → YAML", "Convert JSON to YAML", &["json", "yaml", "convert"], vec![]),
                ],
            ),
        }
    }
}

impl Default for YamlPlugin {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Detection
// ---------------------------------------------------------------------------

/// Cheap line-shape check: how many of the first lines are YAML structure
/// (`key: value`, `key:`, `- item`, `---`), out of the meaningful ones.
fn yaml_shape(text: &str) -> (usize, usize) {
    let (mut structured, mut total) = (0, 0);
    for line in text.lines().take(DETECT_LINES) {
        let l = line.trim();
        if l.is_empty() || l.starts_with('#') {
            continue;
        }
        total += 1;
        let is_key = l.split_once(':').is_some_and(|(k, rest)| {
            let k = k.trim_matches(|c| c == '"' || c == '\'');
            !k.is_empty()
                && !k.contains(char::is_whitespace)
                && (rest.is_empty() || rest.starts_with(' '))
                && !k.contains("//")
        });
        let is_item = l == "-" || l.starts_with("- ");
        // Continuation of a block scalar / flow value under an indented key.
        let is_nested = line.starts_with(' ') && total > 1;
        if is_key || is_item || l == "---" || is_nested {
            structured += 1;
        }
    }
    (structured, total)
}

/// The first document, if `text` parses as YAML.
fn load(text: &str) -> Result<Vec<Yaml>, String> {
    YamlLoader::load_from_str(text).map_err(|e| {
        let m = e.marker();
        format!("line {}, column {}: {}", m.line(), m.col() + 1, e.info())
    })
}

// ---------------------------------------------------------------------------
// Conversion
// ---------------------------------------------------------------------------

fn scalar_key(y: &Yaml) -> Result<String, String> {
    Ok(match y {
        Yaml::String(s) | Yaml::Real(s) => s.clone(),
        Yaml::Integer(i) => i.to_string(),
        Yaml::Boolean(b) => b.to_string(),
        Yaml::Null => "null".into(),
        _ => return Err("a mapping key that isn't a plain value (JSON keys must be strings)".into()),
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
    Ok(out.strip_prefix("---\n").or_else(|| out.strip_prefix("---")).unwrap_or(&out).trim_start().to_string() + "\n")
}

fn converted(label: &str, language: &str, text: String, copy: &str) -> ToolOutput {
    ToolOutput::default()
        .block(Block::code(label, language, text.clone()))
        .action(Action::copy("copy", copy, text.clone()).primary())
        .action(Action::replace_input("replace", "Replace input", text))
}

impl Plugin for YamlPlugin {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    fn detect(&self, input: &Input) -> Vec<Detection> {
        let t = input.trimmed;
        // JSON (parsed once, shared) is also valid YAML: offer the JSON → YAML direction.
        if let Some(v) = input.json().filter(|v| v.is_object() || v.is_array()) {
            let what = if v.is_object() { "object" } else { "array" };
            return vec![Detection::new("json_to_yaml", 0.5).reason(format!("JSON {what}"))];
        }
        if t.is_empty() || input.json_error().is_some() {
            return Vec::new();
        }
        // Cheap shape check first: plain text is valid YAML too (a scalar).
        let (structured, total) = yaml_shape(t);
        if structured == 0 || (structured as f32) < 0.8 * total as f32 {
            return Vec::new();
        }
        // One `key: value` line could be prose ("Note: …").
        let confidence = if structured >= 2 { 0.85 } else { 0.35 };
        if t.len() <= DETECT_PARSE_LIMIT {
            let Ok(docs) = load(t) else { return Vec::new() };
            let Some(first) = docs.first().filter(|d| matches!(d, Yaml::Hash(_) | Yaml::Array(_))) else {
                return Vec::new();
            };
            let what = match first {
                Yaml::Hash(h) => format!("{} top-level keys", h.len()),
                Yaml::Array(a) => format!("list of {}", a.len()),
                _ => String::new(),
            };
            let docs_note = if docs.len() > 1 { format!(" · {} documents", docs.len()) } else { String::new() };
            return vec![Detection::new("yaml_to_json", confidence).reason("YAML").preview(format!("{what}{docs_note}"))];
        }
        vec![Detection::new("yaml_to_json", confidence * 0.9).reason("looks like YAML")]
    }

    fn run(&self, req: &RunRequest) -> Result<ToolOutput, PluginError> {
        let text = req.input.trim();
        match req.operation_id.as_str() {
            "yaml_to_json" => {
                let docs = match load(text) {
                    Ok(docs) => docs,
                    Err(e) => return Ok(ToolOutput::notice(NoticeLevel::Error, format!("Invalid YAML — {e}"))),
                };
                let values = match docs.iter().map(to_json).collect::<Result<Vec<_>, _>>() {
                    Ok(v) => v,
                    Err(e) => return Ok(ToolOutput::notice(NoticeLevel::Error, format!("Can't convert to JSON: {e}"))),
                };
                let (value, note) = match values.len() {
                    0 => (Value::Null, None),
                    1 => (values.into_iter().next().expect("one"), None),
                    n => (Value::Array(values), Some(format!("{n} YAML documents → a JSON array"))),
                };
                let out = converted("JSON", "json", pretty(&value, "2"), "Copy JSON");
                Ok(match note {
                    Some(n) => {
                        let mut out = out;
                        out.blocks.insert(0, Block::notice(NoticeLevel::Info, n));
                        out
                    }
                    None => out,
                })
            }
            "json_to_yaml" => {
                let value: Value = serde_json::from_str(text).map_err(|e| PluginError::Invalid(format!("not JSON: {e}")))?;
                let yaml = emit(&to_yaml(&value)).map_err(PluginError::Failed)?;
                Ok(converted("YAML", "yaml", yaml, "Copy YAML"))
            }
            other => Err(PluginError::UnknownOperation(other.into())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn detect(s: &str) -> Vec<(String, f32)> {
        YamlPlugin::new().detect(&Input::new(s)).into_iter().map(|d| (d.operation_id, d.confidence)).collect()
    }

    fn run(op: &str, input: &str) -> ToolOutput {
        YamlPlugin::new()
            .run(&RunRequest::new(op, input))
            .unwrap()
    }

    fn code(out: &ToolOutput) -> String {
        out.blocks.iter().find_map(|b| match b {
            Block::Code { text, .. } => Some(text.clone()),
            _ => None,
        })
        .unwrap()
    }

    const K8S: &str = "apiVersion: v1\nkind: Service\nmetadata:\n  name: api\n  labels:\n    app: api\nspec:\n  ports:\n    - port: 80\n      targetPort: 8080\n";

    #[test]
    fn detects_yaml_but_not_prose_json_or_env() {
        assert_eq!(detect(K8S)[0].0, "yaml_to_json");
        assert!(detect(K8S)[0].1 > 0.8);
        assert_eq!(detect(r#"{"a": 1}"#), [("json_to_yaml".to_string(), 0.5)]);
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
        let json = code(&run("yaml_to_json", K8S));
        let v: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["spec"]["ports"][0]["targetPort"], 8080);
        assert_eq!(v["metadata"]["labels"]["app"], "api");
        // Key order is kept.
        assert!(json.find("apiVersion").unwrap() < json.find("spec").unwrap());

        let yaml = code(&run("json_to_yaml", &json));
        let back = to_json(&load(&yaml).unwrap()[0]).unwrap();
        assert_eq!(back, v);
        assert!(!yaml.starts_with("---"));
    }

    #[test]
    fn edge_cases() {
        // Several documents → array, with a note.
        let out = run("yaml_to_json", "a: 1\n---\nb: 2\n");
        assert!(matches!(&out.blocks[0], Block::Notice { text, .. } if text.contains("2 YAML documents")));
        assert_eq!(serde_json::from_str::<Value>(&code(&out)).unwrap(), serde_json::json!([{"a": 1}, {"b": 2}]));
        // Non-string keys, specials, anchors.
        let v: Value = serde_json::from_str(&code(&run("yaml_to_json", "1: one\ntrue: yes\ninf: .inf\nbase: &b {x: 1}\nuse: *b\n"))).unwrap();
        assert_eq!(v["1"], "one");
        assert_eq!(v["true"], "yes");
        assert_eq!(v["inf"], ".inf");
        assert_eq!(v["use"]["x"], 1);
        // Errors point at the line.
        assert!(matches!(&run("yaml_to_json", "a: [1, 2\nb: 3").blocks[0], Block::Notice { level: NoticeLevel::Error, text } if text.contains("line")));
        // Strings that look like other types stay strings in YAML.
        let yaml = code(&run("json_to_yaml", r#"{"s": "true", "n": "123", "e": ""}"#));
        let back = to_json(&load(&yaml).unwrap()[0]).unwrap();
        assert_eq!(back, serde_json::json!({"s": "true", "n": "123", "e": ""}));
    }
}
