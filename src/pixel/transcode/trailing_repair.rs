use super::path::{Staged, Txn, begin_bounded, sync_directory};
use super::{Error as TranscodeError, TAG_DOUBLE_FLOAT_PIXEL_DATA, TAG_FLOAT_PIXEL_DATA};
use super::{TAG_PIXEL_DATA, TranscodeLimits};
use crate::net::ul::{Ctx, Kind as CtxKind};
use crate::object::deferred::{DeferOptions, read_file_deferred};
use crate::object::error::{Error as ObjectError, Kind, Op, PathStep};
use crate::object::parser::source::MemFile;
use crate::object::parser::{
    Disposition, ReaderOptions, SelectiveReader, SelectiveToken, TokenKind,
};
use crate::object::read_part10_header_mem;
use crate::object::uniform_validation::validate_part10_identity;
use crate::object::{ElementHeader, File, Part10Header, ReadFileOptions, Tag, Vr};
use crate::pixel::pixeldata::{extract_metadata, native_packed_byte_count, total_size};
use std::cell::RefCell;
use std::collections::HashSet;
use std::fmt;
use std::fs;
use std::rc::Rc;
pub const ERR_TRAILING_DATA_NOT_REPAIRABLE: &str =
    "dicom: no repairable trailing data after complete Pixel Data";
