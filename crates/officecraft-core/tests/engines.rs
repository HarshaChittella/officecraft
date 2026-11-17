//! Each engine adapter, driven the way the host drives it: frames with `logic` and `ui` in one
//! egui pass, documents made, edited with the app's own commands, saved, reopened and closed.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test helpers fail loudly

use officecraft_core::engines::{DeckEditor, PdfEditor, SheetEditor, WordEditor, new_editor};
use officecraft_core::{CloseState, DocId, Editor, Kind};
use serde_json::json;
use std::path::PathBuf;

/// A fresh directory for one test's files.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("officecraft-core-{}-{name}", std::process::id()));
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

fn info(ed: &dyn Editor, doc: DocId) -> officecraft_core::DocInfo {
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
    ("text.insert", json!({"text": "Hello from OfficeCraft"}))
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

/// New PDF ("+" held down): PdfCraft's Create ▸ Blank Page, one empty page in a new tab that
/// has no file yet.
#[test]
fn a_new_pdf_is_one_blank_unsaved_page() {
    let ctx = egui::Context::default();
    let mut ed = PdfEditor::new();
    frames(&ctx, &mut ed, 2);
    let doc = ed.new_document().expect("PdfCraft makes a blank PDF");
    frames(&ctx, &mut ed, 2);
    assert_eq!(ed.active(), Some(doc), "the new PDF is the one in front");
    let docs = ed.documents();
    assert_eq!(docs.len(), 1);
    assert_eq!(docs[0].id, doc);
    assert!(docs[0].path.is_none(), "it isn't saved anywhere until you save it");
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
    use officecraft_core::index::{Location, read_text};
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

/// A file saved again is read again, and its new words are found (the counts alone don't move).
#[test]
fn the_indexer_rereads_a_changed_file() {
    use officecraft_core::index::Indexer;
    use officecraft_core::tree::{Limits, scan};
    let dir = scratch("reread");
    let pdf = |text: &str| pdfcraft_engine::Session::new().create_from_text("Notes", text).unwrap();
    std::fs::write(dir.join("notes.pdf"), pdf("Osprey").as_slice()).unwrap();
    let ix = Indexer::start(|| {});
    let wait_for = |word: &str| {
        for _ in 0..500 {
            if !ix.search(word, 5).is_empty() {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        false
    };
    ix.update(scan(&dir, Limits::default()));
    assert!(wait_for("osprey"));
    let before = ix.progress().generation;
    std::thread::sleep(std::time::Duration::from_millis(20));
    std::fs::write(dir.join("notes.pdf"), pdf("Kestrel").as_slice()).unwrap();
    ix.update(scan(&dir, Limits::default()));
    assert!(wait_for("kestrel"));
    assert!(ix.search("osprey", 5).is_empty(), "the old text is gone");
    assert!(ix.progress().generation > before, "the generation moves even though the counts don't");
}

/// Closing a presentation that isn't DeckCraft's front one closes that one.
#[test]
fn closing_a_background_presentation_closes_it() {
    let dir = scratch("deck-bg-close");
    let ctx = egui::Context::default();
    let mut ed = new_editor(Kind::Presentation);
    frames(&ctx, ed.as_mut(), 2);
    let a = ed.new_document().unwrap();
    frames(&ctx, ed.as_mut(), 2);
    ed.run_command("insert.textBox", json!({"text": "One"})).unwrap();
    ed.save_as(a, &dir.join("a.pptx")).unwrap();
    let b = ed.new_document().unwrap();
    frames(&ctx, ed.as_mut(), 2);
    ed.save_as(b, &dir.join("b.pptx")).unwrap();
    frames(&ctx, ed.as_mut(), 2);
    ed.run_command("slide.go", json!({"index": 0})).unwrap();
    frames(&ctx, ed.as_mut(), 2);
    let state: Vec<_> = ed.documents().iter().map(|d| (d.id, d.dirty)).collect();
    assert!(state.iter().all(|(_, dirty)| !dirty), "both saved: {state:?}");
    ed.request_close(a).unwrap();
    frames(&ctx, ed.as_mut(), 3);
    assert_eq!(ed.close_state(a), CloseState::Closed, "documents left: {:?}", ed.documents().iter().map(|d| d.path.clone()).collect::<Vec<_>>());
}

/// A live request (Phase 7) edits the open document through the app's own headless backend,
/// and the app's Undo takes the edit back.
#[test]
fn live_requests_edit_the_open_document_and_undo() {
    let ctx = egui::Context::default();
    // WordCraft.
    let mut ed = new_editor(Kind::Document);
    frames(&ctx, ed.as_mut(), 2);
    let doc = ed.new_document().unwrap();
    frames(&ctx, ed.as_mut(), 2);
    ed.control(doc, "engine.execute", json!({"command": "text.insert", "params": {"text": "Live edit"}})).unwrap();
    frames(&ctx, ed.as_mut(), 1);
    let text = |ed: &mut Box<dyn Editor>| ed.control(doc, "document.inspect", json!({})).unwrap().to_string();
    assert!(text(&mut ed).contains("Live edit"));
    assert!(info(ed.as_ref(), doc).dirty);
    ed.run_command("edit.undo", json!({})).unwrap();
    frames(&ctx, ed.as_mut(), 1);
    assert!(!text(&mut ed).contains("Live edit"), "WordCraft's Undo takes the AI's edit back");

    // GridCraft.
    let mut ed = new_editor(Kind::Sheet);
    frames(&ctx, ed.as_mut(), 2);
    let doc = ed.new_document().unwrap();
    frames(&ctx, ed.as_mut(), 2);
    ed.control(doc, "engine.execute", json!({"command": "cell.set", "params": {"cell": "B2", "input": "1350"}})).unwrap();
    frames(&ctx, ed.as_mut(), 1);
    let read = ed.control(doc, "sheet.read", json!({"range": "B2"})).unwrap().to_string();
    assert!(read.contains("1350"), "{read}");
    ed.run_command("edit.undo", json!({})).unwrap();
    frames(&ctx, ed.as_mut(), 1);
    let read = ed.control(doc, "sheet.read", json!({"range": "B2"})).unwrap().to_string();
    assert!(!read.contains("1350"), "GridCraft's Undo takes it back: {read}");

    // DeckCraft.
    let mut ed = new_editor(Kind::Presentation);
    frames(&ctx, ed.as_mut(), 2);
    let doc = ed.new_document().unwrap();
    frames(&ctx, ed.as_mut(), 2);
    ed.control(doc, "engine.execute", json!({"command": "insert.textBox", "params": {"text": "From the AI"}})).unwrap();
    frames(&ctx, ed.as_mut(), 1);
    let inspect = ed.control(doc, "engine.execute", json!({"command": "slide.inspect", "params": {}})).unwrap().to_string();
    assert!(inspect.contains("From the AI"), "{inspect}");
    ed.run_command("edit.undo", json!({})).unwrap();
    frames(&ctx, ed.as_mut(), 1);
    let inspect = ed.control(doc, "engine.execute", json!({"command": "slide.inspect", "params": {}})).unwrap().to_string();
    assert!(!inspect.contains("From the AI"), "DeckCraft's Undo takes it back");
}

/// H7: a live request runs PdfCraft's own tool on the open PDF; the view's document changes
/// and PdfCraft's Undo takes it back.
#[test]
fn live_tool_calls_edit_the_open_pdf_and_undo() {
    let dir = scratch("pdf-live");
    let path = dir.join("live.pdf");
    let bytes = pdfcraft_engine::Session::new().create_from_text("Live", "Page one").unwrap();
    std::fs::write(&path, bytes.as_slice()).unwrap();
    let ctx = egui::Context::default();
    let mut ed = PdfEditor::new();
    let doc = open_pdf(&ctx, &mut ed, &path);
    let rotation = |ed: &mut PdfEditor| {
        let r = ed.control(doc, "tools/call", json!({"name": "doc_info", "arguments": {}})).unwrap();
        r["structuredContent"]["pages"][0]["rotation"].as_i64()
    };
    assert_eq!(rotation(&mut ed), Some(0));
    let r = ed.control(doc, "tools/call", json!({"name": "page_rotate", "arguments": {"pages": [1], "degrees": 90}})).unwrap();
    assert_ne!(r["isError"], json!(true), "{r}");
    frames(&ctx, &mut ed, 2);
    assert_eq!(rotation(&mut ed), Some(90), "the open document is rotated");
    assert!(info(&ed, doc).dirty, "and has unsaved changes in its tab");
    ed.run_command("edit.undo", json!({})).unwrap();
    frames(&ctx, &mut ed, 2);
    assert_eq!(rotation(&mut ed), Some(0), "PdfCraft's Undo takes the AI's edit back");
    assert!(ed.control(doc, "document.inspect", json!({})).is_err(), "PdfCraft answers tools/call only");
}

/// A new, empty document closes at once, and no other document turns up in its place: the
/// blank one an app makes for itself at start never becomes a tab (the "sheet that won't
/// close": GridCraft's own Book1 took the closed sheet's place).
#[test]
fn a_new_empty_document_closes_and_nothing_takes_its_place() {
    for kind in [Kind::Sheet, Kind::Document, Kind::Presentation] {
        let ctx = egui::Context::default();
        let mut ed = new_editor(kind);
        frames(&ctx, ed.as_mut(), 2);
        let at_start: Vec<_> = ed.documents().iter().map(|d| d.title.clone()).collect();
        let doc = ed.new_document().unwrap();
        frames(&ctx, ed.as_mut(), 2);
        ed.request_close(doc).unwrap();
        frames(&ctx, ed.as_mut(), 2);
        let after: Vec<_> = ed.documents().iter().map(|d| d.title.clone()).collect();
        assert_eq!(ed.close_state(doc), CloseState::Closed, "{kind:?}: still open, prompt {}", ed.close_prompt_open());
        assert!(after.is_empty(), "{kind:?}: {after:?} appeared in its place (open at start: {at_start:?})");
    }
}
