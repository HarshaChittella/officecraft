# Decisions

Each entry has a status: **ASK** (waiting for the owner), **PROPOSED**, or **SETTLED** (with the date). Findings F1–F16 refer to the table in [PLAN.md](PLAN.md).

## D1 One egui `Context` per engine (not per tab, not shared). SETTLED 2026-11-14

**Recommendation:** up to 4 contexts, one each for Word, Sheet, Deck and PDF. They share one `wgpu::Device` and `Queue`.

- **Why not one shared `Context`:**
  - The apps call `set_fonts` again at runtime: on a language change, for CJK and Arabic fallbacks, and in GridCraft whenever the workbook's fonts change (F8).
  - The apps reuse panel ids (`title_bar`, `status`, `tabs`).
  - They set global style and theme.
  - Merging all of this would mean deep patches to all four apps.
- **Why not one `Context` per tab:**
  - Textures (`TextureHandle`: page rasters, slide thumbnails, chart caches) belong to the context that created them.
  - With D2, one app instance draws many documents, so all of its tabs have to share that instance's context.
- **Cost:** one font atlas and one style set per engine, 4 at most. Phase 1 task 1.7 measures it.

## D2 One app instance per engine type. SETTLED 2026-11-14

**Recommendation:** each of the four apps runs as a single instance, and each of our tabs points at one document inside it.

| Engine | How a tab maps to a document | Why one instance |
|---|---|---|
| WordCraft | doc id in `Session`; the host turns off native windows (H1) and activates the document before drawing | Fits WordCraft's own single-window mode (the one the web build uses). Shares proofing dictionaries and font DB. Keeps Window ▸ Switch Windows and AutoSave bookkeeping correct |
| GridCraft | window id; `show_window(id)` then `ui()`; `DocWindows` is not used | Cross-workbook references (`[Book2]Sheet1!A1`) only work inside one `Session`. Shares the function library |
| DeckCraft | index into `Session::documents()` | The session is already multi-document. Reuse slides and Compare need the other decks |
| PdfCraft | its view (internal tab); its tab strip is hidden (H5) | It already keeps many views. Combine and Compare work across open files |

- **Downside:** a panic guard trips for the whole engine, not just one tab. Each app already has a last-resort guard that keeps the documents.

## D3 PdfCraft's internal tabs. SETTLED 2026-11-14

- Hosted mode hides PdfCraft's tab strip and window controls.
- Our tab strip calls `select_view(i)`, and PdfCraft reports its views back to us.
- PdfCraft's **Home** screen is replaced by our "+" chooser. Its tools list is still reachable through the PDF tab's mode bar.
- Split panes with **other** documents are turned off in hosted mode for v1, because a tab shows one document. Splitting one document into panes stays.

## D4 Repaint scheduling. SETTLED 2026-11-14

- `NSView.displayLink` (macOS 14+) runs only on the visible engine's view.
- A frame is requested only when egui's `repaint_delay` says so, or on input. Otherwise the display link pauses.
- Engines that aren't visible get a **logic-only pass** at 4 Hz: `run_ui` calls only `app.logic(ctx)`, with no `ui()` and no tessellation or paint. Autosave, background jobs and control requests keep running.
- The visible engine's `logic` also covers its own background documents.

## D5 One `EditorView` per engine, not per tab. SETTLED 2026-11-14 (differs from brief §4.1)

- With D2, an engine draws its active document. A second surface for the same engine would never be visible.
- Switching between two tabs of the same engine is `activate(doc)` plus one frame, with no layer swap.
- Switching engines shows another `NSView` whose last frame is already there, so there's no flash.

## D6 What happens to each app's own title-bar row. SETTLED 2026-11-14: option (a)

The app's title row (WordCraft, DeckCraft and GridCraft: QAT Save/Undo/Redo, AutoSave, file name, search, Share/Comments, theme switch) is drawn **inside** the Rust surface, above the ribbon. Our Swift tab strip can't go between the title row and the ribbon. Options:

- **(a) Keep the app's title row and remove only the window parts** (traffic-light inset, drag-to-move, double-click-to-zoom, window title). The order becomes: macOS title bar with our tabs → the app's QAT/search row → ribbon.
  - Pro: keeps each app's layout exactly, including search and AutoSave.
  - Con: two rows of chrome above the ribbon.
