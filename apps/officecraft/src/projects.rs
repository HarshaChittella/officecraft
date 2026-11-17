//! The window's projects: the known ones (`projects.json` in the settings folder), the open
//! ones (a tab group each, with a polling watcher and a search index), the one the sidebar
//! shows, and where a search hit asked to go once its file is open.

use crate::settings;
use officecraft_core::Kind;
use officecraft_core::index::{Hit, Indexer, Location, Progress};
use officecraft_core::project::Project;
use officecraft_core::tree::{Limits, Snapshot, Watcher};
use officecraft_shell::ProjectItem;
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

/// A project open in the window: one tab group, its files and its search index.
pub struct OpenProject {
    pub project: Project,
    pub snapshot: Snapshot,
    pub progress: Progress,
    watcher: Watcher,
    indexer: Indexer,
}

#[derive(Default)]
pub struct Projects {
    /// The open projects, in the order they were opened.
    pub open: Vec<OpenProject>,
    /// The one the sidebar shows (an index into `open`).
    pub current: Option<usize>,
    /// Known project folders, most recent first.
    pub known: Vec<PathBuf>,
    pub items: Vec<ProjectItem>,
    pub hits: Vec<Hit>,
    hits_for: Option<(PathBuf, String, Progress)>,
    pub query: String,
    /// The current project's indexing progress.
    pub progress: Progress,
    pub goto: Option<GoTo>,
    /// The sidebar was open at quit.
    pub sidebar_open: bool,
    /// Projects whose tab group was collapsed at quit.
    pub collapsed: Vec<PathBuf>,
}

/// What `projects.json` says to bring back.
pub struct Restore {
    pub open: Vec<PathBuf>,
    pub current: Option<PathBuf>,
}

