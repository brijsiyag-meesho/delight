//! GPUI, as linked into the shared `delight-sdk` dylib.
//!
//! Import it under the name `gpui` (a Cargo dependency rename): GPUI's macros
//! expand to `gpui::…` paths, and a direct `gpui` dependency could link a
//! second, separate copy of GPUI into a plugin.

pub use delight_sdk::gpui::*;
