#!/usr/bin/env bash
# build.sh — build mundus-animarum into the local plugin tree, the same way the
# release workflow (.github/workflows/release.yml) packages it.
#
# Compiles the CLI (debug by default, --release for a release build) and zips
# the bare binary as the release-named cli_zip — the exact asset name from
# objectiveai.json's `cli_zip` map — then drops it, UNEXTRACTED, into the plugin
# tree under .objectiveai, with the manifest at the version-dir head:
#
#   .objectiveai/bin/plugins/<owner>/<name>/<version>/
#     objectiveai.json                       ← the manifest
#     cli/<name>-<os>-<arch>.zip             ← the cli_zip (the bare binary)
#
# This mirrors what the host's `plugins install` lays down, so a test harness
# (or the host) can unpack the cli_zip in place. Coords + version come from
# objectiveai.json (kept in sync by version.sh).
#
# Usage:
#   bash build.sh            # debug
#   bash build.sh --release  # release
set -euo pipefail

REL=""
PROFILE="debug"
for arg in "$@"; do
  case "$arg" in
    --release) REL="--release"; PROFILE="release" ;;
    *) echo "build.sh: unknown arg: $arg (usage: build.sh [--release])" >&2; exit 1 ;;
  esac
done

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$REPO_ROOT"

# ── platform / arch (drives the cli_zip name + the binary's extension) ──────
case "$(uname -s)" in
  Linux*)               PLATFORM="linux"   ;;
  Darwin*)              PLATFORM="macos"   ;;
  CYGWIN*|MINGW*|MSYS*) PLATFORM="windows" ;;
  *) echo "build.sh: unsupported OS: $(uname -s)" >&2; exit 1 ;;
esac
case "$(uname -m)" in
  x86_64|amd64)  ARCH="x86_64"  ;;
  arm64|aarch64) ARCH="aarch64" ;;
  *) echo "build.sh: unsupported arch: $(uname -m)" >&2; exit 1 ;;
esac
if [ "$PLATFORM" = "windows" ]; then EXE=".exe"; else EXE=""; fi

# ── plugin coords (single source of truth: objectiveai.json) ───────────────
OWNER="$(sed -n -E 's/.*"owner"[[:space:]]*:[[:space:]]*"([^"]+)".*/\1/p' objectiveai.json | head -1)"
NAME="$(sed -n -E 's/.*"name"[[:space:]]*:[[:space:]]*"([^"]+)".*/\1/p' objectiveai.json | head -1)"
VERSION="$(sed -n -E 's/.*"version"[[:space:]]*:[[:space:]]*"([^"]+)".*/\1/p' objectiveai.json | head -1)"
[ -n "$OWNER" ] && [ -n "$NAME" ] && [ -n "$VERSION" ] \
  || { echo "build.sh: could not read owner/name/version from objectiveai.json" >&2; exit 1; }

# ── build (same as the releaser: cargo build [--release]) ──────────────────
echo "==> build.sh ($PROFILE): cargo build${REL:+ $REL}"
cargo build $REL
CLI_BIN="$REPO_ROOT/target/$PROFILE/$NAME$EXE"
[ -f "$CLI_BIN" ] || { echo "build.sh: built binary missing at $CLI_BIN" >&2; exit 1; }

# ── stage into the plugin tree under .objectiveai ──────────────────────────
PLUGIN_DIR="$REPO_ROOT/.objectiveai/bin/plugins/$OWNER/$NAME/$VERSION"
CLI_DIR="$PLUGIN_DIR/cli"
CLI_ZIP="$CLI_DIR/$NAME-$PLATFORM-$ARCH.zip"

mkdir -p "$PLUGIN_DIR"
# Manifest sits at the head, above cli/.
cp "$REPO_ROOT/objectiveai.json" "$PLUGIN_DIR/objectiveai.json"

# cli_zip = the bare binary, flat at the zip root (matches the release asset).
rm -rf "$CLI_DIR"; mkdir -p "$CLI_DIR"
case "$PLATFORM" in
  windows)
    powershell.exe -NoProfile -Command \
      "Compress-Archive -Path '$(cygpath -w "$CLI_BIN")' -DestinationPath '$(cygpath -w "$CLI_ZIP")' -Force"
    ;;
  *)
    zip -j "$CLI_ZIP" "$CLI_BIN"
    ;;
esac

echo "==> done ($PROFILE) -> $PLUGIN_DIR"
echo "      objectiveai.json"
echo "      cli/$(basename "$CLI_ZIP")"
