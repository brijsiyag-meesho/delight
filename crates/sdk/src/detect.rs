//! Detection: which operations fit the launcher's input, and how well.

use gpui::SharedString;

/// Detections at or above this confidence are listed under "Suggested"; the
/// rest under "Other Tools".
pub const SUGGESTED_CONFIDENCE: f32 = 0.5;

/// A plugin's claim that one of its operations applies to the input.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Detection {
    pub operation_id: String,
    /// 0.0 – 1.0, ranking across plugins. Also where the operation is listed:
    /// from [`SUGGESTED_CONFIDENCE`] up under "Suggested", below it under
    /// "Other Tools" (e.g. a tool that takes any input returns a low value
    /// for every non-empty input). Operations without a detection aren't
    /// listed.
    pub confidence: f32,
}

impl Detection {
    pub fn new(operation_id: impl Into<String>, confidence: f32) -> Self {
        Self { operation_id: operation_id.into(), confidence: confidence.clamp(0.0, 1.0) }
    }
}

/// The launcher's input, as handed to [`crate::Plugin::detect`] and to tool views
/// ([`crate::ToolContext::input`]). One per input change, shared by every plugin:
/// cloning it never copies the text. Only generic facts about the text; each
/// tool parses it (as JSON, YAML, …) itself.
///
/// Detection runs for every plugin on every input change, so keep
/// [`crate::Plugin::detect`] cheap: reject with an O(1) look at the input's shape
/// (first character, prefix, length) before any full parse.
#[derive(Debug, Clone, Default, PartialEq)]
#[non_exhaustive]
pub struct Input {
    pub text: SharedString,
}

impl Input {
    pub fn new(text: impl Into<SharedString>) -> Self {
        Self { text: text.into() }
    }
}
