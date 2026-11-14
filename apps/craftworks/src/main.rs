//! Craftworks: documents, sheets, presentations and PDFs in tabs of one window. Each tab is
//! the full Craft app (WordCraft, GridCraft, DeckCraft, PdfCraft); the chrome around them is
//! ours (docs/design.md).
//!
//! ```text
//! craftworks [FILE…]                    open files (routed by extension), or the start page
//! craftworks --check DIR [--appearance light|dark]
//!                                       the acceptance run: screenshots, checks and a report
//!                                       in DIR/report.txt, then quit
//! ```
//! `CW_LOG=debug` logs more (default: warnings).
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod app;
mod check;
mod logger;
mod menu;
mod session;
mod settings;
mod titlebar;

use std::path::PathBuf;

fn main() -> eframe::Result {
    logger::install();
    let mut files = Vec::new();
    let mut check_dir = None;
    let mut forced_dark = None;
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--check" => check_dir = it.next().map(PathBuf::from),
            "--appearance" => forced_dark = it.next().map(|v| v == "dark"),
            "-h" | "--help" => {
                println!("craftworks [FILE…] | craftworks --check DIR [--appearance light|dark]");
                return Ok(());
            }
            _ => files.push(PathBuf::from(a)),
        }
    }
    if let Some(dir) = &check_dir {
        // A check run keeps its settings (recent files, session) to itself.
        settings::use_dir(dir.join("settings"));
    } else if let Some(dir) = std::env::var_os("CRAFTWORKS_SETTINGS_DIR") {
        // Tests of the app bundle (scripts/finder-open-test.sh) keep yours untouched.
        settings::use_dir(PathBuf::from(dir));
    }
    // Finder's open-documents and the Dock's Quit arrive as Apple events: register before the
    // event loop starts, so the event that launched the app is caught too.
    #[cfg(target_os = "macos")]
    let (_apple_events, apple_inbox) = fmv_macos_events::Registration::install();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Craftworks")
            .with_inner_size([1440.0, 920.0])
            .with_min_inner_size([760.0, 480.0])
            .with_fullsize_content_view(true)
            .with_titlebar_shown(false)
            .with_title_shown(false),
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };
    eframe::run_native(
        "Craftworks",
        options,
        Box::new(move |cc| {
            let render = cc.wgpu_render_state.clone().ok_or("Craftworks needs the wgpu renderer")?;
            let mut app = app::App::new(render, &cc.egui_ctx, forced_dark);
            #[cfg(target_os = "macos")]
            {
                let ctx = cc.egui_ctx.clone();
                apple_inbox.set_wake(move || ctx.request_repaint());
                app.set_apple_events(apple_inbox.clone());
            }
            match check_dir {
                Some(dir) => app = app.with_check(check::Check::new(dir, forced_dark)),
                None if files.is_empty() => app.restore_session(),
                None => files.iter().for_each(|f| app.open(f)),
            }
            Ok(Box::new(app))
        }),
    )
}