impl Projects {
    /// The known projects, and the ones that were open, from `projects.json`.
    pub fn load() -> (Projects, Restore) {
        let v: Value = settings::dir()
            .and_then(|d| std::fs::read(d.join("projects.json")).ok())
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or(Value::Null);
        let paths = |key: &str| -> Vec<PathBuf> {
            v.get(key).and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).map(PathBuf::from).take(MAX_KNOWN).collect()
        };
        let known = paths("projects");
        let current = v.get("current").and_then(Value::as_str).map(PathBuf::from);
        // Before tab groups only one project was open: the current one.
        let mut open = paths("open");
        if v.get("open").is_none()
            && let Some(c) = &current
        {
            open.push(c.clone());
        }
        let sidebar_open = v.get("sidebar").and_then(Value::as_bool).unwrap_or(false);
        let mut p = Projects { known, sidebar_open, collapsed: paths("collapsed"), ..Projects::default() };
        p.refresh_items();
        (p, Restore { open, current })
    }

    pub fn save(&self) {
        let Some(d) = settings::dir() else { return };
        let roots = |it: &mut dyn Iterator<Item = &PathBuf>| it.map(|p| p.to_string_lossy().into_owned()).collect::<Vec<_>>();
        let v = json!({
            "version": 1,
            "projects": roots(&mut self.known.iter()),
            "open": roots(&mut self.open.iter().map(|o| &o.project.root)),
            "current": self.project().map(|p| p.root.to_string_lossy().into_owned()),
            "collapsed": roots(&mut self.collapsed.iter()),
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

    /// The project the sidebar shows.
    pub fn project(&self) -> Option<&Project> {
        self.current_open().map(|o| &o.project)
    }

    pub fn current_open(&self) -> Option<&OpenProject> {
        self.current.and_then(|i| self.open.get(i))
    }

    fn index_of(&self, root: &Path) -> Option<usize> {
        self.open.iter().position(|o| o.project.root == root)
    }

    /// The open project whose folder holds `path` (the innermost, if they nest).
    pub fn owner_of(&self, path: &Path) -> Option<&Project> {
        self.open.iter().map(|o| &o.project).filter(|p| path.starts_with(&p.root)).max_by_key(|p| p.root.components().count())
    }

    /// Makes `root` a project if it isn't one, opens it (a watcher and an index of its own) and
    /// makes it the sidebar's. Already open: just the sidebar's.
    pub fn open(&mut self, root: &Path, name: Option<&str>, ctx: &egui::Context) -> Result<(), String> {
        if let Some(i) = self.index_of(root) {
            self.select(i);
            return Ok(());
        }
        let project = Project::create(root, name).map_err(|e| e.to_string())?;
        let root = project.root.clone();
        self.known.retain(|k| *k != root);
        self.known.insert(0, root.clone());
        self.known.truncate(MAX_KNOWN);
        let wake = ctx.clone();
        let watcher = Watcher::start(root, RESCAN, Limits::default(), move || wake.request_repaint());
        let wake = ctx.clone();
        let indexer = Indexer::start(move || wake.request_repaint());
        self.open.push(OpenProject { project, snapshot: Snapshot::default(), progress: Progress::default(), watcher, indexer });
        self.select(self.open.len() - 1);
        self.refresh_items();
        self.save();
        Ok(())
    }

    /// Shows open project `i` in the sidebar.
    pub fn select(&mut self, i: usize) {
        if i < self.open.len() && self.current != Some(i) {
            self.current = Some(i);
            self.hits.clear();
            self.hits_for = None;
            self.save();
        }
    }

    pub fn select_root(&mut self, root: &Path) {
        if let Some(i) = self.index_of(root) {
            self.select(i);
        }
    }

    /// Closes a project (its watcher and index stop); its tabs are the window's business.
    pub fn close(&mut self, root: &Path) {
        let Some(i) = self.index_of(root) else { return };
        self.open.remove(i);
        self.current = match self.current {
            Some(c) if c == i => (!self.open.is_empty()).then(|| i.min(self.open.len() - 1)),
            Some(c) if c > i => Some(c - 1),
            c => c,
        };
        self.hits.clear();
        self.hits_for = None;
        self.collapsed.retain(|c| c != root);
        self.save();
    }

    /// Takes each watcher's news to its index, and refreshes the search results.
    pub fn frame(&mut self) {
        for o in &mut self.open {
            if let Some(snap) = o.watcher.latest() {
                o.indexer.update(snap.clone());
                o.snapshot = snap;
            }
            o.progress = o.indexer.progress();
        }
        let Some(o) = self.current.and_then(|i| self.open.get(i)) else {
            self.progress = Progress::default();
            self.hits.clear();
            return;
        };
        self.progress = o.progress.clone();
        let key = (o.project.root.clone(), self.query.clone(), self.progress.clone());
        if self.hits_for.as_ref() != Some(&key) {
            self.hits = if self.query.trim().is_empty() { Vec::new() } else { o.indexer.search(&self.query, officecraft_shell::sidebar::MAX_HITS) };
            self.hits_for = Some(key);
        }
    }

    /// The current project's pinned files that still exist.
    pub fn pinned(&self) -> Vec<PathBuf> {
        let Some(p) = self.project() else { return Vec::new() };
        p.settings.pinned.iter().filter_map(|r| p.absolute(r)).filter(|a| a.is_file()).collect()
    }

    /// Changes the project that holds `path` and saves it.
    fn edit_owner(&mut self, path: &Path, f: impl FnOnce(&mut Project)) -> Result<(), String> {
        let i = self
            .open
            .iter()
            .enumerate()
            .filter(|(_, o)| path.starts_with(&o.project.root))
            .max_by_key(|(_, o)| o.project.root.components().count())
            .map(|(i, _)| i)
            .ok_or("that file isn't in an open project")?;
        let p = &mut self.open[i].project;
        if p.read_only {
            return Err("this project was saved by a newer OfficeCraft".into());
        }
        f(p);
        p.save().map_err(|e| e.to_string())
    }

    pub fn toggle_pin(&mut self, path: &Path) -> Result<(), String> {
        self.edit_owner(path, |p| {
            p.toggle_pin(path);
        })
    }

    pub fn renamed(&mut self, from: &Path, to: &Path) {
        if let Err(e) = self.edit_owner(from, |p| p.renamed(from, to)) {
            log::info!("pins after a rename: {e}");
        }
    }

    pub fn set_name(&mut self, root: &Path, name: &str) -> Result<(), String> {
        self.edit_owner(root, |p| p.settings.name = name.to_string())?;
        self.refresh_items();
        Ok(())
    }

    pub fn set_color(&mut self, root: &Path, color: &str) -> Result<(), String> {
        self.edit_owner(root, |p| p.settings.color = Some(color.to_ascii_lowercase()))
    }

    /// What the AI connector may do in the project at `root` (`.officecraft/project.json`).
    pub fn set_ai_permission(&mut self, root: &Path, permission: officecraft_core::project::Permission) -> Result<(), String> {
        self.edit_owner(root, |p| p.settings.ai_permission = permission)
    }

    pub fn set_collapsed(&mut self, root: &Path, collapsed: bool) {
        self.collapsed.retain(|c| c != root);
        if collapsed {
            self.collapsed.push(root.to_path_buf());
        }
        self.save();
    }

    pub fn current_item(&self) -> Option<ProjectItem> {
        self.project().map(|p| ProjectItem { name: p.settings.name.clone(), root: p.root.clone() })
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
        (Kind::Pdf, Location::Page(page)) => Some((officecraft_core::engines::GO_TO_PAGE, json!({"page": page}))),
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
