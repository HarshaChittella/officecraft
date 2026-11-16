//! The native macOS menu bar (craft-menu, as the Craft apps use it). It follows the front tab
//! (D42): the shown app's own menus and shortcuts, around a few items that are the window's
//! (the app menu, File's tab items, Edit's clipboard, View's tab items, Window).
//!
//! - WordCraft, GridCraft, DeckCraft: built from the app's command catalogue
//!   (`craftworks_core::catalog`), ribbon tabs mapped onto Mac menus (File, Edit, View,
//!   Insert, Format, Table, Data, Formulas, Arrange, Slide Show, Tools), each ribbon group a
//!   section (a submenu when the menu is long).
//! - PdfCraft: its own menu bar (`Editor::native_menus`), clicks run through its own handler.
//!
//! A click on an item with a shortcut sends that keystroke to the document, so it takes the
//! same path as the key (the app's own handling, dialogs included). Items without a shortcut
//! run the app's command. Key equivalents come back as key presses (`NativeMenu::raw_input`)
//! and are routed like any key (keys::route).

use craft_keymap::{Chord, CommandRef};
use craft_menu::{Document, Item, ItemRole, Menu, MenuBar, MenuRole, Node};
use craftworks_core::Kind;
use craftworks_core::catalog::MenuEntry;
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
    /// An item of the shown app's own menu bar (PdfCraft), by its command key.
    AppMenu(String),
    ToggleSidebar,
    SearchProject,
    /// The chrome's text size, in percent.
    TextScale(u32),
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
            MenuCommand::AppMenu(key) => format!("cw.app:{key}"),
            MenuCommand::ToggleSidebar => "cw.sidebar".into(),
            MenuCommand::SearchProject => "cw.search".into(),
            MenuCommand::TextScale(p) => format!("cw.textScale:{p}"),
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
            ("cw.app", Some(c)) => MenuCommand::AppMenu(c.to_string()),
            ("cw.sidebar", None) => MenuCommand::ToggleSidebar,
            ("cw.search", None) => MenuCommand::SearchProject,
            ("cw.textScale", Some(p)) => MenuCommand::TextScale(p.parse().ok()?),
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

/// The Mac menu a ribbon tab's commands go in, and whether they go as a submenu named after
/// the tab (contextual tabs: Picture Format, Chart Design…). `None`: left out of the menus.
fn mac_menu(tab: &str) -> Option<(&'static str, bool)> {
    Some(match tab {
        "File" => ("File", false),
        "Edit" | "Clipboard" => ("Edit", false),
        // Typing and moving the caret (Left, Backspace, Enter…): keys, not menu items.
        "Editing" | "Navigation" => return None,
        "View" | "Window" | "Outlining" | "Status Bar" => ("View", false),
        "Slide Master" | "Notes Master" | "Handout Master" | "Selection Pane" => ("View", true),
        "Insert" => ("Insert", false),
        "Draw" | "Header & Footer" | "Equation" => ("Insert", true),
        "Home" | "Format" => ("Format", false),
        "Design" | "Layout" | "Page Layout" | "Format Shape" | "Format Background" => ("Format", true),
        "Shape Format" | "Picture Format" | "Video Format" | "SmartArt Design" | "Chart Design" | "Diagram Design" => ("Format", true),
        "Table Layout" | "Table Design" | "Table" => ("Table", false),
        "Data" | "PivotTable Design" | "PivotTable Analyze" => ("Data", false),
        "Formulas" => ("Formulas", false),
        "Arrange" => ("Arrange", false),
        "Slide Show" | "Transitions" | "Animations" | "Playback" => ("Slide Show", false),
        "Review" | "References" | "Mailings" | "Developer" | "Zotero" | "Agents" | "Tools" | "Automate" => ("Tools", true),
        "Quick Access Toolbar" | "Title Bar" => return None,
        _ => ("Tools", true),
    })
}

/// The Mac menus in their order (those after View are the app's; empty ones are left out).
const ORDER: [&str; 11] = ["File", "Edit", "View", "Insert", "Format", "Table", "Data", "Formulas", "Arrange", "Slide Show", "Tools"];

