//! The text of a publication draft: caption to HTML, a suggested title, and sanitizing.
//!
//! The editor in the webview is a `contenteditable` element, so its HTML is untrusted. Everything
//! that goes to WordPress passes through [`PublicationDraft::new`], which cleans it with an allow-list.

use std::collections::{HashMap, HashSet};

use serde::Serialize;
use thiserror::Error;

use crate::image::Image;
use crate::video::VideoEmbed;

const TITLE_SUGGESTION_CHARS: usize = 80;
const MAX_TITLE_CHARS: usize = 200;
const MAX_SLUG_CHARS: usize = 100;
/// Search engines show about this much of a meta description.
const META_DESCRIPTION_SUGGESTION_CHARS: usize = 155;
const MAX_META_DESCRIPTION_CHARS: usize = 300;
/// A focus keyphrase is a few words; this leaves room for a long one.
const MAX_FOCUS_KEYPHRASE_CHARS: usize = 100;
/// More categories than this on one post is a mistake.
const MAX_CATEGORIES: usize = 20;

/// What the editor can produce and WordPress keeps for any user who may write posts (they are
/// all in WordPress's own list of tags allowed in post content). The title of the post is the
/// page's `h1`, so the headings start at `h2`. There are no images (they would need uploading),
/// no `div` or `span` (they only carry styles) and nothing active.
const ALLOWED_TAGS: [&str; 41] = [
    "p",
    "br",
    "hr",
    "strong",
    "b",
    "em",
    "i",
    "u",
    "s",
    "strike",
    "del",
    "ins",
    "sup",
    "sub",
    "mark",
    "small",
    "code",
    "kbd",
    "q",
    "cite",
    "abbr",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "ul",
    "ol",
    "li",
    "blockquote",
    "pre",
    "a",
    "table",
    "caption",
    "thead",
    "tbody",
    "tfoot",
    "tr",
    "th",
    "td",
    "colgroup",
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
    #[error("the meta description has {length} characters; the limit is {max}")]
    MetaDescriptionTooLong { length: usize, max: usize },
    #[error("the focus keyphrase has {length} characters; the limit is {max}")]
    FocusKeyphraseTooLong { length: usize, max: usize },
    #[error("{count} categories are chosen; the limit is {max}")]
    TooManyCategories { count: usize, max: usize },
}

/// The titles of the sections of a publication, as they are written once accents and case are folded
/// away. Where one starts with another the longer one comes first.
const SECTION_HEADINGS: [&str; 15] = [
    "lista de ingredientes",
    "ingredientes necesarios",
    "los ingredientes",
    "ingredientes",
    "modo de preparacion",
    "modo de elaboracion",
    "preparacion",
    "elaboracion",
    "paso a paso",
    "pasos a seguir",
    "pasos",
    "como se hace",
    "como hacerlo",
    "instrucciones",
    "procedimiento",
];
/// A heading may carry a short qualifier ("para 4 personas"); a longer one is a sentence.
const MAX_QUALIFIER_WORDS: usize = 6;

/// Lower-case text without the accents of Spanish, so `Elaboración` and `ELABORACION` compare
/// equal.
fn fold(text: &str) -> String {
    text.chars()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'á' => 'a',
            'é' => 'e',
            'í' => 'i',
            'ó' => 'o',
            'ú' | 'ü' => 'u',
            other => other,
        })
        .collect()
}

/// Whether what follows the title of a section is only a short qualifier: nothing, `para 4
/// personas` or `(4 personas)`. Anything else, such as `: 200 g de harina`, means the line also
/// carries the content of the section.
fn is_heading_qualifier(rest: &str) -> bool {
    if rest.is_empty() {
        return true;
    }
    // The title has to end there: `ingredientesx` is not `ingredientes`.
    let Some(qualifier) = rest
        .strip_prefix(' ')
        .or_else(|| rest.starts_with('(').then_some(rest))
    else {
        return false;
    };
    let short = |words: &str| {
        !words.contains([':', ',']) && words.split_whitespace().count() <= MAX_QUALIFIER_WORDS
    };
    match qualifier.strip_prefix('(') {
        Some(inner) => inner.strip_suffix(')').is_some_and(short),
        None => qualifier.strip_prefix("para ").is_some_and(short),
    }
}

/// The text of `line` when it only names a section of the publication, such as `Ingredientes`,
/// `🥕 INGREDIENTES para 3 personas:` or `Elaboración`; `None` for any other line.
///
/// Emoji, punctuation and other decoration around the title are dropped. A line that goes on
/// with the content (`Ingredientes: 200 g de harina`) and a hashtag are not headings.
fn section_heading(line: &str) -> Option<String> {
    let line = line.trim();
    if line.starts_with('#') {
        return None;
    }
    let text = line.trim_matches(|c: char| !c.is_alphanumeric() && c != '(' && c != ')');
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let folded = fold(&text);
    SECTION_HEADINGS
        .iter()
        .any(|keyword| {
            folded
                .strip_prefix(keyword)
                .is_some_and(is_heading_qualifier)
        })
        .then_some(text)
}

