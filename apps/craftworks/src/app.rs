//! The Craftworks window: our chrome (title row with tabs, new-tab popover, start page,
//! toasts) around the four Craft engines, each in an egui context of its own (D1), painted
//! into a texture by `craftworks-render` and shown below the title row.

use crate::menu::{self, MenuCommand};
use crate::{check, projects, session, settings, titlebar::TitleBar};
use craftworks_core::{CloseState, DocId, Kind, Opened, TabId, Tabs};
use craftworks_render::{EngineHost, FrameResult, embed};
use craftworks_shell::keys::{self, Route};
use craftworks_shell::{
    NewTab, NewTabAction, Palette, Recent, RecentView, Sidebar, SidebarAction, SidebarView, TabAction, TabItem, TabStripState, Toasts, sidebar,
    tab_strip, tokens,
};
use egui::{Event, Rect, Stroke, pos2};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// How often an engine that isn't shown runs `logic` while it has work pending (D4).
const BACKGROUND_TICK: Duration = Duration::from_millis(250);
/// …and when it says nothing: a slow heartbeat, so an idle window stays idle.
const BACKGROUND_IDLE: Duration = Duration::from_secs(5);
/// Ids of tabs still opening (no engine document yet) count down from here.
const PENDING_ID: u64 = u64::MAX;

/// A close the app asked an engine for, waiting for the engine's answer.
struct Closing {
    tab: TabId,
    kind: Kind,
    doc: DocId,
    frames: u32,
}

/// What a frame of the shown engine reported (for `--check`).
#[derive(Clone, Copy, Debug, Default)]
pub struct ShownFrame {
    pub kind: Option<Kind>,
    /// The first frame of this engine after another one was shown.
    pub switched: bool,
    pub font_atlas_rebuilt: bool,
    pub cpu: Duration,
}

pub struct App {
    render: egui_wgpu::RenderState,
    /// Clipboard, file panels, printing: the system's, or isolated under `--check`.
    pub platform: craftworks_platform::Platform,
    engines: BTreeMap<Kind, EngineHost>,
    textures: HashMap<Kind, (egui::TextureId, [u32; 2])>,
    pub tabs: Tabs,
    known: HashMap<Kind, HashSet<DocId>>,
    pending_opens: Vec<(Kind, PathBuf)>,
    closing: Vec<Closing>,
    last_shown: Option<Kind>,
    drawn: HashSet<Kind>,
    pub last_frame: ShownFrame,
    /// Events the check adds to the shown engine's next input (engine coordinates).
    pub inject: Vec<Event>,
    /// Keyboard events taken out of the window's input for the shown document (keys::route).
    document_keys: Vec<Event>,
    /// Window shortcuts pressed since the last frame.
    shortcut_actions: Vec<keys::Action>,
    /// Documents dropped on the window outside the tab strip, to open in tabs.
    dropped_to_open: Vec<PathBuf>,
    /// The native menu bar (macOS), and the state it was last built for.
    native_menu: Option<craft_menu::NativeMenu>,
    menu_state: u64,
    /// Quitting: each tab with unsaved changes is asked about in turn (the app's own prompt).
    pub quitting: Option<HashSet<TabId>>,
    /// The quit was decided: the next close goes through.
    pub quit_confirmed: bool,
    /// The open tabs as last saved to `session.json`.
    saved_session: Option<session::Session>,
    /// After a restore: the file whose tab comes to the front once it's open.
    restore_active: Option<PathBuf>,
    #[cfg(target_os = "macos")]
    apple: Option<fmv_macos_events::Inbox>,
    /// `--check`: whether the window counts as focused (a user looking at it).
    pub check_focused: bool,
    /// Light or dark: forced (`--appearance`), else the system's.
    forced_dark: Option<bool>,
    dark: Option<bool>,
    pub palette: Palette,
    titlebar: TitleBar,
    titlebar_measured: bool,
    strip: TabStripState,
    pub newtab: NewTab,
    pub toasts: Toasts,
    recent: Recent,
    /// The project sidebar, and the projects behind it.
    pub sidebar: Sidebar,
    pub projects: projects::Projects,
    /// Each app's commands for its menus, read once (craftworks_core::catalog).
    catalogs: HashMap<Kind, Vec<craftworks_core::catalog::MenuEntry>>,
    /// Projects being closed from their group's menu, waiting for their tabs to close.
    closing_projects: Vec<PathBuf>,
    /// The front tab (and its group) when groups were last synced.
    last_front: Option<(TabId, Option<craftworks_core::GroupId>)>,
    /// Files of recently closed tabs, for reopening.
    closed: Vec<PathBuf>,
    /// The engine area last frame (for the check's resize timing and toasts).
    pub content_rect: Rect,
    /// The accessibility graft shared with the window's plugin.
    pub a11y: std::sync::Arc<std::sync::Mutex<craftworks_render::a11y::Shared>>,
    /// Frames drawn, and why the last one asked for another (for the check's idle report).
    pub frames: u64,
    pub wake_reasons: Vec<String>,
    check: Option<check::Check>,
}

impl App {
    pub fn new(render: egui_wgpu::RenderState, ctx: &egui::Context, forced_dark: Option<bool>, platform: craftworks_platform::Platform) -> Self {
        ctx.set_fonts(tokens::fonts());
        let palette = Palette::for_dark(forced_dark.unwrap_or(false));
        tokens::apply(ctx, &palette);
        let a11y = std::sync::Arc::new(std::sync::Mutex::new(craftworks_render::a11y::Shared::default()));
        ctx.add_plugin(craftworks_render::a11y::Graft::new(a11y.clone()));
        App {
            render,
            platform,
            engines: BTreeMap::new(),
            textures: HashMap::new(),
            tabs: Tabs::new(),
            known: HashMap::new(),
            pending_opens: Vec::new(),
            closing: Vec::new(),
            last_shown: None,
            drawn: HashSet::new(),
            last_frame: ShownFrame::default(),
            inject: Vec::new(),
            document_keys: Vec::new(),
            shortcut_actions: Vec::new(),
            dropped_to_open: Vec::new(),
            native_menu: None,
            menu_state: 0,
            quitting: None,
            quit_confirmed: false,
            saved_session: None,
            restore_active: None,
            #[cfg(target_os = "macos")]
            apple: None,
            check_focused: true,
            forced_dark,
            dark: None,
            palette,
            titlebar: TitleBar::default(),
            titlebar_measured: false,
            strip: TabStripState::default(),
            newtab: NewTab::default(),
            toasts: Toasts::default(),
            recent: settings::load_recent(),
            sidebar: Sidebar::default(),
            projects: projects::Projects::default(),
            catalogs: HashMap::new(),
            closing_projects: Vec::new(),
            last_front: None,
            closed: Vec::new(),
            content_rect: Rect::NOTHING,
            a11y,
            frames: 0,
            wake_reasons: Vec::new(),
            check: None,
        }
    }

    /// Shows the project the window had at quit, and the sidebar if it was open.
    pub fn restore_project(&mut self, ctx: &egui::Context) {
        let (projects, restore) = projects::Projects::load();
        self.projects = projects;
        self.sidebar.open = self.projects.sidebar_open;
        for root in restore.open.iter().filter(|r| craftworks_core::project::Project::exists(r)) {
            if let Err(e) = self.projects.open(root, None, ctx) {
                self.toasts.error(format!("Couldn't open the project: {e}"));
            }
        }
        if let Some(c) = &restore.current {
            self.projects.select_root(c);
        }
        self.sync_groups();
    }

