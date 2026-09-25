//! Minimal `log` backend: Delight's warnings and errors to stderr
//! (`DELIGHT_LOG=info|debug` for more).

use log::{LevelFilter, Log, Metadata, Record};

struct Stderr;

impl Log for Stderr {
    /// Our own crates at the configured level; dependencies (GPUI logs benign
    /// internal notices) only with `DELIGHT_LOG=debug`.
    fn enabled(&self, metadata: &Metadata) -> bool {
        let ours = metadata.target().starts_with("delight");
        metadata.level() <= log::max_level() && (ours || log::max_level() >= LevelFilter::Debug)
    }

    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            eprintln!("[delight {}] {}", record.level(), record.args());
        }
    }

    fn flush(&self) {}
}

pub fn init() {
    let level = match std::env::var("DELIGHT_LOG").as_deref() {
        Ok("debug") => LevelFilter::Debug,
        Ok("info") => LevelFilter::Info,
        _ => LevelFilter::Warn,
    };
    if log::set_logger(&Stderr).is_ok() {
        log::set_max_level(level);
    }
}
