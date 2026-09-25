//! Settings window (System Settings style), opened from the menu bar icon,
//! ⌘, in the launcher, or a tool's ⚙ button. Plugins with settings get their
//! own page: the plugin's custom view, or a form generated from its manifest.

use delight_core::Settings;
use delight_core::registry::PluginSource;
use delight_core::settings::{ClassifierMode, ThemeMode};
use delight_sdk::{FieldKind, FormField};
use gpui::{
    AnyElement, AnyView, App, AppContext, Bounds, Context, Entity, FocusHandle, Focusable, FontWeight, KeyBinding,
    PathPromptOptions, PromptLevel, SharedString, Subscription, TitlebarOptions, Window, WindowBackgroundAppearance, WindowBounds,
    WindowKind, WindowOptions, actions, div, point, prelude::*, px, size,
};

use delight_sdk::editor::{EditorEvent, TextEditor};
use crate::boundary::PanicBoundary;
use crate::state::{self, AppState};
use delight_sdk::theme::{self, Theme, theme};
use delight_sdk::ui::{self, ButtonStyle};

actions!(settings_window, [CloseSettings]);

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-w", CloseSettings, Some("Settings")),
        KeyBinding::new("escape", CloseSettings, Some("Settings")),
    ]);
}

pub struct SettingsWindow {
    focus_handle: FocusHandle,
    tab: Tab,
    /// A plugin's page (within the Plugins tab), or `None` for the tab itself.
    page: Option<PluginPage>,
    _subs: Vec<Subscription>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    General,
    Plugins,
}

/// A plugin's settings: its own view, or a form generated from the manifest.
struct PluginPage {
    plugin_id: String,
    custom: Option<AnyView>,
    editors: Vec<(String, Entity<TextEditor>)>,
    _subs: Vec<Subscription>,
}

/// Opens (or focuses) the settings window on the main page.
pub fn open(cx: &mut App) {
    open_on(cx, None);
}

/// Opens (or focuses) the settings window on a plugin's page.
pub fn open_plugin(cx: &mut App, plugin_id: &str) {
    open_on(cx, Some(plugin_id.to_string()));
}

fn open_on(cx: &mut App, plugin_id: Option<String>) {
    // Activate first: a window made key while the app is inactive (e.g. opened
    // from the menu bar) is what trips GPUI's focus deadlock.
    cx.activate(true);
    if let Some(handle) = cx.global::<AppState>().settings_window
        && handle
            .update(cx, |view, window, cx| {
                view.navigate(plugin_id.clone(), window, cx);
                window.activate_window();
            })
            .is_ok()
    {
        cx.activate(true);
        return;
    }
    let bounds = Bounds::centered(None, size(px(560.), px(600.)), cx);
    let handle = cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
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
            window_background: WindowBackgroundAppearance::Blurred,
            ..Default::default()
        },
        |window, cx| {
            cx.new(|cx| SettingsWindow {
                focus_handle: cx.focus_handle(),
                tab: Tab::General,
                page: None,
                _subs: vec![cx.observe_window_appearance(window, |_, _, cx| cx.notify())],
            })
        },
    );
    match handle {
        Ok(handle) => {
            let _ = handle.update(cx, |view, window, cx| {
                window.focus(&view.focus_handle);
                view.navigate(plugin_id, window, cx);
            });
            cx.global_mut::<AppState>().settings_window = Some(handle);
            cx.activate(true);
        }
        Err(e) => log::error!("failed to open settings: {e:#}"),
    }
}

impl Focusable for SettingsWindow {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

fn row(t: &Theme, title: &str, detail: Option<&str>, control: AnyElement) -> AnyElement {
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(16.))
        .px(px(12.))
        .py(px(9.))
        .child(
            div()
                .flex()
                .flex_col()
                .min_w(px(0.))
                .child(div().text_size(px(13.)).child(title.to_string()))
                .when_some(detail, |d, s| {
                    d.child(div().text_size(px(11.)).text_color(t.secondary_label).child(s.to_string()))
                }),
        )
        .child(control)
        .into_any_element()
}

fn section(t: &Theme, title: &str, rows: Vec<AnyElement>) -> impl IntoElement {
    div().flex().flex_col().gap(px(6.)).child(div().px(px(4.)).child(ui::caption(t, title.to_string()))).child(ui::group(t, rows))
}