    /// Keeps the tab groups in step with the open projects (one group each, its name and colour
    /// from `project.json`), and each tab in the group of the project that holds its file. The
    /// sidebar follows the front tab's project.
    fn sync_groups(&mut self) {
        let open: Vec<(String, String, Option<String>, PathBuf)> = self
            .projects
            .open
            .iter()
            .map(|o| {
                (
                    o.project.root.to_string_lossy().into_owned(),
                    o.project.settings.name.clone(),
                    o.project.settings.color.clone(),
                    o.project.root.clone(),
                )
            })
            .collect();
        let stale: Vec<craftworks_core::GroupId> = self.tabs.groups().filter(|g| !open.iter().any(|o| o.0 == g.key)).map(|g| g.id).collect();
        for g in stale {
            self.tabs.remove_group(g);
        }
        for (n, (key, name, color, root)) in open.iter().enumerate() {
            let color = color
                .as_deref()
                .and_then(|c| tokens::GROUP_NAMES.iter().position(|g| g.eq_ignore_ascii_case(c)))
                .map_or((n % (tokens::GROUP_NAMES.len() - 1) + 1) as u8, |i| i as u8);
            let fresh = self.tabs.group_by_key(key).is_none();
            let id = self.tabs.add_group(key, name, color);
            if let Some(g) = self.tabs.group_mut(id) {
                g.name.clone_from(name);
                g.color = color;
            }
            if fresh && self.projects.collapsed.contains(root) {
                let _ = self.tabs.set_collapsed(id, true);
            }
        }
        let tabs: Vec<(TabId, Option<craftworks_core::GroupId>, Kind, DocId)> = self.tabs.iter().map(|t| (t.id, t.group, t.kind, t.doc)).collect();
        for (id, group, kind, doc) in tabs {
            let path = self.engine_ref(kind).and_then(|e| e.editor().documents().into_iter().find(|d| d.id == doc)).and_then(|d| d.path);
            let want = path.as_deref().and_then(|p| self.projects.owner_of(p)).and_then(|p| self.tabs.group_by_key(&p.root.to_string_lossy()));
            if want != group {
                self.tabs.set_group(id, want);
            }
        }
        // When another tab comes to the front, the sidebar shows its project (and stays on
        // what was picked in it otherwise).
        let front_tab = self.tabs.active().map(|t| (t.id, t.group));
        if front_tab != self.last_front {
            self.last_front = front_tab;
            let front = front_tab.and_then(|(_, g)| g).and_then(|g| self.tabs.group(g)).map(|g| PathBuf::from(&g.key));
            if let Some(root) = front {
                self.projects.select_root(&root);
            }
        }
        // Projects whose close was asked for: closed once their tabs are gone; kept if a tab's
        // close was cancelled.
        for root in std::mem::take(&mut self.closing_projects) {
            let g = self.tabs.group_by_key(&root.to_string_lossy());
            let left = g.map(|g| self.tabs.in_group(g)).unwrap_or_default();
            if left.is_empty() {
                self.projects.close(&root);
                if let Some(g) = g {
                    self.tabs.remove_group(g);
                }
            } else if left.iter().any(|t| self.closing.iter().any(|c| c.tab == *t)) {
                self.closing_projects.push(root);
            }
        }
    }

    /// The group items for the strip.
    fn group_items(&self) -> Vec<craftworks_shell::GroupItem> {
        self.tabs.groups().map(|g| craftworks_shell::GroupItem { id: g.id.0, name: g.name.clone(), color: g.color, collapsed: g.collapsed }).collect()
    }

    fn group_root(&self, id: u64) -> Option<PathBuf> {
        self.tabs.group(craftworks_core::GroupId(id)).map(|g| PathBuf::from(&g.key))
    }

    /// Makes `root` a project and shows it in the sidebar.
    pub fn open_project(&mut self, root: &Path, name: Option<&str>, ctx: &egui::Context) {
        match self.projects.open(root, name, ctx) {
            Ok(()) => {
                self.sidebar.open = true;
                self.sidebar.query.clear();
                self.projects.query.clear();
                self.save_sidebar_state();
            }
            Err(e) => {
                self.toasts.error(format!("Couldn't open the project: {e}"));
            }
        }
    }

    fn save_sidebar_state(&mut self) {
        if self.projects.sidebar_open != self.sidebar.open {
            self.projects.sidebar_open = self.sidebar.open;
            self.projects.save();
        }
    }

    /// The file of the tab in front, if it has one.
    pub fn active_path(&self) -> Option<PathBuf> {
        let t = self.tabs.active()?;
        self.engine_ref(t.kind)?.editor().documents().into_iter().find(|d| d.id == t.doc)?.path
    }

    /// The tab that shows `path`, if any.
    pub fn tab_for_path(&self, path: &Path) -> Option<TabId> {
        self.tabs
            .iter()
            .find(|t| {
                self.engine_ref(t.kind)
                    .and_then(|e| e.editor().documents().into_iter().find(|d| d.id == t.doc))
                    .and_then(|d| d.path)
                    .is_some_and(|p| p == path)
            })
            .map(|t| t.id)
    }

    /// Whether a tab shows `path` (or a file inside it, for a folder).
    fn is_open(&self, path: &Path) -> bool {
        self.tabs.iter().any(|t| {
            self.engine_ref(t.kind)
                .and_then(|e| e.editor().documents().into_iter().find(|d| d.id == t.doc))
                .and_then(|d| d.path)
                .is_some_and(|p| p.starts_with(path))
        })
    }

    /// Does what the sidebar asked.
    pub fn on_sidebar_action(&mut self, ctx: &egui::Context, a: SidebarAction) {
        match a {
            SidebarAction::Open(p) => self.open(&p),
            SidebarAction::OpenHit(hit) => {
                let query = self.projects.query.clone();
                self.open(&hit.path);
                self.projects.goto = Some(projects::GoTo::new(&hit, &query));
            }
            SidebarAction::Search(q) => self.projects.query = q,
            SidebarAction::NewFile { dir, kind } => self.new_file_in(&dir, kind),
            SidebarAction::Rename { from, to_name } => self.rename(&from, &to_name),
            SidebarAction::Reveal(p) => craftworks_platform::files::reveal(&p),
            SidebarAction::TogglePin(p) => {
                if let Err(e) = self.projects.toggle_pin(&p) {
                    self.toasts.error(format!("Couldn't pin {}: {e}", file_name(&p)));
                }
            }
            SidebarAction::SwitchProject(root) => self.open_project(&root, None, ctx),
            SidebarAction::NewProject => {
                if !self.platform.interactive {
                    return;
                }
                if let Some(path) = rfd::FileDialog::new().set_title("New Project").set_file_name("New Project").save_file() {
                    match std::fs::create_dir_all(&path) {
                        Ok(()) => {
                            let name = path.file_name().map(|n| n.to_string_lossy().into_owned());
                            self.open_project(&path, name.as_deref(), ctx);
                        }
                        Err(e) => {
                            self.toasts.error(format!("Couldn't make the project folder: {e}"));
                        }
                    }
                }
            }
            SidebarAction::OpenFolder => {
                if !self.platform.interactive {
                    return;
                }
                if let Some(dir) = rfd::FileDialog::new().set_title("Open Folder as Project").pick_folder() {
                    self.open_project(&dir, None, ctx);
                }
            }
        }
    }

