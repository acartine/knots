#!/usr/bin/env bash
set -euo pipefail

CHANNEL_ROOT="${KNOTS_CHANNEL_ROOT:-${HOME}/.local/bin/acartine_knots}"
ACTIVE_LINK="${KNOTS_ACTIVE_LINK:-${HOME}/.local/bin/kno}"
LEGACY_LINK="${KNOTS_LEGACY_LINK:-${HOME}/.local/bin/knots}"

usage() {
  cat <<'USAGE'
Select active kno binary by symlink.

Usage:
  channel-use.sh release
  channel-use.sh local
  channel-use.sh show

Default paths:
  release binary: ~/.local/bin/acartine_knots/release/kno
  local binary:   ~/.local/bin/acartine_knots/local/kno
  active link:    ~/.local/bin/kno
  compat link:    ~/.local/bin/knots

Optional env vars:
  KNOTS_CHANNEL_ROOT  Override channel root directory.
  KNOTS_ACTIVE_LINK   Override active kno link path.
  KNOTS_LEGACY_LINK   Override compatibility knots link path.
USAGE
}

resolve_target() {
  case "$1" in
    release|local)
      local preferred="${CHANNEL_ROOT}/$1/kno"
      if [[ -x "${preferred}" ]]; then
        printf '%s' "${preferred}"
      else
        printf '%s/%s/knots' "${CHANNEL_ROOT}" "$1"
      fi
      ;;
    *)
      return 1
      ;;
  esac
}

show_active() {
  if [[ ! -e "${ACTIVE_LINK}" && ! -e "${LEGACY_LINK}" ]]; then
    echo "No active kno/knots links found"
    return 0
  fi
  show_link "kno" "${ACTIVE_LINK}"
  show_link "knots" "${LEGACY_LINK}"
}

show_link() {
  local label="$1"
  local path="$2"
  if [[ ! -e "${path}" ]]; then
    return
  fi
  local resolved="<not-a-symlink>"
  if [[ -L "${path}" ]]; then
    resolved="$(readlink "${path}")"
  fi
  echo "Active ${label} link: ${path}"
  echo "Resolved target: ${resolved}"
  if [[ -x "${path}" ]]; then
    "${path}" --version
  fi
}

channel="${1:-}"
if [[ -z "${channel}" || "${channel}" == "--help" || "${channel}" == "-h" ]]; then
  usage
  exit 0
fi

if [[ "${channel}" == "show" ]]; then
  show_active
  exit 0
fi

target="$(resolve_target "${channel}")" || {
  echo "error: unsupported channel '${channel}' (use release|local|show)" >&2
  usage
  exit 1
}

if [[ ! -x "${target}" ]]; then
  echo "error: channel binary not found at ${target}" >&2
  echo "hint: run scripts/release/channel-install.sh ${channel}" >&2
  exit 1
fi

mkdir -p "$(dirname "${ACTIVE_LINK}")" "$(dirname "${LEGACY_LINK}")"
ln -sfn "${target}" "${ACTIVE_LINK}"
ln -sfn "${target}" "${LEGACY_LINK}"

echo "Active kno -> ${target}"
echo "Compatibility knots -> ${target}"
"${ACTIVE_LINK}" --version
