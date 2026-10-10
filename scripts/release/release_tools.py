#!/usr/bin/env python3
"""Helpers for .github/workflows/release.yml and release-channels.yml.

Plain standard-library Python, so the ubuntu runner needs nothing installed,
and every rule the workflows depend on is a function that
src-tauri/tests/release_pipeline.rs exercises on a developer machine.

Subcommands:

  semver-gt CANDIDATE CURRENT
      Exit 0 when CANDIDATE is a higher SemVer 2.0 precedence than CURRENT,
      1 otherwise (equal included). A leading "v" is ignored, and so is build
      metadata (SemVer section 10). Used to move the beta channel's manifest
      forward only.

  finalise-manifest --manifest F --assets F --repo OWNER/NAME
                    [--body-file F] [--require KEY ...] [--out F]
      Rewrite every platform URL of a tauri-action latest.json to the GitHub
      API asset form (https://api.github.com/repos/<repo>/releases/assets/<id>),
      check that each named asset belongs to this release (--assets is the JSON
      of GET /repos/<repo>/releases/<id>/assets), require the platform keys
      given with --require, and use the release body as the update notes.

  check-manifest --manifest F --version V [--require KEY ...]
      Fail unless the manifest names version V and every required platform
      with a url and a signature.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
import urllib.parse

SEMVER = re.compile(
    r"v?(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)"
    r"(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?"
    r"(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?"
)


def semver_key(version: str):
    """A sort key with SemVer 2.0 precedence. Raises ValueError otherwise."""
    m = SEMVER.fullmatch(version.strip())
    if not m:
        raise ValueError(f"not a SemVer version: {version!r}")
    core = tuple(int(x) for x in m.group(1, 2, 3))
    pre = m.group(4)
    if pre is None:
        # A release ranks above every prerelease of the same core.
        return core, (1,)
    ids = []
    for part in pre.split("."):
        # Numeric identifiers rank below alphanumeric ones (section 11.4.3).
        ids.append((0, int(part), "") if part.isdigit() else (1, 0, part))
    return core, (0, tuple(ids))


def semver_gt(candidate: str, current: str) -> bool:
    return semver_key(candidate) > semver_key(current)


API_ASSET = "https://api.github.com/repos/{repo}/releases/assets/"


def finalise_manifest(manifest: dict, assets: list, repo: str, body: str | None,
                      required: list[str]) -> list[str]:
    """Rewrite `manifest` in place. Returns the list of errors (empty = ok)."""
    errors = []
    ids = {a["name"]: a["id"] for a in assets}
    by_id = set(ids.values())
    prefix = API_ASSET.format(repo=repo)
    platforms = manifest.get("platforms", {})
    for target, entry in platforms.items():
        url = entry.get("url", "")
        last = urllib.parse.unquote(url.rsplit("/", 1)[-1])
        if url.startswith(prefix) and last.isdigit() and int(last) in by_id:
            continue
        if last in ids:
            entry["url"] = f"{prefix}{ids[last]}"
        else:
            errors.append(
                f"latest.json names {last} for {target}, which is not an asset of this release"
            )
    for key in required:
        if key not in platforms:
            errors.append(
                f"latest.json has no {key} entry; re-run the build for that platform"
            )
    if body and body.strip():
        manifest["notes"] = body.strip()
    return errors


def check_manifest(manifest: dict, version: str, required: list[str]) -> list[str]:
    errors = []
    if str(manifest.get("version", "")).lstrip("v") != version.lstrip("v"):
        errors.append(
            f"latest.json is for {manifest.get('version')!r}, not {version!r}"
        )
    platforms = manifest.get("platforms", {})
    for key in required:
        entry = platforms.get(key)
        if not entry:
            errors.append(f"latest.json has no {key} entry")
        elif not entry.get("url") or not entry.get("signature"):
            errors.append(f"latest.json's {key} entry lacks a url or a signature")
    return errors


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(prog="release_tools.py")
    sub = parser.add_subparsers(dest="cmd", required=True)

    p = sub.add_parser("semver-gt")
    p.add_argument("candidate")
    p.add_argument("current")

    p = sub.add_parser("finalise-manifest")
    p.add_argument("--manifest", required=True)
    p.add_argument("--assets", required=True)
    p.add_argument("--repo", required=True)
    p.add_argument("--body-file")
    p.add_argument("--require", action="append", default=[])
    p.add_argument("--out")

    p = sub.add_parser("check-manifest")
    p.add_argument("--manifest", required=True)
    p.add_argument("--version", required=True)
    p.add_argument("--require", action="append", default=[])

    args = parser.parse_args(argv)

    if args.cmd == "semver-gt":
        try:
            return 0 if semver_gt(args.candidate, args.current) else 1
        except ValueError as e:
            print(f"::error::{e}", file=sys.stderr)
            return 2

    if args.cmd == "finalise-manifest":
        with open(args.manifest, encoding="utf-8") as f:
            manifest = json.load(f)
        with open(args.assets, encoding="utf-8") as f:
            assets = json.load(f)
        body = None
        if args.body_file:
            with open(args.body_file, encoding="utf-8") as f:
                body = f.read()
        errors = finalise_manifest(manifest, assets, args.repo, body, args.require)
        for e in errors:
            print(f"::error::{e}", file=sys.stderr)
        if errors:
            return 1
        with open(args.out or args.manifest, "w", encoding="utf-8") as f:
            json.dump(manifest, f, indent=2)
            f.write("\n")
        return 0

    if args.cmd == "check-manifest":
        with open(args.manifest, encoding="utf-8") as f:
            manifest = json.load(f)
        errors = check_manifest(manifest, args.version, args.require)
        for e in errors:
            print(f"::error::{e}", file=sys.stderr)
        return 1 if errors else 0

    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