    /// A new, empty file of `kind` in `dir` ("Untitled Sheet 2.xlsx"…), open in a tab.
    pub fn new_file_in(&mut self, dir: &Path, kind: Kind) {
        let path = sidebar::new_file_name(dir, kind, |p| p.exists());
        let made = self.engine(kind).editor_mut().new_document();
        match made {
            Ok(doc) => {
                self.tabs.open(kind, doc);
                self.known.entry(kind).or_default().insert(doc);
                if let Err(e) = self.engine(kind).editor_mut().save_as(doc, &path) {
                    self.toasts.error(format!("Couldn't save {}: {e}", file_name(&path)));
                }
            }
            Err(e) => {
                self.toasts.error(format!("Couldn't make a new {}: {e}", kind.label().to_lowercase()));
            }
        }
    }

    /// Renames a file or folder in the project. Not while a tab shows it (the app holds it).
    pub fn rename(&mut self, from: &Path, to_name: &str) {
        if !sidebar::valid_name(to_name) {
            self.toasts.error(format!("“{to_name}” can't be a file name"));
            return;
        }
        let Some(to) = from.parent().map(|d| d.join(to_name.trim())) else { return };
        if self.is_open(from) {
            self.toasts.info(format!("Close {} first: it's open in a tab", file_name(from)));
            return;
        }
        if to.exists() {
            self.toasts.error(format!("{} already exists", file_name(&to)));
            return;
        }
        match std::fs::rename(from, &to) {
            Ok(()) => self.projects.renamed(from, &to),
            Err(e) => {
                self.toasts.error(format!("Couldn't rename {}: {e}", file_name(from)));
            }
        }
    }

    /// Shows a search hit once its file's tab is in front and the document is ready.
    fn step_goto(&mut self) {
        let Some(mut g) = self.projects.goto.take() else { return };
        let ready = self.active_path().is_some_and(|p| p == g.path) && self.pending_opens.is_empty();
        if ready {
            if let Some(kind) = Kind::for_path(&g.path)
                && let Some((cmd, args)) = projects::goto_command(kind, &g.location, &g.query)
                && let Err(e) = self.engine(kind).editor_mut().run_command(cmd, args)
            {
                log::info!("showing the search hit: {e}");
            }
            return;
        }
        if g.tick() {
            self.projects.goto = Some(g);
        }
    }

    pub fn with_check(mut self, c: check::Check) -> Self {
        self.check = Some(c);
        self
    }

    #[cfg(target_os = "macos")]
    pub fn set_apple_events(&mut self, inbox: fmv_macos_events::Inbox) {
        self.apple = Some(inbox);
    }

    /// Reopens the tabs open at the last quit. Files that are gone are reported, not fatal.
    pub fn restore_session(&mut self) {
        let s = session::load();
        let (here, gone) = s.split_missing(|p| p.exists());
        self.restore_active = s.active.and_then(|i| s.files.get(i)).filter(|p| p.exists()).cloned();
        for p in &here {
            self.open(p);
        }
        if !gone.is_empty() {
            let names: Vec<String> = gone.iter().map(|p| file_name(p)).collect();
            self.toasts.error(format!(
                "Couldn't find {} from last time: {}",
                if gone.len() == 1 { "a file" } else { "some files" },
                names.join(", ")
            ));
        } else if !here.is_empty() {
            self.toasts.info(format!("Restored {} tab{}", here.len(), if here.len() == 1 { "" } else { "s" }));
        }
    }

    /// The open tabs as the session file keeps them (tabs with a file, in strip order).
    fn current_session(&self) -> session::Session {
        let active = self.tabs.active().map(|t| t.id);
        let mut s = session::Session::default();
        for t in self.tabs.iter() {
            let path = self.engines.get(&t.kind).and_then(|e| e.editor().documents().into_iter().find(|d| d.id == t.doc)).and_then(|d| d.path);
            if let Some(p) = path {
                if Some(t.id) == active {
                    s.active = Some(s.files.len());
                }
                s.files.push(p);
            }
        }
        for (_, p) in &self.pending_opens {
            s.files.push(p.clone());
        }
        s
    }

    /// Saves the open tabs when they changed, but not once a quit is under way or decided:
    /// tabs close as it goes, and the ones open when it started are what comes back.
    fn save_session_if_changed(&mut self) {
        if self.quitting.is_some() || self.quit_confirmed {
            return;
        }
        let now = self.current_session();
        if self.saved_session.as_ref() != Some(&now) {
            session::save(&now);
            self.saved_session = Some(now);
        }
    }

    /// Quit (⌘Q, the Dock, logging out, the window's close button): the open tabs are kept for
    /// next time, then each tab with unsaved changes is asked about with its app's own prompt.
    /// Cancel on any of them stops the quit.
    pub fn start_quit(&mut self) {
        if self.quitting.is_none() {
            let s = self.current_session();
            session::save(&s);
            self.saved_session = Some(s);
            self.quitting = Some(HashSet::new());
        }
    }

    /// One step of the quit: ask the next unsaved tab, or, with none left, quit.
    fn step_quit(&mut self) {
        let Some(asked) = self.quitting.as_mut() else { return };
        if !self.closing.is_empty() {
            return;
        }
        let next = self.tabs.iter().find(|t| {
            !asked.contains(&t.id) && self.engines.get(&t.kind).is_some_and(|e| e.editor().documents().iter().any(|d| d.id == t.doc && d.dirty))
        });
        match next.map(|t| t.id) {
            Some(id) => {
                asked.insert(id);
                self.close_tab(id);
            }
            None => {
                self.quitting = None;
                self.quit_confirmed = true;
                if self.check.is_none() {
                    // AppKit's own terminate: closing the window first can crash on macOS
                    // (craft-quit's notes).
                    craft_quit::quit();
                }
            }
        }
    }

