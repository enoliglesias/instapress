//! Reads the user's own recent posts through the Instagram API (Instagram Login).
//!
//! HTTP sits behind [`HttpGet`] so parsing and the flow are tested with a fake and never touch
//! the network. The access token travels in the `Authorization` header, never in the URL, so it
//! cannot end up in a logged or reported request line.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::http::{HttpResponse, UreqClient};
use crate::token::{AccessToken, TokenError, TokenStore};

const MEDIA_URL: &str = "https://graph.instagram.com/me/media";
const REFRESH_URL: &str = "https://graph.instagram.com/refresh_access_token";
/// A token that was just generated in the Meta panel lasts about this long; the app assumes it
/// when the user pastes one, because Instagram does not say how old a pasted token is. A day is
/// taken off so a token is never believed to last longer than it does.
const ASSUMED_TOKEN_LIFETIME_SECS: u64 = 59 * 24 * 60 * 60;
/// A token is renewed when it has less than this left (or when nobody knows how long it has).
const RENEW_WITHIN_SECS: u64 = 30 * 24 * 60 * 60;
const MEDIA_FIELDS: &str = "id,caption,media_type,media_url,thumbnail_url,permalink,timestamp";

/// Why the recent posts could not be listed. No variant ever carries the access token.
#[derive(Debug, Error)]
pub enum ApiError {
    /// Nothing is saved in the credential store yet.
    #[error("no Instagram access token is saved yet")]
    MissingToken,
    /// The credential store failed.
    #[error(transparent)]
    Token(#[from] TokenError),
    /// The request did not complete; the text is a fixed phrase, never the transport error.
    #[error("the request to Instagram did not complete: {0}")]
    Network(String),
    /// Instagram answered with its error envelope, for example an expired token (code 190).
    #[error("Instagram rejected the request (code {code:?}): {message}")]
    Api { code: Option<i64>, message: String },
    /// A non-2xx status without an error envelope, including redirects, which are not followed.
    #[error("Instagram answered with HTTP status {0}")]
    Status(u16),
    /// A 2xx response whose body is not the expected JSON.
    #[error("Instagram sent a response that is not the expected JSON: {0}")]
    Malformed(String),
}

/// The kind of a post as Instagram reports it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MediaType {
    Image,
    Video,
    CarouselAlbum,
    /// Any type the API adds later, and a post that does not say its type.
    #[serde(other)]
    Unknown,
}

/// One item of `/me/media` as Instagram sends it. Only `id` is required.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub(crate) struct Media {
    id: String,
    caption: Option<String>,
    media_type: Option<MediaType>,
    media_url: Option<String>,
    thumbnail_url: Option<String>,
    permalink: Option<String>,
    timestamp: Option<String>,
}

/// A post as the frontend shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Post {
    pub id: String,
    /// The post text; `None` when the post has no caption.
    pub caption: Option<String>,
    pub media_type: MediaType,
    /// A picture to show: for a video its thumbnail, otherwise the media itself.
    pub preview_url: Option<String>,
    pub permalink: Option<String>,
    /// The raw ISO 8601 string from the API.
    pub timestamp: Option<String>,
}

