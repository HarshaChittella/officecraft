//! The native macOS menu bar (craft-menu, as the Craft apps use it): File, Edit, View and
//! Window are the window's; a menu for the shown kind of document (Format for documents,
//! sheets and presentations, Document for PDFs) holds that app's common commands.
//!
//! A click on an item with a shortcut sends that keystroke to the document, so it takes the
//! same path as the key (the app's own handling, dialogs included). Items without a shortcut
//! run the app's command. Key equivalents come back as key presses (`NativeMenu::raw_input`)
//! and are routed like any key (keys::route).

use craft_keymap::{Chord, CommandRef};
use craft_menu::{Document, Item, ItemRole, Menu, MenuBar, MenuRole, Node};
use craftworks_core::Kind;
use std::path::PathBuf;

/// What a menu command means to the app.
#[derive(Clone, Debug, PartialEq)]
pub enum MenuCommand {
    NewTab,
    New(Kind),
    Open,
    OpenRecent(PathBuf),
    CloseTab,
    ReopenClosedTab,
    NextTab,
    PreviousTab,
    ShowTab(u64),
    Quit,
    /// Send this keystroke to the shown document (a menu item with a shortcut).
    Key(String),
    /// The clipboard: copy, cut, paste for the shown document.
    Copy,
    Cut,
    Paste,
    /// Run this command of the shown app (an item without a shortcut).
    Run(String),
}

impl MenuCommand {
    /// The command's id in the menu (`cw.…`).
    pub fn id(&self) -> String {
        match self {
            MenuCommand::NewTab => "cw.newTab".into(),
            MenuCommand::New(k) => format!("cw.new.{}", k.label()),
            MenuCommand::Open => "cw.open".into(),
            MenuCommand::OpenRecent(p) => format!("cw.recent:{}", p.display()),
            MenuCommand::CloseTab => "cw.closeTab".into(),
            MenuCommand::ReopenClosedTab => "cw.reopenTab".into(),
            MenuCommand::NextTab => "cw.nextTab".into(),
            MenuCommand::PreviousTab => "cw.previousTab".into(),
            MenuCommand::ShowTab(id) => format!("cw.tab:{id}"),
            MenuCommand::Quit => "cw.quit".into(),
            MenuCommand::Key(c) => format!("cw.key:{c}"),
            MenuCommand::Copy => "cw.copy".into(),
            MenuCommand::Cut => "cw.cut".into(),
            MenuCommand::Paste => "cw.paste".into(),
            MenuCommand::Run(id) => format!("cw.run:{id}"),
        }
    }

    /// The command a menu id stands for.
    pub fn from_id(id: &str) -> Option<MenuCommand> {
        let (head, rest) = id.split_once(':').map_or((id, None), |(h, r)| (h, Some(r)));
        Some(match (head, rest) {
            ("cw.newTab", None) => MenuCommand::NewTab,
            ("cw.open", None) => MenuCommand::Open,
            ("cw.closeTab", None) => MenuCommand::CloseTab,
            ("cw.reopenTab", None) => MenuCommand::ReopenClosedTab,
            ("cw.nextTab", None) => MenuCommand::NextTab,
            ("cw.previousTab", None) => MenuCommand::PreviousTab,
            ("cw.quit", None) => MenuCommand::Quit,
            ("cw.copy", None) => MenuCommand::Copy,
            ("cw.cut", None) => MenuCommand::Cut,
            ("cw.paste", None) => MenuCommand::Paste,
            ("cw.recent", Some(p)) => MenuCommand::OpenRecent(PathBuf::from(p)),
            ("cw.tab", Some(n)) => MenuCommand::ShowTab(n.parse().ok()?),
            ("cw.key", Some(c)) => MenuCommand::Key(c.to_string()),
            ("cw.run", Some(c)) => MenuCommand::Run(c.to_string()),
            (h, None) if h.starts_with("cw.new.") => {
                let label = h.trim_start_matches("cw.new.");
                MenuCommand::New(Kind::ALL.into_iter().find(|k| k.label() == label)?)
            }
            _ => return None,
        })
    }
}

/// One tab as the Window menu lists it.
pub struct TabEntry {
    pub id: u64,
    pub label: String,
    pub active: bool,
}

/// What the menus show.
pub struct MenuState<'a> {
    pub shown: Option<Kind>,
    pub tabs: Vec<TabEntry>,
    pub recent: Vec<&'a std::path::Path>,
    pub can_reopen: bool,
}

fn chord(s: &str) -> Option<Chord> {
    Chord::parse(s).ok()
}

fn item(cmd: MenuCommand, label: &str, shortcut: Option<&str>) -> Node {
    let mut it = Item::new(CommandRef::new(cmd.id()), label);
    if let Some(c) = shortcut.and_then(chord) {
        it = it.shortcut(c);
    }
    Node::Item(it)
}

