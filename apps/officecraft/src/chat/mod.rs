//! Ask AI (docs/ai-chat.md, stages 1 and 2): a chat panel at the right of the window that answers
//! from the files of the project in front, knows which document you're looking at, cites where its
//! answers came from, changes files and makes new ones (live in their tabs, each change kept or
//! undone by you), and keeps each chat with the project. It talks to Claude's API with your own
//! key; the tools are the AI connector's, run in-process with the project's permission.

pub mod api;
pub mod engine;
pub mod key;
pub mod store;
mod view;

use engine::Update;
use officecraft_core::index::Location;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::time::Duration;


/// What the panel asks the app to do.
#[derive(Clone, Debug, PartialEq)]
pub enum PanelAction {
    /// Send what's in the box (the app adds the note about what's in front).
    Send,
    /// Open a cited file, at the place cited.
    OpenSource { file: String, at: Option<String> },
    OpenProject,
    /// Let the AI edit files in the project (the panel's question was answered yes).
    AllowEdits,
    /// Keep or undo the AI's changes to a file.
    Review(officecraft_shell::ai::ReviewAction),
}

/// A turn in progress.
struct Running {
    cancel: Arc<AtomicBool>,
    rx: Receiver<Update>,
    /// Answers to "allow edits?".
    answers: Sender<bool>,
}

/// A job for the project's worker thread.
struct Job {
    messages: Vec<Value>,
    cred: api::Credential,
    cancel: Arc<AtomicBool>,
    tx: Sender<Update>,
    answers: Receiver<bool>,
    wake: egui::Context,
}

/// The thread that keeps the project's tools (and their search index) between turns.
struct Worker {
    jobs: Sender<Job>,
}

impl Worker {
    fn start(root: PathBuf, settings: Option<PathBuf>) -> Option<Worker> {
        let (jobs, rx) = channel::<Job>();
        std::thread::Builder::new()
            .name("officecraft-ask-ai".into())
            .spawn(move || {
                let mut tools: Option<engine::Tools> = None;
                for job in rx {
                    if tools.is_none() {
                        match engine::Tools::new(&root, settings.as_deref()) {
                            Ok(t) => tools = Some(t),
                            Err(e) => {
                                let _ = job.tx.send(Update::Done(Some(format!("Couldn't open the project's tools: {e}"))));
                                job.wake.request_repaint();
                                continue;
                            }
                        }
                    }
                    let Some(t) = tools.as_mut() else { continue };
                    let mut messages = job.messages;
                    let (tx, wake, answers, cancel) = (job.tx, job.wake, job.answers, job.cancel);
                    let ask_tx = tx.clone();
                    let ask_wake = wake.clone();
                    let mut ask = |label: &str| {
                        let _ = ask_tx.send(Update::Ask(label.to_string()));
                        ask_wake.request_repaint();
                        // Waits for the person (Stop counts as no).
                        loop {
                            match answers.recv_timeout(Duration::from_millis(100)) {
                                Ok(yes) => return yes,
                                Err(RecvTimeoutError::Timeout) if !cancel.load(Ordering::Relaxed) => {}
                                Err(_) => return false,
                            }
                        }
                    };
                    engine::run(
                        &mut messages,
                        t,
                        &mut engine::Api(job.cred),
                        &cancel,
                        &mut |u| {
                            let _ = tx.send(u);
                            wake.request_repaint();
                        },
                        &mut ask,
                    );
                }
            })
            .ok()?;
        Some(Worker { jobs })
    }
}

#[derive(Default)]
pub struct ChatPanel {
    pub open: bool,
    root: Option<PathBuf>,
    chat: Option<store::Chat>,
    /// The project's chats, for the menu: id, title, last changed.
    chats: Vec<(String, String, i64)>,
    pub input: String,
    key_input: String,
    /// The answer being written, not yet saved.
    live: String,
    /// What the tools are doing now.
    step: Option<String>,
    /// A change waiting for "allow edits?": what it is.
    asking: Option<String>,
    running: Option<Running>,
    /// The last thing that went wrong.
    notice: Option<String>,
    /// Whether a credential is set up (cached: the Keychain isn't read every frame).
    configured: Option<bool>,
    worker: Option<Worker>,
    focus_input: bool,
    /// Showing a made-up chat (the acceptance check): nothing is saved or sent.
    preview: bool,
}