fn toggle_row(
    t: &Theme,
    id: &'static str,
    title: &str,
    detail: Option<&str>,
    on: bool,
    set: fn(&mut Settings, bool),
    cx: &mut Context<SettingsWindow>,
) -> AnyElement {
    let control = ui::switch(t, id, on)
        .on_click(cx.listener(move |_, _, _, cx| state::update_settings(cx, |s| set(s, !on))))
        .into_any_element();
    row(t, title, detail, control)
}

impl SettingsWindow {
    fn navigate(&mut self, plugin_id: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        if plugin_id.is_some() {
            self.tab = Tab::Plugins;
        }
        if self.page.as_ref().map(|p| &p.plugin_id) == plugin_id.as_ref() {
            return;
        }
        self.page = plugin_id.and_then(|id| self.plugin_page(id, cx));
        window.focus(&self.focus_handle);
        cx.notify();
    }

    fn plugin_page(&mut self, plugin_id: String, cx: &mut Context<Self>) -> Option<PluginPage> {
        let plugin = cx.global::<AppState>().registry.get(&plugin_id)?.plugin.clone();
        if let Some(view) = plugin.settings_view(cx) {
            return Some(PluginPage { plugin_id, custom: Some(view.view()), editors: Vec::new(), _subs: Vec::new() });
        }
        let values = state::plugin_settings(cx, &plugin_id);
        let mut editors = Vec::new();
        let mut subs = Vec::new();
        for field in &plugin.manifest().settings {
            let masked = match field.kind {
                FieldKind::Text | FieldKind::Number | FieldKind::Multiline => false,
                FieldKind::Secret => true,
                _ => continue,
            };
            let value = values.get(&field.key).cloned().unwrap_or_default();
            let placeholder = field.placeholder.clone().unwrap_or_default();
            let multiline = field.kind == FieldKind::Multiline;
            let editor = cx.new(|cx| {
                let mut e = TextEditor::new(cx).masked(masked).mono(multiline).placeholder(placeholder);
                if multiline {
                    e = e.multiline(px(18. * 4.));
                }
                e.set_text(value, cx);
                e
            });
            let (pid, key) = (plugin_id.clone(), field.key.clone());
            subs.push(cx.subscribe(&editor, move |_, editor, _: &EditorEvent, cx| {
                let value = editor.read(cx).text().to_string();
                state::set_plugin_setting(cx, &pid, &key, value);
            }));
            editors.push((field.key.clone(), editor));
        }
        Some(PluginPage { plugin_id, custom: None, editors, _subs: subs })
    }