/// Whether `url` is an HTTPS link to Instagram's image CDN, the only hosts the window's content
/// security policy lets an `<img>` load. Anything else from the API is dropped instead of being
/// handed to the frontend.
pub(crate) fn is_trusted_image_url(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else {
        return false;
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    // Letters, digits, dots and hyphens only: this rejects ports, user information, backslashes,
    // percent escapes, whitespace and control characters, which parsers read in different ways.
    let plain_host = authority
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-');
    // Every label must be present and may not start or end with a hyphen.
    let labels_ok = authority
        .split('.')
        .all(|label| !label.is_empty() && !label.starts_with('-') && !label.ends_with('-'));
    if !plain_host || !labels_ok {
        return false;
    }
    let host = authority.to_ascii_lowercase();
    ["cdninstagram.com", "fbcdn.net"]
        .iter()
        .any(|domain| host.strip_suffix(domain).is_some_and(|p| p.ends_with('.')))
}

impl From<Media> for Post {
    fn from(media: Media) -> Self {
        let media_type = media.media_type.unwrap_or(MediaType::Unknown);
        let thumbnail_url = media.thumbnail_url.filter(|u| is_trusted_image_url(u));
        let media_url = media.media_url.filter(|u| is_trusted_image_url(u));
        // For a video `media_url` is the video file, so its thumbnail is the picture to show.
        let preview_url = if media_type == MediaType::Video {
            thumbnail_url.or(media_url)
        } else {
            media_url.or(thumbnail_url)
        };
        Self {
            id: media.id,
            caption: media.caption,
            media_type,
            preview_url,
            permalink: media.permalink,
            timestamp: media.timestamp,
        }
    }
}

/// A `/me/media` body: a page of posts, or an error envelope.
#[derive(Deserialize)]
struct MediaResponse {
    #[serde(default)]
    data: Vec<Media>,
    error: Option<ErrorBody>,
}

#[derive(Deserialize)]
struct ErrorBody {
    message: String,
    code: Option<i64>,
}

/// Turns a `/me/media` response into posts. The error envelope wins over the status, and any
/// other non-2xx status is an error even when the body happens to be valid JSON, so a failure
/// is never mistaken for an empty account.
fn parse_media_page(status: u16, body: &str) -> Result<Vec<Media>, ApiError> {
    match serde_json::from_str::<MediaResponse>(body) {
        Ok(MediaResponse {
            error: Some(error), ..
        }) => Err(ApiError::Api {
            code: error.code,
            message: error.message,
        }),
        _ if !(200..300).contains(&status) => Err(ApiError::Status(status)),
        Ok(response) => Ok(response.data),
        Err(e) => Err(ApiError::Malformed(e.to_string())),
    }
}

/// The one HTTP operation the client needs: a GET with a bearer token. Implementations must not
/// put the token in the URL or in their errors.
pub trait HttpGet {
    fn get(
        &self,
        url: &str,
        query: &[(&str, &str)],
        bearer_token: &str,
    ) -> Result<HttpResponse, ApiError>;
}

impl HttpGet for UreqClient {
    fn get(
        &self,
        url: &str,
        query: &[(&str, &str)],
        bearer_token: &str,
    ) -> Result<HttpResponse, ApiError> {
        self.get_text(url, query, bearer_token)
            .map_err(|e| ApiError::Network(e.to_string()))
    }
}

/// Fetches the newest `limit` posts of the account the token belongs to.
pub(crate) fn fetch_recent_media<H: HttpGet>(
    http: &H,
    token: &AccessToken,
    limit: u8,
) -> Result<Vec<Media>, ApiError> {
    let limit = limit.to_string();
    let response = http.get(
        MEDIA_URL,
        &[("fields", MEDIA_FIELDS), ("limit", &limit)],
        token.expose(),
    )?;
    parse_media_page(response.status, &response.body)
}

/// Loads the saved token and returns the account's newest `limit` posts.
pub fn list_recent_posts<S: TokenStore, H: HttpGet>(
    store: &S,
    http: &H,
    limit: u8,
) -> Result<Vec<Post>, ApiError> {
    let token = store.load()?.ok_or(ApiError::MissingToken)?;
    Ok(fetch_recent_media(http, &token, limit)?
        .into_iter()
        .map(Post::from)
        .collect())
}

/// What the window shows about the saved token.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct TokenStatus {
    pub has_token: bool,
    /// When the token stops working, in seconds since 1970; `None` when nobody knows.
    pub expires_at: Option<u64>,
}

/// Saves a token the user has just pasted, assuming it is fresh (see
/// [`ASSUMED_TOKEN_LIFETIME_SECS`]).
pub fn store_new_token<S: TokenStore>(
    store: &S,
    token: &AccessToken,
    now: u64,
) -> Result<(), TokenError> {
    store.save(token)?;
    store.save_expiry(now.saturating_add(ASSUMED_TOKEN_LIFETIME_SECS))
}

/// Whether there is a saved token and when it expires.
pub fn token_status<S: TokenStore>(store: &S) -> Result<TokenStatus, TokenError> {
    Ok(match store.load()? {
        Some(_) => TokenStatus {
            has_token: true,
            expires_at: store.expires_at()?,
        },
        None => TokenStatus {
            has_token: false,
            expires_at: None,
        },
    })
}

