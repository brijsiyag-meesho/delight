//! Input → ranked tool candidates.
//!
//! The [`Router`] runs a list of [`Classifier`] stages and merges their
//! scores per `(plugin, operation)`. Today the only default stage is the
//! deterministic [`RuleClassifier`] (each plugin's own `detect`). A learned
//! model — e.g. Jev from TypeSafe AI — is added by implementing
//! [`ModelBackend`] and pushing a [`ModelClassifier`] stage:
//!
//! ```ignore
//! let router = Router::deterministic()
//!     .with_stage(ModelClassifier::new(Arc::new(JevBackend::connect(..)?)), 0.8);
//! ```
//!
//! The model only ever sees operation *labels* (title, description, tags from
//! the manifest), so new plugins become classifiable without retraining the
//! host — the schema is the contract.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use delight_sdk::{Detection, Input, OperationSpec};

use crate::registry::{LoadedPlugin, Registry};

/// A tool offered to the user for the current input.
#[derive(Debug, Clone)]
pub struct Candidate {
    pub plugin_id: String,
    pub plugin_name: String,
    pub operation_id: String,
    pub title: String,
    pub icon: String,
    pub accent: Option<String>,
    pub confidence: f32,
    pub reason: Option<String>,
    pub preview: Option<String>,
    /// The mode (tab) the plugin suggests for this input (`Detection::mode`).
    pub mode: Option<String>,
    /// Which classifier stage produced the winning score.
    pub source: &'static str,
}

impl Candidate {
    fn new(plugin: &LoadedPlugin, op: &OperationSpec, confidence: f32, source: &'static str) -> Self {
        let m = plugin.manifest();
        Self {
            plugin_id: m.id.clone(),
            plugin_name: m.name.clone(),
            operation_id: op.id.clone(),
            title: op.title.clone(),
            icon: m.icon.clone().unwrap_or_else(|| m.name.chars().take(2).collect()),
            accent: m.accent.clone(),
            confidence,
            reason: None,
            preview: None,
            mode: None,
            source,
        }
    }

    pub fn key(&self) -> (String, String) {
        (self.plugin_id.clone(), self.operation_id.clone())
    }
}

/// Enabled operations not in `shown` that take any input
/// ([`OperationSpec::show_unmatched`]), in registry order — listed below the
/// suggestions. Tools that recognise their input format and didn't match are
/// left out.
pub fn remaining(registry: &Registry, disabled: &HashSet<String>, shown: &[Candidate]) -> Vec<Candidate> {
    registry
        .plugins()
        .iter()
        .filter(|p| !disabled.contains(&p.manifest().id))
        .flat_map(|p| p.manifest().operations.iter().map(move |op| (p, op)))
        .filter(|(_, op)| op.show_unmatched)
        .filter(|(p, op)| !shown.iter().any(|c| c.plugin_id == p.manifest().id && c.operation_id == op.id))
        .map(|(p, op)| Candidate::new(p, op, 0.0, "none"))
        .collect()
}

/// A detection tagged with the plugin that produced it.
#[derive(Debug, Clone)]
pub struct Scored {
    pub plugin_id: String,
    pub detection: Detection,
}

pub trait Classifier: Send + Sync {
    fn id(&self) -> &'static str;
    fn classify(&self, input: &Input, registry: &Registry) -> Vec<Scored>;
}

/// A plugin's `detect` slower than this is logged as a warning.
const SLOW_DETECT: std::time::Duration = std::time::Duration::from_millis(20);

/// Deterministic stage: asks every plugin's [`delight_sdk::Plugin::detect`].
pub struct RuleClassifier;

impl Classifier for RuleClassifier {
    fn id(&self) -> &'static str {
        "rules"
    }

    fn classify(&self, input: &Input, registry: &Registry) -> Vec<Scored> {
        registry
            .plugins()
            .iter()
            .flat_map(|p| {
                let plugin_id = p.manifest().id.clone();
                let started = std::time::Instant::now();
                let detections = p.plugin.detect(input);
                let took = started.elapsed();
                // Detection runs for every plugin on every keystroke.
                if took > SLOW_DETECT {
                    log::warn!("{plugin_id}: detect took {took:?} for {} bytes — keep detect cheap", input.text.len());
                }
                detections.into_iter().map(move |detection| Scored { plugin_id: plugin_id.clone(), detection })
            })
            .collect()
    }
}

/// What a model classifier scores the input against.
#[derive(Debug, Clone)]
pub struct Label {
    pub plugin_id: String,
    pub operation_id: String,
    /// Natural-language label built from the manifest.
    pub text: String,
}

/// Seam for a learned classifier (Jev or anything else). Implementations
/// return one probability per label, in order.
pub trait ModelBackend: Send + Sync {
    fn name(&self) -> &'static str;
    fn score(&self, input: &str, labels: &[Label]) -> anyhow::Result<Vec<f32>>;
}

pub struct ModelClassifier {
    backend: Arc<dyn ModelBackend>,
    /// Scores below this are dropped so the model can't flood the list.
    pub min_score: f32,
}

