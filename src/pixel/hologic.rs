#[cfg(feature = "jpeg-ls")]
use crate::codec::jpegls;
use crate::net::ul::{Ctx, Kind as CtxKind};
use crate::object::parser::BulkDataPathStep;
use crate::object::transfer::EXPLICIT_VR_LITTLE_ENDIAN;
use crate::object::{File, Object, Tag, Value, Vr};
use crate::pixel::Metadata;
use std::fmt;
use std::sync::{Arc, Mutex};
pub const ERR_NOT_PROFILE: &str = "hologic: not the supported private JPEG-LS profile";
pub const ERR_INVALID_PROFILE: &str = "hologic: invalid private JPEG-LS profile";
const CREATOR_TAG: Tag = Tag::new(0x7e01, 0x0010);
const SEQUENCE_TAG: Tag = Tag::new(0x7e01, 0x1010);
const BLOCK_TAG: Tag = Tag::new(0x7e01, 0x1012);
const PIXEL_DATA: Tag = Tag::new(0x7fe0, 0x0010);
const FLOAT_PIXEL: Tag = Tag::new(0x7fe0, 0x0008);
const DOUBLE_FLOAT_PIXEL: Tag = Tag::new(0x7fe0, 0x0009);
const CREATOR: &str = "HOLOGIC, Inc.";
const DESCRIPTOR_SIZE: i64 = 37;
const TRAILER_SIZE: i64 = 1024;
const MAX_BLOCKS: usize = 4096;
const MAX_STORAGE_BYTES: i64 = 512 << 20;
const MAX_COMPRESSED_FRAME: i64 = 64 << 20;
const MAX_NATIVE_FRAME: i64 = 64 << 20;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    NotProfile,
    InvalidProfile,
    Context(CtxKind),
    Replay,
    UnexpectedEof,
    Decode,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub kind: ErrorKind,
    text: String,
}
impl Error {
    fn invalid(detail: impl fmt::Display) -> Error {
        Error {
            kind: ErrorKind::InvalidProfile,
            text: format!("{ERR_INVALID_PROFILE}: {detail}"),
        }
    }
    fn context(kind: CtxKind) -> Error {
        let text = match kind {
            CtxKind::DeadlineExceeded => "context deadline exceeded",
            _ => "context canceled",
        };
        Error {
            kind: ErrorKind::Context(kind),
            text: text.to_string(),
        }
    }
    pub fn is_not_profile(&self) -> bool {
        self.kind == ErrorKind::NotProfile
    }
    pub fn is_invalid_profile(&self) -> bool {
        self.kind == ErrorKind::InvalidProfile
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}
impl std::error::Error for Error {}
fn ctx_err(ctx: &Ctx) -> Result<(), Error> {
    match ctx.err() {
        Some(kind) => Err(Error::context(kind)),
        None => Ok(()),
    }
}
pub fn block_value_tag() -> Tag {
    BLOCK_TAG
}
#[derive(Debug, Clone)]
struct Block {
    end: i64,
    path: [BulkDataPathStep; 2],
    data: Option<Vec<u8>>,
}
#[derive(Debug)]
pub struct Source {
    file: Arc<File>,
    metadata: Metadata,
    blocks: Vec<Block>,
    frames: Vec<(i64, i64)>,
    gate: Mutex<()>,
}
pub fn is_candidate(file: &File) -> bool {
    claims_creator(file) && file.dataset.has(SEQUENCE_TAG)
}
fn claims_creator(file: &File) -> bool {
    let dataset = &file.dataset;
    !dataset.has(PIXEL_DATA)
        && !dataset.has(FLOAT_PIXEL)
        && !dataset.has(DOUBLE_FLOAT_PIXEL)
        && is_creator(dataset)
}
fn is_creator(obj: &Object) -> bool {
    let Some(elem) = obj.get(CREATOR_TAG) else {
        return false;
    };
    if elem.vr() != Vr::LO || obj.has_duplicate_parsed_tag(CREATOR_TAG) {
        return false;
    }
    obj.get_strings(CREATOR_TAG)
        .is_some_and(|values| matches!(values.as_slice(), [v] if v == CREATOR))
}
pub fn open(ctx: &Ctx, file: &Arc<File>) -> Result<Source, Error> {
    ctx_err(ctx)?;
    if !claims_creator(file) {
        return Err(Error {
            kind: ErrorKind::NotProfile,
            text: ERR_NOT_PROFILE.to_string(),
        });
    }
    if file.transfer_syntax.uid != EXPLICIT_VR_LITTLE_ENDIAN.uid {
        return Err(Error::invalid("explicit little endian required"));
    }
    let mut source = Source {
        file: Arc::clone(file),
        metadata: Metadata::default(),
        blocks: Vec::new(),
        frames: Vec::new(),
        gate: Mutex::new(()),
    };
    source.load_blocks(ctx)?;
    source.load_descriptor(ctx)?;
    Ok(source)
}
impl Source {
    fn dataset(&self) -> &Object {
        &self.file.dataset
    }
    fn load_blocks(&mut self, ctx: &Ctx) -> Result<(), Error> {
        let dataset = &self.file.dataset;
        let well_formed = dataset.get(SEQUENCE_TAG).is_some_and(|e| {
            e.vr() == Vr::SQ
                && matches!(&e.value, Value::Sequence(items) if !items.is_empty() && items.len() <= MAX_BLOCKS)
        });
        if !well_formed || dataset.has_duplicate_parsed_tag(SEQUENCE_TAG) {
            return Err(Error::invalid(
                "missing, ambiguous or oversized block sequence",
            ));
        }
        let items = dataset.get_sequence(SEQUENCE_TAG).unwrap_or_default();
        let mut total = 0i64;
        let mut blocks = Vec::with_capacity(items.len());
        for (index, item) in items.iter().enumerate() {
            ctx_err(ctx)?;
            if item.has(CREATOR_TAG) && !is_creator(item) {
                return Err(Error::invalid(format!(
                    "item {index} reserves the block for another creator"
                )));
            }
            let element = item
                .get(BLOCK_TAG)
                .filter(|e| e.vr() == Vr::OB && !item.has_duplicate_parsed_tag(BLOCK_TAG));
            let Some(element) = element else {
                return Err(Error::invalid(format!(
                    "item {index} has no unique OB block"
                )));
            };
            let path = [
                BulkDataPathStep {
                    tag: SEQUENCE_TAG,
                    item_index: index as i64,
                },
                BulkDataPathStep {
                    tag: BLOCK_TAG,
                    item_index: -1,
                },
            ];
            let data = element.raw().map(<[u8]>::to_vec);
            let length = match &data {
                Some(data) => data.len() as i64,
                None => match dataset.deferred_bulk_data_reference(&path) {
                    Ok(Some(r)) if !r.reference.undefined_length => r.reference.length,
                    _ => {
                        return Err(Error::invalid(format!(
                            "item {index} has no bounded OB source"
                        )));
                    }
                },
            };
            if length <= 0 || length % 2 != 0 || length > MAX_STORAGE_BYTES.saturating_sub(total) {
                return Err(Error::invalid(format!(
                    "item {index} exceeds the stored value bounds"
                )));
            }
            total = total.saturating_add(length);
            blocks.push(Block {
                end: total,
                path,
                data,
            });
        }
        self.blocks = blocks;
        Ok(())
    }
    fn load_descriptor(&mut self, ctx: &Ctx) -> Result<(), Error> {
        let (Some(first), Some((last, before))) = (self.blocks.first(), self.blocks.split_last())
        else {
            return Err(Error::invalid("truncated descriptor or trailer"));
        };
        let total = last.end;
        let last_start = before.last().map_or(0, |b| b.end);
        let first_end = first.end;
        if first_end < DESCRIPTOR_SIZE || total.saturating_sub(last_start) < TRAILER_SIZE {
            return Err(Error::invalid("truncated descriptor or trailer"));
        }
        let mut header = [0u8; DESCRIPTOR_SIZE as usize];
        self.read_at(ctx, 0, &mut header)?;
        let le16 = |at: usize| {
            header
                .get(at..)
                .and_then(|h| h.first_chunk::<2>())
                .map_or(0, |&b| u16::from_le_bytes(b))
        };
        let count = usize::from(le16(20));
        let (width, height) = (le16(24), le16(28));
        let (precision, near) = (u16::from(header[32]), header[36]);
        if count < 1
            || count > (TRAILER_SIZE / 4 - 1) as usize
            || width == 0
            || height == 0
            || !(9..=16).contains(&precision)
            || near != 0
            || i64::from(width)
                .saturating_mul(i64::from(height))
                .saturating_mul(2)
                > MAX_NATIVE_FRAME
        {
            return Err(Error::invalid(
                "unsupported dimensions, frame count, precision or NEAR",
            ));
        }
        self.metadata = Metadata {
            rows: height,
            columns: width,
            number_of_frames: count as i64,
            samples_per_pixel: 1,
            bits_allocated: 16,
            bits_stored: precision,
            high_bit: precision.saturating_sub(1),
            photometric_interpretation: "MONOCHROME2".to_string(),
            ..Metadata::default()
        };
        check_metadata(self.dataset(), &self.metadata)?;
        let mut offsets = vec![0u8; count.saturating_add(1).saturating_mul(4)];
        let tail = total.saturating_sub(TRAILER_SIZE);
        self.read_at(ctx, tail, &mut offsets)?;
        let mut header_end = DESCRIPTOR_SIZE;
        let with_records = (count as i64).saturating_mul(24).saturating_add(40);
        if with_records <= first_end {
            header_end = with_records;
        }
        let le32 = |i: usize| {
            offsets
                .as_chunks::<4>()
                .0
                .get(i)
                .map_or(0, |&b| i64::from(u32::from_le_bytes(b)))
        };
        let mut start = le32(0);
        let mut frames = Vec::with_capacity(count);
        for i in 1..=count {
            let end = le32(i);
            if start < header_end
                || end <= start
                || end > tail
                || end.saturating_sub(start) > MAX_COMPRESSED_FRAME
            {
                return Err(Error::invalid(format!(
                    "frame {} range overlaps a descriptor, trailer or another frame, or exceeds its budget",
                    i.saturating_sub(1)
                )));
            }
            frames.push((start, end));
            start = end;
        }
        self.frames = frames;
        ctx_err(ctx)
    }
    pub fn metadata(&self) -> &Metadata {
        &self.metadata
    }
    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }
    pub fn frame(&self, ctx: &Ctx, index: i64) -> Result<Vec<u8>, Error> {
        ctx_err(ctx)?;
        let Some(&(start, end)) = usize::try_from(index).ok().and_then(|i| self.frames.get(i))
        else {
            return Err(Error::invalid(format!("invalid frame index {index}")));
        };
        let m = &self.metadata;
        let mut header: [u8; 25] = [
            0xff,
            0xd8,
            0xff,
            0xf7,
            0,
            11,
            m.bits_stored as u8,
            0,
            0,
            0,
            0,
            1,
            1,
            0x11,
            0,
            0xff,
            0xda,
            0,
            8,
            1,
            1,
            0,
            0,
            0,
            0,
        ];
        header[7..9].copy_from_slice(&m.rows.to_be_bytes());
        header[9..11].copy_from_slice(&m.columns.to_be_bytes());
        let entropy = end.saturating_sub(start) as usize;
        let mut stream = Vec::with_capacity(header.len().saturating_add(entropy).saturating_add(2));
        stream.extend_from_slice(&header);
        stream.resize(header.len().saturating_add(entropy), 0);
        if let Some(body) = stream.get_mut(header.len()..) {
            let _gate = self.gate.lock().unwrap_or_else(|e| e.into_inner());
            self.read_at(ctx, start, body)?;
        }
        check_entropy(ctx, stream.get(header.len()..).unwrap_or_default())?;
        stream.extend_from_slice(&[0xff, 0xd9]);
        decode_rebuilt(&stream, m, index)
    }
}
#[cfg(feature = "jpeg-ls")]
fn decode_rebuilt(stream: &[u8], m: &Metadata, index: i64) -> Result<Vec<u8>, Error> {
    let codec_metadata = jpegls::Metadata {
        rows: m.rows,
        columns: m.columns,
        samples_per_pixel: 1,
        bits_allocated: 16,
        bits_stored: m.bits_stored,
        high_bit: m.high_bit,
        pixel_representation: 0,
        number_of_frames: 1,
        photometric_interpretation: m.photometric_interpretation.clone(),
        ..jpegls::Metadata::default()
    };
    let decoded = jpegls::Codec::lossless()
        .decode_frames(&[stream], &codec_metadata, jpegls::DecodeOptions::default())
        .map_err(|e| Error {
            kind: ErrorKind::Decode,
            text: format!("hologic: decode frame {index}: {e}"),
        })?;
    Ok(decoded.data.into_iter().next().unwrap_or_default())
}
#[cfg(not(feature = "jpeg-ls"))]
fn decode_rebuilt(_stream: &[u8], _m: &Metadata, index: i64) -> Result<Vec<u8>, Error> {
    Err(Error {
        kind: ErrorKind::Decode,
        text: format!("hologic: decode frame {index}: JPEG-LS codec not built (feature jpeg-ls)"),
    })
}
impl Source {
    fn read_at(&self, ctx: &Ctx, mut start: i64, mut dst: &mut [u8]) -> Result<(), Error> {
        let mut i = self.blocks.partition_point(|b| b.end <= start);
        while !dst.is_empty() && i < self.blocks.len() {
            ctx_err(ctx)?;
            let Some(block) = self.blocks.get(i) else {
                break;
            };
            let block_start = i
                .checked_sub(1)
                .and_then(|p| self.blocks.get(p))
                .map_or(0, |b| b.end);
            let n = (dst.len() as i64).min(block.end.saturating_sub(start));
            let offset = start.saturating_sub(block_start);
            let (head, rest) = std::mem::take(&mut dst).split_at_mut(n as usize);
            match &block.data {
                Some(data) => {
                    let range = offset as usize..offset.saturating_add(n) as usize;
                    let Some(src) = data.get(range) else {
                        return Err(unexpected_eof());
                    };
                    head.copy_from_slice(src);
                }
                None => {
                    let mut buffer = Vec::with_capacity(n as usize);
                    let (_, err) =
                        self.dataset()
                            .copy_bulk_data_range_to(&block.path, offset, n, &mut buffer);
                    if let Some(err) = err {
                        return Err(Error {
                            kind: ErrorKind::Replay,
                            text: format!("hologic: replay private block: {err}"),
                        });
                    }
                    if buffer.len() != head.len() {
                        return Err(unexpected_eof());
                    }
                    head.copy_from_slice(&buffer);
                }
            }
            dst = rest;
            start = start.saturating_add(n);
            i = i.saturating_add(1);
        }
        if !dst.is_empty() {
            return Err(unexpected_eof());
        }
        ctx_err(ctx)
    }
}
fn unexpected_eof() -> Error {
    Error {
        kind: ErrorKind::UnexpectedEof,
        text: "unexpected EOF".to_string(),
    }
}
fn check_metadata(dataset: &Object, expected: &Metadata) -> Result<(), Error> {
    let entries = [
        (Tag::new(0x0028, 0x0010), i64::from(expected.rows)),
        (Tag::new(0x0028, 0x0011), i64::from(expected.columns)),
        (Tag::new(0x0028, 0x0008), expected.number_of_frames),
        (Tag::new(0x0028, 0x0002), 1),
        (Tag::new(0x0028, 0x0100), i64::from(expected.bits_allocated)),
        (Tag::new(0x0028, 0x0101), i64::from(expected.bits_stored)),
        (Tag::new(0x0028, 0x0102), i64::from(expected.high_bit)),
        (Tag::new(0x0028, 0x0103), 0),
    ];
    for (tag, want) in entries {
        if let Some(elem) = dataset.get(tag) {
            let vr = if tag == Tag::new(0x0028, 0x0008) {
                Vr::IS
            } else {
                Vr::US
            };
            let agrees = elem.vr() == vr
                && !dataset.has_duplicate_parsed_tag(tag)
                && dataset
                    .get_ints(tag)
                    .is_ok_and(|values| values.as_slice() == [want]);
            if !agrees {
                return Err(Error::invalid(format!(
                    "{tag} contradicts the private descriptor"
                )));
            }
        }
    }
    let photometric = Tag::new(0x0028, 0x0004);
    if let Some(elem) = dataset.get(photometric) {
        let agrees = elem.vr() == Vr::CS
            && !dataset.has_duplicate_parsed_tag(photometric)
            && dataset.get_strings(photometric).is_some_and(|values| {
                matches!(values.as_slice(), [v] if *v == expected.photometric_interpretation)
            });
        if !agrees {
            return Err(Error::invalid(
                "photometric interpretation contradicts the private descriptor",
            ));
        }
    }
    Ok(())
}
fn check_entropy(ctx: &Ctx, data: &[u8]) -> Result<(), Error> {
    const CHUNK: usize = 32 << 10;
    let mut start = 0;
    while start < data.len() {
        ctx_err(ctx)?;
        let end = start.saturating_add(CHUNK).min(data.len());
        while start < end {
            let Some(n) = data
                .get(start..end)
                .and_then(|chunk| chunk.iter().position(|b| *b == 0xff))
            else {
                break;
            };
            let position = start.saturating_add(n).saturating_add(1);
            if data.get(position).is_none_or(|&b| b & 0x80 != 0) {
                return Err(Error::invalid(
                    "a private entropy range contains a JPEG marker",
                ));
            }
            start = position.saturating_add(1);
        }
        start = start.max(end);
    }
    ctx_err(ctx)
}
