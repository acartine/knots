#!/usr/bin/env bash
set -euo pipefail

max_lines=499
violations=0

while IFS= read -r f; do
  lines=$(wc -l < "$f")
  if [[ "${lines}" -gt "${max_lines}" ]]; then
    echo "error: ${f} is ${lines} lines (max ${max_lines})"
    violations=$((violations + 1))
  fi
done < <(find src tests -name '*.rs' 2>/dev/null | sort)

if [[ "${violations}" -gt 0 ]]; then
  echo "${violations} file-size violation(s) found."
  exit 1
fi

echo "All Rust files are within the ${max_lines}-line limit."
