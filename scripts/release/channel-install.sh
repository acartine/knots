#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
CHANNEL_ROOT="${KNOTS_CHANNEL_ROOT:-${HOME}/.local/bin/acartine_knots}"
ACTIVE_LINK="${KNOTS_ACTIVE_LINK:-${HOME}/.local/bin/kno}"
LEGACY_LINK="${KNOTS_LEGACY_LINK:-${HOME}/.local/bin/knots}"
DEFAULT_INSTALLER_URL="https://raw.githubusercontent.com/acartine/knots/main/install.sh"
INSTALLER_URL="${KNOTS_RELEASE_INSTALLER_URL:-${DEFAULT_INSTALLER_URL}}"
SMOKE_SCRIPT="${ROOT_DIR}/scripts/release/smoke-install.sh"
USE_SCRIPT="${ROOT_DIR}/scripts/release/channel-use.sh"

usage() {
  cat <<'USAGE'
Install kno into a channel path.

Usage:
  channel-install.sh release [--activate]
  channel-install.sh local [--activate]

Default channel root:
  ~/.local/bin/acartine_knots

Installed binaries:
  release -> ~/.local/bin/acartine_knots/release/kno
  local   -> ~/.local/bin/acartine_knots/local/kno

Optional env vars:
  KNOTS_CHANNEL_ROOT         Override base channel directory.
  KNOTS_ACTIVE_LINK          Override active kno link path.
  KNOTS_LEGACY_LINK          Override compatibility knots link path.
  KNOTS_RELEASE_INSTALLER_URL  Override GitHub installer URL.

Pass-through env vars:
  Release channel: KNOTS_VERSION, KNOTS_GITHUB_REPO
  Local channel:   KNOTS_SMOKE_KEEP_TMP
USAGE
}

require_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "error: required command '$1' not found" >&2
    exit 1
  fi
}

activate_channel() {
  KNOTS_CHANNEL_ROOT="${CHANNEL_ROOT}" \
  KNOTS_ACTIVE_LINK="${ACTIVE_LINK}" \
  KNOTS_LEGACY_LINK="${LEGACY_LINK}" \
    "${USE_SCRIPT}" "$1"
}

install_release() {
  require_cmd curl
  mkdir -p "${CHANNEL_ROOT}/release"
  curl -fsSL "${INSTALLER_URL}" \
    | KNOTS_INSTALL_DIR="${CHANNEL_ROOT}/release" sh
  echo "Installed release channel at ${CHANNEL_ROOT}/release/kno"
}

install_local() {
  mkdir -p "${CHANNEL_ROOT}/local"
  KNOTS_SMOKE_INSTALL_DIR="${CHANNEL_ROOT}/local" \
    "${SMOKE_SCRIPT}"
  echo "Installed local channel at ${CHANNEL_ROOT}/local/kno"
}

channel="${1:-}"
if [[ -z "${channel}" || "${channel}" == "--help" || "${channel}" == "-h" ]]; then
  usage
  exit 0
fi
shift

activate=0
while [[ $# -gt 0 ]]; do
  case "$1" in
    --activate)
      activate=1
      ;;
    *)
      echo "error: unknown option '$1'" >&2
      usage
      exit 1
      ;;
  esac
  shift
done

case "${channel}" in
  release)
    install_release
    ;;
  local)
    install_local
    ;;
  *)
    echo "error: unsupported channel '${channel}' (use release|local)" >&2
    usage
    exit 1
    ;;
esac

if [[ "${activate}" == "1" ]]; then
  activate_channel "${channel}"
fi
