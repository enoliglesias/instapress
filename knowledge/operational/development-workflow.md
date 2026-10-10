---
title: Development Workflow & TDD Protocol
status: active
---

# Development Workflow & TDD Protocol

## TDD cycle
Every feature or fix follows Red -> Green -> Refactor:
1. **RED**: failing test in `#[cfg(test)] mod tests` or `src-tauri/tests/`.
2. **GREEN**: minimal code in `src-tauri/src/`.
3. **REFACTOR**: clean up with tests green.

Network, filesystem and credential access sit behind traits or thin adapters so everything else is unit-testable with fakes. Tests never touch the network.

## Quality gates (`rust-check`)
All must pass with zero warnings:
- `cargo fmt --manifest-path src-tauri/Cargo.toml --check`
- `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --all-features -- -D warnings`
- `cargo test --manifest-path src-tauri/Cargo.toml`
- `cargo build --manifest-path src-tauri/Cargo.toml`

## Knowledge sync (`sync-knowledge`)
Update `knowledge/` to the target state before implementing code that changes architecture, data structures or behavior.

## Constraints
- Node/npm is not installed: no npm scripts. The frontend is static files in `ui/` (ADR 0002).
- Git history and remote operations only on explicit user request (see `CLAUDE.md`).
