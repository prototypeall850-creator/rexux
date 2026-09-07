#!/bin/sh
# rexux installer for Termux/Android — one-line install:
#   curl -fsSL https://raw.githubusercontent.com/prototypeall850-creator/rexux/main/install.sh | sh
#
# Downloads the matching prebuilt binary from GitHub Releases and installs it
# to $PREFIX/bin/rexux. Falls back to building from source when no prebuilt
# binary matches this device.
set -eu

REPO="prototypeall850-creator/rexux"
BIN_NAME="rexux"
INSTALL_DIR="${PREFIX:-/data/data/com.termux/files/usr}/bin"

info() { printf '%s\n' "$*"; }
die() { printf 'error: %s\n' "$*" >&2; exit 1; }

command -v curl >/dev/null 2>&1 || die "curl is required (pkg install curl)"
command -v uname >/dev/null 2>&1 || die "uname is required"

ARCH="$(uname -m)"
case "$ARCH" in
  aarch64|arm64) TARGET="aarch64-linux-android" ;;
  x86_64|amd64) TARGET="x86_64-linux-android" ;;
  *) die "unsupported architecture: $ARCH (need aarch64 or x86_64)" ;;
esac

TAG="${REXUX_VERSION:-latest}"
if [ "$TAG" = "latest" ]; then
  info "resolving latest rexux release..."
  RELEASE_JSON="$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest")" \
    || die "failed to query GitHub releases API"
  TAG="$(printf '%s' "$RELEASE_JSON" | grep -m1 '"tag_name"' | cut -d'"' -f4)"
  [ -n "$TAG" ] || die "could not determine latest release tag"
fi
info "installing rexux $TAG for $TARGET..."

ASSET="$BIN_NAME-$TAG-$TARGET"
URL="https://github.com/$REPO/releases/download/$TAG/$ASSET"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT INT TERM

if ! curl -fsSL -o "$TMP/$BIN_NAME" "$URL"; then
  info "no prebuilt binary at $URL"
  info "falling back to building from source (needs: pkg install rust git clang binutils)..."
  command -v cargo >/dev/null 2>&1 || die "cargo not found; install Rust first (pkg install rust)"
  command -v git >/dev/null 2>&1 || die "git not found (pkg install git)"
  rm -rf "$TMP/src" && git clone --depth 1 --branch "$TAG" "https://github.com/$REPO.git" "$TMP/src" \
    || git clone --depth 1 "https://github.com/$REPO.git" "$TMP/src"
  (cd "$TMP/src" && cargo build --release -p rexux-cli) \
    || die "source build failed"
  cp "$TMP/src/target/release/$BIN_NAME" "$TMP/$BIN_NAME"
fi

chmod +x "$TMP/$BIN_NAME"
mkdir -p "$INSTALL_DIR"
mv "$TMP/$BIN_NAME" "$INSTALL_DIR/$BIN_NAME"

info "installed $INSTALL_DIR/$BIN_NAME"
"$INSTALL_DIR/$BIN_NAME" --version
info "next: export OPENAI_API_KEY=... (or another provider key), then run: rexux"
