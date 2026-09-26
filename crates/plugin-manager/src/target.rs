//! The Delight plugins are built for, and asking it things. The app binary
//! answers itself (`--plugin-info`, `--write-sdk-kit`, `--check-plugin`), so
//! the answers, and the SDK kit plugins build in, are always that build's.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::Deserialize;

use crate::error::{Code, Error, Result};

/// A Delight's app binary.
pub struct Target {
    binary: PathBuf,
}

/// What the app says about itself (`--plugin-info`).
#[derive(Deserialize)]
pub struct Info {
    /// `Delight SDK 0.1.0 (rustc …) src …`: what a plugin must be built for.
    pub sdk_build_id: String,
    /// `debug` or `release`: how plugins are built for it.
    pub profile: String,
    /// Where it loads plugins from.
    pub plugin_dir: PathBuf,
    /// The process id of the Delight running now, if one is.
    pub running: Option<u32>,
}

impl Info {
    /// `Delight SDK 0.1.0 (…` → `0.1.0`.
    pub fn sdk_version(&self) -> &str {
        self.sdk_build_id.strip_prefix("Delight SDK ").and_then(|s| s.split(' ').next()).unwrap_or("0.0.0")
    }
}

impl Target {
    /// `--app`, else `DELIGHT_APP`, else the Delight this CLI came with (its
    /// bundle, or the checkout it was built in), else /Applications/Delight.app.
    /// Each can be a Delight.app, its binary, or a Delight checkout.
    pub fn resolve(app: Option<PathBuf>) -> Result<Self> {
        let given = app.or_else(|| std::env::var_os("DELIGHT_APP").map(PathBuf::from));
        if let Some(path) = given {
            let binary = binary_in(&path).ok_or_else(|| {
                Error::new(Code::NoDelight, format!("no Delight at {}", path.display()))
                    .hint("pass a Delight.app, its binary, or a Delight checkout built with `cargo build`")
            })?;
            return Ok(Self { binary });
        }
        let exe = std::env::current_exe()?;
        // Delight.app/Contents/Resources/bin/delight, or <checkout>/crates/cli/target/<profile>/delight.
        let own = exe.ancestors().skip(1).find_map(|dir| {
            let name = dir.file_name()?.to_str()?;
            (name.ends_with(".app") || dir.join("crates/sdk/Cargo.toml").is_file()).then(|| binary_in(dir)).flatten()
        });
        let binary = own.or_else(|| binary_in(Path::new("/Applications/Delight.app"))).ok_or_else(|| {
            Error::new(Code::NoDelight, "no Delight found").hint("install Delight, or pass --app <Delight.app or checkout>")
        })?;
        Ok(Self { binary })
    }

    pub fn describe(&self) -> String {
        self.binary.display().to_string()
    }

    /// The app binary with `args`.
    fn command(&self, args: &[&str]) -> Result<Command> {
        let mut command = Command::new(&self.binary);
        command.args(args);
        // A dev build finds Rust's std as `cargo run` does (a bundle has it in Frameworks).
        if !self.binary.ancestors().any(|d| d.extension().is_some_and(|e| e == "app")) {
            let dir = self.binary.parent().unwrap_or(Path::new("."));
            let std = rust_sysroot(dir)?.join("lib/rustlib").join(host_triple(dir)?).join("lib");
            let paths = std::env::join_paths([dir.to_path_buf(), std]).map_err(anyhow::Error::from)?;
            command.env("DYLD_FALLBACK_LIBRARY_PATH", paths);
        }
        Ok(command)
    }

    fn ask(&self, args: &[&str]) -> Result<std::process::Output> {
        let mut command = self.command(args)?;
        command.output().map_err(|e| Error::new(Code::NoDelight, format!("running {}: {e}", self.binary.display())))
    }

