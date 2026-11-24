# Benchmarks

## Run

```text
cargo bench -p dicom-rust                           the whole suite
cargo bench -p dicom-rust -- codec/j2k              names containing "codec/j2k"
cargo bench -p dicom-rust -- --quick                one short round
cargo bench -p dicom-rust -- --list                 every benchmark name
cargo bench -p dicom-rust -- --save run.json        keep the results
```

Relative paths are relative to the crate directory. The last results are
written to `<target>/tmp/dicom-rust-bench/last-run.json`.

## Compare

```text
benches/compare.sh                       run, compare with the reference
benches/compare.sh -b other.json         compare with another baseline
benches/compare.sh -r run.json           compare a saved run only
benches/compare.sh -t 0.1 -- codec/      10% tolerance, codec benchmarks only
```

The default baseline is `benches/reference/i7-14700k-win11.json`. A
benchmark regresses when its median is more than the tolerance (20% by
default) above the baseline median and its p25 is above the baseline's p75;
`compare.sh` then exits with status 1.
