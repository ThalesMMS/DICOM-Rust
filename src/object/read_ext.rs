#![forbid(unsafe_code)]
use super::contextual::resolve_implicit_contextual_vrs;
use super::deferred;
use super::error::{Error, Kind, Result};
use super::file::{
    File, PartialRead, ReadFileOptions, incomplete_file_options, incomplete_partial,
    new_partial_read, read_part10_header, read_source_file, wrap_data_set_error,
};
use super::inflate::{self, Inflated};
use super::obj::Object;
use super::parser::source::{ByteRead, MemFile};
use super::parser::{
    BoundaryReason, BoundaryRecovery, DEFAULT_MAX_SEQUENCE_DEPTH, Disposition,
    ERR_DATA_SET_PROJECTION_OPTIONS, ReadStop, Reader, SelectiveReader,
};
use super::transfer::Syntax;
use super::types::{DataSet, Tag};
use std::rc::Rc;
pub const ERR_DISCARDED_VALUE: &str = "dicom: payload was discarded during streaming";
pub const ERR_READ_STOPPED: &str = "dicom: stopped at requested root header";
pub const ERR_READ_PREFIX_OPTIONS: &str = "dicom: invalid prefix read options";
pub const ERR_READ_PREFIX_REQUIRES_SEEKABLE: &str =
    "dicom: prefix File reading requires a seekable source";
pub const ERR_PREFIX_DEFERRAL_UNSUPPORTED: &str = "dicom-rust: a prefix read cannot defer values";
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct ReadExtras {
    pub stop_before_tag: Option<Tag>,
    pub prefix_max_tokens: i64,
    pub data_set_tags: Option<Vec<Tag>>,
    pub seek_unselected_sequences: bool,
    pub non_seekable: bool,
    pub defers_values: bool,
    pub inline_value_bytes_threshold: i64,
    pub max_deformable_vector_grid_bytes: i64,
    pub max_private_creators: i64,
    pub max_private_diagnostics: i64,
}
#[derive(Clone, Debug)]
pub struct ExtendedRead {
    pub file: File,
    pub stop: Option<ReadStop>,
}
pub(crate) type ExtendedResult =
    std::result::Result<ExtendedRead, (Option<Box<ExtendedRead>>, Error)>;
