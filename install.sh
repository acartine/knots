#!/usr/bin/env bash
set -euo pipefail

DEFAULT_REPO="acartine/knots"
REPO="${KNOTS_GITHUB_REPO:-${DEFAULT_REPO}}"
INSTALL_DIR="${KNOTS_INSTALL_DIR:-${HOME}/.local/bin}"
API_BASE="${KNOTS_RELEASE_API_BASE:-https://api.github.com/repos}"
DOWNLOAD_BASE="${KNOTS_RELEASE_DOWNLOAD_BASE:-https://github.com}"
REQUESTED_VERSION="${KNOTS_VERSION:-}"

usage() {
  cat <<'USAGE'
kno installer

Environment variables:
  KNOTS_GITHUB_REPO         owner/repo source (default: acartine/knots)
  KNOTS_VERSION             release tag (example: v0.1.0). default: latest
  KNOTS_INSTALL_DIR         target dir for kno/knots binaries (default: ~/.local/bin)
  KNOTS_RELEASE_API_BASE    override API base for latest-release lookup
  KNOTS_RELEASE_DOWNLOAD_BASE  override download base for release assets
USAGE
}

require_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "error: required command '$1' not found" >&2
    exit 1
  fi
}

detect_sha_tool() {
  if command -v sha256sum >/dev/null 2>&1; then
    SHA_CMD=(sha256sum)
  elif command -v shasum >/dev/null 2>&1; then
    SHA_CMD=(shasum -a 256)
  else
    echo "error: no SHA256 tool found (need sha256sum or shasum)" >&2
    exit 1
  fi
}

detect_target() {
  local os arch
  os="$(uname -s | tr '[:upper:]' '[:lower:]')"
  arch="$(uname -m | tr '[:upper:]' '[:lower:]')"

  case "${os}/${arch}" in
    darwin/arm64|darwin/aarch64)
      TARGET_SUFFIX="darwin-arm64"
      ;;
    linux/x86_64|linux/amd64)
      TARGET_SUFFIX="linux-x86_64"
      ;;
    *)
      echo "error: unsupported platform '${os}/${arch}'" >&2
      exit 1
      ;;
  esac
}

resolve_version() {
  if [[ -n "${REQUESTED_VERSION}" ]]; then
    RESOLVED_TAG="${REQUESTED_VERSION}"
  else
    local latest_url
    latest_url="${API_BASE%/}/${REPO}/releases/latest"
    local latest_json
    latest_json="$(curl -fsSL "${latest_url}")"
    RESOLVED_TAG="$(printf '%s' "${latest_json}" | \
      sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' | head -n 1)"

    if [[ -z "${RESOLVED_TAG}" ]]; then
      echo "error: failed to resolve latest release tag from ${latest_url}" >&2
      exit 1
    fi
  fi

  if [[ "${RESOLVED_TAG}" != v* ]]; then
    RESOLVED_TAG="v${RESOLVED_TAG}"
  fi
}

download_release_assets() {
  local asset_file checksums_file asset_url checksums_url
  asset_file="knots-${RESOLVED_TAG}-${TARGET_SUFFIX}.tar.gz"
  checksums_file="knots-${RESOLVED_TAG}-checksums.txt"

  asset_url="${DOWNLOAD_BASE%/}/${REPO}/releases/download/${RESOLVED_TAG}/${asset_file}"
  checksums_url="${DOWNLOAD_BASE%/}/${REPO}/releases/download/${RESOLVED_TAG}/${checksums_file}"

  curl -fsSL "${asset_url}" -o "${TMP_DIR}/${asset_file}"
  curl -fsSL "${checksums_url}" -o "${TMP_DIR}/${checksums_file}"

  ASSET_FILE="${asset_file}"
  CHECKSUMS_FILE="${checksums_file}"
}

verify_checksum() {
  local expected actual
  expected="$(awk -v name="${ASSET_FILE}" '$2==name {print $1}' "${TMP_DIR}/${CHECKSUMS_FILE}")"

  if [[ -z "${expected}" ]]; then
    echo "error: checksum entry for ${ASSET_FILE} was not found" >&2
    exit 1
  fi

  actual="$(${SHA_CMD[@]} "${TMP_DIR}/${ASSET_FILE}" | awk '{print $1}')"
  if [[ "${actual}" != "${expected}" ]]; then
    echo "error: checksum verification failed for ${ASSET_FILE}" >&2
    exit 1
  fi
}

install_binary() {
  mkdir -p "${INSTALL_DIR}"
  tar -xzf "${TMP_DIR}/${ASSET_FILE}" -C "${TMP_DIR}"

  local extracted="${TMP_DIR}/knots"
  if [[ ! -f "${extracted}" ]]; then
    echo "error: expected 'knots' binary in ${ASSET_FILE}" >&2
    exit 1
  fi

  local legacy_destination="${INSTALL_DIR}/knots"
  local preferred_destination="${INSTALL_DIR}/kno"
  local staging="${legacy_destination}.new"

  if [[ -f "${legacy_destination}" ]]; then
    cp "${legacy_destination}" "${INSTALL_DIR}/kno.previous"
    cp "${legacy_destination}" "${INSTALL_DIR}/knots.previous"
  fi

  install -m 0755 "${extracted}" "${staging}"
  mv "${staging}" "${legacy_destination}"
  ln -sfn "knots" "${preferred_destination}"
}

print_result() {
  echo "Installed kno to ${INSTALL_DIR}/kno"
  echo "Compatibility binary at ${INSTALL_DIR}/knots"
  "${INSTALL_DIR}/kno" --version
}

install_completions() {
  "${INSTALL_DIR}/kno" completions --install || true
}

if [[ "${1:-}" == "--help" || "${1:-}" == "-h" ]]; then
  usage
  exit 0
fi

require_cmd curl
require_cmd tar
detect_sha_tool
detect_target
resolve_version

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "${TMP_DIR}"' EXIT

download_release_assets
verify_checksum
install_binary
print_result
install_completions
