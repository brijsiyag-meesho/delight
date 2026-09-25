//! The design tokens tool views draw with. Only the shape is part of the
//! contract: the host fills in the values at render time
//! ([`crate::Host::theme`]), so Delight's look can change without breaking
//! plugins.
//!
//! Every token has a meaningful value in both light and dark mode. Views stay
//! transparent — the result pane's background is the host's window.

use gpui::{Hsla, Pixels, SharedString};

/// The current appearance. Fetch it in `render` with
/// `host(cx).theme(window, cx)`; when the appearance changes, Delight redraws
/// every window.
#[derive(Debug, Clone, Default, PartialEq)]
#[non_exhaustive]
pub struct Theme {
    /// Dark appearance.
    pub dark: bool,
    pub colors: Colors,
    pub status: Status,
    pub palette: Palette,
    pub syntax: Syntax,
    pub text: Text,
    pub metrics: Metrics,
}

/// Semantic colours, after AppKit's.
#[derive(Debug, Clone, Default, PartialEq)]
#[non_exhaustive]
pub struct Colors {
    /// Primary text.
    pub label: Hsla,
    /// Secondary text: captions, hints.
    pub secondary_label: Hsla,
    /// Placeholder and disabled text.
    pub tertiary_label: Hsla,

    /// Cards and panels.
    pub surface: Hsla,
    /// Popovers and menus a tool opens.
    pub surface_elevated: Hsla,
    /// Grouped content: code, key/value lists, fields.
    pub fill: Hsla,
    /// Emphasised content, and the track of an unselected control.
    pub fill_strong: Hsla,

    /// A hovered row or button.
    pub hover: Hsla,
    /// A pressed row or button.
    pub active: Hsla,
    /// A selected row.
    pub selected: Hsla,

    /// Selected text.
    pub selection: Hsla,
    /// The text caret.
    pub cursor: Hsla,
    /// Ring around the control with keyboard focus.
    pub focus_ring: Hsla,

    /// Hairlines between content.
    pub separator: Hsla,
    /// Outlines of fields and controls.
    pub border: Hsla,

    /// The accent colour: default buttons, selected controls, links.
    pub accent: Hsla,
    /// Text on [`Colors::accent`].
    pub accent_text: Hsla,
}

/// A status colour: `fg` for text and icons, `bg` for a subtle tinted
/// background behind them (a notice, a badge).
#[derive(Debug, Clone, Default, PartialEq)]
#[non_exhaustive]
pub struct Tint {
    pub fg: Hsla,
    pub bg: Hsla,
}

#[derive(Debug, Clone, Default, PartialEq)]
#[non_exhaustive]
pub struct Status {
    pub success: Tint,
    pub warning: Tint,
    pub error: Tint,
    pub info: Tint,
}

/// Named colours — macOS's system colours — for when a tool wants "purple",
/// not a status (categories, charts).
#[derive(Debug, Clone, Default, PartialEq)]
#[non_exhaustive]
pub struct Palette {
    pub red: Hsla,
    pub orange: Hsla,
    pub yellow: Hsla,
    pub green: Hsla,
    pub teal: Hsla,
    pub blue: Hsla,
    pub indigo: Hsla,
    pub purple: Hsla,
    pub pink: Hsla,
    pub gray: Hsla,
}

/// Syntax highlighting for tool output (JSON, YAML, env files, code).
#[derive(Debug, Clone, Default, PartialEq)]
#[non_exhaustive]
pub struct Syntax {
    pub keyword: Hsla,
    pub string: Hsla,
    pub number: Hsla,
    pub comment: Hsla,
    /// Keys: JSON / YAML keys, env names, object fields.
    pub property: Hsla,
    pub function: Hsla,
    pub type_: Hsla,
    /// `true`, `false`, `null`, named constants.
    pub constant: Hsla,
    pub punctuation: Hsla,
}

#[derive(Debug, Clone, Default, PartialEq)]
#[non_exhaustive]
pub struct Text {
    /// Font family for interface text.
    pub ui_font: SharedString,
    /// Font family for code and tool output.
    pub mono_font: SharedString,
    /// Captions and hints.
    pub size_sm: Pixels,
    /// Body text.
    pub size_base: Pixels,
    /// Titles.
    pub size_lg: Pixels,
    /// Code and tool output, in [`Text::mono_font`].
    pub mono_size: Pixels,
}

#[derive(Debug, Clone, Default, PartialEq)]
#[non_exhaustive]
pub struct Metrics {
    /// The spacing unit; use multiples of it.
    pub space: Pixels,
    /// Corners of small controls: buttons, fields, badges.
    pub radius_sm: Pixels,
    /// Corners of cards and groups.
    pub radius_md: Pixels,
    /// Height of buttons and single-line fields.
    pub control_height: Pixels,
    /// Height of a list row.
    pub row_height: Pixels,
}
