---
id: gtk-theme-loader
kind: module
authority:
  - ~/.config/gtk-4.0/gtk.css
mutates: []
observes:
  - gtk-css-define-color-lines
persists_to: []
depends_on: []
staleness_risks:
  - startup-only-theme-load
entrypoints:
  - src/theme.rs
  - src/app.rs
---

# GTK Theme Loader

## Purpose
Parses named GTK color definitions from the user’s `gtk.css` and exposes semantic color accessors used by the egui shell. This keeps the app visually aligned with the device theme without shipping its own palette.

## Scope of Touch
Safe to edit when changing:
- supported color formats
- semantic accessor naming
- fallback behavior for missing colors

Risky to edit when changing:
- assumptions about stable GTK variable names
- panel/selection/background mapping in the app shell

## Authority Notes
The GTK CSS file is the authoritative palette input. The current implementation loads it at startup only, so runtime theme changes require an app restart.

## Links
- [App Shell](app-shell.md)
- [Syntax Highlighter](syntax-highlighter.md)