    fn render_plugin_page(&self, t: &Theme, page: &PluginPage, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let back = div()
            .id("back")
            .text_size(px(12.))
            .text_color(t.accent)
            .cursor_pointer()
            .child("‹ All Plugins")
            .on_click(cx.listener(|this, _, window, cx| this.navigate(None, window, cx)));
        let Some(plugin) = cx.global::<AppState>().registry.get(&page.plugin_id).map(|p| p.plugin.clone()) else {
            return div().flex().flex_col().gap(px(18.)).child(back).child("This plugin is no longer loaded.").into_any_element();
        };
        let m = plugin.manifest();
        let icon = m.icon.clone().unwrap_or_else(|| m.name.chars().take(2).collect());
        let header = div()
            .flex()
            .items_center()
            .gap(px(12.))
            .child(ui::badge(t, &icon, t.badge(m.accent.as_deref()), 36.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .min_w(px(0.))
                    .child(div().text_size(px(15.)).font_weight(FontWeight::SEMIBOLD).child(m.name.clone()))
                    .child(div().text_size(px(11.5)).text_color(t.secondary_label).child(m.description.clone())),
            );
        let body: AnyElement = match &page.custom {
            Some(view) => PanicBoundary::new(view.clone().into_any_element(), m.id.clone(), m.name.clone()).into_any_element(),
            None => {
                let values = state::plugin_settings(cx, &page.plugin_id);
                let rows = m
                    .settings
                    .iter()
                    .map(|field| {
                        let control = self.setting_control(t, page, field, values.get(&field.key), window, cx);
                        row(t, &field.label, field.help.as_deref(), control)
                    })
                    .collect();
                section(t, "Settings", rows).into_any_element()
            }
        };
        div().flex().flex_col().gap(px(18.)).child(back).child(header).child(body).into_any_element()
    }

    fn setting_control(
        &self,
        t: &Theme,
        page: &PluginPage,
        field: &FormField,
        value: Option<&String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (pid, key) = (page.plugin_id.clone(), field.key.clone());
        match &field.kind {
            FieldKind::Toggle => {
                let on = value.is_some_and(|v| v == "true");
                ui::switch(t, SharedString::from(format!("set-{key}")), on)
                    .on_click(move |_, _, cx| state::set_plugin_setting(cx, &pid, &key, (!on).to_string()))
                    .into_any_element()
            }
            FieldKind::Select { options } => {
                let labels: Vec<(SharedString, bool)> =
                    options.iter().map(|o| (SharedString::from(o.label.clone()), Some(&o.value) == value)).collect();
                ui::segmented(t, &format!("set-{key}"), &labels, |i, seg| {
                    let (pid, key, v) = (pid.clone(), key.clone(), options[i].value.clone());
                    seg.on_click(move |_, _, cx| state::set_plugin_setting(cx, &pid, &key, v.clone()))
                })
                .into_any_element()
            }
            _ => match page.editors.iter().find(|(k, _)| *k == field.key) {
                Some((_, editor)) => {
                    let focused = editor.focus_handle(cx).is_focused(window);
                    div()
                        .w(px(240.))
                        .px(px(8.))
                        .py(px(4.))
                        .rounded(px(6.))
                        .bg(if t.dark { gpui::hsla(0., 0., 0., 0.25) } else { gpui::white() })
                        .border_1()
                        .border_color(if focused { t.accent.opacity(0.8) } else { t.separator })
                        .child(editor.clone())
                        .into_any_element()
                }
                None => div().into_any_element(),
            },
        }
    }

    fn render_general(&self, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let settings = state::settings(cx).clone();

        // --- General -----------------------------------------------------
        let shortcut = div()
            .flex()
            .gap(px(3.))
            .children(["⌘", "⇧", "Space"].map(|k| ui::keycap(t, k, false)))
            .into_any_element();
        let appearance = {
            let modes = [(ThemeMode::System, "Auto"), (ThemeMode::Light, "Light"), (ThemeMode::Dark, "Dark")];
            let labels: Vec<(SharedString, bool)> =
                modes.iter().map(|(m, l)| (SharedString::from(*l), *m == settings.theme)).collect();
            ui::segmented(t, "appearance", &labels, |i, seg| {
                let mode = modes[i].0;
                seg.on_click(cx.listener(move |_, _, _, cx| {
                    state::update_settings(cx, |s| s.theme = mode);
                    theme::set_mode(cx, mode);
                }))
            })
            .into_any_element()
        };
        let general = section(
            t,
            "General",
            vec![
                row(t, "Open Delight", Some("Global shortcut"), shortcut),
                row(t, "Appearance", None, appearance),
                toggle_row(
                    t,
                    "login",
                    "Open at login",
                    Some("Start Delight when you log in"),
                    settings.open_at_login,
                    |s, v| s.open_at_login = v,
                    cx,
                ),
                toggle_row(t, "blur", "Hide when focus is lost", None, settings.hide_on_blur, |s, v| s.hide_on_blur = v, cx),
                toggle_row(t, "hide-copy", "Hide after copying with ↵", None, settings.hide_after_copy, |s, v| s.hide_after_copy = v, cx),
                toggle_row(
                    t,
                    "remember",
                    "Remember input",
                    Some("Keep the global input between launches"),
                    settings.remember_input,
                    |s, v| s.remember_input = v,
                    cx,
                ),
                toggle_row(
                    t,
                    "paste-clipboard",
                    "Auto-paste clipboard",
                    Some("Put the clipboard's text into the input when Delight opens"),
                    settings.paste_clipboard_on_open,
                    |s, v| s.paste_clipboard_on_open = v,
                    cx,
                ),
            ],
        );

        // --- Detection ---------------------------------------------------
        let classifier = {
            let modes = [(ClassifierMode::Deterministic, "Rules"), (ClassifierMode::Hybrid, "Rules + Model")];
            let labels: Vec<(SharedString, bool)> =
                modes.iter().map(|(m, l)| (SharedString::from(*l), *m == settings.classifier)).collect();
            ui::segmented(t, "classifier", &labels, |i, seg| {
                let mode = modes[i].0;
                seg.on_click(cx.listener(move |_, _, _, cx| state::update_settings(cx, |s| s.classifier = mode)))
            })
            .into_any_element()
        };
        let detection = section(
            t,
            "Tool Detection",
            vec![row(
                t,
                "Classifier",
                Some(if settings.classifier == ClassifierMode::Hybrid {
                    "No model backend configured yet — using rules"
                } else {
                    "Deterministic detection rules from each plugin"
                }),
                classifier,
            )],
        );

        div()
            .flex()
            .flex_col()
            .gap(px(18.))
            .child(general)
            .child(detection)
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(t.tertiary_label)
                    .child(format!(
                        "Delight {} · Plugin SDK {} · {} · {}",
                        env!("CARGO_PKG_VERSION"),
                        delight_sdk::SDK_VERSION,
                        delight_sdk::RUSTC_VERSION.split(' ').take(2).collect::<Vec<_>>().join(" "),
                        &delight_sdk::SOURCE_HASH[..8],
                    )),
            )
            .into_any_element()
    }

