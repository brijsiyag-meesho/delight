//! Services the app provides to plugins, reached with [`host`].

use std::path::PathBuf;
use std::rc::Rc;

use gpui::{App, Global, SharedString, Window};
use serde_json::Value;

use crate::Theme;

/// Services the app provides to plugins: storage and a few app-level
/// effects. The host never interprets what a plugin stores.
///
/// Where a plugin keeps what:
/// * [`Host::settings`] — its preferences, one JSON value in Delight's
///   global settings store. Keep it small: it's rewritten on every save.
/// * [`Host::data_dir`] — everything else (history, caches, databases), in
///   whatever format the plugin chooses.
/// * [`Host::secret`] — tokens and passwords, in the Keychain.
pub trait Host {
    /// The plugin's settings: the JSON value it last saved, or
    /// [`Value::Null`] if it never saved any.
    fn settings(&self, plugin_id: &str, cx: &App) -> Value;
    /// Replaces the plugin's settings with `value` and persists them.
    /// [`Value::Null`] removes them.
    fn set_settings(&self, plugin_id: &str, value: Value, cx: &mut App);
    /// The plugin's own folder for any other data, created on first use and
    /// deleted with the plugin.
    fn data_dir(&self, plugin_id: &str, cx: &App) -> PathBuf;
    /// A secret from the Keychain (e.g. an API token); `None` if unset.
    fn secret(&self, plugin_id: &str, key: &str, cx: &App) -> Option<String>;
    /// Stores a secret in the Keychain; an empty value removes it.
    fn set_secret(&self, plugin_id: &str, key: &str, value: String, cx: &mut App);
    /// The colours and fonts to draw `window` with. Fetch it in `render`: it
    /// follows the appearance, and Delight redraws when that changes.
    fn theme(&self, window: &Window, cx: &App) -> Theme;
    /// Brief confirmation in the launcher's status bar.
    fn toast(&self, message: SharedString, cx: &mut App);
    fn open_settings(&self, plugin_id: &str, cx: &mut App);
}

/// The app installs its [`Host`] as a GPUI global at startup.
pub struct HostHandle(pub Rc<dyn Host>);

impl Global for HostHandle {}

pub fn host(cx: &App) -> Rc<dyn Host> {
    cx.global::<HostHandle>().0.clone()
}
