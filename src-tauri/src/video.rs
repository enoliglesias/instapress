//! The Instagram player that can go below the text of a draft, and the link that jumps to it.
//!
//! The player is Instagram's own embed, the markup that Instagram gives under "Embed" in a post:
//! a `blockquote` and the script `embed.js`, which turns it into the player and sizes it. Nothing
//! is downloaded or stored on the site. The address the window sends is not trusted: it is parsed,
//! only a post or a reel is accepted, and the markup is built from the validated shortcode alone,
//! so no text from the window ever reaches it.

/// The `id` of the section that holds the player, and the target of the "watch the video" link.
const SECTION_ID: &str = "video";
/// The text of the link at the top of the post. The site is in Spanish, so this is too.
const LINK_TEXT: &str = "Ver el vídeo";

use serde::Deserialize;
use thiserror::Error;

use crate::instagram_url::{self, InstagramTarget};

/// Width of the player on the page, in pixels. Instagram's player is never wider than this.
const PLAYER_WIDTH: u32 = 540;
/// Version of Instagram's embed that the `blockquote` asks the script for.
const EMBED_VERSION: u32 = 14;
/// The script of the embed. Without the script the visitor sees only the link inside the
/// `blockquote`; with it the player plays the video on the page.
const EMBED_SCRIPT: &str = "https://www.instagram.com/embed.js";
/// The text of the link that shows until the script has made the player. The site is in Spanish.
const FALLBACK_TEXT: &str = "Ver esta publicación en Instagram";

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
    permalink: String,
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
            permalink: format!("https://www.instagram.com/{path}/"),
        })
    }

    /// The canonical address of the post or reel, built from the validated shortcode.
    pub fn permalink(&self) -> &str {
        &self.permalink
    }

    /// The link for the top of the post, a `wp:paragraph` block that jumps to the player.
    pub fn link_html(&self) -> String {
        format!(
            "<!-- wp:paragraph -->\n\
             <p><a href=\"#{SECTION_ID}\">{LINK_TEXT}</a></p>\n\
             <!-- /wp:paragraph -->"
        )
    }

    /// The markup for below the text: a `wp:html` block with Instagram's embed in a container that
    /// is the target of [`link_html`](Self::link_html). Instagram's script replaces the
    /// `blockquote` with the player and gives it the right height.
    pub fn html(&self) -> String {
        let link = format!(
            "{}?utm_source=ig_embed&amp;utm_campaign=loading",
            self.permalink
        );
        format!(
            "<!-- wp:html -->\n\
             <div id=\"{SECTION_ID}\" style=\"max-width:{PLAYER_WIDTH}px;margin:1.5em auto 0\">\
             <blockquote class=\"instagram-media\" data-instgrm-permalink=\"{link}\" \
             data-instgrm-version=\"{EMBED_VERSION}\" style=\"background:#fff;border:0;\
             border-radius:3px;box-shadow:0 0 1px 0 rgba(0,0,0,.5),0 1px 10px 0 rgba(0,0,0,.15);\
             margin:1px;max-width:540px;min-width:326px;padding:0;width:calc(100% - 2px)\">\
             <div style=\"padding:16px\"><a href=\"{link}\" style=\"color:#3897f0;\
             font-family:Arial,sans-serif;font-size:14px;text-decoration:none\" \
             target=\"_blank\" rel=\"noopener\">{FALLBACK_TEXT}</a></div></blockquote>\
             <script async src=\"{EMBED_SCRIPT}\"></script></div>\n\
             <!-- /wp:html -->"
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
            embed.permalink(),
            "https://www.instagram.com/reel/Dx1Ab-cdEfG/"
        );
    }

    #[test]
    fn a_post_gets_a_post_player() {
        let embed = VideoEmbed::from_permalink("https://www.instagram.com/p/AbC_123/").unwrap();
        assert_eq!(embed.permalink(), "https://www.instagram.com/p/AbC_123/");
    }

    #[test]
    fn the_permalink_under_a_user_name_works_and_always_leaves_through_the_main_host() {
        let embed = VideoEmbed::from_permalink(
            "https://m.instagram.com/example.user/reel/Dx1Ab-cdEfG/?igsh=abc#top",
        )
        .unwrap();
        assert_eq!(
            embed.permalink(),
            "https://www.instagram.com/reel/Dx1Ab-cdEfG/"
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
        assert_eq!(html.matches("<blockquote").count(), 1);
        assert_eq!(html.matches("<script").count(), 1);
        assert!(!html.contains("<iframe"));
        assert!(html.contains(
            "data-instgrm-permalink=\"https://www.instagram.com/reel/Dx1Ab-cdEfG/?utm_source=ig_embed&amp;utm_campaign=loading\""
        ));
        assert!(html.contains("data-instgrm-version=\"14\""));
        assert!(html.contains("<script async src=\"https://www.instagram.com/embed.js\"></script>"));
        assert!(html.contains("<div id=\"video\" style=\"max-width:540px"));
        assert!(html.contains("<div id=\"video\""));
        assert!(!html.contains("?x="));
    }

    #[test]
    fn the_link_jumps_to_the_section_that_holds_the_player() {
        let embed = VideoEmbed::from_permalink("https://www.instagram.com/reel/Dx1Ab/").unwrap();
        let link = embed.link_html();
        assert!(link.contains("<a href=\"#video\">Ver el vídeo</a>"));
        assert!(!link.contains("script"));
        assert!(link.starts_with("<!-- wp:paragraph -->"));
        assert!(embed.html().contains("id=\"video\""));
        assert!(!link.contains("blockquote"));
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
            ok.unwrap().permalink(),
            "https://www.instagram.com/reel/Dx1Ab/"
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
