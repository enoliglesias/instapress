//! Creating a post on the user's WordPress site through its REST API.
//!
//! The site address, the user name and an Application Password are kept together as one secret
//! in the OS credential store. Requests use HTTP Basic authentication over HTTPS only, behind
//! [`WpHttp`] so everything here is tested with a fake and never touches the network.

use std::fmt;

use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::http::{HttpResponse, UreqClient};
use crate::recipe_html::RecipeDraft;
use crate::token::{OsSecret, TokenError};

const WORDPRESS_SECRET: OsSecret = OsSecret::new("wordpress");

/// Why the settings were rejected.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SettingsError {
    #[error("the site address is empty")]
    EmptySite,
    #[error("`{0}` is not a valid https:// site address")]
    InvalidSite(String),
    #[error("the user name is empty")]
    EmptyUsername,
    #[error("the user name cannot contain a colon")]
    UsernameHasColon,
    #[error("the application password is empty")]
    EmptyPassword,
    /// A value too long to be real, which the OS credential store would also refuse to keep.
    #[error("the {field} is longer than {max} characters")]
    TooLong { field: &'static str, max: usize },
}

const MAX_SITE_CHARS: usize = 200;
const MAX_USERNAME_CHARS: usize = 100;
const MAX_PASSWORD_CHARS: usize = 200;

/// Why a post could not be created.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum WordPressError {
    /// No WordPress settings are saved yet.
    #[error("no WordPress settings are saved yet")]
    NoSettings,
    #[error(transparent)]
    Settings(#[from] SettingsError),
    /// The OS credential store failed.
    #[error(transparent)]
    Store(#[from] TokenError),
    /// The request did not complete; a fixed phrase, never the transport error.
    #[error("the request to the WordPress site did not complete: {0}")]
    Network(String),
    /// WordPress answered with its error body, for example `rest_cannot_create`.
    #[error("WordPress rejected the request ({code}): {message}")]
    Api { code: String, message: String },
    /// The site redirected, which is not followed so the credentials stay with this address.
    #[error("the site answered with a redirect (HTTP {0}); enter its final address")]
    Redirect(u16),
    /// Any other non-2xx status without a readable error body.
    #[error("the site answered with HTTP status {0}")]
    Status(u16),
    /// A 2xx answer that is not the expected JSON.
    #[error("the site sent an answer that is not the expected JSON: {0}")]
    Malformed(String),
}

/// The address of a WordPress site: `https://`, a host, an optional port and an optional folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SiteUrl(String);

impl SiteUrl {
    /// Normalizes the address: lower-case host and no trailing slash. A query, a fragment, user
    /// information, a scheme other than `https`, a port outside 1-65535 and a folder with
    /// anything but letters, digits and `-_.~` (or a `.`/`..` segment) are rejected. Names with
    /// non-ASCII letters must be given in their punycode form.
    pub fn parse(raw: &str) -> Result<Self, SettingsError> {
        let raw = raw.trim();
        if raw.is_empty() {
            return Err(SettingsError::EmptySite);
        }
        if raw.chars().count() > MAX_SITE_CHARS {
            return Err(SettingsError::TooLong {
                field: "site address",
                max: MAX_SITE_CHARS,
            });
        }
        let invalid = || SettingsError::InvalidSite(raw.to_owned());
        let rest = match raw.get(..8) {
            Some(scheme) if scheme.eq_ignore_ascii_case("https://") => &raw[8..],
            _ => return Err(invalid()),
        };
        if rest.contains(['?', '#', '@', '\\']) || rest.chars().any(char::is_whitespace) {
            return Err(invalid());
        }
        let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
        let (host, port) = match authority.split_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (authority, None),
        };
        let host_ok = !host.is_empty()
            && !host.starts_with(['.', '-'])
            && !host.ends_with(['.', '-'])
            && host
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-');
        let port_ok = port.is_none_or(|p| {
            p.chars().all(|c| c.is_ascii_digit()) && p.parse::<u16>().is_ok_and(|n| n != 0)
        });
        let path = path.trim_matches('/');
        let path_ok = path.split('/').all(|segment| {
            segment != ".."
                && segment != "."
                && segment
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '~'))
        });
        if !host_ok || !port_ok || !path_ok {
            return Err(invalid());
        }
        let mut site = format!("https://{}", authority.to_ascii_lowercase());
        if !path.is_empty() {
            site.push('/');
            site.push_str(path);
        }
        Ok(Self(site))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A WordPress Application Password. Never printed.
