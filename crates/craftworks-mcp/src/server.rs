//! The MCP server (`craftworks mcp --project <dir>`): JSON-RPC over stdio.
//!
//! - Project tools: `project_files`, `project_search`, `project_read`, `project_open`,
//!   `project_save`.
//! - Each app's own tools with a prefix (`doc.`, `sheet.`, `deck.`, `pdf.`) and a `file`
//!   argument naming the project file they work on. A file open in the Craftworks window is
//!   edited live in its tab (you watch it change, and Undo takes it back); any other file is
//!   worked on headless, by the app's own backend.
//! - Compact by default (as PdfCraft's `--compact`): a few core tools plus `tool_search` and
//!   `tool_call`, which reach every tool.
//! - Safety: every path stays inside the project ([`Jail`]); `project.json`'s permission
//!   decides what the AI may do (`read`: read only; `edit`: edit, in the window so you review
//!   it; `edit_save`: edit and save).

use crate::jail::Jail;
use crate::live::{LiveClient, is_read_only, settings_dir};
use craftworks_core::Kind;
use craftworks_core::index::{Indexer, read_text};
use craftworks_core::project::{Permission, Project};
use craftworks_core::tree::{Limits, scan};
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

pub const PROTOCOL_VERSION: &str = "2025-06-18";
const TOOL_SEARCH: &str = "tool_search";
const TOOL_CALL: &str = "tool_call";

/// The tools listed in compact mode, besides `tool_search` and `tool_call`.
const CORE: &[&str] = &[
    "project_files",
    "project_search",
    "project_read",
    "project_open",
    "project_save",
    "doc.execute",
    "doc.insert_table",
    "sheet.read_range",
    "sheet.write_range",
    "deck.add_slide",
    "deck.add_text_box",
];

/// Each app's tools that Craftworks leaves out: it opens, makes and saves files itself, and
/// these need the app's own window.
const LEFT_OUT: &[&str] = &[
    "new_document",
    "open_document",
    "save_document",
    "list_documents",
    "parity",
    "screenshot",
    "click",
    "key",
    "ui_inspect",
    "list_windows",
    "open_workbook",
    "save_workbook",
    "new_workbook",
    "new_presentation",
    "open_presentation",
    "save_presentation",
    "export",
    "pointer",
    "select_tool",
    "call_app",
];

/// Each app's tools that only read.
const READS: &[&str] = &[
    "list_commands",
    "get_text",
    "inspect_document",
    "render_page",
    "inspect_workbook",
    "read_range",
    "get_cell",
    "evaluate_formula",
    "list_functions",
    "inspect_slide",
    "render_slide",
    "go_to_slide",
];

/// Arguments that name files: kept inside the project.
const PATH_ARGS: &[&str] = &["path", "image", "picture", "output", "src", "dest", "file_path"];

fn prefix(kind: Kind) -> &'static str {
    match kind {
        Kind::Document => "doc",
        Kind::Sheet => "sheet",
        Kind::Presentation => "deck",
        Kind::Pdf => "pdf",
    }
}

fn kind_of_prefix(p: &str) -> Option<Kind> {
    Kind::ALL.into_iter().find(|k| prefix(*k) == p)
}

/// The live window as each app's MCP backend: `call` goes to the open tab.
struct LiveTab<'a> {
    client: &'a mut LiveClient,
    path: PathBuf,
}

impl LiveTab<'_> {
    fn call(&mut self, method: &str, params: Value) -> Result<Value, String> {
        self.client.request("call", json!({"path": self.path.to_string_lossy(), "method": method, "params": params}))
    }
}

macro_rules! live_backend {
    ($tr:path) => {
        impl $tr for LiveTab<'_> {
            fn call(&mut self, method: &str, params: Value) -> Result<Value, String> {
                LiveTab::call(self, method, params)
            }
            fn has_ui(&self) -> bool {
                false
            }
            fn describe(&self) -> String {
                format!("{} open in Craftworks (live)", self.path.display())
            }
        }
    };
}
live_backend!(wordcraft_mcp::Backend);
live_backend!(gridcraft_mcp::Backend);
live_backend!(deckcraft_mcp::Backend);

