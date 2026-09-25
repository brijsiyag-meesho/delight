//! JSON: format, minify, escape and unescape.

use delight_sdk::{Action, Detection, Input, OperationSpec, Plugin, PluginManifest, ToolView};
use delight_ui::{IconButton, IconName, Language, SegmentedControl, Selectable, h_flex, v_flex};
use gpui::{
    App, AppContext, Context, FocusHandle, IntoElement, ParentElement, Render, SharedString, Styled, Task, Window, px,
};
use serde::Serialize;
use serde_json::Value;

use crate::output::{Converter, ConverterView, Output};

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
        let manifest = crate::manifest("delight.json", "JSON", include_bytes!("../assets/json.svg"))
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
            Ok(Value::String(s)) if is_container(&s) => 0.9,
            Ok(_) => return Vec::new(),
            // Looks like JSON but doesn't parse: show where it breaks.
            Err(_) if starts_container => 0.6,
            Err(_) => return Vec::new(),
        };
        vec![Detection::new("json", confidence)]
    }

    fn tool_view(&self, _operation_id: &str, cx: &mut App) -> Box<dyn ToolView> {
        Box::new(ConverterView(cx.new(JsonView::new)))
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum Mode {
    #[default]
    Format,
    Minify,
    Escape,
    Unescape,
}

const MODES: [(Mode, &str); 4] =
    [(Mode::Format, "Format"), (Mode::Minify, "Minify"), (Mode::Escape, "Escape"), (Mode::Unescape, "Unescape")];

/// Spaces per level.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum Indent {
    #[default]
    Two,
    Four,
}

impl Indent {
    fn bytes(self) -> &'static [u8] {
        match self {
            Indent::Two => b"  ",
            Indent::Four => b"    ",
        }
    }

    fn toggled(self) -> Self {
        match self {
            Indent::Two => Indent::Four,
            Indent::Four => Indent::Two,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Indent::Two => "2",
            Indent::Four => "4",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Options {
    mode: Mode,
    indent: Indent,
    sort_keys: bool,
}

struct JsonView {
    /// The mode tabs' focus: a Tab stop, where ← / → switch modes.
    modes_focus: FocusHandle,
    input: SharedString,
    options: Options,
    output: Output,
    /// Minified, for "Copy minified" next to a formatted result.
    minified: Option<String>,
    /// Replacing it cancels the conversion in progress.
    _task: Option<Task<()>>,
}

impl JsonView {
    fn new(cx: &mut Context<Self>) -> Self {
        Self {
            modes_focus: cx.focus_handle().tab_stop(true),
            input: SharedString::default(),
            options: Options::default(),
            output: Output::default(),
            minified: None,
            _task: None,
        }
    }

    fn set_options(&mut self, options: Options, cx: &mut Context<Self>) {
        self.options = options;
        self.convert(cx);
    }

    /// Converts the input in the background, then shows the result.
    fn convert(&mut self, cx: &mut Context<Self>) {
        let (input, options) = (self.input.clone(), self.options);
        self._task = Some(cx.spawn(async move |this, cx| {
            let (output, minified) = cx.background_executor().spawn(async move { convert(&input, options) }).await;
            let _ = this.update(cx, |this, cx| {
                this.output = output;
                this.minified = minified;
                cx.notify();
            });
        }));
    }
}

impl Converter for JsonView {
    fn set_input(&mut self, input: SharedString, cx: &mut Context<Self>) {
        self.input = input;
        self.convert(cx);
    }

    fn actions(&self) -> Vec<Action> {
        let copy = match self.options.mode {
            Mode::Format => "Copy formatted",
            Mode::Minify => "Copy minified",
            Mode::Escape => "Copy escaped",
            Mode::Unescape => "Copy unescaped",
        };
        let mut actions = self.output.actions(copy);
        if let Some(minified) = &self.minified {
            actions.insert(1, Action::copy("copy_minified", "Copy minified", minified.clone()));
        }
        actions
    }
}

impl Render for JsonView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let options = self.options;
        let modes = SegmentedControl::new("json-mode")
            .focus(&self.modes_focus)
            .options(MODES.map(|(_, label)| label))
            .selected(MODES.iter().position(|(m, _)| *m == options.mode).unwrap_or(0))
            .on_change(cx.listener(move |this, index: &usize, _, cx| {
                this.set_options(Options { mode: MODES[*index].0, ..options }, cx)
            }));
        // Formatting buttons at the right of the result's title.
        let indent = IconButton::new("json-indent", IconName::IndentIncrease)
            .label(options.indent.label())
            .tooltip(format!("Indent: {} spaces", options.indent.label()))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.set_options(Options { indent: options.indent.toggled(), ..options }, cx)
            }));
        let sort_keys = IconButton::new("json-sort-keys", IconName::ArrowDownAZ)
            .selected(options.sort_keys)
            .tooltip(if options.sort_keys { "Keys sorted A→Z" } else { "Sort keys A→Z" })
            .on_click(cx.listener(move |this, _, _, cx| {
                this.set_options(Options { sort_keys: !options.sort_keys, ..options }, cx)
            }));
        let mut accessory = h_flex().gap(px(2.));
        if matches!(options.mode, Mode::Format | Mode::Unescape) {
            accessory = accessory.child(indent);
        }
        if options.mode == Mode::Format {
            accessory = accessory.child(sort_keys);
        }
        v_flex().gap(px(14.)).child(h_flex().child(modes)).child(self.output.render(Some(accessory.into_any_element()), cx))
    }
}

// ---------------------------------------------------------------------------
// Conversion
// ---------------------------------------------------------------------------

