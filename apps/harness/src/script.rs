//! `harness --shots DIR`: the Phase 1 acceptance run, scripted.
//!
//! 1. One tab of each kind: a document, a sheet and a presentation with a little content
//!    (made with each app's own commands), and a PDF made by PdfCraft's engine and opened.
//! 2. A screenshot of the window on each tab.
//! 3. Twelve tab switches, noting whether the font atlas was rebuilt on a switch (a flash).
//! 4. An edit and a save in every tab.
//! 5. Closing a document with unsaved changes: the app's own prompt (screenshot), then Cancel.
//! 6. Memory: the process's resident size at each stage, and each engine's font atlas.
//!
//! Everything goes to DIR/report.txt, then the window closes.

use crate::{Harness, logger};
use craftworks_core::{CloseState, DocId, Kind, TabId};
use serde_json::json;
use std::fmt::Write as _;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

const ORDER: [Kind; 4] = [Kind::Document, Kind::Sheet, Kind::Presentation, Kind::Pdf];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    Setup,
    WaitForPdf,
    Shots { i: usize, shoot: bool },
    Switches { n: usize, check: bool },
    Saves,
    Typing { phase: u8 },
    DirtyClose { phase: u8 },
    Memory { phase: u8 },
    Report,
    Done,
}

pub struct Script {
    dir: PathBuf,
    stage: Stage,
    wait: u32,
    tries: u32,
    shot_done: Option<Arc<AtomicBool>>,
    report: String,
    failures: Vec<String>,
}

impl Script {
    pub fn new(dir: PathBuf) -> Self {
        let _ = std::fs::create_dir_all(&dir);
        Script { dir, stage: Stage::Setup, wait: 0, tries: 0, shot_done: None, report: String::new(), failures: Vec::new() }
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
        let kb = std::process::Command::new("ps")
            .args(["-o", "rss=", "-p", &std::process::id().to_string()])
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .and_then(|s| s.trim().parse::<u64>().ok())
            .unwrap_or(0);
        self.line(format!("MEM {stage}: {:.0} MB resident", kb as f64 / 1024.0));
    }

    fn tab_of(h: &Harness, kind: Kind) -> Option<(TabId, DocId)> {
        h.tabs.iter().find(|t| t.kind == kind).map(|t| (t.id, t.doc))
    }

