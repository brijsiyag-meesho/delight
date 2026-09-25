//! Delight's own UI kit, used only by the app. Plugins never depend on it:
//! they get the colours and fonts ([`delight_sdk::Theme`]) through the host,
//! so everything here can change without breaking a plugin.
//!
//! Components are builders rendered with `RenderOnce`, reading the theme with
//! `cx.theme()`; controls are controlled (they report the new value, the
//! caller owns it). See `docs/ui-components.md` for the design notes.
//!
//! Call [`init`] once at startup and install [`Assets`] as the app's asset
//! source.

mod assets;
mod badge;
mod button;
pub mod editor;
mod group;
mod icon;
mod keycap;
mod segmented;
mod styled;
mod switch;
pub mod theme;

pub use assets::{Assets, IconName, LOGO_SVG};
pub use badge::LogoBadge;
pub use button::{Button, ButtonVariant, IconButton};
pub use editor::{EditorEvent, EditorFont, TextEditor};
pub use group::{Caption, Divider, Group};
pub use icon::Icon;
pub use keycap::{Keycap, KeycapStyle};
pub use segmented::SegmentedControl;
pub use styled::{Disableable, Selectable, Sizable, Size, StyledExt, h_flex, v_flex};
pub use switch::Switch;
pub use theme::{ActiveTheme, Theme, ThemeMode};

/// Resolves the theme (loading the bundled font) and binds the editor's keys.
pub fn init(cx: &mut gpui::App, mode: ThemeMode) {
    theme::init(cx, mode);
    editor::init(cx);
}
