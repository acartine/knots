#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
INSTALLER="${ROOT_DIR}/install.sh"
KEEP_TMP="${KNOTS_SMOKE_KEEP_TMP:-0}"
INSTALL_DIR_OVERRIDE="${KNOTS_SMOKE_INSTALL_DIR:-}"

if ! command -v python3 >/dev/null 2>&1; then
  echo "error: python3 is required for smoke installer test" >&2
  exit 1
fi

case "$(uname -s | tr '[:upper:]' '[:lower:]')/$(uname -m | tr '[:upper:]' '[:lower:]')" in
  darwin/arm64|darwin/aarch64)
    target_suffix="darwin-arm64"
    binary_path="${ROOT_DIR}/target/release/knots"
    ;;
  linux/x86_64|linux/amd64)
    target_suffix="linux-x86_64"
    binary_path="${ROOT_DIR}/target/release/knots"
    ;;
  *)
    echo "error: smoke installer script supports only darwin arm64 and linux x86_64" >&2
    exit 1
    ;;
esac

if command -v sha256sum >/dev/null 2>&1; then
  SHA_CMD=(sha256sum)
elif command -v shasum >/dev/null 2>&1; then
  SHA_CMD=(shasum -a 256)
else
  echo "error: no SHA256 tool found (need sha256sum or shasum)" >&2
  exit 1
fi

(cd "${ROOT_DIR}" && cargo build --release)

built_version="$("${binary_path}" --version | awk '{print $2}')"
if [[ -z "${built_version}" ]]; then
  echo "error: failed to read version from ${binary_path}" >&2
  exit 1
fi
VERSION="v${built_version#v}"
built_sha="$(${SHA_CMD[@]} "${binary_path}" | awk '{print $1}')"

tmp="$(mktemp -d)"
server_pid=""

cleanup() {
  if [[ -n "${server_pid}" ]]; then
    kill "${server_pid}" >/dev/null 2>&1 || true
    wait "${server_pid}" 2>/dev/null || true
  fi
  if [[ "${KEEP_TMP}" == "1" ]]; then
    echo "Retained smoke test artifacts at ${tmp}"
  else
    rm -rf "${tmp}"
  fi
}

trap cleanup EXIT

mkdir -p "${tmp}/local/knots/releases/download/${VERSION}"
mkdir -p "${tmp}/repos/local/knots/releases"

asset="knots-${VERSION}-${target_suffix}.tar.gz"
checksum_file="knots-${VERSION}-checksums.txt"

cp "${binary_path}" "${tmp}/knots"
(
  cd "${tmp}"
  tar -czf "${asset}" knots
  mv "${asset}" "${tmp}/local/knots/releases/download/${VERSION}/${asset}"
)

(
  cd "${tmp}/local/knots/releases/download/${VERSION}"
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "${asset}" > "${checksum_file}"
  else
    shasum -a 256 "${asset}" > "${checksum_file}"
  fi
)

cat > "${tmp}/repos/local/knots/releases/latest" <<JSON
{
  "tag_name": "${VERSION}"
}
JSON

port=18765
python3 -m http.server "${port}" --directory "${tmp}" >/dev/null 2>&1 &
server_pid=$!
sleep 1

install_dir="${tmp}/install"
if [[ -n "${INSTALL_DIR_OVERRIDE}" ]]; then
  install_dir="${INSTALL_DIR_OVERRIDE}"
fi
mkdir -p "${install_dir}"

KNOTS_GITHUB_REPO="local/knots" \
KNOTS_INSTALL_DIR="${install_dir}" \
KNOTS_RELEASE_API_BASE="http://127.0.0.1:${port}/repos" \
KNOTS_RELEASE_DOWNLOAD_BASE="http://127.0.0.1:${port}" \
"${INSTALLER}" </dev/null

KNOTS_GITHUB_REPO="local/knots" \
KNOTS_INSTALL_DIR="${install_dir}" \
KNOTS_RELEASE_API_BASE="http://127.0.0.1:${port}/repos" \
KNOTS_RELEASE_DOWNLOAD_BASE="http://127.0.0.1:${port}" \
KNOTS_VERSION="${VERSION}" \
"${INSTALLER}" </dev/null

if [[ ! -x "${install_dir}/knots" ]]; then
  echo "error: knots compatibility binary was not installed" >&2
  exit 1
fi

if [[ ! -x "${install_dir}/kno" ]]; then
  echo "error: kno alias was not installed" >&2
  exit 1
fi

if [[ ! -x "${install_dir}/kno.previous" ]]; then
  echo "error: kno.previous was not retained after pinned reinstall" >&2
  exit 1
fi

installed_version="$("${install_dir}/kno" --version | awk '{print $2}')"
if [[ "${installed_version}" != "${built_version}" ]]; then
  echo "error: installed version ${installed_version} != built version ${built_version}" >&2
  exit 1
fi

installed_sha="$(${SHA_CMD[@]} "${install_dir}/kno" | awk '{print $1}')"
if [[ "${installed_sha}" != "${built_sha}" ]]; then
  echo "error: installed binary hash does not match locally built binary" >&2
  exit 1
fi

echo "Installer smoke test passed for ${VERSION} (${target_suffix})"
echo "Installed binary matches local release build at ${install_dir}/kno"
