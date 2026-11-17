//! PdfCraft: one `PdfCraftApp` in hosted mode (upstream patches H5, H6): no tab strip of its
//! own, and the host picks the shown tab. A tab's [`DocId`] is PdfCraft's document id.
//!
//! File dialogs stay PdfCraft's own (`rfd`, docs/decisions.md D12). PdfCraft has no API that
//! saves to a path the host names, so [`Editor::save_as`] is not available yet.

use super::path_str;
use crate::{DocId, DocInfo, Editor, EditorError, Kind, Opened, Result};
use pdfcraft_ui_egui::{OpenOutcome, PdfCraftApp, SaveTarget};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// Shows a page (0-based) of the active PDF: `{"page": n}`. OfficeCraft's own command.
pub const GO_TO_PAGE: &str = "officecraft.goToPage";

pub struct PdfEditor {
    app: PdfCraftApp,
    /// PdfCraft's own MCP server, lent the open documents for each live request (H7).
    tools: Option<pdfcraft_automation::mcp::McpServer>,
}

impl PdfEditor {
    pub fn new() -> Self {
        let mut app = PdfCraftApp::new();
        app.hosted = true;
        PdfEditor { app, tools: None }
    }

    pub fn app(&mut self) -> &mut PdfCraftApp {
        &mut self.app
    }

    fn index_of(&self, doc: DocId) -> Option<usize> {
        self.app.views.iter().position(|v| v.id.0 == doc.0)
    }

    fn index_or_err(&self, doc: DocId) -> Result<usize> {
        self.index_of(doc).ok_or_else(|| EditorError(format!("PdfCraft has no tab for document {}", doc.0)))
    }
}

impl Default for PdfEditor {
    fn default() -> Self {
        Self::new()
    }
}

impl Editor for PdfEditor {
    fn kind(&self) -> Kind {
        Kind::Pdf
    }

    fn documents(&self) -> Vec<DocInfo> {
        // One entry per document: split view can show a document in two tabs.
        let mut out: Vec<DocInfo> = Vec::new();
        for (i, v) in self.app.views.iter().enumerate() {
            if out.iter().any(|d| d.id.0 == v.id.0) {
                continue;
            }
            let doc = self.app.session.get(v.id);
            out.push(DocInfo {
                id: DocId(v.id.0),
                title: doc.map(|d| d.name.clone()).unwrap_or_default(),
                path: doc.and_then(|d| d.path.as_ref()).map(PathBuf::from),
                dirty: self.app.has_unsaved_work(i),
            });
        }
        out
    }

    fn active(&self) -> Option<DocId> {
        self.app.active.and_then(|i| self.app.views.get(i)).map(|v| DocId(v.id.0))
    }

    fn activate(&mut self, doc: DocId) -> Result<()> {
        let i = self.index_or_err(doc)?;
        self.app.active = Some(i);
        Ok(())
    }

    fn new_document(&mut self) -> Result<DocId> {
        // PdfCraft's Create ▸ Blank Page: one empty page of its usual paper size, not yet saved.
        let before = self.app.views.len();
        self.app.run_command("create.blank");
        if self.app.views.len() > before { self.active().ok_or_else(|| "PdfCraft made no tab".into()) } else { Err("PdfCraft couldn't make a blank PDF.".into()) }
    }

    fn open(&mut self, path: &Path) -> Result<Opened> {
        match self.app.open_path(path_str(path)?) {
            OpenOutcome::Opened | OpenOutcome::Focused => self.active().map(Opened::Now).ok_or_else(|| "PdfCraft opened no tab".into()),
            // Read on a worker; its tab appears on a later frame.
            OpenOutcome::Pending => Ok(Opened::Later),
            _ => Err(EditorError(format!("PdfCraft couldn't open {}", path.display()))),
        }
    }

    fn save(&mut self, doc: DocId) -> Result<()> {
        let i = self.index_or_err(doc)?;
        if self.app.save_view(i, SaveTarget::InPlace) {
            Ok(())
        } else {
            Err("PdfCraft didn't save the document now (it may be asking where).".into())
        }
    }

