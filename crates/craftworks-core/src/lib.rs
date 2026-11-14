//! Craftworks core: the shell's view of the four Craft engines.
//!
//! Phase 0 stub: it only links the four editors so the workspace proves they build together.
//! The `Editor` trait, tab model and routing arrive in Phase 1 (docs/PLAN.md).
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

pub use deckcraft_ui_egui::SlideApp;
pub use gridcraft_ui_egui::SheetApp;
pub use pdfcraft_ui_egui::PdfCraftApp;
pub use wordcraft_ui_egui::WordApp;
