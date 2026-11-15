//! The window's projects: the known ones (`projects.json` in the settings folder), the one in
//! the sidebar, its files (a polling watcher) and its search index, and where a search hit
//! asked to go once its file is open.

use crate::settings;
use craftworks_core::Kind;
use craftworks_core::index::{Hit, Indexer, Location, Progress};
use craftworks_core::project::Project;
use craftworks_core::tree::{Limits, Snapshot, Watcher};
use craftworks_shell::ProjectItem;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// How often the project folder is rescanned.
const RESCAN: Duration = Duration::from_secs(2);
/// Projects the switcher remembers.
const MAX_KNOWN: usize = 30;
/// Frames to wait for a hit's file to open before giving up on showing the hit.
const GOTO_FRAMES: u32 = 900;

/// A search hit to show once its file's tab is in front.
pub struct GoTo {
    pub path: PathBuf,
    pub location: Location,
    pub query: String,
    pub frames: u32,
}

#[derive(Default)]
pub struct Projects {
    pub current: Option<Project>,
    /// Known project folders, most recent first.
    pub known: Vec<PathBuf>,
    pub items: Vec<ProjectItem>,
    pub snapshot: Snapshot,
    watcher: Option<Watcher>,
    indexer: Option<Indexer>,
    pub hits: Vec<Hit>,
    hits_for: Option<(String, Progress)>,
    pub query: String,
    pub progress: Progress,
    pub goto: Option<GoTo>,
    /// The sidebar was open at quit.
    pub sidebar_open: bool,
}

impl Projects {
    /// The known projects and the last one, from `projects.json`.
    pub fn load() -> (Projects, Option<PathBuf>) {
        let v: Value = settings::dir()
            .and_then(|d| std::fs::read(d.join("projects.json")).ok())
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or(Value::Null);
        let known: Vec<PathBuf> =
            v.get("projects").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).map(PathBuf::from).take(MAX_KNOWN).collect();
        let current = v.get("current").and_then(Value::as_str).map(PathBuf::from);
        let sidebar_open = v.get("sidebar").and_then(Value::as_bool).unwrap_or(false);
        let mut p = Projects { known, sidebar_open, ..Projects::default() };
        p.refresh_items();
        (p, current)
    }

    pub fn save(&self) {
        let Some(d) = settings::dir() else { return };
        let v = json!({
            "version": 1,
            "projects": self.known.iter().map(|p| p.to_string_lossy()).collect::<Vec<_>>(),
            "current": self.current.as_ref().map(|p| p.root.to_string_lossy().into_owned()),
            "sidebar": self.sidebar_open,
        });
        if let Err(e) = settings::write_atomic(&d.join("projects.json"), v.to_string().as_bytes()) {
            log::warn!("saving the project list: {e}");
        }
    }

    fn refresh_items(&mut self) {
        self.items = self
            .known
            .iter()
            .filter(|r| Project::exists(r))
            .map(|r| {
                let name = Project::open(r)
                    .map(|p| p.settings.name)
                    .unwrap_or_else(|_| r.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
                ProjectItem { name, root: r.clone() }
            })
            .collect();
    }

    /// Makes `root` a project if it isn't one, and shows it.
    pub fn open(&mut self, root: &Path, name: Option<&str>, ctx: &egui::Context) -> Result<(), String> {
        let project = Project::create(root, name).map_err(|e| e.to_string())?;
        let root = project.root.clone();
        self.known.retain(|k| *k != root);
        self.known.insert(0, root.clone());
        self.known.truncate(MAX_KNOWN);
        self.current = Some(project);
        self.snapshot = Snapshot::default();
        self.hits.clear();
        self.hits_for = None;
        self.progress = Progress::default();
        let wake = ctx.clone();
        self.watcher = Some(Watcher::start(root, RESCAN, Limits::default(), move || wake.request_repaint()));
        let wake = ctx.clone();
        self.indexer = Some(Indexer::start(move || wake.request_repaint()));
        self.refresh_items();
        self.save();
        Ok(())
    }

    /// Takes the watcher's news to the index, and refreshes the search results.
    pub fn frame(&mut self) {
        if let Some(snap) = self.watcher.as_ref().and_then(Watcher::latest) {
            if let Some(ix) = &self.indexer {
                ix.update(snap.clone());
            }
            self.snapshot = snap;
        }
        let Some(ix) = &self.indexer else { return };
        self.progress = ix.progress();
        let key = (self.query.clone(), self.progress.clone());
        if self.hits_for.as_ref() != Some(&key) {
            self.hits = if self.query.trim().is_empty() { Vec::new() } else { ix.search(&self.query, craftworks_shell::sidebar::MAX_HITS) };
            self.hits_for = Some(key);
        }
    }

    /// The pinned files that still exist.
    pub fn pinned(&self) -> Vec<PathBuf> {
        let Some(p) = &self.current else { return Vec::new() };
        p.settings.pinned.iter().filter_map(|r| p.absolute(r)).filter(|a| a.is_file()).collect()
    }

    pub fn toggle_pin(&mut self, path: &Path) -> Result<bool, String> {
        let p = self.current.as_mut().ok_or("no project is open")?;
        if p.read_only {
            return Err("this project was saved by a newer Craftworks".into());
        }
        let pinned = p.toggle_pin(path);
        p.save().map_err(|e| e.to_string())?;
        Ok(pinned)
    }

    pub fn renamed(&mut self, from: &Path, to: &Path) {
        if let Some(p) = self.current.as_mut()
            && !p.read_only
        {
            p.renamed(from, to);
            if let Err(e) = p.save() {
                log::warn!("saving the project after a rename: {e}");
            }
        }
    }

    pub fn current_item(&self) -> Option<ProjectItem> {
        self.current.as_ref().map(|p| ProjectItem { name: p.settings.name.clone(), root: p.root.clone() })
    }
}

