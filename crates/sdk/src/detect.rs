//! Detection: which operations fit the launcher's input, and how well.

use std::path::PathBuf;
use std::sync::Arc;

use gpui::SharedString;

/// Detections at or above this confidence are listed under "Recommended";
/// the rest under "Other Matches".
pub const RECOMMENDED_CONFIDENCE: f32 = 0.5;

/// A plugin's claim that one of its operations applies to the input.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Detection {
    pub operation_id: String,
    /// 0.0 – 1.0, ranking across plugins. Also where the operation is listed:
    /// from [`RECOMMENDED_CONFIDENCE`] up under "Recommended", below it under
    /// "Other Matches" (e.g. a tool that takes any input returns a low value
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
/// ([`crate::ToolContext::input`]): the text typed or pasted, and the files
/// pasted (⌘V on files copied in Finder). One per input change, shared by
/// every plugin: cloning it never copies the text or the paths. Only generic
/// facts; each tool parses the text (as JSON, YAML, …) and reads the files
/// itself.
///
/// Detection runs for every plugin on every input change, so keep
/// [`crate::Plugin::detect`] cheap: reject with an O(1) look at the input's shape
/// (first character, prefix, length; the files' count and extensions) before
/// any full parse — and never read a file there: read it in the tool's view,
/// on the background executor.
#[derive(Debug, Clone, Default, PartialEq)]
#[non_exhaustive]
pub struct Input {
    pub text: SharedString,
    /// The pasted files and folders, in the order they were copied; they
    /// may have been moved or deleted since.
    pub files: Arc<[PathBuf]>,
}

impl Input {
    pub fn new(text: impl Into<SharedString>) -> Self {
        Self { text: text.into(), files: Arc::default() }
    }
    pub fn files(mut self, files: impl Into<Arc<[PathBuf]>>) -> Self {
        self.files = files.into();
        self
    }
}
