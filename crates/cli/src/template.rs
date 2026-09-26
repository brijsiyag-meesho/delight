//! New plugin repos and plugin crates, made from the template in
//! `crates/cli/template` (a minimal plugin, compiled into the CLI).

use std::path::Path;

use delight_plugin_manager::project;
use toml_edit::{Array, DocumentMut, value};

use crate::report::{Code, Error, Result};

const TEMPLATE_LIB: &str = include_str!("../template/lib.rs");
const TEMPLATE_ICON: &str = include_str!("../template/icon.svg");

/// Crate and folder names: lowercase letters, digits and dashes.
pub fn check_name(name: &str) -> Result<()> {
    let valid = name.starts_with(|c: char| c.is_ascii_lowercase())
        && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if valid {
        Ok(())
    } else {
        Err(Error::new(Code::Usage, format!("{name:?} isn't a valid name")).hint("use lowercase letters, digits and dashes, starting with a letter"))
    }
}

/// A new repo at `dir` with one plugin crate, `plugin`.
pub fn new_repo(dir: &Path, plugin: &str, id: &str, sdk_version: &str) -> Result<()> {
    if dir.exists() {
        return Err(Error::new(Code::Usage, format!("{} already exists", dir.display())).hint("pick a new folder name"));
    }
    std::fs::create_dir_all(dir)?;
    let manifest = format!(
        "# Delight plugins: each member is a plugin crate ([lib] crate-type = [\"dylib\"])\n\
         # or a library they share. Build with `delight build`, install with\n\
         # `delight install`; add a plugin with `delight add <name>`.\n\
         [workspace]\nresolver = \"2\"\nmembers = [\"{plugin}\"]\n"
    );
    std::fs::write(dir.join("Cargo.toml"), manifest)?;
    std::fs::write(dir.join(".gitignore"), "/target\n")?;
    new_plugin(dir, plugin, id, sdk_version)
}

/// A new plugin crate `name` in the repo at `root`, added to its members.
pub fn add_plugin(root: &Path, name: &str, id: &str, sdk_version: &str) -> Result<()> {
    if root.join(name).exists() {
        return Err(Error::new(Code::Usage, format!("{} already exists", root.join(name).display())));
    }
    new_plugin(root, name, id, sdk_version)?;
    let manifest_path = root.join("Cargo.toml");
    let mut manifest: DocumentMut = project::read(&manifest_path)?;
    let mut members: Array = manifest["workspace"]["members"].as_array().cloned().unwrap_or_default();
    members.push(name);
    manifest["workspace"]["members"] = value(members);
    std::fs::write(&manifest_path, manifest.to_string())?;
    Ok(())
}

/// The crate: Cargo.toml, src/lib.rs (the template, renamed), src/icon.svg.
fn new_plugin(root: &Path, name: &str, id: &str, sdk_version: &str) -> Result<()> {
    let title = title(name);
    let dir = root.join(name);
    std::fs::create_dir_all(dir.join("src"))?;
    let manifest = format!(
        "[package]\nname = \"delight-plugin-{name}\"\nversion = \"0.1.0\"\nedition = \"2024\"\npublish = false\n\n\
         [lib]\ncrate-type = [\"dylib\"]\n\n\
         # Delight SDK {sdk_version}. Prefer the SDK's re-exports (delight_sdk::serde_json,\n\
         # delight_sdk::regex, delight_sdk::gpui) to your own copies of crates it has:\n\
         # a different version or feature of one changes the SDK build, and Delight\n\
         # won't load the plugin.\n\
         [dependencies]\ndelight-sdk = \"={sdk_version}\"\ngpui = {{ package = \"delight-gpui\", version = \"={sdk_version}\" }}\n"
    );
    std::fs::write(dir.join("Cargo.toml"), manifest)?;
    let lib = TEMPLATE_LIB
        .replace("PluginManifest::new(\"example.hello\", \"Hello\"", &format!("PluginManifest::new(\"{id}\", \"{title}\""))
        .replace("HelloPlugin", &format!("{}Plugin", title.replace(' ', "")));
    std::fs::write(dir.join("src/lib.rs"), lib)?;
    std::fs::write(dir.join("src/icon.svg"), TEMPLATE_ICON)?;
    Ok(())
}

/// `my-tool` → `My Tool`.
fn title(name: &str) -> String {
    name.split('-')
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut chars = w.chars();
            chars.next().map(|c| c.to_ascii_uppercase().to_string() + chars.as_str()).unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_titles() {
        assert!(check_name("my-tool2").is_ok());
        assert!(check_name("My_Tool").is_err());
        assert!(check_name("2tool").is_err());
        assert_eq!(title("my-tool"), "My Tool");
    }

    #[test]
    fn the_template_is_renamed() {
        assert!(TEMPLATE_LIB.contains("PluginManifest::new(\"example.hello\", \"Hello\""), "the template's anchor moved");
        assert!(TEMPLATE_LIB.contains("HelloPlugin"));
    }
}