/// A block of the editor: a paragraph made of lines, or the title of a section.
enum Block<'a> {
    Paragraph(Vec<&'a str>),
    Heading(String),
}

fn close_paragraph<'a>(blocks: &mut Vec<Block<'a>>, lines: &mut Vec<&'a str>) {
    if !lines.is_empty() {
        blocks.push(Block::Paragraph(std::mem::take(lines)));
    }
}

/// Splits text with `\n` line endings into paragraphs and section headings.
fn parse_blocks(text: &str) -> Vec<Block<'_>> {
    let mut blocks: Vec<Block> = Vec::new();
    let mut current: Vec<&str> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            close_paragraph(&mut blocks, &mut current);
        } else if let Some(heading) = section_heading(line) {
            close_paragraph(&mut blocks, &mut current);
            blocks.push(Block::Heading(heading));
        } else {
            current.push(line);
        }
    }
    close_paragraph(&mut blocks, &mut current);
    blocks
}

/// Turns a plain-text caption into HTML: a blank line starts a paragraph, a single newline is a
/// line break, a line that names a section of the publication (`Ingredientes`, `Elaboración`...)
/// becomes an `<h2>`, and everything special in HTML is escaped.
pub fn caption_to_html(caption: &str) -> String {
    let normalized = caption.replace("\r\n", "\n").replace('\r', "\n");
    parse_blocks(&normalized)
        .iter()
        .map(|block| match block {
            Block::Paragraph(lines) => {
                let body: Vec<String> = lines.iter().map(|line| escape_html(line)).collect();
                format!("<p>{}</p>", body.join("<br>"))
            }
            Block::Heading(text) => format!("<h2>{}</h2>", escape_html(text)),
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

/// Turns a title into the slug of its address: lower-case ASCII letters and digits separated by
/// single hyphens. Accents are folded (`Cañas` becomes `canas`); emoji, symbols, punctuation and
/// letters of other alphabets are separators, so none of them reaches the address. The result is
/// cut at 100 characters and is empty when nothing usable is left.
pub fn slugify(text: &str) -> String {
    let mut slug = String::new();
    let mut pending_hyphen = false;
    for c in text.chars() {
        let folded = fold_letter(c);
        if folded.is_empty() {
            pending_hyphen = !slug.is_empty();
            continue;
        }
        if pending_hyphen {
            slug.push('-');
            pending_hyphen = false;
        }
        slug.push_str(folded);
    }
    slug.truncate(MAX_SLUG_CHARS);
    slug.trim_end_matches('-').to_owned()
}

/// The ASCII a character becomes in a slug, or `""` when it is a separator.
fn fold_letter(c: char) -> &'static str {
    // Digits and ASCII letters stand for themselves; build their `&'static str` from a table.
    const ASCII: &str = "0123456789abcdefghijklmnopqrstuvwxyz";
    let lower = c.to_ascii_lowercase();
    if let Some(index) = ASCII.find(lower) {
        return &ASCII[index..=index];
    }
    match c.to_lowercase().next().unwrap_or(c) {
        'á' | 'à' | 'â' | 'ä' | 'ã' | 'å' => "a",
        'é' | 'è' | 'ê' | 'ë' => "e",
        'í' | 'ì' | 'î' | 'ï' => "i",
        'ó' | 'ò' | 'ô' | 'ö' | 'õ' => "o",
        'ú' | 'ù' | 'û' | 'ü' => "u",
        'ñ' => "n",
        'ç' => "c",
        'ß' => "ss",
        'æ' => "ae",
        'œ' => "oe",
        _ => "",
    }
}

/// One line of plain text with no four-byte character (emoji). The SEO fields are stored as they
/// are, so a database on the 3-byte `utf8` charset would refuse such a character, and an entity
/// cannot stand in for it because the text is escaped when the page is built.
fn one_line_without_emoji(text: &str) -> String {
    text.chars()
        .filter(|&c| !is_four_byte(c))
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Cleans a meta description: one line of text without emoji, at most 300 characters.
fn clean_meta_description(text: &str) -> Result<String, DraftError> {
    let cleaned = one_line_without_emoji(text);
    let length = cleaned.chars().count();
    if length > MAX_META_DESCRIPTION_CHARS {
        return Err(DraftError::MetaDescriptionTooLong {
            length,
            max: MAX_META_DESCRIPTION_CHARS,
        });
    }
    Ok(cleaned)
}

/// Cleans a focus keyphrase: one line of text without emoji, at most 100 characters.
fn clean_focus_keyphrase(text: &str) -> Result<String, DraftError> {
    let cleaned = one_line_without_emoji(text);
    let length = cleaned.chars().count();
    if length > MAX_FOCUS_KEYPHRASE_CHARS {
        return Err(DraftError::FocusKeyphraseTooLong {
            length,
            max: MAX_FOCUS_KEYPHRASE_CHARS,
        });
    }
    Ok(cleaned)
}

/// A suggestion for the meta description: the first paragraph of the body that is text and not a
/// run of hashtags, cut at 155 characters on a word boundary and marked with an ellipsis when it
/// was cut. Empty when there is none.
fn suggest_meta_description(body: &str) -> String {
    let paragraph = parse_blocks(body)
        .into_iter()
        .find_map(|block| match block {
            Block::Paragraph(lines) => {
                let text = lines.join(" ");
                let only_hashtags = text.split_whitespace().all(|word| word.starts_with('#'));
                (!only_hashtags).then_some(text)
            }
            Block::Heading(_) => None,
        });
    let Some(text) = paragraph else {
        return String::new();
    };
    let text = text
        .chars()
        .filter(|&c| !is_four_byte(c))
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if text.chars().count() <= META_DESCRIPTION_SUGGESTION_CHARS {
        return text;
    }
    let cut: String = text
        .chars()
        .take(META_DESCRIPTION_SUGGESTION_CHARS)
        .collect();
    // Back up to the last whole word, unless the text is one long word.
    let cut = match cut.rfind(' ') {
        Some(index) if index > 0 => &cut[..index],
        _ => &cut,
    };
    format!("{}…", cut.trim_end_matches(|c: char| !c.is_alphanumeric()))
}

/// The editable starting point of a draft.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DraftText {
    pub title: String,
    pub html: String,
    /// The address slug made from the title.
    pub slug: String,
    /// A suggestion for the meta description, taken from the start of the text.
    pub meta_description: String,
}

/// Splits a caption into the suggested title and the body of the editor. The line that gives
/// the title leaves the body, so it is not said twice. Two cases keep it there: a line too long
/// to fit the title, because the title is only the start of it, and a caption that has no other
/// text, because the body would be empty.
pub fn prepare_draft_text(caption: &str) -> DraftText {
    let normalized = caption.replace("\r\n", "\n").replace('\r', "\n");
    let lines: Vec<&str> = normalized.lines().collect();
    let Some(title_line) = lines.iter().position(|line| !line.trim().is_empty()) else {
        return DraftText {
            title: String::new(),
            html: String::new(),
            slug: String::new(),
            meta_description: String::new(),
        };
    };
    let fits = lines[title_line].trim().chars().count() <= TITLE_SUGGESTION_CHARS;
    let rest = lines
        .iter()
        .enumerate()
        .filter(|(index, _)| *index != title_line)
        .map(|(_, line)| *line)
        .collect::<Vec<_>>()
        .join("\n");
    let body = if fits && !rest.trim().is_empty() {
        &rest
    } else {
        &normalized
    };
    let title = suggest_title(&normalized);
    DraftText {
        slug: slugify(&title),
        html: caption_to_html(body),
        meta_description: suggest_meta_description(body),
        title,
    }
}

/// Cleans editor HTML with an allow-list (see [`ALLOWED_TAGS`]): text structure, emphasis,
/// headings from `h2`, lists, quotes, code, tables and links. Scripts, images, styles, classes and
/// event handlers go. Only a few attributes survive: `href`, `title` and `target="_blank"` on a
/// link, `title` on an abbreviation, `start` on an ordered list and the spans of table cells.
pub fn sanitize_html(html: &str) -> String {
    let tag_attributes: HashMap<&str, HashSet<&str>> = HashMap::from([
        ("a", HashSet::from(["href", "title", "target"])),
        ("abbr", HashSet::from(["title"])),
        ("ol", HashSet::from(["start"])),
        ("th", HashSet::from(["colspan", "rowspan", "scope"])),
        ("td", HashSet::from(["colspan", "rowspan"])),
    ]);
    ammonia::Builder::default()
        .tags(ALLOWED_TAGS.into_iter().collect())
        .tag_attributes(tag_attributes)
        .generic_attributes(HashSet::new())
        .url_schemes(ALLOWED_SCHEMES.into_iter().collect())
        .url_relative(ammonia::UrlRelative::Deny)
        .link_rel(Some("noopener noreferrer"))
        // A link may open in a new tab; no other target (a frame name, `_top`...) is wanted.
        .attribute_filter(|element, attribute, value| match (element, attribute) {
            ("a", "target") if value != "_blank" => None,
            _ => Some(value.into()),
        })
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

/// A validated draft, ready to send: a plain-text title, sanitized HTML, an optional image, an
/// optional Instagram player to show below the text, and the optional slug, meta description and
/// focus keyphrase.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicationDraft {
    title: String,
    html: String,
    image: Option<Image>,
    video: Option<VideoEmbed>,
    slug: Option<String>,
    meta_description: Option<String>,
    focus_keyphrase: Option<String>,
    categories: Vec<u64>,
}

impl PublicationDraft {
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
            slug: None,
            meta_description: None,
            focus_keyphrase: None,
            categories: Vec::new(),
        })
    }

    /// Sets the slug of the address. Whatever is given is cleaned with [`slugify`], so emoji,
    /// accents and symbols never reach the address; when nothing usable is left there is no slug
    /// and WordPress makes its own from the title.
    pub fn with_slug(mut self, slug: &str) -> Self {
        self.slug = Some(slugify(slug)).filter(|slug| !slug.is_empty());
        self
    }

    /// Sets the meta description, one line without emoji. A blank one means there is none.
    pub fn with_meta_description(mut self, text: &str) -> Result<Self, DraftError> {
        self.meta_description = Some(clean_meta_description(text)?).filter(|text| !text.is_empty());
        Ok(self)
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
    /// added after sanitizing because the sanitizer would remove the script and the styles.
    pub fn content(&self) -> String {
        match &self.video {
            Some(video) => format!("{}\n\n{}\n\n{}", video.link_html(), self.html, video.html()),
            None => self.html.clone(),
        }
    }

    pub fn video(&self) -> Option<&VideoEmbed> {
        self.video.as_ref()
    }

    /// Sets the focus keyphrase, one line without emoji. A blank one means there is none.
    pub fn with_focus_keyphrase(mut self, text: &str) -> Result<Self, DraftError> {
        self.focus_keyphrase = Some(clean_focus_keyphrase(text)?).filter(|text| !text.is_empty());
        Ok(self)
    }

    /// Sets the categories by their WordPress ids. An id that is repeated or is 0 is dropped.
    /// With none, WordPress puts the post in its default category.
    pub fn with_categories(mut self, ids: &[u64]) -> Result<Self, DraftError> {
        let mut unique: Vec<u64> = Vec::new();
        for &id in ids {
            if id != 0 && !unique.contains(&id) {
                unique.push(id);
            }
        }
        if unique.len() > MAX_CATEGORIES {
            return Err(DraftError::TooManyCategories {
                count: unique.len(),
                max: MAX_CATEGORIES,
            });
        }
        self.categories = unique;
        Ok(self)
    }

    pub fn slug(&self) -> Option<&str> {
        self.slug.as_deref()
    }

    pub fn meta_description(&self) -> Option<&str> {
        self.meta_description.as_deref()
    }

    pub fn focus_keyphrase(&self) -> Option<&str> {
        self.focus_keyphrase.as_deref()
    }

    pub fn categories(&self) -> &[u64] {
        &self.categories
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
    fn lines_that_name_a_section_become_headings() {
        let html = caption_to_html(
            "Pan de molde\nIngredientes\n200 g de harina\n2 huevos\n\nElaboración\nMezcla todo",
        );
        assert_eq!(
            html,
            "<p>Pan de molde</p>\n<h2>Ingredientes</h2>\n<p>200 g de harina<br>2 huevos</p>\n\
             <h2>Elaboración</h2>\n<p>Mezcla todo</p>"
        );
    }

    #[test]
    fn a_heading_in_the_middle_of_a_paragraph_splits_it() {
        assert_eq!(
            caption_to_html("Texto\nIngredientes:\n- a\n- b\nPreparación:\n1. mezcla"),
            "<p>Texto</p>\n<h2>Ingredientes</h2>\n<p>- a<br>- b</p>\n<h2>Preparación</h2>\n\
             <p>1. mezcla</p>"
        );
    }

    #[test]
    fn the_names_of_the_sections_are_found_however_they_are_written() {
        for (line, heading) in [
            ("Ingredientes", "Ingredientes"),
            ("INGREDIENTES", "INGREDIENTES"),
            ("ingredientes:", "ingredientes"),
            ("🥕 INGREDIENTES 🥕", "INGREDIENTES"),
            ("✨ Ingredientes ✨", "Ingredientes"),
            ("*Ingredientes*", "Ingredientes"),
            ("- Ingredientes -", "Ingredientes"),
            ("Lista de ingredientes", "Lista de ingredientes"),
            ("Los ingredientes", "Los ingredientes"),
            ("Ingredientes necesarios:", "Ingredientes necesarios"),
            (
                "Ingredientes para 3 personas",
                "Ingredientes para 3 personas",
            ),
            ("Ingredientes para la masa:", "Ingredientes para la masa"),
            (
                "Ingredientes (para 4 raciones):",
                "Ingredientes (para 4 raciones)",
            ),
            ("Ingredientes (3 personas)", "Ingredientes (3 personas)"),
            ("Elaboración", "Elaboración"),
            ("ELABORACIÓN:", "ELABORACIÓN"),
            ("elaboracion", "elaboracion"),
            ("👩‍🍳 Preparación:", "Preparación"),
            ("Preparacion", "Preparacion"),
            ("Modo de preparación", "Modo de preparación"),
            ("Modo de elaboración:", "Modo de elaboración"),
            ("Paso a paso", "Paso a paso"),
            ("Pasos a seguir", "Pasos a seguir"),
            ("Pasos:", "Pasos"),
            ("Instrucciones", "Instrucciones"),
            ("Procedimiento", "Procedimiento"),
            ("Cómo se hace", "Cómo se hace"),
            (
                "  Ingredientes   para   3   personas ",
                "Ingredientes para 3 personas",
            ),
        ] {
            assert_eq!(section_heading(line).as_deref(), Some(heading), "{line:?}");
        }
    }

    #[test]
    fn a_line_that_goes_on_with_content_is_not_a_heading() {
        for line in [
            "Ingredientes: 200 g de harina",
            "Ingredientes: harina, agua y sal",
            "Preparación: mezcla todo y hornea",
            "Ingredientes (200 g de harina, 2 huevos, 100 ml de leche)",
            "Ingredientes para 4 personas, 2 huevos",
            "Los ingredientes son muy fáciles de encontrar",
            "Preparación para hacer pan en casa de forma fácil y rápida",
            "Ingredientes y preparación en el vídeo",
            "Ingredientesx",
            "Pasos de baile",
            "Mi preparación favorita",
            "#ingredientes",
            "#Preparación",
            "Tarta de queso",
            "",
            "💚",
        ] {
            assert_eq!(section_heading(line), None, "{line:?}");
        }
    }

    #[test]
    fn a_heading_is_escaped() {
        // The qualifier cannot hold markup: it would be longer than a heading may be, or
        // it is escaped like any text.
        assert_eq!(
            caption_to_html("Ingredientes (para <b>2</b>)"),
            "<h2>Ingredientes (para &lt;b&gt;2&lt;/b&gt;)</h2>"
        );
    }

    #[test]
    fn the_title_line_leaves_the_body() {
        let text = prepare_draft_text(
            "Tarta de queso\n\nIngredientes\n200 g de queso\n\nElaboración\nMezcla",
        );
        assert_eq!(text.title, "Tarta de queso");
        assert_eq!(
            text.html,
            "<h2>Ingredientes</h2>\n<p>200 g de queso</p>\n<h2>Elaboración</h2>\n<p>Mezcla</p>"
        );
    }

    #[test]
    fn the_title_line_leaves_the_body_even_without_a_blank_line() {
        let text = prepare_draft_text("\n  \nTítulo 💚\nLínea dos\nLínea tres");
        assert_eq!(text.title, "Título 💚");
        assert_eq!(text.html, "<p>Línea dos<br>Línea tres</p>");
    }

    #[test]
    fn a_title_line_too_long_to_fit_stays_in_the_body() {
        let long = "a".repeat(100);
        let text = prepare_draft_text(&format!("{long}\n\nResto"));
        assert_eq!(text.title, "a".repeat(80));
        assert_eq!(text.html, format!("<p>{long}</p>\n<p>Resto</p>"));
    }

    #[test]
    fn a_caption_that_is_only_the_title_keeps_it_in_the_body() {
        let text = prepare_draft_text("Solo una línea");
        assert_eq!(text.title, "Solo una línea");
        assert_eq!(text.html, "<p>Solo una línea</p>");
    }

    #[test]
    fn an_empty_caption_gives_an_empty_start() {
        for caption in ["", "   \n \r\n\t"] {
            assert_eq!(
                prepare_draft_text(caption),
                DraftText {
                    title: String::new(),
                    html: String::new(),
                    slug: String::new(),
                    meta_description: String::new()
                }
            );
        }
    }

    #[test]
    fn a_slug_has_only_ascii_letters_digits_and_single_hyphens() {
        for (title, slug) in [
            ("Tarta de queso", "tarta-de-queso"),
            ("  Tarta   de  queso  ", "tarta-de-queso"),
            ("Aida Lídice 💚🌱 Recetas", "aida-lidice-recetas"),
            ("Cañas y pingüinos", "canas-y-pinguinos"),
            ("Crème brûlée à la façon", "creme-brulee-a-la-facon"),
            ("ÁÉÍÓÚ Ñ Ü", "aeiou-n-u"),
            ("Straße", "strasse"),
            ("Fabada vegana: ¡receta!", "fabada-vegana-receta"),
            ("100% vegano & fácil", "100-vegano-facil"),
            ("✨ Magia ✨", "magia"),
            ("a---b___c", "a-b-c"),
            ("COCIDO", "cocido"),
            ("Tarta 2024", "tarta-2024"),
        ] {
            assert_eq!(slugify(title), slug, "{title:?}");
        }
    }

    #[test]
    fn a_slug_that_has_nothing_usable_is_empty() {
        for title in ["", "   ", "💚🌱", "日本語", "---", "¡¿!?"] {
            assert_eq!(slugify(title), "", "{title:?}");
        }
    }

    #[test]
    fn a_long_slug_is_cut_without_a_trailing_hyphen() {
        let slug = slugify(&format!("{} {}", "a".repeat(99), "b".repeat(50)));
        assert!(slug.len() <= 100);
        assert!(!slug.ends_with('-'));
        let slug = slugify(&"palabra ".repeat(30));
        assert!(slug.len() <= 100 && !slug.ends_with('-'), "{slug}");
    }

    #[test]
    fn a_slug_is_idempotent_and_has_no_forbidden_character() {
        for title in ["Aida 💚 Lídice", "ÁÉÍ ñ ç", "a--b", "100% ¡vegano!", "x"] {
            let once = slugify(title);
            assert_eq!(slugify(&once), once);
            assert!(once
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'));
            assert!(!once.starts_with('-') && !once.ends_with('-') && !once.contains("--"));
        }
    }

    #[test]
    fn the_start_of_a_draft_has_a_slug_from_its_title() {
        let text = prepare_draft_text("Tarta de queso 💚\n\nIngredientes\n200 g de queso");
        assert_eq!(text.title, "Tarta de queso 💚");
        assert_eq!(text.slug, "tarta-de-queso");
    }

    #[test]
    fn the_meta_description_suggestion_is_the_first_paragraph_of_text() {
        let text = prepare_draft_text(
            "Tarta\n\nUna tarta cremosa y fácil 💚\ncon solo tres ingredientes.\n\nIngredientes\n200 g",
        );
        assert_eq!(
            text.meta_description,
            "Una tarta cremosa y fácil con solo tres ingredientes."
        );
    }

    #[test]
    fn the_meta_description_suggestion_skips_headings_and_hashtags() {
        let text = prepare_draft_text("Tarta\n\n#vegano #tarta\n\nIngredientes\n\nMezcla bien.");
        assert_eq!(text.meta_description, "Mezcla bien.");
        let text = prepare_draft_text("Tarta\n\n#vegano #tarta");
        assert_eq!(text.meta_description, "");
        // The caption of a single line keeps it in the body, so the suggestion is that line.
        let text = prepare_draft_text("Solo una línea");
        assert_eq!(text.meta_description, "Solo una línea");
    }

    #[test]
    fn a_long_meta_description_suggestion_is_cut_on_a_word() {
        let words = "palabra ".repeat(40);
        let text = prepare_draft_text(&format!("Titulo\n\n{words}"));
        assert!(text.meta_description.ends_with('…'));
        let without = text.meta_description.trim_end_matches('…');
        assert!(without.chars().count() <= 155, "{}", text.meta_description);
        assert!(without.ends_with("palabra"), "{}", text.meta_description);
    }

    #[test]
    fn the_meta_description_suggestion_has_no_four_byte_character() {
        let text = prepare_draft_text("Titulo\n\nHola 💚 mundo 🌱");
        assert_eq!(text.meta_description, "Hola mundo");
    }

    #[test]
    fn a_draft_cleans_the_slug_it_is_given() {
        let draft = PublicationDraft::new("Titulo", "<p>Hola</p>")
            .unwrap()
            .with_slug("Aida 💚 Lídice");
        assert_eq!(draft.slug(), Some("aida-lidice"));
        let draft = draft.with_slug("💚");
        assert_eq!(draft.slug(), None);
        let draft = draft.with_slug("");
        assert_eq!(draft.slug(), None);
    }

    #[test]
    fn a_draft_cleans_the_meta_description_it_is_given() {
        let draft = PublicationDraft::new("Titulo", "<p>Hola</p>")
            .unwrap()
            .with_meta_description("  Una  tarta\n 💚 cremosa  ")
            .unwrap();
        assert_eq!(draft.meta_description(), Some("Una tarta cremosa"));
        let draft = draft.with_meta_description("  \n ").unwrap();
        assert_eq!(draft.meta_description(), None);
    }

    #[test]
    fn a_meta_description_that_is_too_long_is_rejected() {
        let draft = PublicationDraft::new("Titulo", "<p>Hola</p>").unwrap();
        assert!(draft
            .clone()
            .with_meta_description(&"a".repeat(300))
            .is_ok());
        assert_eq!(
            draft.with_meta_description(&"a".repeat(301)),
            Err(DraftError::MetaDescriptionTooLong {
                length: 301,
                max: 300
            })
        );
    }

    #[test]
    fn a_draft_cleans_the_focus_keyphrase_it_is_given() {
        let draft = PublicationDraft::new("Titulo", "<p>Hola</p>")
            .unwrap()
            .with_focus_keyphrase("  tarta  de\n queso 💚 vegana ")
            .unwrap();
        assert_eq!(draft.focus_keyphrase(), Some("tarta de queso vegana"));
        let draft = draft.with_focus_keyphrase(" \n ").unwrap();
        assert_eq!(draft.focus_keyphrase(), None);
        assert!(draft.categories().is_empty());
    }

    #[test]
    fn a_focus_keyphrase_that_is_too_long_is_rejected() {
        let draft = PublicationDraft::new("Titulo", "<p>Hola</p>").unwrap();
        assert!(draft.clone().with_focus_keyphrase(&"a".repeat(100)).is_ok());
        assert_eq!(
            draft.with_focus_keyphrase(&"a".repeat(101)),
            Err(DraftError::FocusKeyphraseTooLong {
                length: 101,
                max: 100
            })
        );
    }

    #[test]
    fn a_draft_keeps_its_categories_without_repeats_or_zeros() {
        let draft = PublicationDraft::new("Titulo", "<p>Hola</p>")
            .unwrap()
            .with_categories(&[7, 3, 7, 0, 12, 3])
            .unwrap();
        assert_eq!(draft.categories(), [7, 3, 12]);
        let draft = draft.with_categories(&[]).unwrap();
        assert!(draft.categories().is_empty());
    }

    #[test]
    fn too_many_categories_are_rejected() {
        let draft = PublicationDraft::new("Titulo", "<p>Hola</p>").unwrap();
        let twenty: Vec<u64> = (1..=20).collect();
        assert!(draft.clone().with_categories(&twenty).is_ok());
        let twenty_one: Vec<u64> = (1..=21).collect();
        assert_eq!(
            draft.with_categories(&twenty_one),
            Err(DraftError::TooManyCategories { count: 21, max: 20 })
        );
    }

    #[test]
    fn a_draft_without_slug_or_description_has_none() {
        let draft = PublicationDraft::new("Titulo", "<p>Hola</p>").unwrap();
        assert_eq!(draft.slug(), None);
        assert_eq!(draft.meta_description(), None);
        assert_eq!(draft.focus_keyphrase(), None);
    }

    #[test]
    fn windows_line_endings_work_for_the_title_too() {
        let text = prepare_draft_text("Titulo\r\nIngredientes\r\n200 g");
        assert_eq!(text.title, "Titulo");
        assert_eq!(text.html, "<h2>Ingredientes</h2>\n<p>200 g</p>");
    }

    #[test]
    fn a_draft_built_from_the_start_keeps_the_headings() {
        let text = prepare_draft_text("Pan\nIngredientes\n200 g\nElaboración\nMezcla");
        let draft = PublicationDraft::new(&text.title, &text.html).unwrap();
        assert_eq!(draft.title(), "Pan");
        assert_eq!(
            draft.html(),
            "<h2>Ingredientes</h2>\n<p>200 g</p>\n<h2>Elaboración</h2>\n<p>Mezcla</p>"
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
        assert!(cleaned.contains(
            "<a href=\"https://x.test/a\" target=\"_blank\" rel=\"noopener noreferrer\">ok</a>"
        ));
        assert!(!cleaned.contains("javascript:"));
        assert!(!cleaned.contains("data:"));
        assert!(!cleaned.contains("/relative"));
        assert!(!cleaned.contains("onclick"));
    }

    #[test]
    fn a_link_may_open_in_a_new_tab_but_no_other_target_survives() {
        for target in ["_top", "_parent", "_self", "frame", "_BLANK"] {
            let cleaned = sanitize_html(&format!(
                "<a href=\"https://x.test/\" target=\"{target}\">x</a>"
            ));
            assert!(!cleaned.contains("target"), "{target} -> {cleaned}");
        }
        let cleaned = sanitize_html("<a href=\"https://x.test/\" target=\"_blank\">x</a>");
        assert!(cleaned.contains("target=\"_blank\""));
        assert!(cleaned.contains("rel=\"noopener noreferrer\""));
    }

    #[test]
    fn a_link_keeps_its_title() {
        let cleaned = sanitize_html("<a href=\"https://x.test/\" title=\"La receta\">x</a>");
        assert!(cleaned.contains("title=\"La receta\""));
    }

    #[test]
    fn every_heading_level_below_the_title_is_kept_and_the_title_level_is_not() {
        for level in 2..=6 {
            let html = format!("<h{level}>Paso</h{level}>");
            assert_eq!(sanitize_html(&html), html);
        }
        assert_eq!(sanitize_html("<h1>Titulo</h1>"), "Titulo");
    }

    #[test]
    fn the_wider_formatting_of_the_editor_is_kept() {
        let html = "<p><s>a</s> <del>b</del> <ins>c</ins> <sup>2</sup> <sub>1</sub> <mark>d</mark>                     <small>e</small> <code>f</code> <kbd>g</kbd> <q>h</q> <cite>i</cite>                     <abbr title=\"Organizacion\">ONG</abbr></p><hr><pre>uno
  dos</pre>";
        assert_eq!(sanitize_html(html), html);
    }

    #[test]
    fn tables_and_list_options_are_kept_without_styles() {
        let html = "<table><caption>Medidas</caption><thead><tr><th scope=\"col\">A</th>                    <th colspan=\"2\">B</th></tr></thead><tbody><tr><td rowspan=\"2\">1</td>                    <td>2</td></tr></tbody></table><ol start=\"3\"><li>tres</li></ol>";
        assert_eq!(sanitize_html(html), html);
        let cleaned = sanitize_html("<table style=\"width:9px\" class=\"x\"><tr><td style=\"color:red\">a</td></tr></table>");
        assert!(
            !cleaned.contains("style") && !cleaned.contains("class"),
            "{cleaned}"
        );
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
        let draft = PublicationDraft::new(
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
        let draft = PublicationDraft::new("Aida Lídice 💚 🌱 Recetas", "<p>Hola</p>").unwrap();
        assert_eq!(draft.title(), "Aida Lídice Recetas");
        let draft = PublicationDraft::new("💚Tarta de queso🌱", "<p>Hola</p>").unwrap();
        assert_eq!(draft.title(), "Tarta de queso");
    }

    #[test]
    fn a_title_that_is_only_emoji_is_empty() {
        assert_eq!(
            PublicationDraft::new("💚🌱", "<p>Hola</p>"),
            Err(DraftError::EmptyTitle)
        );
    }

    #[test]
    fn a_body_that_is_only_an_emoji_still_counts_as_text() {
        let draft = PublicationDraft::new("Titulo", "<p>💚</p>").unwrap();
        assert_eq!(draft.html(), "<p>&#x1F49A;</p>");
    }

    #[test]
    fn the_title_limit_counts_what_is_sent() {
        let title = format!("{}💚", "a".repeat(200));
        assert!(PublicationDraft::new(&title, "<p>Hola</p>").is_ok());
        let title = format!("{}💚", "a".repeat(201));
        assert!(matches!(
            PublicationDraft::new(&title, "<p>Hola</p>"),
            Err(DraftError::TitleTooLong { length: 201, .. })
        ));
    }

    #[test]
    fn without_a_player_the_content_is_the_body() {
        let draft = PublicationDraft::new("Titulo", "<p>Hola</p>").unwrap();
        assert_eq!(draft.content(), "<p>Hola</p>");
        assert!(draft.video().is_none());
    }

    #[test]
    fn the_link_comes_first_then_the_body_and_the_player_last() {
        let embed = VideoEmbed::from_permalink("https://www.instagram.com/reel/Dx1Ab/").unwrap();
        let draft = PublicationDraft::new("Titulo", "<p>Hola 💚</p>")
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
        let player = content.find("<blockquote").unwrap();
        assert!(link < text && text < player, "{content}");
        // The editable body stays clean of the link and the player.
        assert_eq!(draft.html(), "<p>Hola &#x1F49A;</p>");
        assert_eq!(draft.video(), Some(&embed));
        assert_eq!(draft.with_video(None).content(), "<p>Hola &#x1F49A;</p>");
    }

    #[test]
    fn an_iframe_in_the_editor_is_still_removed() {
        let draft = PublicationDraft::new(
            "Titulo",
            "<p>Hola</p><iframe src=\"https://evil.test\"></iframe>",
        )
        .unwrap();
        assert!(!draft.content().contains("iframe"));
    }

    #[test]
    fn a_visible_entity_is_still_text() {
        assert!(PublicationDraft::new("Titulo", "<p>&amp;</p>").is_ok());
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
            "<a href=\"https://x.test/\" target=\"_blank\">x</a>",
            "<table><tr><td>a</td></tr></table><h4>t</h4><pre>x</pre>",
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
            PublicationDraft::new("  Tarta \n de   queso ", "<p>Hola</p><script>x</script>")
                .unwrap();
        assert_eq!(draft.title(), "Tarta de queso");
        assert_eq!(draft.html(), "<p>Hola</p>");
        assert!(draft.image().is_none());
    }

    #[test]
    fn a_draft_needs_a_title() {
        assert_eq!(
            PublicationDraft::new(" \n ", "<p>Hola</p>"),
            Err(DraftError::EmptyTitle)
        );
    }

    #[test]
    fn a_draft_rejects_a_title_over_the_limit() {
        assert_eq!(
            PublicationDraft::new(&"a".repeat(201), "<p>Hola</p>"),
            Err(DraftError::TitleTooLong {
                length: 201,
                max: 200
            })
        );
        assert!(PublicationDraft::new(&"a".repeat(200), "<p>Hola</p>").is_ok());
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
                PublicationDraft::new("Titulo", html),
                Err(DraftError::EmptyBody),
                "{html:?}"
            );
        }
    }
}
