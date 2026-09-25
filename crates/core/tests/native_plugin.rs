//! End-to-end: build the example plugin dylib, load it from a plugins folder,
//! and route input to it.

use std::collections::HashSet;
use std::path::PathBuf;
use std::process::Command;

use delight_core::{Registry, Router};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Builds a plugin of this workspace (debug, like the test) and returns its dylib.
fn build_plugin(package: &str) -> PathBuf {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let status = Command::new(cargo).args(["build", "-p", package]).current_dir(workspace_root()).status().expect("run cargo");
    assert!(status.success(), "building {package} failed");
    let target = std::env::var_os("CARGO_TARGET_DIR").map(PathBuf::from).unwrap_or_else(|| workspace_root().join("target"));
    target.join(format!("debug/lib{}.dylib", package.replace('-', "_")))
}

fn build_hello() -> PathBuf {
    build_plugin("delight-plugin-hello")
}

/// A panic inside a plugin dylib unwinds into the host's guard: the plugin is
/// turned off, the app (here: the test process) keeps running.
#[test]
fn panics_in_a_plugin_dylib_are_caught() {
    let dylib = build_plugin("delight-plugin-panicky");
    let dir = std::env::temp_dir().join(format!("delight-panicky-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::copy(&dylib, dir.join("panicky.dylib")).unwrap();
    let registry = Registry::load(Some(&dir));
    let router = Router::deterministic();
    let disabled = HashSet::new();

    // `run` panics: an error, not an abort.
    let plugin = registry.get("test.panicky").expect("loaded").plugin.clone();
    let err = plugin.run(&delight_sdk::RunRequest::new("declarative", "panic run")).unwrap_err().to_string();
    assert!(err.contains("panicky: run exploded"), "{err}");
    let crash = registry.get("test.panicky").unwrap().crash().expect("crash recorded");
    assert!(crash.contains("crates/test-plugins/panicky/src/lib.rs"), "location recorded: {crash}");

    // Crashed: never called again, while every other tool still works.
    let candidates = router.classify("panic run", &registry, &disabled);
    assert!(candidates.iter().all(|c| c.plugin_id != "test.panicky"));
    assert_eq!(router.classify(r#"{"a":1}"#, &registry, &disabled)[0].plugin_id, "delight.json");

    // `detect` panics (fresh registry, so the plugin is healthy again).
    let registry = Registry::load(Some(&dir));
    let candidates = router.classify("panic detect", &registry, &disabled);
    assert!(candidates.iter().all(|c| c.plugin_id != "test.panicky"));
    let crash = registry.get("test.panicky").unwrap().crash().expect("crash recorded");
    assert!(crash.contains("index out of bounds"), "{crash}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn loads_and_routes_to_a_plugin_dylib() {
    let dylib = build_hello();
    let dir = std::env::temp_dir().join(format!("delight-plugins-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::copy(&dylib, dir.join("hello.dylib")).unwrap();
    std::fs::write(dir.join("broken.dylib"), b"not a library").unwrap();

    let registry = Registry::load(Some(&dir));
    assert!(registry.get("example.hello").is_some(), "errors: {:?}", registry.load_errors);
    assert_eq!(registry.load_errors.len(), 1);
    assert!(registry.load_errors[0].starts_with("broken.dylib"));

    let candidates = Router::deterministic().classify("hello delight", &registry, &HashSet::new());
    assert_eq!(candidates[0].plugin_id, "example.hello");
    assert_eq!(candidates[0].operation_id, "greet");

    let _ = std::fs::remove_dir_all(&dir);
}