/// Files worked on headless, each in its own session of its app.
#[derive(Default)]
struct Headless {
    word: HashMap<PathBuf, wordcraft_mcp::Headless>,
    sheet: HashMap<PathBuf, gridcraft_mcp::Headless>,
    deck: HashMap<PathBuf, deckcraft_mcp::Headless>,
}

/// One tool call's outcome: MCP content, and whether it's an error.
fn result(content: Vec<Value>, is_error: bool) -> Value {
    json!({"content": content, "isError": is_error})
}

fn text_result(t: impl Into<String>) -> Value {
    result(vec![json!({"type": "text", "text": t.into()})], false)
}

fn json_result(v: &Value) -> Value {
    let text = serde_json::to_string_pretty(v).unwrap_or_default();
    json!({"content": [{"type": "text", "text": text}], "structuredContent": if v.is_object() { v.clone() } else { json!({"result": v}) }, "isError": false})
}

fn error_result(t: impl Into<String>) -> Value {
    result(vec![json!({"type": "text", "text": t.into()})], true)
}

pub struct Server {
    jail: Jail,
    project: Project,
    compact: bool,
    settings: Option<PathBuf>,
    live: Option<LiveClient>,
    headless: Headless,
    pdf: pdfcraft_automation::mcp::McpServer,
    /// PDFs opened headless by `file`, and their document ids there.
    pdf_docs: HashMap<PathBuf, u64>,
    index: Option<Indexer>,
    /// Every tool, and which ones only read.
    tools: Vec<Value>,
    reads: Vec<String>,
}

impl Server {
    /// A server for the project in `root` (made a project if it isn't one).
    pub fn new(root: &Path, compact: bool) -> Result<Server, String> {
        let jail = Jail::new(root).map_err(|e| format!("{}: {e}", root.display()))?;
        let project = Project::create(jail.root(), None).map_err(|e| e.to_string())?;
        let automation = pdfcraft_automation::Automation::new().with_root(jail.root()).map_err(|e| e.to_string())?;
        let pdf = pdfcraft_automation::mcp::McpServer::new(automation);
        let mut s = Server {
            jail,
            project,
            compact,
            settings: settings_dir(),
            live: None,
            headless: Headless::default(),
            pdf,
            pdf_docs: HashMap::new(),
            index: None,
            tools: Vec::new(),
            reads: Vec::new(),
        };
        s.collect_tools();
        Ok(s)
    }

    /// Uses another settings folder to find the window (tests).
    pub fn with_settings_dir(mut self, dir: &Path) -> Server {
        self.settings = Some(dir.to_path_buf());
        self
    }

    fn collect_tools(&mut self) {
        let file_prop = json!({"type": "string", "description": "The file to work on, relative to the project folder."});
        let mut tools = project_tools();
        let mut reads: Vec<String> = ["project_files", "project_search", "project_read"].iter().map(|s| s.to_string()).collect();
        let apps: [(Kind, Vec<Value>); 3] = [
            (Kind::Document, wordcraft_mcp::tools::tool_definitions().as_array().cloned().unwrap_or_default()),
            (Kind::Sheet, gridcraft_mcp::tool_definitions()),
            (Kind::Presentation, deckcraft_mcp::tool_definitions()),
        ];
        for (kind, defs) in apps {
            for mut t in defs {
                let Some(name) = t.get("name").and_then(Value::as_str).map(str::to_string) else { continue };
                if LEFT_OUT.contains(&name.as_str()) {
                    continue;
                }
                let full = format!("{}.{name}", prefix(kind));
                if READS.contains(&name.as_str()) {
                    reads.push(full.clone());
                }
                t["name"] = json!(full);
                let schema = t.get_mut("inputSchema").and_then(Value::as_object_mut);
                if let Some(schema) = schema {
                    if let Some(props) = schema.get_mut("properties").and_then(Value::as_object_mut) {
                        props.remove("doc");
                        props.remove("window");
                        props.insert("file".into(), file_prop.clone());
                    }
                    let req = schema.entry("required").or_insert(json!([]));
                    if let Some(r) = req.as_array_mut() {
                        r.retain(|x| x != "doc" && x != "window");
                        r.insert(0, json!("file"));
                    }
                }
                tools.push(t);
            }
        }
        // PdfCraft's own tools; its paths are confined to the project by its own root.
        let listed = self.pdf.handle(&json!({"jsonrpc": "2.0", "id": 0, "method": "tools/list"}));
        let pdf_tools = listed.and_then(|v| v["result"]["tools"].as_array().cloned()).unwrap_or_default();
        let pdf_file = json!({"type": "string", "description": "A PDF in the project, relative to its folder: the tool works on it (live in its tab when it's open in Craftworks). Instead of `doc`."});
        for mut t in pdf_tools {
            let Some(name) = t.get("name").and_then(Value::as_str).map(str::to_string) else { continue };
            let full = format!("pdf.{name}");
            if t["annotations"]["readOnlyHint"] == json!(true) {
                reads.push(full.clone());
            }
            t["name"] = json!(full);
            // Tools on one document also take `file`, so they can reach an open tab.
            let takes_doc = t["inputSchema"]["properties"].get("doc").is_some();
            if takes_doc && let Some(schema) = t.get_mut("inputSchema").and_then(Value::as_object_mut) {
                if let Some(props) = schema.get_mut("properties").and_then(Value::as_object_mut) {
                    props.insert("file".into(), pdf_file.clone());
                }
                if let Some(req) = schema.get_mut("required").and_then(Value::as_array_mut) {
                    req.retain(|x| x != "doc");
                }
            }
            tools.push(t);
        }
        self.tools = tools;
        self.reads = reads;
    }

