use delight_sdk::{
    Action, Block, Detection, Input, NoticeLevel, Plugin, PluginError, PluginManifest, RunRequest, ToolOutput,
};
use serde_json::{Map, Value};

use super::{manifest, op};
use crate::util::preview;

/// Lines looked at by `detect`.
const DETECT_LINES: usize = 400;

pub struct EnvPlugin {
    manifest: PluginManifest,
}

impl EnvPlugin {
    pub fn new() -> Self {
        Self {
            manifest: manifest(
                "delight.env",
                "Env",
                "Convert between JSON and .env / dotenv files.",
                "$=",
                "#007AFF",
                &["env", "dotenv", "environment variables", "config"],
                vec![
                    op("json_to_env", "JSON → .env", "Flatten a JSON object into KEY=value lines", &["json", "env", "convert"], vec![]),
                    op("env_to_json", ".env → JSON", "Parse KEY=value lines into JSON", &["env", "json", "convert"], vec![]),
                ],
            ),
        }
    }
}

impl Default for EnvPlugin {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// .env parsing
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
struct ParsedEnv {
    pairs: Vec<(String, String)>,
    skipped: Vec<usize>,
}

fn split_key(line: &str) -> Option<(&str, &str)> {
    let line = line.strip_prefix("export ").map(str::trim_start).unwrap_or(line);
    let eq = line.find('=')?;
    let key = line[..eq].trim_end();
    let mut chars = key.chars();
    let first = chars.next()?;
    let valid = (first.is_ascii_alphabetic() || first == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'));
    valid.then(|| (key, line[eq + 1..].trim_start()))
}

fn unescape_double(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c == '\\' {
            match it.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                Some(o) => out.push(o),
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Closing quote index (not escaped) in `s`.
fn closing_quote(s: &str, q: char) -> Option<usize> {
    let mut escaped = false;
    for (i, c) in s.char_indices() {
        match c {
            '\\' if q == '"' && !escaped => escaped = true,
            c if c == q && !escaped => return Some(i),
            _ => escaped = false,
        }
    }
    None
}

fn parse_env(text: &str) -> ParsedEnv {
    let mut env = ParsedEnv::default();
    let lines: Vec<&str> = text.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let line_no = i + 1;
        let line = lines[i].trim();
        i += 1;
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, rest)) = split_key(line) else {
            env.skipped.push(line_no);
            continue;
        };
        let value = match rest.chars().next() {
            Some(q @ ('"' | '\'')) => {
                let mut body = rest[1..].to_string();
                // Values may span lines until the closing quote.
                loop {
                    if let Some(end) = closing_quote(&body, q) {
                        body.truncate(end);
                        break;
                    }
                    if i >= lines.len() {
                        break;
                    }
                    body.push('\n');
                    body.push_str(lines[i]);
                    i += 1;
                }
                if q == '"' { unescape_double(&body) } else { body }
            }
            _ => match rest.find(" #") {
                Some(idx) => rest[..idx].trim_end().to_string(),
                None => rest.trim_end().to_string(),
            },
        };
        env.pairs.push((key.to_string(), value));
    }
    env
}

fn infer(value: &str) -> Value {
    match value {
        "true" => Value::Bool(true),
        "false" => Value::Bool(false),
        "null" => Value::Null,
        _ => {
            let numeric_ok = !value.is_empty() && !(value.len() > 1 && value.starts_with('0') && !value.starts_with("0."));
            if numeric_ok && let Ok(n) = value.parse::<i64>() {
                return Value::from(n);
            }
            if numeric_ok
                && value.contains('.')
                && let Ok(f) = value.parse::<f64>()
                && f.is_finite()
            {
                return Value::from(f);
            }
            Value::String(value.to_string())
        }
    }
}

// ---------------------------------------------------------------------------
// JSON → .env
// ---------------------------------------------------------------------------

fn flatten(prefix: &str, sep: &str, value: &Value, out: &mut Vec<(String, String)>) {
    match value {
        Value::Object(m) => {
            for (k, v) in m {
                let key = if prefix.is_empty() { k.clone() } else { format!("{prefix}{sep}{k}") };
                flatten(&key, sep, v, out);
            }
        }
        Value::String(s) => out.push((prefix.to_string(), s.clone())),
        Value::Null => out.push((prefix.to_string(), String::new())),
        other => out.push((prefix.to_string(), other.to_string())),
    }
}

/// `db.host-name` → `DB_HOST_NAME`.
fn env_key(raw: &str) -> String {
    raw.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_uppercase() } else { '_' }).collect()
}

fn env_value(v: &str) -> String {
    let needs_quotes = v.is_empty() || v.chars().any(|c| c.is_whitespace() || "#\"'$`\\=".contains(c));
    if !needs_quotes {
        return v.to_string();
    }
    let escaped = v.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n");
    format!("\"{escaped}\"")
}

