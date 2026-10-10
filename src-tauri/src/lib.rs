//! recipe-sync core: turns an Instagram recipe post into a WordPress post.
//!
//! Parsing, domain logic and I/O live in separate modules so everything except the thin Tauri
//! shell in `gui/main.rs` is unit-testable without a window or the network.

pub mod instagram_api;
pub mod instagram_url;
pub mod token;