    fn render_plugins(&self, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let settings = state::settings(cx).clone();
        // --- Plugins -----------------------------------------------------
        let registry = cx.global::<AppState>().registry.clone();
        let mut plugin_rows: Vec<AnyElement> = Vec::new();
        for (i, p) in registry.plugins().iter().enumerate() {
            let m = p.manifest();
            let enabled = !settings.disabled_plugins.contains(&m.id);
            let id = m.id.clone();
            let page_id = m.id.clone();
            let (source, deletable) = match &p.source {
                PluginSource::Builtin => ("Built-in", false),
                PluginSource::Bundled { .. } => ("Included", false),
                PluginSource::Dylib { .. } => ("Plugin", true),
            };
            let (delete_id, delete_name) = (m.id.clone(), m.name.clone());
            let icon = m.icon.clone().unwrap_or_else(|| m.name.chars().take(2).collect());
            // Crashed in this run (off until restart), or turned off after
            // crashing a previous run (until the user turns it back on).
            let crash = match (p.crash(), settings.crashed_plugins.get(&m.id)) {
                (Some(message), _) => Some(format!("Crashed: {message} — off until Delight restarts")),
                (None, Some(message)) if !enabled => Some(format!("Turned off after a crash: {message}")),
                _ => None,
            };
            plugin_rows.push(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .px(px(12.))
                    .py(px(8.))
                    .child(ui::badge(t, &icon, t.badge(m.accent.as_deref()), 26.))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .flex()
                                    .gap(px(6.))
                                    .items_baseline()
                                    .child(div().text_size(px(13.)).child(m.name.clone()))
                                    .child(
                                        div()
                                            .text_size(px(11.))
                                            .text_color(t.tertiary_label)
                                            .child(format!("{source} · v{} · {} ops", m.version, m.operations.len())),
                                    ),
                            )
                            .child(div().text_size(px(11.)).text_color(t.secondary_label).truncate().child(m.description.clone()))
                            .when_some(crash, |d, crash| d.child(div().text_size(px(11.)).text_color(t.red).child(crash))),
                    )
                    .when(p.plugin.has_settings(), |d| {
                        d.child(ui::icon_button(t, ("plugin-settings", i), "icons/settings.svg").on_click(cx.listener(
                            move |this, _, window, cx| this.navigate(Some(page_id.clone()), window, cx),
                        )))
                    })
                    .when(deletable, |d| {
                        d.child(ui::icon_button(t, ("plugin-delete", i), "icons/trash-2.svg").on_click(cx.listener(
                            move |this, _, window, cx| this.confirm_delete(delete_id.clone(), delete_name.clone(), window, cx),
                        )))
                    })
                    .child(ui::switch(t, ("plugin", i), enabled).on_click(cx.listener(move |_, _, _, cx| {
                        let id = id.clone();
                        state::update_settings(cx, move |s| {
                            if enabled {
                                s.disabled_plugins.insert(id);
                            } else {
                                s.disabled_plugins.remove(&id);
                                s.crashed_plugins.remove(&id);
                            }
                        })
                    })))
                    .into_any_element(),
            );
        }
        let dir = settings.effective_plugin_dir();
        let dir_label = dir.display().to_string().replace(&dirs_home(), "~");
        plugin_rows.push(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .px(px(12.))
                .py(px(9.))
                .child(ui::icon("icons/folder.svg", 14., t.secondary_label))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .text_size(px(11.5))
                        .font_family(t.mono_font.clone())
                        .text_color(t.secondary_label)
                        .truncate()
                        .child(dir_label),
                )
                .child(
                    ui::button(t, "install", "Install…", None, None, ButtonStyle::Secondary)
                        .on_click(cx.listener(|this, _, window, cx| this.install(window, cx))),
                )
                .child(
                    ui::button(t, "open-dir", "Open Folder", None, None, ButtonStyle::Secondary).on_click(move |_, _, _| {
                        let _ = std::fs::create_dir_all(&dir);
                        let _ = std::process::Command::new("open").arg(&dir).spawn();
                    }),
                )
                .child(
                    ui::button(t, "restart", "Restart", Some("icons/refresh-cw.svg"), None, ButtonStyle::Secondary)
                        .on_click(|_, _, cx| cx.defer(state::restart)),
                )
                .into_any_element(),
        );
        let plugins = section(t, "Plugins", plugin_rows);
        let errors = registry.load_errors.clone();

        div()
            .flex()
            .flex_col()
            .gap(px(18.))
            .child(plugins)
            .when(!errors.is_empty(), |d| {
                d.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(4.))
                        .px(px(12.))
                        .py(px(9.))
                        .rounded(px(8.))
                        .bg(t.orange.opacity(0.12))
                        .text_size(px(11.5))
                        .child(div().font_weight(FontWeight::SEMIBOLD).child("Some plugins failed to load"))
                        .children(errors.into_iter().map(|e| div().text_color(t.secondary_label).child(e))),
                )
            })
            .into_any_element()
    }

    fn render_tabs(&self, t: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let tab = |id: &'static str, label: &'static str, icon: &'static str, which: Tab| {
            let selected = self.tab == which;
            div()
                .id(id)
                .w(px(72.))
                .py(px(4.))
                .flex()
                .flex_col()
                .items_center()
                .gap(px(2.))
                .rounded(px(6.))
                .cursor_pointer()
                .text_size(px(11.))
                .text_color(if selected { t.accent } else { t.secondary_label })
                .when(selected, |d| d.bg(t.fill_strong))
                .when(!selected, |d| d.hover(|s| s.bg(t.hover)))
                .child(ui::icon(icon, 18., if selected { t.accent } else { t.secondary_label }))
                .child(label)
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.tab = which;
                    this.page = None;
                    window.focus(&this.focus_handle);
                    cx.notify();
                }))
        };
        div()
            .flex_shrink_0()
            .pt(px(8.))
            .pb(px(8.))
            .flex()
            .justify_center()
            .gap(px(4.))
            .child(tab("tab-general", "General", "icons/settings.svg", Tab::General))
            .child(tab("tab-plugins", "Plugins", "icons/puzzle.svg", Tab::Plugins))
    }

    /// Picks a plugin (`.zip` from `delight package`, or a `.dylib`) and installs it.
    fn install(&self, window: &mut Window, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Install".into()),
        });
        cx.spawn_in(window, async move |_, cx| {
            let Ok(Ok(Some(paths))) = picked.await else { return };
            let Some(path) = paths.into_iter().next() else { return };
            let _ = cx.update(|window, cx| {
                let (level, title, detail) = match state::install_plugin(cx, &path) {
                    Ok(message) => (PromptLevel::Info, message, None),
                    Err(e) => (PromptLevel::Warning, "Couldn't install the plugin".to_string(), Some(format!("{e:#}"))),
                };
                let _answer = window.prompt(level, &title, detail.as_deref(), &["OK"], cx);
            });
        })
        .detach();
    }

    fn confirm_delete(&self, plugin_id: String, name: String, window: &mut Window, cx: &mut Context<Self>) {
        let answer = window.prompt(
            PromptLevel::Warning,
            &format!("Delete “{name}”?"),
            Some("This removes the plugin file, its settings and its saved secrets. It can't be undone."),
            &["Delete", "Cancel"],
            cx,
        );
        cx.spawn(async move |_, cx| {
            if answer.await == Ok(0) {
                let _ = cx.update(|cx| {
                    if let Err(e) = state::delete_plugin(cx, &plugin_id) {
                        log::error!("deleting plugin {plugin_id}: {e:#}");
                    }
                });
            }
        })
        .detach();
    }
}

impl Render for SettingsWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(window, cx);
        let body = match (self.tab, self.page.take()) {
            (_, Some(page)) => {
                let body = self.render_plugin_page(&t, &page, window, cx);
                self.page = Some(page);
                body
            }
            (Tab::General, None) => self.render_general(&t, cx),
            (Tab::Plugins, None) => self.render_plugins(&t, cx),
        };
        div()
            .key_context("Settings")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|_, _: &CloseSettings, window, _| window.remove_window()))
            .size_full()
            .flex()
            .flex_col()
            .bg(t.window_tint)
            .font_family(t.ui_font.clone())
            .text_color(t.label)
            .child(self.render_tabs(&t, cx))
            .child(ui::hairline(&t))
            .child(
                div()
                    .id("settings-scroll")
                    .flex_1()
                    .overflow_y_scroll()
                    .px(px(20.))
                    .py(px(18.))
                    .child(body),
            )
    }
}

fn dirs_home() -> String {
    std::env::var("HOME").unwrap_or_default()
}