/// Commands the window does itself, so the app's own copies stay out of its menus.
fn windows_own(id: &str, label: &str) -> bool {
    let l = label.trim_end_matches('…').to_lowercase();
    let own_labels = ["undo", "redo", "cut", "copy", "paste", "select all", "find", "new", "open", "close", "quit", "exit"];
    own_labels.contains(&l.as_str())
        || ["file.new", "file.open", "file.close", "file.recent", "file.exit", "file.quit", "app.quit", "app.closePresentation"].contains(&id)
        || id.ends_with(".undo")
        || id.ends_with(".redo")
}

/// A window shortcut (⌘T, ⌘W…): an app item with one of these shows no shortcut, since the
/// key does the window's thing.
fn is_window_chord(c: &Chord) -> bool {
    craftworks_shell::keys::SHORTCUTS.iter().any(|s| {
        let w = s.chord();
        let mine = to_app_chord(c);
        mine == w
    })
}

fn to_app_chord(c: &Chord) -> craftworks_core::keymaps::Chord {
    let mut a = craftworks_core::keymaps::Chord::new(c.key.name());
    a.cmd = c.mods.cmd;
    a.ctrl = c.mods.ctrl;
    a.alt = c.mods.alt;
    a.shift = c.mods.shift;
    a
}

/// An app command as a menu item: its keystroke when it has a shortcut the window doesn't
/// take, else the command (if a click can run it).
fn entry_item(e: &MenuEntry) -> Option<Node> {
    let chord = e.shortcut.as_deref().and_then(|s| Chord::parse(s).ok()).filter(|c| !is_window_chord(c));
    let label = e.label.as_str();
    match chord {
        Some(c) => {
            let keys = c.to_string();
            Some(Node::Item(Item::new(CommandRef::new(MenuCommand::Key(keys).id()), label).shortcut(c)))
        }
        None if !e.needs_args => Some(item(MenuCommand::Run(e.id.clone()), label, None)),
        None => None,
    }
}

/// Part of a menu: the ribbon tab it's a submenu for (if any), its group, its items.
type Section = (Option<String>, String, Vec<Node>);

