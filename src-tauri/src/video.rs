//! The Instagram player that can go below the text of a draft, and the link that jumps to it.
//!
//! The player is Instagram's own embed in an `<iframe>`; nothing is downloaded or stored on the
//! site. The address the window sends is not trusted: it is parsed, only a post or a reel is
//! accepted, and the iframe address is built from the validated shortcode alone, so no text from
//! the window ever reaches the markup.

/// The `id` of the section that holds the player, and the target of the "watch the video" link.
const SECTION_ID: &str = "video";
/// The text of the link at the top of the post. The site is in Spanish, so this is too.
const LINK_TEXT: &str = "Ver el vídeo";

use serde::Deserialize;
use thiserror::Error;

use crate::instagram_url::{self, InstagramTarget};

/// Width of the player on the page, in pixels. Instagram's player is never wider than this.
const PLAYER_WIDTH: u32 = 400;
/// Height sized for a vertical reel at that width. Instagram resizes its player only with a
/// script that the app does not add, so the height is fixed.
const PLAYER_HEIGHT: u32 = 880;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum VideoError {
    #[error("`{0}` is not the address of an Instagram post or reel")]
    NotAPost(String),
}

/// What the user chose for the video.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum VideoChoice {
    /// No video in the post.
    #[default]
    None,
    /// Instagram's player for the post at `permalink`.
    InstagramEmbed { permalink: String },
}

/// A validated player address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoEmbed {
    src: String,
}

impl VideoEmbed {
    /// Accepts the address of a post (`/p/<code>`) or a reel (`/reel/<code>`, also under a user
    /// name). A profile or anything else is refused.
    pub fn from_permalink(permalink: &str) -> Result<Self, VideoError> {
        let path = match instagram_url::parse(permalink) {
            Ok(InstagramTarget::Post { shortcode }) => format!("p/{shortcode}"),
            Ok(InstagramTarget::Reel { shortcode }) => format!("reel/{shortcode}"),
            _ => return Err(VideoError::NotAPost(permalink.to_owned())),
        };
        Ok(Self {
            src: format!("https://www.instagram.com/{path}/embed/"),
        })
    }

    /// The address of the iframe.
    pub fn src(&self) -> &str {
        &self.src
    }

    /// The link for the top of the post, a `wp:paragraph` block that jumps to the player.
    pub fn link_html(&self) -> String {
        format!(
            "<!-- wp:paragraph -->\n\
             <p><a href=\"#{SECTION_ID}\">{LINK_TEXT}</a></p>\n\
             <!-- /wp:paragraph -->"
        )
    }

    /// The markup for below the text: a `wp:html` block with the player in a container that is
    /// the target of [`link_html`](Self::link_html).
    pub fn html(&self) -> String {
        format!(
            "<!-- wp:html -->\n\
             <div id=\"{SECTION_ID}\" style=\"max-width:{PLAYER_WIDTH}px;margin:1.5em auto 0\">\
             <iframe src=\"{}\" title=\"Instagram video\" width=\"{PLAYER_WIDTH}\" \
             height=\"{PLAYER_HEIGHT}\" style=\"width:100%;border:0\" loading=\"lazy\" \
             allowfullscreen></iframe></div>\n\
             <!-- /wp:html -->",
            self.src
        )
    }
}

