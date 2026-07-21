#!/usr/bin/env bash
#
# Download the SumatraPDF binary MaeclyPrint bundles on Windows.
#
# This is the POSIX twin of scripts/fetch-sumatrapdf.ps1 and exists for
# completeness: a Linux CI runner cross-compiling a Windows bundle, or a
# maintainer on macOS who wants to verify the pin without a Windows box. The
# fetched executable is a Windows PE — it does nothing on this host, and the
# `resources/sumatrapdf/*` glob in src-tauri/tauri.conf.json deliberately
# matches nothing on a macOS/Linux bundle, so running this on a Mac has no
# effect on the .app you build afterwards.
#
# THE BINARY IS DELIBERATELY NOT COMMITTED TO THIS REPOSITORY.
#
#   * ~20 MB of platform-specific artifact that git would keep forever, paid for
#     by every contributor including the ones who can never run it.
#   * It is not ours to re-host: fetching the vendor's own release at packaging
#     time keeps provenance (and the GPLv3 obligations in
#     docs/09-legal/third-party-licenses.md) where they belong. The upstream
#     LICENSE is copied next to the executable so the bundle still carries it.
#   * A pinned URL + pinned checksum is a stronger guarantee than a committed
#     blob, and it is reviewable in a diff.
#
# Why `resources` and not `externalBin` in tauri.conf.json: `externalBin` is the
# sidecar mechanism — it requires a `-$TARGET_TRIPLE` suffix on the file name,
# is invoked through the shell plugin's `sidecar()` API (which we do not use;
# the engine spawns the process itself so it can pass CREATE_NO_WINDOW), and
# expects a binary per build target, which makes no sense for a Windows-only
# helper that must not end up inside the macOS .app. `resources` copies files
# verbatim into the bundle's resource directory, which is where
# engine::deps::bundled_dirs looks first.
#
# Usage:
#   SUMATRAPDF_SHA256=<hex> scripts/fetch-sumatrapdf.sh [--force]

set -euo pipefail

# --- Pinned upstream release -------------------------------------------------
# Bump these together, never one alone.
VERSION="3.5.2"
ARCHIVE_URL="https://www.sumatrapdfreader.org/dl/rel/${VERSION}/SumatraPDF-${VERSION}-64.zip"

# PIN ME. Intentionally empty: no checksum has been verified against the
# upstream release from this repository yet, and inventing one would make the
# verification below a lie that always passes. Fill it with the SHA-256 the
# SumatraPDF project publishes for exactly the file above, commit it, and the
# build becomes reproducible and tamper-evident. Until then the checksum has to
# be supplied explicitly and this script fails closed.
EXPECTED_SHA256=""

force=0
for arg in "$@"; do
  case "$arg" in
    --force) force=1 ;;
    *) echo "unknown argument: $arg" >&2; exit 2 ;;
  esac
done

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
dest_dir="${repo_root}/src-tauri/resources/sumatrapdf"
dest_exe="${dest_dir}/SumatraPDF.exe"

sha256="${SUMATRAPDF_SHA256:-$EXPECTED_SHA256}"

if [[ -f "$dest_exe" && $force -eq 0 ]]; then
  echo "SumatraPDF already present at ${dest_exe} (use --force to refresh)."
  exit 0
fi

if [[ -z "$sha256" ]]; then
  cat >&2 <<'EOF'
No SHA-256 to verify against.

Set the pin in this script (EXPECTED_SHA256), or pass one explicitly:
    SUMATRAPDF_SHA256=<hex> scripts/fetch-sumatrapdf.sh

Downloading an unverified binary and bundling it into a signed installer is not
something this script will do silently.
EOF
  exit 1
fi

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

archive="${work}/sumatrapdf.zip"
echo "Downloading ${ARCHIVE_URL}"
curl --fail --location --silent --show-error --output "$archive" "$ARCHIVE_URL"

if command -v sha256sum >/dev/null 2>&1; then
  actual="$(sha256sum "$archive" | cut -d' ' -f1)"
else
  actual="$(shasum -a 256 "$archive" | cut -d' ' -f1)"
fi

expected="$(printf '%s' "$sha256" | tr '[:upper:]' '[:lower:]')"
if [[ "$actual" != "$expected" ]]; then
  echo "Checksum mismatch for ${ARCHIVE_URL}" >&2
  echo "  expected ${expected}" >&2
  echo "  actual   ${actual}" >&2
  exit 1
fi
echo "Checksum OK (${actual})."

unzip -q "$archive" -d "${work}/unpacked"

# The portable archive holds a single, version-stamped executable
# (SumatraPDF-3.5.2-64.exe). Install it under the stable name the resolver looks
# for (engine::deps::spec -> "SumatraPDF.exe") so a version bump never changes
# the lookup.
exe="$(find "${work}/unpacked" -type f -name '*.exe' | head -n 1)"
if [[ -z "$exe" ]]; then
  echo "No .exe inside ${ARCHIVE_URL} — has the archive layout changed?" >&2
  exit 1
fi

mkdir -p "$dest_dir"
cp "$exe" "$dest_exe"

# GPLv3 obligation: the licence travels with the binary we redistribute.
license="$(find "${work}/unpacked" -type f -name 'COPYING*' | head -n 1)"
if [[ -n "$license" ]]; then
  cp "$license" "${dest_dir}/LICENSE.txt"
else
  echo "warning: no licence file in the archive; add SumatraPDF LICENSE.txt to the bundle by hand." >&2
fi

echo "SumatraPDF ${VERSION} installed at ${dest_exe}"
