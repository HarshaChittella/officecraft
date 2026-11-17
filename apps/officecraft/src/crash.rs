//! Crash reports (Phase 8.3): a panic anywhere in the app writes a report to
//! `~/Library/Logs/OfficeCraft/crash-<time>.log` (version, macOS, the panic and where, a
//! backtrace, and the last lines logged). The next launch says so and offers to show it.
//! Nothing is sent anywhere.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

static DIR: OnceLock<PathBuf> = OnceLock::new();

/// Where reports go: `~/Library/Logs/OfficeCraft`, or `logs` in a check run's or test's
/// settings folder.
pub fn dir() -> Option<PathBuf> {
    if let Some(d) = DIR.get() {
        return Some(d.clone());
    }
    // Windows: %APPDATA%\OfficeCraft\Logs, where Windows apps keep theirs.
    if cfg!(windows)
        && let Some(appdata) = std::env::var_os("APPDATA")
    {
        return Some(PathBuf::from(appdata).join("OfficeCraft/Logs"));
    }
    let home = std::env::home_dir()?;
    Some(if cfg!(target_os = "macos") { home.join("Library/Logs/OfficeCraft") } else { home.join(".local/state/officecraft") })
}

/// Sends reports to `dir` instead (check runs). Only the first call counts.
pub fn use_dir(dir: PathBuf) {
    let _ = DIR.set(dir);
}

fn macos_version() -> String {
    std::process::Command::new("sw_vers")
        .arg("-productVersion")
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

/// The report for a panic.
pub fn report(message: &str, location: &str, thread: &str, backtrace: &str, recent: &[String], now: &str) -> String {
    let mut s = String::new();
    s.push_str(&format!("OfficeCraft {} crash report\n", env!("CARGO_PKG_VERSION")));
    s.push_str(&format!("Time: {now}\nmacOS: {}\nArchitecture: {}\n", macos_version(), std::env::consts::ARCH));
    s.push_str(&format!("Thread: {thread}\nPanic: {message}\nAt: {location}\n\nBacktrace:\n{backtrace}\n"));
    s.push_str("\nLast log lines:\n");
    for l in recent {
        s.push_str(l);
        s.push('\n');
    }
    s
}

/// Installs the panic hook (after the default one prints to stderr).
pub fn install() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        default(info);
        let message = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| (*s).to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "(no message)".into());
        let location = info.location().map(|l| format!("{}:{}", l.file(), l.line())).unwrap_or_default();
        let thread = std::thread::current().name().unwrap_or("unnamed").to_string();
        let backtrace = std::backtrace::Backtrace::force_capture().to_string();
        let now = chrono::Local::now();
        let text = report(&message, &location, &thread, &backtrace, &crate::logger::recent(), &now.to_rfc3339());
        if let Some(d) = dir() {
            let _ = std::fs::create_dir_all(&d);
            let path = d.join(format!("crash-{}.log", now.format("%Y%m%d-%H%M%S")));
            match std::fs::write(&path, text) {
                Ok(()) => eprintln!("OfficeCraft wrote a crash report: {}", path.display()),
                Err(e) => eprintln!("OfficeCraft couldn't write its crash report: {e}"),
            }
        }
    }));
}

/// Reports written since the last launch looked: the newest, if any. They're then marked seen
/// (`.seen` next to them), so each is mentioned once.
pub fn unseen(dir: &Path) -> Option<PathBuf> {
    let mut reports: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "log") && p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("crash-")))
        .filter(|p| !p.with_extension("seen").exists())
        .collect();
    reports.sort();
    for r in &reports {
        let _ = std::fs::write(r.with_extension("seen"), b"");
    }
    reports.pop()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn a_report_has_what_a_bug_report_needs() {
        let r = report("index out of bounds", "src/app.rs:10", "main", "0: frame", &["[WARN x] before".into()], "2026-11-16T10:00:00");
        for part in ["OfficeCraft", "macOS:", "Panic: index out of bounds", "At: src/app.rs:10", "Backtrace:", "[WARN x] before"] {
            assert!(r.contains(part), "{part} missing from:\n{r}");
        }
    }

    #[test]
    fn each_report_is_mentioned_once() {
        let d = std::env::temp_dir().join(format!("cw-crash-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        assert_eq!(unseen(&d), None);
        std::fs::write(d.join("crash-20261116-100000.log"), "a").unwrap();
        std::fs::write(d.join("crash-20261116-110000.log"), "b").unwrap();
        assert_eq!(unseen(&d), Some(d.join("crash-20261116-110000.log")), "the newest");
        assert_eq!(unseen(&d), None, "then no more");
    }
}
