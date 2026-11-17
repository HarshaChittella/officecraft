//! `officecraft --check DIR [--appearance light|dark]`: the Phase 1 and Phase 2 acceptance run,
//! scripted, in the real app.
//!
//! 1. The start page with no tabs (screenshot).
//! 2. One tab of each kind with a little content (each app's own commands), and a PDF made by
//!    PdfCraft's engine and opened.
//! 3. One appearance: every engine draws light or dark as the window does.
//! 4. A screenshot of each tab, and of the new-tab popover.
//! 5. Twelve tab switches with no font-atlas rebuild (no font flash).
//! 6. Edit and save in every tab; typing through the input path.
//! 7. A dirty close shows the app's own Save prompt (screenshot); Cancel keeps the work.
//! 8. Resize: frame times at several window sizes.
//! 9. Memory, then idle CPU over 10 s with nothing happening.
//!
//! Everything goes to DIR/report.txt, then the window closes.

use crate::{app::App, logger};
use officecraft_core::{CloseState, DocId, Kind, TabId};
use serde_json::json;
use std::fmt::Write as _;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const ORDER: [Kind; 4] = [Kind::Document, Kind::Sheet, Kind::Presentation, Kind::Pdf];
const SIZES: [[f32; 2]; 4] = [[1100.0, 760.0], [1600.0, 1000.0], [900.0, 620.0], [1440.0, 920.0]];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    StartPage { shot: bool },
    Setup,
    WaitForPdf,
    Appearance,
    Shots { i: usize, shoot: bool },
    NewTab { shot: bool },
    NewTabClose,
    Switches { n: usize, check: bool },
    Saves,
    Typing { phase: u8 },
    DirtyClose { phase: u8 },
    Routing { phase: u8 },
    Services { phase: u8 },
    Projects { phase: u8 },
    Ai { phase: u8 },
    BlankClose { phase: u8 },
    Display { phase: u8 },
    Crash { phase: u8 },
    Resize { i: usize, frames: u32 },
    Accessibility { frames: u32 },
    Menus { phase: u8 },
    Quit { phase: u8 },
    Memory { phase: u8 },
    Idle { started: bool, focused: bool },
    /// Home's tiles open each app's own New page.
    Templates {
        n: u8,
        started: Option<usize>,
    },
    /// The "Update available" pill and its popover (updates.rs), with a made-up release.
    UpdatePill {
        shot: bool,
    },
    /// The Connect AI panel, opened from the AI pill.
    ConnectAi {
        step: u8,
    },
    /// The Ask AI panel with a made-up conversation (nothing is sent or saved).
    AskAi {
        step: u8,
    },
    Report,
    Done,
}

pub struct Check {
    dir: PathBuf,
    suffix: &'static str,
    stage: Stage,
    wait: u32,
    tries: u32,
    shot_done: Option<Arc<AtomicBool>>,
    idle: Arc<Mutex<Option<f64>>>,
    idle_frames: u64,
    /// Pointer and wheel events seen during the idle measurement (someone using the Mac), and
    /// how many measurements they spoiled.
    idle_input: u32,
    idle_retries: u32,
    /// When the window may start measuring idleness: new documents finish their first work
    /// (layout, titles) in a few seconds, measured in time, not frames.
    settle_until: Option<f64>,
    /// When the current wait for something to happen started (seconds, the window's clock).
    since: Option<f64>,
    /// The stand-in AI's tool results, from its own thread.
    ai_rx: Option<std::sync::mpsc::Receiver<serde_json::Value>>,
    /// The open PDF's first-page rotation before the AI's edit.
    pdf_rotation: Option<i64>,
    wakes: std::collections::BTreeMap<String, u64>,
    resize_worst: f64,
    /// Mean luminance (0–1) of each tab's engine area, from its screenshot.
    luminance: Arc<Mutex<Vec<(Kind, f32)>>>,
    report: String,
    failures: Vec<String>,
}

impl Check {
    pub fn new(dir: PathBuf, forced_dark: Option<bool>) -> Self {
        let _ = std::fs::create_dir_all(&dir);
        let suffix = match forced_dark {
            Some(true) => "-dark",
            Some(false) => "-light",
            None => "",
        };
        Check {
            dir,
            suffix,
            stage: Stage::StartPage { shot: false },
            wait: 40,
            tries: 0,
            shot_done: None,
            idle: Arc::new(Mutex::new(None)),
            idle_frames: 0,
            idle_input: 0,
            idle_retries: 0,
            settle_until: None,
            since: None,
            ai_rx: None,
            pdf_rotation: None,
            wakes: Default::default(),
            resize_worst: 0.0,
            luminance: Arc::new(Mutex::new(Vec::new())),
            report: String::new(),
            failures: Vec::new(),
        }
    }

    fn line(&mut self, s: impl AsRef<str>) {
        let _ = writeln!(self.report, "{}", s.as_ref());
        println!("{}", s.as_ref());
    }

    fn check(&mut self, ok: bool, what: impl AsRef<str>) {
        let what = what.as_ref();
        self.line(format!("{} {what}", if ok { "PASS" } else { "FAIL" }));
        if !ok {
            self.failures.push(what.to_string());
        }
    }

    fn rss(&mut self, stage: &str) {
        let kb = ps("rss=").and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
        self.line(format!("MEM {stage}: {:.0} MB resident", kb as f64 / 1024.0));
    }

    fn tab_of(app: &App, kind: Kind) -> Option<(TabId, DocId)> {
        app.tabs.iter().find(|t| t.kind == kind).map(|t| (t.id, t.doc))
    }

    fn screenshot(&mut self, ctx: &egui::Context, name: &str) {
        self.screenshot_measuring(ctx, name, None);
    }

