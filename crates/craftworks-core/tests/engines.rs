//! Each engine adapter, driven the way the host drives it: frames with `logic` and `ui` in one
//! egui pass, documents made, edited with the app's own commands, saved, reopened and closed.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test helpers fail loudly

use craftworks_core::engines::{DeckEditor, PdfEditor, SheetEditor, WordEditor, new_editor};
use craftworks_core::{CloseState, DocId, Editor, Kind};
use serde_json::json;
use std::path::PathBuf;

/// A fresh directory for one test's files.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("craftworks-core-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// `n` frames, as the host runs them (input hook, then one pass with `logic` and `ui`).
fn frames(ctx: &egui::Context, ed: &mut dyn Editor, n: usize) {
    for _ in 0..n {
        let mut input =
            egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0))), ..Default::default() };
        ed.raw_input_hook(ctx, &mut input);
        let mut out = ctx.run_ui(input, |ui| {
            ed.logic(ui.ctx());
            ed.ui(ui);
        });
        out.textures_delta.clear();
    }
}

fn info(ed: &dyn Editor, doc: DocId) -> craftworks_core::DocInfo {
    ed.documents().into_iter().find(|d| d.id == doc).unwrap()
}

/// New → edit (dirty) → Save As (clean, file written) → reopen in a fresh engine → close.
fn round_trip(kind: Kind, ext: &str, edit: (&str, serde_json::Value)) {
    let dir = scratch(ext);
    let ctx = egui::Context::default();
    let mut ed = new_editor(kind);
    assert_eq!(ed.kind(), kind);
    frames(&ctx, ed.as_mut(), 3);

    let doc = ed.new_document().unwrap();
    assert_eq!(ed.active(), Some(doc));
    frames(&ctx, ed.as_mut(), 2);
    ed.run_command(edit.0, edit.1.clone()).unwrap();
    frames(&ctx, ed.as_mut(), 1);
    assert!(info(ed.as_ref(), doc).dirty, "{kind:?}: an edit makes the document dirty");
    assert!(ed.save(doc).is_err(), "{kind:?}: an untitled document needs Save As");

    let path = dir.join(format!("saved.{ext}"));
    ed.save_as(doc, &path).unwrap();
    frames(&ctx, ed.as_mut(), 1);
    let saved = info(ed.as_ref(), doc);
    assert!(!saved.dirty, "{kind:?}: clean after saving");
    assert_eq!(saved.path.as_deref(), Some(path.as_path()));
    assert!(std::fs::metadata(&path).unwrap().len() > 0, "{kind:?}: the file is written");

    // Saving again in place works now that it has a file.
    ed.run_command(edit.0, edit.1).unwrap();
    ed.save(doc).unwrap();
    assert!(!info(ed.as_ref(), doc).dirty);

    // A fresh engine opens it.
    let ctx2 = egui::Context::default();
    let mut other = new_editor(kind);
    frames(&ctx2, other.as_mut(), 2);
    let opened = other.open(&path).unwrap().now().unwrap();
    frames(&ctx2, other.as_mut(), 2);
    let o = info(other.as_ref(), opened);
    assert_eq!(o.path.as_deref(), Some(path.as_path()), "{kind:?}: reopened from its file");
    assert!(!o.dirty);

    // A clean document closes without asking.
    other.request_close(opened).unwrap();
    frames(&ctx2, other.as_mut(), 2);
    assert_eq!(other.close_state(opened), CloseState::Closed, "{kind:?}");
}

/// A dirty document's close asks first: the app's own prompt stays open, nothing is lost.
fn dirty_close_asks(kind: Kind, edit: (&str, serde_json::Value)) {
    let ctx = egui::Context::default();
    let mut ed = new_editor(kind);
    frames(&ctx, ed.as_mut(), 2);
    let doc = ed.new_document().unwrap();
    frames(&ctx, ed.as_mut(), 2);
    ed.run_command(edit.0, edit.1).unwrap();
    frames(&ctx, ed.as_mut(), 1);
    ed.request_close(doc).unwrap();
    frames(&ctx, ed.as_mut(), 2);
    assert_eq!(ed.close_state(doc), CloseState::Pending, "{kind:?}: the Save prompt is up");
    assert!(info(ed.as_ref(), doc).dirty, "{kind:?}: the edit is still there");
}

fn word_edit() -> (&'static str, serde_json::Value) {
    ("text.insert", json!({"text": "Hello from Craftworks"}))
}
fn sheet_edit() -> (&'static str, serde_json::Value) {
    ("cell.set", json!({"cell": "A1", "input": "42"}))
}
fn deck_edit() -> (&'static str, serde_json::Value) {
    ("slide.new", json!({}))
}

