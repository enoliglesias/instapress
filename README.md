# Instapress

Desktop app (Rust and Tauri v2) that turns one of your own Instagram publications into a WordPress draft:

1. Connect your Instagram account with an access token and pick one of your recent posts or reels.
2. Its caption is loaded into an editable rich text editor, with a suggested title.
3. Choose the picture: the post's own image, a file from your computer, or none.
4. Create the post on your WordPress site. It is always created as a **draft**, so you review and publish it from WordPress.

## Status

Working end to end for the first 25 posts of the connected account. Not implemented yet: more than 25 posts, screenshots of a post, tags, and publishing directly.

Instagram has no official API for reading other people's public posts, so the app only reads the account that owns the token (an Instagram professional account that is a tester of your Meta app). Only publish content you own or have permission to use.

Your Instagram token and your WordPress Application Password are kept in the operating system's credential store (Windows Credential Manager, macOS Keychain, Secret Service). The post text is cleaned before it is sent, so pasted scripts and odd markup never reach your site.

## Download

Get the Windows installer or the macOS disk image (Apple Silicon) from the [Releases page](https://github.com/enoliglesias/instapress/releases). Neither is signed, so the first run shows an operating system warning; the setup guide says how to get past it.

## Setup

Before the first run you need an Instagram token, a WordPress Application Password and two small changes on the site. The step-by-step guide is in [`knowledge/how-to/get-started.md`](knowledge/how-to/get-started.md).

## Development

Requires Rust 1.90 or newer. There is no Node toolchain: the frontend in `ui/` is static.

```sh
cargo test --manifest-path src-tauri/Cargo.toml
cargo run --manifest-path src-tauri/Cargo.toml
```

The living design notes are in [`knowledge/`](knowledge/00-index.md). Contributor and agent rules are in [`CLAUDE.md`](CLAUDE.md).

## Disclaimer

Instapress is an independent project. It is not affiliated with, endorsed by or sponsored by Meta, Instagram or WordPress. Instagram and WordPress are trademarks of their owners.

## Author and license

Enol Iglesias. Released under the [MIT License](LICENSE).
