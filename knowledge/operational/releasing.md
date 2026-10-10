---
title: Building and releasing
status: active
---

# Building and releasing

GitHub Actions (`.github/workflows/release.yml`) builds the Windows installer (NSIS, an `.exe`) on a Windows runner and the macOS disk image (a `.dmg`) on a macOS runner, with the Tauri CLI at a version pinned in the workflow. No Mac is needed to make the dmg. The files are named after the product and the version.

## Make a release
1. Raise `version` in `src-tauri/Cargo.toml` (`Cargo.lock` follows when the project is built), commit it and push.
2. On GitHub create a release with a new tag made of that number prefixed by `v` (for example `v0.2.0`) and **publish** it. Nothing is built from a tag alone: the release and its tag are created by hand.
3. Publishing the release starts the workflow. It refuses to go on if the tag and the version differ, runs the tests, builds both installers (one job per system) and attaches them to the release.
4. Open the release and check that the `.exe` and the `.dmg` are there. The guide for users points to the Releases page.

## Build for a release that already exists
If the build failed or the installer has to be made again, run the **Release** workflow by hand from the Actions tab and give the tag of the release. The installer replaces the one attached before.

## Checks on every push
`.github/workflows/ci.yml` runs `cargo fmt --check`, `cargo clippy -- -D warnings` and `cargo test` on Windows and macOS for each push to `main` and each pull request.

## Notes
- The installer is **not code-signed**, so Windows SmartScreen shows a warning the first time ("Windows protected your PC": More info, then Run anyway). Signing needs a certificate and is not set up.
- The macOS bundle is configured in `src-tauri/tauri.macos.conf.json`, which Tauri merges on macOS only: targets `app` and `dmg`, the `icons/icon.icns` icon (made from the app icon), minimum macOS 11 and `signingIdentity` `-`, which signs the whole bundle ad hoc. The runner `macos-latest` is Apple Silicon, so the dmg is arm64 only (no Intel Macs).
- The dmg is **not signed by a developer and not notarized**. A user who downloads it opens the app with a right click, then **Open**, or runs `xattr -dr com.apple.quarantine Instapress.app` in the Terminal. Notarization needs a paid Apple Developer account and is not set up.
- Building locally needs the Tauri CLI (`cargo install tauri-cli --version "^2"`) and runs `cargo tauri build` in `src-tauri/`.
