//! Delight core: everything that isn't UI.
//!
//! * [`registry`] — loaded plugins (built-in + plugin dylibs).
//! * [`classifier`] — turns an input into a ranked list of tool [`Candidate`]s.
//!   Deterministic today; the [`classifier::ModelBackend`] seam is where a
//!   learned classifier (e.g. Jev) plugs in.
//! * [`builtin`] — JSON, JWT, Base64, JSON ⇄ env, curl, DNS, ports, YAML ⇄ JSON, IDs.
//! * [`native`] — loads plugin dylibs built against `delight-sdk`.
//! * [`guard`] — panic guards around every plugin call.
//! * [`stats`] — size / line / char counts for the status bar.
//! * [`settings`] — persisted user preferences, incl. per-plugin settings.
//! * [`secrets`] — plugin secrets in the Keychain.

pub mod builtin;
pub mod classifier;
pub mod guard;
pub mod native;
pub mod registry;
pub mod secrets;
pub mod settings;
pub mod stats;
mod util;

pub use classifier::{Candidate, Router};
pub use registry::Registry;
pub use settings::Settings;
