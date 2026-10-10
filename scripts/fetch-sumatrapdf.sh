#!/usr/bin/env bash
#
# Download the SumatraPDF binary Pluryprint bundles on Windows, with its
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
# SHA-256. A missing or altered licence file fails the script. A
# SOURCE-OFFER.txt (GPL-3.0 section 6) is written next to the executable.
#
# Usage:
#   scripts/fetch-sumatrapdf.sh [--arch x64|arm64] [--force]
#   SUMATRAPDF_SHA256=<hex> overrides the archive pin (testing a new release).

set -euo pipefail

# --- Pinned upstream release -------------------------------------------------
# Bump these together, never one alone, and keep fetch-sumatrapdf.ps1 in step
# (src-tauri/tests/fetch_scripts.rs compares the two).
VERSION="3.6.1"
SHA256_X64="98b33a518d42986856d225064b0cd2d3643ecf78cbf84ab873d26cc51877a544"
SHA256_ARM64="f21091ca6741e4049b0ca375121708a7959803d39336f0468084808943667ffe"
SOURCE_COMMIT="16c59fde8b824ab54c56f23aef910a6fdd874ad0"
SOURCE_REPO="https://github.com/sumatrapdfreader/sumatrapdf"
# Where Pluryprint publishes its releases (and the source archive above).
OFFER_RELEASES="https://github.com/MAECLY/pluryprint/releases"
OFFER_ISSUES="https://github.com/MAECLY/pluryprint/issues"
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
offer="${dest_dir}/SOURCE-OFFER.txt"
stamp_line="SumatraPDF ${VERSION} ${arch} ${sha256}"

if [[ -f "$dest_exe" && -f "$stamp" && -f "$offer" && $force -eq 0 ]]; then
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

# GPL-3.0 section 6: the Corresponding Source of the object code we convey.
# release.yml attaches the same tree to every release as
# SumatraPDF-<version>-source.tar.gz.
{
  printf '%s\r\n' "Written offer for the Corresponding Source of SumatraPDF"
  printf '%s\r\n' "(GNU General Public License version 3, section 6)" ""
  printf '%s\r\n' "This copy of Pluryprint includes SumatraPDF.exe, the unmodified ${arch} build"
  printf '%s\r\n' "of SumatraPDF ${VERSION}. SumatraPDF is free software under the GNU General"
  printf '%s\r\n' "Public License version 3 (LICENSE.txt); parts of it are under the BSD"
  printf '%s\r\n' "licence in COPYING.BSD.txt, and MuPDF, compiled into it, is under the GNU"
  printf '%s\r\n' "Affero General Public License version 3. Pluryprint runs it as a separate"
  printf '%s\r\n' "program and does not modify it." ""
  printf '%s\r\n' "Its Corresponding Source is the SumatraPDF source tree at commit"
  printf '%s\r\n' "${SOURCE_COMMIT} (tag ${VERSION}rel):"
  printf '%s\r\n' "  ${SOURCE_REPO}/tree/${SOURCE_COMMIT}"
  printf '%s\r\n' "An archive of that exact tree is attached to every Pluryprint release that"
  printf '%s\r\n' "ships this build, next to the installers, as SumatraPDF-${VERSION}-source.tar.gz:"
  printf '%s\r\n' "  ${OFFER_RELEASES}" ""
  printf '%s\r\n' "For at least three years after the last Pluryprint release that ships this"
  printf '%s\r\n' "build, the Pluryprint maintainers will give anyone who asks a copy of that"
  printf '%s\r\n' "Corresponding Source, at no charge beyond the cost of physically performing"
  printf '%s\r\n' "the transfer. Ask through the issue tracker:"
  printf '%s\r\n' "  ${OFFER_ISSUES}"
} >"$offer"

echo "SumatraPDF ${VERSION} (${arch}) installed at ${dest_exe}, with its licence."
