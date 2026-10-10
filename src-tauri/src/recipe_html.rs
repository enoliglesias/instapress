//! The text of a recipe draft: caption to HTML, a suggested title, and sanitizing.
//!
//! The editor in the webview is a `contenteditable` element, so its HTML is untrusted. Everything
//! that goes to WordPress passes through [`RecipeDraft::new`], which cleans it with an allow-list.

use std::collections::{HashMap, HashSet};

use thiserror::Error;

use crate::image::Image;
use crate::video::VideoEmbed;

const TITLE_SUGGESTION_CHARS: usize = 80;
const MAX_TITLE_CHARS: usize = 200;

const ALLOWED_TAGS: [&str; 14] = [
    "p",
    "br",
    "strong",
    "b",
    "em",
    "i",
    "u",
    "h2",
    "h3",
    "ul",
    "ol",
    "li",
    "blockquote",
    "a",
];
const ALLOWED_SCHEMES: [&str; 3] = ["http", "https", "mailto"];

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DraftError {
    #[error("the title is empty")]
    EmptyTitle,
    #[error("the title has {length} characters; the limit is {max}")]
    TitleTooLong { length: usize, max: usize },
    #[error("the text of the post is empty")]
    EmptyBody,
}

/// Turns a plain-text caption into HTML: a blank line starts a paragraph, a single newline is a
/// line break, and everything special in HTML is escaped.
pub fn caption_to_html(caption: &str) -> String {
    let normalized = caption.replace("\r\n", "\n").replace('\r', "\n");
    let mut paragraphs: Vec<Vec<&str>> = Vec::new();
    let mut current: Vec<&str> = Vec::new();
    for line in normalized.lines() {
        let line = line.trim();
        if line.is_empty() {
            if !current.is_empty() {
                paragraphs.push(std::mem::take(&mut current));
            }
        } else {
            current.push(line);
        }
    }
    if !current.is_empty() {
        paragraphs.push(current);
    }
    paragraphs
        .iter()
        .map(|lines| {
            let body: Vec<String> = lines.iter().map(|line| escape_html(line)).collect();
            format!("<p>{}</p>", body.join("<br>"))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn escape_html(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            other => escaped.push(other),
        }
    }
    escaped
}

/// The first non-blank line of the caption, cut at 80 characters. Empty when there is no text.
pub fn suggest_title(caption: &str) -> String {
    caption
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(|line| {
            line.chars()
                .take(TITLE_SUGGESTION_CHARS)
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .unwrap_or_default()
}

/// Cleans editor HTML with an allow-list: paragraphs, line breaks, emphasis, two heading
/// levels, lists, quotes and links. Scripts, images, styles, classes and event handlers go.
pub fn sanitize_html(html: &str) -> String {
    let tag_attributes: HashMap<&str, HashSet<&str>> =
        HashMap::from([("a", HashSet::from(["href"]))]);
    ammonia::Builder::default()
        .tags(ALLOWED_TAGS.into_iter().collect())
        .tag_attributes(tag_attributes)
        .generic_attributes(HashSet::new())
        .url_schemes(ALLOWED_SCHEMES.into_iter().collect())
        .url_relative(ammonia::UrlRelative::Deny)
        .link_rel(Some("noopener noreferrer"))
        .clean(html)
        .to_string()
}

/// Whether a character needs four bytes in UTF-8: emoji and every other character beyond U+FFFF.
/// A WordPress database still on the old 3-byte `utf8` charset cannot store them and refuses the
/// whole post (`db_insert_error`).
fn is_four_byte(c: char) -> bool {
    c.len_utf8() == 4
}

/// Writes each four-byte character as a numeric HTML entity (`💚` becomes `&#x1F49A;`). The
/// entity is plain ASCII, so any database stores it, and a browser shows the original character.
fn encode_four_byte_chars(html: &str) -> String {
    let mut encoded = String::with_capacity(html.len());
    for c in html.chars() {
        if is_four_byte(c) {
            encoded.push_str(&format!("&#x{:X};", u32::from(c)));
        } else {
            encoded.push(c);
        }
    }
    encoded
}

/// Whether `html` shows any text once all markup is removed. A no-break space counts as blank:
/// the serializer writes it as `&nbsp;`, which `trim` alone would take for text.
fn has_visible_text(html: &str) -> bool {
    !ammonia::Builder::empty()
        .clean(html)
        .to_string()
        .replace("&nbsp;", " ")
        .trim()
        .is_empty()
}

/// A validated draft, ready to send: a plain-text title, sanitized HTML, an optional image and an
/// optional Instagram player to show below the text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecipeDraft {
    title: String,
    html: String,
    image: Option<Image>,
    video: Option<VideoEmbed>,
}

impl RecipeDraft {
    /// Collapses the title to one line, sanitizes the body and rejects an empty title or text.
    ///
    /// Four-byte characters (emoji) are removed from the title and written as HTML entities in
    /// the body, so the post can be stored even by a database that only has 3-byte `utf8`. The
    /// title cannot use entities: the block editor would show them as literal text.
    pub fn new(title: &str, html: &str) -> Result<Self, DraftError> {
        let title = title
            .chars()
            .filter(|&c| !is_four_byte(c))
            .collect::<String>()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        if title.is_empty() {
            return Err(DraftError::EmptyTitle);
        }
        let length = title.chars().count();
        if length > MAX_TITLE_CHARS {
            return Err(DraftError::TitleTooLong {
                length,
                max: MAX_TITLE_CHARS,
            });
        }
        let html = sanitize_html(html);
        if !has_visible_text(&html) {
            return Err(DraftError::EmptyBody);
        }
        Ok(Self {
            title,
            html: encode_four_byte_chars(&html),
            image: None,
            video: None,
        })
    }

    /// Attaches the featured image, or removes it with `None`.
    pub fn with_image(mut self, image: Option<Image>) -> Self {
        self.image = image;
        self
    }

    /// Puts an Instagram player below the text, with a link to it at the top, or removes both
    /// with `None`.
    pub fn with_video(mut self, video: Option<VideoEmbed>) -> Self {
        self.video = video;
        self
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    /// The sanitized body, without the player.
    pub fn html(&self) -> &str {
        &self.html
    }

    /// What goes into the post. With a video: the link that jumps to it, the body and then the
    /// player, separated by blank lines. Without one: the body alone. The link and the player are
    /// added after sanitizing because the sanitizer would remove the iframe.
    pub fn content(&self) -> String {
        match &self.video {
            Some(video) => format!("{}\n\n{}\n\n{}", video.link_html(), self.html, video.html()),
            None => self.html.clone(),
        }
    }

    pub fn video(&self) -> Option<&VideoEmbed> {
        self.video.as_ref()
    }

    pub fn image(&self) -> Option<&Image> {
        self.image.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_blank_line_starts_a_paragraph_and_a_newline_is_a_break() {
        let html = caption_to_html("Fabada vegana\nCon fabes\n\nPaso 1\n\n\n\nPaso 2");
        assert_eq!(
            html,
            "<p>Fabada vegana<br>Con fabes</p>\n<p>Paso 1</p>\n<p>Paso 2</p>"
        );
    }

    #[test]
    fn caption_text_is_escaped() {
        assert_eq!(
            caption_to_html("1 < 2 & \"a\" > 'b' <script>alert(1)</script>"),
            "<p>1 &lt; 2 &amp; &quot;a&quot; &gt; &#39;b&#39; &lt;script&gt;alert(1)&lt;/script&gt;</p>"
        );
    }

    #[test]
    fn windows_and_old_mac_line_endings_work() {
        assert_eq!(
            caption_to_html("a\r\nb\r\n\r\nc\rd"),
            "<p>a<br>b</p>\n<p>c<br>d</p>"
        );
    }

    #[test]
    fn an_empty_or_blank_caption_gives_no_html() {
        assert_eq!(caption_to_html(""), "");
        assert_eq!(caption_to_html("  \n \r\n\t"), "");
    }

    #[test]
    fn the_title_is_the_first_non_blank_line() {
        assert_eq!(
            suggest_title("\n  \n  Tarta de queso  \nresto"),
            "Tarta de queso"
        );
        assert_eq!(suggest_title("   \n"), "");
        assert_eq!(suggest_title(""), "");
    }

    #[test]
    fn the_suggested_title_is_cut_on_a_character_boundary() {
        let title = suggest_title(&"ñ".repeat(120));
        assert_eq!(title.chars().count(), 80);
        assert!(title.chars().all(|c| c == 'ñ'));
    }

    #[test]
    fn sanitizing_keeps_the_allowed_formatting() {
        let html = "<h2>Pasos</h2><p>Mezcla <strong>bien</strong> y <em>espera</em>.</p>\
                    <ul><li>uno</li></ul><blockquote>nota</blockquote>";
        assert_eq!(sanitize_html(html), html);
    }

    #[test]
    fn sanitizing_removes_scripts_handlers_images_and_styles() {
        let cleaned = sanitize_html(
            "<p style=\"color:red\" class=\"x\" onclick=\"evil()\">hola</p>\
             <script>alert(1)</script><img src=\"https://x.test/a.png\" onerror=\"evil()\">\
             <style>p{}</style><iframe src=\"https://x.test\"></iframe>",
        );
        assert_eq!(cleaned, "<p>hola</p>");
    }

    #[test]
    fn links_keep_only_safe_schemes_and_get_a_rel() {
        let cleaned = sanitize_html(
            "<a href=\"https://x.test/a\" target=\"_blank\" onclick=\"evil()\">ok</a>\
             <a href=\"javascript:alert(1)\">bad</a><a href=\"/relative\">rel</a>\
             <a href=\"data:text/html,x\">data</a>",
        );
        assert!(cleaned.contains("<a href=\"https://x.test/a\" rel=\"noopener noreferrer\">ok</a>"));
        assert!(!cleaned.contains("javascript:"));
        assert!(!cleaned.contains("data:"));
        assert!(!cleaned.contains("/relative"));
        assert!(!cleaned.contains("onclick"));
        assert!(!cleaned.contains("target"));
    }

    #[test]
    fn four_byte_characters_become_numeric_entities() {
        assert_eq!(
            encode_four_byte_chars("Hola 💚 y 🌱"),
            "Hola &#x1F49A; y &#x1F331;"
        );
        assert_eq!(encode_four_byte_chars("𝗥𝗲"), "&#x1D5E5;&#x1D5F2;");
        assert_eq!(encode_four_byte_chars("😀"), "&#x1F600;");
    }

    #[test]
    fn other_text_is_left_alone() {
        for text in [
            "",
            "plain text",
            "ñandú áéíóú ç € ™",
            "&amp; &#x1F49A; &lt;",
            "日本語",
            "a\u{200d}b\u{fe0f}",
        ] {
            assert_eq!(encode_four_byte_chars(text), text);
        }
    }

    #[test]
    fn a_joined_emoji_keeps_its_invisible_joiner() {
        // The joiner and the variation selector have three bytes, so they stay as they are.
        assert_eq!(
            encode_four_byte_chars("👩\u{200d}🍳"),
            "&#x1F469;\u{200d}&#x1F373;"
        );
    }

    #[test]
    fn a_draft_never_holds_a_four_byte_character() {
        let draft = RecipeDraft::new(
            "Aida Lídice 💚🌱 Recetas 𝗥𝗲𝗰𝗲𝘁𝗮",
            "<p>Hola 💚</p><p><strong>Paso 1 🌱</strong></p><a href=\"https://x.test/😀\">x</a>",
        )
        .unwrap();
        assert!(
            !draft.title().chars().any(is_four_byte),
            "{}",
            draft.title()
        );
        assert!(!draft.html().chars().any(is_four_byte), "{}", draft.html());
        assert!(draft.html().contains("Hola &#x1F49A;"));
        assert!(draft.html().contains("<strong>Paso 1 &#x1F331;</strong>"));
    }

    #[test]
    fn emoji_in_the_title_are_removed_and_the_spaces_collapse() {
        let draft = RecipeDraft::new("Aida Lídice 💚 🌱 Recetas", "<p>Hola</p>").unwrap();
        assert_eq!(draft.title(), "Aida Lídice Recetas");
        let draft = RecipeDraft::new("💚Tarta de queso🌱", "<p>Hola</p>").unwrap();
        assert_eq!(draft.title(), "Tarta de queso");
    }

    #[test]
    fn a_title_that_is_only_emoji_is_empty() {
        assert_eq!(
            RecipeDraft::new("💚🌱", "<p>Hola</p>"),
            Err(DraftError::EmptyTitle)
        );
    }

    #[test]
    fn a_body_that_is_only_an_emoji_still_counts_as_text() {
        let draft = RecipeDraft::new("Titulo", "<p>💚</p>").unwrap();
        assert_eq!(draft.html(), "<p>&#x1F49A;</p>");
    }

    #[test]
    fn the_title_limit_counts_what_is_sent() {
        let title = format!("{}💚", "a".repeat(200));
        assert!(RecipeDraft::new(&title, "<p>Hola</p>").is_ok());
        let title = format!("{}💚", "a".repeat(201));
        assert!(matches!(
            RecipeDraft::new(&title, "<p>Hola</p>"),
            Err(DraftError::TitleTooLong { length: 201, .. })
        ));
    }

    #[test]
    fn without_a_player_the_content_is_the_body() {
        let draft = RecipeDraft::new("Titulo", "<p>Hola</p>").unwrap();
        assert_eq!(draft.content(), "<p>Hola</p>");
        assert!(draft.video().is_none());
    }

    #[test]
    fn the_link_comes_first_then_the_body_and_the_player_last() {
        let embed = VideoEmbed::from_permalink("https://www.instagram.com/reel/DJrCx/").unwrap();
        let draft = RecipeDraft::new("Titulo", "<p>Hola 💚</p>")
            .unwrap()
            .with_video(Some(embed.clone()));
        let content = draft.content();
        assert_eq!(
            content,
            format!(
                "{}\n\n<p>Hola &#x1F49A;</p>\n\n{}",
                embed.link_html(),
                embed.html()
            )
        );
        let link = content.find("href=\"#video\"").unwrap();
        let text = content.find("<p>Hola").unwrap();
        let player = content.find("<iframe").unwrap();
        assert!(link < text && text < player, "{content}");
        // The editable body stays clean of the link and the player.
        assert_eq!(draft.html(), "<p>Hola &#x1F49A;</p>");
        assert_eq!(draft.video(), Some(&embed));
        assert_eq!(draft.with_video(None).content(), "<p>Hola &#x1F49A;</p>");
    }

    #[test]
    fn an_iframe_in_the_editor_is_still_removed() {
        let draft = RecipeDraft::new(
            "Titulo",
            "<p>Hola</p><iframe src=\"https://evil.test\"></iframe>",
        )
        .unwrap();
        assert!(!draft.content().contains("iframe"));
    }

    #[test]
    fn a_visible_entity_is_still_text() {
        assert!(RecipeDraft::new("Titulo", "<p>&amp;</p>").is_ok());
    }

    #[test]
    fn disguised_script_schemes_are_removed() {
        for href in [
            "JaVaScRiPt:alert(1)",
            "jav&#x61;script:alert(1)",
            "jav&#97;script:alert(1)",
            "java&Tab;script:alert(1)",
            "java\tscript:alert(1)",
            "java\nscript:alert(1)",
            " javascript:alert(1)",
            "vbscript:msgbox(1)",
            "data:text/html;base64,PHNjcmlwdD4=",
            "//evil.test/x",
        ] {
            let cleaned = sanitize_html(&format!("<a href=\"{href}\">x</a>"));
            assert!(
                !cleaned.to_lowercase().contains("script"),
                "{href}: {cleaned}"
            );
            assert!(!cleaned.contains("href"), "{href}: {cleaned}");
        }
    }

    #[test]
    fn mailto_and_plain_http_links_are_kept() {
        let cleaned = sanitize_html(
            "<a href=\"mailto:aida@recetas.test\">mail</a><a href=\"http://x.test/\">http</a>",
        );
        assert!(cleaned.contains("href=\"mailto:aida@recetas.test\""));
        assert!(cleaned.contains("href=\"http://x.test/\""));
    }

    #[test]
    fn foreign_content_and_forms_leave_no_active_markup() {
        for html in [
            "<svg><script>alert(1)</script></svg>",
            "<svg onload=alert(1)><circle/></svg>",
            "<math><mi xlink:href=\"javascript:alert(1)\">x</mi></math>",
            "<noscript><p title=\"</noscript><img src=x onerror=alert(1)>\"></noscript>",
            "<form action=\"https://evil.test\"><input name=a><button>go</button></form>",
            "<base href=\"https://evil.test/\">",
            "<object data=\"x\"></object><embed src=\"x\">",
            "<p><svg><style><img src=x onerror=alert(1)></style></svg></p>",
            "<meta http-equiv=\"refresh\" content=\"0;url=https://evil.test\">",
        ] {
            let cleaned = sanitize_html(html).to_lowercase();
            for bad in [
                "<script",
                "<svg",
                "<math",
                "<form",
                "<input",
                "<base",
                "<object",
                "<embed",
                "<meta",
                "<img",
                "onerror",
                "onload",
                "javascript:",
            ] {
                assert!(!cleaned.contains(bad), "{html} -> {cleaned}");
            }
        }
    }

    #[test]
    fn sanitizing_twice_changes_nothing() {
        for html in [
            "<p>Hola <strong>mundo</strong></p>",
            "<a href=\"https://x.test/\">x</a><script>x</script>",
            "<ul><li>a<li>b</ul><h2>t</h2>",
            "<p>1 &lt; 2 &amp; 3</p>",
            "<p onclick=\"x()\">a<br>b",
        ] {
            let once = sanitize_html(html);
            assert_eq!(sanitize_html(&once), once, "{html}");
        }
    }

    #[test]
    fn a_draft_cleans_the_body_and_collapses_the_title() {
        let draft =
            RecipeDraft::new("  Tarta \n de   queso ", "<p>Hola</p><script>x</script>").unwrap();
        assert_eq!(draft.title(), "Tarta de queso");
        assert_eq!(draft.html(), "<p>Hola</p>");
        assert!(draft.image().is_none());
    }

    #[test]
    fn a_draft_needs_a_title() {
        assert_eq!(
            RecipeDraft::new(" \n ", "<p>Hola</p>"),
            Err(DraftError::EmptyTitle)
        );
    }

    #[test]
    fn a_draft_rejects_a_title_over_the_limit() {
        assert_eq!(
            RecipeDraft::new(&"a".repeat(201), "<p>Hola</p>"),
            Err(DraftError::TitleTooLong {
                length: 201,
                max: 200
            })
        );
        assert!(RecipeDraft::new(&"a".repeat(200), "<p>Hola</p>").is_ok());
    }

    #[test]
    fn a_draft_needs_visible_text() {
        for html in [
            "",
            "<p><br></p>",
            "<script>x</script>",
            "<p>  </p>",
            "<p>&nbsp;</p>",
            "<p>&#160; &#xA0;</p>",
            "<p>\u{a0}</p>",
        ] {
            assert_eq!(
                RecipeDraft::new("Titulo", html),
                Err(DraftError::EmptyBody),
                "{html:?}"
            );
        }
    }
}
