#!/usr/bin/env bash
set -euo pipefail

repo="acartine/knots"
github_remote="https://github.com/acartine/knots.git"
workflow=".github/workflows/knots-v2-control-epoch.yml"
policy="config/github-knots-v2-trusted-signers.json"
evidence_path="${1:-${TMPDIR:-/tmp}/knots-v2-canary-evidence.json}"
authority_ref="$(jq -er '.trusted_signers | select(length == 1) | .[0].ref' "${policy}")"
authority_sha="$(jq -er '.trusted_signers | select(length == 1) | .[0].sha' "${policy}")"
signer_workflow="$(jq -er '.trusted_signers[0].workflow' "${policy}")"
main_sha="$(git rev-parse HEAD)"
remote_main="$(git ls-remote "${github_remote}" refs/heads/main | awk '{print $1}')"

[[ "${main_sha}" == "${remote_main}" ]] || {
  echo "error: canary tooling must run from exact GitHub main" >&2
  exit 1
}
[[ "${signer_workflow}" == "${workflow}" ]] || {
  echo "error: allowlisted signer workflow is unexpected" >&2
  exit 1
}
[[ "$(git ls-remote "${github_remote}" "${authority_ref}" | awk '{print $1}')" \
  == "${authority_sha}" ]] || {
  echo "error: immutable authority ref does not resolve to its reviewed SHA" >&2
  exit 1
}

node scripts/repo/knots-v2-rulesets.mjs apply canary
work="$(mktemp -d "${TMPDIR:-/tmp}/knots-v2-canary.XXXXXX")"

# Every invocation makes a fresh immutable ref and proves ordinary update/delete denial.
canary_id="$(openssl rand -hex 16)"
probe_ref="refs/heads/knots-v2-canary/immutable/${canary_id}"
git push --no-verify "${github_remote}" "${main_sha}:${probe_ref}"
ordinary_creation_succeeded=true
ordinary_rewrite_rejected=false
if ! git push --no-verify "${github_remote}" "+${authority_sha}:${probe_ref}"; then
  ordinary_rewrite_rejected=true
fi
ordinary_deletion_rejected=false
if ! git push --no-verify "${github_remote}" ":${probe_ref}"; then
  ordinary_deletion_rejected=true
fi
[[ "${ordinary_rewrite_rejected}" == true && "${ordinary_deletion_rejected}" == true ]]
[[ "$(git ls-remote "${github_remote}" "${probe_ref}" | awk '{print $1}')" \
  == "${main_sha}" ]]

