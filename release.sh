#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")" && pwd)"
CARGO_TOML="$REPO_ROOT/Cargo.toml"

VERSION=$(grep -oP '^version\s*=\s*"\K[^"]+' "$CARGO_TOML" | head -1)
TAG="v${VERSION}"
APP_NAME="ferritor"
ARCH="$(uname -m)"
PLATFORM="$(uname -s | tr '[:upper:]' '[:lower:]')"
TARBALL="${APP_NAME}-${TAG}-${PLATFORM}-${ARCH}.tar.xz"
GH_REPO="${GH_REPO:-jaycee1285/ferritor}"

echo "==> Building ${APP_NAME} ${TAG} (${PLATFORM}/${ARCH})"

cd "$REPO_ROOT"
nix build .#default

NIX_RESULT="$REPO_ROOT/result"
if [[ ! -d "$NIX_RESULT/bin" ]]; then
  echo "ERROR: nix build output not found at ${NIX_RESULT}/bin"
  exit 1
fi

STAGING="$(mktemp -d)"
trap 'chmod -R u+w "$STAGING" && rm -rf "$STAGING"' EXIT

mkdir -p "$STAGING/bin"

if [[ -f "$NIX_RESULT/bin/.${APP_NAME}-wrapped" ]]; then
  REAL_BIN="$NIX_RESULT/bin/.${APP_NAME}-wrapped"
elif [[ -f "$NIX_RESULT/bin/.${APP_NAME}-wrapped_" ]]; then
  REAL_BIN="$NIX_RESULT/bin/.${APP_NAME}-wrapped_"
else
  REAL_BIN="$NIX_RESULT/bin/${APP_NAME}"
fi

if [[ ! -f "$REAL_BIN" ]]; then
  echo "ERROR: built binary not found at ${REAL_BIN}"
  exit 1
fi

cp "$REAL_BIN" "$STAGING/bin/${APP_NAME}"
chmod u+wx "$STAGING/bin/${APP_NAME}"

if command -v strip >/dev/null 2>&1; then
  echo "==> Stripping binary"
  strip --strip-unneeded "$STAGING/bin/${APP_NAME}" || true
fi

echo "==> Stripping Nix store paths for cross-machine portability"
nix shell nixpkgs#patchelf --command patchelf --remove-rpath "$STAGING/bin/${APP_NAME}"
nix shell nixpkgs#patchelf --command patchelf --set-interpreter /lib64/ld-linux-x86-64.so.2 "$STAGING/bin/${APP_NAME}"

echo "==> Creating ${TARBALL}"
tar -cJf "$REPO_ROOT/$TARBALL" -C "$STAGING" bin

SIZE=$(du -h "$REPO_ROOT/$TARBALL" | awk '{print $1}')
echo "==> Tarball: ${REPO_ROOT}/${TARBALL} (${SIZE})"

if [[ "${SKIP_UPLOAD:-0}" == "1" ]]; then
  echo "==> SKIP_UPLOAD=1, leaving tarball at ${REPO_ROOT}/${TARBALL}"
elif [[ -n "$GH_REPO" ]]; then
  echo "==> Uploading to GitHub release ${TAG} on ${GH_REPO}"
  if gh release view "$TAG" --repo "$GH_REPO" &>/dev/null; then
    gh release upload "$TAG" "$REPO_ROOT/$TARBALL" --repo "$GH_REPO" --clobber
  else
    gh release create "$TAG" "$REPO_ROOT/$TARBALL" \
      --repo "$GH_REPO" \
      --title "${APP_NAME} ${TAG}" \
      --notes "${APP_NAME} ${TAG}" \
      --latest
  fi
fi

echo "==> Done"
