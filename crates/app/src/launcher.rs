//! The ⌘⇧Space launcher: a compact floating input bar (quick-entry style)
//! that expands downward into tools + results once there's input.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use delight_core::{Candidate, classifier};
use delight_core::stats::InputStats;
use delight_sdk::{
    Action as ToolAction, ActionKind, Block, FieldKind, FormField, NoticeLevel, OperationSpec, Params, Plugin, RunRequest,
    ToolContext, ToolOutput, ToolView,
};
use gpui::{
    AnyElement, App, AppContext, Bounds, ClipboardItem, Context, Entity, FocusHandle, Focusable, FontWeight,
    KeyBinding, ScrollHandle, SharedString, StyledText, Subscription, Task, Window, WindowBackgroundAppearance,
    WindowBounds, WindowHandle, WindowKind, WindowOptions, actions, div, point, prelude::*, px, size,
};

use delight_sdk::editor::{EditorEvent, TextEditor};
use crate::highlight::{Syntax, highlight};
use crate::state::{self, AppState};
use delight_sdk::theme::{INPUT_FONT_SIZE, INPUT_LINE_HEIGHT, Theme, theme};
use delight_sdk::ui;
use crate::boundary::PanicBoundary;
use crate::{platform, settings_window};

/// Empty bar, and the panel once there's input.
pub const BAR_WIDTH: f32 = 640.;
pub const EXPANDED_WIDTH: f32 = 800.;
/// 14pt padding + one input line (`INPUT_LINE_HEIGHT`) + 14pt padding.
pub const BAR_HEIGHT: f32 = 14. + INPUT_LINE_HEIGHT + 14.;
pub const EXPANDED_HEIGHT: f32 = 540.;
const RADIUS: f32 = 16.;
const LIST_WIDTH: f32 = 200.;
/// Output code blocks are clipped for rendering; actions always carry the full text.
const MAX_RENDER_BYTES: usize = 64 * 1024;

actions!(
    launcher,
    [
        Dismiss,
        PrimaryAction,
        SelectPrev,
        SelectNext,
        ClearInput,
        OpenSettings,
        FocusNextField,
        FocusPrevField,
        PrevMode,
        NextMode,
        Tool1,
        Tool2,
        Tool3,
        Tool4,
        Tool5,
        Tool6,
        Tool7,
        Tool8,
        Tool9,
        Act2,
        Act3,
        Act4,
        Act5,
        Act6,
    ]
);

const CONTEXT: &str = "Launcher";

pub fn bind_keys(cx: &mut App) {
    let c = Some(CONTEXT);
    cx.bind_keys([
        KeyBinding::new("escape", Dismiss, c),
        KeyBinding::new("enter", PrimaryAction, c),
        KeyBinding::new("up", SelectPrev, c),
        KeyBinding::new("down", SelectNext, c),
        KeyBinding::new("alt-up", SelectPrev, c),
        KeyBinding::new("alt-down", SelectNext, c),
        KeyBinding::new("ctrl-p", SelectPrev, c),
        KeyBinding::new("ctrl-n", SelectNext, c),
        KeyBinding::new("cmd-k", ClearInput, c),
        KeyBinding::new("cmd-,", OpenSettings, c),
        KeyBinding::new("tab", FocusNextField, c),
        KeyBinding::new("shift-tab", FocusPrevField, c),
        // ←/→ reach the launcher only when the input's cursor is at an edge.
        KeyBinding::new("left", PrevMode, c),
        KeyBinding::new("right", NextMode, c),
        KeyBinding::new("cmd-shift-[", PrevMode, c),
        KeyBinding::new("cmd-shift-]", NextMode, c),
        KeyBinding::new("cmd-1", Tool1, c),
        KeyBinding::new("cmd-2", Tool2, c),
        KeyBinding::new("cmd-3", Tool3, c),
        KeyBinding::new("cmd-4", Tool4, c),
        KeyBinding::new("cmd-5", Tool5, c),
        KeyBinding::new("cmd-6", Tool6, c),
        KeyBinding::new("cmd-7", Tool7, c),
        KeyBinding::new("cmd-8", Tool8, c),
        KeyBinding::new("cmd-9", Tool9, c),
        KeyBinding::new("alt-2", Act2, c),
        KeyBinding::new("alt-3", Act3, c),
        KeyBinding::new("alt-4", Act4, c),
        KeyBinding::new("alt-5", Act5, c),
        KeyBinding::new("alt-6", Act6, c),
    ]);
}

type OpKey = (String, String);

/// Per-operation form: text fields are editors, the rest plain values.
struct FormState {
    values: Params,
    editors: Vec<(String, Entity<TextEditor>)>,
    _subs: Vec<Subscription>,
}

enum RunResult {
    Output(ToolOutput),
    Error(String),
}

pub struct Launcher {
    focus_handle: FocusHandle,
    input: Entity<TextEditor>,
    /// Suggestions for the input (the first `matched`), then every other tool.
    candidates: Vec<Candidate>,
    matched: usize,
    /// Index into `candidates`; out of range when nothing is selected.
    selected: usize,
    /// Set when the user explicitly picked a tool; survives re-classification.
    pinned: Option<OpKey>,
    forms: HashMap<OpKey, FormState>,
    /// Custom views of operations that have one (`None`: uses `run`).
    views: HashMap<OpKey, Option<Rc<dyn ToolView>>>,
    /// The mode each operation was last switched to from a detection's
    /// suggestion — so a tab the user picks stays until the suggestion changes.
    suggested_modes: HashMap<OpKey, String>,
    result: Option<(OpKey, RunResult)>,
    running: bool,
    stats: InputStats,
    toast: Option<SharedString>,
    generation: u64,
    run_generation: u64,
    classify_task: Option<Task<()>>,
    run_task: Option<Task<()>>,
    toast_task: Option<Task<()>>,
    detail_scroll: ScrollHandle,
    list_scroll: ScrollHandle,
    expanded: Option<bool>,
    /// Clipboard text last put into the input on open, so an unchanged
    /// clipboard doesn't overwrite what was typed since.
    last_auto_paste: Option<String>,
    /// Plugins whose crash was already announced with a toast.
    announced_crashes: HashSet<String>,
    _subs: Vec<Subscription>,
}

