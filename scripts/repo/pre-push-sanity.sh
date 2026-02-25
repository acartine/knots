#!/usr/bin/env bash
set -euo pipefail

repo_root="$(git rev-parse --show-toplevel 2>/dev/null || true)"
if [[ -z "${repo_root}" ]]; then
  echo "error: pre-push sanity must run inside a git repository" >&2
  exit 1
fi

cd "${repo_root}"
echo "Running make sanity before push..."
make sanity