    fn save_as(&mut self, _doc: DocId, _path: &Path) -> Result<()> {
        Err("Save As for PDFs uses PdfCraft's own File ▸ Save As for now.".into())
    }

    fn request_close(&mut self, doc: DocId) -> Result<()> {
        let i = self.index_or_err(doc)?;
        self.app.request_close_tab(i);
        Ok(())
    }

    fn close_prompt_open(&self) -> bool {
        self.app.close_request.is_some()
    }

    fn set_dark(&mut self, dark: bool) {
        use pdfcraft_ui_egui::theme::ThemePreference;
        self.app.set_theme_preference(if dark { ThemePreference::Dark } else { ThemePreference::Light });
    }

    fn run_command(&mut self, id: &str, args: Value) -> Result<Value> {
        if id == GO_TO_PAGE {
            // Ours, not PdfCraft's (its catalogue has no page argument): a search hit's page.
            let page = args.get("page").and_then(Value::as_u64).ok_or_else(|| EditorError("officecraft.goToPage needs {page}".into()))?;
            let view = self.app.active.and_then(|i| self.app.views.get_mut(i)).ok_or_else(|| EditorError("no PDF is open".into()))?;
            view.go_to_page(usize::try_from(page).unwrap_or(usize::MAX));
            return Ok(json!({"page": page}));
        }
        // PdfCraft's catalogue commands take no arguments; they act on the active tab.
        self.app.run_command(id);
        Ok(json!({}))
    }

    /// `tools/call` with `{name, arguments}`: one of PdfCraft's automation tools, run on the
    /// open documents (the window's session, lent for the call: H7). `doc` is filled in when
    /// the tool takes one and the request didn't say. The answer is the MCP tool result.
    fn control(&mut self, doc: DocId, method: &str, params: Value) -> std::result::Result<Value, String> {
        if method != "tools/call" {
            return Err(format!("{method}: PdfCraft answers tools/call"));
        }
        self.activate(doc).map_err(|e| e.to_string())?;
        let name = params.get("name").and_then(Value::as_str).ok_or("missing `name`")?.to_string();
        let mut args = params.get("arguments").cloned().filter(Value::is_object).unwrap_or(json!({}));
        let takes_doc =
            pdfcraft_automation::tools().iter().find(|t| t.name == name).is_some_and(|t| t.input_schema["properties"].get("doc").is_some());
        if takes_doc && let Some(o) = args.as_object_mut() {
            o.entry("doc").or_insert(json!(doc.0));
        }
        let server = self.tools.get_or_insert_with(|| pdfcraft_automation::mcp::McpServer::new(pdfcraft_automation::Automation::new()));
        let msg = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": name, "arguments": args}});
        server.automation_mut().swap_session(&mut self.app.session);
        let reply = server.handle(&msg);
        server.automation_mut().swap_session(&mut self.app.session);
        match reply {
            Some(r) => match (r.get("result"), r.get("error")) {
                (Some(v), _) => Ok(v.clone()),
                (_, Some(e)) => Err(e["message"].as_str().unwrap_or("error").to_string()),
                _ => Err("no answer".into()),
            },
            None => Err("no answer".into()),
        }
    }

    fn native_menus(&self, ctx: &egui::Context) -> Option<craft_menu::MenuBar> {
        Some(self.app.menu_bar(ctx))
    }

    fn run_native_menu(&mut self, ctx: &egui::Context, key: &str) -> bool {
        let Some(command) = craft_keymap::CommandRef::parse_key(key) else { return false };
        self.app.run_menu_command(ctx, &command);
        true
    }

    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        self.app.raw_input_hook(ctx, raw);
    }

    fn logic(&mut self, ctx: &egui::Context) {
        self.app.logic(ctx);
    }

    fn ui(&mut self, ui: &mut egui::Ui) {
        self.app.ui(ui);
    }

    fn persist(&self) -> Option<String> {
        Some(self.app.persist())
    }

    fn restore(&mut self, settings: &str) {
        self.app.restore(settings);
    }
}
