---
name: sync-knowledge
description: Read and update the living knowledge base (knowledge/) BEFORE implementing any feature, refactor or architectural change.
---

# Skill: Sync Knowledge Base (`sync-knowledge`)

Use this skill **BEFORE** starting implementation of any feature, refactor, or architectural change.

## Protocol

1. **Read Knowledge Base**:
   - Read `knowledge/00-index.md` and relevant domain/architecture files in `knowledge/` to establish context on existing modules, models, and constraints.

2. **Reflect Target State**:
   - Determine what changes will occur in the architecture, data models, or state logic.
   - Edit the relevant markdown files in `knowledge/` immediately to reflect the **new state of the system as it will be after implementation**.
   - Prune any obsolete logic or outdated structural descriptions.

3. **Rule**:
   - Never write production code before `knowledge/` accurately describes the target application state.
   - Maintain the snapshot principle (no incremental changelogs; keep current state only).
