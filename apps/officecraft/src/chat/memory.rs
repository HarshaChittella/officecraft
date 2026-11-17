//! The project's memory (docs/ai-chat.md, stage 3): Anthropic's memory tool (`memory_20250818`),
//! run here. Claude sees a folder `/memories`; it is the project's `.officecraft/memory/`, so what
//! it notes ("the budget is in GBP", "Priya owns the deck") travels with the project and every chat
//! in it reads it. The panel shows it, and you can edit, delete or clear it.
//!
//! Every path stays inside that folder: `..`, `.`, a backslash, a hidden name or a link that
//! leads out is refused.

use serde_json::{Value, json};
use std::path::{Component, Path, PathBuf};

/// Claude's name for the folder.
pub const ROOT: &str = "/memories";
/// The tool, as the request lists it (Anthropic defines its input).
pub fn tool_def() -> Value {
    json!({"type": "memory_20250818", "name": "memory"})
}

/// The largest memory file, and the most a `view` shows before it asks for a range.
const MAX_FILE: usize = 100_000;
const VIEW_CHARS: usize = 16_000;
const MAX_FILES: usize = 200;

pub struct Memory {
    dir: PathBuf,
}

/// A result: the text for Claude, and whether it's an error.
type Out = (String, bool);

fn ok(s: impl Into<String>) -> Out {
    (s.into(), false)
}

fn err(s: impl Into<String>) -> Out {
    (s.into(), true)
}

impl Memory {
    /// The memory of the project in `root`.
    pub fn new(root: &Path) -> Memory {
        Memory { dir: root.join(officecraft_core::project::DIR).join("memory") }
    }

    #[cfg(test)]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// `/memories/...` as a real path inside the folder; `None` when it would lead out.
    fn resolve(&self, path: &str) -> Option<PathBuf> {
        let rest = if path == ROOT || path == "/memories/" { "" } else { path.strip_prefix("/memories/")? };
        let mut real = self.dir.clone();
        for part in Path::new(rest).components() {
            match part {
                Component::Normal(n) => {
                    let n = n.to_str()?;
                    if n.starts_with('.') || n.contains('\\') || n.contains('\0') || n.to_ascii_lowercase().contains("%2e") {
                        return None;
                    }
                    real.push(n);
                }
                _ => return None,
            }
        }
        // A link inside mustn't lead out (the folder exists from the first use, so the check
        // stops there).
        std::fs::create_dir_all(&self.dir).ok()?;
        let base = self.dir.canonicalize().ok()?;
        let mut probe = real.clone();
        while !probe.exists() {
            if !probe.pop() {
                break;
            }
        }
        if probe.exists() && !probe.canonicalize().ok()?.starts_with(&base) {
            return None;
        }
        Some(real)
    }

