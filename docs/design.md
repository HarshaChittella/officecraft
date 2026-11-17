# OfficeCraft design: the shell's look and feel

Status: proposed 2026-11-14, for Phase 2. Covers only **our** chrome: the tab strip, the new-tab and start pages, the sidebar, toasts and the review panel. Everything inside a tab is the Craft app, drawn as the app draws it.

## Principles

1. **Familiar to an office user.** Someone who knows Word, Excel, PowerPoint and Acrobat should know where everything is without being told: a Home tab that never closes, a start screen with *New* and *Recent*, tabs that belong to the document under them, blue for documents, green for workbooks, orange for presentations, red for PDFs, and the names those apps use ("Blank workbook", "Open…", "Recent").
2. **One product.** The shell and the four apps share one ramp of neutral colours (below). The row under the tabs is the same colour in every app, so the active tab is visibly part of it. Each app keeps its own accent.
3. **The document is the hero.** The chrome is one slim row; colour means something (a file's kind, a state).
4. **One window, one appearance.** Light or dark follows the system, and every tab follows it too (PLAN 2.6).
5. **Keyboard first, mouse friendly.** Everything is reachable from the keyboard, with a visible focus ring. Hover reveals secondary controls (×) instead of showing them all the time.
6. **Calm motion.** Short, eased transitions that explain a change. Never block input. With Reduce Motion on, everything is instant.
7. **Familiar, not borrowed.** Every icon, colour and shape is our own. There is no Microsoft or Adobe iconography (the Craft repos' asset rules apply to us too).

## Layout

```
┌──────────────────────────────────────────────────────────────────────────────┐
│ ● ● ●  [▣ Q3 report   •] [▦ Budget        ×] [▤ Pitch deck] [◧ Contract] (+) │  ← 40 pt title row
├──────────────────────────────────────────────────────────────────────────────┤
│ (the app's own title row: Quick Access, file name, search, Share — D6)       │
│ (the app's ribbon)                                                           │
│                                                                              │
│                       (the app's document canvas)                            │
│                                                                              │
└──────────────────────────────────────────────────────────────────────────────┘
```

- **Title row:** sized to centre the traffic lights, which AppKit draws and which we can't move without `unsafe`. It's twice the lights' measured centre: 32 pt on macOS 27. Tabs are the row's height less 6 pt.
  - The traffic lights sit at the left; the tabs start the same gap after them (measured, 82 pt here).
  - Tabs follow, then "+".
  - The row is `band`. Beside the traffic lights: the sidebar toggle, then the **Home tab** (the mark; it never closes), then the tabs and "+". At the right: the AI status pill (Phase 7).
  - The row's background is the window surface (below). It blends into the app's own title row underneath, separated only by a hairline.
- **Sidebar (Phase 6):** 248 pt by default, 200–400 pt resizable, collapsed with ⌃⌘S (D40). It slides in from the left under the title row and pushes the tab content over; it never covers it.

## Tokens

All chrome colours come from these, defined for light and dark. The contrast pairs meet WCAG AA (4.5:1 for text, 3:1 for icons and focus rings).

| Token | Light | Dark | Use |
|---|---|---|---|
| `band` | `#E2E3E9` | `#141416` | the title row behind the tabs: the darkest band of the window |
| `surface` | `#F6F6F8` | `#26262A` | the first row of every app (so the active tab runs into it), the sidebar, Home |
| `surface-raised` | `#FFFFFF` | `#2E2E33` | active tab, cards, popovers |
| `surface-hover` | `#EAEAEF` | `#37373D` | hovered tab, row or button |
| `sunken` | `#EDEDF1` | `#1B1B1E` | text fields, keycaps, empty-state tiles |
| `hairline` | `#00000014` | `#FFFFFF14` | the one line under the title row, dividers |
| `border` | `#00000021` | `#FFFFFF24` | outlines of raised things: active tab, cards, popovers, buttons |
| `text` | `#1A1A1F` | `#EDEDF0` | titles |
| `text-muted` | `#5E5E6A` | `#AEAEB8` | inactive tab titles, metadata |
| `text-faint` | `#9A9AA6` | `#6A6A75` | decoration only: placeholders, the pin on a pinned row |
| `accent` | `#3B5BDB` | `#7A93F0` | focus ring, primary button, selection |
| `accent-ink` | `#FFFFFF` | `#11162E` | text on an accent fill |
| `doc` | `#3B5BDB` | `#7A93F0` | Document kind |
| `sheet` | `#107C41` | `#5CC587` | Sheet kind |
| `deck` | `#D95A1A` | `#F58A4B` | Presentation kind |
| `pdf` | `#D93A3A` | `#F27C7C` | PDF kind |
| `danger` | `#C42B1C` | `#F1705F` | destructive actions, errors |
| `success` | `#107C41` | `#5CC587` | the tick on a toast |

The kind colours are the apps' own accents (WordCraft's blue, GridCraft's green, DeckCraft's orange), so a tab's icon and the app under it agree.

**In the apps** (`vendor/*/crates/ui-egui/src/theme.rs`): each app's first row, tab bar and status bar are `surface`; ribbons and cards `surface-raised`; canvases `#E9E9ED` / `#161618`; borders `#DCDCE1` / `#3C3C43`; text and hover as above. Only these neutrals changed; layouts, icons and accents are the apps' own.

- **Type:** Inter (from craft-fonts, the Craft apps' UI face), regular and semibold. Tab titles 12.5 pt, body 13 pt, metadata 11.5 pt, card and row titles 13 pt semibold, popover headings 14 pt semibold, the start page's title 26 pt semibold set 0.5 pt tight. Section labels ("CREATE", "PINNED") are 10.5 pt semibold capitals set 0.7 pt loose.
- **Spacing:** 4 pt grid (`tokens::space`): 4, 8, 12, 16, 24 and 32. Panels pad 16; rows are 28 pt in the sidebar and 34 pt in panels; controls are 28 pt tall.
- **Radii:** tab 8 pt (top corners only), cards 10 pt, popovers 12 pt, buttons and fields 6 pt, keycaps 4 pt, pills full.
- **Elevation:** popovers, the start panel and toasts float with a soft shadow (0 12 32). The paper in a New tile carries a much fainter one that deepens as it rises. Nothing else casts a shadow.
- **Motion** (`tokens::motion`): everything eases out (cubic) and can be interrupted.
  - 120 ms for hover, press and focus: fills and colours cross-fade, never snap.
  - 180 ms for panels (fade plus 6 pt of travel) and for a tab becoming active (its fill, outline and colour bar grow together).
  - 240 ms for position: tabs, group labels and "+" *glide* to where they belong, so a reorder, a close or a resize never jumps. A new tab fades in and grows from its left edge. Toasts rise in and the ones above glide down as one leaves.
  - The start page's cards arrive one after another, 35 ms apart.
  - Frames are requested only while something is moving; an idle window draws nothing (`tabs::tests`, `tokens::tests`). With Reduce Motion on, all of it is instant.

## Controls

Shared pieces in `officecraft_shell::widgets`, so a hover or a focus looks the same everywhere:

- **Buttons:** 28 pt tall, 12 pt side padding, 12.5 pt semibold. *Primary* is filled with `accent` (one per panel: "Keep", "New Project…"); *secondary* is a `border` outline that fills with `surface-hover`.
- **Icon buttons** ("+", the sidebar toggle): a square the tab's height, icon in `text-muted` turning `text` on hover over a `surface-hover` fill.
- **Search field:** a `sunken` well with a magnifier; the outline and the magnifier turn `accent` while it has the keyboard.
- **Keycap:** a shortcut in a small `sunken` chip (⌘O).
- **Focus ring:** 2 pt `accent`, 2 pt outside the control, only on keyboard focus.

## Icons

All our own, in two families:

- **Things are Liquid Glass tiles.** A kind of file, a folder, a project, Home, the AI's sparkle and a toast's result are artwork drawn in code by `crates/officecraft-shell/assets/make_icons.py` and embedded as images (64 px for small sizes, 256 px for the New tiles). Each is a tinted glass tile lit from above (a sheen across its top, a bright rim fading down its sides, a shade along its foot, a soft shadow under it), with the glyph as a pane of frosted white glass floating over a fainter pane behind:
  - Document: blue, a page with a turned corner and lines of text
  - Sheet: green, a table with a header row
  - Presentation: orange, a slide on a stand with a chart
  - PDF: red, a page with a "PDF" band
  - Home (blue house), folder (amber), project (teal folder with lines), tick (green), alert (red), sparkle (violet)
  
  The glass keeps its own colour; only opacity changes (a missing file's icon is faded, the AI's sparkle breathes).
- **Controls are line glyphs.** Plus, close, chevron (it turns as a folder opens), search, pin, the sidebar toggle and the busy ring are drawn in code on a 16 pt grid with a 1.5 pt round stroke, in the colour they are given, as symbols sit on glass.
- **The mark** is the app icon: a fan of four glass sheets on a blue glass tile (a white page of text in front; green, orange and red sheets behind, one for each kind of file). `packaging/macos/make_icon.py` writes the `.icns` from the same drawing. The Dock shows the bundle's icon (no run-time icon is set), so the Dock, Finder and Launchpad agree.
- Nothing is traced from a vendor's file icons. The apps' own toolbar icons (inside a tab) are theirs and unchanged.

## Tab strip

- **Active tab:** a piece of the app below pulled up into the title row: `surface`, rounded at the top, open at the bottom, outlined up its sides and over its top, its title semibold.
- **Inactive tab:** transparent on `band` with `text-muted`; a rounded fill fades in on hover and the title brightens. A hairline parts two resting tabs.
- **Width:** 120–220 pt. Tabs shrink evenly to the minimum, then the strip scrolls sideways, with 28 pt gradient fades at the edges and a tab-count overflow menu (⌄) at the right end.
- **Content:** kind icon, then the title (truncated in the middle, so `Q3 rep…final.docx` keeps its extension visible), then a 16 pt slot that shows:
  - a dot (●) in the kind's colour when there are unsaved changes,
  - × on hover or when the tab is active (a round `surface-hover` button under the pointer),
  - a busy ring while the file opens (PdfCraft opens in the background).
- **Interactions:**
  - click to switch, middle-click to close
  - drag to reorder: the tab lifts with a small shadow, and the others slide aside
  - drop files on the strip to open them; a caret shows where they'll land
  - double-click empty strip space to zoom the window, as the system title bar does
  - right-click for Close, Close Others, Close Tabs to the Right, **Add to Project ▸** (New Project…, then every project; the one the file is already in is ticked), Reveal in Finder, Copy Path. A project is a folder, so adding a file saves a copy of the document into it and the tab carries on with that copy; the original stays where it was, and nothing is replaced (a taken name becomes "Name 2"). PDFs are copied as they are on disk (save first if changed).
- **Tooltip** (after 600 ms): full path, kind, and "Unsaved changes" when dirty.
- **Closing:** a clean tab collapses over 200 ms. A dirty tab shows the app's own Save prompt in the tab (D20), and the tab stays until it's answered.

## Home (the new tab)

```
┌ ● ● ●  ▯ [▤] [▣ Q3 report ×] [▦ Budget] (+)                      ✦ AI · idle ┐
├───────────────┬──────────────────────────────────────────────────────────────┤
│ ▤ OfficeCraft  │  Good afternoon                                              │
│               │                                                              │
│ ⌂ Home        │  NEW                                                         │
│ ▢ Open…   ⌘O  │  [ page ]   [ grid ]   [ slide ]   [ PDF ]                   │
│ ▣ Open Project│  Blank      Blank      Blank       Open a PDF                │
│               │  document   workbook   presentation                          │
│               │                                                              │
│               │  RECENT                                  [ Search recent… ]  │
│               │  Name                Location                     Opened     │
│               │  ▣ Q3 report.docx    ~/Work/Finance               2 h ago    │
│ Tip: ⌘T …     │  ▦ Budget.xlsx       ~/Work/Finance               Yesterday  │
└───────────────┴──────────────────────────────────────────────────────────────┘
```

- **Home is the new tab.** It is what OfficeCraft opens to, and what "+", ⌘T and the Home tab show: a page of the window in place of the document, never a panel over it (as Acrobat's Home and a browser's new tab are). Choosing a tab, anything on the page, or Esc puts it away; nothing is closed.
- **It lays itself out to the window.** The column's margins and width grow with the window (up to 1160 pt); the New tiles share the width (four across, two in a narrow window, one in a very narrow one) and their icons grow with them; the search field takes a third of the row.
- **The rail** (232 pt; dropped under 780 pt of width): the mark and name, then Home (marked with a bar of `accent`, as the office apps mark the current place), Open… (⌘O) and Open Project….
- **New:** four tiles, each the kind's glass icon on a `sunken` desk. **New document, New workbook and New presentation open that app's own New page** (WordCraft's, GridCraft's and DeckCraft's File ▸ New: a blank file or any of its templates) in a new tab; picking a template there replaces the blank stand-in the page stands on. **Open a PDF** asks for the file. On hover the icon rises 3 pt and grows a touch, and the desk takes the kind's colour.
- **Recent:** a table (Name, Location, Opened) of up to 12 files with a search field; long names and folders end in "…". A missing file is dimmed with "Not found", and choosing it offers to remove it. With none, a two-line empty state says so.
- **Quick blank files:** hold "+" for 0.4 s (or right-click it) for a menu of New Document, New Sheet, New Presentation and New PDF, each with its glass icon; they open a blank file straight away, with no page in between. A blank PDF is one empty page, made by PdfCraft and not yet saved.
- **Keyboard:** focus starts in the search field. The arrow keys move through tiles, Open and rows; Enter opens; Esc goes back to the document.

## Toasts and status

- Short, non-blocking notices slide in at the bottom-centre of the window and stay for 4 s, or until hovered. Examples: "Couldn't open budget.xls: no engine reads .xls", "Restored 4 tabs", "Saved".
- An action can be offered in the toast ("Show in Finder", "Undo close").
- Every toast leads with an icon: a `success` tick, or for errors an alert in `danger` (the icon only, never the whole toast).

## Tab groups (Phase 6b)

- One per open project, as Chrome groups tabs. The label is a full pill in the group's colour with the project name (semibold 11.5 pt), before its first tab; a 2 pt line of the colour runs under the group's tabs.
- Clicking the label collapses the group to just the label, with the number of hidden tabs. Right-click: name field, nine colour swatches (Grey, Blue, Red, Yellow, Green, Pink, Purple, Cyan, Orange), Collapse/Expand, Show in Sidebar, Close Project.
- Label text meets WCAG AA on every colour in both appearances (`tokens::tests`).

## Sidebar (Phase 6)

- **Opening:** the toggle beside the traffic lights or ⌃⌘S; it slides in and out.
- **Header:** the project's badge (a folder on an `accent` tile) and name with a switcher (⌄), and a search field (⌃⇧F; ⇧⌘F is WordCraft's Font and PdfCraft's Advanced Search) that searches across every file in the project.
- **Sections:** Pinned, then Files, as a tree. It shows only supported types and greys out other files when "Show all files" is on.
- **Rows:** 28 pt tall, with kind icon and name; the row of the file in front is tinted with `accent`. A folder's chevron turns as it opens. Pinned rows end in a small pin.
- **No project:** an empty state with a primary "New Project…" and a secondary "Open Folder as Project…". Files open in a tab with a single click; a tab already showing the file is brought to the front.
- **Context menu:** New Document / Sheet / Presentation here, Rename, Reveal in Finder, Pin.

## AI status and review (Phase 7)

- **Pill** (a full pill; it fills with an `accent` tint when there is something to review, and its sparkle breathes while the AI is editing) at the right of the title row: `✦ AI · idle`, `✦ AI · editing Budget.xlsx`, or `✦ AI · 3 changes to review`. Clicking it opens the review panel.
- **Review panel:** a popover listing changed files. Each entry has its kind icon, a summary of the changes, a secondary Undo and a primary Keep. "Save all" saves only when permissions allow it.

## Accessibility

- Every chrome control has an AccessKit label and role.
- Tabs are a tab list, and the active one is "selected".
- The focus ring is a 2 pt `accent` outline offset 2 pt, shown only on keyboard focus.
- The chrome's minimum target size is 28 pt.
- Increased Contrast swaps `hairline` and `text-muted` for stronger versions.
