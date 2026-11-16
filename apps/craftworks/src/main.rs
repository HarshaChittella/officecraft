//! Craftworks: documents, sheets, presentations and PDFs in tabs of one window. Each tab is
//! the full Craft app (WordCraft, GridCraft, DeckCraft, PdfCraft); the chrome around them is
//! ours (docs/design.md).
//!
//! ```text
//! craftworks [FILE…]                    open files (routed by extension), or the start page
//! craftworks --check DIR [--appearance light|dark]
//!                                       the acceptance run: screenshots, checks and a report
//!                                       in DIR/report.txt, then quit
//! craftworks mcp --project DIR [--all-tools]
//!                                       the AI connector (MCP over stdio), no window
//! craftworks open FILE…                 open files in the running window (or start it)
//! ```
//! `CW_LOG=debug` logs more (default: warnings).
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod ai;
mod app;
mod check;
mod crash;
mod logger;
mod menu;
mod projects;
mod session;
mod settings;
mod titlebar;

use std::path::PathBuf;

fn main() -> eframe::Result {
    logger::install();
    crash::install();
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("mcp") => std::process::exit(mcp(&args[1..])),
        Some("open") => std::process::exit(open_in_window(&args[1..])),
        _ => {}
    }
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
    // A check run waits and measures in real time: no App Nap while it lasts.
    let _awake = check_dir.as_ref().map(|_| craftworks_platform::activity::begin("Craftworks acceptance check"));
    if let Some(dir) = &check_dir {
        // A check run keeps its settings (recent files, session) to itself.
        settings::use_dir(dir.join("settings"));
        crash::use_dir(dir.join("logs"));
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
            // No icon of our own at run time: the Dock then shows the bundle's icon, the same one
            // Finder and Launchpad show (eframe would otherwise put egui's default icon in the Dock).
            .with_icon(std::sync::Arc::new(egui::IconData::default()))
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
            // `--check` must not touch the user's clipboard, open panels or print.
            let platform = if check_dir.is_some() {
                craftworks_platform::Platform::isolated(std::sync::Arc::new(craftworks_platform::MemoryClipboard::default()))
            } else {
                craftworks_platform::Platform::system()
            };
            let mut app = app::App::new(render, &cc.egui_ctx, forced_dark, platform);
            // The AI connector (`craftworks mcp`) reaches this window through the settings folder.
            if let Some(dir) = settings::dir() {
                app.ai.start(&dir, &cc.egui_ctx);
            }
            #[cfg(target_os = "macos")]
            {
                let ctx = cc.egui_ctx.clone();
                apple_inbox.set_wake(move || ctx.request_repaint());
                app.set_apple_events(apple_inbox.clone());
            }
            app.mention_crash();
            match check_dir {
                Some(dir) => app = app.with_check(check::Check::new(dir, forced_dark)),
                None if files.is_empty() => {
                    app.restore_project(&cc.egui_ctx);
                    app.restore_session();
                }
                None => {
                    app.restore_project(&cc.egui_ctx);
                    files.iter().for_each(|f| app.open(f));
                }
            }
            Ok(Box::new(app))
        }),
    )
}

/// `craftworks mcp --project DIR [--all-tools]`: the MCP server over stdin and stdout.
fn mcp(args: &[String]) -> i32 {
    let mut project = None;
    let mut compact = true;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--project" => project = it.next().map(PathBuf::from),
            "--all-tools" => compact = false,
            "-h" | "--help" => {
                eprintln!("craftworks mcp --project DIR [--all-tools]");
                return 0;
            }
            other => {
                eprintln!("craftworks mcp: unknown argument {other:?}");
                return 2;
            }
        }
    }
    let project = project.or_else(|| std::env::current_dir().ok()).unwrap_or_else(|| PathBuf::from("."));
    let mut server = match craftworks_mcp::Server::new(&project, compact) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("craftworks mcp: {e}");
            return 1;
        }
    };
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    match server.serve(stdin.lock(), stdout.lock()) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("craftworks mcp: {e}");
            1
        }
    }
}

/// `craftworks open FILE…`: in the running window, or a new one.
fn open_in_window(files: &[String]) -> i32 {
    let paths: Vec<PathBuf> = files.iter().map(PathBuf::from).map(|p| p.canonicalize().unwrap_or(p)).collect();
    if let Some(mut w) = craftworks_mcp::live::settings_dir().as_deref().and_then(craftworks_mcp::live::LiveClient::connect) {
        let mut failed = 0;
        for p in &paths {
            if let Err(e) = w.request("open", serde_json::json!({"path": p.to_string_lossy()})) {
                eprintln!("craftworks open: {}: {e}", p.display());
                failed += 1;
            }
        }
        return i32::from(failed > 0);
    }
    match std::env::current_exe().and_then(|exe| std::process::Command::new(exe).args(&paths).spawn()) {
        Ok(_) => 0,
        Err(e) => {
            eprintln!("craftworks open: {e}");
            1
        }
    }
}
