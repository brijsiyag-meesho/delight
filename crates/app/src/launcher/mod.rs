//! The ⌘⇧Space launcher: a floating input bar that grows into a panel once
//! there's input — the tools that fit it on the left, the selected tool on
//! the right, its actions in the footer.
//!
//! * `window` — opening, showing and hiding the window.
//! * this file — the launcher's state and behaviour.
//! * `view` — drawing it.
//! * `footer` — which key runs which footer action.
//! * `history` — ⌃R, searching the input history.
//!
//! Keys come from the keymap (`crate::keymap`) by focus: the input
//! (`Launcher > Editor`) edits text, the tool list (`Launcher > ToolList`)
//! moves between tools, and `Launcher` keys work in both.

mod footer;
mod history;
mod view;
mod window;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use delight_core::stats::InputStats;
use delight_core::{Candidate, classify};
use delight_sdk::{Action, Input, ToolContext, ToolView};
use delight_ui::theme::{INPUT_FONT_SIZE, INPUT_LINE_HEIGHT};
use delight_ui::{EditorEvent, EditorFont, TextEditor};
use gpui::private::schemars::JsonSchema;
use gpui::{
    App, AppContext, Context, Entity, FocusHandle, Focusable, KeyDownEvent, Keystroke, ScrollHandle, SharedString,
    Subscription, Task, Window, actions, px,
};
use serde::Deserialize;

pub use window::{hide, open, plugins_reloaded, refresh, set_input, show, toast, toggle};

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
const TOOL_LIST_CONTEXT: &str = "ToolList";

actions!(
    launcher,
    [
        Dismiss,
        ClearInput,
        OpenSettings,
        /// Moves focus to the next field (the input, the tool list, the
        /// tool's own fields).
        FocusNext,
        FocusPrevious,
        /// Moves focus to the tool list.
        FocusTools,
        /// Moves focus from the tool list into the selected tool (its first
        /// field), if it has one.
        FocusTool,
        /// In the tool list; on the first tool, back to the input.
        SelectPrevious,
        SelectNext,
        /// Completes the input with an older remembered input.
        OlderCompletion,
        /// Completes the input with a newer remembered input.
        NewerCompletion,
    ]
);

/// Selects the nth tool in the list, from 1.
#[derive(Clone, Debug, PartialEq, Deserialize, JsonSchema, gpui::Action)]
#[action(namespace = launcher)]
#[schemars(crate = "gpui::private::schemars")]
struct SelectTool(usize);

/// An operation's identity: `(plugin id, operation id)`.
type OperationKey = (String, String);

pub struct Launcher {
    focus_handle: FocusHandle,
    input: Entity<TextEditor>,
    /// The files pasted into the input (⌘V on files copied in Finder),
    /// shown as tags under its text.
    files: Arc<[PathBuf]>,
    /// Their total size, for the status bar.
    files_bytes: u64,
    /// The tool list's focus (it's a Tab stop after the input).
    list_focus: FocusHandle,
    /// The tools that fit the input, best first.
    candidates: Vec<Candidate>,
    selected: Option<usize>,
    /// The tool the user picked: it stays selected while the input changes,
    /// as long as it still fits.
    picked: Option<OperationKey>,
    /// The tool the input completion showing now was remembered for.
    completion_tool: Option<OperationKey>,
    /// Which remembered input the completion shows: 0 the newest that fits,
    /// then older ones (⌃N next, ⌃P previous). Back to 0 as the input changes.
    completion_index: usize,
    /// After a remembered input is taken (a completion, or from ⌃R): the
    /// tool it was for, to select once the tools are asked about it.
    prefer_tool: Option<OperationKey>,
    /// The input history search (⌃R), while it's open.
    history: Option<history::HistorySearch>,
    /// The views opened so far, kept (with their state) while Delight runs.
    views: HashMap<OperationKey, Rc<dyn ToolView>>,
    stats: InputStats,
    toast: Option<SharedString>,
    toast_task: Option<Task<()>>,
    /// Replacing it cancels the previous classification.
    classify_task: Option<Task<()>>,
    list_scroll: ScrollHandle,
    /// The window's size (width, height), once set.
    size: Option<(f32, f32)>,
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
            files: Arc::default(),
            files_bytes: 0,
            list_focus: cx.focus_handle().tab_stop(true),
            candidates: Vec::new(),
            selected: None,
            picked: None,
            completion_tool: None,
            completion_index: 0,
            prefer_tool: None,
            history: None,
            views: HashMap::new(),
            stats: InputStats::default(),
            toast: None,
            toast_task: None,
            classify_task: None,
            list_scroll: ScrollHandle::new(),
            size: None,
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

    /// What the tools get: the text and the pasted files.
    fn input(&self, cx: &App) -> Input {
        Input::new(self.text(cx)).files(self.files.clone())
    }

    fn is_expanded(&self, cx: &App) -> bool {
        !self.input.read(cx).text().trim().is_empty() || !self.files.is_empty()
    }