    /// Runs a menu command (a click, or a key equivalent the window doesn't otherwise know).
    pub fn on_menu(&mut self, ctx: &egui::Context, id: &str) {
        let Some(cmd) = MenuCommand::from_id(id) else {
            log::warn!("unknown menu command {id}");
            return;
        };
        match cmd {
            MenuCommand::NewTab => self.newtab.toggle(),
            MenuCommand::New(Kind::Pdf) => self.choose_file(true),
            MenuCommand::New(kind) => self.new_document(kind),
            MenuCommand::Open => self.choose_file(false),
            MenuCommand::OpenRecent(p) => self.open(&p),
            MenuCommand::CloseTab => self.shortcut_actions.push(keys::Action::CloseTab),
            MenuCommand::ReopenClosedTab => self.shortcut_actions.push(keys::Action::ReopenClosedTab),
            MenuCommand::NextTab => self.shortcut_actions.push(keys::Action::NextTab),
            MenuCommand::PreviousTab => self.shortcut_actions.push(keys::Action::PreviousTab),
            MenuCommand::ShowTab(id) => {
                self.tabs.activate(TabId(id));
            }
            MenuCommand::Quit => self.start_quit(),
            MenuCommand::Key(k) => self.document_keys.extend(menu::key_events(&k)),
            MenuCommand::Copy => self.document_keys.push(Event::Copy),
            MenuCommand::Cut => self.document_keys.push(Event::Cut),
            // eframe answers with a Paste event of the clipboard's text next frame, which the
            // input hook routes to the document.
            MenuCommand::Paste => ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaste),
            MenuCommand::AppMenu(key) => {
                if let Some(kind) = self.tabs.active().map(|t| t.kind) {
                    let host = self.engine(kind);
                    let ectx = host.context().clone();
                    if !host.editor_mut().run_native_menu(&ectx, &key) {
                        log::warn!("{kind:?} has no menu item {key}");
                    }
                }
            }
            MenuCommand::ToggleSidebar => self.sidebar.toggle(),
            MenuCommand::SearchProject => self.sidebar.focus_search(),
            MenuCommand::Run(id) => {
                if let Some(kind) = self.tabs.active().map(|t| t.kind)
                    && let Err(e) = self.engine(kind).editor_mut().run_command(&id, serde_json::json!({}))
                {
                    self.toasts.error(format!("{id}: {e}"));
                }
            }
        }
    }

    pub fn has_native_menu(&self) -> bool {
        self.native_menu.is_some()
    }

    /// Installs the native menu bar (macOS); without it (other platforms) there's none yet.
    fn install_menu(&mut self, ctx: &egui::Context) {
        #[cfg(target_os = "macos")]
        {
            let layout = craft_menu::mac_layout(&self.menu_bar(), &craft_menu::LayoutOptions::new("Craftworks"));
            self.native_menu = craft_menu::mac::install_with(ctx, &layout.bar, craft_menu::mac::MacOptions::default().window_list(false));
            if self.native_menu.is_none() {
                log::warn!("AppKit refused the menu bar");
            }
        }
        #[cfg(not(target_os = "macos"))]
        let _ = ctx;
    }

    pub fn menu_bar(&mut self) -> craft_menu::MenuBar {
        let shown = self.tabs.active().map(|t| t.kind);
        if let Some(k) = shown.filter(|k| *k != Kind::Pdf) {
            self.catalogs.entry(k).or_insert_with(|| craftworks_core::catalog::entries(k));
        }
        let native = shown.filter(|k| *k == Kind::Pdf).and_then(|k| self.engine_ref(k)).and_then(|h| h.editor().native_menus(h.context()));
        let items = self.tab_items();
        let active = self.tabs.active().map(|t| t.id.0);
        let state = menu::MenuState {
            shown: self.tabs.active().map(|t| t.kind),
            tabs: items.iter().map(|t| menu::TabEntry { id: t.id, label: t.label.clone(), active: Some(t.id) == active }).collect(),
            recent: self.recent.files().iter().map(|f| f.path.as_path()).collect(),
            can_reopen: !self.closed.is_empty(),
        };
        let app = match (shown, native) {
            (Some(_), Some(bar)) => menu::AppMenus::Native(bar),
            (Some(k), None) => self.catalogs.get(&k).map_or(menu::AppMenus::None, |e| menu::AppMenus::Catalog(e)),
            (None, _) => menu::AppMenus::None,
        };
        menu::bar(&state, app)
    }

    /// Keeps the menu bar in step (the shown kind, the tabs, recent files), and runs its clicks.
    fn menu_frame(&mut self, ctx: &egui::Context) {
        if self.native_menu.is_none() {
            return;
        }
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.tabs.active().map(|t| (t.kind, t.id)).hash(&mut h);
        for t in self.tab_items() {
            (t.id, t.label).hash(&mut h);
        }
        self.recent.files().len().hash(&mut h);
        self.recent.files().first().map(|f| f.path.clone()).hash(&mut h);
        self.closed.is_empty().hash(&mut h);
        let state = h.finish();
        let bar = (state != self.menu_state).then(|| self.menu_bar());
        let modal = self.tabs.active().and_then(|t| self.engines.get(&t.kind)).is_some_and(|e| e.editor().close_prompt_open());
        let chrome = self.chrome_has_keyboard(ctx);
        let Some(native) = self.native_menu.as_mut() else { return };
        if let Some(bar) = bar {
            let clashes = native.sync(craft_menu::mac_layout(&bar, &craft_menu::LayoutOptions::new("Craftworks")));
            for c in clashes {
                log::debug!("menu shortcut clash: {c:?}");
            }
            self.menu_state = state;
        }
        native.set_gate(craft_menu::Gate { modal, text_editing: chrome, ..Default::default() });
        let clicks = native.take_clicks(ctx);
        for c in clicks {
            self.on_menu(ctx, &c.id);
        }
    }

    /// The engine for `kind`, made on first use and given the window's appearance.
    pub fn engine(&mut self, kind: Kind) -> &mut EngineHost {
        let (device, format, dark) = (&self.render.device, self.render.target_format, self.dark.unwrap_or(false));
        let platform = &self.platform;
        self.engines.entry(kind).or_insert_with(|| {
            let mut host = EngineHost::new(device, format, craftworks_platform::editor(kind, platform));
            host.editor_mut().set_dark(dark);
            host
        })
    }

    pub fn engine_ref(&self, kind: Kind) -> Option<&EngineHost> {
        self.engines.get(&kind)
    }

    pub fn is_dark(&self) -> bool {
        self.dark.unwrap_or(false)
    }

    pub fn new_document(&mut self, kind: Kind) {
        match self.engine(kind).editor_mut().new_document() {
            Ok(doc) => {
                self.tabs.open(kind, doc);
                self.known.entry(kind).or_default().insert(doc);
            }
            Err(e) => {
                self.toasts.error(format!("Couldn't make a new {}: {e}", kind.label().to_lowercase()));
            }
        }
    }

    pub fn open(&mut self, path: &Path) {
        let Some(kind) = Kind::for_path(path) else {
            let ext = path.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_else(|| "this file".into());
            self.toasts.error(format!("Craftworks can't open {} files", ext));
            return;
        };
        // Already open: its tab comes to the front (DeckCraft would open a second copy).
        if let Some(id) = self.tab_for_path(path) {
            self.tabs.activate(id);
            self.recent.opened(path, chrono::Local::now().timestamp());
            settings::save_recent(&self.recent);
            return;
        }
        if self.pending_opens.iter().any(|(_, p)| p == path) {
            return;
        }
        match self.engine(kind).editor_mut().open(path) {
            Ok(Opened::Now(doc)) => {
                self.tabs.open(kind, doc);
                self.known.entry(kind).or_default().insert(doc);
            }
            Ok(Opened::Later) => self.pending_opens.push((kind, path.to_path_buf())),
            Err(e) => {
                self.toasts.error(format!("Couldn't open {}: {e}", file_name(path)));
                return;
            }
        }
        self.recent.opened(path, chrono::Local::now().timestamp());
        settings::save_recent(&self.recent);
    }

    /// Starts closing tab `id` through its app's own close (which may ask to save).
    pub fn close_tab(&mut self, id: TabId) {
        let Some(tab) = self.tabs.get(id).copied() else { return };
        if self.closing.iter().any(|c| c.tab == id) {
            return;
        }
        self.tabs.activate(id);
        if let Some(path) = self.engine_ref(tab.kind).and_then(|e| e.editor().documents().into_iter().find(|d| d.id == tab.doc)).and_then(|d| d.path)
        {
            self.closed.push(path);
        }
        match self.engine(tab.kind).editor_mut().request_close(tab.doc) {
            Ok(()) => self.closing.push(Closing { tab: id, kind: tab.kind, doc: tab.doc, frames: 0 }),
            Err(e) => {
                self.toasts.error(format!("Couldn't close the tab: {e}"));
            }
        }
    }

    fn choose_file(&mut self, pdf_only: bool) {
        let dialog = rfd::FileDialog::new();
        let dialog = if pdf_only {
            dialog.add_filter("PDF", &["pdf"])
        } else {
            dialog.add_filter(
                "Documents, sheets, presentations and PDFs",
                &["docx", "doc", "odt", "rtf", "html", "htm", "md", "markdown", "tex", "txt", "xlsx", "csv", "tsv", "pptx", "pdf"],
            )
        };
        if let Some(paths) = dialog.pick_files() {
            for p in paths {
                self.open(&p);
            }
        }
    }

    /// Keeps the tabs and the engines in step: closes that finished, files that finished
    /// opening, documents an app closed or opened by itself, and the active tab's document.
    fn sync(&mut self) {
        let mut closed_now: HashSet<Kind> = HashSet::new();
        let mut still = Vec::new();
        for mut c in std::mem::take(&mut self.closing) {
            c.frames = c.frames.saturating_add(1);
            let state = self.engines.get(&c.kind).map_or(CloseState::Closed, |e| e.editor().close_state(c.doc));
            match (c.frames >= 2, state) {
                (true, CloseState::Closed) => {
                    self.tabs.remove(c.tab);
                    closed_now.insert(c.kind);
                }
                (true, CloseState::Cancelled) => {
                    self.closed.pop();
                    if self.quitting.take().is_some() {
                        self.toasts.info("Quit cancelled");
                    }
                }
                _ => still.push(c),
            }
        }
        self.closing = still;

        let mut opened = Vec::new();
        self.pending_opens.retain(|(kind, path)| match self.engines.get(kind).and_then(|e| e.editor().find_path(path)) {
            Some(doc) => {
                opened.push((*kind, doc));
                false
            }
            None => true,
        });
        for (kind, doc) in opened {
            self.tabs.open(kind, doc);
        }
        if let Some(want) = self.restore_active.clone() {
            let found =
                self.tabs.iter().find(|t| self.engines.get(&t.kind).is_some_and(|e| e.editor().find_path(&want) == Some(t.doc))).map(|t| t.id);
            if let Some(id) = found {
                self.tabs.activate(id);
                self.restore_active = None;
            } else if self.pending_opens.is_empty() {
                self.restore_active = None;
            }
        }

        let shown_kind = self.tabs.active().map(|t| t.kind);
        for (kind, host) in &self.engines {
            let ed = host.editor();
            let ids: HashSet<DocId> = ed.documents().iter().map(|d| d.id).collect();
            let gone: Vec<TabId> = self
                .tabs
                .iter()
                .filter(|t| t.kind == *kind && !ids.contains(&t.doc) && !self.closing.iter().any(|c| c.tab == t.id))
                .map(|t| t.id)
                .collect();
            for t in gone {
                self.tabs.remove(t);
            }
            let known = self.known.entry(*kind).or_default();
            if shown_kind == Some(*kind)
                && !closed_now.contains(kind)
                && let Some(active) = ed.active()
                && ids.contains(&active)
                && !known.contains(&active)
            {
                self.tabs.open(*kind, active);
            }
            *known = ids;
        }

        if let Some(tab) = self.tabs.active().copied()
            && let Some(host) = self.engines.get_mut(&tab.kind)
            && host.editor().active() != Some(tab.doc)
            && host.editor().documents().iter().any(|d| d.id == tab.doc)
            && let Err(e) = host.editor_mut().activate(tab.doc)
        {
            log::warn!("switching to {:?} {:?}: {e}", tab.kind, tab.doc);
        }
    }

    /// The window follows the system's light or dark mode, and so does every engine: each is
    /// told the mode explicitly, and its input reports it as the system's (see `engine_input`),
    /// so whatever in an app follows "System" (egui's own visuals included) matches too.
    fn sync_appearance(&mut self, ctx: &egui::Context) {
        let system = ctx.input(|i| i.raw.system_theme).map(|t| t == egui::Theme::Dark);
        let dark = self.forced_dark.or(system).unwrap_or(false);
        if self.dark == Some(dark) {
            return;
        }
        self.dark = Some(dark);
        self.palette = Palette::for_dark(dark);
        tokens::apply(ctx, &self.palette);
        for host in self.engines.values_mut() {
            host.editor_mut().set_dark(dark);
        }
    }

    fn tab_items(&self) -> Vec<TabItem> {
        let mut items: Vec<TabItem> = self
            .tabs
            .iter()
            .map(|t| {
                let info = self.engines.get(&t.kind).and_then(|e| e.editor().documents().into_iter().find(|d| d.id == t.doc));
                let label = info
                    .as_ref()
                    .and_then(|d| d.path.as_deref().map(file_name))
                    .unwrap_or_else(|| info.as_ref().map_or_else(String::new, |d| d.title.clone()));
                TabItem {
                    id: t.id.0,
                    kind: t.kind,
                    label,
                    path: info.as_ref().and_then(|d| d.path.clone()),
                    dirty: info.is_some_and(|d| d.dirty),
                    busy: false,
                    group: t.group.map(|g| g.0),
                }
            })
            .collect();
        for (i, (kind, path)) in self.pending_opens.iter().enumerate() {
            items.push(TabItem {
                id: PENDING_ID - i as u64,
                kind: *kind,
                label: file_name(path),
                path: Some(path.clone()),
                dirty: false,
                busy: true,
                group: None,
            });
        }
        items
    }

    pub fn on_tab_action(&mut self, ctx: &egui::Context, a: TabAction) {
        let tab = |id: u64| TabId(id);
        match a {
            TabAction::Activate(id) => {
                self.tabs.activate(tab(id));
            }
            TabAction::Close(id) if id > PENDING_ID - 1024 => {
                let i = (PENDING_ID - id) as usize;
                if i < self.pending_opens.len() {
                    self.pending_opens.remove(i);
                }
            }
            TabAction::Close(id) => self.close_tab(tab(id)),
            TabAction::CloseOthers(id) => {
                let others: Vec<TabId> = self.tabs.iter().filter(|t| t.id != tab(id)).map(|t| t.id).collect();
                others.into_iter().for_each(|t| self.close_tab(t));
            }
            TabAction::CloseToRight(id) => {
                let right: Vec<TabId> = self.tabs.iter().skip_while(|t| t.id != tab(id)).skip(1).map(|t| t.id).collect();
                right.into_iter().for_each(|t| self.close_tab(t));
            }
            TabAction::Reveal(id) | TabAction::CopyPath(id) => {
                let path = self.tab_items().into_iter().find(|t| t.id == id).and_then(|t| t.path);
                if let Some(path) = path {
                    if matches!(a, TabAction::Reveal(_)) {
                        if let Err(e) = std::process::Command::new("open").arg("-R").arg(&path).spawn() {
                            self.toasts.error(format!("Couldn't show it in Finder: {e}"));
                        }
                    } else {
                        ctx.copy_text(path.display().to_string());
                        self.toasts.info("Path copied");
                    }
                }
            }
            TabAction::Move { id, to } => {
                self.tabs.move_to(tab(id), to);
            }
            TabAction::NewTab => self.newtab.toggle(),
            TabAction::Drop { at, paths } => {
                for p in &paths {
                    self.open(p);
                    if let Some(t) = self.tabs.active().map(|t| t.id) {
                        self.tabs.move_to(t, at);
                    }
                }
            }
            TabAction::DragWindow => ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag),
            TabAction::ZoomWindow => {
                let max = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
                ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!max));
            }
            TabAction::ToggleGroup(id) => self.toggle_group(id),
            TabAction::RenameGroup { id, name } => {
                if let Some(root) = self.group_root(id)
                    && let Err(e) = self.projects.set_name(&root, &name)
                {
                    self.toasts.error(format!("Couldn't rename the project: {e}"));
                }
            }
            TabAction::SetGroupColor { id, color } => {
                if let Some(root) = self.group_root(id)
                    && let Some(name) = tokens::GROUP_NAMES.get(usize::from(color))
                    && let Err(e) = self.projects.set_color(&root, name)
                {
                    self.toasts.error(format!("Couldn't change the project's colour: {e}"));
                }
            }
            TabAction::ShowGroup(id) => {
                if let Some(root) = self.group_root(id) {
                    self.projects.select_root(&root);
                    self.sidebar.open = true;
                }
            }
            TabAction::CloseGroup(id) => {
                if let Some(root) = self.group_root(id) {
                    self.close_project(&root);
                }
            }
        }
    }

    /// Collapses or expands a project's tab group.
    pub fn toggle_group(&mut self, id: u64) {
        let gid = craftworks_core::GroupId(id);
        let Some(collapsed) = self.tabs.group(gid).map(|g| !g.collapsed) else { return };
        if self.tabs.set_collapsed(gid, collapsed) {
            if let Some(root) = self.group_root(id) {
                self.projects.set_collapsed(&root, collapsed);
            }
        } else {
            self.toasts.info("It's the only open tab, so its group stays open");
        }
    }

    /// Closes a project: each of its tabs through its app's own close (which may ask to save),
    /// then the project. A cancelled close keeps the project open.
    pub fn close_project(&mut self, root: &Path) {
        let ids = self.tabs.group_by_key(&root.to_string_lossy()).map(|g| self.tabs.in_group(g)).unwrap_or_default();
        for id in ids {
            self.close_tab(id);
        }
        self.closing_projects.push(root.to_path_buf());
    }

    fn on_newtab_action(&mut self, a: NewTabAction) {
        match a {
            NewTabAction::New(kind) => self.new_document(kind),
            NewTabAction::OpenPdf => self.choose_file(true),
            NewTabAction::OpenFile => self.choose_file(false),
            NewTabAction::OpenRecent(p) => self.open(&p),
            NewTabAction::ForgetRecent(p) => {
                self.recent.forget(&p);
                settings::save_recent(&self.recent);
                self.toasts.info(format!("Removed {} from Recent", file_name(&p)));
            }
            NewTabAction::Dismiss => {}
        }
    }

    /// Runs the shown engine's frame in `rect` and the hidden engines' logic.
    fn engines_frame(&mut self, ui: &mut egui::Ui, rect: Rect) {
        let ctx = ui.ctx().clone();
        let ppp = ctx.pixels_per_point();
        let shown = self.tabs.active().map(|t| t.kind);
        let window_input = ctx.input(|i| i.raw.clone());

        // Hidden engines: logic only, often while they have work, else a slow heartbeat.
        let theme = Some(if self.is_dark() { egui::Theme::Dark } else { egui::Theme::Light });
        let mut quiet = embed::input_for_area(&egui::RawInput { events: Vec::new(), ..window_input.clone() }, rect, ppp, None, |_| false);
        quiet.system_theme = theme;
        let mut next: Option<Duration> = None;
        for (kind, host) in &mut self.engines {
            if Some(*kind) == shown {
                continue;
            }
            let busy = host.context().has_requested_repaint();
            if busy {
                let causes = if self.check.is_some() { format!(" {:?}", host.context().repaint_causes().first()) } else { String::new() };
                self.wake_reasons.push(format!("{kind:?} (hidden) has work{causes}"));
            }
            let every = if busy { BACKGROUND_TICK } else { BACKGROUND_IDLE };
            if host.logic_due(every) {
                let _ = host.logic_only(quiet.clone());
            }
            next = Some(next.map_or(every, |n| n.min(every)));
        }
        if let Some(n) = next {
            ctx.request_repaint_after(n);
        }

        let Some(kind) = shown else { return };
        // The tab content's node in the window's accessibility tree: the engine's tree hangs
        // under it (a11y::Graft).
        let content_id = egui::Id::unique("cw_tab_content");
        let _ = ctx.accesskit_node_builder(content_id, |n| {
            n.set_role(egui::accesskit::Role::Group);
            n.set_label("Tab content");
        });
        let a11y_active = self.a11y.lock().map(|mut s| {
            s.host = Some(content_id.accesskit_id());
            s.active
        });
        if a11y_active.unwrap_or(false) {
            self.engine(kind).context().enable_accesskit();
        }
        // Keys reached the window's input hook already and wait in `document_keys`.
        let pointer = ctx.input(|i| i.pointer.latest_pos());
        let mut input = embed::input_for_area(&window_input, rect, ppp, pointer, |_| false);
        // Documents are opened in tabs, never handed to the shown app.
        input.dropped_files.retain(|f| !craftworks_shell::drops::opens_in_a_tab(f.path()));
        input.hovered_files.retain(|f| f.path.as_deref().is_none_or(|p| !craftworks_shell::drops::opens_in_a_tab(p)));
        input.events.append(&mut self.document_keys);
        let engine_root = egui::accesskit_root_id().accesskit_id();
        for e in &mut input.events {
            if let Event::AccessKitActionRequest(req) = e {
                *req = craftworks_render::a11y::engine_action(req.clone(), engine_root);
            }
        }
        if self.check.is_some() || !self.inject.is_empty() {
            // `--check` decides the window's focus itself, whatever app is in front: focused all
            // along while it stands for a user (WordCraft drops a keystroke that arrives in the
            // same frame as focus does), unfocused for the background idle measurement.
            let focused = self.check_focused || !self.inject.is_empty();
            input.focused = focused;
            if let Some(vp) = input.viewports.get_mut(&egui::ViewportId::ROOT) {
                vp.focused = Some(focused);
            }
            input.events.append(&mut self.inject);
        }
        input.system_theme = theme;
        let size_px = embed::pixels(rect.size(), ppp);
        let switched = self.last_shown.is_some_and(|k| k != kind) && self.drawn.contains(&kind);
        self.drawn.insert(kind);
        self.last_shown = Some(kind);
        let (device, queue) = (self.render.device.clone(), self.render.queue.clone());
        let result = self.engine(kind).frame(&device, &queue, input, size_px);
        self.last_frame = ShownFrame { kind: Some(kind), switched, font_atlas_rebuilt: result.font_atlas_rebuilt, cpu: result.cpu };
        if result.repaint_after < Duration::from_secs(1) {
            let cause = if result.repaint_after.is_zero() && self.check.is_some() {
                format!(" {:?}", self.engine(kind).context().repaint_causes().first())
            } else {
                String::new()
            };
            self.wake_reasons.push(format!("{kind:?} (shown) repaint after {:?}{cause}", result.repaint_after));
        }
        if switched && result.font_atlas_rebuilt {
            // Fonts the app changed while its tab was hidden (logic-only passes) arrive with its
            // first shown frame; the frames it then skips keep its last image (no blank flash).
            log::debug!("{kind:?}: fonts changed while hidden; the new atlas uploads with its first frame");
        }
        if result.kept_last_image {
            log::debug!("{kind:?} drew nothing this frame; its last image stays");
        }

        let Some(view) = self.engines.get(&kind).and_then(|h| h.view()) else { return };
        let mut renderer = self.render.renderer.write();
        let tex = match self.textures.get(&kind) {
            Some((id, size)) if *size == size_px => *id,
            Some((id, _)) => {
                renderer.update_egui_texture_from_wgpu_texture(&device, view, wgpu::FilterMode::Nearest, *id);
                *id
            }
            None => renderer.register_native_texture(&device, view, wgpu::FilterMode::Nearest),
        };
        drop(renderer);
        self.textures.insert(kind, (tex, size_px));
        let mut result = result;
        if let Ok(mut s) = self.a11y.lock() {
            s.engine = result.platform.accesskit_update.take();
        }
        ui.painter().image(tex, rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), egui::Color32::WHITE);
        let chrome_focused = self.chrome_has_keyboard(&ctx);
        let to_open = forward_output(&ctx, result, rect, chrome_focused);
        self.dropped_to_open.extend(to_open);
    }

    /// Whether the chrome has the keyboard: the new-tab popover is open, or the start page's
    /// filter field has focus. With tabs open and no popover the chrome has no text field, so
    /// the keyboard is the document's (a stale focus left by a closed popover doesn't count).
    fn chrome_has_keyboard(&self, ctx: &egui::Context) -> bool {
        self.newtab.open || self.sidebar.wants_keyboard(ctx) || (self.tabs.is_empty() && ctx.egui_wants_keyboard_input())
    }

    fn shortcuts(&mut self) {
        for a in std::mem::take(&mut self.shortcut_actions) {
            match a {
                keys::Action::NewTab => self.newtab.toggle(),
                keys::Action::CloseTab => {
                    if let Some(id) = self.tabs.active().map(|t| t.id) {
                        self.close_tab(id);
                    }
                }
                keys::Action::Open => self.choose_file(false),
                keys::Action::ReopenClosedTab => match self.closed.pop() {
                    Some(path) => self.open(&path),
                    None => {
                        self.toasts.info("No closed tab to reopen");
                    }
                },
                keys::Action::NextTab => {
                    self.tabs.cycle(1);
                }
                keys::Action::PreviousTab => {
                    self.tabs.cycle(-1);
                }
                keys::Action::ToggleSidebar => self.sidebar.toggle(),
                keys::Action::SearchProject => self.sidebar.focus_search(),
            }
        }
    }
}

