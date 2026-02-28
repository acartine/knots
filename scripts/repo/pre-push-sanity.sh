#!/usr/bin/env bash
set -euo pipefail

repo_root="$(git rev-parse --show-toplevel 2>/dev/null || true)"
if [[ -z "${repo_root}" ]]; then
  echo "error: pre-push sanity must run inside a git repository" >&2
  exit 1
fi

cd "${repo_root}"

# Unset git env vars that leak into hooks so tests creating
# temporary git repos are not confused by the parent context.
while IFS= read -r var; do
  unset "$var" 2>/dev/null || true
done < <(env | grep '^GIT_' | cut -d= -f1)

echo "Running make sanity before push..."
make sanity
