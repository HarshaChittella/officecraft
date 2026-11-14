//! Recent files for the new-tab page: newest first, kept between launches as JSON.

use chrono::{DateTime, Datelike, Local, TimeZone};
use craftworks_core::Kind;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// How many files the list keeps.
pub const MAX: usize = 30;

#[derive(Clone, Debug, PartialEq)]
pub struct RecentFile {
    pub path: PathBuf,
    pub kind: Kind,
    /// When it was last opened, in seconds since the Unix epoch.
    pub opened: i64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Recent {
    files: Vec<RecentFile>,
}

impl Recent {
    pub fn files(&self) -> &[RecentFile] {
        &self.files
    }

    /// Notes that `path` was opened at `when` (seconds since the epoch): it moves to the top.
    /// Files no engine opens are ignored.
    pub fn opened(&mut self, path: &Path, when: i64) {
        let Some(kind) = Kind::for_path(path) else { return };
        self.files.retain(|f| f.path != path);
        self.files.insert(0, RecentFile { path: path.to_path_buf(), kind, opened: when });
        self.files.truncate(MAX);
    }

    pub fn forget(&mut self, path: &Path) {
        self.files.retain(|f| f.path != path);
    }

    /// The files whose name or folder contains `filter` (any case), newest first.
    pub fn matching(&self, filter: &str) -> Vec<&RecentFile> {
        let f = filter.trim().to_lowercase();
        self.files.iter().filter(|r| f.is_empty() || r.path.to_string_lossy().to_lowercase().contains(&f)).collect()
    }

    pub fn to_json(&self) -> Value {
        json!({
            "version": 1,
            "files": self.files.iter().map(|f| json!({"path": f.path.to_string_lossy(), "opened": f.opened})).collect::<Vec<_>>(),
        })
    }

