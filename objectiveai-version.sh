#!/usr/bin/env bash
#
# Set the objectiveai dependency version everywhere it appears, in sync.
#
#   ./objectiveai-version.sh <new-version>     e.g.  ./objectiveai-version.sh 2.2.4
#
# Updates every objectiveai-* dependency requirement in Cargo.toml (both the
# normal and the dev-dependency entries), then re-resolves Cargo.lock to that
# version with `cargo update` — which pins objectiveai-sdk and pulls the
# matching objectiveai-sdk-macros, refreshing their checksums.
#
# Unlike version.sh (which bumps this crate's own version with pure sed), this
# touches a REGISTRY dependency: Cargo.lock carries checksums a plain sed would
# invalidate, so it shells out to cargo. Requires network and the version must
# be published on crates.io. Does NOT commit.
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

# Every `objectiveai-<crate> = { version = "..." }` line in Cargo.toml — the
# [dependencies] and [dev-dependencies] objectiveai-sdk entries (and any future
# objectiveai-* deps). The rest of each line (features, etc.) is preserved.
sed -i -E 's/^(objectiveai-[A-Za-z0-9_-]+ = \{ version = ")[^"]*(")/\1'"$new"'\2/' Cargo.toml

# Re-resolve the lock to the new version (objectiveai-sdk pinned exactly, the
# matching objectiveai-sdk-macros pulled in, checksums refreshed).
cargo update -p objectiveai-sdk --precise "$new"

echo "Set objectiveai to $new in: Cargo.toml, Cargo.lock"
