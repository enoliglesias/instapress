//! The whole "create a draft on WordPress" flow, in the order that spends the least:
//! check the text and the video, check the saved settings, get the image, then talk to WordPress.

use serde::Deserialize;
use thiserror::Error;

use crate::image::{resolve, ImageChoice, ImageError, ImageFetcher};
use crate::publication_html::{DraftError, PublicationDraft};
use crate::video::{self, VideoChoice, VideoError};
use crate::wordpress::{create_draft, CreatedPost, CredentialsStore, WordPressError, WpHttp};

/// What the frontend sends when the user presses "Create draft".
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct DraftInput {
    pub title: String,
    pub html: String,
    pub image: ImageChoice,
    /// Optional: a request without it means no video.
    #[serde(default)]
    pub video: VideoChoice,
    /// Optional: the slug of the address. It is cleaned, and blank means WordPress makes one.
    #[serde(default)]
    pub slug: String,
    /// Optional: the meta description. Blank means none.
    #[serde(default)]
    pub meta_description: String,
    /// Optional: the focus keyphrase. Blank means none.
    #[serde(default)]
    pub focus_keyphrase: String,
    /// Optional: the ids of the categories. None means WordPress chooses its default.
    #[serde(default)]
    pub categories: Vec<u64>,
}

/// Why the draft could not be created.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PublishError {
    #[error(transparent)]
    Draft(#[from] DraftError),
    #[error(transparent)]
    Video(#[from] VideoError),
    #[error(transparent)]
    Image(#[from] ImageError),
    #[error(transparent)]
    WordPress(#[from] WordPressError),
}

/// Validates and sanitizes the text, checks the video choice, loads the saved settings, resolves
/// the image and creates the draft. Nothing is downloaded or sent before the text, the video and
/// the settings are known good.
pub fn publish_draft<S, H, F>(
    store: &S,
    http: &H,
    fetcher: &F,
    input: DraftInput,
) -> Result<CreatedPost, PublishError>
where
    S: CredentialsStore,
    H: WpHttp,
    F: ImageFetcher,
{
    let draft = PublicationDraft::new(&input.title, &input.html)?
        .with_slug(&input.slug)
        .with_meta_description(&input.meta_description)?
        .with_focus_keyphrase(&input.focus_keyphrase)?
        .with_categories(&input.categories)?;
    let draft = draft.with_video(video::resolve(input.video)?);
    let credentials = store.load()?.ok_or(WordPressError::NoSettings)?;
    let image = resolve(input.image, fetcher)?;
    Ok(create_draft(http, &credentials, &draft.with_image(image))?)
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;
    use crate::http::{Download, TransportError};
    use crate::wordpress::tests::{answer, credentials, FakeSite};

    const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 1, 2, 3];

    #[derive(Default)]
    struct MemoryCredentials(RefCell<Option<crate::wordpress::WpCredentials>>);

    impl CredentialsStore for MemoryCredentials {
        fn load(&self) -> Result<Option<crate::wordpress::WpCredentials>, WordPressError> {
            Ok(self.0.borrow().clone())
        }
        fn save(&self, c: &crate::wordpress::WpCredentials) -> Result<(), WordPressError> {
            *self.0.borrow_mut() = Some(c.clone());
            Ok(())
        }
        fn clear(&self) -> Result<(), WordPressError> {
            *self.0.borrow_mut() = None;
            Ok(())
        }
    }

    struct FakeFetcher {
        status: u16,
        asked: RefCell<usize>,
    }

    impl ImageFetcher for FakeFetcher {
        fn fetch(&self, _url: &str, _max_bytes: usize) -> Result<Download, TransportError> {
            *self.asked.borrow_mut() += 1;
            Ok(Download {
                status: self.status,
                content_type: None,
                bytes: PNG.to_vec(),
            })
        }
    }

    fn fetcher(status: u16) -> FakeFetcher {
        FakeFetcher {
            status,
            asked: RefCell::new(0),
        }
    }

    fn saved() -> MemoryCredentials {
        let store = MemoryCredentials::default();
        store.save(&credentials()).unwrap();
        store
    }

    fn input(image: ImageChoice) -> DraftInput {
        DraftInput {
            title: "Fabada vegana".into(),
            html: "<p>Con fabes</p><script>x</script>".into(),
            image,
            video: VideoChoice::None,
            slug: String::new(),
            meta_description: String::new(),
            focus_keyphrase: String::new(),
            categories: Vec::new(),
        }
    }

    fn reel() -> VideoChoice {
        VideoChoice::InstagramEmbed {
            permalink: "https://www.instagram.com/example.user/reel/Dx1Ab-cdEfG/".into(),
        }
    }

    #[test]
    fn the_slug_and_the_meta_description_reach_wordpress_cleaned() {
        let site = FakeSite::new(vec![answer(201, r#"{"id":5}"#)]);
        let mut seo = input(ImageChoice::None);
        seo.slug = "Fabada 💚 Vegana ñam".into();
        seo.meta_description = "  Fabada  💚\n vegana ".into();
        let post = publish_draft(&saved(), &site, &fetcher(200), seo).unwrap();
        // The fake site did not echo the meta back, so it counts as not kept.
        assert_eq!(post.meta_description_saved, Some(false));

        let sent = site.sent.borrow();
        let body: serde_json::Value = serde_json::from_slice(&sent[0].2).unwrap();
        assert_eq!(body["slug"], "fabada-vegana-nam");
        assert_eq!(body["meta"]["_yoast_wpseo_metadesc"], "Fabada vegana");
        assert!(String::from_utf8_lossy(&sent[0].2)
            .chars()
            .all(|c| c.len_utf8() < 4));
    }

    #[test]
    fn the_focus_keyphrase_reaches_wordpress_cleaned() {
        let site = FakeSite::new(vec![answer(201, r#"{"id":5}"#)]);
        let mut seo = input(ImageChoice::None);
        seo.focus_keyphrase = "  fabada \n vegana 💚 ".into();
        let post = publish_draft(&saved(), &site, &fetcher(200), seo).unwrap();
        assert_eq!(post.focus_keyphrase_saved, Some(false));
        let sent = site.sent.borrow();
        let body: serde_json::Value = serde_json::from_slice(&sent[0].2).unwrap();
        assert_eq!(body["meta"]["_yoast_wpseo_focuskw"], "fabada vegana");
    }

    #[test]
    fn a_focus_keyphrase_that_is_too_long_stops_everything_before_any_work() {
        let site = FakeSite::new(vec![]);
        let mut bad = input(ImageChoice::None);
        bad.focus_keyphrase = "a".repeat(101);
        assert_eq!(
            publish_draft(&saved(), &site, &fetcher(200), bad),
            Err(PublishError::Draft(DraftError::FocusKeyphraseTooLong {
                length: 101,
                max: 100
            }))
        );
        assert!(site.sent.borrow().is_empty());
    }

    #[test]
    fn the_chosen_categories_reach_wordpress() {
        let site = FakeSite::new(vec![answer(201, r#"{"id":5}"#)]);
        let mut with = input(ImageChoice::None);
        with.categories = vec![7, 7, 3];
        publish_draft(&saved(), &site, &fetcher(200), with).unwrap();
        let sent = site.sent.borrow();
        let body: serde_json::Value = serde_json::from_slice(&sent[0].2).unwrap();
        assert_eq!(body["categories"], serde_json::json!([7, 3]));
    }

    #[test]
    fn too_many_categories_stop_everything_before_any_work() {
        let site = FakeSite::new(vec![]);
        let mut bad = input(ImageChoice::None);
        bad.categories = (1..=21).collect();
        assert_eq!(
            publish_draft(&saved(), &site, &fetcher(200), bad),
            Err(PublishError::Draft(DraftError::TooManyCategories {
                count: 21,
                max: 20
            }))
        );
        assert!(site.sent.borrow().is_empty());
    }

    #[test]
    fn a_meta_description_that_is_too_long_stops_everything_before_any_work() {
        let site = FakeSite::new(vec![]);
        let fetcher = fetcher(200);
        let mut bad = input(ImageChoice::PostImage {
            url: "https://scontent.cdninstagram.com/v/1.jpg".into(),
        });
        bad.meta_description = "a".repeat(301);
        assert_eq!(
            publish_draft(&saved(), &site, &fetcher, bad),
            Err(PublishError::Draft(DraftError::MetaDescriptionTooLong {
                length: 301,
                max: 300
            }))
        );
        assert_eq!(*fetcher.asked.borrow(), 0);
        assert!(site.sent.borrow().is_empty());
    }

    #[test]
    fn a_request_without_slug_or_description_is_understood() {
        let input: DraftInput =
            serde_json::from_str(r#"{"title":"T","html":"<p>x</p>","image":{"kind":"none"}}"#)
                .unwrap();
        assert_eq!(input.slug, "");
        assert_eq!(input.meta_description, "");
        assert_eq!(input.focus_keyphrase, "");
        assert!(input.categories.is_empty());
        let with: DraftInput = serde_json::from_str(
            r#"{"title":"T","html":"<p>x</p>","image":{"kind":"none"},
                "slug":"mi-slug","meta_description":"Una descripción"}"#,
        )
        .unwrap();
        assert_eq!(with.slug, "mi-slug");
        assert_eq!(with.meta_description, "Una descripción");
    }

    #[test]
    fn a_reel_player_goes_below_the_text_with_a_link_to_it_at_the_top() {
        let site = FakeSite::new(vec![answer(201, r#"{"id":5}"#)]);
        let mut with_video = input(ImageChoice::None);
        with_video.video = reel();
        publish_draft(&saved(), &site, &fetcher(200), with_video).unwrap();

        let sent = site.sent.borrow();
        let body: serde_json::Value = serde_json::from_slice(&sent[0].2).unwrap();
        let content = body["content"].as_str().unwrap();
        let link = content.find("<a href=\"#video\">Ver el vídeo</a>").unwrap();
        let text = content.find("<p>Con fabes</p>").unwrap();
        let player = content.find("<iframe").unwrap();
        assert!(link < text && text < player, "{content}");
        assert!(content.contains("src=\"https://www.instagram.com/reel/Dx1Ab-cdEfG/embed/\""));
        assert!(content.contains("<div id=\"video\""));
        assert!(content.ends_with("<!-- /wp:html -->"));
    }

    #[test]
    fn a_request_without_video_means_no_player() {
        let input: DraftInput =
            serde_json::from_str(r#"{"title":"T","html":"<p>x</p>","image":{"kind":"none"}}"#)
                .unwrap();
        assert_eq!(input.video, VideoChoice::None);
        let site = FakeSite::new(vec![answer(201, r#"{"id":5}"#)]);
        publish_draft(&saved(), &site, &fetcher(200), input).unwrap();
        let sent = site.sent.borrow();
        assert!(!String::from_utf8_lossy(&sent[0].2).contains("iframe"));
    }

    #[test]
    fn a_video_address_that_is_not_a_post_stops_everything_before_any_work() {
        let site = FakeSite::new(vec![]);
        let fetcher = fetcher(200);
        let mut bad = input(ImageChoice::PostImage {
            url: "https://scontent.cdninstagram.com/v/1.jpg".into(),
        });
        bad.video = VideoChoice::InstagramEmbed {
            permalink: "https://evil.test/reel/x/".into(),
        };
        assert_eq!(
            publish_draft(&saved(), &site, &fetcher, bad),
            Err(PublishError::Video(VideoError::NotAPost(
                "https://evil.test/reel/x/".into()
            )))
        );
        assert_eq!(*fetcher.asked.borrow(), 0);
        assert!(site.sent.borrow().is_empty());
    }

    #[test]
    fn the_video_json_the_frontend_sends_is_understood() {
        let input: DraftInput = serde_json::from_str(
            r#"{"title":"T","html":"<p>x</p>","image":{"kind":"none"},
                "video":{"kind":"instagram_embed","permalink":"https://www.instagram.com/reel/A/"}}"#,
        )
        .unwrap();
        assert_eq!(
            input.video,
            VideoChoice::InstagramEmbed {
                permalink: "https://www.instagram.com/reel/A/".into()
            }
        );
    }

    #[test]
    fn a_draft_without_image_is_created_with_sanitized_text() {
        let site = FakeSite::new(vec![answer(201, r#"{"id":5}"#)]);
        let post = publish_draft(&saved(), &site, &fetcher(200), input(ImageChoice::None)).unwrap();
        assert_eq!(post.id, 5);
        let sent = site.sent.borrow();
        let body: serde_json::Value = serde_json::from_slice(&sent[0].2).unwrap();
        assert_eq!(body["content"], "<p>Con fabes</p>");
    }

    #[test]
    fn nothing_sent_to_wordpress_holds_a_four_byte_character() {
        // A database on the old 3-byte `utf8` charset refuses such a post (`db_insert_error`).
        let site = FakeSite::new(vec![answer(201, r#"{"id":5}"#)]);
        let mut with_emoji = input(ImageChoice::None);
        with_emoji.title = "Aida Lídice 💚 Recetas".into();
        with_emoji.html = "<p>Hola 💚 𝗥𝗲𝗰𝗲𝘁𝗮 🌱</p>".into();
        publish_draft(&saved(), &site, &fetcher(200), with_emoji).unwrap();

        let sent = site.sent.borrow();
        let body = std::str::from_utf8(&sent[0].2).unwrap();
        assert!(body.chars().all(|c| c.len_utf8() < 4), "{body}");
        let json: serde_json::Value = serde_json::from_str(body).unwrap();
        assert_eq!(json["title"], "Aida Lídice Recetas");
        assert_eq!(
            json["content"],
            "<p>Hola &#x1F49A; &#x1D5E5;&#x1D5F2;&#x1D5F0;&#x1D5F2;&#x1D601;&#x1D5EE; &#x1F331;</p>"
        );
    }

    #[test]
    fn the_posts_own_image_is_downloaded_then_uploaded() {
        let site = FakeSite::new(vec![answer(201, r#"{"id":9}"#), answer(201, r#"{"id":5}"#)]);
        let fetcher = fetcher(200);
        let choice = ImageChoice::PostImage {
            url: "https://scontent.cdninstagram.com/v/1.jpg".into(),
        };
        publish_draft(&saved(), &site, &fetcher, input(choice)).unwrap();
        assert_eq!(*fetcher.asked.borrow(), 1);
        assert_eq!(site.sent.borrow().len(), 2);
    }

    #[test]
    fn bad_text_stops_everything_before_any_work() {
        let site = FakeSite::new(vec![]);
        let fetcher = fetcher(200);
        let mut bad = input(ImageChoice::PostImage {
            url: "https://scontent.cdninstagram.com/v/1.jpg".into(),
        });
        bad.title = "  ".into();
        assert_eq!(
            publish_draft(&saved(), &site, &fetcher, bad),
            Err(PublishError::Draft(DraftError::EmptyTitle))
        );
        assert_eq!(*fetcher.asked.borrow(), 0);
        assert!(site.sent.borrow().is_empty());
    }

    #[test]
    fn missing_settings_stop_before_the_image_is_downloaded() {
        let site = FakeSite::new(vec![]);
        let fetcher = fetcher(200);
        let choice = ImageChoice::PostImage {
            url: "https://scontent.cdninstagram.com/v/1.jpg".into(),
        };
        assert_eq!(
            publish_draft(
                &MemoryCredentials::default(),
                &site,
                &fetcher,
                input(choice)
            ),
            Err(PublishError::WordPress(WordPressError::NoSettings))
        );
        assert_eq!(*fetcher.asked.borrow(), 0);
        assert!(site.sent.borrow().is_empty());
    }

    #[test]
    fn an_image_that_cannot_be_had_creates_nothing_on_the_site() {
        let site = FakeSite::new(vec![]);
        let choice = ImageChoice::PostImage {
            url: "https://scontent.cdninstagram.com/v/1.jpg".into(),
        };
        assert_eq!(
            publish_draft(&saved(), &site, &fetcher(404), input(choice)),
            Err(PublishError::Image(ImageError::DownloadStatus(404)))
        );
        assert!(site.sent.borrow().is_empty());
    }

    #[test]
    fn a_wordpress_error_comes_through_unchanged() {
        let site = FakeSite::new(vec![answer(
            401,
            r#"{"code":"incorrect_password","message":"Invalid username or password."}"#,
        )]);
        assert_eq!(
            publish_draft(&saved(), &site, &fetcher(200), input(ImageChoice::None)),
            Err(PublishError::WordPress(WordPressError::Api {
                code: "incorrect_password".into(),
                message: "Invalid username or password.".into()
            }))
        );
    }

    #[test]
    fn the_input_is_read_from_the_json_the_frontend_sends() {
        let input: DraftInput =
            serde_json::from_str(r#"{"title":"T","html":"<p>x</p>","image":{"kind":"none"}}"#)
                .unwrap();
        assert_eq!(input.image, ImageChoice::None);
    }
}
