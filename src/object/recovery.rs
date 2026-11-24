#![forbid(unsafe_code)]
use super::contextual::resolve_implicit_contextual_vrs;
use super::error::{Error, Kind};
use super::file::{
    File, ReadFileOptions, incomplete_partial, open_file_with_options, read_file_meta_elements,
    read_file_with_options, read_source_file, resolve_file_meta_transfer_syntax,
    wrap_data_set_error,
};
use super::inflate::{self, Inflated};
use super::obj::Object;
use super::parser::source::{ByteRead, MemFile};
use super::parser::{
    BoundaryReason, BoundaryRecovery, Reader, SequenceRecoveryOptions, VrRecoveryOptions,
};
use super::probe::{CandidateReport, ProbeOptions, ProbeReport, failure, strictness};
use super::transfer::{self, Syntax};
use super::types::{DataSet, Element, Tag, Value};
use std::rc::Rc;
pub const ERR_RECOVERY_BUFFER_EXCEEDED: &str =
    "dicom: transfer syntax recovery non-seekable buffer limit exceeded";
pub const ERR_RECOVERY_POLICY: &str = "dicom: invalid transfer syntax recovery policy";
const DEFAULT_NON_SEEKABLE_BYTES: i64 = 16 << 20;
const PREAMBLE_LENGTH: usize = 128;
const MAGIC: &[u8] = b"DICM";
const TAG_TRANSFER_SYNTAX_UID: Tag = Tag::new(0x0002, 0x0010);
const TAG_BITS_ALLOCATED: Tag = Tag::new(0x0028, 0x0100);
#[derive(Clone, Debug, Default)]
pub struct TransferSyntaxRecoveryOptions {
    pub allow_missing_preamble: bool,
    pub allow_missing_file_meta: bool,
    pub allow_missing_transfer_syntax_uid: bool,
    pub allow_unknown_transfer_syntax_uid: bool,
    pub allow_declared_mismatch: bool,
    pub allow_legacy_syntaxes: bool,
    pub probe: ProbeOptions,
    pub max_non_seekable_bytes: i64,
}
impl TransferSyntaxRecoveryOptions {
    fn enables_file_recovery(&self) -> bool {
        self.allow_missing_preamble
            || self.allow_missing_file_meta
            || self.allow_missing_transfer_syntax_uid
            || self.allow_unknown_transfer_syntax_uid
            || self.allow_declared_mismatch
            || self.allow_legacy_syntaxes
    }
    fn probe_options(&self) -> ProbeOptions {
        let mut probe = self.probe.clone();
        if self.allow_legacy_syntaxes && probe.candidates.is_empty() {
            probe.candidates = vec![
                transfer::IMPLICIT_VR_LITTLE_ENDIAN,
                transfer::EXPLICIT_VR_LITTLE_ENDIAN,
                transfer::lookup("1.2.840.10008.1.2.2")
                    .unwrap_or(transfer::EXPLICIT_VR_LITTLE_ENDIAN),
                transfer::IMPLICIT_VR_BIG_ENDIAN_ACR_NEMA,
            ];
        }
        probe
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ResolutionSource {
    #[default]
    None,
    Declared,
    DeclaredMissingPreamble,
    InferredRawDataSet,
    InferredMissingPreamble,
    InferredMissingFileMeta,
    InferredMissingUid,
    InferredUnknownUid,
    RecoveredMismatch,
}
impl ResolutionSource {
    pub fn text(self) -> &'static str {
        match self {
            ResolutionSource::None => "",
            ResolutionSource::Declared => "declared",
            ResolutionSource::DeclaredMissingPreamble => "declared-missing-preamble",
            ResolutionSource::InferredRawDataSet => "inferred-raw-dataset",
            ResolutionSource::InferredMissingPreamble => "inferred-missing-preamble",
            ResolutionSource::InferredMissingFileMeta => "inferred-missing-file-meta",
            ResolutionSource::InferredMissingUid => "inferred-missing-transfer-syntax-uid",
            ResolutionSource::InferredUnknownUid => "inferred-unknown-transfer-syntax-uid",
            ResolutionSource::RecoveredMismatch => "recovered-declared-mismatch",
        }
    }
}
#[derive(Clone, Debug, Default)]
pub struct TransferSyntaxResolution {
    pub syntax: Option<Syntax>,
    pub source: ResolutionSource,
    pub declared_uid: String,
    pub confidence: f64,
    pub probe: ProbeReport,
    pub legacy: Option<Syntax>,
}
impl TransferSyntaxResolution {
    pub fn inferred(&self) -> bool {
        !matches!(
            self.source,
            ResolutionSource::None
                | ResolutionSource::Declared
                | ResolutionSource::DeclaredMissingPreamble
        )
    }
    pub fn render(&self) -> String {
        let mut b = format!("source={}", self.source.text());
        if let Some(s) = self.syntax.filter(|s| !s.uid.is_empty()) {
            b.push_str(&format!(" syntax={}", s.uid));
        }
        if let Some(s) = self.legacy.filter(|s| !s.uid.is_empty()) {
            b.push_str(&format!(" legacy={}", s.uid));
        }
        if self.confidence != 0.0 {
            b.push_str(&format!(" confidence={:.3}", self.confidence));
        }
        if !self.probe.outcome.text().is_empty() {
            b.push_str(&format!(" probe={{{}}}", self.probe.render()));
        }
        b
    }
    fn from_probe(report: &ProbeReport, source: ResolutionSource, declared_uid: &str) -> Self {
        TransferSyntaxResolution {
            syntax: report.selected,
            source,
            declared_uid: safe_declared_syntax_uid(declared_uid),
            confidence: report.confidence,
            probe: report.clone(),
            legacy: None,
        }
    }
    fn declared(syntax: Syntax) -> Self {
        TransferSyntaxResolution {
            syntax: Some(syntax),
            source: ResolutionSource::Declared,
            declared_uid: syntax.uid.to_string(),
            confidence: 1.0,
            probe: ProbeReport::default(),
            legacy: None,
        }
    }
    fn normalize_legacy(&mut self) {
        if let Some(legacy) = self.syntax.and_then(|s| transfer::legacy_syntax(s.uid)) {
            self.legacy = Some(legacy);
            self.syntax = Some(transfer::IMPLICIT_VR_LITTLE_ENDIAN);
        }
    }
}
fn safe_declared_syntax_uid(uid: &str) -> String {
    let uid = transfer::normalize_uid(uid);
    if uid.is_empty() {
        return String::new();
    }
    if transfer::lookup(uid).is_some() || transfer::legacy_syntax(uid).is_some() {
        return uid.to_string();
    }
    String::new()
}
pub type FileRecovery = (Option<File>, TransferSyntaxResolution, Option<Error>);
pub(crate) type DataSetRecovery = (Option<Object>, TransferSyntaxResolution, Option<Error>);
fn fail<T>(err: Error) -> (Option<T>, TransferSyntaxResolution, Option<Error>) {
    (None, TransferSyntaxResolution::default(), Some(err))
}
fn prepare_source(
    data: &[u8],
    seekable: bool,
    read_opts: &ReadFileOptions,
    recovery: &TransferSyntaxRecoveryOptions,
) -> Result<Rc<[u8]>, Error> {
    if seekable {
        return Ok(Rc::from(data));
    }
    let mut limit = recovery.max_non_seekable_bytes;
    if limit == 0 {
        limit = DEFAULT_NON_SEEKABLE_BYTES;
    }
    if limit < 0 {
        return Err(Error::msg(format!(
            "{ERR_RECOVERY_POLICY}: MaxNonSeekableBytes must not be negative"
        )));
    }
    let mut hard_total_limit = false;
    if read_opts.max_total_bytes > 0 && read_opts.max_total_bytes < limit {
        limit = read_opts.max_total_bytes;
        hard_total_limit = true;
    } else if read_opts.max_total_bytes > 0 && read_opts.max_total_bytes == limit {
        hard_total_limit = true;
    }
    let read_limit = if hard_total_limit {
        limit
    } else {
        limit.saturating_add(1)
    };
    let got = (data.len() as i64).min(read_limit);
    if hard_total_limit && got == limit {
        return Err(Error::join(
            Some(Error::msg(format!(
                "{ERR_RECOVERY_BUFFER_EXCEEDED}: limit {limit}"
            ))),
            Some(Error::kind_suffix(
                Kind::MaxTotalBytes,
                &format!(": read {got} bytes, limit {limit}"),
            )),
        )
        .expect("joined error"));
    }
    if got > limit {
        return Err(Error::msg(format!(
            "{ERR_RECOVERY_BUFFER_EXCEEDED}: limit {limit}"
        )));
    }
    Ok(Rc::from(&data[..got as usize]))
}
fn probe_transfer_syntax_at(
    file: &MemFile,
    absolute_offset: i64,
    options: &ProbeOptions,
    max_total_bytes: i64,
    base_offset: i64,
) -> (ProbeReport, Option<Error>) {
    file.seek(absolute_offset.max(0) as usize);
    let mut limit = options.max_probe_bytes;
    if limit == 0 {
        limit = super::probe::DEFAULT_PROBE_BYTES;
    }
    if limit < 0 {
        return super::probe::probe_transfer_syntax(&[], options);
    }
    let mut read_limit = limit.saturating_add(1);
    if max_total_bytes > 0 {
        let remaining = max_total_bytes - base_offset;
        if remaining <= 0 {
            return (
                ProbeReport::default(),
                Some(Error::kind_suffix(
                    Kind::MaxTotalBytes,
                    &format!(": base offset {base_offset} reaches limit {max_total_bytes}"),
                )),
            );
        }
        if remaining <= limit {
            read_limit = remaining;
        }
    }
    let rest = file.remaining();
    let n = (rest.len() as i64).min(read_limit) as usize;
    super::probe::probe_transfer_syntax(&rest[..n], options)
}
fn read_data_set_object(
    file: &MemFile,
    syntax: Syntax,
    opts: &ReadFileOptions,
    base_offset: i64,
) -> Result<Object, Error> {
    if opts.allow_incomplete_pixel_data && syntax.deflated {
        return Err(Error::sentinel(Kind::IncompletePixelDataOptions));
    }
    let legacy = transfer::legacy_syntax(syntax.uid);
    let mut zlib_wrapped = false;
    let (source, sized): (Box<dyn ByteRead>, Option<(i64, i64)>) = if syntax.deflated {
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
    let mut reader = Reader::new(
        source,
        sized,
        syntax,
        opts.parser_reader_options(base_offset),
    );
    let (mut dataset, err) = reader.read_data_set();
    let mut incomplete = None;
    if let Some(err) = err {
        if !opts.allow_incomplete_pixel_data || !err.is(Kind::IncompletePixelData) {
            return Err(err);
        }
        match reader.incomplete_pixel_data() {
            Some(info) => incomplete = Some(info),
            None => return Err(err),
        }
    }
    if !syntax.explicit_vr {
        resolve_implicit_contextual_vrs(&mut dataset, syntax.big_endian);
    }
    let mut value_big_endian = syntax.big_endian;
    if let Some(legacy) = legacy {
        normalize_legacy_data_set(&mut dataset, &legacy);
        value_big_endian = false;
    }
    let entries = dataset.elements.len() as i64;
    let mut obj = Object::from_elements(dataset.elements);
    obj.set_value_big_endian(value_big_endian);
    obj.parsed_pixel_data = obj.has(Tag::PIXEL_DATA);
    let (recoveries, truncated) = reader.boundary_recoveries();
    obj.boundary_recoveries = recoveries;
    obj.boundary_recoveries_truncated = truncated;
    if zlib_wrapped {
        let mut zlib = BoundaryRecovery::new(base_offset, -1, -1, BoundaryReason::ZlibDeflate);
        zlib.path = "(dataset)".to_string();
        obj.boundary_recoveries.insert(0, zlib);
    }
    if let Some(info) = incomplete {
        let partial = incomplete_partial(&info, entries);
        return Err(Error::relabel(&partial.error_text(), partial.err.clone()));
    }
    Ok(obj)
}
fn reverse_fixed_width(data: &mut [u8], width: usize) {
    if width <= 1 {
        return;
    }
    for unit in data.chunks_exact_mut(width) {
        unit.reverse();
    }
}
fn normalize_legacy_data_set(ds: &mut DataSet, legacy: &Syntax) {
    if legacy.uid == transfer::IMPLICIT_VR_BIG_ENDIAN_ACR_NEMA.uid {
        little_endian_values(&mut ds.elements);
    } else if legacy.uid == transfer::PRIVATE_GE_IMPLICIT_VR_BIG_ENDIAN_PIXEL_DATA.uid {
        if bits_allocated(&ds.elements) != 16 {
            return;
        }
        for element in &mut ds.elements {
            if element.tag() == Tag::PIXEL_DATA
                && let Value::Raw(raw) = &mut element.value
            {
                reverse_fixed_width(raw, 2);
            }
        }
    }
}
fn little_endian_values(elements: &mut [Element]) {
    for element in elements {
        let width = element.vr().byte_order_width();
        match &mut element.value {
            Value::Raw(raw) => {
                if width > 1 && raw.len() % width == 0 {
                    reverse_fixed_width(raw, width);
                }
            }
            Value::Sequence(items) => {
                for item in items {
                    little_endian_values(&mut item.elements);
                }
            }
            _ => {}
        }
    }
}
fn bits_allocated(elements: &[Element]) -> u16 {
    for element in elements {
        if element.tag() != TAG_BITS_ALLOCATED {
            continue;
        }
        if let Value::Raw(raw) = &element.value
            && raw.len() >= 2
        {
            return u16::from_le_bytes([raw[0], raw[1]]);
        }
    }
    0
}
pub fn read_data_set_with_transfer_syntax_recovery(
    data: &[u8],
    seekable: bool,
    read_opts: ReadFileOptions,
    recovery: &TransferSyntaxRecoveryOptions,
) -> DataSetRecovery {
    let data = match prepare_source(data, seekable, &read_opts, recovery) {
        Ok(data) => data,
        Err(err) => return fail(err),
    };
    let file = MemFile::new(data);
    let (report, err) = probe_transfer_syntax_at(
        &file,
        0,
        &recovery.probe_options(),
        read_opts.max_total_bytes,
        0,
    );
    let mut resolution =
        TransferSyntaxResolution::from_probe(&report, ResolutionSource::InferredRawDataSet, "");
    if let Some(err) = err {
        return (None, resolution, Some(err));
    }
    let Some(selected) = report.selected else {
        return (None, resolution, None);
    };
    file.seek(0);
    match read_data_set_object(&file, selected, &read_opts, 0) {
        Ok(obj) => {
            resolution.normalize_legacy();
            (Some(obj), resolution, None)
        }
        Err(err) => (None, resolution, Some(err)),
    }
}
struct FileReader<'a> {
    data: Rc<[u8]>,
    file: MemFile,
    read_opts: ReadFileOptions,
    recovery: &'a TransferSyntaxRecoveryOptions,
}
struct Header<'a> {
    dataset_offset: i64,
    preamble: &'a [u8],
    meta: Option<Object>,
    declared_uid: &'a str,
}
fn new_resolved_file(
    preamble: &[u8],
    meta: Option<Object>,
    dataset: Object,
    resolution: &TransferSyntaxResolution,
) -> File {
    File {
        preamble: preamble.to_vec(),
        meta: meta.unwrap_or_default(),
        dataset,
        transfer_syntax: resolution
            .syntax
            .unwrap_or(transfer::IMPLICIT_VR_LITTLE_ENDIAN),
        partial_read: None,
        incomplete_pixel_data: None,
    }
}
impl FileReader<'_> {
    fn read_strict(&self) -> FileRecovery {
        read_strict(&self.data, self.read_opts)
    }
    fn probe(&self, dataset_offset: i64) -> (ProbeReport, Option<Error>) {
        probe_transfer_syntax_at(
            &self.file,
            dataset_offset,
            &self.recovery.probe_options(),
            self.read_opts.max_total_bytes,
            dataset_offset,
        )
    }
    fn read_meta(&self, meta_offset: i64) -> Option<(Object, i64, String, Result<Syntax, Error>)> {
        self.file.seek(meta_offset as usize);
        let (meta, dataset_offset) =
            read_file_meta_elements(&self.file, meta_offset, &self.read_opts).ok()?;
        let declared_uid = meta
            .get_default_string(TAG_TRANSFER_SYNTAX_UID)
            .unwrap_or_default();
        let declared_uid = transfer::normalize_uid(&declared_uid).to_string();
        let mut resolved = resolve_file_meta_transfer_syntax(&meta);
        if self.recovery.allow_legacy_syntaxes
            && let Some(legacy) = transfer::legacy_syntax(&declared_uid)
        {
            resolved = Ok(legacy);
        }
        Some((meta, dataset_offset, declared_uid, resolved))
    }
    fn recover_data_set_at(&self, header: Header, source: ResolutionSource) -> FileRecovery {
        let (report, probe_err) = self.probe(header.dataset_offset);
        let resolution = TransferSyntaxResolution::from_probe(&report, source, header.declared_uid);
        if let Some(err) = probe_err {
            return (None, resolution, Some(err));
        }
        self.read_recovered_from_report(header, resolution)
    }
    fn read_recovered_from_report(
        &self,
        header: Header,
        mut resolution: TransferSyntaxResolution,
    ) -> FileRecovery {
        let Some(syntax) = resolution.syntax else {
            return (None, resolution, None);
        };
        self.file.seek(header.dataset_offset as usize);
        match read_data_set_object(&self.file, syntax, &self.read_opts, header.dataset_offset) {
            Ok(dataset) => {
                resolution.normalize_legacy();
                let file = new_resolved_file(header.preamble, header.meta, dataset, &resolution);
                (Some(file), resolution, None)
            }
            Err(err) => (None, resolution, Some(wrap_data_set_error(err))),
        }
    }
    fn read_declared_at(
        &self,
        header: &Header,
        syntax: Syntax,
        source: ResolutionSource,
    ) -> FileRecovery {
        let mut resolution = TransferSyntaxResolution {
            syntax: Some(syntax),
            source,
            declared_uid: safe_declared_syntax_uid(header.declared_uid),
            confidence: 1.0,
            probe: ProbeReport::default(),
            legacy: None,
        };
        self.file.seek(header.dataset_offset as usize);
        match read_data_set_object(&self.file, syntax, &self.read_opts, header.dataset_offset) {
            Ok(dataset) => {
                resolution.normalize_legacy();
                let file =
                    new_resolved_file(header.preamble, header.meta.clone(), dataset, &resolution);
                (Some(file), resolution, None)
            }
            Err(err) => (None, resolution, Some(wrap_data_set_error(err))),
        }
    }
    fn read_declared_with_probe_at(
        &self,
        header: &Header,
        syntax: Syntax,
        source: ResolutionSource,
        report: &ProbeReport,
        probe_err: Option<Error>,
    ) -> FileRecovery {
        let (file, mut resolution, err) = self.read_declared_at(header, syntax, source);
        resolution.probe = report.clone();
        match err {
            None => (file, resolution, None),
            Some(err) => (None, resolution, Error::join(Some(err), probe_err)),
        }
    }
    fn read_declared_or_mismatch_at(
        &self,
        header: Header,
        declared: Syntax,
        declared_source: ResolutionSource,
    ) -> FileRecovery {
        if !(self.recovery.allow_declared_mismatch && eligible_for_mismatch_recovery(&declared)) {
            return self.read_declared_at(&header, declared, declared_source);
        }
        let (report, probe_err) = self.probe(header.dataset_offset);
        let candidate = report
            .candidates
            .iter()
            .find(|c| c.syntax.uid == declared.uid);
        let selected_uid = report.selected.map_or("", |s| s.uid);
        if probe_err.is_none()
            && selected_uid != declared.uid
            && let Some(candidate) = candidate
            && candidate.failure_class == failure::MALFORMED
        {
            let recovered = TransferSyntaxResolution::from_probe(
                &report,
                ResolutionSource::RecoveredMismatch,
                header.declared_uid,
            );
            if caller_allows_strictness_violation(&self.read_opts, candidate) {
                let (file, resolution, declared_err) = self.read_declared_with_probe_at(
                    &header,
                    declared,
                    declared_source,
                    &report,
                    None,
                );
                let Some(declared_err) = declared_err else {
                    return (file, resolution, None);
                };
                let (file, resolution, recovery_err) =
                    self.read_recovered_from_report(header, recovered);
                return match recovery_err {
                    Some(recovery_err) => (
                        None,
                        resolution,
                        Error::join(Some(declared_err), Some(recovery_err)),
                    ),
                    None => (file, resolution, None),
                };
            }
            return self.read_recovered_from_report(header, recovered);
        }
        self.read_declared_with_probe_at(&header, declared, declared_source, &report, probe_err)
    }
    fn after_meta(
        &self,
        meta_offset: i64,
        preamble: &[u8],
        declared_source: ResolutionSource,
    ) -> FileRecovery {
        let Some((meta, dataset_offset, declared_uid, resolved)) = self.read_meta(meta_offset)
        else {
            return self.read_strict();
        };
        let header = Header {
            dataset_offset,
            preamble,
            meta: Some(meta),
            declared_uid: &declared_uid,
        };
        match resolved {
            Ok(declared) => self.read_declared_or_mismatch_at(header, declared, declared_source),
            Err(err)
                if err.is(Kind::MissingTransferSyntax)
                    && self.recovery.allow_missing_transfer_syntax_uid =>
            {
                self.recover_data_set_at(header, ResolutionSource::InferredMissingUid)
            }
            Err(err)
                if err.is(Kind::UnknownTransferSyntax)
                    && self.recovery.allow_unknown_transfer_syntax_uid
                    && !transfer::is_legacy(&declared_uid) =>
            {
                self.recover_data_set_at(header, ResolutionSource::InferredUnknownUid)
            }
            Err(_) => self.read_strict(),
        }
    }
    fn recover_without_preamble(&self) -> FileRecovery {
        let bare = |offset: i64, source: ResolutionSource| {
            self.recover_data_set_at(
                Header {
                    dataset_offset: offset,
                    preamble: &[],
                    meta: None,
                    declared_uid: "",
                },
                source,
            )
        };
        let data = &self.data;
        if data.len() < 4 {
            return bare(0, ResolutionSource::InferredMissingPreamble);
        }
        let meta_offset: i64 = if &data[..4] == MAGIC {
            MAGIC.len() as i64
        } else if data[0] == 0x02 && data[1] == 0x00 {
            0
        } else {
            return bare(0, ResolutionSource::InferredMissingPreamble);
        };
        if meta_offset > 0 {
            let at = meta_offset as usize;
            if data.len() < at + 4 {
                return self.read_strict();
            }
            if data[at] != 0x02 || data[at + 1] != 0x00 {
                if !self.recovery.allow_missing_file_meta {
                    return self.read_strict();
                }
                return bare(meta_offset, ResolutionSource::InferredMissingFileMeta);
            }
        }
        self.after_meta(meta_offset, &[], ResolutionSource::DeclaredMissingPreamble)
    }
}
fn read_strict(data: &[u8], read_opts: ReadFileOptions) -> FileRecovery {
    match read_file_with_options(data, read_opts) {
        Ok(file) => {
            let resolution = TransferSyntaxResolution::declared(file.transfer_syntax);
            (Some(file), resolution, None)
        }
        Err((_, err)) => fail(err),
    }
}
fn caller_allows_strictness_violation(
    read_opts: &ReadFileOptions,
    candidate: &CandidateReport,
) -> bool {
    match candidate.strictness_violation {
        strictness::RESERVED_BYTES => !read_opts.strict_reserved_bytes,
        strictness::ODD_ELEMENT_LENGTH => read_opts.accept_odd_length,
        _ => false,
    }
}
fn eligible_for_mismatch_recovery(syntax: &Syntax) -> bool {
    syntax.supported && !syntax.encapsulated && !syntax.deflated
}
pub fn read_file_with_transfer_syntax_recovery(
    data: &[u8],
    seekable: bool,
    mut read_opts: ReadFileOptions,
    recovery: &TransferSyntaxRecoveryOptions,
) -> FileRecovery {
    read_opts.allow_partial_read = false;
    if !recovery.enables_file_recovery() {
        return read_strict(data, read_opts);
    }
    let data = match prepare_source(data, seekable, &read_opts, recovery) {
        Ok(data) => data,
        Err(err) => return fail(err),
    };
    let reader = FileReader {
        file: MemFile::new(data.clone()),
        data,
        read_opts,
        recovery,
    };
    let data = &reader.data;
    let prefix_length = PREAMBLE_LENGTH + MAGIC.len();
    if data.len() < prefix_length || &data[PREAMBLE_LENGTH..prefix_length] != MAGIC {
        if !recovery.allow_missing_preamble {
            return reader.read_strict();
        }
        return reader.recover_without_preamble();
    }
    let preamble = &data[..PREAMBLE_LENGTH];
    if data.len() < prefix_length + 4 {
        return reader.read_strict();
    }
    if data[prefix_length] != 0x02 || data[prefix_length + 1] != 0x00 {
        if !recovery.allow_missing_file_meta {
            return reader.read_strict();
        }
        return reader.recover_data_set_at(
            Header {
                dataset_offset: prefix_length as i64,
                preamble,
                meta: None,
                declared_uid: "",
            },
            ResolutionSource::InferredMissingFileMeta,
        );
    }
    reader.after_meta(prefix_length as i64, preamble, ResolutionSource::Declared)
}
pub fn open_file_with_transfer_syntax_recovery(
    path: impl AsRef<std::path::Path>,
    read_opts: ReadFileOptions,
    recovery: &TransferSyntaxRecoveryOptions,
) -> FileRecovery {
    let path = path.as_ref().to_string_lossy();
    let path: &str = &path;
    match read_source_file(path) {
        Ok(data) => read_file_with_transfer_syntax_recovery(&data, true, read_opts, recovery),
        Err(err) => fail(err),
    }
}
pub fn open_data_set_with_transfer_syntax_recovery(
    path: impl AsRef<std::path::Path>,
    read_opts: ReadFileOptions,
    recovery: &TransferSyntaxRecoveryOptions,
) -> DataSetRecovery {
    let path = path.as_ref().to_string_lossy();
    let path: &str = &path;
    match read_source_file(path) {
        Ok(data) => read_data_set_with_transfer_syntax_recovery(&data, true, read_opts, recovery),
        Err(err) => fail(err),
    }
}
#[derive(Clone, Copy, Debug)]
pub struct ReadRecovery {
    pub name: &'static str,
    pub transfer_syntax: bool,
    pub apply: fn(&mut ReadFileOptions, &mut TransferSyntaxRecoveryOptions),
}
pub fn non_conformant_read_recoveries() -> Vec<ReadRecovery> {
    fn plain(
        name: &'static str,
        apply: fn(&mut ReadFileOptions, &mut TransferSyntaxRecoveryOptions),
    ) -> ReadRecovery {
        ReadRecovery {
            name,
            transfer_syntax: false,
            apply,
        }
    }
    vec![
        plain("odd-length", |o, _| o.accept_odd_length = true),
        plain("container-overrun", |o, _| {
            o.recover_container_overruns = true
        }),
        plain("missing-meta-group-length", |o, _| {
            o.allow_missing_meta_element_group_length = true
        }),
        plain("implicit-vr-file-meta", |o, _| {
            o.allow_implicit_vr_file_meta = true
        }),
        plain("implicit-vr-element", |o, _| {
            o.recover_implicit_vr_elements = true
        }),
        plain("delimiter-anomalies", |o, _| {
            o.recover_delimiter_anomalies = true
        }),
        plain("zlib-deflate", |o, _| o.allow_zlib_deflate = true),
        plain("vr-repairs", |o, _| {
            o.vr_recovery = VrRecoveryOptions {
                map_ox_to_ow: true,
                correct_private_ul_length: true,
                short_length_un: true,
                correct_ge_text_length: true,
                ..VrRecoveryOptions::default()
            }
        }),
        plain("sequence-repairs", |o, _| {
            o.sequence_recovery = SequenceRecoveryOptions {
                undefined_length_obow_as_sq: true,
                voi_lut_sequence_ow_as_sq: true,
                replace_wrong_delimiter: true,
                private_sequence_as_bytes: true,
            }
        }),
        ReadRecovery {
            name: "transfer-syntax",
            transfer_syntax: true,
            apply: |_, t| {
                t.allow_missing_preamble = true;
                t.allow_missing_file_meta = true;
                t.allow_missing_transfer_syntax_uid = true;
                t.allow_unknown_transfer_syntax_uid = true;
                t.allow_declared_mismatch = true;
                t.allow_legacy_syntaxes = true;
            },
        },
    ]
}
#[derive(Clone, Debug, Default)]
pub struct RecoveryReadReport {
    pub applied: Vec<&'static str>,
    pub resolution: TransferSyntaxResolution,
    pub attempts: i64,
}
impl RecoveryReadReport {
    pub fn recovered(&self) -> bool {
        !self.applied.is_empty()
    }
}
pub fn open_file_with_recovery_ladder(
    path: impl AsRef<std::path::Path>,
    base: ReadFileOptions,
    recoveries: &[ReadRecovery],
) -> (Option<File>, RecoveryReadReport, Option<Error>) {
    let path = path.as_ref().to_string_lossy();
    let path: &str = &path;
    let mut report = RecoveryReadReport {
        attempts: 1,
        ..RecoveryReadReport::default()
    };
    let strict_err = match open_file_with_options(path, base) {
        Ok(file) => return (Some(file), report, None),
        Err((_, err)) => err,
    };
    let attempt = |set: &[ReadRecovery],
                   report: &mut RecoveryReadReport|
     -> Option<(File, TransferSyntaxResolution)> {
        report.attempts += 1;
        let mut opts = base;
        let mut ts_opts = TransferSyntaxRecoveryOptions::default();
        let mut use_ts = false;
        for recovery in set {
            (recovery.apply)(&mut opts, &mut ts_opts);
            use_ts = use_ts || recovery.transfer_syntax;
        }
        if use_ts {
            match open_file_with_transfer_syntax_recovery(path, opts, &ts_opts) {
                (Some(file), resolution, None) => Some((file, resolution)),
                _ => None,
            }
        } else {
            open_file_with_options(path, opts)
                .ok()
                .map(|file| (file, TransferSyntaxResolution::default()))
        }
    };
    let succeed = |file: File,
                   resolution: TransferSyntaxResolution,
                   set: &[ReadRecovery],
                   mut report: RecoveryReadReport| {
        report.applied = set.iter().map(|r| r.name).collect();
        if set.iter().any(|r| r.transfer_syntax) {
            report.resolution = resolution;
        }
        (Some(file), report, None)
    };
    for single in recoveries.chunks(1) {
        if let Some((file, resolution)) = attempt(single, &mut report) {
            return succeed(file, resolution, single, report);
        }
    }
    if recoveries.len() < 2 {
        return (None, report, Some(strict_err));
    }
    let mut set = recoveries.to_vec();
    let Some((mut file, mut resolution)) = attempt(&set, &mut report) else {
        return (None, report, Some(strict_err));
    };
    let mut i = 0;
    while i < set.len() && set.len() > 1 {
        let mut smaller = set.clone();
        smaller.remove(i);
        if let Some((f, r)) = attempt(&smaller, &mut report) {
            file = f;
            resolution = r;
            set = smaller;
            continue;
        }
        i += 1;
    }
    succeed(file, resolution, &set, report)
}
