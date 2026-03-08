#!/bin/sh
set -eu

DEFAULT_REPO="acartine/knots"
REPO="${KNOTS_GITHUB_REPO:-${DEFAULT_REPO}}"
INSTALL_DIR="${KNOTS_INSTALL_DIR:-${HOME}/.local/bin}"
DOWNLOAD_BASE="${KNOTS_RELEASE_DOWNLOAD_BASE:-https://github.com}"
REQUESTED_VERSION="${KNOTS_VERSION:-}"

usage() {
  cat <<'USAGE'
kno installer

Environment variables:
  KNOTS_GITHUB_REPO         owner/repo source (default: acartine/knots)
  KNOTS_VERSION             release tag (example: v0.1.0). default: latest
  KNOTS_INSTALL_DIR         target dir for kno/knots binaries (default: ~/.local/bin)
  KNOTS_RELEASE_DOWNLOAD_BASE  override download base for release assets
USAGE
}

require_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "error: required command '$1' not found" >&2
    exit 1
  fi
}

sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | awk '{print $1}'
  else
    echo "error: no SHA256 tool found (need sha256sum or shasum)" >&2
    exit 1
  fi
}

detect_target() {
  os="$(uname -s | tr '[:upper:]' '[:lower:]')"
  arch="$(uname -m | tr '[:upper:]' '[:lower:]')"

  case "${os}/${arch}" in
    darwin/arm64|darwin/aarch64)
      TARGET_SUFFIX="darwin-arm64"
      ;;
    linux/x86_64|linux/amd64)
      TARGET_SUFFIX="linux-x86_64"
      ;;
    linux/aarch64|linux/arm64)
      TARGET_SUFFIX="linux-aarch64"
      ;;
    *)
      echo "error: unsupported platform '${os}/${arch}'" >&2
      exit 1
      ;;
  esac
}

resolve_version() {
  if [ -n "${REQUESTED_VERSION}" ]; then
    RESOLVED_TAG="${REQUESTED_VERSION}"
  else
    # Resolve latest tag via the redirect on /releases/latest (no API quota).
    latest_url="${DOWNLOAD_BASE%/}/${REPO}/releases/latest"
    redirect="$(curl -fsSI "${latest_url}" | \
      tr -d '\r' | awk 'tolower($1)=="location:" {print $2}' | head -n 1)"
    RESOLVED_TAG="${redirect##*/}"

    if [ -z "${RESOLVED_TAG}" ]; then
      echo "error: failed to resolve latest release tag from ${latest_url}" >&2
      exit 1
    fi
  fi

  case "${RESOLVED_TAG}" in
    v*) ;;
    *)  RESOLVED_TAG="v${RESOLVED_TAG}" ;;
  esac
}

download_release_assets() {
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
  expected="$(awk -v name="${ASSET_FILE}" '$2==name {print $1}' \
    "${TMP_DIR}/${CHECKSUMS_FILE}")"

  if [ -z "${expected}" ]; then
    echo "error: checksum entry for ${ASSET_FILE} was not found" >&2
    exit 1
  fi

  actual="$(sha256_of "${TMP_DIR}/${ASSET_FILE}")"
  if [ "${actual}" != "${expected}" ]; then
    echo "error: checksum verification failed for ${ASSET_FILE}" >&2
    exit 1
  fi
}

install_binary() {
  mkdir -p "${INSTALL_DIR}"
  tar -xzf "${TMP_DIR}/${ASSET_FILE}" -C "${TMP_DIR}"

  extracted="${TMP_DIR}/knots"
  if [ ! -f "${extracted}" ]; then
    echo "error: expected 'knots' binary in ${ASSET_FILE}" >&2
    exit 1
  fi

  legacy_destination="${INSTALL_DIR}/knots"
  preferred_destination="${INSTALL_DIR}/kno"
  staging="${legacy_destination}.new"

  if [ -f "${legacy_destination}" ]; then
    cp "${legacy_destination}" "${INSTALL_DIR}/kno.previous"
    cp "${legacy_destination}" "${INSTALL_DIR}/knots.previous"
  fi

  install -m 0755 "${extracted}" "${staging}"
  mv "${staging}" "${legacy_destination}"
  ln -sfn "knots" "${preferred_destination}"
}

print_result() {
  ver="$("${INSTALL_DIR}/kno" --version)"
  comp_out="$("${INSTALL_DIR}/kno" completions --install 2>/dev/null || true)"
  comp_path="${comp_out#completions installed to }"

  printf "%13s  %s\n" "kno" "${INSTALL_DIR}/kno"
  printf "%13s  %s\n" "compat" "${INSTALL_DIR}/knots"
  printf "%13s  %s\n" "version" "${ver}"
  if [ -n "${comp_path}" ] && [ "${comp_path}" != "${comp_out}" ]; then
    printf "%13s  %s\n" "completions" "${comp_path}"
  fi
}

case "${1:-}" in
  --help|-h) usage; exit 0 ;;
esac

require_cmd curl
require_cmd tar
detect_target
resolve_version

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "${TMP_DIR}"' EXIT

download_release_assets
verify_checksum
install_binary
print_result