    fn ask_text(&self, args: &[&str]) -> Result<String> {
        let output = self.ask(args)?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let message = format!("`{} {}` failed: {}", self.describe(), args.join(" "), stderr.trim());
            return Err(Error::new(Code::Failed, message));
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    pub fn info(&self) -> Result<Info> {
        let text = self.ask_text(&["--plugin-info"])?;
        serde_json::from_str(&text).map_err(|e| {
            Error::new(Code::NoDelight, format!("{} didn't answer --plugin-info ({e})", self.describe()))
                .hint("it may be an older Delight: update it, or rebuild the checkout")
        })
    }

    /// Writes this Delight's SDK kit into `dir`.
    pub fn write_sdk_kit(&self, dir: &Path) -> Result<()> {
        self.ask_text(&["--write-sdk-kit", &dir.to_string_lossy()]).map(drop)
    }

    /// Whether the app loads `dylib`: its id and version, or why not.
    pub fn check(&self, dylib: &Path) -> Result<std::result::Result<(String, String), String>> {
        let output = self.ask(&["--check-plugin", &dylib.to_string_lossy()])?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        if output.status.success()
            && let Some(rest) = stdout.trim().strip_prefix("ok: ")
        {
            let (id, version) = rest.split_once(' ').unwrap_or((rest, ""));
            return Ok(Ok((id.to_string(), version.to_string())));
        }
        let stderr = String::from_utf8_lossy(&output.stderr);
        Ok(Err(stderr.trim().trim_start_matches("error: ").to_string()))
    }

    /// Asks the running Delight (`pid`) to quit, waits until it has, and
    /// starts this Delight again, on its own (its output isn't shown).
    pub fn restart(&self, pid: u32) -> Result<()> {
        let pid = pid.to_string();
        let alive = || Command::new("kill").args(["-0", &pid]).stderr(Stdio::null()).status().is_ok_and(|s| s.success());
        Command::new("kill").arg(&pid).status()?;
        // Until it's gone it holds the single-instance lock: a new one wouldn't start.
        let deadline = Instant::now() + Duration::from_secs(10);
        while alive() {
            if Instant::now() > deadline {
                return Err(Error::new(Code::Failed, format!("Delight (process {pid}) didn't quit"))
                    .hint("quit it from the menu bar, then start it again"));
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let mut command = self.command(&[])?;
        command.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        command.spawn().map_err(|e| Error::new(Code::Failed, format!("starting Delight again: {e}")))?;
        Ok(())
    }
}

/// The app binary of a Delight.app, a checkout (its debug build), or the
/// binary itself.
fn binary_in(path: &Path) -> Option<PathBuf> {
    let candidates = [
        path.join("Contents/MacOS/Delight"),
        path.join("Contents/MacOS/delight-app"),
        path.join("target/debug/delight-app"),
        path.to_path_buf(),
    ];
    candidates.into_iter().find(|p| p.is_file())
}

/// `cargo` or `rustc`: `CARGO` / `RUSTC` if set, else on the PATH, else
/// rustup's `~/.cargo/bin` (an app started from Finder has a bare PATH).
pub fn rust_tool(name: &str) -> PathBuf {
    if let Some(path) = std::env::var_os(name.to_uppercase()) {
        return PathBuf::from(path);
    }
    let on_path = std::env::var_os("PATH").is_some_and(|paths| std::env::split_paths(&paths).any(|d| d.join(name).is_file()));
    let rustup = dirs::home_dir().unwrap_or_default().join(".cargo/bin").join(name);
    if !on_path && rustup.is_file() { rustup } else { PathBuf::from(name) }
}

/// `rustc --print sysroot` in `dir` (a rust-toolchain.toml above it picks the compiler).
pub fn rust_sysroot(dir: &Path) -> Result<PathBuf> {
    rustc(dir, &["--print", "sysroot"]).map(PathBuf::from)
}

/// e.g. `aarch64-apple-darwin`.
pub fn host_triple(dir: &Path) -> Result<String> {
    let info = rustc(dir, &["-vV"])?;
    Ok(info.lines().find_map(|l| l.strip_prefix("host: ")).unwrap_or_default().to_string())
}

pub fn rust_version(dir: &Path) -> Result<String> {
    rustc(dir, &["--version"])
}

fn rustc(dir: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new(rust_tool("rustc")).args(args).current_dir(dir).output().map_err(|e| {
        Error::new(Code::NoRust, format!("rustc: {e}")).hint("install Rust from https://rustup.rs")
    })?;
    if !output.status.success() {
        return Err(Error::new(Code::NoRust, String::from_utf8_lossy(&output.stderr).trim().to_string()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}