/// The app's commands, sorted into Mac menus: title and items.
pub fn catalog_menus(entries: &[MenuEntry]) -> Vec<(&'static str, Vec<Node>)> {
    // menu → sections, in first-seen order: (submenu title or none, group, items)
    let mut menus: Vec<(&'static str, Vec<Section>)> = Vec::new();
    for e in entries {
        let Some(tab) = e.place.first() else { continue };
        let Some((mut menu, as_sub)) = mac_menu(tab) else { continue };
        // Home's clipboard and editing groups are Edit's, as on the Mac.
        if tab == "Home" && e.place.get(1).is_some_and(|g| g == "Clipboard" || g == "Editing") {
            menu = "Edit";
        }
        if windows_own(&e.id, &e.label) {
            continue;
        }
        let Some(node) = entry_item(e) else { continue };
        let group = e.place.get(1).cloned().unwrap_or_default();
        let sub = as_sub.then(|| tab.clone());
        let m = match menus.iter_mut().find(|m| m.0 == menu) {
            Some(m) => m,
            None => {
                menus.push((menu, Vec::new()));
                let last = menus.len() - 1;
                &mut menus[last]
            }
        };
        match m.1.iter_mut().find(|s| s.0 == sub && s.1 == group) {
            Some(s) => s.2.push(node),
            None => m.1.push((sub, group, vec![node])),
        }
    }
    let mut out = Vec::new();
    for title in ORDER {
        let Some((_, sections)) = menus.iter().find(|m| m.0 == title) else { continue };
        let direct: usize = sections.iter().filter(|s| s.0.is_none()).map(|s| s.2.len()).sum();
        // A long menu shows each group (of four or more) as a submenu.
        let long = direct > 30;
        let mut nodes: Vec<Node> = Vec::new();
        let mut subs: Vec<(String, Vec<Node>)> = Vec::new();
        for (sub, group, items) in sections {
            match sub {
                Some(tab) => match subs.iter_mut().find(|s| s.0 == *tab) {
                    Some(s) => {
                        s.1.push(Node::Separator);
                        s.1.extend(items.iter().cloned());
                    }
                    None => subs.push((tab.clone(), items.clone())),
                },
                None if long && items.len() >= 4 && !group.is_empty() => {
                    nodes.push(Node::Submenu { label: group.clone(), children: items.clone(), platforms: Default::default(), role: None });
                }
                None => {
                    if !nodes.is_empty() {
                        nodes.push(Node::Separator);
                    }
                    nodes.extend(items.iter().cloned());
                }
            }
        }
        if !subs.is_empty() {
            if !nodes.is_empty() {
                nodes.push(Node::Separator);
            }
            for (label, children) in subs {
                // A ribbon tab named like a group already here joins that group's submenu.
                let same = nodes.iter_mut().find_map(|n| match n {
                    Node::Submenu { label: l, children: c, .. } if *l == label => Some(c),
                    _ => None,
                });
                match same {
                    Some(c) => {
                        c.push(Node::Separator);
                        c.extend(children);
                    }
                    None => nodes.push(Node::Submenu { label, children, platforms: Default::default(), role: None }),
                }
            }
        }
        out.push((title, nodes));
    }
    out
}

/// PdfCraft's own menus, with each command's id made ours (`cw.app:…`), the window's
/// shortcuts left off, and its app, Window and Help menus left to the window.
pub fn native_menus(theirs: &MenuBar) -> Vec<(String, MenuRole, Vec<Node>)> {
    fn map(n: &Node) -> Option<Node> {
        match n {
            Node::Item(i) => {
                if windows_native_item(&i.command.id) {
                    return None;
                }
                let mut it = i.clone();
                it.command = CommandRef::new(MenuCommand::AppMenu(i.command.key()).id());
                if it.shortcut.as_ref().is_some_and(is_window_chord) {
                    it.shortcut = None;
                }
                Some(Node::Item(it))
            }
            Node::Submenu { label, children, role, platforms } => {
                let children: Vec<Node> = children.iter().filter_map(map).collect();
                (!children.is_empty()).then(|| Node::Submenu { label: label.clone(), children, role: *role, platforms: *platforms })
            }
            other => Some(other.clone()),
        }
    }
    theirs
        .menus
        .iter()
        .filter(|m| !matches!(m.role, MenuRole::App | MenuRole::Window))
        .map(|m| (m.title.clone(), m.role, tidy(m.children.iter().filter_map(map).collect())))
        .collect()
}

/// PdfCraft items the window does itself.
fn windows_native_item(id: &str) -> bool {
    matches!(id, "file.open" | "file.close" | "file.open_recent" | "file.no_recent" | "app.quit" | "window.document") || id.starts_with("file.recent")
}

/// Drops leading, trailing and doubled separators.
fn tidy(nodes: Vec<Node>) -> Vec<Node> {
    let mut out: Vec<Node> = Vec::with_capacity(nodes.len());
    for n in nodes {
        if matches!(n, Node::Separator) && out.last().is_none_or(|l| matches!(l, Node::Separator)) {
            continue;
        }
        out.push(n);
    }
    while out.last().is_some_and(|l| matches!(l, Node::Separator)) {
        out.pop();
    }
    out
}

/// Redo's key in each app (⇧⌘Z in WordCraft and PdfCraft, ⌘Y in GridCraft and DeckCraft).
pub fn redo_keys(kind: Kind) -> &'static str {
    match kind {
        Kind::Document | Kind::Pdf => "⇧⌘Z",
        Kind::Sheet | Kind::Presentation => "⌘Y",
    }
}

/// The shown app's menus: from its catalogue, or its own menu bar.
pub enum AppMenus<'a> {
    None,
    Catalog(&'a [MenuEntry]),
    Native(MenuBar),
}