impl eframe::App for App {
    /// Sorts the window's input before its pass (keys::route): shortcuts to the window,
    /// keyboard input to the shown document (unless the chrome has the keyboard), everything
    /// else to both. The window's own egui never sees the document's keys, so Tab, arrows and
    /// typing can't move focus in the chrome.
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        // The menu bar's key equivalents come back as the key presses they were.
        if let Some(native) = self.native_menu.as_mut() {
            native.raw_input(raw);
        }
        let chrome_focused = self.chrome_has_keyboard(ctx) || self.tabs.is_empty();
        if !chrome_focused && ctx.egui_wants_keyboard_input() {
            // Focus left on a field that's gone (the popover closed): let it go.
            ctx.memory_mut(|m| m.stop_text_input());
        }
        let mut keep = Vec::with_capacity(raw.events.len());
        for e in std::mem::take(&mut raw.events) {
            match keys::route(&e, chrome_focused) {
                Route::Shortcut(Some(a)) => self.shortcut_actions.push(a),
                Route::Shortcut(None) => {}
                Route::Document => self.document_keys.push(e),
                Route::Chrome => keep.push(e),
                Route::Both => {
                    if let Event::ModifiersChanged(_) = e {
                        self.document_keys.push(e.clone());
                    }
                    keep.push(e);
                }
            }
        }
        raw.events = keep;

