//! Delight core: everything that isn't UI.
//!
//! * [`settings`] — Delight's own preferences.
//! * [`input_history`] — the last input, restored on launch, and inputs
//!   plugins remembered, offered as completions while typing.
//! * [`plugin_store`] — what Delight stores for plugins: settings (JSON) and
//!   a data folder each.
//! * [`secrets`] — plugin secrets in the Keychain.
//! * [`stats`] — size / line / char counts for the status bar.

mod files;
pub mod input_history;
pub mod plugin_store;
#[cfg(target_os = "macos")]
pub mod secrets;
pub mod settings;
pub mod stats;

pub use files::app_dir;
pub use input_history::InputHistory;
pub use plugin_store::PluginStore;
pub use settings::Settings;
