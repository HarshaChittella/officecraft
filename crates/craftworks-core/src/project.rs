//! A project: a folder, plus `.craftworks/project.json` with its name, pinned files and the AI
//! connector's settings (Phase 7). The schema is versioned; a file written by a newer
//! Craftworks is read but never overwritten.

use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// The schema version this build writes.
pub const SCHEMA_VERSION: u64 = 1;
/// The folder inside a project that holds Craftworks' files.
pub const DIR: &str = ".craftworks";
const FILE: &str = "project.json";
/// Pinned files kept at most (a longer list in the file is cut).
const MAX_PINNED: usize = 200;
/// AI instructions kept at most, in bytes.
const MAX_INSTRUCTIONS: usize = 64 * 1024;

/// What the AI connector may do in this project (Phase 7).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Permission {
    /// Read files and open documents; change nothing. The default, and what anything unknown
    /// in the file means.
    #[default]
    Read,
    /// Edit open documents; the user saves.
    Edit,
    /// Edit and save.
    EditSave,
}

impl Permission {
    pub fn as_str(self) -> &'static str {
        match self {
            Permission::Read => "read",
            Permission::Edit => "edit",
            Permission::EditSave => "edit_save",
        }
    }

    fn parse(s: &str) -> Permission {
        match s {
            "edit" => Permission::Edit,
            "edit_save" => Permission::EditSave,
            _ => Permission::Read,
        }
    }
}

/// The contents of `project.json`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Settings {
    pub name: String,
    /// Pinned files, relative to the project folder, with `/` separators.
    pub pinned: Vec<String>,
    pub ai_instructions: String,
    pub ai_permission: Permission,
    /// The tab group's colour, by name (`blue`…); `None` lets the window choose.
    pub color: Option<String>,
}

impl Settings {
    pub fn to_json(&self) -> Value {
        json!({
            "version": SCHEMA_VERSION,
            "name": self.name,
            "pinned": self.pinned,
            "ai": {"instructions": self.ai_instructions, "permission": self.ai_permission.as_str()},
            "color": self.color,
        })
    }

    /// Reads [`Settings::to_json`]'s output leniently: missing or malformed fields take their
    /// defaults. Returns the file's schema version too.
    pub fn from_json(v: &Value) -> (Settings, u64) {
        let version = v.get("version").and_then(Value::as_u64).unwrap_or(1);
        let name = v.get("name").and_then(Value::as_str).unwrap_or_default().chars().take(200).collect();
        let pinned = v
            .get("pinned")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .filter(|p| is_safe_relative(p))
            .map(str::to_string)
            .take(MAX_PINNED)
            .collect();
        let ai = v.get("ai");
        let mut ai_instructions: String = ai.and_then(|a| a.get("instructions")).and_then(Value::as_str).unwrap_or_default().to_string();
        if ai_instructions.len() > MAX_INSTRUCTIONS {
            let mut cut = MAX_INSTRUCTIONS;
            while !ai_instructions.is_char_boundary(cut) {
                cut -= 1;
            }
            ai_instructions.truncate(cut);
        }
        let ai_permission = ai.and_then(|a| a.get("permission")).and_then(Value::as_str).map_or(Permission::Read, Permission::parse);
        let color = v
            .get("color")
            .and_then(Value::as_str)
            .filter(|c| c.len() <= 20 && c.chars().all(|ch| ch.is_ascii_alphabetic()))
            .map(str::to_ascii_lowercase);
        (Settings { name, pinned, ai_instructions, ai_permission, color }, version)
    }
}

/// A relative path that stays inside the project: no root, no `..`, no empty parts.
pub fn is_safe_relative(p: &str) -> bool {
    !p.is_empty() && !p.starts_with('/') && !p.contains('\\') && p.split('/').all(|c| !c.is_empty() && c != "." && c != "..")
}

/// An error reading or writing a project.
#[derive(Debug)]
pub struct ProjectError(pub String);

impl std::fmt::Display for ProjectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ProjectError {}

/// An open project.
#[derive(Clone, Debug, PartialEq)]
pub struct Project {
    pub root: PathBuf,
    pub settings: Settings,
    /// Written by a newer Craftworks: shown, but never saved over.
    pub read_only: bool,
}

impl Project {
    /// Whether `root` holds a project.
    pub fn exists(root: &Path) -> bool {
        root.join(DIR).join(FILE).is_file()
    }

    /// Opens the project in `root`.
    pub fn open(root: &Path) -> Result<Project, ProjectError> {
        let path = root.join(DIR).join(FILE);
        let bytes = std::fs::read(&path).map_err(|e| ProjectError(format!("{}: {e}", path.display())))?;
        let v: Value = serde_json::from_slice(&bytes).map_err(|e| ProjectError(format!("{}: {e}", path.display())))?;
        let (mut settings, version) = Settings::from_json(&v);
        if settings.name.trim().is_empty() {
            settings.name = default_name(root);
        }
        Ok(Project { root: root.to_path_buf(), settings, read_only: version > SCHEMA_VERSION })
    }

    /// Makes `root` a project (the folder must exist), or opens the one already there.
    pub fn create(root: &Path, name: Option<&str>) -> Result<Project, ProjectError> {
        if !root.is_dir() {
            return Err(ProjectError(format!("{} is not a folder", root.display())));
        }
        if Project::exists(root) {
            return Project::open(root);
        }
        let name = name.map(str::trim).filter(|n| !n.is_empty()).map_or_else(|| default_name(root), str::to_string);
        let p = Project { root: root.to_path_buf(), settings: Settings { name, ..Settings::default() }, read_only: false };
        p.save()?;
        Ok(p)
    }