    /// `/memories/...` for a real path in the folder.
    fn shown(&self, real: &Path) -> String {
        match real.strip_prefix(&self.dir) {
            Ok(r) if r.as_os_str().is_empty() => ROOT.to_string(),
            Ok(r) => format!("{ROOT}/{}", r.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/")),
            Err(_) => ROOT.to_string(),
        }
    }

    /// Runs one command of the memory tool.
    pub fn run(&self, input: &Value) -> Out {
        let s = |k: &str| input[k].as_str().unwrap_or_default();
        let path = s("path");
        let bad = |p: &str| err(format!("The path {p} is outside {ROOT}. Use paths under {ROOT}."));
        match s("command") {
            "view" => {
                let Some(real) = self.resolve(path) else { return bad(path) };
                if path == ROOT || path == "/memories/" {
                    let _ = std::fs::create_dir_all(&self.dir);
                }
                if real.is_dir() {
                    ok(self.listing(&real))
                } else if real.is_file() {
                    self.view_file(&real, path, &input["view_range"])
                } else {
                    err(format!("The path {path} does not exist. Please provide a valid path."))
                }
            }
            "create" => {
                let Some(real) = self.resolve(path) else { return bad(path) };
                if real == self.dir || real.is_dir() {
                    return err(format!("Error: {path} is a directory"));
                }
                let text = s("file_text");
                if text.len() > MAX_FILE {
                    return err(format!("Error: memory files are at most {MAX_FILE} characters; keep notes short."));
                }
                if !real.exists() && self.count() >= MAX_FILES {
                    return err(format!("Error: the memory holds at most {MAX_FILES} files; combine or delete some."));
                }
                if let Some(d) = real.parent()
                    && let Err(e) = std::fs::create_dir_all(d)
                {
                    return err(format!("Error: {e}"));
                }
                match crate::settings::write_atomic(&real, text.as_bytes()) {
                    Ok(()) => ok(format!("File created successfully at: {path}")),
                    Err(e) => err(format!("Error: {e}")),
                }
            }
            "str_replace" => {
                let Some(real) = self.resolve(path) else { return bad(path) };
                let Some(text) = real.is_file().then(|| std::fs::read_to_string(&real).ok()).flatten() else {
                    return err(format!("Error: The path {path} does not exist. Please provide a valid path."));
                };
                let (old, new) = (s("old_str"), s("new_str"));
                if old.is_empty() {
                    return err("Error: old_str must not be empty");
                }
                let at: Vec<usize> = text.match_indices(old).map(|(i, _)| i).collect();
                match at.len() {
                    0 => err(format!("No replacement was performed, old_str `{old}` did not appear verbatim in {path}.")),
                    1 => {
                        let out = text.replacen(old, new, 1);
                        if out.len() > MAX_FILE {
                            return err(format!("Error: memory files are at most {MAX_FILE} characters."));
                        }
                        match crate::settings::write_atomic(&real, out.as_bytes()) {
                            Ok(()) => ok("The memory file has been edited."),
                            Err(e) => err(format!("Error: {e}")),
                        }
                    }
                    _ => {
                        let lines: Vec<String> = at.iter().map(|i| (text[..*i].matches('\n').count() + 1).to_string()).collect();
                        err(format!(
                            "No replacement was performed. Multiple occurrences of old_str `{old}` in lines: {}. Please ensure it is unique",
                            lines.join(", ")
                        ))
                    }
                }
            }
            "insert" => {
                let Some(real) = self.resolve(path) else { return bad(path) };
                let Some(text) = real.is_file().then(|| std::fs::read_to_string(&real).ok()).flatten() else {
                    return err(format!("Error: The path {path} does not exist"));
                };
                let mut lines: Vec<&str> = text.lines().collect();
                let n = input["insert_line"].as_i64().unwrap_or(-1);
                let Some(at) = usize::try_from(n).ok().filter(|a| *a <= lines.len()) else {
                    return err(format!("Error: Invalid `insert_line` parameter: {n}. It should be within the range of lines of the file: [0, {}]", lines.len()));
                };
                let add = s("insert_text").trim_end_matches('\n');
                lines.insert(at, add);
                let mut out = lines.join("\n");
                out.push('\n');
                if out.len() > MAX_FILE {
                    return err(format!("Error: memory files are at most {MAX_FILE} characters."));
                }
                match crate::settings::write_atomic(&real, out.as_bytes()) {
                    Ok(()) => ok(format!("The file {path} has been edited.")),
                    Err(e) => err(format!("Error: {e}")),
                }
            }
            "delete" => {
                let Some(real) = self.resolve(path) else { return bad(path) };
                if real == self.dir {
                    return err(format!("Error: {ROOT} itself can't be deleted"));
                }
                let done = if real.is_dir() {
                    std::fs::remove_dir_all(&real)
                } else if real.is_file() {
                    std::fs::remove_file(&real)
                } else {
                    return err(format!("Error: The path {path} does not exist"));
                };
                match done {
                    Ok(()) => ok(format!("Successfully deleted {path}")),
                    Err(e) => err(format!("Error: {e}")),
                }
            }
            "rename" => {
                let (from, to) = (s("old_path"), s("new_path"));
                let (Some(a), Some(b)) = (self.resolve(from), self.resolve(to)) else { return bad(if self.resolve(from).is_none() { from } else { to }) };
                if a == self.dir || b == self.dir {
                    return err(format!("Error: {ROOT} itself can't be renamed"));
                }
                if !a.exists() {
                    return err(format!("Error: The path {from} does not exist"));
                }
                if b.exists() {
                    return err(format!("Error: The destination {to} already exists"));
                }
                if let Some(d) = b.parent() {
                    let _ = std::fs::create_dir_all(d);
                }
                match std::fs::rename(&a, &b) {
                    Ok(()) => ok(format!("Successfully renamed {from} to {to}")),
                    Err(e) => err(format!("Error: {e}")),
                }
            }
            other => err(format!("Error: unknown command {other}")),
        }
    }

    fn view_file(&self, real: &Path, path: &str, range: &Value) -> Out {
        let Ok(text) = std::fs::read_to_string(real) else { return err(format!("Error: {path} isn't a text file")) };
        let lines: Vec<&str> = text.lines().collect();
        let (from, to) = match range.as_array().map(|r| (r.first().and_then(Value::as_i64), r.get(1).and_then(Value::as_i64))) {
            Some((Some(a), Some(b))) => {
                let end = if b == -1 { lines.len() as i64 } else { b };
                if a < 1 || end < a || a as usize > lines.len().max(1) {
                    return err(format!("Error: Invalid `view_range`: [{a}, {b}]. The file has {} lines.", lines.len()));
                }
                (a as usize, (end as usize).min(lines.len()))
            }
            _ => (1, lines.len()),
        };
        let mut out = format!("Here's the content of {path} with line numbers:\n");
        let mut cut = false;
        for (i, line) in lines.iter().enumerate().take(to).skip(from - 1) {
            if out.len() + line.len() > VIEW_CHARS {
                cut = true;
                out.push_str(&format!("[Shown up to line {i}; view a range for the rest.]\n"));
                break;
            }
            out.push_str(&format!("{:>6}\t{line}\n", i + 1));
        }
        if !cut && out.ends_with('\n') {
            out.pop();
        }
        ok(out)
    }

    /// The folder's listing, two levels deep, hidden items left out.
    fn listing(&self, dir: &Path) -> String {
        let mut rows = vec![format!("{}\t{}", human(dir_size(dir)), self.shown(dir))];
        fn rec(d: &Path, depth: usize, rows: &mut Vec<String>, m: &Memory) {
            let Ok(rd) = std::fs::read_dir(d) else { return };
            let mut entries: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
            entries.sort();
            for p in entries {
                let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                if name.starts_with('.') || name == "node_modules" {
                    continue;
                }
                let size = if p.is_dir() { dir_size(&p) } else { p.metadata().map_or(0, |m| m.len()) };
                rows.push(format!("{}\t{}", human(size), m.shown(&p)));
                if p.is_dir() && depth < 2 {
                    rec(&p, depth + 1, rows, m);
                }
            }
        }
        rec(dir, 1, &mut rows, self);
        format!("Here're the files and directories up to 2 levels deep in {}, excluding hidden items and node_modules:\n{}", self.shown(dir), rows.join("\n"))
    }

    fn count(&self) -> usize {
        self.files().len()
    }

    /// Every memory file: its path under `/memories` and its text (for the panel).
    pub fn files(&self) -> Vec<(String, String)> {
        fn rec(d: &Path, out: &mut Vec<PathBuf>) {
            let Ok(rd) = std::fs::read_dir(d) else { return };
            for e in rd.flatten() {
                let p = e.path();
                if p.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.')) {
                    continue;
                }
                if p.is_dir() {
                    rec(&p, out);
                } else {
                    out.push(p);
                }
            }
        }
        let mut paths = Vec::new();
        rec(&self.dir, &mut paths);
        paths.sort();
        paths.into_iter().map(|p| (self.shown(&p), std::fs::read_to_string(&p).unwrap_or_default())).collect()
    }