impl Plugin for EnvPlugin {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    fn detect(&self, input: &Input) -> Vec<Detection> {
        let t = input.trimmed;
        let mut out = Vec::new();
        if t.starts_with('{') {
            if let Some(v @ Value::Object(_)) = input.json() {
                let mut pairs = Vec::new();
                flatten("", "_", v, &mut pairs);
                let first = pairs.first().map(|(k, v)| format!("{}={}", env_key(k), env_value(v)));
                let mut d = Detection::new("json_to_env", 0.5).preview(format!("{} variables", pairs.len()));
                if let Some(first) = first {
                    d = d.preview(preview(&format!("{first} …"), 80));
                }
                out.push(d);
            }
            return out;
        }
        if t.starts_with('[') {
            return out;
        }
        // The first lines decide; a huge paste isn't parsed in full on every keystroke.
        let head = t.char_indices().filter(|(_, c)| *c == '\n').nth(DETECT_LINES).map_or(t, |(i, _)| &t[..i]);
        let env = parse_env(head);
        if !env.pairs.is_empty() {
            let total = env.pairs.len() + env.skipped.len();
            let ratio = env.pairs.len() as f32 / total as f32;
            // A lone `a=b` could just as well be a query string or an equation.
            let base = if env.pairs.len() == 1 { 0.55 } else { 0.9 };
            if ratio >= 0.7 {
                out.push(
                    Detection::new("env_to_json", base * ratio)
                        .reason(format!("{} KEY=value lines", env.pairs.len()))
                        .preview(format!("{} variables → JSON object", env.pairs.len())),
                );
            }
        }
        out
    }

    fn run(&self, req: &RunRequest) -> Result<ToolOutput, PluginError> {
        match req.operation_id.as_str() {
            "json_to_env" => {
                let v: Value = serde_json::from_str(req.input.trim())
                    .map_err(|e| PluginError::Invalid(format!("input must be a JSON object: {e}")))?;
                if !v.is_object() {
                    return Err(PluginError::Invalid("input must be a JSON object".into()));
                }
                let mut pairs = Vec::new();
                flatten("", "_", &v, &mut pairs);
                let text = pairs.iter().map(|(k, v)| format!("{}={}", env_key(k), env_value(v))).collect::<Vec<_>>().join("\n");
                Ok(ToolOutput::default()
                    .block(Block::code(".env", "env", text.clone()))
                    .action(Action::copy("copy", "Copy .env", text.clone()).primary())
                    .action(Action::replace_input("replace", "Replace input", text)))
            }
            "env_to_json" => {
                let env = parse_env(&req.input);
                let root: Map<String, Value> = env.pairs.iter().map(|(k, v)| (k.clone(), infer(v))).collect();
                let json = super::json::pretty(&Value::Object(root), "2");
                let mut out = ToolOutput::default();
                if !env.skipped.is_empty() {
                    let lines: Vec<String> = env.skipped.iter().map(|n| n.to_string()).collect();
                    out = out.block(Block::Notice {
                        level: NoticeLevel::Warning,
                        text: format!("Skipped unparseable line(s): {}", lines.join(", ")),
                    });
                }
                Ok(out
                    .block(Block::code("JSON", "json", json.clone()))
                    .action(Action::copy("copy", "Copy JSON", json.clone()).primary())
                    .action(Action::replace_input("replace", "Replace input", json)))
            }
            other => Err(PluginError::UnknownOperation(other.into())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(op: &str, input: &str) -> String {
        let out = EnvPlugin::new()
            .run(&RunRequest::new(op, input))
            .unwrap();
        out.blocks
            .into_iter()
            .find_map(|b| match b {
                Block::Code { text, .. } => Some(text),
                _ => None,
            })
            .unwrap()
    }

    #[test]
    fn parses_dotenv() {
        let env = parse_env("# c\nexport A=1\nB = \"x\\ny\" \nC='lit $x'\nD=plain # comment\nE=\"multi\nline\"\nnot a pair");
        assert_eq!(
            env.pairs,
            vec![
                ("A".into(), "1".into()),
                ("B".into(), "x\ny".into()),
                ("C".into(), "lit $x".into()),
                ("D".into(), "plain".into()),
                ("E".into(), "multi\nline".into()),
            ]
        );
        assert_eq!(env.skipped, vec![8]);
    }

    #[test]
    fn env_to_json_infers_types() {
        let out = run("env_to_json", "PORT=8080\nDEBUG=true\nZIP=01234\nDB__HOST=x");
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["PORT"], 8080);
        assert_eq!(v["DEBUG"], true);
        assert_eq!(v["ZIP"], "01234");
        assert_eq!(v["DB__HOST"], "x");
    }

    #[test]
    fn json_to_env_flattens() {
        let out = run("json_to_env", r#"{"db":{"host":"a b","port":5432},"tags":["x"]}"#);
        assert_eq!(out, "DB_HOST=\"a b\"\nDB_PORT=5432\nTAGS=\"[\\\"x\\\"]\"");
    }
}
