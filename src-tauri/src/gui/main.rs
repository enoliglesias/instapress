#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
//! Tauri shell: one window that hosts the static frontend in `ui/`.
//!
//! Commands are thin adapters over the `recipe_sync` library; the webview only renders and
//! sends validated requests. No async runtime is needed yet.

use anyhow::{Context, Result};
use recipe_sync::instagram_url::{self, InstagramTarget};

#[tauri::command]
fn classify_instagram_url(url: &str) -> Result<InstagramTarget, String> {
    instagram_url::parse(url).map_err(|e| e.to_string())
}

fn main() -> Result<()> {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![classify_instagram_url])
        .run(tauri::generate_context!())
        .context("failed to run the recipe-sync window")
}
