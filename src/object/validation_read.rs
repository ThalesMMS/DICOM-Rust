#![forbid(unsafe_code)]
use super::contextual::resolve_implicit_contextual_vrs;
use super::deferred::{self, DeferOptions};
use super::error::{Error, Kind};
use super::file::{File, ReadFileOptions, read_part10_header};
use super::inflate::{self, Inflated};
use super::obj::Object;
use super::parser::source::{ByteRead, MemFile};
use super::parser::{
    BoundaryReason, BoundaryRecovery, ERR_DATA_SET_PROJECTION_OPTIONS, ValidatedReadError,
    ValidatedReader,
};
use super::read_ext::{ERR_READ_PREFIX_OPTIONS, ReadExtras};
use super::transfer::Syntax;
use super::types::{Tag, Vr};
use crate::validation::{
    HookChain, HookDecision, HookPoint, HookRegistration, Report, ValidationOptions,
};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
pub const ERR_METADATA_READ_LIMIT: &str = "dicom: Part 10 metadata exceeds resource limits";
pub(crate) type ValidatedRead = (Option<Object>, Report, Option<ValidatedReadError>);
fn fail(err: Error) -> ValidatedRead {
    (
        None,
        Report::default(),
        Some(ValidatedReadError::Parse(err)),
    )
}
pub fn read_data_set_with_validation(
    data: &[u8],
    seekable: bool,
    syntax: Syntax,
    read_opts: ReadFileOptions,
    extras: &ReadExtras,
    defer: Option<&DeferOptions>,
    validation_opts: ValidationOptions,
) -> ValidatedRead {
    if extras.stop_before_tag.is_some() || extras.prefix_max_tokens != 0 {
        return fail(Error::msg(format!(
            "{ERR_READ_PREFIX_OPTIONS}: this entrypoint requires a complete dataset"
        )));
    }
    if defer.is_some_and(deferred::requires_seekable) && !seekable {
        return fail(Error::msg(deferred::ERR_DEFERRED_VALUE_REQUIRES_SEEKABLE));
    }
    read_validated(
        data,
        syntax,
        read_opts,
        extras,
        defer,
        0,
        seekable,
        validation_opts,
        None,
    )
}
pub fn open_data_set_with_validation(
    path: impl AsRef<std::path::Path>,
    syntax: Syntax,
    read_opts: ReadFileOptions,
    extras: &ReadExtras,
    defer: Option<&DeferOptions>,
    validation_opts: ValidationOptions,
) -> ValidatedRead {
    let path = path.as_ref().to_string_lossy();
    let path: &str = &path;
    if extras.stop_before_tag.is_some() || extras.prefix_max_tokens != 0 {
        return fail(Error::msg(format!(
            "{ERR_READ_PREFIX_OPTIONS}: this entrypoint requires a complete dataset"
        )));
    }
    if path.contains('\0') {
        return fail(Error::msg(format!("open {path}: invalid argument")));
    }
    if std::fs::metadata(path).is_ok_and(|meta| meta.is_dir()) {
        return read_validated(
            &[],
            syntax,
            read_opts,
            extras,
            defer,
            0,
            true,
            validation_opts,
            Some(Error::msg(format!("read {path}: Incorrect function."))),
        );
    }
    let data = match super::file::read_source_file(path) {
        Ok(data) => data,
        Err(err) => return fail(err),
    };
    read_data_set_with_validation(
        &data,
        true,
        syntax,
        read_opts,
        extras,
        defer,
        validation_opts,
    )
}
struct Unreadable(Error);
impl ByteRead for Unreadable {
    fn read(&mut self, _p: &mut [u8]) -> (usize, Option<Error>) {
        (0, Some(self.0.clone()))
    }
}
#[allow(clippy::too_many_arguments)]
fn read_validated(
    data: &[u8],
    syntax: Syntax,
    opts: ReadFileOptions,
    extras: &ReadExtras,
    defer: Option<&DeferOptions>,
    base_offset: i64,
    mut stream_values: bool,
    validation_opts: ValidationOptions,
    unreadable: Option<Error>,
) -> ValidatedRead {
    if extras.data_set_tags.is_some() {
        return fail(Error::msg(ERR_DATA_SET_PROJECTION_OPTIONS));
    }
    let bytes: Rc<[u8]> = Rc::from(data);
    let mut zlib_wrapped = false;
    let (source, sized): (Box<dyn ByteRead>, Option<(i64, i64)>) = if syntax.deflated {
        if defer.is_some_and(deferred::requires_seekable) {
            return fail(deferred::deflated_deferred_value_error());
        }
        stream_values = false;
        if let Some(err) = unreadable {
            (Box::new(Unreadable(err)), None)
        } else {
            let (decoded, err) = if opts.allow_zlib_deflate && inflate::is_zlib_header(data) {
                zlib_wrapped = true;
                inflate::inflate_zlib_limited(data, opts.limits.max_inflated_bytes)
            } else {
                let (decoded, err, _) =
                    inflate::inflate_raw_limited(data, opts.limits.max_inflated_bytes);
                (decoded, err)
            };
            (Box::new(Inflated::new(decoded, err)), None)
        }
    } else if let Some(err) = unreadable {
        (Box::new(Unreadable(err)), None)
    } else if stream_values {
        let file = MemFile::new(bytes.clone());
        let sized = Some((file.len() as i64, 0));
        (Box::new(file), sized)
    } else {
        (Box::new(StreamOnly(MemFile::new(bytes.clone()))), None)
    };
    let reader_opts = opts.parser_reader_options(base_offset);
    let mut reader = match ValidatedReader::new(source, sized, syntax, reader_opts, validation_opts)
    {
        Ok(reader) => reader,
        Err(err) => {
            return (
                None,
                Report::default(),
                Some(ValidatedReadError::Validation(err)),
            );
        }
    };
    let mut defer_options = defer.cloned().unwrap_or_default();
    if !stream_values {
        defer_options.inline_value_bytes_threshold = 0;
    }
    if defer.is_some() {
        reader.set_deferral(&defer_options);
    }
    let (mut dataset, read_err) = reader.read_data_set();
    let report = reader.validation_report();
    if read_err.as_ref().is_some_and(|e| !e.is_validation_failed()) {
        return (None, report, read_err);
    }
    if !syntax.explicit_vr {
        resolve_implicit_contextual_vrs(&mut dataset, syntax.big_endian);
    }
    let parsed_tags: Vec<Tag> = dataset.elements.iter().map(|e| e.tag()).collect();
    let mut obj = Object::from_elements(dataset.elements);
    obj.ambiguous_deferred = deferred::duplicate_tags(&parsed_tags);
    obj.set_value_big_endian(syntax.big_endian);
    obj.parsed_pixel_data = obj.has(Tag::PIXEL_DATA);
    let (recoveries, truncated) = reader.boundary_recoveries();
    obj.boundary_recoveries = recoveries;
    obj.boundary_recoveries_truncated = truncated;
    if zlib_wrapped {
        let mut zlib = BoundaryRecovery::new(base_offset, -1, -1, BoundaryReason::ZlibDeflate);
        zlib.path = "(dataset)".to_string();
        obj.boundary_recoveries.insert(0, zlib);
    }
    if stream_values {
        deferred::attach_value_provider_validated(
            &mut obj,
            &parsed_tags,
            reader.reader(),
            &bytes,
            syntax,
            reader_opts,
            &defer_options,
        );
    }
    (Some(obj), report, read_err)
}
struct StreamOnly(MemFile);
impl ByteRead for StreamOnly {
    fn read(&mut self, p: &mut [u8]) -> (usize, Option<Error>) {
        self.0.read(p)
    }
}
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct MetadataReadLimits {
    pub source_bytes: i64,
    pub logical_bytes: i64,
    pub metadata_bytes: i64,
}
#[derive(Clone, Debug)]
pub enum MetadataReadError {
    Limit,
    LimitCause(ValidatedReadError),
    Read(ValidatedReadError),
}
impl std::fmt::Display for MetadataReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MetadataReadError::Limit => f.write_str(ERR_METADATA_READ_LIMIT),
            MetadataReadError::LimitCause(e) => write!(f, "{ERR_METADATA_READ_LIMIT}\n{e}"),
            MetadataReadError::Read(e) => e.fmt(f),
        }
    }
}
impl std::error::Error for MetadataReadError {}
impl MetadataReadError {
    pub fn is_limit(&self) -> bool {
        !matches!(self, MetadataReadError::Read(_))
    }
}
fn metadata_read_error(err: ValidatedReadError) -> MetadataReadError {
    let limit = match &err {
        ValidatedReadError::Parse(e) => {
            let text = e.to_string();
            e.is(Kind::MaxTotalBytes)
                || [
                    "dicom: maximum total bytes exceeded",
                    "dicom: maximum element value size exceeded",
                    "dicom: maximum Pixel Data size exceeded",
                    "dicom: maximum element count exceeded",
                    "dicom: maximum fragment count exceeded",
                    "dicom: maximum sequence nesting depth exceeded",
                ]
                .iter()
                .any(|sentinel| text.contains(sentinel))
        }
        _ => false,
    };
    if limit {
        MetadataReadError::LimitCause(err)
    } else {
        MetadataReadError::Read(err)
    }
}
pub fn read_file_metadata(
    source: &[u8],
    start: usize,
    mut opts: ReadFileOptions,
    extras: &ReadExtras,
    mut limits: MetadataReadLimits,
) -> Result<File, MetadataReadError> {
    let read = |e: Error| MetadataReadError::Read(ValidatedReadError::Parse(e));
    if limits.source_bytes <= 0 || limits.logical_bytes <= 0 || limits.metadata_bytes <= 0 {
        return Err(MetadataReadError::Limit);
    }
    if extras.stop_before_tag.is_some() || extras.prefix_max_tokens != 0 {
        return Err(read(Error::msg(format!(
            "{ERR_READ_PREFIX_OPTIONS}: this entrypoint requires a complete dataset"
        ))));
    }
    if extras.data_set_tags.is_some() || extras.seek_unselected_sequences {
        return Err(read(Error::msg(ERR_DATA_SET_PROJECTION_OPTIONS)));
    }
    if opts.allow_incomplete_pixel_data {
        return Err(read(Error::sentinel(Kind::IncompletePixelDataOptions)));
    }
    if extras.defers_values || extras.inline_value_bytes_threshold != 0 {
        return Err(read(Error::msg(
            "dicom: metadata-only read cannot defer or stream values",
        )));
    }
    if opts.max_total_bytes > 0 {
        limits.logical_bytes = limits.logical_bytes.min(opts.max_total_bytes);
    }
    let start = start.min(source.len());
    let section = &source[start..];
    if section.len() as i64 > limits.source_bytes {
        return Err(MetadataReadError::Limit);
    }
    opts.skip_pixel_data = true;
    opts.max_total_bytes = limits.metadata_bytes.min(limits.logical_bytes);
    opts.max_element_bytes = if opts.max_element_bytes <= 0 {
        limits.metadata_bytes
    } else {
        opts.max_element_bytes.min(limits.metadata_bytes)
    };
    opts.max_pixel_data_bytes = if opts.max_pixel_data_bytes <= 0 {
        limits.logical_bytes
    } else {
        opts.max_pixel_data_bytes.min(limits.logical_bytes)
    };
    if opts.max_elements <= 0 {
        opts.max_elements = 100_000;
    }
    if opts.max_fragments <= 0 {
        opts.max_fragments = 10_000;
    }
    if opts.max_sequence_depth <= 0 {
        opts.max_sequence_depth = 32;
    }
    let file = MemFile::new(Rc::from(section));
    let (preamble, meta, syntax, data_set_offset) = read_part10_header(&file, &opts)
        .map_err(|e| metadata_read_error(ValidatedReadError::Parse(e)))?;
    let remaining = limits.metadata_bytes - data_set_offset;
    let logical = limits.logical_bytes - data_set_offset;
    if remaining <= 0 || logical <= 0 {
        return Err(MetadataReadError::Limit);
    }
    let budget = Arc::new(AtomicI64::new(remaining));
    let refused = Arc::new(AtomicBool::new(false));
    let hook_budget = budget.clone();
    let hook_refused = refused.clone();
    let hooks = HookChain::new(vec![HookRegistration {
        name: "metadata-byte-budget".to_string(),
        points: vec![HookPoint::ElementHeaderRead],
        hook: Some(Arc::new(move |event| {
            let Some(h) = event.header else {
                return Ok(HookDecision::default());
            };
            let mut cost = 12i64;
            if h.vr != Vr::SQ
                && !h.is_undefined()
                && !matches!(
                    (h.tag.group, h.tag.element),
                    (0x7FE0, 0x0010) | (0x7FE0, 0x0008) | (0x7FE0, 0x0009)
                )
            {
                cost += i64::from(h.length);
            }
            let left = hook_budget.load(Ordering::Relaxed);
            if cost > left {
                hook_refused.store(true, Ordering::Relaxed);
                return Err(ERR_METADATA_READ_LIMIT.into());
            }
            hook_budget.store(left - cost, Ordering::Relaxed);
            Ok(HookDecision::default())
        })),
        ..HookRegistration::default()
    }])
    .map_err(|e| MetadataReadError::Read(ValidatedReadError::Validation(e)))?;
    opts.max_total_bytes = logical;
    let max_depth = opts.max_sequence_depth / 2 + opts.max_sequence_depth % 2;
    let validation_opts = ValidationOptions {
        hooks: Some(Arc::new(hooks)),
        max_elements: opts.max_elements,
        max_depth,
        max_findings: 32,
        ..ValidationOptions::default()
    };
    let data_set = &section[(data_set_offset as usize).min(section.len())..];
    let (dataset, _, err) = read_validated(
        data_set,
        syntax,
        opts,
        &ReadExtras::default(),
        None,
        0,
        true,
        validation_opts,
        None,
    );
    if let Some(err) = err {
        if refused.load(Ordering::Relaxed) {
            return Err(MetadataReadError::LimitCause(err));
        }
        return Err(metadata_read_error(err));
    }
    let Some(dataset) = dataset else {
        return Err(MetadataReadError::Limit);
    };
    Ok(File {
        preamble,
        meta,
        dataset,
        transfer_syntax: syntax,
        partial_read: None,
        incomplete_pixel_data: None,
    })
}