// ---------------------------------------------------------------------------
// Window management
// ---------------------------------------------------------------------------

pub fn open(cx: &mut App) -> anyhow::Result<WindowHandle<Launcher>> {
    let display = cx.primary_display();
    let screen = display.as_ref().map(|d| d.bounds()).unwrap_or(Bounds::new(point(px(0.), px(0.)), size(px(1440.), px(900.))));
    // Spotlight sits roughly a quarter of the way down the screen.
    let origin = point(
        screen.origin.x + (screen.size.width - px(BAR_WIDTH)) / 2.,
        screen.origin.y + screen.size.height * 0.22,
    );
    let bounds = Bounds::new(origin, size(px(BAR_WIDTH), px(BAR_HEIGHT)));
    let handle = cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: None,
            focus: true,
            show: true,
            kind: WindowKind::PopUp,
            is_movable: true,
            is_resizable: false,
            is_minimizable: false,
            display_id: display.map(|d| d.id()),
            window_background: WindowBackgroundAppearance::Blurred,
            ..Default::default()
        },
        |window, cx| {
            platform::patch_gpui_focus();
            platform::style_floating_panel(window, RADIUS as f64);
            cx.new(|cx| Launcher::new(window, cx))
        },
    )?;
    handle.update(cx, |view, window, cx| {
        window.focus(&view.input.focus_handle(cx));
        view.sync_window_size(window, cx);
        // Why the previous run ended, if it crashed.
        if let Some(note) = delight_core::guard::take_crash_note() {
            view.flash_for(note, CRASH_TOAST, cx);
        }
    })?;
    Ok(handle)
}

pub fn toggle(cx: &mut App) {
    let Some(handle) = cx.global::<AppState>().launcher else { return };
    let visible_and_key = handle
        .update(cx, |_, window, _| platform::is_window_visible(window) && window.is_window_active())
        .unwrap_or(false);
    if visible_and_key { hide(cx) } else { show(cx) }
}

pub fn show(cx: &mut App) {
    let Some(handle) = cx.global::<AppState>().launcher else { return };
    let native = handle
        .update(cx, |view, window, cx| {
            view.sync_window_size(window, cx);
            platform::native_window(window)
        })
        .ok()
        .flatten();
    let Some(native) = native else { return };
    // Present outside the GPUI update (AppKit calls back into GPUI), then focus
    // the input once the window is key — focus set before that doesn't stick.
    cx.spawn(async move |cx| {
        platform::present(native);
        let _ = handle.update(cx, |view, window, cx| {
            let input = view.input.clone();
            window.focus(&input.focus_handle(cx));
            if state::settings(cx).paste_clipboard_on_open {
                view.paste_clipboard(cx);
            }
            // Like Spotlight: the previous text is kept and selected, so typing replaces it.
            input.update(cx, |e, cx| e.select_all_text(cx));
        });
    })
    .detach();
}

/// Brief confirmation in the status bar (used by plugins through the host API).
pub fn toast(cx: &mut App, message: SharedString) {
    let Some(handle) = cx.global::<AppState>().launcher else { return };
    let _ = handle.update(cx, |view, _, cx| view.flash(message, cx));
}

pub fn hide(cx: &mut App) {
    let Some(handle) = cx.global::<AppState>().launcher else { return };
    let text = handle.update(cx, |view, window, cx| {
        platform::hide_window(window);
        view.input.read(cx).text().to_string()
    });
    if let Ok(text) = text {
        let state = cx.global_mut::<AppState>();
        if state.settings.remember_input {
            state.settings.last_input = text;
            let _ = state.settings.save();
        }
    }
}

// ---------------------------------------------------------------------------
// Behaviour
// ---------------------------------------------------------------------------

/// Clipboard text larger than this isn't auto-pasted.
const AUTO_PASTE_MAX_BYTES: usize = 1 << 20;

/// How long a crash is shown in the status bar.
const CRASH_TOAST: Duration = Duration::from_secs(8);