/// Turns the user's choice into a player, if any.
pub fn resolve(choice: VideoChoice) -> Result<Option<VideoEmbed>, VideoError> {
    match choice {
        VideoChoice::None => Ok(None),
        VideoChoice::InstagramEmbed { permalink } => {
            VideoEmbed::from_permalink(&permalink).map(Some)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reel_gets_a_reel_player() {
        let embed =
            VideoEmbed::from_permalink("https://www.instagram.com/reel/Dx1Ab-cdEfG/").unwrap();
        assert_eq!(
            embed.src(),
            "https://www.instagram.com/reel/Dx1Ab-cdEfG/embed/"
        );
    }

    #[test]
    fn a_post_gets_a_post_player() {
        let embed = VideoEmbed::from_permalink("https://www.instagram.com/p/AbC_123/").unwrap();
        assert_eq!(embed.src(), "https://www.instagram.com/p/AbC_123/embed/");
    }

    #[test]
    fn the_permalink_under_a_user_name_works_and_always_leaves_through_the_main_host() {
        let embed = VideoEmbed::from_permalink(
            "https://m.instagram.com/example.user/reel/Dx1Ab-cdEfG/?igsh=abc#top",
        )
        .unwrap();
        assert_eq!(
            embed.src(),
            "https://www.instagram.com/reel/Dx1Ab-cdEfG/embed/"
        );
    }

    #[test]
    fn anything_that_is_not_a_post_or_a_reel_is_refused() {
        for permalink in [
            "",
            "https://www.instagram.com/example.user/",
            "https://www.instagram.com/explore/",
            "https://evil.test/reel/Dx1Ab/",
            "https://www.instagram.com.evil.test/reel/Dx1Ab/",
            "javascript:alert(1)",
            "https://www.instagram.com/reel/\"><script>alert(1)</script>/",
            "https://www.instagram.com/reel/a b/",
        ] {
            assert_eq!(
                VideoEmbed::from_permalink(permalink),
                Err(VideoError::NotAPost(permalink.to_owned())),
                "{permalink:?}"
            );
        }
    }

    #[test]
    fn the_markup_holds_only_the_validated_address() {
        let html = VideoEmbed::from_permalink("https://www.instagram.com/reel/Dx1Ab-cdEfG/?x=\"y")
            .unwrap()
            .html();
        assert!(html.starts_with("<!-- wp:html -->\n"));
        assert!(html.ends_with("\n<!-- /wp:html -->"));
        assert_eq!(html.matches("<iframe").count(), 1);
        assert!(html.contains("src=\"https://www.instagram.com/reel/Dx1Ab-cdEfG/embed/\""));
        assert!(html.contains("max-width:400px"));
        assert!(html.contains("height=\"880\""));
        assert!(html.contains("<div id=\"video\""));
        assert!(!html.contains("script"));
        assert!(!html.contains("?x="));
    }

    #[test]
    fn the_link_jumps_to_the_section_that_holds_the_player() {
        let embed = VideoEmbed::from_permalink("https://www.instagram.com/reel/Dx1Ab/").unwrap();
        let link = embed.link_html();
        assert!(link.contains("<a href=\"#video\">Ver el vídeo</a>"));
        assert!(link.starts_with("<!-- wp:paragraph -->"));
        assert!(embed.html().contains("id=\"video\""));
        assert!(!link.contains("iframe"));
    }

    #[test]
    fn no_choice_gives_no_player() {
        assert_eq!(resolve(VideoChoice::None), Ok(None));
    }

    #[test]
    fn an_embed_choice_is_validated() {
        let ok = resolve(VideoChoice::InstagramEmbed {
            permalink: "https://www.instagram.com/reel/Dx1Ab/".into(),
        })
        .unwrap();
        assert_eq!(
            ok.unwrap().src(),
            "https://www.instagram.com/reel/Dx1Ab/embed/"
        );
        assert_eq!(
            resolve(VideoChoice::InstagramEmbed {
                permalink: "https://evil.test/".into()
            }),
            Err(VideoError::NotAPost("https://evil.test/".into()))
        );
    }

    #[test]
    fn the_choice_is_read_from_the_json_the_frontend_sends() {
        let none: VideoChoice = serde_json::from_str(r#"{"kind":"none"}"#).unwrap();
        let embed: VideoChoice =
            serde_json::from_str(r#"{"kind":"instagram_embed","permalink":"https://x"}"#).unwrap();
        assert_eq!(none, VideoChoice::None);
        assert_eq!(
            embed,
            VideoChoice::InstagramEmbed {
                permalink: "https://x".into()
            }
        );
        assert_eq!(VideoChoice::default(), VideoChoice::None);
    }
}
