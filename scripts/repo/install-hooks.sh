#!/usr/bin/env bash
set -euo pipefail

repo_root="$(git rev-parse --show-toplevel 2>/dev/null || true)"
if [[ -z "${repo_root}" ]]; then
  echo "error: hook installer must run inside a git repository" >&2
  exit 1
fi

hooks_dir="${repo_root}/.git/hooks"
managed_hook="${hooks_dir}/pre-push"
local_hook="${hooks_dir}/pre-push.local"
marker="knots-managed-pre-push-hook"

mkdir -p "${hooks_dir}"

if [[ -f "${managed_hook}" ]] && ! grep -q "${marker}" "${managed_hook}"; then
  if [[ -f "${local_hook}" ]]; then
    backup="${hooks_dir}/pre-push.backup.$(date +%s)"
    mv "${managed_hook}" "${backup}"
    echo "Moved existing pre-push hook to ${backup}"
  else
    mv "${managed_hook}" "${local_hook}"
    chmod +x "${local_hook}" || true
    echo "Moved existing pre-push hook to ${local_hook}"
  fi
fi

cat > "${managed_hook}" <<EOF
#!/usr/bin/env bash
set -euo pipefail
# ${marker}
repo_root="${repo_root}"
local_hook="\${repo_root}/.git/hooks/pre-push.local"

if [[ -x "\${local_hook}" ]]; then
  "\${local_hook}" "\$@"
fi

"\${repo_root}/scripts/repo/pre-push-sanity.sh" "\$@"
EOF

chmod +x "${managed_hook}"
echo "Installed managed pre-push hook at ${managed_hook}"
