//! Delight's key bindings, loaded as Zed loads its keymaps: the default
//! keymap (`assets/keymaps/default-macos.json`), then the user's
//! `keymap.json` in the same format on top, so the user's bindings win. The
//! user's file is watched: saving it rebinds the keys at once.
//!
//! A keymap is a list of sections, each binding keystrokes to action names
//! within a key context (`"Launcher > Editor && end_of_input"`). GPUI sends a
//! key to the innermost focused context that binds it.

use std::path::PathBuf;
use std::rc::Rc;

use anyhow::{Context as _, bail};
use delight_core::Settings;
use futures::channel::mpsc::{self, UnboundedReceiver};
use gpui::{Action, App, Global, KeyBinding, KeyBindingContextPredicate, NoAction};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use serde::Deserialize;
use serde_json::Value;

use crate::launcher;

const DEFAULT_KEYMAP: &str = include_str!("../assets/keymaps/default-macos.json");

/// What a new keymap.json starts with.
const USER_KEYMAP_TEMPLATE: &str = r#"// Your key bindings, over Delight's defaults (the same format as Zed's
// keymap.json). Saved changes apply at once. For example:
//
// [
//   {
//     "context": "Launcher",
//     "bindings": {
//       "cmd-l": "launcher::ClearInput",
//       "cmd-k": null
//     }
//   }
// ]
//
// Contexts: Launcher, Launcher > Editor (the input), Launcher > ToolList,
// Settings, Editor. `null` turns a key off.
[
]
"#;

/// One section of a keymap file.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Section {
    #[serde(default)]
    context: String,
    /// Keystrokes → action, in the file's order.
    #[serde(default)]
    bindings: serde_json::Map<String, Value>,
}

/// Keeps keymap.json watched while it's alive.
struct KeymapWatcher {
    _watcher: RecommendedWatcher,
}

impl Global for KeymapWatcher {}

/// Binds the keys, and rebinds them whenever keymap.json changes.
pub fn init(cx: &mut App) {
    reload(cx);
    match watch() {
        Ok((watcher, changes)) => {
            cx.set_global(KeymapWatcher { _watcher: watcher });
            crate::run_on_main_thread(cx, changes, |(), cx| reload(cx));
        }
        Err(e) => log::error!("not watching keymap.json: {e:#}"),
    }
}

/// Opens keymap.json in the default editor, creating it first if needed.
pub fn open_user_keymap(cx: &mut App) -> anyhow::Result<()> {
    let path = Settings::keymap_path();
    if !path.exists() {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&path, USER_KEYMAP_TEMPLATE)?;
    }
    cx.open_with_system(&path);
    Ok(())
}

/// Binds every key again: the defaults, then keymap.json. A mistake in
/// keymap.json skips that binding (or the whole file, if it isn't valid
/// JSON) and says why in the launcher.
fn reload(cx: &mut App) {
    let (mut bindings, errors) = parse(DEFAULT_KEYMAP, cx);
    for error in errors {
        log::error!("default keymap: {error}");
    }
    let mut errors = Vec::new();
    match std::fs::read_to_string(Settings::keymap_path()) {
        Ok(text) => {
            let (user, user_errors) = parse(&text, cx);
            bindings.extend(user);
            errors = user_errors;
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => errors.push(e.to_string()),
    }
    cx.clear_key_bindings();
    cx.bind_keys(bindings);
    if !errors.is_empty() {
        let message = format!("keymap.json: {}", errors.join("; "));
        log::warn!("{message}");
        launcher::toast(cx, message.into());
    }
}

/// The bindings in a keymap file, and why the skipped ones were skipped.
fn parse(text: &str, cx: &App) -> (Vec<KeyBinding>, Vec<String>) {
    let sections: Vec<Section> = match serde_json_lenient::from_str(text) {
        Ok(sections) => sections,
        Err(e) => return (Vec::new(), vec![e.to_string()]),
    };
    let mut bindings = Vec::new();
    let mut errors = Vec::new();
    for section in sections {
        let context = match section.context.as_str() {
            "" => None,
            context => match KeyBindingContextPredicate::parse(context) {
                Ok(predicate) => Some(Rc::new(predicate)),
                Err(e) => {
                    errors.push(format!("context {context:?}: {e}"));
                    continue;
                }
            },
        };
        for (keystrokes, action) in &section.bindings {
            match binding(keystrokes, action, context.clone(), cx) {
                Ok(binding) => bindings.push(binding),
                Err(e) => errors.push(format!("{keystrokes:?}: {e}")),
            }
        }
    }
    (bindings, errors)
}

/// One binding: the action is a name, `[name, argument]`, or `null` (the
/// key does nothing).
fn binding(
    keystrokes: &str,
    action: &Value,
    context: Option<Rc<KeyBindingContextPredicate>>,
    cx: &App,
) -> anyhow::Result<KeyBinding> {
    let action: Box<dyn Action> = match action {
        Value::Null => Box::new(NoAction {}),
        Value::String(name) => cx.build_action(name, None)?,
        Value::Array(pair) => match pair.as_slice() {
            [Value::String(name), argument] => cx.build_action(name, Some(argument.clone()))?,
            _ => bail!("expected [\"action\", argument]"),
        },
        _ => bail!("expected an action name, [\"action\", argument] or null"),
    };
    Ok(KeyBinding::load(keystrokes, action, context, false, None, cx.keyboard_mapper().as_ref())?)
}

/// Watches Delight's folder (keymap.json may not exist yet): one item per
/// change to keymap.json.
fn watch() -> anyhow::Result<(RecommendedWatcher, UnboundedReceiver<()>)> {
    let (sender, changes) = mpsc::unbounded();
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        let is_keymap = |path: &PathBuf| path.file_name().is_some_and(|name| name == "keymap.json");
        if event.is_ok_and(|event| event.paths.iter().any(is_keymap)) {
            let _ = sender.unbounded_send(());
        }
    })?;
    let path = Settings::keymap_path();
    let dir = path.parent().context("keymap.json has no folder")?;
    std::fs::create_dir_all(dir)?;
    watcher.watch(dir, RecursiveMode::NonRecursive)?;
    Ok((watcher, changes))
}