fn now() -> i64 {
    chrono::Local::now().timestamp()
}

impl ChatPanel {
    pub fn toggle(&mut self) {
        self.open = !self.open;
        if self.open {
            self.focus_input = true;
            self.configured = None;
        }
    }

    /// Whether the panel's text fields have the keyboard.
    pub fn wants_keyboard(&self, ctx: &egui::Context) -> bool {
        self.open && ctx.memory(|m| m.focused().is_some_and(|f| f == view::input_id() || f == view::key_id()))
    }

    pub fn busy(&self) -> bool {
        self.running.is_some()
    }

    /// Follows the project in front: its latest chat comes back.
    pub fn set_project(&mut self, root: Option<PathBuf>) {
        if self.root == root || self.preview {
            return;
        }
        self.stop();
        self.running = None;
        self.worker = None;
        self.root = root;
        self.live.clear();
        self.step = None;
        self.notice = None;
        self.chats = self.root.as_deref().map(store::list).unwrap_or_default();
        self.chat = self.root.as_deref().zip(self.chats.first()).and_then(|(r, (id, ..))| store::load(r, id));
    }

    pub fn new_chat(&mut self) {
        if self.busy() {
            return;
        }
        self.chat = None;
        self.live.clear();
        self.notice = None;
        self.focus_input = true;
    }

    fn switch_to(&mut self, id: &str) {
        if self.busy() {
            return;
        }
        self.chat = self.root.as_deref().and_then(|r| store::load(r, id));
        self.notice = None;
    }

    pub fn stop(&mut self) {
        if let Some(r) = &self.running {
            r.cancel.store(true, Ordering::Relaxed);
        }
        self.asking = None;
    }

    /// Answers "allow edits?" (the app has set the project's permission first when yes).
    pub fn answer(&mut self, yes: bool) {
        if self.asking.take().is_some()
            && let Some(r) = &self.running
        {
            let _ = r.answers.send(yes);
        }
    }

    /// The project the panel follows.
    pub fn root(&self) -> Option<&Path> {
        self.root.as_deref()
    }

    fn save(&mut self) {
        if self.preview {
            return;
        }
        let (Some(root), Some(chat)) = (self.root.clone(), self.chat.as_mut()) else { return };
        chat.updated = now();
        chat.titled();
        if let Err(e) = store::save(&root, chat) {
            self.notice = Some(format!("Couldn't save the chat: {e}"));
        }
        self.chats = store::list(&root);
    }

    /// Sends the question in the box, with `front` (what's in front, for the AI) as a note.
    pub fn send(&mut self, ctx: &egui::Context, front: &str) {
        let question = self.input.trim().to_string();
        if question.is_empty() || self.busy() || self.preview {
            return;
        }
        let Some(root) = self.root.clone() else { return };
        let Some(cred) = key::credential() else {
            self.notice = Some(if key::use_ant() { "Your Anthropic sign-in has run out: run `ant auth login` in Terminal.".into() } else { "Add your API key first.".into() });
            self.configured = None;
            return;
        };
        if self.worker.is_none() {
            self.worker = Worker::start(root, crate::settings::dir());
        }
        let Some(worker) = &self.worker else { return };
        let chat = self.chat.get_or_insert_with(|| store::Chat::new(now()));
        chat.messages.push(json!({"role": "user", "content": [{"type": "text", "text": question}]}));
        // What's in front goes as a note from the app, not as something the person typed.
        chat.messages.push(json!({"role": "system", "content": front}));
        let (tx, rx) = channel();
        let (answer_tx, answers) = channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let job = Job { messages: chat.messages.clone(), cred, cancel: cancel.clone(), tx, answers, wake: ctx.clone() };
        if worker.jobs.send(job).is_err() {
            self.worker = None;
            self.notice = Some("Ask AI stopped working; try again.".into());
            self.unsend();
            return;
        }
        self.input.clear();
        self.notice = None;
        self.live.clear();
        self.step = None;
        self.asking = None;
        self.running = Some(Running { cancel, rx, answers: answer_tx });
        self.save();
    }