    fn permission(&self) -> Permission {
        // Read afresh: you may change it in the project while the AI works.
        Project::open(self.jail.root()).map(|p| p.settings.ai_permission).unwrap_or(self.project.settings.ai_permission)
    }

    /// The running window, connecting (again) if needed.
    fn window(&mut self) -> Option<&mut LiveClient> {
        let alive = match self.live.as_mut() {
            Some(c) => c.request("tabs", json!({})).is_ok(),
            None => false,
        };
        if !alive {
            self.live = self.settings.as_deref().and_then(LiveClient::connect);
        }
        self.live.as_mut()
    }

    /// Whether `path` is open in the window.
    fn open_in_window(&mut self, path: &Path) -> bool {
        self.window()
            .and_then(|w| w.tabs().ok())
            .is_some_and(|tabs| tabs.iter().any(|(p, _)| p == path || p.canonicalize().ok().as_deref() == Some(path)))
    }

    fn instructions(&self) -> String {
        let p = &self.project.settings;
        let mut s = format!(
            "Craftworks project \"{}\" at {}. Files are documents (doc.*), sheets (sheet.*), presentations (deck.*) and PDFs (pdf.*); \
             every doc/sheet/deck tool takes `file`, a path in the project. Start with project_files or project_search; read any file \
             with project_read. A file open in the Craftworks window is edited live in its tab, where the user watches and can undo; \
             open one with project_open. Permission: {}.",
            p.name,
            self.jail.root().display(),
            match p.ai_permission {
                Permission::Read => "read only (no edits)",
                Permission::Edit => "edit; edits wait for the user's review in Craftworks before they're saved",
                Permission::EditSave => "edit and save",
            }
        );
        if self.compact {
            s.push_str(" Only the core tools are listed: tool_search finds the others, tool_call runs them.");
        }
        if !p.ai_instructions.trim().is_empty() {
            s.push_str("\n\nThe project's instructions:\n");
            s.push_str(p.ai_instructions.trim());
        }
        s
    }

