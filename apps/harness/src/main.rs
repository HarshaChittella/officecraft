//! Phase 1 test harness (temporary): the four Craft engines side by side in tabs of one
//! eframe window, each in an egui context of its own (D1), painted into a texture by
//! `craftworks-render` and shown below a plain egui tab strip.
//!
//! ```text
//! harness [FILE…]              open files (routed by extension) or a new document
//! harness --shots DIR          scripted run: one of each type, screenshots, switch and save
//!                              checks, a report in DIR/report.txt, then quit
//! ```
//! `CW_LOG=debug` shows dropped window commands too (default: warnings).
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod logger;
mod script;

use craftworks_core::engines::new_editor;
use craftworks_core::{CloseState, DocId, Kind, Opened, TabId, Tabs};
use craftworks_render::{EngineHost, FrameResult, embed};
use egui::{Color32, Event, Key, RichText};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Height of the harness's tab strip, in points.
const TAB_BAR: f32 = 34.0;
/// How often an engine that isn't shown runs `logic` (D4).
const BACKGROUND_TICK: Duration = Duration::from_millis(250);

fn main() -> eframe::Result {
    logger::install();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut files = Vec::new();
    let mut shots = None;
    let mut it = args.into_iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--shots" => shots = it.next().map(PathBuf::from),
            "-h" | "--help" => {
                println!("harness [FILE…] | harness --shots DIR");
                return Ok(());
            }
            _ => files.push(PathBuf::from(a)),
        }
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_title("Craftworks harness").with_inner_size([1500.0, 960.0]),
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };
    eframe::run_native(
        "Craftworks harness",
        options,
        Box::new(move |cc| {
            let render = cc.wgpu_render_state.clone().ok_or("the harness needs the wgpu renderer")?;
            let mut h = Harness::new(render);
            match shots {
                Some(dir) => h.script = Some(script::Script::new(dir)),
                None if files.is_empty() => h.new_document(Kind::Document),
                None => files.iter().for_each(|f| h.open(f)),
            }
            Ok(Box::new(h))
        }),
    )
}

/// A close the harness asked an engine for, waiting for the app's answer.
struct Closing {
    tab: TabId,
    kind: Kind,
    doc: DocId,
    frames: u32,
}

/// What a frame of the shown engine reported, for the script and the status line.
#[derive(Clone, Copy, Debug, Default)]
pub struct ShownFrame {
    pub kind: Option<Kind>,
    /// The first frame of this engine after another one was shown.
    pub switched: bool,
    pub font_atlas_rebuilt: bool,
    pub cpu: Duration,
}

pub struct Harness {
    render: egui_wgpu::RenderState,
    engines: BTreeMap<Kind, EngineHost>,
    /// Each engine's texture as registered with the window's renderer, and its pixel size.
    textures: HashMap<Kind, (egui::TextureId, [u32; 2])>,
    pub tabs: Tabs,
    /// The documents each engine had last frame (to notice ones the app opened itself).
    known: HashMap<Kind, HashSet<DocId>>,
    pending_opens: Vec<(Kind, PathBuf)>,
    closing: Vec<Closing>,
    last_shown: Option<Kind>,
    /// Engines that have drawn at least once (their first frame installs their fonts).
    drawn: HashSet<Kind>,
    pub last_frame: ShownFrame,
    pub status: String,
    /// Events the script adds to the shown engine's next input (engine coordinates).
    pub inject: Vec<Event>,
    script: Option<script::Script>,
}

impl Harness {
    fn new(render: egui_wgpu::RenderState) -> Self {
        Harness {
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
            status: String::new(),
            inject: Vec::new(),
            script: None,
        }
    }

    /// The engine for `kind`, made on first use.
    pub fn engine(&mut self, kind: Kind) -> &mut EngineHost {
        let (device, format) = (&self.render.device, self.render.target_format);
        self.engines.entry(kind).or_insert_with(|| EngineHost::new(device, format, new_editor(kind)))
    }

    pub fn engine_ref(&self, kind: Kind) -> Option<&EngineHost> {
        self.engines.get(&kind)
    }

    pub fn new_document(&mut self, kind: Kind) {
        match self.engine(kind).editor_mut().new_document() {
            Ok(doc) => {
                self.tabs.open(kind, doc);
                self.known.entry(kind).or_default().insert(doc);
            }
            Err(e) => self.status = format!("New {}: {e}", kind.label()),
        }
    }

