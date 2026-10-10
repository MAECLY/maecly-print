#!/usr/bin/env bash
#
# Regenerate THIRD-PARTY-NOTICES.md, the notice file every bundle ships.
#
#   scripts/licenses/generate-notices.sh          rewrite the file
#   scripts/licenses/generate-notices.sh --check  fail if it is out of date
#
# Needs cargo-about 0.9.2 (`cargo install cargo-about --locked --features cli
# --version 0.9.2`), python3 and the frontend dependencies installed
# (`pnpm install`). The hand-written part is scripts/licenses/notices-header.md;
# the crate and package notices are generated from Cargo.lock and
# pnpm-lock.yaml. Run it after any dependency change and commit the result:
# CI runs it with --check.

set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
here="$root/scripts/licenses"
work="$root/src-tauri/target/notices"
mode="write"
case "${1:-}" in
  --check) mode="check" ;;
  "") ;;
  *) echo "usage: $0 [--check]" >&2; exit 2 ;;
esac

want="0.9.2"
have="$(cargo about --version 2>/dev/null | awk '{print $2}')" || true
if [[ "$have" != "$want" ]]; then
  echo "cargo-about $want is required (found: ${have:-none})." >&2
  echo "  cargo install cargo-about --locked --features cli --version $want" >&2
  exit 1
fi

mkdir -p "$work"

# The crate sources must be on disk: cargo-about then reads every licence
# file offline, so the output depends only on the lockfiles.
for manifest in src-tauri/Cargo.toml shell-ext-win/Cargo.toml; do
  cargo fetch --locked --manifest-path "$root/$manifest" >/dev/null
done

cargo about generate --format json --offline --locked --fail \
  -c "$here/about.toml" -m "$root/src-tauri/Cargo.toml" -o "$work/app.json"
cargo about generate --format json --offline --locked --fail \
  -c "$here/about.toml" -m "$root/shell-ext-win/Cargo.toml" -o "$work/shellext.json"

(cd "$root" && pnpm licenses list --prod --json) > "$work/frontend.json"

python3 -I "$here/render_notices.py" \
  --header "$here/notices-header.md" \
  --nsis "$here/texts/NSIS-COPYING.txt" \
  --app "$work/app.json" \
  --shellext "$work/shellext.json" \
  --pnpm "$work/frontend.json" \
  --node-modules "$root/node_modules" \
  --extra-package tailwindcss \
  --out "$work/raw.md"

# Through the repository's own Prettier, so the commit hook leaves it alone.
(cd "$root" && pnpm exec prettier --stdin-filepath THIRD-PARTY-NOTICES.md) \
  < "$work/raw.md" > "$work/THIRD-PARTY-NOTICES.md"

if [[ "$mode" == "check" ]]; then
  if ! diff -u "$root/THIRD-PARTY-NOTICES.md" "$work/THIRD-PARTY-NOTICES.md" > "$work/notices.diff"; then
    head -n 80 "$work/notices.diff" >&2
    echo "::error file=THIRD-PARTY-NOTICES.md::out of date: run scripts/licenses/generate-notices.sh and commit the result" >&2
    exit 1
  fi
  echo "THIRD-PARTY-NOTICES.md is current."
else
  cp "$work/THIRD-PARTY-NOTICES.md" "$root/THIRD-PARTY-NOTICES.md"
  echo "THIRD-PARTY-NOTICES.md regenerated ($(wc -c < "$root/THIRD-PARTY-NOTICES.md") bytes)."
fi
