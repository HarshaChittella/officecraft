//! OfficeCraft core: the shell's view of the four Craft engines.
//!
//! - [`Kind`]: the four kinds of tab, and routing files to them by extension.
//! - [`Editor`]: one Craft app instance with all its open documents (docs/decisions.md D2).
//! - [`Tabs`]: the window's tab strip as data.
//! - [`engines`]: the [`Editor`] adapters for WordCraft, GridCraft, DeckCraft and PdfCraft.
//! - [`project`], [`tree`], [`index`]: projects (a folder and its settings), their files, and
//!   search across them with each app's own reader.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod catalog;
mod editor;
pub mod engines;
pub mod index;
pub mod keymaps;
mod kind;
pub mod project;
mod tabs;
pub mod tree;

pub use editor::{CloseState, DocId, DocInfo, Editor, EditorError, Opened, Result};
pub use kind::Kind;
pub use tabs::{Group, GroupId, Tab, TabId, Tabs};
