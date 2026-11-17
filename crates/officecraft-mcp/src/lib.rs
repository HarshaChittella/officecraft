//! OfficeCraft's one AI connector (Phase 7): an MCP server over all four Craft engines and the
//! project folder. Edits to a file open in the OfficeCraft window go to its tab, live
//! ([`live`]); other files are worked on headless and saved only when allowed.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod jail;
pub mod live;
pub mod server;

pub use server::Server;
