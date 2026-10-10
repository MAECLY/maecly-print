#!/usr/bin/env bash
# Prints, NUL-separated, the tracked text files the mechanical rename touches.
# Excluded on purpose (see APPLY.md, "Keeps"): history (CHANGELOG.md, the
# existing ADRs), the archived design hand-off, the landing site (owned by
# another branch), lockfiles (cargo regenerates them) and this folder.
#
# Optional arguments narrow the set to pathspecs, e.g. `files.sh src-tauri`.
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"
if [ "$#" -eq 0 ]; then set -- .; fi
git grep -lzIiE 'maecly' -- "$@" \
  ':!CHANGELOG.md' \
  ':!docs/adr/0*.md' \
  ':!claude design/**' \
  ':!site/**' \
  ':!**/Cargo.lock' \
  ':!pnpm-lock.yaml' \
  ':!scripts/rename/**'
