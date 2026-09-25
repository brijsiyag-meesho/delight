//! # Delight SDK
//!
//! Everything a Delight tool is written against. Built as a Rust `dylib` so
//! the app and every plugin share **one** copy of GPUI (and of this crate) at
//! runtime — that's what lets a plugin's views live inside Delight's windows.
//!
//! A tool implements [`Plugin`]:
//!
//! * [`Plugin::detect`] — *"can you handle this input?"* ([`Detection`]s,
//!   ranked by confidence; declarative [`DetectRule`]s cover most cases).
//! * [`Plugin::run`] — a declarative result ([`Block`]s + [`Action`]s) that the
//!   host renders natively. Enough for most tools.
//! * [`Plugin::tool_view`] / [`Plugin::settings_view`] — optional custom GPUI
//!   UI for the result pane and the plugin's Settings page.
//!
//! [`theme`], [`ui`], [`editor`] and [`assets`] are the app's own native
//! building blocks, so plugin views can match it exactly.
//!
//! Plugins outside the app are `dylib` crates that call [`export_plugin!`] and
//! depend on GPUI only through this crate (see the `delight-gpui` alias
//! crate). Delight refuses plugins built against a different SDK build
//! ([`BUILD_ID`]).
//!
//! ## Compatibility
//!
//! Every public struct and enum is `#[non_exhaustive]`: build values with the
//! constructors and builder methods (`OperationSpec::new(..).detect(..)`),
//! and give `match`es on SDK enums a `_` arm. SDK releases can then add
//! fields, variants and trait methods (always with a default body) without
//! breaking plugin sources — such a release only needs a rebuild.

use std::collections::BTreeMap;

pub mod assets;
pub mod editor;
pub mod rules;
pub mod theme;
pub mod ui;
mod view;

pub use gpui;
pub use serde_json;
pub use view::*;

// ---------------------------------------------------------------------------
// Manifest
// ---------------------------------------------------------------------------

/// Static description of a plugin and the operations it offers.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct PluginManifest {
    /// Globally unique, reverse-DNS style id, e.g. `delight.json`.
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub author: Option<String>,
    /// Short glyph (1–3 chars) shown in the tool badge, e.g. `{}`.
    pub icon: Option<String>,
    /// Accent colour for the badge, `#RRGGBB`.
    pub accent: Option<String>,
    /// Free-form tags. Also fed to model-based classifiers as label hints.
    pub tags: Vec<String>,
    pub operations: Vec<OperationSpec>,
    /// Plugin-wide preferences (base URLs, tokens …). The host renders them on
    /// the plugin's page in Settings, persists them — `secret` fields go to the
    /// macOS Keychain — and passes the values in [`RunRequest::settings`].
    pub settings: Vec<FormField>,
}

impl PluginManifest {
    /// A manifest with just an id and a name; add the rest with the builder
    /// methods below.
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            version: String::new(),
            description: String::new(),
            author: None,
            icon: None,
            accent: None,
            tags: Vec::new(),
            operations: Vec::new(),
            settings: Vec::new(),
        }
    }

    pub fn operation(&self, id: &str) -> Option<&OperationSpec> {
        self.operations.iter().find(|op| op.id == id)
    }

    pub fn version(mut self, version: impl Into<String>) -> Self {
        self.version = version.into();
        self
    }
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }
    pub fn author(mut self, author: impl Into<String>) -> Self {
        self.author = Some(author.into());
        self
    }
    pub fn icon(mut self, icon: impl Into<String>) -> Self {
        self.icon = Some(icon.into());
        self
    }
    pub fn accent(mut self, accent: impl Into<String>) -> Self {
        self.accent = Some(accent.into());
        self
    }
    pub fn tags<S: Into<String>>(mut self, tags: impl IntoIterator<Item = S>) -> Self {
        self.tags = tags.into_iter().map(Into::into).collect();
        self
    }
    pub fn operations(mut self, operations: impl IntoIterator<Item = OperationSpec>) -> Self {
        self.operations = operations.into_iter().collect();
        self
    }
    pub fn settings(mut self, settings: impl IntoIterator<Item = FormField>) -> Self {
        self.settings = settings.into_iter().collect();
        self
    }
}