/// Whether a token should be renewed now: when it has less than 30 days left, when it has
/// already expired, or when nobody knows. A token that has expired cannot be renewed, but trying
/// is what tells the user so.
pub fn renewal_due(expires_at: Option<u64>, now: u64) -> bool {
    expires_at.is_none_or(|expires_at| expires_at <= now.saturating_add(RENEW_WITHIN_SECS))
}

/// What Instagram answers when a token is renewed.
#[derive(Deserialize)]
struct RefreshResponse {
    access_token: Option<String>,
    expires_in: Option<u64>,
    error: Option<ErrorBody>,
}

fn parse_refresh(status: u16, body: &str) -> Result<(AccessToken, u64), ApiError> {
    match serde_json::from_str::<RefreshResponse>(body) {
        Ok(RefreshResponse {
            error: Some(error), ..
        }) => Err(ApiError::Api {
            code: error.code,
            message: error.message,
        }),
        _ if !(200..300).contains(&status) => Err(ApiError::Status(status)),
        Ok(RefreshResponse {
            access_token: Some(token),
            expires_in: Some(seconds),
            ..
        }) if seconds > 0 => Ok((AccessToken::new(&token)?, seconds)),
        Ok(_) => Err(ApiError::Malformed(
            "the answer has no new token or no lifetime".into(),
        )),
        Err(e) => Err(ApiError::Malformed(e.to_string())),
    }
}

/// Asks Instagram for a new 60-day token in exchange for the saved one, and saves it with its
/// expiry. Instagram only renews a token that is at least a day old and has not expired.
///
/// This endpoint is documented with the token as a query parameter, so here it goes in the
/// query as well as in the header; it never appears in an error.
pub fn refresh_token<S: TokenStore, H: HttpGet>(
    store: &S,
    http: &H,
    now: u64,
) -> Result<TokenStatus, ApiError> {
    let token = store.load()?.ok_or(ApiError::MissingToken)?;
    let response = http.get(
        REFRESH_URL,
        &[
            ("grant_type", "ig_refresh_token"),
            ("access_token", token.expose()),
        ],
        token.expose(),
    )?;
    let (renewed, seconds) = parse_refresh(response.status, &response.body)?;
    store.save(&renewed)?;
    let expires_at = now.saturating_add(seconds);
    store.save_expiry(expires_at)?;
    Ok(TokenStatus {
        has_token: true,
        expires_at: Some(expires_at),
    })
}

/// What happened when the app looked at the token on start.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct RenewalOutcome {
    pub renewed: bool,
    pub status: TokenStatus,
}

