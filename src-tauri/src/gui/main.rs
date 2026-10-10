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
use recipe_sync::instagram_api::{self, Post, RenewalOutcome, TokenStatus};
use recipe_sync::instagram_url::{self, InstagramTarget};
use recipe_sync::publish::{publish_draft, DraftInput};
use recipe_sync::recipe_html::{prepare_draft_text, slugify, DraftText};
use recipe_sync::token::{AccessToken, KeyringStore, TokenStore};
use recipe_sync::wordpress::{
    self, CategoryItem, CreatedPost, CredentialsStore, KeyringCredentials, WpCredentials,
};
use serde::Serialize;
use std::time::{SystemTime, UNIX_EPOCH};

/// How many of the newest posts the main window asks for.
const RECENT_POSTS: u8 = 25;

/// The current time in seconds since 1970; a clock set before then counts as 0.
fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or_default()
}

#[derive(Default)]
struct AppState {
    store: KeyringStore,
    wordpress: KeyringCredentials,
    http: UreqClient,
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
    prepare_draft_text(caption)
}

#[tauri::command]
fn suggest_slug(title: &str) -> String {
    slugify(title)
}

#[tauri::command(async)]
fn token_status(state: tauri::State<'_, AppState>) -> Result<TokenStatus, String> {
    instagram_api::token_status(&state.store).map_err(|e| e.to_string())
}

#[tauri::command(async)]
fn save_token(token: &str, state: tauri::State<'_, AppState>) -> Result<(), String> {
    let token = AccessToken::new(token).map_err(|e| e.to_string())?;
    instagram_api::store_new_token(&state.store, &token, unix_now()).map_err(|e| e.to_string())
}

#[tauri::command(async)]
fn refresh_token(state: tauri::State<'_, AppState>) -> Result<TokenStatus, String> {
    instagram_api::refresh_token(&state.store, &state.http, unix_now()).map_err(|e| e.to_string())
}

#[tauri::command(async)]
fn renew_token_if_due(state: tauri::State<'_, AppState>) -> Result<RenewalOutcome, String> {
    instagram_api::renew_token_if_due(&state.store, &state.http, unix_now())
        .map_err(|e| e.to_string())
}

#[tauri::command(async)]
fn clear_token(state: tauri::State<'_, AppState>) -> Result<(), String> {
    state.store.clear().map_err(|e| e.to_string())
}

#[tauri::command(async)]
fn list_posts(state: tauri::State<'_, AppState>) -> Result<Vec<Post>, String> {
    instagram_api::list_recent_posts(&state.store, &state.http, RECENT_POSTS)
        .map_err(|e| e.to_string())
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
fn list_categories(state: tauri::State<'_, AppState>) -> Result<Vec<CategoryItem>, String> {
    wordpress::list_categories(&state.wordpress, &state.http).map_err(|e| e.to_string())
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
            suggest_slug,
            token_status,
            save_token,
            refresh_token,
            renew_token_if_due,
            clear_token,
            list_posts,
            get_wordpress_settings,
            save_wordpress_settings,
            clear_wordpress_settings,
            list_categories,
            create_wordpress_draft
        ])
        .run(tauri::generate_context!())
        .context("failed to run the recipe-sync window")
}
