#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
MAX_AGE_HOURS="${1:-${ARTIFACT_MAX_AGE_HOURS:-24}}"
TEMP_ROOT="${TMPDIR:-/tmp}"

case "$MAX_AGE_HOURS" in
  ''|*[!0-9]*)
    echo "MAX_AGE_HOURS must be a positive integer" >&2
    exit 2
    ;;
esac

if [ "$MAX_AGE_HOURS" -eq 0 ]; then
  echo "MAX_AGE_HOURS must be greater than zero" >&2
  exit 2
fi

if ! command -v python3 >/dev/null 2>&1; then
  echo "python3 is required to compute the artifact cutoff time" >&2
  exit 2
fi

STAMP="$(mktemp "${TEMP_ROOT}/knots-artifact-cutoff.XXXXXX")"
trap 'rm -f "$STAMP"' EXIT

python3 - "$STAMP" "$MAX_AGE_HOURS" <<'PY'
import os
import sys
import time

stamp = sys.argv[1]
hours = int(sys.argv[2])
cutoff = time.time() - hours * 3600
os.utime(stamp, (cutoff, cutoff))
PY

removed=0

# A tree is stale when neither it nor anything inside it is newer than the
# cutoff stamp. Both artifact classes share this test so there is exactly one
# age mechanism in the reaper.
is_stale() {
  local tree="$1"

  if [[ "$tree" -nt "$STAMP" ]]; then
    return 1
  fi

  if find "$tree" -type f -newer "$STAMP" -print -quit | grep -q .; then
    return 1
  fi

  return 0
}

reap_tree() {
  local tree="$1"
  local label="$2"

  if ! is_stale "$tree"; then
    return 0
  fi

  echo "Reaping stale artifact tree: ${label}"
  rm -rf "$tree"
  removed=1
}

# Build artifacts: the immediate children of the repository target directory.
if [ -d "$ROOT/target" ]; then
  while IFS= read -r -d '' artifact; do
    reap_tree "$artifact" "${artifact#"$ROOT"/}"
  done < <(find "$ROOT/target" -mindepth 1 -maxdepth 1 -type d -print0)
fi

# Leaked test workspaces: `knots-*` directories directly under the temp root.
# Restricted to trees this user owns, because a shared temp root holding another
# user's `knots-*` tree would fail removal and abort the script under `set -e`.
# The cutoff stamp is a file, so `-type d` already excludes it and concurrent
# reaper runs cannot remove each other's stamps.
if [ -d "$TEMP_ROOT" ]; then
  while IFS= read -r -d '' workspace; do
    reap_tree "$workspace" "$workspace"
  done < <(find "$TEMP_ROOT" -mindepth 1 -maxdepth 1 -type d -name 'knots-*' \
    -uid "$(id -u)" -print0)
fi

if [ "$removed" -eq 0 ]; then
  echo "No stale build artifacts older than ${MAX_AGE_HOURS}h."
fi
