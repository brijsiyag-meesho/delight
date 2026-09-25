//! Delight's built-in tools. Each is an ordinary [`Plugin`]: the app lists
//! and runs them like installed plugins (an installed plugin with the same
//! id replaces one). Unlike plugins, they're drawn with Delight's own UI kit
//! (`delight-ui`), whose widgets and syntax highlighting plugins don't get.
//!
//! * `output` — what the converting tools share: their result and actions.
//! * `json` — format, minify, escape and unescape JSON.
//! * `yaml` — YAML ⇄ JSON.

mod json;
mod output;
mod yaml;

use std::sync::Arc;

use delight_sdk::{Plugin, PluginManifest};

/// Every built-in tool.
pub fn all() -> Vec<Arc<dyn Plugin>> {
    vec![Arc::new(json::JsonPlugin::new()), Arc::new(yaml::YamlPlugin::new())]
}

/// A built-in tool's manifest: versioned and authored as Delight.
fn manifest(id: &str, name: &str, icon_svg: &'static [u8]) -> PluginManifest {
    PluginManifest::new(id, name, icon_svg).version(env!("CARGO_PKG_VERSION")).author("Delight")
}
