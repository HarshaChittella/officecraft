//! The [`Editor`] trait: one Craft app instance, holding every open document of its kind
//! (docs/decisions.md D2), as the shell sees it.

use crate::Kind;
use serde_json::Value;
use std::fmt;
use std::path::{Path, PathBuf};

/// A document inside one engine: WordCraft's document id, GridCraft's window id, DeckCraft's
/// presentation uid, PdfCraft's document id. Stable while the document is open; never reused
/// by the same engine.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DocId(pub u64);

/// What the shell shows about an open document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocInfo {
    pub id: DocId,
    /// The engine's own title for it (file name without extension, or `Document1`, `Book1`…).
    pub title: String,
    pub path: Option<PathBuf>,
    /// Changed since it was opened or last saved.
    pub dirty: bool,
}

/// What [`Editor::open`] did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Opened {
    /// The document is open now (or was already, and is shown).
    Now(DocId),
    /// The engine reads the file in the background; the document appears in
    /// [`Editor::documents`] with this path on a later frame (see [`Editor::find_path`]).
    Later,
}

impl Opened {
    /// The document, when it is open now.
    pub fn now(self) -> Option<DocId> {
        match self {
            Opened::Now(d) => Some(d),
            Opened::Later => None,
        }
    }
}

/// Where a close the shell asked for stands. Closing is asynchronous: the app may ask
/// Save / Don't Save / Cancel in its own dialog and answer on a later frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CloseState {
    /// The app is still asking (its prompt is open, or a save it started is running).
    Pending,
    /// The document is closed.
    Closed,
    /// The user cancelled: the document stays open.
    Cancelled,
}

/// An error an editor operation returns, with a message a person can act on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditorError(pub String);

impl fmt::Display for EditorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for EditorError {}

impl From<String> for EditorError {
    fn from(s: String) -> Self {
        EditorError(s)
    }
}

impl From<&str> for EditorError {
    fn from(s: &str) -> Self {
        EditorError(s.to_string())
    }
}

pub type Result<T> = std::result::Result<T, EditorError>;

/// One engine (one Craft app instance) with all its open documents. The shell draws the
/// active document; the others stay open in the same instance.
///
/// A frame: [`Editor::raw_input_hook`] on the input, then one egui pass that calls
/// [`Editor::logic`] and [`Editor::ui`], or only `logic` (`Context::run_logic`) while the engine
/// isn't shown.
pub trait Editor {
    fn kind(&self) -> Kind;

    /// Every open document, in the engine's order.
    fn documents(&self) -> Vec<DocInfo>;

    /// The document the engine shows and commands act on.
    fn active(&self) -> Option<DocId>;

    /// Show `doc` and make commands act on it. Takes effect on the next frame.
    fn activate(&mut self, doc: DocId) -> Result<()>;

    /// A new, empty document, made active.
    fn new_document(&mut self) -> Result<DocId>;

    /// Opens `path` as a new document, made active. Opening a file already open shows that one.
    fn open(&mut self, path: &Path) -> Result<Opened>;

    /// The open document whose file is `path`.
    fn find_path(&self, path: &Path) -> Option<DocId> {
        self.documents()
            .into_iter()
            .find(|d| d.path.as_deref() == Some(path))
            .map(|d| d.id)
    }

    /// Saves `doc` where it came from. An untitled document needs [`Editor::save_as`].
    fn save(&mut self, doc: DocId) -> Result<()>;

    /// Saves `doc` to `path`, in the format the extension names; the document then belongs there.
    fn save_as(&mut self, doc: DocId, path: &Path) -> Result<()>;

    /// Starts closing `doc` the way the user's ⌘W would: it closes at once when clean, and with
    /// unsaved changes the app asks Save / Don't Save / Cancel in its own dialog. Follow it with
    /// [`Editor::close_state`] on later frames.
    fn request_close(&mut self, doc: DocId) -> Result<()>;

    /// Whether the app's own close prompt (or another modal question) is open.
    fn close_prompt_open(&self) -> bool;

    /// Where a [`Editor::request_close`] of `doc` stands. Ask after at least one frame has run.
    fn close_state(&self, doc: DocId) -> CloseState {
        if !self.documents().iter().any(|d| d.id == doc) {
            CloseState::Closed
        } else if self.close_prompt_open() {
            CloseState::Pending
        } else {
            CloseState::Cancelled
        }
    }

    /// Runs one of the app's commands by id, as its ribbon or menu would.
    fn run_command(&mut self, id: &str, args: Value) -> Result<Value>;

    /// Called on each frame's input before the pass.
    fn raw_input_hook(&mut self, _ctx: &egui::Context, _raw: &mut egui::RawInput) {}

    /// The frame's work before drawing (opens, timers, autosave, background jobs).
    fn logic(&mut self, ctx: &egui::Context);

    /// Draws the whole app (title row, ribbon, document, panes, status bar) into `ui`.
    fn ui(&mut self, ui: &mut egui::Ui);

    /// The app's settings, to keep between launches; `None` when it keeps its own.
    fn persist(&self) -> Option<String> {
        None
    }

    /// Settings from [`Editor::persist`].
    fn restore(&mut self, _settings: &str) {}
}
