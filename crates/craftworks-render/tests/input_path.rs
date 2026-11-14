//! The app's exact input path into an engine, without a GPU: `input_for_area` from a window's
//! input, a running clock, the window's theme as the system's, then one pass of logic + ui.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use craftworks_core::engines::new_editor;
use craftworks_core::{Editor, Kind};
use craftworks_render::embed;
use serde_json::json;

fn pass(ctx: &egui::Context, ed: &mut dyn Editor, t: &mut f64, events: Vec<egui::Event>, variant: u8) {
    *t += 1.0 / 60.0;
    let area = egui::Rect::from_min_size(egui::pos2(0.0, 32.0), egui::vec2(1400.0, 860.0));
    let window = egui::RawInput { focused: true, events, ..Default::default() };
    let mut input = embed::input_for_area(&window, area, 2.0, None, |_| true);
    if variant & 1 != 0 {
        input.time = Some(*t);
    }
    if variant & 2 != 0 {
        input.system_theme = Some(egui::Theme::Light);
    }
    ed.raw_input_hook(ctx, &mut input);
    let mut out = ctx.run_ui(input, |ui| {
        ed.logic(ui.ctx());
        ed.ui(ui);
    });
    out.textures_delta.clear();
}

#[test]
fn typing_reaches_wordcraft_through_the_apps_input_path() {
    let mut failures = Vec::new();
    for variant in 0..4u8 {
        let ctx = egui::Context::default();
        let mut ed = new_editor(Kind::Document);
        let mut t = 0.0;
        for _ in 0..3 {
            pass(&ctx, ed.as_mut(), &mut t, vec![], variant);
        }
        ed.new_document().unwrap();
        for _ in 0..10 {
            pass(&ctx, ed.as_mut(), &mut t, vec![], variant);
        }
        pass(&ctx, ed.as_mut(), &mut t, vec![egui::Event::Text(" Typed.".into())], variant);
        for _ in 0..3 {
            pass(&ctx, ed.as_mut(), &mut t, vec![], variant);
        }
        let text = ed.run_command("document.text", json!({})).unwrap().to_string();
        if !text.contains("Typed.") {
            failures.push(format!("variant {variant} (time={}, theme={}): {text}", variant & 1 != 0, variant & 2 != 0));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// The app's sequence through the real `EngineHost`: shown, hidden (logic-only passes), shown
/// again, then a keystroke. `focused`: whether the window had focus before the keystroke.
fn type_through_host(window_focused: bool, hidden_passes: usize) -> String {
    use craftworks_render::EngineHost;
    let area = egui::Rect::from_min_size(egui::pos2(0.0, 32.0), egui::vec2(1400.0, 860.0));
    let input = |events: Vec<egui::Event>, focused: bool| {
        let window = egui::RawInput { focused, events, ..Default::default() };
        let mut i = embed::input_for_area(&window, area, 2.0, None, |_| true);
        i.system_theme = Some(egui::Theme::Light);
        i
    };
    let mut host = EngineHost::headless(new_editor(Kind::Document));
    host.editor_mut().set_dark(false);
    let f = window_focused;
    for _ in 0..3 {
        host.frame_headless(input(vec![], f));
    }
    host.editor_mut().new_document().unwrap();
    for _ in 0..5 {
        host.frame_headless(input(vec![], f));
    }
    // Hidden: logic only, a few times.
    for _ in 0..hidden_passes {
        host.logic_only(input(vec![], f));
    }
    for _ in 0..10 {
        host.frame_headless(input(vec![], f));
    }
    let mut typed = input(vec![egui::Event::Text(" Typed.".into())], true);
    typed.viewports.values_mut().for_each(|v| v.focused = Some(true));
    host.frame_headless(typed);
    for _ in 0..3 {
        host.frame_headless(input(vec![], true));
    }
    host.editor_mut().run_command("document.text", json!({})).unwrap().to_string()
}

/// In a focused window, typing reaches the document, also after the tab was hidden (its
/// engine ran logic-only passes). (A keystroke in the very frame the window gains focus is
/// dropped by WordCraft itself, as in the standalone app; users always focus first.)
#[test]
fn typing_reaches_wordcraft_through_the_engine_host() {
    for hidden in [0, 4] {
        let text = type_through_host(true, hidden);
        assert!(text.contains("Typed."), "after {hidden} hidden passes: {text}");
    }
}

/// A headless engine fed the way the app feeds it: focused window, the window's light theme,
/// events in the engine's own coordinates.
struct Driver {
    host: craftworks_render::EngineHost,
}

impl Driver {
    fn new(kind: Kind) -> Driver {
        let mut host = craftworks_render::EngineHost::headless(new_editor(kind));
        host.editor_mut().set_dark(false);
        let mut d = Driver { host };
        d.frames(3);
        d.host.editor_mut().new_document().unwrap();
        d.frames(5);
        d
    }

    fn send(&mut self, events: Vec<egui::Event>) {
        let area = egui::Rect::from_min_size(egui::pos2(0.0, 32.0), egui::vec2(1400.0, 860.0));
        let window = egui::RawInput { focused: true, ..Default::default() };
        let mut input = embed::input_for_area(&window, area, 2.0, None, |_| true);
        input.system_theme = Some(egui::Theme::Light);
        input.viewports.values_mut().for_each(|v| v.focused = Some(true));
        input.events = events;
        self.host.frame_headless(input);
    }

    fn frames(&mut self, n: usize) {
        for _ in 0..n {
            self.send(vec![]);
        }
    }

    fn run(&mut self, id: &str, args: serde_json::Value) -> serde_json::Value {
        self.host.editor_mut().run_command(id, args).unwrap()
    }
}

fn key(k: egui::Key, m: egui::Modifiers) -> Vec<egui::Event> {
    vec![
        egui::Event::Key { key: k, physical_key: Some(k), pressed: true, repeat: false, modifiers: m },
        egui::Event::Key { key: k, physical_key: Some(k), pressed: false, repeat: false, modifiers: m },
    ]
}

const CMD: egui::Modifiers = egui::Modifiers { alt: false, ctrl: false, shift: false, mac_cmd: true, command: true };

/// ⌘A, ⌘B bolds the selection in WordCraft; ⌘Z undoes it.
#[test]
fn command_b_bolds_and_command_z_undoes_in_wordcraft() {
    let mut d = Driver::new(Kind::Document);
    d.send(vec![egui::Event::Text("Bold me".into())]);
    d.frames(2);
    d.send(key(egui::Key::A, CMD));
    d.send(key(egui::Key::B, CMD));
    d.frames(2);
    let state = d.run("format.state", json!({}));
    assert_eq!(state["bold"], json!(true), "after ⌘B: {state}");
    d.send(key(egui::Key::Z, CMD));
    d.frames(2);
    let state = d.run("format.state", json!({}));
    assert_ne!(state["bold"], json!(true), "after ⌘Z: {state}");
}

/// Composed text (an input method: Japanese here) reaches WordCraft: the preedit shows while
/// composing, the commit inserts it once.
#[test]
fn ime_composition_reaches_wordcraft() {
    let mut d = Driver::new(Kind::Document);
    d.send(vec![egui::Event::Ime(egui::ImeEvent::Preedit { text: "にほん".into(), active_range_chars: Some(0..3) })]);
    d.frames(1);
    d.send(vec![egui::Event::Ime(egui::ImeEvent::Preedit { text: "日本".into(), active_range_chars: Some(0..2) })]);
    d.frames(1);
    d.send(vec![egui::Event::Ime(egui::ImeEvent::Commit("日本".into()))]);
    d.frames(2);
    let text = d.run("document.text", json!({})).to_string();
    assert!(text.contains("日本") && !text.contains("にほん") && text.matches("日本").count() == 1, "{text}");
}

/// Hindi (Devanagari) and emoji as committed text, and a dead-key accent (⌥E then E gives é,
/// delivered by macOS as a preedit "´" then a commit "é").
#[test]
fn devanagari_emoji_and_dead_keys_reach_wordcraft() {
    let mut d = Driver::new(Kind::Document);
    d.send(vec![egui::Event::Ime(egui::ImeEvent::Commit("नमस्ते ".into()))]);
    d.send(vec![egui::Event::Text("👋 ".into())]);
    d.send(vec![egui::Event::Ime(egui::ImeEvent::Preedit { text: "´".into(), active_range_chars: Some(0..1) })]);
    d.send(vec![egui::Event::Ime(egui::ImeEvent::Commit("é".into()))]);
    d.frames(2);
    let text = d.run("document.text", json!({})).to_string();
    assert!(text.contains("नमस्ते") && text.contains('👋') && text.contains('é') && !text.contains('´'), "{text}");
}

/// Arrow keys move GridCraft's selection; typing then Enter fills the cell; composed text
/// too, once the cell is being edited (F2): GridCraft turns its input method on only in the
/// cell editor, as the standalone app does (docs/decisions.md D30).
#[test]
fn arrows_typing_and_ime_reach_gridcraft() {
    let mut d = Driver::new(Kind::Sheet);
    d.send(key(egui::Key::ArrowDown, egui::Modifiers::NONE));
    d.send(key(egui::Key::ArrowDown, egui::Modifiers::NONE));
    d.send(vec![egui::Event::Text("typed".into())]);
    d.send(key(egui::Key::Enter, egui::Modifiers::NONE));
    d.send(key(egui::Key::F2, egui::Modifiers::NONE));
    d.frames(1);
    d.send(vec![egui::Event::Ime(egui::ImeEvent::Preedit { text: "にほん".into(), active_range_chars: Some(0..3) })]);
    d.send(vec![egui::Event::Ime(egui::ImeEvent::Commit("日本".into()))]);
    d.send(key(egui::Key::Enter, egui::Modifiers::NONE));
    d.frames(2);
    let a3 = d.run("cell.get", json!({"cell": "A3"})).to_string();
    let a4 = d.run("cell.get", json!({"cell": "A4"})).to_string();
    assert!(a3.contains("typed"), "A3: {a3}");
    assert!(a4.contains("日本"), "A4: {a4}");
}

/// While WordCraft's document has the caret, the engine reports where it is, so the window can
/// switch the input method on and place its candidate list there (the app moves it into
/// window coordinates).
#[test]
fn wordcraft_reports_its_text_cursor_for_the_input_method() {
    let mut d = Driver::new(Kind::Document);
    d.send(vec![egui::Event::Text("Caret here".into())]);
    let area = egui::Rect::from_min_size(egui::pos2(0.0, 32.0), egui::vec2(1400.0, 860.0));
    let window = egui::RawInput { focused: true, ..Default::default() };
    let mut input = embed::input_for_area(&window, area, 2.0, None, |_| true);
    input.viewports.values_mut().for_each(|v| v.focused = Some(true));
    let out = d.host.frame_headless(input);
    let ime = out.platform.ime.expect("an IME request while the document has the caret");
    let local = egui::Rect::from_min_size(egui::Pos2::ZERO, area.size());
    assert!(local.contains(ime.cursor_rect.center()), "caret rect {:?} inside the engine's area", ime.cursor_rect);
    let in_window = embed::to_window(ime.cursor_rect, area);
    assert_eq!(in_window.top(), ime.cursor_rect.top() + 32.0, "moved below the title row");
}

/// After WordCraft's Save prompt is cancelled, typing goes back into the page.
#[test]
#[ignore = "upstream: WordCraft leaves keyboard focus off the page after its Save prompt (docs/decisions.md D31)"]
fn typing_after_cancelling_wordcrafts_save_prompt() {
    let mut d = Driver::new(Kind::Document);
    d.send(vec![egui::Event::Text("Before.".into())]);
    d.frames(2);
    let doc = d.host.editor().active().unwrap();
    d.host.editor_mut().request_close(doc).unwrap();
    d.frames(3);
    assert!(d.host.editor().close_prompt_open(), "the prompt is up");
    d.run("ui.saveChanges", json!({"answer": "cancel"}));
    d.frames(3);
    d.send(vec![egui::Event::Text(" After.".into())]);
    d.frames(2);
    let text = d.run("document.text", json!({})).to_string();
    println!("after cancel: {text}");
    assert!(text.contains("After."), "{text}");
}
