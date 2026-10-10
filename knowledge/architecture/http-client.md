---
title: HTTP Client
status: active
---

# HTTP Client

The `http` module holds the one blocking client (`UreqClient`, over `ureq`) that the Instagram and WordPress code share. A single instance lives in the application state.

## Rules
- **HTTPS only.** An `http://` address is refused.
- **No redirects.** Credentials travel in headers (`Authorization`) and must not follow a redirect to another host. A 3xx answer comes back as an ordinary response and each caller turns it into its own error.
- **4xx and 5xx are answers, not failures.** They are returned with their body so callers can read Instagram's and WordPress's error envelopes.
- **Credentials never appear in a URL.** The Instagram token is sent as `Authorization: Bearer`, the WordPress credentials as HTTP Basic.
- **Timeouts.** Calls that exchange a little JSON (reading the posts) have 20 seconds. Calls that move an image or wait for WordPress to process one (an upload, a download, creating a post) have 120 seconds, so a slow uplink does not cut a transfer that a retry would then duplicate on the site.

## Operations
- `get_text(url, query, bearer_token)`: `GET`, body as text.
- `post(url, headers, body)`: `POST` of raw bytes, body as text.
- `get_bytes(url, max_bytes)`: `GET` without credentials, reading at most `max_bytes`; returns the status, the declared `Content-Type` and the bytes (`Download`). The declared type is not used: the image type is read from the bytes (see `../domain/recipe-draft.md`).

All three are `pub(crate)`; other modules reach them through their own traits (`HttpGet`, `WpHttp`, `ImageFetcher`), which tests replace with fakes so nothing touches the network.

## Errors
A transport failure is a `TransportError` holding one fixed phrase and never the underlying error text, which can carry request details: `timed out`, `could not connect`, `response too large` (the body passed the read limit) or `transport error`. Each caller wraps it in its own error (`ApiError::Network`, `WordPressError::Network`, `ImageError::Download`).
