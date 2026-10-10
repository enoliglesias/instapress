//! Opens the address of a draft in the user's browser, so there is no copying and pasting.
//!
//! The window never opens an address of its own: the backend only agrees to open a page of the
//! admin area of the WordPress site that is saved in the settings.

use crate::wordpress::SiteUrl;
use std::process::Command;
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum BrowserError {
    #[error("no WordPress settings are saved, so there is no site to open")]
    NoSettings,
    #[error("refusing to open {0}: it is not a page of the admin area of the saved site")]
    NotOnSite(String),
    #[error("could not start the browser: {0}")]
    Launch(String),
}

/// Accepts `url` only when it is under `{site}/wp-admin/` and holds no whitespace or control
/// character, so it reaches the system opener as a single plain address.
pub fn check_admin_url(url: &str, site: &SiteUrl) -> Result<(), BrowserError> {
    let prefix = format!("{}/wp-admin/", site.as_str());
    let plain = !url.chars().any(|c| c.is_whitespace() || c.is_control());
    if plain && url.starts_with(&prefix) && url.len() > prefix.len() {
        Ok(())
    } else {
        Err(BrowserError::NotOnSite(url.to_owned()))
    }
}

/// Hands `url` to the system's default browser. The address is a separate argument, never part
/// of a shell command line.
pub fn open_in_browser(url: &str, site: &SiteUrl) -> Result<(), BrowserError> {
    check_admin_url(url, site)?;
    let mut command = if cfg!(target_os = "windows") {
        let mut command = Command::new("rundll32");
        command.arg("url.dll,FileProtocolHandler");
        command
    } else if cfg!(target_os = "macos") {
        Command::new("open")
    } else {
        Command::new("xdg-open")
    };
    command
        .arg(url)
        .spawn()
        .map(|_| ())
        .map_err(|e| BrowserError::Launch(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn site() -> SiteUrl {
        SiteUrl::parse("https://example.test/blog").unwrap()
    }

    #[test]
    fn an_edit_page_of_the_site_is_accepted() {
        let url = "https://example.test/blog/wp-admin/post.php?post=77&action=edit";
        assert_eq!(check_admin_url(url, &site()), Ok(()));
    }

    #[test]
    fn another_host_is_refused() {
        let url = "https://evil.test/blog/wp-admin/post.php?post=77&action=edit";
        assert_eq!(
            check_admin_url(url, &site()),
            Err(BrowserError::NotOnSite(url.to_owned()))
        );
    }

    #[test]
    fn a_lookalike_prefix_is_refused() {
        let url = "https://example.test/blog.evil.test/wp-admin/post.php";
        assert!(check_admin_url(url, &site()).is_err());
        let url = "https://example.test/blog/wp-admin-evil/post.php";
        assert!(check_admin_url(url, &site()).is_err());
    }

    #[test]
    fn a_page_outside_the_admin_area_is_refused() {
        assert!(check_admin_url("https://example.test/blog/", &site()).is_err());
        assert!(check_admin_url("https://example.test/blog/wp-admin/", &site()).is_err());
    }

    #[test]
    fn other_schemes_are_refused() {
        assert!(check_admin_url("file:///c:/windows/system32/calc.exe", &site()).is_err());
        assert!(check_admin_url("http://example.test/blog/wp-admin/post.php", &site()).is_err());
    }

    #[test]
    fn whitespace_and_control_characters_are_refused() {
        for url in [
            "https://example.test/blog/wp-admin/post.php a",
            "https://example.test/blog/wp-admin/post.php\n",
            "https://example.test/blog/wp-admin/post.php\u{0}",
        ] {
            assert!(check_admin_url(url, &site()).is_err(), "{url:?}");
        }
    }

    #[test]
    fn open_checks_the_address_before_starting_anything() {
        assert_eq!(
            open_in_browser("https://evil.test/", &site()),
            Err(BrowserError::NotOnSite("https://evil.test/".to_owned()))
        );
    }
}
