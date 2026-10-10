---
title: WordPress Publishing
status: active
---

# WordPress Publishing

The app creates a post on the user's own WordPress site through its REST API (`/wp-json/wp/v2`).

## Credentials
- The user gives the site address, a WordPress user name and an **Application Password** (created in the user's profile in WordPress). The normal account password is never used.
- `SiteUrl` normalizes the address: `https://` only (any case), lower-case host, trailing slashes removed. A folder is kept so a site in a subfolder works. It rejects, with an error that names the input: another scheme, user information, a query or fragment, whitespace, a backslash, a host with characters other than letters, digits, dots and hyphens (names with non-ASCII letters must be given in punycode) or with a label that starts or ends with a hyphen or dot, a port that is not a number from 1 to 65535, and a folder with anything but letters, digits and `-_.~` or with a `.` or `..` segment.
- `AppPassword` is a newtype with a redacted `Debug`. All whitespace is removed because WordPress shows the password in groups.
- The user name is trimmed, may not be empty and may not contain a colon (the separator of HTTP Basic).
- Lengths are capped (`TooLong`): 200 characters for the address and the password, 100 for the user name. The OS credential store would refuse much larger values with an opaque error.
- The three values are stored together as one JSON secret `{ "site", "username", "app_password" }` in the OS credential store (service `recipe-sync`, user `wordpress`). Reading the settings from the frontend (`get_wordpress_settings`) returns the site, the user name and whether a password exists, never the password. A stored value that no longer validates counts as no settings.
- The address is typed by the user, so it is not restricted to public hosts; the credentials go to whatever address is saved.

Settings errors (`SettingsError`): `EmptySite`, `InvalidSite`, `EmptyUsername`, `UsernameHasColon`, `EmptyPassword`, `TooLong`.

## Categories
`list_categories` reads the categories of the site (`GET /wp-json/wp/v2/categories?per_page=100&page=N&_fields=id,name,parent`) with the same Basic credentials, a hundred at a time, up to ten pages. A page after the last one is an error in WordPress (`rest_post_invalid_page_number`); on a page past the first it only means there are no more. Without saved settings it stops with `NoSettings` and calls nothing.

The window gets them as `CategoryItem { id, name, depth }` in **tree order**: each category is followed by its children, brothers are sorted by name without regard to case, `depth` is 0 for a top-level category and 1, 2... for its descendants, and a category whose parent is not in the list is shown at the top level (a loop in the parents cannot repeat or hang anything). WordPress leaves HTML entities in the names (`Lemon &amp; Limes`), so the few it writes (`&amp; &lt; &gt; &quot; &apos; &nbsp;` and numeric ones) are decoded; anything else is left as it is.

## Publishing
`create_draft` does, in order:
1. If there is an image, `POST /media` with the raw bytes, its real `Content-Type` and `Content-Disposition: attachment; filename="..."`. The answer's `id` becomes the featured image.
2. `POST /posts` with a JSON body: `title`, `content` (the sanitized HTML, with a "Ver el vídeo" link before it and the Instagram player after it when there is a video), `status`, `featured_media` when there is an image, `slug` when the cleaned slug is not empty, `categories` (a list of ids) when some are chosen, and `meta` with `_yoast_wpseo_metadesc` (the meta description) and `_yoast_wpseo_focuskw` (the focus keyphrase) for those that are not blank.

The status is always `draft`: the user reviews and publishes from WordPress. Both requests use HTTP Basic authentication over HTTPS, follow no redirects and use the longer transfer timeout (see [`../architecture/http-client.md`](../architecture/http-client.md)).

The result (`CreatedPost`) is the post `id`, its `link` (optional; for a draft WordPress reports a preview-style address), `meta_description_saved`, `focus_keyphrase_saved` and `edit_url`, which is `{site}/wp-admin/post.php?post={id}&action=edit`. The window shows `edit_url`, because that is where the user reviews and publishes the draft. `meta_description_saved` and `focus_keyphrase_saved` are `None` when that field was not sent, `true` when the answer holds it under `meta` and `false` when it does not: WordPress ignores a meta key that the site has not registered for the REST API, without any error, so the window warns the user. See [`../operational/yoast-meta-description.md`](../operational/yoast-meta-description.md).

If the image upload succeeds and the post creation fails, the uploaded image stays in the media library; nothing is rolled back. If an answer never arrives (a timeout), the post may exist anyway: the window says so and asks the user to check the drafts in WordPress before trying again.

## Errors (`WordPressError`)
- `NoSettings`: nothing saved yet.
- `Settings(..)`: an invalid site address, user name or password.
- `Store(..)`: the credential store failed.
- `Network(..)`: the request did not complete; a fixed phrase, never the transport error.
- `Api { code, message }`: WordPress answered with its error body (`code`, `message`), for example `rest_cannot_create` or `incorrect_password`. The text comes from the site, so the window renders it as text only.
- `Redirect(status)`: the site redirected; the message tells the user to enter the final address. A redirect on the image upload stops everything before a post is created.
- `Status(code)`: any other non-2xx status without a readable error body.

A `db_insert_error` ("could not insert the post into the database") comes back as an `Api` error. On a site whose database cannot store four-byte characters it was caused by emoji; the app now never sends them (see [`recipe-draft.md`](recipe-draft.md)).
- `Malformed(..)`: a 2xx answer that is not the expected JSON.

The Application Password is never in a URL, a log line or an error message.

## Order of the whole flow (`publish::publish_draft`)
`DraftInput` is `{ title, html, image, video, slug, meta_description, focus_keyphrase, categories }` (the last five are optional). Nothing is downloaded or sent before the cheap checks pass:
1. The title and body are validated and sanitized, and the video choice is checked. A bad title, an empty text, a meta description over 300 characters, a focus keyphrase over 100, more than 20 categories or a video address that is not a post or reel stops here.
2. The saved settings are loaded. Without them the flow stops with `NoSettings`.
3. The image is resolved (downloaded or decoded). A bad image stops here and nothing is created on the site.
4. The image is uploaded, then the post is created.

`PublishError` wraps the draft, image and WordPress errors without changing their text; the window receives it as a plain string. The IPC command is `create_wordpress_draft`.
