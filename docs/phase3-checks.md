# Phase 3 checks: input and keyboard routing

Each Phase 3 "done when" item, how it's verified, and its status. **Auto** items run on every build (`cargo test`, or `craftworks --check DIR`). **Hand** items need a person at a real keyboard, trackpad and Finder, because they depend on macOS input sources that a test can't drive.

Status as of 2026-11-14.

## Keyboard routing (window shortcuts vs. the document)

| Check | How | Status |
|---|---|---|
| ⌘T, ⌘W, ⌘O, ⌥⌘T, ⌃Tab, ⌃⇧Tab go to the window, never to the document (key press and release) | Auto: `craftworks-shell` `keys::tests::shortcuts_go_to_the_window…` | ✅ |
| No window shortcut takes an app's key except those taken on purpose (D26) | Auto: `keys::tests::window_shortcuts_collide_only_where_intended`, against all four apps' registries | ✅ (13 intended overlaps, listed in D26) |
| Keys typed into the window reach the shown document | Auto: `--check` "keys typed into the window reach the shown document" (through the window's input hook) | ✅ |
| ⌘T opens the new-tab popover; typing there stays out of the document | Auto: `--check` | ✅ |
| ⌃Tab moves to the next tab; ⌘W closes the tab; ⌥⌘T reopens it from its file | Auto: `--check` | ✅ |
| A field the closed popover left focused doesn't keep the keyboard | Fixed in Phase 3; covered by the `--check` routing stage | ✅ |
| Plain Tab, arrows and typing never move focus in the chrome | Auto (by construction: the window's egui no longer sees the document's keys) | ✅ |

## Typing, shortcuts and composition in the apps (through the real `EngineHost` path)

| Check | How | Status |
|---|---|---|
| ⌘A, ⌘B bolds in WordCraft; ⌘Z undoes it | Auto: `input_path::command_b_bolds_and_command_z_undoes_in_wordcraft` | ✅ |
| Japanese composition (preedit, then commit) in WordCraft | Auto: `input_path::ime_composition_reaches_wordcraft` | ✅ |
| Hindi (Devanagari), emoji, dead-key accent (´ then é) in WordCraft | Auto: `input_path::devanagari_emoji_and_dead_keys_reach_wordcraft` | ✅ |
| GridCraft arrows, typing + Enter, composition in the cell editor | Auto: `input_path::arrows_typing_and_ime_reach_gridcraft` | ✅ (composition needs the cell editor open, D30) |
| The engine reports its caret, so the input method switches on and places its candidates there | Auto: `input_path::wordcraft_reports_its_text_cursor_for_the_input_method` | ✅ |
| Typing after cancelling WordCraft's Save prompt | Auto, marked *ignored*: `input_path::typing_after_cancelling_wordcrafts_save_prompt` | ❌ upstream (D31) |
| Real Hindi input source (Devanagari – QWERTY), typing नमस्ते in a document and a sheet cell | Hand | ⏳ |
| Real Japanese input source (Kana–Kanji): type nihon, Space to convert, Enter, in a document and a cell (after F2) | Hand | ⏳ |
| Emoji picker (⌃⌘Space) into a document | Hand | ⏳ |
| Dead keys with the US keyboard: ⌥E then E gives é; ⌥U then U gives ü | Hand | ⏳ |
| ⌘B, ⌘I, ⌘Z, ⌘S, ⌘C / ⌘V between two tabs, with the real keyboard | Hand | ⏳ |

## Pointer

| Check | How | Status |
|---|---|---|
| Pointer positions move into the document's coordinates; a press on the tab strip stays with the window; a release always reaches the document | Auto: `embed::tests` | ✅ |
| Scrolling or pinching over the tab strip leaves the page alone | Auto: `embed::tests::the_wheel_scrolls_the_document_only_while_the_pointer_is_over_it` | ✅ |
| Trackpad scroll is precise (points) with momentum; mouse wheel scrolls in lines | Passed through unchanged (`MouseWheelSource`, `TouchPhase`); Hand to confirm the feel | ⏳ |
| Pinch zooms the page (WordCraft, DeckCraft, PdfCraft) | Hand | ⏳ |
| The cursor matches the app (I-beam over text, cell cross in GridCraft, resize arrows) | Hand | ⏳ |
| A drag started in the page (selecting text) and released over the tab strip ends cleanly | Auto (release rule) + Hand | ✅ / ⏳ |

## Drag and drop

| Check | How | Status |
|---|---|---|
| Documents dropped anywhere open in tabs; other files go to the shown document | Auto: `drops::tests` (the split rule) | ✅ |
| Dropping a .docx, .xlsx, .pptx and .pdf from Finder onto the tab strip opens them at the drop position | Hand | ⏳ |
| Dropping a PNG onto a document inserts it (WordCraft's own drop handling) | Hand | ⏳ |

## How to run the hand checks

```bash
cd ~/src/craftworks && cargo run -p craftworks --profile measure
```

Then work through the ⏳ rows. Add input sources in System Settings ▸ Keyboard ▸ Text Input if needed. Note anything that behaves differently from the standalone apps.
