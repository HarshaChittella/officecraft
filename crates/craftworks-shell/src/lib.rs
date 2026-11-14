//! Craftworks' own chrome around the Craft engines (docs/design.md): design tokens, icons
//! drawn in code, the tab strip, the new-tab and start page, recent files and toasts. Pure
//! egui; no editor code lives here.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod icons;
pub mod newtab;
pub mod recent;
pub mod tabs;
pub mod toast;
pub mod tokens;

pub use newtab::{NewTab, NewTabAction, RecentView};
pub use recent::{Recent, RecentFile};
pub use tabs::{TabAction, TabItem, TabStripState, tab_strip};
pub use toast::Toasts;
pub use tokens::Palette;
