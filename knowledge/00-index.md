---
title: Knowledge Base Index
status: active
---

# Knowledge Base Index

Living snapshot of the current state of `recipe-sync`, structured with C4 and Diataxis. No changelogs.

## Structure

- **`architecture/`**
  - [`system-context.md`](architecture/system-context.md): what the app is, its actors and external systems, modules, IPC commands, the main window behavior, security, dependencies, what exists today and what does not.
  - [`http-client.md`](architecture/http-client.md): the shared blocking HTTP client, its rules (HTTPS only, no redirects, timeouts) and its fixed-phrase errors.
- **`domain/`**
  - [`instagram-url.md`](domain/instagram-url.md): how an Instagram URL is classified as a profile, post or reel, and which inputs are rejected.
  - [`instagram-api.md`](domain/instagram-api.md): reading the user's own recent posts through the Instagram API, the access token and where it is stored, the post model and the errors.
  - [`recipe-draft.md`](domain/recipe-draft.md): the draft (title, sanitized HTML body, image), caption to HTML and the three image choices.
  - [`wordpress.md`](domain/wordpress.md): WordPress credentials, how a post and its image are created, and the errors.
- **`adr/`**
  - [`0001-use-rust-and-tauri-v2.md`](adr/0001-use-rust-and-tauri-v2.md)
  - [`0002-static-frontend-without-node.md`](adr/0002-static-frontend-without-node.md)
  - [`0003-sanitize-html-with-ammonia.md`](adr/0003-sanitize-html-with-ammonia.md)
- **`operational/`**
  - [`yoast-meta-description.md`](operational/yoast-meta-description.md): why the Yoast meta description and focus keyphrase need a one-time snippet on the WordPress site, the snippet, and how the app notices when a field was not kept.
  - [`development-workflow.md`](operational/development-workflow.md): TDD, quality gates, constraints.