# Reuse a completed, attested epoch after any post-publication interruption.
writer_id="$(printf 'knots-v2-policy-canary' | shasum -a 256 | awk '{print $1}')"
authority="$(scripts/repo/find-knots-v2-control-epoch.sh canary "${repo}")"
existing_control_ref="$(jq -r '.control_ref // empty' <<<"${authority}")"
if [[ -n "${existing_control_ref}" ]]; then
  printf '%s\n' "${authority}" >"${work}/previous.json"
  lookalike_epoch="$(jq -r '.manifest.epoch + 1' <<<"${authority}")"
  writer_vector="$(jq -c --arg writer "${writer_id}" \
    '.manifest.writer_vector as $vector
      | $vector + {($writer): (($vector[$writer] // 0) + 1)}' <<<"${authority}")"
  generation_ref="$(jq -er .manifest.generation.ref <<<"${authority}")"
  generation_oid="$(jq -er .manifest.generation.oid <<<"${authority}")"
else
  printf '{"control_ref":null,"control_oid":null,"manifest":null}\n' \
    >"${work}/previous.json"
  lookalike_epoch="1"
  writer_vector="$(jq -cn --arg writer "${writer_id}" '{($writer): 1}')"
  generation_ref="${probe_ref}"
  generation_oid="${main_sha}"
fi
writer_vector_base64="$(printf '%s' "${writer_vector}" | base64 | tr -d '\n')"
archives_base64="$(printf '[]' | base64 | tr -d '\n')"

# This manifest is structurally valid and chain-dominating, but has no OIDC attestation.
export GITHUB_REPOSITORY="${repo}"
export GITHUB_REF="${authority_ref}"
export GITHUB_SHA="${authority_sha}"
GITHUB_RUN_ID="$(date +%s)"
export GITHUB_RUN_ID
export NAMESPACE="canary"
export EPOCH="${lookalike_epoch}"
export GENERATION_REF="${generation_ref}"
export GENERATION_OID="${generation_oid}"
export WRITER_VECTOR_JSON="${writer_vector}"
export ARCHIVES_JSON="[]"
CONTROL_NONCE="$(openssl rand -hex 16)"
export CONTROL_NONCE
export PREVIOUS_STATE_PATH="${work}/previous.json"
export MANIFEST_PATH="${work}/lookalike.json"
export GITHUB_OUTPUT="${work}/lookalike-output"
node scripts/repo/knots-v2-control-epoch.mjs prepare
lookalike_ref="$(sed -n 's/^control_ref=//p' "${GITHUB_OUTPUT}")"
lookalike_blob="$(git hash-object -w "${MANIFEST_PATH}")"
lookalike_index="${work}/lookalike.index"
GIT_INDEX_FILE="${lookalike_index}" git read-tree --empty
GIT_INDEX_FILE="${lookalike_index}" git update-index \
  --add --cacheinfo "100644,${lookalike_blob},.knots/v2/control-epoch.json"
lookalike_tree="$(GIT_INDEX_FILE="${lookalike_index}" git write-tree)"
git config user.name "Knots Policy Canary"
git config user.email "knots-canary@users.noreply.github.com"
lookalike_oid="$(printf '%s\n' 'Publish unattested Knots v2 lookalike' | \
  git commit-tree "${lookalike_tree}")"
git push --no-verify "${github_remote}" "${lookalike_oid}:${lookalike_ref}"
unattested_lookalike_rejected=false
if ! scripts/repo/verify-knots-v2-control-epoch.sh "${lookalike_ref}" "${repo}"; then
  unattested_lookalike_rejected=true
fi
[[ "${unattested_lookalike_rejected}" == true ]]
[[ "$(git ls-remote "${github_remote}" "${lookalike_ref}" | awk '{print $1}')" \
  == "${lookalike_oid}" ]]

# Phase A can emit the first canary epoch once. Later invocations recover that verified epoch.
authority_branch="${authority_ref#refs/heads/}"
if [[ -z "${existing_control_ref}" ]]; then
  gh workflow run knots-v2-control-epoch-canary.yml --repo "${repo}" \
    --ref "${authority_branch}" \
    -f "canary_id=${canary_id}" \
    -f epoch=1 \
    -f "generation_ref=${generation_ref}" \
    -f "generation_oid=${generation_oid}" \
    -f "writer_vector_base64=${writer_vector_base64}" \
    -f "archives_base64=${archives_base64}"

  actions_run_id=""
  for _ in {1..30}; do
    actions_run_id="$(gh run list --repo "${repo}" \
      --workflow knots-v2-control-epoch-canary.yml --branch "${authority_branch}" \
      --event workflow_dispatch --limit 20 \
      --json databaseId,displayTitle,headSha \
      --jq ".[] | select(.displayTitle == \"Knots v2 control canary ${canary_id}\"
        and .headSha == \"${authority_sha}\") | .databaseId" | head -n 1)"
    [[ -n "${actions_run_id}" ]] && break
    sleep 2
  done
  [[ -n "${actions_run_id}" ]] || {
    echo "error: dispatched Actions run not found" >&2
    exit 1
  }
  gh run watch "${actions_run_id}" --repo "${repo}" --exit-status
  authority="$(scripts/repo/find-knots-v2-control-epoch.sh canary "${repo}")"
fi

control_ref="$(jq -er .control_ref <<<"${authority}")"
control_oid="$(jq -er .control_oid <<<"${authority}")"
verified="$(scripts/repo/verify-knots-v2-control-epoch.sh "${control_ref}" "${repo}")"
actions_run_id="$(jq -er .manifest.run_id <<<"${verified}")"
generation_ref="$(jq -er .manifest.generation.ref <<<"${verified}")"
generation_oid="$(jq -er .manifest.generation.oid <<<"${verified}")"
[[ "$(jq -r .manifest.source_ref <<<"${verified}")" == "${authority_ref}" ]]
[[ "$(jq -r .manifest.source_sha <<<"${verified}")" == "${authority_sha}" ]]
[[ "$(jq -r .manifest.workflow <<<"${verified}")" == "${workflow}" ]]
[[ "$(git ls-remote "${github_remote}" "${generation_ref}" | awk '{print $1}')" \
  == "${generation_oid}" ]]

git fetch --quiet --no-tags "${github_remote}" "${control_ref}"
[[ "$(git rev-parse FETCH_HEAD)" == "${control_oid}" ]]
control_rewrite="$(printf '%s\n' 'Forbidden ordinary control rewrite' | \
  git commit-tree "${control_oid}^{tree}" -p "${control_oid}")"
if git push --no-verify "${github_remote}" "${control_rewrite}:${control_ref}"; then
  echo "error: ordinary credential rewrote the attested control epoch" >&2
  exit 1
fi
if git push --no-verify "${github_remote}" ":${control_ref}"; then
  echo "error: ordinary credential deleted the attested control epoch" >&2
  exit 1
fi
[[ "$(git ls-remote "${github_remote}" "${control_ref}" | awk '{print $1}')" \
  == "${control_oid}" ]]

manifest_sha256="${control_ref##*-}"
jq -n \
  --arg main_sha "${main_sha}" \
  --arg authority_sha "${authority_sha}" \
  --arg authority_ref "${authority_ref}" \
  --arg signer_workflow "${signer_workflow}" \
  --arg actions_run_id "${actions_run_id}" \
  --arg control_ref "${control_ref}" \
  --arg control_oid "${control_oid}" \
  --arg generation_ref "${generation_ref}" \
  --arg generation_oid "${generation_oid}" \
  --arg probe_ref "${probe_ref}" \
  --arg probe_oid "${main_sha}" \
  --arg lookalike_ref "${lookalike_ref}" \
  --arg lookalike_oid "${lookalike_oid}" \
  --arg manifest_sha256 "${manifest_sha256}" \
  --argjson ordinary_creation_succeeded "${ordinary_creation_succeeded}" \
  --argjson ordinary_rewrite_rejected "${ordinary_rewrite_rejected}" \
  --argjson ordinary_deletion_rejected "${ordinary_deletion_rejected}" \
  --argjson unattested_lookalike_rejected "${unattested_lookalike_rejected}" \
  '{main_sha: $main_sha, authority_sha: $authority_sha, authority_ref: $authority_ref,
    signer_workflow: $signer_workflow, actions_run_id: $actions_run_id,
    control_ref: $control_ref, control_oid: $control_oid,
    generation_ref: $generation_ref, generation_oid: $generation_oid,
    probe_ref: $probe_ref, probe_oid: $probe_oid,
    lookalike_ref: $lookalike_ref, lookalike_oid: $lookalike_oid,
    manifest_sha256: $manifest_sha256,
    ordinary_creation_succeeded: $ordinary_creation_succeeded,
    ordinary_rewrite_rejected: $ordinary_rewrite_rejected,
    ordinary_deletion_rejected: $ordinary_deletion_rejected,
    actions_attestation_verified: true, actions_control_rewrite_rejected: true,
    actions_control_deletion_rejected: true,
    unattested_lookalike_rejected: $unattested_lookalike_rejected,
    exact_workflow_head_verified: true}' >"${evidence_path}"

jq . "${evidence_path}"