/// One thing a plugin can do with an input, e.g. "Decode Base64".
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct OperationSpec {
    pub id: String,
    pub title: String,
    pub description: String,
    /// Label hints for model-based classification (e.g. `["base64", "decode"]`).
    pub tags: Vec<String>,
    /// User-editable parameters rendered as a form above the output.
    pub params: Vec<FormField>,
    /// Declarative detection rules evaluated by the host. Plugins that
    /// implement [`Plugin::detect`] themselves may leave this empty.
    pub detect: Vec<DetectRule>,
    /// Wait until typing pauses this long before calling [`Plugin::run`] —
    /// for operations that hit the network. `0` runs on every change.
    pub run_delay_ms: u32,
    /// Key of a [`FieldKind::Select`] param that is the operation's *mode*
    /// (e.g. Format / Minify). The host renders it as tabs, first and without
    /// a label, and switches it with ←/→ (at the input's edges) and ⌘⇧[ / ⌘⇧].
    pub mode: Option<String>,
    /// List the operation under "Other Tools" even when its detection doesn't
    /// match the input. For tools that take any input, where the input can't
    /// tell whether they're wanted (e.g. an experiment lookup). Leave `false`
    /// for tools that recognise their input format — they're hidden when the
    /// input isn't in it.
    pub show_unmatched: bool,
}

impl OperationSpec {
    /// An operation with just an id and a title; add the rest with the
    /// builder methods below.
    pub fn new(id: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            description: String::new(),
            tags: Vec::new(),
            params: Vec::new(),
            detect: Vec::new(),
            run_delay_ms: 0,
            mode: None,
            show_unmatched: false,
        }
    }

    /// The param named by [`OperationSpec::mode`].
    pub fn mode_field(&self) -> Option<&FormField> {
        let key = self.mode.as_deref()?;
        self.params.iter().find(|f| f.key == key && matches!(f.kind, FieldKind::Select { .. }))
    }

    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }
    pub fn tags<S: Into<String>>(mut self, tags: impl IntoIterator<Item = S>) -> Self {
        self.tags = tags.into_iter().map(Into::into).collect();
        self
    }
    pub fn params(mut self, params: impl IntoIterator<Item = FormField>) -> Self {
        self.params = params.into_iter().collect();
        self
    }
    pub fn detect(mut self, rules: impl IntoIterator<Item = DetectRule>) -> Self {
        self.detect = rules.into_iter().collect();
        self
    }
    pub fn run_delay_ms(mut self, ms: u32) -> Self {
        self.run_delay_ms = ms;
        self
    }
    /// Key of the `Select` param shown as mode tabs (see [`OperationSpec::mode`]).
    pub fn mode(mut self, key: impl Into<String>) -> Self {
        self.mode = Some(key.into());
        self
    }
    pub fn show_unmatched(mut self, show: bool) -> Self {
        self.show_unmatched = show;
        self
    }
}

// ---------------------------------------------------------------------------
// Forms — parameters an operation needs (secrets, options, future API forms)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct FormField {
    pub key: String,
    pub label: String,
    pub kind: FieldKind,
    pub default: Option<String>,
    pub placeholder: Option<String>,
    pub help: Option<String>,
    /// Only show the field while another field has one of these values
    /// (e.g. "Sort keys" only in the Format mode). `None`: always shown.
    pub show_when: Option<ShowWhen>,
}

#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct ShowWhen {
    pub key: String,
    pub one_of: Vec<String>,
}

impl ShowWhen {
    pub fn new(key: impl Into<String>, one_of: &[&str]) -> Self {
        Self { key: key.into(), one_of: one_of.iter().map(|v| v.to_string()).collect() }
    }
}