        // Dropped documents open in tabs: at the drop position when dropped on the tab strip
        // (the strip handles them), else after the active tab. Other files are the document's.
        if !raw.dropped_files.is_empty() {
            let over_strip = ctx.input(|i| i.pointer.latest_pos()).is_some_and(|p| p.y < self.titlebar.height);
            let mut others = Vec::new();
            for f in std::mem::take(&mut raw.dropped_files) {
                let path = f.path().to_path_buf();
                // Other files are the document's; documents dropped on the strip are the strip's.
                if over_strip || !craftworks_shell::drops::opens_in_a_tab(&path) {
                    others.push(f);
                } else {
                    self.dropped_to_open.push(path);
                }
            }
            raw.dropped_files = others;
        }
    }

    /// Every frame, also while the window is hidden: Finder's and the Dock's requests, and a
    /// close request, which becomes a quit that asks about unsaved work first.
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        #[cfg(target_os = "macos")]
        if let Some(inbox) = &self.apple {
            for e in inbox.drain() {
                match e {
                    fmv_macos_events::Event::Open(paths) => self.dropped_to_open.extend(paths),
                    fmv_macos_events::Event::Quit => self.start_quit(),
                }
            }
        }
        if ctx.input(|i| i.viewport().close_requested()) && !self.quit_confirmed {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.start_quit();
            ctx.request_repaint();
        }
        // Last, so it sees the CancelClose above (craft-quit's rule).
        craft_quit::on_close_request(ctx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if self.frames == 0 {
            self.install_menu(&ctx);
        }
        self.frames += 1;
        if let Some(mut c) = self.check.take() {
            c.step(self, &ctx);
            if !c.measuring_idle() {
                // The check counts frames; it asks for its own, except while it measures idleness.
                ctx.request_repaint();
            }
            self.check = Some(c);
        }
        self.wake_reasons.clear();
        if !self.titlebar_measured
            && let Some(t) = crate::titlebar::measure()
        {
            log::info!("title row: {t:?}");
            self.titlebar = t;
            self.titlebar_measured = true;
        }
        self.sync_appearance(&ctx);
        self.menu_frame(&ctx);
        self.shortcuts();
        for p in std::mem::take(&mut self.dropped_to_open) {
            self.open(&p);
        }
        self.sync();
        self.step_quit();
        self.save_session_if_changed();
        self.sync_groups();
        self.projects.frame();
        self.step_goto();
        self.save_sidebar_state();

        let p = self.palette;
        let fullscreen = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
        let tb = if fullscreen { self.titlebar.fullscreen() } else { self.titlebar };
        let mut actions = Vec::new();
        egui::Panel::top("cw_title").exact_size(tb.height).frame(egui::Frame::NONE.fill(p.surface)).show(ui, |ui| {
            let r = ui.max_rect();
            ui.painter().hline(r.x_range(), r.bottom() - 0.5, Stroke::new(1.0, p.hairline));
            let strip = Rect::from_min_max(pos2(r.left() + tb.content_left, r.top()), pos2(r.right() - 12.0, r.bottom()));
            let items = self.tab_items();
            let groups = self.group_items();
            let active = self.tabs.active().map(|t| t.id.0);
            actions = tab_strip(ui, strip, &p, &items, &groups, active, &mut self.strip);
        });
        for a in actions {
            self.on_tab_action(&ctx, a);
        }

        if self.sidebar.open {
            let pinned = self.projects.pinned();
            let active = self.active_path();
            let current = self.projects.current_item();
            let read_only = self.projects.project().is_some_and(|p| p.read_only);
            let view = SidebarView {
                project: current.as_ref(),
                projects: &self.projects.items,
                entries: self.projects.current_open().map_or(&[][..], |o| &o.snapshot.entries),
                pinned: &pinned,
                active: active.as_deref(),
                hits: &self.projects.hits,
                progress: &self.projects.progress,
                read_only,
            };
            let sidebar = &mut self.sidebar;
            let acts = egui::Panel::left("cw_sidebar")
                .resizable(true)
                .default_size(sidebar::WIDTH)
                .size_range(sidebar::MIN_WIDTH..=sidebar::MAX_WIDTH)
                .frame(egui::Frame::NONE.fill(p.surface))
                .show(ui, |ui| sidebar.show(ui, &p, &view))
                .inner;
            for a in acts {
                self.on_sidebar_action(&ctx, a);
            }
        }

        let mut page_action = None;
        egui::CentralPanel::default().frame(egui::Frame::NONE.fill(p.surface)).show(ui, |ui| {
            let rect = ui.max_rect();
            self.content_rect = rect;
            if self.tabs.is_empty() && self.pending_opens.is_empty() {
                let home = settings::home();
                let exists = |p: &Path| p.exists();
                let view = RecentView { recent: &self.recent, now: chrono::Local::now(), home: home.as_deref(), exists: &exists };
                page_action = self.newtab.page(ui, rect, &p, &view);
                self.last_shown = None;
                // The engines keep their background work going.
                self.engines_frame(ui, Rect::from_min_size(rect.min, egui::vec2(1.0, 1.0)));
            } else {
                self.engines_frame(ui, rect);
            }
        });
        if let Some(a) = page_action {
            self.on_newtab_action(a);
        }
        if let Some(anchor) = self.strip.plus_rect {
            let home = settings::home();
            let exists = |p: &Path| p.exists();
            let view = RecentView { recent: &self.recent, now: chrono::Local::now(), home: home.as_deref(), exists: &exists };
            if let Some(a) = self.newtab.popover(&ctx, &p, anchor, &view) {
                self.on_newtab_action(a);
            }
        }
        let _ = self.toasts.show(&ctx, &p, self.content_rect);
        if self.check.is_some()
            && let Some(d) = self.a11y.lock().ok().and_then(|s| s.window_repaint_delay)
        {
            // The previous frame's request (this frame's is known only after the pass).
            let bucket = if d.is_zero() {
                "now".to_string()
            } else if d < Duration::from_millis(100) {
                format!("{} ms", d.as_millis())
            } else {
                "≥100 ms".to_string()
            };
            self.wake_reasons.push(format!("window asked for its next frame: {bucket}"));
        }
    }
}

