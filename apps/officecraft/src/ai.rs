//! The window's side of the AI connector (Phase 7): it answers the MCP server's requests on the
//! main thread (`officecraft_mcp::live`), runs edits to open files in their tabs, and keeps the
//! list of files the AI changed for the review panel. Nothing an AI edits in a tab is saved
//! until you keep it, unless the file's project allows the AI to save (`edit_save`).

use crate::app::App;
use officecraft_core::Kind;
use officecraft_core::project::Permission;
use officecraft_mcp::live::{LiveServer, Request, is_read_only};
use officecraft_shell::ai::{AiStatus, Changed, ReviewAction};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// How long the pill says "editing" after the last edit.
const EDITING_FOR: f64 = 3.0;

#[derive(Default)]
pub struct AiState {
    server: Option<LiveServer>,
    /// The files the AI changed, waiting for review.
    pub changed: Vec<Changed>,
    /// The file last edited, and when (the window's clock).
    editing: Option<(String, f64)>,
    /// A request came since the window opened (the pill shows from then on).
    pub seen: bool,
    pub review_open: bool,
    /// The Connect AI panel is showing (the pill before any AI has been in touch).
    pub connect_open: bool,
    pub pill_rect: Option<egui::Rect>,
}

impl AiState {
    /// Listens for the MCP server in OfficeCraft's settings folder.
    pub fn start(&mut self, dir: &Path, ctx: &egui::Context) {
        let wake = ctx.clone();
        match LiveServer::start(dir, move || wake.request_repaint()) {
            Ok(s) => self.server = Some(s),
            Err(e) => log::warn!("the AI connector can't reach this window: {e}"),
        }
    }

    pub fn status(&self, now: f64) -> Option<AiStatus> {
        if !self.seen && self.changed.is_empty() {
            return Some(AiStatus::Offline);
        }
        Some(match &self.editing {
            Some((f, t)) if now - t < EDITING_FOR => AiStatus::Editing(f.clone()),
            _ if !self.changed.is_empty() => AiStatus::Review(self.changed.iter().map(|c| c.edits as usize).sum()),
            _ => AiStatus::Idle,
        })
    }

    fn record(&mut self, path: &Path, kind: Kind, now: f64) {
        match self.changed.iter_mut().find(|c| c.path == path) {
            Some(c) => c.edits += 1,
            None => self.changed.push(Changed { path: path.to_path_buf(), kind, edits: 1 }),
        }
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        self.editing = Some((name, now));
    }

    fn forget(&mut self, path: &Path) {
        self.changed.retain(|c| c.path != path);
    }
}

impl App {
    /// Answers what the MCP server asked since the last frame.
    pub fn serve_ai(&mut self, ctx: &egui::Context) {
        let Some(reqs) = self.ai.server.as_ref().map(LiveServer::drain) else { return };
        if reqs.is_empty() {
            return;
        }
        self.ai.seen = true;
        let now = ctx.input(|i| i.time);
        for r in reqs {
            let answer = self.ai_request(&r, now);
            r.answer(answer);
        }
        ctx.request_repaint();
    }

    fn ai_request(&mut self, r: &Request, now: f64) -> Result<Value, String> {
        let path = || r.args.get("path").and_then(Value::as_str).map(PathBuf::from).ok_or("missing `path`".to_string());
        match r.op.as_str() {
            "tabs" => {
                // The tab in front, unless Home is showing over it.
                let front = self.tabs.active().map(|t| t.id).filter(|_| !self.newtab.open);
                Ok(Value::Array(
                    self.tabs
                        .iter()
                        .filter_map(|t| {
                            let d = self.engine_ref(t.kind)?.editor().documents().into_iter().find(|d| d.id == t.doc)?;
                            let path = d.path?;
                            let real = path.canonicalize().unwrap_or(path);
                            Some(json!({"path": real.to_string_lossy(), "kind": t.kind.label(), "dirty": d.dirty, "active": Some(t.id) == front}))
                        })
                        .collect(),
                ))
            }
            "open" => {
                let p = path()?;
                self.open(&p);
                Ok(json!({"opening": p.to_string_lossy()}))
            }
            "call" => {
                let p = path()?;
                let method = r.args.get("method").and_then(Value::as_str).ok_or("missing `method`")?.to_string();
                let params = r.args.get("params").cloned().unwrap_or(json!({}));
                let id = self.tab_for_path(&p).ok_or_else(|| format!("{} isn't open in OfficeCraft", p.display()))?;
                let tab = *self.tabs.get(id).ok_or("the tab closed")?;
                let edit = !is_read_only(&method, &params);
                if edit {
                    // You watch it change: the tab comes to the front.
                    self.tabs.activate(id);
                }
                let out = self.engine(tab.kind).editor_mut().control(tab.doc, &method, params)?;
                if edit {
                    let real = p.canonicalize().unwrap_or(p);
                    self.ai.record(&real, tab.kind, now);
                    self.hold_autosaves();
                }
                Ok(out)
            }
            "save" => {
                let p = path()?;
                let allowed = self.projects.owner_of(&p).is_some_and(|pr| pr.settings.ai_permission == Permission::EditSave);
                if !allowed {
                    return Err(format!(
                        "{} isn't saved: the AI's edits wait for your review in OfficeCraft (the project allows edit_save to save directly)",
                        p.display()
                    ));
                }
                self.save_path(&p)?;
                Ok(json!({"saved": p.to_string_lossy()}))
            }
            other => Err(format!("unknown request `{other}`")),
        }
    }

    /// While an app has AI edits waiting for review, its AutoSave is paused (it would save them).
    fn hold_autosaves(&mut self) {
        for kind in [Kind::Document, Kind::Sheet] {
            let waiting = self.ai.changed.iter().any(|c| c.kind == kind);
            if waiting || self.engine_ref(kind).is_some() {
                self.engine(kind).editor_mut().hold_autosave(waiting);
            }
        }
    }

    fn save_path(&mut self, p: &Path) -> Result<(), String> {
        let id = self.tab_for_path(p).ok_or_else(|| format!("{} isn't open", p.display()))?;
        let tab = *self.tabs.get(id).ok_or("the tab closed")?;
        self.engine(tab.kind).editor_mut().save(tab.doc).map_err(|e| e.to_string())?;
        self.ai.forget(p);
        self.hold_autosaves();
        Ok(())
    }

    /// Keep (save) or undo what the AI changed.
    pub fn on_review(&mut self, a: ReviewAction) {
        match a {
            ReviewAction::Keep(p) => {
                if let Err(e) = self.save_path(&p) {
                    self.toasts.error(format!("Couldn't save {}: {e}", p.display()));
                }
            }
            ReviewAction::KeepAll => {
                let all: Vec<PathBuf> = self.ai.changed.iter().map(|c| c.path.clone()).collect();
                for p in all {
                    self.on_review(ReviewAction::Keep(p));
                }
            }
            ReviewAction::Undo(p) => {
                let edits = self.ai.changed.iter().find(|c| c.path == p).map_or(0, |c| c.edits);
                if let Some(id) = self.tab_for_path(&p)
                    && let Some(tab) = self.tabs.get(id).copied()
                {
                    self.tabs.activate(id);
                    let ed = self.engine(tab.kind).editor_mut();
                    let _ = ed.activate(tab.doc);
                    // Each edit the AI made is one step of the app's own undo.
                    for _ in 0..edits {
                        if let Err(e) = ed.run_command("edit.undo", json!({})) {
                            log::info!("undoing the AI's edit: {e}");
                            break;
                        }
                    }
                }
                self.ai.forget(&p);
                self.hold_autosaves();
            }
        }
    }
}