impl FormField {
    pub fn new(key: impl Into<String>, label: impl Into<String>, kind: FieldKind) -> Self {
        Self { key: key.into(), label: label.into(), kind, default: None, placeholder: None, help: None, show_when: None }
    }
    pub fn text(key: impl Into<String>, label: impl Into<String>) -> Self {
        Self::new(key, label, FieldKind::Text)
    }
    /// Masked text. As a plugin setting, it's stored in the Keychain.
    pub fn secret(key: impl Into<String>, label: impl Into<String>) -> Self {
        Self::new(key, label, FieldKind::Secret)
    }
    pub fn multiline(key: impl Into<String>, label: impl Into<String>) -> Self {
        Self::new(key, label, FieldKind::Multiline)
    }
    pub fn number(key: impl Into<String>, label: impl Into<String>) -> Self {
        Self::new(key, label, FieldKind::Number)
    }
    pub fn toggle(key: impl Into<String>, label: impl Into<String>, on: bool) -> Self {
        Self::new(key, label, FieldKind::Toggle).default(if on { "true" } else { "false" })
    }
    /// A select from `(value, label)` pairs; the first is the default.
    pub fn select<V: Into<String>, L: Into<String>>(
        key: impl Into<String>,
        label: impl Into<String>,
        options: impl IntoIterator<Item = (V, L)>,
    ) -> Self {
        let options: Vec<SelectOption> = options.into_iter().map(|(v, l)| SelectOption::new(v, l)).collect();
        let default = options.first().map(|o| o.value.clone());
        Self { default, ..Self::new(key, label, FieldKind::Select { options }) }
    }

    /// Whether the field is shown for the current `values`.
    pub fn visible(&self, values: &Params) -> bool {
        self.show_when.as_ref().is_none_or(|c| values.get(&c.key).is_some_and(|v| c.one_of.contains(v)))
    }

    pub fn default(mut self, value: impl Into<String>) -> Self {
        self.default = Some(value.into());
        self
    }
    pub fn placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = Some(placeholder.into());
        self
    }
    pub fn help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }
    pub fn show_when(mut self, condition: ShowWhen) -> Self {
        self.show_when = Some(condition);
        self
    }
}

#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum FieldKind {
    Text,
    /// Masked single-line text.
    Secret,
    /// Multi-line text (e.g. a request body).
    Multiline,
    Number,
    /// Value is `"true"` / `"false"`.
    Toggle,
    Select { options: Vec<SelectOption> },
}

#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct SelectOption {
    pub value: String,
    pub label: String,
}

impl SelectOption {
    pub fn new(value: impl Into<String>, label: impl Into<String>) -> Self {
        Self { value: value.into(), label: label.into() }
    }
}

/// Form values keyed by [`FormField::key`]. Everything is a string; toggles
/// are `"true"`/`"false"`.
pub type Params = BTreeMap<String, String>;

pub trait ParamsExt {
    fn text(&self, key: &str) -> &str;
    fn flag(&self, key: &str) -> bool;
}

impl ParamsExt for Params {
    fn text(&self, key: &str) -> &str {
        self.get(key).map(String::as_str).unwrap_or("")
    }
    fn flag(&self, key: &str) -> bool {
        matches!(self.text(key), "true" | "1" | "yes")
    }
}

// ---------------------------------------------------------------------------
// Detection
// ---------------------------------------------------------------------------

/// Declarative detection rule, evaluated by the host (see [`rules`]).
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum DetectRule {
    /// Matches when the trimmed input matches the regex.
    Regex {
        pattern: String,
        confidence: f32,
    },
    /// Matches when the trimmed input starts with `value`.
    Prefix {
        value: String,
        confidence: f32,
    },
    /// Matches when the trimmed input parses as a JSON object or array.
    Json {
        confidence: f32,
    },
    /// Matches any non-empty input (useful for "encode" style operations).
    Always {
        confidence: f32,
    },
}

