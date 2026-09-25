//! The Plugins tab — every plugin, turned on or off, deleted, or opened on
//! its own settings page — and the plugins folder.

use delight_core::registry::{LoadedPlugin, PluginSource};
use delight_ui::{ActiveTheme, Button, Icon, IconButton, IconName, LogoBadge, Switch, h_flex, v_flex};
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, ParentElement, PathPromptOptions, PromptLevel, Styled, Window, div,
    prelude::*, px,
};

use super::{PluginPage, SettingsWindow, section};
use crate::boundary::PanicBoundary;
use crate::state::{self, AppState};
use crate::{install, lifecycle};

impl SettingsWindow {
    pub(super) fn render_plugins(&self, cx: &mut Context<Self>) -> AnyElement {
        let registry = cx.global::<AppState>().registry.clone();
        let mut rows: Vec<AnyElement> =
            registry.plugins().iter().enumerate().map(|(i, plugin)| self.plugin_row(i, plugin, cx)).collect();
        rows.push(self.folder_row(cx));
        let t = cx.theme();
        let errors = registry.load_errors.clone();
        let warning = t.status.warning.clone();
        v_flex()
            .gap(px(18.))
            .child(section("Plugins", rows))
            .when(!errors.is_empty(), |d| {
                d.child(
                    v_flex()
                        .gap(px(4.))
                        .px(px(12.))
                        .py(px(9.))
                        .rounded(px(8.))
                        .bg(warning.bg)
                        .text_size(px(11.5))
                        .child(div().font_weight(FontWeight::SEMIBOLD).child("Some plugins failed to load"))
                        .children(errors.into_iter().map(|e| div().text_color(t.colors.secondary_label).child(e))),
                )
            })
            .into_any_element()
    }

