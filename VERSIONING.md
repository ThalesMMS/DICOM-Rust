# Versioning and releases

The package follows [Semantic Versioning 2.0.0](https://semver.org/) as
Cargo applies it. Every release is listed in [CHANGELOG.md](CHANGELOG.md).

## Version numbers

- **Before 1.0** (`0.y.z`): a breaking change raises `y` and resets `z`;
  anything else raises `z`. Cargo treats `0.y` as the compatibility range,
  so `0.3.1` replaces `0.3.0` for every user and `0.4.0` is a new range.
- **From 1.0** (`x.y.z`): a breaking change raises `x`; a compatible
  addition raises `y`; a fix that adds nothing raises `z`.
- The first published version is `0.1.0` (the version in `Cargo.toml`).
  1.0 follows once the public surface is settled: the second pass over the
  API, the module renames, idiomatic errors and the API documentation.
- Pre-releases (`-alpha.N`, `-rc.N`) are used only when a release needs
  outside testing before it is final.

## What is a breaking change

A change is breaking when code, a build or stored output that worked with
the previous version can stop working with the new one. For this package
that means:

1. **Public API.** Any line removed from or changed in
   `api/public-api.txt` (a public item removed, renamed, moved or made
   private; a signature, field, variant, trait bound or implemented trait
   changed). Lines only added are compatible. Items marked
   `#[doc(hidden)]` are not part of the API and are not in the listing;
   they may change in any release. The `semver` job of the continuous
   integration (`cargo semver-checks`) checks the same against the last
   published version, including what the listing cannot show (for example
   a type that stops being `Send`).
2. **Features.** Removing or renaming a Cargo feature, removing something
   from the `default` set, or making a feature need another one that it did
   not need. Adding a feature, or adding to what an existing feature
   enables without changing what it already enabled, is compatible.
3. **Minimum supported Rust version.** See below; raising it is not a
   breaking change, but it never happens in a patch release.
4. **Bytes written for the same input.** For the same input and the same
   options, the bytes the package writes (Part 10 files, data sets,
   encoded Pixel Data of the lossless codecs, DICOMDIR, DICOM JSON and XML,
   PDUs and DIMSE messages) and the values it decodes (pixel values of every
   codec, lossless or not) stay the same within a compatible range. A
   change is breaking unless it corrects output that did not conform to the
   DICOM standard or to the codec's own specification; such a correction is
   released as a fix, with a "Changed output" entry in the change log that
   says which inputs are affected.
5. **Accepted input.** Lowering a default of `limits::Limits` (or of any
   other limit type), or refusing input that the previous version read, is
   breaking unless the input was malformed. Raising a default is
   compatible.
6. **Rendered images.** A change in the pixels that the rendering
   functions produce for the same input and parameters (window, transfer
   function, colour table, interpolation) is breaking unless it fixes a
   defect; the change log names the functions affected.

Not part of the compatibility promise: the text of error messages and of
`Debug` output, performance, memory use below the documented limits, and
the order of items in a hash-based collection.

## Minimum supported Rust version (MSRV)

- The MSRV is the `rust-version` field of `Cargo.toml`. The `msrv` job
  checks the library with the default features and without them on exactly
  that toolchain. Tests, benchmarks and the fuzz harness need a current
  stable toolchain, and the public-API and Miri jobs need the pinned
  nightly toolchain named in the workflow.
- Measured on 2026-11-23: the library needs **Rust 1.88** (let chains in
  the 2024 edition are stable from 1.88; trait upcasting needs 1.86). It
  does not build on 1.85. The benchmarks need a later stable toolchain.
- The MSRV is raised only in a minor release (a `0.y` release before 1.0),
  never in a patch release, and never to a toolchain released less than
  six months before. The change log says when it moves.

## Release checklist

Each step is done on the commit that will be published, in this order.

1. **Change log.** Rename "Unreleased" to `[x.y.z] - YYYY-MM-DD`, check that
   every entry is there and that each breaking change is marked as such;
   start a new empty "Unreleased" section.
2. **Version.** Choose `x.y.z` by the rules above: any breaking change
   (public API lines removed or changed, a feature removed, changed output)
   needs the major step (`0.y` before 1.0). Set it in `Cargo.toml`.
3. **Continuous integration.** Every blocking job passes on that commit:
   rustfmt, Clippy with warnings denied, tests on Windows, Linux and macOS
   with each feature set, the build without default features, the MSRV
   check, the cross-target check, the public API snapshot, `cargo deny`
   (licences, bans, sources and advisories), the NOTICES check, the unsafe
   check, Miri, the fuzz smoke run and `cargo semver-checks`.
4. **Public API.** Review the diff of `api/public-api.txt` since the last
   release tag; it must agree with the version step.
5. **Fuzzing gate.** Run every fuzz target for one hour on a quiet machine:

   ```sh
   sh fuzz/fuzz-all.sh 3600
   ```

   (about five hours with four targets at a time). Where the instrumented
   build fits, also `sh fuzz/fuzz-all.sh 3600 libfuzzer`. The release needs
   zero failures. Any failing input is fixed, kept in `fuzz/corpus/`, and
   the run is repeated. Record the result in the release's "Release gates"
   note: commit, runner, seed, executions and failures per target.
6. **Notices and licence.**
   `python3 third_party/gen_notices.py --strict` passes (no entry or
   licence text still pending). The `license` field of `Cargo.toml` holds
   the licence as an SPDX expression, the matching licence
   file or files are at the package root, and `cargo package --list` shows
   them together with `NOTICES`, `CHANGELOG.md` and the README.
7. **Benchmarks** (informational). `benches/compare.sh` against the
   reference numbers; a regression is explained in the change log or
   fixed.
8. **Publish.** `cargo publish --dry-run`, then `cargo publish`. Tag the
   published commit `vx.y.z` (annotated tag) and push the tag.

A published version is yanked only for a soundness or security defect, and
only after a fixed version is out; the change log says why.
