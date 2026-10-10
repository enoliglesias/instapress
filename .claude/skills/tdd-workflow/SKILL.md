---
name: tdd-workflow
description: Strict Red-Green-Refactor test-driven development cycle for any new feature, bug fix or refactor in Rust or Tauri code.
---

# Skill: Test-Driven Development Workflow (`tdd-workflow`)

Use this skill whenever implementing a new feature, bug fix, or refactor in Rust or Tauri code.

## Mandatory TDD Cycle (Red -> Green -> Refactor)

1. **RED (Write Failing Test First)**:
   - Identify the expected behavior or logic change.
   - Write unit tests in `tests/` or in `#[cfg(test)] mod tests` that specify the desired outcome.
   - Run `cargo test` to confirm the test fails as expected.

2. **GREEN (Make Test Pass with Minimal Code)**:
   - Write the simplest production code in `src-tauri/src/` to make the failing test pass.
   - Run `cargo test` to verify all tests pass.

3. **REFACTOR (Clean Up Code & Validate)**:
   - Refactor production and test code for clarity, performance, and idiomatic Rust.
   - Run `rust-check` skill (`cargo fmt`, `cargo clippy -- -D warnings`, `cargo test`).
   - Confirm all lints and tests pass with 0 warnings.
