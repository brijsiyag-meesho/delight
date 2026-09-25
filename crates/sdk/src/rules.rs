//! Evaluation of declarative [`DetectRule`]s.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use regex::Regex;

use crate::{DetectRule, Detection, Input, PluginManifest};

fn compiled(pattern: &str) -> Option<Regex> {
    static CACHE: OnceLock<Mutex<HashMap<String, Option<Regex>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    let mut cache = cache.lock().ok()?;
    cache
        .entry(pattern.to_owned())
        .or_insert_with(|| Regex::new(pattern).ok())
        .clone()
}

/// Returns the confidence of a single rule, or `None` when it doesn't match.
pub fn rule_confidence(rule: &DetectRule, input: &Input) -> Option<f32> {
    if input.trimmed.is_empty() {
        return None;
    }
    match rule {
        DetectRule::Regex { pattern, confidence } => {
            compiled(pattern).filter(|re| re.is_match(input.trimmed)).map(|_| *confidence)
        }
        DetectRule::Prefix { value, confidence } => {
            input.trimmed.starts_with(value.as_str()).then_some(*confidence)
        }
        DetectRule::Json { confidence } => {
            input.json().filter(|v| v.is_object() || v.is_array()).map(|_| *confidence)
        }
        DetectRule::Always { confidence } => Some(*confidence),
    }
}

/// Best matching rule per operation.
pub fn evaluate(manifest: &PluginManifest, input: &Input) -> Vec<Detection> {
    manifest
        .operations
        .iter()
        .filter_map(|op| {
            op.detect
                .iter()
                .filter_map(|rule| rule_confidence(rule, input))
                .reduce(f32::max)
                .map(|c| Detection::new(op.id.clone(), c).reason("matched manifest rule"))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::OperationSpec;

    fn manifest(rules: Vec<DetectRule>) -> PluginManifest {
        PluginManifest {
            id: "t".into(),
            name: "t".into(),
            version: String::new(),
            description: String::new(),
            author: None,
            icon: None,
            accent: None,
            tags: vec![],
            operations: vec![OperationSpec {
                id: "op".into(),
                title: "Op".into(),
                description: String::new(),
                tags: vec![],
                params: vec![],
                detect: rules,
                run_delay_ms: 0,
                mode: None,
                show_unmatched: false,
            }],
            settings: vec![],
        }
    }

    #[test]
    fn picks_max_matching_rule() {
        let m = manifest(vec![
            DetectRule::Always { confidence: 0.1 },
            DetectRule::Regex { pattern: r"^\d+$".into(), confidence: 0.9 },
        ]);
        assert_eq!(evaluate(&m, &Input::new(" 1234 "))[0].confidence, 0.9);
        assert_eq!(evaluate(&m, &Input::new("abc"))[0].confidence, 0.1);
        assert!(evaluate(&m, &Input::new("   ")).is_empty());
    }
}
