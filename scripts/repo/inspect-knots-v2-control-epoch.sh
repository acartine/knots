#!/usr/bin/env bash
set -euo pipefail

control_ref="${1:?control ref is required}"
repo="${2:-acartine/knots}"
remote="https://github.com/${repo}.git"
workdir="$(mktemp -d "${TMPDIR:-/tmp}/knots-v2-control-inspect.XXXXXX")"
git -C "${workdir}" init --bare >/dev/null
git -C "${workdir}" fetch --quiet --no-tags "${remote}" "${control_ref}"
control_oid="$(git -C "${workdir}" rev-parse FETCH_HEAD)"
manifest_path="${workdir}/control-epoch.json"
git -C "${workdir}" show \
  "${control_oid}:.knots/v2/control-epoch.json" >"${manifest_path}"

manifest="$(CONTROL_REF="${control_ref}" \
  node scripts/repo/knots-v2-control-epoch.mjs verify-file "${manifest_path}")"
generation_ref="$(jq -er .generation.ref <<<"${manifest}")"
generation_oid="$(jq -er .generation.oid <<<"${manifest}")"
observed="$(git ls-remote "${remote}" "${generation_ref}" | awk '{print $1}')"
[[ "${observed}" == "${generation_oid}" ]] || {
  echo "error: generation ref does not match the control manifest" >&2
  exit 1
}
while IFS=$'\t' read -r archive_ref archive_oid; do
  [[ -n "${archive_ref}" ]] || continue
  observed="$(git ls-remote "${remote}" "${archive_ref}" | awk '{print $1}')"
  [[ "${observed}" == "${archive_oid}" ]] || {
    echo "error: archive ref does not match the control manifest" >&2
    exit 1
  }
done < <(jq -r '.archives[] | [.ref,.oid] | @tsv' <<<"${manifest}")

manifest_sha256="$(shasum -a 256 "${manifest_path}" | awk '{print $1}')"
jq -n --arg control_ref "${control_ref}" --arg control_oid "${control_oid}" \
  --arg manifest_sha256 "${manifest_sha256}" --arg subject_path "${manifest_path}" \
  --argjson manifest "${manifest}" \
  '{control_ref:$control_ref,control_oid:$control_oid,subject_path:$subject_path,
    manifest_sha256:$manifest_sha256,manifest:$manifest}'