    /// Writes `project.json` (atomically: a temporary file, then a rename).
    pub fn save(&self) -> Result<(), ProjectError> {
        if self.read_only {
            return Err(ProjectError("this project was saved by a newer Craftworks; it isn't changed".into()));
        }
        let dir = self.root.join(DIR);
        std::fs::create_dir_all(&dir).map_err(|e| ProjectError(format!("{}: {e}", dir.display())))?;
        let tmp = dir.join(format!(".{FILE}.tmp"));
        let text = serde_json::to_string_pretty(&self.settings.to_json()).map_err(|e| ProjectError(e.to_string()))?;
        std::fs::write(&tmp, text).map_err(|e| ProjectError(format!("{}: {e}", tmp.display())))?;
        std::fs::rename(&tmp, dir.join(FILE)).map_err(|e| ProjectError(format!("{}: {e}", dir.display())))
    }

    /// `path` relative to the project, with `/` separators, if it's inside it.
    pub fn relative(&self, path: &Path) -> Option<String> {
        let rel = path.strip_prefix(&self.root).ok()?;
        let parts: Vec<String> = rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
        let s = parts.join("/");
        is_safe_relative(&s).then_some(s)
    }

    pub fn absolute(&self, rel: &str) -> Option<PathBuf> {
        is_safe_relative(rel).then(|| self.root.join(rel))
    }

    pub fn is_pinned(&self, path: &Path) -> bool {
        self.relative(path).is_some_and(|r| self.settings.pinned.contains(&r))
    }

    /// Pins or unpins `path`; returns whether it's pinned now.
    pub fn toggle_pin(&mut self, path: &Path) -> bool {
        let Some(rel) = self.relative(path) else { return false };
        if let Some(i) = self.settings.pinned.iter().position(|p| *p == rel) {
            self.settings.pinned.remove(i);
            false
        } else {
            self.settings.pinned.push(rel);
            self.settings.pinned.truncate(MAX_PINNED);
            true
        }
    }

    /// Keeps pins pointing at a renamed file or folder.
    pub fn renamed(&mut self, from: &Path, to: &Path) {
        let (Some(f), Some(t)) = (self.relative(from), self.relative(to)) else { return };
        for p in &mut self.settings.pinned {
            if *p == f {
                *p = t.clone();
            } else if let Some(rest) = p.strip_prefix(&format!("{f}/")) {
                *p = format!("{t}/{rest}");
            }
        }
    }
}

fn default_name(root: &Path) -> String {
    root.file_name().map_or_else(|| "Project".to_string(), |n| n.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("cw-project-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn settings_round_trip_and_junk_takes_defaults() {
        let s = Settings {
            name: "Q3".into(),
            pinned: vec!["a/b.docx".into()],
            ai_instructions: "Be brief.".into(),
            ai_permission: Permission::EditSave,
            color: Some("green".into()),
        };
        assert_eq!(Settings::from_json(&s.to_json()), (s, SCHEMA_VERSION));
        let junk = json!({"name": 3, "pinned": ["ok.pdf", "../escape.pdf", "/abs.pdf", 7, "a//b"], "ai": {"permission": "root"}});
        let (j, v) = Settings::from_json(&junk);
        assert_eq!(v, 1);
        assert_eq!(j.pinned, vec!["ok.pdf".to_string()], "paths that leave the project are dropped");
        assert_eq!(j.ai_permission, Permission::Read, "an unknown permission is the least one");
        assert_eq!(Settings::from_json(&json!(null)).0, Settings::default());
    }

    #[test]
    fn create_open_pin_rename() {
        let root = scratch("create");
        let mut p = Project::create(&root, Some("  ")).unwrap();
        assert_eq!(p.settings.name, root.file_name().unwrap().to_string_lossy(), "a blank name becomes the folder's");
        assert!(Project::exists(&root));
        let file = root.join("docs").join("plan.docx");
        assert!(p.toggle_pin(&file));
        assert!(p.is_pinned(&file));
        p.renamed(&root.join("docs"), &root.join("papers"));
        assert_eq!(p.settings.pinned, vec!["papers/plan.docx".to_string()]);
        p.save().unwrap();
        let again = Project::open(&root).unwrap();
        assert_eq!(again, p);
        assert!(!p.toggle_pin(&root.join("papers/plan.docx")));
        assert!(!p.toggle_pin(Path::new("/elsewhere/x.pdf")), "files outside the project can't be pinned");
        assert!(Project::create(&root.join("missing"), None).is_err());
    }

    #[test]
    fn a_newer_schema_is_read_but_never_overwritten() {
        let root = scratch("newer");
        std::fs::create_dir_all(root.join(DIR)).unwrap();
        let text = r#"{"version": 9, "name": "Future", "future_field": true}"#;
        std::fs::write(root.join(DIR).join(FILE), text).unwrap();
        let p = Project::open(&root).unwrap();
        assert!(p.read_only);
        assert_eq!(p.settings.name, "Future");
        assert!(p.save().is_err());
        assert_eq!(std::fs::read_to_string(root.join(DIR).join(FILE)).unwrap(), text);
    }
}
