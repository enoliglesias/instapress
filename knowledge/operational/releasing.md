---
title: Building and releasing
status: active
---

# Building and releasing

The Windows installer is built by GitHub Actions (`.github/workflows/release.yml`) with the Tauri CLI and NSIS (`bundle.targets` in `tauri.conf.json`). The installer is an `.exe` named after the product and the version.

## Make a release
1. Raise `version` in `src-tauri/Cargo.toml` (and `Cargo.lock` follows when the project is built) and commit it.
2. Tag the commit with the same number prefixed by `v` (for example `v0.2.0`) and push the tag.
3. The workflow refuses to run if the tag and the version differ, builds the installer and creates a **draft** release with it attached and generated notes.
4. Open the draft on GitHub, check the installer, and publish it. The guide for users points to the Releases page.

## Test the build without releasing
Run the **Release** workflow by hand from the Actions tab. It builds the installer and keeps it as the `instapress-windows` workflow artifact, with no release and no tag.

## Checks on every push
`.github/workflows/ci.yml` runs `cargo fmt --check`, `cargo clippy -- -D warnings` and `cargo test` on Windows for each push to `main` and each pull request.

## Notes
- The installer is **not code-signed**, so Windows SmartScreen shows a warning the first time ("Windows protected your PC": More info, then Run anyway). Signing needs a certificate and is not set up.
- Building locally needs the Tauri CLI (`cargo install tauri-cli --version "^2"`) and runs `cargo tauri build` in `src-tauri/`.
