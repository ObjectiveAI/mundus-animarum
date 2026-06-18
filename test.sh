#!/usr/bin/env bash
# test.sh — reset the local .objectiveai integration sandbox (keeping the
# expensive-to-rebuild bits), reinstall the host + the already-built plugin,
# then run the test suite.
#
# Assumes build.sh has ALREADY staged the plugin (cli_zip + manifest) into the
# tree — test.sh does NOT build. It DOES install the plugin (via install.sh, in
# reuse mode), so you can re-test without rebuilding.
#
#   1. If the host binary is present, `kill-all` to stop any servers it left
#      running (they'd otherwise hold files open).
#   2. Wipe .objectiveai/bin/ down to the keepers — the `plugins` (the built
#      plugin) and `pg-bin` dirs, plus any .zip sitting DIRECTLY in bin/.
#   3. Delete the state folder (.objectiveai/state) entirely.
#   4. (Re)install the objectiveai host via the upstream curl installer,
#      pointed at our .objectiveai (--objectiveai-dir), no PATH changes.
#   5. Install the built plugin via install.sh (the zip is already present, so
#      it just cleans + unpacks in place — no download).
#   6. Apply the global API config the run needs (mcp timeout, backoff).
#   7. Run the suite (cargo nextest), then `kill-all`, exit on the nextest rc.
#
# Requires `cargo-nextest` on PATH. Extra args forward to nextest
# (e.g. `bash test.sh mcp_notifications_basic`).
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$REPO_ROOT"
OAI_DIR="$REPO_ROOT/.objectiveai"
BIN_DIR="$OAI_DIR/bin"

case "$(uname -s)" in
  Linux*)               PLATFORM="linux"   ;;
  Darwin*)              PLATFORM="macos"   ;;
  CYGWIN*|MINGW*|MSYS*) PLATFORM="windows" ;;
  *) echo "unsupported OS: $(uname -s)" >&2; exit 1 ;;
esac
if [ "$PLATFORM" = "windows" ]; then EXE=".exe"; else EXE=""; fi

HOST="$BIN_DIR/objectiveai$EXE"

# 1. Stop any running host servers.
if [ -x "$HOST" ]; then
  echo "==> objectiveai kill-all"
  "$HOST" kill-all || true
fi

# 2. Clean bin/ down to the keepers: `plugins` (the built plugin from build.sh)
#    and `pg-bin` (the postgres binaries), plus any host zip cached directly in
#    bin/. Everything else — host binaries, other dirs — goes.
if [ -d "$BIN_DIR" ]; then
  shopt -s nullglob
  for entry in "$BIN_DIR"/*; do
    name="$(basename "$entry")"
    case "$name" in
      plugins|pg-bin) continue ;;
    esac
    # Keep a .zip sitting directly in bin/; zips nested in removed dirs go.
    if [ -f "$entry" ] && [ "$name" != "${name%.zip}" ]; then
      continue
    fi
    rm -rf "$entry"
  done
  shopt -u nullglob
fi

# 3. Delete the state folder entirely.
rm -rf "$OAI_DIR/state"

# 4. (Re)install the objectiveai host into our .objectiveai dir, no PATH change.
echo "==> installing objectiveai host into $OAI_DIR"
curl -fsSL https://raw.githubusercontent.com/ObjectiveAI/objectiveai/main/install.sh \
  | bash -s -- --no-export-path --objectiveai-dir "$OAI_DIR"

# 5. Install the already-built plugin (build.sh produced the zip → unpack only).
echo "==> installing the built plugin into $OAI_DIR"
bash "$REPO_ROOT/install.sh" --dir "$OAI_DIR"

# 6. Global API config for the run. mcp-timeout-ms keeps the MCP-heavy resume
#    tests from timing out; backoff is best-effort (older hosts lack the key).
echo "==> objectiveai api config (global)"
"$HOST" api config mcp-timeout-ms set 300000 --global
"$HOST" api config backoff-max-elapsed-time-ms set 0 --global || true

# 7. Run the suite, then stop the host's servers and exit on the nextest result.
echo "==> cargo nextest run"
rc=0
cargo nextest run "$@" || rc=$?

echo "==> objectiveai kill-all"
"$HOST" kill-all || true

exit "$rc"
