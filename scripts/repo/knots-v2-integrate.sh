#!/usr/bin/env bash
set -euo pipefail

required=(
  GITHUB_REPOSITORY GITHUB_TOKEN EXPECTED_REPOSITORY PROPOSAL_REF PROPOSAL_OID
  SIGNED_SUBMISSION_BASE64
)
for name in "${required[@]}"; do
  [[ -n "${!name:-}" ]] || { echo "error: ${name} is required" >&2; exit 1; }
done
[[ "${GITHUB_REPOSITORY}" == "${EXPECTED_REPOSITORY}" ]] || {
  echo "error: repository identity mismatch" >&2
  exit 1
}

oid_pattern='^[0-9a-f]{40}([0-9a-f]{24})?$'
proposal_pattern='^refs/heads/knots-v2-proposals/[0-9a-f]{64}/[1-9][0-9]*$'
[[ "${PROPOSAL_OID}" =~ ${oid_pattern} ]] || { echo "error: invalid proposal OID" >&2; exit 1; }
if [[ ! "${PROPOSAL_REF}" =~ ${proposal_pattern} ]]; then
  echo "error: invalid proposal ref" >&2
  exit 1
fi

workdir="$(mktemp -d "${RUNNER_TEMP:-/tmp}/knots-v2-integrate.XXXXXX")"
submission="${workdir}/signed-submission.json"
plan="${workdir}/promotion-plan.json"
printf '%s' "${SIGNED_SUBMISSION_BASE64}" | base64 --decode >"${submission}"

git -C "${workdir}" init --bare >/dev/null
origin_url="https://x-access-token:${GITHUB_TOKEN}@github.com/${GITHUB_REPOSITORY}.git"
git -C "${workdir}" remote add origin "${origin_url}"
git -C "${workdir}" fetch --no-tags origin "${PROPOSAL_REF}"
observed="$(git -C "${workdir}" rev-parse FETCH_HEAD)"
[[ "${observed}" == "${PROPOSAL_OID}" ]] || { echo "error: proposal lease changed" >&2; exit 1; }

args=(
  compact --verify-github-proposal --json
  --git-dir "${workdir}"
  --repository-id "${GITHUB_REPOSITORY}"
  --proposal-ref "${PROPOSAL_REF}"
  --proposal-oid "${PROPOSAL_OID}"
  --signed-submission "${submission}"
)
target/release/knots "${args[@]}" >"${plan}"

inbox_ref="$(jq -er .inbox_ref "${plan}")"
expected_old="$(jq -er '.expected_old_oid // ""' "${plan}")"
registry_ref="$(jq -er .writer_registry_ref "${plan}")"
expected_registry="$(jq -er '.expected_registry_oid // ""' "${plan}")"
verified_oid="$(jq -er .proposal_oid "${plan}")"
writer_id="$(jq -er .writer_id "${plan}")"
public_key="$(jq -er .public_key "${plan}")"
sequence="$(jq -er .sequence "${plan}")"
parent_writer_id="$(jq -er '.parent_writer_id // ""' "${plan}")"
purpose="$(jq -er .purpose "${plan}")"
[[ "${verified_oid}" == "${PROPOSAL_OID}" ]] || exit 1
jq -e '.signed_writer_verified == true and .authority_constructed == true' "${plan}" >/dev/null

registry_json="${workdir}/writer.json"
jq -n \
  --arg writer_id "${writer_id}" \
  --arg public_key "${public_key}" \
  --arg inbox_oid "${verified_oid}" \
  --arg parent_writer_id "${parent_writer_id}" \
  --arg purpose "${purpose}" \
  --argjson sequence "${sequence}" \
  '{schema_version: 1, writer_id: $writer_id, public_key: $public_key,
    inbox_oid: $inbox_oid, sequence: $sequence,
    parent_writer_id: (if $parent_writer_id == "" then null else $parent_writer_id end),
    purpose: $purpose}' \
  >"${registry_json}"

registry_blob="$(git -C "${workdir}" hash-object -w "${registry_json}")"
registry_index="${workdir}/registry.index"
GIT_INDEX_FILE="${registry_index}" git -C "${workdir}" read-tree --empty
GIT_INDEX_FILE="${registry_index}" git -C "${workdir}" update-index \
  --add --cacheinfo "100644,${registry_blob},.knots/v2/writer.json"
registry_tree="$(GIT_INDEX_FILE="${registry_index}" git -C "${workdir}" write-tree)"
parent_args=()
if [[ -n "${expected_registry}" ]]; then
  parent_args=(-p "${expected_registry}")
fi
registry_commit="$(
  GIT_AUTHOR_NAME='Knots GitHub Integrator' \
  GIT_AUTHOR_EMAIL='actions@users.noreply.github.com' \
  GIT_COMMITTER_NAME='Knots GitHub Integrator' \
  GIT_COMMITTER_EMAIL='actions@users.noreply.github.com' \
  git -C "${workdir}" commit-tree "${registry_tree}" "${parent_args[@]}" \
    -m "Advance protected writer registry"
)"

git -C "${workdir}" push --atomic origin \
  --force-with-lease="${inbox_ref}:${expected_old}" \
  --force-with-lease="${registry_ref}:${expected_registry}" \
  "${verified_oid}:${inbox_ref}" \
  "${registry_commit}:${registry_ref}"
