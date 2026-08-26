#!/usr/bin/env bash
set -euo pipefail

repo="${1:-acartine/knots}"
canary_id="canary-$(date -u +%Y%m%d%H%M%S)"
workdir="$(mktemp -d "${TMPDIR:-/tmp}/knots-v2-canary.XXXXXX")"
evidence="${workdir}/evidence.json"
remote="https://github.com/${repo}.git"
ordinary_token="$(gh auth token)"
ordinary_auth="$(printf 'x-access-token:%s' "${ordinary_token}" | base64 | tr -d '\n')"

ordinary_git() {
  git -c "http.extraheader=AUTHORIZATION: basic ${ordinary_auth}" -C "${workdir}" "$@"
}

node scripts/repo/knots-v2-rulesets.mjs apply canary

git -C "${workdir}" init
git -C "${workdir}" config user.name "Knots Policy Canary"
git -C "${workdir}" config user.email "knots-policy-canary@users.noreply.github.com"
git -C "${workdir}" commit --allow-empty -m "canary base"
base="$(git -C "${workdir}" rev-parse HEAD)"
git -C "${workdir}" commit --allow-empty -m "canary fast-forward"
head="$(git -C "${workdir}" rev-parse HEAD)"

protected_ref="refs/heads/knots-v2-canary/protected/ordinary-${canary_id}"
if ordinary_git push "${remote}" "${head}:${protected_ref}"; then
  echo "error: ordinary credential bypassed protected canary ruleset" >&2
  exit 1
fi

inbox_ref="refs/heads/knots-v2-canary/inbox/${canary_id}"
if ordinary_git push "${remote}" "${base}:${inbox_ref}"; then
  echo "error: ordinary credential created a mediated inbox ref" >&2
  exit 1
fi

gh workflow run knots-v2-policy-canary.yml --repo "${repo}" --ref main -f "canary_id=${canary_id}"
run_id=""
for _ in {1..20}; do
  run_id="$(gh run list --repo "${repo}" --workflow knots-v2-policy-canary.yml \
    --event workflow_dispatch --limit 20 --json databaseId,displayTitle \
    --jq ".[] | select(.displayTitle == \"Knots v2 policy canary ${canary_id}\") | .databaseId" \
    | head -n 1)"
  [[ -n "${run_id}" ]] && break
  sleep 2
done
[[ -n "${run_id}" ]] || { echo "error: canary Actions run not found" >&2; exit 1; }
gh run watch "${run_id}" --repo "${repo}" --exit-status

protected_action_ref="refs/heads/knots-v2-canary/protected/${canary_id}"
protected_action_oid="$(git ls-remote "${remote}" "${protected_action_ref}" | awk '{print $1}')"
inbox_action_oid="$(git ls-remote "${remote}" "${inbox_ref}" | awk '{print $1}')"
[[ "${protected_action_oid}" =~ ^[0-9a-f]{40}$ ]] || exit 1
[[ "${inbox_action_oid}" =~ ^[0-9a-f]{40}$ ]] || exit 1
git -C "${workdir}" fetch --no-tags "${remote}" "${inbox_ref}"
parent_count="$(git -C "${workdir}" rev-list --parents -n 1 "${inbox_action_oid}" | wc -w)"
if [[ "${parent_count// /}" != "2" ]]; then
  echo "error: Actions inbox did not fast-forward" >&2
  exit 1
fi

ordinary_ff="$(
  GIT_AUTHOR_NAME='Knots Ordinary Canary' \
  GIT_AUTHOR_EMAIL='knots-policy-canary@users.noreply.github.com' \
  GIT_COMMITTER_NAME='Knots Ordinary Canary' \
  GIT_COMMITTER_EMAIL='knots-policy-canary@users.noreply.github.com' \
  git -C "${workdir}" commit-tree "${inbox_action_oid}^{tree}" \
    -p "${inbox_action_oid}" -m 'ordinary fast-forward probe'
)"
if ordinary_git push "${remote}" "${ordinary_ff}:${inbox_ref}"; then
  echo "error: inbox accepted ordinary fast-forward update" >&2
  exit 1
fi
if ordinary_git push "${remote}" "${base}:${inbox_ref}"; then
  echo "error: inbox accepted non-fast-forward update" >&2
  exit 1
fi
if ordinary_git push "${remote}" ":${inbox_ref}"; then
  echo "error: inbox accepted deletion" >&2
  exit 1
fi

jq -n --arg canary_id "${canary_id}" --arg run_id "${run_id}" \
  --arg protected_oid "${protected_action_oid}" --arg inbox_oid "${inbox_action_oid}" \
  '{canary_id:$canary_id,actions_run_id:$run_id,
    actions_protected_oid:$protected_oid,actions_inbox_oid:$inbox_oid,
    ordinary_protected_rejected:true,ordinary_inbox_rejected:true,
    ordinary_inbox_fast_forward_rejected:true,
    actions_inbox_creation_succeeded:true,actions_inbox_fast_forward_succeeded:true,
    inbox_non_fast_forward_rejected:true,
    inbox_deletion_rejected:true,actions_protected_succeeded:true}' >"${evidence}"
echo "${evidence}"
