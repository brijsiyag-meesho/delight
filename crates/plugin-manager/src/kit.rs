//! Building plugins so Delight loads them. A plugin must be compiled
//! against the exact SDK build of the app (Rust has no stable ABI), and
//! Cargo's build identity depends on the whole workspace: its Cargo.lock,
//! features, flags and toolchain. So plugins are built inside the SDK kit:
//! a workspace the app writes itself (`--write-sdk-kit`) holding the SDK's
//! crates and those files, with the repo's crates linked in as members.
//!
//! One kit, `<cache>/kit`, rewritten for whichever Delight is built for,
//! with its build output in `<cache>/target`: a new SDK recompiles only what
//! changed. (A kit per SDK build sharing that output would not work: Cargo
//! would take one kit's SDK as already built for another.)

use std::io::{BufRead as _, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::Context as _;
use toml_edit::{Array, Item, value};

use crate::Env;
use crate::error::{Code, Error, Result};
use crate::project::{Member, Project};
use crate::target;

/// Where kits and build output are kept: `DELIGHT_CACHE`, else
/// `~/Library/Caches/Delight`.
pub fn cache_root() -> PathBuf {
    std::env::var_os("DELIGHT_CACHE")
        .map(PathBuf::from)
        .unwrap_or_else(|| dirs::cache_dir().unwrap_or_else(std::env::temp_dir).join("Delight"))
}

pub fn kit_dir() -> PathBuf {
    cache_root().join("kit")
}

/// A built plugin, as the app read it.
pub struct Built<'a> {
    pub member: &'a Member,
    /// Cargo's output in the kit.
    pub dylib: PathBuf,
    pub id: String,
    pub version: String,
}

/// Builds `plugins` of `project` (linked into the kit as `plugins/<name>`)
/// for the Delight of `env`, and checks it loads each.
pub fn build<'a>(env: &Env, project: &Project, name: &str, plugins: &[&'a Member]) -> Result<Vec<Built<'a>>> {
    let kit = kit_dir();
    env.target.write_sdk_kit(&kit)?;
    link_project(project, name, &kit)?;

    let target_dir = cache_root().join("target");
    let profile = env.info.profile.as_str();
    if !target_dir.join(profile).is_dir() {
        env.progress("the first build compiles GPUI: a few minutes");
    }
    let names: Vec<&str> = plugins.iter().map(|m| m.package.as_str()).collect();
    env.progress(&format!("building {}", names.join(", ")));
    let mut cargo = Command::new(target::rust_tool("cargo"));
    cargo.arg("build").current_dir(&kit).env("CARGO_TARGET_DIR", &target_dir);
    if profile == "release" {
        cargo.arg("--release");
    }
    for name in &names {
        cargo.args(["-p", name]);
    }
    // Cargo's output (on stderr) is progress, and the log of a build that fails.
    let mut child = cargo.stdout(Stdio::null()).stderr(Stdio::piped()).spawn().map_err(|e| {
        Error::new(Code::NoRust, format!("cargo: {e}")).hint("install Rust from https://rustup.rs")
    })?;
    let mut output = String::new();
    for line in BufReader::new(child.stderr.take().context("cargo's output")?).lines().map_while(std::result::Result::ok) {
        env.progress(&line);
        output += &line;
        output.push('\n');
    }
    if !child.wait()?.success() {
        let log = cache_root().join("logs").join(format!("{name}.log"));
        std::fs::create_dir_all(log.parent().unwrap_or(&log))?;
        std::fs::write(&log, &output)?;
        let first_error = output.lines().find(|l| l.starts_with("error")).unwrap_or("see the log");
        let message = format!("cargo build failed for {}: {first_error}", names.join(", "));
        return Err(Error::new(Code::BuildFailed, message).log(log));
    }

    plugins
        .iter()
        .map(|member| {
            let dylib = target_dir.join(profile).join(member.dylib_name());
            match env.target.check(&dylib)? {
                Ok((id, version)) => Ok(Built { member, dylib, id, version }),
                Err(why) => Err(mismatch(&member.package, &why, &kit, &names)),
            }
        })
        .collect()
}

/// Delight won't load the plugin: say which crates the build changed.
fn mismatch(package: &str, why: &str, kit: &Path, built: &[&str]) -> Error {
    let error = Error::new(Code::SdkMismatch, format!("Delight won't load {package}: {why}"));
    let fix = "for a crate Delight already builds, use its exact version with `default-features = false` and only features it already has (or the SDK's re-export: delight_sdk::serde_json, delight_sdk::regex, delight_sdk::gpui); another major version is fine";
    match crate::diagnose::changed_crates(kit, built) {
        Some(changes) if !changes.is_empty() => error.hint(format!("the plugin's dependencies changed crates Delight's SDK is built from: {}. Fix: {fix}", changes.join("; "))),
        _ => error.hint(format!("a dependency may have changed the SDK build. Fix: {fix}")),
    }
}

/// Copies a file to `to`, replacing it. Never overwritten in place: macOS
/// caches a loaded dylib's code signature per file, and a process that loads
/// the rewritten file gets killed. A new file (new inode) is safe.
pub fn replace_file(from: &Path, to: &Path) -> Result<()> {
    if let Some(dir) = to.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let _ = std::fs::remove_file(to);
    std::fs::copy(from, to).with_context(|| format!("copying {} to {}", from.display(), to.display()))?;
    Ok(())
}

/// Links each crate of the repo into `<kit>/plugins/<name>/<its path>` (the
/// repo's own layout, so `path = "../common"` still resolves), and makes the
/// kit's workspace the SDK's crates plus those.
fn link_project(project: &Project, name: &str, kit: &Path) -> Result<()> {
    let plugins = kit.join("plugins");
    let _ = std::fs::remove_dir_all(&plugins);
    let mut members: Array = ["crates/sdk", "crates/gpui-alias"].into_iter().collect();
    for member in &project.members {
        let link = plugins.join(name).join(&member.path);
        if let Some(dir) = link.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::os::unix::fs::symlink(project.root.join(&member.path), &link)?;
        members.push(format!("plugins/{name}/{}", member.path));
    }
    let manifest_path = kit.join("Cargo.toml");
    let mut manifest = crate::project::read(&manifest_path)?;
    let workspace = manifest["workspace"].as_table_mut().context("the kit's Cargo.toml has no [workspace]")?;
    workspace["members"] = value(members);
    workspace.remove("exclude");
    // The app's own crates aren't in the kit.
    if let Some(dependencies) = workspace.get_mut("dependencies").and_then(Item::as_table_mut) {
        dependencies.retain(|_, dependency| {
            dependency.get("path").and_then(Item::as_str).is_none_or(|path| kit.join(path).is_dir())
        });
    }
    std::fs::write(&manifest_path, manifest.to_string())?;
    Ok(())
}
