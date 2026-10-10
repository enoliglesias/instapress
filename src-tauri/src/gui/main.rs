#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
//! Tauri shell: one window that hosts the static frontend in `ui/`.
//!
//! Commands are thin adapters over the `recipe_sync` library; the webview only renders and
//! sends validated requests. The HTTP client blocks, so the command that uses it runs on a
//! worker thread; no async runtime of our own is needed.

use anyhow::{Context, Result};
use recipe_sync::instagram_api::{list_recent_posts, Post, UreqClient};
use recipe_sync::instagram_url::{self, InstagramTarget};
use recipe_sync::token::{AccessToken, KeyringStore, TokenStore};

/// How many of the newest posts the main window asks for.
const RECENT_POSTS: u8 = 25;

#[derive(Default)]
struct AppState {
    store: KeyringStore,
    http: UreqClient,
}

#[tauri::command]
fn classify_instagram_url(url: &str) -> Result<InstagramTarget, String> {
    instagram_url::parse(url).map_err(|e| e.to_string())
}

#[tauri::command(async)]
fn has_token(state: tauri::State<'_, AppState>) -> Result<bool, String> {
    state
        .store
        .load()
        .map(|token| token.is_some())
        .map_err(|e| e.to_string())
}

#[tauri::command(async)]
fn save_token(token: &str, state: tauri::State<'_, AppState>) -> Result<(), String> {
    let token = AccessToken::new(token).map_err(|e| e.to_string())?;
    state.store.save(&token).map_err(|e| e.to_string())
}

#[tauri::command(async)]
fn clear_token(state: tauri::State<'_, AppState>) -> Result<(), String> {
    state.store.clear().map_err(|e| e.to_string())
}

#[tauri::command(async)]
fn list_posts(state: tauri::State<'_, AppState>) -> Result<Vec<Post>, String> {
    list_recent_posts(&state.store, &state.http, RECENT_POSTS).map_err(|e| e.to_string())
}

fn main() -> Result<()> {
    tauri::Builder::default()
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            classify_instagram_url,
            has_token,
            save_token,
            clear_token,
            list_posts
        ])
        .run(tauri::generate_context!())
        .context("failed to run the recipe-sync window")
}