impl ModelClassifier {
    pub fn new(backend: Arc<dyn ModelBackend>) -> Self {
        Self { backend, min_score: 0.3 }
    }

    pub fn labels(registry: &Registry) -> Vec<Label> {
        registry
            .plugins()
            .iter()
            .flat_map(|p| {
                let m = p.manifest();
                m.operations.iter().map(move |op| {
                    let mut tags = m.tags.clone();
                    tags.extend(op.tags.iter().cloned());
                    Label {
                        plugin_id: m.id.clone(),
                        operation_id: op.id.clone(),
                        text: format!("{}: {} [{}]", op.title, op.description, tags.join(", ")),
                    }
                })
            })
            .collect()
    }
}

impl Classifier for ModelClassifier {
    fn id(&self) -> &'static str {
        self.backend.name()
    }

    fn classify(&self, input: &Input, registry: &Registry) -> Vec<Scored> {
        let labels = Self::labels(registry);
        match self.backend.score(input.text, &labels) {
            Ok(scores) => labels
                .into_iter()
                .zip(scores)
                .filter(|(_, s)| *s >= self.min_score)
                .map(|(l, s)| Scored {
                    plugin_id: l.plugin_id,
                    detection: Detection::new(l.operation_id, s).reason(format!("{} model", self.backend.name())),
                })
                .collect(),
            Err(err) => {
                log::warn!("classifier {} failed: {err:#}", self.backend.name());
                Vec::new()
            }
        }
    }
}

struct Stage {
    classifier: Box<dyn Classifier>,
    weight: f32,
}

pub struct Router {
    stages: Vec<Stage>,
    /// Candidates below this confidence are hidden.
    pub min_confidence: f32,
    pub max_results: usize,
}

impl Router {
    pub fn empty() -> Self {
        Self { stages: Vec::new(), min_confidence: 0.05, max_results: 12 }
    }

    pub fn deterministic() -> Self {
        Self::empty().with_stage(RuleClassifier, 1.0)
    }

    pub fn with_stage(mut self, classifier: impl Classifier + 'static, weight: f32) -> Self {
        self.stages.push(Stage { classifier: Box::new(classifier), weight });
        self
    }