    /// Takes back a question that got no answer (the API couldn't be reached), into the box.
    fn unsend(&mut self) {
        let Some(chat) = self.chat.as_mut() else { return };
        if chat.messages.last().is_some_and(|m| m["role"] == "system") && chat.messages.len() >= 2 && chat.messages[chat.messages.len() - 2]["role"] == "user" {
            chat.messages.pop();
            if let Some(q) = chat.messages.pop() {
                if self.input.trim().is_empty() {
                    self.input = store::user_text(&q);
                }
            }
        }
        if chat.messages.is_empty() {
            self.chat = None;
        }
    }

    /// Picks up what the worker said (each frame).
    pub fn poll(&mut self) {
        let Some(running) = &self.running else { return };
        let updates: Vec<Update> = running.rx.try_iter().collect();
        for u in updates {
            match u {
                Update::Text(t) => self.live.push_str(&t),
                Update::Step(s) => self.step = Some(s),
                Update::Ask(s) => self.asking = Some(s),
                Update::Saved(messages) => {
                    if let Some(chat) = self.chat.as_mut() {
                        chat.messages = messages;
                    }
                    self.live.clear();
                    self.save();
                }
                Update::Done(err) => {
                    self.running = None;
                    self.step = None;
                    self.asking = None;
                    self.live.clear();
                    if let Some(e) = err {
                        self.notice = Some(e);
                        self.unsend();
                    }
                    self.save();
                    self.focus_input = true;
                    return;
                }
            }
        }
    }

    /// Shows `chat` for the acceptance check: nothing is saved or sent.
    pub fn show_preview(&mut self, chat: store::Chat, root: PathBuf) {
        self.root = Some(root);
        self.chat = Some(chat);
        self.preview = true;
        self.configured = Some(true);
        self.open = true;
    }

    /// Shows the "allow edits?" question in the preview.
    pub fn preview_ask(&mut self, what: &str) {
        if self.preview {
            self.asking = Some(what.to_string());
        }
    }

    pub fn end_preview(&mut self) {
        self.preview = false;
        self.asking = None;
        self.chat = None;
        self.root = None;
        self.open = false;
    }

    fn is_configured(&mut self) -> bool {
        *self.configured.get_or_insert_with(|| {
            std::env::var("ANTHROPIC_API_KEY").is_ok_and(|k| !k.trim().is_empty()) || key::has_saved_key() || key::use_ant()
        })
    }
}

/// A piece of an answer's line.
#[derive(Clone, Debug, PartialEq)]
pub enum Seg {
    Text(String),
    Bold(String),
    /// A source: `[[file]]` or `[[file|where]]`.
    Cite { file: String, at: Option<String> },
}

/// Splits a line of an answer into text, bold and sources.
pub fn segments(line: &str) -> Vec<Seg> {
    let mut out = Vec::new();
    let mut rest = line;
    while !rest.is_empty() {
        let cite = rest.find("[[");
        let bold = rest.find("**");
        let next = match (cite, bold) {
            (Some(c), Some(b)) => Some((c.min(b), c <= b)),
            (Some(c), None) => Some((c, true)),
            (None, Some(b)) => Some((b, false)),
            (None, None) => None,
        };
        let Some((at, is_cite)) = next else {
            out.push(Seg::Text(rest.to_string()));
            break;
        };
        let (open, close) = if is_cite { ("[[", "]]") } else { ("**", "**") };
        let body_start = at + open.len();
        let Some(len) = rest[body_start..].find(close) else {
            out.push(Seg::Text(rest.to_string()));
            break;
        };
        if at > 0 {
            out.push(Seg::Text(rest[..at].to_string()));
        }
        let body = &rest[body_start..body_start + len];
        if is_cite {
            let (file, place) = match body.split_once('|') {
                Some((f, w)) => (f.trim(), Some(w.trim().to_string()).filter(|w| !w.is_empty())),
                None => (body.trim(), None),
            };
            out.push(Seg::Cite { file: file.to_string(), at: place });
        } else {
            out.push(Seg::Bold(body.to_string()));
        }
        rest = &rest[body_start + len + close.len()..];
    }
    out
}