#[derive(Clone, PartialEq, Eq)]
pub struct AppPassword(String);

impl AppPassword {
    /// WordPress shows the password in groups of four; the spaces are not part of it.
    pub fn new(raw: &str) -> Result<Self, SettingsError> {
        let password: String = raw.chars().filter(|c| !c.is_whitespace()).collect();
        if password.is_empty() {
            return Err(SettingsError::EmptyPassword);
        }
        if password.chars().count() > MAX_PASSWORD_CHARS {
            return Err(SettingsError::TooLong {
                field: "application password",
                max: MAX_PASSWORD_CHARS,
            });
        }
        Ok(Self(password))
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for AppPassword {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AppPassword(<redacted>)")
    }
}

/// Everything needed to talk to the site.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WpCredentials {
    site: SiteUrl,
    username: String,
    password: AppPassword,
}

impl WpCredentials {
    pub fn new(site: &str, username: &str, password: &str) -> Result<Self, SettingsError> {
        let site = SiteUrl::parse(site)?;
        let username = username.trim();
        if username.is_empty() {
            return Err(SettingsError::EmptyUsername);
        }
        if username.contains(':') {
            return Err(SettingsError::UsernameHasColon);
        }
        if username.chars().count() > MAX_USERNAME_CHARS {
            return Err(SettingsError::TooLong {
                field: "user name",
                max: MAX_USERNAME_CHARS,
            });
        }
        Ok(Self {
            site,
            username: username.to_owned(),
            password: AppPassword::new(password)?,
        })
    }

    pub fn site(&self) -> &SiteUrl {
        &self.site
    }

    pub fn username(&self) -> &str {
        &self.username
    }

    fn authorization(&self) -> String {
        let pair = format!("{}:{}", self.username, self.password.expose());
        format!("Basic {}", STANDARD.encode(pair))
    }
}

/// Persistent storage for the WordPress settings.
pub trait CredentialsStore {
    /// The saved settings, or `None` when there are none.
    fn load(&self) -> Result<Option<WpCredentials>, WordPressError>;
    /// Replaces any saved settings.
    fn save(&self, credentials: &WpCredentials) -> Result<(), WordPressError>;
    /// Removing settings that are not there is not an error.
    fn clear(&self) -> Result<(), WordPressError>;
}

/// The shape of the secret kept in the credential store.
#[derive(Deserialize)]
struct StoredCredentials {
    site: String,
    username: String,
    app_password: String,
}

/// [`CredentialsStore`] backed by the OS credential store, holding one JSON secret.
#[derive(Debug, Default)]
pub struct KeyringCredentials;

impl CredentialsStore for KeyringCredentials {
    fn load(&self) -> Result<Option<WpCredentials>, WordPressError> {
        // A stored value that no longer validates counts as no settings: the user enters them
        // again and the save overwrites the bad entry.
        Ok(WORDPRESS_SECRET
            .load("read the WordPress settings")?
            .and_then(|raw| serde_json::from_str::<StoredCredentials>(&raw).ok())
            .and_then(|stored| {
                WpCredentials::new(&stored.site, &stored.username, &stored.app_password).ok()
            }))
    }

    fn save(&self, credentials: &WpCredentials) -> Result<(), WordPressError> {
        let json = serde_json::json!({
            "site": credentials.site.as_str(),
            "username": credentials.username,
            "app_password": credentials.password.expose(),
        })
        .to_string();
        Ok(WORDPRESS_SECRET.save("save the WordPress settings", &json)?)
    }

