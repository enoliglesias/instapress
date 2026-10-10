---
title: Recipe Draft (text and image)
status: active
---

# Recipe Draft

A draft is what the user edits before publishing: a title, an HTML body and an optional image. The text starts from an Instagram caption.

## From caption to editor (`recipe_html`)
- `caption_to_html(caption)` turns plain text into HTML: line endings are normalized, each line is trimmed, a blank line starts a new `<p>`, a single newline inside a paragraph becomes `<br>`, paragraphs are joined with a newline, and every character that is special in HTML is escaped. An empty or blank caption gives an empty string.
- `suggest_title(caption)` is the first non-blank line, with surrounding whitespace removed, cut at 80 characters on a character boundary. A caption with no text gives an empty title. The user edits it.
- Both are exposed to the window as `prepare_draft(caption)`, which returns `{ title, html }`.

## Sanitizing (`sanitize_html`)
The editor is a `contenteditable` element, so its HTML is untrusted: pasted content can carry scripts, event handlers or odd markup. Before anything is sent to WordPress the body is cleaned with an allow-list (ADR 0003):
- Allowed tags: `p`, `br`, `strong`, `b`, `em`, `i`, `u`, `h2`, `h3`, `ul`, `ol`, `li`, `blockquote`, `a`. The window's toolbar produces only `h2`; `h3` survives if it is pasted.
- The only allowed attribute is `href` on `a`, with an `http`, `https` or `mailto` scheme. Relative links are denied, and every link gets `rel="noopener noreferrer"`. Script schemes written in disguise (mixed case, HTML entities, tabs or newlines inside) are removed.
- Everything else, including `script`, `style`, `svg`, `math`, `form`, `base`, `meta`, `img`, inline styles, classes and event handlers, is removed. Cleaning twice gives the same result.

## The validated draft (`RecipeDraft`)
`RecipeDraft::new(title, html)` returns a draft or one of these errors:
- `EmptyTitle`: the title, collapsed to a single line, is empty.
- `TitleTooLong { length, max }`: more than 200 characters. A long title is rejected, not cut.
- `EmptyBody`: the sanitized body shows no text once markup is removed. A no-break space counts as blank. `<p><br></p>` and a body that was only a script are empty.

### Four-byte characters (emoji)
Emoji and every other character beyond U+FFFF take four bytes in UTF-8. A WordPress database still on the old 3-byte `utf8` charset cannot store them and refuses the whole post with `db_insert_error`; Instagram captions are full of them, so `RecipeDraft::new` makes sure none is ever sent:
- **In the body**, each one is written as a numeric HTML entity (`💚` becomes `&#x1F49A;`) after sanitizing. The entity is plain ASCII and a browser shows the original character, so the post looks the same. The editor still shows the real emoji; the conversion happens only when the draft is built.
- **In the title**, they are removed and the spaces collapse (`Aida Lídice 💚 Recetas` becomes `Aida Lídice Recetas`). An entity cannot be used there because the block editor would show it as literal text. A title that is only emoji is an `EmptyTitle`.
- Characters with three bytes, such as the zero-width joiner and the variation selector that glue an emoji sequence together, are left as they are. The 200-character title limit counts what is sent.
- If the site's database is `utf8mb4` this is unnecessary but harmless, except that emoji in titles are lost. Converting the database would make it avoidable.

`with_image(image)` attaches or removes the featured image. The text is validated first so a bad title never costs a download.

## Image (`image`)
The user chooses one of (`ImageChoice`, sent as `{ "kind": "none" }`, `{ "kind": "post_image", "url": ... }` or `{ "kind": "file", "data_url": ..., "filename": ... }`):
- **None**: the post is created without a featured image.
- **The post's own image**: the backend checks the URL again with the same trusted-host rule as the previews, because the frontend is not trusted: HTTPS and a host that is `cdninstagram.com` or `fbcdn.net` or one of their subdomains, with only letters, digits, dots and hyphens in it (no port, user information, backslash, percent escape or whitespace) and no empty or hyphen-edged label. Anything else is `UntrustedUrl` and nothing is requested. Then it downloads the image with a size cap. Only an HTTP 200 counts; any other status, including a redirect, is `DownloadStatus`. The saved name is `recipe-image` plus the real extension.
- **A file from the computer**: the frontend reads it and sends a `data:` URL with the file name. Only `data:<type>;base64,<payload>` is accepted. A payload too large is refused before it is decoded.

An `Image` has a safe file name, a type and the bytes:
- The **type is read from the first bytes** (JPEG, PNG, GIF87a/89a, WebP), never from the file name, the `data:` URL or the server's `Content-Type`. Anything else is `NotAnImage`.
- The size limit is 10 MB (`TooLarge`).
- The **file name** is cut to its last path segment, loses its extension, keeps only ASCII letters, digits, `-` and `_` (other runs become one `-`), is cut at 60 characters and falls back to `recipe-image`. It then gets the extension of the real type. It travels in a raw `Content-Disposition` header, which is why it is ASCII.

Image errors (`ImageError`): `NotAnImage`, `TooLarge`, `BadDataUrl`, `UntrustedUrl`, `Download` (a transport failure) and `DownloadStatus`.

Screenshots of a post are not supported; the user attaches the file instead.

## Video (`video`)
When the post is a reel or a video, the user can ask for Instagram's own player to appear **below the text**, with a link at the top that jumps to it (`VideoChoice`, sent as `{ "kind": "none" }`, the default when the field is absent, or `{ "kind": "instagram_embed", "permalink": ... }`). The window offers it only for posts of type `VIDEO` that have a permalink, and it is checked by default there.

- The backend does not trust the address the window sends. It parses it with the Instagram URL rules and accepts only a post (`/p/<code>`) or a reel (`/reel/<code>`, also under a user name); a profile or anything else is `VideoError::NotAPost`. The player address is then built from the validated shortcode alone, as `https://www.instagram.com/p/<code>/embed/` or `.../reel/<code>/embed/`, so no text from the window ends up in the markup.
- The player is an `<iframe>` inside a `wp:html` block, in a container with `id="video"` and at most 400 px wide, **below** the text. At the top of the post goes a `wp:paragraph` block with the link `<a href="#video">Ver el vídeo</a>` (the site is in Spanish, so the text is too). Both are added after sanitizing, because the sanitizer would remove an iframe, and neither is part of the editable body. The content sent to WordPress is the link, a blank line, the body, a blank line and the player (`RecipeDraft::content`).
- The iframe has a fixed height (880 px) sized for a vertical reel, because Instagram resizes its player only with a script that the app does not add. The user can adjust it in WordPress.
- The video is Instagram's, so nothing is downloaded or stored on the site and it only plays while the post is public. WordPress keeps an iframe in the content only for users who may post unfiltered HTML (administrators and editors on a single site); for any other role it silently removes it.