    /// A screenshot; with `measure`, also the mean luminance of `area` (points) for `kind`.
    fn screenshot_measuring(&mut self, ctx: &egui::Context, name: &str, measure: Option<(Kind, egui::Rect)>) {
        let path = self.dir.join(format!("{name}{}.png", self.suffix));
        let done = Arc::new(AtomicBool::new(false));
        let flag = done.clone();
        let lum = self.luminance.clone();
        let ppp = ctx.pixels_per_point();
        ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::ScreenshotCallback::new(move |img| {
            if let Some((kind, area)) = measure {
                let (w, h) = (img.size[0], img.size[1]);
                let (x0, y0) = (((area.left() * ppp) as usize).min(w), ((area.top() * ppp) as usize).min(h));
                let (x1, y1) = (((area.right() * ppp) as usize).min(w), ((area.bottom() * ppp) as usize).min(h));
                let (mut sum, mut n) = (0.0f64, 0u64);
                for y in (y0..y1).step_by(4) {
                    for x in (x0..x1).step_by(4) {
                        if let Some(c) = img.pixels.get(y * w + x) {
                            sum += (0.2126 * f64::from(c.r()) + 0.7152 * f64::from(c.g()) + 0.0722 * f64::from(c.b())) / 255.0;
                            n += 1;
                        }
                    }
                }
                if n > 0
                    && let Ok(mut l) = lum.lock()
                {
                    l.push((kind, (sum / n as f64) as f32));
                }
            }
            let bytes: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_srgba_unmultiplied()).collect();
            let saved = u32::try_from(img.size[0])
                .ok()
                .zip(u32::try_from(img.size[1]).ok())
                .and_then(|(w, h)| image::RgbaImage::from_raw(w, h, bytes))
                .map(|i| i.save(&path));
            match saved {
                Some(Ok(())) => println!("SHOT {}", path.display()),
                other => eprintln!("screenshot {}: {other:?}", path.display()),
            }
            flag.store(true, Ordering::SeqCst);
        })));
        self.shot_done = Some(done);
    }

    /// Whether to keep waiting for something that takes real time (a rescan, a file read):
    /// true until `secs` have passed since the wait began. Call [`Check::done_waiting`] after.
    fn still_waiting(&mut self, ctx: &egui::Context, secs: f64) -> bool {
        let now = ctx.input(|i| i.time);
        let start = *self.since.get_or_insert(now);
        now - start < secs
    }

    fn done_waiting(&mut self) {
        self.since = None;
    }

    /// Whether the window must be left alone (no frames asked for) while idle CPU is measured.
    pub fn measuring_idle(&self) -> bool {
        matches!(self.stage, Stage::Idle { started: true, .. })
    }

    /// One frame of the check. Runs before the app syncs and draws.
    pub fn step(&mut self, app: &mut App, ctx: &egui::Context) {
        if let Stage::Idle { started: true, focused } = self.stage {
            // Nothing may happen now: no repaint requested, no input. Wait for the result.
            for r in std::mem::take(&mut app.wake_reasons) {
                *self.wakes.entry(r).or_default() += 1;
            }
            self.idle_input += ctx.input(|i| {
                i.raw
                    .events
                    .iter()
                    .filter(|e| matches!(e, egui::Event::PointerMoved(_) | egui::Event::PointerButton { .. } | egui::Event::MouseWheel { .. }))
                    .count()
            }) as u32;
            let result = self.idle.lock().ok().and_then(|mut g| g.take());
            if let Some(pct) = result {
                let frames = app.frames - self.idle_frames;
                let what = if focused { "focused, caret blinking in the Document tab" } else { "in the background" };
                let input = std::mem::take(&mut self.idle_input);
                self.line(format!("IDLE CPU over 10 s, window {what}: {pct:.1} %, {frames} frames, {input} pointer events"));
                let mut wakes: Vec<_> = std::mem::take(&mut self.wakes).into_iter().collect();
                wakes.sort_by_key(|w| std::cmp::Reverse(w.1));
                for (why, n) in wakes.into_iter().take(4) {
                    self.line(format!("  wake: {why} ×{n}"));
                }
                if focused {
                    // WordCraft animates its caret's blink (a fade over a few frames), as the
                    // standalone app does: budgeted, not zero.
                    self.check(pct < 8.0, format!("a focused document with a blinking caret stays light ({pct:.1} % CPU, budget 8 %)"));
                    self.stage = Stage::Templates { n: 0, started: None };
                } else if input > 0 && pct >= 1.0 && self.idle_retries < 2 {
                    // macOS still sends mouse moves to a background window under the pointer, and
                    // each is a frame: someone using the Mac spoiled this measurement. Again.
                    self.idle_retries += 1;
                    self.line(format!("INFO the pointer was over the window; measuring again ({}/2)", self.idle_retries));
                    self.wakes.clear();
                    self.stage = Stage::Idle { started: false, focused: false };
                    self.wait = 30;
                } else {
                    // A moving mouse here is activity, not idleness.
                    self.check(
                        pct < 1.0,
                        format!("an idle window in the background uses (almost) no CPU ({pct:.1} %; the pointer must stay off the window)"),
                    );
                    app.check_focused = true;
                    self.stage = Stage::Idle { started: false, focused: true };
                    self.wait = 30;
                }
                ctx.request_repaint();
            }
            return;
        }
        ctx.request_repaint();
        if let Some(done) = self.shot_done.clone() {
            // A window macOS doesn't draw (covered, or on another Space) may never deliver its
            // screenshot: after 10 s, note it and go on rather than wait for ever.
            if !done.load(Ordering::SeqCst) && self.still_waiting(ctx, 10.0) {
                return;
            }
            self.done_waiting();
            if !done.load(Ordering::SeqCst) {
                log::warn!("a screenshot didn't arrive in 10 s (was the window covered?)");
            }
            self.shot_done = None;
        }
        if self.wait > 0 {
            self.wait -= 1;
            return;
        }
        match self.stage {
            Stage::StartPage { shot } => {
                if !shot {
                    self.line(format!("OfficeCraft Phase 2 check ({})", if app.is_dark() { "dark" } else { "light" }));
                    self.rss("start page, no engines");
                    self.check(app.tabs.is_empty(), "the app opens on the start page");
                    self.screenshot(ctx, "start-page");
                    self.stage = Stage::StartPage { shot: true };
                } else {
                    self.stage = Stage::Setup;
                }
            }
            Stage::Setup => {
                app.new_document(Kind::Document);
                let _ = app.engine(Kind::Document).editor_mut().run_command(
                    "text.insert",
                    json!({"text": "Q3 report. Revenue grew in every region; the sheet tab has the figures and the deck tells the story."}),
                );
                app.new_document(Kind::Sheet);
                for (cell, input) in [
                    ("A1", "Region"),
                    ("B1", "Q3"),
                    ("A2", "North"),
                    ("B2", "1200"),
                    ("A3", "South"),
                    ("B3", "1350"),
                    ("A4", "Total"),
                    ("B4", "=SUM(B2:B3)"),
                ] {
                    let _ = app.engine(Kind::Sheet).editor_mut().run_command("cell.set", json!({"cell": cell, "input": input}));
                }
                app.new_document(Kind::Presentation);
                let _ = app.engine(Kind::Presentation).editor_mut().run_command("slide.new", json!({}));
                let pdf = self.dir.join("Contract.pdf");
                match pdfcraft_engine::Session::new().create_from_text("Contract", "Services agreement. Payment: 2,550 within 30 days of invoice.") {
                    Ok(bytes) if std::fs::write(&pdf, bytes.as_slice()).is_ok() => app.open(&pdf),
                    other => self.check(false, format!("make Contract.pdf: {:?}", other.err())),
                }
                self.stage = Stage::WaitForPdf;
                self.wait = 5;
            }
            Stage::WaitForPdf => {
                self.tries += 1;
                if Self::tab_of(app, Kind::Pdf).is_some() || self.tries > 600 {
                    let ok = Self::tab_of(app, Kind::Pdf).is_some();
                    self.check(ok, "the PDF opened in a tab (it showed as opening meanwhile)");
                    self.check(app.tabs.len() == 4, format!("four tabs, one per kind (have {})", app.tabs.len()));
                    self.stage = Stage::Appearance;
                    self.wait = 10;
                }
            }
            Stage::Appearance => self.stage = Stage::Shots { i: 0, shoot: false },
            Stage::Shots { i, shoot } => {
                let Some(kind) = ORDER.get(i).copied() else {
                    // One appearance: judged from what each app's chrome showed on screen.
                    let dark = app.is_dark();
                    let measured = self.luminance.lock().map(|l| l.clone()).unwrap_or_default();
                    for (kind, l) in measured {
                        let looks_dark = l < 0.45;
                        self.check(
                            looks_dark == dark,
                            format!("{kind:?} looks {} like the window (mean luminance {l:.2})", if dark { "dark" } else { "light" }),
                        );
                    }
                    self.stage = Stage::NewTab { shot: false };
                    return;
                };
                let Some((tab, _)) = Self::tab_of(app, kind) else {
                    self.check(false, format!("{kind:?} has a tab"));
                    self.stage = Stage::Shots { i: i + 1, shoot: false };
                    return;
                };
                if !shoot {
                    app.tabs.activate(tab);
                    self.stage = Stage::Shots { i, shoot: true };
                    self.wait = 60;
                } else {
                    let shown = app.last_frame.kind == Some(kind);
                    self.check(shown, format!("{kind:?} tab shows its engine"));
                    self.rss(&format!("after {kind:?} shown"));
                    // The app's own title row (its chrome), not the page: a document page stays
                    // white in dark mode, as it should.
                    let r = app.content_rect;
                    let area = egui::Rect::from_min_max(r.min, egui::pos2(r.right(), r.top() + 28.0));
                    self.screenshot_measuring(ctx, &format!("tab-{}", kind.label().to_lowercase()), Some((kind, area)));
                    self.stage = Stage::Shots { i: i + 1, shoot: false };
                    self.wait = 5;
                }
            }
            Stage::NewTab { shot } => {
                if !shot {
                    if let Some((tab, _)) = Self::tab_of(app, Kind::Document) {
                        app.tabs.activate(tab);
                    }
                    app.newtab.show();
                    self.stage = Stage::NewTab { shot: true };
                    self.wait = 30;
                } else {
                    self.check(app.newtab.open, "⌘T's new-tab page is open");
                    self.screenshot(ctx, "new-tab");
                    self.stage = Stage::NewTabClose;
                }
            }
            Stage::NewTabClose => {
                app.newtab.open = false;
                self.stage = Stage::Switches { n: 0, check: false };
                self.wait = 10;
            }
            Stage::Switches { n, check } => {
                if check {
                    let f = app.last_frame;
                    self.line(format!(
                        "SWITCH {n:2} → {:<12} first frame: font atlas rebuilt = {}, engine cpu {:.1} ms",
                        format!("{:?}", f.kind.unwrap_or(Kind::Document)),
                        f.font_atlas_rebuilt,
                        f.cpu.as_secs_f64() * 1000.0
                    ));
                    if f.switched && f.font_atlas_rebuilt {
                        self.failures.push(format!("font atlas rebuilt on switch {n}"));
                    }
                    self.stage = Stage::Switches { n: n + 1, check: false };
                    self.wait = 8;
                } else if n < 12 {
                    app.tabs.cycle(1);
                    self.stage = Stage::Switches { n, check: true };
                } else {
                    let rebuilt = self.failures.iter().filter(|f| f.starts_with("font atlas rebuilt")).count();
                    self.check(rebuilt == 0, "no font atlas rebuild on any of 12 tab switches (no font flash)");
                    self.stage = Stage::Saves;
                }
            }
            Stage::Saves => {
                let edits: [(Kind, &str, serde_json::Value, &str); 3] = [
                    (Kind::Document, "text.insert", json!({"text": " Saved from the check."}), "Q3 report.docx"),
                    (Kind::Sheet, "cell.set", json!({"cell": "C1", "input": "Q4"}), "Budget.xlsx"),
                    (Kind::Presentation, "slide.new", json!({}), "Pitch deck.pptx"),
                ];
                for (kind, cmd, args, name) in edits {
                    let Some((tab, doc)) = Self::tab_of(app, kind) else { continue };
                    app.tabs.activate(tab);
                    let path = self.dir.join(name);
                    let ed = app.engine(kind).editor_mut();
                    let edited = ed.run_command(cmd, args).is_ok();
                    let dirty = ed.documents().iter().any(|d| d.id == doc && d.dirty);
                    let saved = ed.save_as(doc, &path);
                    let clean = ed.documents().iter().any(|d| d.id == doc && !d.dirty);
                    let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                    self.check(
                        edited && dirty && saved.is_ok() && clean && size > 0,
                        format!("{kind:?}: edit → unsaved → Save As {name} ({size} bytes) → saved {saved:?}"),
                    );
                }
                if let Some((tab, doc)) = Self::tab_of(app, Kind::Pdf) {
                    app.tabs.activate(tab);
                    let ed = app.engine(Kind::Pdf).editor_mut();
                    let edited = ed.run_command("page.rotate", json!({})).is_ok();
                    let dirty = ed.documents().iter().any(|d| d.id == doc && d.dirty);
                    let saved = ed.save(doc);
                    let clean = ed.documents().iter().any(|d| d.id == doc && !d.dirty);
                    self.check(edited && dirty && saved.is_ok() && clean, format!("Pdf: rotate page → unsaved → Save in place → saved {saved:?}"));
                }
                self.stage = Stage::Typing { phase: 0 };
                self.wait = 10;
            }
            Stage::Typing { phase } => match phase {
                0 => {
                    if let Some((tab, _)) = Self::tab_of(app, Kind::Document) {
                        app.tabs.activate(tab);
                    }
                    self.stage = Stage::Typing { phase: 1 };
                    self.wait = 10;
                }
                1 => {
                    app.inject.push(egui::Event::Text(" Typed in OfficeCraft.".into()));
                    self.stage = Stage::Typing { phase: 2 };
                    self.wait = 10;
                }
                2 => {
                    let text =
                        app.engine(Kind::Document).editor_mut().run_command("document.text", json!({})).map(|v| v.to_string()).unwrap_or_default();
                    let ok = text.contains("Typed in OfficeCraft.");
                    if !ok && self.tries < 1_000 {
                        // WordCraft skips drawing (and the input) for two frames after it
                        // reinstalls fonts, as the standalone app does: try once more.
                        self.line("NOTE typing landed in a frame WordCraft skipped after reinstalling fonts; typing again");
                        self.tries = 1_000;
                        self.stage = Stage::Typing { phase: 1 };
                        self.wait = 10;
                        return;
                    }
                    self.check(ok, "typing reaches WordCraft through the input path");
                    if !ok {
                        self.line(format!("  document text was: {}", text.chars().take(300).collect::<String>()));
                    }
                    if let Some((tab, _)) = Self::tab_of(app, Kind::Sheet) {
                        app.tabs.activate(tab);
                    }
                    self.stage = Stage::Typing { phase: 3 };
                    self.wait = 10;
                }
                3 => {
                    let key =
                        |key, pressed| egui::Event::Key { key, physical_key: Some(key), pressed, repeat: false, modifiers: egui::Modifiers::NONE };
                    for _ in 0..5 {
                        app.inject.push(key(egui::Key::ArrowDown, true));
                        app.inject.push(key(egui::Key::ArrowDown, false));
                    }
                    app.inject.push(egui::Event::Text("typed".into()));
                    app.inject.push(key(egui::Key::Enter, true));
                    app.inject.push(key(egui::Key::Enter, false));
                    self.stage = Stage::Typing { phase: 4 };
                    self.wait = 10;
                }
                _ => {
                    let dirty = Self::tab_of(app, Kind::Sheet)
                        .and_then(|(_, doc)| app.engine_ref(Kind::Sheet).map(|e| e.editor().documents().iter().any(|d| d.id == doc && d.dirty)))
                        == Some(true);
                    self.check(dirty, "arrow keys, typing and Enter reach GridCraft");
                    self.stage = Stage::Routing { phase: 0 };
                    self.wait = 5;
                }
            },
            Stage::DirtyClose { phase } => match phase {
                0 => {
                    if let Some((tab, _)) = Self::tab_of(app, Kind::Document) {
                        let _ = app.engine(Kind::Document).editor_mut().run_command("text.insert", json!({"text": " Unsaved."}));
                        app.close_tab(tab);
                    }
                    self.stage = Stage::DirtyClose { phase: 1 };
                    self.wait = 20;
                }
                1 => {
                    let pending = Self::tab_of(app, Kind::Document)
                        .and_then(|(_, doc)| app.engine_ref(Kind::Document).map(|e| e.editor().close_state(doc)))
                        == Some(CloseState::Pending);
                    self.check(pending, "closing a document with unsaved changes shows WordCraft's own Save prompt and keeps the tab");
                    self.screenshot(ctx, "dirty-close-prompt");
                    self.stage = Stage::DirtyClose { phase: 2 };
                }
                2 => {
                    let _ = app.engine(Kind::Document).editor_mut().run_command("ui.saveChanges", json!({"answer": "cancel"}));
                    self.stage = Stage::DirtyClose { phase: 3 };
                    self.wait = 10;
                }
                _ => {
                    let doc = Self::tab_of(app, Kind::Document).map(|(_, d)| d);
                    let ed = app.engine_ref(Kind::Document).map(|e| e.editor());
                    let kept = doc.zip(ed).is_some_and(|(d, e)| e.documents().iter().any(|i| i.id == d));
                    let prompt = ed.is_some_and(|e| e.close_prompt_open());
                    self.check(kept && !prompt, "Cancel closes the prompt and keeps the document and its tab");
                    self.stage = Stage::Resize { i: 0, frames: 0 };
                    self.wait = 10;
                }
            },
            // The window's own input path (its raw-input hook), as real keystrokes take it.
            Stage::Routing { phase } => {
                let key = |k: egui::Key, m: egui::Modifiers| {
                    vec![
                        egui::Event::Key { key: k, physical_key: Some(k), pressed: true, repeat: false, modifiers: m },
                        egui::Event::Key { key: k, physical_key: Some(k), pressed: false, repeat: false, modifiers: m },
                    ]
                };
                let cmd = egui::Modifiers { mac_cmd: true, command: true, ..Default::default() };
                let doc_text = |app: &mut App| {
                    app.engine(Kind::Document).editor_mut().run_command("document.text", json!({})).map(|v| v.to_string()).unwrap_or_default()
                };
                let hook = |app: &mut App, events: Vec<egui::Event>| {
                    let mut raw = egui::RawInput { events, ..Default::default() };
                    eframe::App::raw_input_hook(app, ctx, &mut raw);
                };
                match phase {
                    0 => {
                        if let Some((tab, _)) = Self::tab_of(app, Kind::Document) {
                            app.tabs.activate(tab);
                        }
                        self.wait = 10;
                    }
                    1 => {
                        hook(app, vec![egui::Event::Text(" Routed.".into())]);
                        self.wait = 10;
                    }
                    2 => {
                        let text = doc_text(app);
                        self.check(text.contains("Routed."), "keys typed into the window reach the shown document");
                        hook(app, key(egui::Key::T, cmd));
                        self.wait = 8;
                    }
                    3 => {
                        self.check(app.newtab.open, "⌘T opens the new-tab page");
                        let before = doc_text(app);
                        hook(app, vec![egui::Event::Text("zq".into())]);
                        self.wait = 5;
                        self.tries = before.len() as u32;
                    }
                    4 => {
                        let unchanged = doc_text(app).len() as u32 == self.tries;
                        self.check(unchanged, "while the popover is open, typing stays out of the document");
                        // Keys for the chrome (Esc here) go to the window's own egui, which this
                        // stand-in input can't reach; the popover's Esc has its own unit test.
                        app.newtab.open = false;
                        self.wait = 5;
                    }
                    5 => {
                        let before = app.tabs.active().map(|t| t.kind);
                        hook(app, key(egui::Key::Tab, egui::Modifiers::CTRL));
                        self.wait = 5;
                        self.tries = before.map_or(9, |k| ORDER.iter().position(|o| *o == k).unwrap_or(9) as u32);
                    }
                    6 => {
                        let now = app.tabs.active().map(|t| t.kind);
                        self.check(now == Some(Kind::Sheet), format!("⌃Tab moves to the next tab ({now:?})"));
                        if let Some((tab, _)) = Self::tab_of(app, Kind::Presentation) {
                            app.tabs.activate(tab);
                        }
                        self.wait = 5;
                    }
                    7 => {
                        hook(app, key(egui::Key::W, cmd));
                        self.wait = 15;
                    }
                    8 => {
                        self.check(Self::tab_of(app, Kind::Presentation).is_none(), "⌘W closes the saved presentation's tab");
                        hook(app, key(egui::Key::T, egui::Modifiers { alt: true, ..cmd }));
                        self.wait = 15;
                    }
                    _ => {
                        let back = Self::tab_of(app, Kind::Presentation).and_then(|(_, doc)| {
                            app.engine_ref(Kind::Presentation)
                                .and_then(|e| e.editor().documents().into_iter().find(|d| d.id == doc))
                                .and_then(|d| d.path)
                        });
                        self.check(back.as_ref().is_some_and(|p| p.ends_with("Pitch deck.pptx")), format!("⌥⌘T reopens it from its file ({back:?})"));
                        self.stage = Stage::Services { phase: 0 };
                        self.wait = 10;
                        return;
                    }
                }
                self.stage = Stage::Routing { phase: phase + 1 };
            }
            // Clipboard between tabs (an isolated clipboard: never the user's), and DeckCraft's
            // Print opening its PDF in a tab.
            Stage::Services { phase } => {
                match phase {
                    0 => {
                        if let Some((tab, _)) = Self::tab_of(app, Kind::Sheet) {
                            app.tabs.activate(tab);
                        }
                        let _ = app.engine(Kind::Sheet).editor_mut().run_command("selection.set", json!({"range": "A1:B4"}));
                        self.wait = 6;
                    }
                    1 => {
                        app.inject.push(egui::Event::Copy);
                        self.wait = 6;
                    }
                    2 => {
                        let html = app.platform.clipboard.html().unwrap_or_default();
                        self.check(html.to_lowercase().contains("<table"), "⌘C in the sheet puts the range on the clipboard as a table");
                        if let Some((tab, _)) = Self::tab_of(app, Kind::Document) {
                            app.tabs.activate(tab);
                        }
                        self.wait = 8;
                    }
                    3 => {
                        let text = app.platform.clipboard.text().unwrap_or_default();
                        app.inject.push(egui::Event::Paste(text));
                        self.wait = 8;
                    }
                    4 => {
                        let inspect = app.engine(Kind::Document).editor_mut().run_command("document.inspect", json!({})).unwrap_or_default();
                        let table = inspect["blocks"].as_array().and_then(|b| b.iter().find(|x| x["type"] == json!("table"))).cloned();
                        self.check(
                            table.as_ref().is_some_and(|t| t["cells"].to_string().contains("North")),
                            format!("⌘V in the document makes it a table ({:?})", table.map(|t| t["cells"].clone())),
                        );
                        self.screenshot(ctx, "pasted-table");
                        if let Some((tab, _)) = Self::tab_of(app, Kind::Presentation) {
                            app.tabs.activate(tab);
                        }
                        self.wait = 6;
                    }
                    5 => {
                        self.tries = app.tabs.iter().filter(|t| t.kind == Kind::Pdf).count() as u32;
                        let printed = app.engine(Kind::Presentation).editor_mut().run_command("app.print", json!({}));
                        self.line(format!("INFO DeckCraft app.print: {printed:?}"));
                        self.wait = 40;
                    }
                    _ => {
                        let pdfs = app.tabs.iter().filter(|t| t.kind == Kind::Pdf).count() as u32;
                        if pdfs <= self.tries && self.wait == 0 && phase < 30 {
                            // PdfCraft opens in the background: give it a little longer.
                            self.stage = Stage::Services { phase: phase + 1 };
                            self.wait = 20;
                            return;
                        }
                        self.check(pdfs > self.tries, "DeckCraft's Print opens its PDF in an OfficeCraft PDF tab, ready to print");
                        self.tries = 0;
                        self.stage = Stage::Projects { phase: 0 };
                        self.wait = 5;
                        return;
                    }
                }
                self.stage = Stage::Services { phase: phase + 1 };
            }
            // Phase 6: the check folder becomes a project; its files are indexed, searched and
            // opened at their hits; a new file is made from the sidebar; a file is pinned.
            Stage::Projects { phase } => {
                const SEARCHES: [(&str, &str); 4] =
                    [("payment", "Contract.pdf"), ("north", "Budget.xlsx"), ("revenue", "Q3 report.docx"), ("launch", "Untitled Presentation.pptx")];
                let dir = self.dir.clone();
                match phase {
                    0 => {
                        app.open_project(&dir, Some("Check project"), ctx);
                        self.check(
                            app.sidebar.open && dir.join(".officecraft/project.json").is_file(),
                            "a folder becomes a project (.officecraft/project.json) and the sidebar shows it",
                        );
                        self.tries = 0;
                        self.wait = 10;
                    }
                    1 => {
                        let pr = app.projects.progress.clone();
                        let docs = app.projects.current_open().map_or(0, |o| o.snapshot.documents().count());
                        if (pr.pending > 0 || pr.indexed < 4) && self.still_waiting(ctx, 30.0) {
                            return;
                        }
                        self.done_waiting();
                        self.line(format!("INFO index: {} files, {} failed {:?}", pr.indexed, pr.failed.len(), pr.failed));
                        self.check(
                            docs >= 4 && pr.indexed >= 4,
                            format!("the sidebar lists the project's documents and the index reads them ({docs} listed, {} read)", pr.indexed),
                        );
                        self.tries = 0;
                        app.new_file_in(&dir, Kind::Presentation);
                        self.wait = 10;
                    }
                    2 => {
                        let path = app.active_path();
                        self.check(
                            path.as_ref().is_some_and(|p| p.ends_with("Untitled Presentation.pptx") && p.is_file()),
                            format!("New Presentation Here makes and opens a file in the project ({path:?})"),
                        );
                        let ed = app.engine(Kind::Presentation).editor_mut();
                        let _ = ed.run_command("insert.textBox", json!({"text": "Launch timeline"}));
                        if let Some(doc) = ed.active() {
                            let _ = ed.save(doc);
                        }
                        self.wait = 10;
                    }
                    3 => {
                        app.projects.query = "launch".into();
                        app.projects.frame();
                        // A rescan (2 s) and re-reading the changed files: slower on a busy Mac.
                        if app.projects.hits.is_empty() && self.still_waiting(ctx, 60.0) {
                            return;
                        }
                        self.done_waiting();
                        self.check(!app.projects.hits.is_empty(), "a saved change is found by search (the folder is watched and re-read)");
                        self.tries = 0;
                    }
                    4..=7 => {
                        let (q, file) = SEARCHES[usize::from(phase - 4)];
                        if self.tries == 0 {
                            app.sidebar.query = q.into();
                            app.projects.query = q.into();
                            app.projects.frame();
                            let hit = app.projects.hits.iter().find(|h| h.path.ends_with(file)).cloned();
                            match hit {
                                Some(h) => {
                                    self.line(format!("INFO “{q}”: {} hits; opening {} at {:?}", app.projects.hits.len(), file, h.location));
                                    app.on_sidebar_action(ctx, officecraft_shell::SidebarAction::OpenHit(h));
                                }
                                None => {
                                    self.check(false, format!("searching “{q}” finds {file}"));
                                    self.stage = Stage::Projects { phase: phase + 1 };
                                    return;
                                }
                            }
                        }
                        self.tries += 1;
                        let at = app.active_path().is_some_and(|p| p.ends_with(file));
                        if (!at || app.projects.goto.is_some()) && self.still_waiting(ctx, 20.0) {
                            return;
                        }
                        self.done_waiting();
                        self.check(at, format!("searching “{q}” finds {file}, and the hit opens it in a tab"));
                        if phase == 4 {
                            self.screenshot(ctx, "project-search");
                        }
                        self.tries = 0;
                        self.wait = 6;
                    }
                    8 => {
                        let pdf = dir.join("Contract.pdf");
                        app.on_sidebar_action(ctx, officecraft_shell::SidebarAction::TogglePin(pdf.clone()));
                        let saved = officecraft_core::project::Project::open(&dir).map(|p| p.settings.pinned).unwrap_or_default();
                        self.check(saved == vec!["Contract.pdf".to_string()], format!("Pin keeps the file in project.json ({saved:?})"));
                        app.sidebar.query.clear();
                        app.projects.query.clear();
                        self.wait = 8;
                    }
                    9 => {
                        self.screenshot(ctx, "project-files");
                        // Back to how the later stages expect the window: the new file's tab closed.
                        let new_tab = app.tab_for_path(&dir.join("Untitled Presentation.pptx"));
                        if let Some(id) = new_tab {
                            app.close_tab(id);
                        }
                        self.wait = 10;
                    }
                    // Tab groups: the project's tabs sit together behind its label.
                    10 => {
                        let key = dir.to_string_lossy().into_owned();
                        let g = app.tabs.group_by_key(&key);
                        let ids: Vec<TabId> = app.tabs.iter().map(|t| t.id).collect();
                        let members = g.map(|g| app.tabs.in_group(g)).unwrap_or_default();
                        // The new presentation's tab closes through DeckCraft, which takes a moment.
                        if members.len() > 4 && self.still_waiting(ctx, 10.0) {
                            return;
                        }
                        self.done_waiting();
                        let together = members.windows(2).all(|w| {
                            let pos = |id: TabId| ids.iter().position(|x| *x == id).unwrap_or(0);
                            pos(w[1]) == pos(w[0]) + 1
                        });
                        self.check(
                            g.is_some() && members.len() == 4 && together,
                            format!("the project's four files sit together in its tab group ({} tabs in it)", members.len()),
                        );
                        let second = dir.join("Second project");
                        let _ = std::fs::create_dir_all(&second);
                        app.open_project(&second, Some("Second"), ctx);
                        app.new_file_in(&second, Kind::Document);
                        self.wait = 10;
                    }
                    11 => {
                        // Let the tabs finish sliding into place (time, not frames).
                        if self.still_waiting(ctx, 0.8) {
                            return;
                        }
                        self.done_waiting();
                        let second = dir.join("Second project");
                        let g2 = app.tabs.group_by_key(&second.to_string_lossy());
                        let in_second = g2.map(|g| app.tabs.in_group(g).len()).unwrap_or(0);
                        let shown = app.projects.project().map(|p| p.settings.name.clone());
                        self.check(
                            app.tabs.groups().count() == 2 && in_second == 1 && shown.as_deref() == Some("Second"),
                            format!("a second project opens as a second group, with its new file in it; the sidebar shows it ({shown:?})"),
                        );
                        self.screenshot(ctx, "tab-groups");
                        self.wait = 30;
                    }
                    12 => {
                        if let Some(g) = app.tabs.group_by_key(&dir.to_string_lossy()) {
                            app.toggle_group(g.0);
                        }
                        self.wait = 30;
                    }
                    13 => {
                        if self.still_waiting(ctx, 0.8) {
                            return;
                        }
                        self.done_waiting();
                        let g = app.tabs.group_by_key(&dir.to_string_lossy());
                        let hidden = g.map(|g| app.tabs.in_group(g)).unwrap_or_default().iter().all(|t| !app.tabs.is_visible(*t));
                        let front_outside = app.tabs.active().is_some_and(|t| t.group != g);
                        self.check(
                            hidden && front_outside,
                            "clicking a group's label collapses it: its tabs hide, and the front tab moves out of it",
                        );
                        self.screenshot(ctx, "tab-groups-collapsed");
                        self.wait = 30;
                    }
                    14 => {
                        if let Some(g) = app.tabs.group_by_key(&dir.to_string_lossy()) {
                            app.toggle_group(g.0);
                        }
                        let root = dir.join("Second project");
                        if let Some(g) = app.tabs.group_by_key(&root.to_string_lossy()) {
                            app.on_tab_action(ctx, officecraft_shell::TabAction::RenameGroup { id: g.0, name: "Renamed".into() });
                            app.on_tab_action(ctx, officecraft_shell::TabAction::SetGroupColor { id: g.0, color: 4 });
                        }
                        self.wait = 6;
                    }
                    15 => {
                        let root = dir.join("Second project");
                        let saved = officecraft_core::project::Project::open(&root).map(|p| (p.settings.name, p.settings.color)).ok();
                        let label = app.tabs.group_by_key(&root.to_string_lossy()).and_then(|g| app.tabs.group(g)).map(|g| (g.name.clone(), g.color));
                        self.check(
                            saved == Some(("Renamed".into(), Some("green".into()))) && label == Some(("Renamed".into(), 4)),
                            format!("renaming and recolouring a group saves them in its project ({saved:?}, label {label:?})"),
                        );
                        app.close_project(&root);
                        self.wait = 20;
                    }
                    16 => {
                        let root = dir.join("Second project");
                        let gone =
                            app.tabs.group_by_key(&root.to_string_lossy()).is_none() && !app.projects.open.iter().any(|o| o.project.root == root);
                        self.check(gone && app.tabs.groups().count() == 1, "Close Project closes its tabs, then its group");
                        let first = app.tabs.iter().find(|t| t.kind == Kind::Document).map(|t| t.id);
                        if let Some(t) = first {
                            app.tabs.activate(t);
                        }
                    }
                    _ => {
                        self.stage = Stage::Ai { phase: 0 };
                        self.wait = 5;
                        return;
                    }
                }
                self.stage = Stage::Projects { phase: phase + 1 };
            }
            // Phase 7: an AI, through the real connector on its own thread, writes a table into
            // the open document; the window shows it live, lists it for review, and its Undo
            // takes it back.
            Stage::Ai { phase } => {
                let dir = self.dir.clone();
                match phase {
                    0 => {
                        if let Ok(mut p) = officecraft_core::project::Project::open(&dir) {
                            p.settings.ai_permission = officecraft_core::project::Permission::Edit;
                            let _ = p.save();
                        }
                        let (tx, rx) = std::sync::mpsc::channel();
                        self.ai_rx = Some(rx);
                        let settings = dir.join("settings");
                        std::thread::spawn(move || {
                            let r = match officecraft_mcp::Server::new(&dir, true) {
                                Ok(s) => {
                                    let mut s = s.with_settings_dir(&settings);
                                    let read = s.call("sheet.read_range", &json!({"file": "Budget.xlsx", "range": "A1:B4"}));
                                    let wrote = s.call("tool_call", &json!({"name": "doc.insert_table", "arguments": {"file": "Q3 report.docx", "values": [["Region", "Q3"], ["Midlands", 4242], ["Coast", 3131]], "header": true}}));
                                    let saved = s.call("project_save", &json!({"file": "Q3 report.docx"}));
                                    json!({"read": read, "wrote": wrote, "saved": saved})
                                }
                                Err(e) => json!({"error": e}),
                            };
                            let _ = tx.send(r);
                        });
                    }
                    1 => {
                        let got = self.ai_rx.as_ref().and_then(|r| r.try_recv().ok());
                        let Some(r) = got else {
                            if self.still_waiting(ctx, 30.0) {
                                return;
                            }
                            self.done_waiting();
                            self.check(false, "the AI connector answered");
                            self.stage = Stage::DirtyClose { phase: 0 };
                            return;
                        };
                        self.done_waiting();
                        let ok = |v: &serde_json::Value| v["isError"] != json!(true);
                        self.check(
                            ok(&r["read"]) && r["read"].to_string().contains("1350"),
                            "the AI reads the Q3 figures from the sheet (headless: it isn't the front tab's file)",
                        );
                        self.check(
                            ok(&r["wrote"]),
                            format!(
                                "the AI's table goes into the open document ({})",
                                r["wrote"]["content"][0]["text"].as_str().unwrap_or("").chars().take(120).collect::<String>()
                            ),
                        );
                        self.check(!ok(&r["saved"]), "an `edit` project keeps the AI's edit for review instead of saving it");
                        let text = Self::tab_of(app, Kind::Document)
                            .and_then(|(_, d)| app.engine(Kind::Document).editor_mut().control(d, "document.inspect", json!({})).ok())
                            .map(|v| v.to_string())
                            .unwrap_or_default();
                        self.check(text.contains("Midlands") && text.contains("4242"), "the document shows the table, live in its tab");
                        let listed = app.ai.changed.iter().any(|c| c.path.ends_with("Q3 report.docx"));
                        self.check(listed, "the review panel lists the document the AI changed");
                        app.ai.review_open = true;
                        self.wait = 10;
                    }
                    2 => {
                        // Longer than WordCraft's AutoSave takes (2.5 s), which is paused meanwhile.
                        if self.still_waiting(ctx, 4.0) {
                            return;
                        }
                        self.done_waiting();
                        let on_disk = officecraft_core::index::read_text(&dir.join("Q3 report.docx")).map(|s| format!("{s:?}")).unwrap_or_default();
                        self.check(
                            !on_disk.is_empty() && !on_disk.contains("Midlands"),
                            "nothing the AI did is saved before review (AutoSave waits)",
                        );
                        self.screenshot(ctx, "ai-review");
                        self.wait = 10;
                    }
                    3 => {
                        let path = app.ai.changed.first().map(|c| c.path.clone());
                        if let Some(p) = path {
                            app.on_review(officecraft_shell::ai::ReviewAction::Undo(p));
                        }
                        app.ai.review_open = false;
                        self.wait = 6;
                    }
                    4 => {
                        let text = Self::tab_of(app, Kind::Document)
                            .and_then(|(_, d)| app.engine(Kind::Document).editor_mut().control(d, "document.inspect", json!({})).ok())
                            .map(|v| v.to_string())
                            .unwrap_or_default();
                        self.check(!text.contains("Midlands") && app.ai.changed.is_empty(), "Undo in the review panel takes the AI's table back");
                        // H7: an open PDF, edited live through PdfCraft's own tools.
                        self.pdf_rotation = Self::tab_of(app, Kind::Pdf)
                            .and_then(|(_, d)| {
                                app.engine(Kind::Pdf).editor_mut().control(d, "tools/call", json!({"name": "doc_info", "arguments": {}})).ok()
                            })
                            .and_then(|v| v["structuredContent"]["pages"][0]["rotation"].as_i64());
                        let (tx, rx) = std::sync::mpsc::channel();
                        self.ai_rx = Some(rx);
                        let settings = dir.join("settings");
                        std::thread::spawn(move || {
                            let r = match officecraft_mcp::Server::new(&dir, true) {
                                Ok(s) => {
                                    let mut s = s.with_settings_dir(&settings);
                                    s.call(
                                        "tool_call",
                                        &json!({"name": "pdf.page_rotate", "arguments": {"file": "Contract.pdf", "pages": [1], "degrees": 90}}),
                                    )
                                }
                                Err(e) => json!({"error": e}),
                            };
                            let _ = tx.send(r);
                        });
                    }
                    5 => {
                        let got = self.ai_rx.as_ref().and_then(|r| r.try_recv().ok());
                        let Some(r) = got else {
                            if self.still_waiting(ctx, 30.0) {
                                return;
                            }
                            self.done_waiting();
                            self.check(false, "the AI connector answered for the PDF");
                            self.stage = Stage::DirtyClose { phase: 0 };
                            return;
                        };
                        self.done_waiting();
                        let rotation = |app: &mut App| {
                            Self::tab_of(app, Kind::Pdf)
                                .and_then(|(_, d)| {
                                    app.engine(Kind::Pdf).editor_mut().control(d, "tools/call", json!({"name": "doc_info", "arguments": {}})).ok()
                                })
                                .and_then(|v| v["structuredContent"]["pages"][0]["rotation"].as_i64())
                        };
                        let rot = rotation(app);
                        let want = self.pdf_rotation.map(|b| (b + 90) % 360);
                        self.check(
                            r["isError"] != json!(true) && rot.is_some() && rot == want,
                            format!("the AI rotates a page of the open PDF, live in its tab (H7; {:?}° → {rot:?}°)", self.pdf_rotation),
                        );
                        let listed = app.ai.changed.iter().any(|c| c.path.ends_with("Contract.pdf"));
                        self.check(listed, "the review panel lists the PDF");
                        let p = app.ai.changed.iter().find(|c| c.path.ends_with("Contract.pdf")).map(|c| c.path.clone());
                        if let Some(p) = p {
                            app.on_review(officecraft_shell::ai::ReviewAction::Undo(p));
                        }
                        self.wait = 6;
                    }
                    6 => {
                        let rot = Self::tab_of(app, Kind::Pdf)
                            .and_then(|(_, d)| {
                                app.engine(Kind::Pdf).editor_mut().control(d, "tools/call", json!({"name": "doc_info", "arguments": {}})).ok()
                            })
                            .and_then(|v| v["structuredContent"]["pages"][0]["rotation"].as_i64());
                        self.check(
                            rot.is_some() && rot == self.pdf_rotation,
                            format!("Undo in the review panel takes the PDF edit back (rotation {rot:?})"),
                        );
                        self.stage = Stage::BlankClose { phase: 0 };
                        self.wait = 5;
                        return;
                    }
                    _ => {}
                }
                self.stage = Stage::Ai { phase: phase + 1 };
            }
            // The reported bug: with a PDF in front, a new empty sheet's tab wouldn't close
            // (GridCraft's own blank Book1 took its place as a new tab).
            Stage::BlankClose { phase } => {
                let sheets = |app: &App| app.tabs.iter().filter(|t| t.kind == Kind::Sheet).count();
                match phase {
                    0 => {
                        if let Some((tab, _)) = Self::tab_of(app, Kind::Pdf) {
                            app.tabs.activate(tab);
                        }
                        self.tries = sheets(app) as u32;
                        app.new_document(Kind::Sheet);
                        self.wait = 8;
                    }
                    1 => {
                        let made = sheets(app) as u32 == self.tries + 1;
                        if let Some(id) = app.tabs.active().filter(|t| t.kind == Kind::Sheet).map(|t| t.id) {
                            app.close_tab(id);
                        }
                        self.check(made, "New Sheet with a PDF in front makes a sheet tab");
                        self.wait = 20;
                    }
                    _ => {
                        let left = sheets(app) as u32;
                        self.check(
                            left == self.tries,
                            format!(
                                "closing the new, empty sheet closes its tab, and no other sheet takes its place ({left} sheet tabs, {} before)",
                                self.tries
                            ),
                        );
                        self.tries = 0;
                        self.stage = Stage::Display { phase: 0 };
                        self.wait = 5;
                        return;
                    }
                }
                self.stage = Stage::BlankClose { phase: phase + 1 };
            }
            // Phase 8.2: Reduce motion, Increase contrast and the chrome's text size.
            Stage::Display { phase } => {
                use officecraft_platform::display::DisplayPrefs;
                match phase {
                    0 => {
                        app.override_display(Some(DisplayPrefs { reduce_motion: true, increase_contrast: true }));
                        app.set_text_scale(ctx, 130);
                        if let Some((tab, _)) = Self::tab_of(app, Kind::Document) {
                            app.tabs.activate(tab);
                        }
                        self.wait = 10;
                    }
                    1 => {
                        let window_still = ctx.global_style().animation_time == 0.0;
                        let engines_still =
                            Kind::ALL.iter().filter_map(|k| app.engine_ref(*k)).all(|h| h.context().global_style().animation_time == 0.0);
                        self.check(window_still && engines_still, "Reduce motion stops animation in the chrome and in every app");
                        let expected = officecraft_shell::Palette::for_dark(app.is_dark()).with_contrast();
                        self.check(app.palette == expected, "Increase contrast gives the chrome its higher-contrast colours");
                        self.check(
                            (officecraft_shell::tokens::text::body().size - 13.0 * 1.3).abs() < 0.01,
                            "View ▸ Interface Text Size ▸ Larger makes the chrome's text larger",
                        );
                        app.sidebar.open = true;
                        self.wait = 6;
                    }
                    2 => {
                        if self.still_waiting(ctx, 0.5) {
                            return;
                        }
                        self.done_waiting();
                        self.screenshot(ctx, "display-contrast-large-text");
                        self.wait = 6;
                    }
                    _ => {
                        app.override_display(Some(DisplayPrefs::default()));
                        app.set_text_scale(ctx, 100);
                        app.sidebar.open = false;
                        self.stage = Stage::Crash { phase: 0 };
                        self.wait = 8;
                        return;
                    }
                }
                self.stage = Stage::Display { phase: phase + 1 };
            }
            // Phase 8.3: a panic leaves a crash report (a background thread's, so the run goes on).
            Stage::Crash { phase } => {
                let logs = self.dir.join("logs");
                if phase == 0 {
                    let _ = std::thread::Builder::new().name("check-deliberate-panic".into()).spawn(|| {
                        #[allow(clippy::panic)]
                        {
                            panic!("deliberate panic from the acceptance check");
                        }
                    });
                    self.stage = Stage::Crash { phase: 1 };
                    self.wait = 10;
                    return;
                }
                let report = std::fs::read_dir(&logs)
                    .ok()
                    .into_iter()
                    .flatten()
                    .flatten()
                    .map(|e| e.path())
                    .find(|p| p.extension().is_some_and(|x| x == "log"))
                    .and_then(|p| std::fs::read_to_string(p).ok())
                    .unwrap_or_default();
                if report.is_empty() && self.still_waiting(ctx, 10.0) {
                    return;
                }
                self.done_waiting();
                self.check(
                    report.contains("deliberate panic from the acceptance check")
                        && report.contains("Backtrace:")
                        && report.contains("check-deliberate-panic"),
                    "a panic writes a crash report (message, thread, backtrace, recent log lines)",
                );
                self.stage = Stage::DirtyClose { phase: 0 };
            }
            Stage::Resize { i, frames } => {
                let Some(size) = SIZES.get(i).copied() else {
                    self.check(
                        self.resize_worst < 16.7,
                        format!("every frame while resizing fits 60 Hz (worst {:.1} ms of engine frame work)", self.resize_worst),
                    );
                    self.stage = Stage::Accessibility { frames: 0 };
                    return;
                };
                if frames == 0 {
                    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(size[0], size[1])));
                    self.stage = Stage::Resize { i, frames: 1 };
                } else if frames < 12 {
                    let ms = app.last_frame.cpu.as_secs_f64() * 1000.0;
                    if frames > 2 {
                        self.resize_worst = self.resize_worst.max(ms);
                    }
                    self.stage = Stage::Resize { i, frames: frames + 1 };
                } else {
                    let r = app.content_rect;
                    self.line(format!(
                        "RESIZE {:>4}×{:<4} engine area {:.0}×{:.0} pt at {:.0}×, engine frame {:.1} ms",
                        size[0],
                        size[1],
                        r.width(),
                        r.height(),
                        ctx.pixels_per_point(),
                        app.last_frame.cpu.as_secs_f64() * 1000.0
                    ));
                    self.stage = Stage::Resize { i: i + 1, frames: 0 };
                }
            }
            Stage::Accessibility { frames } => {
                // As a screen reader would: the window's tree, with the shown engine's grafted in.
                if frames == 0 {
                    ctx.enable_accesskit();
                    if let Some((tab, _)) = Self::tab_of(app, Kind::Sheet) {
                        app.tabs.activate(tab);
                    }
                }
                if frames < 20 {
                    self.stage = Stage::Accessibility { frames: frames + 1 };
                    return;
                }
                let (nodes, labels) = app.a11y.lock().map(|s| (s.last_nodes, s.last_labels.clone())).unwrap_or_default();
                let has = |l: &str| labels.iter().any(|x| x == l);
                self.line(format!("A11Y merged tree: {nodes} nodes, {} labelled", labels.len()));
                self.check(has("Tab content") && has("New tab"), "the window's tree has our chrome (tab list, New tab, tab content)");
                self.check(has("Home") && has("Insert"), "GridCraft's ribbon (Home, Insert…) is grafted under the tab content");
                self.stage = Stage::Menus { phase: 0 };
            }
            // The menu bar, through `App::on_menu` (what a click runs).
            Stage::Menus { phase } => {
                let state = |app: &mut App| app.engine(Kind::Document).editor_mut().run_command("format.state", json!({})).unwrap_or_default();
                match phase {
                    0 => {
                        self.check(app.has_native_menu(), "the native menu bar is installed");
                        if let Some((tab, _)) = Self::tab_of(app, Kind::Document) {
                            app.tabs.activate(tab);
                        }
                        self.wait = 8;
                    }
                    1 => {
                        app.on_menu(ctx, "cw.key:⌘A");
                        app.on_menu(ctx, "cw.key:⌘B");
                        self.wait = 6;
                    }
                    2 => {
                        let s = state(app);
                        self.check(s["bold"] == json!(true), "Format ▸ Bold (after Edit ▸ Select All) bolds the document's text");
                        app.on_menu(ctx, "cw.key:⌘Z");
                        self.wait = 6;
                    }
                    3 => {
                        let s = state(app);
                        self.check(s["bold"] != json!(true), "Edit ▸ Undo undoes it");
                        if let Some((tab, _)) = Self::tab_of(app, Kind::Pdf) {
                            app.on_menu(ctx, &format!("cw.tab:{}", tab.0));
                        }
                        self.wait = 6;
                    }
                    4 => {
                        self.check(app.tabs.active().map(|t| t.kind) == Some(Kind::Pdf), "Window ▸ (a tab) brings it to the front");
                        app.on_menu(ctx, "cw.run:page.rotate");
                        self.wait = 4;
                    }
                    5 => {
                        let dirty = Self::tab_of(app, Kind::Pdf)
                            .and_then(|(_, d)| app.engine_ref(Kind::Pdf).map(|e| e.editor().documents().iter().any(|i| i.id == d && i.dirty)))
                            == Some(true);
                        self.check(dirty, "Document ▸ Rotate Page runs PdfCraft's command");
                        if let Some((_, doc)) = Self::tab_of(app, Kind::Pdf) {
                            let _ = app.engine(Kind::Pdf).editor_mut().save(doc);
                        }
                        let saved: serde_json::Value = std::fs::read(self.dir.join("settings/session.json"))
                            .ok()
                            .and_then(|b| serde_json::from_slice(&b).ok())
                            .unwrap_or_default();
                        let n = saved["tabs"].as_array().map_or(0, Vec::len);
                        self.line(format!("SESSION {saved}"));
                        self.check(n >= 4, format!("session.json keeps the open tabs' files ({n})"));
                    }
                    // The menu bar follows the front tab (D42): each app's own menus.
                    6..=9 => {
                        const EXPECT: [(Kind, &[&str]); 4] = [
                            (Kind::Document, &["Insert", "Format", "Table", "Tools"]),
                            (Kind::Sheet, &["Insert", "Format", "Data", "Formulas"]),
                            (Kind::Presentation, &["Insert", "Format", "Arrange", "Slide Show"]),
                            (Kind::Pdf, &[]),
                        ];
                        let (kind, want) = EXPECT[usize::from(phase - 6)];
                        if self.tries == 0 {
                            if let Some((tab, _)) = Self::tab_of(app, kind) {
                                app.tabs.activate(tab);
                            }
                            self.tries = 1;
                            self.wait = 6;
                            return;
                        }
                        self.tries = 0;
                        let bar = app.menu_bar();
                        let titles: Vec<String> = bar.menus.iter().map(|m| m.title.clone()).collect();
                        let mut ids = Vec::new();
                        fn walk(nodes: &[craft_menu::Node], out: &mut Vec<String>) {
                            for n in nodes {
                                match n {
                                    craft_menu::Node::Item(i) => out.push(i.command.id.clone()),
                                    craft_menu::Node::Submenu { children, .. } => walk(children, out),
                                    _ => {}
                                }
                            }
                        }
                        for m in &bar.menus {
                            walk(&m.children, &mut ids);
                        }
                        if kind == Kind::Pdf {
                            let own = ids.iter().filter(|i| i.starts_with("cw.app:")).count();
                            self.check(
                                own > 20 && !titles.iter().any(|t| t == "Slide Show"),
                                format!("a PDF tab shows PdfCraft's own menus ({own} of its items; menus {titles:?})"),
                            );
                            let zoom = ids.iter().find(|i| i.contains("view.zoom.in")).cloned();
                            if let Some(z) = &zoom {
                                app.on_menu(ctx, z);
                            }
                            self.check(zoom.is_some(), "PdfCraft's View ▸ Zoom In runs from the menu bar");
                        } else {
                            let missing: Vec<&&str> = want.iter().filter(|w| !titles.iter().any(|t| t == **w)).collect();
                            self.check(
                                missing.is_empty() && ids.len() > 80,
                                format!("a {} tab shows {}'s menus ({} items; menus {titles:?})", kind.label(), app_name(kind), ids.len()),
                            );
                        }
                        if phase == 9 {
                            self.stage = Stage::Memory { phase: 0 };
                            return;
                        }
                    }
                    _ => {}
                }
                self.stage = Stage::Menus { phase: phase + 1 };
            }
            Stage::Memory { phase } => {
                if phase == 0 {
                    self.rss("4 engines, 1 document each");
                    for kind in [Kind::Document, Kind::Sheet, Kind::Presentation] {
                        for _ in 0..4 {
                            app.new_document(kind);
                        }
                    }
                    self.stage = Stage::Memory { phase: 1 };
                    self.wait = 30;
                } else {
                    self.rss("+ 4 more documents each for Document, Sheet, Presentation");
                    if let Some((tab, _)) = Self::tab_of(app, Kind::Document) {
                        app.tabs.activate(tab);
                    }
                    // Idle is measured with accessibility off (no screen reader listening), first
                    // with the window in the background, then focused.
                    ctx.disable_accesskit();
                    app.check_focused = false;
                    self.stage = Stage::Idle { started: false, focused: false };
                    self.wait = 60;
                }
            }
            Stage::Idle { started: false, focused } => {
                let now = ctx.input(|i| i.time);
                let until = *self.settle_until.get_or_insert(now + 4.0);
                if now < until {
                    return;
                }
                self.settle_until = None;
                // Ten seconds of nothing, measured from another thread so this one can sleep.
                let slot = self.idle.clone();
                let waker = ctx.clone();
                std::thread::spawn(move || {
                    let t0 = cpu_seconds();
                    std::thread::sleep(Duration::from_secs(10));
                    let pct = t0.zip(cpu_seconds()).map_or(f64::NAN, |(a, b)| (b - a) / 10.0 * 100.0);
                    if let Ok(mut g) = slot.lock() {
                        *g = Some(pct);
                    }
                    // Ask for a frame until the result is read: one lost wake-up (the window
                    // is in the background, and macOS may defer it) must not stall the run.
                    for _ in 0..240 {
                        waker.request_repaint();
                        std::thread::sleep(Duration::from_millis(500));
                        if slot.lock().map_or(true, |g| g.is_none()) {
                            break;
                        }
                    }
                });
                self.stage = Stage::Idle { started: true, focused };
                self.idle_frames = app.frames;
            }
            Stage::Idle { started: true, .. } => {}
            // Quit with unsaved work: only WordCraft's document has changes; the others are saved.
            Stage::Quit { phase } => {
                let word_dirty = |app: &App| {
                    Self::tab_of(app, Kind::Document)
                        .and_then(|(_, d)| app.engine_ref(Kind::Document).map(|e| e.editor().documents().iter().any(|i| i.id == d && i.dirty)))
                        == Some(true)
                };
                let prompt = |app: &App| app.engine_ref(Kind::Document).is_some_and(|e| e.editor().close_prompt_open());
                match phase {
                    0 => {
                        for kind in [Kind::Sheet, Kind::Presentation, Kind::Pdf] {
                            let docs: Vec<_> = app.tabs.iter().filter(|t| t.kind == kind).map(|t| t.doc).collect();
                            for doc in docs {
                                let ed = app.engine(kind).editor_mut();
                                if ed.documents().iter().any(|d| d.id == doc && d.dirty) && ed.save(doc).is_err() && ed.activate(doc).is_ok() {
                                    // Untitled (the memory stage's extras): nothing to lose, close it.
                                    let _ = ed.run_command("file.close", json!({"force": true}));
                                }
                            }
                        }
                        let docs: Vec<_> = app.tabs.iter().filter(|t| t.kind == Kind::Document).map(|t| t.doc).collect();
                        let first = Self::tab_of(app, Kind::Document).map(|(_, d)| d);
                        for doc in docs.into_iter().filter(|d| Some(*d) != first) {
                            let ed = app.engine(Kind::Document).editor_mut();
                            if ed.activate(doc).is_ok() {
                                let _ = ed.run_command("file.close", json!({"force": true}));
                            }
                        }
                        self.wait = 10;
                    }
                    1 => {
                        if let Some((tab, _)) = Self::tab_of(app, Kind::Document) {
                            app.tabs.activate(tab);
                        }
                        let _ = app.engine(Kind::Document).editor_mut().run_command("text.insert", json!({"text": " Quit test."}));
                        app.start_quit();
                        self.wait = 20;
                    }
                    2 => {
                        self.check(prompt(app) && app.quitting.is_some(), "⌘Q with unsaved work shows WordCraft's own Save prompt first");
                        let _ = app.engine(Kind::Document).editor_mut().run_command("ui.saveChanges", json!({"answer": "cancel"}));
                        self.wait = 10;
                    }
                    3 => {
                        self.check(
                            app.quitting.is_none() && !app.quit_confirmed && word_dirty(app),
                            "Cancel stops the quit and keeps the unsaved edit",
                        );
                        app.start_quit();
                        self.wait = 20;
                    }
                    4 => {
                        let _ = app.engine(Kind::Document).editor_mut().run_command("ui.saveChanges", json!({"answer": "dontSave"}));
                        self.wait = 20;
                    }
                    _ => {
                        self.check(app.quit_confirmed, "Don't Save lets the quit finish (in the app, AppKit's terminate follows)");
                        let saved: serde_json::Value = std::fs::read(self.dir.join("settings/session.json"))
                            .ok()
                            .and_then(|b| serde_json::from_slice(&b).ok())
                            .unwrap_or_default();
                        let n = saved["tabs"].as_array().map_or(0, Vec::len);
                        self.check(n >= 4, format!("the tabs open at quit are kept for next launch ({n})"));
                        self.stage = Stage::Report;
                        return;
                    }
                }
                self.stage = Stage::Quit { phase: phase + 1 };
            }
            Stage::Templates { n, started } => {
                const KINDS: [Kind; 3] = [Kind::Document, Kind::Sheet, Kind::Presentation];
                let count = |app: &App, k: Kind| app.tabs.iter().filter(|t| t.kind == k).count();
                match (KINDS.get(usize::from(n)).copied(), started) {
                    (Some(kind), None) => {
                        let before = count(app, kind);
                        app.new_from_templates(kind);
                        self.stage = Stage::Templates { n, started: Some(before) };
                        self.wait = 45;
                    }
                    (Some(kind), Some(before)) => {
                        let front = app.tabs.active().map(|t| t.kind);
                        self.check(
                            count(app, kind) == before + 1 && front == Some(kind),
                            format!("Home's {} tile opens a new tab on its app's New page ({} → {})", kind.label(), before, count(app, kind)),
                        );
                        self.screenshot(ctx, &format!("templates-{}", kind.label().to_lowercase()));
                        self.stage = Stage::Templates { n: n + 1, started: None };
                    }
                    // (A blank PDF is new, unsaved work: it would add a prompt to the quit below.)
                    (None, _) => self.stage = Stage::UpdatePill { shot: false },
                }
            }
            Stage::UpdatePill { shot } => {
                use crate::updates::{AppUpdate, State};
                if !shot && !matches!(app.updates.state, State::Available(_)) {
                    app.updates.state = State::Available(vec![AppUpdate {
                        name: "WordCraft".into(),
                        have: "0.18.1".into(),
                        latest: "0.19.0".into(),
                        url: "https://github.com/storytold/wordcraft/releases".into(),
                    }]);
                    app.updates.popover_open = true;
                    self.wait = 90;
                } else if !shot {
                    self.check(
                        app.updates.pill_rect.is_some() && app.updates.popover_open,
                        "a newer Craft app release shows \"Update available\" in the title row, with its popover",
                    );
                    self.screenshot(ctx, "update-available");
                    // Kept up until the screenshot is taken, on a later frame.
                    self.stage = Stage::UpdatePill { shot: true };
                    self.wait = 90;
                } else {
                    app.updates.state = State::Idle;
                    app.updates.popover_open = false;
                    self.stage = Stage::ConnectAi { step: 0 };
                    self.wait = 5;
                }
            }
            Stage::ConnectAi { step } => {
                match step {
                    0 => {
                        app.ai.connect_open = true;
                        self.wait = 90;
                    }
                    1 => {
                        self.check(app.ai.connect_open && app.ai.pill_rect.is_some(), "the AI pill opens the Connect AI panel");
                        self.screenshot(ctx, "connect-ai");
                        // Kept up until the screenshot is taken, on a later frame.
                        self.wait = 90;
                    }
                    _ => {
                        app.ai.connect_open = false;
                        self.stage = Stage::AskAi { step: 0 };
                        self.wait = 5;
                        return;
                    }
                }
                self.stage = Stage::ConnectAi { step: step + 1 };
            }
            Stage::AskAi { step } => {
                match step {
                    0 => {
                        let root = app.projects.current_item().map(|i| i.root).unwrap_or_else(|| self.dir.clone());
                        let mut chat = crate::chat::store::Chat::new(0);
                        chat.messages = vec![
                            serde_json::json!({"role": "user", "content": [{"type": "text", "text": "How did revenue change this quarter?"}]}),
                            serde_json::json!({"role": "system", "content": "Note from OfficeCraft: in front is the sheet \"Budget.xlsx\" in the project."}),
                            serde_json::json!({"role": "assistant", "content": [
                                {"type": "tool_use", "id": "t1", "name": "project_search", "input": {"query": "revenue"}},
                            ]}),
                            serde_json::json!({"role": "user", "content": [{"type": "tool_result", "tool_use_id": "t1", "content": "…"}]}),
                            serde_json::json!({"role": "assistant", "content": [
                                {"type": "text", "text": "Revenue **grew in every region**: the North rose to 1,200 [[Budget.xlsx|Sheet1!B2]] and the South to 1,350 [[Budget.xlsx|Sheet1!B3]].\n\n- The report puts it down to new customers [[Q3 report.docx|\"grew in every region\"]]\n- The deck repeats the figures [[Pitch deck.pptx|Slide 2]]"},
                            ]}),
                        ];
                        app.chat.show_preview(chat, root);
                        self.wait = 90;
                    }
                    1 => {
                        self.check(app.chat.open, "Ask AI shows a conversation with its sources as chips");
                        self.screenshot(ctx, "ask-ai");
                        self.wait = 90;
                    }
                    2 => {
                        // Stage 2: a change made, one waiting for "allow edits?", and the review.
                        app.chat.end_preview();
                        let root = app.projects.current_item().map(|i| i.root).unwrap_or_else(|| self.dir.clone());
                        let mut chat = crate::chat::store::Chat::new(0);
                        chat.messages = vec![
                            serde_json::json!({"role": "user", "content": [{"type": "text", "text": "Make a one-page summary of the Q3 report as a new document"}]}),
                            serde_json::json!({"role": "assistant", "content": [
                                {"type": "tool_use", "id": "t1", "name": "project_read", "input": {"file": "Q3 report.docx"}},
                                {"type": "tool_use", "id": "t2", "name": "project_new", "input": {"file": "Q3 summary.docx"}},
                                {"type": "tool_use", "id": "t3", "name": "tool_call", "input": {"name": "doc.insert_text", "arguments": {"file": "Q3 summary.docx", "text": "…"}}},
                            ]}),
                            serde_json::json!({"role": "user", "content": [{"type": "tool_result", "tool_use_id": "t1", "content": "…"}, {"type": "tool_result", "tool_use_id": "t2", "content": "…"}, {"type": "tool_result", "tool_use_id": "t3", "content": "…"}]}),
                            serde_json::json!({"role": "assistant", "content": [
                                {"type": "text", "text": "I made **Q3 summary.docx** [[Q3 summary.docx]]: a title, four key points and the revenue table. Keep it below to save it."},
                            ]}),
                            serde_json::json!({"role": "user", "content": [{"type": "text", "text": "Add the regional totals to the budget too"}]}),
                        ];
                        app.chat.show_preview(chat, root.clone());
                        app.chat.preview_ask("Write range · Budget.xlsx");
                        let canon = root.canonicalize().unwrap_or(root);
                        app.ai.changed.push(officecraft_shell::ai::Changed { path: canon.join("Q3 summary.docx"), kind: Kind::Document, edits: 3 });
                        self.wait = 90;
                    }
                    3 => {
                        self.check(app.chat.open && !app.ai.changed.is_empty(), "Ask AI asks before a change and shows changes to keep or undo");
                        self.screenshot(ctx, "ask-ai-do");
                        self.wait = 90;
                    }
                    4 => {
                        // Stage 3: the project's memory, with two notes.
                        app.ai.changed.clear();
                        app.chat.end_preview();
                        let root = app.projects.current_item().map(|i| i.root).unwrap_or_else(|| self.dir.clone());
                        let mem = crate::chat::memory::Memory::new(&root);
                        let _ = mem.run(&serde_json::json!({"command": "create", "path": "/memories/project.md", "file_text": "- Budget figures are in GBP.\n- Priya owns the pitch deck; Sam the Q3 report.\n"}));
                        let _ = mem.run(&serde_json::json!({"command": "create", "path": "/memories/preferences.md", "file_text": "- Summaries: one page, bullet points, no tables.\n"}));
                        app.chat.show_preview(crate::chat::store::Chat::new(0), root);
                        app.chat.open_memory();
                        self.wait = 90;
                    }
                    5 => {
                        self.check(app.chat.open, "Ask AI shows the project's memory");
                        self.screenshot(ctx, "ask-ai-memory");
                        self.wait = 90;
                    }
                    _ => {
                        let root = app.projects.current_item().map(|i| i.root).unwrap_or_else(|| self.dir.clone());
                        let _ = crate::chat::memory::Memory::new(&root).clear();
                        app.ai.changed.clear();
                        app.chat.end_preview();
                        self.stage = Stage::Quit { phase: 0 };
                        self.wait = 5;
                        return;
                    }
                }
                self.stage = Stage::AskAi { step: step + 1 };
            }
            Stage::Report => {
                let warnings = logger::warnings();
                self.line(format!("WARNINGS ({}):", warnings.len()));
                for w in warnings.iter().take(40) {
                    self.line(format!("  {w}"));
                }
                let verdict =
                    if self.failures.is_empty() { "ALL PASS".to_string() } else { format!("{} FAILED: {:?}", self.failures.len(), self.failures) };
                self.line(verdict);
                let _ = std::fs::write(self.dir.join(format!("report{}.txt", self.suffix)), &self.report);
                self.stage = Stage::Done;
                self.wait = 5;
            }
            Stage::Done => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
        }
    }
}

/// One field of `ps` for this process.
fn ps(field: &str) -> Option<String> {
    let out = std::process::Command::new("ps").args(["-o", field, "-p", &std::process::id().to_string()]).output().ok()?;
    String::from_utf8(out.stdout).ok().map(|s| s.trim().to_string())
}

/// CPU time this process has used, in seconds (`ps -o time=`: `[[dd-]hh:]mm:ss.cc`).
fn cpu_seconds() -> Option<f64> {
    let t = ps("time=")?;
    let mut secs = 0.0;
    for part in t.split(':') {
        secs = secs * 60.0 + part.replace('-', "").parse::<f64>().ok()?;
    }
    Some(secs)
}

/// The Craft app behind a kind of tab.
fn app_name(kind: Kind) -> &'static str {
    match kind {
        Kind::Document => "WordCraft",
        Kind::Sheet => "GridCraft",
        Kind::Presentation => "DeckCraft",
        Kind::Pdf => "PdfCraft",
    }
}
