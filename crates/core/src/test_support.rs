//! A configurable plugin for tests.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use delight_sdk::gpui::App;
use delight_sdk::{Detection, Input, OperationSpec, Plugin, PluginManifest, ToolView};

/// How many times a [`TestPlugin`]'s `detect` ran.
#[derive(Clone, Default)]
pub struct Calls(Arc<AtomicUsize>);

impl Calls {
    pub fn get(&self) -> usize {
        self.0.load(Ordering::SeqCst)
    }
}

/// Detects its operations, with their confidences, on every non-empty input
/// (or only inputs starting with a prefix), and can panic on one input.
pub struct TestPlugin {
    manifest: PluginManifest,
    detections: Vec<(String, f32)>,
    prefix: Option<String>,
    panic_on: Option<String>,
    calls: Calls,
}

impl TestPlugin {
    /// One operation, detected at 0.9.
    pub fn new(id: &str, operation: &str) -> Self {
        Self::with_operations(id, &[(operation, 0.9)])
    }

    pub fn with_operations(id: &str, operations: &[(&str, f32)]) -> Self {
        let specs = operations.iter().map(|(op, _)| OperationSpec::new(*op, op.to_uppercase()));
        Self {
            manifest: PluginManifest::new(id, id, b"<svg/>").operations(specs),
            detections: operations.iter().map(|(op, c)| (op.to_string(), *c)).collect(),
            prefix: None,
            panic_on: None,
            calls: Calls::default(),
        }
    }

    /// Detects only inputs starting with `prefix`.
    pub fn matching(mut self, prefix: &str) -> Self {
        self.prefix = Some(prefix.into());
        self
    }

    /// Panics (index out of bounds) when the input is `text`.
    pub fn panicking_on(mut self, text: &str) -> Self {
        self.panic_on = Some(text.into());
        self
    }

    pub fn calls(&self) -> Calls {
        self.calls.clone()
    }
}

impl Plugin for TestPlugin {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    fn detect(&self, input: &Input) -> Vec<Detection> {
        self.calls.0.fetch_add(1, Ordering::SeqCst);
        if self.panic_on.as_deref() == Some(&*input.text) {
            let empty: Vec<u8> = Vec::new();
            let _ = empty[3];
        }
        if self.prefix.as_ref().is_some_and(|p| !input.text.starts_with(p.as_str())) {
            return Vec::new();
        }
        self.detections.iter().map(|(op, c)| Detection::new(op.clone(), *c)).collect()
    }

    fn tool_view(&self, _: &str, _: &mut App) -> Box<dyn ToolView> {
        unreachable!("tests don't open views")
    }
}
