//! Craftworks' own chrome around the Craft engines (docs/design.md): design tokens, icons
//! drawn in code, the tab strip, the new-tab and start page, recent files, the project sidebar and toasts. Pure
//! egui; no editor code lives here.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod ai;
pub mod drops;
pub mod icons;
pub mod keys;
pub mod newtab;
pub mod recent;
pub mod sidebar;
pub mod tabs;
pub mod toast;
pub mod tokens;
pub mod widgets;

pub use newtab::{NewTab, NewTabAction, RecentView};
pub use recent::{Recent, RecentFile};
pub use sidebar::{ProjectItem, Sidebar, SidebarAction, SidebarView};
pub use tabs::{GroupItem, ProjectChoice, TabAction, TabItem, TabStripState, tab_strip};
pub use toast::Toasts;
pub use tokens::Palette;
