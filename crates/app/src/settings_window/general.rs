//! The General tab: Delight's own preferences.

use delight_core::Settings;
use delight_core::settings::{Appearance, DEFAULT_LAUNCHER_SHORTCUT};
use delight_ui::{ActiveTheme, Button, SegmentedControl, Switch, h_flex, v_flex};
use gpui::{AnyElement, Context, IntoElement, Keystroke, ParentElement, Styled, div, px};

use super::shortcut_recorder::Recorded;
use super::{SettingsWindow, row, section};
use crate::{keymap, state};

const APPEARANCES: [(Appearance, &str); 3] =
    [(Appearance::System, "Auto"), (Appearance::Light, "Light"), (Appearance::Dark, "Dark")];

impl SettingsWindow {
    pub(super) fn render_general(&self, cx: &mut Context<Self>) -> AnyElement {
        let settings = state::settings(cx).clone();

        let shortcut = h_flex().gap(px(4.)).items_start().child(self.shortcut.clone()).child(
            Button::new("reset-shortcut", "Reset").text().on_click(cx.listener(|this, _, _, cx| {
                let default = Keystroke::parse(DEFAULT_LAUNCHER_SHORTCUT).expect("the default shortcut parses");
                this.shortcut.update(cx, |_, cx| cx.emit(Recorded(default)));
            })),
        );
        let selected = APPEARANCES.iter().position(|(a, _)| *a == settings.appearance).unwrap_or(0);
        let appearance = SegmentedControl::new("appearance")
            .options(APPEARANCES.map(|(_, label)| label))
            .selected(selected)
            .on_change(|index, _, cx| state::update_settings(cx, |s| s.appearance = APPEARANCES[*index].0));

        let keymap_button = Button::new("open-keymap", "Open keymap.json").on_click(|_, _, cx| {
            if let Err(e) = keymap::open_user_keymap(cx) {
                log::error!("opening keymap.json: {e:#}");
            }
        });

        let rows = vec![
            row("Open Delight", Some("Shortcut, anywhere"), shortcut, cx),
            row("Keyboard shortcuts", Some("Delight's other keys, as in Zed. Saved changes apply at once."), keymap_button, cx),
            row("Appearance", None, appearance, cx),
            toggle("login", "Open at login", Some("Start Delight when you log in"), settings.open_at_login, |s, on| {
                s.open_at_login = on
            }, cx),
            toggle("blur", "Hide when focus is lost", None, settings.hide_on_blur, |s, on| s.hide_on_blur = on, cx),
            toggle(
                "history",
                "Input history",
                Some("Restore the last input, and complete inputs tools remembered. Turning it off erases it."),
                settings.input_history,
                |s, on| s.input_history = on,
                cx,
            ),
            toggle(
                "paste",
                "Auto-paste clipboard",
                Some("Put the clipboard's text into the input when Delight opens"),
                settings.paste_clipboard_on_open,
                |s, on| s.paste_clipboard_on_open = on,
                cx,
            ),
        ];
        let about = format!(
            "Delight {} · Plugin SDK {} · {} · {}",
            env!("CARGO_PKG_VERSION"),
            delight_sdk::SDK_VERSION,
            delight_sdk::RUSTC_VERSION.split(' ').take(2).collect::<Vec<_>>().join(" "),
            &delight_sdk::SOURCE_HASH[..8],
        );
        v_flex()
            .gap(px(18.))
            .child(section("General", rows))
            .child(div().text_size(px(11.)).text_color(cx.theme().colors.tertiary_label).child(about))
            .into_any_element()
    }
}

/// A row with a switch that sets one setting.
fn toggle(
    id: &'static str,
    title: &'static str,
    detail: Option<&str>,
    on: bool,
    set: fn(&mut Settings, bool),
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    let switch = Switch::new(id).checked(on).on_change(move |on, _, cx| state::update_settings(cx, |s| set(s, *on)));
    row(title, detail, switch, cx)
}
