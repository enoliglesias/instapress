#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
//! Tauri shell: one window that hosts the static frontend in `ui/`.
//!
//! Commands are thin adapters over the `recipe_sync` library; the webview only renders and
//! sends validated requests. The HTTP client and the credential store block, so the commands
//! that use them are `#[tauri::command(async)]`: they run on Tauri's own runtime instead of the
//! UI thread and hold one of its workers while they wait, which the client timeouts bound. No
//! async runtime of our own is needed.

use anyhow::{Context, Result};
use recipe_sync::http::UreqClient;
use recipe_sync::instagram_api::{list_recent_posts, Post};
use recipe_sync::instagram_url::{self, InstagramTarget};
use recipe_sync::publish::{publish_draft, DraftInput};
use recipe_sync::recipe_html::{caption_to_html, suggest_title};
use recipe_sync::token::{AccessToken, KeyringStore, TokenStore};
use recipe_sync::wordpress::{CreatedPost, CredentialsStore, KeyringCredentials, WpCredentials};
use serde::Serialize;

/// How many of the newest posts the main window asks for.
const RECENT_POSTS: u8 = 25;

#[derive(Default)]
struct AppState {
    store: KeyringStore,
    wordpress: KeyringCredentials,
    http: UreqClient,
}

/// The editable starting point of a draft.
#[derive(Serialize)]
struct DraftText {
    title: String,
    html: String,
}

/// The saved WordPress settings as the window may see them: never the password.
#[derive(Serialize)]
struct WordPressSettings {
    site: Option<String>,
    username: Option<String>,
    has_password: bool,
}

#[tauri::command]
fn classify_instagram_url(url: &str) -> Result<InstagramTarget, String> {
    instagram_url::parse(url).map_err(|e| e.to_string())
}

#[tauri::command]
fn prepare_draft(caption: &str) -> DraftText {
    DraftText {
        title: suggest_title(caption),
        html: caption_to_html(caption),
    }
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

#[tauri::command(async)]
fn get_wordpress_settings(state: tauri::State<'_, AppState>) -> Result<WordPressSettings, String> {
    let saved = state.wordpress.load().map_err(|e| e.to_string())?;
    Ok(WordPressSettings {
        site: saved.as_ref().map(|c| c.site().as_str().to_owned()),
        username: saved.as_ref().map(|c| c.username().to_owned()),
        has_password: saved.is_some(),
    })
}

#[tauri::command(async)]
fn save_wordpress_settings(
    site: &str,
    username: &str,
    app_password: &str,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let credentials =
        WpCredentials::new(site, username, app_password).map_err(|e| e.to_string())?;
    state
        .wordpress
        .save(&credentials)
        .map_err(|e| e.to_string())
}

#[tauri::command(async)]
fn clear_wordpress_settings(state: tauri::State<'_, AppState>) -> Result<(), String> {
    state.wordpress.clear().map_err(|e| e.to_string())
}

#[tauri::command(async)]
fn create_wordpress_draft(
    draft: DraftInput,
    state: tauri::State<'_, AppState>,
) -> Result<CreatedPost, String> {
    publish_draft(&state.wordpress, &state.http, &state.http, draft).map_err(|e| e.to_string())
}

fn main() -> Result<()> {
    tauri::Builder::default()
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            classify_instagram_url,
            prepare_draft,
            has_token,
            save_token,
            clear_token,
            list_posts,
            get_wordpress_settings,
            save_wordpress_settings,
            clear_wordpress_settings,
            create_wordpress_draft
        ])
        .run(tauri::generate_context!())
        .context("failed to run the recipe-sync window")
}
