//! The light and dark palettes: macOS system colours (HIG) and Xcode's syntax
//! colours, filling every token of [`delight_sdk::Theme`].

use delight_sdk::{Colors, Metrics, Palette, Status, Syntax, Text, Tint};
use gpui::{Hsla, SharedString, hsla, px, rgb};

use super::Theme;

fn c(hex: u32) -> Hsla {
    rgb(hex).into()
}

fn gray(l: f32, a: f32) -> Hsla {
    hsla(0., 0., l, a)
}

/// A status colour with its tinted background.
fn tint(fg: Hsla, bg_alpha: f32) -> Tint {
    let mut t = Tint::default();
    t.fg = fg;
    t.bg = fg.opacity(bg_alpha);
    t
}

impl Theme {
    pub fn light(mono_font: SharedString) -> Self {
        let mut p = Palette::default();
        (p.red, p.orange, p.yellow, p.green, p.teal) = (c(0xFF3B30), c(0xFF9500), c(0xFFCC00), c(0x28A745), c(0x30B0C7));
        (p.blue, p.indigo, p.purple, p.pink, p.gray) = (c(0x007AFF), c(0x5856D6), c(0xAF52DE), c(0xFF2D55), c(0x8E8E93));

        let mut k = Colors::default();
        (k.label, k.secondary_label, k.tertiary_label) = (gray(0., 0.85), gray(0., 0.5), gray(0., 0.26));
        (k.surface, k.surface_elevated) = (gray(1., 0.6), gray(0.99, 1.));
        (k.fill, k.fill_strong) = (gray(0., 0.04), gray(0., 0.08));
        (k.hover, k.active, k.selected) = (gray(0., 0.05), gray(0., 0.1), p.blue);
        (k.selection, k.cursor, k.focus_ring) = (hsla(211. / 360., 1., 0.5, 0.25), p.blue, p.blue.opacity(0.5));
        (k.separator, k.border) = (gray(0., 0.1), gray(0., 0.15));
        (k.accent, k.accent_text) = (p.blue, gray(1., 1.));

        let mut x = Syntax::default();
        (x.keyword, x.string, x.number, x.comment) = (c(0x9B2393), c(0xC41A16), c(0x1C00CF), c(0x5D6C79));
        (x.property, x.function, x.type_) = (c(0x0B4F79), c(0x326D74), c(0x3900A0));
        (x.constant, x.punctuation) = (c(0x9B2393), gray(0., 0.5));

        Self::build(false, k, p, x, 0.12, mono_font, gray(0.98, 0.72))
    }

    pub fn dark(mono_font: SharedString) -> Self {
        let mut p = Palette::default();
        (p.red, p.orange, p.yellow, p.green, p.teal) = (c(0xFF453A), c(0xFF9F0A), c(0xFFD60A), c(0x30D158), c(0x40C8E0));
        (p.blue, p.indigo, p.purple, p.pink, p.gray) = (c(0x0A84FF), c(0x5E5CE6), c(0xBF5AF2), c(0xFF375F), c(0x98989D));

        let mut k = Colors::default();
        (k.label, k.secondary_label, k.tertiary_label) = (gray(1., 0.88), gray(1., 0.55), gray(1., 0.28));
        (k.surface, k.surface_elevated) = (gray(1., 0.06), gray(0.18, 1.));
        (k.fill, k.fill_strong) = (gray(1., 0.05), gray(1., 0.1));
        (k.hover, k.active, k.selected) = (gray(1., 0.06), gray(1., 0.12), p.blue);
        (k.selection, k.cursor, k.focus_ring) = (hsla(211. / 360., 1., 0.52, 0.35), p.blue, p.blue.opacity(0.5));
        (k.separator, k.border) = (gray(1., 0.1), gray(1., 0.15));
        (k.accent, k.accent_text) = (p.blue, gray(1., 1.));

        let mut x = Syntax::default();
        (x.keyword, x.string, x.number, x.comment) = (c(0xFC5FA3), c(0xFC6A5D), c(0xD0BF69), c(0x6C7986));
        (x.property, x.function, x.type_) = (c(0x67B7A4), c(0xA167E6), c(0x5DD8FF));
        (x.constant, x.punctuation) = (c(0xFC5FA3), gray(1., 0.55));

        Self::build(true, k, p, x, 0.18, mono_font, gray(0.12, 0.62))
    }

    /// The parts both appearances share: status tints from the palette, type
    /// and metrics.
    fn build(dark: bool, colors: Colors, palette: Palette, syntax: Syntax, tint_alpha: f32, mono_font: SharedString, window_tint: Hsla) -> Self {
        let mut status = Status::default();
        status.success = tint(palette.green, tint_alpha);
        status.warning = tint(palette.orange, tint_alpha);
        status.error = tint(palette.red, tint_alpha);
        status.info = tint(palette.blue, tint_alpha);

        let mut text = Text::default();
        (text.ui_font, text.mono_font) = (".SystemUIFont".into(), mono_font.clone());
        (text.size_sm, text.size_base, text.size_lg, text.mono_size) = (px(11.), px(13.), px(15.), px(12.));

        let mut metrics = Metrics::default();
        (metrics.space, metrics.radius_sm, metrics.radius_md) = (px(4.), px(6.), px(8.));
        (metrics.control_height, metrics.row_height) = (px(26.), px(30.));

        let mut sdk = delight_sdk::Theme::default();
        (sdk.dark, sdk.colors, sdk.status, sdk.palette, sdk.syntax, sdk.text, sdk.metrics) =
            (dark, colors, status, palette, syntax, text, metrics);
        Self { sdk, window_tint, input_font: mono_font }
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    /// Every token the SDK promises plugins has a real value in both
    /// appearances — an unset one would be transparent, zero or empty.
    #[test]
    fn palettes_fill_every_token() {
        for theme in [Theme::light("Menlo".into()), Theme::dark("Menlo".into())] {
            let t = &theme.sdk;
            let (k, s, p, x) = (&t.colors, &t.status, &t.palette, &t.syntax);
            let colors = [
                k.label, k.secondary_label, k.tertiary_label, k.surface, k.surface_elevated, k.fill, k.fill_strong,
                k.hover, k.active, k.selected, k.selection, k.cursor, k.focus_ring, k.separator, k.border, k.accent,
                k.accent_text, s.success.fg, s.success.bg, s.warning.fg, s.warning.bg, s.error.fg, s.error.bg,
                s.info.fg, s.info.bg, p.red, p.orange, p.yellow, p.green, p.teal, p.blue, p.indigo, p.purple, p.pink,
                p.gray, x.keyword, x.string, x.number, x.comment, x.property, x.function, x.type_, x.constant,
                x.punctuation,
            ];
            for (i, color) in colors.iter().enumerate() {
                assert!(color.a > 0., "colour #{i} unset (dark: {})", t.dark);
            }
            let (text, m) = (&t.text, &t.metrics);
            for size in [text.size_sm, text.size_base, text.size_lg, text.mono_size, m.space, m.radius_sm, m.radius_md, m.control_height, m.row_height] {
                assert!(size > px(0.));
            }
            assert!(!text.ui_font.is_empty() && !text.mono_font.is_empty());
        }
    }
}
