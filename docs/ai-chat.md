# Ask AI: a chat built into OfficeCraft

Status: **stage 1 (Ask) and stage 2 (Do) built 2026-11-17** (`apps/officecraft/src/chat/`);
stages 3–4 not yet.

**Stage 1 as built:** the **Ask AI** button in the title row opens the panel at the right. With a
project open and a credential set up, you ask; Claude (Opus 5.5, effort medium) reads and searches
the project through `project_active`, `project_files`, `project_search`, `project_read` and
`project_open` only, and answers with its sources as chips that open the file at the cell, slide,
page or passage. Each question carries a note of what's in front and what else is open. Chats are
saved in `.officecraft/chats/`; the menu in the panel's header switches between them, starts a new
one, and removes the key. The credential is, first match wins: `ANTHROPIC_API_KEY`; a key pasted in
the panel (macOS Keychain, service "OfficeCraft"); or the `ant` sign-in when chosen in the panel
(a token from `ant auth print-credentials --access-token`, billed to the Console organization).
A question that gets no answer goes back into the box.

**Stage 2 as built:** Ask AI also changes files and makes new ones. It has the connector's compact
tool list (the project tools, the core `doc.`/`sheet.`/`deck.` tools, and `tool_search` /
`tool_call` for every other app tool, PDFs included), plus `project_new`, a new connector tool that
makes a blank .docx, .xlsx, .pptx or .pdf in the project and opens it. Changes go through the
project's permission, as for Claude Desktop:

