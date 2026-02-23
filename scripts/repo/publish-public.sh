#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage: scripts/repo/publish-public.sh [--dry-run] [owner/repo]

Environment variables:
  KNOTS_GITHUB_REPO       Optional fallback for owner/repo.
  KNOTS_REPO_DESCRIPTION  Optional GitHub repo description.
  KNOTS_REPO_HOMEPAGE     Optional homepage URL.
  KNOTS_REPO_TOPICS       Optional comma-separated topics.
USAGE
}

run() {
  if [[ "${DRY_RUN}" == "1" ]]; then
    printf '[dry-run] %q' "$1"
    shift
    for arg in "$@"; do
      printf ' %q' "$arg"
    done
    printf '\n'
  else
    "$@"
  fi
}

require_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "error: missing required command: $1" >&2
    exit 1
  fi
}

ensure_clean_git_state() {
  if ! git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
    echo "error: run this script from inside a git repository" >&2
    exit 1
  fi

  if [[ -n "$(git status --porcelain)" ]]; then
    echo "error: repository has uncommitted changes" >&2
    exit 1
  fi

  local branch
  branch="$(git branch --show-current)"
  if [[ "${branch}" != "main" ]]; then
    echo "error: expected current branch 'main', found '${branch}'" >&2
    exit 1
  fi
}

set_repo_metadata() {
  local repo="$1"
  local description="${KNOTS_REPO_DESCRIPTION:-}"
  local homepage="${KNOTS_REPO_HOMEPAGE:-}"
  local topics="${KNOTS_REPO_TOPICS:-}"

  if [[ -n "${description}" ]]; then
    run gh repo edit "${repo}" --description "${description}"
  fi

  if [[ -n "${homepage}" ]]; then
    run gh repo edit "${repo}" --homepage "${homepage}"
  fi

  if [[ -n "${topics}" ]]; then
    IFS=',' read -ra topic_list <<<"${topics}"
    for topic in "${topic_list[@]}"; do
      topic="$(echo "${topic}" | xargs)"
      if [[ -n "${topic}" ]]; then
        run gh repo edit "${repo}" --add-topic "${topic}"
      fi
    done
  fi
}

print_security_checklist() {
  cat <<'CHECKLIST'

Post-publish security checklist (manual):
1. Open repository Settings -> Security & analysis.
2. Enable Private vulnerability reporting.
3. Confirm SECURITY.md is visible at repository root.
4. Configure branch protection and required workflow checks.
CHECKLIST
}

DRY_RUN=0
TARGET_REPO=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --dry-run)
      DRY_RUN=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      if [[ -n "${TARGET_REPO}" ]]; then
        echo "error: only one owner/repo argument is allowed" >&2
        exit 1
      fi
      TARGET_REPO="$1"
      shift
      ;;
  esac
done

require_cmd git
require_cmd gh
ensure_clean_git_state

if [[ -z "${TARGET_REPO}" ]]; then
  TARGET_REPO="${KNOTS_GITHUB_REPO:-}"
fi

if [[ -z "${TARGET_REPO}" ]]; then
  user_login="$(gh api user --jq .login)"
  repo_name="$(basename "$(pwd)")"
  TARGET_REPO="${user_login}/${repo_name}"
fi

if ! gh auth status >/dev/null 2>&1; then
  echo "error: gh is not authenticated. run: gh auth login" >&2
  exit 1
fi

if git remote get-url origin >/dev/null 2>&1; then
  run gh repo create "${TARGET_REPO}" --public
  run git remote set-url origin "https://github.com/${TARGET_REPO}.git"
  run git push -u origin main
else
  run gh repo create "${TARGET_REPO}" --public --source . --remote origin --push
fi

set_repo_metadata "${TARGET_REPO}"

echo "Repository published: https://github.com/${TARGET_REPO}"
print_security_checklist
