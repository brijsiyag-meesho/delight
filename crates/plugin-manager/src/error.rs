//! What went wrong: a stable code scripts and agents can match on, a
//! message, what to do about it, and the log of a build that failed.

use std::fmt;
use std::path::PathBuf;

use serde_json::{Value, json};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Code {
    /// Bad arguments.
    Usage,
    /// No Delight to build for, or it can't answer (too old).
    NoDelight,
    /// `rustc` / `cargo` missing.
    NoRust,
    /// `git` missing.
    NoGit,
    /// Cloning or updating a plugin repo failed.
    GitFailed,
    /// The folder isn't a plugin repo, or a crate in it breaks a rule.
    NotAPluginRepo,
    /// `cargo build` failed.
    BuildFailed,
    /// Built, but Delight won't load it (a different SDK build).
    SdkMismatch,
    /// Anything else (a file that can't be written…).
    Failed,
}

impl Code {
    pub fn as_str(self) -> &'static str {
        match self {
            Code::Usage => "usage",
            Code::NoDelight => "no_delight",
            Code::NoRust => "no_rust",
            Code::NoGit => "no_git",
            Code::GitFailed => "git_failed",
            Code::NotAPluginRepo => "not_a_plugin_repo",
            Code::BuildFailed => "build_failed",
            Code::SdkMismatch => "sdk_mismatch",
            Code::Failed => "failed",
        }
    }
}

/// A failure: what, why, and what to do about it.
#[derive(Debug, Clone)]
pub struct Error {
    pub code: Code,
    pub message: String,
    pub hint: Option<String>,
    /// The full output of a build that failed.
    pub log: Option<PathBuf>,
}

impl Error {
    pub fn new(code: Code, message: impl Into<String>) -> Self {
        Self { code, message: message.into(), hint: None, log: None }
    }

    pub fn hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    pub fn log(mut self, log: PathBuf) -> Self {
        self.log = Some(log);
        self
    }

    /// `code`, `message`, `hint` and `log`, as JSON.
    pub fn to_json(&self) -> Value {
        json!({ "code": self.code.as_str(), "message": self.message, "hint": self.hint, "log": self.log })
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Error {}

/// Any other error (I/O, a bad file) is a plain failure.
impl From<anyhow::Error> for Error {
    fn from(e: anyhow::Error) -> Self {
        Error::new(Code::Failed, format!("{e:#}"))
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::new(Code::Failed, e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, Error>;