/// Renews the token if it is due (see [`renewal_due`]) and leaves it alone otherwise. Having no
/// token is not an error here.
pub fn renew_token_if_due<S: TokenStore, H: HttpGet>(
    store: &S,
    http: &H,
    now: u64,
) -> Result<RenewalOutcome, ApiError> {
    let status = token_status(store)?;
    if !status.has_token || !renewal_due(status.expires_at, now) {
        return Ok(RenewalOutcome {
            renewed: false,
            status,
        });
    }
    Ok(RenewalOutcome {
        renewed: true,
        status: refresh_token(store, http, now)?,
    })
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;
    use crate::token::tests::MemoryStore;

    const PAGE: &str = r#"{
      "data": [
        {"id":"1","caption":"Fabada vegana\n#receta","media_type":"IMAGE",
         "media_url":"https://scontent.cdninstagram.com/1.jpg","permalink":"https://www.instagram.com/p/AAA/",
         "timestamp":"2026-05-20T10:00:00+0000"},
        {"id":"2","media_type":"VIDEO","media_url":"https://scontent.cdninstagram.com/2.mp4",
         "thumbnail_url":"https://scontent.cdninstagram.com/2.jpg","permalink":"https://www.instagram.com/reel/BBB/"},
        {"id":"3","caption":"Album","media_type":"CAROUSEL_ALBUM","media_url":"https://scontent.cdninstagram.com/3.jpg"},
        {"id":"4","media_type":"SOMETHING_NEW"}
      ],
      "paging": {"cursors": {"before": "x", "after": "y"}}
    }"#;

    /// One recorded request: the URL, its query pairs and the bearer token.
    type Seen = (String, Vec<(String, String)>, String);

    struct FakeHttp {
        response: HttpResponse,
        seen: RefCell<Vec<Seen>>,
    }

    impl FakeHttp {
        fn ok(status: u16, body: &str) -> Self {
            Self {
                response: HttpResponse {
                    status,
                    body: body.to_owned(),
                },
                seen: RefCell::default(),
            }
        }
    }

    impl HttpGet for FakeHttp {
        fn get(
            &self,
            url: &str,
            query: &[(&str, &str)],
            bearer_token: &str,
        ) -> Result<HttpResponse, ApiError> {
            self.seen.borrow_mut().push((
                url.to_owned(),
                query
                    .iter()
                    .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                    .collect(),
                bearer_token.to_owned(),
            ));
            Ok(self.response.clone())
        }
    }

    fn token() -> AccessToken {
        AccessToken::new("IGAAtoken").unwrap()
    }

    #[test]
    fn parses_a_page_of_mixed_media() {
        let media = parse_media_page(200, PAGE).unwrap();
        assert_eq!(media.len(), 4);
        assert_eq!(media[0].caption.as_deref(), Some("Fabada vegana\n#receta"));
        assert_eq!(media[3].media_type, Some(MediaType::Unknown));
    }

    #[test]
    fn parses_an_empty_account() {
        assert_eq!(parse_media_page(200, r#"{"data":[]}"#).unwrap(), vec![]);
        assert_eq!(parse_media_page(200, "{}").unwrap(), vec![]);
    }

    #[test]
    fn turns_the_error_envelope_into_an_api_error() {
        let body =
            r#"{"error":{"message":"Session has expired","type":"OAuthException","code":190}}"#;
        match parse_media_page(400, body) {
            Err(ApiError::Api { code, message }) => {
                assert_eq!(code, Some(190));
                assert_eq!(message, "Session has expired");
            }
            other => panic!("expected an API error, got {other:?}"),
        }
    }

    #[test]
    fn rejects_a_body_that_is_not_json() {
        assert!(matches!(
            parse_media_page(200, "<html>nope</html>"),
            Err(ApiError::Malformed(_))
        ));
    }

    #[test]
    fn a_video_previews_with_its_thumbnail_and_the_rest_with_the_media() {
        let posts: Vec<Post> = parse_media_page(200, PAGE)
            .unwrap()
            .into_iter()
            .map(Post::from)
            .collect();
        assert_eq!(
            posts[0].preview_url.as_deref(),
            Some("https://scontent.cdninstagram.com/1.jpg")
        );
        assert_eq!(
            posts[1].preview_url.as_deref(),
            Some("https://scontent.cdninstagram.com/2.jpg")
        );
        assert_eq!(
            posts[2].preview_url.as_deref(),
            Some("https://scontent.cdninstagram.com/3.jpg")
        );
        assert_eq!(posts[3].preview_url, None);
        assert_eq!(posts[3].media_type, MediaType::Unknown);
        assert_eq!(posts[1].caption, None);
    }

    #[test]
    fn requests_the_media_endpoint_with_fields_and_limit() {
        let http = FakeHttp::ok(200, PAGE);
        fetch_recent_media(&http, &token(), 25).unwrap();
        let seen = http.seen.borrow();
        assert_eq!(seen[0].0, "https://graph.instagram.com/me/media");
        assert!(seen[0].1.contains(&("fields".into(), MEDIA_FIELDS.into())));
        assert!(seen[0].1.contains(&("limit".into(), "25".into())));
    }

    #[test]
    fn the_token_is_sent_as_the_bearer_and_never_in_the_url_or_query() {
        let http = FakeHttp::ok(200, PAGE);
        fetch_recent_media(&http, &token(), 25).unwrap();
        let seen = http.seen.borrow();
        assert_eq!(seen[0].2, "IGAAtoken");
        assert!(!seen[0].0.contains("IGAAtoken"));
        assert!(seen[0]
            .1
            .iter()
            .all(|(k, v)| k != "access_token" && !v.contains("IGAAtoken")));
    }

    #[test]
    fn only_https_links_to_the_instagram_image_cdn_are_trusted() {
        for url in [
            "https://scontent.cdninstagram.com/v/1.jpg?x=1",
            "https://scontent-mad1-1.cdninstagram.com/v/1.jpg",
            "https://instagram.fabc1-1.fna.fbcdn.net/v/1.jpg",
            "https://SCONTENT.CDNINSTAGRAM.COM/v/1.jpg",
        ] {
            assert!(is_trusted_image_url(url), "{url}");
        }
        for url in [
            "http://scontent.cdninstagram.com/1.jpg",
            "data:image/png;base64,AAAA",
            "https://evil.test/1.jpg",
            "https://evilcdninstagram.com/1.jpg",
            "https://cdninstagram.com.evil.test/1.jpg",
            "https://user@scontent.cdninstagram.com/1.jpg",
            "https://scontent.cdninstagram.com:8443/1.jpg",
            "https://evil.test/?u=https://scontent.cdninstagram.com/1.jpg",
            "https://evil.test\\.cdninstagram.com/x",
            "https://evil.test%2f.cdninstagram.com/x",
            "https://evil.test%00.cdninstagram.com/x",
            "https://evil.test .cdninstagram.com/x",
            "https://evil.test\t.cdninstagram.com/x",
            "https://evil.test\n.cdninstagram.com/x",
            "https://scontent.cdninstagram.com\\@evil.test/x",
            "https://.cdninstagram.com/x",
            "https://cdninstagram.com/x",
            "https://",
            "https:///x",
            "javascript:alert(1)",
            "",
        ] {
            assert!(!is_trusted_image_url(url), "{url}");
        }
    }

    #[test]
    fn an_untrusted_preview_is_dropped_and_the_other_candidate_is_used() {
        let media = parse_media_page(
            200,
            r#"{"data":[
              {"id":"1","media_type":"IMAGE","media_url":"https://evil.test/a.jpg",
               "thumbnail_url":"https://scontent.cdninstagram.com/t.jpg"},
              {"id":"2","media_type":"IMAGE","media_url":"data:image/png;base64,AAAA"}
            ]}"#,
        )
        .unwrap();
        let posts: Vec<Post> = media.into_iter().map(Post::from).collect();
        assert_eq!(
            posts[0].preview_url.as_deref(),
            Some("https://scontent.cdninstagram.com/t.jpg")
        );
        assert_eq!(posts[1].preview_url, None);
    }

    #[test]
    fn a_non_success_status_without_an_error_body_is_reported_by_status() {
        let http = FakeHttp::ok(502, "<html>bad gateway</html>");
        assert!(matches!(
            fetch_recent_media(&http, &token(), 5),
            Err(ApiError::Status(502))
        ));
    }

    #[test]
    fn an_error_body_wins_over_the_status() {
        let http = FakeHttp::ok(400, r#"{"error":{"message":"Invalid token","code":190}}"#);
        assert!(matches!(
            fetch_recent_media(&http, &token(), 5),
            Err(ApiError::Api {
                code: Some(190),
                ..
            })
        ));
    }

    #[test]
    fn listing_without_a_saved_token_asks_for_one_and_does_not_call_out() {
        let http = FakeHttp::ok(200, PAGE);
        let result = list_recent_posts(&MemoryStore::default(), &http, 25);
        assert!(matches!(result, Err(ApiError::MissingToken)));
        assert!(http.seen.borrow().is_empty());
    }

    #[test]
    fn lists_posts_with_the_saved_token() {
        let store = MemoryStore::default();
        store.save(&token()).unwrap();
        let posts = list_recent_posts(&store, &FakeHttp::ok(200, PAGE), 25).unwrap();
        assert_eq!(posts.len(), 4);
        assert_eq!(posts[0].id, "1");
    }

    #[test]
    fn a_failing_status_is_never_taken_for_an_empty_account() {
        for body in ["{}", r#"{"data":[]}"#, r#"{"message":"slow down"}"#] {
            let http = FakeHttp::ok(500, body);
            assert!(matches!(
                fetch_recent_media(&http, &token(), 5),
                Err(ApiError::Status(500))
            ));
        }
    }

    #[test]
    fn a_redirect_is_an_error_not_something_to_follow() {
        let http = FakeHttp::ok(302, "");
        assert!(matches!(
            fetch_recent_media(&http, &token(), 5),
            Err(ApiError::Status(302))
        ));
    }

    const NOW: u64 = 1_800_000_000;
    const DAY: u64 = 24 * 60 * 60;
    const REFRESHED: &str =
        r#"{"access_token":"IGAAnew","token_type":"bearer","expires_in":5183944}"#;

    fn store_with(token: &str, expires_at: Option<u64>) -> MemoryStore {
        let store = MemoryStore::default();
        store.save(&AccessToken::new(token).unwrap()).unwrap();
        if let Some(at) = expires_at {
            store.save_expiry(at).unwrap();
        }
        store
    }

    #[test]
    fn a_pasted_token_is_assumed_fresh_and_a_day_short_of_sixty() {
        let store = MemoryStore::default();
        store_new_token(&store, &token(), NOW).unwrap();
        assert_eq!(store.load().unwrap(), Some(token()));
        assert_eq!(store.expires_at().unwrap(), Some(NOW + 59 * DAY));
    }

    #[test]
    fn the_status_says_whether_there_is_a_token_and_when_it_expires() {
        let empty = MemoryStore::default();
        assert_eq!(
            token_status(&empty),
            Ok(TokenStatus {
                has_token: false,
                expires_at: None
            })
        );
        let with = store_with("IGAAtoken", Some(NOW + 40 * DAY));
        assert_eq!(
            token_status(&with),
            Ok(TokenStatus {
                has_token: true,
                expires_at: Some(NOW + 40 * DAY)
            })
        );
        let unknown = store_with("IGAAtoken", None);
        assert_eq!(token_status(&unknown).unwrap().expires_at, None);
    }

    #[test]
    fn a_token_is_due_for_renewal_with_under_thirty_days_left_or_when_unknown() {
        assert!(renewal_due(None, NOW));
        assert!(renewal_due(Some(NOW - DAY), NOW), "already expired");
        assert!(renewal_due(Some(NOW), NOW));
        assert!(
            renewal_due(Some(NOW + 30 * DAY), NOW),
            "exactly thirty days"
        );
        assert!(!renewal_due(Some(NOW + 30 * DAY + 1), NOW));
        assert!(!renewal_due(Some(NOW + 59 * DAY), NOW));
    }

    #[test]
    fn renewing_saves_the_new_token_and_its_expiry() {
        let store = store_with("IGAAold", Some(NOW + DAY));
        let http = FakeHttp::ok(200, REFRESHED);
        let status = refresh_token(&store, &http, NOW).unwrap();
        assert_eq!(
            status,
            TokenStatus {
                has_token: true,
                expires_at: Some(NOW + 5_183_944)
            }
        );
        assert_eq!(
            store.load().unwrap(),
            Some(AccessToken::new("IGAAnew").unwrap())
        );
        assert_eq!(store.expires_at().unwrap(), Some(NOW + 5_183_944));
    }

    #[test]
    fn renewing_asks_the_refresh_endpoint_with_the_saved_token() {
        let store = store_with("IGAAold", None);
        let http = FakeHttp::ok(200, REFRESHED);
        refresh_token(&store, &http, NOW).unwrap();
        let seen = http.seen.borrow();
        assert_eq!(
            seen[0].0,
            "https://graph.instagram.com/refresh_access_token"
        );
        assert!(seen[0]
            .1
            .contains(&("grant_type".into(), "ig_refresh_token".into())));
        assert!(seen[0]
            .1
            .contains(&("access_token".into(), "IGAAold".into())));
        assert_eq!(seen[0].2, "IGAAold");
    }

    #[test]
    fn a_renewal_that_fails_leaves_the_saved_token_alone() {
        for (status, body) in [
            (
                400,
                r#"{"error":{"message":"Cannot refresh a token less than 24 hours old","code":10}}"#,
            ),
            (
                400,
                r#"{"error":{"message":"Session has expired","code":190}}"#,
            ),
            (500, "<html>down</html>"),
            (200, "<html>nope</html>"),
            (200, r#"{"access_token":"IGAAnew"}"#),
            (200, r#"{"expires_in":100}"#),
            (200, r#"{"access_token":"IGAAnew","expires_in":0}"#),
            (200, r#"{"access_token":"has space","expires_in":100}"#),
        ] {
            let store = store_with("IGAAold", Some(NOW + DAY));
            let result = refresh_token(&store, &FakeHttp::ok(status, body), NOW);
            assert!(result.is_err(), "{status} {body}");
            assert_eq!(
                store.load().unwrap(),
                Some(AccessToken::new("IGAAold").unwrap())
            );
            assert_eq!(store.expires_at().unwrap(), Some(NOW + DAY));
        }
    }

    #[test]
    fn a_renewal_error_carries_instagrams_message_and_never_the_token() {
        let store = store_with("IGAAold", None);
        let http = FakeHttp::ok(
            400,
            r#"{"error":{"message":"Cannot refresh a token less than 24 hours old","code":10}}"#,
        );
        match refresh_token(&store, &http, NOW) {
            Err(ApiError::Api { code, message }) => {
                assert_eq!(code, Some(10));
                assert!(message.contains("24 hours"));
                assert!(!message.contains("IGAAold"));
            }
            other => panic!("expected an API error, got {other:?}"),
        }
    }

    #[test]
    fn renewing_without_a_token_asks_for_one_and_does_not_call_out() {
        let http = FakeHttp::ok(200, REFRESHED);
        assert!(matches!(
            refresh_token(&MemoryStore::default(), &http, NOW),
            Err(ApiError::MissingToken)
        ));
        assert!(http.seen.borrow().is_empty());
    }

    #[test]
    fn a_token_that_is_not_due_is_left_alone_without_a_request() {
        let store = store_with("IGAAold", Some(NOW + 45 * DAY));
        let http = FakeHttp::ok(200, REFRESHED);
        let outcome = renew_token_if_due(&store, &http, NOW).unwrap();
        assert!(!outcome.renewed);
        assert_eq!(outcome.status.expires_at, Some(NOW + 45 * DAY));
        assert!(http.seen.borrow().is_empty());
    }

    #[test]
    fn a_token_that_is_due_is_renewed_on_start() {
        for expires_at in [None, Some(NOW + 10 * DAY), Some(NOW - DAY)] {
            let store = store_with("IGAAold", expires_at);
            let http = FakeHttp::ok(200, REFRESHED);
            let outcome = renew_token_if_due(&store, &http, NOW).unwrap();
            assert!(outcome.renewed, "{expires_at:?}");
            assert_eq!(outcome.status.expires_at, Some(NOW + 5_183_944));
            assert_eq!(http.seen.borrow().len(), 1);
        }
    }

    #[test]
    fn with_no_token_there_is_nothing_to_renew() {
        let http = FakeHttp::ok(200, REFRESHED);
        let outcome = renew_token_if_due(&MemoryStore::default(), &http, NOW).unwrap();
        assert!(!outcome.renewed);
        assert!(!outcome.status.has_token);
        assert!(http.seen.borrow().is_empty());
    }

    #[test]
    fn a_failed_renewal_on_start_is_reported_and_keeps_the_old_token() {
        let store = store_with("IGAAold", None);
        let http = FakeHttp::ok(
            400,
            r#"{"error":{"message":"Cannot refresh a token less than 24 hours old","code":10}}"#,
        );
        assert!(renew_token_if_due(&store, &http, NOW).is_err());
        assert_eq!(
            store.load().unwrap(),
            Some(AccessToken::new("IGAAold").unwrap())
        );
    }

    #[test]
    fn no_error_display_or_debug_contains_the_token() {
        let token = token();
        let errors = [
            ApiError::MissingToken,
            ApiError::Network("could not connect".into()),
            ApiError::Status(500),
            ApiError::Malformed("expected value at line 1".into()),
        ];
        for error in errors {
            assert!(!error.to_string().contains(token.expose()));
            assert!(!format!("{error:?}").contains(token.expose()));
        }
    }
}