    fn screenshot(&mut self, ctx: &egui::Context, name: &str) {
        let path = self.dir.join(format!("{name}.png"));
        let done = Arc::new(AtomicBool::new(false));
        let flag = done.clone();
        ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::ScreenshotCallback::new(move |img| {
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

    /// One frame of the script. Runs before the harness syncs and draws.
    pub fn step(&mut self, h: &mut Harness, ctx: &egui::Context) {
        ctx.request_repaint();
        if let Some(done) = &self.shot_done {
            if !done.load(Ordering::SeqCst) {
                return;
            }
            self.shot_done = None;
        }
        if self.wait > 0 {
            self.wait -= 1;
            return;
        }
        match self.stage {
            Stage::Setup => {
                self.line("Craftworks Phase 1 harness run");
                self.rss("start, no engines");
                h.new_document(Kind::Document);
                let _ = h
                    .engine(Kind::Document)
                    .editor_mut()
                    .run_command("text.insert", json!({"text": "Craftworks harness: this is WordCraft in a tab."}));
                h.new_document(Kind::Sheet);
                for (cell, input) in [("A1", "Craftworks"), ("A2", "Q3"), ("B2", "1200"), ("A3", "Q4"), ("B3", "1350"), ("B4", "=SUM(B2:B3)")] {
                    let _ = h.engine(Kind::Sheet).editor_mut().run_command("cell.set", json!({"cell": cell, "input": input}));
                }
                h.new_document(Kind::Presentation);
                let _ = h.engine(Kind::Presentation).editor_mut().run_command("slide.new", json!({}));
                let pdf = self.dir.join("sample.pdf");
                match pdfcraft_engine::Session::new().create_from_text("Craftworks", "Craftworks harness: this is PdfCraft in a tab.") {
                    Ok(bytes) if std::fs::write(&pdf, bytes.as_slice()).is_ok() => h.open(&pdf),
                    other => self.check(false, format!("make sample.pdf: {:?}", other.err())),
                }
                self.stage = Stage::WaitForPdf;
                self.wait = 5;
            }
            Stage::WaitForPdf => {
                self.tries += 1;
                if Self::tab_of(h, Kind::Pdf).is_some() || self.tries > 600 {
                    let ok = Self::tab_of(h, Kind::Pdf).is_some();
                    self.check(ok, "the PDF opened in a tab");
                    self.check(h.tabs.len() == 4, format!("four tabs, one per kind (have {})", h.tabs.len()));
                    self.stage = Stage::Shots { i: 0, shoot: false };
                }
            }
            Stage::Shots { i, shoot } => {
                let Some(kind) = ORDER.get(i).copied() else {
                    self.stage = Stage::Switches { n: 0, check: false };
                    return;
                };
                let Some((tab, _)) = Self::tab_of(h, kind) else {
                    self.check(false, format!("{kind:?} has a tab"));
                    self.stage = Stage::Shots { i: i + 1, shoot: false };
                    return;
                };
                if !shoot {
                    h.tabs.activate(tab);
                    self.stage = Stage::Shots { i, shoot: true };
                    self.wait = 60;
                } else {
                    let shown = h.last_frame.kind == Some(kind);
                    self.check(shown, format!("{kind:?} tab shows its engine"));
                    self.rss(&format!("after {kind:?} shown"));
                    self.screenshot(ctx, &format!("tab-{}", kind.label().to_lowercase()));
                    self.stage = Stage::Shots { i: i + 1, shoot: false };
                    self.wait = 5;
                }
            }
            Stage::Switches { n, check } => {
                if check {
                    let f = h.last_frame;
                    self.line(format!(
                        "SWITCH {n:2} → {:<12} first frame: font atlas rebuilt = {}, cpu {:.1} ms",
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
                    h.tabs.cycle(1);
                    self.stage = Stage::Switches { n, check: true };
                } else {
                    let rebuilt = self.failures.iter().filter(|f| f.starts_with("font atlas rebuilt")).count();
                    self.check(rebuilt == 0, "no font atlas rebuild on any of 12 tab switches (no font flash)");
                    self.stage = Stage::Saves;
                }
            }
            Stage::Saves => {
                let edits: [(Kind, &str, serde_json::Value, &str); 3] = [
                    (Kind::Document, "text.insert", json!({"text": " Saved from the harness."}), "docx"),
                    (Kind::Sheet, "cell.set", json!({"cell": "C1", "input": "saved"}), "xlsx"),
                    (Kind::Presentation, "slide.new", json!({}), "pptx"),
                ];
                for (kind, cmd, args, ext) in edits {
                    let Some((tab, doc)) = Self::tab_of(h, kind) else { continue };
                    h.tabs.activate(tab);
                    let ed = h.engine(kind).editor_mut();
                    let edited = ed.run_command(cmd, args).is_ok();
                    let dirty = ed.documents().iter().any(|d| d.id == doc && d.dirty);
                    let path = self.dir.join(format!("saved.{ext}"));
                    let saved = ed.save_as(doc, &path);
                    let clean = ed.documents().iter().any(|d| d.id == doc && !d.dirty);
                    let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                    self.check(
                        edited && dirty && saved.is_ok() && clean && size > 0,
                        format!("{kind:?}: edit → dirty → Save As {} ({size} bytes) → clean {saved:?}", path.display()),
                    );
                }
                if let Some((tab, doc)) = Self::tab_of(h, Kind::Pdf) {
                    h.tabs.activate(tab);
                    let ed = h.engine(Kind::Pdf).editor_mut();
                    let edited = ed.run_command("page.rotate", json!({})).is_ok();
                    let dirty = ed.documents().iter().any(|d| d.id == doc && d.dirty);
                    let saved = ed.save(doc);
                    let clean = ed.documents().iter().any(|d| d.id == doc && !d.dirty);
                    self.check(edited && dirty && saved.is_ok() && clean, format!("Pdf: rotate page → dirty → Save in place → clean {saved:?}"));
                }
                self.stage = Stage::Typing { phase: 0 };
                self.wait = 10;
            }
            // Keystrokes through the same input path as the window's own: text into the
            // document at its caret; into the sheet, text then Enter in the selected cell.
            Stage::Typing { phase } => match phase {
                0 => {
                    if let Some((tab, _)) = Self::tab_of(h, Kind::Document) {
                        h.tabs.activate(tab);
                    }
                    self.stage = Stage::Typing { phase: 1 };
                    self.wait = 10;
                }
                1 => {
                    h.inject.push(egui::Event::Text(" Typed in the harness.".into()));
                    self.stage = Stage::Typing { phase: 2 };
                    self.wait = 10;
                }
                2 => {
                    let text =
                        h.engine(Kind::Document).editor_mut().run_command("document.text", json!({})).map(|v| v.to_string()).unwrap_or_default();
                    self.check(text.contains("Typed in the harness."), "typing reaches WordCraft through the input path");
                    if let Some((tab, _)) = Self::tab_of(h, Kind::Sheet) {
                        h.tabs.activate(tab);
                    }
                    self.stage = Stage::Typing { phase: 3 };
                    self.wait = 10;
                }
                3 => {
                    let key =
                        |key, pressed| egui::Event::Key { key, physical_key: Some(key), pressed, repeat: false, modifiers: egui::Modifiers::NONE };
                    // Down to A5 (below the little table), then type and commit.
                    for _ in 0..4 {
                        h.inject.push(key(egui::Key::ArrowDown, true));
                        h.inject.push(key(egui::Key::ArrowDown, false));
                    }
                    h.inject.push(egui::Event::Text("typed".into()));
                    h.inject.push(key(egui::Key::Enter, true));
                    h.inject.push(key(egui::Key::Enter, false));
                    self.stage = Stage::Typing { phase: 4 };
                    self.wait = 10;
                }
                _ => {
                    let dirty = Self::tab_of(h, Kind::Sheet)
                        .and_then(|(_, doc)| h.engine_ref(Kind::Sheet).map(|e| e.editor().documents().iter().any(|d| d.id == doc && d.dirty)))
                        == Some(true);
                    self.check(dirty, "arrow keys, typing and Enter reach GridCraft (the saved workbook is dirty again)");
                    self.screenshot(ctx, "typed-in-sheet");
                    self.stage = Stage::DirtyClose { phase: 0 };
                    self.wait = 5;
                }
            },
            Stage::DirtyClose { phase } => match phase {
                0 => {
                    if let Some((tab, _)) = Self::tab_of(h, Kind::Document) {
                        let _ = h.engine(Kind::Document).editor_mut().run_command("text.insert", json!({"text": " Unsaved."}));
                        h.close_tab(tab);
                    }
                    self.stage = Stage::DirtyClose { phase: 1 };
                    self.wait = 20;
                }
                1 => {
                    let pending = Self::tab_of(h, Kind::Document)
                        .and_then(|(_, doc)| h.engine_ref(Kind::Document).map(|e| e.editor().close_state(doc)))
                        == Some(CloseState::Pending);
                    self.check(pending, "closing a document with unsaved changes shows WordCraft's own Save prompt and keeps the tab");
                    self.screenshot(ctx, "dirty-close-prompt");
                    self.stage = Stage::DirtyClose { phase: 2 };
                }
                2 => {
                    let _ = h.engine(Kind::Document).editor_mut().run_command("ui.saveChanges", json!({"answer": "cancel"}));
                    self.stage = Stage::DirtyClose { phase: 3 };
                    self.wait = 10;
                }
                _ => {
                    let doc = Self::tab_of(h, Kind::Document).map(|(_, d)| d);
                    let ed = h.engine_ref(Kind::Document).map(|e| e.editor());
                    let info = doc.zip(ed).and_then(|(d, e)| e.documents().into_iter().find(|i| i.id == d));
                    let prompt = ed.is_some_and(|e| e.close_prompt_open());
                    self.check(info.is_some() && !prompt, "Cancel closes the prompt and keeps the document and its tab");
                    // The document was saved to a file, so WordCraft's AutoSave is on and may write
                    // the edit once the prompt is gone: dirty or not, the text must still be there.
                    let text =
                        h.engine(Kind::Document).editor_mut().run_command("document.text", json!({})).map(|v| v.to_string()).unwrap_or_default();
                    self.check(
                        text.contains("Unsaved."),
                        format!("the edit made before Cancel is still in the document (dirty now: {:?})", info.map(|i| i.dirty)),
                    );
                    self.stage = Stage::Memory { phase: 0 };
                }
            },
            Stage::Memory { phase } => {
                if phase == 0 {
                    self.rss("4 engines, 1 document each");
                    for kind in [Kind::Document, Kind::Sheet, Kind::Presentation] {
                        for _ in 0..4 {
                            h.new_document(kind);
                        }
                    }
                    self.stage = Stage::Memory { phase: 1 };
                    self.wait = 30;
                } else if phase <= 3 {
                    // Show one new document of each kind so its engine draws it.
                    if let Some(kind) = [Kind::Document, Kind::Sheet, Kind::Presentation].get(usize::from(phase - 1)).copied()
                        && let Some(tab) = h.tabs.iter().filter(|t| t.kind == kind).last().map(|t| t.id)
                    {
                        h.tabs.activate(tab);
                    }
                    self.stage = Stage::Memory { phase: phase + 1 };
                    self.wait = 30;
                } else {
                    self.rss("5 documents each for Document, Sheet, Presentation");
                    for kind in ORDER {
                        if let Some(e) = h.engine_ref(kind) {
                            let (rebuilds, size) = e.font_stats();
                            let mb = size.map_or(0.0, |[w, h]| (w * h * 4) as f64 / 1_048_576.0);
                            self.line(format!("FONTS {kind:?}: atlas {size:?} ≈ {mb:.1} MB RGBA, uploaded whole {rebuilds} time(s)"));
                        }
                    }
                    self.stage = Stage::Report;
                }
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
                let _ = std::fs::write(self.dir.join("report.txt"), &self.report);
                self.stage = Stage::Done;
                self.wait = 5;
            }
            Stage::Done => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
        }
    }
}