/// Where in a file a source points, from the tools' "where" (`Sheet1!B4`, `Slide 3`,
/// `Slide 3 notes`, `Page 2`) or a quoted passage of a document: the location, and the words to
/// find (for a passage).
pub fn parse_where(at: &str) -> Option<(Location, String)> {
    let at = at.trim();
    if let Some(quoted) = at.strip_prefix('"').and_then(|s| s.strip_suffix('"')).or_else(|| at.strip_prefix('“').and_then(|s| s.strip_suffix('”'))) {
        return Some((Location::Paragraph(0), quoted.to_string()));
    }
    let number = |s: &str| s.trim().parse::<usize>().ok().filter(|n| *n > 0).map(|n| n - 1);
    if let Some(n) = at.strip_prefix("Page ") {
        return Some((Location::Page(number(n)?), String::new()));
    }
    if let Some(n) = at.strip_prefix("Slide ") {
        let (n, notes) = match n.strip_suffix(" notes") {
            Some(n) => (n, true),
            None => (n, false),
        };
        return Some((Location::Slide { index: number(n)?, notes }, String::new()));
    }
    let (sheet, cell) = at.rsplit_once('!')?;
    let sheet = sheet.trim_matches('\'').replace("''", "'");
    let cell = cell.trim().to_string();
    (!sheet.is_empty() && !cell.is_empty() && cell.chars().all(|c| c.is_ascii_alphanumeric() || c == ':' || c == '$'))
        .then_some((Location::Cell { sheet, cell }, String::new()))
}

/// The label a source chip shows: the file's name, and where.
pub fn cite_label(file: &str, at: Option<&str>) -> String {
    let name = Path::new(file).file_name().map_or_else(|| file.to_string(), |n| n.to_string_lossy().into_owned());
    match at {
        Some(w) if !w.starts_with('"') && !w.starts_with('“') => format!("{name} · {w}"),
        _ => name,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answers_split_into_text_bold_and_sources() {
        assert_eq!(
            segments("Revenue **grew** 4% [[Budget.xlsx|Sheet1!B4]] and [[Q3 report.docx]]."),
            vec![
                Seg::Text("Revenue ".into()),
                Seg::Bold("grew".into()),
                Seg::Text(" 4% ".into()),
                Seg::Cite { file: "Budget.xlsx".into(), at: Some("Sheet1!B4".into()) },
                Seg::Text(" and ".into()),
                Seg::Cite { file: "Q3 report.docx".into(), at: None },
                Seg::Text(".".into()),
            ]
        );
        assert_eq!(segments("an [[unclosed source"), vec![Seg::Text("an [[unclosed source".into())]);
        assert_eq!(segments(""), vec![]);
    }

    #[test]
    fn sources_point_at_the_right_place() {
        assert_eq!(parse_where("Sheet1!B4"), Some((Location::Cell { sheet: "Sheet1".into(), cell: "B4".into() }, String::new())));
        assert_eq!(parse_where("'Q3 data'!A1:B2"), Some((Location::Cell { sheet: "Q3 data".into(), cell: "A1:B2".into() }, String::new())));
        assert_eq!(parse_where("Slide 3 notes"), Some((Location::Slide { index: 2, notes: true }, String::new())));
        assert_eq!(parse_where("Page 1"), Some((Location::Page(0), String::new())));
        assert_eq!(parse_where("\"revenue grew\""), Some((Location::Paragraph(0), "revenue grew".into())));
        assert_eq!(parse_where("Page 0"), None);
        assert_eq!(parse_where("somewhere"), None);
        assert_eq!(cite_label("Finance/Budget.xlsx", Some("Sheet1!B4")), "Budget.xlsx · Sheet1!B4");
        assert_eq!(cite_label("Q3.docx", Some("\"growth\"")), "Q3.docx");
    }

    #[test]
    fn a_question_that_got_no_answer_goes_back_in_the_box() {
        let mut p = ChatPanel::default();
        let mut chat = store::Chat::new(1);
        chat.messages.push(json!({"role": "user", "content": [{"type": "text", "text": "What's new?"}]}));
        chat.messages.push(json!({"role": "system", "content": "In front: nothing"}));
        p.chat = Some(chat);
        p.unsend();
        assert_eq!(p.input, "What's new?");
        assert!(p.chat.is_none(), "an empty chat goes");
    }
}
