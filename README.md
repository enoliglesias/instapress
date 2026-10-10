# recipe-sync

Desktop app (Rust and Tauri v2) that turns an Instagram recipe into a WordPress post:

1. Paste the URL of a public Instagram profile, post or reel.
2. The caption is extracted and loaded into an editable rich text editor.
3. Attach a photo, or use the post's own image or a screenshot of it.
4. Publish it as a post on your WordPress site.

## Status

Early scaffold. Only step 1 is partly there: the window recognizes whether a URL is a profile, a post or a reel. Caption extraction, the editor, images and the WordPress client are not implemented yet.

Instagram has no official API for reading other people's public posts, so extraction is best-effort and will always have a manual paste fallback. Only publish content you own or have permission to use.

## Development

Requires Rust 1.90 or newer. There is no Node toolchain: the frontend in `ui/` is static.

```sh
cargo test --manifest-path src-tauri/Cargo.toml
cargo run --manifest-path src-tauri/Cargo.toml
```

The living design notes are in [`knowledge/`](knowledge/00-index.md). Contributor and agent rules are in [`CLAUDE.md`](CLAUDE.md).

## Authors and license

Enol Iglesias and Rubén Sierra. Released under the [MIT License](LICENSE).
