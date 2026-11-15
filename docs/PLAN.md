# Craftworks plan

Status: **approved** 2026-11-14. **Phases 0–2 and 4–6 done**, Phase 3 done except its hand checks (Phase 5: printing on paper unverified, no printer), 2026-11-14 (decisions D1–D29; D25 look and feel awaits the owner's confirmation).
**Architecture changed 2026-11-14 (D23): the shell is Rust-only (eframe/winit), not Swift.** Phases 2–8 below are rewritten for it; the look and feel is specified in [design.md](design.md). Phase 2 is next. Xcode is not required.
Companions: [decisions.md](decisions.md) (open and settled choices), [upstream-patches.md](upstream-patches.md) (hooks we need in the Craft repos).

This plan turns the brief's §5 into tasks. Each phase lists its tasks, then the checks that must pass before the next phase starts. After every phase: `cargo test`, `cargo clippy --all-targets -- -D warnings`, `xcodebuild build` (from Phase 2), and screenshots where the UI changed.

---

## What the repos showed (differences from brief §3)

Read at HEAD on 2026-11-14: wordcraft `514085c`, gridcraft `c0ee120`, deckcraft `69379cf`, pdfcraft `e98dc17`. These are 97, 145, 93 and 104 commits past the brief's pins.

| # | Brief said | Found |
|---|---|---|
| 1 | Pin to `1327d20` / `6dbfbc4` / `203fa79` / `37fde37` or newer | All four repos move fast (about 30–50 commits a day). We pin to whatever HEAD is when Phase 0 starts and record the SHAs. |
| 2 | Force PdfCraft from egui `725a39f` down to `6d518fc` | `725a39f` is **4 commits ahead** of `6d518fc` and 0 behind: lint fixes, JS syntax highlighting, a `Resize` corner fix, and patch notes E22/E23 (19 files, about 200 lines). Pinning everyone to `725a39f` loses nothing. Forcing PdfCraft down drops fixes it may depend on. **Settled: `725a39f` for everyone (D7).** |
| 3 | Every app has `new(session, services)` | Only WordCraft, GridCraft and DeckCraft do. `PdfCraftApp::new()` takes no arguments and there is **no `Services` struct**. It calls `rfd` directly (52 call sites) and has a few host hooks in `hooks.rs` (`Inbox`, `OsEvent`, `HostSaveFn`, `HostDirtyFn`). |
| 4 | PdfCraft only needs `logic`/`ui` without `eframe::Frame` | It also uses `eframe::App::raw_input_hook` (pointer and drag fixes), `save(&mut dyn Storage)` (its settings via `persist()`) and `on_exit`. The wrapper has to call all of these, not just `logic` and `ui`. |
| 5 | Swift fills the `Services` callbacks | The desktop implementations of those callbacks live in each **app binary** (`apps/<app>/src/clipboard.rs`, `print.rs`, `file_dialogs.rs`, `apple_events.rs`), not in the library crates. We can't link them, so the host provides each one (Swift, Phase 5). Before that, a `None` callback falls back to egui's own clipboard through `PlatformOutput`, so Phase 2 has to handle `PlatformOutput` properly. |
| 6 | GridCraft and WordCraft keep many documents in one instance | True. Details: **WordCraft** turns native windows on with `cfg(not(target_arch = "wasm32"))`. Its single-window switch, `Windows::single()`, exists but is `#[cfg(test)]`, so we need a small patch to expose it. **GridCraft**'s `DocWindows` is opt-in from the binary, so a host can just call `show_window(id)` and `ui()`. **DeckCraft**'s `Session` also holds many documents (`documents()`, `active_index()`). |
| 7 | (not covered) | All four apps open **extra egui viewports**: WordCraft `windows.rs`, DeckCraft `doc_window.rs` and the presenter view, PdfCraft `wheel_reset.rs`. They also send 13–25 `ViewportCommand`s each (title, drag, maximize, close/cancel-close, focus). Without eframe, the host must act on `FullOutput.viewport_output`. Until then we set `ctx.set_embed_viewports(true)`, so extra windows draw as floating egui windows inside the tab. |
| 8 | Fonts: merge once, or use a `Context` per tab | Apps call `set_fonts` **again at runtime**: on a language change, when a CJK or Arabic fallback is needed, and in GridCraft whenever the fonts used in the workbook change (`font_definitions_with(language, used)`). A single shared `Context` would need deep patches to all four apps. This rules it out (D1). |
| 9 | `request_close() -> CloseAnswer` | Each app's close flow is **asynchronous**. The host sends a close event, the app replies `CancelClose` and shows its own Save / Don't Save / Cancel dialog, and later sends `Close` or nothing. So `request_close` is a small state machine (`Pending` → `Closed` / `Cancelled`), not a single return value. |
| 10 | craft-libs at 2–3 revisions | At HEAD there are about **8** craft-libs revisions: WordCraft uses 4 (`01315d9`, `762a06a`, `6e03f65`, `75c70fa`), GridCraft `bb57d42`, DeckCraft `63196e6`, PdfCraft `e2dc681`. `craft-update` alone shows up at 4 revisions. DeckCraft patches `krilla` to `63196e6` (`=0.8.2`), but the brief's patch block uses `6e03f65`, so this must be re-verified. |
| 11 | (not covered) | Build inputs: `CRAFT_I18N_DIR` is pinned to **different revisions** (WordCraft `1e51542`, PdfCraft `575cfde`), but one `cargo` invocation sees only one value. `CRAFT_FONTS_DIR` is also needed for full font coverage. Phase 0 has to find one craft-i18n revision that both apps accept, or ask upstream to align. |
| 12 | `accesskit_macos` 0.27.2 | The lockfiles have **0.27.1**. Minor. |
| 13 | ≈400k lines of egui code | About **309k** lines in the four `ui-egui` crates (W 97k, G 56k, D 47k, P 108k). The conclusion doesn't change. |
| 14 | MCP entry points | WordCraft's `tool_definitions()` returns a `Value` (an array); the other two return `Vec<Value>`. WordCraft's `Headless` lives in `lib.rs`. Remote auth differs per app: WordCraft sends a key with every request, GridCraft sends an `auth` line with a token. **PdfCraft has no live or remote backend at all**: `Automation` works on files only, and its UI control channel is separate (`--control <file>`). Live PDF edits are new work. |
| 15 | House rules | All four repos set `unsafe_code = "forbid"`, and the shared rules forbid `unsafe` unless a repo names an exception. The licence allowlist has **no MPL-2.0**, which excludes UniFFI and cbindgen as dependencies (settled in D8, D9). |
| 16 | Environment | This Mac has **no Rust toolchain** and **only the Command Line Tools, not Xcode**, so `xcodebuild` fails. The project path contains spaces and a curly apostrophe (`<name>’s MacBook Pro`), which is a known risk for build scripts, `xcodebuild` and some `build.rs` files. The folder is not a git repo yet. See "Before Phase 0". |

---

## Before Phase 0

1. Rust: installed 2026-11-14 with the official rustup installer (stable 1.100.0, targets `aarch64-apple-darwin` and `x86_64-apple-darwin`).
2. Xcode: **no longer required** (D23). Packaging uses `codesign`, `hdiutil` and `notarytool`, which come with the Command Line Tools, as in the Craft repos. Xcode stays optional for the Metal debugger and Instruments.
3. Location: `~/src/craftworks` (D10).
4. Forks: under the owner's GitHub account, added as remote `fork` (D15).

---

## Phase 0: Workspace and pins

Tasks
- 0.1 `git init`; add `.gitignore` (`target/`, `plan/`, `macos/build/`, `*.xcframework`).
- 0.2 Add submodules `vendor/{wordcraft,gridcraft,deckcraft,pdfcraft}` at the SHAs of the day, and the optional build inputs `vendor/craft-fonts` and `vendor/craft-i18n`. Write the SHAs into `docs/decisions.md`.
- 0.3 Root `Cargo.toml`: a workspace with members `crates/*` and `apps/*` only. Vendored repos are path dependencies, not members. Add the brief's patch block with egui rev `725a39f` (D7), and copy the shared lints (`unsafe_code = "forbid"` and the clippy denials) into `[workspace.lints]`.
- 0.4 `crates/craftworks-core`, a stub that depends on the four `*-ui-egui` crates and the `*-engine` crates. `cargo check` should pass.
- 0.5 `.cargo/config.toml`: `CRAFT_FONTS_DIR` and `CRAFT_I18N_DIR` pointing into `vendor/`. Pick one craft-i18n revision that builds and tests for WordCraft and PdfCraft (finding 11), and record the reasoning.
- 0.6 `scripts/upstream-tests.sh`: run each repo's own `cargo test --workspace` **inside its vendored checkout**, with the shared pins applied through a generated `[patch]` override (`--config`), so we test their code on our pins without editing their files. Start with PdfCraft's UI tests (`pdfcraft-ui-egui`, `egui_kittest`).
- 0.7 Align craft-libs revisions where one revision satisfies every user: try `[patch."https://github.com/storytold/craft-libs"]` per crate, keep each one that builds and tests, and revert the rest.
- 0.8 Fork the four repos to the owner's account and add each as remote `fork` (D15).
- 0.9 `cargo tree -d` → table of duplicates in decisions.md: crate, versions, who pulls each, aligned or why not.

Done when: `cargo check` passes for the workspace; every upstream suite passes, or each failure is listed with its cause in decisions.md; the duplicates table is written.

## Phase 1: Rust editor host (no Swift)

Tasks
- 1.1 Upstream hosted-mode patches H1–H6 (upstream-patches.md), each a small branch in our fork, with each repo's own tests passing (`cargo xtask ci` where it runs).
- 1.2 `craftworks-core::Editor` trait. It covers **one engine instance** that holds many documents (D2):
  - `kind()`, `documents() -> Vec<DocInfo{id, title, path, dirty}>`, `activate(doc)`
  - `logic(ctx)`, `ui(ui)`, `raw_input_hook(ctx, &mut RawInput)`
  - `new_document()`, `open(path) -> DocId`, `save(doc)`, `save_as(doc, path)`
  - `request_close(doc) -> CloseTicket`, then `poll_close(ticket) -> Pending | Closed | Cancelled` (finding 9)
  - `run_command(id, args) -> Result<Value>`, wrapping each app's `run`, `execute` or `run_command`
  - `persist() -> String`, `restore(&str)` for settings
- 1.3 Adapters `WordEditor`, `SheetEditor`, `SlideEditor`, `PdfEditor`, each in its own module and filling in that app's `Services` with what the host provides. Initially that's `None` everywhere. PdfCraft keeps its own `rfd` dialogs (D12).
- 1.4 Routing: `Kind::for_path(&Path)` by extension, as in brief §5 Phase 1. `.doc` opens read-only. Tests cover every extension, uppercase, no extension, and unknown types.
- 1.5 Tab model in core: `Tab { id, kind, doc }`, an ordered list, active tab, reorder, close. This is pure data, shared later by the FFI and the harness.
- 1.6 `apps/harness` (eframe, temporary): an egui tab strip at the top, then a **child `egui::Context` per engine** drawn into the area below. The harness runs each engine's context itself and paints with egui-wgpu into the same frame. This is a rehearsal of Phase 2's renderer, so Phase 2 can reuse the code.
- 1.7 Measure memory per engine context (font atlas, textures) with 1 and with 5 documents per engine. Record it in decisions.md.
- 1.8 Screenshots of each tab: `harness --screenshot <dir>`, written with `egui_kittest` or a `ui.screenshot`-style readback.

Done when: the harness opens one document of each type in tabs; switching tabs swaps the ribbon; editing and saving work in every tab; switching shows no font flash (no `set_fonts` on switch, and no glyph-atlas rebuild in the frame log); four screenshots exist.

**Result (2026-11-14): all met.** `harness --shots` reports ALL PASS with 0 warnings ([report](screenshots/phase1/harness-report.txt)):
- Four tabs, one per kind, each showing its full app (title row, ribbon, canvas, status bar).
- 12 switches with no font-atlas rebuild.
- Edit → dirty → Save As → clean in Document, Sheet and Presentation, and edit → Save in place for the PDF.
- Typing reaches WordCraft, and arrow keys + typing + Enter reach GridCraft, through the input path.
- A dirty close shows WordCraft's own Save prompt, and Cancel keeps the document.
- Screenshots: [Document](screenshots/phase1/tab-document.png), [Sheet](screenshots/phase1/tab-sheet.png), [Presentation](screenshots/phase1/tab-presentation.png), [PDF](screenshots/phase1/tab-pdf.png), [Save prompt](screenshots/phase1/dirty-close-prompt.png), [typed in sheet](screenshots/phase1/typed-in-sheet.png).
- Core: 20 tests, including 8 that drive the real engines. Render: 7 tests.
- Not covered yet: real keyboard, mouse and IME from macOS (the scripted input goes through the same path, but it isn't NSEvent), and PDF Save As to a chosen path (D21).

## Phase 2: The app window and its chrome

The harness becomes the real app, `apps/craftworks`: one eframe/winit window (wgpu) with our chrome around the four engines. The chrome follows [design.md](design.md).

Tasks
- 2.1 `crates/craftworks-shell`: the shell's UI, separate from the engines:
  - design tokens (colours light and dark, type scale, spacing, radii, motion), and an icon set drawn in code
  - the tab strip, the new-tab page, the start page and the status toasts.
  Everything is egui, with no editor code (brief §0).
- 2.2 The window: `ViewportBuilder` with a full-size content view and a transparent, hidden title bar. **Our tab strip sits in the title-bar row**, beside the traffic lights, which are inset to centre in it. The strip's empty space drags the window, and double-click zooms it (the macOS behaviour). Native window tabbing is off.
- 2.3 Tab strip:
  - tabs with the type icon, a title that truncates in the middle, and a dirty dot that becomes × on hover
  - tabs shrink to fit, then scroll with fades at the edges
  - drag to reorder (animated), and drag a file onto the strip to open it
  - middle-click closes; a context menu offers Close / Close Others / Reveal in Finder / Copy Path
  - hovering a tab shows a tooltip with the full path
  - shortcuts: ⌘T, ⌘W, ⌘⇧T (reopen the last closed tab), ⌃Tab / ⌃⇧Tab, ⌘1…9.
- 2.4 New-tab page (⌘T or "+"):
  - a centred panel with four large cards (New Document, New Sheet, New Presentation, Open PDF) and "Open File…"
  - recent files with type icon, folder and relative time, filtered as you type
  - fully usable from the keyboard: arrows, Enter, Esc.
  - With no tabs open, the same panel is the start page.
- 2.5 Rendering: the harness's texture path (`craftworks-render`) becomes production code. Each engine paints into its own texture, which the window composites, so a tab switch is a texture swap. The full-window extra blit is measured and kept under 1 ms. Repaint uses each engine's `repaint_delay`, so an idle window draws nothing. Background engines tick at 4 Hz (D4).
- 2.6 One appearance: the window follows the system's light or dark mode, and each engine is told to use the same mode at startup and whenever it changes. A Document tab never shows dark while a Sheet tab shows light (Phase 1 screenshots show that problem). Each app's theme preference is set through its own option or command. Where one is missing, that's an upstream patch.
- 2.7 Retina and resize: the scale changes when the window moves between displays; live resize redraws every frame without stutter; frame times are logged at 60 Hz and on a 120 Hz display.

Done when:
- The app opens the start page.
- One tab of each kind can be made and switched between, with the chrome matching design.md in light and dark. There are screenshots of both.
- Resizing is smooth, with frame times recorded.
- An idle window uses no CPU (Activity Monitor shows 0%).
- The Phase 1 checks still pass, run as `craftworks --check DIR`.

**Result (2026-11-14): all met, light and dark.** `craftworks --check DIR --appearance light|dark` on an optimised build reports ALL PASS, 31 checks each ([light report](screenshots/phase2/report-light.txt), [dark report](screenshots/phase2/report-dark.txt)):
- Chrome: start page, tab strip in the title row (sized around the traffic lights, 32 pt on macOS 27), ⌘T new-tab popover with cards and filterable recent files, toasts. Screenshots: [start page](screenshots/phase2/start-page-light.png), [new tab](screenshots/phase2/new-tab-light.png), [Document](screenshots/phase2/tab-document-light.png) / [dark](screenshots/phase2/tab-document-dark.png), [Sheet](screenshots/phase2/tab-sheet-light.png) / [dark](screenshots/phase2/tab-sheet-dark.png), [Presentation](screenshots/phase2/tab-presentation-light.png), [PDF](screenshots/phase2/tab-pdf-dark.png), [Save prompt](screenshots/phase2/dirty-close-prompt-dark.png).
- One appearance: every app's chrome measures light (luminance ≥ 0.90) or dark (≤ 0.18) as the window does (D27).
- Every Phase 1 check still passes: 12 switches with no font rebuild, edit and save in every tab, typing through the input path, the app's own Save prompt.
- Resize: 0.8–2.1 ms of engine work per frame at 4 window sizes, at 2×.
- Idle: 0.2–0.3 % CPU in the background (5 frames in 10 s); 5.1–5.2 % with a focused document, where WordCraft animates its caret's blink. That's under its 8 % budget, but not zero (it isn't zero standalone either).
- Accessibility spike (D28): the window's tree has our chrome with GridCraft's ribbon grafted in, 176 nodes.
- Memory: 120 MB at the start page, about 420 MB with all four engines, 431 MB with 16 documents.
- Tests: 53 across the workspace (core 23, render 11, shell 18, app 1). clippy `-D warnings` and fmt are clean.
- Not covered: real keyboard, IME and trackpad from macOS (Phase 3); a 120 Hz display measurement (none attached); keys for reopen-closed-tab and go-to-tab-N (⌘⇧T and ⌘1…9 stay with the apps, D26).

## Phase 3: Input and keyboard routing

The Craft apps' winit fork already handles macOS input (Retina, precise trackpad scroll, pinch, IME through `NSTextInputClient`, dead keys). This phase makes sure all of it reaches the right engine intact.

Tasks
- 3.1 Shortcut routing table (`craftworks-shell::keys`): the shell takes only its own shortcuts (2.3, plus ⌘\ for the sidebar and ⌘⇧O for Quick Open), and every other key goes to the active engine. A test loads each app's keymap (`Kind`'s command catalogue) and fails if a shell shortcut collides with an app's.
- 3.2 IME through the embedding: the engine's `IMEOutput` (owner, generation, rects) reaches the window unchanged except for the offset, and IME events reach the engine. Manual checks: Hindi (Devanagari), Japanese Kana–Kanji, emoji picker (⌃⌘Space), dead keys (⌥E E), in each engine.
- 3.3 Pointer details: precise versus line scrolling and momentum, pinch → `Zoom`, hover leaving the engine area, drags that start in the engine and end over the tab strip, and the cursor icon.
- 3.4 Drag and drop: files dropped on a tab or the strip open in tabs (routing 1.4); files dropped on the engine area go to the engine (for example, an image into a document).
- 3.5 Input tests: recorded event scripts fed into an `EngineHost` (the harness's injection path), checking engine state: ⌘B bolds the selection, ⌘Z undoes, arrow keys move the GridCraft cell, IME preedit then commit inserts text.

Done when: typing, selection, ribbon clicks, ⌘B/⌘Z/⌘S, GridCraft arrows, IME composition and dragged-in files behave as in the standalone apps. Each check is ticked in `docs/phase3-checks.md`.

**Result (2026-11-14): the automated part is done; the hand checks wait for a person at the keyboard** ([checklist](phase3-checks.md)).
- `craftworks --check` reports ALL PASS in light and dark. That includes the new routing stage, which goes through the window's real input hook: typing reaches the document, ⌘T opens the popover and its typing stays out of the document, ⌃Tab, ⌘W, and ⌥⌘T reopening from the file.
- Input tests drive the real `EngineHost` path: ⌘A/⌘B/⌘Z in WordCraft; Japanese, Hindi, emoji and dead-key input in WordCraft; GridCraft arrows, typing and composition; the caret reported for the input method.
- Fixed: the window's egui no longer sees the document's keys; a stale focus left by the popover no longer holds the keyboard; wheel and pinch over the tab strip no longer scroll the page; dropped documents open in tabs instead of going to the shown app too.
- Found upstream: GridCraft composes only in its cell editor (D30); WordCraft leaves focus off the page after its Save prompt (D31).
- Still to do by hand: the real Hindi and Japanese input sources, the emoji picker, dead keys, the trackpad feel and pinch, cursors, and drags from Finder.

## Phase 4: Menus, session, quit and file associations

Tasks
- 4.1 Native macOS menu bar through `craft-menu`/`muda` (already used by the Craft apps).
  - File, Edit, View and Window are ours.
  - A menu per engine (for example, Format) is built from that engine's command catalogue and rebuilt when the active tab's kind changes.
  - Edit's Undo, Redo, Cut, Copy and Paste go to the active engine's own commands.
- 4.2 Session restore: open tabs, their order, the active tab and window geometry go in `~/Library/Application Support/Craftworks/session.json`. Missing files are skipped with a toast.
- 4.3 Quit (⌘Q, Dock, logout): goes through every tab with unsaved changes one at a time, using each app's own prompt (D20). Cancel stops the quit. Built on `craft-quit` where it fits.
- 4.4 App bundle and file associations: `packaging/macos/` scripts, modelled on the Craft repos' scripts.
  - `Info.plist` with document types and UTIs for every format in brief §1, and the app icon.
  - Files opened from Finder (Apple Events) arrive through winit and are routed into tabs.

Done when: someone new to the app makes and switches between all four types without seeing a second window; a scripted check of close and quit with unsaved changes never loses edits; double-clicking a `.docx`, `.xlsx`, `.pptx` and `.pdf` in Finder opens each in a tab.

**Result (2026-11-14): done; two things are left to you (below).**
- **Menu bar** (`craft-menu`, D32):
  - Craftworks (Quit), File (New Tab, New Document/Sheet/Presentation, Open…, Open Recent, Close Tab, Reopen Closed Tab, Save, Save As…, Print…), Edit (Undo, Redo, Cut, Copy, Paste, Select All, Find…), View (next and previous tab), a menu for the shown kind (Format for documents, sheets and presentations; Document for PDFs), Window (the tabs), and Help.
  - `--check` clicks through it: Format ▸ Bold then Edit ▸ Undo in WordCraft, Window ▸ a tab, Document ▸ Rotate Page in PdfCraft.
  - A test checks that every shortcut a menu sends is one the shown app binds.
- **Quit** (⌘Q, Dock, logout, the close button), D33: the tabs open at that moment are saved, then each tab with unsaved changes gets its app's own prompt. Cancel stops the quit and keeps the edit; Don't Save lets it finish; the process ends with AppKit's `terminate` (craft-quit). `--check` covers Cancel and Don't Save.
- **Session restore:** `session.json` is saved whenever the tabs change, and restored at a launch without files. Missing files are reported in a toast.
- **App bundle:** `packaging/macos/bundle.sh` builds `build/Craftworks.app` with document types and UTIs for every format, an icon drawn in code (`make_icon.py`) and an ad-hoc signature. `scripts/finder-open-test.sh` passes 3 of 3: four files sent the way Finder sends them become four tabs; the Dock's Quit ends the app; a relaunch restores the tabs whose files still exist.
- `--check`: 42 checks, ALL PASS in light and dark. 72 unit and integration tests pass, plus 1 ignored (upstream, D31).
- **Left to you:**
  - Making Craftworks the *default* app for these types (Finder ▸ Get Info ▸ Open with ▸ Change All). It's a system setting, so I didn't change it. The bundle declares the types as an Alternate handler, so Open With works now.
  - A look at the menu bar itself: it's outside the window, so the screenshots can't show it.

## Phase 5: Services in Rust

Tasks
- 5.1 `crates/craftworks-platform`: the `Services` callbacks each app asks for, built from the same safe crates the Craft binaries use: `arboard` for the clipboard (text, HTML, RTF, images), `rfd` for file panels, `craft-print` for printing, and `NSWorkspace`/`open` for URLs. **No `unsafe`** (D24). Where a binary's own module (`clipboard.rs`, `print.rs`) does what we need, ask upstream to move it into a library crate we can depend on, rather than copying it.
- 5.2 Pasteboard formats as each app expects them (each app's own "copied from me" marker included), so a copy in one tab pastes with formatting in another.
- 5.3 Printing for all four kinds: the app produces PDF bytes and `craft-print` prints them.
- 5.4 Cross-tab paste tests: a sheet range pasted into a document becomes a table, and document text pasted into a slide keeps formatting. These are automated through `run_command` and the clipboard, then checked once by hand.

Done when: copy and paste between tabs keeps formatting in both test cases, and printing works for all four kinds.

**Result (2026-11-14): done except printing on paper, which this Mac can't show (no printer).**
- **`crates/craftworks-platform`** (D35): the `Services` for WordCraft, GridCraft and DeckCraft, with no `unsafe`.
  - Clipboard: the macOS pasteboard through objc2's safe calls (plain text, HTML, RTF, MathML), and `arboard` for pictures. Each app has its own "this is my copy" token, so a copy in one tab is never mistaken for the pasting app's own (D36).
  - File panels: `rfd`. Printing: `craft-print` on a worker thread. Links: only web, mail and file links open (D37).
  - PdfCraft keeps its own panels, clipboard and printing (D12).
- **Cross-tab paste** (5.4), automated twice:
  - `cargo test -p craftworks-platform --test cross_tab_paste`: a GridCraft range pasted into WordCraft becomes a 3×2 table; bold WordCraft text pasted into DeckCraft becomes a text box with bold runs.
  - `--check`: ⌘C on a sheet range, then ⌘V in the document tab, gives a table (screenshot `pasted-table-*.png`). The check uses a private clipboard, never yours.
- **Printing** (5.3): WordCraft and GridCraft hand their PDF to `craft-print`; DeckCraft's Print exports a PDF that opens in a Craftworks PDF tab, where PdfCraft prints it; PdfCraft prints its own. `--check` covers DeckCraft's route. A test covers printer discovery and that a missing printer is an error, not a crash.
- The WordCraft binary's own clipboard and print modules aren't a library, so instead of copying them (5.1), the glue is written against the `Services` contract each app documents. Asking upstream to move them into a library is still open.
- `--check`: 45 checks, ALL PASS in light and dark (new: ⌘C/⌘V sheet → document, DeckCraft Print → PDF tab). 80 unit and integration tests pass, plus 1 ignored (upstream, D31). fmt and clippy `-D warnings` clean.
- The check itself got steadier:
  - It now asks for its own frames between steps, so a quiet app can't stall it (a run took 5 minutes, or hit the time limit).
  - It waits 4 seconds of wall time before measuring idle CPU, so new documents finish their first work.
  - It measures again if the pointer moves over the window during the 10 seconds.
  - **Open:** in about 1 run in 8, idle CPU in the background lands just over its 1% budget (1.1%) with only a few frames, so something on a worker thread is busy. One run also hung once and the log was lost; it hasn't come back in 10 runs. Both are on the Phase 8 list.
- **Left to you:**
  - Printing on paper: `lpstat` lists no printers on this Mac. Either print once on a real printer (⌘P in each kind), or allow me to add a virtual PDF printer (a system change).
  - ⌘C / ⌘V between two tabs with the real keyboard and your own clipboard (already in docs/phase3-checks.md).

## Phase 6: Projects

Tasks
- 6.1 `craftworks-core::project`: a folder plus `.craftworks/project.json` (`name`, `pinned`, `ai.instructions`, `ai.permission` = `read` | `edit` | `edit_save`), with a versioned schema and tests.
- 6.2 Sidebar (`craftworks-shell`, see design.md "Sidebar"):
  - collapsible with ⌃⌘S (D40; the brief's ⌘\\ is PdfCraft's Split View), resizable
  - project switcher, Pinned and Files sections, and a file tree filtered to supported types with type icons
  - new file in the project, rename, Reveal in Finder
  - a file watcher that refreshes the tree (`notify`, if it's already in the Craft graph; otherwise ask first).
- 6.3 Text index (`craftworks-core::index`): text from each engine's headless reader (Word text, sheet cell values, slide text and notes, PDF text), held in an in-memory inverted index that updates on file changes. No new dependency: this is a simple token index.
- 6.4 Project search UI: a search field in the sidebar; clicking a result opens the file at the hit where the engine supports it.

Done when: you can create a project, add one of each type, open them from the sidebar, and search across all of them.

**Result (2026-11-16): done; a look by hand is left to you (below).**
- **Projects** (`craftworks-core::project`, D38): `.craftworks/project.json` with `version`, `name`, `pinned`, `ai.instructions` and `ai.permission` (`read`, the default and what anything unknown means; `edit`; `edit_save`). Pins can't point outside the folder. A file from a newer Craftworks is shown but never overwritten. The known projects and the open one are kept in `projects.json` in the settings folder.
- **Sidebar** (`craftworks-shell::sidebar`):
  - ⌃⌘S shows or hides it (D40, the macOS Show Sidebar key); it's resizable from 200 to 400 pt.
  - The switcher has the known projects, New Project…, Open Folder as Project… and "Show all files".
  - Below the switcher: a search field, then Pinned and Files. The tree shows supported documents with kind icons, and long names end in "…".
  - The right-click menu has New Document/Sheet/Presentation Here, Rename… (refused while the file is open in a tab), Reveal in Finder, and Pin.
- **Watcher** (`craftworks-core::tree`, D39): rescans every 2 s on a background thread and only reports changes. There's no new dependency: `notify` isn't in the Craft apps' set.
- **Index** (`craftworks-core::index`): each file is read with its own app's reader (WordCraft's `open_path`, GridCraft's `open_bytes`, DeckCraft's `read_presentation_file` including tables and notes, and PdfCraft's text layer), into an in-memory token index on a background thread. Every word must match and the last also matches as a prefix. Hits are paragraphs, cells, slides or pages.
- **Opening a hit** goes to it. The document runs Find for the word; the sheet selects the cell; the presentation goes to the slide; the PDF goes to the page (a Craftworks command, `craftworks.goToPage`, on PdfCraft's own `go_to_page`).
- **Search shortcut:** ⌃⇧F. The design's ⇧⌘F is WordCraft's Font and PdfCraft's Advanced Search, and ⌥⌘F is WordCraft's footnote.
- The collision test now treats `\` and `Backslash` as one key. It had been missing punctuation clashes: ⌘\, the brief's sidebar key, is PdfCraft's Split View, so the sidebar uses ⌃⌘S (owner's choice, D40).
- `--check`: 54 checks, ALL PASS in light and dark. The new ones:
  - the folder becomes a project;
  - 4 files are listed and read;
  - New Presentation Here makes a file;
  - a saved change is found again;
  - "payment", "north", "revenue" and "launch" each find and open their file at the hit;
  - a pin is saved.
  
  Screenshots are `project-search-*.png` and `project-files-*.png`. Idle CPU in the background stays at 0.3% with the watcher on. 91 tests pass, plus 1 ignored (D31).
- **Left to you:** create a project with New Project…, drag a file of each type into its folder in Finder, then search and right-click in the sidebar.

## Phase 6b: Projects as tab groups (owner's request, 2026-11-16)

Each open project is a tab group, as in Chrome, and the sidebar stays (D41).

**Result (2026-11-16): done.**
- **Model** (`craftworks-core::tabs`): groups with a key (the project folder), name, colour and collapsed flag.
  - A group's tabs always sit together.
  - A tab opened from inside a group goes after the group.
  - A dragged tab stays inside its own group, and an ungrouped tab can't be dropped into the middle of one.
  - Collapsing the front tab's group moves to the nearest tab outside it; ⌃Tab skips hidden tabs; showing a tab expands its group.
- **Strip** (`craftworks-shell::tabs`):
  - a coloured label before each group's tabs and a line of its colour under them;
  - a click on the label collapses or expands the group, and a collapsed label shows how many tabs it hides;
  - the label's menu has Rename, nine colours, Collapse/Expand, Show in Sidebar and Close Project.
  
  The layout now uses measured slots instead of equal widths. All nine colours pass WCAG AA for the label text in both modes.
- **Projects** (`apps/craftworks/src/projects.rs`):
  - several projects can be open at once, each with its own watcher and index;
  - a tab joins the group of the project that holds its file (the innermost, if folders nest);
  - the sidebar follows the front tab's project and stays on one picked in it;
  - name and colour are saved in `project.json` (`color`), and the open projects and collapsed groups in `projects.json`;
  - Close Project closes each tab through its app's own close, then the project, and a cancelled close keeps it open.
- **Bugs found and fixed:**
  - Opening a file that's already open made DeckCraft open a second copy. A tab that already shows the file now comes to the front.
  - Search results didn't refresh when a file was read again with the counts unchanged. The index now has a generation number.
  - `--check` waits for real-time events (rescans, file reads, animations) in seconds, not frames.
- `--check`: 59 checks, ALL PASS in light and dark. The new checks:
  - the project's tabs sit together;
  - a second project becomes a second group;
  - collapse hides a group's tabs;
  - rename and colour are saved;
  - Close Project removes the group.
  
  Screenshots are `tab-groups-*.png` and `tab-groups-collapsed-*.png`. 97 tests pass, plus 1 ignored (D31).

## Phase 6c: Menus that follow the front tab (owner's request, 2026-11-16)

Each app's own menus and shortcuts in the macOS menu bar, swapped when another kind of tab comes to the front (D42). Craftworks keeps only its app menu, File's tab items and Window.
- PdfCraft: its own `menus.rs` (in its library).
- WordCraft, DeckCraft, GridCraft: built from each app's command catalogue (title, ribbon location, shortcut), so they match the ribbons. GridCraft's hand-made menu lives in its binary, which we can't link.
- Done when: the menu bar shows each app's menus with its shortcuts, a test checks that every menu shortcut is one that app binds, and `--check` runs an item from each app's menus.

**Result (2026-11-16): done.**
- `craftworks-core::catalog`: each app's commands with label, ribbon place, shortcut and whether they need arguments (a menu click can't give any, so those are left out unless they have a shortcut).
- **Ribbon tabs map onto Mac menus:** File, Edit, View, Insert, Format, Table, Data, Formulas, Arrange, Slide Show, Tools.
  - Home's Clipboard and Editing groups go in Edit.
  - Contextual tabs (Picture Format, Chart Design…) are submenus.
  - A long menu shows each ribbon group as a submenu (Format ▸ Font, Paragraph, Styles).
  - Caret keys (the Editing and Navigation tabs) stay out.
  
  WordCraft gets 471 items, GridCraft 219, DeckCraft 174.
- **PdfCraft:** its own menu bar (`Editor::native_menus`), with clicks run by its own `run_menu_command`. Its Edit menu replaces ours, since it handles its own clipboard.
- **What the window keeps:**
  - the app menu (Quit);
  - File's tab items (New Tab, New…, Open, Open Recent, Close Tab, Reopen);
  - Edit's Undo, Redo, Cut, Copy, Paste, Select All and Find;
  - View's tab items, plus Show or Hide Sidebar (⌃⌘S) and Search Project (⌃⇧F);
  - Window.
  
  An app item with a window shortcut (⌘T, ⌘W…) shows no shortcut.
- **Tests:** every keystroke an app's menu item sends is one that app binds, and no app item shows a window shortcut. `--check` brings each kind to the front and checks its menus, and runs PdfCraft's View ▸ Zoom In from the bar. 64 checks pass in light and dark.

## Phase 7: One AI connector

Tasks
- 7.1 `craftworks-mcp` (library):
  - merges `tool_definitions` from the four engines with prefixes `doc.`, `sheet.`, `deck.`, `pdf.`
  - dispatches each call to that engine's `call_tool` or `Automation::call`
  - project tools: `project_files`, `project_search`, `project_read`, `project_open`.
- 7.2 Compact mode by default, as in PdfCraft: about 10 core tools plus `tool_search` and `tool_call`.
- 7.3 Backends per engine: `Headless`, or `Live`. `Live` connects to the running app over a Unix socket in the app's container, with a per-launch token file (mode 0600, the way GridCraft's `control_auth` works). In the app, the socket server sends requests to the engine's existing control handlers on the main thread, so the AI's edits are normal commands and can be undone. This task covers Word, Sheet and Deck; PDF stays headless until 7.7.
- 7.4 Safety:
  - a project-root jail for every path (canonicalise, refuse symlinks that leave the root, like PdfCraft's `--root`)
  - the permissions from `project.json`
  - a **review panel** in the shell listing files the AI changed; nothing is saved until you approve. Documents use WordCraft track changes where available.
- 7.5 `apps/craftworks-cli`: `craftworks mcp --project <dir>` and `craftworks open <file>`. Docs give a one-line config for Claude Code (`claude mcp add craftworks -- craftworks mcp --project .`) and a JSON snippet for Claude Desktop.
- 7.6 End-to-end scripts for the three scenarios in the brief, with fixture files we generate ourselves.
- 7.7 **Last:** live AI edits to open PDFs: upstream H7 plus a PdfCraft `Live` backend (finding 14, D14).

Done when: with only this server connected, Claude Code completes the three brief scenarios, and in scenario 1 the open `report.docx` tab changes live and the change can be undone.

**Result so far (2026-11-16): 7.1–7.6 done; 7.7 (live PDF edits, H7) and the run with Claude Code itself are open.** Setup and use are in `docs/ai.md`.
- **7.1–7.2** `crates/craftworks-mcp`:
  - one MCP server (JSON-RPC over stdio, no new dependency);
  - WordCraft's, GridCraft's and DeckCraft's own tools prefixed `doc.`, `sheet.`, `deck.`, each with a `file` argument;
  - PdfCraft's own MCP server inside it as `pdf.*`, confined to the project by its own root;
  - `project_files`, `project_search`, `project_read`, `project_open`, `project_save`;
  - compact by default (11 core tools plus `tool_search` and `tool_call`).
- **7.3 Live** (D44):
  - **Window side:** the window listens on a Unix socket in its settings folder, with a per-launch token (mode 0600). It answers on the main thread by lending the open tab's session to the app's own `Headless` backend (`Editor::control`), so an AI's edit is an ordinary command: shown at once, and undone by the app's own Undo. Tested in WordCraft, GridCraft and DeckCraft.
  - **Server side:** a file open in the window is edited there; other files run headless.
- **7.4 Safety:**
  - Every path goes through the project jail: `..`, absolute paths outside and escaping symlinks are refused, and so are path arguments inside tools.
  - `project.json` permissions: `read` (default) refuses edits; `edit` routes an edit into the window for review; only `edit_save` saves. The window re-checks the permission when asked to save.
  - Review: the AI pill in the title row, and a panel with Keep (save) or Undo per file.
  - WordCraft's and GridCraft's AutoSave pause while edits wait for review (D45). Without that, WordCraft had already saved the AI's edit.
  - Not done: WordCraft track changes for AI edits. Undo per file covers it for now.
- **7.5:** the same binary is the CLI: `craftworks mcp --project DIR [--all-tools]` and `craftworks open FILE…` (D43). There's no separate `apps/craftworks-cli`.
- **7.6:** `scripts/ai-scenarios.py` runs the brief's three scenarios through the real binary with fixture files the connector makes itself: **9/9 pass**. Scenario 1 has a real window with `report.docx` open: the table appears in the tab, the `edit` project doesn't save it, and Undo takes it back. `--check` has an AI stage (a stand-in AI on its own thread, through the real connector): read the sheet, write a table into the open document, nothing saved after 4 s, the review panel lists it, its Undo takes it back. Screenshot `ai-review-*.png`.
- **Tests:** 71 `--check` checks pass in light and dark; 107 tests pass, plus 2 ignored.
- **Left:**
  - 7.7, live PDF edits (an upstream PdfCraft hook, H7).
  - Your run with Claude Code (`docs/ai.md` has the one-line setup).

## Phase 8: Polish and ship

- 8.1 Accessibility: one AccessKit tree for the window. The shell's nodes, with the active engine's tree grafted under the tab's content node, go to eframe's `accesskit_macos` adapter. Then a VoiceOver pass over the chrome and each engine. (Combining trees from several egui contexts is the main risk here; it's spiked early in Phase 2.)
- 8.2 Reduced motion, increased contrast and Dynamic Type-like text sizing are respected by the chrome.
- 8.3 Signing with hardened runtime, notarisation (`notarytool`), updates through `craft-update` (what the Craft apps use; no new dependency), and crash logs.
- 8.4 `docs/licences.md`, generated from `cargo metadata` plus each repo's `ATTRIBUTION.*`; fail the build on any licence outside the allowlist.
- 8.5 `.dmg`, then a clean-Mac test on Apple Silicon and Intel.

Done when: a signed, notarised `.dmg` installs and runs on a clean Mac of each architecture.

---

## Risks

- **Upstream churn** of 30–50 commits a day per repo: re-pin once per phase, not continuously, and keep our patches small so they rebase easily.
- **Several egui contexts in one window**: AccessKit trees, IME ownership and the cursor have to be merged into the one window. Phase 1 settled input and painting; AccessKit is spiked early in Phase 2.
- **The apps' own window logic**: quit-when-last-window-closes, macOS menu-bar handling inside the apps (GridCraft `native_menu.rs` lives in its binary, so it's not linked) and the Dock. All of it is turned off or routed through hosted mode.
- **Compile time**: about 900 crates. Use a shared `CARGO_TARGET_DIR`, `debug = "line-tables-only"`, and avoid workspace-wide `--release` during iteration.
