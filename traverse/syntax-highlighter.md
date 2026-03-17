---
id: syntax-highlighter
kind: module
authority:
  - ~/.config/syntect/current.tmTheme
mutates: []
observes:
  - source-file-extension
  - source-file-name
  - file-content
  - syntect-theme-file
persists_to: []
depends_on:
  - syntect-defaults
staleness_risks:
  - once-lock-theme-cache
  - bundled-syntax-name-availability
entrypoints:
  - src/syntax.rs
  - src/app.rs
---

# Ferritor Syntax Highlighter

## Purpose
Owns syntax-colored rendering for both read mode and edit mode. It resolves a syntax from filename, extension, or file-path heuristics, then builds egui `LayoutJob` output using the active `~/.config/syntect/current.tmTheme`.

## Scope of Touch
Safe to edit when changing:
- extension-to-syntax matching
- JavaScript/TypeScript fallback behavior
- text formatting produced for egui
- editor/viewer syntax rendering behavior

Risky to edit when changing:
- theme loading and fallback behavior
- global cache lifetime
- file-type assumptions for unknown extensions

## Authority Notes
The theme file on disk is the visual source of truth, but the loaded theme is cached in a `OnceLock` for the lifetime of the process. Syntax availability still depends on the bundled syntect grammar set present at build time.

## Links
- [App Shell](app-shell.md)
- [GTK Theme Loader](gtk-theme-loader.md)
