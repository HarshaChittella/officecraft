//! Project search: each file's text, read with its own app's reader (no window, no egui), in
//! an in-memory inverted index of lowercase word tokens. An [`Indexer`] keeps the index in step
//! with the [`crate::tree::Watcher`]'s snapshots on a background thread.

use crate::Kind;
use crate::tree::Snapshot;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

/// Files larger than this aren't read.
const MAX_FILE: u64 = 200 * 1024 * 1024;
/// Sections (paragraphs, cells, slides, pages) kept per file.
const MAX_SECTIONS: usize = 100_000;
/// Text kept per file, in bytes.
const MAX_TEXT: usize = 32 * 1024 * 1024;
/// PDF pages read per file.
const MAX_PDF_PAGES: usize = 5_000;

/// Where in a file a piece of text is: what the engine can be asked to show.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Location {
    /// A document's paragraph (0-based); shown with the app's Find.
    Paragraph(usize),
    /// A sheet's cell, as `Sheet1` and `B4`.
    Cell { sheet: String, cell: String },
    /// A slide (0-based); `notes` when the text is in its speaker notes.
    Slide { index: usize, notes: bool },
    /// A PDF page (0-based).
    Page(usize),
}

impl Location {
    /// How the result list names it.
    pub fn label(&self) -> String {
        match self {
            Location::Paragraph(_) => String::new(),
            Location::Cell { sheet, cell } => format!("{sheet}!{cell}"),
            Location::Slide { index, notes: false } => format!("Slide {}", index + 1),
            Location::Slide { index, notes: true } => format!("Slide {} notes", index + 1),
            Location::Page(p) => format!("Page {}", p + 1),
        }
    }
}

/// A file's text, in pieces with their locations.
pub type Sections = Vec<(Location, String)>;

/// Reads the text of `path` with the reader of the app that opens it.
pub fn read_text(path: &Path) -> Result<Sections, String> {
    let kind = Kind::for_path(path).ok_or("not a document Craftworks opens")?;
    let len = std::fs::metadata(path).map_err(|e| e.to_string())?.len();
    if len > MAX_FILE {
        return Err(format!("too large to index ({} MB)", len / (1024 * 1024)));
    }
    // A reader that trips over a damaged file must not take the indexer with it.
    let read = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match kind {
        Kind::Document => read_document(path),
        Kind::Sheet => read_sheet(path),
        Kind::Presentation => read_presentation(path),
        Kind::Pdf => read_pdf(path),
    }));
    let mut sections = read.map_err(|_| "the reader failed on this file".to_string())??;
    sections.retain(|(_, t)| !t.trim().is_empty());
    sections.truncate(MAX_SECTIONS);
    let mut total = 0;
    sections.retain(|(_, t)| {
        total += t.len();
        total <= MAX_TEXT
    });
    Ok(sections)
}

fn read_document(path: &Path) -> Result<Sections, String> {
    let doc = wordcraft_engine::io::open_path(path)?;
    let text = doc.plain_text(wordcraft_doc::StoryRef::Body);
    Ok(text.split(['\n', '\r']).enumerate().map(|(i, t)| (Location::Paragraph(i), t.to_string())).collect())
}

fn read_sheet(path: &Path) -> Result<Sections, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let (wb, _) = gridcraft_engine::io::open_bytes(&name, &bytes).map_err(|e| e.to_string())?;
    let mut out = Sections::new();
    for sheet in &wb.sheets {
        for (at, cell) in sheet.cells.iter() {
            if let Ok(t) = cell.value.to_text()
                && !t.is_empty()
            {
                out.push((Location::Cell { sheet: sheet.name.clone(), cell: at.a1() }, t));
            }
            if out.len() >= MAX_SECTIONS {
                return Ok(out);
            }
        }
    }
    Ok(out)
}

