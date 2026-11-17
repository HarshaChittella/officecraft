//! Where Ask AI gets its credential (docs/ai-chat.md), first match wins:
//!
//! 1. `ANTHROPIC_API_KEY` in the environment (for development);
//! 2. the key the person saved in the panel: the macOS Keychain (elsewhere a file in the settings
//!    folder that only they can read; never in a project);
//! 3. the `ant` sign-in, when they chose it in the panel: a short-lived token from
//!    `ant auth print-credentials --access-token`, billed to their Console organization.
//!
//! A Claude Pro or Max subscription can't be used here: Anthropic doesn't allow other apps to
//! sign in with one.

use super::api::Credential;
use std::path::PathBuf;
use std::process::{Command, Stdio};

#[cfg(target_os = "macos")]
const SERVICE: &str = "OfficeCraft";
#[cfg(target_os = "macos")]
const ACCOUNT: &str = "Anthropic API key";

/// The credential to use now, if there is one.
pub fn credential() -> Option<Credential> {
    if let Some(k) = std::env::var("ANTHROPIC_API_KEY").ok().filter(|k| !k.trim().is_empty()) {
        return Some(Credential::ApiKey(k.trim().to_string()));
    }
    if let Some(k) = saved_key() {
        return Some(Credential::ApiKey(k));
    }
    if use_ant() {
        return ant_token().map(Credential::Token);
    }
    None
}

/// Whether a key has been saved in the panel.
pub fn has_saved_key() -> bool {
    saved_key().is_some()
}

#[cfg(target_os = "macos")]
fn saved_key() -> Option<String> {
    let bytes = security_framework::passwords::get_generic_password(SERVICE, ACCOUNT).ok()?;
    String::from_utf8(bytes).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

#[cfg(not(target_os = "macos"))]
fn key_file() -> Option<PathBuf> {
    crate::settings::dir().map(|d| d.join("anthropic-key"))
}

#[cfg(not(target_os = "macos"))]
fn saved_key() -> Option<String> {
    std::fs::read_to_string(key_file()?).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

/// Saves the key the person pasted.
pub fn save_key(key: &str) -> Result<(), String> {
    let key = key.trim();
    if key.is_empty() || key.contains(char::is_whitespace) {
        return Err("That doesn't look like an API key.".into());
    }
    #[cfg(target_os = "macos")]
    {
        security_framework::passwords::set_generic_password(SERVICE, ACCOUNT, key.as_bytes()).map_err(|e| format!("Couldn't save it in the Keychain: {e}"))
    }
    #[cfg(not(target_os = "macos"))]
    {
        let path = key_file().ok_or("Couldn't find the settings folder")?;
        if let Some(d) = path.parent() {
            std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
        }
        crate::settings::write_atomic(&path, key.as_bytes()).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
        }
        Ok(())
    }
}

/// Removes the saved key (and the choice of the `ant` sign-in).
pub fn forget() {
    #[cfg(target_os = "macos")]
    let _ = security_framework::passwords::delete_generic_password(SERVICE, ACCOUNT);
    #[cfg(not(target_os = "macos"))]
    if let Some(p) = key_file() {
        let _ = std::fs::remove_file(p);
    }
    set_use_ant(false);
}

fn prefs_file() -> Option<PathBuf> {
    crate::settings::dir().map(|d| d.join("ai-chat.json"))
}

/// Whether the person chose the `ant` sign-in.
pub fn use_ant() -> bool {
    prefs_file()
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
        .is_some_and(|v| v["auth"] == "ant")
}

pub fn set_use_ant(on: bool) {
    if let Some(p) = prefs_file() {
        let v = serde_json::json!({"auth": if on { "ant" } else { "key" }});
        let _ = crate::settings::write_atomic(&p, v.to_string().as_bytes());
    }
}

/// Anthropic's `ant` command, if it's installed (an app started from Finder doesn't get the
/// shell's PATH, so the usual places are tried too).
pub fn ant_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    dirs.extend(["/opt/homebrew/bin", "/usr/local/bin"].map(PathBuf::from));
    if let Some(h) = &home {
        dirs.push(h.join("go/bin"));
    }
    let name = if cfg!(windows) { "ant.exe" } else { "ant" };
    dirs.into_iter().map(|d| d.join(name)).find(|p| p.is_file())
}

/// A fresh access token from the `ant` sign-in; `None` when it isn't signed in.
pub fn ant_token() -> Option<String> {
    let out = Command::new(ant_path()?).args(["auth", "print-credentials", "--access-token"]).stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
    let token = String::from_utf8(out.stdout).ok()?.trim().to_string();
    (out.status.success() && !token.is_empty() && !token.contains(char::is_whitespace)).then_some(token)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_with_spaces_or_nothing_is_refused_before_anything_is_saved() {
        assert!(save_key("  ").is_err());
        assert!(save_key("sk ant").is_err());
    }
}
