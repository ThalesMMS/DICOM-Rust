#!/usr/bin/env bash
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$here"
pkg=dicom-rust
nightly="${NIGHTLY:-nightly}"
python() {
  if [ -n "${PYTHON:-}" ]; then "$PYTHON" "$@"; return; fi
  if python3 -c '' >/dev/null 2>&1; then python3 "$@"; else command python "$@"; fi
}
features() {
  case "${1:-default}" in
    default) ;;
    none) echo "--no-default-features" ;;
    all) echo "--all-features" ;;
    *) echo "unknown feature set: $1 (default, none, all)" >&2; exit 2 ;;
  esac
}
msrv() {
  if [ -n "${MSRV:-}" ]; then echo "$MSRV"; return; fi
  cargo metadata --format-version 1 --no-deps \
    | python -c 'import json,sys; m=json.load(sys.stdin); print(next(p["rust_version"] for p in m["packages"] if p["name"]=="dicom-rust"))'
}
job="${1:-}"
[ $# -gt 0 ] && shift
case "$job" in
  fmt)
    cargo fmt -p "$pkg" -- --check
    cargo fmt --manifest-path fuzz/Cargo.toml -p dicom-rust-fuzz -p dicom-rust-fuzz-harness \
      -p dicom-rust-fuzz-sancov-sections -- --check
    ;;
  clippy)
    cargo clippy -p "$pkg" --all-targets $(features "${1:-default}") -- -D warnings
    if [ "${1:-default}" = default ]; then
      cargo clippy --manifest-path fuzz/Cargo.toml -p dicom-rust-fuzz-harness --all-targets -- -D warnings
    fi
    ;;
  test)
    cargo test -p "$pkg" $(features "${1:-default}")
    ;;
  features)
    bash scripts/check-features.sh "${1:-check}"
    ;;
  build-no-default)
    cargo build -p "$pkg" --lib --no-default-features
    ;;
  msrv)
    version="$(msrv)"
    echo "minimum supported Rust: $version"
    rustup toolchain install "$version" --profile minimal --no-self-update
    cargo "+$version" check -p "$pkg" --lib
    cargo "+$version" check -p "$pkg" --lib --no-default-features
    ;;
  cross)
    for target in ${CROSS_TARGETS:-x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu x86_64-apple-darwin aarch64-apple-darwin x86_64-pc-windows-msvc aarch64-pc-windows-msvc}; do
      rustup target add "$target"
      cargo check -p "$pkg" --lib --tests --no-default-features --target "$target"
      cargo check -p "$pkg" --lib --tests --no-default-features --target "$target" \
        --features net,dicomweb,codecs,render,codes,dsig,deid-encrypted
    done
    ;;
  public-api)
    PUBLIC_API_TOOLCHAIN="$nightly" bash scripts/check-public-api.sh
    ;;
  deny)
    cargo deny --manifest-path Cargo.toml --all-features check licenses bans sources
    ;;
  advisories)
    cargo deny --manifest-path Cargo.toml --all-features check advisories
    ;;
  notices)
    python third_party/gen_notices.py --check
    ;;
  unsafe)
    python scripts/check-unsafe.py
    ;;
  miri)
    cargo "+$nightly" miri setup "$@"
    cargo "+$nightly" miri test -p "$pkg" --no-default-features "$@" --lib -- render::plane_buffer net::dimse::pdata
    cargo "+$nightly" miri test -p "$pkg" --no-default-features "$@" --test progressive_shared_planes
    cargo "+$nightly" miri test -p "$pkg" --no-default-features "$@" --test volume_link_race -- a_waiter
    ;;
  fuzz)
    seconds="${1:-10}"
    (cd fuzz && cargo run --release -p dicom-rust-fuzz-harness --bin soak -- \
      --target all --seconds "$seconds" --jobs "${FUZZ_JOBS:-4}")
    cargo check --manifest-path fuzz/Cargo.toml --bins
    ;;
  docs)
    RUSTDOCFLAGS="${RUSTDOCFLAGS:-} -D warnings" cargo doc -p "$pkg" --no-deps --all-features
    ;;
  semver)
    if [ -n "${1:-}" ]; then
      cargo semver-checks check-release -p "$pkg" --baseline-version "$1"
    elif cargo info "$pkg" --registry crates-io >/dev/null 2>&1; then
      cargo semver-checks check-release -p "$pkg"
    else
      echo "note: no published version of $pkg yet; nothing to compare with"
    fi
    ;;
  conformance)
    cache="${1:-${DICOM_RUST_CONFORMANCE_CACHE:-${XDG_CACHE_HOME:-$HOME/.cache}/dicom-rust-conformance}}"
    bash scripts/conformance-fetch.sh "$cache"
    if command -v cygpath >/dev/null 2>&1; then cache="$(cygpath -m "$cache")"; fi
    DICOM_RUST_CONFORMANCE_CACHE="$cache" DICOM_RUST_REQUIRE_CONFORMANCE=1       cargo test -p "$pkg" --test conformance -- --ignored --nocapture --test-threads=1
    if [ "${CONFORMANCE_DCMTK:-required}" = optional ]; then
      cargo test -p "$pkg" --test interop_dcmtk -- --ignored --nocapture --test-threads=1
    else
      DICOM_RUST_REQUIRE_DCMTK=1         cargo test -p "$pkg" --test interop_dcmtk -- --ignored --nocapture --test-threads=1
    fi
    ;;
  *)
    echo "usage: scripts/ci.sh {fmt|clippy|test|features|build-no-default|msrv|cross|public-api|deny|advisories|notices|unsafe|miri|fuzz|docs|semver|conformance} [arguments]" >&2
    exit 2
    ;;
esac
