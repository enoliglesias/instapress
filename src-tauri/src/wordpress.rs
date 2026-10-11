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
use crate::publication_html::PublicationDraft;
use crate::token::{OsSecret, TokenError};

const WORDPRESS_SECRET: OsSecret = OsSecret::new("wordpress");
/// The post meta where Yoast SEO keeps the meta description. WordPress ignores it over the REST
/// API unless the site registers it with `show_in_rest` (see `knowledge/operational`).
const META_DESCRIPTION_KEY: &str = "_yoast_wpseo_metadesc";
/// The post meta where Yoast SEO keeps the focus keyphrase; it needs the same registration.
const FOCUS_KEYPHRASE_KEY: &str = "_yoast_wpseo_focuskw";

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

/// The HTTP operations the client needs: a `POST` of raw bytes and a `GET`, with headers. Implementations
/// must not put the headers in their errors.
pub trait WpHttp {
    fn post(
        &self,
        url: &str,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> Result<HttpResponse, WordPressError>;

    fn get(&self, url: &str, headers: &[(&str, &str)]) -> Result<HttpResponse, WordPressError>;
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

    fn get(&self, url: &str, headers: &[(&str, &str)]) -> Result<HttpResponse, WordPressError> {
        self.get_with_headers(url, headers)
            .map_err(|e| WordPressError::Network(e.to_string()))
    }
}

/// A category as the window lists it. Its place in the tree is its `depth`: 0 for a top-level
/// category, 1 for a child of one, and so on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CategoryItem {
    pub id: u64,
    pub name: String,
    pub depth: usize,
}

#[derive(Deserialize)]
struct RawCategory {
    id: u64,
    name: String,
    /// 0 for a top-level category.
    #[serde(default)]
    parent: u64,
}

/// WordPress answers up to this many categories per page.
const CATEGORY_PAGE_SIZE: usize = 100;
/// A site with more categories than this many pages hold is not expected.
const MAX_CATEGORY_PAGES: u32 = 10;

/// Decodes the HTML entities WordPress leaves in the name of a category (`Lemon &amp; Limes`).
/// Only the few it writes are known; anything else stays as it is.
fn decode_entities(text: &str) -> String {
    let mut decoded = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('&') {
        decoded.push_str(&rest[..start]);
        rest = &rest[start..];
        let entity = rest
            .find(';')
            .filter(|end| *end <= 10)
            .map(|end| &rest[1..end]);
        let character = entity.and_then(|name| match name {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some('\u{a0}'),
            _ => name
                .strip_prefix('#')
                .and_then(|digits| match digits.strip_prefix(['x', 'X']) {
                    Some(hex) => u32::from_str_radix(hex, 16).ok(),
                    None => digits.parse().ok(),
                })
                .and_then(char::from_u32),
        });
        match (character, entity) {
            (Some(c), Some(name)) => {
                decoded.push(c);
                rest = &rest[name.len() + 2..];
            }
            _ => {
                decoded.push('&');
                rest = &rest[1..];
            }
        }
    }
    decoded.push_str(rest);
    decoded
}

/// Puts the categories in tree order: each category is followed by its children, and brothers
/// are sorted by name without regard to case. A category whose parent is not in the list is
/// shown at the top level, and a loop in the parents cannot make one appear twice.
fn arrange_categories(raw: Vec<RawCategory>) -> Vec<CategoryItem> {
    let ids: std::collections::HashSet<u64> = raw.iter().map(|category| category.id).collect();
    let mut children: std::collections::HashMap<u64, Vec<(String, u64, String)>> =
        std::collections::HashMap::new();
    for category in raw {
        let name = decode_entities(&category.name);
        let parent = if ids.contains(&category.parent) && category.parent != category.id {
            category.parent
        } else {
            0
        };
        children
            .entry(parent)
            .or_default()
            .push((name.to_lowercase(), category.id, name));
    }
    for siblings in children.values_mut() {
        siblings.sort();
    }

    let mut arranged = Vec::new();
    let mut seen = std::collections::HashSet::new();
    // Depth-first, taking the first brother off the front of each level.
    let mut stack: Vec<(u64, usize)> = children
        .get(&0)
        .into_iter()
        .flatten()
        .rev()
        .map(|(_, id, _)| (*id, 0))
        .collect();
    let names: std::collections::HashMap<u64, String> = children
        .values()
        .flatten()
        .map(|(_, id, name)| (*id, name.clone()))
        .collect();
    while let Some((id, depth)) = stack.pop() {
        if !seen.insert(id) {
            continue;
        }
        arranged.push(CategoryItem {
            id,
            name: names[&id].clone(),
            depth,
        });
        for (_, child, _) in children.get(&id).into_iter().flatten().rev() {
            stack.push((*child, depth + 1));
        }
    }
    arranged
}

