//! Connect an AI (docs/ai.md): what the AI pill opens before any AI has been in touch. It sets
//! OfficeCraft's connector up for the project in front, in Claude Desktop (a line in its
//! settings file, the rest of the file kept and a copy saved first) or Claude Code (a command to
//! paste), and sets what the AI may do in the project. ChatGPT is named and explained: it only
//! reaches connectors on the internet, and this one runs on your Mac.

use officecraft_core::project::Permission;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// The name OfficeCraft's server has in an AI app's list.
pub const SERVER: &str = "officecraft";

/// Claude Desktop's settings file.
pub fn claude_desktop_config() -> Option<PathBuf> {
    if cfg!(windows) {
        return std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("Claude/claude_desktop_config.json"));
    }
    std::env::home_dir().map(|h| h.join("Library/Application Support/Claude/claude_desktop_config.json"))
}

/// Whether Claude Desktop is installed here.
pub fn claude_desktop_installed() -> bool {
    if cfg!(windows) {
        return std::env::var_os("LOCALAPPDATA").is_some_and(|l| PathBuf::from(l).join("AnthropicClaude").is_dir());
    }
    let home_apps = std::env::home_dir().map(|h| h.join("Applications/Claude.app"));
    Path::new("/Applications/Claude.app").is_dir() || home_apps.is_some_and(|p| p.is_dir())
}

/// This program, as an AI app should start it.
pub fn executable() -> Option<PathBuf> {
    std::env::current_exe().ok()
}

/// The project Claude Desktop's settings point OfficeCraft at, if any.
pub fn claude_desktop_project(text: &str) -> Option<PathBuf> {
    let v: Value = serde_json::from_str(text).ok()?;
    let args = v.get("mcpServers")?.get(SERVER)?.get("args")?.as_array()?;
    let i = args.iter().position(|a| a == "--project")?;
    args.get(i + 1)?.as_str().map(PathBuf::from)
}

/// Claude Desktop's settings `existing` (the file's text, if there is one) with OfficeCraft's
/// server set to `exe` working in `project`. Every other setting and server is kept. Refused
/// when the file isn't a JSON object, so nothing of yours is ever overwritten.
pub fn with_officecraft(existing: Option<&str>, exe: &Path, project: &Path) -> Result<String, String> {
    let mut v: Value = match existing.map(str::trim) {
        None | Some("") => json!({}),
        Some(t) => serde_json::from_str(t).map_err(|e| format!("Claude Desktop's settings file isn't valid JSON ({e}); fix it or add OfficeCraft by hand"))?,
    };
    let root = v.as_object_mut().ok_or("Claude Desktop's settings file isn't a JSON object; add OfficeCraft by hand")?;
    let servers = root.entry("mcpServers").or_insert_with(|| json!({}));
    let servers = servers.as_object_mut().ok_or("Claude Desktop's \"mcpServers\" isn't an object; add OfficeCraft by hand")?;
    servers.insert(
        SERVER.into(),
        json!({ "command": exe.to_string_lossy(), "args": ["mcp", "--project", project.to_string_lossy()] }),
    );
    serde_json::to_string_pretty(&v).map(|s| s + "\n").map_err(|e| e.to_string())
}

/// Sets Claude Desktop up for `project`: saves a copy of its settings the first time
/// (`claude_desktop_config.json.before-officecraft`), then writes the new settings in one step.
pub fn add_to_claude_desktop(exe: &Path, project: &Path) -> Result<(), String> {
    let path = claude_desktop_config().ok_or("couldn't find your home folder")?;
    let existing = std::fs::read_to_string(&path).ok();
    let text = with_officecraft(existing.as_deref(), exe, project)?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    if existing.is_some() {
        let backup = path.with_extension("json.before-officecraft");
        if !backup.exists() {
            std::fs::copy(&path, &backup).map_err(|e| format!("couldn't save a copy of Claude's settings: {e}"))?;
        }
    }
    crate::settings::write_atomic(&path, text.as_bytes()).map_err(|e| e.to_string())
}

/// The command that adds OfficeCraft to Claude Code, for `project`.
pub fn claude_code_command(exe: &Path, project: &Path) -> String {
    let q = |p: &Path| format!("'{}'", p.to_string_lossy().replace('\'', "'\\''"));
    format!("claude mcp add {SERVER} -- {} mcp --project {}", q(exe), q(project))
}

/// What each permission is called in the panel, and what it lets the AI do.
pub const PERMISSIONS: [(Permission, &str, &str); 3] = [
    (Permission::Read, "Read only", "Read and search the project's files"),
    (Permission::Edit, "Edit", "Edit files in their tabs; you keep or undo each change"),
    (Permission::EditSave, "Edit and save", "Edit and save files, open or not"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn officecraft_joins_claudes_settings_and_nothing_else_changes() {
        let exe = Path::new("/Applications/OfficeCraft.app/Contents/MacOS/officecraft");
        let mine = r#"{"mcpServers": {"other": {"command": "x"}}, "theme": "dark"}"#;
        let out = with_officecraft(Some(mine), exe, Path::new("/p/Q3 plans")).unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["theme"], "dark");
        assert_eq!(v["mcpServers"]["other"]["command"], "x");
        assert_eq!(v["mcpServers"]["officecraft"]["args"], json!(["mcp", "--project", "/p/Q3 plans"]));
        assert_eq!(claude_desktop_project(&out), Some(PathBuf::from("/p/Q3 plans")));
        // A fresh file, and an empty one.
        assert!(with_officecraft(None, exe, Path::new("/p")).unwrap().contains("officecraft"));
        assert!(with_officecraft(Some("  "), exe, Path::new("/p")).is_ok());
    }

    #[test]
    fn a_settings_file_it_cant_read_is_left_alone() {
        let exe = Path::new("/x");
        assert!(with_officecraft(Some("{not json"), exe, Path::new("/p")).is_err());
        assert!(with_officecraft(Some("[1, 2]"), exe, Path::new("/p")).is_err());
        assert!(with_officecraft(Some(r#"{"mcpServers": 3}"#), exe, Path::new("/p")).is_err());
    }

    #[test]
    fn the_claude_code_command_quotes_paths() {
        let c = claude_code_command(Path::new("/A B/officecraft"), Path::new("/p/it's here"));
        assert_eq!(c, "claude mcp add officecraft -- '/A B/officecraft' mcp --project '/p/it'\\''s here'");
    }
}
