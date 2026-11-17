//! One turn of Ask AI (docs/ai-chat.md): send the conversation, run the tools Claude asks for,
//! send their results, until Claude has answered. The tools are the AI connector's own
//! (`officecraft_mcp::Server`), run in-process with the same project-folder jail and the project's
//! permission. Edits to files happen in their tabs, where you keep or undo them; in a project
//! that lets the AI only read, the first edit asks you first.

use super::api::{self, ApiError, Credential, Event, Reply};
use super::memory::{self, Memory};
use officecraft_core::project::{Permission, Project};
use officecraft_mcp::Server;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

/// The model, and how hard it thinks (Claude Opus 5.5 defaults to `medium`; said explicitly).
pub const MODEL: &str = "claude-opus-5-5";
pub const EFFORT: &str = "medium";
/// Room for a long answer; the request streams, so a large cap is safe.
const MAX_TOKENS: u32 = 64_000;
/// Rounds of tool calls in one turn before it stops (making a document takes many).
const MAX_ROUNDS: usize = 40;

/// When the conversation is summarised server-side: at this many input tokens (the API's default).
const COMPACT_AT: u64 = 150_000;

/// What a tool result says when the person didn't allow edits.
const DECLINED: &str = "Not run: the person didn't allow changes to files in this project. Don't try other changes in this turn; tell them what you would change, and that they can allow edits when asked.";

/// The fixed instructions (cached with the tool list; what changes goes in later messages).
pub const SYSTEM: &str = "You are the assistant built into OfficeCraft, where people work on documents, spreadsheets, presentations and PDFs in tabs of one window. You help with the files of the project that's open: a folder of those files.

Look at the files with the tools before answering questions about them; don't guess at what they say. project_search finds text across every file and says where each match is; project_read reads a file; project_files lists them; project_active says which file the person has in front of them; project_open shows a file to them in a tab. Each time the person writes, a note from OfficeCraft says what's in front of them: use it when they say \"this\", \"here\" or \"the current one\".

