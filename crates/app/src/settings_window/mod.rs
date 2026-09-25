//! The Settings window (System Settings style), opened from the menu bar,
//! ⌘, in the launcher, or a tool's ⚙ button.
//!
//! * `general` — Delight's own preferences.
//! * `plugins` — the plugin list, and each plugin's own settings page.
//! * `shortcut_recorder` — the field that records the launcher shortcut.

mod general;
mod plugins;
mod shortcut_recorder;

use delight_sdk::SettingsView;
use delight_ui::{ActiveTheme, Caption, Divider, Group, Icon, IconName, h_flex, v_flex};
use gpui::{
    AnyElement, App, AppContext, Bounds, Context, Entity, FocusHandle, Focusable, IntoElement,
    ParentElement, Render, SharedString, Styled, Subscription, TitlebarOptions, Window, WindowBackgroundAppearance,
    WindowBounds, WindowKind, WindowOptions, actions, div, point, prelude::*, px, size,
};

use self::shortcut_recorder::{Recorded, ShortcutRecorder};
use crate::hotkey::LauncherShortcut;
use crate::state::{self, AppState};

const CONTEXT: &str = "Settings";

actions!(settings_window, [CloseSettings]);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    General,
    Plugins,
}

/// A plugin's settings page (within the Plugins tab).
struct PluginPage {
    plugin_id: String,
    /// The plugin's own page; `None` if it has none (or crashed).
    view: Option<Box<dyn SettingsView>>,
}

pub struct SettingsWindow {
    focus_handle: FocusHandle,
    tab: Tab,
    /// A plugin's page, or `None` for the tab itself.
    page: Option<PluginPage>,
    shortcut: Entity<ShortcutRecorder>,
    _subscriptions: Vec<Subscription>,
}

/// Opens (or brings forward) the Settings window on the General tab.
pub fn open(cx: &mut App) {
    open_on(cx, None);
}

/// Opens (or brings forward) the Settings window on a plugin's page.
pub fn open_plugin(cx: &mut App, plugin_id: &str) {
    open_on(cx, Some(plugin_id.to_string()));
}

fn open_on(cx: &mut App, plugin_id: Option<String>) {
    // Activate first: a window made key while the app is inactive trips
    // GPUI's focus deadlock (see `platform::patch_gpui_focus`).
    cx.activate(true);
    if let Some(handle) = cx.global::<AppState>().settings_window {
        let shown = handle.update(cx, |this, window, cx| {
            this.navigate(plugin_id.clone(), window, cx);
            window.activate_window();
        });
        if shown.is_ok() {
            return;
        }
    }
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(None, size(px(560.), px(600.)), cx))),
        titlebar: Some(TitlebarOptions {
            title: Some("Delight Settings".into()),
            appears_transparent: true,
            traffic_light_position: Some(point(px(14.), px(14.))),
        }),
        focus: true,
        show: true,
        kind: WindowKind::Normal,
        is_resizable: false,
        is_minimizable: false,
        // Solid, like System Settings: GPUI's blur is a mid-grey in dark mode
        // that washes out text.
        window_background: WindowBackgroundAppearance::Opaque,
        ..Default::default()
    };
    let opened = cx.open_window(options, |window, cx| cx.new(|cx| SettingsWindow::new(window, cx)));
    match opened {
        Ok(handle) => {
            let _ = handle.update(cx, |this, window, cx| {
                window.focus(&this.focus_handle);
                this.navigate(plugin_id, window, cx);
            });
            cx.global_mut::<AppState>().settings_window = Some(handle);
        }
        Err(e) => log::error!("opening Settings: {e:#}"),
    }
}

