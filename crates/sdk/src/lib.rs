//! # Delight SDK
//!
//! Everything a Delight tool is written against. Built as a Rust `dylib` so
//! the app and every plugin share **one** copy of GPUI (and of this crate) at
//! runtime — that's what lets a plugin's views live inside Delight's windows.
//!
//! A tool implements [`Plugin`]:
//!
//! * [`Plugin::detect`] — *"can you handle this input?"* ([`Detection`]s,
//!   ranked by confidence). Always the tool's own code.
//! * [`Plugin::tool_view`] — the tool's own GPUI view for the result pane: its
//!   options, its output, everything. The host only frames it and draws the
//!   footer [`Action`]s the view lists.
//! * [`Plugin::settings_view`] — the tool's own Settings page, if it has one.
//!   The [`Host`] stores its settings (JSON), data folder and secrets.
//!
//! The host is generic: it never draws tool-specific UI. Tool views draw
//! everything themselves with GPUI, in the colours and fonts of the host's
//! [`Theme`]. The SDK is only this contract — no components — so it can stay
//! frozen; Delight's own UI kit isn't part of it.
//!
//! Plugins outside the app are `dylib` crates that call [`export_plugin!`] and
//! depend on GPUI only through this crate (see the `delight-gpui` alias
//! crate). Delight refuses plugins built against a different SDK build
//! ([`BUILD_ID`]).
//!
//! ## Compatibility
//!
//! Every public struct and enum is `#[non_exhaustive]`: build values with the
//! constructors and builder methods (`OperationSpec::new(..).description(..)`),
//! and give `match`es on SDK enums a `_` arm. SDK releases can then add
//! fields, variants and trait methods (always with a default body) without
//! breaking plugin sources — such a release only needs a rebuild.

mod action;
mod detect;
mod host;
mod identity;
mod manifest;
mod plugin;
mod theme;
mod view;

pub use gpui;
/// Tools that need regular expressions or JSON use these copies (GPUI links
/// them anyway): a plugin's own could change the SDK build, and Delight would
/// refuse the plugin.
pub use regex;
pub use serde_json;

pub use action::*;
pub use detect::*;
pub use host::*;
pub use identity::*;
pub use manifest::*;
pub use plugin::*;
pub use theme::*;
pub use view::*;