#[test]
fn documents_round_trip() {
    round_trip(Kind::Document, "docx", word_edit());
}

#[test]
fn sheets_round_trip() {
    round_trip(Kind::Sheet, "xlsx", sheet_edit());
}

#[test]
fn presentations_round_trip() {
    round_trip(Kind::Presentation, "pptx", deck_edit());
}

#[test]
fn closing_a_dirty_document_asks() {
    dirty_close_asks(Kind::Document, word_edit());
}

#[test]
fn closing_a_dirty_sheet_asks() {
    dirty_close_asks(Kind::Sheet, sheet_edit());
}

#[test]
fn closing_a_dirty_presentation_asks() {
    dirty_close_asks(Kind::Presentation, deck_edit());
}

#[test]
fn switching_documents_shows_the_one_asked_for() {
    let ctx = egui::Context::default();
    let mut eds: Vec<Box<dyn Editor>> = vec![Box::new(WordEditor::new()), Box::new(SheetEditor::new()), Box::new(DeckEditor::new())];
    for ed in &mut eds {
        let kind = ed.kind();
        frames(&ctx, ed.as_mut(), 2);
        let a = ed.new_document().unwrap();
        let b = ed.new_document().unwrap();
        assert_ne!(a, b, "{kind:?}");
        frames(&ctx, ed.as_mut(), 2);
        assert_eq!(ed.active(), Some(b));
        ed.activate(a).unwrap();
        frames(&ctx, ed.as_mut(), 2);
        assert_eq!(ed.active(), Some(a), "{kind:?}: the engine shows the document the tab picked");
        assert!(ed.activate(DocId(u64::MAX)).is_err(), "{kind:?}: an unknown document is an error");
    }
}

/// Opens `path` and waits (up to ~5 s of frames) for its tab.
fn open_pdf(ctx: &egui::Context, ed: &mut PdfEditor, path: &std::path::Path) -> DocId {
    if let Some(d) = ed.open(path).unwrap().now() {
        return d;
    }
    for _ in 0..500 {
        frames(ctx, ed, 1);
        if let Some(d) = ed.find_path(path) {
            return d;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("{} never opened", path.display());
}

#[test]
fn pdfs_open_switch_save_and_close() {
    let dir = scratch("pdf");
    let make = |name: &str, text: &str| {
        let bytes = pdfcraft_engine::Session::new().create_from_text("t", text).unwrap();
        let p = dir.join(name);
        std::fs::write(&p, bytes.as_slice()).unwrap();
        p
    };
    let (a_path, b_path) = (make("a.pdf", "First"), make("b.pdf", "Second"));
    let ctx = egui::Context::default();
    let mut ed = PdfEditor::new();
    frames(&ctx, &mut ed, 2);
    assert!(ed.new_document().is_err(), "a PDF is opened, not made");

    // PdfCraft reads files on a worker: the tabs appear on later frames.
    let a = open_pdf(&ctx, &mut ed, &a_path);
    let b = open_pdf(&ctx, &mut ed, &b_path);
    assert_ne!(a, b);
    assert_eq!(ed.active(), Some(b));
    assert_eq!(open_pdf(&ctx, &mut ed, &a_path), a, "opening it again shows its tab");
    ed.activate(b).unwrap();
    frames(&ctx, &mut ed, 2);
    assert_eq!(ed.active(), Some(b));

    ed.run_command("page.rotate", json!({})).unwrap();
    frames(&ctx, &mut ed, 1);
    assert!(info(&ed, b).dirty, "rotating a page is an unsaved change");
    frames(&ctx, &mut ed, 1);
    ed.save(b).unwrap();
    frames(&ctx, &mut ed, 1);
    assert!(!info(&ed, b).dirty);

    ed.request_close(a).unwrap();
    frames(&ctx, &mut ed, 2);
    assert_eq!(ed.close_state(a), CloseState::Closed);
    assert_eq!(ed.documents().len(), 1);
}

/// One appearance across tabs: every engine draws dark or light as the window asks.
#[test]
fn every_engine_follows_the_windows_appearance() {
    for kind in Kind::ALL {
        let ctx = egui::Context::default();
        let mut ed = new_editor(kind);
        frames(&ctx, ed.as_mut(), 3);
        for dark in [true, false, true] {
            ed.set_dark(dark);
            frames(&ctx, ed.as_mut(), 3);
            assert_eq!(ctx.global_style().visuals.dark_mode, dark, "{kind:?} asked for dark = {dark}");
        }
    }
}

/// A frame with `events`, from a focused window.
fn frames_with(ctx: &egui::Context, ed: &mut dyn Editor, events: Vec<egui::Event>) {
    let mut input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0))),
        focused: true,
        events,
        ..Default::default()
    };
    ed.raw_input_hook(ctx, &mut input);
    let mut out = ctx.run_ui(input, |ui| {
        ed.logic(ui.ctx());
        ed.ui(ui);
    });
    out.textures_delta.clear();
}

