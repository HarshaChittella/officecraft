# Craftworks design: the shell's look and feel

Status: proposed 2026-11-14, for Phase 2. Covers only **our** chrome: the tab strip, the new-tab and start pages, the sidebar, toasts and the review panel. Everything inside a tab is the Craft app, drawn as the app draws it.

## Principles

1. **The document is the hero.** The chrome is one slim row and stays quiet: no borders where spacing will do, no colour except to mean something (a file's kind, a state).
2. **One window, one appearance.** Light or dark follows the system, and every tab follows it too (PLAN 2.6).
3. **Keyboard first, mouse friendly.** Everything is reachable from the keyboard, with a visible focus ring. Hover reveals secondary controls (×, the reorder handle) instead of showing them all the time.
4. **Calm motion.** Short, eased transitions that explain a change (a tab sliding into place, the new-tab page fading in). Never block input. With Reduce Motion on, everything is instant.
5. **Familiar, not borrowed.** It behaves like a modern browser's or editor's tabs, but every icon, colour and shape is our own. There is no Microsoft or Adobe iconography (the Craft repos' asset rules apply to us too).

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
  - At the right: a sidebar toggle (Phase 6) and an AI status pill (Phase 7).
  - The row's background is the window surface (below). It blends into the app's own title row underneath, separated only by a hairline.
- **Sidebar (Phase 6):** 248 pt by default, 200–400 pt resizable, collapsed with ⌘\. It slides in from the left under the title row and pushes the tab content over; it never covers it.

## Tokens

All chrome colours come from these, defined for light and dark. The contrast pairs meet WCAG AA (4.5:1 for text, 3:1 for icons and focus rings).

| Token | Light | Dark | Use |
|---|---|---|---|
| `surface` | `#F4F4F6` | `#1C1C1F` | title row, sidebar |
| `surface-raised` | `#FFFFFF` | `#2A2A2E` | active tab, cards, popovers |
| `surface-hover` | `#E8E8EC` | `#2F2F34` | hovered tab or row |
| `hairline` | `#00000014` | `#FFFFFF14` | the one line under the title row, card outlines |
| `text` | `#1D1D21` | `#ECECEF` | titles |
| `text-muted` | `#62626C` | `#9A9AA5` | inactive tab titles, metadata |
| `accent` | `#4F6BED` | `#7C8CFF` | focus ring, primary button, selection |
| `doc` | `#3B82F6` | `#60A5FA` | Document kind |
| `sheet` | `#16A34A` | `#4ADE80` | Sheet kind |
| `deck` | `#D9650B` | `#FB923C` | Presentation kind |
| `pdf` | `#DC3545` | `#F87171` | PDF kind |
| `danger` | `#D92D20` | `#F97066` | destructive actions, errors |

- **Type:** Inter (from craft-fonts, the Craft apps' UI face). Tab titles 12.5 pt medium, page titles 22 pt semibold, body 13 pt, metadata 11.5 pt. Tabular numbers in counts.
- **Spacing:** 4 pt grid. Common steps are 4, 8, 12, 16, 24 and 32.
- **Radii:** tab 8 pt, card 12 pt, popover 12 pt, buttons and fields 6 pt, pills full.
- **Elevation:** only popovers and the new-tab panel float, with a soft shadow (0 8 24 at 12% opacity in light, 40% in dark). Nothing else casts a shadow.
- **Motion:** 140 ms ease-out for hover and press, 200 ms for a tab moving or appearing, 160 ms fade and scale (0.98 → 1) for the new-tab panel. None of these with Reduce Motion on.

## Icons

- Drawn in code (vector paths), 16 pt on a 1.5 pt stroke, like the Craft apps' own icon sets.
- **Kind icons:** a sheet of paper with a folded corner, and inside it a simple glyph in the kind's colour:
  - Document: three text lines
  - Sheet: a 2×2 grid
  - Presentation: a slide with a bar chart
  - PDF: a page with a stamp
  
  The icons are ours, not lookalikes of any vendor's file icons.
- **Chrome icons:** plus, close, chevrons, sidebar, search, sparkle (AI) and folder, drawn the same way. Lucide (ISC) may be used as a source, with `ATTRIBUTION.md` rows, as PdfCraft does.

## Tab strip

- **Active tab:** `surface-raised`, `text`, and a 2 pt bar along its bottom edge in the kind's colour. It merges visually into the app's title row below.
- **Inactive tab:** transparent with `text-muted`; `surface-hover` on hover.
- **Width:** 120–220 pt. Tabs shrink evenly to the minimum, then the strip scrolls sideways, with 24 pt fades at the edges and a tab-count overflow menu (⌄) at the right end.
- **Content:** kind icon, then the title (truncated in the middle, so `Q3 rep…final.docx` keeps its extension visible), then a 16 pt slot that shows:
  - a dot (●) when there are unsaved changes,
  - × on hover or when the tab is active,
  - a spinner while the file opens (PdfCraft opens in the background).
- **Interactions:**
  - click to switch, middle-click to close
  - drag to reorder: the tab lifts with a small shadow, and the others slide aside
  - drop files on the strip to open them; a caret shows where they'll land
  - double-click empty strip space to zoom the window, as the system title bar does
  - right-click for Close, Close Others, Close Tabs to the Right, Reveal in Finder, Copy Path
- **Tooltip** (after 600 ms): full path, kind, and "Unsaved changes" when dirty.
- **Closing:** a clean tab collapses over 200 ms. A dirty tab shows the app's own Save prompt in the tab (D20), and the tab stays until it's answered.

## New-tab page and start page

```
                    ┌────────────────────────────────────────────────────┐
                    │  Create                                            │
                    │  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌──────────┐│
                    │  │   ▣      │ │   ▦      │ │   ▤      │ │   ◧      ││
                    │  │ Document │ │  Sheet   │ │  Deck    │ │ Open PDF ││
                    │  └──────────┘ └──────────┘ └──────────┘ └──────────┘│
                    │  [ Open File…  ⌘O ]                                │
                    │                                                    │
                    │  Recent                      [ Filter recent…   ]  │
                    │  ▣ Q3 report.docx    ~/Work/Finance    2 h ago     │
                    │  ▦ Budget.xlsx       ~/Work/Finance    Yesterday   │
                    │  ◧ Contract.pdf      ~/Downloads       Mon         │
                    └────────────────────────────────────────────────────┘
```

- **Opening it:** "+" or ⌘T opens it as a popover anchored under "+". With no tabs, the same content fills the window as the start page, centred, with the app name above it.
- **Cards:** 132×112 pt each: a large kind icon (32 pt), a label, and on hover a soft tint of the kind's colour.
- **Recent:** up to 12 entries, each with kind icon, file name, folder (shortened with `~`) and relative time. Typing filters the list straight away. A missing file is shown dimmed with "Not found", and choosing it offers to remove it.
- **Keyboard:** focus starts on the filter field. The arrow keys move through cards and rows, Enter opens, Esc closes the popover.

## Toasts and status

- Short, non-blocking notices slide in at the bottom-centre of the window and stay for 4 s, or until hovered. Examples: "Couldn't open budget.xls: no engine reads .xls", "Restored 4 tabs", "Saved".
- An action can be offered in the toast ("Show in Finder", "Undo close").
- Errors use `danger` for the icon only, never for the whole toast.

## Sidebar (Phase 6)

- **Header:** the project name with a switcher (⌄), and a search field (⌘⇧F) that searches across every file in the project.
- **Sections:** Pinned, then Files, as a tree. It shows only supported types and greys out other files when "Show all files" is on.
- **Rows:** 26 pt tall, with kind icon and name; the selected row is `surface-raised`. Files open in a tab with a single click; a tab already showing the file is brought to the front.
- **Context menu:** New Document / Sheet / Presentation here, Rename, Reveal in Finder, Pin.

## AI status and review (Phase 7)

- **Pill** at the right of the title row: `✦ AI · idle`, `✦ AI · editing Budget.xlsx`, or `✦ AI · 3 changes to review`. Clicking it opens the review panel.
- **Review panel:** a popover listing changed files. Each entry has its kind icon, a summary of the changes and Keep / Undo buttons. "Save all" saves only when permissions allow it.

## Accessibility

- Every chrome control has an AccessKit label and role.
- Tabs are a tab list, and the active one is "selected".
- The focus ring is a 2 pt `accent` outline offset 2 pt, shown only on keyboard focus.
- The chrome's minimum target size is 28 pt.
- Increased Contrast swaps `hairline` and `text-muted` for stronger versions.