/// An item that sends `keys` to the document when clicked, and shows them as its shortcut.
fn key_item(label: &str, keys: &str) -> Node {
    item(MenuCommand::Key(keys.to_string()), label, Some(keys))
}

fn enabled(node: Node, on: bool) -> Node {
    match node {
        Node::Item(i) => Node::Item(i.enabled(on)),
        other => other,
    }
}

/// The app's own commands for the shown kind: title and items (each id is checked by a test
/// against the app's registry).
pub fn kind_menu(kind: Kind) -> (&'static str, Vec<Node>) {
    match kind {
        Kind::Document => (
            "Format",
            vec![
                key_item("Bold", "⌘B"),
                key_item("Italic", "⌘I"),
                key_item("Underline", "⌘U"),
                Node::Separator,
                key_item("Align Left", "⌘L"),
                key_item("Center", "⌘E"),
                key_item("Align Right", "⌘R"),
                key_item("Justify", "⌘J"),
                Node::Separator,
                key_item("Font…", "⌘D"),
            ],
        ),
        Kind::Sheet => (
            "Format",
            vec![
                key_item("Bold", "⌘B"),
                key_item("Italic", "⌘I"),
                key_item("Underline", "⌘U"),
                Node::Separator,
                key_item("Align Left", "⌘L"),
                key_item("Center", "⌘E"),
                Node::Separator,
                key_item("Fill Down", "⌘D"),
                key_item("Fill Right", "⌘R"),
            ],
        ),
        Kind::Presentation => (
            "Format",
            vec![
                key_item("Bold", "⌘B"),
                key_item("Italic", "⌘I"),
                key_item("Underline", "⌘U"),
                Node::Separator,
                key_item("Align Left", "⌘L"),
                key_item("Center", "⌘E"),
                key_item("Align Right", "⌘R"),
                key_item("Justify", "⌘J"),
                Node::Separator,
                key_item("Duplicate", "⌘D"),
            ],
        ),
        Kind::Pdf => (
            "Document",
            vec![
                item(MenuCommand::Run("page.rotate".into()), "Rotate Page", None),
                key_item("Add Bookmark", "⌘B"),
                Node::Separator,
                key_item("Properties…", "⌘D"),
                key_item("Full Screen", "⌘L"),
            ],
        ),
    }
}

/// Redo's key in each app (⇧⌘Z in WordCraft and PdfCraft, ⌘Y in GridCraft and DeckCraft).
pub fn redo_keys(kind: Kind) -> &'static str {
    match kind {
        Kind::Document | Kind::Pdf => "⇧⌘Z",
        Kind::Sheet | Kind::Presentation => "⌘Y",
    }
}

/// The whole menu bar for `s`.
pub fn bar(s: &MenuState<'_>) -> MenuBar {
    let doc = s.shown.is_some();
    let mut recent: Vec<Node> = s
        .recent
        .iter()
        .take(10)
        .map(|p| item(MenuCommand::OpenRecent(p.to_path_buf()), &p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(), None))
        .collect();
    if recent.is_empty() {
        recent.push(enabled(item(MenuCommand::Open, "No recent files", None), false));
    }
    let file = vec![
        item(MenuCommand::NewTab, "New Tab", Some("⌘T")),
        item(MenuCommand::New(Kind::Document), "New Document", None),
        item(MenuCommand::New(Kind::Sheet), "New Sheet", None),
        item(MenuCommand::New(Kind::Presentation), "New Presentation", None),
        Node::Separator,
        item(MenuCommand::Open, "Open…", Some("⌘O")),
        Node::Submenu { label: "Open Recent".into(), children: recent, platforms: Default::default(), role: None },
        Node::Separator,
        enabled(item(MenuCommand::CloseTab, "Close Tab", Some("⌘W")), doc),
        enabled(item(MenuCommand::ReopenClosedTab, "Reopen Closed Tab", Some("⌥⌘T")), s.can_reopen),
        Node::Separator,
        enabled(key_item("Save", "⌘S"), doc),
        enabled(key_item("Save As…", "⇧⌘S"), doc),
        Node::Separator,
        enabled(key_item("Print…", "⌘P"), doc),
    ];
    let redo = s.shown.map_or("⇧⌘Z", redo_keys);
    let edit = vec![
        enabled(key_item("Undo", "⌘Z"), doc),
        enabled(key_item("Redo", redo), doc),
        Node::Separator,
        enabled(item(MenuCommand::Cut, "Cut", Some("⌘X")), doc),
        enabled(item(MenuCommand::Copy, "Copy", Some("⌘C")), doc),
        enabled(item(MenuCommand::Paste, "Paste", Some("⌘V")), doc),
        enabled(key_item("Select All", "⌘A"), doc),
        Node::Separator,
        enabled(key_item("Find…", "⌘F"), doc),
    ];
    let view = vec![
        enabled(item(MenuCommand::NextTab, "Show Next Tab", Some("⌃Tab")), s.tabs.len() > 1),
        enabled(item(MenuCommand::PreviousTab, "Show Previous Tab", Some("⌃⇧Tab")), s.tabs.len() > 1),
    ];
    let mut menus = vec![
        Menu::new("Craftworks", vec![Node::Item(Item::new(CommandRef::new(MenuCommand::Quit.id()), "Quit Craftworks").role(ItemRole::Quit))])
            .role(MenuRole::App),
        Menu::new("File", file).role(MenuRole::File),
        Menu::new("Edit", edit).role(MenuRole::Edit),
        Menu::new("View", view).role(MenuRole::View),
    ];
    if let Some(kind) = s.shown {
        let (title, items) = kind_menu(kind);
        menus.push(Menu::new(title, items));
    }
    menus.push(Menu::new("Window", vec![]).role(MenuRole::Window));
    menus.push(Menu::new("Help", vec![]).role(MenuRole::Help));
    MenuBar {
        menus,
        documents: s.tabs.iter().map(|t| Document::new(CommandRef::new(MenuCommand::ShowTab(t.id).id()), t.label.clone()).active(t.active)).collect(),
    }
}

