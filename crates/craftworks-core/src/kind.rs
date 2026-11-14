//! The four kinds of tab, and which one opens a file (routing by extension).

use std::path::Path;

/// Which engine a tab belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Kind {
    /// WordCraft.
    Document,
    /// GridCraft.
    Sheet,
    /// DeckCraft.
    Presentation,
    /// PdfCraft.
    Pdf,
}

impl Kind {
    /// Every kind, in the order the "+" chooser lists them.
    pub const ALL: [Kind; 4] = [Kind::Document, Kind::Sheet, Kind::Presentation, Kind::Pdf];

    /// The kind that opens `path`, from its extension (any case). `None` for a file no engine
    /// opens, or one without an extension.
    pub fn for_path(path: &Path) -> Option<Kind> {
        path.extension()
            .and_then(|e| e.to_str())
            .and_then(Kind::for_extension)
    }

    /// The kind that opens files with extension `ext` (without the dot, any case).
    pub fn for_extension(ext: &str) -> Option<Kind> {
        match ext.to_ascii_lowercase().as_str() {
            "docx" | "doc" | "odt" | "rtf" | "html" | "htm" | "md" | "markdown" | "tex" | "txt" => {
                Some(Kind::Document)
            }
            "xlsx" | "csv" | "tsv" => Some(Kind::Sheet),
            "pptx" => Some(Kind::Presentation),
            "pdf" => Some(Kind::Pdf),
            _ => None,
        }
    }

    /// Whether files of this extension open read-only: they can be read but not written back in
    /// the same format (`.doc`), so Save asks for another format.
    pub fn read_only_extension(ext: &str) -> bool {
        ext.eq_ignore_ascii_case("doc")
    }

    /// The name the interface uses for a new file of this kind.
    pub fn label(self) -> &'static str {
        match self {
            Kind::Document => "Document",
            Kind::Sheet => "Sheet",
            Kind::Presentation => "Presentation",
            Kind::Pdf => "PDF",
        }
    }

    /// Whether the "+" chooser can make a new, empty file of this kind (a PDF is opened, not made).
    pub fn can_create(self) -> bool {
        !matches!(self, Kind::Pdf)
    }
}

#[cfg(test)]
mod tests {
    use super::Kind;
    use std::path::Path;

    #[test]
    fn every_supported_extension_routes_to_its_engine() {
        for ext in [
            "docx", "doc", "odt", "rtf", "html", "htm", "md", "markdown", "tex", "txt",
        ] {
            assert_eq!(Kind::for_extension(ext), Some(Kind::Document), "{ext}");
        }
        for ext in ["xlsx", "csv", "tsv"] {
            assert_eq!(Kind::for_extension(ext), Some(Kind::Sheet), "{ext}");
        }
        assert_eq!(Kind::for_extension("pptx"), Some(Kind::Presentation));
        assert_eq!(Kind::for_extension("pdf"), Some(Kind::Pdf));
    }

    #[test]
    fn extensions_match_in_any_case() {
        assert_eq!(
            Kind::for_path(Path::new("/x/Report.DOCX")),
            Some(Kind::Document)
        );
        assert_eq!(Kind::for_path(Path::new("Budget.Xlsx")), Some(Kind::Sheet));
        assert_eq!(Kind::for_path(Path::new("a.PDF")), Some(Kind::Pdf));
    }

    #[test]
    fn unknown_or_missing_extensions_route_nowhere() {
        for p in [
            "notes",
            "archive.zip",
            "photo.png",
            ".docx",
            "deck.ppt",
            "book.xls",
            "dir.docx/",
            "",
        ] {
            let got = Kind::for_path(Path::new(p));
            // `dir.docx/` is a path whose last part is `dir.docx`: it routes like the file name does.
            if p == "dir.docx/" {
                assert_eq!(got, Some(Kind::Document));
            } else {
                assert_eq!(got, None, "{p:?}");
            }
        }
    }

    #[test]
    fn only_legacy_word_opens_read_only() {
        assert!(Kind::read_only_extension("doc"));
        assert!(Kind::read_only_extension("DOC"));
        assert!(!Kind::read_only_extension("docx"));
    }

    #[test]
    fn a_pdf_is_opened_not_created() {
        assert!(Kind::ALL.iter().filter(|k| k.can_create()).count() == 3);
        assert!(!Kind::Pdf.can_create());
    }
}