/// Typed text reaches WordCraft's document after it has been hidden (logic-only passes, D4).
#[test]
fn typing_reaches_the_document_after_logic_only_passes() {
    let ctx = egui::Context::default();
    let mut ed = new_editor(Kind::Document);
    frames(&ctx, ed.as_mut(), 3);
    ed.new_document().unwrap();
    frames(&ctx, ed.as_mut(), 3);
    for _ in 0..5 {
        let raw = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0))), ..Default::default() };
        let _ = ctx.run_logic(&raw, |ctx| ed.logic(ctx));
    }
    frames(&ctx, ed.as_mut(), 10);
    frames_with(&ctx, ed.as_mut(), vec![egui::Event::Text(" Typed.".into())]);
    frames(&ctx, ed.as_mut(), 2);
    let text = ed.run_command("document.text", json!({})).unwrap().to_string();
    assert!(text.contains("Typed."), "{text}");
}

/// Typed text reaches WordCraft's document, also after a theme change and a Save As.
#[test]
fn typing_reaches_the_document() {
    let dir = scratch("typing");
    for (label, dark, save) in
        [("plain", None, false), ("after set_dark", Some(false), false), ("after Save As", None, true), ("both", Some(false), true)]
    {
        let ctx = egui::Context::default();
        let mut ed = new_editor(Kind::Document);
        if let Some(d) = dark {
            ed.set_dark(d);
        }
        frames(&ctx, ed.as_mut(), 3);
        let doc = ed.new_document().unwrap();
        frames(&ctx, ed.as_mut(), 3);
        ed.run_command("text.insert", json!({"text": "Hello."})).unwrap();
        if save {
            ed.save_as(doc, &dir.join(format!("{}.docx", label.replace(' ', "-")))).unwrap();
        }
        frames(&ctx, ed.as_mut(), 10);
        frames_with(&ctx, ed.as_mut(), vec![egui::Event::Text(" Typed.".into())]);
        frames(&ctx, ed.as_mut(), 2);
        let text = ed.run_command("document.text", json!({})).unwrap().to_string();
        assert!(text.contains("Typed."), "{label}: {text}");
    }
}

/// The project index reads one file of each kind with its app's own reader.
#[test]
fn the_index_reads_every_kind() {
    use craftworks_core::index::{Location, read_text};
    let dir = scratch("index");
    let ctx = egui::Context::default();
    let make = |kind: Kind, name: &str, edits: Vec<(&str, serde_json::Value)>| {
        let mut ed = new_editor(kind);
        frames(&ctx, ed.as_mut(), 2);
        let doc = ed.new_document().unwrap();
        frames(&ctx, ed.as_mut(), 2);
        for (id, args) in edits {
            ed.run_command(id, args).unwrap();
            frames(&ctx, ed.as_mut(), 1);
        }
        ed.save_as(doc, &dir.join(name)).unwrap();
        frames(&ctx, ed.as_mut(), 2);
    };
    make(Kind::Document, "memo.docx", vec![("text.insert", json!({"text": "Quarterly heron count"}))]);
    make(Kind::Sheet, "birds.xlsx", vec![("cell.set", json!({"cell": "B3", "input": "Kingfisher"}))]);
    make(Kind::Presentation, "talk.pptx", vec![("slide.new", json!({})), ("insert.textBox", json!({"text": "Wetland survey"}))]);
    let pdf = pdfcraft_engine::Session::new().create_from_text("Notes", "Osprey nests by the river").unwrap();
    std::fs::write(dir.join("notes.pdf"), pdf.as_slice()).unwrap();

    let find = |name: &str, word: &str| -> Location {
        let sections = read_text(&dir.join(name)).unwrap_or_else(|e| panic!("{name}: {e}"));
        sections.into_iter().find(|(_, t)| t.contains(word)).unwrap_or_else(|| panic!("{name}: no {word:?}")).0
    };
    assert!(matches!(find("memo.docx", "heron"), Location::Paragraph(_)));
    assert_eq!(find("birds.xlsx", "Kingfisher"), Location::Cell { sheet: "Sheet1".into(), cell: "B3".into() });
    assert!(matches!(find("talk.pptx", "Wetland"), Location::Slide { notes: false, .. }));
    assert_eq!(find("notes.pdf", "Osprey"), Location::Page(0));
    assert!(read_text(&dir.join("missing.docx")).is_err());
}
