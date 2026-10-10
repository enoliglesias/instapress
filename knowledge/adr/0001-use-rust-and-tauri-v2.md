---
title: ADR 0001 - Use Rust and Tauri v2
status: accepted
date: 2026-10-10
---

# ADR 0001: Use Rust and Tauri v2

## Context
We need a small cross-platform desktop app that fetches web content, edits it and publishes it over HTTP, with credentials kept out of plain files.

## Decision
- Use **Rust** for the backend: typed errors, safe parsing of untrusted HTML and JSON, and one language shared with our other desktop project.
- Use **Tauri v2** for packaging and IPC between the Rust backend and the webview, with capability-based permissions.

## Consequences
- Strict adherence to the Rust guidelines in `CLAUDE.md` and to Tauri capabilities (each window gets only the commands it needs).
- Small installers and low memory use compared with Electron.
- There is no async runtime of our own. Network and credential store access use blocking clients (`ureq`, `keyring`) and the commands that call them are `#[tauri::command(async)]`, which Tauri runs on a worker thread so the UI thread never blocks. Revisit this if the app needs concurrent requests.
