//! Built-in tools. Each is an ordinary [`Plugin`] — the host treats them
//! exactly like plugin dylibs.

use std::sync::Arc;

use delight_sdk::{FieldKind, FormField, OperationSpec, Plugin, PluginManifest};

pub mod base64;
pub mod curl;
pub mod dns;
pub mod env;
pub mod id;
pub mod json;
pub mod jwt;
pub mod port;
pub mod yaml;

pub fn all() -> Vec<Arc<dyn Plugin>> {
    vec![
        Arc::new(jwt::JwtPlugin::new()),
        Arc::new(json::JsonPlugin::new()),
        Arc::new(base64::Base64Plugin::new()),
        Arc::new(env::EnvPlugin::new()),
        Arc::new(curl::CurlPlugin::new()),
        Arc::new(dns::DnsPlugin::new()),
        Arc::new(port::PortPlugin::new()),
        Arc::new(yaml::YamlPlugin::new()),
        Arc::new(id::IdPlugin::new()),
    ]
}

pub(crate) fn manifest(
    id: &str,
    name: &str,
    description: &str,
    icon: &str,
    accent: &str,
    tags: &[&str],
    operations: Vec<OperationSpec>,
) -> PluginManifest {
    PluginManifest::new(id, name)
        .version(env!("CARGO_PKG_VERSION"))
        .description(description)
        .author("Delight")
        .icon(icon)
        .accent(accent)
        .tags(tags.iter().copied())
        .operations(operations)
}

pub(crate) fn op(id: &str, title: &str, description: &str, tags: &[&str], params: Vec<FormField>) -> OperationSpec {
    OperationSpec::new(id, title).description(description).tags(tags.iter().copied()).params(params)
}

pub(crate) fn field(key: &str, label: &str, kind: FieldKind, default: Option<&str>) -> FormField {
    let field = FormField::new(key, label, kind);
    match default {
        Some(value) => field.default(value),
        None => field,
    }
}

pub(crate) fn select(key: &str, label: &str, options: &[(&str, &str)], default: &str) -> FormField {
    FormField::select(key, label, options.iter().copied()).default(default)
}

pub(crate) fn toggle(key: &str, label: &str, default: bool) -> FormField {
    FormField::toggle(key, label, default)
}

/// Value of `key`, falling back to the field default declared in the manifest.
pub(crate) fn param<'a>(
    manifest: &'a PluginManifest,
    req: &'a delight_sdk::RunRequest,
    key: &str,
) -> &'a str {
    if let Some(v) = req.params.get(key) {
        return v;
    }
    manifest
        .operation(&req.operation_id)
        .and_then(|op| op.params.iter().find(|f| f.key == key))
        .and_then(|f| f.default.as_deref())
        .unwrap_or("")
}

pub(crate) fn flag(manifest: &PluginManifest, req: &delight_sdk::RunRequest, key: &str) -> bool {
    matches!(param(manifest, req, key), "true" | "1" | "yes")
}
