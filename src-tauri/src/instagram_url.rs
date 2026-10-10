//! Classifies an Instagram URL as a profile, a post or a reel.
//!
//! Pure string work: no network and no URL crate. Anything that is not clearly one of the three
//! supported shapes is rejected with an error that names the input.

use serde::Serialize;
use thiserror::Error;

/// What an Instagram URL points at, with the identifier needed to fetch it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum InstagramTarget {
    Profile { username: String },
    Post { shortcode: String },
    Reel { shortcode: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum InstagramUrlError {
    #[error("the URL is empty")]
    Empty,
    #[error("`{0}` is not an instagram.com URL")]
    NotInstagram(String),
    #[error("`{0}` is not a profile, post or reel URL")]
    Unsupported(String),
}

/// Top-level paths that look like a username but are Instagram sections.
const RESERVED: &[&str] = &[
    "accounts", "direct", "explore", "p", "reel", "reels", "stories", "tv",
];

/// Parses `input` into an [`InstagramTarget`].
///
/// The scheme is optional, a trailing slash, query string or fragment is ignored, and the
/// `www.` and `m.` hosts are accepted.
///
/// ```
/// use recipe_sync::instagram_url::{parse, InstagramTarget};
///
/// let target = parse("https://www.instagram.com/reel/C8xYz/?igsh=abc").unwrap();
/// assert_eq!(target, InstagramTarget::Reel { shortcode: "C8xYz".into() });
/// ```
pub fn parse(input: &str) -> Result<InstagramTarget, InstagramUrlError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(InstagramUrlError::Empty);
    }

    let without_scheme = ["https://", "http://"]
        .iter()
        .find_map(|scheme| {
            let head = trimmed.get(..scheme.len())?;
            head.eq_ignore_ascii_case(scheme)
                .then(|| &trimmed[scheme.len()..])
        })
        .unwrap_or(trimmed);
    let without_extras = without_scheme
        .split(['?', '#'])
        .next()
        .unwrap_or(without_scheme);

    let (host, path) = without_extras
        .split_once('/')
        .unwrap_or((without_extras, ""));
    let host = host.to_ascii_lowercase();
    if !matches!(
        host.as_str(),
        "instagram.com" | "www.instagram.com" | "m.instagram.com"
    ) {
        return Err(InstagramUrlError::NotInstagram(trimmed.to_owned()));
    }

    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    let unsupported = || InstagramUrlError::Unsupported(trimmed.to_owned());

    match segments.as_slice() {
        ["p" | "tv", code] | [_, "p", code] if is_shortcode(code) => Ok(InstagramTarget::Post {
            shortcode: (*code).to_owned(),
        }),
        ["reel" | "reels", code] | [_, "reel", code] if is_shortcode(code) => {
            Ok(InstagramTarget::Reel {
                shortcode: (*code).to_owned(),
            })
        }
        [user] if is_username(user) && !RESERVED.contains(&user.to_ascii_lowercase().as_str()) => {
            Ok(InstagramTarget::Profile {
                username: (*user).to_owned(),
            })
        }
        _ => Err(unsupported()),
    }
}

fn is_shortcode(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn is_username(s: &str) -> bool {
    (1..=30).contains(&s.len())
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn post(code: &str) -> InstagramTarget {
        InstagramTarget::Post {
            shortcode: code.into(),
        }
    }

    fn reel(code: &str) -> InstagramTarget {
        InstagramTarget::Reel {
            shortcode: code.into(),
        }
    }

    fn profile(name: &str) -> InstagramTarget {
        InstagramTarget::Profile {
            username: name.into(),
        }
    }

    #[test]
    fn parses_a_post() {
        assert_eq!(
            parse("https://www.instagram.com/p/C8xYz_-1/"),
            Ok(post("C8xYz_-1"))
        );
    }

    #[test]
    fn parses_a_reel_and_the_reels_alias() {
        assert_eq!(
            parse("https://instagram.com/reel/AbC123"),
            Ok(reel("AbC123"))
        );
        assert_eq!(
            parse("https://instagram.com/reels/AbC123/"),
            Ok(reel("AbC123"))
        );
    }

    #[test]
    fn parses_post_and_reel_under_a_username() {
        assert_eq!(
            parse("https://www.instagram.com/some.user/p/AbC/"),
            Ok(post("AbC"))
        );
        assert_eq!(
            parse("https://www.instagram.com/some.user/reel/AbC/"),
            Ok(reel("AbC"))
        );
    }

    #[test]
    fn parses_a_profile() {
        assert_eq!(
            parse("https://www.instagram.com/beginveganbegun/"),
            Ok(profile("beginveganbegun"))
        );
        assert_eq!(parse("instagram.com/some.user"), Ok(profile("some.user")));
    }

    #[test]
    fn ignores_query_fragment_whitespace_scheme_and_host_case() {
        assert_eq!(
            parse("  HTTP://M.Instagram.com/reel/AbC/?igsh=xyz#top \n"),
            Ok(reel("AbC"))
        );
    }

    #[test]
    fn rejects_empty_input() {
        assert_eq!(parse("   "), Err(InstagramUrlError::Empty));
    }

    #[test]
    fn rejects_other_hosts_including_lookalikes() {
        for url in [
            "https://example.com/p/AbC",
            "https://instagram.com.evil.test/p/AbC",
            "not a url",
        ] {
            assert_eq!(parse(url), Err(InstagramUrlError::NotInstagram(url.into())));
        }
    }

    #[test]
    fn rejects_instagram_sections_that_are_not_profiles() {
        for url in [
            "https://www.instagram.com/",
            "https://www.instagram.com/explore/",
            "https://www.instagram.com/accounts/login/",
            "https://www.instagram.com/p/",
            "https://www.instagram.com/reel/",
            "https://www.instagram.com/stories/user/123/",
        ] {
            assert_eq!(parse(url), Err(InstagramUrlError::Unsupported(url.into())));
        }
    }

    #[test]
    fn rejects_malformed_shortcodes_and_usernames() {
        assert!(matches!(
            parse("https://www.instagram.com/p/bad code"),
            Err(InstagramUrlError::Unsupported(_))
        ));
        assert!(matches!(
            parse("https://www.instagram.com/user!name"),
            Err(InstagramUrlError::Unsupported(_))
        ));
        let long = format!("https://www.instagram.com/{}", "a".repeat(31));
        assert!(matches!(
            parse(&long),
            Err(InstagramUrlError::Unsupported(_))
        ));
    }
}
