use delight_sdk::{
    Action, Block, Detection, Input, NoticeLevel, Plugin, PluginError, PluginManifest, RunRequest,
    ShowWhen, ToolOutput,
};
use serde::Serialize;
use serde_json::Value;

use super::{flag, manifest, op, param, select, toggle};

pub struct JsonPlugin {
    manifest: PluginManifest,
}

impl JsonPlugin {
    pub fn new() -> Self {
        let mode = select(
            "mode",
            "Mode",
            &[("format", "Format"), ("minify", "Minify"), ("escape", "Escape"), ("unescape", "Unescape")],
            "format",
        );
        let indent = select("indent", "Indent", &[("2", "2"), ("4", "4"), ("tab", "Tab")], "2")
            .show_when(ShowWhen::new("mode", &["format", "unescape"]));
        let sort_keys = toggle("sort_keys", "Sort keys", false).show_when(ShowWhen::new("mode", &["format"]));
        Self {
            manifest: manifest(
                "delight.json",
                "JSON",
                "Format, minify, escape and unescape JSON.",
                "{}",
                "#FF9500",
                &["json", "format", "pretty print"],
                vec![op(
                    "json",
                    "JSON",
                    "Format, minify, escape or unescape JSON",
                    &["json", "format", "pretty", "minify", "escape", "unescape", "validate"],
                    vec![mode, indent, sort_keys],
                )
                .mode("mode")],
            ),
        }
    }
}

impl Default for JsonPlugin {
    fn default() -> Self {
        Self::new()
    }
}

fn looks_like_container(t: &str) -> bool {
    (t.starts_with('{') && t.ends_with('}')) || (t.starts_with('[') && t.ends_with(']'))
}

fn describe(v: &Value) -> String {
    match v {
        Value::Object(m) => format!("object · {} keys", m.len()),
        Value::Array(a) => format!("array · {} items", a.len()),
        Value::String(_) => "string".into(),
        Value::Number(_) => "number".into(),
        Value::Bool(_) => "boolean".into(),
        Value::Null => "null".into(),
    }
}

pub(crate) fn sort_keys(v: Value) -> Value {
    match v {
        Value::Object(m) => {
            let mut entries: Vec<(String, Value)> = m.into_iter().collect();
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            Value::Object(entries.into_iter().map(|(k, v)| (k, sort_keys(v))).collect())
        }
        Value::Array(a) => Value::Array(a.into_iter().map(sort_keys).collect()),
        other => other,
    }
}

pub(crate) fn pretty(v: &Value, indent: &str) -> String {
    let indent_bytes: &[u8] = match indent {
        "4" => b"    ",
        "tab" => b"\t",
        _ => b"  ",
    };
    let mut out = Vec::new();
    let fmt = serde_json::ser::PrettyFormatter::with_indent(indent_bytes);
    let mut ser = serde_json::Serializer::with_formatter(&mut out, fmt);
    v.serialize(&mut ser).expect("serializing a Value cannot fail");
    String::from_utf8(out).expect("serde_json emits UTF-8")
}

/// Error notice plus a caret-annotated excerpt of the offending line.
fn parse_error_output(text: &str, err: &serde_json::Error) -> ToolOutput {
    let (line, col) = (err.line(), err.column());
    let mut out = ToolOutput::notice(NoticeLevel::Error, format!("Invalid JSON — line {line}, column {col}: {err}"));
    if line > 0 {
        let lines: Vec<&str> = text.lines().collect();
        let start = line.saturating_sub(3);
        let mut excerpt = String::new();
        for (i, l) in lines.iter().enumerate().skip(start).take(line - start) {
            excerpt.push_str(&format!("{:>5} │ {}\n", i + 1, l));
        }
        excerpt.push_str(&format!("{:>5} │ {}^", "", " ".repeat(col.saturating_sub(1))));
        out = out.block(Block::Code { label: Some("Near".into()), language: Some("text".into()), text: excerpt });
    }
    out
}

impl Plugin for JsonPlugin {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    fn detect(&self, input: &Input) -> Vec<Detection> {
        let t = input.trimmed;
        let detection = match (input.json(), input.json_error()) {
            (Some(v @ (Value::Object(_) | Value::Array(_))), _) => {
                Detection::new("json", 0.92).reason("valid JSON").preview(describe(v))
            }
            // A JSON string: only interesting if it holds JSON (cheap check first).
            (Some(Value::String(s)), _) if matches!(s.trim_start().as_bytes().first(), Some(b'{' | b'[')) => {
                match serde_json::from_str::<Value>(s) {
                    Ok(inner @ (Value::Object(_) | Value::Array(_))) => Detection::new("json", 0.9)
                        .reason("escaped JSON")
                        .preview(format!("{} inside a string", describe(&inner))),
                    _ => return Vec::new(),
                }
            }
            (None, Some((line, col))) if looks_like_container(t) || t.starts_with('{') => Detection::new("json", 0.6)
                .reason("looks like JSON but does not parse")
                .preview(format!("⚠ line {line}, col {col}")),
            _ => return Vec::new(),
        };
        vec![detection]
    }