/// The whole menu bar for `s`, with the shown app's menus.
pub fn bar(s: &MenuState<'_>, app: AppMenus<'_>) -> MenuBar {
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
    let mut file = vec![
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
    ];
    let redo = s.shown.map_or("⇧⌘Z", redo_keys);
    let mut edit = vec![
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
    let mut view = vec![
        enabled(item(MenuCommand::NextTab, "Show Next Tab", Some("⌃Tab")), s.tabs.len() > 1),
        enabled(item(MenuCommand::PreviousTab, "Show Previous Tab", Some("⌃⇧Tab")), s.tabs.len() > 1),
        Node::Separator,
        item(MenuCommand::ToggleSidebar, "Show or Hide Sidebar", Some("⌃⌘S")),
        item(MenuCommand::SearchProject, "Search Project…", Some("⌃⇧F")),
        Node::Submenu {
            label: "Interface Text Size".into(),
            children: craftworks_shell::tokens::text::SCALES
                .iter()
                .map(|(p, name)| {
                    let label = if *p == craftworks_shell::tokens::text::scale() { format!("✓ {name}") } else { (*name).to_string() };
                    item(MenuCommand::TextScale(*p), &label, None)
                })
                .collect(),
            platforms: Default::default(),
            role: None,
        },
    ];
    let mut help: Vec<Node> = Vec::new();
    // The app's menus between View and Window; its File, Edit, View and Help items join ours.
    let mut middle: Vec<Menu> = Vec::new();
    let join = |into: &mut Vec<Node>, items: Vec<Node>| {
        if !items.is_empty() {
            into.push(Node::Separator);
            into.extend(items);
        }
    };
    match app {
        AppMenus::None => {}
        AppMenus::Catalog(entries) => {
            let mut saw_print = false;
            for (title, items) in catalog_menus(entries) {
                match title {
                    "File" => {
                        saw_print = true;
                        join(&mut file, items);
                    }
                    "Edit" => join(&mut edit, items),
                    "View" => join(&mut view, items),
                    t => middle.push(Menu::new(t, items)),
                }
            }
            if !saw_print {
                join(&mut file, vec![key_item("Save", "⌘S"), key_item("Save As…", "⇧⌘S"), Node::Separator, key_item("Print…", "⌘P")]);
            }
        }
        AppMenus::Native(theirs) => {
            for (title, role, items) in native_menus(&theirs) {
                match role {
                    MenuRole::File => join(&mut file, items),
                    // PdfCraft's Edit runs its own clipboard and undo: it replaces ours.
                    MenuRole::Edit => edit = items,
                    MenuRole::View => join(&mut view, items),
                    MenuRole::Help => help = items,
                    _ => middle.push(Menu::new(title, items)),
                }
            }
        }
    }
    let mut menus = vec![
        Menu::new("Craftworks", vec![Node::Item(Item::new(CommandRef::new(MenuCommand::Quit.id()), "Quit Craftworks").role(ItemRole::Quit))])
            .role(MenuRole::App),
        Menu::new("File", tidy(file)).role(MenuRole::File),
        Menu::new("Edit", tidy(edit)).role(MenuRole::Edit),
        Menu::new("View", tidy(view)).role(MenuRole::View),
    ];
    menus.extend(middle.into_iter().filter(|m| !m.children.is_empty()).map(|m| Menu::new(m.title.clone(), tidy(m.children))));
    menus.push(Menu::new("Window", vec![]).role(MenuRole::Window));
    menus.push(Menu::new("Help", tidy(help)).role(MenuRole::Help));
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
    use craftworks_core::keymaps::app_shortcuts;

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

    fn all_items(nodes: &[Node], out: &mut Vec<(String, Option<Chord>, String)>) {
        for n in nodes {
            match n {
                Node::Item(i) => out.push((i.command.id.clone(), i.shortcut, i.label.clone())),
                Node::Submenu { children, .. } => all_items(children, out),
                _ => {}
            }
        }
    }

    /// Each app's menus: every keystroke an item sends is one that app binds, no app item
    /// shows a window shortcut, and the usual Mac menus are there.
    #[test]
    fn each_apps_menus_use_its_own_shortcuts() {
        for kind in [Kind::Document, Kind::Sheet, Kind::Presentation] {
            let entries = craftworks_core::catalog::entries(kind);
            let theirs = app_shortcuts(kind);
            let s = MenuState { shown: Some(kind), tabs: vec![], recent: vec![], can_reopen: false };
            let b = bar(&s, AppMenus::Catalog(&entries));
            let titles: Vec<&str> = b.menus.iter().map(|m| m.title.as_str()).collect();
            for t in ["File", "Edit", "View", "Insert", "Format", "Tools", "Window"] {
                assert!(titles.contains(&t), "{kind:?}: no {t} menu in {titles:?}");
            }
            let mut items = Vec::new();
            for m in &b.menus {
                all_items(&m.children, &mut items);
            }
            let mut keyed = 0;
            for (id, shortcut, label) in &items {
                if let Some(MenuCommand::Key(k)) = MenuCommand::from_id(id) {
                    let c = Chord::parse(&k).unwrap_or_else(|e| panic!("{k}: {e:?}"));
                    let ours = ["⌘Z", "⌘A", "⌘F", "⌘Y", "⇧⌘Z", "⌘S", "⇧⌘S", "⌘P"].contains(&k.as_str());
                    assert!(ours || theirs.iter().any(|t| t.chord == to_app_chord(&c)), "{kind:?}: {label} sends {k}, which the app doesn't bind");
                    keyed += 1;
                }
                if let Some(c) = shortcut
                    && is_window_chord(c)
                {
                    let window_item = !id.starts_with("cw.key:") && !id.starts_with("cw.run:");
                    assert!(window_item, "{kind:?}: {label} shows the window's shortcut {c}");
                }
            }
            assert!(keyed > 20, "{kind:?}: only {keyed} items with shortcuts");
            let built = craft_menu::mac_layout(&b, &craft_menu::LayoutOptions::new("Craftworks"));
            assert!(built.bar.menus.len() >= 7);
        }
    }

    /// `cargo test -p craftworks menus_outline -- --nocapture --ignored` prints each app's menus.
    #[test]
    #[ignore]
    fn menus_outline() {
        fn show(nodes: &[Node], depth: usize, out: &mut String) {
            for n in nodes {
                match n {
                    Node::Item(i) => {
                        out.push_str(&format!("{}{}{}\n", "  ".repeat(depth), i.label, i.shortcut.map(|c| format!("  {c}")).unwrap_or_default()))
                    }
                    Node::Submenu { label, children, .. } => {
                        out.push_str(&format!("{}{label} ▸ ({})\n", "  ".repeat(depth), children.len()));
                    }
                    Node::Separator => out.push_str(&format!("{}—\n", "  ".repeat(depth))),
                    _ => {}
                }
            }
        }
        for kind in [Kind::Document, Kind::Sheet, Kind::Presentation] {
            let entries = craftworks_core::catalog::entries(kind);
            let s = MenuState { shown: Some(kind), tabs: vec![], recent: vec![], can_reopen: false };
            let b = bar(&s, AppMenus::Catalog(&entries));
            let mut out = format!("=== {kind:?}\n");
            for m in &b.menus {
                out.push_str(&format!("[{}] {} items\n", m.title, m.children.len()));
                if ["Format", "Edit", "Insert", "Arrange", "Slide Show"].contains(&m.title.as_str()) {
                    show(&m.children, 1, &mut out);
                }
            }
            println!("{out}");
        }
    }

    #[test]
    fn ribbon_tabs_land_in_mac_menus() {
        assert_eq!(mac_menu("Home"), Some(("Format", false)));
        assert_eq!(mac_menu("Picture Format"), Some(("Format", true)));
        assert_eq!(mac_menu("Table Layout"), Some(("Table", false)));
        assert_eq!(mac_menu("Quick Access Toolbar"), None);
        assert_eq!(mac_menu("Navigation"), None, "caret keys aren't menu items");
        assert_eq!(mac_menu("Something New"), Some(("Tools", true)));
        assert!(windows_own("file.open", "Open…") && windows_own("x", "Paste") && !windows_own("format.bold", "Bold"));
    }

    #[test]
    fn keys_become_press_and_release() {
        let e = key_events("⇧⌘S");
        assert_eq!(e.len(), 2);
        assert!(matches!(e[0], egui::Event::Key { key: egui::Key::S, pressed: true, modifiers, .. } if modifiers.command && modifiers.shift));
    }
}
