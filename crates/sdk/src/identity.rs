//! The SDK build a plugin is compiled against, and the entry point that
//! exports it. Plugins are called through Rust's unstable ABI, so Delight
//! loads only plugins built by the same compiler against the same SDK build:
//! the loader compares a plugin's `DELIGHT_SDK_BUILD` and
//! `DELIGHT_SDK_TYPE_ID` with [`BUILD_ID`] and [`SDK_TYPE_ID`] before calling
//! into it.

use std::any::TypeId;

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
