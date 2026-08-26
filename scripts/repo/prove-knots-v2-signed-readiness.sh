#!/usr/bin/env bash
set -euo pipefail

repo="${1:-acartine/knots}"
git fetch origin main
head_sha="$(git rev-parse HEAD)"
main_sha="$(git rev-parse origin/main)"
[[ "${head_sha}" == "${main_sha}" ]] || {
  echo "error: signed readiness must run on exact origin/main" >&2
  exit 1
}
cargo build --release --locked

workdir="$(mktemp -d "${TMPDIR:-/tmp}/knots-v2-signed-canary.XXXXXX")"
event_id="github-live-canary-$(date -u +%Y%m%d%H%M%S)"
event_path="events/2026/08/26/${event_id}.json"
payload="${workdir}/event.json"
jq -n --arg event_id "${event_id}" \
  '{event_id:$event_id,occurred_at:"2026-08-26T06:00:00Z",
    knot_id:"knots-canary",type:"knot.created",data:{title:$event_id}}' >"${payload}"

git -C "${workdir}" init
git -C "${workdir}" config user.name "Knots Signed Canary"
git -C "${workdir}" config user.email "knots-signed-canary@users.noreply.github.com"
mkdir -p "${workdir}/.knots/v2/inbox/$(dirname "${event_path}")"
cp "${payload}" "${workdir}/.knots/v2/inbox/${event_path}"
git -C "${workdir}" add .
git -C "${workdir}" commit -m "signed proposal canary"
proposal_oid="$(git -C "${workdir}" rev-parse HEAD)"
envelope="${workdir}/signed-submission.json"
target/release/knots compact --create-github-proposal-canary --json \
  --repository-id "${repo}" --proposal-oid "${proposal_oid}" \
  --event-id "${event_id}" --event-path "${event_path}" \
  --event-payload "${payload}" >"${envelope}"
proposal_ref="$(jq -er .proposal_ref "${envelope}")"
inbox_ref="$(jq -er .target_ref "${envelope}")"
writer_id="$(jq -er .bundle.writer_id "${envelope}")"
registry_ref="refs/heads/knots-v2-writers/${writer_id}"

git -C "${workdir}" push "https://github.com/${repo}.git" \
  "${proposal_oid}:${proposal_ref}"
encoded="$(base64 <"${envelope}" | tr -d '\n')"
gh workflow run knots-v2-integrate.yml --repo "${repo}" --ref main \
  -f "proposal_ref=${proposal_ref}" -f "proposal_oid=${proposal_oid}" \
  -f "signed_submission_base64=${encoded}"
run_id=""
for _ in {1..30}; do
  run_id="$(gh run list --repo "${repo}" --workflow knots-v2-integrate.yml \
    --event workflow_dispatch --limit 30 --json databaseId,displayTitle \
    --jq ".[] | select(.displayTitle == \"Knots v2 integrate ${proposal_oid}\") | .databaseId" \
    | head -n 1)"
  [[ -n "${run_id}" ]] && break
  sleep 2
done
[[ -n "${run_id}" ]] || { echo "error: signed canary run not found" >&2; exit 1; }
gh run watch "${run_id}" --repo "${repo}" --exit-status
run_head="$(gh run view "${run_id}" --repo "${repo}" --json headSha --jq .headSha)"
[[ "${run_head}" == "${main_sha}" ]] || {
  echo "error: signed canary executed a different main SHA" >&2
  exit 1
}

remote="https://github.com/${repo}.git"
inbox_oid="$(git ls-remote "${remote}" "${inbox_ref}" | awk '{print $1}')"
registry_oid="$(git ls-remote "${remote}" "${registry_ref}" | awk '{print $1}')"
[[ "${inbox_oid}" == "${proposal_oid}" ]] || exit 1
[[ "${registry_oid}" =~ ^[0-9a-f]{40}$ ]] || exit 1
git -C "${workdir}" fetch --no-tags "${remote}" "${registry_ref}"
registry="$(git -C "${workdir}" show "${registry_oid}:.knots/v2/writer.json")"
jq -e --arg writer_id "${writer_id}" --arg inbox_oid "${proposal_oid}" \
  '.writer_id == $writer_id and .inbox_oid == $inbox_oid and .sequence == 1' \
  <<<"${registry}" >/dev/null

workflow_sha="$(shasum -a 256 .github/workflows/knots-v2-integrate.yml | awk '{print $1}')"
evidence="${workdir}/signed-readiness.json"
jq -n --arg main_sha "${main_sha}" --arg workflow_sha256 "${workflow_sha}" \
  --arg run_id "${run_id}" --arg proposal_oid "${proposal_oid}" \
  --arg registry_oid "${registry_oid}" \
  '{main_sha:$main_sha,workflow_sha256:$workflow_sha256,trusted_main:true,
    actions_run_id:$run_id,proposal_oid:$proposal_oid,registry_oid:$registry_oid,
    signed_writer_verified:true,authority_constructed:true}' >"${evidence}"
echo "${evidence}"
