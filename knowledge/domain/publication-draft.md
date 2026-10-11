---
title: Publication Draft (text and image)
status: active
---

# Publication Draft

A draft is what the user edits before publishing: a title, an HTML body and an optional image. The text starts from an Instagram caption.

## From caption to editor (`publication_html`)
- `caption_to_html(caption)` turns plain text into HTML: line endings are normalized, each line is trimmed, a blank line starts a new `<p>`, a single newline inside a paragraph becomes `<br>`, paragraphs are joined with a newline, and every character that is special in HTML is escaped. A line that names a section of the publication becomes an `<h2>` and ends the paragraph it was in (see below). An empty or blank caption gives an empty string.
- `suggest_title(caption)` is the first non-blank line, with surrounding whitespace removed, cut at 80 characters on a character boundary. A caption with no text gives an empty title. The user edits it.
- `prepare_draft_text(caption)` is what the window asks for (`prepare_draft(caption)`, which returns `{ title, html, slug, meta_description }`). It gives the suggested title, its slug, a suggested meta description and a body **without the title line**, so the title is not said twice. The line stays in the body in two cases: when it is longer than the 80 characters of the title, because the title is only the start of it, and when it is the only text of the caption, because the body would be empty.

### Section headings
A line that only names a section of the publication becomes an `<h2>`, so the text has the same structure as the rest of the site. Matching ignores case and the accents of Spanish (`Elaboración`, `ELABORACION` and `elaboracion` are the same):
- **Ingredients**: `ingredientes`, `lista de ingredientes`, `los ingredientes`, `ingredientes necesarios`.
- **Method**: `preparación`, `elaboración`, `modo de preparación`, `modo de elaboración`, `paso a paso`, `pasos`, `pasos a seguir`, `cómo se hace`, `cómo hacerlo`, `instrucciones`, `procedimiento`.
- Emoji, punctuation and other decoration around the title are dropped (`🥕 INGREDIENTES 🥕:` becomes `<h2>INGREDIENTES</h2>`); the case the author used is kept.
- The title may carry a short qualifier of up to six words: `para 3 personas`, `para la masa` or `(4 raciones)`.
- A line that goes on with the content is **not** a heading: `Ingredientes: 200 g de harina`, `Preparación: mezcla todo`, a parenthesis or `para ...` with a comma or a colon, a qualifier longer than six words, a sentence such as `Los ingredientes son muy fáciles`, `Mi preparación favorita`, and any line that starts with `#` (a hashtag).
The matcher is written by hand, with no regular expression crate. Anything it misses, or takes for a heading by mistake, the user fixes in the editor with the heading button.

## Categories
The user ticks the categories of the post in a list that mirrors WordPress's own panel (see [`wordpress.md`](wordpress.md)). `PublicationDraft::with_categories` takes their ids, drops repeated ones and 0, keeps the order the user chose and accepts at most 20 (`TooManyCategories`). With none chosen, nothing is sent and WordPress puts the post in its default category.

## Slug, meta description and focus keyphrase
- **Slug**: the end of the post's address. `slugify(text)` makes lower-case ASCII letters and digits separated by single hyphens: accents are folded (`Cañas` becomes `canas`, `ß` becomes `ss`), and emoji, symbols, punctuation and letters of other alphabets are separators, so none of them reaches the address. It is cut at 100 characters without a trailing hyphen and is empty when nothing usable is left. Left to WordPress, the slug is built from the title, and the emoji and symbols that remain in a title can end up in the address as escape codes. `prepare_draft_text` fills the suggested slug from the title; the window keeps it in step with the title until the user edits it (`suggest_slug(title)`). Whatever the window sends is cleaned again with `slugify`, and an empty result means no slug is sent and WordPress makes its own.
- **Meta description**: one line of plain text. The suggestion is the first paragraph of the body that is text (not a heading and not a run of hashtags), cut at 155 characters on a word boundary and marked with an ellipsis when it was cut, which is about what search engines show. Whatever the window sends is collapsed to one line and has its four-byte characters (emoji) removed, because the field is stored as it is and an entity cannot stand in for them. More than 300 characters is `MetaDescriptionTooLong`; blank means none is sent.
- **Focus keyphrase**: Yoast's target phrase for the post, one line of plain text that the user fills in; nothing is suggested. It is collapsed to one line and has its four-byte characters removed, for the same reason as the meta description. More than 100 characters is `FocusKeyphraseTooLong`; blank means none is sent. How it reaches Yoast is in [`../operational/yoast-meta-description.md`](../operational/yoast-meta-description.md).

