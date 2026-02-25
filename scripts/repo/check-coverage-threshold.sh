#!/usr/bin/env bash
set -euo pipefail

base_ref="${1:-origin/main}"
threshold_file=".ci/coverage-threshold.txt"

if [[ ! -f "${threshold_file}" ]]; then
  echo "error: ${threshold_file} is missing" >&2
  exit 1
fi

current_value="$(tr -d '[:space:]' < "${threshold_file}")"
if [[ ! "${current_value}" =~ ^[0-9]+$ ]]; then
  echo "error: ${threshold_file} must contain an integer percentage" >&2
  exit 1
fi

if ! git cat-file -e "${base_ref}^{commit}" 2>/dev/null; then
  echo "Base ref '${base_ref}' not available; skipping threshold regression check."
  exit 0
fi

if ! git show "${base_ref}:${threshold_file}" >/dev/null 2>&1; then
  echo "No ${threshold_file} on ${base_ref}; skipping threshold regression check."
  exit 0
fi

base_value="$(git show "${base_ref}:${threshold_file}" | tr -d '[:space:]')"
if [[ ! "${base_value}" =~ ^[0-9]+$ ]]; then
  echo "error: ${base_ref}:${threshold_file} does not contain an integer percentage" >&2
  exit 1
fi

if (( current_value < base_value )); then
  echo "error: coverage threshold regression: ${current_value} < ${base_value}" >&2
  exit 1
fi

echo "Coverage threshold check passed: current=${current_value}, base=${base_value}"