You can also change files and make new ones. Each app's tools work on a project file named by `file`: doc.* for documents, sheet.* for sheets, deck.* for presentations, pdf.* for PDFs. Only a few are listed: tool_search finds the others (with `name`, it gives a tool's full input), and tool_call runs them. project_new makes a blank .docx, .xlsx, .pptx or .pdf to fill. Read a file before changing it, and change only what was asked. A change happens live in the file's tab, where the person watches it and then keeps or undoes it, so don't save unless they ask (project_save says when the project doesn't allow it). If the project lets you only read, OfficeCraft asks the person when you first try a change; if they say no, the tool says so. Afterwards, say in a sentence or two what you changed and where, citing it.

Cite what you take from a file right after the statement, as [[file|where]]: file is the path the tools gave, and where is the tool's \"where\" value, such as Sheet1!B4, Slide 3 or Page 2. For a passage of a document, put a few of its words in quotes as where: [[Q3 report.docx|\"revenue grew in every region\"]]. Use [[file]] for a file as a whole.

You have a memory for this project: the memory tool's /memories folder, kept with the project and read by every chat in it. Note there what will matter in later chats: lasting facts about the project and its files (\"the budget is in GBP\", \"Priya owns the pitch deck\"), how the person likes things done, and work left to finish. Don't copy file contents into it (the files are always there to read), don't store passwords or other secrets, and keep it short: a few organised files you update, not a new file per chat. When the person asks you to remember or forget something, do it there.

Write for a chat panel about 360 points wide: lead with the answer, keep paragraphs short, use simple \"- \" lists and **bold** sparingly, and no tables or headings.";

/// Sends a request and reads the streamed reply (the API, or a stand-in in tests).
pub trait Transport {
    fn send(&mut self, body: &Value, cancel: &AtomicBool, on: &mut dyn FnMut(Event)) -> Result<Reply, ApiError>;
}

/// The API itself.
pub struct Api(pub Credential);

impl Transport for Api {
    fn send(&mut self, body: &Value, cancel: &AtomicBool, on: &mut dyn FnMut(Event)) -> Result<Reply, ApiError> {
        api::send(&self.0, body, cancel, on)
    }
}

/// What the panel hears during a turn.
#[derive(Clone, Debug, PartialEq)]
pub enum Update {
    /// More text of the answer being written.
    Text(String),
    /// A tool is running: what it's doing, in words.
    Step(String),
    /// A change waits for the person to allow edits in the project (answered through `ask`).
    Ask(String),
    /// The conversation so far, to show and save.
    Saved(Vec<Value>),
    /// The turn ended; `None` when it went well, else what went wrong.
    Done(Option<String>),
}

/// The tools, run in-process.
pub struct Tools {
    server: Server,
    defs: Vec<Value>,
    root: PathBuf,
    memory: Memory,
}

impl Tools {
    /// The project's tools. `settings` is where the window's live link is (so `project_active`
    /// and `project_open` reach this window).
    pub fn new(root: &Path, settings: Option<&Path>) -> Result<Tools, String> {
        // Compact: the core tools, and tool_search / tool_call for the rest (the full list of
        // every app's tools would be most of each request).
        let mut server = Server::new(root, true)?;
        if let Some(dir) = settings {
            server = server.with_settings_dir(dir);
        }
        let listed = server.handle(&json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list", "params": {}})).unwrap_or_default();
        let defs = listed["result"]["tools"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|t| {
                json!({
                    "name": t["name"],
                    "description": t["description"],
                    "input_schema": t["inputSchema"],
                    // Streamed as generated; checked here before anything runs.
                    "eager_input_streaming": true,
                })
            })
            .collect::<Vec<Value>>();
        let mut defs = defs;
        defs.push(memory::tool_def());
        Ok(Tools { server, defs, root: root.to_path_buf(), memory: Memory::new(root) })
    }

    /// Whether running `name` would change a file in a project that lets the AI only read
    /// (so the person is asked first).
    pub fn needs_approval(&self, name: &str, input: &Value) -> bool {
        // Its own notes, in the project's .officecraft folder, aren't the person's files.
        name != "memory"
            && self.server.is_edit(name, input) && Project::open(&self.root).map(|p| p.settings.ai_permission).unwrap_or(Permission::Read) == Permission::Read
    }

    pub fn defs(&self) -> &[Value] {
        &self.defs
    }

    /// Runs `name` with `input`: the result's text, and whether it failed.
    pub fn run(&mut self, name: &str, input: &Value) -> (String, bool) {
        if name == "memory" {
            return self.memory.run(input);
        }
        let Some(def) = self.defs.iter().find(|d| d["name"] == name) else {
            return (format!("{name} isn't available in this chat."), true);
        };
        if let Err(e) = check_input(&def["input_schema"], input) {
            return (e, true);
        }
        let r = self.server.call(name, input);
        let text: Vec<&str> = r["content"].as_array().into_iter().flatten().filter_map(|c| c["text"].as_str()).collect();
        (text.join("\n"), r["isError"] == json!(true))
    }
}

/// Checks a tool's input against the parts of its schema that matter here: an object, the
/// required fields present, and each field of the declared simple type.
pub fn check_input(schema: &Value, input: &Value) -> Result<(), String> {
    let Some(obj) = input.as_object() else { return Err("The tool's input must be an object.".into()) };
    for req in schema["required"].as_array().into_iter().flatten().filter_map(Value::as_str) {
        if !obj.contains_key(req) {
            return Err(format!("Missing \"{req}\"."));
        }
    }
    for (k, v) in obj {
        let ok = match schema["properties"][k]["type"].as_str() {
            Some("string") => v.is_string(),
            Some("integer") => v.is_i64() || v.is_u64(),
            Some("number") => v.is_number(),
            Some("boolean") => v.is_boolean(),
            Some("object") => v.is_object(),
            Some("array") => v.is_array(),
            _ => true,
        };
        if !ok {
            return Err(format!("\"{k}\" has the wrong type."));
        }
    }
    Ok(())
}

/// What a tool call is doing, for the panel ("Searched “budget”", "Write range · Budget.xlsx").
pub fn step_label(name: &str, input: &Value) -> String {
    if name == "tool_call" {
        let inner = input["name"].as_str().unwrap_or_default();
        return step_label(inner, input.get("arguments").unwrap_or(&Value::Null));
    }
    let s = |k: &str| input[k].as_str().unwrap_or_default().to_string();
    match name {
        "memory" => memory::step_label(input),
        "project_search" => format!("Searched “{}”", s("query")),
        "project_read" => format!("Read {}", s("file")),
        "project_files" => "Listed the project's files".into(),
        "project_active" => "Looked at what's in front".into(),
        "project_open" => format!("Opened {}", s("file")),
        "project_new" => format!("Made {}", s("file")),
        "project_save" => format!("Saved {}", s("file")),
        "tool_search" if input["name"].is_string() => format!("Looked up {}", s("name")),
        "tool_search" => format!("Looked for tools: “{}”", s("query")),
        other => {
            // An app's tool: what it does, in words, and on which file.
            let tool = other.split_once('.').map_or(other, |(_, t)| t);
            let mut words = tool.replace('_', " ");
            if let Some(first) = words.get(..1) {
                words = first.to_uppercase() + &words[1..];
            }
            match input["file"].as_str() {
                Some(f) => format!("{words} · {f}"),
                None => words,
            }
        }
    }
}

/// Whether a step label is a change (for its icon): the tool changes files.
pub fn is_change(name: &str, input: &Value) -> bool {
    if name == "tool_call" {
        return is_change(input["name"].as_str().unwrap_or_default(), input.get("arguments").unwrap_or(&Value::Null));
    }
    name != "memory" && !name.starts_with("project_") && !name.starts_with("tool_") && !is_reading(name) || matches!(name, "project_new" | "project_save")
}

/// An app's tool that only reads, by its name (`sheet.read_range`, `doc.get_text`, …).
fn is_reading(name: &str) -> bool {
    let tool = name.split_once('.').map_or(name, |(_, t)| t);
    ["read", "get", "inspect", "render", "list", "evaluate", "find", "search", "go_to", "info", "describe"].iter().any(|w| tool.starts_with(w))
}

/// The request for `messages`.
pub fn request(tools: &[Value], messages: &[Value]) -> Value {
    json!({
        "model": MODEL,
        "max_tokens": MAX_TOKENS,
        "stream": true,
        "thinking": {"type": "adaptive"},
        "output_config": {"effort": EFFORT},
        // Declined requests are retried on another model, server-side.
        "fallbacks": "default",
        // The tool list and instructions don't change: cached, and the conversation after them.
        "cache_control": {"type": "ephemeral"},
        "system": [{"type": "text", "text": SYSTEM, "cache_control": {"type": "ephemeral"}}],
        "tools": tools,
        "messages": messages,
        // A long chat is summarised on the server instead of running out of room; the summary
        // comes back as a `compaction` block, and what came before it is dropped.
        "context_management": {"edits": [{"type": "compact_20260112", "trigger": {"type": "input_tokens", "value": COMPACT_AT}}]},
    })
}

/// Runs one turn: `messages` ends with the person's message (and the note about what's in
/// front). Appends the replies and tool results to `messages` as it goes.
/// `ask` asks the person to allow edits in the project (true when they do).
pub fn run(
    messages: &mut Vec<Value>,
    tools: &mut Tools,
    transport: &mut dyn Transport,
    cancel: &AtomicBool,
    emit: &mut dyn FnMut(Update),
    ask: &mut dyn FnMut(&str) -> bool,
) {
    // Once they say no, other changes in the turn aren't asked about again.
    let mut declined = false;
    for _ in 0..MAX_ROUNDS {
        let body = request(tools.defs(), messages);
        let mut on = |e: Event| match e {
            Event::Text(_, t) => emit(Update::Text(t)),
            Event::ToolCall(..) => {}
        };
        let reply = match transport.send(&body, cancel, &mut on) {
            Ok(r) => r,
            Err(e) => {
                emit(Update::Done(Some(e.message())));
                return;
            }
        };
        messages.push(json!({"role": "assistant", "content": api::echo_content(&reply.content)}));
        emit(Update::Saved(messages.clone()));
        let calls: Vec<(usize, Value)> =
            reply.content.iter().enumerate().filter(|(_, b)| b["type"] == "tool_use").map(|(i, b)| (i, b.clone())).collect();
        match reply.stop_reason.as_str() {
            "refusal" => {
                answer_unrun(messages, &calls, "Not run: the request was declined.");
                emit(Update::Saved(messages.clone()));
                emit(Update::Done(Some("Claude declined this request.".into())));
                return;
            }
            "max_tokens" if !calls.is_empty() => {
                answer_unrun(messages, &calls, "Not run: the reply was cut off.");
                emit(Update::Saved(messages.clone()));
                emit(Update::Done(Some("The answer ran too long and was cut off.".into())));
                return;
            }
            "tool_use" if !calls.is_empty() => {
                let mut results = Vec::new();
                for (i, call) in &calls {
                    let name = call["name"].as_str().unwrap_or_default();
                    let id = call["id"].clone();
                    if let Some((_, raw)) = reply.invalid_inputs.iter().find(|(j, _)| j == i) {
                        results.push(json!({"type": "tool_result", "tool_use_id": id, "is_error": true, "content": json!({"INVALID_JSON": raw}).to_string()}));
                        continue;
                    }
                    let label = step_label(name, &call["input"]);
                    if tools.needs_approval(name, &call["input"]) && (declined || !ask(&label)) {
                        declined = true;
                        results.push(json!({"type": "tool_result", "tool_use_id": id, "is_error": true, "content": DECLINED}));
                        continue;
                    }
                    emit(Update::Step(label));
                    let (text, failed) = tools.run(name, &call["input"]);
                    let mut r = json!({"type": "tool_result", "tool_use_id": id, "content": text});
                    if failed {
                        r["is_error"] = json!(true);
                    }
                    results.push(r);
                }
                messages.push(json!({"role": "user", "content": results}));
                emit(Update::Saved(messages.clone()));
            }
            _ => {
                emit(Update::Done(None));
                return;
            }
        }
    }
    emit(Update::Done(Some(format!("Stopped after {MAX_ROUNDS} rounds of tool use; ask it to carry on."))))
}

/// Tool calls that won't run still get an answer, so the conversation stays valid.
fn answer_unrun(messages: &mut Vec<Value>, calls: &[(usize, Value)], why: &str) {
    if calls.is_empty() {
        return;
    }
    let results: Vec<Value> = calls.iter().map(|(_, c)| json!({"type": "tool_result", "tool_use_id": c["id"], "is_error": true, "content": why})).collect();
    messages.push(json!({"role": "user", "content": results}));
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    /// A stand-in for the API: hands out the scripted replies in turn and keeps the requests.
    struct Script {
        replies: Vec<Reply>,
        sent: Vec<Value>,
    }

    impl Transport for Script {
        fn send(&mut self, body: &Value, _: &AtomicBool, on: &mut dyn FnMut(Event)) -> Result<Reply, ApiError> {
            self.sent.push(body.clone());
            if self.replies.is_empty() {
                return Err(ApiError::Status(500, "no more".into()));
            }
            let r = self.replies.remove(0);
            for (i, b) in r.content.iter().enumerate() {
                if b["type"] == "text" {
                    on(Event::Text(i, b["text"].as_str().unwrap().into()));
                }
            }
            Ok(r)
        }
    }

    fn reply(stop: &str, content: Vec<Value>) -> Reply {
        Reply { content, stop_reason: stop.into(), model: MODEL.into(), usage: json!({}), invalid_inputs: vec![] }
    }

    fn project(name: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!("oc-chat-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let bytes = pdfcraft_engine::Session::new().create_from_text("t", "Quarterly revenue grew in every region").unwrap();
        std::fs::write(root.join("report.pdf"), bytes.as_slice()).unwrap();
        root
    }

    #[test]
    fn a_turn_reads_the_project_then_answers() {
        let root = project("reads");
        let mut tools = Tools::new(&root, Some(&root.join(".no-window"))).unwrap();
        let names: Vec<&str> = tools.defs().iter().filter_map(|d| d["name"].as_str()).collect();
        for n in ["project_read", "project_new", "sheet.write_range", "tool_search", "tool_call"] {
            assert!(names.contains(&n), "{n} offered: {names:?}");
        }
        assert!(names.len() < 25, "the compact list, not every tool: {}", names.len());
        assert!(tools.defs().iter().filter(|d| d["name"] != "memory").all(|d| d["eager_input_streaming"] == true && d["input_schema"]["type"] == "object"));
        assert!(tools.defs().iter().any(|d| d["type"] == "memory_20250818"), "the memory tool");

        let mut script = Script {
            replies: vec![
                reply("tool_use", vec![
                    json!({"type": "thinking", "thinking": "", "signature": "s"}),
                    json!({"type": "tool_use", "id": "t1", "name": "project_read", "input": {"file": "report.pdf"}}),
                ]),
                reply("end_turn", vec![json!({"type": "text", "text": "It grew. [[report.pdf|Page 1]]"})]),
            ],
            sent: vec![],
        };
        let mut messages = vec![json!({"role": "user", "content": [{"type": "text", "text": "What happened to revenue?"}]})];
        let mut updates = Vec::new();
        run(&mut messages, &mut tools, &mut script, &AtomicBool::new(false), &mut |u| updates.push(u), &mut |_| true);

        assert_eq!(updates.last(), Some(&Update::Done(None)));
        assert!(updates.contains(&Update::Step("Read report.pdf".into())));
        assert_eq!(messages.len(), 4, "question, tool call, result, answer");
        assert_eq!(messages[1]["content"][0]["signature"], "s", "thinking goes back unchanged");
        let result = &messages[2]["content"][0];
        assert_eq!(result["tool_use_id"], "t1");
        assert!(result["content"].as_str().unwrap().contains("Quarterly revenue"), "{result}");
        assert!(result.get("is_error").is_none());
        // The second request carried the tool result; both carry the same fixed prefix.
        assert_eq!(script.sent.len(), 2);
        assert_eq!(script.sent[0]["system"], script.sent[1]["system"]);
        assert_eq!(script.sent[0]["tools"], script.sent[1]["tools"]);
        assert_eq!(script.sent[1]["messages"].as_array().unwrap().len(), 3);
        assert_eq!(script.sent[0]["fallbacks"], "default");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_tool_it_doesnt_have_or_bad_input_is_answered_with_an_error() {
        let root = project("bad-input");
        let mut tools = Tools::new(&root, Some(&root.join(".no-window"))).unwrap();
        let (text, failed) = tools.run("doc.get_text", &json!({}));
        assert!(failed && text.contains("isn't available"));
        let (_, failed) = tools.run("project_read", &json!({"file": 3}));
        assert!(failed);
        let (_, failed) = tools.run("project_read", &json!({}));
        assert!(failed);

        let mut bad = reply("tool_use", vec![json!({"type": "tool_use", "id": "t1", "name": "project_search", "input": {}})]);
        bad.invalid_inputs = vec![(0, "{\"query\": \"a".into())];
        let mut script = Script { replies: vec![bad, reply("end_turn", vec![json!({"type": "text", "text": "Sorry."})])], sent: vec![] };
        let mut messages = vec![json!({"role": "user", "content": "find a"})];
        run(&mut messages, &mut tools, &mut script, &AtomicBool::new(false), &mut |_| {}, &mut |_| true);
        let r = &messages[2]["content"][0];
        assert_eq!(r["is_error"], true);
        assert!(r["content"].as_str().unwrap().contains("INVALID_JSON"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_refusal_or_an_error_ends_the_turn_and_leaves_a_valid_conversation() {
        let root = project("refusal");
        let mut tools = Tools::new(&root, Some(&root.join(".no-window"))).unwrap();
        let mut script = Script {
            replies: vec![reply("refusal", vec![json!({"type": "tool_use", "id": "t1", "name": "project_files", "input": {}})])],
            sent: vec![],
        };
        let mut messages = vec![json!({"role": "user", "content": "x"})];
        let mut last = None;
        run(&mut messages, &mut tools, &mut script, &AtomicBool::new(false), &mut |u| last = Some(u), &mut |_| true);
        assert_eq!(last, Some(Update::Done(Some("Claude declined this request.".into()))));
        assert_eq!(messages.last().unwrap()["content"][0]["tool_use_id"], "t1", "the unrun call is answered");
        // The API failing: a message, nothing added.
        let mut messages = vec![json!({"role": "user", "content": "x"})];
        run(&mut messages, &mut tools, &mut script, &AtomicBool::new(false), &mut |u| last = Some(u), &mut |_| true);
        assert!(matches!(last, Some(Update::Done(Some(_)))));
        assert_eq!(messages.len(), 1);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_change_in_a_read_only_project_asks_first() {
        let root = project("approve");
        let mut tools = Tools::new(&root, Some(&root.join(".no-window"))).unwrap();
        assert!(tools.needs_approval("project_new", &json!({"file": "a.docx"})), "a new project lets the AI only read");
        assert!(!tools.needs_approval("project_read", &json!({"file": "report.pdf"})));
        let make = |id: &str, f: &str| json!({"type": "tool_use", "id": id, "name": "project_new", "input": {"file": f}});
        let script = || Script {
            replies: vec![
                reply("tool_use", vec![make("t1", "a.docx"), make("t2", "b.xlsx")]),
                reply("end_turn", vec![json!({"type": "text", "text": "Done."})]),
            ],
            sent: vec![],
        };
        // No: neither is made, and the second isn't asked about.
        let mut asked = Vec::new();
        let mut messages = vec![json!({"role": "user", "content": "make two files"})];
        run(&mut messages, &mut tools, &mut script(), &AtomicBool::new(false), &mut |_| {}, &mut |l| {
            asked.push(l.to_string());
            false
        });
        assert_eq!(asked, vec!["Made a.docx"]);
        assert!(!root.join("a.docx").exists() && !root.join("b.xlsx").exists());
        let results = &messages[2]["content"];
        assert!(results[0]["content"].as_str().unwrap().contains("didn't allow") && results[1]["is_error"] == true);

        // Yes (the app sets the project to edit): both are made, and only the first asks.
        let mut asked = 0;
        let mut messages = vec![json!({"role": "user", "content": "make two files"})];
        let r = root.clone();
        run(&mut messages, &mut tools, &mut script(), &AtomicBool::new(false), &mut |_| {}, &mut |_| {
            asked += 1;
            let mut p = Project::open(&r).unwrap();
            p.settings.ai_permission = Permission::Edit;
            p.save().unwrap();
            true
        });
        assert_eq!(asked, 1);
        assert!(root.join("a.docx").is_file() && root.join("b.xlsx").is_file());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn notes_go_to_the_project_memory_without_asking_and_requests_compact() {
        let root = project("memory");
        let mut tools = Tools::new(&root, Some(&root.join(".no-window"))).unwrap();
        let note = json!({"type": "tool_use", "id": "m1", "name": "memory", "input": {"command": "create", "path": "/memories/project.md", "file_text": "Budget in GBP\n"}});
        let mut script = Script { replies: vec![reply("tool_use", vec![note]), reply("end_turn", vec![json!({"type": "text", "text": "Noted."})])], sent: vec![] };
        let mut messages = vec![json!({"role": "user", "content": "Remember the budget is in GBP"})];
        let mut steps = Vec::new();
        run(&mut messages, &mut tools, &mut script, &AtomicBool::new(false), &mut |u| steps.push(u), &mut |_| panic!("a note doesn't ask"));
        assert!(steps.contains(&Update::Step("Noted in memory: project.md".into())));
        assert_eq!(std::fs::read_to_string(root.join(".officecraft/memory/project.md")).unwrap(), "Budget in GBP\n");
        let edit = &script.sent[0]["context_management"]["edits"][0];
        assert_eq!(edit["type"], "compact_20260112");
        assert_eq!(edit["trigger"]["value"], COMPACT_AT);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn step_labels_and_input_checks() {
        assert_eq!(step_label("project_search", &json!({"query": "budget"})), "Searched “budget”");
        let call = json!({"name": "sheet.write_range", "arguments": {"file": "Budget.xlsx", "range": "A1"}});
        assert_eq!(step_label("tool_call", &call), "Write range · Budget.xlsx");
        assert!(is_change("tool_call", &call) && is_change("project_new", &json!({})));
        assert!(!is_change("sheet.read_range", &json!({})) && !is_change("project_read", &json!({})) && !is_change("tool_search", &json!({})));
        let schema = json!({"type": "object", "properties": {"limit": {"type": "integer"}}, "required": ["query"]});
        assert!(check_input(&schema, &json!({"query": "a", "limit": 3})).is_ok());
        assert!(check_input(&schema, &json!({"query": "a", "limit": "3"})).is_err());
        assert!(check_input(&schema, &json!([])).is_err());
    }
}
