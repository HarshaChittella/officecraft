//! GridCraft: one `SheetApp`, drawn without its `DocWindows` driver, so it draws the active
//! window only. A tab's [`DocId`] is a GridCraft window id (a workbook's window).

use super::{path_str, reply_id};
use crate::{DocId, DocInfo, Editor, Kind, Opened, Result};
use gridcraft_engine::Session;
use gridcraft_ui_egui::{Services, SheetApp};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

pub struct SheetEditor {
    app: SheetApp,
}

impl SheetEditor {
    pub fn new() -> Self {
        SheetEditor {
            app: SheetApp::new(Session::new(), Services::default()),
        }
    }

    pub fn app(&mut self) -> &mut SheetApp {
        &mut self.app
    }

    fn ensure_active(&mut self, doc: DocId) -> Result<()> {
        if self.active() != Some(doc) {
            self.activate(doc)?;
        }
        Ok(())
    }

    fn after_open(&mut self, reply: &Value) -> Result<DocId> {
        reply_id(reply, "window")
            .or(self.app.session.active_window())
            .map(DocId)
            .ok_or_else(|| "GridCraft opened no window".into())
    }
}

impl Default for SheetEditor {
    fn default() -> Self {
        Self::new()
    }
}

impl Editor for SheetEditor {
    fn kind(&self) -> Kind {
        Kind::Sheet
    }

    fn documents(&self) -> Vec<DocInfo> {
        let s = &self.app.session;
        s.windows()
            .iter()
            .filter(|w| !w.hidden)
            .map(|w| {
                let book = s.documents().iter().find(|d| d.uid == w.doc_uid);
                DocInfo {
                    id: DocId(w.id),
                    title: s.window_title(w.id).unwrap_or_default(),
                    path: book.and_then(|d| d.path.as_ref()).map(PathBuf::from),
                    dirty: book.is_some_and(|d| d.is_dirty()),
                }
            })
            .collect()
    }

    fn active(&self) -> Option<DocId> {
        self.app.session.active_window().map(DocId)
    }

    fn activate(&mut self, doc: DocId) -> Result<()> {
        if self.app.session.window(doc.0).is_none() {
            return Err(format!("GridCraft has no window {}", doc.0).into());
        }
        self.app.enter_window(doc.0);
        Ok(())
    }

    fn new_document(&mut self) -> Result<DocId> {
        let reply = self.app.run("file.new", json!({}))?;
        self.after_open(&reply)
    }

    fn open(&mut self, path: &Path) -> Result<Opened> {
        let reply = self
            .app
            .run("file.open", json!({"path": path_str(path)?}))?;
        self.after_open(&reply).map(Opened::Now)
    }

    fn save(&mut self, doc: DocId) -> Result<()> {
        self.ensure_active(doc)?;
        if self
            .documents()
            .iter()
            .find(|d| d.id == doc)
            .and_then(|d| d.path.as_ref())
            .is_none()
        {
            return Err("This workbook has no file yet: use Save As.".into());
        }
        self.app.run("file.save", json!({}))?;
        Ok(())
    }

    fn save_as(&mut self, doc: DocId, path: &Path) -> Result<()> {
        self.ensure_active(doc)?;
        self.app
            .run("file.saveAs", json!({"path": path_str(path)?}))?;
        Ok(())
    }

    fn request_close(&mut self, doc: DocId) -> Result<()> {
        self.ensure_active(doc)?;
        // Closes the window; a workbook's last window asks Save / Don't Save / Cancel when dirty.
        self.app.run("file.close", json!({}))?;
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
