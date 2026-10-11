#!/usr/bin/env python3
"""Fill the landing page in site/ with the published release.

Used by .github/workflows/pages.yml, so the landing follows every release on
its own: nothing in site/ names a version, a date, a download link or a file
size. Those places carry markers instead, and this script copies site/ to an
output folder with every marker replaced from the release's GitHub API record
(GET /repos/<repo>/releases/latest). The generated folder is what Pages
publishes; it is never committed.

Plain standard-library Python, so the runner needs nothing installed, and
src-tauri/tests/site_release.rs runs it against a fixture.

  apply_release.py --release-json F --repo OWNER/NAME --site DIR --out DIR

Markers (in any .html, .txt, .webmanifest, .xml or .json file):

  {{release.version}}           0.0.2 (the tag without its "v")
  {{release.date}}              2026-10-10 (published_at, UTC)
  {{release.date_es}}           10 de octubre de 2026
  {{release.date_en}}           10 October 2026
  {{release.notes_url}}         the release page
  {{release.sha256sums_url}}    the SHA256SUMS asset
  {{release.<file>.url}}        a download, <file> being windows_x64,
                                windows_arm64, apple_silicon or apple_intel
  {{release.<file>.mib_en}}     its size in MiB, one decimal: 17.3
  {{release.<file>.mib_es}}     the same with a decimal comma: 17,3

It fails, and so the deploy fails, when the release is a draft or a
pre-release, its tag is not v<SemVer>, any of the four installers or
SHA256SUMS is missing, empty or not where the landing would link it, a marker
is unknown or left over, a download marker is used nowhere, or a JSON file or
JSON-LD block no longer parses. There is no flag to skip any of that: the
landing never links a file that is not there.
"""

from __future__ import annotations

import argparse
import datetime
import json
import re
import shutil
import sys
from pathlib import Path

PRODUCT = "Pluryprint"

# Every download the landing offers: marker name -> file extension.
DOWNLOADS = {
    "windows_x64": ".exe",
    "windows_arm64": ".exe",
    "apple_silicon": ".dmg",
    "apple_intel": ".dmg",
}

SEMVER = re.compile(
    r"^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)"
    r"(?:-[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?$"
)
REPO = re.compile(r"^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$")
MARKER = re.compile(r"\{\{\s*release\.([^}]*?)\s*\}\}")
LEFTOVER = re.compile(r"\{\{\s*release|release\.[a-z_.0-9]*\}\}")
TEXT_SUFFIXES = {".html", ".txt", ".webmanifest", ".xml", ".json"}
JSON_LD = re.compile(
    r'<script type="application/ld\+json">(.*?)</script>', re.DOTALL
)

MONTHS_ES = [
    "enero", "febrero", "marzo", "abril", "mayo", "junio", "julio",
    "agosto", "septiembre", "octubre", "noviembre", "diciembre",
]
MONTHS_EN = [
    "January", "February", "March", "April", "May", "June", "July",
    "August", "September", "October", "November", "December",
]


class ReleaseError(Exception):
    pass


def mib(size: int) -> str:
    return f"{size / (1024 * 1024):.1f}"


