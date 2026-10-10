#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
//! Tauri shell: one window that hosts the static frontend in `ui/`.
//!
//! Commands are thin adapters over the `instapress` library; the webview only renders and
//! sends validated requests. The HTTP client and the credential store block, so the commands
//! that use them are `#[tauri::command(async)]`: they run on Tauri's own runtime instead of the
//! UI thread and hold one of its workers while they wait, which the client timeouts bound. No
//! async runtime of our own is needed.

use anyhow::{Context, Result};
use instapress::browser::{self, BrowserError};
use instapress::http::UreqClient;
use instapress::instagram_api::{self, Post, RenewalOutcome, TokenStatus};
use instapress::publication_html::{prepare_draft_text, slugify, DraftText};
use instapress::publish::{publish_draft, DraftInput};
use instapress::token::{AccessToken, KeyringStore, TokenStore};
use instapress::wordpress::{
    self, CategoryItem, CreatedPost, CredentialsStore, KeyringCredentials, WpCredentials,
};
use serde::Serialize;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::menu::{
    CheckMenuItem, CheckMenuItemBuilder, MenuBuilder, MenuItemBuilder, SubmenuBuilder,
};
use tauri::{Emitter, Manager, Wry};

/// How many of the newest posts the main window asks for.
const RECENT_POSTS: u8 = 25;

const MENU_EXIT: &str = "exit";
const MENU_TWO_COLUMNS: &str = "two-columns";
const MENU_ABOUT: &str = "about";
/// The event the window listens to for the state of View > 2 columns.
const LAYOUT_EVENT: &str = "layout";
/// The event that asks the window to show its About dialog; the payload is the app version.
const ABOUT_EVENT: &str = "about";

/// The View > 2 columns item, kept so the window can set its check mark when it starts.
struct LayoutMenu(CheckMenuItem<Wry>);

/// Builds the menu bar: File > Exit, View > 2 columns and Help > About. On macOS the Edit menu is added too,
/// because the copy, paste and undo shortcuts of a webview work through it there.
fn build_menu(app: &tauri::App) -> tauri::Result<()> {
    let exit = MenuItemBuilder::with_id(MENU_EXIT, "Exit").build(app)?;
    let file = SubmenuBuilder::new(app, "File").item(&exit).build()?;
    let two_columns = CheckMenuItemBuilder::with_id(MENU_TWO_COLUMNS, "2 columns")
        .checked(false)
        .build(app)?;
    let view = SubmenuBuilder::new(app, "View")
        .item(&two_columns)
        .build()?;
    let about = MenuItemBuilder::with_id(MENU_ABOUT, "About Instapress").build(app)?;
    let help = SubmenuBuilder::new(app, "Help").item(&about).build()?;

    let menu = MenuBuilder::new(app).item(&file);
    #[cfg(target_os = "macos")]
    let menu = menu.item(
        &SubmenuBuilder::new(app, "Edit")
            .undo()
            .redo()
            .separator()
            .cut()
            .copy()
            .paste()
            .select_all()
            .build()?,
    );
    app.set_menu(menu.item(&view).item(&help).build()?)?;
    app.manage(LayoutMenu(two_columns));
    Ok(())
}

/// Acts on a menu click: Exit closes the app, About asks the window to show its dialog, and a click on 2 columns tells the window the new
/// state of the check mark, which the menu has already flipped.
fn on_menu_event(app: &tauri::AppHandle, event: tauri::menu::MenuEvent) {
    match event.id().as_ref() {
        MENU_EXIT => app.exit(0),
        MENU_TWO_COLUMNS => {
            if let Some(menu) = app.try_state::<LayoutMenu>() {
                // Nothing to do if either step fails: the window keeps the layout it has.
                if let Ok(checked) = menu.0.is_checked() {
                    let _ = app.emit(LAYOUT_EVENT, checked);
                }
            }
        }
        MENU_ABOUT => {
            let _ = app.emit(ABOUT_EVENT, env!("CARGO_PKG_VERSION"));
        }
        _ => {}
    }
}

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

/// Sets the check mark of View > 2 columns, which the window calls on start with the layout it
/// remembers.
#[tauri::command]
fn set_layout_menu(two_columns: bool, menu: tauri::State<'_, LayoutMenu>) -> Result<(), String> {
    menu.0.set_checked(two_columns).map_err(|e| e.to_string())
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

/// Opens the admin address of a created draft in the default browser. Only a page of the saved
/// site is opened; see `browser::check_admin_url`.
#[tauri::command(async)]
fn open_in_browser(url: &str, state: tauri::State<'_, AppState>) -> Result<(), String> {
    let saved = state.wordpress.load().map_err(|e| e.to_string())?;
    let credentials = saved
        .ok_or(BrowserError::NoSettings)
        .map_err(|e| e.to_string())?;
    browser::open_in_browser(url, credentials.site()).map_err(|e| e.to_string())
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
        .setup(|app| Ok(build_menu(app)?))
        .on_menu_event(on_menu_event)
        .invoke_handler(tauri::generate_handler![
            set_layout_menu,
            open_in_browser,
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
        .context("failed to run the instapress window")
}
