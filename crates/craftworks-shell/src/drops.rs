//! Files dragged onto the window: documents Craftworks opens go to tabs, anything else (a
//! picture, say) to the shown document.

use craftworks_core::Kind;
use std::path::{Path, PathBuf};

/// Splits dropped files into those that open in tabs and those the shown document gets.
pub fn split(paths: &[PathBuf]) -> (Vec<PathBuf>, Vec<PathBuf>) {
    paths.iter().cloned().partition(|p| opens_in_a_tab(p))
}

/// Whether a dropped `path` opens in a tab of its own (any kind Craftworks opens).
pub fn opens_in_a_tab(path: &Path) -> bool {
    Kind::for_path(path).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn documents_open_in_tabs_and_pictures_go_to_the_document() {
        let paths: Vec<PathBuf> =
            ["/a/Report.docx", "/a/photo.png", "/a/Contract.PDF", "/a/notes.zip", "/a/Budget.xlsx"].iter().map(PathBuf::from).collect();
        let (tabs, document) = split(&paths);
        assert_eq!(tabs, vec![PathBuf::from("/a/Report.docx"), PathBuf::from("/a/Contract.PDF"), PathBuf::from("/a/Budget.xlsx")]);
        assert_eq!(document, vec![PathBuf::from("/a/photo.png"), PathBuf::from("/a/notes.zip")]);
    }
}