    /// Logo, name, what it is, why it's off (if it crashed), then ⚙, 🗑 and
    /// the on/off switch.
    fn plugin_row(&self, index: usize, plugin: &LoadedPlugin, cx: &mut Context<Self>) -> AnyElement {
        let manifest = plugin.manifest();
        let settings = state::settings(cx);
        let enabled = !settings.disabled_plugins.contains(&manifest.id);
        // Crashed in this run (off until restart), or turned off after
        // crashing an earlier run (until turned back on).
        let crash = match (plugin.plugin.crash(), settings.crashed_plugins.get(&manifest.id)) {
            (Some(message), _) => Some(format!("Crashed: {message} — off until Delight restarts")),
            (None, Some(message)) if !enabled => Some(format!("Turned off after a crash: {message}")),
            _ => None,
        };
        let source = match plugin.source {
            PluginSource::Builtin => "Built-in",
            PluginSource::Installed { .. } => "Plugin",
        };
        let tools = match manifest.operations.len() {
            1 => "1 tool".to_string(),
            n => format!("{n} tools"),
        };
        let t = cx.theme().clone();
        let (id, name) = (manifest.id.clone(), manifest.name.clone());
        let deletable = matches!(plugin.source, PluginSource::Installed { .. });
        h_flex()
            .gap(px(10.))
            .px(px(12.))
            .py(px(8.))
            .child(LogoBadge::new(manifest.icon_svg).size(px(26.)))
            .child(
                v_flex()
                    .flex_1()
                    .min_w(px(0.))
                    .child(
                        h_flex()
                            .gap(px(6.))
                            .child(div().child(name.clone()))
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(t.colors.tertiary_label)
                                    .child(format!("{source} · v{} · {tools}", manifest.version)),
                            ),
                    )
                    .child(div().text_size(px(11.)).text_color(t.colors.secondary_label).truncate().child(manifest.description.clone()))
                    .when_some(crash, |d, crash| d.child(div().text_size(px(11.)).text_color(t.status.error.fg).child(crash))),
            )
            .when(plugin.plugin.has_settings(), |row| {
                let id = id.clone();
                row.child(
                    IconButton::new(("plugin-settings", index), IconName::Settings)
                        .on_click(cx.listener(move |this, _, window, cx| this.navigate(Some(id.clone()), window, cx))),
                )
            })
            .when(deletable, |row| {
                let (id, name) = (id.clone(), name.clone());
                row.child(IconButton::new(("plugin-delete", index), IconName::Trash).on_click(cx.listener(
                    move |this, _, window, cx| this.confirm_delete(id.clone(), name.clone(), window, cx),
                )))
            })
            .child(Switch::new(("plugin", index)).checked(enabled).on_change(move |on, _, cx| {
                let id = id.clone();
                let on = *on;
                state::update_settings(cx, move |s| {
                    if on {
                        s.disabled_plugins.remove(&id);
                        s.crashed_plugins.remove(&id);
                    } else {
                        s.disabled_plugins.insert(id);
                    }
                });
            }))
            .into_any_element()
    }

    /// The plugins folder, with Install…, Open Folder and Restart.
    fn folder_row(&self, cx: &mut Context<Self>) -> AnyElement {
        let dir = state::settings(cx).effective_plugin_dir();
        let home = dirs::home_dir().map(|h| h.display().to_string()).unwrap_or_default();
        let shown = dir.display().to_string().replacen(&home, "~", 1);
        let t = cx.theme();
        h_flex()
            .gap(px(8.))
            .px(px(12.))
            .py(px(9.))
            .child(Icon::new(IconName::Folder).size(px(14.)).color(t.colors.secondary_label))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .text_size(px(11.5))
                    .font_family(t.text.mono_font.clone())
                    .text_color(t.colors.secondary_label)
                    .truncate()
                    .child(shown),
            )
            .child(Button::new("install", "Install…").on_click(cx.listener(|this, _, window, cx| this.install(window, cx))))
            .child(Button::new("open-folder", "Open Folder").on_click(move |_, _, cx| {
                let _ = std::fs::create_dir_all(&dir);
                cx.open_with_system(&dir);
            }))
            .child(Button::new("restart", "Restart").icon(IconName::RefreshCw).on_click(|_, _, cx| cx.defer(lifecycle::restart)))
            .into_any_element()
    }

    /// The plugin's own settings page, under a header.
    pub(super) fn render_plugin_page(&self, page: &PluginPage, cx: &mut Context<Self>) -> AnyElement {
        let t = cx.theme().clone();
        let back = div()
            .id("back")
            .text_size(px(12.))
            .text_color(t.colors.accent)
            .cursor_pointer()
            .child("‹ All Plugins")
            .on_click(cx.listener(|this, _, window, cx| this.navigate(None, window, cx)));
        let registry = cx.global::<AppState>().registry.clone();
        let Some(plugin) = registry.get(&page.plugin_id) else {
            return v_flex().gap(px(18.)).child(back).child("This plugin is no longer loaded.").into_any_element();
        };
        let manifest = plugin.manifest();
        let header = h_flex()
            .gap(px(12.))
            .child(LogoBadge::new(manifest.icon_svg).size(px(36.)))
            .child(
                v_flex()
                    .min_w(px(0.))
                    .child(div().text_size(px(15.)).font_weight(FontWeight::SEMIBOLD).child(manifest.name.clone()))
                    .child(div().text_size(px(11.5)).text_color(t.colors.secondary_label).child(manifest.description.clone())),
            );
        let body: AnyElement = match &page.view {
            Some(view) => {
                PanicBoundary::new(view.view().into_any_element(), manifest.id.clone(), manifest.name.clone()).into_any_element()
            }
            None => div().text_color(t.colors.secondary_label).child("This plugin has no settings.").into_any_element(),
        };
        v_flex().gap(px(18.)).child(back).child(header).child(body).into_any_element()
    }

    /// Picks a plugin (`.zip` or `.dylib`) and installs it.
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
                let (level, title, detail) = match install::install_plugin(cx, &path) {
                    Ok(message) => (PromptLevel::Info, message, None),
                    Err(e) => (PromptLevel::Warning, "Couldn't install the plugin".to_string(), Some(format!("{e:#}"))),
                };
                let _ = window.prompt(level, &title, detail.as_deref(), &["OK"], cx);
            });
        })
        .detach();
    }

    fn confirm_delete(&self, plugin_id: String, name: String, window: &mut Window, cx: &mut Context<Self>) {
        let answer = window.prompt(
            PromptLevel::Warning,
            &format!("Delete “{name}”?"),
            Some("This removes the plugin, its settings, its data and its saved secrets. It can't be undone."),
            &["Delete", "Cancel"],
            cx,
        );
        cx.spawn(async move |_, cx| {
            if answer.await == Ok(0) {
                let _ = cx.update(|cx| {
                    if let Err(e) = state::delete_plugin(cx, &plugin_id) {
                        log::error!("deleting {plugin_id}: {e:#}");
                    }
                });
            }
        })
        .detach();
    }
}
