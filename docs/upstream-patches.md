# Upstream patches

Every change we need inside the Craft repos. Each patch:

- goes on its own branch in our fork,
- is small and tested (it follows that repo's AGENTS.md: a test, `cargo xtask ci`, no panics, no `unsafe`), and
- can be offered upstream as a PR.

Status as of 2026-11-14 (Phase 1).

| Id | Repo | Change | Status |
|---|---|---|---|
| H1 | wordcraft | `WordApp::use_one_window()`: every document in the one window, as the web app does (`Windows::single`, previously test-only). Activating a document (`session.activate`) shows it on the next frame. | **Done** `793e65c` on `HarshaChittella/wordcraft` `craftworks/hosted`. Test `one_window_shows_each_document_it_is_asked_to` fails without it (2 windows). `wordcraft-ui-egui`: 1,255 tests pass, clippy clean. |
| H2–H4 | wordcraft, deckcraft, gridcraft | Window parts of the title row (D6). | **Not needed.** The traffic-light inset is already off in the libraries (`integrated_titlebar: false` is the default; only the desktop binaries turn it on). Drag-to-move and double-click-to-zoom only send `ViewportCommand::StartDrag`/`Maximized`, which the host drops (D18). |
| H5 | pdfcraft | Inherent `raw_input_hook(ctx, raw)`, `logic(ctx)`, `ui(ui)`, `exiting()`; `eframe::App` calls them. `persist()`/`restore()` already existed. | **Done** `20063ac` on `HarshaChittella/pdfcraft` `craftworks/hosted`. |
| H6 | pdfcraft | `PdfCraftApp::hosted`: no tab strip. The host picks the tab with `active` (already public) and reads `has_unsaved_work(i)` (now public). Optional for later: host hooks in place of `rfd` (D12); turning off cross-document split panes (D3). | **Done**, same commit. 3 tests drive the app with a plain egui pass; the hosted one fails without the change. `pdfcraft-ui-egui`: all tests pass except the 8 known environment failures. |
| H7 | pdfcraft | Run automation tools against the open views. | Phase 7, last task (D14). |
| H8 | all | Per-document list and activate API. | **Not needed.** WordCraft `session.documents()`/`activate`; GridCraft `session.windows()`/`enter_window`; DeckCraft `session.documents()`/`set_active`; PdfCraft `views`/`active`. All public. |

Submodules `vendor/wordcraft` and `vendor/pdfcraft` now track the forks' `craftworks/hosted` branch (`.gitmodules`), because the recorded commits exist only there. Commits in the forks leave out agent attribution, per the Craft repos' house rule 15.

Not needed:

- Font hooks. With one context per engine (D1), each app keeps calling `set_fonts` on its own context.