impl DetectRule {
    pub fn regex(pattern: impl Into<String>, confidence: f32) -> Self {
        DetectRule::Regex { pattern: pattern.into(), confidence }
    }
    pub fn prefix(value: impl Into<String>, confidence: f32) -> Self {
        DetectRule::Prefix { value: value.into(), confidence }
    }
    pub fn json(confidence: f32) -> Self {
        DetectRule::Json { confidence }
    }
    pub fn always(confidence: f32) -> Self {
        DetectRule::Always { confidence }
    }
}

/// A plugin's claim that one of its operations applies to the input.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Detection {
    pub operation_id: String,
    /// 0.0 – 1.0. Ranking across plugins is by this value.
    pub confidence: f32,
    /// Why it matched ("valid JSON object", "3 dot-separated base64url parts").
    pub reason: Option<String>,
    /// One-line preview of the result shown in the tool list.
    pub preview: Option<String>,
    /// For operations with a [`OperationSpec::mode`]: the mode (tab) that
    /// fits this input, e.g. `uuid7` → the UUID v7 tab. The host switches to
    /// it when the suggestion changes; a tab the user picks stays until then.
    pub mode: Option<String>,
}

impl Detection {
    pub fn new(operation_id: impl Into<String>, confidence: f32) -> Self {
        Self {
            operation_id: operation_id.into(),
            confidence: confidence.clamp(0.0, 1.0),
            reason: None,
            preview: None,
            mode: None,
        }
    }
    /// The mode (tab value) to open for this input.
    pub fn mode(mut self, value: impl Into<String>) -> Self {
        self.mode = Some(value.into());
        self
    }
    pub fn reason(mut self, reason: impl Into<String>) -> Self {
        self.reason = Some(reason.into());
        self
    }
    pub fn preview(mut self, preview: impl Into<String>) -> Self {
        self.preview = Some(preview.into());
        self
    }
}

/// The input handed to detection — one per classification, shared by every
/// plugin. `trimmed` is precomputed and [`Input::json`] parsed at most once,
/// because nearly every detector wants them.
///
/// Detection runs for every plugin on every input change, so keep
/// [`Plugin::detect`] cheap: reject with an O(1) look at the input's shape
/// (first character, prefix, length) before any full parse, and use
/// [`Input::json`] instead of parsing JSON yourself. See the README.
#[derive(Debug, Clone)]
pub struct Input<'a> {
    pub text: &'a str,
    pub trimmed: &'a str,
    json: std::sync::OnceLock<JsonParse>,
}

#[derive(Debug, Clone)]
enum JsonParse {
    NotJson,
    Ok(serde_json::Value),
    /// Looked like JSON but didn't parse: (line, column).
    Invalid(usize, usize),
}

impl<'a> Input<'a> {
    pub fn new(text: &'a str) -> Self {
        Self { text, trimmed: text.trim(), json: std::sync::OnceLock::new() }
    }

    fn parsed(&self) -> &JsonParse {
        self.json.get_or_init(|| {
            // Only text that starts like a JSON document is parsed at all.
            if !matches!(self.trimmed.as_bytes().first(), Some(b'{' | b'[' | b'"')) {
                return JsonParse::NotJson;
            }
            match serde_json::from_str(self.trimmed) {
                Ok(v) => JsonParse::Ok(v),
                Err(e) => JsonParse::Invalid(e.line(), e.column()),
            }
        })
    }

    /// The input parsed as a JSON document (object, array or string), parsed
    /// once and shared by every plugin; `None` if it isn't JSON.
    pub fn json(&self) -> Option<&serde_json::Value> {
        match self.parsed() {
            JsonParse::Ok(v) => Some(v),
            _ => None,
        }
    }

