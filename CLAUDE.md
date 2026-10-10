# CLAUDE.md

Guidelines for working in this repository (`instapress`, a Rust & Tauri desktop app).

## What the app does

Loads the caption and image of one of the user's own Instagram posts or reels through the Instagram API (the user's access token, kept in the OS credential store), puts the caption into an editable rich text editor, lets the user choose the picture (the post's own image, a file, or none), and creates the result as a publication post on a WordPress site through its REST API. The post is always created as a draft: the user reviews and publishes it from WordPress.

Product constraints to keep in mind:

- Instagram has no official API for reading other people's public posts and scraping the public web does not return captions. The app reads only the account that owns the token.
- Publishing someone else's text or photos requires their permission. The UI should make the origin of the content visible and never create a post without an explicit user action.
- The Instagram token and the WordPress Application Password live in the OS credential store, never in plain files, URLs, logs or error messages, and are never sent back to the webview.
- The editor's HTML is untrusted: it is sanitized in the backend before it leaves the app.

## Agents & Skills Configuration

This repository includes specialized Claude Code agents and skills under `.claude/` aligned with official community standards:

- **Agents** (`.claude/agents/`):
  - `knowledge-manager`: Maintains the living knowledge base under `knowledge/` (structured via [C4 Model](https://c4model.com/) and [Diátaxis Framework](https://diataxis.fr/)) as a non-incremental snapshot before implementing code.
  - `rust-reviewer`: Senior Rust reviewer enforcing idiomacy, ownership, TDD, and zero warnings ([Rust API Guidelines](https://rust-lang.github.io/api-guidelines/)).
  - `tauri-architect`: Tauri v2 and async/Tokio integration expert ([Tauri v2 Docs](https://v2.tauri.app/), [Tokio Tutorial](https://tokio.rs/tokio/tutorial)).
  - `git-workflow`: Git workflow and release manager for atomic commits and Conventional Commits ([Conventional Commits](https://www.conventionalcommits.org/)).
- **Skills** (`.claude/skills/`):
  - `sync-knowledge`: Reads and updates the living knowledge base (`knowledge/`) BEFORE code implementation.
  - `tdd-workflow`: Strict Red-Green-Refactor test-driven development cycle.
  - `rust-check`: Runs formatting (`cargo fmt`), clippy (`-D warnings`), tests, and build checks.
  - `atomic-commit`: Structures changes into atomic Conventional Commits.

### When to use them

- Write or update `knowledge/` **before** the code of a change (`sync-knowledge`), and write the failing tests first (`tdd-workflow`).
- When a feature or group of changes is done and **before asking for a commit**, run `rust-reviewer` on the Rust that changed; if the change touches `tauri.conf.json`, `capabilities/`, IPC commands, windows or `ui/`, run `tauri-architect` too. When behavior changed, have `knowledge-manager` check that `knowledge/` matches the code.
- The reviewers only read. Report their findings to the user **before** fixing them, and fix only what the user accepts.
- Finish every Rust change with the `rust-check` gates, and prepare commits with `atomic-commit` (only when the user asks for them).

---

## Language

- The user may speak Spanish in chat, but **everything written to the repository must be in English**: code, identifiers, comments, docs, commit messages, PR/issue text, error messages, log output and test names.
- Chat replies may follow the language the user uses.
- Exception: user-facing publication content (the text extracted from Instagram and the posts published to WordPress) is data and keeps its original language.

## Strict rules: Git and remote

**Do not do anything that changes git history or touches the remote unless the user explicitly asks for it at that moment.**

- Do NOT run `git commit`, `git push`, `git pull`, `git fetch`, `git merge`, `git rebase`, `git reset`, `git tag`, `git stash`, `git checkout`/`git switch` to another branch, or `git branch` (create/delete) on your own initiative.
- Do NOT create, edit or close PRs, issues, comments, releases or anything else on GitHub (`gh ...`) without an explicit request.
- Do NOT publish anything (crates.io, artifacts, gists, etc.).
- Authorization for one action (e.g. "commit this") does NOT extend to the next one (e.g. "and push"). Ask again.
- Read-only commands are allowed: `git status`, `git diff`, `git log`, `git show`, `git blame`.
- If you think a commit or push would be appropriate, suggest it and wait for confirmation.
- When the user asks for a commit: clear imperative messages, one logical change per commit, no unrelated files. Never use `--no-verify`, `--force` or `--amend` unless explicitly requested.
- Do NOT add `Co-Authored-By` trailers or any other AI attribution lines to commit messages or PR descriptions.
- Do not modify git configuration or credentials.

## General guidelines

- Authorship text (Cargo `authors`, installer publisher/copyright, `LICENSE`, About dialog, README) always lists Enol Iglesias only. This does NOT apply to git commit messages or trailers (no co-author trailers).

- Do only what is asked; do not refactor or "improve" unrelated code.
- Read the surrounding code before editing and match its style, naming and comment density.
- Prefer small, reviewable changes. If the task is ambiguous or risky, ask first.
- Do not add dependencies without justification; prefer the standard library when reasonable.
- Leave no dead code, debug `println!`s or context-free `TODO`s.
- Comments explain *why*, not *what*. No obvious comments.
- Never delete or overwrite user files without confirmation.
- Never commit or print secrets, tokens, Application Passwords or personal paths.

## Rust best practices

### Style and quality

- Format with `cargo fmt`; code must pass `cargo clippy --all-targets --all-features -- -D warnings`.
- Before considering a task done: `cargo fmt --check`, `cargo clippy`, `cargo test` and `cargo build`.
- Declare the edition and minimum Rust version (`rust-version`) in `Cargo.toml`.
- Naming: `snake_case` for functions/variables/modules, `UpperCamelCase` for types and traits, `SCREAMING_SNAKE_CASE` for constants.

### Errors

- No `unwrap()`/`expect()`/`panic!` in production code; allowed in tests and, with a clear `expect("reason")`, for truly impossible invariants.
- Use `Result` and propagate with `?`. Typed errors (`thiserror`) in library code; `anyhow` with `.context(...)` in binaries/`main`.
- Error messages must say what failed and on which input (URL, endpoint, HTTP status).
- Unexpected Instagram markup or a failed request must not crash the program: surface a clear error to the UI and continue.

### Design

- Model with types: enums for states, newtypes for IDs and URLs (avoid loose `String`/`u32` with different meanings).
- Make invalid states unrepresentable; prefer `Option`/`enum` over sentinel values.
- Prefer small, pure functions; separate parsing, domain logic and I/O (Instagram extraction, WordPress client and UI commands live in separate modules).
- Minimal API surface: private or `pub(crate)` by default; expose only what is needed.
- Derive/implement the usual traits (`Debug`, `Clone`, `PartialEq`, `Default`, ...) where it makes sense.
- Avoid `unsafe`; if unavoidable, isolate it, document with `// SAFETY:` and justify it.

### Ownership and performance

- Accept `&str`/`&[T]` instead of `String`/`Vec<T>` when ownership is not needed.
- Avoid unnecessary `clone()`s; do not optimize without measuring.
- Prefer iterators over index loops; avoid needless intermediate collections.
- Stream large downloads (images) instead of loading everything into memory when reasonable.
- Use deterministic keys or `BTreeMap` when output order matters.

### Serde and data

- Use `#[derive(Deserialize)]` with explicit types; `#[serde(default)]`/`Option` for optional fields; `#[serde(deny_unknown_fields)]` only for formats we own.
- Tolerate Instagram and WordPress response changes: never assume a field is always present.

### Tests

- Unit tests in `#[cfg(test)] mod tests` next to the code; integration tests in `tests/`.
- Cover edge cases: empty input, malformed URLs, missing caption, private profile, HTTP errors.
- Small, versioned fixtures in `tests/fixtures/` (trimmed HTML/JSON); no full real pages or personal data.
- Tests must be deterministic and must not depend on the network or the user's filesystem; mock HTTP.

### Documentation and dependencies

- `///` on public items; `//!` at module/crate level when useful. Use doc-tests for examples where helpful.
- Keep `Cargo.lock` committed (this is an application).
- Vet dependencies: maintained, compatible license, minimal features (`default-features = false` when possible).
- Consider `cargo audit` and `cargo deny` for vulnerabilities and licenses.

### Concurrency

- If async is not needed, do not use it. Prefer channels/simple structures over `Arc<Mutex<_>>` everywhere.
- Do not block inside async code; do not hold locks across `.await`.

## GitHub best practices (apply when the user requests the action)

- **Branches**: work on feature branches (`feat/...`, `fix/...`, `docs/...`, `chore/...`), not directly on `main`.
- **Commits**: small and atomic, imperative message (subject ≤ 72 chars, body explaining why). Conventional Commits recommended (`feat:`, `fix:`, `docs:`, `refactor:`, `test:`, `chore:`).
- **Pull requests**: clear title; description covering what changed, why, how it was tested and risks. Keep them small and focused; link issues (`Closes #N`).
- **Review**: do not mix refactors with functional changes; address review comments with follow-up commits rather than rewriting already-reviewed history.
- **History**: never `push --force` to `main`; use `--force-with-lease` only on own branches and with explicit permission.
- **CI**: checks (fmt, clippy, test) must pass before merge. Never skip hooks or branch protections.
- **Issues**: reproducible, with context and acceptance criteria; use labels.
- **Security**: never commit secrets, tokens, WordPress credentials, downloaded Instagram content or personal data; keep `.gitignore` up to date.
- **Releases**: semantic versioning, updated changelog, tags only when the user asks.
- **Content licensing**: respect Instagram's terms of use and the rights of the original authors of any text or image the app handles.
