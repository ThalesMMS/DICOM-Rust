use super::{Error, ErrorKind};
use crate::object::{File, Object, Tag, Value, Vr, transfer};
use crate::pixel::Metadata;
use crate::pixel::encapsulated::{self, Format, Limits, Recovery, TableAttribute, TableValue};
use crate::pixel::pixeldata::{PixelData, decode_frames, extract_metadata, frame_size, total_size};
use std::borrow::Cow;
pub(super) const ERR_PARTIAL_DECODE_UNSUPPORTED: &str =
    "dicom: codec cannot serve this partial decode";
pub(super) const ERR_DECOMPRESS_RESOURCE_LIMIT: &str =
    "dicom: decompression resource limit exceeded";
const ERR_FRAME_LAYOUT: &str = "dicom: frames cannot be located in Pixel Data";
const ERR_INVALID_METADATA: &str = "dicom: invalid pixel data metadata";
const ERR_SIZE_MISMATCH: &str = "dicom: pixel data size does not match metadata";
const ERR_INVALID_LIMITS: &str = "dicom: invalid decompression limits";
const ERR_MISSING_METADATA: &str = "dicom: missing required pixel data metadata";
const ERR_PIXEL_DATA_NOT_FOUND: &str = "dicom: pixel data element not found";
const ERR_ENCAPSULATED_RESOURCE: &str = "dicom: encapsulated frame resource limit exceeded";
pub(super) const MAX_REDUCTION_LEVEL: i64 = 16;
const TAG_PIXEL_DATA: Tag = Tag::new(0x7FE0, 0x0010);
const TAG_EXTENDED_OFFSET_TABLE: Tag = Tag::new(0x7FE0, 0x0001);
const TAG_EXTENDED_OFFSET_TABLE_LENGTHS: Tag = Tag::new(0x7FE0, 0x0002);
const TAG_ENCAPSULATED_TOTAL_LENGTH: Tag = Tag::new(0x7FE0, 0x0003);
const TAG_NUMBER_OF_FRAMES: Tag = Tag::new(0x0028, 0x0008);
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DecompressLimits {
    pub max_frames: i64,
    pub max_pixels: i64,
    pub max_native_bytes: i64,
    pub max_input_bytes: i64,
    pub max_expansion_ratio: i64,
}
impl DecompressLimits {
    pub fn defaults() -> DecompressLimits {
        DecompressLimits {
            max_frames: 100_000,
            max_pixels: 256 * 1024 * 1024,
            max_native_bytes: 512 << 20,
            max_input_bytes: 512 << 20,
            max_expansion_ratio: 1024,
        }
    }
    fn normalized(self) -> Result<DecompressLimits, Error> {
        if self.max_frames < 0
            || self.max_pixels < 0
            || self.max_native_bytes < 0
            || self.max_input_bytes < 0
            || self.max_expansion_ratio < 0
        {
            return Err(Error::other(ERR_INVALID_LIMITS));
        }
        let d = DecompressLimits::defaults();
        let pick = |v: i64, d: i64| if v == 0 { d } else { v };
        Ok(DecompressLimits {
            max_frames: pick(self.max_frames, d.max_frames),
            max_pixels: pick(self.max_pixels, d.max_pixels),
            max_native_bytes: pick(self.max_native_bytes, d.max_native_bytes),
            max_input_bytes: pick(self.max_input_bytes, d.max_input_bytes),
            max_expansion_ratio: pick(self.max_expansion_ratio, d.max_expansion_ratio),
        })
    }
}
pub(super) fn limit_error(name: &str, actual: i64, maximum: i64) -> Error {
    Error::new(
        ErrorKind::ResourceLimit,
        format!("{ERR_DECOMPRESS_RESOURCE_LIMIT}: {name} actual={actual} maximum={maximum}"),
    )
}
fn layout(detail: impl std::fmt::Display) -> Error {
    Error::other(format!("{ERR_FRAME_LAYOUT}: {detail}"))
}
fn checked_product(values: &[i64]) -> Option<i64> {
    values
        .iter()
        .try_fold(1i64, |p, &v| if v <= 0 { None } else { p.checked_mul(v) })
}
fn bytes_per_sample(m: &Metadata) -> i64 {
    (i64::from(m.bits_allocated) + 7) / 8
}
pub(super) fn is_subsampled_422(photometric: &str) -> bool {
    let p = crate::textfmt::to_upper(photometric);
    let p = p.trim_matches([' ', '\0']);
    p == "YBR_FULL_422" || p == "YBR_PARTIAL_422"
}
fn normalized_photometric(value: &str) -> String {
    crate::textfmt::to_upper(value)
        .trim_matches([' ', '\0'])
        .to_owned()
}
enum Kind<'a> {
    Native {
        raw: &'a [u8],
        lanes: bool,
    },
    Encapsulated {
        basic: &'a [u8],
        fragments: Vec<&'a [u8]>,
        format: Option<Format>,
    },
}
pub(super) struct FrameReader<'a> {
    dataset: &'a Object,
    syntax: transfer::Syntax,
    pub metadata: Metadata,
    limits: DecompressLimits,
    kind: Kind<'a>,
    max_frames: i64,
    max_fragments: i64,
}
fn build_plan<'b>(
    dataset: &Object,
    basic: &[u8],
    fragments: &'b [&'b [u8]],
    frames: i64,
    format: Format,
    max_frames: i64,
    max_fragments: i64,
) -> Result<encapsulated::Plan<'b>, Error> {
    encapsulated::from_fragments(
        basic,
        fragments,
        crate::pixel::encapsulated::EotAttributes {
            extended: table_attribute(dataset, TAG_EXTENDED_OFFSET_TABLE),
            lengths: table_attribute(dataset, TAG_EXTENDED_OFFSET_TABLE_LENGTHS),
        },
        frames,
        format,
        Limits {
            max_frames,
            max_fragments,
            ..Limits::default()
        },
        Recovery::default(),
    )
    .map_err(layout)
}
fn stream_frame_format(uid: &str) -> Option<Format> {
    match transfer::normalize_uid(uid) {
        "1.2.840.10008.1.2.4.50"
        | "1.2.840.10008.1.2.4.51"
        | "1.2.840.10008.1.2.4.57"
        | "1.2.840.10008.1.2.4.70" => Some(Format::JPEG),
        "1.2.840.10008.1.2.4.80" | "1.2.840.10008.1.2.4.81" => Some(Format::JPEGLS),
        "1.2.840.10008.1.2.4.90"
        | "1.2.840.10008.1.2.4.91"
        | "1.2.840.10008.1.2.4.201"
        | "1.2.840.10008.1.2.4.202"
        | "1.2.840.10008.1.2.4.203" => Some(Format::JPEG2000),
        _ => None,
    }
}
fn table_attribute(obj: &Object, tag: Tag) -> Option<TableAttribute<'_>> {
    let e = obj.get(tag)?;
    Some(TableAttribute {
        vr_ov: e.vr() == Vr::OV,
        value: match &e.value {
            Value::Raw(r) => TableValue::Raw(r),
            Value::Nil => TableValue::Unsupported("<nil>"),
            Value::Strings(_) => TableValue::Unsupported("core.StringValue"),
            Value::Sequence(_) => TableValue::Unsupported("core.SequenceValue"),
            Value::Fragments { .. } => TableValue::Unsupported("core.FragmentSequence"),
            Value::BulkData(_) => TableValue::Unsupported("core.BulkDataValue"),
        },
    })
}
fn value_type_name(v: &Value) -> &'static str {
    match v {
        Value::Nil => "<nil>",
        Value::Raw(_) => "core.RawValue",
        Value::Strings(_) => "core.StringValue",
        Value::Sequence(_) => "core.SequenceValue",
        Value::Fragments { .. } => "core.FragmentSequence",
        Value::BulkData(_) => "core.BulkDataValue",
    }
}
impl<'a> FrameReader<'a> {
    pub fn new(
        file: &'a File,
        limits: DecompressLimits,
        max_frames: i64,
        max_fragments: i64,
    ) -> Result<FrameReader<'a>, Error> {
        if file.partial_read.is_some() || file.incomplete_pixel_data.is_some() {
            return Err(Error::bare(ErrorKind::PartialFile));
        }
        let syntax = file.transfer_syntax;
        let dataset = &file.dataset;
        let pixel = dataset
            .get(TAG_PIXEL_DATA)
            .ok_or_else(|| Error::other(ERR_PIXEL_DATA_NOT_FOUND))?;
        let metadata = extract_metadata(dataset).map_err(Error::other)?;
        if max_frames < 0 || max_fragments < 0 {
            return Err(layout(format!(
                "{ERR_ENCAPSULATED_RESOURCE}: negative frame index limit"
            )));
        }
        let max_frames = if max_frames == 0 { 100_000 } else { max_frames };
        let max_fragments = if max_fragments == 0 {
            100_000
        } else {
            max_fragments
        };
        let frames = metadata.number_of_frames;
        if frames > max_frames {
            if syntax.encapsulated {
                return Err(layout(format!(
                    "{ERR_ENCAPSULATED_RESOURCE}: frame count {frames} exceeds {max_frames}"
                )));
            }
            return Err(layout(limit_error("MaxFrames", frames, max_frames)));
        }
        if metadata.bits_allocated < 8 && (metadata.bits_allocated != 1 || syntax.encapsulated) {
            return Err(layout(format!(
                "BitsAllocated {} packs frames across bytes",
                metadata.bits_allocated
            )));
        }
        let kind = if !syntax.encapsulated {
            let Value::Raw(raw) = &pixel.value else {
                return Err(layout(format!(
                    "native Pixel Data value {}",
                    value_type_name(&pixel.value)
                )));
            };
            if metadata.bits_allocated == 1 {
                return Err(layout("BitsAllocated 1 frames are not ported"));
            }
            let lanes = syntax.big_endian && pixel.vr() == Vr::OW && metadata.bits_allocated <= 8;
            let mut total = total_size(&metadata);
            if total <= 0 || (lanes && total == i64::MAX) {
                return Err(layout("invalid native Pixel Data dimensions"));
            }
            if lanes {
                total += total % 2;
                if raw.len() % 2 != 0 {
                    return Err(Error::other(format!(
                        "{ERR_SIZE_MISMATCH}: Big Endian OW Pixel Data ends with an incomplete word"
                    )));
                }
            }
            if (raw.len() as i64) < total {
                return Err(Error::other(format!(
                    "{ERR_SIZE_MISMATCH}: {} bytes of Pixel Data, need {total} for {frames} frames",
                    raw.len()
                )));
            }
            Kind::Native { raw, lanes }
        } else {
            let Value::Fragments {
                offset_table,
                fragments,
            } = &pixel.value
            else {
                return Err(layout(format!(
                    "encapsulated Pixel Data value {}",
                    value_type_name(&pixel.value)
                )));
            };
            if fragments.len() as i64 > max_fragments {
                return Err(layout(format!(
                    "{ERR_ENCAPSULATED_RESOURCE}: fragment count {} exceeds {max_fragments}",
                    fragments.len()
                )));
            }
            if (offset_table.len() as u64).div_ceil(4) > max_frames as u64 {
                return Err(layout(format!(
                    "{ERR_ENCAPSULATED_RESOURCE}: Basic Offset Table exceeds the frame index limit"
                )));
            }
            let slices: Vec<&'a [u8]> = fragments.iter().map(Vec::as_slice).collect();
            let format = stream_frame_format(syntax.uid);
            if let Some(format) = format {
                build_plan(
                    dataset,
                    offset_table,
                    &slices,
                    frames,
                    format,
                    max_frames,
                    max_fragments,
                )?;
            } else {
                if slices.len() as i64 != frames {
                    return Err(layout(format!(
                        "{} Items for {frames} frames",
                        slices.len()
                    )));
                }
                if !offset_table.is_empty() {
                    if offset_table.len() as i64 != 4 * frames {
                        return Err(layout(format!(
                            "Basic Offset Table of {} bytes for {frames} frames",
                            offset_table.len()
                        )));
                    }
                    let mut next = 0u64;
                    for (i, f) in slices.iter().enumerate() {
                        let size = f.len() as u64;
                        let entry = u32::from_le_bytes(
                            offset_table[4 * i..4 * i + 4].try_into().unwrap_or([0; 4]),
                        );
                        if u64::from(entry) != next {
                            return Err(layout(format!(
                                "Basic Offset Table entry {i} does not point at Item {i}"
                            )));
                        }
                        next += 8 + size + size % 2;
                    }
                }
            }
            Kind::Encapsulated {
                basic: offset_table,
                fragments: slices,
                format,
            }
        };
        Ok(FrameReader {
            dataset,
            syntax,
            metadata,
            limits,
            kind,
            max_frames,
            max_fragments,
        })
    }
    fn check_index(&self, i: i64) -> Result<(), Error> {
        if i < 0 || i >= self.metadata.number_of_frames {
            return Err(Error::other(format!(
                "{ERR_INVALID_METADATA}: frame {i} of {}",
                self.metadata.number_of_frames
            )));
        }
        Ok(())
    }
    fn frame_metadata(&self) -> Metadata {
        let mut m = self.metadata.clone();
        m.number_of_frames = 1;
        m
    }
    pub fn is_native(&self) -> bool {
        matches!(self.kind, Kind::Native { .. })
    }
    fn proxy(&self) -> Object {
        let mut elements = Vec::new();
        for e in self.dataset.elements() {
            let tag = e.tag();
            if tag == TAG_PIXEL_DATA
                || tag == TAG_EXTENDED_OFFSET_TABLE
                || tag == TAG_EXTENDED_OFFSET_TABLE_LENGTHS
                || tag == TAG_ENCAPSULATED_TOTAL_LENGTH
            {
                continue;
            }
            if tag == TAG_NUMBER_OF_FRAMES {
                elements.push(crate::object::Element {
                    header: crate::object::ElementHeader {
                        tag,
                        vr: Vr::IS,
                        length: 0,
                        length_set: false,
                    },
                    value: Value::Strings(vec!["1".into()]),
                });
                continue;
            }
            elements.push(e.clone());
        }
        let mut proxy = Object::from_elements(elements);
        proxy.set_value_big_endian(self.syntax.big_endian);
        proxy
    }
    fn read_native(
        &self,
        i: i64,
        region: Option<(i64, i64, i64, i64)>,
    ) -> Result<(Vec<u8>, Metadata), Error> {
        self.check_index(i)?;
        let Kind::Native { raw, lanes } = self.kind else {
            unreachable!()
        };
        let mut m = self.frame_metadata();
        let (x0, y0, x1, y1) = region.unwrap_or((
            0,
            0,
            i64::from(self.metadata.columns),
            i64::from(self.metadata.rows),
        ));
        if region.is_some() && is_subsampled_422(&m.photometric_interpretation) {
            return Err(Error::other(ERR_FRAME_LAYOUT));
        }
        m.rows = (y1 - y0) as u16;
        m.columns = (x1 - x0) as u16;
        let bytes = frame_size(&m);
        if m.samples_per_pixel == 0 || m.bits_allocated < 8 || bytes <= 0 {
            return Err(Error::other(ERR_FRAME_LAYOUT));
        }
        let mut limits = self.limits;
        limits.max_frames = 1;
        let limits = limits.normalized()?;
        if region.is_none() && self.syntax.big_endian {
            extract_metadata(&self.proxy()).map_err(Error::other)?;
        }
        let input = if lanes {
            let start = i * frame_size(&self.frame_metadata());
            let (first, end) = (start & !1, start + bytes);
            let end = end + end % 2;
            end - first
        } else {
            bytes
        };
        check_frame_read_limits(&limits, &m, input, bytes)?;
        let full = frame_size(&self.frame_metadata());
        let start = (i * full) as usize;
        let mut data = vec![0u8; bytes as usize];
        let width = bytes_per_sample(&self.metadata) as usize;
        let samples = usize::from(self.metadata.samples_per_pixel);
        let (planes, per_pixel) = if samples > 1 && self.metadata.planar_configuration == 1 {
            (samples, 1)
        } else {
            (1, samples)
        };
        let columns = usize::from(self.metadata.columns);
        let rows = usize::from(self.metadata.rows);
        let mut touched = 0usize;
        let mut copy_run = |offset: usize, len: usize| {
            let src = start + offset;
            if lanes {
                for k in 0..len {
                    let at = src + k;
                    data[touched + k] = raw[at ^ 1];
                }
            } else {
                data[touched..touched + len].copy_from_slice(&raw[src..src + len]);
            }
            touched += len;
        };
        if region.is_none() {
            copy_run(0, bytes as usize);
        } else {
            let row_units = (x1 - x0) as usize * per_pixel * width;
            for p in 0..planes {
                for y in y0 as usize..y1 as usize {
                    let offset = ((p * rows + y) * columns + x0 as usize) * per_pixel * width;
                    copy_run(offset, row_units);
                }
            }
        }
        if self.syntax.big_endian && width > 1 && !lanes {
            for chunk in data.chunks_exact_mut(width) {
                chunk.reverse();
            }
        }
        Ok((data, m))
    }
    pub fn frame(&self, i: i64) -> Result<(Vec<u8>, Metadata), Error> {
        if self.is_native() {
            return self.read_native(i, None);
        }
        self.check_index(i)?;
        let Kind::Encapsulated {
            basic,
            fragments,
            format,
        } = &self.kind
        else {
            unreachable!()
        };
        let plan = match format {
            Some(f) => Some(build_plan(
                self.dataset,
                basic,
                fragments,
                self.metadata.number_of_frames,
                *f,
                self.max_frames,
                self.max_fragments,
            )?),
            None => None,
        };
        let mut limits = self.limits;
        limits.max_frames = 1;
        let limits = limits.normalized()?;
        let proxy = self.proxy();
        let decode_metadata = extract_metadata(&proxy).map_err(Error::other)?;
        let input_bytes = match &plan {
            Some(p) => p
                .frame_length(i as usize)
                .ok_or_else(|| layout(format!("frame {}", i + 1)))? as i64,
            None => fragments[i as usize].len() as i64,
        };
        check_decode_admission(&limits, &decode_metadata, &self.syntax, input_bytes)?;
        let frame: Cow<'_, [u8]> = match &plan {
            Some(p) => p.frame(i as usize).map_err(Error::other)?.data,
            None => Cow::Borrowed(fragments[i as usize]),
        };
        let one = [frame.to_vec()];
        let view = PixelData::Encapsulated {
            offset_table: &[],
            fragments: &one,
        };
        let decoded = decode_frames(self.syntax.uid, &view, &proxy).map_err(Error::other)?;
        let mut metadata = decompressed_output_metadata(
            decode_metadata,
            self.syntax.uid,
            &decoded.photometric_interpretation,
            &decoded.data,
        );
        let mut data = decoded.data;
        if self.syntax.big_endian && bytes_per_sample(&metadata) > 1 {
            let w = bytes_per_sample(&metadata) as usize;
            for (k, f) in data.iter_mut().enumerate() {
                if f.len() % w != 0 {
                    return Err(Error::other(format!(
                        "{ERR_SIZE_MISMATCH}: decoded frame {k} has {} bytes, not a multiple of {w}-byte samples",
                        f.len()
                    )));
                }
                for c in f.chunks_exact_mut(w) {
                    c.reverse();
                }
            }
        }
        if decoded.rows != usize::from(metadata.rows)
            || decoded.columns != usize::from(metadata.columns)
        {
            return Err(Error::other(format!(
                "{ERR_SIZE_MISMATCH}: decoded frame geometry={}x{} metadata={}x{}",
                decoded.rows, decoded.columns, metadata.rows, metadata.columns
            )));
        }
        if data.len() as i64 != metadata.number_of_frames {
            return Err(Error::other(format!(
                "{ERR_SIZE_MISMATCH}: decoded frame count={} metadata NumberOfFrames={}",
                data.len(),
                metadata.number_of_frames
            )));
        }
        let fsize = frame_size(&metadata);
        if fsize <= 0 || metadata.bits_allocated < 8 {
            return Err(Error::other(format!(
                "{ERR_INVALID_METADATA}: rows={} columns={} samples_per_pixel={} bits_allocated={} number_of_frames={}",
                metadata.rows,
                metadata.columns,
                metadata.samples_per_pixel,
                metadata.bits_allocated,
                metadata.number_of_frames
            )));
        }
        let out = data.swap_remove(0);
        if out.len() as i64 != fsize {
            return Err(Error::other(format!(
                "{ERR_SIZE_MISMATCH}: decoded frame 0 has {} bytes, want {fsize}",
                out.len()
            )));
        }
        metadata.photometric_interpretation = metadata
            .photometric_interpretation
            .trim_matches([' ', '\0'])
            .to_owned();
        if metadata.photometric_interpretation.is_empty() {
            return Err(Error::other(format!(
                "{ERR_MISSING_METADATA}: PhotometricInterpretation ((0028,0004))"
            )));
        }
        Ok((out, metadata))
    }
}
fn check_frame_read_limits(
    limits: &DecompressLimits,
    m: &Metadata,
    input_bytes: i64,
    native_bytes: i64,
) -> Result<(), Error> {
    if input_bytes < 0 {
        return Err(limit_error(
            "MaxInputBytes",
            i64::MAX,
            limits.max_input_bytes,
        ));
    }
    if input_bytes > limits.max_input_bytes {
        return Err(limit_error(
            "MaxInputBytes",
            input_bytes,
            limits.max_input_bytes,
        ));
    }
    if m.rows == 0
        || m.columns == 0
        || m.samples_per_pixel == 0
        || m.bits_allocated == 0
        || m.number_of_frames <= 0
    {
        return Err(Error::other(format!(
            "{ERR_INVALID_METADATA}: rows={} columns={} samples_per_pixel={} bits_allocated={} number_of_frames={}",
            m.rows, m.columns, m.samples_per_pixel, m.bits_allocated, m.number_of_frames
        )));
    }
    let Some(pixels) =
        checked_product(&[i64::from(m.rows), i64::from(m.columns), m.number_of_frames])
    else {
        return Err(limit_error("MaxPixels", i64::MAX, limits.max_pixels));
    };
    if pixels > limits.max_pixels {
        return Err(limit_error("MaxPixels", pixels, limits.max_pixels));
    }
    if native_bytes <= 0 {
        return Err(limit_error(
            "MaxNativeBytes",
            i64::MAX,
            limits.max_native_bytes,
        ));
    }
    if native_bytes > limits.max_native_bytes {
        return Err(limit_error(
            "MaxNativeBytes",
            native_bytes,
            limits.max_native_bytes,
        ));
    }
    Ok(())
}
fn check_decode_admission(
    limits: &DecompressLimits,
    m: &Metadata,
    source: &transfer::Syntax,
    input_bytes: i64,
) -> Result<(), Error> {
    if input_bytes < 0 {
        return Err(limit_error(
            "MaxInputBytes",
            i64::MAX,
            limits.max_input_bytes,
        ));
    }
    if input_bytes > limits.max_input_bytes {
        return Err(limit_error(
            "MaxInputBytes",
            input_bytes,
            limits.max_input_bytes,
        ));
    }
    if m.rows == 0
        || m.columns == 0
        || m.samples_per_pixel == 0
        || m.bits_allocated == 0
        || m.number_of_frames <= 0
    {
        return Err(Error::other(format!(
            "{ERR_INVALID_METADATA}: rows={} columns={} samples_per_pixel={} bits_allocated={} number_of_frames={}",
            m.rows, m.columns, m.samples_per_pixel, m.bits_allocated, m.number_of_frames
        )));
    }
    if m.number_of_frames > limits.max_frames {
        return Err(limit_error(
            "MaxFrames",
            m.number_of_frames,
            limits.max_frames,
        ));
    }
    let Some(pixels) =
        checked_product(&[i64::from(m.rows), i64::from(m.columns), m.number_of_frames])
    else {
        return Err(limit_error("MaxPixels", i64::MAX, limits.max_pixels));
    };
    if pixels > limits.max_pixels {
        return Err(limit_error("MaxPixels", pixels, limits.max_pixels));
    }
    let native = estimated_native_bytes(m, source);
    let Some(native) = native.filter(|&n| n > 0) else {
        return Err(limit_error(
            "MaxNativeBytes",
            i64::MAX,
            limits.max_native_bytes,
        ));
    };
    if native > limits.max_native_bytes {
        return Err(limit_error(
            "MaxNativeBytes",
            native,
            limits.max_native_bytes,
        ));
    }
    if source.encapsulated && transfer::normalize_uid(source.uid) != "1.2.840.10008.1.2.1.98" {
        let ratio = if input_bytes <= 0 {
            i64::MAX
        } else {
            native / input_bytes + i64::from(native % input_bytes != 0)
        };
        if ratio > limits.max_expansion_ratio {
            return Err(limit_error(
                "MaxExpansionRatio",
                ratio,
                limits.max_expansion_ratio,
            ));
        }
    }
    Ok(())
}
fn estimated_native_bytes(m: &Metadata, source: &transfer::Syntax) -> Option<i64> {
    let native = total_size(m);
    if native <= 0 {
        return None;
    }
    if !is_subsampled_422(&m.photometric_interpretation) {
        return Some(native);
    }
    match transfer::normalize_uid(source.uid) {
        "1.2.840.10008.1.2.4.50" | "1.2.840.10008.1.2.4.51" => {}
        _ => return Some(native),
    }
    if m.bits_allocated < 8 {
        return Some(native);
    }
    let expanded = checked_product(&[
        i64::from(m.rows),
        i64::from(m.columns),
        i64::from(m.samples_per_pixel),
        bytes_per_sample(m),
        m.number_of_frames,
    ])?;
    Some(expanded.max(native))
}
fn decompressed_output_metadata(
    mut m: Metadata,
    uid: &str,
    photometric: &str,
    data: &[Vec<u8>],
) -> Metadata {
    if m.samples_per_pixel != 3 {
        return m;
    }
    let set_rgb = |m: &mut Metadata, p: &str| {
        m.photometric_interpretation = p.to_owned();
        m.planar_configuration = 0;
        m.planar_configuration_present = true;
    };
    if !photometric.is_empty() {
        set_rgb(&mut m, photometric);
        return m;
    }
    match transfer::normalize_uid(uid) {
        "1.2.840.10008.1.2.5" => {
            m.planar_configuration = 0;
            m.planar_configuration_present = true;
        }
        "1.2.840.10008.1.2.4.50" | "1.2.840.10008.1.2.4.51" => {
            match normalized_photometric(&m.photometric_interpretation).as_str() {
                "YBR_FULL" => set_rgb(&mut m, "RGB"),
                "YBR_FULL_422" | "YBR_PARTIAL_422" => {
                    let size = i64::from(m.rows) * i64::from(m.columns) * 3 * bytes_per_sample(&m);
                    if m.bits_allocated >= 8
                        && !data.is_empty()
                        && data.iter().all(|f| f.len() as i64 == size)
                    {
                        set_rgb(&mut m, "RGB");
                    }
                }
                _ => {}
            }
        }
        _ => {}
    }
    m
}
pub(super) fn sample_format_error(e: impl std::fmt::Display) -> Error {
    let cause = e.to_string();
    let field = cause
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_owned();
    Error::other(format!(
        "dicom: invalid stored sample format: {field}: {cause}"
    ))
}