pub const ERR_TRAILING_DATA_REPAIR_LIMIT: &str = "dicom: trailing data repair resource limit";
const REPAIR_INPUT_LIMIT: i64 = 2 << 30;
const REPAIR_METADATA_LIMIT: i64 = 64 << 20;
const REPAIR_ELEMENT_LIMIT: i64 = 4 << 20;
const REPAIR_MAX_ELEMENTS: i64 = 100_000;
const REPAIR_MAX_FRAGMENTS: i64 = 100_000;
const REPAIR_MAX_DEPTH: i64 = 64;
const REPAIR_MAX_TOKENS: i64 = 500_000;
const LIMIT_TEXTS: [&str; 7] = [
    "dicom: maximum element count exceeded",
    "dicom: maximum element value size exceeded",
    "dicom: maximum Pixel Data size exceeded",
    "dicom: maximum sequence nesting depth exceeded",
    "dicom: maximum fragment count exceeded",
    "dicom: maximum total bytes exceeded",
    "dicom: selective reader token limit exceeded",
];
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TrailingDataRepairOptions {
    pub max_input_bytes: i64,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TrailingDataRepairReport {
    pub kept_bytes: i64,
    pub removed_bytes: i64,
    pub published: bool,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrailingRepairError {
    NotRepairable { size_mismatch: bool },
    Limit,
    Exists,
    Context(CtxKind),
    Transaction(TranscodeError),
    NotDurable(TrailingDataRepairReport, String),
    Io(String),
}
impl TrailingRepairError {
    pub fn is_not_repairable(&self) -> bool {
        matches!(self, TrailingRepairError::NotRepairable { .. })
    }
}
impl fmt::Display for TrailingRepairError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TrailingRepairError::NotRepairable {
                size_mismatch: false,
            } => f.write_str(ERR_TRAILING_DATA_NOT_REPAIRABLE),
            TrailingRepairError::NotRepairable {
                size_mismatch: true,
            } => write!(
                f,
                "{ERR_TRAILING_DATA_NOT_REPAIRABLE}: dicom: pixel data size does not match metadata"
            ),
            TrailingRepairError::Limit => f.write_str(ERR_TRAILING_DATA_REPAIR_LIMIT),
            TrailingRepairError::Exists => f.write_str("file already exists"),
            TrailingRepairError::Context(CtxKind::DeadlineExceeded) => {
                f.write_str("context deadline exceeded")
            }
            TrailingRepairError::Context(_) => f.write_str("context canceled"),
            TrailingRepairError::Transaction(e) => write!(f, "{e}"),
            TrailingRepairError::NotDurable(_, e) | TrailingRepairError::Io(e) => f.write_str(e),
        }
    }
}
impl std::error::Error for TrailingRepairError {}
type Res<T> = Result<T, TrailingRepairError>;
const NOT_REPAIRABLE: TrailingRepairError = TrailingRepairError::NotRepairable {
    size_mismatch: false,
};
fn ctx_check(ctx: &Ctx) -> Res<()> {
    match ctx.err() {
        Some(kind) => Err(TrailingRepairError::Context(kind)),
        None => Ok(()),
    }
}
fn read_failure(ctx: &Ctx, err: Option<&ObjectError>) -> TrailingRepairError {
    if let Some(kind) = ctx.err() {
        return TrailingRepairError::Context(kind);
    }
    if let Some(err) = err {
        let text = err.to_string();
        if err.is(Kind::MaxTotalBytes)
            || text.contains(ERR_TRAILING_DATA_REPAIR_LIMIT)
            || LIMIT_TEXTS.iter().any(|t| text.contains(t))
        {
            return TrailingRepairError::Limit;
        }
    }
    NOT_REPAIRABLE
}
pub fn repair_part10_trailing_data(
    ctx: &Ctx,
    source_path: &str,
    destination_path: &str,
    options: TrailingDataRepairOptions,
) -> Res<TrailingDataRepairReport> {
    ctx_check(ctx)?;
    let mut limit = options.max_input_bytes;
    if !(0..=REPAIR_INPUT_LIMIT).contains(&limit) {
        return Err(TrailingRepairError::Limit);
    }
    if limit == 0 {
        limit = REPAIR_INPUT_LIMIT;
    }
    let mut txn = begin_bounded(source_path, destination_path, Some(limit as u64))
        .map_err(TrailingRepairError::Transaction)?;
    if txn.previous.is_some() {
        return Err(TrailingRepairError::Exists);
    }
    let size = txn.source.len as i64;
    if size > limit {
        return Err(TrailingRepairError::Limit);
    }
    let read_options = ReadFileOptions {
        max_total_bytes: limit,
        max_element_bytes: REPAIR_ELEMENT_LIMIT,
        max_pixel_data_bytes: limit,
        max_elements: REPAIR_MAX_ELEMENTS,
        max_fragments: REPAIR_MAX_FRAGMENTS,
        max_sequence_depth: REPAIR_MAX_DEPTH,
        skip_pixel_data: true,
        strict_reserved_bytes: true,
        ..ReadFileOptions::default()
    };
    let source: Rc<[u8]> = Rc::from(std::mem::take(&mut txn.source_bytes));
    let intact = find_boundary(ctx, &source, size, &read_options)?;
    ctx_check(ctx)?;
    let Some(prefix) = source.get(..intact as usize).map(<[u8]>::to_vec) else {
        return Err(NOT_REPAIRABLE);
    };
    drop(source);
    let limits = TranscodeLimits {
        max_output_bytes: intact,
        ..TranscodeLimits::default()
    };
    let staged = txn
        .stage(Ok(prefix), &limits)
        .map_err(TrailingRepairError::Transaction)?;
    let defer = DeferOptions {
        defer_pixel_data: true,
        ..DeferOptions::default()
    };
    let file = read_file_deferred(&staged.bytes, true, read_options, &defer)
        .map_err(|(_, err)| read_failure(ctx, Some(&err)))?;
    verify_prefix(&file, intact)?;
    txn.precommit(&staged)
        .map_err(TrailingRepairError::Transaction)?;
    match fs::metadata(&staged.path) {
        Ok(meta) if meta.len() as i64 == intact => {}
        Ok(_) => return Err(NOT_REPAIRABLE),
        Err(e) => return Err(TrailingRepairError::Io(e.to_string())),
    }
    ctx_check(ctx)?;
    publish_create_only(&txn, staged)?;
    let report = TrailingDataRepairReport {
        kept_bytes: intact,
        removed_bytes: size.saturating_sub(intact),
        published: true,
    };
    if let Err(e) = sync_directory(&txn.parent) {
        return Err(TrailingRepairError::NotDurable(report, e.to_string()));
    }
    Ok(report)
}
fn publish_create_only(txn: &Txn, staged: Staged) -> Res<()> {
    match fs::hard_link(&staged.path, &txn.destination) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Err(TrailingRepairError::Exists),
        Err(_) => {
            if fs::symlink_metadata(&txn.destination).is_ok() {
                return Err(TrailingRepairError::Exists);
            }
            let mut staged = staged;
            fs::rename(&staged.path, &txn.destination)
                .map_err(|e| TrailingRepairError::Io(e.to_string()))?;
            staged.published = true;
            Ok(())
        }
    }
}
struct BoundaryState {
    pixel_offset: i64,
    seen_root: HashSet<Tag>,
}
fn find_boundary(ctx: &Ctx, data: &Rc<[u8]>, size: i64, opts: &ReadFileOptions) -> Res<i64> {
    let file = MemFile::new(data.clone());
    let header = read_part10_header_mem(&file, opts).map_err(|e| read_failure(ctx, Some(&e)))?;
    if header.transfer_syntax.deflated {
        return Err(NOT_REPAIRABLE);
    }
    let syntax = header.transfer_syntax;
    let parser_options = ReaderOptions {
        base_offset: header.data_set_offset,
        strict_reserved_bytes: true,
        max_total_bytes: opts.max_total_bytes,
        max_element_bytes: opts.max_element_bytes,
        max_pixel_data_bytes: opts.max_pixel_data_bytes,
        max_elements: opts.max_elements,
        max_fragments: opts.max_fragments,
        max_sequence_depth: opts.max_sequence_depth,
        skip_pixel_data: true,
        ..ReaderOptions::default()
    };
    let state = Rc::new(RefCell::new(BoundaryState {
        pixel_offset: 0,
        seen_root: HashSet::new(),
    }));
    let select_state = state.clone();
    let encapsulated = syntax.encapsulated;
    let select = Box::new(move |path: &[PathStep], h: &ElementHeader, offset: i64| {
        let mut st = select_state.borrow_mut();
        if path.len() == 1 {
            if h.tag.group < 8 || h.tag.group == 0xffff || !st.seen_root.insert(h.tag) {
                return Ok(Disposition::Stop);
            }
            if h.tag == TAG_PIXEL_DATA {
                if h.is_undefined() != encapsulated
                    || (h.vr != Vr::OB && h.vr != Vr::OW)
                    || (encapsulated && h.vr != Vr::OB)
                {
                    return Ok(Disposition::Stop);
                }
                st.pixel_offset = offset;
                if encapsulated {
                    return Ok(Disposition::Materialize);
                }
            }
        }
        if h.vr == Vr::SQ || (h.vr == Vr::UN && h.is_undefined()) {
            return Ok(Disposition::Materialize);
        }
        Ok(Disposition::Skip)
    });
    let mut reader = SelectiveReader::with_context(
        ctx.clone(),
        Box::new(file),
        None,
        syntax,
        parser_options,
        select,
        REPAIR_MAX_TOKENS,
    )
    .map_err(|e| read_failure(ctx, Some(&e)))?;
    loop {
        let token = reader.next().map_err(|e| read_failure(ctx, Some(&e)))?;
        let token = match token {
            SelectiveToken::Stop { .. } => return Err(NOT_REPAIRABLE),
            SelectiveToken::Token(token) => token,
        };
        let pixel_offset = state.borrow().pixel_offset;
        if pixel_offset == 0 || token.offset != pixel_offset {
            continue;
        }
        if token.kind == TokenKind::StartPixelSequence {
            read_fragments(&mut reader).map_err(|e| match e {
                Some(e) => read_failure(ctx, Some(&e)),
                None => read_failure(ctx, None),
            })?;
        }
        let end = reader.position();
        if end >= size {
            return Err(NOT_REPAIRABLE);
        }
        check_tail(ctx, data, &header, end, size)?;
        return Ok(end);
    }
}
fn read_fragments(reader: &mut SelectiveReader) -> Result<(), Option<ObjectError>> {
    let mut items = 0usize;
    loop {
        let token = match reader.next().map_err(Some)? {
            SelectiveToken::Token(token) => token,
            SelectiveToken::Stop { .. } => return Err(None),
        };
        let (tag, length) = token
            .header
            .map_or((Tag::default(), 0), |h| (h.tag, h.length));
        match token.kind {
            TokenKind::Element => {
                if !tag.is_item()
                    || (items == 0 && length % 4 != 0)
                    || (items > 0 && (length == 0 || length % 2 != 0))
                {
                    return Err(None);
                }
            }
            TokenKind::EndSequence => {
                if items >= 2 && tag.is_sequence_delimitation() {
                    return Ok(());
                }
                return Err(None);
            }
            _ => return Err(None),
        }
        items = items.saturating_add(1);
    }
}
fn check_tail(ctx: &Ctx, data: &[u8], header: &Part10Header, end: i64, size: i64) -> Res<()> {
    let start = end as usize;
    let tail = size.saturating_sub(end).clamp(0, 12) as usize;
    let span = data.get(start..start.saturating_add(tail)).unwrap_or(&[]);
    let syntax = header.transfer_syntax;
    if let Some(&pair) = span.first_chunk::<2>() {
        let group = if syntax.big_endian {
            u16::from_be_bytes(pair)
        } else {
            u16::from_le_bytes(pair)
        };
        if group < 8 || group == 0xffff {
            return ctx_check(ctx);
        }
    }
    let len = span.len() as i64;
    let mut reader = SelectiveReader::with_context(
        ctx.clone(),
        Box::new(MemFile::new(Rc::from(span))),
        Some((len, 0)),
        syntax,
        ReaderOptions {
            strict_reserved_bytes: true,
            ..ReaderOptions::default()
        },
        Box::new(|_: &[PathStep], _: &ElementHeader, _: i64| Ok(Disposition::Stop)),
        0,
    )
    .map_err(|e| match ctx.err() {
        Some(kind) => TrailingRepairError::Context(kind),
        None => TrailingRepairError::Io(e.to_string()),
    })?;
    let err = reader.next().err();
    if let Some(e) = &err
        && let Some(info) = e.parse_info()
        && info.tag != TAG_PIXEL_DATA
        && (e.is(Kind::UnexpectedEof)
            || info.op == Op::ReadVr
            || e.to_string()
                .contains("dicom: explicit VR long-form reserved bytes must be zero"))
    {
        return Ok(());
    }
    Err(read_failure(ctx, err.as_ref()))
}
fn verify_prefix(file: &File, end: i64) -> Res<()> {
    if validate_part10_identity(file).has_errors() {
        return Err(NOT_REPAIRABLE);
    }
    let dataset = &file.dataset;
    let Some(pixel) = dataset.get(TAG_PIXEL_DATA) else {
        return Err(NOT_REPAIRABLE);
    };
    if dataset.has(TAG_FLOAT_PIXEL_DATA)
        || dataset.has(TAG_DOUBLE_FLOAT_PIXEL_DATA)
        || dataset
            .check_pixel_data_encoding(&file.transfer_syntax)
            .is_err()
    {
        return Err(NOT_REPAIRABLE);
    }
    let mut root_end = -1i64;
    let mut root_length = 0i64;
    for location in file.value_locations(TAG_PIXEL_DATA) {
        if location.item_offset.is_some() {
            continue;
        }
        if root_end != -1 || location.length <= 0 {
            return Err(NOT_REPAIRABLE);
        }
        root_end = location.value_offset.saturating_add(location.length);
        root_length = location.length;
    }
    if root_end != end {
        return Err(NOT_REPAIRABLE);
    }
    if end.saturating_sub(root_length) > REPAIR_METADATA_LIMIT {
        return Err(TrailingRepairError::Limit);
    }
    if file.transfer_syntax.encapsulated {
        return Ok(());
    }
    let Ok(m) = extract_metadata(dataset) else {
        return Err(NOT_REPAIRABLE);
    };
    if m.rows == 0
        || m.columns == 0
        || m.samples_per_pixel == 0
        || m.bits_allocated != 1
            && (m.bits_allocated == 0 || m.bits_allocated % 8 != 0 || m.bits_allocated > 64)
        || m.bits_stored == 0
        || m.bits_stored > m.bits_allocated
        || m.high_bit >= m.bits_allocated
        || m.high_bit < m.bits_stored.saturating_sub(1)
        || m.pixel_representation > 1
    {
        return Err(NOT_REPAIRABLE);
    }
    let mut length = total_size(&m);
    if m.bits_allocated == 1 {
        length = native_packed_byte_count(&m);
    }
    if length <= 0
        || length == i64::MAX
        || i64::from(pixel.header.length) != length.saturating_add(length % 2)
    {
        return Err(TrailingRepairError::NotRepairable {
            size_mismatch: true,
        });
    }
    Ok(())
}