/// The result, and (for a formatted one) the minified JSON.
fn convert(input: &str, options: Options) -> (Output, Option<String>) {
    let text = input.trim();
    if text.is_empty() {
        return (Output::Empty, None);
    }
    let parse_error = |e: serde_json::Error| {
        let message = format!("Invalid JSON: line {}, column {}: {e}", e.line(), e.column());
        Output::error(message, text, Some((e.line(), e.column())))
    };
    match options.mode {
        Mode::Format => {
            let value: Value = match serde_json::from_str(text) {
                Ok(value) => value,
                Err(e) => return (parse_error(e), None),
            };
            // A JSON string holding JSON: format what's inside it.
            let (value, unescaped) = match value {
                Value::String(s) if is_container(&s) => (serde_json::from_str(&s).unwrap_or(Value::String(s)), true),
                other => (other, false),
            };
            let value = if options.sort_keys { sort_keys(value) } else { value };
            let output = Output::code("Formatted", Language::Json, pretty(&value, options.indent));
            let output = if unescaped { output.with_note("Unescaped from a JSON string") } else { output };
            (output, Some(value.to_string()))
        }
        Mode::Minify => match serde_json::from_str::<Value>(text) {
            Ok(value) => (Output::code("Minified", Language::Json, value.to_string()), None),
            Err(e) => (parse_error(e), None),
        },
        // Valid JSON is compacted first; any other text is escaped as it is.
        Mode::Escape => {
            let raw = serde_json::from_str::<Value>(text).map(|v| v.to_string()).unwrap_or_else(|_| text.to_string());
            (Output::code("Escaped", Language::Json, Value::String(raw).to_string()), None)
        }
        Mode::Unescape => match unescape(text) {
            Some(s) => {
                let out = match serde_json::from_str::<Value>(&s) {
                    Ok(value @ (Value::Object(_) | Value::Array(_))) => pretty(&value, options.indent),
                    _ => s,
                };
                (Output::code("Unescaped", Language::Json, out), None)
            }
            None => (Output::error("Not a JSON string literal".to_string(), text, None), None),
        },
    }
}

/// Whether `s` holds a JSON object or array.
fn is_container(s: &str) -> bool {
    matches!(s.trim_start().as_bytes().first(), Some(b'{' | b'['))
        && matches!(serde_json::from_str::<Value>(s), Ok(Value::Object(_) | Value::Array(_)))
}

fn sort_keys(value: Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut entries: Vec<(String, Value)> = map.into_iter().collect();
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            Value::Object(entries.into_iter().map(|(k, v)| (k, sort_keys(v))).collect())
        }
        Value::Array(items) => Value::Array(items.into_iter().map(sort_keys).collect()),
        other => other,
    }
}

/// `value` as indented JSON.
pub(crate) fn pretty(value: &Value, indent: Indent) -> String {
    let mut out = Vec::new();
    let formatter = serde_json::ser::PrettyFormatter::with_indent(indent.bytes());
    let mut serializer = serde_json::Serializer::with_formatter(&mut out, formatter);
    value.serialize(&mut serializer).expect("serializing a Value can't fail");
    String::from_utf8(out).expect("serde_json writes UTF-8")
}

/// The content of a JSON string literal; also accepts the literal without
/// its quotes (`{\"a\":1}`), as it's often copied from logs.
fn unescape(text: &str) -> Option<String> {
    serde_json::from_str::<String>(text).ok().or_else(|| serde_json::from_str::<String>(&format!("\"{text}\"")).ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(input: &str, options: Options) -> String {
        match convert(input, options).0 {
            Output::Code { text, .. } => text.to_string(),
            Output::Error { message, .. } => format!("error: {message}"),
            Output::Empty => String::new(),
        }
    }

    fn mode(mode: Mode) -> Options {
        Options { mode, ..Options::default() }
    }

    #[test]
    fn formats_keeping_order_or_sorting() {
        assert_eq!(run(r#"{"b":1,"a":[true]}"#, Options::default()), "{\n  \"b\": 1,\n  \"a\": [\n    true\n  ]\n}");
        let sorted = Options { indent: Indent::Four, sort_keys: true, ..Options::default() };
        assert_eq!(run(r#"{"b":1,"a":2}"#, sorted), "{\n    \"a\": 2,\n    \"b\": 1\n}");
    }

    #[test]
    fn reports_where_it_breaks() {
        let (output, _) = convert("{\n  \"a\": 1,\n  \"b\" 2\n}", Options::default());
        let Output::Error { message, near } = output else { panic!("expected an error") };
        assert!(message.contains("line 3"));
        assert!(near.is_some());
    }

    #[test]
    fn modes() {
        assert_eq!(run(r#"{ "a" : [1, 2] }"#, mode(Mode::Minify)), r#"{"a":[1,2]}"#);
        assert_eq!(run(r#"{"a": 1}"#, mode(Mode::Escape)), r#""{\"a\":1}""#);
        assert_eq!(run("say \"hi\"", mode(Mode::Escape)), r#""say \"hi\"""#);
        assert_eq!(run(r#""{\"a\":1}""#, mode(Mode::Unescape)), "{\n  \"a\": 1\n}");
        assert_eq!(run(r#"{\"a\":1}"#, mode(Mode::Unescape)), "{\n  \"a\": 1\n}");
    }

    #[test]
    fn format_unwraps_escaped_json() {
        let (output, minified) = convert(r#""{\"a\":1}""#, Options::default());
        assert!(matches!(output, Output::Code { note: Some(_), .. }));
        assert_eq!(minified.as_deref(), Some(r#"{"a":1}"#));
    }

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