    /// Saves your edit of a memory file.
    pub fn save(&self, path: &str, text: &str) -> Result<(), String> {
        let real = self.resolve(path).filter(|r| r.is_file()).ok_or("that note is gone")?;
        crate::settings::write_atomic(&real, text.as_bytes()).map_err(|e| e.to_string())
    }

    /// Deletes one memory file.
    pub fn delete(&self, path: &str) -> Result<(), String> {
        let real = self.resolve(path).filter(|r| r.is_file()).ok_or("that note is gone")?;
        std::fs::remove_file(real).map_err(|e| e.to_string())
    }

    /// Forgets everything.
    pub fn clear(&self) -> Result<(), String> {
        match std::fs::remove_dir_all(&self.dir) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
            _ => Ok(()),
        }
    }
}

fn dir_size(d: &Path) -> u64 {
    std::fs::read_dir(d)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| {
            let p = e.path();
            if p.is_dir() { dir_size(&p) } else { p.metadata().map_or(0, |m| m.len()) }
        })
        .sum::<u64>()
        .max(if d.is_dir() { 4096 } else { 0 })
}

/// `1.5K`, `2.0M`, `512`.
fn human(n: u64) -> String {
    match n {
        0..1024 => n.to_string(),
        1024..1_048_576 => format!("{:.1}K", n as f64 / 1024.0),
        _ => format!("{:.1}M", n as f64 / 1_048_576.0),
    }
}

