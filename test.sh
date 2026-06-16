#!/usr/bin/env bash
#
# test.sh — full integration-test cycle for mundus-animarum.
#
#   1. Fetch the objectiveai host (cli/api/db) for this platform into
#      .objectiveai/bin — pinned to the objectiveai-sdk version in Cargo.toml,
#      skipped when .objectiveai/bin/version.txt is already current.
#   2. Build mundus-animarum (debug) and install it as a plugin under
#      .objectiveai/bin/plugins — always, the way `plugins install` does
#      (replace cli/, copy the binary, copy the manifest).
#   3. Stop any running api/db and wipe the state dir for a clean slate.
#   4. Ensure cargo-nextest is in ./bin, run the tests, stop api/db again.
#   5. Exit with nextest's status.
#
# Extra args are forwarded to nextest (e.g. `bash test.sh harness_smoke`).
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"
export OBJECTIVEAI_DIR="$SCRIPT_DIR/.objectiveai"
BIN_DIR="$OBJECTIVEAI_DIR/bin"

# ── platform → asset suffix ───────────────────────────────────────────────
case "$(uname -s)" in
  Linux)                plat_os="linux" ;;
  Darwin)               plat_os="macos" ;;
  MINGW*|MSYS*|CYGWIN*) plat_os="windows" ;;
  *) echo "test.sh: unsupported OS '$(uname -s)'" >&2; exit 1 ;;
esac
case "$(uname -m)" in
  x86_64|amd64)  plat_arch="x86_64" ;;
  arm64|aarch64) plat_arch="aarch64" ;;
  *) echo "test.sh: unsupported arch '$(uname -m)'" >&2; exit 1 ;;
esac
ext=""; [ "$plat_os" = "windows" ] && ext=".exe"
platarch="${plat_os}-${plat_arch}"

OAI_BIN="$BIN_DIR/objectiveai${ext}"

# Stop any running api + db servers, in parallel, best-effort.
kill_servers() {
  [ -x "$OAI_BIN" ] || return 0
  "$OAI_BIN" api kill --global >/dev/null 2>&1 &
  local api_pid=$!
  "$OAI_BIN" db kill --global >/dev/null 2>&1 &
  local db_pid=$!
  wait "$api_pid" 2>/dev/null || true
  wait "$db_pid" 2>/dev/null || true
}

# ── 1. objectiveai host (pinned to the objectiveai-sdk dep version) ────────
OAI_VER="$(sed -n -E 's/^objectiveai-sdk = \{ version = "([^"]+)".*/\1/p' Cargo.toml | head -1)"
[ -n "$OAI_VER" ] || { echo "test.sh: could not read objectiveai-sdk version from Cargo.toml" >&2; exit 1; }
VERSION_FILE="$BIN_DIR/version.txt"
if [ -f "$VERSION_FILE" ] && [ "$(cat "$VERSION_FILE")" = "$OAI_VER" ]; then
  echo "objectiveai v$OAI_VER already present in $BIN_DIR"
else
  echo "objectiveai: downloading v$OAI_VER ($platarch)"
  mkdir -p "$BIN_DIR"
  base_url="https://github.com/ObjectiveAI/objectiveai/releases/download/v$OAI_VER"
  for entry in \
    "objectiveai|objectiveai-${platarch}${ext}" \
    "objectiveai-api|objectiveai-${platarch}-api${ext}" \
    "objectiveai-db|objectiveai-${platarch}-db${ext}"; do
    dest="${entry%%|*}"; asset="${entry#*|}"
    out="$BIN_DIR/${dest}${ext}"
    echo "  $asset"
    curl -fSL --retry 3 -o "$out" "$base_url/$asset" \
      || { echo "test.sh: failed to download $base_url/$asset" >&2; exit 1; }
    [ "$plat_os" = "windows" ] || chmod +x "$out"
  done
  echo "$OAI_VER" > "$VERSION_FILE"
fi

# ── 2. build + install mundus-animarum (debug, unconditional) ─────────────
OWNER="$(sed -n -E 's/.*"owner"[[:space:]]*:[[:space:]]*"([^"]+)".*/\1/p' objectiveai.json | head -1)"
NAME="$(sed -n -E 's/.*"name"[[:space:]]*:[[:space:]]*"([^"]+)".*/\1/p' objectiveai.json | head -1)"
PVER="$(sed -n -E 's/.*"version"[[:space:]]*:[[:space:]]*"([^"]+)".*/\1/p' objectiveai.json | head -1)"
[ -n "$OWNER" ] && [ -n "$NAME" ] && [ -n "$PVER" ] \
  || { echo "test.sh: could not read owner/name/version from objectiveai.json" >&2; exit 1; }

echo "build: mundus-animarum (debug)"
cargo build
CLI_BIN="$SCRIPT_DIR/target/debug/mundus-animarum${ext}"
[ -f "$CLI_BIN" ] || { echo "test.sh: built binary missing at $CLI_BIN" >&2; exit 1; }

PLUGIN_DIR="$BIN_DIR/plugins/$OWNER/$NAME/$PVER"
echo "install: $PLUGIN_DIR"
rm -rf "$PLUGIN_DIR/cli"
mkdir -p "$PLUGIN_DIR/cli"
cp "$CLI_BIN" "$PLUGIN_DIR/cli/mundus-animarum${ext}"
cp "$SCRIPT_DIR/objectiveai.json" "$PLUGIN_DIR/objectiveai.json"

# ── 3. clean slate: stop api/db, wipe state ───────────────────────────────
echo "cleanup: stopping api/db"
kill_servers
echo "cleanup: wiping state"
rm -rf "$OBJECTIVEAI_DIR/state"

# ── 4. nextest ────────────────────────────────────────────────────────────
NEXTEST="$SCRIPT_DIR/bin/cargo-nextest${ext}"
if [ ! -x "$NEXTEST" ]; then
  echo "nextest: installing into ./bin"
  cargo install cargo-nextest --locked --root "$SCRIPT_DIR"
fi
echo "nextest: running"
rc=0
"$NEXTEST" nextest run "$@" || rc=$?

# ── 5. final cleanup + exit ───────────────────────────────────────────────
echo "cleanup: stopping api/db"
kill_servers
exit "$rc"
