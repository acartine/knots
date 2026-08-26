#!/usr/bin/env bash
set -euo pipefail

namespace="${1:?namespace is required}"
repo="${2:-acartine/knots}"
case "${namespace}" in
  production) prefix="refs/heads/knots-v2-control/epochs/" ;;
  canary) prefix="refs/heads/knots-v2-canary/control/epochs/" ;;
  *) echo "error: invalid namespace" >&2; exit 1 ;;
esac

workdir="$(mktemp -d "${TMPDIR:-/tmp}/knots-v2-authority.XXXXXX")"
verified="${workdir}/verified.jsonl"
: >"${verified}"
while read -r _ ref; do
  [[ -n "${ref:-}" ]] || continue
  if result="$(scripts/repo/verify-knots-v2-control-epoch.sh \
    "${ref}" "${repo}" 2>/dev/null)";
  then
    printf '%s\n' "${result}" >>"${verified}"
  fi
done < <(git ls-remote --refs "https://github.com/${repo}.git" "${prefix}*")

jq -s '.' "${verified}" >"${workdir}/chain.json"
node scripts/repo/knots-v2-control-epoch.mjs verify-chain "${workdir}/chain.json"
