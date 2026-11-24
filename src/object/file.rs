use super::contextual::resolve_implicit_contextual_vrs_chained;
use super::deferred::{self, DeferOptions};
use super::error::{Error, Kind, Op, PartialContainer, PathStep, Result, path_string};
use super::frame_sink::ReadSinks;
use super::inflate::{self, Inflated};
use super::obj::Object;
use super::parser::source::{self, ByteRead, Limited, MemFile, Shared};
use super::parser::{
    BoundaryReason, BoundaryRecovery, IncompletePixelData, PrivateDictionaryOptions, Reader,
    ReaderOptions, SequenceRecoveryOptions, TokenKind, VrRecoveryOptions,
};
use super::transfer::{self, EXPLICIT_VR_LITTLE_ENDIAN, IMPLICIT_VR_LITTLE_ENDIAN, Syntax};
use super::types::{DataSet, Tag, Value, Vr};
use crate::limits::Limits;
use std::cell::RefCell;
use std::rc::Rc;
const PREAMBLE_LENGTH: usize = 128;
const TAG_TRANSFER_SYNTAX_UID: Tag = Tag::new(0x0002, 0x0010);
const TAG_FILE_META_GROUP_LENGTH: Tag = Tag::new(0x0002, 0x0000);
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct ReadFileOptions {
    pub allow_incomplete_pixel_data: bool,
    pub max_element_bytes: i64,
    pub max_pixel_data_bytes: i64,
    pub max_total_bytes: i64,
    pub max_sequence_depth: i64,
    pub max_elements: i64,
    pub max_fragments: i64,
    pub skip_pixel_data: bool,
    pub allow_missing_meta_element_group_length: bool,
    pub allow_implicit_vr_file_meta: bool,
    pub strict_reserved_bytes: bool,
    pub accept_odd_length: bool,
    pub recover_container_overruns: bool,
    pub max_boundary_recoveries: i64,
    pub recover_implicit_vr_elements: bool,
    pub max_implicit_vr_elements: i64,
    pub sequence_recovery: SequenceRecoveryOptions,
    pub allow_partial_read: bool,
    pub vr_recovery: VrRecoveryOptions,
    pub recover_delimiter_anomalies: bool,
    pub allow_zlib_deflate: bool,
    pub max_delimiter_recoveries: i64,
    pub limits: Limits,
}
impl ReadFileOptions {
    pub(super) fn parser_reader_options(&self, base_offset: i64) -> ReaderOptions {
        let limits = if self.allow_incomplete_pixel_data {
            Limits {
                max_sequence_depth: self.limits.max_sequence_depth,
                max_items: self.limits.max_items,
                ..Limits::NONE
            }
        } else {
            self.limits
        };
        let depth_limit =
            if i64::from(limits.max_sequence_depth) == super::parser::DEFAULT_MAX_SEQUENCE_DEPTH {
                0
            } else {
                u64::from(limits.max_sequence_depth)
            };
        ReaderOptions {
            allow_incomplete_pixel_data: self.allow_incomplete_pixel_data,
            max_element_bytes: Limits::effective(self.max_element_bytes, limits.max_element_bytes),
            max_pixel_data_bytes: Limits::effective(
                self.max_pixel_data_bytes,
                limits.max_pixel_data_bytes,
            ),
            max_total_bytes: Limits::effective(self.max_total_bytes, limits.max_data_set_bytes),
            base_offset,
            max_sequence_depth: Limits::effective(self.max_sequence_depth, depth_limit),
            max_elements: Limits::effective(self.max_elements, limits.max_elements),
            max_items: Limits::effective(0, limits.max_items),
            max_fragments: Limits::effective(self.max_fragments, limits.max_fragments),
            strict_reserved_bytes: self.strict_reserved_bytes,
            accept_odd_length: self.accept_odd_length,
            skip_pixel_data: self.skip_pixel_data,
            recover_container_overruns: self.recover_container_overruns,
            max_boundary_recoveries: self.max_boundary_recoveries,
            recover_implicit_vr_elements: self.recover_implicit_vr_elements,
            sequence_recovery: self.sequence_recovery,
            vr_recovery: self.vr_recovery,
            recover_delimiter_anomalies: self.recover_delimiter_anomalies,
            max_delimiter_recoveries: self.max_delimiter_recoveries,
            max_implicit_vr_elements: self.max_implicit_vr_elements,
        }
    }
}
#[derive(Clone, Debug)]
pub struct PartialRead {
    pub offset: i64,
    pub tag: Tag,
    pub elements: i64,
    pub retained_elements: i64,
    pub path: Vec<PathStep>,
    pub containers: Vec<PartialContainer>,
    pub err: Error,
}
impl PartialRead {
    pub fn error_text(&self) -> String {
        format!(
            "{}: kept {} complete elements ({} root entries) before offset {} ({}), path {}: {}",
            Kind::PartialRead.text(),
            self.elements,
            self.retained_elements,
            self.offset,
            self.tag,
            path_string(&self.path),
            self.err
        )
    }
}
#[derive(Clone, Debug)]
pub struct File {
    pub preamble: Vec<u8>,
    pub meta: Object,
    pub dataset: Object,
    pub transfer_syntax: Syntax,
    pub partial_read: Option<PartialRead>,
    pub incomplete_pixel_data: Option<IncompletePixelData>,
}
pub type ReadResult = std::result::Result<File, (Option<Box<File>>, Error)>;
pub fn open_file(path: impl AsRef<std::path::Path>) -> ReadResult {
    open_file_with_options(path, ReadFileOptions::default())
}
pub fn open_file_with_options(
    path: impl AsRef<std::path::Path>,
    opts: ReadFileOptions,
) -> ReadResult {
    let path = path.as_ref().to_string_lossy();
    let path: &str = &path;
    if let Err(err) = incomplete_file_options(&opts) {
        return Err((None, err));
    }
    let data = read_source_file(path).map_err(|e| (None, e))?;
    read_file_with_options(&data, opts)
}
pub fn read_file(data: &[u8]) -> ReadResult {
    read_file_with_options(data, ReadFileOptions::default())
}
pub(super) fn read_source_file(path: &str) -> Result<Vec<u8>> {
    if path.contains('\0') {
        return Err(Error::msg("invalid argument"));
    }
    let open_error = |e: std::io::Error| {
        let text = e.to_string();
        let text = match text.rfind(" (os error ") {
            Some(i) => text[..i].to_string(),
            None => text,
        };
        Error::msg(format!("open {path}: {text}"))
    };
    if let Some(name) = path.rsplit(['/', '\\']).next()
        && name.len() > 1
        && name.starts_with(':')
        && !name[1..].contains([':', '<', '>', '"', '|', '?', '*'])
        && !name.chars().any(char::is_control)
    {
        return Err(Error::msg(format!(
            "open {path}: The system cannot find the file specified."
        )));
    }
    let meta = std::fs::metadata(path).map_err(open_error)?;
    if meta.is_dir() {
        return Err(Error::msg(format!("open {path}: Access is denied.")));
    }
    std::fs::read(path).map_err(open_error)
}
#[doc(hidden)]
pub fn open_file_header_with_options(path: &str, opts: ReadFileOptions) -> ReadResult {
    if let Err(err) = incomplete_file_options(&opts) {
        return Err((None, err));
    }
    #[cfg(windows)]
    {
        if path.contains('\0')
            || std::fs::metadata(path).map_or(true, |m| m.is_dir() || m.len() == 0)
        {
            let data = read_source_file(path).map_err(|e| (None, e))?;
            return read_file_with_options(&data, opts);
        }
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_SHARE_READ: u32 = 0x1;
        let file = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ)
            .open(path)
            .ok();
        #[allow(unsafe_code)]
        let mapped = file
            .as_ref()
            .and_then(|f| unsafe { memmap2::Mmap::map(f) }.ok());
        let result = match &mapped {
            Some(map) => read_file_with_options(map, opts),
            None => {
                let data = read_source_file(path).map_err(|e| (None, e))?;
                read_file_with_options(&data, opts)
            }
        };
        drop(mapped);
        drop(file);
        result
    }
    #[cfg(not(windows))]
    {
        let data = read_source_file(path).map_err(|e| (None, e))?;
        read_file_with_options(&data, opts)
    }
}
pub(super) fn incomplete_file_options(o: &ReadFileOptions) -> Result<()> {
    if o.allow_incomplete_pixel_data && (o.allow_partial_read || o.skip_pixel_data) {
        return Err(Error::sentinel(Kind::IncompletePixelDataOptions));
    }
    Ok(())
}
fn wrap_file_meta_error(err: Error) -> Error {
    if err.is(Kind::FileMeta)
        || err.is(Kind::MissingTransferSyntax)
        || err.is(Kind::UnknownTransferSyntax)
        || err.is(Kind::UnsupportedTransferSyntax)
    {
        return err;
    }
    Error::stage(Kind::FileMeta, err)
}
pub(super) fn wrap_data_set_error(err: Error) -> Error {
    if err.is(Kind::DataSet) {
        return err;
    }
    Error::stage(Kind::DataSet, err)
}
pub fn read_file_with_options(data: &[u8], opts: ReadFileOptions) -> ReadResult {
    read_file_with_deferral(data, opts, None)
}
pub(super) fn read_file_with_deferral(
    data: &[u8],
    opts: ReadFileOptions,
    defer: Option<&DeferOptions>,
) -> ReadResult {
    read_file_core(data, opts, defer, &mut None, true, None)
}
pub(super) fn read_file_core(
    data: &[u8],
    opts: ReadFileOptions,
    defer: Option<&DeferOptions>,
    sinks: &mut Option<ReadSinks>,
    seekable: bool,
    private: Option<&PrivateDictionaryOptions>,
) -> ReadResult {
    if let Err(err) = incomplete_file_options(&opts) {
        return Err((None, err));
    }
    let bytes: Rc<[u8]> = Rc::from(data);
    let file = MemFile::new(bytes.clone());
    let (preamble, meta, syntax, dataset_offset) =
        read_part10_header(&file, &opts).map_err(|e| (None, e))?;
    let mut zlib_wrapped = false;
    let (reader_source, sized): (Box<dyn ByteRead>, Option<(i64, i64)>) = if syntax.deflated {
        if opts.allow_incomplete_pixel_data {
            return Err((
                None,
                wrap_data_set_error(Error::sentinel(Kind::IncompletePixelDataOptions)),
            ));
        }
        if defer.is_some_and(deferred::requires_seekable) {
            return Err((
                None,
                wrap_data_set_error(deferred::deflated_deferred_value_error()),
            ));
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
    let reader_opts = opts.parser_reader_options(dataset_offset);
    let mut reader = Reader::new(reader_source, sized, syntax, reader_opts);
    let defer = defer.filter(|_| !syntax.deflated);
    if let Some(defer) = defer {
        reader.set_deferral(defer);
    }
    if let Some(private) = private {
        reader.set_private_dictionary(private);
    }
    if let Some(sinks) = sinks.take() {
        sinks.install(&mut reader);
    }
    let mut partial = None;
    let mut dataset;
    if opts.allow_partial_read {
        let (ds, err) = reader.read_data_set_partial();
        dataset = ds;
        if let Some(err) = err {
            if err.is(Kind::ContextCanceled) {
                return Err((None, wrap_data_set_error(err)));
            }
            partial = Some(new_partial_read(
                wrap_data_set_error(err),
                dataset.elements.len() as i64,
            ));
        }
    } else {
        let (ds, err) = reader.read_data_set();
        dataset = ds;
        if let Some(err) = err {
            if !opts.allow_incomplete_pixel_data || !err.is(Kind::IncompletePixelData) {
                return Err((None, wrap_data_set_error(err)));
            }
            let Some(info) = reader.incomplete_pixel_data() else {
                return Err((None, wrap_data_set_error(err)));
            };
            partial = Some(incomplete_partial(&info, dataset.elements.len() as i64));
        }
    }
    if !syntax.explicit_vr {
        resolve_implicit_contextual_vrs_chained(
            &mut dataset,
            syntax.big_endian,
            private.map(|p| p.catalog.as_ref()),
        );
    }
    let parsed_tags: Vec<Tag> = dataset.elements.iter().map(|e| e.tag()).collect();
    let mut dataset_object = Object::from_elements(dataset.elements);
    dataset_object.ambiguous_deferred = deferred::duplicate_tags(&parsed_tags);
    dataset_object.set_value_big_endian(syntax.big_endian);
    dataset_object.parsed_pixel_data = dataset_object.has(Tag::PIXEL_DATA);
    if reader.encoded_pixel_data_discarded() {
        dataset_object.discarded.push(Tag::PIXEL_DATA);
    }
    if !seekable {
    } else if reader.has_frame_sink() && !syntax.deflated {
        let none = DeferOptions::default();
        deferred::attach_value_provider_validated(
            &mut dataset_object,
            &parsed_tags,
            &reader,
            &bytes,
            syntax,
            reader_opts,
            defer.unwrap_or(&none),
        );
    } else if let Some(defer) = defer {
        deferred::attach_value_provider(
            &mut dataset_object,
            &parsed_tags,
            &reader,
            &bytes,
            syntax,
            reader_opts,
            defer,
        );
    }
    let incomplete = reader.incomplete_pixel_data();
    (
        dataset_object.private_diagnostics,
        dataset_object.private_diagnostics_truncated,
    ) = reader.private_diagnostics();
    let (recoveries, truncated) = reader.boundary_recoveries();
    dataset_object.boundary_recoveries = recoveries;
    dataset_object.boundary_recoveries_truncated = truncated;
    if zlib_wrapped {
        let mut zlib = BoundaryRecovery::new(dataset_offset, -1, -1, BoundaryReason::ZlibDeflate);
        zlib.path = "(dataset)".to_string();
        dataset_object.boundary_recoveries.insert(0, zlib);
    }
    let mut result = File {
        preamble,
        meta,
        dataset: dataset_object,
        transfer_syntax: syntax,
        partial_read: None,
        incomplete_pixel_data: None,
    };
    if let Some(partial) = partial {
        let err = Error::new_kind(Kind::PartialRead, partial.error_text());
        result.partial_read = Some(partial);
        result.incomplete_pixel_data = incomplete;
        return Err((Some(Box::new(result)), err));
    }
    Ok(result)
}
pub(super) fn new_partial_read(err: Error, elements: i64) -> PartialRead {
    let mut p = PartialRead {
        offset: -1,
        tag: Tag::default(),
        elements,
        retained_elements: elements,
        path: Vec::new(),
        containers: Vec::new(),
        err: err.clone(),
    };
    if let Some(pe) = err.parse_info() {
        p.offset = pe.offset;
        p.tag = pe.tag;
    }
    if let Some(partial) = err.partial_info() {
        p.path = partial.path.clone();
        p.containers = partial.containers.clone();
        if p.containers.first().is_some_and(|c| c.retained) {
            p.elements -= 1;
        }
    }
    p
}
pub(super) fn incomplete_partial(info: &IncompletePixelData, entries: i64) -> PartialRead {
    PartialRead {
        offset: info.failure_offset,
        tag: Tag::PIXEL_DATA,
        elements: (entries - 1).max(0),
        retained_elements: entries,
        path: vec![PathStep {
            tag: Tag::PIXEL_DATA,
            item_index: -1,
        }],
        containers: Vec::new(),
        err: wrap_data_set_error(super::parser::incomplete_pixel_data_error(
            info.cause.clone(),
        )),
    }
}
pub(super) fn read_part10_header(
    file: &MemFile,
    opts: &ReadFileOptions,
) -> Result<(Vec<u8>, Object, Syntax, i64)> {
    let mut prefix = [0u8; PREAMBLE_LENGTH + 4];
    let mut f = file.clone();
    let (_, err) = if opts.max_total_bytes > 0 {
        let mut limited = Limited {
            r: Box::new(f.clone()),
            n: opts.max_total_bytes,
        };
        let res = source::read_full(&mut limited, &mut prefix);
        if let Some(err) = &res.1
            && limited.n == 0
            && (err.is(Kind::Eof) || err.is(Kind::UnexpectedEof))
        {
            return Err(Error::sentinel(Kind::MaxTotalBytes));
        }
        res
    } else {
        source::read_full(&mut f, &mut prefix)
    };
    if let Some(err) = err {
        return Err(err);
    }
    if &prefix[PREAMBLE_LENGTH..] != b"DICM" {
        return Err(Error::sentinel(Kind::MissingPreamble));
    }
    let (meta, dataset_offset) = read_file_meta_elements(file, (PREAMBLE_LENGTH + 4) as i64, opts)
        .map_err(wrap_file_meta_error)?;
    let syntax = resolve_file_meta_transfer_syntax(&meta).map_err(wrap_file_meta_error)?;
    file.seek(dataset_offset as usize);
    Ok((
        prefix[..PREAMBLE_LENGTH].to_vec(),
        meta,
        syntax,
        dataset_offset,
    ))
}
pub(super) fn resolve_file_meta_transfer_syntax(meta: &Object) -> Result<Syntax> {
    let Some(uid) = meta.get_default_string(TAG_TRANSFER_SYNTAX_UID) else {
        return Err(Error::sentinel(Kind::MissingTransferSyntax));
    };
    let uid = transfer::normalize_uid(&uid);
    if uid.is_empty() {
        return Err(Error::sentinel(Kind::MissingTransferSyntax));
    }
    let Some(syntax) = transfer::lookup(uid) else {
        return Err(Error::kind_suffix(
            Kind::UnknownTransferSyntax,
            &format!(": {}", crate::textfmt::quote(uid)),
        ));
    };
    if !syntax.supported {
        let name = if syntax.name.is_empty() {
            String::new()
        } else {
            format!(" ({})", syntax.name)
        };
        return Err(Error::kind_suffix(
            Kind::UnsupportedTransferSyntax,
            &format!(
                ": transfer syntax {}{name} is registered but not supported by this reader",
                crate::textfmt::quote(syntax.uid)
            ),
        ));
    }
    Ok(syntax)
}
pub(super) fn read_file_meta_elements(
    file: &MemFile,
    base_offset: i64,
    opts: &ReadFileOptions,
) -> Result<(Object, i64)> {
    let mut meta_opts = *opts;
    meta_opts.skip_pixel_data = false;
    let reader_opts = meta_opts.parser_reader_options(base_offset);
    if opts.allow_implicit_vr_file_meta && file_meta_is_implicit_vr(file) {
        let reader = Reader::new(
            Box::new(file.clone()),
            None,
            IMPLICIT_VR_LITTLE_ENDIAN,
            reader_opts,
        );
        let (mut meta, end) = read_file_meta_elements_by_group(file, reader)?;
        let mut recovery = BoundaryRecovery::new(
            base_offset,
            -1,
            end - base_offset,
            BoundaryReason::ImplicitVrFileMeta,
        );
        recovery.path = "(0002,xxxx)".to_string();
        meta.boundary_recoveries.push(recovery);
        return Ok((meta, end));
    }
    if opts.allow_missing_meta_element_group_length {
        let reader = Reader::new(
            Box::new(file.clone()),
            None,
            EXPLICIT_VR_LITTLE_ENDIAN,
            reader_opts,
        );
        return read_file_meta_elements_by_group(file, reader);
    }
    read_declared_file_meta(file, reader_opts)
}
fn read_file_meta_elements_by_group(file: &MemFile, mut reader: Reader) -> Result<(Object, i64)> {
    let mut elements = Vec::new();
    loop {
        let position = file.position();
        let mut tag_bytes = [0u8; 4];
        let (_, read_err) = source::read_full(&mut file.clone(), &mut tag_bytes);
        file.seek(position);
        if let Some(err) = read_err {
            if err.is(Kind::Eof) || err.is(Kind::UnexpectedEof) {
                break;
            }
            return Err(err);
        }
        if u16::from_le_bytes([tag_bytes[0], tag_bytes[1]]) != 0x0002 {
            break;
        }
        let token = reader.next()?;
        if token.kind != TokenKind::Element {
            return Err(Error::sentinel(Kind::InvalidFileMetaGroupLength));
        }
        elements.push(
            token
                .element
                .ok_or_else(|| Error::sentinel(Kind::InvalidFileMetaGroupLength))?,
        );
    }
    if elements.is_empty() {
        return Err(Error::sentinel(Kind::InvalidFileMetaGroupLength));
    }
    Ok((Object::from_elements(elements), reader.position()))
}
fn file_meta_is_implicit_vr(file: &MemFile) -> bool {
    let rest = file.remaining();
    if rest.len() < 6 {
        return false;
    }
    u16::from_le_bytes([rest[0], rest[1]]) == 0x0002 && Vr::parse(&rest[4..6]).is_none()
}
struct MetaGroupSource {
    source: Box<dyn ByteRead>,
    tag: [u8; 4],
    pending: usize,
}
impl ByteRead for MetaGroupSource {
    fn read(&mut self, p: &mut [u8]) -> (usize, Option<Error>) {
        if self.pending == 0 {
            return self.source.read(p);
        }
        let start = 4 - self.pending;
        let n = p.len().min(self.pending);
        p[..n].copy_from_slice(&self.tag[start..start + n]);
        self.pending -= n;
        (n, None)
    }
}
fn meta_group_next(
    checked: &Shared<MetaGroupSource>,
    reader: &mut Reader,
    limit: i64,
    first: bool,
) -> Result<super::parser::Token> {
    if limit > 0 && limit - reader.position() < 4 {
        return Err(Error::parse(
            Op::ReadTag,
            reader.position(),
            Tag::default(),
            None,
            0,
            Some(Error::sentinel(Kind::MaxTotalBytes)),
        ));
    }
    let mut tag = [0u8; 4];
    let (_, err) = {
        let mut state = checked.0.borrow_mut();
        source::read_full(state.source.as_mut(), &mut tag)
    };
    if let Some(mut err) = err {
        if !first && err.is(Kind::Eof) {
            err = Error::unexpected_eof();
        }
        return Err(Error::parse(
            Op::ReadTag,
            reader.position(),
            Tag::default(),
            None,
            0,
            Some(err),
        ));
    }
    let t = Tag::new(
        u16::from_le_bytes([tag[0], tag[1]]),
        u16::from_le_bytes([tag[2], tag[3]]),
    );
    if t.group != 0x0002 || (first && t != TAG_FILE_META_GROUP_LENGTH) {
        return Err(Error::parse(
            Op::ReadTag,
            reader.position(),
            t,
            None,
            0,
            Some(Error::sentinel(Kind::InvalidFileMetaGroupLength)),
        ));
    }
    {
        let mut state = checked.0.borrow_mut();
        state.tag = tag;
        state.pending = 4;
    }
    reader.next()
}
fn read_declared_file_meta(file: &MemFile, mut opts: ReaderOptions) -> Result<(Object, i64)> {
    let checked = Shared(Rc::new(RefCell::new(MetaGroupSource {
        source: Box::new(file.clone()),
        tag: [0; 4],
        pending: 0,
    })));
    let mut reader = Reader::new(
        Box::new(checked.clone()),
        None,
        EXPLICIT_VR_LITTLE_ENDIAN,
        opts,
    );
    let first = meta_group_next(&checked, &mut reader, opts.max_total_bytes, true)?;
    let raw = match (&first.kind, first.element.as_ref().map(|e| &e.value)) {
        (TokenKind::Element, Some(Value::Raw(raw))) if raw.len() == 4 => {
            [raw[0], raw[1], raw[2], raw[3]]
        }
        _ => return Err(Error::sentinel(Kind::InvalidFileMetaGroupLengthValue)),
    };
    let length = u32::from_le_bytes(raw) as i64;
    opts.base_offset = reader.position();
    if opts.max_total_bytes > 0 && length > opts.max_total_bytes - opts.base_offset {
        return Err(Error::parse(
            Op::CheckTotalBytes,
            opts.base_offset,
            TAG_FILE_META_GROUP_LENGTH,
            None,
            0,
            Some(Error::sentinel(Kind::MaxTotalBytes)),
        ));
    }
    let declared = Rc::new(RefCell::new(Limited {
        r: Box::new(file.clone()),
        n: length,
    }));
    checked.0.borrow_mut().source = Box::new(Shared(declared.clone()));
    let mut reader = Reader::new(
        Box::new(checked.clone()),
        None,
        EXPLICIT_VR_LITTLE_ENDIAN,
        opts,
    );
    let mut elements = vec![
        first
            .element
            .ok_or_else(|| Error::sentinel(Kind::InvalidFileMetaGroupLength))?,
    ];
    while declared.borrow().n > 0 {
        let token = meta_group_next(&checked, &mut reader, opts.max_total_bytes, false)?;
        if token.kind != TokenKind::Element {
            return Err(Error::kind_suffix(
                Kind::InvalidFileMetaGroupLength,
                &format!(": unexpected file meta token {}", token.kind.text()),
            ));
        }
        elements.push(
            token
                .element
                .ok_or_else(|| Error::sentinel(Kind::InvalidFileMetaGroupLength))?,
        );
    }
    Ok((Object::from_elements(elements), reader.position()))
}
pub fn data_set(file: &File) -> DataSet {
    DataSet {
        elements: file.dataset.elements().to_vec(),
        item_offset: None,
    }
}
pub fn read_data_set_with_options(
    r: Box<dyn ByteRead>,
    syntax: Syntax,
    opts: ReadFileOptions,
) -> Result<Object> {
    read_data_set_stream_core(r, syntax, opts, &mut None, None)
}
pub(super) fn read_data_set_stream_core(
    mut r: Box<dyn ByteRead>,
    syntax: Syntax,
    opts: ReadFileOptions,
    sinks: &mut Option<ReadSinks>,
    private: Option<&PrivateDictionaryOptions>,
) -> Result<Object> {
    incomplete_file_options(&opts)?;
    let mut zlib_wrapped = false;
    let source: Box<dyn ByteRead> = if syntax.deflated {
        if opts.allow_incomplete_pixel_data {
            return Err(Error::sentinel(Kind::IncompletePixelDataOptions));
        }
        let mut compressed = Vec::new();
        let mut buf = vec![0u8; 32 << 10];
        loop {
            let (n, err) = r.read(&mut buf);
            compressed.extend_from_slice(&buf[..n]);
            match err {
                None => {}
                Some(e) if e.is_exactly(Kind::Eof) => break,
                Some(e) => return Err(e),
            }
        }
        let (decoded, err) = if opts.allow_zlib_deflate && inflate::is_zlib_header(&compressed) {
            zlib_wrapped = true;
            inflate::inflate_zlib_limited(&compressed, opts.limits.max_inflated_bytes)
        } else {
            let (decoded, err, _) =
                inflate::inflate_raw_limited(&compressed, opts.limits.max_inflated_bytes);
            (decoded, err)
        };
        Box::new(Inflated::new(decoded, err))
    } else {
        r
    };
    let reader_opts = opts.parser_reader_options(0);
    let mut reader = Reader::new(source, None, syntax, reader_opts);
    if let Some(private) = private {
        reader.set_private_dictionary(private);
    }
    if let Some(sinks) = sinks.take() {
        sinks.install(&mut reader);
    }
    let (mut dataset, err) = reader.read_data_set();
    if let Some(err) = err {
        return Err(err);
    }
    if !syntax.explicit_vr {
        resolve_implicit_contextual_vrs_chained(
            &mut dataset,
            syntax.big_endian,
            private.map(|p| p.catalog.as_ref()),
        );
    }
    let parsed_tags: Vec<Tag> = dataset.elements.iter().map(|e| e.tag()).collect();
    let mut obj = Object::from_elements(dataset.elements);
    obj.ambiguous_deferred = deferred::duplicate_tags(&parsed_tags);
    obj.set_value_big_endian(syntax.big_endian);
    obj.parsed_pixel_data = obj.has(Tag::PIXEL_DATA);
    (obj.private_diagnostics, obj.private_diagnostics_truncated) = reader.private_diagnostics();
    if reader.encoded_pixel_data_discarded() {
        obj.discarded.push(Tag::PIXEL_DATA);
    }
    let (recoveries, truncated) = reader.boundary_recoveries();
    obj.boundary_recoveries = recoveries;
    obj.boundary_recoveries_truncated = truncated;
    if zlib_wrapped {
        let mut zlib = BoundaryRecovery::new(0, -1, -1, BoundaryReason::ZlibDeflate);
        zlib.path = "(dataset)".to_string();
        obj.boundary_recoveries.insert(0, zlib);
    }
    Ok(obj)
}
pub(super) fn read_data_set_with_deferral(
    source: Rc<[u8]>,
    syntax: Syntax,
    opts: ReadFileOptions,
    defer: &DeferOptions,
) -> Result<Object> {
    read_data_set_seekable_core(source, syntax, opts, defer, &mut None, None)
}
pub(super) fn read_data_set_seekable_core(
    source: Rc<[u8]>,
    syntax: Syntax,
    opts: ReadFileOptions,
    defer: &DeferOptions,
    sinks: &mut Option<ReadSinks>,
    private: Option<&PrivateDictionaryOptions>,
) -> Result<Object> {
    incomplete_file_options(&opts)?;
    let file = MemFile::new(source.clone());
    let sized = Some((file.len() as i64, 0));
    let reader_opts = opts.parser_reader_options(0);
    let mut reader = Reader::new(Box::new(file), sized, syntax, reader_opts);
    reader.set_deferral(defer);
    if let Some(private) = private {
        reader.set_private_dictionary(private);
    }
    if let Some(sinks) = sinks.take() {
        sinks.install(&mut reader);
    }
    let (mut dataset, err) = reader.read_data_set();
    if let Some(err) = err {
        return Err(err);
    }
    if !syntax.explicit_vr {
        resolve_implicit_contextual_vrs_chained(
            &mut dataset,
            syntax.big_endian,
            private.map(|p| p.catalog.as_ref()),
        );
    }
    let parsed_tags: Vec<Tag> = dataset.elements.iter().map(|e| e.tag()).collect();
    let mut obj = Object::from_elements(dataset.elements);
    obj.ambiguous_deferred = deferred::duplicate_tags(&parsed_tags);
    obj.set_value_big_endian(syntax.big_endian);
    obj.parsed_pixel_data = obj.has(Tag::PIXEL_DATA);
    let (recoveries, truncated) = reader.boundary_recoveries();
    obj.boundary_recoveries = recoveries;
    obj.boundary_recoveries_truncated = truncated;
    (obj.private_diagnostics, obj.private_diagnostics_truncated) = reader.private_diagnostics();
    if reader.encoded_pixel_data_discarded() {
        obj.discarded.push(Tag::PIXEL_DATA);
    }
    if reader.has_frame_sink() {
        deferred::attach_value_provider_validated(
            &mut obj,
            &parsed_tags,
            &reader,
            &source,
            syntax,
            reader_opts,
            defer,
        );
    } else {
        deferred::attach_value_provider(
            &mut obj,
            &parsed_tags,
            &reader,
            &source,
            syntax,
            reader_opts,
            defer,
        );
    }
    Ok(obj)
}
#[doc(hidden)]
pub fn read_data_set_bytes(data: &[u8], syntax: Syntax) -> Result<Object> {
    read_data_set_with_options(
        Box::new(MemFile::new(Rc::from(data))),
        syntax,
        ReadFileOptions::default(),
    )
}
#[derive(Clone, Debug)]
pub struct Part10Header {
    pub preamble: Vec<u8>,
    pub meta: Object,
    pub transfer_syntax: Syntax,
    pub data_set_offset: i64,
}
pub fn read_part10_header_mem(file: &MemFile, opts: &ReadFileOptions) -> Result<Part10Header> {
    let (preamble, meta, transfer_syntax, data_set_offset) = read_part10_header(file, opts)?;
    Ok(Part10Header {
        preamble,
        meta,
        transfer_syntax,
        data_set_offset,
    })
}
