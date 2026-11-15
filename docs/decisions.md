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

## D5 One `EditorView` per engine, not per tab. SETTLED 2026-11-14 (differs from brief §4.1). *Under D23: one render texture per engine, the same idea.*

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

## D8 FFI binding: hand-written C ABI plus a checked-in header. SETTLED 2026-11-14 — **superseded by D23 (no FFI)**

- UniFFI and cbindgen are **MPL-2.0**, which is not on the house licence allowlist (F15).
- swift-bridge (MIT/Apache) would be a new dependency, and its macros are heavy.
- The surface is about 15 functions plus a callback table, so writing it by hand is cheap.
- A Rust test keeps the header and the exports in sync.
- No cbindgen, not even as a dev tool.

## D9 `unsafe` exception for our two crates. SETTLED 2026-11-14 — **superseded by D24 (no `unsafe` anywhere)**

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

## D16 craft-libs: one revision, `e2dc681`, for every app. SETTLED 2026-11-14

- Before: 9 craft-libs revisions in the graph (`92d392c`, `762a06a`, `01315d9`, `6e03f65`, `aa84415`, `bb57d42`, `63196e6`, `75c70fa`, `e2dc681`). They lie on one line of history, and `e2dc681` (PdfCraft's pin, 2026-11-13) contains all of them.
- Change: `vendor/craft-libs` is a submodule at `e2dc681`. Root `[patch."https://github.com/storytold/craft-libs"]` points all 37 craft-libs crates at it. `krilla` stays on the brief's `6e03f65` through the crates-io patch.
- `cargo check` passes. craft-libs duplicates drop to 0, and lockfile packages go from 942 to 926.
- **Kept:** every app's own suite gives exactly the same results with and without it (see "Upstream test results").
- One exception, outside our graph: WordCraft's **desktop binary** (`apps/wordcraft`) pins its own `craft-print` (`a3e0b5a`), and with it moved to `e2dc681` the binary no longer compiles (`craft_print::Job` gained `driver_settings` and `grey_pages`). We never link that binary, so its tests are run without the alignment, and the aligned run uses `--exclude wordcraft`.

## D17 Build inputs. SETTLED 2026-11-14: craft-i18n `1e51542`

- `CRAFT_FONTS_DIR` → `vendor/craft-fonts` at `c1db6d1` (HEAD). No app pins craft-fonts.
- `CRAFT_I18N_DIR` → `vendor/craft-i18n` at `1e51542` (WordCraft's pin). It is 57 commits ahead of PdfCraft's `575cfde` and contains it. One cargo build can only point at one copy.
- **Known cost:** 2 PdfCraft UI tests fail on `1e51542` and pass on `575cfde`:
  - `fonts::primary_ui_fonts_cover_latin_and_cyrillic_catalogs`: the French catalog now uses `▸`, which Inter lacks.
  - `comments::hotbar_picker_searches_translated_tool_labels`: the label "Hotbar settings" changed.
- In the app, that means a missing-glyph box in French PDF menus, plus one label that differs from PdfCraft's tests.
- WordCraft's suite gives identical results on either revision.
- **We keep `1e51542`.** The older revision would drop about 27k lines of translations, most of them WordCraft's (`wordcraft.tsv` across 36 locales, including missing French and Arabic rows), and no test would notice. The newer one costs one glyph and one label in PdfCraft, which PdfCraft upstream will hit as well when it moves to this revision.

## D18 Window commands from the apps are dropped by the host. SETTLED 2026-11-14

- `StartDrag`, `Maximized`, `Minimized`, `Fullscreen`, `OuterPosition`, `InnerSize`, `Decorations` and `Title` (the window is ours; tab titles come from each engine's document list) are **dropped deliberately** and logged at `debug`.
- `CancelClose` and `Close` feed the close state machine.
- Every other command is logged at `warn` (D13).
- This is what makes D6's "remove the window parts" work without patches H2–H4.

## D19 `Editor::open` may finish later. SETTLED 2026-11-14

PdfCraft reads files on a worker thread, so its tab appears on a later frame. `open` returns `Opened::Now(doc)` or `Opened::Later`. For `Later`, the host waits until `find_path(path)` finds the document. Word, Sheet and Deck always return `Now`.

## D20 Closing goes through each app's own "user asked" path. SETTLED 2026-11-14

| Engine | Close as the user would | Prompt open while |
|---|---|---|
| WordCraft | `session.activate` + `WordApp::run("file.close")` | `dialog.is_some()` |
| GridCraft | `enter_window` + `SheetApp::run("file.close")` | `dialog.is_some()` |
| DeckCraft | `session.set_active` + `close_presentation()` | `dialog.is_some()` |
| PdfCraft | `request_close_tab(i)` | `close_request.is_some()` |

`Editor::close_state` reads `Closed` once the document is gone, `Pending` while the prompt is open, and `Cancelled` otherwise. The host asks only after at least one frame has run.

## D21 PdfCraft Save As to a host-chosen path is not available yet. SETTLED 2026-11-14

PdfCraft has no public "save to this path" API; Save As always goes through its own file panel. `PdfEditor::save_as` returns an error saying so, and PdfCraft's own File ▸ Save As works inside the tab. Re-implementing its save (incremental writes, xattr preservation, signature flattening) would break the "never re-implement an editor" rule. A small upstream hook can be added with the Services patch (D12).

## D22 Phase 1 measurements (D1's cost). 2026-11-14

Debug build, `harness --shots`, Apple Silicon. Resident memory:

| Stage | RSS |
|---|---|
| Harness window, no engines | 114 MB |
| + WordCraft shown | 409 MB (+295: its fonts, proofing data and layout caches) |
| + GridCraft shown | 458 MB (+49) |
| + DeckCraft shown | 479 MB (+21) |
| + PdfCraft shown | 481 MB (+2) |
| + 12 more documents (5 each for Word, Sheet, Deck) | 492 MB (+11) |

Font atlas per engine context: Word 8192×64 (2 MB RGBA), the others 8192×32 (1 MB). So a context per engine costs about 5 MB of atlases, small next to the engines themselves. Each atlas is uploaded whole 1–2 times while the engine first starts (fonts install on the frame after `logic`), and **never on a later tab switch** (12 switches checked). The first frame after a switch takes 3.7–6.4 ms of CPU.

## D23 The shell is Rust-only, not Swift. SETTLED 2026-11-14 (owner's decision; changes brief §4.1)

- **Change:** one eframe/winit window (wgpu) in Rust holds our chrome (tabs, new-tab page, sidebar, menus) and the four engines. There is no Swift, no C ABI and no Xcode project.
- **Why:**
  - The Craft apps are Rust-only and already handle macOS input (Retina, trackpad, IME, dead keys) through their winit fork.
  - The Phase 1 harness proved the embedding.
  - The FFI layer and the NSEvent/NSTextInputClient bridge (old Phases 2–3), the riskiest work, disappear.
  - It's one language, matching the Craft house rule "Rust only for native Craft apps".
  - Packaging uses `codesign`, `hdiutil` and `notarytool`, which the Command Line Tools include, as the Craft repos do.
- **Cost:** the chrome is drawn with egui, not AppKit. [design.md](design.md) sets the bar for a modern, consistent look.
- **Native where it matters:** the menu bar (`craft-menu`/`muda`), file panels (`rfd`), clipboard (`arboard`), printing (`craft-print`), accessibility (AccessKit through eframe), updates (`craft-update`).
- **Repo layout now:**
  - `crates/craftworks-core` (Editor, tabs, routing, projects)
  - `crates/craftworks-render` (engine contexts and painting)
  - `crates/craftworks-shell` (chrome UI, design tokens, icons)
  - `crates/craftworks-platform` (Services)
  - `crates/craftworks-mcp`
  - `apps/craftworks` (the app)
  - `apps/craftworks-cli`
  - `packaging/macos`
  
  `craftworks-ffi`, `macos/` and `scripts/build-rust.sh` are dropped.
- **Superseded:** D8. D5 and D9 are adjusted.

## D24 No `unsafe` anywhere. SETTLED 2026-11-14

With no FFI and no raw `CAMetalLayer` pointer, every crate keeps the workspace's `unsafe_code = "forbid"`. The Craft binaries reach macOS services through safe APIs (`arboard`, `muda`, `rfd`, objc2's safe methods), and so do we. If a platform API ever needs `unsafe`, we ask first.

## D25 Look and feel. PROPOSED 2026-11-14

[design.md](design.md):
- The tab strip sits in the title row, beside the traffic lights.
- New-tab and start page with large kind cards and filterable recent files.
- Tokens for light and dark, Inter type, an icon set drawn in code, calm motion, keyboard first.
- One appearance across all tabs.

To be confirmed on the Phase 2 screenshots.

## D26 Window shortcuts. SETTLED 2026-11-14: ⌘T opens tabs (owner's decision)

The shell takes ⌘T (new tab, per the brief), ⌘W (close tab), ⌘O (open) and ⌃Tab / ⌃⇧Tab. The apps' own bindings for these keys:

| Key | Apps' own use | Effect of the shell taking it |
|---|---|---|
| ⌘W, ⌘O | close / open: the same meaning | none (the shell goes through each app's own close and open) |
| ⌘T | WordCraft hanging indent (`para.hangingIndent`-style), GridCraft Insert Table, a DeckCraft Format command | those commands lose their shortcut; they stay on the ribbon |
| ⌘⇧T, ⌘1…9 | WordCraft and GridCraft formatting (line spacing, Format Cells…) | **not taken**: "reopen closed tab" and "go to tab N" need other keys |

The owner chose ⌘T for new tabs, so WordCraft's, GridCraft's and DeckCraft's ⌘T commands (hanging indent, Insert Table, Font dialog) stay reachable from their ribbons only.

Phase 3 made this a table (`craftworks_shell::keys::SHORTCUTS`). A test reads all four apps' registries (`craftworks_core::keymaps`) and fails on any overlap not listed in `keys::INTENDED`. Phase 3 added **⌥⌘T, reopen closed tab**, which no app uses. The test lists the 13 intended overlaps: ⌘T ×3 (above), ⌘W and ⌘O in all four apps (close and open), and ⌃Tab / ⌃⇧Tab in PdfCraft (its next and previous document tab).

Routing: the window's input hook (`App::raw_input_hook`) takes keyboard events out of the window's own egui input. Shortcuts go to the window; everything else goes to the shown document, unless the chrome has the keyboard (the new-tab popover, or the start page's filter). Pointer events go to both.

## D27 One appearance: the window's mode is the engines' "system" mode. SETTLED 2026-11-14

- Each engine is told the mode explicitly (`Editor::set_dark`: WordCraft `ui.theme`, GridCraft `ui.dark`, DeckCraft `view.dark`, PdfCraft `set_theme_preference`).
- Each engine's input also reports the window's mode as the system theme, so everything in an app that follows "System", egui's own visuals included, matches. DeckCraft paints its own colours while its egui visuals follow the system; only the second step makes it fully consistent.
- `--check` judges the result by what's on screen: the mean luminance of each app's title row is ≥ 0.9 in light and about 0.17 in dark.

## D28 Accessibility across contexts: graft in the window's output hook. SETTLED 2026-11-14 (spike done; VoiceOver pass in Phase 8)

- An egui plugin on the window's context (`craftworks_render::a11y::Graft`) appends the shown engine's AccessKit tree to the window's update after the pass, under a "Tab content" node of the chrome.
- egui node ids are 64-bit hashes, so only egui's fixed root id collides; the engine's root is renamed, and screen-reader actions aimed at it are renamed back.
- Engines build their trees only while a client listens (the window produced a tree), so it costs nothing otherwise.
- `--check` turns accessibility on: the merged tree has 176 nodes with our tab list and New tab, and GridCraft's ribbon (Home, Insert…) under the tab content.
- This needs no egui change, and AccessKit 0.25's subtree feature isn't needed.

## D29 WordCraft drops a keystroke that arrives with focus. NOTED 2026-11-14

A `Text` event in the same frame as the window gaining focus is ignored by WordCraft (reproduced headlessly through `EngineHost`; with focus first, typing works). That's upstream behaviour and harmless for users, who focus before typing. `--check` therefore counts the window as focused throughout. Possible upstream issue (asking first).

## D30 GridCraft composes text only in its cell editor. NOTED 2026-11-14

GridCraft has no input-method handling of its own on the grid. Composition (Japanese, Chinese…) works once a cell's editor is open (F2, or a first plain keystroke), not by starting to compose on a selected cell, as Excel allows. Our host passes IME events through unchanged; this is GridCraft's behaviour, the same in its standalone app. Possible upstream issue (asking first).

## D31 WordCraft leaves keyboard focus off the page after its Save prompt. NOTED 2026-11-14

After its "Do you want to save changes…" prompt is cancelled, WordCraft doesn't give keyboard focus back to the page: typing is lost until the page is clicked. Reproduced headlessly through `EngineHost` (`input_path::typing_after_cancelling_wordcrafts_save_prompt`, marked ignored until fixed). Our host could refocus WordCraft's canvas by its internal id, but that would reach into the app; this belongs upstream (asking first).

## D32 The menu bar sends keystrokes. SETTLED 2026-11-14

- The menu bar is `craft-menu`'s (as in PdfCraft), so its macOS layout, Window list, Help search and system-key clash rules are shared with the Craft apps.
- A click on an item that has a shortcut **sends that keystroke to the shown document**, so a click and the key take one path: the app's own handling, its dialogs, its text-field rules. Items without one (Document ▸ Rotate Page) run the app's command by id.
- Cut, Copy and Paste send egui's clipboard events; Paste asks eframe for the clipboard (`RequestPaste`).
- Key equivalents come back as key presses (`NativeMenu::raw_input`) and go through the window's routing (D26).
- The per-kind menu is short on purpose: the common commands, each tested against the app's registry. The ribbon stays the full set.
- Redo shows each app's own key: ⇧⌘Z in WordCraft and PdfCraft, ⌘Y in GridCraft and DeckCraft.

## D33 Quitting and the session. SETTLED 2026-11-14

- Every way to quit (⌘Q, the Dock, logging out, the window's close button) does three things:
  1. saves the open tabs' files,
  2. asks about each unsaved tab in turn with its app's own prompt,
  3. ends with AppKit's `terminate` (`craft_quit::quit`; closing the window first can crash on macOS, per craft-quit).
- Cancel stops the quit and the session keeps saving from then on. Once the quit is decided, the session isn't rewritten: the tabs open when it started are what comes back.
- Untitled documents have no file and aren't restored.
- Window size and position are eframe's own persisted state.

## D34 App bundle. SETTLED 2026-11-14 (identifier to confirm before Phase 8)

- Bundle id `dev.craftworks.Craftworks` (a placeholder; Phase 8 signing needs the real one).
- Document types use neutral names (Document, Spreadsheet, Presentation, PDF) and `LSHandlerRank` **Alternate**: Craftworks offers to open these files but never takes over the user's default app.
- The icon is our own, drawn by `packaging/macos/make_icon.py` at build time. No icon file is committed, so there's no asset row to add.
- Finder's open-documents and the Dock's Quit arrive through `fmv-macos-events`, the safe wrapper the Craft apps use (MIT OR Apache-2.0).

## D35 Platform services live in `craftworks-platform`. SETTLED 2026-11-14

- One crate builds each app's `Services` (`craftworks_platform::editor(kind, &platform)`), from crates already in the four apps' dependency set: `objc2-app-kit` (NSPasteboard), `arboard` (pictures), `rfd`, `craft-print`, `image`. No `unsafe` (D24).
- `Platform::system()` in the app; `Platform::isolated(MemoryClipboard)` for tests and `--check`, which then never touch your clipboard, open a panel or print.
- The Craft binaries' own `clipboard.rs`/`print.rs` aren't libraries, so they weren't copied: the glue follows each app's documented `Services` contract instead.

## D36 Clipboard ownership is per app. SETTLED 2026-11-14

- Each app asks "is this my copy?" to decide between its rich internal paste and the pasteboard's formats. With one shared pasteboard, a single "we wrote last" flag would make WordCraft think GridCraft's copy was its own.
- So each app gets an `Owner` holding the pasteboard change count of its own last copy; a copy is "own" only while the count hasn't moved since.

## D37 Links the engines open. SETTLED 2026-11-14

- `file://` links to a document Craftworks opens go to a tab: that's how DeckCraft's Print (export to PDF, then open it) lands in a PDF tab, ready to print.
- `https`, `http` and `mailto` open in the system (`open --`). Anything else is refused and logged.
- Real printing isn't verified on this Mac: it has no printer (`lpstat: No destinations added`). Adding a virtual PDF printer is a system change, so it waits for the owner.

## D38 Projects are folders with `.craftworks/project.json`. SETTLED 2026-11-16

- Schema version 1: `name`, `pinned` (relative paths with `/`), `ai.instructions`, `ai.permission`.
- Reading is lenient: missing fields take defaults; pins that leave the folder are dropped; an unknown permission becomes `read`.
- A file whose version is newer than this build's is shown read-only and never written.
- Writes are atomic. The list of known projects lives in Craftworks' settings, not in the folders.

## D39 The project folder is polled, not watched with `notify`. SETTLED 2026-11-16

- `notify` isn't in any Craft app's dependencies, and adding one needs the owner's OK. A bounded rescan every 2 s on a background thread is enough for a project folder: at most 20,000 entries and 12 levels deep; symlinks, hidden folders, `node_modules`, `target`, `__pycache__` and `Library` aren't entered.
- It only wakes the window when something changed.
- The index (`craftworks-core::index`) reads files with each app's own headless reader. That needs one new path dependency, `pdfcraft-render`, which is from the PdfCraft repo, so nothing outside the four repos is added. A reader that fails on a damaged file is caught, and the file is listed as "couldn't be read".

## D40 Sidebar shortcuts: ⌃⌘S and ⌃⇧F. SETTLED 2026-11-16 (owner's decision)

- The brief's ⌘\ is PdfCraft's Split View, and the owner chose not to take it. The sidebar uses ⌃⌘S, macOS's standard Show Sidebar key (Finder, Mail, Notes), which no Craft app binds.
- Search uses ⌃⇧F, which no app binds. The design's ⇧⌘F is WordCraft's Font dialog and PdfCraft's Advanced Search, and ⌥⌘F is WordCraft's Insert Footnote.

## Upstream test results (task 0.6)

Runs on 2026-11-14. Every run is logged under `plan/logs/` (gitignored), one timestamped file per run.

### PdfCraft (egui `725a39f` is already its own pin; our run changes `krilla` and the craft-i18n checkout)

**9,640 passed, 19 failed.** None of the 19 is caused by egui or krilla.

| Tests | Count | Cause | Evidence |
|---|---|---|---|
| xattr-preserving saves: `staging::*` ×4, `macos_association::*` ×3, `file_watch::*` ×2, `editing::a_save_preserves…`, `automation::saving_a_pdf_keeps…`, `signing::signing_preserves…` | 12 | `could not preserve the file's extended attributes … Result too large (os error 34)` on **macOS 27.2**. | 11 of the 12 also fail on PdfCraft's unmodified pins, both inside and outside the tool sandbox. `signing::…` has the same cause but was not re-run. `pdfcraft-platform` does not depend on egui. To report upstream (asking first). |
| LibreOffice conversion: `office::*` ×2, `libreoffice_converts…`, `open_worker::file_open_converts…`, `engine office_documents…` | 5 | This machine has a stale Homebrew wrapper: `/opt/homebrew/bin/soffice` points at `/Applications/LibreOffice.app`, which is gone, so it exits 126. | All 5 also fail on unmodified pins. Fix: `brew reinstall --cask libreoffice` or `brew uninstall --cask libreoffice`. |
| `fonts::primary_ui_fonts…`, `comments::hotbar_picker…` | 2 | craft-i18n `1e51542` vs `575cfde` (D17). | Both pass on `575cfde`. |

The aligned run (D16) fails exactly the same 19 tests.

### WordCraft, GridCraft, DeckCraft (move from egui `6d518fc` to `725a39f`)

| App | Aligned run | Failures | Cause | Evidence |
|---|---|---|---|---|
| DeckCraft | 2,262 passed | 0 | — | — |
| GridCraft | 3,875 passed | 1: `caret_after_typing_arabic_is_drawn_at_its_left_end` (caret at x=10, text spans 0–26.9) | Already failing upstream in this environment, probably Arabic font coverage | Also fails on GridCraft's own pins (egui `6d518fc`) |
| WordCraft | 5,475 passed (binary excluded); 5,548 unaligned with the binary | 38, the same with and without D16 and on either craft-i18n: `wordcraft-docx` (glossary, line numbering, run properties, tracked styles) 12, `wordcraft-engine` lib (`fit_one_page`, tracked styles) 12, `strict_export` 10 ("the host has faces covering all of plane 4"), `wordcraft-cli` 4 | Already failing at WordCraft HEAD on this machine: none of these crates depends on egui | All 38 also fail on WordCraft's own pins |

**Conclusion:** moving to egui `725a39f` and aligning craft-libs causes **no** test failures in any app. The only failures our setup causes are PdfCraft's 2 craft-i18n tests (D17).

Upstream reports to offer (each needs the owner's OK first):
- PdfCraft: xattr-preserving saves fail with `ERANGE` on macOS 27.2.
- WordCraft: 38 failures at `514085c`.
- GridCraft: the Arabic caret test.
- PdfCraft: French catalog `▸` not in Inter.

## Duplicate crates (`cargo tree -d`, task 0.9)

With D16, **no craft-libs crate is duplicated**. The rest are third-party crates at different semver versions, which Cargo cannot merge without upstream changes. The list is in `plan/logs/dups-aligned.txt`.

| Crate | Copies | Why it cannot merge |
|---|---|---|
| `read-fonts` | 4 (0.39, 0.41, 0.43, 0.44) | each app's font stack pins a different `skrifa`/`harfrust` generation |
| `skrifa` | 3 (0.42, 0.44, 0.47) | same as `read-fonts` |
| `phf`, `phf_shared`, `phf_generator`, `phf_macros` | 3 each | 0.11, 0.13 and 0.14, from different dependents |
| `getrandom` | 3 (0.2, 0.3, 0.4) | different `rand` generations |
| `vello_cpu`, `vello_common` | 2 each | 0.0.8 is PdfCraft's vendored renderer (hayro); 0.2 is DeckCraft's renderer. Intended |
| `objc2`, `objc2-app-kit`, `objc2-foundation`, `block2` | 2 each | 0.5 and 0.6 generations |
| `zip`, `quick-xml`, `roxmltree`, `png`, `kurbo`, `harfrust`, `font-types`, `hashbrown`, `syn`, `thiserror`, `rand`, `sha1`, `sha2`, `md-5`, `digest`, `base64`, `bitflags`, `miniz_oxide`, and 10 small helpers | 2 each | semver splits, for example `zip` 3 and 9, `quick-xml` 0.41 and 0.42, `kurbo` 0.11 and 0.13, `png` 0.17 and 0.18 |

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
