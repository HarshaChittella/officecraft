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
        if old.is_dir() {
            adopt_dir(&old, &new);
        }
    }
}

/// Moves `old`'s files into `new`: the whole folder when `new` isn't there yet, else file by
/// file. Of a file both have, the old one wins only over the window state (`app.ron`), which the
/// GUI library may have written already; nothing else of `new` is replaced.
fn adopt_dir(old: &Path, new: &Path) {
    if !new.exists() {
        if let Err(e) = std::fs::rename(old, new) {
            log::warn!("moving {} to {}: {e}", old.display(), new.display());
        }
        return;
    }
    let Ok(entries) = std::fs::read_dir(old) else { return };
    for entry in entries.flatten() {
        let to = new.join(entry.file_name());
        if (!to.exists() || entry.file_name() == "app.ron")
            && let Err(e) = std::fs::rename(entry.path(), &to)
        {
            log::warn!("moving {}: {e}", entry.path().display());
        }
    }
    // Gone once empty; anything left (a name both had) stays where it was.
    let _ = std::fs::remove_dir(old);
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

#[cfg(test)]
mod adopt_tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn old_settings_join_a_folder_the_gui_library_already_made() {
        let base = std::env::temp_dir().join(format!("oc-adopt-settings-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (old, new) = (base.join("Craftworks"), base.join("OfficeCraft"));
        std::fs::create_dir_all(&old).unwrap();
        std::fs::create_dir_all(&new).unwrap();
        std::fs::write(old.join("recent.json"), "old recent").unwrap();
        std::fs::write(old.join("app.ron"), "old window").unwrap();
        std::fs::write(new.join("app.ron"), "fresh window").unwrap();
        adopt_dir(&old, &new);
        assert_eq!(std::fs::read_to_string(new.join("recent.json")).unwrap(), "old recent");
        assert_eq!(std::fs::read_to_string(new.join("app.ron")).unwrap(), "old window");
        assert!(!old.exists(), "the old folder goes once it's empty");
        // A brand-new folder is just the old one renamed.
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(old.join("session.json"), "s").unwrap();
        let fresh = base.join("Fresh");
        adopt_dir(&old, &fresh);
        assert!(fresh.join("session.json").is_file() && !old.exists());
        let _ = std::fs::remove_dir_all(&base);
    }
}
