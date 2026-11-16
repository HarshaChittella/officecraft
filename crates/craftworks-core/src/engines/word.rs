//! WordCraft: one `WordApp` showing every document in one window (upstream patch H1,
//! `WordApp::use_one_window`). A tab's [`DocId`] is WordCraft's document id.

use super::path_str;
use crate::{DocId, DocInfo, Editor, EditorError, Kind, Opened, Result};
use serde_json::{Value, json};
use std::path::Path;
use wordcraft_engine::Session;
use wordcraft_ui_egui::{Services, WordApp};

pub struct WordEditor {
    app: WordApp,
    /// AutoSave as it was before an AI's edits paused it.
    autosave_before: Option<bool>,
    /// WordCraft's own blank document, set aside (never a tab).
    blanks: super::blanks::Blanks,
}

impl WordEditor {
    pub fn new() -> Self {
        Self::with_services(Services::default())
    }

    /// WordCraft with the host's services (clipboard, panels, printing).
    pub fn with_services(services: Services) -> Self {
        let mut app = WordApp::new(Session::new(wordcraft_doc::Document::new()), services);
        app.use_one_window();
        WordEditor { app, autosave_before: None, blanks: Default::default() }
    }

    /// The wrapped app, for the host's own needs (control, tests).
    pub fn app(&mut self) -> &mut WordApp {
        &mut self.app
    }

    fn active_id(&self) -> DocId {
        DocId(self.app.session.document_id())
    }

    fn ensure_active(&mut self, doc: DocId) -> Result<()> {
        if self.active_id() != doc {
            self.activate(doc)?;
        }
        Ok(())
    }
}

impl WordEditor {
    /// Every document, and whether it's untouched (no file, nothing changed).
    fn seen(&self) -> Vec<super::blanks::Seen> {
        self.app.session.documents().into_iter().map(|d| (d.id, d.path.is_none() && !d.dirty)).collect()
    }
}

impl Default for WordEditor {
    fn default() -> Self {
        Self::new()
    }
}

impl Editor for WordEditor {
    fn kind(&self) -> Kind {
        Kind::Document
    }

    fn documents(&self) -> Vec<DocInfo> {
        let set_aside = self.blanks.pristine(&self.seen());
        self.app
            .session
            .documents()
            .into_iter()
            .filter(|d| !set_aside.contains(&d.id))
            .map(|d| DocInfo { id: DocId(d.id), title: d.title, path: d.path, dirty: d.dirty })
            .collect()
    }

    fn active(&self) -> Option<DocId> {
        Some(self.active_id())
    }

    fn activate(&mut self, doc: DocId) -> Result<()> {
        // The window follows the session's active document on the next frame (`sync_document`).
        self.app.session.activate(doc.0).map_err(|e| EditorError(e.to_string()))
    }

    fn new_document(&mut self) -> Result<DocId> {
        let seen = self.seen();
        self.blanks.note_startup(&seen);
        // WordCraft's blank document is the new one (a fresh Document1 is replaced, not kept).
        if let Some(id) = self.blanks.take(&seen) {
            self.activate(DocId(id))?;
            return Ok(DocId(id));
        }
        self.app.run("file.new", json!({}))?;
        Ok(self.active_id())
    }

    fn open(&mut self, path: &Path) -> Result<Opened> {
        self.blanks.note_startup(&self.seen());
        self.app.open_file(path_str(path)?, None)?;
        let doc = self.active_id().0;
        // The opened file replaces the untouched blank document.
        for id in self.blanks.pristine(&self.seen()) {
            if id != doc && self.app.session.activate(id).is_ok() {
                let _ = self.app.run("file.close", json!({}));
            }
        }
        self.blanks.clear();
        let _ = self.app.session.activate(doc);
        Ok(Opened::Now(DocId(doc)))
    }

    fn save(&mut self, doc: DocId) -> Result<()> {
        self.ensure_active(doc)?;
        if self.app.session.path.is_none() {
            return Err("This document has no file yet: use Save As.".into());
        }
        self.app.run("file.save", json!({}))?;
        Ok(())
    }

    fn save_as(&mut self, doc: DocId, path: &Path) -> Result<()> {
        self.ensure_active(doc)?;
        self.app.run("file.saveAs", json!({"path": path_str(path)?}))?;
        Ok(())
    }

    fn request_close(&mut self, doc: DocId) -> Result<()> {
        self.ensure_active(doc)?;
        let seen = self.seen();
        self.blanks.note_startup(&seen);
        let before: Vec<u64> = seen.iter().map(|d| d.0).collect();
        // `run` is the user's path: a document with unsaved changes asks first.
        self.app.run("file.close", json!({}))?;
        self.blanks.after_close(&before, &self.seen());
        Ok(())
    }

    fn close_prompt_open(&self) -> bool {
        self.app.dialog.is_some()
    }

    fn set_dark(&mut self, dark: bool) {
        use wordcraft_ui_egui::theme::Appearance;
        self.app.ui.theme = if dark { Appearance::Dark } else { Appearance::Light };
    }

    fn run_command(&mut self, id: &str, args: Value) -> Result<Value> {
        Ok(self.app.run(id, args)?)
    }

    fn hold_autosave(&mut self, hold: bool) {
        match (hold, self.autosave_before) {
            (true, None) => {
                self.autosave_before = Some(self.app.autosave);
                self.app.autosave = false;
            }
            (false, Some(was)) => {
                self.app.autosave = was;
                self.autosave_before = None;
            }
            _ => {}
        }
    }

    fn control(&mut self, doc: DocId, method: &str, params: Value) -> std::result::Result<Value, String> {
        use wordcraft_mcp::Backend;
        self.activate(doc).map_err(|e| e.to_string())?;
        // WordCraft's own headless backend answers, on the open document's session (lent to
        // it for this call and taken straight back).
        let mut h = wordcraft_mcp::Headless::default();
        std::mem::swap(&mut h.session, &mut self.app.session);
        let r = h.call(method, params);
        std::mem::swap(&mut h.session, &mut self.app.session);
        r
    }

    fn logic(&mut self, ctx: &egui::Context) {
        let seen = self.seen();
        self.blanks.note_startup(&seen);
        self.blanks.adopt(&seen);
        self.app.logic(ctx);
    }

    fn ui(&mut self, ui: &mut egui::Ui) {
        self.app.ui(ui);
    }
}
