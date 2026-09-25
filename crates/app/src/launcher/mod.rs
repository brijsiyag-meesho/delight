//! The ⌘⇧Space launcher: a floating input bar that grows into a panel once
//! there's input — the tools that fit it on the left, the selected tool on
//! the right, its actions in the footer.
//!
//! * `window` — opening, showing and hiding the window.
//! * this file — the launcher's state and behaviour.
//! * `view` — drawing it.
//! * `footer` — which key runs which footer action.

mod footer;
mod view;
mod window;

use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::time::Duration;

use delight_core::stats::InputStats;
use delight_core::{Candidate, classify};
use delight_sdk::{Action, ActionKind, Input, ToolContext, ToolView};
use delight_ui::theme::{INPUT_FONT_SIZE, INPUT_LINE_HEIGHT};
use delight_ui::{EditorEvent, EditorFont, TextEditor};
use gpui::{
    App, AppContext, ClipboardItem, Context, Entity, FocusHandle, Focusable, KeyBinding, KeyDownEvent, ScrollHandle, SharedString,
    Subscription, Task, Window, actions, point, px,
};

pub use window::{hide, open, set_input, show, toast, toggle};

use self::footer::ActionKey;
use crate::platform;
use crate::state::{self, AppState};

/// The empty bar, and the panel once there's input.
const BAR_WIDTH: f32 = 640.;
const PANEL_WIDTH: f32 = 800.;
/// macOS Spotlight's search field: 56pt tall, a pill (fully rounded ends).
const BAR_HEIGHT: f32 = 56.;
const BAR_RADIUS: f32 = BAR_HEIGHT / 2.;
/// Spotlight's padding before the icon, and between the icon and the text.
const BAR_PADDING_X: f32 = 20.;
const BAR_ICON_GAP: f32 = 16.;
const BAR_ICON_SIZE: f32 = 22.;
const PANEL_HEIGHT: f32 = 540.;
const PANEL_RADIUS: f32 = 24.;
/// Typing pauses this long before the tools are asked again.
const CLASSIFY_DELAY: Duration = Duration::from_millis(30);
const TOAST: Duration = Duration::from_millis(1600);
const CRASH_TOAST: Duration = Duration::from_secs(8);
/// Clipboard text larger than this isn't auto-pasted.
const AUTO_PASTE_MAX_BYTES: usize = 1 << 20;

const CONTEXT: &str = "Launcher";

actions!(launcher, [Dismiss, RunPrimaryAction, SelectPrevious, SelectNext, ClearInput]);

/// ⌘1…⌘9: selects the nth tool in the list.
#[derive(Clone, Debug, PartialEq, gpui::Action)]
#[action(namespace = launcher, no_json)]
struct SelectTool(usize);

/// ⌥2…⌥9: runs the footer action with that key.
#[derive(Clone, Debug, PartialEq, gpui::Action)]
#[action(namespace = launcher, no_json)]
struct RunAltAction(usize);

pub fn bind_keys(cx: &mut App) {
    let context = Some(CONTEXT);
    cx.bind_keys([
        KeyBinding::new("escape", Dismiss, context),
        KeyBinding::new("enter", RunPrimaryAction, context),
        // Reach the launcher when the input's cursor can't move further.
        KeyBinding::new("up", SelectPrevious, context),
        KeyBinding::new("down", SelectNext, context),
        KeyBinding::new("ctrl-p", SelectPrevious, context),
        KeyBinding::new("ctrl-n", SelectNext, context),
        KeyBinding::new("cmd-k", ClearInput, context),
    ]);
    cx.bind_keys((1..=9).map(|n| KeyBinding::new(&format!("cmd-{n}"), SelectTool(n - 1), context)));
    cx.bind_keys((2..=9).map(|n| KeyBinding::new(&format!("alt-{n}"), RunAltAction(n), context)));
}

/// An operation's identity: `(plugin id, operation id)`.
type OperationKey = (String, String);

