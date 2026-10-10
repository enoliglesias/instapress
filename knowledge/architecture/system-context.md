---
title: System Context
status: active
---

# System Context

## Purpose
recipe-sync is a desktop app that takes the URL of a public Instagram profile, post or reel, extracts the caption, lets the user edit it in a rich text editor with a photo attached, and publishes it as a recipe post on a WordPress site.

## Context (C4 level 1)
- **User**: the person who curates recipes and owns or has permission to publish the content.
- **Instagram** (external, public web): source of the caption and image. There is no official API for reading third-party public posts, so extraction is best-effort and must degrade to manual paste.
- **WordPress site** (external): target of the published post, reached through its REST API with an Application Password stored in the OS credential store.

## What exists today
Only the first step of the flow is implemented:
- The main window asks for an Instagram URL and reports whether it is a profile, a post or a reel (see [`../domain/instagram-url.md`](../domain/instagram-url.md)).

## Not implemented yet
Caption extraction, the rich text editor, image attachment or capture, the WordPress client, credential storage and settings. Each of these will be documented here when it exists.

## Containers (C4 level 2)
One Tauri v2 process:
- **Rust backend** (`src-tauri/`): library crate `recipe_sync` plus the binary `recipe-sync` (`src/gui/main.rs`).
- **Static frontend** (`ui/`): `index.html`, `app.css`, `app.js`, embedded at compile time and talking to the backend through Tauri IPC (`withGlobalTauri`).

## Modules
- `instagram_url`: pure URL classification (`parse`, `InstagramTarget`, `InstagramUrlError`).

## IPC commands
- `classify_instagram_url(url) -> InstagramTarget | string`: thin adapter over `instagram_url::parse`; the error is the message of `InstagramUrlError`. Declared in `build.rs` and granted to the `main` window in `capabilities/main.json`.

## Security
Content Security Policy allows only same-origin scripts and styles and the Tauri IPC endpoint; the window has no network access of its own. The `main` capability grants only the commands it uses.
