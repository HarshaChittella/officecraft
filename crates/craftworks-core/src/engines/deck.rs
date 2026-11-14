//! DeckCraft: one `SlideApp`; its session holds every open presentation. A tab's [`DocId`] is
//! the presentation's uid (its index in the session shifts as others close).

use super::path_str;
use crate::{DocId, DocInfo, Editor, Kind, Opened, Result};
use deckcraft_engine::Session;
use deckcraft_ui_egui::{Services, SlideApp};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

pub struct DeckEditor {
    app: SlideApp,
}

impl DeckEditor {
    pub fn new() -> Self {
        DeckEditor { app: SlideApp::new(Session::new(), Services::default()) }
    }

    pub fn app(&mut self) -> &mut SlideApp {
        &mut self.app
    }

    fn index_of(&self, doc: DocId) -> Option<usize> {
        self.app.session.documents().iter().position(|d| d.uid == doc.0)
    }

    fn ensure_active(&mut self, doc: DocId) -> Result<()> {
        if self.active() != Some(doc) {
            self.activate(doc)?;
        }
        Ok(())
    }

    fn active_or_err(&self) -> Result<DocId> {
        self.active().ok_or_else(|| "DeckCraft has no presentation open".into())
    }
}

impl Default for DeckEditor {
    fn default() -> Self {
        Self::new()
    }
}

impl Editor for DeckEditor {
    fn kind(&self) -> Kind {
        Kind::Presentation
    }

    fn documents(&self) -> Vec<DocInfo> {
        self.app
            .session
            .documents()
            .iter()
            .map(|d| DocInfo { id: DocId(d.uid), title: d.title(), path: d.path.as_ref().map(PathBuf::from), dirty: d.is_dirty() })
            .collect()
    }

    fn active(&self) -> Option<DocId> {
        self.app.session.active().map(|d| DocId(d.uid))
    }

    fn activate(&mut self, doc: DocId) -> Result<()> {
        let i = self.index_of(doc).ok_or_else(|| format!("DeckCraft has no presentation {}", doc.0))?;
        self.app.session.set_active(i);
        Ok(())
    }

    fn new_document(&mut self) -> Result<DocId> {
        self.app.run("file.new", json!({}))?;
        self.active_or_err()
    }

    fn open(&mut self, path: &Path) -> Result<Opened> {
        self.app.open_path(path_str(path)?)?;
        self.active_or_err().map(Opened::Now)
    }

    fn save(&mut self, doc: DocId) -> Result<()> {
        self.ensure_active(doc)?;
        if self.app.session.active().and_then(|d| d.path.as_ref()).is_none() {
            return Err("This presentation has no file yet: use Save As.".into());
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
        // File › Close: asks Save / Don't Save / Cancel first when there are unsaved changes.
        self.app.close_presentation();
        Ok(())
    }

    fn close_prompt_open(&self) -> bool {
        self.app.dialog.is_some()
    }

    fn run_command(&mut self, id: &str, args: Value) -> Result<Value> {
        Ok(self.app.run(id, args)?)
    }

    fn logic(&mut self, ctx: &egui::Context) {
        self.app.logic(ctx);
    }

    fn ui(&mut self, ui: &mut egui::Ui) {
        self.app.ui(ui);
    }
}
