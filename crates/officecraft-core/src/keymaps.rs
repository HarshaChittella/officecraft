//! Each Craft app's keyboard shortcuts, read from its own command registry and normalised to
//! macOS chords, so the shell can check its own shortcuts against them (docs/decisions.md D26).

use crate::Kind;
use std::fmt;

/// A key with modifiers, as macOS names them: ⌘ (`cmd`), ⌥ (`alt`), ⌃ (`ctrl`), ⇧ (`shift`).
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Chord {
    pub cmd: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    /// The key, upper-case for letters (`T`, `TAB`, `F4`, `[`, `=`).
    pub key: String,
}

impl Chord {
    pub fn new(key: &str) -> Chord {
        Chord { cmd: false, ctrl: false, alt: false, shift: false, key: normalise_key(key) }
    }
    pub fn cmd(mut self) -> Chord {
        self.cmd = true;
        self
    }
    pub fn ctrl(mut self) -> Chord {
        self.ctrl = true;
        self
    }
    pub fn shift(mut self) -> Chord {
        self.shift = true;
        self
    }
    pub fn alt(mut self) -> Chord {
        self.alt = true;
        self
    }
}

impl fmt::Display for Chord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fn m(on: bool, s: &'static str) -> &'static str {
            if on { s } else { "" }
        }
        write!(f, "{}{}{}{}{}", m(self.ctrl, "⌃"), m(self.alt, "⌥"), m(self.shift, "⇧"), m(self.cmd, "⌘"), self.key)
    }
}

/// One app command and the chord that runs it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppShortcut {
    pub kind: Kind,
    pub command: String,
    pub chord: Chord,
}

fn normalise_key(k: &str) -> String {
    let k = k.trim();
    // Punctuation by its symbol, however it's spelt: egui's `Backslash` and an app's `\` are
    // the same key.
    if let Some(sym) = egui::Key::from_name(k).map(egui::Key::symbol_or_name)
        && sym.len() == 1
        && sym.chars().all(|c| c.is_ascii_punctuation())
    {
        return sym.to_string();
    }
    match k.to_ascii_lowercase().as_str() {
        "esc" | "escape" => "ESCAPE".into(),
        "return" | "enter" => "ENTER".into(),
        "left" | "arrowleft" => "ARROWLEFT".into(),
        "right" | "arrowright" => "ARROWRIGHT".into(),
        "up" | "arrowup" => "ARROWUP".into(),
        "down" | "arrowdown" => "ARROWDOWN".into(),
        "del" | "delete" => "DELETE".into(),
        "pgup" | "pageup" => "PAGEUP".into(),
        "pgdn" | "pagedown" => "PAGEDOWN".into(),
        _ => k.to_ascii_uppercase(),
    }
}

/// Parses a shortcut as the apps write it: `Mod+Shift+T`, `Cmd+Shift+K`, `Ctrl+[`, `Cmd++`,
/// `F7`, or alternatives joined by ` / ` (`Mod+Shift+. / Mod+Shift+>`). `Mod` is ⌘ on macOS.
/// Anything that isn't a shortcut (some registries keep other strings there) gives nothing.
pub fn parse(s: &str) -> Vec<Chord> {
    s.split(" / ").filter_map(parse_one).collect()
}

fn parse_one(s: &str) -> Option<Chord> {
    let s = s.trim();
    if s.is_empty() || s.contains(' ') {
        return None;
    }
    // A trailing "+" key ("Cmd++") is the key, not a separator.
    let (mods, key) = if let Some(rest) = s.strip_suffix("++") {
        (rest, "+")
    } else {
        match s.rsplit_once('+') {
            Some((m, k)) if !k.is_empty() => (m, k),
            Some(_) => return None,
            None => ("", s),
        }
    };
    let mut c = Chord::new(key);
    if !mods.is_empty() {
        for m in mods.split('+') {
            match m.to_ascii_lowercase().as_str() {
                "mod" | "cmd" | "command" | "meta" | "super" => c.cmd = true,
                "ctrl" | "control" => c.ctrl = true,
                "alt" | "option" | "opt" => c.alt = true,
                "shift" => c.shift = true,
                _ => return None,
            }
        }
    }
    let keyish = c.key.chars().count() == 1 || c.key.starts_with('F') || c.key.chars().all(|ch| ch.is_ascii_alphabetic());
    keyish.then_some(c)
}

