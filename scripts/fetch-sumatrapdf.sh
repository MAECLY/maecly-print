#!/usr/bin/env bash
#
# Download the SumatraPDF binary MaeclyPrint bundles on Windows, with its
# licence.
#
# This is the POSIX twin of scripts/fetch-sumatrapdf.ps1: a Linux CI runner
# cross-compiling a Windows bundle, or a maintainer on macOS who wants to
# verify the pins without a Windows box. The fetched executable is a Windows
# PE that does nothing on this host.
#
# THE BINARY IS DELIBERATELY NOT COMMITTED TO THIS REPOSITORY.
#
#   * ~16 MB of platform-specific artifact that git would keep forever, paid
#     for by every contributor including the ones who can never run it.
#   * It is not ours to re-host: fetching the vendor's own release at packaging
#     time keeps provenance (and the GPLv3 obligations in
#     docs/09-legal/third-party-licenses.md) where they belong.
#   * A pinned URL + pinned checksum is a stronger guarantee than a committed
#     blob, and it is reviewable in a diff.
#
# The portable archives hold the executable only. The GPLv3 text (COPYING),
# the BSD notice (COPYING.BSD) and AUTHORS come from the SumatraPDF source
# repository at the commit the release tag points to, each with its own pinned
# SHA-256. A missing or altered licence file fails the script.
#
# Usage:
#   scripts/fetch-sumatrapdf.sh [--arch x64|arm64] [--force]
#   SUMATRAPDF_SHA256=<hex> overrides the archive pin (testing a new release).

set -euo pipefail

# --- Pinned upstream release -------------------------------------------------
# Bump these together, never one alone, and keep fetch-sumatrapdf.ps1 in step
# (src-tauri/tests/fetch_scripts.rs compares the two).
VERSION="3.5.2"
SHA256_X64="66ccb395c9184dce6822dfbb9970c877383b3ead6d9417b5106a844aac512989"
SHA256_ARM64="d6fa6678f3dd3017515faa74735ecbf860cb6825bd61f109f9157f0b171e882f"
SOURCE_COMMIT="646d1feddcc80b3b51072c5b27a1446487904175"
SOURCE_REPO="https://github.com/sumatrapdfreader/sumatrapdf"
# name in the repository : name in the bundle : SHA-256
LICENSES=(
  "COPYING:LICENSE.txt:3972dc9744f6499f0f9b2dbf76696f2ae7ad8af9b23dde66d6af86c9dfb36986"
  "COPYING.BSD:COPYING.BSD.txt:ff33648659aa06892ed13a731588a57006fafee2f848d35f70bf273a13cf9d27"
  "AUTHORS:AUTHORS.txt:e16c411b4a9e65058e198dbb3a5e5941a65d9c44bd1a321b977644ec0b18aa53"
)

arch="x64"
force=0
while [[ $# -gt 0 ]]; do
  case "$1" in
    --force) force=1 ;;
    --arch) shift; arch="${1:-}" ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
  shift
done

case "$arch" in
  x64) suffix="64"; pin="$SHA256_X64" ;;
  arm64) suffix="arm64"; pin="$SHA256_ARM64" ;;
  *) echo "--arch must be x64 or arm64" >&2; exit 2 ;;
esac

ARCHIVE_URL="https://www.sumatrapdfreader.org/dl/rel/${VERSION}/SumatraPDF-${VERSION}-${suffix}.zip"
sha256="$(printf '%s' "${SUMATRAPDF_SHA256:-$pin}" | tr '[:lower:]' '[:upper:]')"

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
dest_dir="${repo_root}/src-tauri/resources/sumatrapdf"
dest_exe="${dest_dir}/SumatraPDF.exe"
stamp="${dest_dir}/SOURCE.txt"
stamp_line="SumatraPDF ${VERSION} ${arch} ${sha256}"

if [[ -f "$dest_exe" && -f "$stamp" && $force -eq 0 ]]; then
  if [[ "$(head -n 1 "$stamp" | tr -d '\r')" == "$stamp_line" ]]; then
    echo "SumatraPDF ${VERSION} (${arch}) already present at ${dest_exe} (use --force to refresh)."
    exit 0
  fi
fi

sha_of() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | cut -d' ' -f1
  else
    shasum -a 256 "$1" | cut -d' ' -f1
  fi
}

fetch_verified() { # url out expected
  echo "Downloading $1"
  curl --fail --location --silent --show-error --output "$2" "$1"
  local actual expected
  actual="$(sha_of "$2" | tr '[:lower:]' '[:upper:]')"
  expected="$(printf '%s' "$3" | tr '[:lower:]' '[:upper:]')"
  if [[ "$actual" != "$expected" ]]; then
    echo "Checksum mismatch for $1" >&2
    echo "  expected ${expected}" >&2
    echo "  actual   ${actual}" >&2
    exit 1
  fi
  echo "Checksum OK (${actual})."
}

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

archive="${work}/sumatrapdf.zip"
fetch_verified "$ARCHIVE_URL" "$archive" "$sha256"
unzip -q "$archive" -d "${work}/unpacked"

exe="$(find "${work}/unpacked" -type f -name '*.exe' | head -n 1)"
if [[ -z "$exe" ]]; then
  echo "No .exe inside ${ARCHIVE_URL} - has the archive layout changed?" >&2
  exit 1
fi

# GPLv3 obligation: the licence travels with the binary we redistribute.
mkdir -p "${work}/licenses"
for entry in "${LICENSES[@]}"; do
  IFS=: read -r from to hash <<<"$entry"
  fetch_verified "https://raw.githubusercontent.com/sumatrapdfreader/sumatrapdf/${SOURCE_COMMIT}/${from}" \
    "${work}/licenses/${to}" "$hash"
done

mkdir -p "$dest_dir"
cp "$exe" "$dest_exe"
cp "${work}/licenses/"* "$dest_dir/"
{
  printf '%s\r\n' "$stamp_line" ""
  printf '%s\r\n' "SumatraPDF.exe is the unmodified ${arch} build of SumatraPDF ${VERSION},"
  printf '%s\r\n' "downloaded from ${ARCHIVE_URL}" ""
  printf '%s\r\n' "SumatraPDF is free software under the GNU General Public License v3"
  printf '%s\r\n' "(LICENSE.txt); parts of it are under the BSD licence in COPYING.BSD.txt."
  printf '%s\r\n' "Its complete corresponding source code is the upstream repository at"
  printf '%s\r\n' "commit ${SOURCE_COMMIT} (tag ${VERSION}rel):"
  printf '%s\r\n' "${SOURCE_REPO}/tree/${SOURCE_COMMIT}"
} >"$stamp"

echo "SumatraPDF ${VERSION} (${arch}) installed at ${dest_exe}, with its licence."