/// Hands the engine's output to the window: cursor, clipboard and URL commands, IME position
/// (only while the document has the keyboard), and when to paint next. Links go through
/// [`route_url`]: files Craftworks opens come back to open in tabs (DeckCraft's Print opens its
/// PDF), web and mail links open in the system, anything else is refused.
fn forward_output(ctx: &egui::Context, result: FrameResult, area: Rect, chrome_focused: bool) -> Vec<PathBuf> {
    let p = result.platform;
    if ctx.input(|i| i.pointer.hover_pos()).is_some_and(|pos| area.contains(pos)) {
        ctx.set_cursor_icon(p.cursor_icon);
    }
    let mut to_open = Vec::new();
    let mut commands = Vec::with_capacity(p.commands.len());
    for c in p.commands {
        match c {
            egui::OutputCommand::OpenUrl(u) => match route_url(&u.url) {
                UrlRoute::Tab(path) => to_open.push(path),
                UrlRoute::System => craftworks_platform::files::open_url(&u.url),
                UrlRoute::Refuse => log::warn!("refused to open {:?} (only web, mail and file links open)", u.url),
            },
            other => commands.push(other),
        }
    }
    ctx.output_mut(|o| {
        o.commands.extend(commands);
        if let Some(mut ime) = p.ime.filter(|_| !chrome_focused) {
            ime.rect = embed::to_window(ime.rect, area);
            ime.cursor_rect = embed::to_window(ime.cursor_rect, area);
            o.ime = Some(ime);
        }
    });
    if result.repaint_after == Duration::ZERO {
        ctx.request_repaint();
    } else if result.repaint_after < Duration::MAX {
        ctx.request_repaint_after(result.repaint_after);
    }
    to_open
}