    /// Where parsing failed, `(line, column)`, when the input starts like JSON
    /// (`{`, `[`, `"`) but isn't valid.
    pub fn json_error(&self) -> Option<(usize, usize)> {
        match self.parsed() {
            JsonParse::Invalid(line, column) => Some((*line, *column)),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Running
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct RunRequest {
    pub operation_id: String,
    pub input: String,
    pub params: Params,
    /// The plugin's [`PluginManifest::settings`] values, defaults filled in.
    pub settings: Params,
}

impl RunRequest {
    pub fn new(operation_id: impl Into<String>, input: impl Into<String>) -> Self {
        Self { operation_id: operation_id.into(), input: input.into(), params: Params::new(), settings: Params::new() }
    }
    pub fn params(mut self, params: Params) -> Self {
        self.params = params;
        self
    }
    pub fn settings(mut self, settings: Params) -> Self {
        self.settings = settings;
        self
    }
}

/// Declarative result of running an operation.
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct ToolOutput {
    pub blocks: Vec<Block>,
    /// The first action with `primary: true` (or the first action) is bound to ↵.
    pub actions: Vec<Action>,
}

impl ToolOutput {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn block(mut self, block: Block) -> Self {
        self.blocks.push(block);
        self
    }
    pub fn action(mut self, action: Action) -> Self {
        self.actions.push(action);
        self
    }
    pub fn notice(level: NoticeLevel, text: impl Into<String>) -> Self {
        Self::default().block(Block::Notice { level, text: text.into() })
    }
}

#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum Block {
    /// Monospaced, optionally syntax-highlighted text.
    Code {
        label: Option<String>,
        /// `json`, `env`, `text` … — the host highlights what it knows.
        language: Option<String>,
        text: String,
    },
    Text {
        label: Option<String>,
        text: String,
    },
    KeyValue {
        label: Option<String>,
        rows: Vec<KeyValueRow>,
    },
    Notice { level: NoticeLevel, text: String },
    /// Renders the operation's param field `key` at this spot instead of in
    /// the form above the output — e.g. a secret right next to what it verifies.
    Field { key: String },
}

impl Block {
    pub fn code(label: impl Into<String>, language: impl Into<String>, text: impl Into<String>) -> Self {
        Block::Code { label: Some(label.into()), language: Some(language.into()), text: text.into() }
    }
    pub fn text(label: impl Into<String>, text: impl Into<String>) -> Self {
        Block::Text { label: Some(label.into()), text: text.into() }
    }
    pub fn key_value(label: impl Into<String>, rows: Vec<KeyValueRow>) -> Self {
        Block::KeyValue { label: Some(label.into()), rows }
    }
    pub fn notice(level: NoticeLevel, text: impl Into<String>) -> Self {
        Block::Notice { level, text: text.into() }
    }
    /// The operation's param `key`, rendered here (see [`Block::Field`]).
    pub fn field(key: impl Into<String>) -> Self {
        Block::Field { key: key.into() }
    }
}

#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct KeyValueRow {
    pub key: String,
    pub value: String,
    /// Dimmed annotation, e.g. "expired 3h ago".
    pub hint: Option<String>,
}

impl KeyValueRow {
    pub fn new(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self { key: key.into(), value: value.into(), hint: None }
    }
    pub fn hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum NoticeLevel {
    Info,
    Success,
    Warning,
    Error,
}

#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Action {
    pub id: String,
    pub label: String,
    pub primary: bool,
    /// The action's own key, as a GPUI keystroke (`"cmd-enter"`,
    /// `"cmd-shift-enter"`, `"alt-k"`). Use it for destructive actions: ↵ is
    /// only ever given to an action *without* a shortcut (the primary one, or
    /// the first), and ⌥2… to the rest. `None`: a key is assigned.
    pub shortcut: Option<String>,
    pub kind: ActionKind,
}

impl Action {
    pub fn new(id: impl Into<String>, label: impl Into<String>, kind: ActionKind) -> Self {
        Self { id: id.into(), label: label.into(), primary: false, shortcut: None, kind }
    }
    pub fn copy(id: impl Into<String>, label: impl Into<String>, text: impl Into<String>) -> Self {
        Self::new(id, label, ActionKind::Copy { text: text.into() })
    }
    pub fn replace_input(id: impl Into<String>, label: impl Into<String>, text: impl Into<String>) -> Self {
        Self::new(id, label, ActionKind::ReplaceInput { text: text.into() })
    }
    pub fn open_url(id: impl Into<String>, label: impl Into<String>, url: impl Into<String>) -> Self {
        Self::new(id, label, ActionKind::OpenUrl { url: url.into() })
    }
    /// Switches to `operation_id` (of `plugin_id`, or this plugin) on the
    /// current input, with `params` merged into its form.
    pub fn run_operation(
        id: impl Into<String>,
        label: impl Into<String>,
        plugin_id: Option<String>,
        operation_id: impl Into<String>,
        params: Params,
    ) -> Self {
        Self::new(id, label, ActionKind::RunOperation { plugin_id, operation_id: operation_id.into(), params })
    }
    /// Runs the operation again on the same input ([`ActionKind::Rerun`]).
    pub fn rerun(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self::new(id, label, ActionKind::Rerun)
    }
    /// An action the plugin handles itself (see [`ActionKind::Custom`]).
    pub fn custom(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self::new(id, label, ActionKind::Custom)
    }
    pub fn open_settings(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self::new(id, label, ActionKind::OpenSettings)
    }
    /// Binds the action to `keystroke` (e.g. `"cmd-enter"`) instead of ↵ / ⌥N.
    pub fn shortcut(mut self, keystroke: impl Into<String>) -> Self {
        self.shortcut = Some(keystroke.into());
        self
    }
    pub fn primary(mut self) -> Self {
        self.primary = true;
        self
    }
}

/// What the host does when an action is triggered. Plugins never touch the
/// clipboard or UI directly — they describe the effect and the host applies it.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum ActionKind {
    Copy { text: String },
    /// Replace the global input (lets tools chain: decode → format …).
    ReplaceInput { text: String },
    OpenUrl { url: String },
    /// Run another operation (optionally of another plugin) on the current input.
    RunOperation {
        plugin_id: Option<String>,
        operation_id: String,
        params: Params,
    },
    /// Open this plugin's page in Settings (e.g. "Add token").
    OpenSettings,
    /// Handled by the plugin: Delight renders the button (and its ↵/⌥
    /// shortcut) and calls [`ToolView::perform`] with the action's id.
    Custom,
    /// Run the operation again with the same input (e.g. "Regenerate").
    Rerun,
}

