//! A small stderr logger (no new dependency). Level from `CW_LOG` (`error`, `warn`, `info`,
//! `debug`, `trace`; default `warn`). Warnings are also kept for the script's report.

use std::sync::Mutex;

static WARNINGS: Mutex<Vec<String>> = Mutex::new(Vec::new());

struct Logger;

impl log::Log for Logger {
    fn enabled(&self, m: &log::Metadata<'_>) -> bool {
        m.level() <= log::max_level()
    }

    fn log(&self, r: &log::Record<'_>) {
        if !self.enabled(r.metadata()) {
            return;
        }
        let line = format!("[{} {}] {}", r.level(), r.target(), r.args());
        eprintln!("{line}");
        if r.level() <= log::Level::Warn
            && let Ok(mut w) = WARNINGS.lock()
        {
            w.push(line);
        }
    }

    fn flush(&self) {}
}

static LOGGER: Logger = Logger;

pub fn install() {
    let level = match std::env::var("CW_LOG").ok().as_deref() {
        Some("error") => log::LevelFilter::Error,
        Some("info") => log::LevelFilter::Info,
        Some("debug") => log::LevelFilter::Debug,
        Some("trace") => log::LevelFilter::Trace,
        _ => log::LevelFilter::Warn,
    };
    if log::set_logger(&LOGGER).is_ok() {
        log::set_max_level(level);
    }
}

/// The warnings logged so far.
pub fn warnings() -> Vec<String> {
    WARNINGS.lock().map(|w| w.clone()).unwrap_or_default()
}
