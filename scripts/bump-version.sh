#!/usr/bin/env bash
#
# Set the app version for a release: scripts/bump-version.sh X.Y.Z[-pre.N]
#
# Cargo.toml is the only version the app reads (tauri.conf.json has no
# `version`, so the bundles, the updater and CARGO_PKG_VERSION agree by
# construction). package.json is kept equal so the frontend and the release
# guard see the same number, and Cargo.lock is refreshed in the same commit so
# the next cargo command does not leave the tree dirty.
#
# The Explorer extension (shell-ext-win) ships in the same installer, so its
# Cargo.toml, Cargo.lock and the VERSIONINFO of res/shellext.rc (the version
# Windows shows for shell_ext_win.dll) move with the app. VERSIONINFO is four
# numbers: X.Y.Z.0, without the pre-release suffix.
#
# A version with a pre-release suffix (0.2.0-beta.1) is published to the beta
# channel; one without is stable. .github/workflows/release.yml refuses a tag
# that differs from what this script wrote.
#
# Options:
#   --root DIR   operate on another checkout (tests)
#   --no-cargo   do not run cargo; patch Cargo.lock's own entry directly
#
# It does not commit or tag; it prints the commands to do so
# (docs/07-build-and-release/releasing.md, section 4).

set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
run_cargo=1
version=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --root) shift; root="$(cd "${1:?--root needs a directory}" && pwd)" ;;
    --no-cargo) run_cargo=0 ;;
    -h|--help) sed -n '2,21p' "$0"; exit 0 ;;
    -*) echo "unknown option: $1" >&2; exit 2 ;;
    *) version="$1" ;;
  esac
  shift
done

# SemVer 2.0, no leading "v", no build metadata (a tag is v<version>).
semver='^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-[0-9A-Za-z-]+(\.[0-9A-Za-z-]+)*)?$'
if [[ ! "$version" =~ $semver ]]; then
  echo "usage: $0 X.Y.Z[-pre.N]   ('${version}' is not a SemVer version without a leading v)" >&2
  exit 2
fi

cargo_toml="$root/src-tauri/Cargo.toml"
cargo_lock="$root/src-tauri/Cargo.lock"
package_json="$root/package.json"
ext_toml="$root/shell-ext-win/Cargo.toml"
ext_lock="$root/shell-ext-win/Cargo.lock"
ext_rc="$root/shell-ext-win/res/shellext.rc"
for f in "$cargo_toml" "$cargo_lock" "$package_json" "$ext_toml" "$ext_lock" "$ext_rc"; do
  [[ -f "$f" ]] || { echo "missing $f" >&2; exit 1; }
done

# The [package] version is the first `version = ` line of Cargo.toml.
python3 -I - "$cargo_toml" "$cargo_lock" "$package_json" "$ext_toml" "$ext_lock" "$ext_rc" "$version" <<'PY'
import json, re, sys

cargo_toml, cargo_lock, package_json, ext_toml, ext_lock, ext_rc, version = sys.argv[1:]


def crate(toml_path, lock_path):
    with open(toml_path, encoding="utf-8") as f:
        text = f.read()
    new, n = re.subn(r'(?m)^version = "[^"]*"$', f'version = "{version}"', text, count=1)
    if n != 1:
        sys.exit(f"no [package] version in {toml_path}")
    with open(toml_path, "w", encoding="utf-8") as f:
        f.write(new)

    name = re.search(r'(?m)^name = "([^"]+)"$', text).group(1)
    with open(lock_path, encoding="utf-8") as f:
        lock = f.read()
    lock, n = re.subn(
        rf'(?m)^(name = "{re.escape(name)}"\nversion = )"[^"]*"$', rf'\1"{version}"', lock, count=1
    )
    if n != 1:
        sys.exit(f"no {name} entry in {lock_path}")
    with open(lock_path, "w", encoding="utf-8") as f:
        f.write(lock)


crate(cargo_toml, cargo_lock)
crate(ext_toml, ext_lock)

# VERSIONINFO: FILEVERSION / PRODUCTVERSION a,b,c,0 and the matching strings.
nums = version.split("-", 1)[0].split(".")
with open(ext_rc, encoding="utf-8") as f:
    rc = f.read()
rc, n1 = re.subn(
    r'(?m)^(  (?:FILEVERSION|PRODUCTVERSION)\s+)[0-9]+,[0-9]+,[0-9]+,[0-9]+$',
    lambda m: m.group(1) + ",".join(nums + ["0"]),
    rc,
)
rc, n2 = re.subn(
    r'(?m)^(\s+VALUE "(?:FileVersion|ProductVersion)",\s+)"[0-9.]+"$',
    lambda m: m.group(1) + '"' + ".".join(nums + ["0"]) + '"',
    rc,
)
if n1 != 2 or n2 != 2:
    sys.exit(f"{ext_rc}: expected 2 numeric and 2 string versions, found {n1} and {n2}")
with open(ext_rc, "w", encoding="utf-8") as f:
    f.write(rc)

with open(package_json, encoding="utf-8") as f:
    raw = f.read()
pkg, n = re.subn(r'(?m)^(  "version": )"[^"]*"', rf'\1"{version}"', raw, count=1)
if n != 1 or json.loads(pkg)["version"] != version:
    sys.exit("no top-level version in package.json")
with open(package_json, "w", encoding="utf-8") as f:
    f.write(pkg)
PY

if [[ $run_cargo -eq 1 ]]; then
  # Confirms the lockfile edit is what cargo itself would write.
  cargo metadata --manifest-path "$cargo_toml" --format-version 1 --offline >/dev/null
  cargo metadata --manifest-path "$ext_toml" --format-version 1 --offline >/dev/null
fi

channel="stable"
[[ "$version" == *-* ]] && channel="beta"
cat <<EOF
Pluryprint is now version $version ($channel channel) in
  src-tauri/Cargo.toml, src-tauri/Cargo.lock, package.json and the Explorer
  extension (shell-ext-win/Cargo.toml, Cargo.lock, res/shellext.rc).
Next:
  add a "## [$version]" section to CHANGELOG.md
  git commit -s -am "chore(release): v$version"
  git tag -a v$version -m "Pluryprint $version"
  git push origin HEAD v$version
EOF