- **read** (a new project's default): the first change in a turn stops and asks in the panel,
  "Allow changes to files?". **Allow Edits** sets the project to `edit` and the change runs; **Not
  Now** (or Stop) refuses it, and the rest of that turn's changes, and Claude is told so.
- **edit**: a change opens its file if needed and happens live in its tab (it comes to the front).
  Nothing is saved: the panel's **Changes to review** card lists each changed file with **Keep**
  (save) and **Undo** (the app's own undo, once per change), and **Keep All**; the same as the AI
  pill's review. A file made with `project_new` exists on disk as a blank file; Undo takes back
  what was written into it.
- **edit_save**: Claude may also save, when asked.

Each step shows in the conversation ("Made Q3 summary.docx", "Write range · Budget.xlsx"); the note
sent with each question says what the project allows. A turn may take up to 40 rounds of tools.

Goal: an **Ask AI** panel in the window that knows what you're looking at, answers from every
file in the project, does things in your documents (with Keep/Undo), and remembers: the
conversation carries on where you left it, and the project keeps a memory the AI reads each time.

## What exists to build on

- **Tools.** The AI connector (`crates/officecraft-mcp`) already has every tool the chat needs:
  `project_active` (what's in front), `project_files`, `project_search` (every kind of file, with
  locations), `project_read`, `project_open`, `project_save`, and each app's own `doc.*`,
  `sheet.*`, `deck.*`, `pdf.*` tools. The chat calls the same tools in-process, with the same
  project-folder jail and the same permissions (read / edit / edit and save).
- **Live edits and review.** Edits to open files happen in their tabs and wait for Keep or
  Undo in the review panel (`apps/officecraft/src/ai.rs`).
- **Go to a hit.** Search hits already open their file at the paragraph, cell, slide or page.
  Citations in answers reuse it.

## How it works

```
 Ask AI panel ──▶ conversation (.officecraft/chats/…)          tools, in-process
   │  you type                                                 (same jail, same permissions)
   ▼                                                                  ▲
 request: system prompt (fixed) + tools (fixed)  ──▶  Claude API  ──┤ tool_use
          + history + "now in front: Budget.xlsx, Sheet1!B4"         │
          (a mid-conversation system message)        ◀── streamed text, tool calls, citations
```

- **Context-aware.** Each turn, the panel adds a short system note: the tab in front (file, kind,
  and where you are: page, slide, sheet and selection where the app exposes them) and the open
  tabs. It goes in as a mid-conversation system message, so the cached part of the request isn't
  invalidated and the model treats it as trusted context, not as something you typed.
- **Answers from all documents.** The model searches and reads through the tools; answers name
  their sources (`Q3 report.docx ¶12`, `Budget.xlsx Sheet1!B4`), shown as chips that open
  that spot.
- **Does things.** "Make a one-page summary of the three reports as a new document" creates it
  (with the project's edit permission); edits to open files appear live and wait for Keep/Undo.
  "Edit and save" lets it save. The panel shows each tool step as it happens.
- **Continuity.**
  - Each chat is saved in the project: `.officecraft/chats/<id>.json`, the full exchange,
    including the model's thinking blocks as returned (they must be sent back unchanged).
    Reopening the project reopens the chat; a list holds older chats.
  - **Project memory:** Anthropic's memory tool (`memory_20250818`), with the memory files kept
    in `.officecraft/memory/`. The model reads it at the start of each chat and writes to it
    ("budget figures are in GBP", "Priya owns the deck"). The panel shows it; you can edit or
    clear it.
  - Long chats are compacted server-side (beta `compact-2026-01-12`) instead of cut off.
- **Streaming.** Text appears as it's written; tool inputs stream too
  (`eager_input_streaming`), validated before a tool runs.
- **Refusals.** A declined request is shown as such. The request opts into the API's
  server-side fallback (`fallbacks: "default"`), which retries a declined request on another
  model.

## The model, the key and the cost

- **Model:** Claude Opus 5.5 (`claude-opus-5-5`), with adaptive thinking and effort `medium` to
  start; a setting can change the effort. Cheaper choices (Claude Sonnet 5.5, Claude Haiku 5.5)
  can be offered as a setting if you want them.
- **Price** (Anthropic's list price, November 2026): Opus 5.5 $4 per million input tokens, $20
  per million output; cached input $0.20. The fixed prompt and tool list are cached, so a typical
  question that reads a few files costs a few cents; a long working session, more.
- **Key:** your own Anthropic API key, from console.anthropic.com, billed per use there. It is
  kept in the macOS Keychain (Windows Credential Manager), never in a project. Anthropic doesn't
  let other apps sign in with a Claude Pro or Max subscription
  ([Claude Code legal and compliance](https://code.claude.com/docs/en/legal-and-compliance.md)),
  so a subscription can't pay for the built-in chat. It still pays for Claude Desktop and Claude
  Code through Connect AI.
- **What leaves the Mac:** what you type, the context note, and whatever the model reads through
  the tools go to Anthropic's API. Nothing else.
- **Rust:** Anthropic has no Rust SDK, so the panel talks to `POST /v1/messages` over HTTPS
  directly (ureq, already in the build through craft-update), with streaming.

## ChatGPT

- **ChatGPT desktop can't reach OfficeCraft's connector.** ChatGPT connects to MCP servers only
  over the internet (developer mode, HTTPS); servers on your Mac aren't supported except through
  a tunnel offered to enterprise plans
  ([OpenAI developer mode](https://developers.openai.com/api/docs/guides/developer-mode.md)).
  Publishing the connector on the internet would expose your documents to it; not recommended.
- **"Sign in with ChatGPT"** lets Plus and Pro subscribers use their plan in other apps, but only
  in apps OpenAI has approved (16 partners at launch); others apply
  ([The New Stack](https://thenewstack.io/sign-in-with-chatgpt/)). OfficeCraft could apply.
- **What works now:** an OpenAI API key, as a second provider in the same panel (pay per use).
  The panel keeps the provider separate so this can be added.

## Stages

1. **Ask:** the panel, the key, chat about the project with the front tab as context, search and
   read tools, cited answers, streaming, the chat saved and reopened per project.
2. **Do:** the editing and creating tools, live in tabs with Keep/Undo, permission prompts, tool
   steps shown in the panel.
3. **Remember:** project memory (memory tool), compaction, a list of past chats, a memory view.
4. **Optional:** an OpenAI provider; Sign in with ChatGPT if OpenAI approves OfficeCraft.

Each stage ships on its own and is checked with `--check` stages and a few scripted
conversations; a live test spends a little of your API credit, and only with your OK.

## Decisions needed

1. **An Anthropic API key** (pay per use, separate from any Claude subscription): yes or no.
2. **Model:** Claude Opus 5.5 (the default here), or a cheaper one as the default.
3. **ChatGPT:** add an OpenAI API key option in stage 4, apply for Sign in with ChatGPT, or
   leave it.