impl Launcher {
    /// "Auto-paste clipboard": replaces the input with the clipboard's text when
    /// it changed since the last auto-paste (and isn't blank or huge).
    fn paste_clipboard(&mut self, cx: &mut Context<Self>) {
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else { return };
        if text.trim().is_empty() || text.len() > AUTO_PASTE_MAX_BYTES || self.last_auto_paste.as_ref() == Some(&text) {
            return;
        }
        self.last_auto_paste = Some(text.clone());
        if self.input.read(cx).text() != text {
            self.input.update(cx, |e, cx| e.set_text(text, cx));
        }
    }

    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let initial = {
            let s = state::settings(cx);
            if s.remember_input { s.last_input.clone() } else { String::new() }
        };
        let input = cx.new(|cx| {
            let mut e = TextEditor::new(cx)
                .multiline(px(INPUT_LINE_HEIGHT * 4.))
                .input_font()
                .text_size(px(INPUT_FONT_SIZE), px(INPUT_LINE_HEIGHT))
                .placeholder("What you got this time?");
            e.set_text(initial, cx);
            e
        });
        let mut subs = vec![
            cx.subscribe(&input, |this, _, _: &EditorEvent, cx| this.on_input_changed(cx)),
            cx.observe_global::<AppState>(|this, cx| this.schedule_classify(cx)),
            cx.observe_window_appearance(window, |_, _, cx| cx.notify()),
        ];
        subs.push(cx.observe_window_activation(window, |_, window, cx| {
            let (active, visible) = (window.is_window_active(), platform::is_window_visible(window));
            log::debug!("launcher activation changed: active={active} visible={visible}");
            if !active && state::settings(cx).hide_on_blur && visible {
                log::debug!("hiding launcher: lost focus");
                cx.defer(hide);
            }
        }));
        let mut this = Self {
            focus_handle: cx.focus_handle(),
            input,
            candidates: Vec::new(),
            matched: 0,
            selected: 0,
            pinned: None,
            forms: HashMap::new(),
            views: HashMap::new(),
            suggested_modes: HashMap::new(),
            result: None,
            running: false,
            stats: InputStats::default(),
            toast: None,
            generation: 0,
            run_generation: 0,
            classify_task: None,
            run_task: None,
            toast_task: None,
            detail_scroll: ScrollHandle::new(),
            list_scroll: ScrollHandle::new(),
            expanded: None,
            last_auto_paste: None,
            announced_crashes: HashSet::new(),
            _subs: subs,
        };
        this.on_input_changed(cx);
        this
    }

    fn text(&self, cx: &App) -> String {
        self.input.read(cx).text().to_string()
    }

    fn is_expanded(&self, cx: &App) -> bool {
        !self.input.read(cx).text().trim().is_empty()
    }

    /// Compact bar when empty; full panel otherwise. Grows downward.
    fn sync_window_size(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let want = self.is_expanded(cx);
        if self.expanded != Some(want) {
            let first = self.expanded.is_none();
            self.expanded = Some(want);
            let (w, h) = if want { (EXPANDED_WIDTH, EXPANDED_HEIGHT) } else { (BAR_WIDTH, BAR_HEIGHT) };
            // Never resize mid-frame: GPUI would miss the resize callback.
            cx.defer_in(window, move |_, window, _| platform::resize_keep_top(window, w as f64, h as f64, !first));
        }
    }

    fn on_input_changed(&mut self, cx: &mut Context<Self>) {
        let text = self.text(cx);
        self.stats = InputStats::of(&text);
        if text.trim().is_empty() {
            self.pinned = None;
        }
        self.schedule_classify(cx);
        cx.notify();
    }

    fn schedule_classify(&mut self, cx: &mut Context<Self>) {
        self.generation += 1;
        let generation = self.generation;
        let text = self.text(cx);
        let state = cx.global::<AppState>();
        let (registry, router, disabled) =
            (state.registry.clone(), state.router.clone(), state.settings.disabled_plugins.clone());
        self.classify_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_millis(30)).await;
            let (matched, others) = cx
                .background_executor()
                .spawn(async move {
                    let matched = router.classify(&text, &registry, &disabled);
                    let others = if matched.is_empty() && text.trim().is_empty() {
                        Vec::new()
                    } else {
                        classifier::remaining(&registry, &disabled, &matched)
                    };
                    (matched, others)
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.generation == generation {
                    this.apply_candidates(matched, others, cx);
                }
            });
        }));
    }

    fn apply_candidates(&mut self, matched: Vec<Candidate>, others: Vec<Candidate>, cx: &mut Context<Self>) {
        let previous = self.candidates.get(self.selected).map(Candidate::key);
        self.matched = matched.len();
        self.candidates = matched;
        self.candidates.extend(others);
        let find = |key: &Option<OpKey>| key.as_ref().and_then(|k| self.candidates.iter().position(|c| &c.key() == k));
        // Without a pick, the best suggestion — or nothing when none matched.
        let fallback = if self.matched > 0 { 0 } else { self.candidates.len() };
        self.selected = find(&self.pinned).or_else(|| find(&previous).filter(|_| self.pinned.is_some())).unwrap_or(fallback);
        if self.selected_candidate().is_none() {
            self.result = None;
        }
        self.announce_crashes(cx);
        self.schedule_run(cx);
        cx.notify();
    }

    /// A toast for each plugin that crashed since the last check (a crash
    /// caught on a background thread: Delight keeps running without it).
    fn announce_crashes(&mut self, cx: &mut Context<Self>) {
        let registry = cx.global::<AppState>().registry.clone();
        for p in registry.plugins() {
            let id = &p.manifest().id;
            if let Some(message) = p.crash()
                && self.announced_crashes.insert(id.clone())
            {
                let name = &p.manifest().name;
                self.flash_for(format!("{name} crashed and is off until Delight restarts: {message}"), CRASH_TOAST, cx);
            }
        }
    }

    /// Why the plugin crashed in this process, if it did.
    fn crash_of(&self, plugin_id: &str, cx: &App) -> Option<String> {
        cx.global::<AppState>().registry.get(plugin_id)?.crash()
    }

    fn selected_candidate(&self) -> Option<&Candidate> {
        self.candidates.get(self.selected)
    }

    fn select(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.candidates.len() && index != self.selected {
            self.selected = index;
            self.pinned = Some(self.candidates[index].key());
            self.list_scroll.scroll_to_item(index);
            self.detail_scroll.set_offset(point(px(0.), px(0.)));
            self.schedule_run(cx);
            cx.notify();
        }
    }

    fn spec(&self, cx: &App, key: &OpKey) -> Option<(Arc<dyn Plugin>, OperationSpec)> {
        let registry = &cx.global::<AppState>().registry;
        let (p, op) = registry.operation(&key.0, &key.1)?;
        Some((p.plugin.clone(), op.clone()))
    }

    fn ensure_form(&mut self, key: &OpKey, op: &OperationSpec, cx: &mut Context<Self>) {
        if self.forms.contains_key(key) {
            return;
        }
        let mut values = Params::new();
        let mut editors = Vec::new();
        let mut subs = Vec::new();
        for field in &op.params {
            let default = field.default.clone().unwrap_or_default();
            match &field.kind {
                FieldKind::Text | FieldKind::Secret | FieldKind::Number | FieldKind::Multiline => {
                    let masked = field.kind == FieldKind::Secret;
                    let multiline = field.kind == FieldKind::Multiline;
                    let placeholder = field.placeholder.clone().unwrap_or_default();
                    let editor = cx.new(|cx| {
                        let mut e = TextEditor::new(cx).masked(masked).mono(multiline).placeholder(placeholder);
                        if multiline {
                            e = e.multiline(px(18. * 4.));
                        }
                        e.set_text(default, cx);
                        e
                    });
                    subs.push(cx.subscribe(&editor, |this, _, _: &EditorEvent, cx| this.schedule_run(cx)));
                    editors.push((field.key.clone(), editor));
                }
                _ => {
                    values.insert(field.key.clone(), default);
                }
            }
        }
        self.forms.insert(key.clone(), FormState { values, editors, _subs: subs });
    }

    fn params(&self, key: &OpKey, cx: &App) -> Params {
        let Some(form) = self.forms.get(key) else { return Params::new() };
        let mut p = form.values.clone();
        for (k, e) in &form.editors {
            p.insert(k.clone(), e.read(cx).text().to_string());
        }
        p
    }

    fn schedule_run(&mut self, cx: &mut Context<Self>) {
        let Some(c) = self.selected_candidate() else {
            self.run_task = None;
            self.running = false;
            return;
        };
        let (key, suggested) = (c.key(), c.mode.clone());
        let Some((plugin, op)) = self.spec(cx, &key) else { return };
        self.ensure_form(&key, &op, cx);
        // Open the tab the plugin suggests for this input, once per suggestion.
        if let (Some(mode), Some(field)) = (suggested, op.mode_field())
            && self.suggested_modes.get(&key) != Some(&mode)
            && let Some(form) = self.forms.get_mut(&key)
        {
            form.values.insert(field.key.clone(), mode.clone());
            self.suggested_modes.insert(key.clone(), mode);
        }
        let settings = state::plugin_settings(cx, &key.0);
        if let Some(view) = self.tool_view(&key, &plugin, cx) {
            self.run_task = None;
            self.running = false;
            let context = ToolContext::new(key.1.clone(), self.text(cx), settings);
            view.update(&context, cx);
            cx.notify();
            return;
        }
        let request = RunRequest::new(key.1.clone(), self.text(cx)).params(self.params(&key, cx)).settings(settings);
        self.run_generation += 1;
        let generation = self.run_generation;
        self.running = true;
        let delay = Duration::from_millis(op.run_delay_ms.into());
        self.run_task = Some(cx.spawn(async move |this, cx| {
            if !delay.is_zero() {
                cx.background_executor().timer(delay).await;
            }
            let result = cx
                .background_executor()
                .spawn(async move {
                    match plugin.run(&request) {
                        Ok(out) => RunResult::Output(out),
                        Err(e) => RunResult::Error(e.to_string()),
                    }
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.run_generation == generation {
                    this.result = Some((key, result));
                    this.running = false;
                    this.announce_crashes(cx);
                    cx.notify();
                }
            });
        }));
    }

    /// The operation's custom view, created on first use.
    fn tool_view(&mut self, key: &OpKey, plugin: &Arc<dyn Plugin>, cx: &mut Context<Self>) -> Option<Rc<dyn ToolView>> {
        if let Some(view) = self.views.get(key) {
            return view.clone();
        }
        let view: Option<Rc<dyn ToolView>> = plugin.tool_view(&key.1, cx).map(Rc::from);
        self.views.insert(key.clone(), view.clone());
        view
    }

    fn selected_view(&self) -> Option<Rc<dyn ToolView>> {
        let key = self.selected_candidate()?.key();
        self.views.get(&key).cloned().flatten()
    }

    fn current_output(&self) -> Option<&ToolOutput> {
        let key = self.selected_candidate()?.key();
        match &self.result {
            Some((k, RunResult::Output(o))) if *k == key => Some(o),
            _ => None,
        }
    }

    fn actions(&self, cx: &App) -> Vec<ToolAction> {
        let mut actions = match (self.selected_view(), self.current_output()) {
            (Some(view), _) => view.actions(cx),
            (None, Some(out)) => out.actions.clone(),
            (None, None) => return Vec::new(),
        };
        // Primary action first, so it gets ↵.
        if let Some(i) = actions.iter().position(|a| a.primary) {
            let a = actions.remove(i);
            actions.insert(0, a);
        }
        actions
    }

    fn perform(&mut self, action: ToolAction, window: &mut Window, cx: &mut Context<Self>) {
        match action.kind {
            ActionKind::Copy { text } => {
                cx.write_to_clipboard(ClipboardItem::new_string(text));
                self.flash(format!("{} — copied to clipboard", action.label), cx);
                if state::settings(cx).hide_after_copy {
                    cx.defer(hide);
                }
            }
            ActionKind::ReplaceInput { text } => {
                self.pinned = None;
                self.input.update(cx, |e, cx| e.set_text(text, cx));
                window.focus(&self.input.focus_handle(cx));
            }
            ActionKind::OpenUrl { url } => cx.open_url(&url),
            ActionKind::Custom => {
                if let Some(view) = self.selected_view() {
                    view.perform(&action.id, cx);
                }
            }
            ActionKind::Rerun => self.schedule_run(cx),
            ActionKind::OpenSettings => {
                if let Some(c) = self.selected_candidate() {
                    let plugin_id = c.plugin_id.clone();
                    cx.defer(move |cx| settings_window::open_plugin(cx, &plugin_id));
                }
            }
            ActionKind::RunOperation { plugin_id, operation_id, params } => {
                let plugin_id = plugin_id.or_else(|| self.selected_candidate().map(|c| c.plugin_id.clone()));
                if let Some(pid) = plugin_id {
                    let key = (pid, operation_id);
                    if let Some(form) = self.forms.get_mut(&key) {
                        form.values.extend(params);
                    }
                    if let Some(i) = self.candidates.iter().position(|c| c.key() == key) {
                        self.select(i, cx);
                    }
                }
            }
            _ => {}
        }
    }

    fn flash(&mut self, message: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.flash_for(message, Duration::from_millis(1600), cx);
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

    /// The selected tool's actions with their keys: an action's own
    /// `shortcut`, else ↵ for the first (primary-sorted) one, ⌥2… for the rest.
    fn keyed_actions(&self, cx: &App) -> Vec<(ToolAction, ActionKey)> {
        let mut next_alt = 2;
        let mut enter_taken = false;
        self.actions(cx)
            .into_iter()
            .map(|a| {
                let key = match &a.shortcut {
                    Some(s) => ActionKey::Custom(s.clone()),
                    None if !enter_taken => {
                        enter_taken = true;
                        ActionKey::Enter
                    }
                    None => {
                        next_alt += 1;
                        ActionKey::Alt(next_alt - 1)
                    }
                };
                (a, key)
            })
            .collect()
    }

    fn run_action_key(&mut self, key: ActionKey, window: &mut Window, cx: &mut Context<Self>) {
        if let Some((a, _)) = self.keyed_actions(cx).into_iter().find(|(_, k)| *k == key) {
            self.perform(a, window, cx);
        }
    }

    /// Runs the action whose own shortcut is `event`'s keystroke.
    fn on_key_down(&mut self, event: &gpui::KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let pressed = &event.keystroke;
        let hit = self.keyed_actions(cx).into_iter().find(|(_, k)| match k {
            ActionKey::Custom(s) => gpui::Keystroke::parse(s)
                .is_ok_and(|ks| ks.modifiers == pressed.modifiers && ks.key.eq_ignore_ascii_case(&pressed.key)),
            _ => false,
        });
        if let Some((a, _)) = hit {
            cx.stop_propagation();
            self.perform(a, window, cx);
        }
    }

    fn focus_order(&self, cx: &App) -> Vec<FocusHandle> {
        let mut v = vec![self.input.focus_handle(cx)];
        if let Some(c) = self.selected_candidate()
            && let Some(form) = self.forms.get(&c.key())
        {
            v.extend(form.editors.iter().map(|(_, e)| e.focus_handle(cx)));
        }
        v
    }

    fn cycle_focus(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        let order = self.focus_order(cx);
        let current = order.iter().position(|h| h.is_focused(window)).unwrap_or(0) as isize;
        let next = (current + delta).rem_euclid(order.len() as isize) as usize;
        window.focus(&order[next]);
    }

    /// Moves the selected tool's mode (see `OperationSpec::mode`) by `delta`,
    /// stopping at the first and last. Returns whether the tool has modes.
    fn step_mode(&mut self, delta: isize, cx: &mut Context<Self>) -> bool {
        let Some(key) = self.selected_candidate().map(Candidate::key) else { return false };
        let Some((_, op)) = self.spec(cx, &key) else { return false };
        let Some(field) = op.mode_field() else { return false };
        let FieldKind::Select { options } = &field.kind else { return false };
        let current = self.params(&key, cx).get(&field.key).cloned().or_else(|| field.default.clone()).unwrap_or_default();
        let index = options.iter().position(|o| o.value == current).unwrap_or(0) as isize;
        let next = (index + delta).clamp(0, options.len() as isize - 1) as usize;
        if next as isize != index {
            self.set_value(&key, &field.key, options[next].value.clone(), cx);
        }
        true
    }

    fn set_value(&mut self, key: &OpKey, field: &str, value: String, cx: &mut Context<Self>) {
        if let Some(form) = self.forms.get_mut(key) {
            form.values.insert(field.to_string(), value);
            self.schedule_run(cx);
            cx.notify();
        }
    }
}

