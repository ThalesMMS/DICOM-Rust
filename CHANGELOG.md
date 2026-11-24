# Change log

All notable changes to this package are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the package
follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html) as
described in [VERSIONING.md](VERSIONING.md), which also says what counts as
a breaking change and what each release must pass.

Each release ends with a "Release gates" note: the result of the
hour-per-target fuzz run, the public API diff against the previous version
and the minimum supported Rust version.

## [0.1.0] - 2026-11-24

First release.

### Added

- Part 10 files and data sets: reading, writing, transfer syntaxes, the
  data dictionary, character sets, DICOM JSON and XML, DICOMDIR, validation
  and de-identification.
- Pixel Data: native and RLE, JPEG baseline, extended and lossless, JPEG-LS,
  JPEG 2000 and HTJ2K, JPEG XL; decoding, encoding and transcoding.
- Display and rendering: grayscale and colour pipelines, presentation
  states, volume assembly, multiplanar and volume rendering, ROIs, filters,
  meshes, registration, segmentations and RT objects.
- Networking: the DICOM Upper Layer with TLS, DIMSE services as user and
  provider, MLLP, and DICOMweb client and server with OpenID Connect and
  audit logging.
- PS3.16 terminology and Structured Reporting, digital signatures and
  reversible de-identification.
- Features per area (`net`, `tls`, `dicomweb`, `jpeg`, `jpeg-lossless`,
  `jpeg-ls`, `jpeg2000`, `jpeg-xl`, `codecs`, `render`, `codes`, `dsig`,
  `deid-encrypted`), all on by default. Without default features the crate
  is the core: files, data sets, dictionaries, JSON/XML, DICOMDIR, native
  and RLE Pixel Data, display and metadata de-identification.
- Resource limits for untrusted input in one place (`limits::Limits`),
  applied by every read, every inflated stream, every decoder and the upper
  layer.
- Unsafe code confined to 12 items in 9 files; every other module carries
  `#![forbid(unsafe_code)]`.
- Fuzz targets over every path that reads untrusted bytes, a benchmark
  suite, the public API snapshot (`api/public-api.txt`) and `NOTICES`
  generated from `third_party/manifest.json`.

### Release gates

- Fuzzing: all 30 fuzz targets ran for one hour each under libFuzzer, eight
  at a time, with no crash, timeout, leak or out-of-memory report.
- Public API: first release; `api/public-api.txt` is the baseline for the
  next diff.
- Minimum supported Rust version: 1.88.

[0.1.0]: https://github.com/ThalesMMS/dicom-rust/releases/tag/v0.1.0
