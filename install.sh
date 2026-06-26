#!/usr/bin/env bash
# install.sh — build mundus-animarum and install it into an objectiveai dir
# (default $HOME/.objectiveai), the way the host's `plugins install` lays it out.
#
# Runs build.sh to compile + stage the cli_zip locally, copies the staged plugin
# version dir onto the host, then unpacks the cli_zip in place so the bare binary
# is runnable from cli/.
#
# Usage:
#   bash install.sh [--release] [--dir <objectiveai-dir>]   # --dir defaults to ~/.objectiveai
set -euo pipefail

REL=""
DIR="$HOME/.objectiveai"
while [ "$#" -gt 0 ]; do
  case "$1" in
    --release) REL="--release"; shift ;;
    --dir)     DIR="$2"; shift 2 ;;
    --dir=*)   DIR="${1#--dir=}"; shift ;;
    *) echo "install.sh: unknown arg: $1 (usage: install.sh [--release] [--dir <dir>])" >&2; exit 1 ;;
  esac
done

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$REPO_ROOT"

# ── platform / arch (drives the cli_zip name) ──────────────────────────────
case "$(uname -s)" in
  Linux*)               PLATFORM="linux"   ;;
  Darwin*)              PLATFORM="macos"   ;;
  CYGWIN*|MINGW*|MSYS*) PLATFORM="windows" ;;
  *) echo "install.sh: unsupported OS: $(uname -s)" >&2; exit 1 ;;
esac
case "$(uname -m)" in
  x86_64|amd64)  ARCH="x86_64"  ;;
  arm64|aarch64) ARCH="aarch64" ;;
  *) echo "install.sh: unsupported arch: $(uname -m)" >&2; exit 1 ;;
esac

# ── plugin coords (single source of truth: objectiveai.json) ───────────────
OWNER="$(sed -n -E 's/.*"owner"[[:space:]]*:[[:space:]]*"([^"]+)".*/\1/p' objectiveai.json | head -1)"
NAME="$(sed -n -E 's/.*"name"[[:space:]]*:[[:space:]]*"([^"]+)".*/\1/p' objectiveai.json | head -1)"
VERSION="$(sed -n -E 's/.*"version"[[:space:]]*:[[:space:]]*"([^"]+)".*/\1/p' objectiveai.json | head -1)"
[ -n "$OWNER" ] && [ -n "$NAME" ] && [ -n "$VERSION" ] \
  || { echo "install.sh: could not read owner/name/version from objectiveai.json" >&2; exit 1; }

CLI_ZIP_NAME="$NAME-$PLATFORM-$ARCH.zip"

# ── 1. build + stage into the local plugin tree ────────────────────────────
echo "==> install.sh: build"
bash build.sh ${REL:+"$REL"}

SRC_DIR="$REPO_ROOT/.objectiveai/bin/plugins/$OWNER/$NAME/$VERSION"
[ -f "$SRC_DIR/cli/$CLI_ZIP_NAME" ] \
  || { echo "install.sh: staged cli_zip missing at $SRC_DIR/cli/$CLI_ZIP_NAME" >&2; exit 1; }

# ── 2. copy the staged version dir onto the host ───────────────────────────
DEST_DIR="$DIR/bin/plugins/$OWNER/$NAME/$VERSION"
echo "==> install.sh: copy -> $DEST_DIR"
rm -rf "$DEST_DIR"
mkdir -p "$DEST_DIR"
cp -R "$SRC_DIR/." "$DEST_DIR/"

# ── 3. unpack the cli_zip in place (strip cli/ to just the zip, then extract) ─
CLI_DIR="$DEST_DIR/cli"
find "$CLI_DIR" -mindepth 1 -maxdepth 1 -not -name '*.zip' -exec rm -rf {} +
echo "==> install.sh: unpack $CLI_ZIP_NAME"
case "$PLATFORM" in
  windows)
    powershell.exe -NoProfile -Command \
      "Expand-Archive -Force -LiteralPath '$(cygpath -w "$CLI_DIR/$CLI_ZIP_NAME")' -DestinationPath '$(cygpath -w "$CLI_DIR")'"
    ;;
  *)
    unzip -o -q "$CLI_DIR/$CLI_ZIP_NAME" -d "$CLI_DIR"
    ;;
esac

echo "==> installed -> $DEST_DIR"
