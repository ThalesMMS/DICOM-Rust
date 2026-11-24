#!/usr/bin/env bash
set -euo pipefail
here="$(cd "$(dirname "$0")/.." && pwd)"
snapshot="$here/api/public-api.txt"
toolchain="${PUBLIC_API_TOOLCHAIN:-nightly}"
if ! cargo public-api --version >/dev/null 2>&1; then
  echo "error: cargo-public-api is not installed (cargo install cargo-public-api --locked)" >&2
  exit 2
fi
current="$(mktemp)"
trap 'rm -f "$current"' EXIT
cargo "+$toolchain" public-api --manifest-path "$here/Cargo.toml" -sss \
  | tr -d '\r' > "$current"
if [ ! -s "$current" ]; then
  echo "error: the public API listing is empty" >&2
  exit 2
fi
if [ "${1:-}" = "--update" ]; then
  mkdir -p "$(dirname "$snapshot")"
  cp "$current" "$snapshot"
  echo "snapshot updated: $(wc -l < "$snapshot") lines"
  exit 0
fi
if [ ! -f "$snapshot" ]; then
  echo "error: no snapshot at $snapshot (run with --update)" >&2
  exit 2
fi
if diff -u <(tr -d '\r' < "$snapshot") "$current" > "$current.diff"; then
  echo "public API unchanged: $(wc -l < "$current") lines"
  rm -f "$current.diff"
  exit 0
fi
added="$(grep -c '^+[^+]' "$current.diff" || true)"
removed="$(grep -c '^-[^-]' "$current.diff" || true)"
cat "$current.diff"
rm -f "$current.diff"
echo >&2
echo "error: the public API changed: $added lines added, $removed removed." >&2
echo "If the change is intended, run scripts/check-public-api.sh --update and commit the snapshot." >&2
exit 1
