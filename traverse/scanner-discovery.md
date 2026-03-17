---
id: scanner-discovery
kind: module
authority:
  - surface-entry-list
  - provenance-grouping
mutates: []
observes:
  - ~/.claude/skills
  - ~/.claude/settings.json
  - ~/.codex/skills
  - ~/.codex/rules
  - ~/repos/digtwin
persists_to: []
depends_on:
  - sync-boundary
staleness_risks:
  - live-filesystem-state
  - hook-script-resolution
entrypoints:
  - src/scanner/mod.rs
  - src/scanner/claude.rs
  - src/scanner/codex.rs
  - src/scanner/digtwin.rs
---

# Scanner Discovery

## Purpose
Builds the combined `SkillEntry` list across Claude skills, Claude hooks, Codex skills, and Codex rules. It also derives provenance grouping metadata and computes the initial green/yellow/red sync status against the digtwin backup tree.

## Scope of Touch
Safe to edit when changing:
- which filesystem surfaces are scanned
- YAML frontmatter grouping behavior
- hook content shaping and script inlining
- sync-status computation rules

Risky to edit when changing:
- `SkillEntry` shape
- assumptions about Codex default provenance
- backup path alignment with `sync::backup_dest`
- hook identity naming, since UI selection relies on stable-ish names

## Authority Notes
`SkillEntry` is the app’s canonical discovered-entry projection. It is derived from the filesystem each scan and should be treated as disposable state, not durable truth.

## Links
- [App Shell](app-shell.md)
- [Sync Boundary](sync-boundary.md)