    pub fn open(&mut self, path: &Path) {
        let Some(kind) = Kind::for_path(path) else {
            self.status = format!("No engine opens {}", path.display());
            return;
        };
        match self.engine(kind).editor_mut().open(path) {
            Ok(Opened::Now(doc)) => {
                self.tabs.open(kind, doc);
                self.known.entry(kind).or_default().insert(doc);
            }
            Ok(Opened::Later) => self.pending_opens.push((kind, path.to_path_buf())),
            Err(e) => self.status = format!("Open {}: {e}", path.display()),
        }
    }

    /// Starts closing tab `id` through its app's own close (which may ask to save).
    pub fn close_tab(&mut self, id: TabId) {
        let Some(tab) = self.tabs.get(id).copied() else { return };
        if self.closing.iter().any(|c| c.tab == id) {
            return;
        }
        self.tabs.activate(id);
        match self.engine(tab.kind).editor_mut().request_close(tab.doc) {
            Ok(()) => self.closing.push(Closing { tab: id, kind: tab.kind, doc: tab.doc, frames: 0 }),
            Err(e) => self.status = format!("Close: {e}"),
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
                (true, CloseState::Cancelled) => self.status = "Close cancelled".into(),
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

        let shown_kind = self.tabs.active().map(|t| t.kind);
        for (kind, host) in &self.engines {
            let ed = host.editor();
            let ids: HashSet<DocId> = ed.documents().iter().map(|d| d.id).collect();
            // Documents the app closed by itself lose their tabs.
            let gone: Vec<TabId> = self
                .tabs
                .iter()
                .filter(|t| t.kind == *kind && !ids.contains(&t.doc) && !self.closing.iter().any(|c| c.tab == t.id))
                .map(|t| t.id)
                .collect();
            for t in gone {
                self.tabs.remove(t);
            }
            // A document the shown app opened by itself (its own File › New or Open) gets a tab,
            // but not the blank one an app leaves behind when its last document closes.
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
            && let Err(e) = host.editor_mut().activate(tab.doc)
        {
            self.status = format!("Switch: {e}");
        }
    }

    /// The tab strip: one button per tab, a dirty dot, close buttons and the "+" menu.
    fn tab_strip(&mut self, ui: &mut egui::Ui) {
        let mut activate = None;
        let mut close = None;
        let mut new_kind = None;
        let mut open_file = false;
        ui.horizontal_centered(|ui| {
            let active = self.tabs.active().map(|t| t.id);
            for tab in self.tabs.iter() {
                let info = self.engines.get(&tab.kind).and_then(|e| e.editor().documents().into_iter().find(|d| d.id == tab.doc));
                let title = info.as_ref().map_or_else(|| "…".to_string(), |d| d.title.clone());
                let dirty = info.as_ref().is_some_and(|d| d.dirty);
                let (dot, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                ui.painter().circle_filled(dot.center(), 4.5, kind_color(tab.kind));
                let text = RichText::new(format!("{title}{}", if dirty { "  (unsaved)" } else { "" })).color(kind_color(tab.kind));
                let r = ui.selectable_label(active == Some(tab.id), text);
                if r.clicked() {
                    activate = Some(tab.id);
                }
                if r.middle_clicked() || ui.small_button("×").clicked() {
                    close = Some(tab.id);
                }
                ui.separator();
            }
            ui.menu_button("+", |ui| {
                for kind in Kind::ALL.into_iter().filter(|k| k.can_create()) {
                    if ui.button(format!("New {}", kind.label())).clicked() {
                        new_kind = Some(kind);
                        ui.close();
                    }
                }
                if ui.button("Open File…").clicked() {
                    open_file = true;
                    ui.close();
                }
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(RichText::new(&self.status).weak());
            });
        });
        if let Some(id) = activate {
            self.tabs.activate(id);
        }
        if let Some(id) = close {
            self.close_tab(id);
        }
        if let Some(kind) = new_kind {
            self.new_document(kind);
        }
        if open_file
            && let Some(path) = rfd::FileDialog::new()
                .add_filter("Documents", &["docx", "doc", "odt", "rtf", "html", "htm", "md", "tex", "txt", "xlsx", "csv", "tsv", "pptx", "pdf"])
                .pick_file()
        {
            self.open(&path);
        }
    }

    /// Runs the shown engine's frame in `rect` and the background engines' logic.
    fn engines_frame(&mut self, ui: &mut egui::Ui, rect: egui::Rect) {
        let ctx = ui.ctx().clone();
        let ppp = ctx.pixels_per_point();
        let shown = self.tabs.active().map(|t| t.kind);
        let window_input = ctx.input(|i| i.raw.clone());

        // Background engines: logic only, a few times a second.
        let quiet = embed::input_for_area(&egui::RawInput { events: Vec::new(), ..window_input.clone() }, rect, ppp, |_| false);
        let mut any_background = false;
        for (kind, host) in &mut self.engines {
            if Some(*kind) != shown {
                any_background = true;
                if host.logic_due(BACKGROUND_TICK) {
                    let _ = host.logic_only(quiet.clone());
                }
            }
        }
        if any_background {
            ctx.request_repaint_after(BACKGROUND_TICK);
        }

        let Some(kind) = shown else {
            ui.painter().rect_filled(rect, 0.0, Color32::from_gray(245));
            ui.put(rect, egui::Label::new("No tabs. Use + to make or open one."));
            self.last_shown = None;
            return;
        };
        let mut input = embed::input_for_area(&window_input, rect, ppp, |e| window_shortcut(e).is_none());
        input.events.append(&mut self.inject);
        let size_px = embed::pixels(rect.size(), ppp);
        // A switch to an engine that has drawn before; its first frame ever installs its fonts.
        let switched = self.last_shown.is_some_and(|k| k != kind) && self.drawn.contains(&kind);
        self.drawn.insert(kind);
        self.last_shown = Some(kind);
        let (device, queue) = (self.render.device.clone(), self.render.queue.clone());
        let result = self.engine(kind).frame(&device, &queue, input, size_px);
        self.last_frame = ShownFrame { kind: Some(kind), switched, font_atlas_rebuilt: result.font_atlas_rebuilt, cpu: result.cpu };
        if switched && result.font_atlas_rebuilt {
            log::warn!("{kind:?}: the font atlas was rebuilt on the frame that switched to it");
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
        ui.painter().image(tex, rect, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
        forward_output(&ctx, result, rect);
    }

    /// The harness's own shortcuts: ⌘T new document of the shown kind, ⌘W close the tab,
    /// ⌃Tab / ⌃⇧Tab next and previous tab.
    fn shortcuts(&mut self, ctx: &egui::Context) {
        let actions: Vec<Shortcut> = ctx.input(|i| i.raw.events.iter().filter_map(window_shortcut).collect());
        for a in actions {
            match a {
                Shortcut::NewTab => self.new_document(self.tabs.active().map_or(Kind::Document, |t| t.kind)),
                Shortcut::CloseTab => {
                    if let Some(id) = self.tabs.active().map(|t| t.id) {
                        self.close_tab(id);
                    }
                }
                Shortcut::Cycle(n) => {
                    self.tabs.cycle(n);
                }
            }
        }
    }
}

impl eframe::App for Harness {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if let Some(mut s) = self.script.take() {
            s.step(self, ui.ctx());
            self.script = Some(s);
        }
        self.shortcuts(ui.ctx());
        self.sync();
        egui::Panel::top("craftworks_tabs").exact_size(TAB_BAR).show(ui, |ui| self.tab_strip(ui));
        egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| {
            let rect = ui.max_rect();
            self.engines_frame(ui, rect);
        });
    }
}

/// Hands the engine's output to the window: cursor, clipboard and URL commands, IME position,
/// and when to paint next.
fn forward_output(ctx: &egui::Context, result: FrameResult, area: egui::Rect) {
    let p = result.platform;
    if ctx.input(|i| i.pointer.hover_pos()).is_some_and(|pos| area.contains(pos)) {
        ctx.set_cursor_icon(p.cursor_icon);
    }
    ctx.output_mut(|o| {
        o.commands.extend(p.commands);
        if let Some(mut ime) = p.ime {
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Shortcut {
    NewTab,
    CloseTab,
    Cycle(isize),
}

/// The harness's own shortcut in `e`, if it is one. These keys don't reach the engines.
fn window_shortcut(e: &Event) -> Option<Shortcut> {
    let Event::Key { key, pressed: true, modifiers, .. } = e else { return None };
    match key {
        Key::T if modifiers.command && !modifiers.shift => Some(Shortcut::NewTab),
        Key::W if modifiers.command && !modifiers.shift => Some(Shortcut::CloseTab),
        Key::Tab if modifiers.ctrl => Some(Shortcut::Cycle(if modifiers.shift { -1 } else { 1 })),
        _ => None,
    }
}

/// The tab's colour by kind (our own palette).
fn kind_color(kind: Kind) -> Color32 {
    match kind {
        Kind::Document => Color32::from_rgb(0x3B, 0x82, 0xF6),
        Kind::Sheet => Color32::from_rgb(0x22, 0xA0, 0x6B),
        Kind::Presentation => Color32::from_rgb(0xF0, 0x8C, 0x2E),
        Kind::Pdf => Color32::from_rgb(0xE5, 0x48, 0x4D),
    }
}
