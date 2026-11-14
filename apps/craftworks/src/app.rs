//! The Craftworks window: our chrome (title row with tabs, new-tab popover, start page,
//! toasts) around the four Craft engines, each in an egui context of its own (D1), painted
//! into a texture by `craftworks-render` and shown below the title row.

use crate::menu::{self, MenuCommand};
use crate::{check, session, settings, titlebar::TitleBar};
use craftworks_core::engines::new_editor;
use craftworks_core::{CloseState, DocId, Kind, Opened, TabId, Tabs};
use craftworks_render::{EngineHost, FrameResult, embed};
use craftworks_shell::keys::{self, Route};
use craftworks_shell::{NewTab, NewTabAction, Palette, Recent, RecentView, TabAction, TabItem, TabStripState, Toasts, tab_strip, tokens};
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
    pub fn new(render: egui_wgpu::RenderState, ctx: &egui::Context, forced_dark: Option<bool>) -> Self {
        ctx.set_fonts(tokens::fonts());
        let palette = Palette::for_dark(forced_dark.unwrap_or(false));
        tokens::apply(ctx, &palette);
        let a11y = std::sync::Arc::new(std::sync::Mutex::new(craftworks_render::a11y::Shared::default()));
        ctx.add_plugin(craftworks_render::a11y::Graft::new(a11y.clone()));
        App {
            render,
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
            closed: Vec::new(),
            content_rect: Rect::NOTHING,
            a11y,
            frames: 0,
            wake_reasons: Vec::new(),
            check: None,
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

    fn menu_bar(&self) -> craft_menu::MenuBar {
        let items = self.tab_items();
        let active = self.tabs.active().map(|t| t.id.0);
        let state = menu::MenuState {
            shown: self.tabs.active().map(|t| t.kind),
            tabs: items.iter().map(|t| menu::TabEntry { id: t.id, label: t.label.clone(), active: Some(t.id) == active }).collect(),
            recent: self.recent.files().iter().map(|f| f.path.as_path()).collect(),
            can_reopen: !self.closed.is_empty(),
        };
        menu::bar(&state)
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
        self.engines.entry(kind).or_insert_with(|| {
            let mut host = EngineHost::new(device, format, new_editor(kind));
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
            });
        }
        items
    }

    fn on_tab_action(&mut self, ctx: &egui::Context, a: TabAction) {
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
        }
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
                self.wake_reasons.push(format!("{kind:?} (hidden) has work"));
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
            self.wake_reasons.push(format!("{kind:?} (shown) repaint after {:?}", result.repaint_after));
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
        forward_output(&ctx, result, rect, chrome_focused);
    }

    /// Whether the chrome has the keyboard: the new-tab popover is open, or the start page's
    /// filter field has focus. With tabs open and no popover the chrome has no text field, so
    /// the keyboard is the document's (a stale focus left by a closed popover doesn't count).
    fn chrome_has_keyboard(&self, ctx: &egui::Context) -> bool {
        self.newtab.open || (self.tabs.is_empty() && ctx.egui_wants_keyboard_input())
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

        let p = self.palette;
        let fullscreen = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
        let tb = if fullscreen { self.titlebar.fullscreen() } else { self.titlebar };
        let mut actions = Vec::new();
        egui::Panel::top("cw_title").exact_size(tb.height).frame(egui::Frame::NONE.fill(p.surface)).show(ui, |ui| {
            let r = ui.max_rect();
            ui.painter().hline(r.x_range(), r.bottom() - 0.5, Stroke::new(1.0, p.hairline));
            let strip = Rect::from_min_max(pos2(r.left() + tb.content_left, r.top()), pos2(r.right() - 12.0, r.bottom()));
            let items = self.tab_items();
            let active = self.tabs.active().map(|t| t.id.0);
            actions = tab_strip(ui, strip, &p, &items, active, &mut self.strip);
        });
        for a in actions {
            self.on_tab_action(&ctx, a);
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
/// (only while the document has the keyboard), and when to paint next.
fn forward_output(ctx: &egui::Context, result: FrameResult, area: Rect, chrome_focused: bool) {
    let p = result.platform;
    if ctx.input(|i| i.pointer.hover_pos()).is_some_and(|pos| area.contains(pos)) {
        ctx.set_cursor_icon(p.cursor_icon);
    }
    ctx.output_mut(|o| {
        o.commands.extend(p.commands);
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
}

fn file_name(p: &Path) -> String {
    p.file_name().map_or_else(|| p.display().to_string(), |n| n.to_string_lossy().to_string())
}
