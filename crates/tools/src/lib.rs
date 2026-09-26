//! Delight's built-in tools. Each is an ordinary [`Plugin`]: the app lists
//! and runs them like installed plugins (an installed plugin with the same
//! id replaces one). Unlike plugins, they're drawn with Delight's own UI kit
//! (`delight-ui`), whose widgets and syntax highlighting plugins don't get.
//!
//! One folder per tool, each on its own (no code shared between tools), split
//! the same way: `mod.rs` is the plugin (manifest and detection), `view.rs`
//! its view, and `convert.rs` (or `render.rs`) the work itself, without UI.
//! Next to them, `icon.svg`.
//!
//! * `json` — format, minify, escape and unescape JSON.
//! * `yaml` — YAML ⇄ JSON.
//! * `svg` — preview SVG images; copy them as PNG or a data URI.

mod json;
mod svg;
mod yaml;

use std::sync::Arc;

use delight_sdk::{Plugin, PluginManifest};

/// Every built-in tool.
pub fn all() -> Vec<Arc<dyn Plugin>> {
    vec![Arc::new(json::JsonPlugin::new()), Arc::new(yaml::YamlPlugin::new()), Arc::new(svg::SvgPlugin::new())]
}

/// A built-in tool's manifest: versioned and authored as Delight.
fn manifest(id: &str, name: &str, icon_svg: &'static [u8]) -> PluginManifest {
    PluginManifest::new(id, name, icon_svg).version(env!("CARGO_PKG_VERSION")).author("Delight")
}
