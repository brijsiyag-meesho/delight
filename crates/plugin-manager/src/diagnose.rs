//! Why a built plugin doesn't match the app's SDK build: the crates the SDK
//! is built from that the plugins' build changed (another version, or more
//! features). Cargo compiles each crate once per build, with the union of
//! the features the packages being built ask for, so a plugin asking a
//! shared crate for more changes it, and every crate above it (up to GPUI
//! and the SDK).
//!
//! It compares what Cargo builds, not the whole workspace: the SDK alone
//! (which is what the app gets) against the SDK built with the plugins.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::Command;

/// Crate name → version → features, as Cargo resolved one build.
type Resolved = BTreeMap<String, BTreeMap<String, BTreeSet<String>>>;

const SDK: &str = "delight-sdk";

/// One line per crate of the SDK's build that building `packages` with it
/// changed, or `None` if Cargo can't tell (the check still stands).
pub fn changed_crates(kit: &Path, packages: &[&str]) -> Option<Vec<String>> {
    let alone = resolve(kit, &[SDK])?;
    let mut selection = vec![SDK];
    selection.extend_from_slice(packages);
    let with_plugins = resolve(kit, &selection)?;
    let mut changes = Vec::new();
    for (name, sdk_versions) in &alone {
        let Some(plugin_versions) = with_plugins.get(name) else { continue };
        for (version, sdk_features) in sdk_versions {
            match plugin_versions.get(version) {
                Some(features) => {
                    let added: Vec<&str> = features.difference(sdk_features).map(String::as_str).collect();
                    if !added.is_empty() {
                        let what = if added.len() == 1 { "feature" } else { "features" };
                        changes.push(format!("{name} {version} gains {what} {}", quoted(&added)));
                    }
                }
                None => {
                    let others: Vec<&str> =
                        plugin_versions.keys().filter(|v| !sdk_versions.contains_key(*v)).map(String::as_str).collect();
                    if !others.is_empty() {
                        changes.push(format!("{name} {version} becomes {}", others.join(", ")));
                    }
                }
            }
        }
    }
    Some(changes)
}

/// Every crate `cargo build -p <packages>` compiles, with its features:
/// `cargo tree` over the same selection.
fn resolve(kit: &Path, packages: &[&str]) -> Option<Resolved> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let mut command = Command::new(cargo);
    command.args(["tree", "--edges", "normal,build", "--prefix", "none", "--format", "{p} [{f}]"]).current_dir(kit);
    for package in packages {
        command.args(["-p", package]);
    }
    let output = command.output().ok()?;
    if !output.status.success() {
        return None;
    }
    let mut resolved = Resolved::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        if let Some((name, version, features)) = parse_line(line) {
            resolved.entry(name).or_default().entry(version).or_default().extend(features);
        }
    }
    Some(resolved)
}

/// `getrandom v0.2.17 [std]` (maybe with a path, or ` (*)` for a repeat).
fn parse_line(line: &str) -> Option<(String, String, Vec<String>)> {
    let (package, features) = line.rsplit_once(" [")?;
    let features = features.trim_end_matches(" (*)").trim_end_matches(']');
    let mut words = package.split_whitespace();
    let (name, version) = (words.next()?, words.next()?.strip_prefix('v')?);
    let features = features.split(',').map(str::trim).filter(|f| !f.is_empty()).map(String::from).collect();
    Some((name.to_string(), version.to_string(), features))
}

fn quoted(names: &[&str]) -> String {
    names.iter().map(|n| format!("`{n}`")).collect::<Vec<_>>().join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_cargo_tree_lines() {
        assert_eq!(parse_line("getrandom v0.2.17 [std]"), Some(("getrandom".into(), "0.2.17".into(), vec!["std".into()])));
        assert_eq!(
            parse_line("delight-sdk v0.1.0 (/tmp/kit/crates/sdk) [] (*)"),
            Some(("delight-sdk".into(), "0.1.0".into(), vec![]))
        );
        assert_eq!(parse_line("serde v1.0.228 [default,derive,std]").unwrap().2, ["default", "derive", "std"]);
    }
}