fn prefix_options_error(detail: &str) -> Error {
    Error::msg(format!("{ERR_READ_PREFIX_OPTIONS}: {detail}"))
}
pub(crate) fn read_stop_text(stop: &ReadStop, elements: i64) -> String {
    format!(
        "{ERR_READ_STOPPED}: retained {elements} complete elements before offset {} ({})",
        stop.offset, stop.header.tag
    )
}
pub fn is_read_stopped(err: &Error) -> bool {
    err.is(Kind::PartialRead) && err.to_string().starts_with(ERR_READ_STOPPED)
}
fn projection_read_options(opts: &ReadFileOptions, extras: &ReadExtras) -> Result<()> {
    let invalid = || Error::msg(ERR_DATA_SET_PROJECTION_OPTIONS);
    if extras.data_set_tags.is_none() {
        if extras.seek_unselected_sequences {
            return Err(invalid());
        }
        return Ok(());
    }
    if opts.allow_incomplete_pixel_data {
        return Err(Error::sentinel(Kind::IncompletePixelDataOptions));
    }
    if !opts.skip_pixel_data
        || opts.allow_partial_read
        || extras.stop_before_tag.is_some()
        || extras.prefix_max_tokens != 0
        || extras.defers_values
        || extras.inline_value_bytes_threshold != 0
    {
        return Err(invalid());
    }
    Ok(())
}
fn incomplete_read_options(opts: &ReadFileOptions, extras: &ReadExtras) -> Result<()> {
    incomplete_file_options(opts)?;
    if opts.allow_incomplete_pixel_data
        && (extras.stop_before_tag.is_some()
            || extras.data_set_tags.is_some()
            || extras.defers_values
            || extras.inline_value_bytes_threshold != 0)
    {
        return Err(Error::sentinel(Kind::IncompletePixelDataOptions));
    }
    Ok(())
}
fn reject_prefix_options(extras: &ReadExtras) -> Result<()> {
    if extras.stop_before_tag.is_some() || extras.prefix_max_tokens != 0 {
        return Err(prefix_options_error(
            "this entrypoint requires a complete dataset",
        ));
    }
    Ok(())
}
fn normalize_prefix_options(
    mut opts: ReadFileOptions,
    extras: &ReadExtras,
) -> Result<(ReadFileOptions, i64)> {
    if extras.stop_before_tag.is_none() {
        if extras.prefix_max_tokens != 0 {
            return Err(prefix_options_error(
                "PrefixMaxTokens requires StopBeforeTag",
            ));
        }
        return Ok((opts, 0));
    }
    if opts.allow_partial_read {
        return Err(prefix_options_error(
            "sinks and AllowPartialRead cannot accompany StopBeforeTag",
        ));
    }
    if extras.prefix_max_tokens < 0
        || opts.max_sequence_depth < 0
        || opts.max_elements < 0
        || opts.max_fragments < 0
        || opts.max_total_bytes < 0
        || opts.max_element_bytes < 0
        || opts.max_pixel_data_bytes < 0
        || extras.max_deformable_vector_grid_bytes < 0
        || extras.inline_value_bytes_threshold < 0
        || extras.max_private_creators < 0
        || extras.max_private_diagnostics < 0
        || opts.max_boundary_recoveries < 0
        || opts.max_implicit_vr_elements < 0
        || opts.max_delimiter_recoveries < 0
    {
        return Err(prefix_options_error(
            "prefix limits must be finite and nonnegative",
        ));
    }
    if opts.max_total_bytes == 0 {
        opts.max_total_bytes = 512 << 20;
    }
    if opts.max_element_bytes == 0 {
        opts.max_element_bytes = 1 << 20;
    }
    if opts.max_elements == 0 {
        opts.max_elements = 200_000;
    }
    if opts.max_fragments == 0 {
        opts.max_fragments = 200_000;
    }
    if opts.max_sequence_depth == 0 {
        opts.max_sequence_depth = DEFAULT_MAX_SEQUENCE_DEPTH;
    }
    let mut max_tokens = extras.prefix_max_tokens;
    if max_tokens == 0 {
        max_tokens = 1_000_000;
    }
    Ok((opts, max_tokens))
}
fn prefix_reader(
    source: Box<dyn ByteRead>,
    sized: Option<(i64, i64)>,
    syntax: Syntax,
    opts: &ReadFileOptions,
    base_offset: i64,
    threshold: Tag,
    max_tokens: i64,
) -> Result<SelectiveReader> {
    SelectiveReader::new(
        source,
        sized,
        syntax,
        opts.parser_reader_options(base_offset),
        Box::new(move |path, header, _| {
            if path.len() == 1 && header.tag >= threshold {
                Ok(Disposition::Stop)
            } else {
                Ok(Disposition::Materialize)
            }
        }),
        max_tokens,
    )
}
fn zlib_recovery(offset: i64) -> BoundaryRecovery {
    let mut zlib = BoundaryRecovery::new(offset, -1, -1, BoundaryReason::ZlibDeflate);
    zlib.path = "(dataset)".to_string();
    zlib
}
fn finish_object(
    mut dataset: DataSet,
    syntax: Syntax,
    discarded: Vec<Tag>,
    recoveries: (Vec<BoundaryRecovery>, bool),
    zlib_offset: Option<i64>,
) -> Object {
    if !syntax.explicit_vr {
        resolve_implicit_contextual_vrs(&mut dataset, syntax.big_endian);
    }
    let parsed_tags: Vec<Tag> = dataset.elements.iter().map(|e| e.tag()).collect();
    let mut obj = Object::from_elements(dataset.elements);
    obj.ambiguous_deferred = deferred::duplicate_tags(&parsed_tags);
    obj.set_value_big_endian(syntax.big_endian);
    obj.parsed_pixel_data = obj.has(Tag::PIXEL_DATA);
    obj.discarded = discarded;
    obj.boundary_recoveries = recoveries.0;
    obj.boundary_recoveries_truncated = recoveries.1;
    if let Some(offset) = zlib_offset {
        obj.boundary_recoveries.insert(0, zlib_recovery(offset));
    }
    obj
}
#[doc(hidden)]
pub fn read_file_extended(
    data: &[u8],
    opts: ReadFileOptions,
    extras: &ReadExtras,
) -> ExtendedResult {
    let fail = |e: Error| (None, e);
    projection_read_options(&opts, extras).map_err(fail)?;
    incomplete_read_options(&opts, extras).map_err(fail)?;
    let (opts, max_tokens) = normalize_prefix_options(opts, extras).map_err(fail)?;
    if extras.stop_before_tag.is_some() {
        if extras.non_seekable {
            return Err(fail(Error::msg(ERR_READ_PREFIX_REQUIRES_SEEKABLE)));
        }
        if extras.defers_values || extras.inline_value_bytes_threshold != 0 {
            return Err(fail(Error::msg(ERR_PREFIX_DEFERRAL_UNSUPPORTED)));
        }
    }
    let bytes: Rc<[u8]> = Rc::from(data);
    let file = MemFile::new(bytes);
    let (preamble, meta, syntax, dataset_offset) =
        read_part10_header(&file, &opts).map_err(fail)?;
    let mut zlib_wrapped = false;
    let (reader_source, sized): (Box<dyn ByteRead>, Option<(i64, i64)>) = if syntax.deflated {
        if opts.allow_incomplete_pixel_data {
            return Err(fail(wrap_data_set_error(Error::sentinel(
                Kind::IncompletePixelDataOptions,
            ))));
        }
        let rest = file.remaining();
        let (decoded, err) = if opts.allow_zlib_deflate && inflate::is_zlib_header(rest) {
            zlib_wrapped = true;
            inflate::inflate_zlib_limited(rest, opts.limits.max_inflated_bytes)
        } else {
            let (decoded, err, _) =
                inflate::inflate_raw_limited(rest, opts.limits.max_inflated_bytes);
            (decoded, err)
        };
        (Box::new(Inflated::new(decoded, err)), None)
    } else {
        (
            Box::new(file.clone()),
            Some((file.len() as i64, file.position() as i64)),
        )
    };
    let zlib_offset = zlib_wrapped.then_some(dataset_offset);
    if let Some(threshold) = extras.stop_before_tag {
        let mut reader = prefix_reader(
            reader_source,
            sized,
            syntax,
            &opts,
            dataset_offset,
            threshold,
            max_tokens,
        )
        .map_err(|e| fail(wrap_data_set_error(e)))?;
        let (dataset, stop) = reader
            .read_data_set_prefix()
            .map_err(|e| fail(wrap_data_set_error(e)))?;
        let elements = dataset.elements.len() as i64;
        let obj = finish_object(
            dataset,
            syntax,
            Vec::new(),
            reader.boundary_recoveries(),
            zlib_offset,
        );
        let mut result = ExtendedRead {
            file: File {
                preamble,
                meta,
                dataset: obj,
                transfer_syntax: syntax,
                partial_read: None,
                incomplete_pixel_data: None,
            },
            stop,
        };
        let Some(stop) = stop else {
            return Ok(result);
        };
        let err = Error::new_kind(Kind::PartialRead, read_stop_text(&stop, elements));
        result.file.partial_read = Some(PartialRead {
            offset: stop.offset,
            tag: stop.header.tag,
            elements,
            retained_elements: elements,
            path: Vec::new(),
            containers: Vec::new(),
            err: err.clone(),
        });
        return Err((Some(Box::new(result)), err));
    }
    let mut reader = Reader::new(
        reader_source,
        sized,
        syntax,
        opts.parser_reader_options(dataset_offset),
    );
    let mut partial = None;
    let mut discarded = Vec::new();
    let dataset;
    if opts.allow_partial_read {
        let (ds, err) = reader.read_data_set_partial();
        dataset = ds;
        if let Some(err) = err {
            partial = Some(new_partial_read(
                wrap_data_set_error(err),
                dataset.elements.len() as i64,
            ));
        }
    } else if let Some(tags) = &extras.data_set_tags {
        let (projected, err) =
            reader.read_data_set_projected(tags, extras.seek_unselected_sequences);
        if let Some(err) = err {
            return Err(fail(wrap_data_set_error(err)));
        }
        dataset = projected.data_set;
        discarded = projected.discarded;
    } else {
        let (ds, err) = reader.read_data_set();
        dataset = ds;
        if let Some(err) = err {
            if !opts.allow_incomplete_pixel_data || !err.is(Kind::IncompletePixelData) {
                return Err(fail(wrap_data_set_error(err)));
            }
            let Some(info) = reader.incomplete_pixel_data() else {
                return Err(fail(wrap_data_set_error(err)));
            };
            partial = Some(incomplete_partial(&info, dataset.elements.len() as i64));
        }
    }
    let incomplete = reader.incomplete_pixel_data();
    let obj = finish_object(
        dataset,
        syntax,
        discarded,
        reader.boundary_recoveries(),
        zlib_offset,
    );
    let mut result = ExtendedRead {
        file: File {
            preamble,
            meta,
            dataset: obj,
            transfer_syntax: syntax,
            partial_read: None,
            incomplete_pixel_data: None,
        },
        stop: None,
    };
    if let Some(partial) = partial {
        let err = Error::new_kind(Kind::PartialRead, partial.error_text());
        result.file.partial_read = Some(partial);
        result.file.incomplete_pixel_data = incomplete;
        return Err((Some(Box::new(result)), err));
    }
    Ok(result)
}
#[doc(hidden)]
pub fn read_file_prefix_deferred(
    data: &[u8],
    seekable: bool,
    opts: ReadFileOptions,
    extras: &ReadExtras,
    defer: &deferred::DeferOptions,
) -> ExtendedResult {
    let fail = |e: Error| (None, e);
    let mut extras = extras.clone();
    extras.defers_values = deferred::requires_seekable(defer);
    extras.inline_value_bytes_threshold = defer.inline_value_bytes_threshold;
    extras.non_seekable = !seekable;
    projection_read_options(&opts, &extras).map_err(fail)?;
    incomplete_read_options(&opts, &extras).map_err(fail)?;
    let (opts, max_tokens) = normalize_prefix_options(opts, &extras).map_err(fail)?;
    let Some(threshold) = extras.stop_before_tag else {
        if extras.data_set_tags.is_some() {
            return read_file_extended(data, opts, &extras);
        }
        return match deferred::read_file_deferred(data, seekable, opts, defer) {
            Ok(file) => Ok(ExtendedRead { file, stop: None }),
            Err((file, err)) => Err((
                file.map(|file| {
                    Box::new(ExtendedRead {
                        file: *file,
                        stop: None,
                    })
                }),
                err,
            )),
        };
    };
    if !seekable {
        return Err(fail(Error::msg(ERR_READ_PREFIX_REQUIRES_SEEKABLE)));
    }
    let bytes: Rc<[u8]> = Rc::from(data);
    let file = MemFile::new(bytes);
    let (preamble, meta, syntax, dataset_offset) =
        read_part10_header(&file, &opts).map_err(fail)?;
    let mut defer = defer.clone();
    let mut zlib_wrapped = false;
    let (reader_source, sized): (Box<dyn ByteRead>, Option<(i64, i64)>) = if syntax.deflated {
        if opts.allow_incomplete_pixel_data {
            return Err(fail(wrap_data_set_error(Error::sentinel(
                Kind::IncompletePixelDataOptions,
            ))));
        }
        if deferred::requires_seekable(&defer) {
            return Err(fail(wrap_data_set_error(
                deferred::deflated_deferred_value_error(),
            )));
        }
        defer.inline_value_bytes_threshold = 0;
        let rest = file.remaining();
        let (decoded, err) = if opts.allow_zlib_deflate && inflate::is_zlib_header(rest) {
            zlib_wrapped = true;
            inflate::inflate_zlib_limited(rest, opts.limits.max_inflated_bytes)
        } else {
            let (decoded, err, _) =
                inflate::inflate_raw_limited(rest, opts.limits.max_inflated_bytes);
            (decoded, err)
        };
        (Box::new(Inflated::new(decoded, err)), None)
    } else {
        (
            Box::new(file.clone()),
            Some((file.len() as i64, file.position() as i64)),
        )
    };
    let zlib_offset = zlib_wrapped.then_some(dataset_offset);
    let mut reader = prefix_reader(
        reader_source,
        sized,
        syntax,
        &opts,
        dataset_offset,
        threshold,
        max_tokens,
    )
    .map_err(|e| fail(wrap_data_set_error(e)))?;
    reader.set_deferral(&defer);
    let (dataset, stop) = reader
        .read_data_set_prefix()
        .map_err(|e| fail(wrap_data_set_error(e)))?;
    let elements = dataset.elements.len() as i64;
    let mut obj = finish_object(
        dataset,
        syntax,
        Vec::new(),
        reader.boundary_recoveries(),
        zlib_offset,
    );
    if !syntax.deflated {
        let parsed_tags: Vec<Tag> = obj.elements().iter().map(|e| e.tag()).collect();
        let (stopped, tokens) = reader.selective_state();
        deferred::attach_value_provider_prefix(
            &mut obj,
            &parsed_tags,
            reader.reader(),
            data,
            syntax,
            opts.parser_reader_options(dataset_offset),
            &defer,
            Some(deferred::PrefixRescan {
                threshold,
                max_tokens,
                state: std::sync::Mutex::new((stopped, tokens)),
            }),
        );
    }
    let mut result = ExtendedRead {
        file: File {
            preamble,
            meta,
            dataset: obj,
            transfer_syntax: syntax,
            partial_read: None,
            incomplete_pixel_data: None,
        },
        stop,
    };
    let Some(stop) = stop else {
        return Ok(result);
    };
    let err = Error::new_kind(Kind::PartialRead, read_stop_text(&stop, elements));
    result.file.partial_read = Some(PartialRead {
        offset: stop.offset,
        tag: stop.header.tag,
        elements,
        retained_elements: elements,
        path: Vec::new(),
        containers: Vec::new(),
        err: err.clone(),
    });
    Err((Some(Box::new(result)), err))
}
#[doc(hidden)]
pub fn read_data_set_extended(
    data: &[u8],
    syntax: Syntax,
    opts: ReadFileOptions,
    extras: &ReadExtras,
) -> Result<Object> {
    projection_read_options(&opts, extras)?;
    incomplete_read_options(&opts, extras)?;
    reject_prefix_options(extras)?;
    let Some(tags) = &extras.data_set_tags else {
        return super::file::read_data_set_with_options(
            Box::new(MemFile::new(Rc::from(data))),
            syntax,
            opts,
        );
    };
    type Prepared = (Box<dyn ByteRead>, Option<(i64, i64)>, Option<i64>);
    let (source, sized, zlib_offset): Prepared = if syntax.deflated {
        let (decoded, err, zlib) = if opts.allow_zlib_deflate && inflate::is_zlib_header(data) {
            let (decoded, err) =
                inflate::inflate_zlib_limited(data, opts.limits.max_inflated_bytes);
            (decoded, err, Some(0))
        } else {
            let (decoded, err, _) =
                inflate::inflate_raw_limited(data, opts.limits.max_inflated_bytes);
            (decoded, err, None)
        };
        (Box::new(Inflated::new(decoded, err)), None, zlib)
    } else {
        let file = MemFile::new(Rc::from(data));
        let sized = (!extras.non_seekable).then_some((file.len() as i64, 0));
        (Box::new(file), sized, None)
    };
    let mut reader = Reader::new(source, sized, syntax, opts.parser_reader_options(0));
    let (projected, err) = reader.read_data_set_projected(tags, extras.seek_unselected_sequences);
    if let Some(err) = err {
        return Err(err);
    }
    Ok(finish_object(
        projected.data_set,
        syntax,
        projected.discarded,
        reader.boundary_recoveries(),
        zlib_offset,
    ))
}
pub(crate) fn open_file_extended(
    path: &str,
    opts: ReadFileOptions,
    extras: &ReadExtras,
) -> ExtendedResult {
    let fail = |e: Error| (None, e);
    projection_read_options(&opts, extras).map_err(fail)?;
    incomplete_read_options(&opts, extras).map_err(fail)?;
    normalize_prefix_options(opts, extras).map_err(fail)?;
    let data = read_source_file(path).map_err(fail)?;
    read_file_extended(&data, opts, extras)
}
pub fn open_files(
    paths: &[String],
    opts: ReadFileOptions,
    extras: &ReadExtras,
    _workers: i64,
) -> Result<Vec<ExtendedRead>> {
    let mut files = Vec::with_capacity(paths.len());
    for (index, path) in paths.iter().enumerate() {
        match open_file_extended(path, opts, extras) {
            Ok(file) => files.push(file),
            Err((_, err)) => {
                return Err(Error::wrap(
                    &format!("dicom: open input {index} ({path})"),
                    err,
                ));
            }
        }
    }
    Ok(files)
}
impl Object {
    #[doc(hidden)]
    pub fn discarded_tags(&self) -> &[Tag] {
        &self.discarded
    }
}