/// Every shortcut `kind`'s app registers, as macOS chords.
pub fn app_shortcuts(kind: Kind) -> Vec<AppShortcut> {
    let mut out = Vec::new();
    let mut push = |command: &str, shortcut: &str| {
        for chord in parse(shortcut) {
            out.push(AppShortcut { kind, command: command.to_string(), chord });
        }
    };
    match kind {
        Kind::Document => {
            for c in wordcraft_engine::cmd::registry().all() {
                push(c.id, c.shortcut);
            }
        }
        Kind::Sheet => {
            for c in gridcraft_engine::Session::new().commands() {
                if let Some(s) = c.shortcut_mac.or(c.shortcut) {
                    push(c.id, s);
                }
            }
        }
        Kind::Presentation => {
            for c in deckcraft_engine::Session::new().commands() {
                if let Some(s) = c.shortcut {
                    push(c.id, s);
                }
            }
            for (id, _, shortcut, _) in deckcraft_ui_egui::UI_COMMANDS {
                if let Some(s) = shortcut {
                    push(id, s);
                }
            }
        }
        Kind::Pdf => {
            for c in pdfcraft_engine::commands::COMMANDS {
                if let Some(s) = c.shortcut {
                    out.push(AppShortcut {
                        kind,
                        command: c.id.to_string(),
                        chord: Chord { cmd: s.command, ctrl: s.mac_ctrl, alt: false, shift: s.shift, key: normalise_key(s.key) },
                    });
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_apps_formats_parse_to_the_same_chords() {
        assert_eq!(parse("Mod+Shift+T"), vec![Chord::new("T").cmd().shift()]);
        assert_eq!(parse("Cmd+Shift+t"), vec![Chord::new("T").cmd().shift()]);
        assert_eq!(parse("Ctrl+["), vec![Chord::new("[").ctrl()]);
        assert_eq!(parse("Cmd++"), vec![Chord::new("+").cmd()]);
        assert_eq!(parse("F7"), vec![Chord::new("F7")]);
        assert_eq!(parse("Mod+Shift+. / Mod+Shift+>"), vec![Chord::new(".").cmd().shift(), Chord::new(">").cmd().shift()]);
        assert_eq!(parse("Alt+Tab"), vec![Chord::new("TAB").alt()]);
        assert_eq!(parse("Left"), vec![Chord::new("ARROWLEFT")]);
        assert_eq!(Chord::new("Backslash"), Chord::new("\\"), "a key is the same key however it is spelt");
    }

    #[test]
    fn things_that_are_not_shortcuts_parse_to_nothing() {
        for s in ["", "'Q1, Q2'!B2", "M 0 0 C 0.1", "Foo+T", "Cmd+"] {
            assert!(parse(s).is_empty(), "{s:?}");
        }
    }

    #[test]
    fn every_app_reports_its_shortcuts() {
        for kind in Kind::ALL {
            let s = app_shortcuts(kind);
            assert!(s.len() > 20, "{kind:?} has only {} shortcuts", s.len());
            assert!(s.iter().any(|a| a.chord == Chord::new("Z").cmd()), "{kind:?} has ⌘Z (undo)");
        }
    }

    #[test]
    fn chords_print_the_mac_way() {
        assert_eq!(Chord::new("TAB").ctrl().shift().to_string(), "⌃⇧TAB");
        assert_eq!(Chord::new("T").cmd().to_string(), "⌘T");
    }
}