    /// The bar while the input is empty, the panel otherwise. The history
    /// search (⌃R) keeps the width and drops down to the panel's height.
    fn wanted_size(&self, cx: &App) -> (f32, f32) {
        if self.history.is_some() {
            (self.size.map_or(BAR_WIDTH, |(width, _)| width), PANEL_HEIGHT)
        } else if self.is_expanded(cx) {
            (PANEL_WIDTH, PANEL_HEIGHT)
        } else {
            (BAR_WIDTH, BAR_HEIGHT)
        }
    }

    /// Resizes the window to [`Self::wanted_size`]; it grows down.
    fn sync_window_size(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let size = self.wanted_size(cx);
        if self.size == Some(size) {
            return;
        }
        let animate = self.size.is_some();
        self.size = Some(size);
        let (width, height) = size;
        let radius = self.corner_radius();
        // Never resize mid-frame: GPUI would miss the resize.
        cx.defer_in(window, move |_, window, _| {
            platform::resize_keep_top(window, width.into(), height.into(), animate);
            platform::set_corner_radius(window, radius.into());
        });
    }

    /// A pill for the bar, rounded corners once it's taller.
    fn corner_radius(&self) -> f32 {
        if self.size.is_some_and(|(_, height)| height > BAR_HEIGHT) { PANEL_RADIUS } else { BAR_RADIUS }
    }

    // -------------------------------------------------------------------------
    // Input
    // -------------------------------------------------------------------------

    fn on_input_event(&mut self, event: &EditorEvent, cx: &mut Context<Self>) {
        match event {
            EditorEvent::Changed => self.input_changed(cx),
            EditorEvent::CompletionAccepted => self.prefer_tool = self.completion_tool.take(),
            EditorEvent::Focus | EditorEvent::Blur => {}
        }
    }

    fn input_changed(&mut self, cx: &mut Context<Self>) {
        let text = self.text(cx);
        self.stats = InputStats::of(&text);
        if text.trim().is_empty() && self.files.is_empty() {
            self.picked = None;
        }
        self.completion_index = 0;
        self.show_completion(&text, cx);
        self.classify(cx);
        cx.notify();
    }

    /// Offers how a remembered input would complete `text` (Tab accepts).
    fn show_completion(&mut self, text: &str, cx: &mut Context<Self>) {
        let state = cx.global::<AppState>();
        let (history, index) = (&state.input_history, self.completion_index);
        let completion = if state.settings.input_history { history.completion_for(text, index) } else { None };
        self.completion_tool = completion.map(|c| (c.plugin_id.to_string(), c.operation_id.to_string()));
        let remainder = completion.map(|c| SharedString::from(c.remainder.to_string()));
        self.input.update(cx, |input, cx| input.set_completion(remainder, cx));
    }

    /// Completes with an older (`by` 1) or newer (-1) remembered input, if
    /// there's one.
    fn step_completion(&mut self, by: isize, cx: &mut Context<Self>) {
        let Some(index) = self.completion_index.checked_add_signed(by) else {
            return;
        };
        let text = self.text(cx);
        if cx.global::<AppState>().input_history.completion_for(&text, index).is_some() {
            self.completion_index = index;
            self.show_completion(&text, cx);
        }
    }

