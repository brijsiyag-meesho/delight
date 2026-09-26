//! Input → ranked tool candidates. Every enabled plugin's own `detect`
//! decides whether its operations fit and how well; the classifier merges
//! and ranks the answers. There are no host-side rules and no other stages.

use std::collections::{BTreeSet, HashMap};
use std::time::{Duration, Instant};

use delight_sdk::{Input, RECOMMENDED_CONFIDENCE};

use crate::registry::{LoadedPlugin, Registry};

/// A plugin's `detect` slower than this is logged: it runs for every plugin
/// on every keystroke.
const SLOW_DETECT: Duration = Duration::from_millis(20);

/// A tool offered to the user for the current input.
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub plugin_id: String,
    pub plugin_name: String,
    pub operation_id: String,
    pub title: String,
    pub icon_svg: &'static [u8],
    pub confidence: f32,
}

impl Candidate {
    fn new(plugin: &LoadedPlugin, operation_id: &str, title: &str, confidence: f32) -> Self {
        let m = plugin.manifest();
        Self {
            plugin_id: m.id.clone(),
            plugin_name: m.name.clone(),
            operation_id: operation_id.to_string(),
            title: title.to_string(),
            icon_svg: m.icon_svg,
            confidence,
        }
    }

    /// `(plugin id, operation id)`: an operation's identity.
    pub fn key(&self) -> (String, String) {
        (self.plugin_id.clone(), self.operation_id.clone())
    }

    /// Listed under "Recommended" (else "Other Matches"); see
    /// [`delight_sdk::Detection::confidence`].
    pub fn recommended(&self) -> bool {
        self.confidence >= RECOMMENDED_CONFIDENCE
    }
}

/// Every operation that fits `input`, best first; ties keep registry order.
/// Plugins in `disabled` aren't asked. An operation is listed when its plugin
/// detected it with a confidence above zero; unknown operation ids and
/// repeated detections (the best one counts) are dropped.
pub fn classify(input: &Input, registry: &Registry, disabled: &BTreeSet<String>) -> Vec<Candidate> {
    if input.text.trim().is_empty() && input.files.is_empty() {
        return Vec::new();
    }
    // Registry order, for ties: (plugin index, operation index).
    let mut best: HashMap<(usize, usize), Candidate> = HashMap::new();
    for (plugin_index, plugin) in registry.plugins().iter().enumerate() {
        let manifest = plugin.manifest();
        if disabled.contains(&manifest.id) {
            continue;
        }
        let started = Instant::now();
        let detections = plugin.plugin.detect(input);
        let took = started.elapsed();
        if took > SLOW_DETECT {
            log::warn!("{}: detect took {took:?} for {} bytes — keep detect cheap", manifest.id, input.text.len());
        }
        for detection in detections.into_iter().filter(|d| d.confidence > 0.) {
            let Some(op_index) = manifest.operations.iter().position(|op| op.id == detection.operation_id) else {
                log::warn!("{}: detected unknown operation {:?}", manifest.id, detection.operation_id);
                continue;
            };
            let op = &manifest.operations[op_index];
            let candidate = Candidate::new(plugin, &op.id, &op.title, detection.confidence);
            best.entry((plugin_index, op_index))
                .and_modify(|c| {
                    if candidate.confidence > c.confidence {
                        *c = candidate.clone();
                    }
                })
                .or_insert(candidate);
        }
    }
    let mut ranked: Vec<((usize, usize), Candidate)> = best.into_iter().collect();
    ranked.sort_by(|(a_order, a), (b_order, b)| b.confidence.total_cmp(&a.confidence).then(a_order.cmp(b_order)));
    ranked.into_iter().map(|(_, c)| c).collect()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::registry::PluginSource;
    use crate::test_support::TestPlugin;

    fn registry(plugins: Vec<TestPlugin>) -> Registry {
        let mut r = Registry::new();
        for p in plugins {
            r.register(Arc::new(p), PluginSource::Builtin);
        }
        r
    }

    fn ranked(r: &Registry, text: &str, disabled: &[&str]) -> Vec<(String, String, bool)> {
        let disabled = disabled.iter().map(|s| s.to_string()).collect();
        classify(&Input::new(text.to_string()), r, &disabled)
            .into_iter()
            .map(|c| (c.plugin_id.clone(), c.operation_id.clone(), c.recommended()))
            .collect()
    }

    fn row(plugin: &str, op: &str, recommended: bool) -> (String, String, bool) {
        (plugin.into(), op.into(), recommended)
    }

    #[test]
    fn ranks_by_confidence_then_registry_order() {
        let r = registry(vec![
            TestPlugin::with_operations("acme.a", &[("low", 0.2), ("high", 0.9)]),
            TestPlugin::with_operations("acme.b", &[("tie", 0.9)]),
            TestPlugin::with_operations("acme.c", &[("none", 0.0)]),
        ]);
        assert_eq!(
            ranked(&r, "anything", &[]),
            [row("acme.a", "high", true), row("acme.b", "tie", true), row("acme.a", "low", false)],
            "zero confidence isn't listed; below 0.5 is Other Matches"
        );
    }

    #[test]
    fn only_matching_enabled_plugins_for_non_blank_input() {
        let r = registry(vec![
            TestPlugin::new("acme.jwt", "decode").matching("eyJ"),
            TestPlugin::new("acme.any", "encode"),
        ]);
        assert_eq!(ranked(&r, "eyJhbGci", &[]), [row("acme.jwt", "decode", true), row("acme.any", "encode", true)]);
        assert_eq!(ranked(&r, "hello", &[]), [row("acme.any", "encode", true)]);
        assert_eq!(ranked(&r, "eyJhbGci", &["acme.jwt"]), [row("acme.any", "encode", true)], "disabled aren't asked");
        assert!(ranked(&r, "  \n ", &[]).is_empty());
    }
}
