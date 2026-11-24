#!/usr/bin/env bash
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$here"
pkg=dicom-rust
mode="${1:-check}"
[ $# -gt 0 ] && shift
python() {
  if [ -n "${PYTHON:-}" ]; then "$PYTHON" "$@"; return; fi
  if python3 -c '' >/dev/null 2>&1; then python3 "$@"; else command python "$@"; fi
}
case "$mode" in
  check) cmd=(check -p "$pkg" --lib --tests); tail=() ;;
  clippy) cmd=(clippy -p "$pkg" --all-targets); tail=(-- -D warnings) ;;
  test) cmd=(test -p "$pkg"); tail=() ;;
  *) echo "unknown mode: $mode (check, clippy, test)" >&2; exit 2 ;;
esac
features="$(cargo metadata --format-version 1 --no-deps \
  | python -c 'import json,sys; m=json.load(sys.stdin); p=next(p for p in m["packages"] if p["name"]=="dicom-rust"); print(" ".join(sorted(f for f in p["features"] if f != "default")))')"
pairs=(
  "net,tls"
  "dicomweb,tls"
  "dicomweb,codes"
  "dicomweb,dsig"
  "dicomweb,render"
  "render,dsig"
  "deid-encrypted,dicomweb"
)
app="net,dicomweb,codecs,render,codes,dsig,deid-encrypted"
run() {
  echo "== dicom-rust: $*" >&2
  if ! cargo "${cmd[@]}" "$@" "${tail[@]}"; then
    echo "FAILED: cargo ${cmd[*]} $* ${tail[*]}" >&2
    exit 1
  fi
}
if [ "$mode" != test ] && cargo hack --version >/dev/null 2>&1; then
  echo "== dicom-rust: cargo hack --each-feature" >&2
  cargo hack "${cmd[@]}" --each-feature "$@" "${tail[@]}"
else
  run --no-default-features "$@"
  for f in $features; do
    run --no-default-features --features "$f" "$@"
  done
  run "$@"
fi
for p in "${pairs[@]}"; do
  run --no-default-features --features "$p" "$@"
done
run --no-default-features --features "$app" "$@"
run --all-features "$@"
echo "dicom-rust: every feature set builds ($mode)" >&2