## Sanitizing (`sanitize_html`)
The editor is a `contenteditable` element, so its HTML is untrusted: pasted content can carry scripts, event handlers or odd markup. Before anything is sent to WordPress the body is cleaned with an allow-list (ADR 0003):
- Allowed tags are the ones the editor can make and that WordPress itself keeps in the content of a post for any user who may write posts (they are all in its own list of tags for post content): `p`, `br`, `hr`, `strong`, `b`, `em`, `i`, `u`, `s`, `strike`, `del`, `ins`, `sup`, `sub`, `mark`, `small`, `code`, `kbd`, `q`, `cite`, `abbr`, headings `h2` to `h6` (the post title is the page's `h1`, so `h1` is not allowed and its text stays as plain text), `ul`, `ol`, `li`, `blockquote`, `pre`, `a`, and the table tags `table`, `caption`, `thead`, `tbody`, `tfoot`, `tr`, `th`, `td`, `colgroup`. There is no `img` (a picture would have to be uploaded to the media library) and no `div` or `span`, which only carry styles.
- Allowed attributes: `href`, `title` and `target` on `a`; `title` on `abbr`; `start` on `ol`; `colspan` and `rowspan` on `th` and `td`; `scope` on `th`. `href` takes an `http`, `https` or `mailto` scheme; relative links are denied and every link gets `rel="noopener noreferrer"`. `target` survives only when it is exactly `_blank`; any other value (`_top`, a frame name...) is removed. Script schemes written in disguise (mixed case, HTML entities, tabs or newlines inside) are removed.
- Everything else, including `script`, `style`, `svg`, `math`, `form`, `base`, `meta`, `img`, inline styles, classes and event handlers, is removed. Cleaning twice gives the same result.

## The validated draft (`PublicationDraft`)
`PublicationDraft::new(title, html)` returns a draft or one of these errors:
- `EmptyTitle`: the title, collapsed to a single line, is empty.
- `TitleTooLong { length, max }`: more than 200 characters. A long title is rejected, not cut.
- `EmptyBody`: the sanitized body shows no text once markup is removed. A no-break space counts as blank. `<p><br></p>` and a body that was only a script are empty.

### Four-byte characters (emoji)
Emoji and every other character beyond U+FFFF take four bytes in UTF-8. A WordPress database still on the old 3-byte `utf8` charset cannot store them and refuses the whole post with `db_insert_error`; Instagram captions are full of them, so `PublicationDraft::new` makes sure none is ever sent:
- **In the body**, each one is written as a numeric HTML entity (`💚` becomes `&#x1F49A;`) after sanitizing. The entity is plain ASCII and a browser shows the original character, so the post looks the same. The editor still shows the real emoji; the conversion happens only when the draft is built.
- **In the title**, they are removed and the spaces collapse (`Aida Lídice 💚 Recetas` becomes `Aida Lídice Recetas`). An entity cannot be used there because the block editor would show it as literal text. A title that is only emoji is an `EmptyTitle`.
- Characters with three bytes, such as the zero-width joiner and the variation selector that glue an emoji sequence together, are left as they are. The 200-character title limit counts what is sent.
- If the site's database is `utf8mb4` this is unnecessary but harmless, except that emoji in titles are lost. Converting the database would make it avoidable.

`with_image(image)` attaches or removes the featured image. The text is validated first so a bad title never costs a download.

## Image (`image`)
The user chooses one of (`ImageChoice`, sent as `{ "kind": "none" }`, `{ "kind": "post_image", "url": ... }` or `{ "kind": "file", "data_url": ..., "filename": ... }`):
- **None**: the post is created without a featured image.
- **The post's own image**: the backend checks the URL again with the same trusted-host rule as the previews, because the frontend is not trusted: HTTPS and a host that is `cdninstagram.com` or `fbcdn.net` or one of their subdomains, with only letters, digits, dots and hyphens in it (no port, user information, backslash, percent escape or whitespace) and no empty or hyphen-edged label. Anything else is `UntrustedUrl` and nothing is requested. Then it downloads the image with a size cap. Only an HTTP 200 counts; any other status, including a redirect, is `DownloadStatus`. The saved name is `publication-image` plus the real extension.
- **A file from the computer**: the frontend reads it and sends a `data:` URL with the file name. Only `data:<type>;base64,<payload>` is accepted. A payload too large is refused before it is decoded.

An `Image` has a safe file name, a type and the bytes:
- The **type is read from the first bytes** (JPEG, PNG, GIF87a/89a, WebP), never from the file name, the `data:` URL or the server's `Content-Type`. Anything else is `NotAnImage`.
- The size limit is 10 MB (`TooLarge`).
- The **file name** is cut to its last path segment, loses its extension, keeps only ASCII letters, digits, `-` and `_` (other runs become one `-`), is cut at 60 characters and falls back to `publication-image`. It then gets the extension of the real type. It travels in a raw `Content-Disposition` header, which is why it is ASCII.

Image errors (`ImageError`): `NotAnImage`, `TooLarge`, `BadDataUrl`, `UntrustedUrl`, `Download` (a transport failure) and `DownloadStatus`.

Screenshots of a post are not supported; the user attaches the file instead.

## Video (`video`)
When the post is a reel or a video, the user can ask for Instagram's own player to appear **below the text**, with a link at the top that jumps to it (`VideoChoice`, sent as `{ "kind": "none" }`, the default when the field is absent, or `{ "kind": "instagram_embed", "permalink": ... }`). The window offers it only for posts of type `VIDEO` that have a permalink, and it is checked by default there.

- The backend does not trust the address the window sends. It parses it with the Instagram URL rules and accepts only a post (`/p/<code>`) or a reel (`/reel/<code>`, also under a user name); a profile or anything else is `VideoError::NotAPost`. The markup is then built from the validated shortcode alone (`https://www.instagram.com/p/<code>/` or `.../reel/<code>/`), so no text from the window ends up in it.
- The player is Instagram's own embed, the code that Instagram gives under *Embed* in a post: a `<blockquote class="instagram-media" data-instgrm-permalink="..." data-instgrm-version="14">` with a fallback link, followed by `<script async src="https://www.instagram.com/embed.js">`. It goes inside a `wp:html` block, in a container with `id="video"` and at most 540 px wide, the widest that Instagram's embed is made for, as in the site's own embedded posts, **below** the text. At the top of the post goes a `wp:paragraph` block with the link `<a href="#video">Ver el vídeo</a>` (the site is in Spanish, so the text is too). Both are added after sanitizing, because the sanitizer would remove the script and the styles, and neither is part of the text the user edits.
- The script is what makes the video play on the page. A bare `<iframe>` of the `/embed/` address does not: Instagram sends the visitor to its own page when the video is clicked, because the embed has to be started by the script, which also tells Instagram which site shows it and sizes the player by itself (so there is no fixed height). Before the script has run, and for visitors who block it, only the fallback link ("Ver esta publicación en Instagram") shows.
- The video is Instagram's, so nothing is downloaded or stored on the site and it only plays while the post is public. WordPress keeps a `script` or an `iframe` in the content only for users who may post unfiltered HTML (administrators and editors on a single site); for any other role it silently removes it, and only the fallback link remains. The script is Instagram's, so a site with a cookie or consent banner has to list it.