macro_rules! tool_listeners {
    ($div:expr, $cx:expr, $($action:ident => $idx:expr),*) => {
        $div$(.on_action($cx.listener(|this, _: &$action, _, cx| this.select($idx, cx))))*
    };
}

macro_rules! act_listeners {
    ($div:expr, $cx:expr, $($action:ident => $idx:expr),*) => {
        $div$(.on_action($cx.listener(|this, _: &$action, window, cx| this.run_action_key(ActionKey::Alt($idx), window, cx))))*
    };
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

impl Focusable for Launcher {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for Launcher {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_window_size(window, cx);
        let t = theme(window, cx);
        let expanded = self.is_expanded(cx);

        let root = div()
            .key_context(CONTEXT)
            .track_focus(&self.focus_handle)
            .size_full()
            .flex()
            .flex_col()
            .overflow_hidden()
            .rounded(px(RADIUS))
            .border_1()
            .border_color(if t.dark { gpui::hsla(0., 0., 1., 0.14) } else { gpui::hsla(0., 0., 0., 0.12) })
            .bg(t.window_tint)
            .font_family(t.ui_font.clone())
            .text_color(t.label)
            .text_size(px(13.))
            .on_action(cx.listener(|_, _: &Dismiss, _, cx| cx.defer(hide)))
            .on_action(cx.listener(|this, _: &PrimaryAction, window, cx| this.run_action_key(ActionKey::Enter, window, cx)))
            .on_key_down(cx.listener(Self::on_key_down))
            .on_action(cx.listener(|this, _: &SelectPrev, _, cx| {
                if this.selected > 0 && this.selected < this.candidates.len() {
                    this.select(this.selected - 1, cx)
                }
            }))
            .on_action(cx.listener(|this, _: &PrevMode, _, cx| {
                this.step_mode(-1, cx);
            }))
            .on_action(cx.listener(|this, _: &NextMode, _, cx| {
                this.step_mode(1, cx);
            }))
            .on_action(cx.listener(|this, _: &SelectNext, _, cx| {
                let next = if this.selected >= this.candidates.len() { 0 } else { this.selected + 1 };
                this.select(next, cx)
            }))
            .on_action(cx.listener(|this, _: &ClearInput, window, cx| {
                this.input.update(cx, |e, cx| e.set_text("", cx));
                window.focus(&this.input.focus_handle(cx));
            }))
            .on_action(cx.listener(|_, _: &OpenSettings, _, cx| cx.defer(settings_window::open)))
            .on_action(cx.listener(|this, _: &FocusNextField, window, cx| this.cycle_focus(1, window, cx)))
            .on_action(cx.listener(|this, _: &FocusPrevField, window, cx| this.cycle_focus(-1, window, cx)));
        let root = tool_listeners!(root, cx,
            Tool1 => 0, Tool2 => 1, Tool3 => 2, Tool4 => 3, Tool5 => 4, Tool6 => 5, Tool7 => 6, Tool8 => 7, Tool9 => 8);
        let root = act_listeners!(root, cx, Act2 => 2, Act3 => 3, Act4 => 4, Act5 => 5, Act6 => 6);

        let root = root.child(self.render_bar(&t, expanded, cx));
        if !expanded {
            return root;
        }
        root.child(ui::hairline(&t))
            .child(
                div()
                    .flex_1()
                    .min_h(px(0.))
                    .flex()
                    .child(self.render_list(&t, cx))
                    .child(div().w(px(1.)).h_full().bg(t.separator))
                    .child(self.render_detail(&t, window, cx)),
            )
            .child(ui::hairline(&t))
            .child(self.render_footer(&t, cx))
    }
}

impl Launcher {
    fn render_bar(&self, t: &Theme, expanded: bool, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex_shrink_0()
            .flex()
            .items_start()
            .gap(px(12.))
            .px(px(16.))
            .py(px(14.))
            .child(div().pt(px(2.)).child(ui::icon("icons/zap.svg", 17., t.secondary_label)))
            .child(div().flex_1().min_w(px(0.)).child(self.input.clone()))
            .when(expanded, |d| {
                d.child(
                    div()
                        .id("clear")
                        .mt(px(1.))
                        .size(px(20.))
                        .rounded_full()
                        .bg(t.fill_strong)
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor_pointer()
                        .hover(|s| s.opacity(0.8))
                        .child(ui::icon("icons/circle-x.svg", 12., t.secondary_label))
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.input.update(cx, |e, cx| e.set_text("", cx));
                            window.focus(&this.input.focus_handle(cx));
                        })),
                )
            })
    }

    fn render_list(&self, t: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let section = |label: &'static str| div().px(px(14.)).pt(px(4.)).pb(px(4.)).child(ui::caption(t, label));
        let mut list = div()
            .id("tools")
            .w(px(LIST_WIDTH))
            .flex_shrink_0()
            .h_full()
            .overflow_y_scroll()
            .track_scroll(&self.list_scroll)
            .flex()
            .flex_col()
            .py(px(6.));
        list = if self.matched == 0 {
            list.child(
                div().px(px(14.)).py(px(6.)).text_size(px(12.)).text_color(t.tertiary_label).child("No suggestions for this input"),
            )
        } else {
            list.child(section("Suggested"))
        };
        for (i, c) in self.candidates.iter().enumerate() {
            if i == self.matched {
                list = list.child(div().mx(px(14.)).my(px(6.)).child(ui::hairline(t))).child(section("Other Tools"));
            }
            let selected = i == self.selected;
            let hover = t.hover;
            list = list.child(
                div()
                    .id(("tool", i))
                    .mx(px(6.))
                    .px(px(8.))
                    .py(px(5.))
                    .rounded(px(6.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .cursor_pointer()
                    .when(selected, |d| d.bg(t.accent))
                    .when(!selected, |d| d.hover(move |s| s.bg(hover)))
                    .on_click(cx.listener(move |this, _, _, cx| this.select(i, cx)))
                    .child(ui::badge(t, &c.icon, t.badge(c.accent.as_deref()), 20.))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .text_size(px(13.))
                            .text_color(if selected { t.accent_text } else { t.label })
                            .truncate()
                            .child(c.title.clone()),
                    )
                    .when(i < 9, |d| d.child(ui::keycap(t, format!("⌘{}", i + 1), selected))),
            );
        }
        list
    }

    fn render_detail(&self, t: &Theme, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut pane = div()
            .id("detail")
            .flex_1()
            .min_w(px(0.))
            .h_full()
            .overflow_y_scroll()
            .track_scroll(&self.detail_scroll)
            .flex()
            .flex_col()
            .gap(px(14.))
            .px(px(18.))
            .py(px(14.));

        let Some(c) = self.selected_candidate().cloned() else {
            return pane.child(self.render_empty(t));
        };
        let key = c.key();

        // Header
        let has_settings = self.spec(cx, &key).is_some_and(|(p, _)| p.has_settings());
        let plugin_id = c.plugin_id.clone();
        // One compact line: badge, title, then the plugin's name dimmed.
        pane = pane.child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .h(px(24.))
                .child(ui::badge(t, &c.icon, t.badge(c.accent.as_deref()), 20.))
                .child(div().flex_shrink_0().text_size(px(13.5)).font_weight(FontWeight::SEMIBOLD).child(c.title.clone()))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .text_size(px(11.5))
                        .text_color(t.tertiary_label)
                        .truncate()
                        .child(c.plugin_name.clone()),
                )
                .when(has_settings, |d| {
                    d.child(
                        ui::icon_button(t, "tool-settings", "icons/settings.svg")
                            .on_click(move |_, _, cx| {
                                let plugin_id = plugin_id.clone();
                                cx.defer(move |cx| settings_window::open_plugin(cx, &plugin_id));
                            }),
                    )
                }),
        );

        // A crashed plugin is never called again: say so instead of its UI.
        if let Some(message) = self.crash_of(&key.0, cx) {
            let text = format!("{} crashed: {message}\n\nIt's off until Delight restarts (menu bar → Restart Delight).", c.plugin_name);
            return pane.child(notice(t, NoticeLevel::Error, &text));
        }

        let op = self.spec(cx, &key).map(|(_, op)| op);
        let output = match &self.result {
            Some((k, RunResult::Output(out))) if *k == key => Some(out),
            _ => None,
        };

        // Form — minus any fields the output places next to its own blocks.
        if let Some(op) = &op {
            let placed: Vec<&str> = output
                .into_iter()
                .flat_map(|o| &o.blocks)
                .filter_map(|b| match b {
                    Block::Field { key } => Some(key.as_str()),
                    _ => None,
                })
                .collect();
            if op.params.iter().any(|f| !placed.contains(&f.key.as_str())) {
                pane = pane.child(self.render_form(t, &key, op, &placed, window, cx));
            }
        }

        // Output
        if let Some(view) = self.selected_view() {
            return pane.child(PanicBoundary::new(view.view().into_any_element(), c.plugin_id.clone(), c.plugin_name.clone()));
        }
        match &self.result {
            Some((k, RunResult::Output(out))) if *k == key => {
                for (i, block) in out.blocks.iter().enumerate() {
                    pane = pane.child(self.render_block(t, i, block, &key, op.as_ref(), window, cx));
                }
            }
            Some((k, RunResult::Error(e))) if *k == key => {
                pane = pane.child(notice(t, NoticeLevel::Error, e));
            }
            _ if self.running => {
                pane = pane.child(div().text_color(t.tertiary_label).child("Working…"));
            }
            _ => {}
        }
        pane
    }

    fn render_empty(&self, t: &Theme) -> impl IntoElement {
        div()
            .flex_1()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(6.))
            .pt(px(120.))
            .child(ui::icon("icons/sparkles.svg", 28., t.tertiary_label))
            .child(div().text_size(px(14.)).font_weight(FontWeight::MEDIUM).child("No matching tool"))
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(t.secondary_label)
                    .child("Try JSON, a JWT, Base64 text or KEY=value lines."),
            )
    }

    fn render_form(
        &self,
        t: &Theme,
        key: &OpKey,
        op: &OperationSpec,
        placed: &[&str],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        if !self.forms.contains_key(key) {
            return div();
        }
        // The mode tabs come first, unlabelled; toggles and selects sit inline on
        // one slim bar; text fields get full-width rows. Hidden fields are skipped.
        let values = self.params(key, cx);
        let mode_key = op.mode_field().map(|f| f.key.clone());
        let mut inline = div().flex().flex_wrap().items_center().gap_x(px(16.)).gap_y(px(6.));
        let mut has_inline = false;
        if let Some(mode) = op.mode_field().filter(|f| !placed.contains(&f.key.as_str())) {
            has_inline = true;
            inline = inline.child(self.render_control(t, key, mode, window, cx));
        }
        let mut rows: Vec<AnyElement> = Vec::new();
        for field in op
            .params
            .iter()
            .filter(|f| !placed.contains(&f.key.as_str()) && Some(&f.key) != mode_key.as_ref() && f.visible(&values))
        {
            let compact = matches!(field.kind, FieldKind::Toggle | FieldKind::Select { .. });
            let control = self.render_control(t, key, field, window, cx);
            if compact {
                has_inline = true;
                inline = inline.child(div().flex().items_center().gap(px(8.)).child(field_label(t, field, true)).child(control));
            } else {
                rows.push(field_row(t, field, control));
            }
        }
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .when(has_inline, |d| d.child(inline))
            .when(!rows.is_empty(), |d| d.child(ui::group(t, rows)))
    }

    fn render_control(
        &self,
        t: &Theme,
        key: &OpKey,
        field: &FormField,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(form) = self.forms.get(key) else { return div().into_any_element() };
        match &field.kind {
                FieldKind::Toggle => {
                    let on = matches!(form.values.get(&field.key).map(String::as_str), Some("true"));
                    let (k, f) = (key.clone(), field.key.clone());
                    ui::switch(t, SharedString::from(format!("sw-{}", field.key)), on)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.set_value(&k, &f, (!on).to_string(), cx)
                        }))
                        .into_any_element()
                }
                FieldKind::Select { options } => {
                    let current = form.values.get(&field.key).cloned().unwrap_or_default();
                    let labels: Vec<(SharedString, bool)> =
                        options.iter().map(|o| (SharedString::from(o.label.clone()), o.value == current)).collect();
                    let values: Vec<String> = options.iter().map(|o| o.value.clone()).collect();
                    let (k, f) = (key.clone(), field.key.clone());
                    ui::segmented(t, &format!("seg-{}", field.key), &labels, |i, seg| {
                        let (k, f, v) = (k.clone(), f.clone(), values[i].clone());
                        seg.on_click(cx.listener(move |this, _, _, cx| this.set_value(&k, &f, v.clone(), cx)))
                    })
                    .into_any_element()
                }
                _ => {
                    let editor = form.editors.iter().find(|(k, _)| *k == field.key).map(|(_, e)| e.clone());
                    match editor {
                        Some(editor) => {
                            let focused = editor.focus_handle(cx).is_focused(window);
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .px(px(8.))
                                .py(px(4.))
                                .rounded(px(6.))
                                .bg(if t.dark { gpui::hsla(0., 0., 0., 0.25) } else { gpui::white() })
                                .border_1()
                                .border_color(if focused { t.accent.opacity(0.8) } else { t.separator })
                                .child(editor)
                                .into_any_element()
                        }
                        None => div().into_any_element(),
                    }
                }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn render_block(
        &self,
        t: &Theme,
        index: usize,
        block: &Block,
        key: &OpKey,
        op: Option<&OperationSpec>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match block {
            Block::Field { key: field_key } => match op.and_then(|op| op.params.iter().find(|f| &f.key == field_key)) {
                Some(field) => {
                    let control = self.render_control(t, key, field, window, cx);
                    ui::group(t, vec![field_row(t, field, control)]).into_any_element()
                }
                None => div().into_any_element(),
            },
            Block::Notice { level, text } => notice(t, *level, text).into_any_element(),
            Block::Text { label, text } => div()
                .flex()
                .flex_col()
                .gap(px(6.))
                .when_some(label.clone(), |d, l| d.child(ui::caption(t, l)))
                .child(div().text_size(px(12.5)).child(text.clone()))
                .into_any_element(),
            Block::KeyValue { label, rows } => {
                let rows = rows
                    .iter()
                    .map(|r| {
                        div()
                            .flex()
                            .items_center()
                            .gap(px(12.))
                            .px(px(12.))
                            .py(px(7.))
                            .text_size(px(12.))
                            .child(div().w(px(110.)).flex_shrink_0().text_color(t.secondary_label).child(r.key.clone()))
                            .child(div().flex_1().min_w(px(0.)).child(r.value.clone()))
                            .when_some(r.hint.clone(), |d, h| {
                                d.child(div().flex_shrink_0().text_size(px(11.)).text_color(t.tertiary_label).child(h))
                            })
                            .into_any_element()
                    })
                    .collect();
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .when_some(label.clone(), |d, l| d.child(ui::caption(t, l)))
                    .child(ui::group(t, rows))
                    .into_any_element()
            }
            Block::Code { label, language, text } => {
                let (shown, clipped) = clip(text);
                let syntax = Syntax::xcode(t.dark);
                let highlights = highlight(language.as_deref(), shown, &syntax);
                let full = text.clone();
                let copy = div()
                    .id(("copy-block", index))
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .px(px(6.))
                    .py(px(2.))
                    .rounded(px(5.))
                    .text_size(px(11.))
                    .text_color(t.secondary_label)
                    .cursor_pointer()
                    .hover(|s| s.bg(t.hover))
                    .child(ui::icon("icons/copy.svg", 11., t.secondary_label))
                    .child("Copy")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(full.clone()));
                        this.flash("Copied to clipboard", cx);
                    }));
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(ui::caption(t, label.clone().unwrap_or_else(|| "Output".into())))
                            .child(copy),
                    )
                    .child(
                        div()
                            .rounded(px(8.))
                            .bg(t.fill)
                            .border_1()
                            .border_color(t.separator)
                            .px(px(12.))
                            .py(px(10.))
                            .font_family(t.mono_font.clone())
                            .text_size(px(12.))
                            .line_height(px(18.))
                            .child(StyledText::new(shown.to_string()).with_highlights(highlights))
                            .when(clipped, |d| {
                                d.child(
                                    div()
                                        .pt(px(6.))
                                        .font_family(t.ui_font.clone())
                                        .text_size(px(11.))
                                        .text_color(t.tertiary_label)
                                        .child("Preview truncated — Copy gives the full result."),
                                )
                            }),
                    )
                    .into_any_element()
            }
            _ => div().into_any_element(),
        }
    }

    fn render_footer(&self, t: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let s = &self.stats;
        let left: AnyElement = match &self.toast {
            Some(msg) => div()
                .flex()
                .items_center()
                .gap(px(6.))
                .text_color(t.green)
                .child(ui::icon("icons/check.svg", 12., t.green))
                .child(msg.clone())
                .into_any_element(),
            None => div()
                .text_color(t.tertiary_label)
                .child(format!(
                    "{}  ·  {}  ·  {}  ·  {}",
                    s.human_size(),
                    plural(s.lines, "line"),
                    plural(s.chars, "char"),
                    plural(s.words, "word")
                ))
                .into_any_element(),
        };
        let mut right = div().flex().items_center().gap(px(2.));
        for (i, (a, key)) in self.keyed_actions(cx).into_iter().take(4).enumerate() {
            let hint = match &key {
                ActionKey::Enter => "↵".to_string(),
                ActionKey::Alt(n) => format!("⌥{n}"),
                ActionKey::Custom(s) => shortcut_label(s),
            };
            if i > 0 {
                right = right.child(ui::divider(t));
            }
            let label = a.label.clone();
            right = right.child(
                ui::text_button(t, ("action", i), label, hint, key == ActionKey::Enter)
                    .on_click(cx.listener(move |this, _, window, cx| this.perform(a.clone(), window, cx))),
            );
        }
        right = right.child(
            ui::icon_button(t, "settings", "icons/settings.svg")
                .ml(px(6.))
                .on_click(cx.listener(|_, _, _, cx| cx.defer(settings_window::open))),
        );
        div()
            .flex_shrink_0()
            .h(px(44.))
            .px(px(14.))
            .flex()
            .items_center()
            .justify_between()
            .gap(px(12.))
            .text_size(px(11.5))
            .child(div().min_w(px(0.)).truncate().child(left))
            .child(right)
    }
}

