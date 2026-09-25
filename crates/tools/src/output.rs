//! What the converting tools share: their result (highlighted code, or why
//! the input can't be converted), its footer actions, and the [`ToolView`]
//! around a tool's view entity.

use delight_sdk::{Action, ToolContext, ToolView};
use delight_ui::{ActiveTheme, Caption, Code, CodeBlock, Language, h_flex, v_flex};
use gpui::{
    AnyElement, AnyView, App, Context, Entity, IntoElement, ParentElement, Render, SharedString, Styled, div, px,
};

/// A tool's result.
#[derive(Clone, Default)]
pub enum Output {
    /// Nothing to show (no input yet).
    #[default]
    Empty,
    Code {
        title: SharedString,
        /// All of it, for copying ([`Code`] may show only its first lines).
        text: SharedString,
        code: Code,
        /// Something worth knowing about the result, shown above it.
        note: Option<SharedString>,
    },
    Error {
        message: SharedString,
        /// The lines around the error, if it has a position.
        near: Option<Code>,
    },
}

impl Output {
    /// A result as highlighted code. Highlighting parses the text: call it
    /// on a background thread.
    pub fn code(title: &str, language: Language, text: String) -> Self {
        let code = Code::new(Some(language), &text);
        Output::Code { title: title.to_string().into(), text: text.into(), code, note: None }
    }

    pub fn with_note(mut self, note: &str) -> Self {
        if let Output::Code { note: n, .. } = &mut self {
            *n = Some(note.to_string().into());
        }
        self
    }

    /// Why the input can't be converted; `near` is the text around a
    /// `line`/`column` position (1-based), marked with a caret.
    pub fn error(message: String, text: &str, position: Option<(usize, usize)>) -> Self {
        let near = position.filter(|(line, _)| *line > 0).map(|(line, column)| {
            let first = line.saturating_sub(3);
            let mut excerpt = String::new();
            for (i, l) in text.lines().enumerate().skip(first).take(line - first) {
                excerpt.push_str(&format!("{:>5} │ {l}\n", i + 1));
            }
            excerpt.push_str(&format!("{:>5} │ {}^", "", " ".repeat(column.saturating_sub(1))));
            Code::new(None, &excerpt)
        });
        Output::Error { message: message.into(), near }
    }

    /// The result's text, for copying.
    pub fn text(&self) -> Option<&SharedString> {
        match self {
            Output::Code { text, .. } => Some(text),
            _ => None,
        }
    }

    /// "Copy …" for the result: the primary action.
    pub fn actions(&self, copy_label: &str) -> Vec<Action> {
        let Some(text) = self.text() else { return Vec::new() };
        vec![Action::copy("copy", copy_label, text.to_string()).primary()]
    }

    /// The result; `accessory` (e.g. formatting buttons) sits at the right
    /// of its title.
    pub fn render(&self, accessory: Option<AnyElement>, cx: &App) -> AnyElement {
        let t = cx.theme();
        match self {
            Output::Empty => div().into_any_element(),
            Output::Code { title, code, note, .. } => v_flex()
                .gap(px(8.))
                .child(h_flex().h(px(24.)).justify_between().child(Caption::new(title.clone())).children(accessory))
                .children(note.clone().map(|note| notice(note, false, cx)))
                .child(CodeBlock::new(code.clone()))
                .into_any_element(),
            Output::Error { message, near } => v_flex()
                .gap(px(8.))
                .child(notice(message.clone(), true, cx))
                .children(near.clone().map(|near| {
                    v_flex().gap(px(8.)).child(Caption::new("Near")).child(CodeBlock::new(near))
                }))
                .text_color(t.colors.label)
                .into_any_element(),
        }
    }
}

/// An info or error line on its tint.
fn notice(text: SharedString, error: bool, cx: &App) -> impl IntoElement {
    let t = cx.theme();
    let tint = if error { &t.status.error } else { &t.status.info };
    h_flex()
        .px(px(12.))
        .py(px(8.))
        .rounded(t.metrics.radius_md)
        .bg(tint.bg)
        .text_color(tint.fg)
        .text_size(t.text.size_sm)
        .child(text)
}

/// A converting tool's view: it gets the input and lists its actions.
pub trait Converter: Render {
    fn set_input(&mut self, input: SharedString, cx: &mut Context<Self>);
    fn actions(&self) -> Vec<Action>;
}

/// The [`ToolView`] around a converter's entity.
pub struct ConverterView<V>(pub Entity<V>);

impl<V: Converter> ToolView for ConverterView<V> {
    fn view(&self) -> AnyView {
        self.0.clone().into()
    }

    fn update(&self, context: &ToolContext, cx: &mut App) {
        let input = context.input.text.clone();
        self.0.update(cx, |view, cx| view.set_input(input, cx));
    }

    fn actions(&self, cx: &App) -> Vec<Action> {
        self.0.read(cx).actions()
    }
}