/// Lists the categories of the site in tree order, for the window to offer. It reads them page by
/// page, up to a hundred at a time.
pub fn list_categories<S: CredentialsStore, H: WpHttp>(
    store: &S,
    http: &H,
) -> Result<Vec<CategoryItem>, WordPressError> {
    let credentials = store.load()?.ok_or(WordPressError::NoSettings)?;
    let authorization = credentials.authorization();
    let site = credentials.site.as_str();

    let mut all = Vec::new();
    for page in 1..=MAX_CATEGORY_PAGES {
        let url = format!(
            "{site}/wp-json/wp/v2/categories?per_page={CATEGORY_PAGE_SIZE}&page={page}&_fields=id,name,parent"
        );
        let body = match success_body(http.get(&url, &[("Authorization", &authorization)])?) {
            Ok(body) => body,
            // Asking for the page after the last one is an error in WordPress; here it only
            // means there are no more.
            Err(WordPressError::Api { code, .. })
                if page > 1 && code == "rest_post_invalid_page_number" =>
            {
                break
            }
            Err(error) => return Err(error),
        };
        let batch: Vec<RawCategory> =
            serde_json::from_str(&body).map_err(|e| WordPressError::Malformed(e.to_string()))?;
        let last = batch.len() < CATEGORY_PAGE_SIZE;
        all.extend(batch);
        if last {
            break;
        }
    }
    Ok(arrange_categories(all))
}