    /// Serves JSON-RPC lines from `input` until it closes.
    pub fn serve(&mut self, input: impl BufRead, mut output: impl Write) -> std::io::Result<()> {
        for line in input.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            let reply = match serde_json::from_str::<Value>(&line) {
                Ok(msg) => self.handle(&msg),
                Err(e) => Some(json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32700, "message": format!("parse error: {e}")}})),
            };
            if let Some(r) = reply {
                writeln!(output, "{r}")?;
                output.flush()?;
            }
        }
        Ok(())
    }

    /// One JSON-RPC message; `None` for a notification.
    pub fn handle(&mut self, msg: &Value) -> Option<Value> {
        let id = msg.get("id").cloned()?;
        let method = msg.get("method").and_then(Value::as_str).unwrap_or_default();
        let params = msg.get("params").cloned().unwrap_or(json!({}));
        let r: Result<Value, (i64, String)> = match method {
            "initialize" => Ok(json!({
                "protocolVersion": params.get("protocolVersion").and_then(Value::as_str).unwrap_or(PROTOCOL_VERSION),
                "capabilities": {"tools": {"listChanged": false}},
                "serverInfo": {"name": "craftworks", "title": "Craftworks", "version": env!("CARGO_PKG_VERSION")},
                "instructions": self.instructions(),
            })),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({"tools": self.listed_tools()})),
            "tools/call" => {
                let name = params.get("name").and_then(Value::as_str).unwrap_or_default().to_string();
                let args = params.get("arguments").cloned().unwrap_or(json!({}));
                Ok(self.call(&name, &args))
            }
            other => Err((-32601, format!("method not found: {other}"))),
        };
        Some(match r {
            Ok(v) => json!({"jsonrpc": "2.0", "id": id, "result": v}),
            Err((code, m)) => json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": m}}),
        })
    }

    fn listed_tools(&self) -> Vec<Value> {
        if !self.compact {
            return self.tools.clone();
        }
        let mut out: Vec<Value> = CORE.iter().filter_map(|n| self.tools.iter().find(|t| t["name"] == json!(n)).cloned()).collect();
        out.push(json!({
            "name": TOOL_SEARCH,
            "title": "Find Craftworks tools",
            "description": "Search every tool (doc.*, sheet.*, deck.*, pdf.*, project_*): words matched against name and description. \
                With `name`, returns that tool in full with its input schema. Run one with tool_call.",
            "inputSchema": {"type": "object", "properties": {"query": {"type": "string"}, "name": {"type": "string"}}},
            "annotations": {"readOnlyHint": true},
        }));
        out.push(json!({
            "name": TOOL_CALL,
            "title": "Run any Craftworks tool",
            "description": "Runs a tool by name with its arguments (find it with tool_search).",
            "inputSchema": {"type": "object", "properties": {"name": {"type": "string"}, "arguments": {"type": "object"}}, "required": ["name"]},
        }));
        out
    }

    /// Runs a tool and returns the MCP `tools/call` result.
    pub fn call(&mut self, name: &str, args: &Value) -> Value {
        match name {
            TOOL_SEARCH => self.tool_search(args),
            TOOL_CALL => {
                let inner = args.get("name").and_then(Value::as_str).unwrap_or_default().to_string();
                if inner == TOOL_CALL || inner == TOOL_SEARCH {
                    return error_result("tool_call runs the other tools");
                }
                let a = args.get("arguments").cloned().unwrap_or(json!({}));
                self.call(&inner, &a)
            }
            "project_files" => self.project_files(args),
            "project_search" => self.project_search(args),
            "project_read" => self.project_read(args),
            "project_open" => self.project_open(args),
            "project_save" => self.project_save(args),
            _ => match name.split_once('.') {
                Some(("pdf", tool)) => self.pdf_call(name, tool, args),
                Some((p, tool)) => match kind_of_prefix(p) {
                    Some(kind) => self.app_call(kind, name, tool, args),
                    None => error_result(format!("unknown tool {name:?}; tool_search finds tools")),
                },
                None => error_result(format!("unknown tool {name:?}; tool_search finds tools")),
            },
        }
    }

    fn tool_search(&self, args: &Value) -> Value {
        if let Some(n) = args.get("name").and_then(Value::as_str) {
            return match self.tools.iter().find(|t| t["name"] == json!(n)) {
                Some(t) => json_result(t),
                None => error_result(format!("no tool named {n:?}")),
            };
        }
        let words: Vec<String> = args.get("query").and_then(Value::as_str).unwrap_or_default().split_whitespace().map(str::to_lowercase).collect();
        let found: Vec<Value> = self
            .tools
            .iter()
            .filter(|t| {
                let hay = format!(
                    "{} {} {}",
                    t["name"].as_str().unwrap_or_default(),
                    t["title"].as_str().unwrap_or_default(),
                    t["description"].as_str().unwrap_or_default()
                )
                .to_lowercase();
                words.iter().all(|w| hay.contains(w.as_str()))
            })
            .take(60)
            .map(|t| json!({"name": t["name"], "description": t["description"]}))
            .collect();
        json_result(&json!({"tools": found}))
    }

    fn is_edit(&self, name: &str, args: &Value) -> bool {
        if self.reads.iter().any(|r| r == name) {
            return false;
        }
        // An app's "run this command" tools read when the command does.
        let cmd = args.get("command").or_else(|| args.get("id")).and_then(Value::as_str);
        match (name.rsplit('.').next(), cmd) {
            (Some("execute" | "execute_command" | "run_command"), Some(c)) => !is_read_only("engine.execute", &json!({"command": c})),
            _ => true,
        }
    }

    fn refuse_edit(&self) -> Option<Value> {
        (self.permission() == Permission::Read)
            .then(|| error_result("This project lets the AI read only (project.json ai.permission is \"read\"). Ask the user to allow edits."))
    }

    /// Keeps every file-naming argument inside the project.
    fn jail_args(&self, args: &Value) -> Result<Value, String> {
        let mut a = args.as_object().cloned().unwrap_or_default();
        for key in PATH_ARGS {
            if let Some(Value::String(p)) = a.get(*key) {
                let real = self.jail.resolve(p)?;
                a.insert((*key).to_string(), json!(real.to_string_lossy()));
            }
        }
        Ok(Value::Object(a))
    }

    fn app_call(&mut self, kind: Kind, full: &str, tool: &str, args: &Value) -> Value {
        let file = match args.get("file").and_then(Value::as_str) {
            Some(f) => f.to_string(),
            None => return error_result(format!("{full} needs `file`, a {} in the project", kind.label().to_lowercase())),
        };
        let path = match self.jail.resolve(&file) {
            Ok(p) => p,
            Err(e) => return error_result(e),
        };
        if Kind::for_path(&path) != Some(kind) {
            return error_result(format!(
                "{file} isn't a {}: use the {}.* tools for it",
                kind.label().to_lowercase(),
                Kind::for_path(&path).map_or("project_read", prefix)
            ));
        }
        let edit = self.is_edit(full, args);
        if edit && let Some(r) = self.refuse_edit() {
            return r;
        }
        let mut a = match self.jail_args(args) {
            Ok(a) => a,
            Err(e) => return error_result(e),
        };
        if let Some(o) = a.as_object_mut() {
            o.remove("file");
        }
        // Live in its tab when it's open; with `edit` (not `edit_save`), an edit opens it so
        // the user reviews it there.
        let mut live = self.open_in_window(&path);
        if !live && edit && self.permission() == Permission::Edit {
            if self.window().is_none() {
                return error_result(
                    "Edits need the user's review: open Craftworks (the edit then happens in its window), or set the project's permission to edit_save.",
                );
            }
            live = self.open_and_wait(&path);
            if !live {
                return error_result(format!("{file} didn't open in Craftworks"));
            }
        }
        if live && let Some(client) = self.window() {
            let mut tab = LiveTab { client, path: path.clone() };
            return match kind {
                Kind::Document => wordcraft_mcp::tools::call_tool(&mut tab, tool, &a).to_value(),
                Kind::Sheet => gridcraft_mcp::call_tool(&mut tab, tool, &a).to_value(),
                Kind::Presentation => deckcraft_mcp::call_tool(&mut tab, tool, &a).to_value(),
                Kind::Pdf => error_result("PDFs use the pdf.* tools"),
            };
        }
        if let Err(e) = self.ensure_headless(kind, &path) {
            return error_result(e);
        }
        match kind {
            Kind::Document => self.headless.word.get_mut(&path).map(|h| wordcraft_mcp::tools::call_tool(h, tool, &a).to_value()),
            Kind::Sheet => self.headless.sheet.get_mut(&path).map(|h| gridcraft_mcp::call_tool(h, tool, &a).to_value()),
            Kind::Presentation => self.headless.deck.get_mut(&path).map(|h| deckcraft_mcp::call_tool(h, tool, &a).to_value()),
            Kind::Pdf => None,
        }
        .unwrap_or_else(|| error_result("no session for that file"))
    }

    /// Opens `path` in the window and waits for its tab.
    fn open_and_wait(&mut self, path: &Path) -> bool {
        let Some(w) = self.window() else { return false };
        if w.request("open", json!({"path": path.to_string_lossy()})).is_err() {
            return false;
        }
        // Up to 20 s (PdfCraft and big files open in the background).
        for _ in 0..200 {
            if self.open_in_window(path) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        false
    }

    /// A headless session of the file's app with the file in it (a new file is made on save).
    fn ensure_headless(&mut self, kind: Kind, path: &Path) -> Result<(), String> {
        let p = path.to_string_lossy().to_string();
        let open = |r: Value| -> Result<(), String> {
            if r["isError"] == json!(true) {
                Err(format!("couldn't open {p}: {}", r["content"][0]["text"].as_str().unwrap_or("error")))
            } else {
                Ok(())
            }
        };
        let exists = path.exists();
        match kind {
            Kind::Document if !self.headless.word.contains_key(path) => {
                let mut h = wordcraft_mcp::Headless::default();
                if exists {
                    open(wordcraft_mcp::tools::call_tool(&mut h, "open_document", &json!({"path": p})).to_value())?;
                }
                self.headless.word.insert(path.to_path_buf(), h);
            }
            Kind::Sheet if !self.headless.sheet.contains_key(path) => {
                let mut h = gridcraft_mcp::Headless::new();
                if exists {
                    open(gridcraft_mcp::call_tool(&mut h, "open_workbook", &json!({"path": p})).to_value())?;
                }
                self.headless.sheet.insert(path.to_path_buf(), h);
            }
            Kind::Presentation if !self.headless.deck.contains_key(path) => {
                let mut h = deckcraft_mcp::Headless::default();
                // DeckCraft's headless session starts empty: a new file starts as a new deck.
                let (tool, args) = if exists { ("open_presentation", json!({"path": p})) } else { ("new_presentation", json!({})) };
                open(deckcraft_mcp::call_tool(&mut h, tool, &args).to_value())?;
                self.headless.deck.insert(path.to_path_buf(), h);
            }
            _ => {}
        }
        Ok(())
    }

    fn pdf_call(&mut self, full: &str, tool: &str, args: &Value) -> Value {
        if self.is_edit(full, args)
            && let Some(r) = self.refuse_edit()
        {
            return r;
        }
        // Saving a PDF writes the file: only with edit_save.
        if tool.contains("save") && self.permission() != Permission::EditSave {
            return error_result("Saving needs the project's edit_save permission.");
        }
        let mut args = args.clone();
        // By `file`: live in its tab when it's open (H7), else opened headless once.
        if let Some(file) = args.get("file").and_then(Value::as_str).map(str::to_string) {
            let path = match self.jail.resolve(&file) {
                Ok(p) => p,
                Err(e) => return error_result(e),
            };
            if Kind::for_path(&path) != Some(Kind::Pdf) {
                return error_result(format!("{file} isn't a PDF"));
            }
            if let Some(o) = args.as_object_mut() {
                o.remove("file");
                o.remove("doc");
            }
            let edit = self.is_edit(full, &args);
            let mut live = self.open_in_window(&path);
            if !live && edit && self.permission() == Permission::Edit {
                if self.window().is_none() {
                    return error_result(
                        "Edits need the user's review: open Craftworks (the edit then happens in its window), or set the project's permission to edit_save.",
                    );
                }
                live = self.open_and_wait(&path);
            }
            if live && let Some(w) = self.window() {
                let params = json!({"name": tool, "arguments": args});
                return match w.request("call", json!({"path": path.to_string_lossy(), "method": "tools/call", "params": params})) {
                    Ok(r) => r,
                    Err(e) => error_result(e),
                };
            }
            let id = match self.pdf_docs.get(&path) {
                Some(id) => *id,
                None => {
                    let open = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "doc_open", "arguments": {"path": path.to_string_lossy()}}});
                    let r = self.pdf.handle(&open).unwrap_or_default();
                    let id = r["result"]["structuredContent"]["doc"].as_u64().or_else(|| r["result"]["structuredContent"]["id"].as_u64());
                    match id {
                        Some(id) => {
                            self.pdf_docs.insert(path.clone(), id);
                            id
                        }
                        None => {
                            return error_result(format!("couldn't open {file}: {}", r["result"]["content"][0]["text"].as_str().unwrap_or("error")));
                        }
                    }
                }
            };
            if let Some(o) = args.as_object_mut() {
                o.insert("doc".into(), json!(id));
            }
        }
        let msg = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": tool, "arguments": args}});
        match self.pdf.handle(&msg) {
            Some(r) => match (r.get("result"), r.get("error")) {
                (Some(v), _) => v.clone(),
                (_, Some(e)) => error_result(e["message"].as_str().unwrap_or("error").to_string()),
                _ => error_result("no answer"),
            },
            None => error_result("no answer"),
        }
    }

    fn project_files(&mut self, args: &Value) -> Value {
        let want = args.get("kind").and_then(Value::as_str).map(str::to_lowercase);
        let open: Vec<PathBuf> = self.window().and_then(|w| w.tabs().ok()).unwrap_or_default().into_iter().map(|(p, _)| p).collect();
        let snap = scan(self.jail.root(), Limits::default());
        let files: Vec<Value> = snap
            .documents()
            .filter(|e| want.as_deref().is_none_or(|k| e.kind.is_some_and(|x| x.label().to_lowercase() == k || prefix(x) == k)))
            .map(|e| {
                let modified = e.modified.and_then(|m| m.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_secs());
                let canon = e.path.canonicalize().unwrap_or_else(|_| e.path.clone());
                json!({
                    "file": e.rel,
                    "kind": e.kind.map(|k| k.label()),
                    "tools": e.kind.map(|k| format!("{}.*", prefix(k))),
                    "modified_unix": modified,
                    "bytes": e.len,
                    "open_in_window": open.iter().any(|o| *o == canon || *o == e.path),
                })
            })
            .collect();
        json_result(&json!({"project": self.project.settings.name, "files": files, "truncated": snap.truncated}))
    }

    fn project_search(&mut self, args: &Value) -> Value {
        let Some(q) = args.get("query").and_then(Value::as_str).filter(|q| !q.trim().is_empty()) else {
            return error_result("project_search needs `query`");
        };
        let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(50).clamp(1, 500) as usize;
        let ix = self.index.get_or_insert_with(|| Indexer::start(|| {}));
        ix.update(scan(self.jail.root(), Limits::default()));
        // Wait for the index to read what changed (at most a minute for a big project).
        std::thread::sleep(Duration::from_millis(30));
        for _ in 0..1200 {
            if ix.progress().pending == 0 {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let hits: Vec<Value> = ix
            .search(q, limit)
            .into_iter()
            .map(|h| json!({"file": self.jail.relative(&h.path), "where": h.location.label(), "text": h.snippet}))
            .collect();
        json_result(&json!({"query": q, "hits": hits}))
    }

    fn project_read(&mut self, args: &Value) -> Value {
        let Some(f) = args.get("file").and_then(Value::as_str) else { return error_result("project_read needs `file`") };
        let path = match self.jail.resolve(f) {
            Ok(p) => p,
            Err(e) => return error_result(e),
        };
        let max = args.get("max_chars").and_then(Value::as_u64).unwrap_or(100_000).clamp(1_000, 2_000_000) as usize;
        match read_text(&path) {
            Ok(sections) => {
                let mut total = 0;
                let mut cut = false;
                let parts: Vec<Value> = sections
                    .into_iter()
                    .take_while(|(_, t)| {
                        total += t.len();
                        cut = total > max;
                        !cut
                    })
                    .map(|(loc, t)| json!({"where": loc.label(), "text": t}))
                    .collect();
                json_result(
                    &json!({"file": self.jail.relative(&path), "kind": Kind::for_path(&path).map(|k| k.label()), "parts": parts, "truncated": cut}),
                )
            }
            Err(e) => error_result(format!("{f}: {e}")),
        }
    }

    fn project_open(&mut self, args: &Value) -> Value {
        let Some(f) = args.get("file").and_then(Value::as_str) else { return error_result("project_open needs `file`") };
        let path = match self.jail.resolve(f) {
            Ok(p) => p,
            Err(e) => return error_result(e),
        };
        if Kind::for_path(&path).is_none() {
            return error_result(format!("Craftworks doesn't open {f}"));
        }
        if self.window().is_none() {
            return error_result("Craftworks isn't running: ask the user to open it, then try again.");
        }
        if self.open_and_wait(&path) {
            // Edits now go to the tab; a headless copy would be stale.
            self.headless.word.remove(&path);
            self.headless.sheet.remove(&path);
            self.headless.deck.remove(&path);
            self.pdf_docs.remove(&path);
            text_result(format!("{f} is open in Craftworks; edits to it now happen live in its tab."))
        } else {
            error_result(format!("{f} didn't open"))
        }
    }

    fn project_save(&mut self, args: &Value) -> Value {
        let Some(f) = args.get("file").and_then(Value::as_str) else { return error_result("project_save needs `file`") };
        let path = match self.jail.resolve(f) {
            Ok(p) => p,
            Err(e) => return error_result(e),
        };
        if self.open_in_window(&path) {
            // The window checks the permission too, and keeps unapproved edits for review.
            return match self.window().map(|w| w.request("save", json!({"path": path.to_string_lossy()}))) {
                Some(Ok(_)) => text_result(format!("Saved {f}.")),
                Some(Err(e)) => error_result(e),
                None => error_result("Craftworks went away"),
            };
        }
        if self.permission() != Permission::EditSave {
            return error_result("Saving needs the project's edit_save permission; otherwise the user keeps edits from Craftworks' review panel.");
        }
        let p = path.to_string_lossy().to_string();
        let r = match Kind::for_path(&path) {
            Some(Kind::Document) => {
                self.headless.word.get_mut(&path).map(|h| wordcraft_mcp::tools::call_tool(h, "save_document", &json!({"path": p})).to_value())
            }
            Some(Kind::Sheet) => {
                self.headless.sheet.get_mut(&path).map(|h| gridcraft_mcp::call_tool(h, "save_workbook", &json!({"path": p})).to_value())
            }
            Some(Kind::Presentation) => {
                self.headless.deck.get_mut(&path).map(|h| deckcraft_mcp::call_tool(h, "save_presentation", &json!({"path": p})).to_value())
            }
            _ => return error_result("PDFs are saved with pdf.doc_save"),
        };
        r.unwrap_or_else(|| error_result(format!("{f} has no unsaved edits")))
    }
}

fn project_tools() -> Vec<Value> {
    let file = json!({"type": "string", "description": "A file in the project, relative to its folder."});
    vec![
        json!({"name": "project_files", "title": "List the project's files",
            "description": "Every document, sheet, presentation and PDF in the project: path, kind, which tools work on it, size, modified time, and whether it's open in the Craftworks window.",
            "inputSchema": {"type": "object", "properties": {"kind": {"type": "string", "description": "Only this kind: document, sheet, presentation or pdf."}}},
            "annotations": {"readOnlyHint": true}}),
        json!({"name": "project_search", "title": "Search the project",
            "description": "Finds text across every file of every kind: each hit's file, where (paragraph, Sheet!cell, slide, page) and the text around it. Every word must match; the last also as a prefix.",
            "inputSchema": {"type": "object", "properties": {"query": {"type": "string"}, "limit": {"type": "integer", "minimum": 1, "maximum": 500}}, "required": ["query"]},
            "annotations": {"readOnlyHint": true}}),
        json!({"name": "project_read", "title": "Read a file",
            "description": "The text of any file in the project, in parts with where each is (paragraphs; Sheet!cell values; slide text and notes; PDF pages).",
            "inputSchema": {"type": "object", "properties": {"file": file, "max_chars": {"type": "integer"}}, "required": ["file"]},
            "annotations": {"readOnlyHint": true}}),
        json!({"name": "project_open", "title": "Open a file in Craftworks",
            "description": "Opens a file in a Craftworks tab, so the user sees it; edits to an open file happen live in its tab and can be undone there.",
            "inputSchema": {"type": "object", "properties": {"file": file}, "required": ["file"]}}),
        json!({"name": "project_save", "title": "Save a file",
            "description": "Saves the AI's edits to a file, if the project allows it (edit_save). Otherwise the user keeps or undoes them in Craftworks' review panel.",
            "inputSchema": {"type": "object", "properties": {"file": file}, "required": ["file"]}}),
    ]
}

/// Builds a `tools/call` message (for scripts and tests).
pub fn call_message(id: u64, name: &str, args: Value) -> Value {
    let mut p = Map::new();
    p.insert("name".into(), json!(name));
    p.insert("arguments".into(), args);
    json!({"jsonrpc": "2.0", "id": id, "method": "tools/call", "params": p})
}
