# Craftworks plan

Status: **approved** 2026-11-14 (D1–D17 settled). **Phase 0 done** 2026-11-14; Phase 1 next.
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
2. Xcode: the owner installs it and runs `xcode-select`. Needed from Phase 2 (`xcodebuild`).
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

## Phase 2: Rendering bridge

Tasks
- 2.1 `craftworks-render`:
  - `Gpu` holds one `wgpu::Instance`, `Adapter`, `Device` and `Queue` for the whole process.
  - `Surface::from_metal_layer(ptr, size, scale)` uses `SurfaceTargetUnsafe::CoreAnimationLayer`. This module is the only place in the crate allowed `unsafe`, and every block has a `// SAFETY:` comment (D9).
  - `resize`, `set_scale`; the present mode is `Fifo`, with `desired_maximum_frame_latency = 1`.
- 2.2 `EngineHost` holds one `egui::Context`, one `Editor` and one `egui_wgpu::Renderer`. `frame(input) -> FrameResult` runs `Context::run_ui`, then tessellates and paints. It returns `PlatformOutput` (cursor, copied text, opened URL, IME rect, AccessKit update), `repaint_delay` and viewport commands, translated into a plain `FrameResult` struct.
- 2.3 Viewport commands: Title goes to the tab title. `CancelClose`/`Close` feed the close state machine. Drag, Maximize and similar commands are ignored or forwarded to Swift. Extra viewports are embedded as floating in-tab windows (D13). **Every unhandled `ViewportCommand` is logged** with engine, viewport id and command.
- 2.4 `craftworks-ffi`: a hand-written `extern "C"` API (D8; `unsafe` allowed here, every block with a `// SAFETY:` comment, D9) and a checked-in `craftworks.h`. A test parses the header and compares it with the exported symbols. API:
  - `cw_host_new(kind)`, `cw_host_free`
  - `cw_open(host, path) -> doc`, `cw_activate(host, doc)`
  - `cw_attach_layer(host, ptr, w, h, scale)`, `cw_resize`
  - `cw_frame(host, timestamp) -> CwFrameResult`
  - `cw_event(host, *const CwEvent)`
  - `cw_request_close(host, doc)`, `cw_poll_close`
  - `cw_set_services(host, *const CwServices)`: a table of C function pointers plus a `void* ctx`
  - Strings and buffers pass as `(ptr, len)`, and Rust-owned results are freed with `cw_free_*`.
- 2.5 `scripts/build-rust.sh`: build release staticlibs for `aarch64` and `x86_64`, combine them with `lipo`, and wrap them with the header as `macos/Craftworks/CraftworksCore.xcframework`.
- 2.6 A minimal `macos/` app: one `NSWindow`, one `EditorView` (an `NSView` backed by a `CAMetalLayer`), WordCraft only. Frames are driven by `NSView.displayLink`.

Done when: Swift shows a WordCraft document drawn by Rust; text is sharp at 2× (pixel check of a screenshot); live resize doesn't stutter (frame time logged at 60 Hz and on a 120 Hz display if available); `xcodebuild build` passes.

## Phase 3: Input bridge

Tasks
- 3.1 `EditorView` turns `NSEvent`s into `CwEvent`s:
  - mouse down, up, moved and dragged, for every button
  - `scrollWheel` with `hasPreciseScrollingDeltas` (points vs lines) and momentum phases
  - `magnify` → `Event::Zoom`
  - keys mapped to `egui::Key` from key codes, so the mapping doesn't depend on keyboard layout, with modifiers: ⌘ → `command`/`mac_cmd`
  - `flagsChanged`, focus in and out, mouse entered and exited.
- 3.2 Text through `NSTextInputClient`:
  - `insertText` → `Event::Text`
  - `setMarkedText` → `Ime::Preedit`, `unmarkText` → `Ime::Commit`
  - `firstRect(forCharacterRange:)` uses the IME rect from `PlatformOutput`.
  - Test manual scripts for Devanagari (Hindi), Japanese Kana–Kanji, emoji picker (⌃⌘Space) and dead keys (⌥E E).
- 3.3 Menu shortcuts vs editor keys: our menu bar takes ⌘T/⌘W/⌃Tab, and everything else goes to the editor first (`performKeyEquivalent` order is documented in decisions.md).
- 3.4 Drag and drop: the view takes `fileURL` drags and sends `hovered_files`/`dropped_files`. A drop on a tab of another type opens a new tab (routing 1.4).
- 3.5 Input tests: a Rust test feeds recorded `CwEvent` scripts into an `EngineHost` and checks engine state (for example: ⌘B makes the selection bold; arrow keys move the GridCraft cell).

Done when: typing, selection, ribbon clicks, ⌘B/⌘Z/⌘S, GridCraft arrow keys, IME composition and dragged-in files all behave as in the standalone apps. Each check is in a checklist in `docs/phase3-checks.md`.

## Phase 4: Swift shell

Tasks
- 4.1 Window: `NSWindow` with `fullSizeContentView` and a transparent title bar. Our **tab strip sits in the title-bar row** next to the traffic lights. Below it is one `EditorView` per engine (D5); only the active engine's view is shown.
- 4.2 Tab strip (SwiftUI inside an `NSHostingView`):
  - type icons drawn by us: blue Document, green Sheet, orange Presentation, red PDF
  - dirty dot, close button, drag to reorder, middle-click to close
  - ⌘T, ⌘W, ⌃Tab / ⌃⇧Tab, ⌘1…9