/// The post that was created.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CreatedPost {
    pub id: u64,
    /// The address WordPress reports for the post; for a draft it is a preview-style link.
    pub link: Option<String>,
    /// Where the user opens the draft in the WordPress editor.
    pub edit_url: String,
    /// `None` when no meta description was sent. Otherwise whether WordPress kept it: `false`
    /// means the site does not expose the field to the REST API and ignored it.
    pub meta_description_saved: Option<bool>,
    /// The same for the focus keyphrase.
    pub focus_keyphrase_saved: Option<bool>,
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
    /// The registered meta of the post. WordPress sends `[]` when it has none.
    meta: Option<serde_json::Value>,
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
    draft: &PublicationDraft,
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
    if let Some(slug) = draft.slug() {
        body["slug"] = slug.into();
    }
    // In UTC, so the date does not depend on the time zone of the site. Left out, WordPress dates
    // the post when it is published. The status stays `draft`, so a future date does not
    // schedule anything until the user publishes.
    if let Some(date) = draft.date_gmt() {
        body["date_gmt"] = date.into();
    }
    if !draft.categories().is_empty() {
        body["categories"] = draft.categories().into();
    }
    let mut meta = serde_json::Map::new();
    if let Some(description) = draft.meta_description() {
        meta.insert(META_DESCRIPTION_KEY.into(), description.into());
    }
    if let Some(keyphrase) = draft.focus_keyphrase() {
        meta.insert(FOCUS_KEYPHRASE_KEY.into(), keyphrase.into());
    }
    if !meta.is_empty() {
        body["meta"] = meta.into();
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
    // Whether the answer holds what was sent under `key`. A meta key the site has not registered
    // for the REST API is ignored without any error and is missing from the answer.
    let kept = |key: &str, sent: Option<&str>| {
        sent.map(|sent| {
            post.meta
                .as_ref()
                .and_then(|meta| meta.get(key))
                .and_then(|saved| saved.as_str())
                == Some(sent)
        })
    };
    let meta_description_saved = kept(META_DESCRIPTION_KEY, draft.meta_description());
    let focus_keyphrase_saved = kept(FOCUS_KEYPHRASE_KEY, draft.focus_keyphrase());
    Ok(CreatedPost {
        edit_url: format!("{site}/wp-admin/post.php?post={}&action=edit", post.id),
        id: post.id,
        link: post.link,
        meta_description_saved,
        focus_keyphrase_saved,
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

        fn get(&self, url: &str, headers: &[(&str, &str)]) -> Result<HttpResponse, WordPressError> {
            self.sent.borrow_mut().push((
                url.to_owned(),
                headers
                    .iter()
                    .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                    .collect(),
                Vec::new(),
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

    fn draft() -> PublicationDraft {
        PublicationDraft::new("Fabada vegana", "<p>Con fabes</p>").unwrap()
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
                edit_url: "https://recetas.test/wp-admin/post.php?post=77&action=edit".into(),
                meta_description_saved: None,
                focus_keyphrase_saved: None
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

    fn seo_draft() -> PublicationDraft {
        draft()
            .with_slug("Fabada vegana 💚")
            .with_meta_description("Una fabada vegana cremosa y fácil.")
            .unwrap()
    }

    #[test]
    fn the_slug_and_the_meta_description_go_in_the_post() {
        let site = FakeSite::new(vec![answer(
            201,
            r#"{"id":77,"meta":{"_yoast_wpseo_metadesc":"Una fabada vegana cremosa y fácil."}}"#,
        )]);
        let post = create_draft(&site, &credentials(), &seo_draft()).unwrap();
        assert_eq!(post.meta_description_saved, Some(true));

        let sent = site.sent.borrow();
        let body: serde_json::Value = serde_json::from_slice(&sent[0].2).unwrap();
        assert_eq!(body["slug"], "fabada-vegana");
        assert_eq!(
            body["meta"],
            serde_json::json!({"_yoast_wpseo_metadesc": "Una fabada vegana cremosa y fácil."})
        );
    }

    fn full_seo_draft() -> PublicationDraft {
        seo_draft().with_focus_keyphrase("fabada vegana").unwrap()
    }

    #[test]
    fn a_chosen_date_goes_in_the_post_as_utc_and_the_status_stays_draft() {
        let site = FakeSite::new(vec![answer(201, r#"{"id":77}"#)]);
        let dated = draft().with_date_gmt("2025-01-28T09:30:00").unwrap();
        create_draft(&site, &credentials(), &dated).unwrap();
        let sent = site.sent.borrow();
        let body: serde_json::Value = serde_json::from_slice(&sent[0].2).unwrap();
        assert_eq!(body["date_gmt"], "2025-01-28T09:30:00");
        assert!(body.get("date").is_none());
        assert_eq!(body["status"], "draft");
    }

    #[test]
    fn without_a_date_none_is_sent_so_wordpress_uses_the_day_it_is_published() {
        let site = FakeSite::new(vec![answer(201, r#"{"id":77}"#)]);
        create_draft(&site, &credentials(), &draft()).unwrap();
        let sent = site.sent.borrow();
        let body: serde_json::Value = serde_json::from_slice(&sent[0].2).unwrap();
        assert!(body.get("date_gmt").is_none());
        assert!(body.get("date").is_none());
    }

    #[test]
    fn the_focus_keyphrase_goes_in_the_meta_next_to_the_description() {
        let site = FakeSite::new(vec![answer(
            201,
            r#"{"id":77,"meta":{"_yoast_wpseo_metadesc":"Una fabada vegana cremosa y fácil.",
                                 "_yoast_wpseo_focuskw":"fabada vegana"}}"#,
        )]);
        let post = create_draft(&site, &credentials(), &full_seo_draft()).unwrap();
        assert_eq!(post.meta_description_saved, Some(true));
        assert_eq!(post.focus_keyphrase_saved, Some(true));

        let sent = site.sent.borrow();
        let body: serde_json::Value = serde_json::from_slice(&sent[0].2).unwrap();
        assert_eq!(
            body["meta"],
            serde_json::json!({
                "_yoast_wpseo_metadesc": "Una fabada vegana cremosa y fácil.",
                "_yoast_wpseo_focuskw": "fabada vegana"
            })
        );
    }

    #[test]
    fn a_keyphrase_alone_sends_only_that_meta() {
        let site = FakeSite::new(vec![answer(201, r#"{"id":1}"#)]);
        let draft = draft().with_focus_keyphrase("fabada").unwrap();
        let post = create_draft(&site, &credentials(), &draft).unwrap();
        assert_eq!(post.meta_description_saved, None);
        assert_eq!(post.focus_keyphrase_saved, Some(false));
        let sent = site.sent.borrow();
        let body: serde_json::Value = serde_json::from_slice(&sent[0].2).unwrap();
        assert_eq!(
            body["meta"],
            serde_json::json!({"_yoast_wpseo_focuskw": "fabada"})
        );
    }

    #[test]
    fn each_seo_field_is_noticed_on_its_own() {
        // The site registered one of the two keys, so one is kept and the other is not.
        let site = FakeSite::new(vec![answer(
            201,
            r#"{"id":1,"meta":{"_yoast_wpseo_metadesc":"Una fabada vegana cremosa y fácil."}}"#,
        )]);
        let post = create_draft(&site, &credentials(), &full_seo_draft()).unwrap();
        assert_eq!(post.meta_description_saved, Some(true));
        assert_eq!(post.focus_keyphrase_saved, Some(false));
    }

    fn raw(id: u64, name: &str, parent: u64) -> RawCategory {
        RawCategory {
            id,
            name: name.into(),
            parent,
        }
    }

    fn item(id: u64, name: &str, depth: usize) -> CategoryItem {
        CategoryItem {
            id,
            name: name.into(),
            depth,
        }
    }

    #[test]
    fn entities_in_a_category_name_are_decoded() {
        for (raw, decoded) in [
            ("Lemon &amp; Limes", "Lemon & Limes"),
            ("Platos &lt;principales&gt;", "Platos <principales>"),
            (
                "&quot;Cocina&quot; &#8217;casera&#8217;",
                "\"Cocina\" \u{2019}casera\u{2019}",
            ),
            ("It&#039;s &#x1F49A;", "It's \u{1F49A}"),
            ("a&nbsp;b", "a\u{a0}b"),
            ("Postres veganos", "Postres veganos"),
            ("Recetas rápidas", "Recetas rápidas"),
            ("", ""),
        ] {
            assert_eq!(decode_entities(raw), decoded, "{raw:?}");
        }
    }

    #[test]
    fn what_is_not_an_entity_is_left_alone() {
        for text in [
            "Tom & Jerry",
            "a &unknown; b",
            "&",
            "&;",
            "&#;",
            "&#xZZ;",
            "&#99999999999;",
            "&amp",
            "x &#1114112; y",
        ] {
            assert_eq!(decode_entities(text), text, "{text:?}");
        }
    }

    #[test]
    fn categories_come_in_tree_order_with_their_depth() {
        let arranged = arrange_categories(vec![
            raw(3, "Tartas veganas", 2),
            raw(1, "Vegan Lifestyle", 0),
            raw(2, "Postres veganos", 0),
            raw(4, "Batidos veganos", 2),
            raw(5, "Recetas veganas para navidad", 0),
            raw(6, "helados veganos", 2),
            raw(7, "Sin categoría", 0),
            raw(8, "Tartas de queso", 3),
        ]);
        assert_eq!(
            arranged,
            vec![
                item(2, "Postres veganos", 0),
                item(4, "Batidos veganos", 1),
                item(6, "helados veganos", 1),
                item(3, "Tartas veganas", 1),
                item(8, "Tartas de queso", 2),
                item(5, "Recetas veganas para navidad", 0),
                item(7, "Sin categoría", 0),
                item(1, "Vegan Lifestyle", 0),
            ]
        );
    }

    #[test]
    fn a_category_whose_parent_is_missing_is_shown_at_the_top() {
        let arranged = arrange_categories(vec![raw(1, "Huérfana", 99), raw(2, "Otra", 0)]);
        assert_eq!(arranged, vec![item(1, "Huérfana", 0), item(2, "Otra", 0)]);
    }

    #[test]
    fn a_loop_in_the_parents_cannot_repeat_or_hang() {
        let arranged = arrange_categories(vec![
            raw(1, "A", 2),
            raw(2, "B", 1),
            raw(3, "Sola", 3),
            raw(4, "Normal", 0),
        ]);
        let ids: Vec<u64> = arranged.iter().map(|c| c.id).collect();
        let mut unique = ids.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(ids.len(), unique.len(), "{ids:?}");
        assert!(ids.contains(&4));
    }

    #[test]
    fn names_with_entities_are_decoded_and_sorted_by_what_they_say() {
        let arranged = arrange_categories(vec![raw(1, "Lemon &amp; Limes", 0), raw(2, "Ajo", 0)]);
        assert_eq!(
            arranged,
            vec![item(2, "Ajo", 0), item(1, "Lemon & Limes", 0)]
        );
    }

    #[test]
    fn no_categories_is_an_empty_list() {
        assert_eq!(arrange_categories(vec![]), vec![]);
    }

    struct SavedSettings;

    impl CredentialsStore for SavedSettings {
        fn load(&self) -> Result<Option<WpCredentials>, WordPressError> {
            Ok(Some(credentials()))
        }
        fn save(&self, _: &WpCredentials) -> Result<(), WordPressError> {
            Ok(())
        }
        fn clear(&self) -> Result<(), WordPressError> {
            Ok(())
        }
    }

    struct NoSettingsSaved;

    impl CredentialsStore for NoSettingsSaved {
        fn load(&self) -> Result<Option<WpCredentials>, WordPressError> {
            Ok(None)
        }
        fn save(&self, _: &WpCredentials) -> Result<(), WordPressError> {
            Ok(())
        }
        fn clear(&self) -> Result<(), WordPressError> {
            Ok(())
        }
    }

    #[test]
    fn the_categories_are_read_with_the_credentials_and_arranged() {
        let site = FakeSite::new(vec![answer(
            200,
            r#"[{"id":2,"name":"Postres veganos","parent":0},
                {"id":4,"name":"Batidos veganos","parent":2},
                {"id":9,"name":"Lemon &amp; Limes","parent":0}]"#,
        )]);
        let list = list_categories(&SavedSettings, &site).unwrap();
        assert_eq!(
            list,
            vec![
                item(9, "Lemon & Limes", 0),
                item(2, "Postres veganos", 0),
                item(4, "Batidos veganos", 1),
            ]
        );
        let sent = site.sent.borrow();
        assert_eq!(sent.len(), 1);
        assert_eq!(
            sent[0].0,
            "https://recetas.test/wp-json/wp/v2/categories?per_page=100&page=1&_fields=id,name,parent"
        );
        let expected = format!("Basic {}", STANDARD.encode("aida:abcdefghijklmnop"));
        assert_eq!(header(&sent[0], "Authorization"), Some(expected.as_str()));
    }

    #[test]
    fn a_full_page_asks_for_the_next_one() {
        let page = |from: u64, count: u64| {
            let items: Vec<String> = (from..from + count)
                .map(|id| format!(r#"{{"id":{id},"name":"Cat {id:03}","parent":0}}"#))
                .collect();
            format!("[{}]", items.join(","))
        };
        let site = FakeSite::new(vec![answer(200, &page(1, 100)), answer(200, &page(101, 5))]);
        let list = list_categories(&SavedSettings, &site).unwrap();
        assert_eq!(list.len(), 105);
        let sent = site.sent.borrow();
        assert_eq!(sent.len(), 2);
        assert!(sent[1].0.contains("page=2"));
    }

    #[test]
    fn a_site_with_exactly_one_full_page_ends_when_wordpress_refuses_the_next() {
        let items: Vec<String> = (1..=100)
            .map(|id| format!(r#"{{"id":{id},"name":"Cat {id:03}","parent":0}}"#))
            .collect();
        let site = FakeSite::new(vec![
            answer(200, &format!("[{}]", items.join(","))),
            answer(
                400,
                r#"{"code":"rest_post_invalid_page_number","message":"La p\u00e1gina no existe."}"#,
            ),
        ]);
        let list = list_categories(&SavedSettings, &site).unwrap();
        assert_eq!(list.len(), 100);
    }

    #[test]
    fn a_wordpress_error_on_the_first_page_is_reported() {
        let site = FakeSite::new(vec![answer(
            401,
            r#"{"code":"incorrect_password","message":"Contraseña incorrecta."}"#,
        )]);
        assert_eq!(
            list_categories(&SavedSettings, &site),
            Err(WordPressError::Api {
                code: "incorrect_password".into(),
                message: "Contraseña incorrecta.".into()
            })
        );
    }

    #[test]
    fn listing_without_settings_does_not_call_out() {
        let site = FakeSite::new(vec![]);
        assert_eq!(
            list_categories(&NoSettingsSaved, &site),
            Err(WordPressError::NoSettings)
        );
        assert!(site.sent.borrow().is_empty());
    }

    #[test]
    fn an_answer_that_is_not_a_list_is_malformed() {
        let site = FakeSite::new(vec![answer(200, r#"{"not":"a list"}"#)]);
        assert!(matches!(
            list_categories(&SavedSettings, &site),
            Err(WordPressError::Malformed(_))
        ));
    }

    #[test]
    fn the_chosen_categories_go_in_the_post() {
        let site = FakeSite::new(vec![answer(201, r#"{"id":1}"#)]);
        let draft = draft().with_categories(&[7, 3]).unwrap();
        create_draft(&site, &credentials(), &draft).unwrap();
        let sent = site.sent.borrow();
        let body: serde_json::Value = serde_json::from_slice(&sent[0].2).unwrap();
        assert_eq!(body["categories"], serde_json::json!([7, 3]));
    }

    #[test]
    fn without_categories_the_post_leaves_the_choice_to_wordpress() {
        let site = FakeSite::new(vec![answer(201, r#"{"id":1}"#)]);
        create_draft(&site, &credentials(), &draft()).unwrap();
        let sent = site.sent.borrow();
        let body: serde_json::Value = serde_json::from_slice(&sent[0].2).unwrap();
        assert!(body.get("categories").is_none());
    }

    #[test]
    fn a_post_without_slug_or_description_sends_neither() {
        let site = FakeSite::new(vec![answer(201, r#"{"id":1}"#)]);
        let post = create_draft(&site, &credentials(), &draft()).unwrap();
        assert_eq!(post.meta_description_saved, None);
        assert_eq!(post.focus_keyphrase_saved, None);
        let sent = site.sent.borrow();
        let body: serde_json::Value = serde_json::from_slice(&sent[0].2).unwrap();
        assert!(body.get("slug").is_none());
        assert!(body.get("meta").is_none());
    }

    #[test]
    fn a_site_that_does_not_keep_the_meta_description_is_noticed() {
        // WordPress sends `[]` for a post whose type exposes no meta, an object without the key
        // when the site does not register Yoast's field, and no `meta` at all in older versions.
        for answer_body in [
            r#"{"id":1,"meta":[]}"#,
            r#"{"id":1,"meta":{}}"#,
            r#"{"id":1,"meta":{"footnotes":""}}"#,
            r#"{"id":1}"#,
            r#"{"id":1,"meta":{"_yoast_wpseo_metadesc":""}}"#,
            r#"{"id":1,"meta":{"_yoast_wpseo_metadesc":"otro texto"}}"#,
            r#"{"id":1,"meta":{"_yoast_wpseo_metadesc":null}}"#,
        ] {
            let site = FakeSite::new(vec![answer(201, answer_body)]);
            let post = create_draft(&site, &credentials(), &seo_draft()).unwrap();
            assert_eq!(post.meta_description_saved, Some(false), "{answer_body}");
        }
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
