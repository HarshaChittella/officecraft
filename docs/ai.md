# The AI connector

OfficeCraft has one MCP server for all four kinds of file: documents (WordCraft), sheets
(GridCraft), presentations (DeckCraft) and PDFs (PdfCraft), plus the project folder around them.

## Connect it

**In the app:** click **Connect AI** at the right of the tab row (it's there until an AI has been in touch, then it shows what the AI is doing). For the project in front it offers:

- **Add to Claude Desktop:** adds OfficeCraft to Claude Desktop's settings file (every other setting kept; a copy saved first as `claude_desktop_config.json.before-officecraft`). Quit and reopen Claude.
- **Copy Setup Command:** the Claude Code command below, for that project, to paste in Terminal.
- **The AI may:** read only (the default), edit, or edit and save, written to the project's `.officecraft/project.json`.
- ChatGPT is named but can't connect: it only reaches connectors on the internet, and this one runs on your Mac.

By hand:

**Claude Code**, from the project folder:

```bash
claude mcp add officecraft -- /Applications/OfficeCraft.app/Contents/MacOS/officecraft mcp --project .
```

While developing, use the built binary instead: `~/src/officecraft/target/release/officecraft`, or `target/measure/officecraft`.

**Claude Desktop**: add this to `~/Library/Application Support/Claude/claude_desktop_config.json`, replacing the project path:

```json
{
  "mcpServers": {
    "officecraft": {
      "command": "/Applications/OfficeCraft.app/Contents/MacOS/officecraft",
      "args": ["mcp", "--project", "/Users/you/Documents/Q3 plans"]
    }
  }
}
```

`--all-tools` lists every tool. Without it the server is compact: it lists about ten core tools,
and `tool_search` and `tool_call` reach the rest.

## What it can do

- `project_active`: what the user has in front of them in OfficeCraft (the active tab's file and kind) and the other open tabs, for "summarise this document".
- `project_files`: every document, sheet, presentation and PDF, with which tools work on it and whether it's open in the window.
- `project_search`: search across every kind of file. Each hit names the file and where in it (paragraph, `Sheet1!B4`, slide, page).
- `project_read`: the text of any file, in parts.
- `project_open`: opens a file in an OfficeCraft tab.
- `project_save`: saves a file the AI edited, if the project allows it.
- `doc.*`, `sheet.*`, `deck.*`: WordCraft's, GridCraft's and DeckCraft's own tools. Each takes `file`, a path in the project.
- `pdf.*`: PdfCraft's own tools, with paths confined to the project. Give them `file` to work on a PDF by its path; when it's open in OfficeCraft, the edit happens live in its tab.

## Live edits and review

When a file is open in the OfficeCraft window, the AI's edits go to its tab:

- You watch the edit happen.
- Each edit is an ordinary command, so the app's Undo takes it back.
- The AI pill at the right of the tabs shows what's happening, for example "AI · editing report.docx" or "AI · 2 changes to review".
- Click the pill for the review panel. **Keep** saves a file; **Undo** takes back the AI's edits to it.
- Until you keep a file, nothing is saved. WordCraft's and GridCraft's AutoSave pause while an AI's edits wait for review.

Files that aren't open are worked on headless, without a window, in each app's own engine.

## Permissions

Set per project in `.officecraft/project.json`, as `"ai": {"permission": …}`:

| Permission | The AI may |
|---|---|
| `read` (the default) | Read and search only. |
| `edit` | Edit. An edit to a file that isn't open opens it in OfficeCraft first, so you review it there. Saving stays with you. |
| `edit_save` | Edit and save, including files that aren't open. |

`"ai": {"instructions": "…"}` is passed to the AI when it connects.

## Safety

- Every path the AI names stays inside the project folder: `..`, absolute paths elsewhere and links that lead out are refused.
- The window and the server talk over a Unix socket in OfficeCraft's settings folder. Each launch writes a new random token there that only you can read (mode 0600), and a request without it is refused.

## Testing it

```bash
python3 scripts/ai-scenarios.py
```

The script runs the brief's three scenarios through the connector with fixed steps. It opens an OfficeCraft window for scenario 1. The real acceptance test is Claude Code doing the same.