- 4.3 The "+" chooser (popover): New Document / Sheet / Presentation, Open PDF, Open File…, and Recent (our own list, plus each app's recent files where they expose them).
- 4.4 Menu bar: File, Edit, View and Window are ours. A Format menu for each engine is built from the engine's command catalogue (`run_command`). Edit's Undo, Redo, Cut, Copy and Paste go to the active editor through its own command ids.
- 4.5 Session restore: open tabs and active tab go in `~/Library/Application Support/Craftworks/session.json`; files that are missing on relaunch are skipped with a notice.
- 4.6 Quit: `applicationShouldTerminate` returns `.terminateLater`, then goes through dirty tabs one at a time with the async close flow (finding 9). Cancel stops the quit.
- 4.7 `Info.plist`: document types and UTIs for every format in brief §1, and `application(_:open:)` routes into tabs.

Done when: someone new to the app creates and switches between all four types without seeing a second window; a scripted test (XCUITest) of close and quit with unsaved changes never loses edits.

## Phase 5: Services in Swift

Tasks
- 5.1 `CwServices` implementation: `NSOpenPanel` and `NSSavePanel` as window sheets, async through the inbox for WordCraft, DeckCraft and GridCraft. PdfCraft keeps using `rfd` (D12) until a later upstream patch adds host hooks.
- 5.2 Pasteboard, with the types each app expects (read from each app's `clipboard.rs` and `pasteboard.rs`, without copying them): text, HTML, RTF, PNG and TIFF images, plus each app's "own clipboard" marker.
- 5.3 Printing: the app produces PDF bytes, and `PDFDocument` + `NSPrintOperation` print them for all four types.
- 5.4 `NSWorkspace.open` for URLs; system font loading where the app asks for it (`load_system_fonts`).
- 5.5 Cross-tab paste tests: a sheet range pasted into a document becomes a table; document text pasted into a slide keeps formatting. These are automated through `run_command` and a fake pasteboard, then checked once by hand.

Done when: copy and paste between tabs keeps formatting in both test cases, and printing works for all four types.

## Phase 6: Projects

Tasks
- 6.1 `craftworks-core::project`: a folder plus `.craftworks/project.json` (`name`, `pinned`, `ai.instructions`, `ai.permission` = `read` | `edit` | `edit_save`), with a versioned schema and tests.
- 6.2 Sidebar (SwiftUI `OutlineGroup`):
  - file tree filtered to supported types
  - new file in project, rename, Reveal in Finder
  - an FSEvents watcher (`FSEventStream`) that refreshes the tree.
- 6.3 Text index (`craftworks-core::index`): text from each engine's headless reader (Word text, sheet cell values, slide text and notes, PDF text), held in an in-memory inverted index that updates on file changes. No new dependency: this is a simple token index.
- 6.4 Project search UI: a search field in the sidebar; clicking a result opens the file at the hit where the engine supports it.

Done when: you can create a project, add one of each type, open them from the sidebar, and search across all of them.

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
  - Swift **review panel** listing files the AI changed, and nothing is saved until you approve. Documents use WordCraft track changes where available.
- 7.5 `apps/craftworks-cli`: `craftworks mcp --project <dir>` and `craftworks open <file>`. Docs give a one-line config for Claude Code (`claude mcp add craftworks -- craftworks mcp --project .`) and a JSON snippet for Claude Desktop.
- 7.6 End-to-end scripts for the three scenarios in the brief, with fixture files we generate ourselves.
- 7.7 **Last:** live AI edits to open PDFs: upstream H7 plus a PdfCraft `Live` backend (finding 14, D14).

Done when: with only this server connected, Claude Code completes the three brief scenarios, and in scenario 1 the open `report.docx` tab changes live and the change can be undone.

## Phase 8: Polish and ship

- 8.1 AccessKit: `accesskit_macos` adapter per `EditorView`, fed by `PlatformOutput.accesskit_update`, with a VoiceOver pass over each engine.
- 8.2 Dark mode: `NSApp.effectiveAppearance` → each app's theme preference.
- 8.3 Signing with hardened runtime, notarisation (`notarytool`), Sparkle updates, crash logs. Sparkle is a new dependency (MIT) and needs your approval.
- 8.4 `docs/licences.md`, generated from `cargo metadata` plus each repo's `ATTRIBUTION.*`; fail the build on any licence outside the allowlist.
- 8.5 `.dmg`, then a clean-Mac test on Apple Silicon and Intel.

Done when: a signed, notarised `.dmg` installs and runs on a clean Mac of each architecture.

---

## Risks

- **Upstream churn** of 30–50 commits a day per repo: re-pin once per phase, not continuously, and keep our patches small so they rebase easily.
- **Hidden eframe dependencies** in the apps (storage, `viewport_output`, `raw_input_hook`, wgpu callbacks such as `egui_wgpu::CallbackTrait` that need the `Renderer`'s callback resources): the Phase 1 harness surfaces these before any FFI work.
- **The apps' own window logic**: quit-when-last-window-closes, macOS menu-bar handling inside the apps (GridCraft `native_menu.rs` lives in its binary, so it's not linked) and the Dock. All of it is turned off or routed through hosted mode.
- **Compile time**: about 900 crates. Use a shared `CARGO_TARGET_DIR`, `debug = "line-tables-only"`, and avoid workspace-wide `--release` during iteration.
