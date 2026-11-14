# Upstream patches

Every change we need inside the Craft repos. Each patch:

- goes on its own branch in our fork,
- is small and tested (it follows that repo's AGENTS.md: a test, `cargo xtask ci`, no panics, no `unsafe`), and
- can be offered upstream as a PR.

Status: **proposed**. Nothing is written yet. Line references are to HEAD on 2026-11-14.

| Id | Repo | Change | Why | Size |
|---|---|---|---|---|
| H1 | wordcraft | Add `WordApp::set_hosted(Hosted)`. It switches to single-window mode (`Windows::single()`, today `#[cfg(test)]` at `crates/ui-egui/src/windows.rs:131`) and sets a `hosted` flag. | Without it, every New or Open on the desktop opens a native window (`native: cfg!(not(wasm32))`) | ~30 lines + test |
| H2 | wordcraft | When hosted, `chrome::title_bar` leaves out the window parts: traffic-light inset, `StartDrag`, double-click maximize, viewport title. The QAT, search and file-name row stays (D6 a). Or the whole row is hidden (D6 b). | Our window owns those parts | ~20 lines |
| H3 | deckcraft | `SlideApp.chrome: Chrome { title_bar, window_controls }`, the same shape as GridCraft's. View ▸ New Window (`doc_window.rs`) is sent to the host when hosted. | Same as H1/H2 | ~40 lines |
| H4 | gridcraft | When hosted, `Chrome` gets `window_controls: bool`, which stops drag and maximize from the title bar while the QAT row stays (D6 a). Nothing if we pick D6 b, since `title_bar: false` already exists. | Same | ~15 lines |
| H5 | pdfcraft | `PdfCraftApp` inherent methods `logic(ctx)`, `ui(ui)`, `raw_input_hook(ctx, raw)`, `persist() -> String` (already exists), `on_exit()`. The `eframe::App` impl calls them. | So it can be driven without an `eframe::Frame` (F4) | ~25 lines |
| H6 | pdfcraft | Hosted mode: hide `chrome::tab_strip` and window controls; add `views() -> Vec<ViewInfo>`, `select_view(i)` and `close_view(i) -> CloseTicket`; turn off cross-document split panes. Optional: a `pick_open`/`pick_save` host hook in place of `rfd`. | D3 | ~80 lines |
| H7 | pdfcraft | A way for an in-process host to run automation tools against the **open** views, for example `Automation::for_session(&mut Session)`, or the UI control channel's dispatcher exposed as a function. | Live AI edits on an open PDF (Phase 7) | to scope in Phase 7 |
| H8 | all | If missing, a public per-document API: list documents with id, title, path and dirty state; activate a document. | `Editor::documents()` / `activate()` | small, per app |

Not needed:

- Font hooks. With one context per engine (D1), each app keeps calling `set_fonts` on its own context.
