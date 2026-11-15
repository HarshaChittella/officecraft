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
}

impl WordEditor {
    pub fn new() -> Self {
        Self::with_services(Services::default())
    }

    /// WordCraft with the host's services (clipboard, panels, printing).
    pub fn with_services(services: Services) -> Self {
        let mut app = WordApp::new(Session::new(wordcraft_doc::Document::new()), services);
        app.use_one_window();
        WordEditor { app, autosave_before: None }
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
        self.app.session.documents().into_iter().map(|d| DocInfo { id: DocId(d.id), title: d.title, path: d.path, dirty: d.dirty }).collect()
    }

    fn active(&self) -> Option<DocId> {
        Some(self.active_id())
    }

    fn activate(&mut self, doc: DocId) -> Result<()> {
        // The window follows the session's active document on the next frame (`sync_document`).
        self.app.session.activate(doc.0).map_err(|e| EditorError(e.to_string()))
    }

    fn new_document(&mut self) -> Result<DocId> {
        self.app.run("file.new", json!({}))?;
        Ok(self.active_id())
    }

    fn open(&mut self, path: &Path) -> Result<Opened> {
        self.app.open_file(path_str(path)?, None)?;
        Ok(Opened::Now(self.active_id()))
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
        // `run` is the user's path: a document with unsaved changes asks first.
        self.app.run("file.close", json!({}))?;
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
        self.app.logic(ctx);
    }

    fn ui(&mut self, ui: &mut egui::Ui) {
        self.app.ui(ui);
    }
}