/// Where a link an app opens goes.
#[derive(Debug, PartialEq, Eq)]
enum UrlRoute {
    /// A file Craftworks opens: a tab.
    Tab(PathBuf),
    /// A web, mail or other file link: the system's default app.
    System,
    Refuse,
}

fn route_url(url: &str) -> UrlRoute {
    if let Some(rest) = url.strip_prefix("file://") {
        // file:///tmp/a.pdf (and DeckCraft's file:////tmp/…): the path, percent-decoded.
        let path = PathBuf::from(format!("/{}", percent_decode(rest).trim_start_matches('/')));
        if craftworks_shell::drops::opens_in_a_tab(&path) {
            return UrlRoute::Tab(path);
        }
    }
    if craftworks_platform::files::openable(url) { UrlRoute::System } else { UrlRoute::Refuse }
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && let Some(hex) = s.get(i + 1..i + 3)
            && let Ok(v) = u8::from_str_radix(hex, 16)
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn file_name(p: &Path) -> String {
    p.file_name().map_or_else(|| p.display().to_string(), |n| n.to_string_lossy().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_to_documents_open_in_tabs_and_others_are_checked() {
        assert_eq!(route_url("file:////tmp/Deck%20preview.pdf"), UrlRoute::Tab(PathBuf::from("/tmp/Deck preview.pdf")));
        assert_eq!(route_url("file:///Users/me/Report.docx"), UrlRoute::Tab(PathBuf::from("/Users/me/Report.docx")));
        assert_eq!(route_url("https://example.com"), UrlRoute::System);
        assert_eq!(route_url("file:///tmp/photo.png"), UrlRoute::System);
        assert_eq!(route_url("javascript:alert(1)"), UrlRoute::Refuse);
        assert_eq!(route_url("x-apple.systempreferences:com.apple.preference"), UrlRoute::Refuse);
    }
}
