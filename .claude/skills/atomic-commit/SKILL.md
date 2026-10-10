---
name: atomic-commit
description: Structure changes into atomic Conventional Commits and write commit messages. Use when preparing commits (only when the user asks for them).
---

# Skill: Conventional Commit & Atomic Changes (`atomic-commit`)

Use this skill when preparing commit messages or structuring code changes into atomic commits following Conventional Commits and GitHub best practices.

## Guidelines

1. **Atomic Principle**:
   - Each commit must represent one single logical change.
   - Separate refactoring from new features or bug fixes.
   - Never mix formatting fixes with business logic changes.

2. **Conventional Commit Format**:
   ```
   <type>(<scope?>): <subject>

   <body (optional, explaining WHY, not WHAT)>
   ```

   **Types**:
   - `feat`: A new feature
   - `fix`: A bug fix
   - `docs`: Documentation only changes
   - `style`: Changes that do not affect the meaning of the code (formatting, missing semi-colons, etc.)
   - `refactor`: A code change that neither fixes a bug nor adds a feature
   - `perf`: A code change that improves performance
   - `test`: Adding missing tests or correcting existing tests
   - `chore`: Changes to the build process or auxiliary tools/libraries

3. **Rules**:
   - Subject line: imperative mood ("add parser", not "added parser" or "adds parser"), no period at the end, max 72 characters.
   - Body: explain *why* the change was made and any trade-offs or design decisions.
   - Strictly in **English**.
   - **Safety**: Do NOT execute git commit/push commands automatically; propose the exact commit message and staged diff structure for user confirmation.
