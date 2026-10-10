//! The picture of a recipe draft and the three ways to get it.
//!
//! The type of an image is read from its first bytes, never from what a file name, a `data:` URL
//! or a server claims, and the file name sent on is reduced to safe characters.

use std::fmt;

use base64::{engine::general_purpose::STANDARD, Engine};
use serde::Deserialize;
use thiserror::Error;

use crate::http::{Download, TransportError, UreqClient};
use crate::instagram_api::is_trusted_image_url;

/// The largest image accepted, in bytes.
pub const MAX_IMAGE_BYTES: usize = 10 * 1024 * 1024;
const DEFAULT_NAME: &str = "recipe-image";
const MAX_NAME_CHARS: usize = 60;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ImageError {
    #[error("the file is not a JPEG, PNG, WebP or GIF image")]
    NotAnImage,
    #[error("the image is larger than the {limit} byte limit")]
    TooLarge { limit: usize },
    #[error("the chosen file could not be read: {0}")]
    BadDataUrl(&'static str),
    #[error("`{0}` is not an Instagram image address")]
    UntrustedUrl(String),
    #[error("the image could not be downloaded: {0}")]
    Download(TransportError),
    #[error("the image download answered with HTTP status {0}")]
    DownloadStatus(u16),
}

/// The image formats WordPress takes that we accept.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageType {
    Jpeg,
    Png,
    Gif,
    Webp,
}

impl ImageType {
    /// Reads the type from the first bytes of a file.
    fn sniff(bytes: &[u8]) -> Option<Self> {
        if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
            Some(Self::Jpeg)
        } else if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
            Some(Self::Png)
        } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
            Some(Self::Gif)
        } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
            Some(Self::Webp)
        } else {
            None
        }
    }

    pub fn mime(self) -> &'static str {
        match self {
            Self::Jpeg => "image/jpeg",
            Self::Png => "image/png",
            Self::Gif => "image/gif",
            Self::Webp => "image/webp",
        }
    }

    fn extension(self) -> &'static str {
        match self {
            Self::Jpeg => "jpg",
            Self::Png => "png",
            Self::Gif => "gif",
            Self::Webp => "webp",
        }
    }
}

/// A validated image: a safe file name that matches its real type, and its bytes.
#[derive(Clone, PartialEq, Eq)]
pub struct Image {
    filename: String,
    kind: ImageType,
    bytes: Vec<u8>,
}

impl Image {
    /// Checks the size and the real type, and builds a safe file name from `name`.
    pub fn new(name: &str, bytes: Vec<u8>) -> Result<Self, ImageError> {
        if bytes.len() > MAX_IMAGE_BYTES {
            return Err(ImageError::TooLarge {
                limit: MAX_IMAGE_BYTES,
            });
        }
        let kind = ImageType::sniff(&bytes).ok_or(ImageError::NotAnImage)?;
        Ok(Self {
            filename: format!("{}.{}", safe_stem(name), kind.extension()),
            kind,
            bytes,
        })
    }

    pub fn filename(&self) -> &str {
        &self.filename
    }

    pub fn mime(&self) -> &'static str {
        self.kind.mime()
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

impl fmt::Debug for Image {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Image({}, {}, {} bytes)",
            self.filename,
            self.kind.mime(),
            self.bytes.len()
        )
    }
}

/// The file name without folders or extension, reduced to ASCII letters, digits, `-` and `_`: it
/// goes into a raw HTTP header, where some hosts mangle other characters.
fn safe_stem(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or_default();
    let stem = base.rsplit_once('.').map_or(base, |(stem, _)| stem);
    let mut safe = String::new();
    let mut last_was_dash = true;
    for c in stem.chars() {
        if c.is_ascii_alphanumeric() || c == '_' {
            safe.push(c);
            last_was_dash = false;
        } else if !last_was_dash {
            safe.push('-');
            last_was_dash = true;
        }
    }
    let safe: String = safe
        .trim_matches('-')
        .chars()
        .take(MAX_NAME_CHARS)
        .collect();
    if safe.is_empty() {
        DEFAULT_NAME.to_owned()
    } else {
        safe
    }
}

/// Decodes a `data:<type>;base64,<payload>` URL into bytes.
fn decode_data_url(data_url: &str) -> Result<Vec<u8>, ImageError> {
    let rest = data_url
        .strip_prefix("data:")
        .ok_or(ImageError::BadDataUrl("it is not a data URL"))?;
    let (header, payload) = rest
        .split_once(',')
        .ok_or(ImageError::BadDataUrl("the data URL has no content"))?;
    if !header.ends_with(";base64") {
        return Err(ImageError::BadDataUrl("the data URL is not base64"));
    }
    // Refuse before decoding so a huge payload never allocates.
    if payload.len() > MAX_IMAGE_BYTES / 3 * 4 + 8 {
        return Err(ImageError::TooLarge {
            limit: MAX_IMAGE_BYTES,
        });
    }
    STANDARD
        .decode(payload)
        .map_err(|_| ImageError::BadDataUrl("the data URL is not valid base64"))
}

