//! macOS-native look: system colours (HIG), SF Pro / SF Mono, vibrancy.
//!
//! Colours mirror AppKit's semantic colours (`labelColor`,
//! `secondaryLabelColor`, `separatorColor`, `controlAccentColor` …) for both
//! the Aqua and Dark Aqua appearances.

use gpui::{App, Global, Hsla, SharedString, Window, WindowAppearance, hsla, rgb};
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

#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct Theme {
    pub dark: bool,
    /// Tint painted over the vibrancy (NSVisualEffectView) background.
    pub window_tint: Hsla,
    pub label: Hsla,
    pub secondary_label: Hsla,
    pub tertiary_label: Hsla,
    pub separator: Hsla,
    /// Fill for grouped content (code blocks, key/value groups, fields).
    pub fill: Hsla,
    pub fill_strong: Hsla,
    pub hover: Hsla,
    pub accent: Hsla,
    pub accent_text: Hsla,
    pub selection: Hsla,
    pub green: Hsla,
    pub orange: Hsla,
    pub red: Hsla,
    pub purple: Hsla,
    pub blue: Hsla,
    pub ui_font: SharedString,
    /// Code and tool output.
    pub mono_font: SharedString,
    /// The launcher's input: Lilex (Zed's editor font), bundled.
    pub input_font: SharedString,
}

fn c(hex: u32) -> Hsla {
    rgb(hex).into()
}

fn gray(l: f32, a: f32) -> Hsla {
    hsla(0., 0., l, a)
}

impl Theme {
    pub fn light(mono_font: SharedString) -> Self {
        Self {
            dark: false,
            window_tint: gray(0.98, 0.72),
            label: gray(0., 0.85),
            secondary_label: gray(0., 0.5),
            tertiary_label: gray(0., 0.26),
            separator: gray(0., 0.1),
            fill: gray(0., 0.04),
            fill_strong: gray(0., 0.08),
            hover: gray(0., 0.05),
            accent: c(0x007AFF),
            accent_text: gray(1., 1.),
            selection: hsla(211. / 360., 1., 0.5, 0.25),
            green: c(0x28A745),
            orange: c(0xFF9500),
            red: c(0xFF3B30),
            purple: c(0xAF52DE),
            blue: c(0x007AFF),
            ui_font: ".SystemUIFont".into(),
            input_font: mono_font.clone(),
            mono_font,
        }
    }

    pub fn dark(mono_font: SharedString) -> Self {
        Self {
            dark: true,
            window_tint: gray(0.12, 0.62),
            label: gray(1., 0.88),
            secondary_label: gray(1., 0.55),
            tertiary_label: gray(1., 0.28),
            separator: gray(1., 0.1),
            fill: gray(1., 0.05),
            fill_strong: gray(1., 0.1),
            hover: gray(1., 0.06),
            accent: c(0x0A84FF),
            accent_text: gray(1., 1.),
            selection: hsla(211. / 360., 1., 0.52, 0.35),
            green: c(0x30D158),
            orange: c(0xFF9F0A),
            red: c(0xFF453A),
            purple: c(0xBF5AF2),
            blue: c(0x0A84FF),
            ui_font: ".SystemUIFont".into(),
            input_font: mono_font.clone(),
            mono_font,
        }
    }

    /// Plugin badge colour: a `#RRGGBB` accent from the manifest, snapped to
    /// the matching system colour when it's one of ours.
    pub fn badge(&self, accent: Option<&str>) -> Hsla {
        let Some(hex) = accent.and_then(|a| u32::from_str_radix(a.trim_start_matches('#'), 16).ok()) else {
            return self.blue;
        };
        match hex {
            0xFF9500 | 0xFF9F0A => self.orange,
            0x34C759 | 0x30D158 | 0x28A745 => self.green,
            0xAF52DE | 0xBF5AF2 => self.purple,
            0x007AFF | 0x0A84FF => self.blue,
            other => rgb(other).into(),
        }
    }
}

/// Resolved theme + user preference, stored globally.
pub struct ThemeState {
    pub mode: ThemeMode,
    pub mono_font: SharedString,
    pub input_font: SharedString,
}

impl Global for ThemeState {}

/// The launcher's input text: Lilex (Zed's editor font) at 13px, with Zed's
/// "comfortable" line-height ratio (1.618 × 13 ≈ 21px).
pub const INPUT_FONT_SIZE: f32 = 13.;
pub const INPUT_LINE_HEIGHT: f32 = 21.;

/// Lilex 2.700 (SIL Open Font License 1.1, `assets/fonts/lilex/OFL.txt`).
const LILEX: &[u8] = include_bytes!("../assets/fonts/lilex/Lilex-Regular.ttf");

pub fn init(cx: &mut App, mode: ThemeMode) {
    if let Err(e) = cx.text_system().add_fonts(vec![std::borrow::Cow::Borrowed(LILEX)]) {
        log::warn!("loading the Lilex font failed: {e:#}");
    }
    let names = cx.text_system().all_font_names();
    let first = |fonts: &[&'static str]| -> SharedString {
        fonts.iter().copied().find(|f| names.iter().any(|n| n == f)).unwrap_or("Menlo").into()
    };
    let mono_font = first(&["SF Mono", "Menlo", "Monaco"]);
    let input_font = first(&["Lilex", "SF Mono", "Menlo"]);
    cx.set_global(ThemeState { mode, mono_font, input_font });
}

pub fn set_mode(cx: &mut App, mode: ThemeMode) {
    cx.global_mut::<ThemeState>().mode = mode;
    cx.refresh_windows();
}

/// Theme for a window, following the system appearance unless overridden.
pub fn theme(window: &Window, cx: &App) -> Theme {
    let state = cx.global::<ThemeState>();
    let dark = match state.mode {
        ThemeMode::Dark => true,
        ThemeMode::Light => false,
        ThemeMode::System => matches!(window.appearance(), WindowAppearance::Dark | WindowAppearance::VibrantDark),
    };
    let mut t = if dark { Theme::dark(state.mono_font.clone()) } else { Theme::light(state.mono_font.clone()) };
    t.input_font = state.input_font.clone();
    t
}

