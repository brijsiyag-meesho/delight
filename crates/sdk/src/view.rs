//! Custom GPUI views, the host API plugins call back into, and the dylib
//! entry point.

use std::any::TypeId;
use std::rc::Rc;

use gpui::{AnyView, App, Global, SharedString};

use crate::{Action, Params};

/// The SDK's own version — bumped by every SDK release, independent of the app's.
pub const SDK_VERSION: &str = env!("CARGO_PKG_VERSION");

/// `rustc --version` of the compiler that built the SDK.
pub const RUSTC_VERSION: &str = env!("DELIGHT_RUSTC_VERSION");

/// Hash of the SDK's sources (see `build.rs`).
pub const SOURCE_HASH: &str = env!("DELIGHT_SDK_SOURCE_HASH");

/// `Delight SDK <version> (<rustc version>) src <hash>`. A plugin is only
/// loaded when the id it was built with equals the app's.
pub const BUILD_ID: &str = concat!(
    "Delight SDK ",
    env!("CARGO_PKG_VERSION"),
    " (",
    env!("DELIGHT_RUSTC_VERSION"),
    ") src ",
    env!("DELIGHT_SDK_SOURCE_HASH")
);

/// A type of this crate. Its [`TypeId`] embeds the crate's build identity
/// (Cargo's metadata hash), so a plugin compiled against any other build of
/// the SDK — even from identical sources — reports a different one.
pub struct BuildMarker;

pub const SDK_TYPE_ID: TypeId = TypeId::of::<BuildMarker>();

/// What a [`ToolView`] is showing.
#[derive(Debug, Clone, Default, PartialEq)]
#[non_exhaustive]
pub struct ToolContext {
    pub operation_id: String,
    pub input: String,
    /// The plugin's settings, defaults filled in and secrets resolved.
    pub settings: Params,
}

impl ToolContext {
    pub fn new(operation_id: impl Into<String>, input: impl Into<String>, settings: Params) -> Self {
        Self { operation_id: operation_id.into(), input: input.into(), settings }
    }
}

/// Custom GPUI UI for a tool's result pane.
///
/// Compatibility: a method added to this trait, [`SettingsView`], [`Host`] or
/// [`crate::Plugin`] must have a default body, so plugins keep compiling.
pub trait ToolView {
    fn view(&self) -> AnyView;
    /// Called after creation and whenever the input or the plugin's settings change.
    fn update(&self, context: &ToolContext, cx: &mut App);
    /// Footer actions; the primary one (or the first) is bound to ↵. Delight
    /// renders them; [`crate::ActionKind::Custom`] ones come back to
    /// [`ToolView::perform`].
    fn actions(&self, _cx: &App) -> Vec<Action> {
        Vec::new()
    }
    /// Runs one of this view's [`crate::ActionKind::Custom`] actions.
    fn perform(&self, _action_id: &str, _cx: &mut App) {}
}

/// Custom GPUI UI for a plugin's page in Settings. Read and write values
/// through [`host`] so they are persisted (secrets in the Keychain).
pub trait SettingsView {
    fn view(&self) -> AnyView;
}

/// Services the app provides to plugins.
pub trait Host {
    /// The plugin's settings, defaults filled in and secrets resolved.
    fn settings(&self, plugin_id: &str, cx: &App) -> Params;
    /// Persists one setting. Fields declared `secret` go to the Keychain.
    fn set_setting(&self, plugin_id: &str, key: &str, value: String, cx: &mut App);
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

/// Exports a plugin from a `dylib` crate:
///
/// ```ignore
/// delight_sdk::export_plugin!(MyPlugin::new);
/// ```
#[macro_export]
macro_rules! export_plugin {
    ($constructor:expr) => {
        #[unsafe(no_mangle)]
        pub static DELIGHT_SDK_BUILD: &str = $crate::BUILD_ID;

        #[unsafe(no_mangle)]
        pub static DELIGHT_SDK_TYPE_ID: ::std::any::TypeId = $crate::SDK_TYPE_ID;

        #[unsafe(no_mangle)]
        pub fn delight_plugin() -> ::std::boxed::Box<dyn $crate::Plugin> {
            ::std::boxed::Box::new($constructor())
        }
    };
}