/// The command that shows `location` in its engine, as `(command, arguments)`.
pub fn goto_command(kind: Kind, location: &Location, query: &str) -> Option<(&'static str, Value)> {
    match (kind, location) {
        (Kind::Document, Location::Paragraph(_)) => {
            let word = query.split_whitespace().next()?;
            Some(("edit.find", json!({"text": word})))
        }
        (Kind::Sheet, Location::Cell { sheet, cell }) => {
            let quoted =
                if sheet.chars().all(|c| c.is_alphanumeric() || c == '_') { sheet.clone() } else { format!("'{}'", sheet.replace('\'', "''")) };
            Some(("selection.set", json!({"range": format!("{quoted}!{cell}")})))
        }
        (Kind::Presentation, Location::Slide { index, .. }) => Some(("slide.go", json!({"index": index}))),
        (Kind::Pdf, Location::Page(page)) => Some((craftworks_core::engines::GO_TO_PAGE, json!({"page": page}))),
        _ => None,
    }
}

impl GoTo {
    pub fn new(hit: &Hit, query: &str) -> GoTo {
        GoTo { path: hit.path.clone(), location: hit.location.clone(), query: query.to_string(), frames: 0 }
    }

    /// Counts a frame; whether to keep waiting.
    pub fn tick(&mut self) -> bool {
        self.frames += 1;
        self.frames < GOTO_FRAMES
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_kind_of_hit_has_a_command_that_shows_it() {
        let cell = Location::Cell { sheet: "Q3 figures".into(), cell: "B4".into() };
        assert_eq!(goto_command(Kind::Sheet, &cell, "x"), Some(("selection.set", json!({"range": "'Q3 figures'!B4"}))));
        assert_eq!(goto_command(Kind::Presentation, &Location::Slide { index: 2, notes: true }, "x").map(|c| c.0), Some("slide.go"));
        assert_eq!(goto_command(Kind::Pdf, &Location::Page(4), "x").map(|c| c.1), Some(json!({"page": 4})));
        assert_eq!(goto_command(Kind::Document, &Location::Paragraph(3), "revenue growth").map(|c| c.1), Some(json!({"text": "revenue"})));
        assert_eq!(goto_command(Kind::Document, &Location::Page(1), "x"), None, "a mismatch does nothing");
    }
}
