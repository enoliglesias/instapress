---
title: Building and releasing
status: active
---

# Building and releasing

GitHub Actions (`.github/workflows/release.yml`) builds the Windows installer (NSIS, an `.exe`) on a Windows runner and the macOS disk image (a `.dmg`) on a macOS runner, with the Tauri CLI at a version pinned in the workflow. No Mac is needed to make the dmg. The files are named after the product and the version.

## Make a release
1. Raise `version` in `src-tauri/Cargo.toml` (`Cargo.lock` follows when the project is built), commit it and push to `main`.
2. That is all. The **Release** workflow starts on a push to `main` that touches `Cargo.toml`, reads the version and works in three jobs:
   - **prepare** creates a *draft* release `v<version>` (with generated notes) unless a release for that version already exists, in which case the run ends without doing anything;
   - **installers** runs the tests and builds the Windows `.exe` and the macOS `.dmg` on their own runners, and attaches each one to the draft;
   - **publish** publishes the release, which creates its tag on the commit that was built, only when both installers are attached.
3. A failed build leaves the draft unpublished and nothing is announced. Fix it and run the workflow again.

Editing `Cargo.toml` without changing `version` does nothing, and so does a push of an already released version.

## Build again
Run the **Release** workflow by hand from the Actions tab. It rebuilds the version found in the branch you choose, replaces the files attached to that release and publishes it. Use it after a failed run, or to refresh the installers of a version without changing the number.

## Checks on every push
`.github/workflows/ci.yml` runs `cargo fmt --check`, `cargo clippy -- -D warnings` and `cargo test` on Windows and macOS for each push to `main` and each pull request.

## Notes
- The installer is **not code-signed**, so Windows SmartScreen shows a warning the first time ("Windows protected your PC": More info, then Run anyway). Signing needs a certificate and is not set up.
- The macOS bundle is configured in `src-tauri/tauri.macos.conf.json`, which Tauri merges on macOS only: targets `app` and `dmg`, the `icons/icon.icns` icon (made from the app icon), minimum macOS 11 and `signingIdentity` `-`, which signs the whole bundle ad hoc. The runner `macos-latest` is Apple Silicon, so the dmg is arm64 only (no Intel Macs).
- The dmg is **not signed by a developer and not notarized**. A user who downloads it opens the app with a right click, then **Open**, or runs `xattr -dr com.apple.quarantine Instapress.app` in the Terminal. Notarization needs a paid Apple Developer account and is not set up.
- Building locally needs the Tauri CLI (`cargo install tauri-cli --version "^2"`) and runs `cargo tauri build` in `src-tauri/`.
