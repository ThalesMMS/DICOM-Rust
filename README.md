# dicom-rust

A DICOM toolkit in pure Rust: Part 10 files and data sets, pixel data codecs,
rendering, DIMSE and DICOMweb networking, and the information objects built on
them.

## Install

```toml
[dependencies]
dicom-rust = "0.1"
```

## Quick start

```rust,no_run
use dicom_rust::object::{self, Tag};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let file = object::open_file("image.dcm").map_err(|(_, e)| e)?;
    println!("transfer syntax {}", file.transfer_syntax.uid);
    let name = file.get_string(Tag::new(0x0010, 0x0010)).unwrap_or_default();
    let rows = file.get_int(Tag::new(0x0028, 0x0010))?;
    let columns = file.get_int(Tag::new(0x0028, 0x0011))?;
    println!("patient {name}, image {columns} x {rows}");
    Ok(())
}
```

More programs are in `examples/`.

## Features

All on by default. Without them the crate reads, writes and validates Part 10
files and data sets, with native and RLE Pixel Data.

| Feature | Adds |
|---|---|
| `net` | DICOM Upper Layer, DIMSE services, MLLP |
| `tls` | TLS for the Upper Layer, MLLP and DICOMweb |
| `dicomweb` | DICOMweb client and server, OpenID Connect, audit logging, FHIR |
| `codecs` | `jpeg`, `jpeg-lossless`, `jpeg-ls`, `jpeg2000`, `jpeg-xl` |
| `render` | rendering, ROIs, filters, meshes, registration, segmentations, RT objects |
| `codes` | PS3.16 terminology and Structured Reporting |
| `dsig` | digital signatures |
| `deid-encrypted` | reversible de-identification |

## Minimum supported Rust version

Rust 1.88.

## Licence

Copyright 2026 Thales Matheus Mendonça Santos.

Apache License, Version 2.0 ([LICENSE](LICENSE)). [NOTICES](NOTICES) lists the
material derived from other works.
