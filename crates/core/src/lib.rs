//! Delight core: everything that isn't UI.
//!
//! * [`registry`] — the loaded plugins, each behind [`guard`]'s panic guards.
//! * [`native`] — loads plugin dylibs built against `delight-sdk`.
//! * [`classifier`] — asks the plugins which tools fit an input and ranks them.
//! * [`guard`] — catches plugin panics and turns the plugin off.
//! * [`settings`] — Delight's own preferences.
//! * [`plugin_store`] — what Delight stores for plugins: settings (JSON) and
//!   a data folder each.
//! * [`secrets`] — plugin secrets in the Keychain.
//! * [`input_history`] — the last input, restored on launch, and inputs
//!   plugins remembered, offered as completions while typing.
//! * [`stats`] — size / line / char counts for the status bar.

pub mod classifier;
mod files;
pub mod guard;
pub mod input_history;
pub mod native;
pub mod plugin_store;
pub mod registry;
#[cfg(target_os = "macos")]
pub mod secrets;
pub mod settings;
pub mod stats;
#[cfg(test)]
mod test_support;

pub use classifier::{Candidate, classify};
pub use files::app_dir;
pub use input_history::InputHistory;
pub use plugin_store::PluginStore;
pub use registry::Registry;
pub use settings::Settings;