fn read_presentation(path: &Path) -> Result<Sections, String> {
    use deckcraft_engine::model::{Shape, ShapeKind};
    fn shapes(list: &[Shape], depth: usize, out: &mut String) {
        if depth > 32 {
            return;
        }
        for s in list {
            if let Some(t) = &s.text {
                out.push_str(&t.text());
                out.push('\n');
            }
            if let ShapeKind::Table(t) = &s.kind {
                for c in t.rows.iter().flat_map(|r| &r.cells) {
                    out.push_str(&c.text.text());
                    out.push('\n');
                }
            }
            shapes(s.children(), depth + 1, out);
        }
    }
    let loaded = deckcraft_engine::cmd::file::read_presentation_file(&path.to_string_lossy()).map_err(|e| e.to_string())?;
    let mut out = Sections::new();
    for (index, slide) in loaded.doc.slides.iter().enumerate() {
        let mut text = String::new();
        shapes(&slide.shapes, 0, &mut text);
        out.push((Location::Slide { index, notes: false }, text.replace('\u{b}', " ")));
        out.push((Location::Slide { index, notes: true }, slide.notes_text()));
    }
    Ok(out)
}

fn read_pdf(path: &Path) -> Result<Sections, String> {
    use pdfcraft_render::{PageRenderer, RenderConfig, RenderRequest, RequestKind};
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let mut r = PageRenderer::new(Arc::new(bytes), RenderConfig::default());
    let pages = r.page_count();
    if pages == 0 {
        return Err("the PDF could not be read (damaged, or it needs a password)".into());
    }
    Ok((0..pages.min(MAX_PDF_PAGES))
        .filter_map(|page| {
            let rendered = r.render(RenderRequest { page, kind: RequestKind::Text, scale: 1.0, ..Default::default() });
            rendered.text.map(|t| (Location::Page(page), t.plain_text()))
        })
        .collect())
}

/// Lowercase word tokens: runs of letters and digits.
pub fn tokens(text: &str) -> impl Iterator<Item = String> + '_ {
    text.split(|c: char| !c.is_alphanumeric()).filter(|t| !t.is_empty()).map(str::to_lowercase)
}

/// A search result.
#[derive(Clone, Debug, PartialEq)]
pub struct Hit {
    pub path: PathBuf,
    pub location: Location,
    /// The text around the first match, on one line.
    pub snippet: String,
}

struct FileEntry {
    path: PathBuf,
    sections: Sections,
}

/// The index: files' sections and, per token, where it occurs.
#[derive(Default)]
pub struct Index {
    files: Vec<Option<FileEntry>>,
    by_path: HashMap<PathBuf, usize>,
    postings: HashMap<String, HashSet<(u32, u32)>>,
}

impl Index {
    pub fn len(&self) -> usize {
        self.by_path.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_path.is_empty()
    }

    /// Adds `path`'s text, replacing what was there.
    pub fn insert(&mut self, path: &Path, sections: Sections) {
        self.remove(path);
        let id = self.files.iter().position(Option::is_none).unwrap_or(self.files.len());
        for (s, (_, text)) in sections.iter().enumerate() {
            for t in tokens(text) {
                self.postings.entry(t).or_default().insert((id as u32, s as u32));
            }
        }
        let entry = Some(FileEntry { path: path.to_path_buf(), sections });
        if id == self.files.len() {
            self.files.push(entry);
        } else {
            self.files[id] = entry;
        }
        self.by_path.insert(path.to_path_buf(), id);
    }

    pub fn remove(&mut self, path: &Path) {
        let Some(id) = self.by_path.remove(path) else { return };
        let id32 = id as u32;
        if let Some(Some(f)) = self.files.get(id) {
            let words: HashSet<String> = f.sections.iter().flat_map(|(_, t)| tokens(t)).collect();
            for w in words {
                if let Some(set) = self.postings.get_mut(&w) {
                    set.retain(|(d, _)| *d != id32);
                    if set.is_empty() {
                        self.postings.remove(&w);
                    }
                }
            }
        }
        if let Some(slot) = self.files.get_mut(id) {
            *slot = None;
        }
    }

