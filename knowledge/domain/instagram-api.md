---
title: Instagram API and Access Token
status: active
---

# Instagram API and Access Token

The app reads the user's **own** posts through the Instagram API with Instagram Login (`graph.instagram.com`). It works only for the account that accepted the Instagram tester invitation of the Meta app (development mode, no app review) and only needs the `instagram_business_basic` permission (read profile and media).

## Access token
- `AccessToken` is a newtype. It is trimmed, must not be empty (`TokenError::Empty`) or contain whitespace (`TokenError::ContainsWhitespace`), and its `Debug` output is redacted. It has no `Display` or `Serialize`; the raw value is only exposed to build the request.
- The token is stored in the OS credential store (Windows Credential Manager, macOS Keychain, Secret Service on Linux) under service `recipe-sync`, user `instagram-token`. It is never written to a file, a log or an error message. A credential store failure is `TokenError::Store` with the action and the store's own message.
- Removing a token that is not there is not an error. A stored value that no longer validates is read as no token, so the user pastes it again and the save overwrites it.
- The user pastes the token in the settings block of the main window; it can also be removed there. The token generated in the Meta panel is long-lived (about 60 days). Refreshing it is not implemented yet.

## Recent posts
`instagram_api::list_recent_posts` loads the token from the store and calls `GET https://graph.instagram.com/me/media` with the fields `id, caption, media_type, media_url, thumbnail_url, permalink, timestamp` and a `limit` (25 from the UI). Only the first page is read; pagination is not implemented.

The response is read once. Only `id` is required in an item; a response without `data` is an empty account ("No posts found." in the UI) and the `paging` object is ignored.

Each item becomes a `Post`:
- `id`, `permalink` and `timestamp` (the raw ISO 8601 string) are passed through.
- `caption`: optional; a post without text has `None`.
- `media_type`: `IMAGE`, `VIDEO` (reels), `CAROUSEL_ALBUM`, or `UNKNOWN` for any value the API adds later and for a post that does not say. It serializes as SCREAMING_SNAKE_CASE.
- `preview_url`: for a video the thumbnail first, for the rest the media URL first, falling back to the other one. A candidate is used only if it is an HTTPS link to `*.cdninstagram.com` or `*.fbcdn.net` (no port, no user info, no lookalike host); anything else is dropped, so the frontend never receives another kind of URL. The URLs are temporary CDN links.

## Errors (`ApiError`)
- `MissingToken`: nothing saved yet.
- `Token(..)`: the credential store failed.
- `Network(..)`: the request did not complete. The text is one of three fixed phrases (`timed out`, `could not connect`, `transport error`), never the transport error itself.
- `Api { code, message }`: Instagram answered with its error envelope (for example an expired token, code 190). The envelope is checked first and wins over the status. The message comes from Instagram.
- `Status(code)`: any other non-2xx status, whatever the body, so a failure is never read as an empty account. A redirect is a `Status` too because redirects are not followed.
- `Malformed(..)`: a 2xx response whose body is not the expected JSON.

## HTTP client
HTTP goes through the `HttpGet` trait, so tests use a fake and never touch the network. The real client (`UreqClient`):
- does not treat 4xx/5xx as errors, so the error body can be read;
- sends the token as `Authorization: Bearer`, never in the URL or the query string, so it cannot appear in a logged or reported request line;
- is HTTPS only and follows no redirects, so the header cannot travel to another host;
- has a 20 second global timeout;
- never puts the request details in an error: transport failures are reduced to fixed phrases.
