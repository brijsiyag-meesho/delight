//! How the CLI reports: people get progress on stderr and a summary on
//! stdout; with `--json`, stdout holds exactly one JSON object and nothing
//! else, and nothing asks questions. Failures carry a stable code, a
//! message and a hint, and the exit code says what kind of failure it was.

use serde_json::{Value, json};

pub use delight_plugin_manager::{Code, Error, Result};

/// 2: bad arguments; 3: the environment (Delight, Rust, git); 1: the rest.
fn exit_code(code: Code) -> i32 {
    match code {
        Code::Usage => 2,
        Code::NoDelight | Code::NoRust | Code::NoGit => 3,
        _ => 1,
    }
}

/// A command's result: `data` for `--json`, `summary` lines for people.
pub struct Outcome {
    pub data: Value,
    pub summary: Vec<String>,
}

/// Progress for people, on stderr (so stdout stays one JSON object).
pub fn progress(message: &str) {
    eprintln!("{message}");
}

/// Prints the result and returns the exit code.
pub fn finish(result: Result<Outcome>, json: bool) -> i32 {
    match result {
        Ok(outcome) => {
            if json {
                let mut data = outcome.data;
                if let Value::Object(map) = &mut data {
                    map.insert("ok".into(), Value::Bool(true));
                }
                println!("{data}");
            } else {
                for line in outcome.summary {
                    println!("{line}");
                }
            }
            0
        }
        Err(error) => {
            if json {
                println!("{}", json!({ "ok": false, "error": error.to_json() }));
            } else {
                eprintln!("delight: error: {}", error.message);
                if let Some(hint) = &error.hint {
                    eprintln!("delight: hint: {hint}");
                }
                if let Some(log) = &error.log {
                    eprintln!("delight: log: {}", log.display());
                }
            }
            exit_code(error.code)
        }
    }
}
