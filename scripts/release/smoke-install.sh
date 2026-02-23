#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
INSTALLER="${ROOT_DIR}/install.sh"
VERSION="v$(node -e 'const fs=require("fs");const p=process.argv[1]; \
  const v=JSON.parse(fs.readFileSync(p,"utf8")).version;process.stdout.write(v);' \
  "${ROOT_DIR}/package.json" 2>/dev/null || true)"

if [[ -z "${VERSION}" || "${VERSION}" == "v" ]]; then
  VERSION="v0.1.0"
fi

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

if [[ ! -x "${binary_path}" ]]; then
  (cd "${ROOT_DIR}" && cargo build --release)
fi

tmp="$(mktemp -d)"
server_pid=""

cleanup() {
  if [[ -n "${server_pid}" ]]; then
    kill "${server_pid}" >/dev/null 2>&1 || true
    wait "${server_pid}" 2>/dev/null || true
  fi
  rm -rf "${tmp}"
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
mkdir -p "${install_dir}"

KNOTS_GITHUB_REPO="local/knots" \
KNOTS_INSTALL_DIR="${install_dir}" \
KNOTS_RELEASE_API_BASE="http://127.0.0.1:${port}/repos" \
KNOTS_RELEASE_DOWNLOAD_BASE="http://127.0.0.1:${port}" \
"${INSTALLER}"

KNOTS_GITHUB_REPO="local/knots" \
KNOTS_INSTALL_DIR="${install_dir}" \
KNOTS_RELEASE_API_BASE="http://127.0.0.1:${port}/repos" \
KNOTS_RELEASE_DOWNLOAD_BASE="http://127.0.0.1:${port}" \
KNOTS_VERSION="${VERSION}" \
"${INSTALLER}"

if [[ ! -x "${install_dir}/knots" ]]; then
  echo "error: knots binary was not installed" >&2
  exit 1
fi

if [[ ! -x "${install_dir}/knots.previous" ]]; then
  echo "error: knots.previous was not retained after pinned reinstall" >&2
  exit 1
fi

echo "Installer smoke test passed for ${VERSION} (${target_suffix})"