    /// Replaces the input with the clipboard's text, if it changed since the
    /// last time (and isn't blank or huge).
    fn paste_clipboard(&mut self, cx: &mut Context<Self>) {
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            return;
        };
        let unchanged = self.last_auto_paste.as_ref() == Some(&text);
        if text.trim().is_empty() || text.len() > AUTO_PASTE_MAX_BYTES || unchanged {
            return;
        }
        self.last_auto_paste = Some(text.clone());
        self.input.update(cx, |input, cx| input.set_text(text, cx));
    }

    fn clear_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.set_files(Vec::new(), cx);
        self.input.update(cx, |input, cx| input.set_text("", cx));
        window.focus(&self.input.focus_handle(cx));
    }

    /// ⌘V in the input with files on the clipboard: adds them to the input's
    /// files (the text stays). `false` if there are none: the input pastes text.
    fn paste_files(&mut self, window: &Window, cx: &mut Context<Self>) -> bool {
        if !self.input.focus_handle(cx).is_focused(window) {
            return false;
        }
        let pasted = platform::files_on_clipboard();
        if pasted.is_empty() {
            return false;
        }
        let mut files = self.files.to_vec();
        files.extend(pasted.into_iter().filter(|path| !self.files.contains(path)));
        self.set_files(files, cx);
        true
    }

    /// Backspace in an empty input removes the last file. `false` if it
    /// doesn't: the input handles it.
    fn backspace_file(&mut self, window: &Window, cx: &mut Context<Self>) -> bool {
        let input = self.input.read(cx);
        if !input.focus_handle(cx).is_focused(window) || !input.text().is_empty() || self.files.is_empty() {
            return false;
        }
        self.remove_file(self.files.len() - 1, cx);
        true
    }

    fn remove_file(&mut self, index: usize, cx: &mut Context<Self>) {
        let mut files = self.files.to_vec();
        if index < files.len() {
            files.remove(index);
            self.set_files(files, cx);
        }
    }

    fn set_files(&mut self, files: Vec<PathBuf>, cx: &mut Context<Self>) {
        if *self.files == *files {
            return;
        }
        self.files_bytes = files.iter().filter_map(|path| std::fs::metadata(path).ok()).map(|m| m.len()).sum();
        self.files = files.into();
        self.input_changed(cx);
    }

    // -------------------------------------------------------------------------
    // Tools
    // -------------------------------------------------------------------------

    /// Asks every tool about the input, in the background, once typing pauses.
    fn classify(&mut self, cx: &mut Context<Self>) {
        let input = self.input(cx);
        let state = cx.global::<AppState>();
        let (registry, disabled) = (state.registry.clone(), state.settings.disabled_plugins.clone());
        self.classify_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(CLASSIFY_DELAY).await;
            let candidates = cx.background_executor().spawn(async move { classify(&input, &registry, &disabled) }).await;
            let _ = this.update(cx, |this, cx| this.show_candidates(candidates, cx));
        }));
    }

    /// Lists the tools and keeps the selection: the picked tool, else the
    /// best recommendation, else nothing.
    fn show_candidates(&mut self, candidates: Vec<Candidate>, cx: &mut Context<Self>) {
        self.candidates = candidates;
        // The remembered input's tool; its plugin's first tool if it isn't
        // listed (or the input was remembered without its operation).
        if let Some(tool) = self.prefer_tool.take()
            && let Some(candidate) = self
                .candidates
                .iter()
                .find(|c| c.key() == tool)
                .or_else(|| self.candidates.iter().find(|c| c.plugin_id == tool.0))
        {
            self.picked = Some(candidate.key());
        }
        let picked = self.picked.as_ref().and_then(|key| self.candidates.iter().position(|c| &c.key() == key));
        let best = self.candidates.first().filter(|c| c.recommended()).map(|_| 0);
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
        self.update_selected_view(cx);
        cx.notify();
    }

    /// On the first tool (or none), back to the input.
    fn select_previous(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.selected.and_then(|i| i.checked_sub(1)) {
            Some(index) => self.select(index, cx),
            None => window.focus(&self.input.focus_handle(cx)),
        }
    }

    fn select_next(&mut self, cx: &mut Context<Self>) {
        self.select(self.selected.map_or(0, |i| i + 1), cx);
    }

    /// Moves focus to the tool list, selecting the first tool if none is.
    fn focus_tools(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.candidates.is_empty() {
            return;
        }
        if self.selected.is_none() {
            self.select(0, cx);
        }
        window.focus(&self.list_focus);
        cx.notify();
    }

    /// Into the selected tool: the next Tab stop after the list. A tool
    /// without one would send focus round to the input: stay in the list.
    fn focus_tool(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        window.focus_next();
        if self.input.focus_handle(cx).is_focused(window) {
            window.focus(&self.list_focus);
        }
        cx.notify();
    }

    /// Typing while the tool list has focus goes on in the input.
    fn on_list_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        let Some(text) = keystroke.key_char.clone() else {
            return;
        };
        if keystroke.modifiers.platform || keystroke.modifiers.control {
            return;
        }
        cx.stop_propagation();
        window.focus(&self.input.focus_handle(cx));
        self.input.update(cx, |input, cx| input.insert(&text, cx));
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
        let Some(view) = self.selected_view(cx) else {
            return;
        };
        let Some(candidate) = self.selected_candidate() else {
            return;
        };
        let context = ToolContext::new(candidate.operation_id.clone(), self.input(cx));
        view.update(&context, cx);
    }

    fn has_settings(&self, plugin_id: &str, cx: &App) -> bool {
        cx.global::<AppState>().registry.get(plugin_id).is_some_and(|p| p.plugin.has_settings())
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

    /// The selected tool's footer actions with their keys. An action's
    /// shortcut gives way to the keymap where the focus is.
    fn keyed_actions(&self, window: &Window, cx: &App) -> Vec<(Action, Option<Keystroke>)> {
        let Some(view) = self.selected_candidate().and_then(|c| self.views.get(&c.key())) else {
            return Vec::new();
        };
        let keymap = cx.key_bindings();
        let keymap = keymap.borrow();
        let context = window.context_stack();
        let is_bound = |keystroke: &Keystroke| !keymap.bindings_for_input(&[keystroke.clone()], &context).0.is_empty();
        footer::keyed(view.actions(cx), is_bound)
    }

    /// Runs the footer action whose shortcut was pressed (the keymap had no
    /// binding for it).
    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let pressed = &event.keystroke;
        let hit = self
            .keyed_actions(window, cx)
            .into_iter()
            .find(|(_, key)| key.as_ref().is_some_and(|own| footer::matches(own, pressed)));
        if let Some((action, _)) = hit {
            cx.stop_propagation();
            self.perform(action, cx);
        }
    }

    /// Runs a footer action: the tool's view does what it's for.
    fn perform(&mut self, action: Action, cx: &mut Context<Self>) {
        let view = self.selected_candidate().and_then(|c| self.views.get(&c.key())).cloned();
        if let Some(view) = view {
            view.perform(&action.id, cx);
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