/// The key events a chord stands for (a press and a release), for the shown document.
pub fn key_events(keys: &str) -> Vec<egui::Event> {
    let Ok(c) = Chord::parse(keys) else { return Vec::new() };
    let Some(key) = egui::Key::from_name(c.key.name()) else { return Vec::new() };
    let m = egui::Modifiers { alt: c.mods.alt, ctrl: c.mods.ctrl, shift: c.mods.shift, mac_cmd: c.mods.cmd, command: c.mods.cmd };
    vec![
        egui::Event::Key { key, physical_key: Some(key), pressed: true, repeat: false, modifiers: m },
        egui::Event::Key { key, physical_key: Some(key), pressed: false, repeat: false, modifiers: m },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use craftworks_core::keymaps::{Chord as AppChord, app_shortcuts};

    #[test]
    fn menu_ids_round_trip() {
        for c in [
            MenuCommand::NewTab,
            MenuCommand::New(Kind::Sheet),
            MenuCommand::OpenRecent(PathBuf::from("/a/b c.docx")),
            MenuCommand::ShowTab(42),
            MenuCommand::Key("⇧⌘S".into()),
            MenuCommand::Run("page.rotate".into()),
            MenuCommand::Quit,
        ] {
            assert_eq!(MenuCommand::from_id(&c.id()), Some(c.clone()), "{}", c.id());
        }
        assert_eq!(MenuCommand::from_id("nonsense"), None);
    }

    /// Every shortcut the kind menus, Edit and File send is one the shown app binds.
    #[test]
    fn menu_shortcuts_are_the_apps_own() {
        let to_app = |keys: &str| {
            let c = Chord::parse(keys).unwrap_or_else(|e| panic!("{keys}: {e:?}"));
            let mut a = AppChord::new(c.key.name());
            a.cmd = c.mods.cmd;
            a.ctrl = c.mods.ctrl;
            a.alt = c.mods.alt;
            a.shift = c.mods.shift;
            a
        };
        for kind in Kind::ALL {
            let theirs = app_shortcuts(kind);
            let has = |keys: &str| theirs.iter().any(|s| s.chord == to_app(keys));
            let (_, items) = kind_menu(kind);
            for node in &items {
                if let Node::Item(i) = node
                    && let Some(MenuCommand::Key(k)) = MenuCommand::from_id(&i.command.id)
                {
                    assert!(has(&k), "{kind:?} doesn't bind {k} ({})", i.label);
                }
            }
            for k in ["⌘Z", redo_keys(kind), "⌘S", "⇧⌘S", "⌘P", "⌘F"] {
                assert!(has(k), "{kind:?} doesn't bind {k}");
            }
        }
    }

    #[test]
    fn the_bar_lists_the_tabs_and_the_shown_kinds_menu() {
        let s = MenuState {
            shown: Some(Kind::Pdf),
            tabs: vec![TabEntry { id: 1, label: "a.docx".into(), active: false }, TabEntry { id: 2, label: "b.pdf".into(), active: true }],
            recent: vec![],
            can_reopen: false,
        };
        let b = bar(&s);
        assert_eq!(b.documents.len(), 2);
        assert!(b.documents[1].active);
        assert!(b.menus.iter().any(|m| m.title == "Document"), "PDFs get the Document menu");
        let built = craft_menu::mac_layout(&b, &craft_menu::LayoutOptions::new("Craftworks"));
        assert!(built.bar.menus.len() >= 5);
    }

    #[test]
    fn keys_become_press_and_release() {
        let e = key_events("⇧⌘S");
        assert_eq!(e.len(), 2);
        assert!(matches!(e[0], egui::Event::Key { key: egui::Key::S, pressed: true, modifiers, .. } if modifiers.command && modifiers.shift));
    }
}