impl SettingsWindow {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let current = cx.try_global::<LauncherShortcut>().map(|s| s.keystroke().clone());
        let shortcut = cx.new(|cx| ShortcutRecorder::new(current, cx));
        let subscriptions = vec![
            cx.subscribe_in(&shortcut, window, |_, recorder, Recorded(keystroke), window, cx| {
                match state::set_launcher_shortcut(cx, keystroke) {
                    Ok(()) => recorder.update(cx, |r, cx| r.set_current(keystroke.clone(), window, cx)),
                    Err(e) => recorder.update(cx, |r, cx| r.set_error(format!("{e:#}"), cx)),
                }
            }),
            cx.observe_window_appearance(window, |_, _, cx| delight_ui::theme::appearance_changed(cx)),
        ];
        Self { focus_handle: cx.focus_handle(), tab: Tab::General, page: None, shortcut, _subscriptions: subscriptions }
    }

    /// Shows a plugin's page (`Some`), or the current tab (`None`).
    fn navigate(&mut self, plugin_id: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        if plugin_id.is_some() {
            self.tab = Tab::Plugins;
        }
        if self.page.as_ref().map(|p| &p.plugin_id) != plugin_id.as_ref() {
            self.page = plugin_id.map(|plugin_id| {
                let registry = cx.global::<AppState>().registry.clone();
                let view = registry.get(&plugin_id).and_then(|p| p.plugin.settings_view(cx));
                PluginPage { plugin_id, view }
            });
        }
        window.focus(&self.focus_handle);
        cx.notify();
    }

    fn select_tab(&mut self, tab: Tab, window: &mut Window, cx: &mut Context<Self>) {
        self.tab = tab;
        self.page = None;
        window.focus(&self.focus_handle);
        cx.notify();
    }

    fn render_tabs(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let k = cx.theme().colors.clone();
        let tab = |id: &'static str, label: &'static str, icon: IconName, which: Tab| {
            let selected = self.tab == which;
            let color = if selected { k.accent } else { k.secondary_label };
            let hover = k.hover;
            v_flex()
                .id(id)
                .w(px(72.))
                .py(px(4.))
                .items_center()
                .gap(px(2.))
                .rounded(px(6.))
                .cursor_pointer()
                .text_size(px(11.))
                .text_color(color)
                .when(selected, |d| d.bg(k.fill_strong))
                .when(!selected, |d| d.hover(move |s| s.bg(hover)))
                .child(Icon::new(icon).size(px(18.)))
                .child(label)
                .on_click(cx.listener(move |this, _, window, cx| this.select_tab(which, window, cx)))
        };
        h_flex()
            .flex_shrink_0()
            .py(px(8.))
            .justify_center()
            .gap(px(4.))
            .child(tab("tab-general", "General", IconName::Settings, Tab::General))
            .child(tab("tab-plugins", "Plugins", IconName::Puzzle, Tab::Plugins))
    }
}

impl Focusable for SettingsWindow {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for SettingsWindow {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let body = match (self.tab, &self.page) {
            (_, Some(page)) => self.render_plugin_page(page, cx),
            (Tab::General, None) => self.render_general(cx),
            (Tab::Plugins, None) => self.render_plugins(cx),
        };
        let t = cx.theme();
        v_flex()
            .key_context(CONTEXT)
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|_, _: &CloseSettings, window, _| window.remove_window()))
            .size_full()
            .bg(t.colors.surface_elevated)
            .font_family(t.text.ui_font.clone())
            .text_color(t.colors.label)
            .text_size(t.text.size_base)
            .child(self.render_tabs(cx))
            .child(Divider::horizontal())
            .child(div().id("settings-scroll").flex_1().overflow_y_scroll().px(px(20.)).py(px(18.)).child(body))
    }
}

/// A `title (detail)   [control]` row in a group.
fn row(title: impl Into<SharedString>, detail: Option<&str>, control: impl IntoElement, cx: &App) -> AnyElement {
    let secondary = cx.theme().colors.secondary_label;
    h_flex()
        .justify_between()
        .gap(px(16.))
        .px(px(12.))
        .py(px(9.))
        .child(
            v_flex()
                .min_w(px(0.))
                .child(div().child(title.into()))
                .when_some(detail, |d, detail| d.child(div().text_size(px(11.)).text_color(secondary).child(detail.to_string()))),
        )
        .child(control)
        .into_any_element()
}

/// A titled group of rows.
fn section(title: &'static str, rows: Vec<AnyElement>) -> impl IntoElement {
    v_flex().gap(px(6.)).child(div().px(px(4.)).child(Caption::new(title))).child(Group::new().children(rows))
}
