//! A plugin repo: a Cargo workspace whose members are plugin crates (a
//! `dylib` each) and, optionally, library crates they share.

use std::path::{Path, PathBuf};

use toml_edit::{DocumentMut, Item};

use crate::error::{Code, Error, Result};

/// A plugin repo.
pub struct Project {
    pub root: PathBuf,
    pub members: Vec<Member>,
}

/// A crate of the repo.
pub struct Member {
    /// Relative to the repo root, as `members` lists it.
    pub path: String,
    pub package: String,
    /// `package.description`, if any.
    pub description: String,
    /// A plugin (`crate-type = ["dylib"]`), or a library other members use.
    pub plugin: bool,
}

impl Member {
    /// The plugin's file name: `delight-plugin-lucide` → `lucide`.
    pub fn file_stem(&self) -> &str {
        self.package.strip_prefix("delight-plugin-").unwrap_or(&self.package)
    }

    /// Cargo's output file for the plugin.
    pub fn dylib_name(&self) -> String {
        format!("lib{}.dylib", self.package.replace('-', "_"))
    }
}

impl Project {
    /// The repo containing `dir`.
    pub fn find(dir: &Path) -> Result<Self> {
        let root = dir
            .ancestors()
            .find(|d| read(&d.join("Cargo.toml")).is_ok_and(|doc| doc.contains_key("workspace")))
            .ok_or_else(|| {
                Error::new(Code::NotAPluginRepo, format!("{} isn't in a plugin repo (no Cargo.toml with [workspace])", dir.display()))
                    .hint("create one with `delight new <name>`, or add [workspace] members = [\"<plugin crate>\"] to the repo's Cargo.toml")
            })?;
        Self::open(root)
    }

    /// The repo at `root`.
    pub fn open(root: &Path) -> Result<Self> {
        let root = root.to_path_buf();
        let manifest = read(&root.join("Cargo.toml"))?;
        if !manifest.contains_key("workspace") {
            return Err(Error::new(Code::NotAPluginRepo, format!("{} isn't a plugin repo (no [workspace])", root.display())));
        }
        let mut members = Vec::new();
        for path in member_paths(&root, &manifest)? {
            members.push(read_member(&root, path)?);
        }
        if !members.iter().any(|m| m.plugin) {
            return Err(Error::new(Code::NotAPluginRepo, "the repo has no plugin crate")
                .hint("a plugin crate has [lib] crate-type = [\"dylib\"]; add one with `delight add <name>`"));
        }
        Ok(Self { root, members })
    }

    /// The plugins to build: those named, or all of them.
    pub fn plugins(&self, packages: &[String]) -> Result<Vec<&Member>> {
        let plugins: Vec<&Member> = self.members.iter().filter(|m| m.plugin).collect();
        if packages.is_empty() {
            return Ok(plugins);
        }
        packages
            .iter()
            .map(|name| {
                plugins.iter().copied().find(|m| &m.package == name || m.file_stem() == name).ok_or_else(|| {
                    let known: Vec<&str> = plugins.iter().map(|m| m.package.as_str()).collect();
                    Error::new(Code::Usage, format!("no plugin {name:?} in this repo")).hint(format!("plugins: {}", known.join(", ")))
                })
            })
            .collect()
    }
}

pub fn read(path: &Path) -> Result<DocumentMut> {
    let text = std::fs::read_to_string(path).map_err(|e| Error::new(Code::Failed, format!("reading {}: {e}", path.display())))?;
    text.parse().map_err(|e| Error::new(Code::NotAPluginRepo, format!("{} isn't valid TOML: {e}", path.display())))
}

/// `members`, with `dir/*` expanded to the crates in `dir`.
fn member_paths(root: &Path, manifest: &DocumentMut) -> Result<Vec<String>> {
    let members = manifest.get("workspace").and_then(|w| w.get("members")).and_then(Item::as_array);
    let listed: Vec<String> = members.into_iter().flatten().filter_map(|v| v.as_str()).map(String::from).collect();
    let mut paths = Vec::new();
    for entry in listed {
        match entry.strip_suffix("/*") {
            Some(dir) => {
                let mut found: Vec<String> = std::fs::read_dir(root.join(dir))?
                    .flatten()
                    .filter(|e| e.path().join("Cargo.toml").is_file())
                    .map(|e| format!("{dir}/{}", e.file_name().to_string_lossy()))
                    .collect();
                found.sort();
                paths.extend(found);
            }
            None if entry.contains('*') => {
                return Err(Error::new(Code::NotAPluginRepo, format!("members pattern {entry:?} isn't supported"))
                    .hint("list the crates, or use `dir/*`"));
            }
            None => paths.push(entry),
        }
    }
    Ok(paths)
}

fn read_member(root: &Path, path: String) -> Result<Member> {
    let manifest_path = root.join(&path).join("Cargo.toml");
    let manifest = read(&manifest_path)?;
    let not_allowed = |what: &str| {
        Error::new(Code::NotAPluginRepo, format!("{}: {what} is inherited from the repo's workspace", manifest_path.display())).hint(
            "write it out in the crate's own Cargo.toml: plugins are built inside Delight's workspace, where the repo's [workspace.*] tables don't exist",
        )
    };
    let package = manifest.get("package").unwrap_or(&Item::None);
    let name = package.get("name").and_then(Item::as_str).ok_or_else(|| Error::new(Code::NotAPluginRepo, format!("{} has no [package] name", manifest_path.display())))?;
    if let Some(table) = package.as_table_like()
        && let Some((key, _)) = table.iter().find(|(_, v)| is_inherited(v))
    {
        return Err(not_allowed(&format!("`package.{key}`")));
    }
    for section in ["dependencies", "dev-dependencies", "build-dependencies"] {
        if let Some(table) = manifest.get(section).and_then(Item::as_table_like)
            && let Some((key, _)) = table.iter().find(|(_, v)| is_inherited(v))
        {
            return Err(not_allowed(&format!("`{section}.{key}`")));
        }
    }
    let description = package.get("description").and_then(Item::as_str).unwrap_or_default().to_string();
    let crate_types = manifest.get("lib").and_then(|lib| lib.get("crate-type")).and_then(Item::as_array);
    let plugin = crate_types.is_some_and(|types| types.iter().any(|t| t.as_str() == Some("dylib")));
    Ok(Member { path, package: name.to_string(), description, plugin })
}

/// `x.workspace = true`, as a dotted key or inline table.
fn is_inherited(item: &Item) -> bool {
    item.as_table_like().and_then(|t| t.get("workspace")).and_then(Item::as_bool) == Some(true)
}