    fn clear(&self) -> Result<(), WordPressError> {
        Ok(WORDPRESS_SECRET.clear("remove the WordPress settings")?)
    }
}

/// The one HTTP operation the client needs: a `POST` of raw bytes with headers. Implementations
/// must not put the headers in their errors.
pub trait WpHttp {
    fn post(
        &self,
        url: &str,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> Result<HttpResponse, WordPressError>;
}

impl WpHttp for UreqClient {
    fn post(
        &self,
        url: &str,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> Result<HttpResponse, WordPressError> {
        UreqClient::post(self, url, headers, body)
            .map_err(|e| WordPressError::Network(e.to_string()))
    }
}

/// The post that was created.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CreatedPost {
    pub id: u64,
    /// The address WordPress reports for the post; for a draft it is a preview-style link.
    pub link: Option<String>,
    /// Where the user opens the draft in the WordPress editor.
    pub edit_url: String,
}

#[derive(Deserialize)]
struct WpErrorBody {
    code: String,
    message: String,
}

#[derive(Deserialize)]
struct WpCreated {
    id: u64,
    link: Option<String>,
}

/// Returns the body of a 2xx answer and turns anything else into the right error.
fn success_body(response: HttpResponse) -> Result<String, WordPressError> {
    match response.status {
        200..=299 => Ok(response.body),
        300..=399 => Err(WordPressError::Redirect(response.status)),
        status => Err(serde_json::from_str::<WpErrorBody>(&response.body).map_or(
            WordPressError::Status(status),
            |e| WordPressError::Api {
                code: e.code,
                message: e.message,
            },
        )),
    }
}

fn created(response: HttpResponse) -> Result<WpCreated, WordPressError> {
    serde_json::from_str(&success_body(response)?)
        .map_err(|e| WordPressError::Malformed(e.to_string()))
}

/// Creates the draft on the site, uploading its image first when there is one. The post is
/// always created with the `draft` status so the user reviews it in WordPress. If the image
/// upload works and the post does not, the image stays in the media library.
pub fn create_draft<H: WpHttp>(
    http: &H,
    credentials: &WpCredentials,
    draft: &RecipeDraft,
) -> Result<CreatedPost, WordPressError> {
    let authorization = credentials.authorization();
    let site = credentials.site.as_str();

    let featured_media = match draft.image() {
        Some(image) => {
            let disposition = format!("attachment; filename=\"{}\"", image.filename());
            let response = http.post(
                &format!("{site}/wp-json/wp/v2/media"),
                &[
                    ("Authorization", &authorization),
                    ("Content-Type", image.mime()),
                    ("Content-Disposition", &disposition),
                ],
                image.bytes(),
            )?;
            Some(created(response)?.id)
        }
        None => None,
    };

    let mut body = serde_json::json!({
        "title": draft.title(),
        "content": draft.content(),
        "status": "draft",
    });
    if let Some(id) = featured_media {
        body["featured_media"] = id.into();
    }
    let response = http.post(
        &format!("{site}/wp-json/wp/v2/posts"),
        &[
            ("Authorization", &authorization),
            ("Content-Type", "application/json"),
        ],
        body.to_string().as_bytes(),
    )?;
    let post = created(response)?;
    Ok(CreatedPost {
        edit_url: format!("{site}/wp-admin/post.php?post={}&action=edit", post.id),
        id: post.id,
        link: post.link,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use std::cell::RefCell;
    use std::collections::VecDeque;

    use super::*;
    use crate::image::Image;

    const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 1, 2, 3];
    const PASSWORD: &str = "abcd efgh ijkl mnop";

    /// One recorded request: URL, headers and body.
    pub(crate) type Sent = (String, Vec<(String, String)>, Vec<u8>);

    /// A fake site that answers with the queued responses in order and records what it got.
    pub(crate) struct FakeSite {
        pub(crate) answers: RefCell<VecDeque<Result<HttpResponse, WordPressError>>>,
        pub(crate) sent: RefCell<Vec<Sent>>,
    }

    impl FakeSite {
        pub(crate) fn new(answers: Vec<Result<HttpResponse, WordPressError>>) -> Self {
            Self {
                answers: RefCell::new(answers.into()),
                sent: RefCell::default(),
            }
        }
    }

    pub(crate) fn answer(status: u16, body: &str) -> Result<HttpResponse, WordPressError> {
        Ok(HttpResponse {
            status,
            body: body.to_owned(),
        })
    }

    impl WpHttp for FakeSite {
        fn post(
            &self,
            url: &str,
            headers: &[(&str, &str)],
            body: &[u8],
        ) -> Result<HttpResponse, WordPressError> {
            self.sent.borrow_mut().push((
                url.to_owned(),
                headers
                    .iter()
                    .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                    .collect(),
                body.to_vec(),
            ));
            self.answers
                .borrow_mut()
                .pop_front()
                .expect("the test queued an answer for every request")
        }
    }

    pub(crate) fn credentials() -> WpCredentials {
        WpCredentials::new("https://recetas.test", "aida", PASSWORD).unwrap()
    }

    fn header<'a>(sent: &'a Sent, name: &str) -> Option<&'a str> {
        sent.1
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    fn draft() -> RecipeDraft {
        RecipeDraft::new("Fabada vegana", "<p>Con fabes</p>").unwrap()
    }

    #[test]
    fn the_site_address_is_normalized() {
        for (raw, expected) in [
            ("https://recetas.test", "https://recetas.test"),
            ("  HTTPS://Recetas.Test/  ", "https://recetas.test"),
            ("https://recetas.test/blog/", "https://recetas.test/blog"),
            ("https://recetas.test:8443/", "https://recetas.test:8443"),
            (
                "https://sub.recetas-1.test//a//",
                "https://sub.recetas-1.test/a",
            ),
        ] {
            assert_eq!(SiteUrl::parse(raw).unwrap().as_str(), expected, "{raw}");
        }
    }

    #[test]
    fn a_bad_site_address_is_rejected() {
        assert_eq!(SiteUrl::parse("  "), Err(SettingsError::EmptySite));
        for raw in [
            "http://recetas.test",
            "recetas.test",
            "ftp://recetas.test",
            "https://",
            "https://:443",
            "https://recetas.test:",
            "https://recetas.test:80a",
            "https://user@recetas.test",
            "https://recetas.test?x=1",
            "https://recetas.test/#top",
            "https://recetas test",
            "https://-recetas.test",
            "https://recetas.test.",
            "https://rec\\etas.test",
            "https://recetas_x.test",
            "https://recetas.test:0",
            "https://recetas.test:65536",
            "https://recetas.test:99999",
            "https://recetas.test/../admin",
            "https://recetas.test/a/./b",
            "https://recetas.test/%2e%2e/admin",
            "https://recetas.test/a\"b",
            "https://recetas.test/ñ",
            "https://recetasñ.test",
        ] {
            assert_eq!(
                SiteUrl::parse(raw),
                Err(SettingsError::InvalidSite(raw.trim().to_owned())),
                "{raw}"
            );
        }
    }

    #[test]
    fn the_port_may_be_any_valid_number() {
        for port in ["1", "443", "8443", "65535"] {
            assert!(SiteUrl::parse(&format!("https://recetas.test:{port}")).is_ok());
        }
    }

    #[test]
    fn values_too_long_to_be_real_are_rejected() {
        let long_site = format!("https://recetas.test/{}", "a".repeat(200));
        assert_eq!(
            SiteUrl::parse(&long_site),
            Err(SettingsError::TooLong {
                field: "site address",
                max: 200
            })
        );
        assert_eq!(
            WpCredentials::new("https://recetas.test", &"u".repeat(101), PASSWORD),
            Err(SettingsError::TooLong {
                field: "user name",
                max: 100
            })
        );
        assert_eq!(
            AppPassword::new(&"p".repeat(201)),
            Err(SettingsError::TooLong {
                field: "application password",
                max: 200
            })
        );
        assert!(AppPassword::new(&"p".repeat(200)).is_ok());
    }

    #[test]
    fn the_application_password_loses_its_spaces_and_is_never_printed() {
        let password = AppPassword::new(PASSWORD).unwrap();
        assert_eq!(password.expose(), "abcdefghijklmnop");
        assert_eq!(format!("{password:?}"), "AppPassword(<redacted>)");
        assert_eq!(AppPassword::new(" \n"), Err(SettingsError::EmptyPassword));
        assert!(!format!("{:?}", credentials()).contains("abcd"));
    }

    #[test]
    fn credentials_need_a_user_name_without_a_colon() {
        assert_eq!(
            WpCredentials::new("https://recetas.test", " ", PASSWORD),
            Err(SettingsError::EmptyUsername)
        );
        assert_eq!(
            WpCredentials::new("https://recetas.test", "a:b", PASSWORD),
            Err(SettingsError::UsernameHasColon)
        );
        assert_eq!(credentials().username(), "aida");
    }

    #[test]
    fn a_draft_without_image_is_one_post_request_with_the_draft_status() {
        let site = FakeSite::new(vec![answer(
            201,
            r#"{"id":77,"link":"https://recetas.test/?p=77","status":"draft"}"#,
        )]);
        let post = create_draft(&site, &credentials(), &draft()).unwrap();
        assert_eq!(
            post,
            CreatedPost {
                id: 77,
                link: Some("https://recetas.test/?p=77".into()),
                edit_url: "https://recetas.test/wp-admin/post.php?post=77&action=edit".into()
            }
        );

        let sent = site.sent.borrow();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].0, "https://recetas.test/wp-json/wp/v2/posts");
        assert_eq!(header(&sent[0], "Content-Type"), Some("application/json"));
        let body: serde_json::Value = serde_json::from_slice(&sent[0].2).unwrap();
        assert_eq!(
            body,
            serde_json::json!({
                "title": "Fabada vegana",
                "content": "<p>Con fabes</p>",
                "status": "draft"
            })
        );
    }

    #[test]
    fn the_credentials_go_in_a_basic_authorization_header_only() {
        let site = FakeSite::new(vec![answer(201, r#"{"id":1}"#)]);
        create_draft(&site, &credentials(), &draft()).unwrap();
        let sent = site.sent.borrow();
        let expected = format!("Basic {}", STANDARD.encode("aida:abcdefghijklmnop"));
        assert_eq!(header(&sent[0], "Authorization"), Some(expected.as_str()));
        assert!(!sent[0].0.contains("abcd"));
        assert!(!String::from_utf8_lossy(&sent[0].2).contains("abcd"));
    }

    #[test]
    fn an_image_is_uploaded_first_and_becomes_the_featured_media() {
        let site = FakeSite::new(vec![
            answer(
                201,
                r#"{"id":55,"source_url":"https://recetas.test/a.png"}"#,
            ),
            answer(201, r#"{"id":78}"#),
        ]);
        let draft = draft().with_image(Some(Image::new("Mi foto.jpg", PNG.to_vec()).unwrap()));
        let post = create_draft(&site, &credentials(), &draft).unwrap();
        assert_eq!(post.id, 78);
        assert_eq!(post.link, None);

        let sent = site.sent.borrow();
        assert_eq!(sent.len(), 2);
        assert_eq!(sent[0].0, "https://recetas.test/wp-json/wp/v2/media");
        assert_eq!(header(&sent[0], "Content-Type"), Some("image/png"));
        assert_eq!(
            header(&sent[0], "Content-Disposition"),
            Some("attachment; filename=\"Mi-foto.png\"")
        );
        assert_eq!(sent[0].2, PNG);
        let body: serde_json::Value = serde_json::from_slice(&sent[1].2).unwrap();
        assert_eq!(body["featured_media"], 55);
        assert_eq!(body["status"], "draft");
    }

    #[test]
    fn a_failed_image_upload_creates_no_post() {
        let site = FakeSite::new(vec![answer(
            403,
            r#"{"code":"rest_cannot_create","message":"Sorry, you are not allowed to upload.","data":{"status":403}}"#,
        )]);
        let draft = draft().with_image(Some(Image::new("a", PNG.to_vec()).unwrap()));
        assert_eq!(
            create_draft(&site, &credentials(), &draft),
            Err(WordPressError::Api {
                code: "rest_cannot_create".into(),
                message: "Sorry, you are not allowed to upload.".into()
            })
        );
        assert_eq!(site.sent.borrow().len(), 1);
    }

    #[test]
    fn a_site_in_a_folder_keeps_its_path() {
        let site = FakeSite::new(vec![answer(201, r#"{"id":9}"#)]);
        let credentials = WpCredentials::new("https://x.test/blog/", "u", "p").unwrap();
        let post = create_draft(&site, &credentials, &draft()).unwrap();
        assert_eq!(
            site.sent.borrow()[0].0,
            "https://x.test/blog/wp-json/wp/v2/posts"
        );
        assert_eq!(
            post.edit_url,
            "https://x.test/blog/wp-admin/post.php?post=9&action=edit"
        );
    }

    #[test]
    fn answers_are_turned_into_the_right_errors() {
        let one = |status, body| {
            create_draft(
                &FakeSite::new(vec![answer(status, body)]),
                &credentials(),
                &draft(),
            )
        };
        assert_eq!(
            one(
                401,
                r#"{"code":"incorrect_password","message":"Invalid username or password."}"#
            ),
            Err(WordPressError::Api {
                code: "incorrect_password".into(),
                message: "Invalid username or password.".into()
            })
        );
        assert_eq!(one(301, ""), Err(WordPressError::Redirect(301)));
        assert_eq!(
            one(502, "<html>bad gateway</html>"),
            Err(WordPressError::Status(502))
        );
        assert_eq!(one(500, "{}"), Err(WordPressError::Status(500)));
        assert!(matches!(
            one(201, "<html>"),
            Err(WordPressError::Malformed(_))
        ));
        assert!(matches!(
            one(201, r#"{"title":"x"}"#),
            Err(WordPressError::Malformed(_))
        ));
    }

    #[test]
    fn a_redirect_on_the_image_upload_stops_before_any_post_is_created() {
        let site = FakeSite::new(vec![answer(302, "")]);
        let draft = draft().with_image(Some(Image::new("a", PNG.to_vec()).unwrap()));
        assert_eq!(
            create_draft(&site, &credentials(), &draft),
            Err(WordPressError::Redirect(302))
        );
        assert_eq!(site.sent.borrow().len(), 1);
    }

    #[test]
    fn a_network_failure_is_reported_without_details() {
        let site = FakeSite::new(vec![Err(WordPressError::Network(
            "could not connect".into(),
        ))]);
        let error = create_draft(&site, &credentials(), &draft()).unwrap_err();
        assert_eq!(
            error.to_string(),
            "the request to the WordPress site did not complete: could not connect"
        );
    }

    #[test]
    fn no_error_text_contains_the_password() {
        let errors = [
            WordPressError::NoSettings,
            WordPressError::Network("timed out".into()),
            WordPressError::Redirect(301),
            WordPressError::Status(500),
            WordPressError::Malformed("expected value at line 1".into()),
        ];
        for error in errors {
            assert!(!error.to_string().contains("abcd"));
            assert!(!format!("{error:?}").contains("abcd"));
        }
    }
}
