# Smoke Human 2026-03-17

## What Changed
- Reframed the app from DotAgent skill/hook management into Ferritor, a filesystem browser plus basic text editor.
- Replaced the old scanner/sync-centered left pane with live directory navigation, parent-directory traversal, file filtering, and a directory-open dialog.
- Kept the GTK-driven visual system and syntect-backed viewer/editor, with explicit JavaScript/TypeScript fallback handling.
- Added a kitty launch action rooted at the current directory.
- Added a real flake package target so release packaging can build a default artifact.

## Command

```sh
nix develop -c cargo run
```

## What John Tested
- Opened `.md` and `.rs` files and confirmed they rendered and edited correctly.
- Verified the directory-open flow works from button, keybind, and direct path entry.
- Reproduced and then rechecked the file-tree cursor lock when switching back from the viewer pane.
- Triggered the terminal-launch action by button and keybind and confirmed kitty opened in the current directory.

## John Observed
- Human smoke passed.
- The initial cursor-lock regression was correctly diagnosed as state-related and fixed in follow-up.
- On NixOS, kitty emitted shell-startup noise (`shopt: progcomp: invalid shell option name` plus prompt escapes), but the terminal still opened in the correct directory.

## Passed
- Directory browsing, parent traversal, file selection, and file viewing worked.
- Text editing and save flow worked for normal text files.
- Cursor movement in the file tree no longer locked to the currently open file.
- Folder-open button and keybind worked.
- Terminal button and `Ctrl+T` launched kitty in the current directory.

## Failed Or Suspicious
- TypeScript highlighting was initially reported as missing and then reworked with explicit fallback logic; follow-up smoke passed for the shipped app behavior overall, but this remains worth rechecking against a few representative `.ts` files later.
- Kitty shell startup noise appears environmental rather than app-side.

## Current Gaps
- The open-directory flow is still a path-entry dialog rather than a native folder picker.
- The app still treats non-UTF-8 content as unsupported text rather than handling binary/encoded files more gracefully.
- Root docs and some traverse nodes from the old DotAgent phase needed cleanup in this same session because they no longer matched the binary.
