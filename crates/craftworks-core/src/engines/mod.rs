//! [`Editor`](crate::Editor) adapters for the four Craft apps. Each wraps one app instance
//! (docs/decisions.md D2) and drives it only through the app's public API: no editor is
//! re-implemented here.

mod deck;
mod pdf;
mod sheet;
mod word;

pub use deck::DeckEditor;
pub use pdf::{GO_TO_PAGE, PdfEditor};
pub use sheet::SheetEditor;
pub use word::WordEditor;

use crate::{Editor, EditorError, Kind, Result};
use std::path::Path;

/// A new engine of `kind`, with no document open yet where the app allows that.
pub fn new_editor(kind: Kind) -> Box<dyn Editor> {
    match kind {
        Kind::Document => Box::new(WordEditor::new()),
        Kind::Sheet => Box::new(SheetEditor::new()),
        Kind::Presentation => Box::new(DeckEditor::new()),
        Kind::Pdf => Box::new(PdfEditor::new()),
    }
}

/// `path` as the UTF-8 string the apps' commands take.
fn path_str(path: &Path) -> Result<&str> {
    path.to_str().ok_or_else(|| EditorError(format!("{} is not a UTF-8 path", path.display())))
}

/// The `u64` id an app uses, from a JSON reply field.
fn reply_id(v: &serde_json::Value, key: &str) -> Option<u64> {
    v.get(key).and_then(serde_json::Value::as_u64)
}
