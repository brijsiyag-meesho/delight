//! Plugin panics. Every call into a plugin goes through [`Guarded`], so a
//! panicking plugin is turned off and reported instead of taking Delight down.
//!
//! * Background calls (`detect`, `run`) don't touch GPUI state: the panic is
//!   caught, the plugin is marked crashed — never called again in this
//!   process — and Delight keeps running.
//! * Main-thread calls that take `&mut App` (`tool_view`, `settings_view`,
//!   `ToolView::update` / `perform`) and panics while rendering a plugin's
//!   view (the app's `PanicBoundary`) can stop halfway through a GPUI update.
//!   GPUI isn't unwind-safe there (its update counter and the entity being
//!   updated are left behind), so carrying on would freeze or crash the UI.
//!   Those crashes are [`fatal`]: the handler from [`set_fatal_handler`] runs —
//!   the app turns the plugin off and relaunches. Without a handler (tests)
//!   they're treated like background crashes.
//!
//! Not catchable: segfaults, stack overflows, aborts, and panics in a plugin's
//! own event listeners or spawned tasks (they unwind into AppKit / GCD, which
//! aborts). For those, the panic hook leaves a crash note (see [`install`])
//! that the next launch shows.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::panic::{self, AssertUnwindSafe};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use delight_sdk::gpui::{AnyView, App};
use delight_sdk::{Action, Detection, Input, Plugin, PluginError, PluginManifest, RunRequest, SettingsView, ToolContext, ToolOutput, ToolView};

/// Why a plugin crashed; `None` while it's healthy. Shared by the plugin's
/// [`Guarded`] wrapper and its views.
pub type CrashCell = Arc<Mutex<Option<String>>>;

/// Called for a crash that leaves GPUI unusable: `(plugin id, plugin name,
/// message)`. Expected not to return (the app relaunches).
pub type FatalHandler = fn(&str, &str, &str);

static FATAL: OnceLock<FatalHandler> = OnceLock::new();
static CRASH_NOTE: OnceLock<PathBuf> = OnceLock::new();
static HOOK: OnceLock<()> = OnceLock::new();

