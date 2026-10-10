---
title: ADR 0002 - Static frontend without Node
status: accepted
date: 2026-10-10
---

# ADR 0002: Static frontend without Node

## Context
Node and npm are not installed on the development machines, and the first screens are simple forms.

## Decision
The frontend is plain HTML, CSS and JavaScript in `ui/`, embedded by Tauri at compile time (`frontendDist`) and using the injected `window.__TAURI__` global. There is no bundler and there are no npm scripts.

## Consequences
- No JavaScript dependency tree to audit.
- The rich text editor must be built on `contenteditable` or a library vendored into `ui/`. Revisit this ADR if the editor needs a build step.
