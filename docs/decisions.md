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

## D16 craft-libs: one revision, `e2dc681`, for every app. PROVISIONAL 2026-11-14

- Before: 9 craft-libs revisions in the graph (`92d392c`, `762a06a`, `01315d9`, `6e03f65`, `aa84415`, `bb57d42`, `63196e6`, `75c70fa`, `e2dc681`). They lie on one line of history, and `e2dc681` (PdfCraft's pin, 2026-11-13) contains all of them.
- Change: `vendor/craft-libs` is a submodule at `e2dc681`. Root `[patch."https://github.com/storytold/craft-libs"]` points all 37 craft-libs crates at it. `krilla` stays on the brief's `6e03f65` through the crates-io patch.
- `cargo check` passes. craft-libs duplicates drop to 0, and lockfile packages go from 942 to 926.
- **Kept only if** every app's own suite passes with it: `CW_ALIGN=1 scripts/upstream-tests.sh`. Results are under "Upstream test results".

## D17 Build inputs. PROVISIONAL 2026-11-14

- `CRAFT_FONTS_DIR` → `vendor/craft-fonts` at `c1db6d1` (HEAD). No app pins craft-fonts.
- `CRAFT_I18N_DIR` → `vendor/craft-i18n` at `1e51542` (WordCraft's pin). It is 57 commits ahead of PdfCraft's `575cfde` and contains it. One cargo build can only point at one copy.
- **Known cost:** 2 PdfCraft UI tests fail on `1e51542` and pass on `575cfde`:
  - `fonts::primary_ui_fonts_cover_latin_and_cyrillic_catalogs`: the French catalog now uses `▸`, which Inter lacks.
  - `comments::hotbar_picker_searches_translated_tool_labels`: the label "Hotbar settings" changed.
- In the app, that means a missing-glyph box in French PDF menus, plus one label that differs from PdfCraft's tests. The choice between the two revisions waits for WordCraft's results on `575cfde`. If WordCraft passes on the older one, we use the older one.

## Upstream test results (task 0.6)

Runs on 2026-11-14. Every run is logged under `plan/logs/` (gitignored), one timestamped file per run.

### PdfCraft (egui `725a39f` is already its own pin; our run changes `krilla` and the craft-i18n checkout)

**9,640 passed, 19 failed.** None of the 19 is caused by egui or krilla.

| Tests | Count | Cause | Evidence |
|---|---|---|---|
| xattr-preserving saves: `staging::*` ×4, `macos_association::*` ×3, `file_watch::*` ×2, `editing::a_save_preserves…`, `automation::saving_a_pdf_keeps…`, `signing::signing_preserves…` | 12 | `could not preserve the file's extended attributes … Result too large (os error 34)` on **macOS 27.2**. | 11 of the 12 also fail on PdfCraft's unmodified pins, both inside and outside the tool sandbox. `signing::…` has the same cause but was not re-run. `pdfcraft-platform` does not depend on egui. To report upstream (asking first). |
| LibreOffice conversion: `office::*` ×2, `libreoffice_converts…`, `open_worker::file_open_converts…`, `engine office_documents…` | 5 | This machine has a stale Homebrew wrapper: `/opt/homebrew/bin/soffice` points at `/Applications/LibreOffice.app`, which is gone, so it exits 126. | All 5 also fail on unmodified pins. Fix: `brew reinstall --cask libreoffice` or `brew uninstall --cask libreoffice`. |
| `fonts::primary_ui_fonts…`, `comments::hotbar_picker…` | 2 | craft-i18n `1e51542` vs `575cfde` (D17). | Both pass on `575cfde`. |

### WordCraft, GridCraft, DeckCraft (move from egui `6d518fc` to `725a39f`)

Running.

## Duplicate crates (`cargo tree -d`, task 0.9)

With D16, **no craft-libs crate is duplicated**. The rest are third-party crates at different semver versions, which Cargo cannot merge without upstream changes. The list is in `plan/logs/dups-aligned.txt`.

| Crate | Copies | Why it cannot merge |
|---|---|---|
| `read-fonts` | 4 | skrifa and harfrust generations differ across the apps' font stacks |
| `skrifa` | 3 | same as `read-fonts` |
| `phf`, `phf_shared`, `phf_generator`, `phf_macros` | 3 each | 0.11/0.12/0.13 from different parsers |
| `getrandom` | 3 | 0.2/0.3/0.4 from rand generations |
| `vello_cpu`, `vello_common` | 2 each | 0.0.8 is PdfCraft's vendored renderer (hayro); 0.2 is DeckCraft's renderer. Intended |
| `objc2`, `objc2-app-kit`, `objc2-foundation`, `block2` | 2 each | 0.5 and 0.6 generations (winit vs newer crates) |
| `zip`, `quick-xml`, `roxmltree`, `png`, `kurbo`, `harfrust`, `font-types`, `hashbrown`, `syn`, `thiserror`, `rand`, `sha1`, `sha2`, `md-5`, `digest`, `base64`, `bitflags`, `miniz_oxide`, and 10 small helpers | 2 each | major-version splits in the wider ecosystem |

## Pinned SHAs (task 0.2)

| Submodule | SHA | Date |
|---|---|---|
| wordcraft | `514085c` | HEAD 2026-11-14 |
| gridcraft | `c0ee120` | HEAD 2026-11-14 |
| deckcraft | `69379cf` | HEAD 2026-11-14 |
| pdfcraft | `e98dc17` | HEAD 2026-11-14 |
| craft-libs | `e2dc681` | PdfCraft's pin (D16) |
| craft-fonts | `c1db6d1` | HEAD 2026-11-14 |
| craft-i18n | `1e51542` | WordCraft's pin (D17) |
