---
title: Instagram URL Classification
status: active
---

# Instagram URL Classification

`instagram_url::parse(&str) -> Result<InstagramTarget, InstagramUrlError>` is pure: no network, no URL crate.

## Accepted input
- Hosts `instagram.com`, `www.instagram.com` and `m.instagram.com`, case-insensitive. A lookalike such as `instagram.com.evil.test` is rejected.
- Scheme `http://` or `https://`, case-insensitive, or no scheme at all.
- Surrounding whitespace, a trailing slash, a query string and a fragment are ignored.

## Result
| Path | Target |
|---|---|
| `/p/<code>`, `/tv/<code>`, `/<user>/p/<code>` | `Post { shortcode }` |
| `/reel/<code>`, `/reels/<code>`, `/<user>/reel/<code>` | `Reel { shortcode }` |
| `/<user>` | `Profile { username }` |

A shortcode is one or more of `A-Z a-z 0-9 _ -`. A username is 1 to 30 of `A-Z a-z 0-9 _ .`. Top-level sections that are not profiles (`accounts`, `direct`, `explore`, `p`, `reel`, `reels`, `stories`, `tv`) are not usernames.

## Errors
- `Empty`: blank input.
- `NotInstagram(input)`: the host is not an Instagram host.
- `Unsupported(input)`: an Instagram URL that is not one of the shapes above, including malformed shortcodes and usernames. Every error names the input.

The target serializes to JSON as `{ "kind": "profile" | "post" | "reel", ... }` for the frontend.
