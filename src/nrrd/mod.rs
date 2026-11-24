#![forbid(unsafe_code)]
#![allow(clippy::needless_range_loop)]
pub mod read;
pub mod slicer;
pub mod write;
use crate::dicomdir::pathrules;
use crate::metaimage::{lossy_text, to_lower};
use crate::nifti;
use crate::nifti::label_table::{fields, trim_space};
use crate::numtext;
use crate::object::error::quote_bytes;
pub use read::{Image, ReadOptions, read, read_file};
pub use slicer::{
    CodedEntry, SegDocumentError, SegmentDef, SlicerSegment, SlicerSegmentMask, new_slicer_segment,
    pack_slicer_segmentation, write_slicer_segmentation,
};
use std::collections::BTreeMap;
use std::fmt;
pub use write::{PlanWriteError, WriteOptions, write, write_plan};
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sentinel {
    NotNrrd,
    MalformedHeader,
    Unsupported,
    LimitExceeded,
    NoPatientSpace,
    TruncatedData,
}
impl Sentinel {
    pub fn text(self) -> &'static str {
        match self {
            Sentinel::NotNrrd => "nrrd: not an NRRD file",
            Sentinel::MalformedHeader => "nrrd: malformed header",
            Sentinel::Unsupported => "nrrd: unsupported feature",
            Sentinel::LimitExceeded => "nrrd: limit exceeded",
            Sentinel::NoPatientSpace => "nrrd: no patient coordinate system",
            Sentinel::TruncatedData => "nrrd: truncated data",
        }
    }
}
#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    Nrrd {
        sentinel: Option<Sentinel>,
        message: String,
    },
    Volume(nifti::Error),
    Io(std::io::ErrorKind, String),
}
impl Error {
    pub fn is(&self, sentinel: Sentinel) -> bool {
        match self {
            Error::Nrrd {
                sentinel: Some(own),
                ..
            } => *own == sentinel,
            Error::Volume(nifti::Error::Read(read)) => {
                sentinel == Sentinel::MalformedHeader
                    && read.cause.as_deref() == Some(read::HEX_DATA_ERROR)
            }
            _ => false,
        }
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Nrrd { message, .. } => f.write_str(message),
            Error::Volume(err) => err.fmt(f),
            Error::Io(_, text) => f.write_str(text),
        }
    }
}
impl std::error::Error for Error {}
pub(crate) fn bare(sentinel: Sentinel) -> Error {
    Error::Nrrd {
        sentinel: Some(sentinel),
        message: sentinel.text().to_string(),
    }
}
pub(crate) fn wrap(sentinel: Sentinel, detail: impl AsRef<str>) -> Error {
    Error::Nrrd {
        sentinel: Some(sentinel),
        message: format!("{}: {}", sentinel.text(), detail.as_ref()),
    }
}
fn malformed(detail: impl AsRef<str>) -> Error {
    wrap(Sentinel::MalformedHeader, detail)
}
pub const MAX_HEADER_BYTES: usize = 1 << 20;
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Header {
    pub version: i64,
    pub fields: BTreeMap<String, Vec<u8>>,
    pub key_values: BTreeMap<Vec<u8>, Vec<u8>>,
    key_order: Vec<Vec<u8>>,
    pub typ: String,
    pub dimension: i64,
    pub sizes: Vec<i64>,
    pub endian: String,
    pub encoding: String,
    pub space: String,
    pub space_dim: i64,
    pub directions: Option<Vec<Option<Vec<f64>>>>,
    pub origin: Option<Vec<f64>>,
    pub spacings: Vec<f64>,
    pub kinds: Vec<String>,
    pub space_units: Vec<Vec<u8>>,
    pub data_file: Vec<u8>,
    pub line_skip: i64,
    pub byte_skip: i64,
}
fn field_alias(name: &str) -> Option<&'static str> {
    Some(match name {
        "datafile" => "data file",
        "lineskip" => "line skip",
        "byteskip" => "byte skip",
        "centerings" => "centers",
        "space dimension" | "spacedimension" => "space dimension",
        "sampleunits" => "sample units",
        "oldmin" => "old min",
        "oldmax" => "old max",
        _ => return None,
    })
}
fn type_name(name: &str) -> Option<&'static str> {
    Some(match name {
        "signed char" | "int8" | "int8_t" => "int8",
        "uchar" | "unsigned char" | "uint8" | "uint8_t" => "uint8",
        "short" | "short int" | "signed short" | "signed short int" | "int16" | "int16_t" => {
            "int16"
        }
        "ushort" | "unsigned short" | "unsigned short int" | "uint16" | "uint16_t" => "uint16",
        "int" | "signed int" | "int32" | "int32_t" => "int32",
        "uint" | "unsigned int" | "uint32" | "uint32_t" => "uint32",
        "longlong"
        | "long long"
        | "long long int"
        | "signed long long"
        | "signed long long int"
        | "int64"
        | "int64_t" => "int64",
        "ulonglong" | "unsigned long long" | "unsigned long long int" | "uint64" | "uint64_t" => {
            "uint64"
        }
        "float" => "float",
        "double" => "double",
        "block" => "block",
        _ => return None,
    })
}
fn encoding_name(name: &str) -> Option<&'static str> {
    Some(match name {
        "raw" => "raw",
        "gzip" | "gz" => "gzip",
        "bzip2" | "bz2" => "bzip2",
        "txt" | "text" | "ascii" => "text",
        "hex" => "hex",
        _ => return None,
    })
}
pub(crate) fn space_axes(space: &str) -> Option<[f64; 3]> {
    Some(match space {
        "left-posterior-superior" => [1.0, 1.0, 1.0],
        "right-anterior-superior" => [-1.0, -1.0, 1.0],
        "left-anterior-superior" => [1.0, -1.0, 1.0],
        "right-posterior-superior" => [-1.0, 1.0, 1.0],
        _ => return None,
    })
}
fn space_alias(name: &str) -> Option<&'static str> {
    Some(match name {
        "lps" => "left-posterior-superior",
        "ras" => "right-anterior-superior",
        "las" => "left-anterior-superior",
        "rps" => "right-posterior-superior",
        "lpst" => "left-posterior-superior-time",
        "rast" => "right-anterior-superior-time",
        "last" => "left-anterior-superior-time",
        _ => return None,
    })
}
pub(crate) fn truncate(s: &[u8]) -> Vec<u8> {
    if s.len() > 40 {
        let mut out = s[..40].to_vec();
        out.extend_from_slice("…".as_bytes());
        return out;
    }
    s.to_vec()
}
fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}
fn unescape_key_value(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        if s[i] == b'\\' && i + 1 < s.len() && (s[i + 1] == b'n' || s[i + 1] == b'\\') {
            out.push(if s[i + 1] == b'n' { b'\n' } else { b'\\' });
            i += 2;
        } else {
            out.push(s[i]);
            i += 1;
        }
    }
    out
}
pub(crate) fn escape_key_value(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c => out.push(c),
        }
    }
    out
}
fn normalize_name(s: &[u8]) -> String {
    let lower = to_lower(s);
    lower.split_whitespace().collect::<Vec<_>>().join(" ")
}
fn atoi(s: &[u8]) -> Option<i64> {
    std::str::from_utf8(s)
        .ok()
        .and_then(|text| numtext::parse_int(text).ok())
}
fn parse_float(s: &[u8]) -> Option<f64> {
    std::str::from_utf8(s)
        .ok()
        .and_then(|text| numtext::parse_float(text).ok())
}
fn equal_fold(s: &[u8], t: &str) -> bool {
    std::str::from_utf8(s).is_ok_and(|s| pathrules::equal_fold(s, t))
}
pub fn parse_header(data: &[u8]) -> Result<(Header, usize), Error> {
    let mut header = Header::default();
    let mut read = 0usize;
    let next_line = |read: &mut usize| -> Result<(&[u8], bool), Error> {
        let rest = &data[*read..];
        let (line, eof) = match rest.iter().position(|&b| b == b'\n') {
            Some(newline) => (&rest[..=newline], false),
            None => (rest, true),
        };
        *read += line.len();
        if *read > MAX_HEADER_BYTES {
            return Err(wrap(
                Sentinel::LimitExceeded,
                format!("header longer than {MAX_HEADER_BYTES} bytes"),
            ));
        }
        let mut end = line.len();
        while end > 0 && (line[end - 1] == b'\r' || line[end - 1] == b'\n') {
            end -= 1;
        }
        Ok((&line[..end], eof && line.is_empty()))
    };
    let (magic, eof) = next_line(&mut read)?;
    if eof
        || !magic.starts_with(b"NRRD000")
        || magic.len() != 8
        || !(b'1'..=b'5').contains(&magic[7])
    {
        return Err(bare(Sentinel::NotNrrd));
    }
    header.version = (magic[7] - b'0') as i64;
    loop {
        let (line, eof) = next_line(&mut read)?;
        if eof || line.is_empty() {
            break;
        }
        if line[0] == b'#' {
            continue;
        }
        if let Some(cut) = find(line, b":=") {
            let key = unescape_key_value(&line[..cut]);
            let value = unescape_key_value(&line[cut + 2..]);
            if !header.key_values.contains_key(&key) {
                header.key_order.push(key.clone());
            }
            header.key_values.insert(key, value);
            continue;
        }
        let Some(cut) = find(line, b": ") else {
            return Err(malformed(format!("line {}", quote_bytes(&truncate(line)))));
        };
        let mut name = normalize_name(&line[..cut]);
        if let Some(alias) = field_alias(&name) {
            name = alias.to_string();
        }
        if header.fields.contains_key(&name) {
            return Err(malformed(format!(
                "field {} repeated",
                crate::textfmt::quote(&name)
            )));
        }
        header
            .fields
            .insert(name, trim_space(&line[cut + 2..]).to_vec());
    }
    parse_fields(&mut header)?;
    Ok((header, read))
}
fn parse_fields(h: &mut Header) -> Result<(), Error> {
    let fields_map = h.fields.clone();
    let get = |name: &str| fields_map.get(name);
    let quoted = |text: &[u8]| quote_bytes(&truncate(text));
    let Some(type_text) = get("type") else {
        return Err(malformed("no type"));
    };
    match type_name(&normalize_name(type_text)) {
        Some(name) => h.typ = name.to_string(),
        None => return Err(malformed(format!("type {}", quoted(type_text)))),
    }
    let Some(dimension) = get("dimension") else {
        return Err(malformed("no dimension"));
    };
    let d = match atoi(dimension) {
        Some(d) if (1..=16).contains(&d) => d,
        _ => return Err(malformed(format!("dimension {}", quoted(dimension)))),
    };
    h.dimension = d;
    let d = d as usize;
    let Some(sizes) = get("sizes") else {
        return Err(malformed("no sizes"));
    };
    for field in fields(sizes) {
        match atoi(field) {
            Some(size) if size >= 1 => h.sizes.push(size),
            _ => return Err(malformed(format!("sizes {}", quoted(sizes)))),
        }
    }
    if h.sizes.len() != d {
        return Err(malformed(format!(
            "{} sizes for dimension {}",
            h.sizes.len(),
            d
        )));
    }
    let Some(encoding) = get("encoding") else {
        return Err(malformed("no encoding"));
    };
    match encoding_name(&to_lower(encoding)) {
        Some(name) => h.encoding = name.to_string(),
        None => {
            return Err(wrap(
                Sentinel::Unsupported,
                format!("encoding {}", quoted(encoding)),
            ));
        }
    }
    if let Some(endian) = get("endian") {
        h.endian = to_lower(endian);
        if h.endian != "little" && h.endian != "big" {
            return Err(malformed(format!("endian {}", quoted(endian))));
        }
    }
    if let Some(space) = get("space") {
        let mut name = to_lower(space);
        if let Some(alias) = space_alias(&name) {
            name = alias.to_string();
        }
        h.space_dim = if name.ends_with("-time") {
            4
        } else if matches!(
            name.as_str(),
            "scanner-xyz" | "3d-right-handed" | "3d-left-handed"
        ) {
            3
        } else if matches!(
            name.as_str(),
            "3d-right-handed-time" | "3d-left-handed-time" | "scanner-xyz-time"
        ) {
            4
        } else {
            if space_axes(&name).is_none() {
                return Err(malformed(format!("space {}", quoted(space))));
            }
            3
        };
        h.space = name;
    }
    if let Some(space_dim) = get("space dimension") {
        match atoi(space_dim) {
            Some(value)
                if (1..=16).contains(&value) && (h.space_dim == 0 || h.space_dim == value) =>
            {
                h.space_dim = value;
            }
            _ => {
                return Err(malformed(format!("space dimension {}", quoted(space_dim))));
            }
        }
    }
    if let Some(directions) = get("space directions") {
        if h.space_dim == 0 {
            return Err(malformed("space directions without a space"));
        }
        let parts = fields(directions);
        if parts.len() != d {
            return Err(malformed(format!(
                "{} space directions for dimension {}",
                parts.len(),
                d
            )));
        }
        let mut out = Vec::with_capacity(d);
        for part in parts {
            if equal_fold(part, "none") {
                out.push(None);
                continue;
            }
            out.push(Some(parse_vector(part, h.space_dim as usize)?));
        }
        h.directions = Some(out);
    }
    if let Some(origin) = get("space origin") {
        if h.space_dim == 0 {
            return Err(malformed("space origin without a space"));
        }
        h.origin = Some(parse_vector(trim_space(origin), h.space_dim as usize)?);
    }
    h.spacings = vec![f64::NAN; d];
    if let Some(spacings) = get("spacings") {
        let parts = fields(spacings);
        if parts.len() != d {
            return Err(malformed("spacings"));
        }
        for (i, part) in parts.into_iter().enumerate() {
            if equal_fold(part, "nan") {
                continue;
            }
            match parse_float(part) {
                Some(value) if !value.is_infinite() && value != 0.0 => h.spacings[i] = value,
                _ => return Err(malformed(format!("spacings {}", quoted(spacings)))),
            }
        }
    }
    h.kinds = vec![String::new(); d];
    if let Some(kinds) = get("kinds") {
        let parts = fields(kinds);
        if parts.len() != d {
            return Err(malformed("kinds"));
        }
        for (i, part) in parts.into_iter().enumerate() {
            h.kinds[i] = to_lower(part);
        }
    }
    if let Some(units) = get("space units") {
        h.space_units = quoted_strings(units);
    }
    h.data_file = get("data file").cloned().unwrap_or_default();
    if let Some(value) = get("line skip") {
        match atoi(value) {
            Some(skip) if skip >= 0 => h.line_skip = skip,
            _ => return Err(malformed("line skip")),
        }
    }
    if let Some(value) = get("byte skip") {
        match atoi(value) {
            Some(skip) if skip >= -1 => h.byte_skip = skip,
            _ => return Err(malformed("byte skip")),
        }
    }
    Ok(())
}
fn parse_vector(text: &[u8], n: usize) -> Result<Vec<f64>, Error> {
    let bad = || malformed(format!("vector {}", quote_bytes(&truncate(text))));
    if text.len() < 2 || text[0] != b'(' || text[text.len() - 1] != b')' {
        return Err(bad());
    }
    let parts: Vec<&[u8]> = text[1..text.len() - 1].split(|&b| b == b',').collect();
    if parts.len() != n {
        return Err(malformed(format!(
            "vector {} has {} components, want {}",
            quote_bytes(&truncate(text)),
            parts.len(),
            n
        )));
    }
    let mut vector = Vec::with_capacity(n);
    for part in parts {
        match parse_float(trim_space(part)) {
            Some(value) if !value.is_nan() && !value.is_infinite() => vector.push(value),
            _ => return Err(bad()),
        }
    }
    Ok(vector)
}
fn quoted_strings(mut text: &[u8]) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    loop {
        let Some(start) = text.iter().position(|&b| b == b'"') else {
            return out;
        };
        let Some(end) = text[start + 1..].iter().position(|&b| b == b'"') else {
            return out;
        };
        out.push(text[start + 1..start + 1 + end].to_vec());
        text = &text[start + end + 2..];
    }
}
impl Header {
    pub fn key_value_keys(&self) -> Vec<Vec<u8>> {
        let mut keys = self.key_order.clone();
        for key in self.key_values.keys() {
            if !self.key_order.contains(key) {
                keys.push(key.clone());
            }
        }
        keys
    }
    pub fn key_value(&self, key: &str) -> Option<String> {
        self.key_values.get(key.as_bytes()).map(|v| lossy_text(v))
    }
    pub(crate) fn bytes_per_sample(&self) -> usize {
        match self.typ.as_str() {
            "int8" | "uint8" => 1,
            "int16" | "uint16" => 2,
            "int32" | "uint32" | "float" => 4,
            "int64" | "uint64" | "double" => 8,
            _ => 0,
        }
    }
}
