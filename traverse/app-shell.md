---
id: ferritor-app-shell
kind: ui-surface
authority:
  - current-directory-state
  - selected-file-state
mutates:
  - current-directory-state
  - selected-file-state
  - loaded-file-content
  - edit-buffer
  - focus-pane
  - shell-launch
observes:
  - filesystem-directory-contents
  - gtk-theme-colors
  - syntax-highlighter
  - keyboard-input
persists_to:
  - files-on-disk
depends_on:
  - syntax-highlighter
  - gtk-theme-loader
staleness_risks:
  - in-memory-loaded-content
  - nav-cursor-vs-open-file-state
entrypoints:
  - src/app.rs
  - src/main.rs
---

# Ferritor App Shell

## Purpose
Owns the egui application state for Ferritor's daily-use workflow: browse the live filesystem, open a text file, switch between viewing and editing, save back to disk, and open a terminal rooted at the current directory.

## Scope of Touch
Safe to edit when changing:
- sidebar navigation behavior
- header/status bar presentation
- keyboard shortcuts and pane focus rules
- edit-mode UX and save prompts
- terminal-launch wiring

Risky to edit when changing:
- dirty-state navigation guards
- current-directory transitions
- selection/cursor coupling in the file list
- assumptions about UTF-8-only file loading

## Authority Notes
This surface is authoritative only for transient UI state such as the current directory, selected file, open buffer, and pane focus.
The filesystem is the source of truth for directory contents and saved file content.

## Links
- [Syntax Highlighter](syntax-highlighter.md)
- [GTK Theme Loader](gtk-theme-loader.md)
- [Release Pipeline](release-pipeline.md)