fn field_label(t: &Theme, field: &FormField, compact: bool) -> impl IntoElement {
    div()
        .flex_shrink_0()
        .when(!compact, |d| d.w(px(96.)))
        .text_size(px(12.))
        .text_color(t.secondary_label)
        .child(field.label.clone())
}

/// Full-width `label  [control]` row for grouped fields.
fn field_row(t: &Theme, field: &FormField, control: AnyElement) -> AnyElement {
    div()
        .flex()
        .items_center()
        .gap(px(12.))
        .px(px(12.))
        .py(px(8.))
        .child(field_label(t, field, false))
        .child(div().flex_1().min_w(px(0.)).flex().child(control))
        .into_any_element()
}

/// Which key runs a footer action.
#[derive(Debug, Clone, PartialEq)]
enum ActionKey {
    Enter,
    Alt(usize),
    /// The action's own GPUI keystroke, e.g. `cmd-enter`.
    Custom(String),
}

/// `cmd-shift-enter` → `⌘⇧↵`.
fn shortcut_label(keystroke: &str) -> String {
    keystroke
        .split('-')
        .map(|part| match part {
            "cmd" => "⌘".to_string(),
            "shift" => "⇧".to_string(),
            "alt" => "⌥".to_string(),
            "ctrl" => "⌃".to_string(),
            "enter" => "↵".to_string(),
            "backspace" => "⌫".to_string(),
            "delete" => "⌦".to_string(),
            "escape" => "⎋".to_string(),
            "tab" => "⇥".to_string(),
            "space" => "Space".to_string(),
            other => other.to_uppercase(),
        })
        .collect()
}

