//! The open tabs, kept between launches (`session.json` in the settings folder): each tab's
//! file in strip order, and which one was in front. Untitled documents have no file and aren't
//! kept; window size and position are eframe's (persisted by it).

use serde_json::{Value, json};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Session {
    /// The tabs' files, left to right.
    pub files: Vec<PathBuf>,
    /// The tab in front, as an index into `files`.
    pub active: Option<usize>,
}

impl Session {
    pub fn to_json(&self) -> Value {
        json!({"version": 1, "tabs": self.files.iter().map(|p| p.to_string_lossy()).collect::<Vec<_>>(), "active": self.active})
    }

    /// Reads [`Session::to_json`]'s output; anything malformed is skipped, never fatal.
    pub fn from_json(v: &Value) -> Session {
        let files: Vec<PathBuf> =
            v.get("tabs").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).map(PathBuf::from).take(200).collect();
        let active = v.get("active").and_then(Value::as_u64).and_then(|a| usize::try_from(a).ok()).filter(|a| *a < files.len());
        Session { files, active }
    }

    /// The files that still exist, and the ones that don't (reported to the user).
    pub fn split_missing(&self, exists: impl Fn(&Path) -> bool) -> (Vec<PathBuf>, Vec<PathBuf>) {
        self.files.iter().cloned().partition(|p| exists(p))
    }
}

pub fn load() -> Session {
    crate::settings::dir()
        .and_then(|d| std::fs::read(d.join("session.json")).ok())
        .and_then(|b| serde_json::from_slice(&b).ok())
        .map(|v| Session::from_json(&v))
        .unwrap_or_default()
}

pub fn save(s: &Session) {
    let Some(d) = crate::settings::dir() else { return };
    if let Err(e) = crate::settings::write_atomic(&d.join("session.json"), s.to_json().to_string().as_bytes()) {
        log::warn!("saving the open tabs: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sessions_round_trip_and_junk_is_skipped() {
        let s = Session { files: vec![PathBuf::from("/a/x.docx"), PathBuf::from("/a/y.pdf")], active: Some(1) };
        assert_eq!(Session::from_json(&s.to_json()), s);
        let junk = json!({"tabs": [1, "/ok.xlsx", null], "active": 9});
        assert_eq!(Session::from_json(&junk), Session { files: vec![PathBuf::from("/ok.xlsx")], active: None });
        assert_eq!(Session::from_json(&json!("nope")), Session::default());
    }

    #[test]
    fn missing_files_are_set_aside() {
        let s = Session { files: vec![PathBuf::from("/here.docx"), PathBuf::from("/gone.pdf")], active: Some(0) };
        let (ok, gone) = s.split_missing(|p| p.ends_with("here.docx"));
        assert_eq!(ok, vec![PathBuf::from("/here.docx")]);
        assert_eq!(gone, vec![PathBuf::from("/gone.pdf")]);
    }
}