- **(b) Hide the app's title row completely** (GridCraft's `Chrome { title_bar: false }` already does this).
  - Pro: the tighter, Office-like look.
  - Con: QAT, search and AutoSave disappear unless Swift draws native equivalents that call `run_command`. That is rewriting a piece of editor UI in Swift, which needs your OK.

**Settled:** (a) for Word, Sheet and Deck: keep the app's title row and remove only the window parts (traffic-light space, drag-to-move; double-click-to-zoom and the viewport title go with them). For PDF, its strip *is* the title bar, so it is hidden either way.

## D7 egui revision: `725a39f` for everyone. SETTLED 2026-11-14

- `725a39f` is 4 commits ahead of `6d518fc` and 0 behind (F2).
- The three apps on `6d518fc` gain small fixes (resize corner, syntax highlighting, lints).
- PdfCraft keeps what it's tested on.
- The brief's block, which forces `6d518fc`, would drop those 4 commits under PdfCraft.

## D8 FFI binding: hand-written C ABI plus a checked-in header. SETTLED 2026-11-14

- UniFFI and cbindgen are **MPL-2.0**, which is not on the house licence allowlist (F15).
- swift-bridge (MIT/Apache) would be a new dependency, and its macros are heavy.
- The surface is about 15 functions plus a callback table, so writing it by hand is cheap.
- A Rust test keeps the header and the exports in sync.
- No cbindgen, not even as a dev tool.

## D9 `unsafe` exception for our two crates. SETTLED 2026-11-14

- Our workspace keeps `unsafe_code = "forbid"`, except in:
  - `craftworks-ffi` (`extern "C"`, raw pointers from Swift), and
  - one module of `craftworks-render` (`create_surface_unsafe` with a `CAMetalLayer`).
- Every `unsafe` block needs a `// SAFETY:` comment (enforced with `clippy::undocumented_unsafe_blocks`), and both crates set `#![deny(unsafe_op_in_unsafe_fn)]`.
- In `craftworks-render`, only the one surface module may use `unsafe`; the crate root is `deny(unsafe_code)` and that module alone has `allow`.
- No upstream crate gets `unsafe`.

## D10 Repository location. SETTLED 2026-11-14: `~/src/craftworks`

- The current path contains spaces and a curly apostrophe (`…/Documents - <name>’s MacBook Pro/Docucraft`).
- Build scripts, `xcodebuild` phases and some `build.rs` files mis-quote such paths.
- It also sits under `~/Documents`, which may be iCloud-synced; a `target/` folder of about 20 GB there is a bad idea.
- **Settled:** `~/src/craftworks` (not iCloud-synced). The docs moved there on 2026-11-14.

## D11 Repo name. SETTLED 2026-11-14

Working name Craftworks, as in the brief: `craftworks` for the folder, crates and app.

## D12 PdfCraft file dialogs stay on `rfd` for now. SETTLED 2026-11-14

- `rfd` uses `NSOpenPanel`/`NSSavePanel` on macOS, on the main thread, which is where our host calls the app. So it works inside the Swift app as it is.
- Adding a `Services`-style host hook to PdfCraft is a later upstream patch (H6, optional part), not part of Phase 1.

## D13 Extra viewports are floating in-tab windows as a stopgap. SETTLED 2026-11-14

- Each engine's `Context` runs with `set_embed_viewports(true)`, so extra windows (View ▸ New Window, the presenter view) draw as floating egui windows inside the tab.
- The host logs **every** `ViewportCommand` it does not handle (`log::warn!` with the engine, the viewport id and the command), so we can see what is missing.

## D14 Live AI edits to open PDFs come last in Phase 7. SETTLED 2026-11-14

H7 and the PdfCraft live backend are the final task in Phase 7, after the Word, Sheet and Deck live backends work.

## D15 Forks. SETTLED 2026-11-14

- The forks of the four repos go under the owner's GitHub account.
- Submodules track upstream `storytold/*`; each fork is added as a second remote named `fork` for the patch branches.

## Duplicate crates (`cargo tree -d`)

To be filled in Phase 0 (task 0.8).

## Pinned SHAs

To be filled in Phase 0 (task 0.2). Read on 2026-11-14: wordcraft `514085c`, gridcraft `c0ee120`, deckcraft `69379cf`, pdfcraft `e98dc17`.