    /// Sections holding every word of `query`; the last word also matches as a prefix (so
    /// results come while typing). In file order, then location order; at most `limit`.
    pub fn search(&self, query: &str, limit: usize) -> Vec<Hit> {
        let words: Vec<String> = tokens(query).collect();
        let Some((last, rest)) = words.split_last() else { return Vec::new() };
        let mut found: Option<HashSet<(u32, u32)>> = None;
        for w in rest {
            let set = self.postings.get(w).cloned().unwrap_or_default();
            found = Some(match found {
                Some(f) => f.intersection(&set).copied().collect(),
                None => set,
            });
        }
        let mut prefix = HashSet::new();
        for (t, set) in &self.postings {
            if t.starts_with(last.as_str()) {
                prefix.extend(set.iter().copied());
            }
        }
        let found = match found {
            Some(f) => f.intersection(&prefix).copied().collect(),
            None => prefix,
        };
        let mut hits: Vec<(u32, u32)> = found.into_iter().collect();
        hits.sort_by(|a, b| {
            let pa = self.files.get(a.0 as usize).and_then(|f| f.as_ref()).map(|f| &f.path);
            let pb = self.files.get(b.0 as usize).and_then(|f| f.as_ref()).map(|f| &f.path);
            pa.cmp(&pb).then(a.1.cmp(&b.1))
        });
        hits.into_iter()
            .filter_map(|(d, s)| {
                let f = self.files.get(d as usize)?.as_ref()?;
                let (loc, text) = f.sections.get(s as usize)?;
                Some(Hit { path: f.path.clone(), location: loc.clone(), snippet: snippet(text, words.first().map_or("", String::as_str)) })
            })
            .take(limit)
            .collect()
    }
}

/// About 90 characters of `text` on one line, around the first match of `word`.
fn snippet(text: &str, word: &str) -> String {
    let flat: String = text.chars().map(|c| if c.is_whitespace() { ' ' } else { c }).collect();
    let chars: Vec<char> = flat.chars().collect();
    let lower: Vec<char> = flat.to_lowercase().chars().collect();
    let w: Vec<char> = word.chars().collect();
    // `to_lowercase` can change lengths for a few scripts; then just start at the beginning.
    let at = if lower.len() == chars.len() && !w.is_empty() { lower.windows(w.len()).position(|x| x == w.as_slice()).unwrap_or(0) } else { 0 };
    let start = at.saturating_sub(30);
    let end = (start + 90).min(chars.len());
    let mut s: String = chars[start..end].iter().collect();
    s = s.trim().to_string();
    if start > 0 {
        s.insert(0, '…');
    }
    if end < chars.len() {
        s.push('…');
    }
    s
}

/// How far the indexer has got.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Progress {
    pub indexed: usize,
    /// Files waiting to be read.
    pub pending: usize,
    /// Files that couldn't be read, with why.
    pub failed: Vec<(PathBuf, String)>,
}

/// Keeps an [`Index`] in step with the project's files on a background thread: it reads the
/// files a snapshot adds or changes and drops the ones it no longer has.
pub struct Indexer {
    tx: Sender<Snapshot>,
    index: Arc<Mutex<Index>>,
    progress: Arc<Mutex<Progress>>,
}

impl Indexer {
    /// `wake` is called whenever the index changed (to refresh the results shown).
    pub fn start(wake: impl Fn() + Send + 'static) -> Indexer {
        let (tx, rx) = channel::<Snapshot>();
        let index = Arc::new(Mutex::new(Index::default()));
        let progress = Arc::new(Mutex::new(Progress::default()));
        let (i2, p2) = (index.clone(), progress.clone());
        let spawned = std::thread::Builder::new().name("craftworks-index".into()).spawn(move || run(rx, i2, p2, wake));
        if let Err(e) = spawned {
            log::warn!("starting the project index: {e}");
        }
        Indexer { tx, index, progress }
    }

    /// The project's files changed (or the project did).
    pub fn update(&self, snap: Snapshot) {
        let _ = self.tx.send(snap);
    }

    pub fn search(&self, query: &str, limit: usize) -> Vec<Hit> {
        self.index.lock().map(|i| i.search(query, limit)).unwrap_or_default()
    }

    pub fn progress(&self) -> Progress {
        self.progress.lock().map(|p| p.clone()).unwrap_or_default()
    }
}

