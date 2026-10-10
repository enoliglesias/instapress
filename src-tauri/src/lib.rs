//! Instapress core: turns an Instagram post into a WordPress post.
//!
//! Parsing, domain logic and I/O live in separate modules so everything except the thin Tauri
//! shell in `gui/main.rs` is unit-testable without a window or the network.

pub mod http;
pub mod image;
pub mod instagram_api;
pub mod instagram_url;
pub mod publication_html;
pub mod publish;
pub mod token;
pub mod video;
pub mod wordpress;
