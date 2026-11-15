//! The window's keyboard: its own shortcuts (docs/decisions.md D26) and where every other key
//! goes. Keys belong to the shown document unless the chrome has keyboard focus (the new-tab
//! filter, say); the chrome's shortcuts never reach the document.

use craftworks_core::keymaps::Chord;
use egui::{Event, Key, Modifiers};

/// What a window shortcut does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    NewTab,
    CloseTab,
    Open,
    ReopenClosedTab,
    NextTab,
    PreviousTab,
    ToggleSidebar,
    SearchProject,
}

/// One window shortcut.
#[derive(Clone, Copy, Debug)]
pub struct Shortcut {
    pub action: Action,
    pub key: Key,
    pub cmd: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub label: &'static str,
}

const fn sc(action: Action, key: Key, cmd: bool, ctrl: bool, alt: bool, shift: bool, label: &'static str) -> Shortcut {
    Shortcut { action, key, cmd, ctrl, alt, shift, label }
}

/// The window's shortcuts. Each app's bindings for the same chords are listed in
/// [`INTENDED`]; any other overlap fails the collision test.
pub const SHORTCUTS: [Shortcut; 8] = [
    sc(Action::NewTab, Key::T, true, false, false, false, "New tab"),
    sc(Action::CloseTab, Key::W, true, false, false, false, "Close tab"),
    sc(Action::Open, Key::O, true, false, false, false, "Open file"),
    sc(Action::ReopenClosedTab, Key::T, true, false, true, false, "Reopen closed tab"),
    sc(Action::NextTab, Key::Tab, false, true, false, false, "Next tab"),
    sc(Action::PreviousTab, Key::Tab, false, true, false, true, "Previous tab"),
    sc(Action::ToggleSidebar, Key::S, true, true, false, false, "Show or hide the sidebar"),
    sc(Action::SearchProject, Key::F, false, true, false, true, "Search the project"),
];

/// App bindings the window takes over on purpose, by chord and command-id fragment (D26):
/// ⌘T (the owner's choice: those commands stay on their ribbons), and keys that already mean
/// the same thing in the app (close, open, next or previous document tab).
pub const INTENDED: [(&str, &str); 5] = [("⌘T", ""), ("⌘W", "close"), ("⌘O", "open"), ("⌃TAB", "tab"), ("⌃⇧TAB", "tab")];

impl Shortcut {
    pub fn chord(&self) -> Chord {
        let mut c = Chord::new(self.key.name());
        c.cmd = self.cmd;
        c.ctrl = self.ctrl;
        c.alt = self.alt;
        c.shift = self.shift;
        c
    }

    fn matches(&self, key: Key, m: Modifiers) -> bool {
        key == self.key && m.command == self.cmd && m.ctrl == self.ctrl && m.alt == self.alt && m.shift == self.shift
    }
}

/// Where an input event goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Route {
    /// A window shortcut: the window acts on it (on press); the document never sees it.
    Shortcut(Option<Action>),
    /// The document only.
    Document,
    /// The window only (the chrome has keyboard focus).
    Chrome,
    /// Both: pointer and window events (the document gets them in its own coordinates).
    Both,
}

/// Where `e` goes, given whether the chrome has keyboard focus.
pub fn route(e: &Event, chrome_focused: bool) -> Route {
    match e {
        Event::Key { key, pressed, modifiers, .. } => {
            if let Some(s) = SHORTCUTS.iter().find(|s| s.matches(*key, *modifiers)) {
                return Route::Shortcut(pressed.then_some(s.action));
            }
            if chrome_focused { Route::Chrome } else { Route::Document }
        }
        Event::Text(_) | Event::Ime(_) | Event::Copy | Event::Cut | Event::Paste(_) | Event::PasteImage(_) => {
            if chrome_focused {
                Route::Chrome
            } else {
                Route::Document
            }
        }
        _ => Route::Both,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use craftworks_core::Kind;
    use craftworks_core::keymaps::app_shortcuts;

    fn key(k: Key, m: Modifiers) -> Event {
        Event::Key { key: k, physical_key: Some(k), pressed: true, repeat: false, modifiers: m }
    }

    #[test]
    fn shortcuts_go_to_the_window_and_everything_else_to_the_document() {
        let cmd = Modifiers { command: true, mac_cmd: true, ..Default::default() };
        assert_eq!(route(&key(Key::T, cmd), false), Route::Shortcut(Some(Action::NewTab)));
        assert_eq!(route(&key(Key::T, Modifiers { alt: true, ..cmd }), false), Route::Shortcut(Some(Action::ReopenClosedTab)));
        assert_eq!(route(&key(Key::Tab, Modifiers::CTRL), true), Route::Shortcut(Some(Action::NextTab)), "even while the chrome has focus");
        assert_eq!(route(&key(Key::B, cmd), false), Route::Document);
        assert_eq!(route(&key(Key::S, Modifiers { ctrl: true, ..cmd }), false), Route::Shortcut(Some(Action::ToggleSidebar)));
        assert_eq!(route(&key(Key::S, cmd), false), Route::Document, "⌘S is still the document's Save");
        assert_eq!(route(&key(Key::Tab, Modifiers::NONE), false), Route::Document, "plain Tab is the document's");
        assert_eq!(route(&Event::Text("a".into()), false), Route::Document);
        assert_eq!(route(&Event::Text("a".into()), true), Route::Chrome);
        assert_eq!(route(&Event::Copy, false), Route::Document);
        assert_eq!(route(&Event::PointerGone, false), Route::Both);
        let released = Event::Key { key: Key::T, physical_key: Some(Key::T), pressed: false, repeat: false, modifiers: cmd };
        assert_eq!(route(&released, false), Route::Shortcut(None), "the release never reaches the document either");
    }

    /// No window shortcut takes a key an app uses, except the ones taken on purpose (D26).
    #[test]
    fn window_shortcuts_collide_only_where_intended() {
        let mut unexpected = Vec::new();
        let mut intended = Vec::new();
        for kind in Kind::ALL {
            for app in app_shortcuts(kind) {
                for s in &SHORTCUTS {
                    if app.chord != s.chord() {
                        continue;
                    }
                    let label = app.chord.to_string();
                    let ok = INTENDED.iter().any(|(c, frag)| *c == label && app.command.to_lowercase().contains(&frag.to_lowercase()));
                    let line = format!("{label} ({}): {kind:?} {}", s.label, app.command);
                    if ok { intended.push(line) } else { unexpected.push(line) }
                }
            }
        }
        println!("taken on purpose:\n  {}", intended.join("\n  "));
        assert!(unexpected.is_empty(), "window shortcuts that would take an app's key:\n  {}", unexpected.join("\n  "));
    }
}