    fn run(&self, req: &RunRequest) -> Result<ToolOutput, PluginError> {
        if req.operation_id != "json" {
            return Err(PluginError::UnknownOperation(req.operation_id.clone()));
        }
        let text = req.input.trim();
        let result = |label: &str, out: String, copy_label: &str| {
            ToolOutput::default()
                .block(Block::code(label, "json", out.clone()))
                .action(Action::copy("copy", copy_label, out.clone()).primary())
                .action(Action::replace_input("replace", "Replace input", out))
        };
        match param(&self.manifest, req, "mode") {
            "minify" => match serde_json::from_str::<Value>(text) {
                Ok(v) => Ok(result("Minified", v.to_string(), "Copy minified")),
                Err(e) => Ok(parse_error_output(text, &e)),
            },
            // Valid JSON is compacted first; any other text is escaped as-is.
            "escape" => {
                let raw = serde_json::from_str::<Value>(text).map(|v| v.to_string()).unwrap_or_else(|_| text.to_string());
                Ok(result("Escaped string", Value::String(raw).to_string(), "Copy escaped"))
            }
            "unescape" => {
                let s = unescape(text).ok_or_else(|| PluginError::Invalid("not a JSON string literal".into()))?;
                let out = match serde_json::from_str::<Value>(&s) {
                    Ok(v @ (Value::Object(_) | Value::Array(_))) => pretty(&v, param(&self.manifest, req, "indent")),
                    _ => s,
                };
                Ok(result("Unescaped", out, "Copy unescaped"))
            }
            _ => {
                let v: Value = match serde_json::from_str(text) {
                    Ok(v) => v,
                    Err(e) => return Ok(parse_error_output(text, &e)),
                };
                // An escaped JSON string: format what's inside it.
                let (v, note) = match v {
                    Value::String(s) => match serde_json::from_str::<Value>(&s) {
                        Ok(inner @ (Value::Object(_) | Value::Array(_))) => (inner, true),
                        _ => (Value::String(s), false),
                    },
                    other => (other, false),
                };
                let v = if flag(&self.manifest, req, "sort_keys") { sort_keys(v) } else { v };
                let formatted = pretty(&v, param(&self.manifest, req, "indent"));
                let mut out = ToolOutput::default();
                if note {
                    out = out.block(Block::Notice { level: NoticeLevel::Info, text: "Unescaped from a JSON string".into() });
                }
                Ok(out
                    .block(Block::code("Formatted", "json", formatted.clone()))
                    .action(Action::copy("copy", "Copy formatted", formatted.clone()).primary())
                    .action(Action::copy("copy_min", "Copy minified", v.to_string()))
                    .action(Action::replace_input("replace", "Replace input", formatted)))
            }
        }
    }
}

/// The content of a JSON string literal; also accepts the literal's body
/// without its quotes (`{\"a\":1}`), as it's often copied from logs.
fn unescape(text: &str) -> Option<String> {
    serde_json::from_str::<String>(text).ok().or_else(|| serde_json::from_str::<String>(&format!("\"{text}\"")).ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(input: &str, params: &[(&str, &str)]) -> ToolOutput {
        JsonPlugin::new()
            .run(&RunRequest::new("json", input).params(params.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()))
            .unwrap()
    }

    fn code(out: &ToolOutput) -> &str {
        out.blocks
            .iter()
            .find_map(|b| match b {
                Block::Code { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .unwrap()
    }

    #[test]
    fn formats_preserving_order_and_sorting() {
        let out = run(r#"{"b":1,"a":[true]}"#, &[]);
        assert_eq!(code(&out), "{\n  \"b\": 1,\n  \"a\": [\n    true\n  ]\n}");
        let out = run(r#"{"b":1,"a":2}"#, &[("sort_keys", "true"), ("indent", "4")]);
        assert_eq!(code(&out), "{\n    \"a\": 2,\n    \"b\": 1\n}");
    }

    #[test]
    fn reports_parse_errors() {
        let out = run("{\n  \"a\": 1,\n  \"b\" 2\n}", &[]);
        assert!(matches!(out.blocks[0], Block::Notice { level: NoticeLevel::Error, .. }));
        assert!(code(&out).contains('^'));
    }

    #[test]
    fn modes() {
        assert_eq!(code(&run(r#"{ "a" : [1, 2] }"#, &[("mode", "minify")])), r#"{"a":[1,2]}"#);
        assert_eq!(code(&run(r#"{"a": 1}"#, &[("mode", "escape")])), r#""{\"a\":1}""#);
        assert_eq!(code(&run("say \"hi\"", &[("mode", "escape")])), r#""say \"hi\"""#);
        assert_eq!(code(&run(r#""{\"a\":1}""#, &[("mode", "unescape")])), "{\n  \"a\": 1\n}");
        assert_eq!(code(&run(r#"{\"a\":1}"#, &[("mode", "unescape")])), "{\n  \"a\": 1\n}");
    }

    #[test]
    fn format_unwraps_escaped_json() {
        let out = run(r#""{\"a\":1}""#, &[]);
        assert!(matches!(out.blocks[0], Block::Notice { level: NoticeLevel::Info, .. }));
        assert_eq!(code(&out), "{\n  \"a\": 1\n}");
    }
}
