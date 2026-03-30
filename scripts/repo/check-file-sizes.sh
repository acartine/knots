#!/usr/bin/env bash
set -euo pipefail

max_lines=499
allowlist_file=".ci/oversized-files.txt"
violations=0
warnings=0

is_allowed() {
  if [[ -f "${allowlist_file}" ]]; then
    grep -qxF "$1" "${allowlist_file}"
  else
    return 1
  fi
}

while IFS= read -r f; do
  lines=$(wc -l < "$f")
  if [[ "${lines}" -gt "${max_lines}" ]]; then
    if is_allowed "${f}"; then
      echo "warning: ${f} is ${lines} lines (max ${max_lines}) [grandfathered]"
      warnings=$((warnings + 1))
    else
      echo "error: ${f} is ${lines} lines (max ${max_lines})"
      violations=$((violations + 1))
    fi
  fi
done < <(find src tests -name '*.rs' 2>/dev/null | sort)

if [[ "${warnings}" -gt 0 ]]; then
  echo "${warnings} grandfathered file(s) still exceed the limit."
fi

if [[ "${violations}" -gt 0 ]]; then
  echo "${violations} file-size violation(s) found."
  exit 1
fi

echo "All new Rust files are within the ${max_lines}-line limit."
