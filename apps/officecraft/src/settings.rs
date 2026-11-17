//! Where OfficeCraft keeps its own files (macOS: `~/Library/Application Support/OfficeCraft`),
//! and the recent-files list kept there. Writes are atomic (temp file + rename).

use officecraft_shell::Recent;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

static OVERRIDE: OnceLock<PathBuf> = OnceLock::new();

/// Uses `dir` instead of the usual folder for this run (`--check`). Only the first call counts.
pub fn use_dir(dir: PathBuf) {
    let _ = OVERRIDE.set(dir);
}

/// The settings folder.
pub fn dir() -> Option<PathBuf> {
    if let Some(d) = OVERRIDE.get() {
        return Some(d.clone());
    }
    // Windows: %APPDATA%\OfficeCraft, where Windows apps keep theirs.
    if cfg!(windows)
        && let Some(appdata) = std::env::var_os("APPDATA")
    {
        return Some(PathBuf::from(appdata).join("OfficeCraft"));
    }
    let home = std::env::home_dir()?;
    Some(if cfg!(target_os = "macos") { home.join("Library/Application Support/OfficeCraft") } else { home.join(".config/officecraft") })
}

/// The app was called Craftworks until 0.1.0: its settings and crash-report folders move to
/// OfficeCraft's names the first time OfficeCraft starts (only when the new ones aren't there).
pub fn adopt_old_dirs() {
    let Some(home) = std::env::home_dir() else { return };
    let pairs: Vec<(PathBuf, PathBuf)> = if cfg!(target_os = "macos") {
        vec![
            (home.join("Library/Application Support/Craftworks"), home.join("Library/Application Support/OfficeCraft")),
            (home.join("Library/Logs/Craftworks"), home.join("Library/Logs/OfficeCraft")),
        ]
    } else if let (true, Some(appdata)) = (cfg!(windows), std::env::var_os("APPDATA")) {
        vec![(PathBuf::from(&appdata).join("Craftworks"), PathBuf::from(appdata).join("OfficeCraft"))]
    } else {
        vec![
            (home.join(".config/craftworks"), home.join(".config/officecraft")),
            (home.join(".local/state/craftworks"), home.join(".local/state/officecraft")),
        ]
    };
    for (old, new) in pairs {
        if old.is_dir() && !new.exists() {
            if let Err(e) = std::fs::rename(&old, &new) {
                log::warn!("moving {} to {}: {e}", old.display(), new.display());
            }
        }
    }
}

pub fn home() -> Option<PathBuf> {
    std::env::home_dir()
}

pub fn load_recent() -> Recent {
    dir()
        .and_then(|d| std::fs::read(d.join("recent.json")).ok())
        .and_then(|b| serde_json::from_slice(&b).ok())
        .map(|v| Recent::from_json(&v))
        .unwrap_or_default()
}

pub fn save_recent(r: &Recent) {
    let Some(d) = dir() else { return };
    if let Err(e) = write_atomic(&d.join("recent.json"), r.to_json().to_string().as_bytes()) {
        log::warn!("saving recent files: {e}");
    }
}

pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}

/// The chrome's text size in percent (`prefs.json`), 100 when unset.
pub fn load_text_scale() -> u32 {
    dir()
        .and_then(|d| std::fs::read(d.join("prefs.json")).ok())
        .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
        .and_then(|v| v.get("text_scale").and_then(serde_json::Value::as_u64))
        .and_then(|n| u32::try_from(n).ok())
        .unwrap_or(100)
}

pub fn save_text_scale(percent: u32) {
    let Some(d) = dir() else { return };
    let v = serde_json::json!({"version": 1, "text_scale": percent});
    if let Err(e) = write_atomic(&d.join("prefs.json"), v.to_string().as_bytes()) {
        log::warn!("saving preferences: {e}");
    }
}
