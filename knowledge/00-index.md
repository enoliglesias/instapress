---
title: Knowledge Base Index
status: active
---

# Knowledge Base Index

Living snapshot of the current state of `instapress`, structured with C4 and Diataxis. No changelogs.

## Structure

- **`architecture/`**
  - [`system-context.md`](architecture/system-context.md): what the app is, its actors and external systems, modules, IPC commands, the main window behavior, security, dependencies, what exists today and what does not.
  - [`ui-design.md`](architecture/ui-design.md): the palette and its contrast, the menu bar, the one-column and two-column layouts and how the pieces behave.
  - [`http-client.md`](architecture/http-client.md): the shared blocking HTTP client, its rules (HTTPS only, no redirects, timeouts) and its fixed-phrase errors.
- **`domain/`**
  - [`instagram-url.md`](domain/instagram-url.md): how an Instagram URL is classified as a profile, post or reel, and which inputs are rejected.
  - [`instagram-api.md`](domain/instagram-api.md): reading the user's own recent posts through the Instagram API, the access token and where it is stored, the post model and the errors.
  - [`publication-draft.md`](domain/publication-draft.md): the draft (title, sanitized HTML body, image), caption to HTML and the three image choices.
  - [`wordpress.md`](domain/wordpress.md): WordPress credentials, how a post and its image are created, and the errors.
- **`adr/`**
  - [`0001-use-rust-and-tauri-v2.md`](adr/0001-use-rust-and-tauri-v2.md)
  - [`0002-static-frontend-without-node.md`](adr/0002-static-frontend-without-node.md)
  - [`0003-sanitize-html-with-ammonia.md`](adr/0003-sanitize-html-with-ammonia.md)
- **`how-to/`**
  - [`get-started.md`](how-to/get-started.md): everything to set up before the first draft: the Instagram token, the WordPress Application Password, the `.htaccess` rule, the Yoast snippet, the first run and a table of common errors.
- **`operational/`**
  - [`yoast-meta-description.md`](operational/yoast-meta-description.md): why the Yoast meta description and focus keyphrase need a one-time snippet on the WordPress site, the snippet, and how the app notices when a field was not kept.
  - [`releasing.md`](operational/releasing.md): how the Windows installer is built and released by GitHub Actions, and the checks that run on every push.
  - [`development-workflow.md`](operational/development-workflow.md): TDD, quality gates, constraints.
