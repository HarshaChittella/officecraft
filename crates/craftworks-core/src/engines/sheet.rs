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
    /// AutoSave as it was before an AI's edits paused it.
    autosave_before: Option<bool>,
    /// GridCraft's own blank workbook, set aside (never a tab).
    blanks: super::blanks::Blanks,
}

impl SheetEditor {
    pub fn new() -> Self {
        Self::with_services(Services::default())
    }

    /// GridCraft with the host's services (clipboard, panels, printing).
    pub fn with_services(services: Services) -> Self {
        SheetEditor { app: SheetApp::new(Session::new(), services), autosave_before: None, blanks: Default::default() }
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
        reply_id(reply, "window").or(self.app.session.active_window()).map(DocId).ok_or_else(|| "GridCraft opened no window".into())
    }
}

impl SheetEditor {
    /// Every window, and whether its workbook is untouched (no file, nothing changed).
    fn seen(&self) -> Vec<super::blanks::Seen> {
        let s = &self.app.session;
        s.windows()
            .iter()
            .filter(|w| !w.hidden)
            .map(|w| (w.id, s.documents().iter().find(|d| d.uid == w.doc_uid).is_some_and(|d| d.path.is_none() && !d.is_dirty())))
            .collect()
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
        let set_aside = self.blanks.pristine(&self.seen());
        s.windows()
            .iter()
            .filter(|w| !w.hidden && !set_aside.contains(&w.id))
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
        let seen = self.seen();
        self.blanks.note_startup(&seen);
        // GridCraft's blank workbook is the new sheet (a fresh Book1 is replaced, not kept).
        if let Some(id) = self.blanks.take(&seen) {
            self.app.enter_window(id);
            return Ok(DocId(id));
        }
        let reply = self.app.run("file.new", json!({}))?;
        self.after_open(&reply)
    }

    fn open(&mut self, path: &Path) -> Result<Opened> {
        self.blanks.note_startup(&self.seen());
        let reply = self.app.run("file.open", json!({"path": path_str(path)?}))?;
        let doc = self.after_open(&reply)?;
        // The opened file replaces the untouched blank workbook.
        for id in self.blanks.pristine(&self.seen()) {
            if id != doc.0 {
                self.app.enter_window(id);
                let _ = self.app.run("file.close", json!({}));
            }
        }
        self.blanks.clear();
        self.app.enter_window(doc.0);
        Ok(Opened::Now(doc))
    }

    fn save(&mut self, doc: DocId) -> Result<()> {
        self.ensure_active(doc)?;
        if self.documents().iter().find(|d| d.id == doc).and_then(|d| d.path.as_ref()).is_none() {
            return Err("This workbook has no file yet: use Save As.".into());
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
        // Closes the window; a workbook's last window asks Save / Don't Save / Cancel when dirty.
        self.app.run("file.close", json!({}))?;
        self.blanks.after_close(&before, &self.seen());
        Ok(())
    }

    fn close_prompt_open(&self) -> bool {
        self.app.dialog.is_some()
    }

    fn set_dark(&mut self, dark: bool) {
        // As GridCraft's own control channel sets it: a fixed choice, not "follow the system".
        self.app.ui.dark = dark;
        self.app.ui.system_theme = false;
    }

    fn run_command(&mut self, id: &str, args: Value) -> Result<Value> {
        Ok(self.app.run(id, args)?)
    }

    fn hold_autosave(&mut self, hold: bool) {
        match (hold, self.autosave_before) {
            (true, None) => {
                self.autosave_before = Some(self.app.ui.autosave);
                self.app.ui.autosave = false;
            }
            (false, Some(was)) => {
                self.app.ui.autosave = was;
                self.autosave_before = None;
            }
            _ => {}
        }
    }

    fn control(&mut self, doc: DocId, method: &str, params: Value) -> std::result::Result<Value, String> {
        use gridcraft_mcp::Backend;
        self.activate(doc).map_err(|e| e.to_string())?;
        let session = std::mem::replace(&mut self.app.session, gridcraft_engine::Session::new());
        let mut h = gridcraft_mcp::Headless::with_session(session);
        let r = h.call(method, params);
        self.app.session = h.session;
        r
    }

    fn logic(&mut self, ctx: &egui::Context) {
        self.app.logic(ctx);
        let seen = self.seen();
        self.blanks.note_startup(&seen);
        self.blanks.adopt(&seen);
    }

    fn ui(&mut self, ui: &mut egui::Ui) {
        self.app.ui(ui);
    }
}
