#!/usr/bin/env bash
#
# Bump the mundus-animarum version everywhere it appears, in sync.
#
#   ./version.sh <new-version>     e.g.  ./version.sh 0.2.0
#
# Updates the five places the version lives:
#   - Cargo.toml          [package] version
#   - Cargo.lock          the mundus-animarum package entry
#   - src/mcp/mod.rs      the MCP server's initialize-response version
#   - objectiveai.json    the plugin manifest version
#   - tests/common/mod.rs the test harness's installed-plugin coordinate
#                         (the version dir test.sh installs the plugin under)
#
# Pure sed, no compile. Does NOT commit — committing the bump is what triggers
# the release workflow (which fires on Cargo.toml changes to main). Requires
# GNU sed (git-bash on Windows, or Linux).
set -euo pipefail

new="${1:-}"
if [[ -z "$new" ]]; then
  echo "usage: $0 <new-version>" >&2
  exit 1
fi
if [[ ! "$new" =~ ^[0-9]+\.[0-9]+\.[0-9]+([-.+][0-9A-Za-z.-]+)?$ ]]; then
  echo "error: '$new' is not a valid version (expected X.Y.Z)" >&2
  exit 1
fi

cd "$(dirname "$0")"

# Cargo.toml — the [package] version is the first `version = "..."` line.
sed -i -E '0,/^version = "[^"]*"/ s//version = "'"$new"'"/' Cargo.toml

# Cargo.lock — the `version` line directly after the package's name line.
sed -i -E '/^name = "mundus-animarum"$/{n;s/^version = "[^"]*"/version = "'"$new"'"/}' Cargo.lock

# MCP server version reported in the initialize response (a string literal).
sed -i -E 's/version: "[^"]*"\.into\(\)/version: "'"$new"'".into()/' src/mcp/mod.rs

# Plugin manifest.
sed -i -E 's/"version": "[^"]*"/"version": "'"$new"'"/' objectiveai.json

# Test harness — the `VERSION` const is the installed-plugin coordinate the
# integration tests request (escape `&` in the `&str` replacement so sed
# doesn't treat it as the whole-match reference).
sed -i -E 's/const VERSION: &str = "[^"]*"/const VERSION: \&str = "'"$new"'"/' tests/common/mod.rs

echo "Bumped to $new in: Cargo.toml, Cargo.lock, src/mcp/mod.rs, objectiveai.json, tests/common/mod.rs"
