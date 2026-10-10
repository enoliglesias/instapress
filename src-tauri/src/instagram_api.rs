//! Reads the user's own recent posts through the Instagram API (Instagram Login).
//!
//! HTTP sits behind [`HttpGet`] so parsing and the flow are tested with a fake and never touch
//! the network. The access token travels in the `Authorization` header, never in the URL, so it
//! cannot end up in a logged or reported request line.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::token::{AccessToken, TokenError, TokenStore};

const MEDIA_URL: &str = "https://graph.instagram.com/me/media";
const MEDIA_FIELDS: &str = "id,caption,media_type,media_url,thumbnail_url,permalink,timestamp";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);

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
fn is_trusted_image_url(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else {
        return false;
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    if authority.contains(['@', ':']) {
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

/// What the HTTP layer got back, whatever the status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    pub body: String,
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

/// [`HttpGet`] over `ureq`. 4xx and 5xx are returned, not raised, so the error body can be read.
/// HTTPS only and no redirects: the token header must not follow one to another host.
pub struct UreqClient {
    agent: ureq::Agent,
}

impl Default for UreqClient {
    fn default() -> Self {
        let config = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .https_only(true)
            .max_redirects(0)
            .timeout_global(Some(REQUEST_TIMEOUT))
            .build();
        Self {
            agent: config.into(),
        }
    }
}

impl HttpGet for UreqClient {
    fn get(
        &self,
        url: &str,
        query: &[(&str, &str)],
        bearer_token: &str,
    ) -> Result<HttpResponse, ApiError> {
        let mut request = self
            .agent
            .get(url)
            .header("Authorization", format!("Bearer {bearer_token}"));
        for (key, value) in query {
            request = request.query(key, value);
        }
        let mut response = request.call().map_err(network_error)?;
        let status = response.status().as_u16();
        let body = response
            .body_mut()
            .read_to_string()
            .map_err(network_error)?;
        Ok(HttpResponse { status, body })
    }
}

/// Keeps only the kind of failure: a `ureq` error text could carry the request details.
fn network_error(error: ureq::Error) -> ApiError {
    let reason = match error {
        ureq::Error::Timeout(_) => "timed out",
        ureq::Error::HostNotFound | ureq::Error::ConnectionFailed => "could not connect",
        _ => "transport error",
    };
    ApiError::Network(reason.to_owned())
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

    #[test]
    fn transport_errors_are_reduced_to_a_fixed_phrase() {
        for (error, phrase) in [
            (ureq::Error::HostNotFound, "could not connect"),
            (ureq::Error::ConnectionFailed, "could not connect"),
            (ureq::Error::Timeout(ureq::Timeout::Global), "timed out"),
            (ureq::Error::TooManyRedirects, "transport error"),
        ] {
            match network_error(error) {
                ApiError::Network(text) => assert_eq!(text, phrase),
                other => panic!("expected a network error, got {other:?}"),
            }
        }
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