// ---------------------------------------------------------------------------
// Plugin trait
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum PluginError {
    #[error("unknown operation `{0}`")]
    UnknownOperation(String),
    #[error("{0}")]
    Invalid(String),
    #[error("plugin failed: {0}")]
    Failed(String),
}

/// Implemented by every tool, built-in or loaded from a plugin dylib.
///
/// `detect` and `run` are called on a background thread; the view methods on
/// the main thread.
pub trait Plugin: Send + Sync {
    fn manifest(&self) -> &PluginManifest;

    /// Deterministic detection. The default evaluates the manifest's
    /// declarative [`DetectRule`]s.
    fn detect(&self, input: &Input) -> Vec<Detection> {
        rules::evaluate(self.manifest(), input)
    }

    /// Declarative result for `request`, rendered by the host. Operations with
    /// a [`Plugin::tool_view`] don't need it.
    fn run(&self, request: &RunRequest) -> Result<ToolOutput, PluginError> {
        Err(PluginError::UnknownOperation(request.operation_id.clone()))
    }

    /// Custom GPUI UI for an operation's result pane, replacing [`Plugin::run`].
    /// Created once per operation and kept while the launcher lives.
    fn tool_view(&self, _operation_id: &str, _cx: &mut gpui::App) -> Option<Box<dyn ToolView>> {
        None
    }

    /// Whether the plugin has a Settings page (⚙ buttons in the tool page and
    /// the plugin list). Override when providing only a [`Plugin::settings_view`].
    fn has_settings(&self) -> bool {
        !self.manifest().settings.is_empty()
    }

    /// Custom GPUI UI for the plugin's page in Settings, replacing the form
    /// generated from [`PluginManifest::settings`].
    fn settings_view(&self, _cx: &mut gpui::App) -> Option<Box<dyn SettingsView>> {
        None
    }
}