def values(release: dict, repo: str) -> dict[str, str]:
    """Every marker's value, after checking the release is one to link."""
    if not REPO.match(repo):
        raise ReleaseError(f"not an OWNER/NAME repository: {repo!r}")
    if release.get("draft") or release.get("prerelease"):
        raise ReleaseError("the release is a draft or a pre-release")
    tag = release.get("tag_name")
    if not isinstance(tag, str) or not tag.startswith("v"):
        raise ReleaseError(f"tag {tag!r} is not v<version>")
    version = tag[1:]
    if not SEMVER.match(version):
        raise ReleaseError(f"tag {tag!r} is not v<SemVer>")

    try:
        published = datetime.datetime.fromisoformat(
            str(release.get("published_at")).replace("Z", "+00:00")
        ).astimezone(datetime.timezone.utc)
    except ValueError as e:
        raise ReleaseError(f"published_at: {e}") from None

    base = f"https://github.com/{repo}/releases"
    notes = f"{base}/tag/{tag}"
    if release.get("html_url") != notes:
        raise ReleaseError(f"html_url {release.get('html_url')!r} is not {notes}")

    assets = {a.get("name"): a for a in release.get("assets") or []}

    def asset_url(name: str) -> tuple[str, int]:
        asset = assets.get(name)
        if asset is None:
            raise ReleaseError(f"{tag} has no asset {name}")
        if asset.get("state", "uploaded") != "uploaded":
            raise ReleaseError(f"{name} is {asset.get('state')}, not uploaded")
        size = asset.get("size")
        if not isinstance(size, int) or size <= 0:
            raise ReleaseError(f"{name} is empty")
        url = f"{base}/download/{tag}/{name}"
        if asset.get("browser_download_url") != url:
            raise ReleaseError(
                f"{name} downloads from {asset.get('browser_download_url')!r},"
                f" not {url}"
            )
        return url, size

    out = {
        "version": version,
        "date": published.strftime("%Y-%m-%d"),
        "date_es": (
            f"{published.day} de {MONTHS_ES[published.month - 1]}"
            f" de {published.year}"
        ),
        "date_en": (
            f"{published.day} {MONTHS_EN[published.month - 1]} {published.year}"
        ),
        "notes_url": notes,
        "sha256sums_url": asset_url("SHA256SUMS")[0],
    }
    for label, ext in DOWNLOADS.items():
        url, size = asset_url(f"{PRODUCT}_{version}_{label}{ext}")
        out[f"{label}.url"] = url
        out[f"{label}.mib_en"] = mib(size)
        out[f"{label}.mib_es"] = mib(size).replace(".", ",")
    return out


def render(text: str, vals: dict[str, str], where: str, used: set[str]) -> str:
    def one(m: re.Match) -> str:
        key = m.group(1)
        if key not in vals:
            raise ReleaseError(f"{where}: unknown marker {m.group(0)}")
        used.add(key)
        return vals[key]

    out = MARKER.sub(one, text)
    if LEFTOVER.search(out):
        raise ReleaseError(f"{where}: a malformed marker is left over")
    return out


def check_json(path: Path, text: str) -> None:
    try:
        if path.suffix in (".json", ".webmanifest"):
            json.loads(text)
        elif path.suffix == ".html":
            for block in JSON_LD.findall(text):
                json.loads(block)
    except json.JSONDecodeError as e:
        raise ReleaseError(f"{path}: no longer valid JSON: {e}") from None


def apply(release: dict, repo: str, site: Path, out: Path) -> dict[str, str]:
    vals = values(release, repo)
    if not site.is_dir():
        raise ReleaseError(f"{site} is not a folder")
    if out.exists() and any(out.iterdir()):
        raise ReleaseError(f"{out} is not empty")
    if out.resolve().is_relative_to(site.resolve()):
        raise ReleaseError(f"{out} is inside {site}")
    shutil.copytree(site, out, dirs_exist_ok=True)

    used: set[str] = set()
    for path in sorted(out.rglob("*")):
        if not path.is_file() or path.suffix not in TEXT_SUFFIXES:
            continue
        text = path.read_text(encoding="utf-8")
        rel = path.relative_to(out).as_posix()
        new = render(text, vals, rel, used)
        check_json(path.relative_to(out), new)
        if new != text:
            path.write_text(new, encoding="utf-8")

    unused = [f"{label}.url" for label in DOWNLOADS if f"{label}.url" not in used]
    if unused:
        raise ReleaseError(f"site/ never links {', '.join(unused)}")
    return vals


def main(argv: list[str]) -> int:
    p = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    p.add_argument("--release-json", required=True, type=Path)
    p.add_argument("--repo", required=True)
    p.add_argument("--site", required=True, type=Path)
    p.add_argument("--out", required=True, type=Path)
    a = p.parse_args(argv)
    try:
        release = json.loads(a.release_json.read_text(encoding="utf-8"))
        vals = apply(release, a.repo, a.site, a.out)
    except (OSError, json.JSONDecodeError, ReleaseError) as e:
        print(f"apply_release: {e}", file=sys.stderr)
        return 1
    print(f"site/ filled with {PRODUCT} {vals['version']} ({vals['date']}):")
    for label in DOWNLOADS:
        print(f"  {vals[label + '.url']} ({vals[label + '.mib_en']} MiB)")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
