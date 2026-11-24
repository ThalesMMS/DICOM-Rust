#![forbid(unsafe_code)]
use super::error::Error;
use super::file::File;
use super::obj::Object;
use super::types::{Element, ElementHeader, Tag, UNDEFINED_LENGTH, Value, Vr};
use super::writer::{self, WriteFileOptions};
pub const ERR_FRAME_SOURCE: &str = "dicom: invalid frame source";
const ERR_PARTIAL_FILE: &str = "dicom: file is the result of a partial read";
const TAG_EXTENDED_OFFSET_TABLE: Tag = Tag::new(0x7FE0, 0x0001);
const TAG_EXTENDED_OFFSET_TABLE_LENGTHS: Tag = Tag::new(0x7FE0, 0x0002);
const TAG_ENCAPSULATED_TOTAL_LENGTH: Tag = Tag::new(0x7FE0, 0x0003);
const TAG_NUMBER_OF_FRAMES: Tag = Tag::new(0x0028, 0x0008);
const TAG_BITS_ALLOCATED: Tag = Tag::new(0x0028, 0x0100);
const MAX_ITEM: u64 = u32::MAX as u64 - 1;
pub mod offset_table {
    pub const AUTO: u8 = 0;
    pub const BASIC: u8 = 1;
    pub const EXTENDED: u8 = 2;
    pub const EMPTY: u8 = 3;
}
#[derive(Clone, Debug, Default)]
pub struct FrameWriteOptions {
    pub write: WriteFileOptions,
    pub offset_table: u8,
    pub max_fragment_bytes: i64,
    pub max_fragments: i64,
}
#[derive(Clone, Copy, Debug)]
pub struct FrameSource<'a> {
    pub frames: &'a [Vec<u8>],
    pub packed_bits: Option<&'a [i64]>,
}
fn source_error(detail: impl std::fmt::Display) -> Error {
    Error::msg(format!("{ERR_FRAME_SOURCE}: {detail}"))
}
#[derive(Debug, Default)]
pub(crate) struct Layout {
    pub(crate) basic: Vec<u8>,
    pub(crate) extended: Option<(Vec<u8>, Vec<u8>)>,
}
pub(crate) fn single_fragment(uid: &str) -> bool {
    matches!(
        uid,
        "1.2.840.10008.1.2.5" | "1.2.840.10008.1.2.1.98" | "1.2.840.10008.1.2.8.1"
    )
}
pub(crate) fn fragment_length(remaining: u64, cap: u64) -> u64 {
    if cap == 0 || remaining <= cap {
        return remaining;
    }
    if cap >= 4 && remaining - cap <= 2 {
        return cap - 2;
    }
    cap
}
pub(crate) fn plan(
    lengths: &[u64],
    max_fragment: u64,
    policy: u8,
    max_fragments: i64,
    single: bool,
) -> Result<Layout, String> {
    let mut result = Layout::default();
    if policy > offset_table::EMPTY
        || max_fragments < 0
        || max_fragment != 0
            && (max_fragment < 2 || !max_fragment.is_multiple_of(2) || max_fragment > MAX_ITEM)
    {
        return Err("invalid encapsulation options".to_string());
    }
    let mut offsets = vec![0u64; lengths.len()];
    let mut next = 0u64;
    let mut fragments = 0i64;
    let (mut fits, mut split) = (true, false);
    for (i, &length) in lengths.iter().enumerate() {
        if length == 0 {
            return Err(format!("frame {} is empty", i + 1));
        }
        let cap = if max_fragment == 0 {
            MAX_ITEM
        } else {
            max_fragment
        };
        let count = (length - 1) / cap + 1;
        if max_fragment == 0 && count > 1 || single && count > 1 {
            return Err(format!("frame {} must fit one Fragment", i + 1));
        }
        if count > (i64::MAX - fragments) as u64
            || max_fragments > 0 && count > (max_fragments - fragments) as u64
        {
            return Err("fragment count exceeds limit".to_string());
        }
        fragments += count as i64;
        split = split || count > 1;
        offsets[i] = next;
        fits = fits && next <= u64::from(u32::MAX);
        if length == u64::MAX || count > (u64::MAX - length - length % 2) / 8 {
            return Err("encapsulated length overflow".to_string());
        }
        let span = length + length % 2 + 8 * count;
        if next > u64::MAX - span {
            return Err("encapsulated offsets overflow".to_string());
        }
        next += span;
    }
    if policy == offset_table::EMPTY {
        return Ok(result);
    }
    if policy == offset_table::BASIC && !fits {
        return Err("frame offsets exceed Basic Offset Table".to_string());
    }
    if policy == offset_table::EXTENDED || !fits {
        if split {
            return Err("Extended Offset Table requires one Fragment per frame".to_string());
        }
        let extended = offsets.iter().flat_map(|o| o.to_le_bytes()).collect();
        let extended_lengths = lengths.iter().flat_map(|l| l.to_le_bytes()).collect();
        result.extended = Some((extended, extended_lengths));
        return Ok(result);
    }
    result.basic = offsets
        .iter()
        .flat_map(|o| (*o as u32).to_le_bytes())
        .collect();
    Ok(result)
}
fn declared_frame_count(dataset: &Object) -> i64 {
    let value = dataset
        .get_string(TAG_NUMBER_OF_FRAMES)
        .map(|v| v.trim().to_string());
    match value {
        None => 1,
        Some(v) if v.is_empty() => 1,
        Some(v) => v.parse::<i64>().unwrap_or(-1),
    }
}
fn native_packed_frame_bits(dataset: &Object) -> Result<i64, Error> {
    if dataset.get_int(TAG_BITS_ALLOCATED).ok() != Some(1) {
        return Err(source_error(
            "packed bit source requires native Bits Allocated 1",
        ));
    }
    let mut bits = 1i64;
    for tag in [
        Tag::new(0x0028, 0x0010),
        Tag::new(0x0028, 0x0011),
        Tag::new(0x0028, 0x0002),
    ] {
        match dataset.get_int(tag) {
            Ok(n) if n > 0 && bits <= (i64::MAX - 7) / n => bits *= n,
            _ => return Err(source_error("invalid packed frame dimensions")),
        }
    }
    Ok(bits)
}
fn pack_bits(frames: &[Vec<u8>], bits: i64, pad: bool) -> Result<Vec<u8>, Error> {
    let mut out = Vec::new();
    let mut carry = 0u16;
    let mut carry_bits = 0u32;
    for frame in frames {
        let mut remaining = bits;
        for &b in frame {
            let n = remaining.min(8) as u32;
            if n < 8 && (b >> n) != 0 {
                return Err(source_error("nonzero unused packed frame bits"));
            }
            carry |= u16::from(b) << carry_bits;
            carry_bits += n;
            remaining -= i64::from(n);
            if carry_bits >= 8 {
                out.push(carry as u8);
                carry >>= 8;
                carry_bits -= 8;
            }
        }
    }
    if carry_bits > 0 {
        out.push(carry as u8);
    }
    if pad {
        out.push(0);
    }
    Ok(out)
}
pub fn write_file_frames(
    file: &File,
    source: FrameSource<'_>,
    opts: &FrameWriteOptions,
) -> Result<Vec<u8>, Error> {
    if file.incomplete_pixel_data.is_some()
        || file.partial_read.is_some() && !opts.write.allow_partial
    {
        return Err(Error::msg(ERR_PARTIAL_FILE));
    }
    let syntax = writer::frame_writer_syntax(file, &opts.write)?;
    let frames = source.frames;
    if source.packed_bits.is_some() && syntax.encapsulated {
        return Err(source_error("packed bit source requires native output"));
    }
    let count = frames.len() as i64;
    if count < 1 {
        return Err(source_error(format!("{count} frames")));
    }
    if syntax.encapsulated && opts.max_fragments > 0 && count > opts.max_fragments {
        return Err(source_error("frame count exceeds fragment cap"));
    }
    let declared = declared_frame_count(&file.dataset);
    if declared != count {
        return Err(source_error(format!(
            "{count} frames for Number of Frames {declared}"
        )));
    }
    let first = frames[0].len();
    for (i, frame) in frames.iter().enumerate() {
        if frame.is_empty() || (!syntax.encapsulated && frame.len() != first) {
            return Err(source_error(format!(
                "frame {} has length {}",
                i + 1,
                frame.len()
            )));
        }
    }
    let mut elements: Vec<Element> = file
        .dataset
        .elements()
        .iter()
        .filter(|e| {
            !matches!(
                e.tag(),
                Tag::PIXEL_DATA
                    | TAG_EXTENDED_OFFSET_TABLE
                    | TAG_EXTENDED_OFFSET_TABLE_LENGTHS
                    | TAG_ENCAPSULATED_TOTAL_LENGTH
            )
        })
        .cloned()
        .collect();
    let value_big_endian = file.dataset.value_big_endian();
    let mut value_error = None;
    if syntax.encapsulated {
        if opts.max_fragment_bytes < 0 {
            return Err(source_error("negative fragment cap"));
        }
        let cap = opts.max_fragment_bytes as u64;
        let lengths: Vec<u64> = frames.iter().map(|f| f.len() as u64).collect();
        let layout = plan(
            &lengths,
            cap,
            opts.offset_table,
            opts.max_fragments,
            single_fragment(syntax.uid),
        )
        .map_err(source_error)?;
        if let Some((mut extended, mut extended_lengths)) = layout.extended {
            if value_big_endian {
                extended
                    .as_chunks_mut::<8>()
                    .0
                    .iter_mut()
                    .for_each(|c| c.reverse());
                extended_lengths
                    .as_chunks_mut::<8>()
                    .0
                    .iter_mut()
                    .for_each(|c| c.reverse());
            }
            for (tag, raw) in [
                (TAG_EXTENDED_OFFSET_TABLE, extended),
                (TAG_EXTENDED_OFFSET_TABLE_LENGTHS, extended_lengths),
            ] {
                elements.push(Element {
                    header: ElementHeader::new(tag, Vr::OV, raw.len() as u32),
                    value: Value::Raw(raw),
                });
            }
        }
        let mut fragments = Vec::new();
        for frame in frames {
            let mut rest = frame.as_slice();
            while !rest.is_empty() {
                let n = fragment_length(rest.len() as u64, cap) as usize;
                fragments.push(rest[..n].to_vec());
                rest = &rest[n..];
            }
        }
        elements.push(Element {
            header: ElementHeader::new(Tag::PIXEL_DATA, Vr::OB, UNDEFINED_LENGTH),
            value: Value::Fragments {
                offset_table: layout.basic,
                fragments,
            },
        });
    } else {
        if opts.max_fragment_bytes != 0
            || opts.offset_table != offset_table::AUTO
            || opts.max_fragments < 0
        {
            return Err(source_error("encapsulation options on native output"));
        }
        let frame_length = first as i64;
        let mut packed = None;
        let total = if let Some(declared_bits) = source.packed_bits {
            let bits = native_packed_frame_bits(&file.dataset)?;
            for (i, frame) in frames.iter().enumerate() {
                let declared = declared_bits
                    .get(i)
                    .or(declared_bits.last())
                    .copied()
                    .unwrap_or(0);
                if declared != bits || frame.len() as i64 != (bits + 7) / 8 {
                    return Err(source_error(format!(
                        "inconsistent packed bit frame {}",
                        i + 1
                    )));
                }
            }
            if bits > (i64::from(u32::MAX) - 1) * 8 / count {
                return Err(source_error(
                    "native packed Pixel Data exceeds 32-bit length",
                ));
            }
            packed = Some(bits);
            (bits * count + 7) / 8
        } else {
            if frame_length > (i64::from(u32::MAX) - 1) / count {
                return Err(source_error("native Pixel Data exceeds 32-bit length"));
            }
            frame_length * count
        };
        let pad = total % 2 == 1;
        let length = (total + total % 2) as u32;
        let previous = file.dataset.get(Tag::PIXEL_DATA).map(Element::vr);
        let mut vr = Vr::OB;
        if previous == Some(Vr::OW) {
            vr = Vr::OW;
        } else if previous != Some(Vr::OB)
            && let Ok(bits) = file.dataset.get_ints(TAG_BITS_ALLOCATED)
            && bits.len() == 1
            && bits[0] > 8
        {
            vr = Vr::OW;
        }
        let raw = match packed {
            Some(bits) => match pack_bits(frames, bits, pad) {
                Ok(raw) => raw,
                Err(err) => {
                    value_error = Some(writer::frame_value_error(vr, length, err));
                    vec![0; length as usize]
                }
            },
            None => {
                let mut raw = frames.concat();
                if pad {
                    raw.push(0);
                }
                raw
            }
        };
        elements.push(Element {
            header: ElementHeader::new(Tag::PIXEL_DATA, vr, length),
            value: Value::Raw(raw),
        });
    }
    let mut dataset =
        Object::from_elements_with_text_options(elements, file.dataset.text_options().clone());
    dataset.set_value_big_endian(value_big_endian);
    dataset.value_provider = file.dataset.value_provider.clone();
    dataset.ambiguous_deferred = file.dataset.ambiguous_deferred.clone();
    dataset.pixel_data_in_destination_order = true;
    let framed = File {
        preamble: file.preamble.clone(),
        meta: file.meta.clone(),
        dataset,
        transfer_syntax: file.transfer_syntax,
        partial_read: None,
        incomplete_pixel_data: None,
    };
    let out = writer::write_file_with_options(&framed, &opts.write)?;
    match value_error {
        Some(err) => Err(err),
        None => Ok(out),
    }
}