pub struct Launcher {
    focus_handle: FocusHandle,
    input: Entity<TextEditor>,
    /// The tools that fit the input, best first.
    candidates: Vec<Candidate>,
    selected: Option<usize>,
    /// The tool the user picked: it stays selected while the input changes,
    /// as long as it still fits.
    picked: Option<OperationKey>,
    /// The plugin that remembered the input completion showing now.
    completion_plugin: Option<String>,
    /// After a completion is accepted: select this plugin's tool once the
    /// tools are asked about the new input.
    prefer_plugin: Option<String>,
    /// The views opened so far, kept (with their state) while Delight runs.
    views: HashMap<OperationKey, Rc<dyn ToolView>>,
    stats: InputStats,
    toast: Option<SharedString>,
    toast_task: Option<Task<()>>,
    /// Replacing it cancels the previous classification.
    classify_task: Option<Task<()>>,
    list_scroll: ScrollHandle,
    detail_scroll: ScrollHandle,
    /// Whether the window is the panel (`Some(true)`) or the bar.
    expanded: Option<bool>,
    /// Clipboard text last auto-pasted, so an unchanged clipboard doesn't
    /// overwrite what was typed since.
    last_auto_paste: Option<String>,
    /// Plugins whose crash was already announced.
    announced_crashes: HashSet<String>,
    _subscriptions: Vec<Subscription>,
}

