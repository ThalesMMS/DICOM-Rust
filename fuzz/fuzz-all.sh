#!/bin/sh
set -e
cd "$(dirname "$0")"
seconds="${1:-3600}"
runner="${2:-soak}"
jobs="${3:-4}"
if [ "$runner" = soak ]; then
  exec cargo run --release -p dicom-rust-fuzz-harness --bin soak -- \
    --target all --seconds "$seconds" --jobs "$jobs"
fi
case "$(uname -s)" in
  MINGW* | MSYS* | CYGWIN* | Windows_NT) default_sanitizer=none ;;
  *) default_sanitizer=address ;;
esac
sanitizer="${SANITIZER:-$default_sanitizer}"
cargo +nightly fuzz build -O -s "$sanitizer"
targets="$(cargo +nightly fuzz list)"
failed=0
run_one() {
  mkdir -p "work/$1" "corpus/$1" "artifacts/$1"
  cargo +nightly fuzz run -O -s "$sanitizer" "$1" "work/$1" "corpus/$1" -- \
    -max_total_time="$seconds" -max_len=65536 -rss_limit_mb=4096 -timeout=60 \
    -artifact_prefix="artifacts/$1/" -print_final_stats=1 > "work/$1.log" 2>&1
}
set -- $targets
while [ $# -gt 0 ]; do
  pids=""
  i=0
  while [ $i -lt "$jobs" ] && [ $# -gt 0 ]; do
    run_one "$1" &
    pids="$pids $!:$1"
    shift
    i=$((i + 1))
  done
  for entry in $pids; do
    if ! wait "${entry%%:*}"; then
      echo "fuzz-all: ${entry#*:} FAILED (see work/${entry#*:}.log and artifacts/${entry#*:}/)"
      failed=1
    else
      echo "fuzz-all: ${entry#*:}: $(grep -E '^Done|stat::number_of_executed_units' "work/${entry#*:}.log" | tr '\n' ' ')"
    fi
  done
done
exit $failed
