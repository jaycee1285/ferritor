---
id: ferritor-feature-index
kind: index
authority: []
mutates: []
observes:
  - traverse-node-docs
persists_to: []
depends_on:
  - ferritor-app-shell
  - syntax-highlighter
  - gtk-theme-loader
  - ferritor-release-pipeline
staleness_risks:
  - legacy-dotagent-traverse-docs
entrypoints:
  - traverse/app-shell.md
  - traverse/syntax-highlighter.md
  - traverse/gtk-theme-loader.md
  - traverse/release-pipeline.md
---

# Ferritor Feature Index

## Purpose
Provides a locality map for the currently wired Ferritor surface: filesystem browsing, text viewing/editing, theme-driven presentation, syntax coloring, and release packaging.

## Feature Neighborhoods
- Browse directories, open files, edit/save text, and launch a terminal: [App Shell](app-shell.md)
- Resolve syntax highlighting for Markdown, Rust, JavaScript, and TypeScript-adjacent files: [Syntax Highlighter](syntax-highlighter.md)
- Load the GTK-derived desktop palette at startup: [GTK Theme Loader](gtk-theme-loader.md)
- Build and package the release artifact that ships to GitHub: [Release Pipeline](release-pipeline.md)

## Notes
- The old scanner/sync traverse docs are now legacy context from the DotAgent phase; they are no longer on the active binary path.
- The highest-risk staleness surfaces are startup-cached theme/highlighter state and root-level docs that still describe the pre-Ferritor skill-sync product.