    /// Reads [`Recent::to_json`]'s output; anything malformed is skipped, never fatal.
    pub fn from_json(v: &Value) -> Recent {
        let mut r = Recent::default();
        for f in v.get("files").and_then(Value::as_array).into_iter().flatten().take(MAX) {
            let (Some(path), Some(opened)) = (f.get("path").and_then(Value::as_str), f.get("opened").and_then(Value::as_i64)) else { continue };
            let path = PathBuf::from(path);
            if let Some(kind) = Kind::for_path(&path)
                && !r.files.iter().any(|x| x.path == path)
            {
                r.files.push(RecentFile { path, kind, opened });
            }
        }
        r
    }
}

/// `path`'s folder for display, with the home folder shortened to `~`.
pub fn folder_label(path: &Path, home: Option<&Path>) -> String {
    let dir = path.parent().unwrap_or(path);
    match home.and_then(|h| dir.strip_prefix(h).ok()) {
        Some(rest) if rest.as_os_str().is_empty() => "~".to_string(),
        Some(rest) => format!("~/{}", rest.display()),
        None => dir.display().to_string(),
    }
}

/// When something happened, relative to `now`: "Just now", "5 min ago", "2 h ago",
/// "Yesterday", a weekday within the week, else a date. Calendar days are local (chrono).
pub fn relative_time(then: i64, now: DateTime<Local>) -> String {
    let Some(then) = Local.timestamp_opt(then, 0).single() else { return String::new() };
    let secs = (now - then).num_seconds();
    if secs < 60 {
        return "Just now".into();
    }
    if secs < 3600 {
        return format!("{} min ago", secs / 60);
    }
    let days = (now.date_naive() - then.date_naive()).num_days();
    match days {
        0 => format!("{} h ago", secs / 3600),
        1 => "Yesterday".into(),
        2..=6 => then.format("%a").to_string(),
        _ if then.year() == now.year() => then.format("%-d %b").to_string(),
        _ => then.format("%-d %b %Y").to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(y: i32, m: u32, d: u32, h: u32, min: u32) -> DateTime<Local> {
        Local.with_ymd_and_hms(y, m, d, h, min, 0).single().unwrap()
    }

    #[test]
    fn opening_moves_a_file_to_the_top_without_duplicates() {
        let mut r = Recent::default();
        r.opened(Path::new("/a/one.docx"), 1);
        r.opened(Path::new("/a/two.xlsx"), 2);
        r.opened(Path::new("/a/one.docx"), 3);
        let names: Vec<_> = r.files().iter().map(|f| f.path.display().to_string()).collect();
        assert_eq!(names, ["/a/one.docx", "/a/two.xlsx"]);
        assert_eq!(r.files()[0].kind, Kind::Document);
        r.opened(Path::new("/a/notes.zip"), 4);
        assert_eq!(r.files().len(), 2, "files no engine opens aren't recent files");
    }

    #[test]
    fn the_list_is_capped() {
        let mut r = Recent::default();
        for i in 0..(MAX + 5) {
            r.opened(Path::new(&format!("/f/{i}.pdf")), i as i64);
        }
        assert_eq!(r.files().len(), MAX);
        assert_eq!(r.files()[0].path, PathBuf::from(format!("/f/{}.pdf", MAX + 4)));
    }

    #[test]
    fn filtering_matches_name_or_folder_in_any_case() {
        let mut r = Recent::default();
        r.opened(Path::new("/Work/Finance/Budget.xlsx"), 1);
        r.opened(Path::new("/Downloads/Contract.pdf"), 2);
        assert_eq!(r.matching("budget").len(), 1);
        assert_eq!(r.matching("FINANCE").len(), 1);
        assert_eq!(r.matching("  ").len(), 2);
        assert!(r.matching("zzz").is_empty());
    }

    #[test]
    fn json_round_trips_and_malformed_entries_are_skipped() {
        let mut r = Recent::default();
        r.opened(Path::new("/a/one.docx"), 10);
        r.opened(Path::new("/a/two.pptx"), 20);
        assert_eq!(Recent::from_json(&r.to_json()), r);
        let junk = json!({"files": [{"path": 5}, {"opened": 1}, {"path": "/x.png", "opened": 1}, {"path": "/ok.pdf", "opened": 2}, {"path": "/ok.pdf", "opened": 3}]});
        assert_eq!(Recent::from_json(&junk).files().len(), 1);
        assert!(Recent::from_json(&json!("nonsense")).files().is_empty());
    }

    #[test]
    fn folders_shorten_the_home_folder() {
        let home = Path::new("/Users/me");
        assert_eq!(folder_label(Path::new("/Users/me/Work/a.pdf"), Some(home)), "~/Work");
        assert_eq!(folder_label(Path::new("/Users/me/a.pdf"), Some(home)), "~");
        assert_eq!(folder_label(Path::new("/tmp/a.pdf"), Some(home)), "/tmp");
    }

    #[test]
    fn relative_times_read_naturally() {
        let now = at(2026, 11, 14, 15, 0);
        let ts = |d: DateTime<Local>| d.timestamp();
        assert_eq!(relative_time(ts(now) - 30, now), "Just now");
        assert_eq!(relative_time(ts(at(2026, 11, 14, 14, 59)), now), "1 min ago");
        assert_eq!(relative_time(ts(at(2026, 11, 14, 14, 30)), now), "30 min ago");
        assert_eq!(relative_time(ts(at(2026, 11, 14, 13, 0)), now), "2 h ago");
        assert_eq!(relative_time(ts(at(2026, 11, 13, 23, 0)), now), "Yesterday");
        assert_eq!(relative_time(ts(at(2026, 11, 10, 9, 0)), now), "Tue");
        assert_eq!(relative_time(ts(at(2026, 3, 2, 9, 0)), now), "2 Mar");
        assert_eq!(relative_time(ts(at(2025, 3, 2, 9, 0)), now), "2 Mar 2025");
    }
}
