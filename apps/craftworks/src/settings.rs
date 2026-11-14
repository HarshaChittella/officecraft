//! Where Craftworks keeps its own files (macOS: `~/Library/Application Support/Craftworks`),
//! and the recent-files list kept there. Writes are atomic (temp file + rename).

use craftworks_shell::Recent;
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
    let home = std::env::home_dir()?;
    Some(if cfg!(target_os = "macos") { home.join("Library/Application Support/Craftworks") } else { home.join(".config/craftworks") })
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

fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}