    /// Rank every applicable operation for `text`. Plugins whose ids are in
    /// `disabled` are skipped.
    pub fn classify(&self, text: &str, registry: &Registry, disabled: &HashSet<String>) -> Vec<Candidate> {
        let input = Input::new(text);
        if input.trimmed.is_empty() {
            return Vec::new();
        }

        // (plugin, op) -> (score, detection, stage)
        let mut best: HashMap<(String, String), (f32, Detection, &'static str)> = HashMap::new();
        for stage in &self.stages {
            for scored in stage.classifier.classify(&input, registry) {
                if disabled.contains(&scored.plugin_id) {
                    continue;
                }
                let score = scored.detection.confidence * stage.weight;
                let key = (scored.plugin_id, scored.detection.operation_id.clone());
                match best.get_mut(&key) {
                    Some(entry) if entry.0 >= score => {
                        // Keep the higher score but borrow any preview the winner lacks.
                        if entry.1.preview.is_none() {
                            entry.1.preview = scored.detection.preview;
                        }
                    }
                    Some(entry) => {
                        let mut detection = scored.detection;
                        detection.preview = detection.preview.or(entry.1.preview.take());
                        detection.mode = detection.mode.or(entry.1.mode.take());
                        *entry = (score, detection, stage.classifier.id());
                    }
                    None => {
                        best.insert(key, (score, scored.detection, stage.classifier.id()));
                    }
                }
            }
        }

        let order: HashMap<(&str, &str), usize> = registry
            .plugins()
            .iter()
            .flat_map(|p| p.manifest().operations.iter().map(move |op| (p.manifest().id.as_str(), op.id.as_str())))
            .enumerate()
            .map(|(i, k)| (k, i))
            .collect();

        let mut out: Vec<(usize, Candidate)> = best
            .into_iter()
            .filter(|(_, (score, _, _))| *score >= self.min_confidence)
            .filter_map(|((plugin_id, op_id), (score, det, source))| {
                let (p, op) = registry.operation(&plugin_id, &op_id)?;
                let rank = order.get(&(plugin_id.as_str(), op_id.as_str())).copied().unwrap_or(usize::MAX);
                let candidate = Candidate {
                    reason: det.reason,
                    preview: det.preview,
                    mode: det.mode,
                    ..Candidate::new(p, op, score.clamp(0.0, 1.0), source)
                };
                Some((rank, candidate))
            })
            .collect();

        out.sort_by(|(ra, a), (rb, b)| b.confidence.total_cmp(&a.confidence).then(ra.cmp(rb)));
        out.into_iter().map(|(_, c)| c).take(self.max_results).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn top(text: &str) -> Vec<(String, String)> {
        let r = Registry::with_builtins();
        Router::deterministic()
            .classify(text, &r, &HashSet::new())
            .into_iter()
            .map(|c| (c.plugin_id, c.operation_id))
            .collect()
    }

    fn first(text: &str) -> (String, String) {
        top(text).into_iter().next().expect("no candidates")
    }

    #[test]
    fn routes_common_inputs() {
        assert_eq!(first(r#"{"a": 1, "b": [1,2]}"#), ("delight.json".into(), "json".into()));
        assert_eq!(first("aGVsbG8gd29ybGQsIHRoaXMgaXMgYmFzZTY0"), ("delight.base64".into(), "decode".into()));
        let jwt = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkpvaG4gRG9lIiwiaWF0IjoxNTE2MjM5MDIyfQ.SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c";
        assert_eq!(first(jwt), ("delight.jwt".into(), "decode".into()));
        assert_eq!(first("DB_HOST=localhost\nDB_PORT=5432\n# c\nexport TOKEN=\"x y\""), ("delight.env".into(), "env_to_json".into()));
        assert!(top("").is_empty());
    }

    #[test]
    fn plain_text_offers_encoders_only() {
        let ops = top("hello there");
        assert!(ops.contains(&("delight.base64".into(), "encode".into())));
        assert!(!ops.contains(&("delight.json".into(), "json".into())));
    }

    struct Fake;
    impl ModelBackend for Fake {
        fn name(&self) -> &'static str {
            "fake"
        }
        fn score(&self, _: &str, labels: &[Label]) -> anyhow::Result<Vec<f32>> {
            Ok(labels.iter().map(|l| if l.operation_id == "json_to_env" { 0.99 } else { 0.0 }).collect())
        }
    }

    #[test]
    fn model_stage_can_rerank() {
        let r = Registry::with_builtins();
        let router = Router::deterministic().with_stage(ModelClassifier::new(Arc::new(Fake)), 1.0);
        let c = router.classify(r#"{"a":1}"#, &r, &HashSet::new());
        assert_eq!(c[0].operation_id, "json_to_env");
        assert_eq!(c[0].source, "fake");
        // Rule preview is kept even though the model won.
        assert!(c[0].preview.is_some());
    }

    #[test]
    fn remaining_lists_only_tools_that_take_any_input() {
        let mut r = Registry::with_builtins();
        // Built-ins recognise their input: none is listed when it doesn't match.
        let matched = Router::deterministic().classify("hello there", &r, &HashSet::new());
        assert!(remaining(&r, &HashSet::new(), &matched).is_empty());

        struct AnyInput(delight_sdk::PluginManifest);
        impl delight_sdk::Plugin for AnyInput {
            fn manifest(&self) -> &delight_sdk::PluginManifest {
                &self.0
            }
        }
        let mut manifest = delight_sdk::Plugin::manifest(&crate::builtin::json::JsonPlugin::new()).clone();
        manifest.id = "test.any".into();
        manifest.operations[0].show_unmatched = true;
        manifest.operations[0].detect.clear();
        r.register(Arc::new(AnyInput(manifest)), crate::registry::PluginSource::Builtin);
        let rest = remaining(&r, &HashSet::new(), &matched);
        assert_eq!(rest.iter().map(|c| c.plugin_id.as_str()).collect::<Vec<_>>(), ["test.any"]);
    }

    /// Detection runs for every plugin on every keystroke: guard against a
    /// detector doing heavy work on large inputs it can't match. The bound is
    /// generous (debug builds, shared CI machines); the point is catching
    /// pathological cases, e.g. an O(n²) scan or repeated full parses.
    #[test]
    fn detection_stays_cheap_on_large_inputs() {
        let r = Registry::with_builtins();
        let big_json = format!("[{}]", vec![r#"{"id": 1, "name": "x", "tags": ["a", "b"]}"#; 25_000].join(","));
        let big_text = "lorem ipsum dolor sit amet, consectetur adipiscing elit\n".repeat(20_000);
        let big_yaml = "item:\n  name: x\n  tags:\n    - a\n".repeat(30_000);
        let big_log = "2026-09-25T10:00:00Z INFO request_id=abc path=/v1/x status=200 took=12ms\n".repeat(15_000);
        for (name, input) in [("json", &big_json), ("text", &big_text), ("yaml", &big_yaml), ("log", &big_log)] {
            assert!(input.len() > 900_000, "{name} input too small");
            let input = Input::new(input);
            for p in r.plugins() {
                let started = std::time::Instant::now();
                let _ = p.plugin.detect(&input);
                let took = started.elapsed();
                println!("{name:>4} {:>16} {took:?}", p.manifest().id);
                assert!(took < std::time::Duration::from_millis(1500), "{} took {took:?} on 1 MB of {name}", p.manifest().id);
            }
        }
    }

    #[test]
    fn disabled_plugins_are_hidden() {
        let r = Registry::with_builtins();
        let disabled = HashSet::from(["delight.json".to_string()]);
        let c = Router::deterministic().classify(r#"{"a":1}"#, &r, &disabled);
        assert!(c.iter().all(|c| c.plugin_id != "delight.json"));
    }
}