impl Launcher {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let state = cx.global::<AppState>();
        let restored = if state.settings.input_history { state.input_history.input_to_restore() } else { None };
        let restored = restored.unwrap_or_default().to_string();
        let input = cx.new(|cx| {
            let mut input = TextEditor::new(window, cx)
                .multiline(px(INPUT_LINE_HEIGHT * 4.))
                .font(EditorFont::Input)
                .text_size(px(INPUT_FONT_SIZE), px(INPUT_LINE_HEIGHT))
                .placeholder("What you got this time?");
            input.set_text(restored, cx);
            input
        });
        let subscriptions = vec![
            cx.subscribe(&input, |this, _, event, cx| this.on_input_event(event, cx)),
            cx.observe_window_appearance(window, |_, _, cx| delight_ui::theme::appearance_changed(cx)),
            cx.observe_window_activation(window, |_, window, cx| {
                let lost_focus = !window.is_window_active() && platform::is_window_visible(window);
                if lost_focus && state::settings(cx).hide_on_blur {
                    cx.defer(hide);
                }
            }),
        ];
        let mut launcher = Self {
            focus_handle: cx.focus_handle(),
            input,
            candidates: Vec::new(),
            selected: None,
            picked: None,
            completion_plugin: None,
            prefer_plugin: None,
            views: HashMap::new(),
            stats: InputStats::default(),
            toast: None,
            toast_task: None,
            classify_task: None,
            list_scroll: ScrollHandle::new(),
            detail_scroll: ScrollHandle::new(),
            expanded: None,
            last_auto_paste: None,
            announced_crashes: HashSet::new(),
            _subscriptions: subscriptions,
        };
        launcher.input_changed(cx);
        launcher
    }

    fn text(&self, cx: &App) -> String {
        self.input.read(cx).text().to_string()
    }

    fn is_expanded(&self, cx: &App) -> bool {
        !self.input.read(cx).text().trim().is_empty()
    }

    /// The bar while the input is empty, the panel otherwise; it grows down.
    fn sync_window_size(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let expanded = self.is_expanded(cx);
        if self.expanded == Some(expanded) {
            return;
        }
        let animate = self.expanded.is_some();
        self.expanded = Some(expanded);
        let (width, height) = if expanded { (PANEL_WIDTH, PANEL_HEIGHT) } else { (BAR_WIDTH, BAR_HEIGHT) };
        let radius = self.corner_radius();
        // Never resize mid-frame: GPUI would miss the resize.
        cx.defer_in(window, move |_, window, _| {
            platform::resize_keep_top(window, width.into(), height.into(), animate);
            platform::set_corner_radius(window, radius.into());
        });
    }

    /// A pill for the bar, a rounded panel once expanded.
    fn corner_radius(&self) -> f32 {
        if self.expanded == Some(true) { PANEL_RADIUS } else { BAR_RADIUS }
    }

    // -------------------------------------------------------------------------
    // Input
    // -------------------------------------------------------------------------

    fn on_input_event(&mut self, event: &EditorEvent, cx: &mut Context<Self>) {
        match event {
            EditorEvent::Changed => self.input_changed(cx),
            EditorEvent::CompletionAccepted => self.prefer_plugin = self.completion_plugin.take(),
            EditorEvent::Focus | EditorEvent::Blur => {}
        }
    }

    fn input_changed(&mut self, cx: &mut Context<Self>) {
        let text = self.text(cx);
        self.stats = InputStats::of(&text);
        if text.trim().is_empty() {
            self.picked = None;
        }
        self.show_completion(&text, cx);
        self.classify(cx);
        cx.notify();
    }

    /// Offers how a remembered input would complete `text` (Tab accepts).
    fn show_completion(&mut self, text: &str, cx: &mut Context<Self>) {
        let state = cx.global::<AppState>();
        let completion = if state.settings.input_history { state.input_history.completion_for(text) } else { None };
        self.completion_plugin = completion.map(|c| c.plugin_id.to_string());
        let remainder = completion.map(|c| SharedString::from(c.remainder.to_string()));
        self.input.update(cx, |input, cx| input.set_completion(remainder, cx));
    }

    /// Replaces the input with the clipboard's text, if it changed since the
    /// last time (and isn't blank or huge).
    fn paste_clipboard(&mut self, cx: &mut Context<Self>) {
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else { return };
        let unchanged = self.last_auto_paste.as_ref() == Some(&text);
        if text.trim().is_empty() || text.len() > AUTO_PASTE_MAX_BYTES || unchanged {
            return;
        }
        self.last_auto_paste = Some(text.clone());
        self.input.update(cx, |input, cx| input.set_text(text, cx));
    }

    fn clear_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.input.update(cx, |input, cx| input.set_text("", cx));
        window.focus(&self.input.focus_handle(cx));
    }

    // -------------------------------------------------------------------------
    // Tools
    // -------------------------------------------------------------------------

    /// Asks every tool about the input, in the background, once typing pauses.
    fn classify(&mut self, cx: &mut Context<Self>) {
        let input = Input::new(self.text(cx));
        let state = cx.global::<AppState>();
        let (registry, disabled) = (state.registry.clone(), state.settings.disabled_plugins.clone());
        self.classify_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(CLASSIFY_DELAY).await;
            let candidates = cx.background_executor().spawn(async move { classify(&input, &registry, &disabled) }).await;
            let _ = this.update(cx, |this, cx| this.show_candidates(candidates, cx));
        }));
    }

    /// Lists the tools and keeps the selection: the picked tool, else the
    /// best suggestion, else nothing.
    fn show_candidates(&mut self, candidates: Vec<Candidate>, cx: &mut Context<Self>) {
        self.candidates = candidates;
        if let Some(plugin_id) = self.prefer_plugin.take()
            && let Some(candidate) = self.candidates.iter().find(|c| c.plugin_id == plugin_id)
        {
            self.picked = Some(candidate.key());
        }
        let picked = self.picked.as_ref().and_then(|key| self.candidates.iter().position(|c| &c.key() == key));
        let best = self.candidates.first().filter(|c| c.suggested()).map(|_| 0);
        self.selected = picked.or(best);
        self.announce_crashes(cx);
        self.update_selected_view(cx);
        cx.notify();
    }

    fn selected_candidate(&self) -> Option<&Candidate> {
        self.candidates.get(self.selected?)
    }

    /// Selects the `index`th tool (the user picked it).
    fn select(&mut self, index: usize, cx: &mut Context<Self>) {
        if index >= self.candidates.len() || self.selected == Some(index) {
            return;
        }
        self.selected = Some(index);
        self.picked = Some(self.candidates[index].key());
        self.list_scroll.scroll_to_item(index);
        self.detail_scroll.set_offset(point(px(0.), px(0.)));
        self.update_selected_view(cx);
        cx.notify();
    }

    fn select_previous(&mut self, cx: &mut Context<Self>) {
        if let Some(index) = self.selected.and_then(|i| i.checked_sub(1)) {
            self.select(index, cx);
        }
    }

    fn select_next(&mut self, cx: &mut Context<Self>) {
        self.select(self.selected.map_or(0, |i| i + 1), cx);
    }

    /// The selected tool's view, opened on first use; `None` if its plugin
    /// has crashed.
    fn selected_view(&mut self, cx: &mut Context<Self>) -> Option<Rc<dyn ToolView>> {
        let key = self.selected_candidate()?.key();
        if let Some(view) = self.views.get(&key) {
            return Some(view.clone());
        }
        let registry = cx.global::<AppState>().registry.clone();
        let (plugin, _) = registry.operation(&key.0, &key.1)?;
        let view: Rc<dyn ToolView> = plugin.plugin.tool_view(&key.1, cx)?.into();
        self.views.insert(key, view.clone());
        Some(view)
    }

    /// Tells the selected tool's view what the input is now.
    fn update_selected_view(&mut self, cx: &mut Context<Self>) {
        let Some(view) = self.selected_view(cx) else { return };
        let Some(candidate) = self.selected_candidate() else { return };
        let context = ToolContext::new(candidate.operation_id.clone(), Input::new(self.text(cx)));
        view.update(&context, cx);
    }

    /// Why the plugin crashed in this run, if it did.
    fn crash_of(&self, plugin_id: &str, cx: &App) -> Option<String> {
        cx.global::<AppState>().registry.get(plugin_id)?.plugin.crash()
    }

    /// A toast for each plugin that crashed since the last check (a crash in
    /// `detect`: Delight keeps running without it).
    fn announce_crashes(&mut self, cx: &mut Context<Self>) {
        let registry = cx.global::<AppState>().registry.clone();
        for plugin in registry.plugins() {
            let manifest = plugin.manifest();
            if let Some(message) = plugin.plugin.crash()
                && self.announced_crashes.insert(manifest.id.clone())
            {
                let text = format!("{} crashed and is off until Delight restarts: {message}", manifest.name);
                self.flash_for(text, CRASH_TOAST, cx);
            }
        }
    }

    // -------------------------------------------------------------------------
    // Footer actions
    // -------------------------------------------------------------------------

    /// The selected tool's footer actions with their keys.
    fn keyed_actions(&self, cx: &App) -> Vec<(Action, ActionKey)> {
        let view = self.selected_candidate().and_then(|c| self.views.get(&c.key()));
        view.map(|view| footer::keyed(view.actions(cx))).unwrap_or_default()
    }

    fn run_action_with_key(&mut self, key: ActionKey, cx: &mut Context<Self>) {
        if let Some((action, _)) = self.keyed_actions(cx).into_iter().find(|(_, k)| *k == key) {
            self.perform(action, cx);
        }
    }

    /// Runs the footer action whose own shortcut was pressed.
    fn on_key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let pressed = &event.keystroke;
        let hit = self.keyed_actions(cx).into_iter().find(|(_, key)| match key {
            ActionKey::Own(own) => footer::matches(own, pressed),
            _ => false,
        });
        if let Some((action, _)) = hit {
            cx.stop_propagation();
            self.perform(action, cx);
        }
    }

    fn perform(&mut self, action: Action, cx: &mut Context<Self>) {
        match action.kind {
            ActionKind::Copy { text } => {
                cx.write_to_clipboard(ClipboardItem::new_string(text));
                self.flash(format!("{} — copied to clipboard", action.label), cx);
                if state::settings(cx).hide_after_copy {
                    cx.defer(hide);
                }
            }
            ActionKind::OpenUrl { url } => cx.open_url(&url),
            ActionKind::Custom => {
                let view = self.selected_candidate().and_then(|c| self.views.get(&c.key())).cloned();
                if let Some(view) = view {
                    view.perform(&action.id, cx);
                }
            }
            _ => log::warn!("unknown action kind for {:?}", action.id),
        }
    }

    // -------------------------------------------------------------------------
    // Status bar messages
    // -------------------------------------------------------------------------

    fn flash(&mut self, message: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.flash_for(message, TOAST, cx);
    }

    fn flash_for(&mut self, message: impl Into<SharedString>, duration: Duration, cx: &mut Context<Self>) {
        self.toast = Some(message.into());
        cx.notify();
        self.toast_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(duration).await;
            let _ = this.update(cx, |this, cx| {
                this.toast = None;
                cx.notify();
            });
        }));
    }
}
