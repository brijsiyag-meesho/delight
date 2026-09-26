//! The SVG tool's view: the drawing on a canvas (checkerboard, light or
//! dark), its size, and the copy actions.

use delight_sdk::{Action, ToolContext, ToolView, host};
use delight_ui::{ActiveTheme, IconButton, IconName, Selectable, h_flex, v_flex};
use gpui::{
    AnyView, App, ClipboardItem, Context, Entity, Hsla, ImageSource, IntoElement, ParentElement, Render, SharedString,
    Styled, Task, Window, div, img, prelude::FluentBuilder, px, rgb, size,
};

use super::checkerboard::Checkerboard;
use super::render::{PREVIEW_HEIGHT, PREVIEW_WIDTH, Preview, render};

/// Space around the drawing on the canvas, and its corners.
const CANVAS_PADDING: f32 = 20.;
const CANVAS_RADIUS: f32 = 12.;

/// What the drawing is shown on.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum Backdrop {
    /// Squares, so transparent areas show.
    #[default]
    Checkerboard,
    Light,
    Dark,
}

const BACKDROPS: [(Backdrop, IconName, &str); 3] = [
    (Backdrop::Checkerboard, IconName::Grid, "Show transparency"),
    (Backdrop::Light, IconName::Sun, "On a light background"),
    (Backdrop::Dark, IconName::Moon, "On a dark background"),
];

#[derive(Default)]
pub struct SvgView {
    preview: Preview,
    backdrop: Backdrop,
    /// Replacing it cancels the rendering in progress.
    _task: Option<Task<()>>,
}

impl SvgView {
    /// Renders `input` in the background, then shows it.
    fn render_input(&mut self, input: SharedString, cx: &mut Context<Self>) {
        self._task = Some(cx.spawn(async move |this, cx| {
            let preview = cx.background_executor().spawn(async move { render(input.trim()) }).await;
            let _ = this.update(cx, |this, cx| {
                this.preview = preview;
                cx.notify();
            });
        }));
    }
}

/// The view, as the host sees it.
pub struct SvgTool(pub Entity<SvgView>);

impl ToolView for SvgTool {
    fn view(&self) -> AnyView {
        self.0.clone().into()
    }

    fn update(&self, context: &ToolContext, cx: &mut App) {
        let input = context.input.text.clone();
        self.0.update(cx, |view, cx| view.render_input(input, cx));
    }

    fn actions(&self, cx: &App) -> Vec<Action> {
        let Preview::Ready(rendered) = &self.0.read(cx).preview else {
            return Vec::new();
        };
        let mut actions = Vec::new();
        if rendered.png.is_some() {
            actions.push(Action::new("copy_png", "Copy PNG").shortcut("enter"));
        }
        // ↵ when there's no PNG to copy.
        let data_uri_key = if rendered.png.is_some() { "cmd-enter" } else { "enter" };
        actions.push(Action::new("copy_data_uri", "Copy data URI").shortcut(data_uri_key));
        actions
    }

    fn perform(&self, action_id: &str, cx: &mut App) {
        let Preview::Ready(rendered) = &self.0.read(cx).preview else {
            return;
        };
        let (label, copied) = match action_id {
            "copy_png" if let Some(png) = rendered.png.clone() => {
                ("Copy PNG", host(cx).copy_file("image.png", &png, cx))
            }
            "copy_data_uri" => {
                cx.write_to_clipboard(ClipboardItem::new_string(rendered.data_uri.clone()));
                ("Copy data URI", Ok(()))
            }
            _ => return,
        };
        let message = match copied {
            Ok(()) => format!("{label} — copied to clipboard"),
            Err(e) => format!("Couldn't copy image.png: {e}"),
        };
        host(cx).toast(message.into(), cx);
    }
}

impl Render for SvgView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let rendered = match &self.preview {
            Preview::Empty => return div(),
            Preview::Failed(message) => return div().child(notice(message.clone(), cx)),
            Preview::Ready(rendered) => rendered,
        };
        let t = cx.theme();
        let drawing = img(ImageSource::Render(rendered.image.clone())).w(px(rendered.shown.0)).h(px(rendered.shown.1));
        // The light and dark backdrops are fixed colours, not the theme's:
        // they show how the drawing looks on either.
        let backdrop: Option<Hsla> = match self.backdrop {
            Backdrop::Checkerboard => None,
            Backdrop::Light => Some(rgb(0xFFFFFF).into()),
            Backdrop::Dark => Some(rgb(0x1C1C1E).into()),
        };
        // About the pane's width; the canvas fills it.
        let canvas_size = size(px(PREVIEW_WIDTH + 2. * CANVAS_PADDING), px(PREVIEW_HEIGHT + 2. * CANVAS_PADDING));
        let canvas = div()
            .relative()
            .w_full()
            .h(canvas_size.height)
            .rounded(px(CANVAS_RADIUS))
            .overflow_hidden()
            .when_some(backdrop, |canvas, color| canvas.bg(color))
            .when(backdrop.is_none(), |canvas| canvas.child(Checkerboard::new(canvas_size).rounded(px(CANVAS_RADIUS))))
            .child(h_flex().absolute().top_0().left_0().size_full().justify_center().child(drawing));

        let (width, height) = rendered.size;
        let size = div().text_size(t.text.size_sm).text_color(t.colors.tertiary_label).child(format!(
            "{} × {} px",
            round(width),
            round(height)
        ));
        let backdrops = h_flex().gap(px(2.)).children(BACKDROPS.map(|(backdrop, icon, tooltip)| {
            IconButton::new(SharedString::from(format!("svg-backdrop-{backdrop:?}")), icon)
                .selected(self.backdrop == backdrop)
                .tooltip(tooltip)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.backdrop = backdrop;
                    cx.notify();
                }))
        }));
        div().child(v_flex().gap(px(8.)).child(canvas).child(h_flex().justify_between().child(size).child(backdrops)))
    }
}

/// `24.0` → `24`, `10.5` → `10.5`.
fn round(value: f32) -> String {
    let rounded = (value * 10.).round() / 10.;
    if rounded.fract() == 0. { format!("{rounded:.0}") } else { format!("{rounded:.1}") }
}

/// An error line on its tint.
fn notice(text: SharedString, cx: &App) -> impl IntoElement {
    let t = cx.theme();
    h_flex()
        .px(px(12.))
        .py(px(8.))
        .rounded(t.metrics.radius_md)
        .bg(t.status.error.bg)
        .text_color(t.status.error.fg)
        .text_size(t.text.size_sm)
        .child(text)
}
