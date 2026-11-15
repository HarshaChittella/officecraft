//! File panels (`rfd`, the system's open and save panels) and opening links.

use std::path::Path;

/// The open panel, filtered to `extensions` (all files when empty).
pub fn pick_open(title: &str, filter: &str, extensions: &[&str]) -> Option<String> {
    let mut d = rfd::FileDialog::new().set_title(title);
    if !extensions.is_empty() {
        d = d.add_filter(filter, extensions);
    }
    d.pick_file().and_then(|p| p.to_str().map(str::to_string))
}

/// The save panel, starting from `suggested` (a name, or a path).
pub fn pick_save(suggested: &str) -> Option<String> {
    let p = Path::new(suggested);
    let mut d = rfd::FileDialog::new();
    if let Some(name) = p.file_name().and_then(|n| n.to_str()) {
        d = d.set_file_name(name);
    }
    if let Some(dir) = p.parent().filter(|d| d.is_dir()) {
        d = d.set_directory(dir);
    }
    d.save_file().and_then(|p| p.to_str().map(str::to_string))
}

/// Whether the system may be asked to open `url`: web, mail and file links only (no app
/// schemes a document could use to start programs).
pub fn openable(url: &str) -> bool {
    let lower = url.trim().to_ascii_lowercase();
    ["https://", "http://", "mailto:", "file://"].iter().any(|s| lower.starts_with(s)) && !lower.contains('\n')
}

/// Opens a link with the system's default app (`open`, macOS).
/// Shows `path` selected in a Finder window.
pub fn reveal(path: &std::path::Path) {
    #[cfg(target_os = "macos")]
    if let Err(e) = std::process::Command::new("open").arg("-R").arg("--").arg(path).spawn() {
        log::warn!("revealing {}: {e}", path.display());
    }
    #[cfg(not(target_os = "macos"))]
    log::info!("reveal {}: not on this system", path.display());
}

pub fn open_url(url: &str) {
    if !openable(url) {
        log::warn!("not opening {url:?}: only web, mail and file links open");
        return;
    }
    if let Err(e) = std::process::Command::new("open").arg("--").arg(url).spawn() {
        log::warn!("opening {url}: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_web_mail_and_file_links_open() {
        for ok in ["https://example.com", "HTTP://x", "mailto:a@b.c", "file:///tmp/a.pdf"] {
            assert!(openable(ok), "{ok}");
        }
        for no in ["javascript:alert(1)", "x-apple.systempreferences:", "ssh://host", "-a Calculator", "https://a\nb"] {
            assert!(!openable(no), "{no}");
        }
    }
}