/// What a memory call is doing, for the panel.
pub fn step_label(input: &Value) -> String {
    let path = input["path"].as_str().unwrap_or_default();
    let name = path.strip_prefix("/memories/").unwrap_or(path);
    match input["command"].as_str().unwrap_or_default() {
        "view" if path == ROOT || path == "/memories/" => "Checked the project's memory".into(),
        "view" => format!("Read its note {name}"),
        "create" | "str_replace" | "insert" => format!("Noted in memory: {name}"),
        "delete" => format!("Removed the note {name}"),
        "rename" => "Tidied its notes".into(),
        _ => "Used its memory".into(),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn mem(name: &str) -> (PathBuf, Memory) {
        let root = std::env::temp_dir().join(format!("oc-mem-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let m = Memory::new(&root);
        (root, m)
    }

    fn run(m: &Memory, v: Value) -> Out {
        m.run(&v)
    }

    #[test]
    fn notes_are_made_read_changed_and_removed() {
        let (root, m) = mem("ops");
        let (list, e) = run(&m, json!({"command": "view", "path": "/memories"}));
        assert!(!e && list.starts_with("Here're the files") && list.contains("\t/memories"), "{list}");
        assert!(!run(&m, json!({"command": "create", "path": "/memories/project.md", "file_text": "Budget in GBP\nPriya owns the deck\n"})).1);
        assert!(root.join(".officecraft/memory/project.md").is_file());
        let (v, _) = run(&m, json!({"command": "view", "path": "/memories/project.md"}));
        assert_eq!(v, "Here's the content of /memories/project.md with line numbers:\n     1\tBudget in GBP\n     2\tPriya owns the deck");
        let (v, _) = run(&m, json!({"command": "view", "path": "/memories/project.md", "view_range": [2, -1]}));
        assert!(v.ends_with("     2\tPriya owns the deck") && !v.contains("GBP"));
        assert!(!run(&m, json!({"command": "str_replace", "path": "/memories/project.md", "old_str": "GBP", "new_str": "EUR"})).1);
        assert!(run(&m, json!({"command": "str_replace", "path": "/memories/project.md", "old_str": "USD", "new_str": "x"})).0.contains("did not appear"));
        assert!(!run(&m, json!({"command": "insert", "path": "/memories/project.md", "insert_line": 0, "insert_text": "# Notes\n"})).1);
        assert_eq!(m.files(), vec![("/memories/project.md".into(), "# Notes\nBudget in EUR\nPriya owns the deck\n".into())]);
        assert!(run(&m, json!({"command": "insert", "path": "/memories/project.md", "insert_line": 9, "insert_text": "x"})).1);
        assert!(!run(&m, json!({"command": "rename", "old_path": "/memories/project.md", "new_path": "/memories/people/owners.md"})).1);
        let (list, _) = run(&m, json!({"command": "view", "path": "/memories"}));
        assert!(list.contains("/memories/people/owners.md"), "{list}");
        assert!(!run(&m, json!({"command": "delete", "path": "/memories/people"})).1);
        assert!(m.files().is_empty());
        assert!(run(&m, json!({"command": "delete", "path": "/memories"})).1, "the root stays");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn paths_stay_in_the_memory_folder() {
        let (root, m) = mem("jail");
        std::fs::write(root.join("secret.txt"), "x").unwrap();
        for bad in ["/memories/../secret.txt", "/etc/passwd", "memories/a.md", "/memories/./a.md", "/memories/.hidden", "/memories/%2e%2e/x", "/memoriesx/a"] {
            let (_, e) = run(&m, json!({"command": "create", "path": bad, "file_text": "x"}));
            assert!(e, "{bad} must be refused");
        }
        #[cfg(unix)]
        {
            std::fs::create_dir_all(m.dir()).unwrap();
            std::os::unix::fs::symlink(&root, m.dir().join("out")).unwrap();
            assert!(run(&m, json!({"command": "view", "path": "/memories/out/secret.txt"})).1, "a link out is refused");
        }
        assert!(run(&m, json!({"command": "create", "path": "/memories/big.md", "file_text": "x".repeat(MAX_FILE + 1)})).1);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn labels() {
        assert_eq!(step_label(&json!({"command": "view", "path": "/memories"})), "Checked the project's memory");
        assert_eq!(step_label(&json!({"command": "create", "path": "/memories/budget.md"})), "Noted in memory: budget.md");
    }
}
