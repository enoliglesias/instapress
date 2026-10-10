---
name: rust-check
description: Run cargo fmt --check, clippy with -D warnings, tests and build on src-tauri before finishing any Rust or Tauri change.
---

# Skill: Rust & Tauri Quality Check (`rust-check`)

Run this skill before finishing any task involving Rust or Tauri code changes in `src-tauri/` to guarantee correctness, formatting, and lint compliance.

## Steps

1. **Format Check**:
   ```bash
   cargo fmt --manifest-path src-tauri/Cargo.toml --check
   ```
   If formatting fails, run `cargo fmt --manifest-path src-tauri/Cargo.toml`.

2. **Clippy Linting**:
   ```bash
   cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --all-features -- -D warnings
   ```

3. **Test Suite**:
   ```bash
   cargo test --manifest-path src-tauri/Cargo.toml
   ```

4. **Build Check**:
   ```bash
   cargo build --manifest-path src-tauri/Cargo.toml
   ```

## Rules
- All checks must pass cleanly with 0 warnings or errors.
- Do not bypass warnings or use `#![allow(...)]` without strong documented justification.
