---
title: Knowledge Base Index
status: active
---

# Knowledge Base Index

Living snapshot of the current state of `recipe-sync`, structured with C4 and Diataxis. No changelogs.

## Structure

- **`architecture/`**
  - [`system-context.md`](architecture/system-context.md): what the app is, its actors and external systems, modules, binaries, frontend files, what exists today and what does not.
- **`domain/`**
  - [`instagram-url.md`](domain/instagram-url.md): how an Instagram URL is classified as a profile, post or reel, and which inputs are rejected.
  - [`instagram-api.md`](domain/instagram-api.md): reading the user's own recent posts through the Instagram API, the access token and where it is stored, the post model and the errors.
- **`adr/`**
  - [`0001-use-rust-and-tauri-v2.md`](adr/0001-use-rust-and-tauri-v2.md)
  - [`0002-static-frontend-without-node.md`](adr/0002-static-frontend-without-node.md)
- **`operational/`**
  - [`development-workflow.md`](operational/development-workflow.md): TDD, quality gates, constraints.
