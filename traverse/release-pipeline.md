---
id: ferritor-release-pipeline
kind: module
authority:
  - flake-default-package
mutates:
  - github-release-artifact
  - local-release-tarball
observes:
  - Cargo.toml
  - flake.nix
  - Cargo.lock
  - result/bin/ferritor
persists_to:
  - repo-root-tarball
  - github-release-assets
depends_on:
  - gh-cli
  - nix-build-output
staleness_risks:
  - app-name-repo-name-drift
  - flake-package-mismatch
entrypoints:
  - flake.nix
  - release.sh
---

# Ferritor Release Pipeline

## Purpose
Builds Ferritor as a flake package, stages the release binary, creates a compressed tarball, and uploads that artifact to the GitHub repo release when upload is enabled.

## Scope of Touch
Safe to edit when changing:
- release repo target
- tarball naming
- package lookup and staging rules

Risky to edit when changing:
- interpreter/rpath rewriting
- flake package wiring
- GitHub release create/upload behavior

## Authority Notes
`flake.nix` is authoritative for whether `nix build .#default` can produce the release binary. `release.sh` is the packaging and upload workflow layered on top of that build output.

## Links
- [App Shell](app-shell.md)
- [Feature Index](feature-index.md)