thread_local! {
    /// > 0 while this thread is inside [`catch`].
    static DEPTH: Cell<u32> = const { Cell::new(0) };
    /// The last caught panic's message and location, set by the hook.
    static LAST_PANIC: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// Installs the panic hook. With `crash_note`, a panic outside any plugin
/// call — likely fatal — is written there for the next launch
/// ([`take_crash_note`]); a clean quit removes it ([`clear_crash_note`]).
pub fn install(crash_note: Option<PathBuf>) {
    if let Some(path) = crash_note {
        let _ = CRASH_NOTE.set(path);
    }
    HOOK.get_or_init(|| {
        let previous = panic::take_hook();
        panic::set_hook(Box::new(move |info| {
            let message = payload_text(info.payload());
            let text = match info.location() {
                Some(l) => format!("{message} ({}:{})", l.file(), l.line()),
                None => message,
            };
            if DEPTH.with(Cell::get) > 0 {
                LAST_PANIC.with(|p| *p.borrow_mut() = Some(text));
            } else if let Some(path) = CRASH_NOTE.get() {
                let thread = std::thread::current().name().unwrap_or("unnamed").to_string();
                let _ = std::fs::write(path, format!("{text} — thread {thread}"));
            }
            previous(info);
        }));
    });
}

pub fn set_fatal_handler(handler: FatalHandler) {
    let _ = FATAL.set(handler);
}

/// Writes a note for the next launch (e.g. "X crashed and was turned off").
pub fn write_crash_note(text: &str) {
    if let Some(path) = CRASH_NOTE.get() {
        let _ = std::fs::write(path, text);
    }
}

/// The note left by the previous run, if it crashed; removes it.
pub fn take_crash_note() -> Option<String> {
    let path = CRASH_NOTE.get()?;
    let note = std::fs::read_to_string(path).ok()?;
    let _ = std::fs::remove_file(path);
    Some(note.trim().to_string()).filter(|n| !n.is_empty())
}

pub fn clear_crash_note() {
    if let Some(path) = CRASH_NOTE.get() {
        let _ = std::fs::remove_file(path);
    }
}

/// Runs `f`, turning a panic into its message (with the panic's location).
pub fn catch<R>(f: impl FnOnce() -> R) -> Result<R, String> {
    install(None);
    LAST_PANIC.with(|p| p.borrow_mut().take());
    DEPTH.with(|d| d.set(d.get() + 1));
    let result = panic::catch_unwind(AssertUnwindSafe(f));
    DEPTH.with(|d| d.set(d.get() - 1));
    result.map_err(|payload| LAST_PANIC.with(|p| p.borrow_mut().take()).unwrap_or_else(|| payload_text(&*payload)))
}

/// Reports a crash that left GPUI unusable: runs the fatal handler, which
/// doesn't return in the app. Without one (tests), re-raises the panic.
pub fn fatal(plugin_id: &str, plugin_name: &str, message: String) -> ! {
    log::error!("plugin {plugin_id} panicked on the main thread: {message}");
    if let Some(handler) = FATAL.get() {
        handler(plugin_id, plugin_name, &message);
    }
    panic::resume_unwind(Box::new(message))
}

fn payload_text(payload: &(dyn Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "panic".into())
}

/// Shared by a plugin's wrapper and its views.
#[derive(Clone)]
struct Tracker {
    id: String,
    name: String,
    crash: CrashCell,
}

impl Tracker {
    fn crashed(&self) -> Option<String> {
        self.crash.lock().ok()?.clone()
    }

    /// Marks the plugin crashed. `fatal`: also runs the fatal handler.
    fn record(&self, message: String, fatal: bool) {
        log::error!("plugin {} panicked: {message}", self.id);
        if let Ok(mut crash) = self.crash.lock() {
            crash.get_or_insert_with(|| message.clone());
        }
        if fatal && let Some(handler) = FATAL.get() {
            handler(&self.id, &self.name, &message);
        }
    }

    fn error(&self, message: &str) -> PluginError {
        PluginError::Failed(format!("{} crashed: {message}. It's off until Delight restarts.", self.name))
    }
}

/// A plugin behind panic guards. The manifest is read once, up front.
pub struct Guarded {
    inner: Arc<dyn Plugin>,
    manifest: PluginManifest,
    has_settings: bool,
    tracker: Tracker,
}

impl Guarded {
    /// Fails with the panic message when reading the manifest panics.
    pub fn new(inner: Arc<dyn Plugin>) -> Result<Self, String> {
        let (manifest, has_settings) = catch(|| (inner.manifest().clone(), inner.has_settings()))?;
        let tracker = Tracker { id: manifest.id.clone(), name: manifest.name.clone(), crash: CrashCell::default() };
        Ok(Self { inner, manifest, has_settings, tracker })
    }

    pub fn crash_cell(&self) -> CrashCell {
        self.tracker.crash.clone()
    }
}

impl Plugin for Guarded {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    fn detect(&self, input: &Input) -> Vec<Detection> {
        if self.tracker.crashed().is_some() {
            return Vec::new();
        }
        catch(|| self.inner.detect(input)).unwrap_or_else(|message| {
            self.tracker.record(message, false);
            Vec::new()
        })
    }

    fn run(&self, request: &RunRequest) -> Result<ToolOutput, PluginError> {
        if let Some(message) = self.tracker.crashed() {
            return Err(self.tracker.error(&message));
        }
        catch(|| self.inner.run(request)).unwrap_or_else(|message| {
            let error = self.tracker.error(&message);
            self.tracker.record(message, false);
            Err(error)
        })
    }

    fn tool_view(&self, operation_id: &str, cx: &mut App) -> Option<Box<dyn ToolView>> {
        if self.tracker.crashed().is_some() {
            return None;
        }
        match catch(|| self.inner.tool_view(operation_id, cx)) {
            Ok(view) => view.map(|inner| Box::new(GuardedView { inner, tracker: self.tracker.clone() }) as Box<dyn ToolView>),
            Err(message) => {
                self.tracker.record(message, true);
                None
            }
        }
    }

    fn has_settings(&self) -> bool {
        self.has_settings
    }

    fn settings_view(&self, cx: &mut App) -> Option<Box<dyn SettingsView>> {
        if self.tracker.crashed().is_some() {
            return None;
        }
        match catch(|| self.inner.settings_view(cx)) {
            Ok(view) => view.map(|inner| Box::new(GuardedSettings { inner, tracker: self.tracker.clone() }) as Box<dyn SettingsView>),
            Err(message) => {
                self.tracker.record(message, true);
                None
            }
        }
    }
}

struct GuardedView {
    inner: Box<dyn ToolView>,
    tracker: Tracker,
}

impl ToolView for GuardedView {
    fn view(&self) -> AnyView {
        catch(|| self.inner.view()).unwrap_or_else(|message| fatal(&self.tracker.id, &self.tracker.name, message))
    }

    fn update(&self, context: &ToolContext, cx: &mut App) {
        if self.tracker.crashed().is_some() {
            return;
        }
        if let Err(message) = catch(|| self.inner.update(context, cx)) {
            self.tracker.record(message, true);
        }
    }

    /// Read-only (`&App`): a panic here leaves GPUI intact.
    fn actions(&self, cx: &App) -> Vec<Action> {
        if self.tracker.crashed().is_some() {
            return Vec::new();
        }
        catch(|| self.inner.actions(cx)).unwrap_or_else(|message| {
            self.tracker.record(message, false);
            Vec::new()
        })
    }

    fn perform(&self, action_id: &str, cx: &mut App) {
        if self.tracker.crashed().is_some() {
            return;
        }
        if let Err(message) = catch(|| self.inner.perform(action_id, cx)) {
            self.tracker.record(message, true);
        }
    }
}

struct GuardedSettings {
    inner: Box<dyn SettingsView>,
    tracker: Tracker,
}

impl SettingsView for GuardedSettings {
    fn view(&self) -> AnyView {
        catch(|| self.inner.view()).unwrap_or_else(|message| fatal(&self.tracker.id, &self.tracker.name, message))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::{PluginSource, Registry};
    use crate::Router;
    use delight_sdk::{DetectRule, OperationSpec};
    use std::collections::HashSet;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Panicky {
        manifest: PluginManifest,
        detects: AtomicUsize,
    }

    impl Panicky {
        fn new() -> Self {
            let op = OperationSpec::new("boom", "Boom").detect([DetectRule::always(0.9)]);
            Self { manifest: PluginManifest::new("test.panicky", "Panicky").operations([op]), detects: AtomicUsize::new(0) }
        }
    }

    impl Plugin for Panicky {
        fn manifest(&self) -> &PluginManifest {
            &self.manifest
        }
        fn detect(&self, input: &Input) -> Vec<Detection> {
            self.detects.fetch_add(1, Ordering::SeqCst);
            if input.trimmed == "boom" {
                let v: Vec<u8> = Vec::new();
                let _ = v[3]; // index out of bounds
            }
            delight_sdk::rules::evaluate(&self.manifest, input)
        }
        fn run(&self, _: &RunRequest) -> Result<ToolOutput, PluginError> {
            panic!("run exploded");
        }
    }

    #[test]
    fn detect_panic_is_caught_and_poisons_the_plugin() {
        let panicky = Arc::new(Panicky::new());
        let mut registry = Registry::with_builtins();
        registry.register(panicky.clone(), PluginSource::Builtin);
        let router = Router::deterministic();

        // Healthy: it's suggested.
        let before = router.classify("hello", &registry, &HashSet::new());
        assert!(before.iter().any(|c| c.plugin_id == "test.panicky"));

        // Panics: classification still completes, other tools still show.
        let during = router.classify("boom", &registry, &HashSet::new());
        assert!(during.iter().all(|c| c.plugin_id != "test.panicky"));
        assert!(!during.is_empty(), "built-ins are unaffected");
        let crash = registry.get("test.panicky").unwrap().crash().expect("crash recorded");
        assert!(crash.contains("index out of bounds"), "{crash}");
        assert!(crash.contains("guard.rs"), "location recorded: {crash}");

        // Poisoned: never called again.
        let calls = panicky.detects.load(Ordering::SeqCst);
        let after = router.classify("hello", &registry, &HashSet::new());
        assert!(after.iter().all(|c| c.plugin_id != "test.panicky"));
        assert_eq!(panicky.detects.load(Ordering::SeqCst), calls);
    }

    #[test]
    fn run_panic_becomes_an_error() {
        let mut registry = Registry::new();
        registry.register(Arc::new(Panicky::new()), PluginSource::Builtin);
        let plugin = &registry.get("test.panicky").unwrap().plugin;
        let err = plugin.run(&RunRequest::new("boom", "x")).unwrap_err().to_string();
        assert!(err.contains("Panicky crashed: run exploded"), "{err}");
        // Later calls fail fast with the same reason.
        let again = plugin.run(&RunRequest::new("boom", "x")).unwrap_err().to_string();
        assert!(again.contains("run exploded"), "{again}");
    }

    #[test]
    fn manifest_panic_is_a_load_error() {
        struct NoManifest;
        impl Plugin for NoManifest {
            fn manifest(&self) -> &PluginManifest {
                panic!("no manifest")
            }
        }
        let mut registry = Registry::new();
        registry.register(Arc::new(NoManifest), PluginSource::Builtin);
        assert!(registry.plugins().is_empty());
        assert!(registry.load_errors[0].contains("no manifest"), "{:?}", registry.load_errors);
    }
}
