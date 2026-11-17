//! The connector end to end, without a window: project tools, headless edits, permissions and
//! the project jail; and a live edit to an open document through a stand-in window.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use officecraft_core::project::{Permission, Project};
use officecraft_core::{DocId, Editor, Kind};
use officecraft_mcp::Server;
use officecraft_mcp::live::LiveServer;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

fn scratch(name: &str) -> PathBuf {
    let d = PathBuf::from(format!("/tmp/cwm-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn allow(root: &Path, perm: Permission) {
    let mut p = Project::create(root, Some("Test")).unwrap();
    p.settings.ai_permission = perm;
    p.save().unwrap();
}

fn ok(r: &Value) -> Value {
    assert_ne!(r["isError"], json!(true), "tool failed: {r}");
    r.get("structuredContent").cloned().unwrap_or_else(|| json!(r["content"][0]["text"]))
}

fn err(r: &Value) -> String {
    assert_eq!(r["isError"], json!(true), "expected a refusal: {r}");
    r["content"][0]["text"].as_str().unwrap_or_default().to_string()
}

#[test]
fn headless_work_permissions_and_the_jail() {
    let root = scratch("headless");
    allow(&root, Permission::EditSave);
    let settings = scratch("headless-nowindow");
    let mut s = Server::new(&root, true).unwrap().with_settings_dir(&settings);

    // The protocol: initialize, then the compact tool list.
    let init = s.handle(&json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18"}})).unwrap();
    assert!(init["result"]["instructions"].as_str().unwrap().contains("edit and save"));
    let list = s.handle(&json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"})).unwrap();
    let names: Vec<&str> = list["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert!(names.len() <= 14 && names.contains(&"tool_search") && names.contains(&"project_search"), "{names:?}");
    assert!(s.handle(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"})).is_none(), "notifications get no answer");

    // A sheet made and saved headless; a document; a PDF.
    ok(&s.call("sheet.write_range", &json!({"file": "budget.xlsx", "range": "A1", "values": [["Region", "Q3"], ["North", 1200], ["South", 1350]]})));
    ok(&s.call("project_save", &json!({"file": "budget.xlsx"})));
    assert!(root.join("budget.xlsx").is_file());
    ok(&s.call("doc.execute", &json!({"file": "report.docx", "command": "text.insert", "params": {"text": "Quarterly report for the board."}})));
    ok(&s.call("project_save", &json!({"file": "report.docx"})));
    let pdf = pdfcraft_engine_bytes("Contract. Payment of 2,550 due in 30 days.");
    std::fs::write(root.join("contract.pdf"), pdf).unwrap();

    let files = ok(&s.call("project_files", &json!({})));
    let listed: Vec<&str> = files["files"].as_array().unwrap().iter().map(|f| f["file"].as_str().unwrap()).collect();
    assert_eq!(listed, ["budget.xlsx", "contract.pdf", "report.docx"]);
    let hits = ok(&s.call("project_search", &json!({"query": "north"})));
    assert_eq!(hits["hits"][0]["file"], json!("budget.xlsx"));
    assert_eq!(hits["hits"][0]["where"], json!("Sheet1!A2"));
    let read = ok(&s.call("project_read", &json!({"file": "contract.pdf"})));
    assert!(read["parts"][0]["text"].as_str().unwrap().contains("2,550"));
    let q3 = ok(&s.call("sheet.read_range", &json!({"file": "budget.xlsx", "range": "A1:B3"})));
    assert!(q3.to_string().contains("1350"), "{q3}");
    // Through the compact mode's meta tools.
    let found = ok(&s.call("tool_search", &json!({"query": "insert table"})));
    assert!(found.to_string().contains("doc.insert_table"));
    ok(&s.call("tool_call", &json!({"name": "pdf.doc_open", "arguments": {"path": "contract.pdf"}})));

    // The jail.
    for bad in ["../outside.docx", "/etc/hosts"] {
        assert!(err(&s.call("project_read", &json!({"file": bad}))).contains("outside") || bad.starts_with("/etc"));
    }
    assert!(err(&s.call("doc.get_text", &json!({"file": "budget.xlsx"}))).contains("isn't a document"));

    // Read-only projects refuse edits but still read.
    allow(&root, Permission::Read);
    assert!(err(&s.call("sheet.set_cell", &json!({"file": "budget.xlsx", "cell": "B2", "value": 1}))).contains("read only"));
    ok(&s.call("sheet.read_range", &json!({"file": "budget.xlsx", "range": "B2"})));
    // `edit` without a window: edits wait for one, since they need the user's review.
    allow(&root, Permission::Edit);
    assert!(err(&s.call("sheet.set_cell", &json!({"file": "budget.xlsx", "cell": "B2", "value": 1}))).contains("review"));
}

fn pdfcraft_engine_bytes(text: &str) -> Vec<u8> {
    pdfcraft_engine::Session::new().create_from_text("Contract", text).unwrap().to_vec()
}

/// Scenario 1's core: the AI reads the sheet and writes a table into the open document; the
/// document changes in its "tab" and the app's Undo takes the change back.
#[test]
fn a_live_edit_lands_in_the_open_tab_and_undoes() {
    let root = scratch("live");
    allow(&root, Permission::Edit);
    let settings = scratch("live-window");
    // The document, saved first so the window opens a real file.
    let report = root.join("report.docx").canonicalize().unwrap_or_else(|_| root.canonicalize().unwrap().join("report.docx"));
    let (done_tx, done_rx) = std::sync::mpsc::channel::<()>();
    let (state_tx, state_rx) = std::sync::mpsc::channel::<String>();
    let window_dir = settings.clone();
    let doc_path = report.clone();
    let window = std::thread::spawn(move || {
        let ctx = egui::Context::default();
        let mut ed = officecraft_core::engines::new_editor(Kind::Document);
        frames(&ctx, ed.as_mut(), 2);
        let doc = ed.new_document().unwrap();
        frames(&ctx, ed.as_mut(), 2);
        ed.run_command("text.insert", json!({"text": "Report"})).unwrap();
        ed.save_as(doc, &doc_path).unwrap();
        let live = LiveServer::start(&window_dir, || {}).unwrap();
        while done_rx.try_recv().is_err() {
            for r in live.drain() {
                let answer = match r.op.as_str() {
                    "tabs" => Ok(json!([{"path": doc_path.to_string_lossy(), "kind": "Document", "dirty": false}])),
                    "call" => ed.control(doc, r.args["method"].as_str().unwrap_or_default(), r.args["params"].clone()),
                    other => Err(format!("unexpected {other}")),
                };
                r.answer(answer);
            }
            frames(&ctx, ed.as_mut(), 1);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let text = |ed: &mut Box<dyn Editor>| ed.control(doc, "document.inspect", json!({})).unwrap().to_string();
        state_tx.send(text(&mut ed)).unwrap();
        ed.run_command("edit.undo", json!({})).unwrap();
        frames(&ctx, ed.as_mut(), 1);
        state_tx.send(text(&mut ed)).unwrap();
        let _ = DocId(0);
    });
    // Wait for the window.
    for _ in 0..500 {
        if settings.join("live.token").exists() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let mut s = Server::new(&root, false).unwrap().with_settings_dir(&settings);
    let files = ok(&s.call("project_files", &json!({})));
    assert_eq!(files["files"][0]["open_in_window"], json!(true), "{files}");
    let r =
        s.call("doc.insert_table", &json!({"file": "report.docx", "values": [["Region", "Q3"], ["North", 1200], ["South", 1350]], "header": true}));
    ok(&r);
    // Saving an open file is the window's to allow (the real one keeps it for review).
    let _ = err(&s.call("project_save", &json!({"file": "report.docx"})));
    done_tx.send(()).unwrap();
    let after = state_rx.recv().unwrap();
    assert!(after.contains("North") && after.contains("1350"), "the table is in the open document: {after}");
    let undone = state_rx.recv().unwrap();
    assert!(!undone.contains("1350"), "WordCraft's Undo takes the AI's table back");
    window.join().unwrap();
}

fn frames(ctx: &egui::Context, ed: &mut dyn Editor, n: usize) {
    for _ in 0..n {
        let mut input =
            egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 800.0))), ..Default::default() };
        ed.raw_input_hook(ctx, &mut input);
        let mut out = ctx.run_ui(input, |ui| {
            ed.logic(ui.ctx());
            ed.ui(ui);
        });
        out.textures_delta.clear();
    }
}