/// What the user chose for the post's picture.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ImageChoice {
    /// No featured image.
    None,
    /// The picture of the Instagram post, downloaded from Instagram's CDN.
    PostImage { url: String },
    /// A file read by the frontend and sent as a `data:` URL.
    File { data_url: String, filename: String },
}

/// Downloads an image, reading at most `max_bytes`.
pub trait ImageFetcher {
    fn fetch(&self, url: &str, max_bytes: usize) -> Result<Download, TransportError>;
}

impl ImageFetcher for UreqClient {
    fn fetch(&self, url: &str, max_bytes: usize) -> Result<Download, TransportError> {
        self.get_bytes(url, max_bytes as u64)
    }
}

/// Turns the user's choice into an image. A post image address is checked again here: the
/// frontend is not trusted to have sent only an Instagram link.
pub fn resolve<F: ImageFetcher>(
    choice: ImageChoice,
    fetcher: &F,
) -> Result<Option<Image>, ImageError> {
    match choice {
        ImageChoice::None => Ok(None),
        ImageChoice::PostImage { url } => {
            if !is_trusted_image_url(&url) {
                return Err(ImageError::UntrustedUrl(url));
            }
            let download = fetcher
                .fetch(&url, MAX_IMAGE_BYTES)
                .map_err(ImageError::Download)?;
            if download.status != 200 {
                return Err(ImageError::DownloadStatus(download.status));
            }
            Image::new(DEFAULT_NAME, download.bytes).map(Some)
        }
        ImageChoice::File { data_url, filename } => {
            Image::new(&filename, decode_data_url(&data_url)?).map(Some)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 0];

    fn png_data_url() -> String {
        format!("data:image/png;base64,{}", STANDARD.encode(PNG))
    }

    struct FakeFetcher {
        status: u16,
        bytes: Vec<u8>,
        asked: RefCell<Vec<String>>,
    }

    impl FakeFetcher {
        fn new(status: u16, bytes: &[u8]) -> Self {
            Self {
                status,
                bytes: bytes.to_vec(),
                asked: RefCell::default(),
            }
        }
    }

    impl ImageFetcher for FakeFetcher {
        fn fetch(&self, url: &str, _max_bytes: usize) -> Result<Download, TransportError> {
            self.asked.borrow_mut().push(url.to_owned());
            Ok(Download {
                status: self.status,
                content_type: Some("text/plain".into()),
                bytes: self.bytes.clone(),
            })
        }
    }

    #[test]
    fn the_type_comes_from_the_first_bytes() {
        let cases: [(&[u8], &str, &str); 4] = [
            (&[0xFF, 0xD8, 0xFF, 0xE0, 0], "image/jpeg", "jpg"),
            (PNG, "image/png", "png"),
            (b"GIF89a....", "image/gif", "gif"),
            (b"RIFF\x10\0\0\0WEBPVP8 ", "image/webp", "webp"),
        ];
        for (bytes, mime, extension) in cases {
            let image = Image::new("photo", bytes.to_vec()).unwrap();
            assert_eq!(image.mime(), mime);
            assert_eq!(image.filename(), format!("photo.{extension}"));
        }
    }

    #[test]
    fn something_that_is_not_an_image_is_rejected_whatever_its_name() {
        for bytes in [&b"<html>not an image</html>"[..], b"", b"RIFF\0\0\0\0WAVE"] {
            assert_eq!(
                Image::new("photo.png", bytes.to_vec()),
                Err(ImageError::NotAnImage)
            );
        }
    }

    #[test]
    fn an_oversized_image_is_rejected() {
        let mut bytes = PNG.to_vec();
        bytes.resize(MAX_IMAGE_BYTES + 1, 0);
        assert_eq!(
            Image::new("big", bytes),
            Err(ImageError::TooLarge {
                limit: MAX_IMAGE_BYTES
            })
        );
    }

    #[test]
    fn the_file_name_is_reduced_to_safe_characters_and_the_real_extension() {
        for (name, stem) in [
            ("../../etc/evil.php", "evil"),
            ("C:\\Users\\me\\Mi foto (1).JPG", "Mi-foto-1"),
            ("   ", "recipe-image"),
            (".htaccess", "recipe-image"),
            ("tarta_de_queso.webp", "tarta_de_queso"),
            ("  --a b--  ", "a-b"),
            ("a\"; filename=\"b", "a-filename-b"),
            ("Tarta ñandú.png", "Tarta-and"),
            ("ñ\u{1f600}.png", "recipe-image"),
            ("a\r\nX-Evil: 1.png", "a-X-Evil-1"),
        ] {
            let image = Image::new(name, PNG.to_vec()).unwrap();
            assert_eq!(image.filename(), format!("{stem}.png"), "{name:?}");
        }
    }

    #[test]
    fn a_long_file_name_is_cut() {
        let image = Image::new(&"a".repeat(200), PNG.to_vec()).unwrap();
        assert_eq!(image.filename().len(), MAX_NAME_CHARS + ".png".len());
    }

    #[test]
    fn debug_output_does_not_dump_the_bytes() {
        let image = Image::new("photo", PNG.to_vec()).unwrap();
        assert_eq!(
            format!("{image:?}"),
            "Image(photo.png, image/png, 12 bytes)"
        );
    }

    #[test]
    fn no_image_choice_gives_no_image_and_no_download() {
        let fetcher = FakeFetcher::new(200, PNG);
        assert_eq!(resolve(ImageChoice::None, &fetcher), Ok(None));
        assert!(fetcher.asked.borrow().is_empty());
    }

    #[test]
    fn a_post_image_is_downloaded_and_its_type_is_sniffed() {
        let fetcher = FakeFetcher::new(200, PNG);
        let url = "https://scontent.cdninstagram.com/v/1.jpg";
        let image = resolve(ImageChoice::PostImage { url: url.into() }, &fetcher)
            .unwrap()
            .unwrap();
        assert_eq!(image.filename(), "recipe-image.png");
        assert_eq!(*fetcher.asked.borrow(), [url]);
    }

    #[test]
    fn a_post_image_from_another_host_is_refused_before_any_request() {
        let fetcher = FakeFetcher::new(200, PNG);
        for url in [
            "https://evil.test/a.png",
            "http://scontent.cdninstagram.com/a.png",
            "file:///etc/passwd",
            "https://169.254.169.254/latest/meta-data",
        ] {
            let result = resolve(ImageChoice::PostImage { url: url.into() }, &fetcher);
            assert_eq!(result, Err(ImageError::UntrustedUrl(url.into())));
        }
        assert!(fetcher.asked.borrow().is_empty());
    }

    #[test]
    fn a_failed_download_or_a_non_image_answer_is_an_error() {
        let url = "https://scontent.cdninstagram.com/v/1.jpg";
        let choice = || ImageChoice::PostImage { url: url.into() };
        assert_eq!(
            resolve(choice(), &FakeFetcher::new(403, b"")),
            Err(ImageError::DownloadStatus(403))
        );
        assert_eq!(
            resolve(choice(), &FakeFetcher::new(200, b"<html>")),
            Err(ImageError::NotAnImage)
        );
    }

    #[test]
    fn a_file_choice_decodes_the_data_url() {
        let choice = ImageChoice::File {
            data_url: png_data_url(),
            filename: "Mi receta.png".into(),
        };
        let image = resolve(choice, &FakeFetcher::new(200, b""))
            .unwrap()
            .unwrap();
        assert_eq!(image.filename(), "Mi-receta.png");
        assert_eq!(image.bytes(), PNG);
    }

    #[test]
    fn a_malformed_data_url_is_rejected() {
        for data_url in [
            "https://x.test/a.png",
            "data:image/png;base64",
            "data:image/png,rawbytes",
            "data:image/png;base64,@@@not-base64@@@",
        ] {
            let choice = ImageChoice::File {
                data_url: data_url.into(),
                filename: "a.png".into(),
            };
            assert!(
                matches!(
                    resolve(choice, &FakeFetcher::new(200, b"")),
                    Err(ImageError::BadDataUrl(_))
                ),
                "{data_url}"
            );
        }
    }

    #[test]
    fn a_huge_data_url_is_refused_before_decoding() {
        let choice = ImageChoice::File {
            data_url: format!("data:image/png;base64,{}", "A".repeat(MAX_IMAGE_BYTES * 2)),
            filename: "a.png".into(),
        };
        assert_eq!(
            resolve(choice, &FakeFetcher::new(200, b"")),
            Err(ImageError::TooLarge {
                limit: MAX_IMAGE_BYTES
            })
        );
    }

    #[test]
    fn the_choice_is_read_from_the_json_the_frontend_sends() {
        let none: ImageChoice = serde_json::from_str(r#"{"kind":"none"}"#).unwrap();
        let post: ImageChoice =
            serde_json::from_str(r#"{"kind":"post_image","url":"https://x"}"#).unwrap();
        let file: ImageChoice =
            serde_json::from_str(r#"{"kind":"file","data_url":"d","filename":"f"}"#).unwrap();
        assert_eq!(none, ImageChoice::None);
        assert_eq!(
            post,
            ImageChoice::PostImage {
                url: "https://x".into()
            }
        );
        assert_eq!(
            file,
            ImageChoice::File {
                data_url: "d".into(),
                filename: "f".into()
            }
        );
    }
}
