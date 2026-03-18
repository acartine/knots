#!/usr/bin/env bash
set -euo pipefail

repo_root="$(git rev-parse --show-toplevel 2>/dev/null || true)"
if [[ -z "${repo_root}" ]]; then
  echo "error: changeset check must run inside a git repository" >&2
  exit 1
fi

cd "${repo_root}"

resolve_base_ref() {
  local remote_head
  remote_head="$(git symbolic-ref refs/remotes/origin/HEAD --short 2>/dev/null || true)"
  if [[ -n "${remote_head}" ]]; then
    echo "${remote_head}"
    return 0
  fi

  if git rev-parse --verify origin/main >/dev/null 2>&1; then
    echo "origin/main"
    return 0
  fi

  echo "error: could not resolve origin default branch for changeset check" >&2
  echo "hint: fetch origin or create refs/remotes/origin/HEAD" >&2
  exit 1
}

if [[ "$#" -eq 2 ]]; then
  diff_range="$1...$2"
elif [[ "$#" -eq 0 ]]; then
  diff_range="$(resolve_base_ref)...HEAD"
else
  echo "usage: $0 [<base-sha> <head-sha>]" >&2
  exit 1
fi

changed_files=()
while IFS= read -r file; do
  changed_files+=("${file}")
done < <(git diff --name-only "${diff_range}")

if [[ "${#changed_files[@]}" -eq 0 ]]; then
  echo "No changed files found; skipping changeset requirement."
  exit 0
fi

has_non_doc_change=0
has_changeset_file=0

is_doc_or_chore() {
  local file="$1"
  case "${file}" in
    *.md|docs/*|.github/*|scripts/repo/*|LICENSE|SECURITY.md|AGENTS.md|CLAUDE.md)
      return 0
      ;;
    *)
      return 1
      ;;
  esac
}

for file in "${changed_files[@]}"; do
  if [[ "${file}" == .changeset/*.md && "${file}" != ".changeset/README.md" ]]; then
    has_changeset_file=1
    continue
  fi

  if is_doc_or_chore "${file}"; then
    continue
  fi

  has_non_doc_change=1
done

if [[ "${has_non_doc_change}" -eq 0 ]]; then
  echo "Only docs/chore files changed; changeset not required."
  exit 0
fi

if [[ "${has_changeset_file}" -eq 0 ]]; then
  echo "A changeset is required for non-doc changes." >&2
  echo "Run: npm run changeset" >&2
  exit 1
fi

echo "Changeset check passed."
