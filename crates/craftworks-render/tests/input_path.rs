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
    let mut input = embed::input_for_area(&window, area, 2.0, |_| true);
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
        let mut i = embed::input_for_area(&window, area, 2.0, |_| true);
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