fn run(rx: Receiver<Snapshot>, index: Arc<Mutex<Index>>, progress: Arc<Mutex<Progress>>, wake: impl Fn()) {
    let mut seen: HashMap<PathBuf, (Option<SystemTime>, u64)> = HashMap::new();
    let mut failed: HashMap<PathBuf, String> = HashMap::new();
    while let Ok(mut snap) = rx.recv() {
        // Only the newest snapshot matters.
        while let Ok(newer) = rx.try_recv() {
            snap = newer;
        }
        let now: HashMap<PathBuf, (Option<SystemTime>, u64)> = snap.documents().map(|e| (e.path.clone(), (e.modified, e.len))).collect();
        let gone: Vec<PathBuf> = seen.keys().filter(|p| !now.contains_key(*p)).cloned().collect();
        if !gone.is_empty()
            && let Ok(mut i) = index.lock()
        {
            for p in &gone {
                i.remove(p);
            }
        }
        for p in &gone {
            seen.remove(p);
            failed.remove(p);
        }
        let mut todo: Vec<PathBuf> = now.iter().filter(|(p, stamp)| seen.get(*p) != Some(stamp)).map(|(p, _)| p.clone()).collect();
        todo.sort();
        let publish = |pending: usize, failed: &HashMap<PathBuf, String>, index: &Arc<Mutex<Index>>| {
            let indexed = index.lock().map(|i| i.len()).unwrap_or(0);
            let mut f: Vec<_> = failed.iter().map(|(p, e)| (p.clone(), e.clone())).collect();
            f.sort();
            if let Ok(mut g) = progress.lock() {
                *g = Progress { indexed, pending, failed: f };
            }
        };
        publish(todo.len(), &failed, &index);
        if !gone.is_empty() && todo.is_empty() {
            wake();
        }
        for (n, p) in todo.iter().enumerate() {
            match read_text(p) {
                Ok(sections) => {
                    failed.remove(p);
                    if let Ok(mut i) = index.lock() {
                        i.insert(p, sections);
                    }
                }
                Err(e) => {
                    log::info!("not indexed: {}: {e}", p.display());
                    failed.insert(p.clone(), e);
                    if let Ok(mut i) = index.lock() {
                        i.remove(p);
                    }
                }
            }
            if let Some(stamp) = now.get(p) {
                seen.insert(p.clone(), *stamp);
            }
            publish(todo.len() - n - 1, &failed, &index);
            wake();
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn words_match_in_order_and_the_last_as_a_prefix() {
        let mut i = Index::default();
        i.insert(
            Path::new("/p/a.docx"),
            vec![(Location::Paragraph(0), "Revenue grew in the North".into()), (Location::Paragraph(1), "Costs fell".into())],
        );
        i.insert(Path::new("/p/b.xlsx"), vec![(Location::Cell { sheet: "S".into(), cell: "A2".into() }, "North".into())]);
        let hits = i.search("nor", 10);
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].path, Path::new("/p/a.docx"));
        assert_eq!(hits[1].location.label(), "S!A2");
        assert_eq!(i.search("revenue north", 10).len(), 1, "every word must be in the section");
        assert!(i.search("costs north", 10).is_empty());
        assert!(i.search("  ", 10).is_empty());
        i.remove(Path::new("/p/a.docx"));
        assert_eq!(i.search("north", 10).len(), 1);
        assert!(i.search("revenue", 10).is_empty(), "a removed file's words are gone");
        i.insert(Path::new("/p/c.pdf"), vec![(Location::Page(2), "Grüße aus München".into())]);
        assert_eq!(i.search("MÜNCHEN", 10)[0].location.label(), "Page 3");
    }

    #[test]
    fn snippets_are_one_line_around_the_match() {
        let long = format!("{}needle\nin the haystack{}", "x ".repeat(60), " y".repeat(60));
        let s = snippet(&long, "needle");
        assert!(s.starts_with('…') && s.ends_with('…') && s.contains("needle in the haystack"), "{s}");
        assert_eq!(snippet("short text", "zzz"), "short text");
    }
}
