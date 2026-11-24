use super::{Error, Header, Sentinel, bare, parse_header, space_axes, truncate, wrap};
use crate::dicomdir::pathrules;
use crate::metaimage::to_upper;
use crate::nifti::gzip::gunzip;
use crate::nifti::{self, ByteStream, NewVolumeOptions, ReadLimits, Terminal, Volume};
use crate::numtext;
use crate::object::error::quote_bytes;
use crate::render::{GeometryAffine, Vec3};
use crate::textfmt::decode_rune;
pub const WARNING_NON_ANATOMICAL_SPACE: &str = "non-anatomical-space-read-as-lps";
pub const WARNING_MEASUREMENT_FRAME_IGNORED: &str = "measurement-frame-ignored";
pub const WARNING_SPACE_UNITS: &str = "space-units-not-mm";
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ReadOptions {
    pub limits: ReadLimits,
    pub allow_non_anatomical_space: bool,
}
#[derive(Debug, Clone, PartialEq)]
pub struct Image {
    pub header: Header,
    pub volume: Volume,
    pub non_spatial_axis: i64,
    pub warnings: Vec<String>,
}
pub fn read(data: &[u8], options: &ReadOptions) -> Result<Image, Error> {
    let (header, consumed) = parse_header(data)?;
    if !header.data_file.is_empty() {
        return Err(wrap(
            Sentinel::Unsupported,
            "detached data file; use ReadFile",
        ));
    }
    decode(header, &data[consumed..], None, options)
}
fn io_error(err: &std::io::Error) -> Error {
    Error::Io(err.kind(), err.to_string())
}
pub fn read_file(path: impl AsRef<std::path::Path>, options: &ReadOptions) -> Result<Image, Error> {
    let path = path.as_ref().to_string_lossy();
    let path: &str = &path;
    let data = std::fs::read(path).map_err(|err| io_error(&err))?;
    let (header, consumed) = parse_header(&data)?;
    if header.data_file.is_empty() {
        return decode(header, &data[consumed..], None, options);
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
    decode(header, &raw, Some(size), options)
}
fn datatype(typ: &str) -> Option<i16> {
    Some(match typ {
        "int8" => nifti::DATATYPE_INT8,
        "uint8" => nifti::DATATYPE_UINT8,
        "int16" => nifti::DATATYPE_INT16,
        "uint16" => nifti::DATATYPE_UINT16,
        "int32" => nifti::DATATYPE_INT32,
        "uint32" => nifti::DATATYPE_UINT32,
        "float" => nifti::DATATYPE_FLOAT32,
        "double" => nifti::DATATYPE_FLOAT64,
        _ => return None,
    })
}
struct Layout {
    spatial: Vec<usize>,
    non_spatial: i64,
    dims: [i64; 4],
    index_to_lps: GeometryAffine,
    warnings: Vec<String>,
}
fn plan_layout(h: &Header, options: &ReadOptions) -> Result<Layout, Error> {
    let mut l = Layout {
        spatial: Vec::new(),
        non_spatial: -1,
        dims: [0; 4],
        index_to_lps: GeometryAffine::default(),
        warnings: Vec::new(),
    };
    let axes = space_axes(&h.space);
    let anatomical = axes.is_some();
    if !h.space.is_empty() && !anatomical {
        if h.space.ends_with("-time") {
            return Err(wrap(
                Sentinel::Unsupported,
                format!("space {} with a time axis", crate::textfmt::quote(&h.space)),
            ));
        }
        if !options.allow_non_anatomical_space {
            return Err(wrap(
                Sentinel::NoPatientSpace,
                format!("space {}", crate::textfmt::quote(&h.space)),
            ));
        }
    }
    let signs = axes.unwrap_or([1.0, 1.0, 1.0]);
    if h.space_dim != 0 && h.space_dim != 3 {
        return Err(wrap(
            Sentinel::Unsupported,
            format!("space dimension {}", h.space_dim),
        ));
    }
    let more_non_spatial = || wrap(Sentinel::Unsupported, "more than one non-spatial axis");
    let more_spatial = || wrap(Sentinel::Unsupported, "more than three spatial axes");
    let mut columns = [Vec3::ZERO; 3];
    if let Some(directions) = &h.directions {
        for (axis, direction) in directions.iter().enumerate() {
            let Some(direction) = direction else {
                if l.non_spatial >= 0 {
                    return Err(more_non_spatial());
                }
                l.non_spatial = axis as i64;
                continue;
            };
            if l.spatial.len() == 3 {
                return Err(more_spatial());
            }
            columns[l.spatial.len()] = Vec3::new(
                signs[0] * direction[0],
                signs[1] * direction[1],
                signs[2] * direction[2],
            );
            l.spatial.push(axis);
        }
    } else {
        if !h.space.is_empty() || !options.allow_non_anatomical_space {
            return Err(wrap(Sentinel::NoPatientSpace, "no space directions"));
        }
        for axis in 0..h.sizes.len() {
            let kind = h.kinds[axis].as_str();
            if !kind.is_empty() && kind != "domain" && kind != "space" {
                if l.non_spatial >= 0 {
                    return Err(more_non_spatial());
                }
                l.non_spatial = axis as i64;
                continue;
            }
            if l.spatial.len() == 3 {
                return Err(more_spatial());
            }
            let mut spacing = h.spacings[axis];
            if spacing.is_nan() {
                spacing = 1.0;
            }
            columns[l.spatial.len()] = match l.spatial.len() {
                0 => Vec3::new(spacing, 0.0, 0.0),
                1 => Vec3::new(0.0, spacing, 0.0),
                _ => Vec3::new(0.0, 0.0, spacing),
            };
            l.spatial.push(axis);
        }
    }
    if !anatomical {
        l.warnings.push(WARNING_NON_ANATOMICAL_SPACE.to_string());
    }
    match l.spatial.len() {
        2 => {
            let normal = columns[0].cross(columns[1]).normalize();
            if normal.length() == 0.0 {
                return Err(wrap(
                    Sentinel::MalformedHeader,
                    "degenerate space directions",
                ));
            }
            columns[2] = normal;
        }
        3 => {}
        count => {
            return Err(wrap(Sentinel::Unsupported, format!("{count} spatial axes")));
        }
    }
    if l.non_spatial > 0 && l.non_spatial < h.sizes.len() as i64 - 1 {
        return Err(wrap(
            Sentinel::Unsupported,
            "non-spatial axis between spatial axes",
        ));
    }
    for (index, &axis) in l.spatial.iter().enumerate() {
        l.dims[index] = h.sizes[axis];
    }
    if l.spatial.len() == 2 {
        l.dims[2] = 1;
    }
    l.dims[3] = 1;
    if l.non_spatial >= 0 {
        l.dims[3] = h.sizes[l.non_spatial as usize];
    }
    let origin = match &h.origin {
        Some(o) => Vec3::new(signs[0] * o[0], signs[1] * o[1], signs[2] * o[2]),
        None => Vec3::ZERO,
    };
    let a = &mut l.index_to_lps.0;
    for column in 0..3 {
        a[column] = columns[column].x;
        a[4 + column] = columns[column].y;
        a[8 + column] = columns[column].z;
    }
    (a[3], a[7], a[11], a[15]) = (origin.x, origin.y, origin.z, 1.0);
    if h.space_units
        .iter()
        .any(|unit| unit != b"mm" && !unit.is_empty())
    {
        l.warnings.push(WARNING_SPACE_UNITS.to_string());
    }
    if h.fields.contains_key("measurement frame") {
        l.warnings
            .push(WARNING_MEASUREMENT_FRAME_IGNORED.to_string());
    }
    Ok(l)
}
fn decode(
    h: Header,
    data: &[u8],
    file_size: Option<i64>,
    options: &ReadOptions,
) -> Result<Image, Error> {
    let Some(volume_datatype) = datatype(&h.typ) else {
        return Err(wrap(Sentinel::Unsupported, format!("type {}", h.typ)));
    };
    let width = h.bytes_per_sample();
    let binary_encoding = h.encoding != "text";
    if width > 1 && binary_encoding && h.endian.is_empty() {
        return Err(wrap(
            Sentinel::MalformedHeader,
            format!("no endian for a {width}-byte type"),
        ));
    }
    let l = plan_layout(&h, options)?;
    let mut limits = options.limits;
    let defaults = nifti::default_read_limits();
    if limits.max_voxels == 0 {
        limits.max_voxels = defaults.max_voxels;
    }
    if limits.max_payload_bytes == 0 {
        limits.max_payload_bytes = defaults.max_payload_bytes;
    }
    let mut voxels = 1u64;
    for &size in &h.sizes {
        if voxels > limits.max_voxels / size as u64 {
            return Err(wrap(
                Sentinel::LimitExceeded,
                format!("voxel count exceeds {}", limits.max_voxels),
            ));
        }
        voxels *= size as u64;
    }
    let byte_limit = limits.max_payload_bytes.min(i64::MAX as u64);
    if voxels > byte_limit / width as u64 {
        return Err(wrap(
            Sentinel::LimitExceeded,
            format!("payload exceeds {byte_limit} bytes"),
        ));
    }
    let payload_bytes = voxels * width as u64;
    let mut at = 0usize;
    for _ in 0..h.line_skip {
        match data[at..].iter().position(|&b| b == b'\n') {
            Some(newline) => at += newline + 1,
            None => return Err(bare(Sentinel::TruncatedData)),
        }
    }
    if h.byte_skip == -1 {
        let Some(file_size) = file_size.filter(|_| h.encoding == "raw") else {
            return Err(wrap(
                Sentinel::Unsupported,
                "byte skip -1 needs raw data in a detached file",
            ));
        };
        let skip = file_size - payload_bytes as i64;
        if skip < 0 || (data.len() - at) < skip as usize {
            return Err(bare(Sentinel::TruncatedData));
        }
        at += skip as usize;
    }
    let data = &data[at..];
    let mut source = match h.encoding.as_str() {
        "gzip" => gunzip(data).map_err(|terminal| {
            wrap(
                Sentinel::MalformedHeader,
                format!("gzip: {}", terminal.text()),
            )
        })?,
        "bzip2" => ByteStream::bzip2(data),
        "hex" => hex_stream(data),
        _ => ByteStream::plain(data),
    };
    if h.byte_skip > 0 && source.discard(h.byte_skip as u64).is_err() {
        return Err(bare(Sentinel::TruncatedData));
    }
    let mut big_endian = h.endian == "big";
    let mut replaced: Option<ByteStream> = None;
    if h.encoding == "text" {
        let payload = decode_text(source.rest(), &h.typ, voxels as usize)?;
        replaced = Some(ByteStream::new(payload, Terminal::Eof));
        big_endian = false;
    }
    if l.non_spatial == 0 && l.dims[3] > 1 {
        let stream = replaced.as_mut().unwrap_or(&mut source);
        let Ok(interleaved) = stream.read_full(payload_bytes as usize) else {
            return Err(bare(Sentinel::TruncatedData));
        };
        replaced = Some(ByteStream::new(
            deinterleave(&interleaved, l.dims[3] as usize, width),
            Terminal::Eof,
        ));
    }
    let volume = nifti::new_volume_exact(
        replaced.as_mut().unwrap_or(&mut source),
        &NewVolumeOptions {
            datatype: volume_datatype,
            dimensions: l.dims,
            index_to_lps: l.index_to_lps,
            big_endian,
            limits,
        },
    )
    .map_err(Error::Volume)?;
    if h.encoding == "gzip" || h.encoding == "bzip2" {
        match source.read_full(1) {
            Ok(_) => {
                return Err(wrap(
                    Sentinel::MalformedHeader,
                    format!("{} payload exceeds declared dimensions", h.encoding),
                ));
            }
            Err(Terminal::Eof) => {}
            Err(Terminal::UnexpectedEof) => {
                return Err(wrap(
                    Sentinel::TruncatedData,
                    format!("{} payload: unexpected EOF", h.encoding),
                ));
            }
            Err(Terminal::Other(text)) => {
                return Err(Error::Nrrd {
                    sentinel: None,
                    message: format!("nrrd: {} payload: {text}", h.encoding),
                });
            }
        }
    }
    let mut warnings = l.warnings;
    warnings.extend(volume.warnings.iter().cloned());
    Ok(Image {
        header: h,
        volume,
        non_spatial_axis: l.non_spatial,
        warnings,
    })
}
fn deinterleave(data: &[u8], components: usize, width: usize) -> Vec<u8> {
    let mut out = vec![0u8; data.len()];
    let points = data.len() / (components * width);
    for p in 0..points {
        for c in 0..components {
            let from = (p * components + c) * width;
            let to = (c * points + p) * width;
            out[to..to + width].copy_from_slice(&data[from..from + width]);
        }
    }
    out
}
pub(crate) const HEX_DATA_ERROR: &str = "nrrd: malformed header: hex data";
const MAX_WORD_BYTES: usize = 1 << 20;
fn decode_text(data: &[u8], typ: &str, count: usize) -> Result<Vec<u8>, Error> {
    let width = match typ {
        "int8" | "uint8" => 1,
        "int16" | "uint16" => 2,
        "double" => 8,
        _ => 4,
    };
    let mut out = Vec::with_capacity(count.min(data.len()) * width);
    let mut at = 0usize;
    for n in 0..count {
        while at < data.len() {
            let (rune, size) = decode_rune(&data[at..]);
            if !rune.is_some_and(char::is_whitespace) {
                break;
            }
            at += size;
        }
        let start = at;
        while at < data.len() {
            let (rune, size) = decode_rune(&data[at..]);
            if rune.is_some_and(char::is_whitespace) {
                break;
            }
            at += size.max(1);
        }
        let word = &data[start..at];
        if word.is_empty() || word.len() >= MAX_WORD_BYTES {
            return Err(bare(Sentinel::TruncatedData));
        }
        let sample_error = || wrap(Sentinel::MalformedHeader, format!("text sample {n}"));
        let text = std::str::from_utf8(word).map_err(|_| sample_error())?;
        match typ {
            "float" | "double" => {
                let value = numtext::parse_float(text).map_err(|_| sample_error())?;
                if typ == "float" {
                    out.extend_from_slice(&(value as f32).to_bits().to_le_bytes());
                } else {
                    out.extend_from_slice(&value.to_bits().to_le_bytes());
                }
            }
            _ => {
                let value = numtext::parse_int(text).map_err(|_| sample_error())?;
                if !in_range(typ, value) {
                    return Err(sample_error());
                }
                match width {
                    1 => out.push(value as u8),
                    2 => out.extend_from_slice(&(value as u16).to_le_bytes()),
                    _ => out.extend_from_slice(&(value as u32).to_le_bytes()),
                }
            }
        }
    }
    Ok(out)
}
fn in_range(typ: &str, v: i64) -> bool {
    match typ {
        "int8" => (i8::MIN as i64..=i8::MAX as i64).contains(&v),
        "uint8" => (0..=u8::MAX as i64).contains(&v),
        "int16" => (i16::MIN as i64..=i16::MAX as i64).contains(&v),
        "uint16" => (0..=u16::MAX as i64).contains(&v),
        "int32" => (i32::MIN as i64..=i32::MAX as i64).contains(&v),
        _ => (0..=u32::MAX as i64).contains(&v),
    }
}
fn hex_stream(data: &[u8]) -> ByteStream {
    let mut out = Vec::with_capacity(data.len() / 2);
    let mut high: Option<u8> = None;
    for &c in data {
        let v = match c {
            b'0'..=b'9' => c - b'0',
            b'a'..=b'f' => c - b'a' + 10,
            b'A'..=b'F' => c - b'A' + 10,
            b' ' | b'\t' | b'\n' | b'\r' => continue,
            _ => {
                return ByteStream::new(out, Terminal::Other(HEX_DATA_ERROR.to_string()));
            }
        };
        match high.take() {
            None => high = Some(v),
            Some(h) => out.push(h << 4 | v),
        }
    }
    ByteStream::new(out, Terminal::Eof)
}