fn plural(n: usize, word: &str) -> String {
    if n == 1 { format!("1 {word}") } else { format!("{n} {word}s") }
}

fn clip(text: &str) -> (&str, bool) {
    if text.len() <= MAX_RENDER_BYTES {
        return (text, false);
    }
    let mut end = MAX_RENDER_BYTES;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    (&text[..end], true)
}

fn notice(t: &Theme, level: NoticeLevel, text: &str) -> impl IntoElement {
    let (color, icon) = match level {
        NoticeLevel::Success => (t.green, "icons/circle-check.svg"),
        NoticeLevel::Warning => (t.orange, "icons/triangle-alert.svg"),
        NoticeLevel::Error => (t.red, "icons/circle-x.svg"),
        _ => (t.blue, "icons/info.svg"),
    };
    div()
        .flex()
        .items_start()
        .gap(px(8.))
        .px(px(12.))
        .py(px(9.))
        .rounded(px(8.))
        .bg(color.opacity(if t.dark { 0.16 } else { 0.1 }))
        .border_1()
        .border_color(color.opacity(0.25))
        .child(div().pt(px(1.)).child(ui::icon(icon, 14., color)))
        .child(div().flex_1().min_w(px(0.)).text_size(px(12.5)).child(text.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_shortcuts() {
        assert_eq!(shortcut_label("cmd-enter"), "⌘↵");
        assert_eq!(shortcut_label("cmd-shift-enter"), "⌘⇧↵");
        assert_eq!(shortcut_label("alt-k"), "⌥K");
    }

    #[test]
    fn plugin_shortcuts_parse_to_what_the_launcher_compares() {
        let ks = gpui::Keystroke::parse("cmd-shift-enter").unwrap();
        assert_eq!(ks.key, "enter");
        assert!(ks.modifiers.platform && ks.modifiers.shift && !ks.modifiers.alt);
        let plain = gpui::Keystroke::parse("enter").unwrap();
        assert_ne!(plain.modifiers, ks.modifiers, "↵ alone must not match ⌘⇧↵");
    }
}
