#!/usr/bin/env bash
set -euo pipefail

control_ref="${1:?control ref is required}"
repo="${2:-acartine/knots}"
policy="config/github-knots-v2-trusted-signers.json"
evidence="$(scripts/repo/inspect-knots-v2-control-epoch.sh "${control_ref}" "${repo}")"
signer_sha="$(jq -er .manifest.source_sha <<<"${evidence}")"
source_ref="$(jq -er .manifest.source_ref <<<"${evidence}")"
subject_path="$(jq -er .subject_path <<<"${evidence}")"
workflow="$(jq -er .manifest.workflow <<<"${evidence}")"
run_id="$(jq -er .manifest.run_id <<<"${evidence}")"
jq -e --arg repo "${repo}" --arg sha "${signer_sha}" \
  --arg ref "${source_ref}" --arg workflow "${workflow}" \
  '.repository == $repo and (.trusted_signers | any(
    .sha == $sha and .ref == $ref and .workflow == $workflow))' \
  "${policy}" >/dev/null || {
  echo "error: signer identity is not in the reviewed provider allowlist" >&2
  exit 1
}

verified=false
for _ in {1..10}; do
  if gh attestation verify "${subject_path}" --repo "${repo}" \
    --signer-workflow "${repo}/${workflow}" --source-ref "${source_ref}" \
    --source-digest "${signer_sha}" --signer-digest "${signer_sha}" \
    --deny-self-hosted-runners --format json >/dev/null 2>&1
  then
    verified=true
    break
  fi
  sleep 2
done
[[ "${verified}" == true ]] || { echo "error: attestation verification failed" >&2; exit 1; }
run="$(gh run view "${run_id}" --repo "${repo}" \
  --json status,conclusion,headSha,headBranch,databaseId)"
source_branch="${source_ref#refs/heads/}"
jq -e --arg sha "${signer_sha}" --arg ref "${source_branch}" --arg run_id "${run_id}" \
  '.status == "completed" and .conclusion == "success" and .headSha == $sha
    and .headBranch == $ref and (.databaseId | tostring) == $run_id' \
  <<<"${run}" >/dev/null || {
  echo "error: signer Actions run did not complete successfully at the reviewed SHA" >&2
  exit 1
}
jq 'del(.subject_path)' <<<"${evidence}"
