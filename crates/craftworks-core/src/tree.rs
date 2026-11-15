//! A project's files: a bounded scan of its folder, and a watcher that rescans on a timer and
//! reports only when something changed. Polling, not file-system events: no new dependency
//! (docs/decisions.md D39), and a project folder is small enough that a scan every two seconds
//! costs little.

use crate::Kind;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, SystemTime};

/// Limits on a scan, so a huge or looping folder can't stall the app.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub max_depth: usize,
    pub max_entries: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits { max_depth: 12, max_entries: 20_000 }
    }
}

/// One file or folder in a project.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub path: PathBuf,
    /// Path relative to the project, `/`-separated.
    pub rel: String,
    pub depth: usize,
    pub is_dir: bool,
    /// The engine that opens it; `None` for folders and files no engine opens.
    pub kind: Option<Kind>,
    pub modified: Option<SystemTime>,
    pub len: u64,
}

/// A scan's result: entries in tree order (each folder followed by its contents; folders
/// before files, then by name, case-insensitively).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub entries: Vec<Entry>,
    /// The scan stopped at a limit.
    pub truncated: bool,
}

impl Snapshot {
    /// The documents Craftworks opens.
    pub fn documents(&self) -> impl Iterator<Item = &Entry> {
        self.entries.iter().filter(|e| e.kind.is_some())
    }
}

/// Folders a scan never enters (besides hidden ones, `.git` and `.craftworks` among them):
/// well-known build and dependency folders.
fn skipped(name: &str) -> bool {
    matches!(name, "node_modules" | "target" | "__pycache__" | "Library")
}

/// Scans `root`. Symbolic links aren't followed (no loops, nothing outside the project).
pub fn scan(root: &Path, limits: Limits) -> Snapshot {
    let mut snap = Snapshot::default();
    walk(root, root, 0, limits, &mut snap);
    snap
}

fn walk(root: &Path, dir: &Path, depth: usize, limits: Limits, snap: &mut Snapshot) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut items: Vec<(String, PathBuf, std::fs::Metadata)> = rd
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            // `DirEntry::metadata` doesn't follow symbolic links.
            let md = e.metadata().ok()?;
            (!name.starts_with('.') && !md.file_type().is_symlink()).then(|| (name, e.path(), md))
        })
        .collect();
    items.sort_by(|a, b| b.2.is_dir().cmp(&a.2.is_dir()).then_with(|| a.0.to_lowercase().cmp(&b.0.to_lowercase())));
    for (name, path, md) in items {
        if snap.entries.len() >= limits.max_entries {
            snap.truncated = true;
            return;
        }
        let is_dir = md.is_dir();
        if is_dir && skipped(&name) {
            continue;
        }
        let rel = path
            .strip_prefix(root)
            .map(|r| r.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/"))
            .unwrap_or_default();
        let kind = if is_dir { None } else { Kind::for_path(&path) };
        snap.entries.push(Entry {
            path: path.clone(),
            rel,
            depth,
            is_dir,
            kind,
            modified: md.modified().ok(),
            len: if is_dir { 0 } else { md.len() },
        });
        if is_dir {
            if depth + 1 >= limits.max_depth {
                snap.truncated = true;
            } else {
                walk(root, &path, depth + 1, limits, snap);
            }
        }
    }
}

/// Rescans a folder on a background thread and sends each snapshot that differs from the
/// last. Dropping it stops the thread (within one interval).
pub struct Watcher {
    rx: Receiver<Snapshot>,
    _stop: Sender<()>,
}

impl Watcher {
    /// Starts watching `root`: the first snapshot comes at once, later ones on change.
    /// `wake` is called after each one is sent (to repaint the window).
    pub fn start(root: PathBuf, every: Duration, limits: Limits, wake: impl Fn() + Send + 'static) -> Watcher {
        let (tx, rx) = channel();
        let (stop_tx, stop_rx) = channel::<()>();
        let spawned = std::thread::Builder::new().name("craftworks-watch".into()).spawn(move || {
            let mut last: Option<Snapshot> = None;
            loop {
                let snap = scan(&root, limits);
                if last.as_ref() != Some(&snap) {
                    if tx.send(snap.clone()).is_err() {
                        return;
                    }
                    wake();
                    last = Some(snap);
                }
                match stop_rx.recv_timeout(every) {
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                    _ => return,
                }
            }
        });
        if let Err(e) = spawned {
            log::warn!("watching the project folder: {e}");
        }
        Watcher { rx, _stop: stop_tx }
    }

    /// The newest snapshot since the last call, if any.
    pub fn latest(&self) -> Option<Snapshot> {
        self.rx.try_iter().last()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("cw-tree-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn scans_in_tree_order_and_skips_hidden_and_build_folders() {
        let root = scratch("scan");
        for f in ["b.docx", "A.pdf", "notes.png", "Sub/deck.pptx", ".craftworks/project.json", "node_modules/x.docx", ".hidden.docx"] {
            let p = root.join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, b"x").unwrap();
        }
        let snap = scan(&root, Limits::default());
        let rels: Vec<_> = snap.entries.iter().map(|e| e.rel.as_str()).collect();
        assert_eq!(rels, ["Sub", "Sub/deck.pptx", "A.pdf", "b.docx", "notes.png"]);
        assert_eq!(snap.documents().count(), 3);
        assert_eq!(snap.entries[1].depth, 1);
        assert!(!snap.truncated);
        let small = scan(&root, Limits { max_depth: 12, max_entries: 2 });
        assert!(small.truncated && small.entries.len() == 2);
    }

    #[test]
    fn the_watcher_reports_changes_only() {
        let root = scratch("watch");
        std::fs::write(root.join("a.docx"), b"x").unwrap();
        let w = Watcher::start(root.clone(), Duration::from_millis(20), Limits::default(), || {});
        let first = (0..200).find_map(|_| {
            std::thread::sleep(Duration::from_millis(10));
            w.latest()
        });
        assert_eq!(first.unwrap().entries.len(), 1);
        std::thread::sleep(Duration::from_millis(100));
        assert!(w.latest().is_none(), "nothing changed, nothing sent");
        std::fs::write(root.join("b.xlsx"), b"x").unwrap();
        let next = (0..200).find_map(|_| {
            std::thread::sleep(Duration::from_millis(10));
            w.latest()
        });
        assert_eq!(next.unwrap().entries.len(), 2);
    }
}
