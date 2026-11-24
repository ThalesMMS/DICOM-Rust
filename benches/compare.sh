#!/usr/bin/env bash
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
baseline="$here/reference/i7-14700k-win11.json"
run_file=""
tolerance="0.20"
extra=()
while [ $# -gt 0 ]; do
  case "$1" in
    -b) baseline="$2"; shift 2 ;;
    -r) run_file="$2"; shift 2 ;;
    -t) tolerance="$2"; shift 2 ;;
    --) shift; extra=("$@"); break ;;
    -h|--help) echo "usage: benches/compare.sh [-b baseline.json] [-r run.json] [-t tolerance] [-- suite arguments]"; exit 0 ;;
    *) echo "unknown argument $1" >&2; exit 2 ;;
  esac
done
abs() { case "$1" in /*|[A-Za-z]:*) echo "$1" ;; *) echo "$PWD/$1" ;; esac; }
baseline="$(abs "$baseline")"
[ -n "$run_file" ] && run_file="$(abs "$run_file")"
cd "$here/.."
if [ -n "$run_file" ]; then
  exec cargo bench -p dicom-rust --bench suite -- \
    --compare "$run_file" --baseline "$baseline" --tolerance "$tolerance"
fi
exec cargo bench -p dicom-rust --bench suite -- \
  --baseline "$baseline" --tolerance "$tolerance" "${extra[@]}"
