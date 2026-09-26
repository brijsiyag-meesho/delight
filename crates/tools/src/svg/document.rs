//! SVG documents rendered with resvg, for the preview and the PNG export.
//! GPUI draws an SVG image at its declared size, blurry on a 2× screen; this
//! renders at any scale.

use std::sync::{Arc, LazyLock};

use delight_ui::render_image;
use gpui::RenderImage;
use resvg::{tiny_skia, usvg};

/// The system fonts, for SVGs with text (loaded once, on first use).
static OPTIONS: LazyLock<usvg::Options<'static>> = LazyLock::new(|| {
    let mut fonts = usvg::fontdb::Database::new();
    fonts.load_system_fonts();
    usvg::Options { fontdb: Arc::new(fonts), ..usvg::Options::default() }
});

/// A parsed SVG document. Parsing and rendering take a while for big
/// documents (and the first one loads the system fonts): do both on a
/// background thread.
pub struct SvgDocument {
    tree: usvg::Tree,
}

impl SvgDocument {
    /// Parses `svg`; the error says what's wrong with it.
    pub fn parse(svg: &[u8]) -> Result<Self, String> {
        usvg::Tree::from_data(svg, &OPTIONS).map(|tree| Self { tree }).map_err(|e| e.to_string())
    }

    /// Its size in pixels, as the document declares it.
    pub fn size(&self) -> (f32, f32) {
        (self.tree.size().width(), self.tree.size().height())
    }

    /// Rendered at `scale` (2 for a 2× screen); `None` if that's empty or
    /// too big.
    pub fn image(&self, scale: f32) -> Option<Arc<RenderImage>> {
        let pixmap = self.pixmap(scale)?;
        let (width, height) = (pixmap.width(), pixmap.height());
        render_image(pixmap.take(), width, height)
    }

    /// Rendered at `scale`, as a PNG file.
    pub fn png(&self, scale: f32) -> Option<Vec<u8>> {
        self.pixmap(scale)?.encode_png().ok()
    }

    fn pixmap(&self, scale: f32) -> Option<tiny_skia::Pixmap> {
        let (width, height) = self.size();
        let mut pixmap = tiny_skia::Pixmap::new((width * scale).ceil() as u32, (height * scale).ceil() as u32)?;
        resvg::render(&self.tree, tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
        Some(pixmap)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED_SQUARE: &[u8] =
        br##"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="5"><rect width="10" height="5" fill="#ff0000"/></svg>"##;

    #[test]
    fn renders_at_any_scale() {
        let svg = SvgDocument::parse(RED_SQUARE).unwrap();
        assert_eq!(svg.size(), (10., 5.));
        let image = svg.image(2.).unwrap();
        assert_eq!((image.size(0).width.0, image.size(0).height.0), (20, 10));
        assert_eq!(&image.as_bytes(0).unwrap()[..4], [0, 0, 255, 255], "red, as BGRA");
        assert!(svg.png(1.).unwrap().starts_with(b"\x89PNG"));
    }

    #[test]
    fn says_what_is_wrong() {
        assert!(SvgDocument::parse(b"<svg").is_err());
        assert!(SvgDocument::parse(b"not an svg").is_err());
    }
}
