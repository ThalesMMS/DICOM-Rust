#![forbid(unsafe_code)]
#![allow(clippy::neg_cmp_op_on_partial_ord)]
mod plan;
use crate::dicomdir::pathrules;
use crate::nifti::gzip::{Container, DeflateWriter, zlib_stream};
use crate::nifti::label_table::{fields, trim_space};
use crate::nifti::{self, ByteStream, NewVolumeOptions, ReadLimits, Terminal, Volume};
use crate::numtext;
use crate::object::error::quote_bytes;
use crate::render::{GeometryAffine, Vec3};
use crate::textfmt::{decode_rune, runes};
pub use plan::{PlanWriteError, write_plan};
use std::collections::BTreeMap;
use std::fmt;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sentinel {
    NotMetaImage,
    MalformedHeader,
    Unsupported,
    LimitExceeded,
    TruncatedData,
}
impl Sentinel {
    pub fn text(self) -> &'static str {
        match self {
            Sentinel::NotMetaImage => "metaimage: not a MetaImage header",
            Sentinel::MalformedHeader => "metaimage: malformed header",
            Sentinel::Unsupported => "metaimage: unsupported feature",
            Sentinel::LimitExceeded => "metaimage: limit exceeded",
            Sentinel::TruncatedData => "metaimage: truncated data",
        }
    }
}
#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    Meta {
        sentinel: Option<Sentinel>,
        message: String,
    },
    Volume(nifti::Error),
    Io(std::io::ErrorKind, String),
}
impl Error {
    pub fn is(&self, sentinel: Sentinel) -> bool {
        matches!(self, Error::Meta { sentinel: Some(own), .. } if *own == sentinel)
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Meta { message, .. } => f.write_str(message),
            Error::Volume(err) => err.fmt(f),
            Error::Io(_, text) => f.write_str(text),
        }
    }
}
impl std::error::Error for Error {}
fn bare(sentinel: Sentinel) -> Error {
    Error::Meta {
        sentinel: Some(sentinel),
        message: sentinel.text().to_string(),
    }
}
fn wrap(sentinel: Sentinel, detail: impl AsRef<str>) -> Error {
    Error::Meta {
        sentinel: Some(sentinel),
        message: format!("{}: {}", sentinel.text(), detail.as_ref()),
    }
}
pub const MAX_HEADER_BYTES: usize = 1 << 20;
pub const WARNING_ORIENTATION_IGNORED: &str = "anatomical-orientation-ignored";
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Header {
    pub fields: BTreeMap<String, Vec<u8>>,
    pub keys: Vec<Vec<u8>>,
    pub ndims: i64,
    pub dim_size: Vec<i64>,
    pub element_type: String,
    pub spacing: Vec<f64>,
    pub offset: Vec<f64>,
    pub direction: Vec<f64>,
    pub big_endian: bool,
    pub compressed: bool,
    pub compressed_size: i64,
    pub header_size: i64,
    pub data_file: Vec<u8>,
}
fn datatype(element_type: &str) -> Option<(i16, u64)> {
    Some(match element_type {
        "MET_CHAR" => (nifti::DATATYPE_INT8, 1),
        "MET_UCHAR" => (nifti::DATATYPE_UINT8, 1),
        "MET_SHORT" => (nifti::DATATYPE_INT16, 2),
        "MET_USHORT" => (nifti::DATATYPE_UINT16, 2),
        "MET_INT" => (nifti::DATATYPE_INT32, 4),
        "MET_UINT" => (nifti::DATATYPE_UINT32, 4),
        "MET_FLOAT" => (nifti::DATATYPE_FLOAT32, 4),
        "MET_DOUBLE" => (nifti::DATATYPE_FLOAT64, 8),
        _ => return None,
    })
}
fn element_type_by_datatype(datatype: i16) -> Option<&'static str> {
    Some(match datatype {
        nifti::DATATYPE_INT8 => "MET_CHAR",
        nifti::DATATYPE_UINT8 => "MET_UCHAR",
        nifti::DATATYPE_INT16 => "MET_SHORT",
        nifti::DATATYPE_UINT16 => "MET_USHORT",
        nifti::DATATYPE_INT32 => "MET_INT",
        nifti::DATATYPE_UINT32 => "MET_UINT",
        nifti::DATATYPE_FLOAT32 => "MET_FLOAT",
        nifti::DATATYPE_FLOAT64 => "MET_DOUBLE",
        _ => return None,
    })
}
pub(crate) fn lossy_text(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(text) => text.to_string(),
        Err(_) => runes(bytes).into_iter().collect(),
    }
}
fn simple_case(c: char, upper: bool) -> char {
    if !upper && c == '\u{130}' {
        return 'i';
    }
    let (first, second) = if upper {
        let mut it = c.to_uppercase();
        (it.next(), it.next())
    } else {
        let mut it = c.to_lowercase();
        (it.next(), it.next())
    };
    match (first, second) {
        (Some(mapped), None) => mapped,
        _ => c,
    }
}
fn map_case(s: &[u8], upper: bool) -> String {
    let mut out = String::with_capacity(s.len());
    let mut at = 0;
    while at < s.len() {
        let (rune, size) = decode_rune(&s[at..]);
        out.push(simple_case(rune.unwrap_or('\u{FFFD}'), upper));
        at += size.max(1);
    }
    out
}
pub(crate) fn to_lower(s: &[u8]) -> String {
    map_case(s, false)
}
pub(crate) fn to_upper(s: &[u8]) -> String {
    map_case(s, true)
}
pub(crate) fn equal_fold(s: &[u8], t: &str) -> bool {
    std::str::from_utf8(s).is_ok_and(|s| pathrules::equal_fold(s, t))
}
fn truncate(s: &[u8]) -> Vec<u8> {
    if s.len() > 40 {
        let mut out = s[..40].to_vec();
        out.extend_from_slice("…".as_bytes());
        return out;
    }
    s.to_vec()
}
fn parse_int(s: &[u8]) -> Option<i64> {
    std::str::from_utf8(s)
        .ok()
        .and_then(|text| numtext::parse_int(text).ok())
}
pub fn parse_header(data: &[u8]) -> Result<(Header, usize), Error> {
    let mut h = Header::default();
    let mut read = 0usize;
    loop {
        let rest = &data[read..];
        let (line, eof) = match rest.iter().position(|&b| b == b'\n') {
            Some(newline) => (&rest[..=newline], false),
            None => (rest, true),
        };
        read += line.len();
        if read > MAX_HEADER_BYTES {
            return Err(wrap(
                Sentinel::LimitExceeded,
                format!("header longer than {MAX_HEADER_BYTES} bytes"),
            ));
        }
        if eof && line.is_empty() {
            break;
        }
        let mut end = line.len();
        while end > 0 && (line[end - 1] == b'\r' || line[end - 1] == b'\n') {
            end -= 1;
        }
        let line = &line[..end];
        if trim_space(line).is_empty() {
            if eof {
                break;
            }
            continue;
        }
        let Some(cut) = line.iter().position(|&b| b == b'=') else {
            return Err(wrap(
                Sentinel::MalformedHeader,
                format!("line {}", quote_bytes(&truncate(line))),
            ));
        };
        let (key, value) = (trim_space(&line[..cut]), trim_space(&line[cut + 1..]));
        let canonical = to_lower(key);
        if h.fields.contains_key(&canonical) {
            return Err(wrap(
                Sentinel::MalformedHeader,
                format!("key {} repeated", quote_bytes(&truncate(key))),
            ));
        }
        let last = canonical == "elementdatafile";
        h.fields.insert(canonical, value.to_vec());
        h.keys.push(key.to_vec());
        if last || eof {
            break;
        }
    }
    if !h.fields.contains_key("elementdatafile") {
        if h.fields.is_empty() {
            return Err(bare(Sentinel::NotMetaImage));
        }
        return Err(wrap(Sentinel::MalformedHeader, "no ElementDataFile"));
    }
    parse_fields(&mut h)?;
    Ok((h, read))
}
fn parse_fields(h: &mut Header) -> Result<(), Error> {
    let fields_map = h.fields.clone();
    let field = |names: &[&str]| -> Option<&Vec<u8>> {
        names
            .iter()
            .find_map(|name| fields_map.get(&name.to_lowercase()))
    };
    let malformed = |detail: String| wrap(Sentinel::MalformedHeader, detail);
    if let Some(object_type) = field(&["ObjectType"])
        && !equal_fold(object_type, "Image")
    {
        return Err(wrap(
            Sentinel::Unsupported,
            format!("ObjectType {}", quote_bytes(&truncate(object_type))),
        ));
    }
    let Some(ndims) = field(&["NDims"]) else {
        return Err(malformed("no NDims".to_string()));
    };
    let n = match parse_int(ndims) {
        Some(n) if n >= 1 => n,
        _ => {
            return Err(malformed(format!(
                "NDims {}",
                quote_bytes(&truncate(ndims))
            )));
        }
    };
    if n > 4 {
        return Err(wrap(Sentinel::Unsupported, format!("NDims {n}")));
    }
    h.ndims = n;
    let n = n as usize;
    let floats = |count: usize, names: &[&str]| -> Result<Option<Vec<f64>>, Error> {
        let Some(value) = field(names) else {
            return Ok(None);
        };
        let parts = fields(value);
        if parts.len() != count {
            return Err(malformed(format!(
                "{} has {} values, want {}",
                names[0],
                parts.len(),
                count
            )));
        }
        let mut out = Vec::with_capacity(count);
        for part in parts {
            let parsed = std::str::from_utf8(part)
                .ok()
                .and_then(|text| numtext::parse_float(text).ok())
                .filter(|v| !v.is_nan() && !v.is_infinite());
            let Some(v) = parsed else {
                return Err(malformed(format!(
                    "{} {}",
                    names[0],
                    quote_bytes(&truncate(value))
                )));
            };
            out.push(v);
        }
        Ok(Some(out))
    };
    {
        let name = "DimSize";
        let Some(value) = field(&[name]) else {
            return Err(malformed(format!("no {name}")));
        };
        let parts = fields(value);
        if parts.len() != n {
            return Err(malformed(format!(
                "{name} has {} values, want {n}",
                parts.len()
            )));
        }
        let mut out = Vec::with_capacity(n);
        for part in parts {
            match parse_int(part) {
                Some(v) if v >= 1 => out.push(v),
                _ => {
                    return Err(malformed(format!(
                        "{name} {}",
                        quote_bytes(&truncate(value))
                    )));
                }
            }
        }
        h.dim_size = out;
    }
    let Some(element_type) = field(&["ElementType"]) else {
        return Err(malformed("no ElementType".to_string()));
    };
    h.element_type = to_upper(element_type);
    if datatype(&h.element_type).is_none() {
        return Err(wrap(
            Sentinel::Unsupported,
            format!("ElementType {}", lossy_text(&truncate(element_type))),
        ));
    }
    if let Some(channels) = field(&["ElementNumberOfChannels"])
        && trim_space(channels) != b"1"
    {
        return Err(wrap(
            Sentinel::Unsupported,
            format!("{} channels", lossy_text(&truncate(channels))),
        ));
    }
    let mut spacing = floats(n, &["ElementSpacing"])?;
    if spacing.is_none() {
        spacing = floats(n, &["ElementSize"])?;
    }
    let spacing = spacing.unwrap_or_else(|| vec![1.0; n]);
    if spacing.iter().any(|&s| s <= 0.0) {
        return Err(malformed("ElementSpacing must be positive".to_string()));
    }
    h.spacing = spacing;
    h.offset = floats(n, &["Offset", "Origin", "Position"])?.unwrap_or_else(|| vec![0.0; n]);
    h.direction = match floats(n * n, &["TransformMatrix", "Rotation", "Orientation"])? {
        Some(direction) => direction,
        None => {
            let mut direction = vec![0.0; n * n];
            for i in 0..n {
                direction[i * n + i] = 1.0;
            }
            direction
        }
    };
    let boolean = |names: &[&str]| -> Result<bool, Error> {
        let Some(value) = field(names) else {
            return Ok(false);
        };
        match to_lower(value).as_str() {
            "true" | "1" => Ok(true),
            "false" | "0" => Ok(false),
            _ => Err(malformed(format!(
                "{} {}",
                names[0],
                quote_bytes(&truncate(value))
            ))),
        }
    };
    h.big_endian = boolean(&["BinaryDataByteOrderMSB", "ElementByteOrderMSB"])?;
    h.compressed = boolean(&["CompressedData"])?;
    if let Some(binary) = field(&["BinaryData"])
        && equal_fold(binary, "False")
    {
        return Err(wrap(Sentinel::Unsupported, "ASCII element data"));
    }
    if let Some(value) = field(&["CompressedDataSize"]) {
        match parse_int(value) {
            Some(size) if size >= 0 => h.compressed_size = size,
            _ => return Err(malformed("CompressedDataSize".to_string())),
        }
    }
    if let Some(value) = field(&["HeaderSize"]) {
        match parse_int(value) {
            Some(size) if size >= -1 => h.header_size = size,
            _ => return Err(malformed("HeaderSize".to_string())),
        }
    }
    h.data_file = field(&["ElementDataFile"]).cloned().unwrap_or_default();
    Ok(())
}
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ReadOptions {
    pub limits: ReadLimits,
}
#[derive(Debug, Clone, PartialEq)]
pub struct Image {
    pub header: Header,
    pub volume: Volume,
    pub warnings: Vec<String>,
}
pub fn read(data: &[u8], options: &ReadOptions) -> Result<Image, Error> {
    let (header, consumed) = parse_header(data)?;
    if !equal_fold(&header.data_file, "LOCAL") {
        return Err(wrap(
            Sentinel::Unsupported,
            "separate data file; use ReadFile",
        ));
    }
    decode(header, ByteStream::plain(&data[consumed..]), None, options)
}
fn io_error(err: &std::io::Error) -> Error {
    Error::Io(err.kind(), err.to_string())
}
pub fn read_file(path: impl AsRef<std::path::Path>, options: &ReadOptions) -> Result<Image, Error> {
    let path = path.as_ref().to_string_lossy();
    let path: &str = &path;
    let data = std::fs::read(path).map_err(|err| io_error(&err))?;
    let (header, consumed) = parse_header(&data)?;
    if equal_fold(&header.data_file, "LOCAL") {
        return decode(header, ByteStream::plain(&data[consumed..]), None, options);
    }
    let name = &header.data_file;
    if to_upper(name).starts_with("LIST") || name.contains(&b'%') || name.contains(&b' ') {
        return Err(wrap(Sentinel::Unsupported, "data file lists and patterns"));
    }
    let outside = || {
        wrap(
            Sentinel::MalformedHeader,
            format!(
                "data file {} outside the header directory",
                quote_bytes(&truncate(name))
            ),
        )
    };
    let Ok(name_text) = std::str::from_utf8(name) else {
        return Err(outside());
    };
    let native = if cfg!(windows) {
        name_text.replace('/', "\\")
    } else {
        name_text.to_string()
    };
    let clean = pathrules::clean(&native);
    let parent_prefix = format!("..{}", pathrules::sep_str());
    if pathrules::is_abs(&clean)
        || clean == ".."
        || clean.starts_with(&parent_prefix)
        || !pathrules::volume_name(&clean).is_empty()
    {
        return Err(outside());
    }
    let data_path = pathrules::join(&[&pathrules::dir(path), &clean]);
    let raw = std::fs::read(&data_path).map_err(|err| io_error(&err))?;
    let size = raw.len() as i64;
    decode(header, ByteStream::plain(&raw), Some(size), options)
}
fn decode(
    h: Header,
    mut data: ByteStream,
    file_size: Option<i64>,
    options: &ReadOptions,
) -> Result<Image, Error> {
    let mut limits = options.limits;
    let defaults = nifti::default_read_limits();
    if limits.max_voxels == 0 {
        limits.max_voxels = defaults.max_voxels;
    }
    if limits.max_payload_bytes == 0 {
        limits.max_payload_bytes = defaults.max_payload_bytes;
    }
    let mut voxels = 1u64;
    for &size in &h.dim_size {
        if voxels > limits.max_voxels / size as u64 {
            return Err(wrap(
                Sentinel::LimitExceeded,
                format!("voxel count exceeds {}", limits.max_voxels),
            ));
        }
        voxels *= size as u64;
    }
    let (volume_datatype, width) = datatype(&h.element_type).expect("validated element type");
    let byte_limit = limits.max_payload_bytes.min(i64::MAX as u64);
    if voxels > byte_limit / width {
        return Err(wrap(
            Sentinel::LimitExceeded,
            format!("payload exceeds {byte_limit} bytes"),
        ));
    }
    let payload_bytes = (voxels * width) as i64;
    let mut skip = h.header_size;
    if skip == -1 {
        let Some(file_size) = file_size.filter(|_| !h.compressed) else {
            return Err(wrap(
                Sentinel::Unsupported,
                "HeaderSize -1 needs uncompressed data in a separate file",
            ));
        };
        skip = file_size - payload_bytes;
        if skip < 0 {
            return Err(bare(Sentinel::TruncatedData));
        }
    }
    if skip > 0 && data.discard(skip as u64).is_err() {
        return Err(bare(Sentinel::TruncatedData));
    }
    let mut stream = data;
    if h.compressed {
        let source = if h.compressed_size > 0 {
            stream.take_limited(h.compressed_size as u64)
        } else {
            stream
        };
        stream = zlib_stream(source.rest()).map_err(|terminal| {
            wrap(
                Sentinel::MalformedHeader,
                format!("zlib: {}", terminal.text()),
            )
        })?;
    }
    let n = h.ndims as usize;
    let spatial = n.min(3);
    let mut dims = [1i64; 4];
    dims[..n].copy_from_slice(&h.dim_size);
    let mut columns = [Vec3::ZERO; 3];
    for axis in 0..spatial {
        let mut d = [0.0f64; 3];
        for (j, slot) in d.iter_mut().enumerate().take(spatial) {
            *slot = h.direction[axis * n + j] * h.spacing[axis];
        }
        columns[axis] = Vec3::new(d[0], d[1], d[2]);
    }
    match spatial {
        1 => {
            columns[1] = Vec3::new(0.0, 1.0, 0.0);
            columns[2] = Vec3::new(0.0, 0.0, 1.0);
        }
        2 => columns[2] = columns[0].cross(columns[1]).normalize(),
        _ => {}
    }
    let mut o = [0.0f64; 3];
    o[..spatial].copy_from_slice(&h.offset[..spatial]);
    let mut affine = GeometryAffine::default();
    for column in 0..3 {
        affine.0[column] = columns[column].x;
        affine.0[4 + column] = columns[column].y;
        affine.0[8 + column] = columns[column].z;
    }
    (affine.0[3], affine.0[7], affine.0[11], affine.0[15]) = (o[0], o[1], o[2], 1.0);
    let volume = nifti::new_volume_exact(
        &mut stream,
        &NewVolumeOptions {
            datatype: volume_datatype,
            dimensions: dims,
            index_to_lps: affine,
            big_endian: h.big_endian,
            limits,
        },
    )
    .map_err(Error::Volume)?;
    if h.compressed {
        match stream.read_full(1) {
            Ok(_) => {
                return Err(wrap(
                    Sentinel::MalformedHeader,
                    "zlib payload exceeds declared dimensions",
                ));
            }
            Err(Terminal::Eof) => {}
            Err(Terminal::UnexpectedEof) => {
                return Err(wrap(
                    Sentinel::TruncatedData,
                    "zlib payload: unexpected EOF",
                ));
            }
            Err(Terminal::Other(text)) => {
                return Err(Error::Meta {
                    sentinel: None,
                    message: format!("metaimage: zlib payload: {text}"),
                });
            }
        }
    }
    let mut warnings = volume.warnings.clone();
    if n == 3
        && let Some(orientation) = h.fields.get("anatomicalorientation")
    {
        let want = anatomical_orientation(&columns);
        if !equal_fold(orientation, &want) && !equal_fold(orientation, "???") {
            warnings.push(WARNING_ORIENTATION_IGNORED.to_string());
        }
    }
    Ok(Image {
        header: h,
        volume,
        warnings,
    })
}
fn anatomical_orientation(columns: &[Vec3; 3]) -> String {
    let mut out = String::new();
    for c in columns {
        let v = [c.x, c.y, c.z];
        let mut best = 0;
        for i in 1..3 {
            if v[i].abs() > v[best].abs() {
                best = i;
            }
        }
        let from = [['R', 'L'], ['A', 'P'], ['I', 'S']][best];
        out.push(if v[best] >= 0.0 { from[0] } else { from[1] });
    }
    out
}
const RESERVED_FIELDS: [&str; 22] = [
    "objecttype",
    "ndims",
    "binarydata",
    "binarydatabyteordermsb",
    "elementbyteordermsb",
    "compresseddata",
    "compresseddatasize",
    "transformmatrix",
    "rotation",
    "orientation",
    "offset",
    "origin",
    "position",
    "centerofrotation",
    "anatomicalorientation",
    "elementspacing",
    "elementsize",
    "dimsize",
    "elementtype",
    "elementdatafile",
    "headersize",
    "elementnumberofchannels",
];
fn format_number(v: f64) -> String {
    if v == 0.0 {
        return "0".to_string();
    }
    numtext::format_float(v, 'g', 17)
}
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WriteOptions {
    pub compress: bool,
    pub fields: BTreeMap<String, String>,
}
struct HeaderSpec<'a> {
    datatype: i16,
    dimensions: [i64; 4],
    index_to_lps: GeometryAffine,
    fourth_spacing: f64,
    compressed_size: i64,
    options: &'a WriteOptions,
}
impl HeaderSpec<'_> {
    fn header(&self) -> Result<String, Error> {
        let Some(element_type) = element_type_by_datatype(self.datatype) else {
            return Err(wrap(
                Sentinel::Unsupported,
                format!("datatype {}", self.datatype),
            ));
        };
        let d = &self.dimensions;
        let n = if d[3] > 1 { 4 } else { 3 };
        let a = &self.index_to_lps.0;
        let mut columns = [Vec3::ZERO; 3];
        let mut spacing_values = [0.0f64; 3];
        for axis in 0..3 {
            if d[axis] < 1 {
                return Err(wrap(
                    Sentinel::MalformedHeader,
                    format!("size {} of axis {}", d[axis], axis),
                ));
            }
            columns[axis] = Vec3::new(a[axis], a[4 + axis], a[8 + axis]);
            spacing_values[axis] = columns[axis].length();
            if !(spacing_values[axis] > 0.0) || spacing_values[axis].is_infinite() {
                return Err(wrap(
                    Sentinel::MalformedHeader,
                    format!("degenerate direction of axis {axis}"),
                ));
            }
        }
        let mut direction = Vec::new();
        for axis in 0..n {
            for j in 0..n {
                let value = if axis < 3 && j < 3 {
                    a[4 * j + axis] / spacing_values[axis]
                } else if axis == j {
                    1.0
                } else {
                    0.0
                };
                direction.push(format_number(value));
            }
        }
        let mut sizes: Vec<String> = d[..3].iter().map(|v| v.to_string()).collect();
        let mut spacing: Vec<String> = spacing_values.iter().map(|&v| format_number(v)).collect();
        let mut offset = vec![
            format_number(a[3]),
            format_number(a[7]),
            format_number(a[11]),
        ];
        if n == 4 {
            let mut fourth = self.fourth_spacing;
            if !(fourth > 0.0) || fourth.is_infinite() {
                fourth = 1.0;
            }
            sizes.push(d[3].to_string());
            spacing.push(format_number(fourth));
            offset.push("0".to_string());
        }
        let mut b = String::new();
        let mut line = |key: &str, value: &str| {
            b.push_str(key);
            b.push_str(" = ");
            b.push_str(value);
            b.push('\n');
        };
        line("ObjectType", "Image");
        line("NDims", &n.to_string());
        line("BinaryData", "True");
        line("BinaryDataByteOrderMSB", "False");
        line(
            "CompressedData",
            if self.options.compress {
                "True"
            } else {
                "False"
            },
        );
        if self.options.compress {
            line("CompressedDataSize", &self.compressed_size.to_string());
        }
        line("TransformMatrix", &direction.join(" "));
        line("Offset", &offset.join(" "));
        line("CenterOfRotation", vec!["0"; n].join(" ").trim());
        if n == 3 {
            line("AnatomicalOrientation", &anatomical_orientation(&columns));
        }
        line("ElementSpacing", &spacing.join(" "));
        line("DimSize", &sizes.join(" "));
        for (key, value) in &self.options.fields {
            if key.is_empty()
                || key.contains(['=', '\r', '\n'])
                || value.contains(['\r', '\n'])
                || RESERVED_FIELDS.contains(&to_lower(key.as_bytes()).as_str())
            {
                return Err(wrap(
                    Sentinel::MalformedHeader,
                    format!("field {}", quote_bytes(&truncate(key.as_bytes()))),
                ));
            }
        }
        for (key, value) in &self.options.fields {
            line(key, value);
        }
        line("ElementType", element_type);
        line("ElementDataFile", "LOCAL");
        Ok(b)
    }
}
pub fn write(v: &Volume, options: &WriteOptions) -> Result<Vec<u8>, Error> {
    if v.scaling.applied {
        return Err(wrap(Sentinel::Unsupported, "value scaling"));
    }
    let mut spec = HeaderSpec {
        datatype: v.header.datatype,
        dimensions: v.dimensions,
        index_to_lps: v.geometry.index_to_lps,
        fourth_spacing: 0.0,
        compressed_size: 0,
        options,
    };
    spec.header()?;
    let mut payload = Vec::new();
    if options.compress {
        let mut compressor = DeflateWriter::new(Container::Zlib);
        for chunk in v.payload().chunks(nifti::PAYLOAD_CHUNK_BYTES) {
            compressor.write(chunk);
        }
        payload = compressor.finish();
        spec.compressed_size = payload.len() as i64;
    }
    let header = spec.header()?;
    let mut out = header.into_bytes();
    if options.compress {
        out.extend_from_slice(&payload);
    } else {
        out.extend_from_slice(v.payload());
    }
    Ok(out)
}
pub fn lower_text(s: &str) -> String {
    to_lower(s.as_bytes())
}
