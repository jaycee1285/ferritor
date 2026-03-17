---
id: sync-boundary
kind: persistence-boundary
authority:
  - digtwin-backup-layout
mutates:
  - ~/repos/digtwin
  - skill-files-on-disk
observes:
  - skill-entry-paths
  - skill-md-content
  - digtwin-backup-tree
persists_to:
  - ~/repos/digtwin
  - skill-files-on-disk
depends_on:
  - scanner-discovery
staleness_risks:
  - backup-tree-missing-external-scripts
  - hook-backup-only-stores-json-fragment
entrypoints:
  - src/sync.rs
  - src/scanner/digtwin.rs
---

# Sync Boundary

## Purpose
Maps entries to their digtwin backup destinations, performs backup and delete operations, and evaluates whether an entry is synced, modified, or unbacked-up. This is the durability boundary between live device state and the Git-backed backup tree.

## Scope of Touch
Safe to edit when changing:
- backup destination layout
- per-surface backup behavior
- external script capture rules for skill backups
- sync status heuristics

Risky to edit when changing:
- directory-copy semantics for skill backups
- delete semantics for skills vs rules
- hook backup format
- anything that changes green/yellow/red meaning

## Authority Notes
The backup layout defined in `backup_dest()` is the canonical mapping the rest of the app relies on. The current session also established a narrower contract for external script capture: only hard-coded absolute `/home/john/...` `.js` and `.ts` paths referenced from `SKILL.md` are mirrored under `_external/` in the backup tree.

## Links
- [Scanner Discovery](scanner-discovery.md)
- [App Shell](app-shell.md)
