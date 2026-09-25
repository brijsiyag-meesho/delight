//! macOS-native look: system colours (HIG), SF Pro / SF Mono, vibrancy.
//!
//! The resolved [`Theme`] is a GPUI global, read with [`ActiveTheme::theme`]
//! (`cx.theme()`) — by reference, never rebuilt per read. It's recomputed
//! only when the user's [`ThemeMode`] or the macOS appearance changes.
//! Plugins receive its [`delight_sdk::Theme`] part ([`Theme::sdk`]), so the
//! palettes can change without breaking a plugin.

mod palette;

use std::ops::Deref;

use gpui::{App, Global, Hsla, SharedString, WindowAppearance};
use serde::{Deserialize, Serialize};

/// Light/dark preference; `System` follows the macOS appearance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ThemeMode {
    #[default]
    System,
    Dark,
    Light,
}

/// The app's theme: what plugins get ([`Theme::sdk`], also reachable through
/// `Deref`), plus what only the app's own windows use.
#[derive(Clone, Debug, PartialEq)]
pub struct Theme {
    pub sdk: delight_sdk::Theme,
    /// Tint painted over the vibrancy (NSVisualEffectView) background.
    pub window_tint: Hsla,
    /// The launcher's input: Lilex (Zed's editor font), bundled.
    pub input_font: SharedString,
}

impl Deref for Theme {
    type Target = delight_sdk::Theme;

    fn deref(&self) -> &delight_sdk::Theme {
        &self.sdk
    }
}

impl Global for Theme {}

/// `cx.theme()`: the current theme, in `Render` and `RenderOnce` alike.
pub trait ActiveTheme {
    fn theme(&self) -> &Theme;
}

impl ActiveTheme for App {
    fn theme(&self) -> &Theme {
        self.global::<Theme>()
    }
}

/// The launcher's input text: Lilex (Zed's editor font) at 13px, with Zed's
/// "comfortable" line-height ratio (1.618 × 13 ≈ 21px).
pub const INPUT_FONT_SIZE: f32 = 13.;
pub const INPUT_LINE_HEIGHT: f32 = 21.;

/// Lilex 2.700 (SIL Open Font License 1.1, `assets/fonts/lilex/OFL.txt`).
const LILEX: &[u8] = include_bytes!("../../assets/fonts/lilex/Lilex-Regular.ttf");

/// What the theme is resolved from.
struct Preference {
    mode: ThemeMode,
    mono_font: SharedString,
    input_font: SharedString,
}

impl Global for Preference {}

/// Loads the bundled font, picks the installed fonts and resolves the theme.
pub(crate) fn init(cx: &mut App, mode: ThemeMode) {
    if let Err(e) = cx.text_system().add_fonts(vec![std::borrow::Cow::Borrowed(LILEX)]) {
        log::warn!("loading the Lilex font failed: {e:#}");
    }
    let names = cx.text_system().all_font_names();
    let first = |fonts: &[&'static str]| -> SharedString {
        fonts.iter().copied().find(|f| names.iter().any(|n| n == f)).unwrap_or("Menlo").into()
    };
    let mono_font = first(&["SF Mono", "Menlo", "Monaco"]);
    let input_font = first(&["Lilex", "SF Mono", "Menlo"]);
    cx.set_global(Preference { mode, mono_font, input_font });
    resolve(cx);
}

pub fn mode(cx: &App) -> ThemeMode {
    cx.global::<Preference>().mode
}

/// Switches Light / Dark / System and redraws.
pub fn set_mode(cx: &mut App, mode: ThemeMode) {
    cx.global_mut::<Preference>().mode = mode;
    resolve(cx);
}

/// Re-resolves after the macOS appearance changed. The app calls it from a
/// window's `observe_window_appearance`.
pub fn appearance_changed(cx: &mut App) {
    resolve(cx);
}

fn resolve(cx: &mut App) {
    let p = cx.global::<Preference>();
    let dark = match p.mode {
        ThemeMode::Dark => true,
        ThemeMode::Light => false,
        ThemeMode::System => matches!(cx.window_appearance(), WindowAppearance::Dark | WindowAppearance::VibrantDark),
    };
    let mut theme = if dark { Theme::dark(p.mono_font.clone()) } else { Theme::light(p.mono_font.clone()) };
    theme.input_font = p.input_font.clone();
    if cx.try_global::<Theme>() != Some(&theme) {
        cx.set_global(theme);
        cx.refresh_windows();
    }
}
