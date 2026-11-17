//! The services the Craft apps ask their host for (each app's `Services`): the clipboard with
//! rich formats, file panels, printing and opening links, built from the same safe crates the
//! Craft apps' own desktop builds use (`arboard`, objc2's safe AppKit calls, `rfd`,
//! `craft-print`). No `unsafe` (docs/decisions.md D24).
//!
//! [`Platform`] holds the clipboard (the system's, or an in-process one for tests and
//! `--check`); [`editor`] makes an engine with its services filled in.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod activity;
pub mod clipboard;
pub mod display;
pub mod files;
pub mod print;
mod services;

pub use clipboard::{Clipboard, Copy, MemoryClipboard, Owner, SystemClipboard};
pub use services::{deck_services, editor, sheet_services, word_services};

use std::sync::Arc;

/// The host's side of the services, shared by every engine.
#[derive(Clone)]
pub struct Platform {
    pub clipboard: Arc<dyn Clipboard>,
    /// Real file panels and printing; `false` in tests and `--check`, which mustn't open
    /// windows or print (a panel then answers "cancelled").
    pub interactive: bool,
}

impl Platform {
    /// The real clipboard, panels and printers.
    pub fn system() -> Platform {
        Platform { clipboard: Arc::new(SystemClipboard), interactive: true }
    }

    /// An in-process clipboard and no panels or printing (tests, `--check`).
    pub fn isolated(clipboard: Arc<MemoryClipboard>) -> Platform {
        Platform { clipboard, interactive: false }
    }
}
