# Ferritor

## Project Summary

Ferritor is an egui desktop app for browsing the live filesystem and editing text files on disk with a low-chrome, keyboard-capable interface. It preserves the digtwin design basics: GTK-derived desktop colors, Spline Sans / Spline Sans Mono typography, and syntect-backed code rendering.

This repo started as DotAgent, a Claude/Codex skill explorer. That phase is now legacy context, not the active product direction.

## Current Product Shape

- Left pane: current directory, filter field, folder-open action, terminal action, file/folder list, `..` parent row.
- Right pane: file viewer/editor with syntax highlighting, dirty-state handling, and save flow.
- Status bar: current directory plus `Viewing` / `Editing` mode.
- Keyboard model: pane switching, parent navigation, directory open, edit, save, and terminal launch.

## Design Contract

- Colors come from `~/.config/gtk-4.0/gtk.css`.
- Syntax colors come from `~/.config/syntect/current.tmTheme`.
- Typography uses the bundled Spline Sans / Spline Sans Mono fonts.
- Layout stays intentionally sparse and progressive-disclosure oriented per `design-basics.md`.

## Implemented

### Product Rename

- [x] Renamed runtime package/window surface from DotAgent to Ferritor
- [x] Reframed the app around filesystem navigation instead of skill/hook surfaces

### Filesystem Surface

- [x] Current-directory state in the main app shell
- [x] Live directory listing with directories sorted before files
- [x] `..` parent-directory row
- [x] Click-to-open folder navigation
- [x] File filter field in the left pane
- [x] Path-entry open-directory dialog
- [x] `Ctrl+Shift+O` directory-open keybind
- [x] `Ctrl+Up` move-to-parent keybind
- [x] File-tree cursor lock regression fixed after human smoke

### Editor Surface

- [x] View selected text files in the right pane
- [x] Enter edit mode with `E`
- [x] Save with `Ctrl+S`
- [x] Dirty-state guard on pane/navigation changes
- [x] UTF-8 load error surfaced in the UI instead of silent failure

### Syntax / Presentation

- [x] Syntect-backed highlighting in view mode and edit mode
- [x] Plain-text fallback for unknown file types
- [x] Explicit JS/TS syntax resolution fallback path
- [x] GTK-derived visual theme preserved
- [x] Spline Sans font stack preserved

### Operator Conveniences

- [x] `Ctrl+H` help overlay
- [x] Terminal button beside the left-pane controls
- [x] `Ctrl+T` opens kitty in the current directory

### Packaging / Release

- [x] Flake default package added for release builds
- [x] `release.sh` updated for Ferritor naming and GitHub target

## Next Likely Work

- [ ] Replace the path-entry directory dialog with a native folder picker
- [ ] Add a lightweight Ferritor state/index file for recent folders, recent files, and reopen state
- [ ] Add a compact formatting toolbar or Markdown/editor actions if that still proves useful in daily use
- [ ] Decide how to handle binary files, non-UTF-8 files, and very large files
- [ ] Clean out or archive the legacy DotAgent code paths once no longer needed for reference

## Indexing / Persistence Steps Later

- [ ] Define the Ferritor state root and on-disk format
  Suggested direction: `~/.ferritor/` or another user-owned app state directory
- [ ] Capture recent directories
- [ ] Capture recently opened files
- [ ] Restore the last working directory on startup
- [ ] Consider whether cursor/selection state is worth restoring per file
- [ ] Decide whether history belongs in one state file or split indexes

## Key Files

- `src/app.rs` — main Ferritor app shell, filesystem navigation, viewer/editor UX, terminal launch
- `src/syntax.rs` — syntax resolution and highlighting
- `src/theme.rs` — GTK color parsing
- `flake.nix` — dev shell plus default release package
- `release.sh` — tarball and GitHub release upload flow
- `traverse/` — architecture locality notes for later agents

## Smoke Test

```sh
nix develop -c cargo run
```

Check:
- browse folders from the left pane
- open and edit a text file
- save with `Ctrl+S`
- use `Ctrl+Up`, `Ctrl+Shift+O`, and `Ctrl+T`
- confirm kitty opens in the current directory

## Session Notes

- Human smoke on 2026-03-17 passed for the filesystem/browser/editor transition and terminal launch.
- Kitty may emit shell-startup noise on this NixOS environment while still opening in the correct directory.
- Legacy DotAgent scanner/sync code is no longer on the active binary path but remains on disk for now.
