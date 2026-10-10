---
title: ADR 0003 - Sanitize editor HTML with ammonia
status: accepted
date: 2026-10-10
---

# ADR 0003: Sanitize editor HTML with ammonia

## Context
The post body comes from a `contenteditable` editor and can contain pasted scripts, event handlers and arbitrary markup. It is sent to a WordPress site under the user's credentials, so it must be cleaned before it leaves the app. A hand-written sanitizer is a classic source of bypasses.

## Decision
Use the `ammonia` crate with an explicit tag allow-list (see `knowledge/domain/publication-draft.md`). Sanitizing happens in the Rust backend, never in the webview.

## Consequences
- One extra dependency tree (html5ever); it is pure Rust and needs no system libraries.
- The allowed formatting is small on purpose: paragraphs, line breaks, emphasis, two heading levels, lists, quotes and links.
