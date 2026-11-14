//! Copy in one tab, paste in another, formatting kept (Phase 5's acceptance): through the real
//! engines, their services and an in-process clipboard (never the user's).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use craftworks_core::Kind;
use craftworks_platform::{MemoryClipboard, Platform, editor};
use craftworks_render::{EngineHost, embed};
use serde_json::json;
use std::sync::Arc;

struct Tab {
    host: EngineHost,
}

impl Tab {
    fn new(kind: Kind, platform: &Platform) -> Tab {
        let mut host = EngineHost::headless(editor(kind, platform));
        host.editor_mut().set_dark(false);
        let mut t = Tab { host };
        t.frames(3);
        t.host.editor_mut().new_document().unwrap();
        t.frames(5);
        t
    }

    fn send(&mut self, events: Vec<egui::Event>) {
        let area = egui::Rect::from_min_size(egui::pos2(0.0, 32.0), egui::vec2(1400.0, 860.0));
        let window = egui::RawInput { focused: true, ..Default::default() };
        let mut input = embed::input_for_area(&window, area, 2.0, None, |_| true);
        input.viewports.values_mut().for_each(|v| v.focused = Some(true));
        input.events = events;
        let out = self.host.frame_headless(input);
        // Plain-text copies egui makes itself (no rich service) would reach the window here.
        let _ = out;
    }

    fn frames(&mut self, n: usize) {
        for _ in 0..n {
            self.send(vec![]);
        }
    }

    fn run(&mut self, id: &str, args: serde_json::Value) -> serde_json::Value {
        self.host.editor_mut().run_command(id, args).unwrap_or_else(|e| panic!("{id}: {e}"))
    }
}

fn key(k: egui::Key, m: egui::Modifiers) -> Vec<egui::Event> {
    vec![
        egui::Event::Key { key: k, physical_key: Some(k), pressed: true, repeat: false, modifiers: m },
        egui::Event::Key { key: k, physical_key: Some(k), pressed: false, repeat: false, modifiers: m },
    ]
}

const CMD: egui::Modifiers = egui::Modifiers { alt: false, ctrl: false, shift: false, mac_cmd: true, command: true };

/// A range copied in a sheet pastes into a document as a table.
#[test]
fn a_sheet_range_pastes_into_a_document_as_a_table() {
    let clipboard = Arc::new(MemoryClipboard::default());
    let platform = Platform::isolated(clipboard.clone());

    let mut sheet = Tab::new(Kind::Sheet, &platform);
    for (cell, v) in [("A1", "Region"), ("B1", "Q3"), ("A2", "North"), ("B2", "1200"), ("A3", "South"), ("B3", "1350")] {
        sheet.run("cell.set", json!({"cell": cell, "input": v}));
    }
    sheet.run("selection.set", json!({"range": "A1:B3"}));
    sheet.frames(2);
    sheet.send(vec![egui::Event::Copy]);
    sheet.frames(2);
    let copied = clipboard.contents();
    assert!(
        copied.html.to_lowercase().contains("<table"),
        "GridCraft put an HTML table on the clipboard: {:?}",
        copied.html.chars().take(200).collect::<String>()
    );
    assert!(copied.text.contains("North"), "and its text: {:?}", copied.text);

    let mut doc = Tab::new(Kind::Document, &platform);
    doc.send(vec![egui::Event::Paste(copied.text.clone())]);
    doc.frames(3);
    let inspect = doc.run("document.inspect", json!({}));
    let s = inspect.to_string();
    let text = doc.run("document.text", json!({})).to_string();
    println!("document after paste: {}", s.chars().take(600).collect::<String>());
    assert!(text.contains("North") && text.contains("1350"), "the cells arrived: {text}");
    assert!(
        s.contains("\"table") || s.contains("Table") || s.contains("tables"),
        "they arrived as a table: {}",
        s.chars().take(600).collect::<String>()
    );
}

/// Bold text copied in a document stays bold when pasted into a presentation.
#[test]
fn document_text_pastes_into_a_slide_with_its_formatting() {
    let clipboard = Arc::new(MemoryClipboard::default());
    let platform = Platform::isolated(clipboard.clone());

    let mut doc = Tab::new(Kind::Document, &platform);
    doc.send(vec![egui::Event::Text("Bold words".into())]);
    doc.send(key(egui::Key::A, CMD));
    doc.send(key(egui::Key::B, CMD));
    doc.frames(2);
    doc.send(vec![egui::Event::Copy]);
    doc.frames(2);
    let copied = clipboard.contents();
    let html = copied.html.to_lowercase();
    assert!(
        html.contains("bold words") && (html.contains("<b") || html.contains("<strong") || html.contains("font-weight")),
        "WordCraft copied bold HTML: {html}"
    );
    assert!(!copied.rtf.is_empty(), "and RTF beside it");

    let mut deck = Tab::new(Kind::Presentation, &platform);
    deck.send(vec![egui::Event::Paste(copied.text.clone())]);
    deck.frames(3);
    let slide = deck.run("slide.inspect", json!({}));
    let s = slide.to_string();
    assert!(s.contains("Bold words"), "the text arrived on the slide");
    // The pasted shape's runs carry the formatting.
    let id =
        slide["shapes"].as_array().and_then(|shapes| shapes.iter().find(|sh| sh["text"] == json!("Bold words"))).and_then(|sh| sh["id"].as_u64());
    let shape = deck.run("shape.inspect", json!({"id": id}));
    let t = shape.to_string();
    println!("pasted shape: {}", t.chars().take(900).collect::<String>());
    assert!(t.contains("\"bold\":true") || t.contains("\"b\":true"), "still bold: {}", t.chars().take(900).collect::<String>());
}
