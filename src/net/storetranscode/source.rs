use super::{SpoolDir, lock, same_descriptor};
use crate::net::dimse::{
    Cause, Error, Kind, OpenedStoreSource, Result, StoreDataSetWriter, StoreDescriptor,
    StoreItemResult, StoreSource, StoreSourceCloser, context_err, new_file_store_source,
};
use crate::net::ul::Ctx;
use crate::object::parser::source::MemFile;
use crate::object::transfer::{Syntax, lookup, normalize_uid};
use crate::object::{
    self, Element, ElementHeader, File, Object, ReadFileOptions, Tag, Value, Vr, is_valid_uid,
};
use crate::pixel::transcode::{
    self, ErrorKind as T, LossyVerificationError, LossyVerificationReport, TranscodeOptions,
    TranscodeReport,
};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fmt;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Duration;
pub const ERR_NO_REPRESENTATION: &str = "dicom: no requested store representation is producible";
const TAG_SOP_INSTANCE_UID: Tag = Tag::new(0x0008, 0x0018);
const ENCAPSULATED_UNCOMPRESSED: &str = "1.2.840.10008.1.2.1.98";
#[derive(Clone, Default)]
pub struct SourceOptions {
    pub target: String,
    pub fallback_original: bool,
    pub transcode: TranscodeOptions,
    pub dictionary_options: ReadFileOptions,
    pub spool_directory: String,
    pub max_spool_bytes: i64,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Candidate {
    pub transfer_syntax_uid: String,
    pub operation: String,
    pub producible: bool,
    pub reason: String,
    pub runtime_missing: bool,
    pub lossy: bool,
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Report {
    pub original_transfer_syntax_uid: String,
    pub candidates: Vec<Candidate>,
    pub selected_transfer_syntax_uid: String,
    pub negotiation: String,
    pub lossy_verification: Option<LossyVerificationReport>,
    pub verification_failure: Option<LossyVerificationError>,
}
#[derive(Debug, Clone)]
pub struct SourceError(Box<SourceErrorInner>);
#[derive(Debug, Clone)]
struct SourceErrorInner {
    error: Error,
    wrapped: bool,
    options: bool,
    no_representation: bool,
    spool_limit: bool,
    transcode: Option<transcode::Error>,
    prepared_closed: bool,
    prepared_binding: bool,
}
impl SourceError {
    fn new(inner: SourceErrorInner) -> SourceError {
        SourceError(Box::new(inner))
    }
    pub fn dimse(&self) -> &Error {
        &self.0.error
    }
    pub fn is_preparation(&self) -> bool {
        self.0.wrapped
    }
    pub fn is_options(&self) -> bool {
        self.0.options
    }
    pub fn is_no_representation(&self) -> bool {
        self.0.no_representation
    }
    pub fn is_spool_limit(&self) -> bool {
        self.0.spool_limit
    }
    pub fn is_prepared_closed(&self) -> bool {
        self.0.prepared_closed
    }
    pub fn is_prepared_binding(&self) -> bool {
        self.0.prepared_binding
    }
    pub fn transcode(&self) -> Option<&transcode::Error> {
        self.0.transcode.as_ref()
    }
    pub fn lossy_verification(&self) -> Option<LossyVerificationError> {
        self.0
            .transcode
            .as_ref()
            .and_then(transcode::Error::lossy_verification)
    }
    pub(super) fn options() -> SourceError {
        SourceError::new(SourceErrorInner {
            error: Error::new(super::ERR_OPTIONS),
            wrapped: false,
            options: true,
            no_representation: false,
            spool_limit: false,
            transcode: None,
            prepared_closed: false,
            prepared_binding: false,
        })
    }
    pub(super) fn prepared_closed() -> SourceError {
        SourceError::new(SourceErrorInner {
            prepared_closed: true,
            ..SourceError::plain(Error::new(super::ERR_PREPARED_CLOSED))
                .0
                .as_ref()
                .clone()
        })
    }
    pub(super) fn prepared_binding() -> SourceError {
        SourceError::new(SourceErrorInner {
            prepared_binding: true,
            ..SourceError::plain(Error::new(super::ERR_PREPARED_BINDING))
                .0
                .as_ref()
                .clone()
        })
    }
    pub(super) fn resource_limit() -> SourceError {
        SourceError::new(SourceErrorInner {
            spool_limit: true,
            ..SourceError::plain(Error::new(super::ERR_RESOURCE_LIMIT))
                .0
                .as_ref()
                .clone()
        })
    }
    pub(super) fn wrap(err: &Error) -> SourceError {
        Fail::of(err).wrapped()
    }
    pub(super) fn wrapped_spool_limit() -> SourceError {
        Fail::spool().wrapped()
    }
    fn transcode_error(e: transcode::Error) -> SourceError {
        SourceError::new(SourceErrorInner {
            error: Error::new(e.to_string()),
            wrapped: false,
            options: false,
            no_representation: false,
            spool_limit: false,
            transcode: Some(e),
            prepared_closed: false,
            prepared_binding: false,
        })
    }
    pub(super) fn plain(error: Error) -> SourceError {
        SourceError::new(SourceErrorInner {
            error,
            wrapped: false,
            options: false,
            no_representation: false,
            spool_limit: false,
            transcode: None,
            prepared_closed: false,
            prepared_binding: false,
        })
    }
}
impl fmt::Display for SourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0.error.text())
    }
}
impl std::error::Error for SourceError {}
impl From<SourceError> for Error {
    fn from(e: SourceError) -> Error {
        e.0.error
    }
}
#[derive(Debug, Clone, Default)]
pub(super) struct Fail {
    causes: Vec<Cause>,
    spool_limit: bool,
    not_exist: bool,
    no_representation: bool,
    transcode: Option<transcode::Error>,
}
impl Fail {
    fn kind(kind: Kind) -> Fail {
        Fail {
            causes: vec![Cause::Dimse(kind)],
            ..Fail::default()
        }
    }
    fn of(err: &Error) -> Fail {
        Fail {
            causes: err.causes().to_vec(),
            ..Fail::default()
        }
    }
    fn spool() -> Fail {
        Fail {
            spool_limit: true,
            ..Fail::default()
        }
    }
    fn transcode(err: transcode::Error) -> Fail {
        Fail {
            transcode: Some(err),
            ..Fail::default()
        }
    }
    fn object(err: object::Error) -> Fail {
        Fail::of(&Error::from(err))
    }
    fn join(mut self, kind: Kind) -> Fail {
        self.causes.insert(0, Cause::Dimse(kind));
        self
    }
    fn probe(&self) -> Error {
        Error::bare_with("", self.causes.clone())
    }
    fn is_transcode(&self, kind: T) -> bool {
        self.transcode.as_ref().is_some_and(|e| e.is(kind))
    }
    fn resource_limit(&self) -> bool {
        self.spool_limit || self.is_transcode(T::ResourceLimit)
    }
    fn reason(&self) -> (&'static str, bool) {
        let probe = self.probe();
        if probe.is_canceled() {
            ("canceled", false)
        } else if probe.is_deadline() {
            ("deadline", false)
        } else if probe.is(Kind::StoreSourceChanged) {
            ("source-changed", false)
        } else if probe.is(Kind::StoreInvalidSource) {
            ("identity-or-source-invalid", false)
        } else if self.is_transcode(T::EncoderUnavailable) {
            ("encoder-unavailable", true)
        } else if self.is_transcode(T::CodecRegistryNil)
            || self.is_transcode(T::Pixel(
                crate::pixel::pixeldata::PixelErrorKind::CodecNotFound,
            ))
        {
            ("decoder-unavailable", true)
        } else if self.not_exist {
            ("runtime-unavailable", true)
        } else if self.is_transcode(T::LossyDisallowed) {
            ("lossy-disallowed", false)
        } else if self.is_transcode(T::PixelError) {
            ("lossy-error-exceeded", false)
        } else if self.is_transcode(T::ComparisonUnsupported) {
            ("lossy-comparison-unsupported", false)
        } else if self.is_transcode(T::VerificationOptions) {
            ("verification-options-invalid", false)
        } else if self.resource_limit() {
            ("resource-limit", false)
        } else if self.is_transcode(T::Unsupported)
            || self.is_transcode(T::UnsupportedEncoderMetadata)
        {
            ("profile-unsupported", false)
        } else {
            ("preparation-failed", false)
        }
    }
    pub(super) fn wrapped(self) -> SourceError {
        let lossy = self
            .transcode
            .as_ref()
            .and_then(transcode::Error::lossy_verification);
        let text = match lossy {
            Some(v) => format!(
                "{}: {}: frame={} measured={} limit={} stored-sample-units block-side={}",
                super::ERR_PREPARATION,
                transcode::ERR_PIXEL_ERROR,
                v.frame,
                crate::numtext::format_float(v.measured, 'g', -1),
                crate::numtext::format_float(v.limit, 'g', -1),
                v.average_block_size
            ),
            None => super::ERR_PREPARATION.to_string(),
        };
        SourceError::new(SourceErrorInner {
            error: Error::bare_with(text, self.causes),
            wrapped: true,
            options: false,
            no_representation: self.no_representation,
            spool_limit: self.spool_limit,
            transcode: self.transcode,
            prepared_closed: false,
            prepared_binding: false,
        })
    }
}
struct Budget {
    remaining: i64,
}
#[derive(Clone)]
pub(super) struct Plan {
    pub(super) input: StoreDescriptor,
    pub(super) output: StoreDescriptor,
    pub(super) digest: [u8; 32],
    derived_uid: String,
}
pub(super) struct Prepared {
    pub(super) plan: Plan,
    _dir: SpoolDir,
    pub(super) payloads: BTreeMap<String, (Arc<[u8]>, usize)>,
    pub(super) spool_bytes: i64,
}
pub struct Source {
    base: Box<dyn StoreSource>,
    opts: SourceOptions,
    target: Option<Syntax>,
    plan: Mutex<Option<Plan>>,
    report: Arc<Mutex<Report>>,
}
pub fn new_source(
    base: Box<dyn StoreSource>,
    mut opts: SourceOptions,
) -> std::result::Result<Source, SourceError> {
    if opts.max_spool_bytes < 0 || opts.fallback_original && opts.transcode.allow_lossy {
        return Err(SourceError::options());
    }
    opts.transcode = transcode::resolve_verification_options(opts.transcode)
        .map_err(SourceError::transcode_error)?;
    let mut target = None;
    if !opts.target.is_empty() {
        let syntax = lookup(normalize_uid(&opts.target)).ok_or_else(SourceError::options)?;
        opts.target = syntax.uid.to_string();
        target = Some(syntax);
    } else if opts.fallback_original
        || opts.transcode.allow_lossy
        || opts.transcode.force_reencode
        || opts.transcode.max_pixel_error.is_some()
        || opts.transcode.average_block_size != 0
    {
        return Err(SourceError::options());
    }
    if opts.max_spool_bytes == 0 {
        opts.max_spool_bytes = 2 << 30;
    }
    opts.transcode.limits =
        transcode::resolve_limits(opts.transcode.limits).map_err(SourceError::transcode_error)?;
    Ok(Source {
        base,
        opts,
        target,
        plan: Mutex::new(None),
        report: Arc::default(),
    })
}
fn requires_codec(uid: &str) -> bool {
    crate::transfer::default_registry()
        .get(uid)
        .is_some_and(|s| s.requires_codec())
}
fn incremental_profile(source: &Syntax, target: &Syntax, options: &TranscodeOptions) -> bool {
    if target.encapsulated
        && target.uid != ENCAPSULATED_UNCOMPRESSED
        && let Some(caps) = options
            .encoders
            .as_ref()
            .and_then(|r| r.capabilities_of(target.uid))
    {
        if !caps.lossless {
            return false;
        }
        let decodable = options.decoder.is_some_and(|r| r.decodes(target.uid));
        if !options.skip_verification && !decodable {
            return false;
        }
    }
    super::incremental_profile(source, target, options)
}
fn spool_write(
    out: &mut Vec<u8>,
    chunk: &[u8],
    limit: &mut i64,
    budget: &mut Budget,
) -> std::result::Result<(), ()> {
    let n = chunk.len() as i64;
    if n > *limit || n > budget.remaining {
        return Err(());
    }
    out.extend_from_slice(chunk);
    *limit -= n;
    budget.remaining -= n;
    Ok(())
}
struct Incremental {
    file: File,
    report: TranscodeReport,
    bytes: Vec<u8>,
    offset: usize,
}
impl Source {
    pub fn report(&self) -> Report {
        lock(&self.report).clone()
    }
    pub fn record_result(&self, result: &StoreItemResult) {
        let mut report = lock(&self.report);
        report.selected_transfer_syntax_uid = result.negotiated_transfer_syntax_uid.clone();
        report.negotiation = result.outcome.as_str().to_string();
    }
    fn read_options(&self) -> ReadFileOptions {
        let limits = &self.opts.transcode.limits;
        let mut o = self.opts.dictionary_options;
        o.max_total_bytes = limits.max_input_bytes;
        o.max_element_bytes = limits.max_input_bytes;
        o.max_pixel_data_bytes = limits.max_input_bytes;
        o.max_elements = limits.max_elements;
        o.max_fragments = limits.max_fragments;
        o.max_sequence_depth = limits.max_depth;
        o
    }
    pub fn inspect_source(&self, ctx: &Ctx) -> std::result::Result<StoreDescriptor, SourceError> {
        if self.target.is_none() {
            return self.base.inspect(ctx).map_err(SourceError::plain);
        }
        if let Some(e) = context_err(ctx) {
            return Err(SourceError::plain(e));
        }
        let mut plan = lock(&self.plan);
        if let Some(e) = context_err(ctx) {
            return Err(SourceError::plain(e));
        }
        if let Some(plan) = plan.as_ref() {
            return Ok(plan.output.clone());
        }
        let (prepared, report) = self.prepare(ctx, None);
        match prepared {
            Ok(prepared) => {
                *lock(&self.report) = report;
                let output = prepared.plan.output.clone();
                *plan = Some(prepared.plan);
                Ok(output)
            }
            Err(fail) => {
                *lock(&self.report) = self.failed_report(report, &fail);
                Err(fail.wrapped())
            }
        }
    }
    pub fn open_source(&self, ctx: &Ctx) -> std::result::Result<OpenedStoreSource, SourceError> {
        let Some(target) = self.target else {
            return self.base.open(ctx).map_err(SourceError::plain);
        };
        self.inspect_source(ctx)?;
        let plan = {
            if let Some(e) = context_err(ctx) {
                return Err(SourceError::plain(e));
            }
            lock(&self.plan).clone()
        };
        let (prepared, report) = self.prepare(ctx, plan.as_ref());
        let prepared = match prepared {
            Ok(prepared) => prepared,
            Err(fail) => {
                *lock(&self.report) = self.failed_report(report, &fail);
                return Err(fail.wrapped());
            }
        };
        let descriptor = prepared.plan.output.clone();
        let handle: Arc<Mutex<Option<Prepared>>> = Arc::new(Mutex::new(Some(prepared)));
        let shared = Arc::clone(&self.report);
        let lossy = report.lossy_verification;
        let write_handle = Arc::clone(&handle);
        let write: StoreDataSetWriter = Arc::new(move |write_ctx, sink, syntax| {
            let (data, offset) = {
                let guard = lock(&write_handle);
                let Some(prepared) = guard.as_ref() else {
                    return Err(Error::sentinel(Kind::StoreInvalidSource));
                };
                let uid = normalize_uid(syntax.uid);
                let Some((data, offset)) = prepared.payloads.get(uid) else {
                    return Err(Error::sentinel(Kind::StoreTransferSyntax));
                };
                let mut report = lock(&shared);
                report.selected_transfer_syntax_uid = uid.to_string();
                report.negotiation = "selected".to_string();
                report.verification_failure = None;
                report.lossy_verification = None;
                if uid == target.uid {
                    report.lossy_verification = lossy;
                }
                (Arc::clone(data), *offset)
            };
            let mut rest = &data[offset..];
            while !rest.is_empty() {
                if let Some(e) = context_err(write_ctx) {
                    return Err(Fail::of(&e).wrapped().into());
                }
                let n = rest.len().min(64 << 10);
                sink(&rest[..n]).map_err(|e| Error::from(Fail::of(&e).wrapped()))?;
                rest = &rest[n..];
            }
            Ok(())
        });
        let close: StoreSourceCloser = Box::new(move || {
            lock(&handle).take();
            Ok(())
        });
        Ok(OpenedStoreSource {
            descriptor,
            write_data_set: Some(write),
            close: Some(close),
            before_store: None,
            read_metadata: None,
        })
    }
    fn failed_report(&self, mut report: Report, fail: &Fail) -> Report {
        let (reason, runtime_missing) = fail.reason();
        if report.candidates.is_empty() {
            report.candidates.push(Candidate {
                transfer_syntax_uid: self.opts.target.clone(),
                operation: "preparation".to_string(),
                ..Candidate::default()
            });
        }
        for candidate in &mut report.candidates {
            candidate.producible = false;
            if candidate.reason.is_empty() {
                candidate.reason = reason.to_string();
                candidate.runtime_missing = runtime_missing;
            }
        }
        report
    }
    fn prepare(
        &self,
        parent: &Ctx,
        expected: Option<&Plan>,
    ) -> (std::result::Result<Prepared, Fail>, Report) {
        let mut report = Report::default();
        let result = self.prepare_into(parent, expected, &mut report, self.opts.max_spool_bytes);
        (result, report)
    }
    pub(super) fn prepare_within(
        &self,
        parent: &Ctx,
        max_spool: i64,
    ) -> (std::result::Result<Prepared, Fail>, Report) {
        let mut report = Report::default();
        let result = self.prepare_into(parent, None, &mut report, max_spool);
        (result, report)
    }
    pub(super) fn base(&self) -> &dyn StoreSource {
        self.base.as_ref()
    }
    pub(super) fn max_spool_bytes(&self) -> i64 {
        self.opts.max_spool_bytes
    }
    pub(super) fn max_input_bytes(&self) -> i64 {
        self.opts.transcode.limits.max_input_bytes
    }
    fn prepare_into(
        &self,
        parent: &Ctx,
        expected: Option<&Plan>,
        report: &mut Report,
        max_spool: i64,
    ) -> std::result::Result<Prepared, Fail> {
        let Some(target) = self.target else {
            return Err(Fail::kind(Kind::StoreInvalidSource));
        };
        let limits = self.opts.transcode.limits;
        let ctx =
            parent.with_timeout(Duration::from_nanos(limits.max_duration_nanos.max(0) as u64));
        let ctx_fail = |ctx: &Ctx| context_err(ctx).map(|e| Fail::of(&e));
        let input = self.base.inspect(&ctx).map_err(|e| Fail::of(&e))?;
        let original = lookup(&input.transfer_syntax_uid);
        let Some(original) = original.filter(|_| {
            is_valid_uid(input.sop_class_uid.as_bytes())
                && is_valid_uid(input.sop_instance_uid.as_bytes())
        }) else {
            return Err(Fail::kind(Kind::StoreInvalidSource));
        };
        report.original_transfer_syntax_uid = original.uid.to_string();
        report.negotiation = "not-negotiated".to_string();
        if expected.is_some_and(|e| !same_descriptor(&e.input, &input)) {
            return Err(Fail::kind(Kind::StoreSourceChanged));
        }
        if let Some(f) = ctx_fail(&ctx) {
            return Err(f);
        }
        let dir = SpoolDir::new(&self.opts.spool_directory).ok_or(Fail {
            not_exist: true,
            ..Fail::default()
        })?;
        let mut budget = Budget {
            remaining: max_spool,
        };
        let mut opened = self
            .base
            .open(&ctx)
            .map_err(|e| Fail::of(&e).join(Kind::StoreInvalidSource))?;
        let close = opened.close.take();
        let (Some(close), Some(write)) = (close, opened.write_data_set.clone()) else {
            if let Some(close) = opened.close.take() {
                let _ = close();
            }
            return Err(Fail::kind(Kind::StoreInvalidSource));
        };
        let copied = (|| {
            if !same_descriptor(&input, &opened.descriptor) {
                return Err(Fail::kind(Kind::StoreSourceChanged));
            }
            let mut data: Vec<u8> = Vec::new();
            let mut limit = limits.max_input_bytes;
            let mut over = false;
            let written = {
                let mut sink = |chunk: &[u8]| -> Result<()> {
                    if let Some(e) = context_err(&ctx) {
                        return Err(e);
                    }
                    spool_write(&mut data, chunk, &mut limit, &mut budget).map_err(|()| {
                        over = true;
                        Error::new(super::ERR_RESOURCE_LIMIT)
                    })
                };
                write(&ctx, &mut sink, &original)
            };
            if over {
                return Err(Fail::spool());
            }
            written.map_err(|e| Fail::of(&e))?;
            Ok(data)
        })();
        let closed = close();
        let input_bytes = match (copied, closed) {
            (Ok(data), Ok(())) => data,
            (Ok(_), Err(e)) => return Err(Fail::of(&e)),
            (Err(f), _) => return Err(f),
        };
        let input_size = input_bytes.len() as i64;
        if let Some(f) = ctx_fail(&ctx) {
            return Err(f);
        }
        let digest: [u8; 32] = Sha256::digest(&input_bytes).into();
        if expected.is_some_and(|e| e.digest != digest) {
            return Err(Fail::kind(Kind::StoreSourceChanged));
        }
        let input_bytes: Arc<[u8]> = Arc::from(input_bytes);
        let read_options = self.read_options();
        let dataset = object::read_data_set_with_options(
            Box::new(MemFile::new(Rc::from(&input_bytes[..]))),
            original,
            read_options,
        )
        .map_err(Fail::object)?;
        let actual = new_file_store_source(
            Some(Arc::new(file_of(dataset.clone(), original))),
            input_size,
        )
        .inspect(&ctx);
        let mut actual = match actual {
            Ok(actual)
                if actual.sop_class_uid == input.sop_class_uid
                    && actual.sop_instance_uid == input.sop_instance_uid =>
            {
                actual
            }
            Ok(_) => return Err(Fail::kind(Kind::StoreSourceChanged)),
            Err(e) => return Err(Fail::of(&e).join(Kind::StoreSourceChanged)),
        };
        actual.origin = input.origin.clone();
        let mut payloads: BTreeMap<String, (Arc<[u8]>, usize)> = BTreeMap::new();
        payloads.insert(original.uid.to_string(), (Arc::clone(&input_bytes), 0));
        let mut plan = Plan {
            input: input.clone(),
            output: StoreDescriptor::default(),
            digest,
            derived_uid: String::new(),
        };
        let force = self.opts.transcode.force_reencode;
        let mut target_len = 0i64;
        let mut candidate = Candidate {
            transfer_syntax_uid: target.uid.to_string(),
            ..Candidate::default()
        };
        if target.uid == original.uid && !force {
            candidate.operation = "pass-through".to_string();
            candidate.producible = true;
            plan.output = actual.clone();
        } else {
            candidate.operation =
                match (requires_codec(original.uid), requires_codec(target.uid)) {
                    (true, true) => "decode-encode",
                    (true, false) => "decode",
                    (false, true) => "encode",
                    (false, false) => "native-reencode",
                }
                .to_string();
            let transformed = self.transform(
                &ctx,
                &dir,
                &input_bytes,
                &dataset,
                &original,
                &target,
                &mut budget,
                expected,
                &input,
                report,
                &mut candidate,
                &mut plan,
            );
            match transformed {
                Ok(Some((bytes, offset))) => {
                    target_len = bytes.len() as i64;
                    payloads.insert(target.uid.to_string(), (bytes, offset));
                    candidate.producible = true;
                }
                Ok(None) => {}
                Err(Step::Return(f)) => return Err(f),
                Err(Step::Failed(f)) => {
                    if let Some(v) = f
                        .transcode
                        .as_ref()
                        .and_then(transcode::Error::lossy_verification)
                    {
                        report.verification_failure = Some(v);
                    }
                    let (reason, runtime) = f.reason();
                    candidate.reason = reason.to_string();
                    candidate.runtime_missing = runtime;
                    report.candidates.push(candidate.clone());
                    if let Some(c) = ctx_fail(&ctx) {
                        return Err(c);
                    }
                    if f.resource_limit() {
                        return Err(f);
                    }
                    if !self.opts.fallback_original {
                        return Err(Fail {
                            no_representation: true,
                            ..f
                        });
                    }
                    plan.output = actual.clone();
                }
            }
        }
        if candidate.producible {
            report.candidates.push(candidate.clone());
            plan.output.writable_transfer_syntax_uids = vec![target.uid.to_string()];
        } else {
            plan.output.writable_transfer_syntax_uids = Vec::new();
        }
        if self.opts.fallback_original && (target.uid != original.uid || !candidate.producible) {
            report.candidates.push(Candidate {
                transfer_syntax_uid: original.uid.to_string(),
                operation: "pass-through".to_string(),
                producible: true,
                ..Candidate::default()
            });
            plan.output
                .writable_transfer_syntax_uids
                .push(original.uid.to_string());
            if input_size > plan.output.size {
                plan.output.size = input_size;
            }
        } else if target.uid != original.uid {
            payloads.remove(original.uid);
        }
        plan.output.origin = input.origin.clone();
        if plan.output.sop_class_uid != input.sop_class_uid
            || (!candidate.lossy && plan.output.sop_instance_uid != input.sop_instance_uid)
        {
            return Err(Fail::kind(Kind::StoreSourceChanged));
        }
        if expected.is_some_and(|e| {
            !same_descriptor(&e.output, &plan.output) || e.derived_uid != plan.derived_uid
        }) {
            return Err(Fail::kind(Kind::StoreSourceChanged));
        }
        if let Some(f) = ctx_fail(&ctx) {
            return Err(f);
        }
        Ok(Prepared {
            plan,
            _dir: dir,
            payloads,
            spool_bytes: input_size + target_len,
        })
    }
    #[allow(clippy::too_many_arguments)]
    fn transform(
        &self,
        ctx: &Ctx,
        dir: &SpoolDir,
        input_bytes: &[u8],
        dataset: &Object,
        original: &Syntax,
        target: &Syntax,
        budget: &mut Budget,
        expected: Option<&Plan>,
        input: &StoreDescriptor,
        report: &mut Report,
        candidate: &mut Candidate,
        plan: &mut Plan,
    ) -> std::result::Result<Option<Payload>, Step> {
        let limits = self.opts.transcode.limits;
        let mut incremental = None;
        if incremental_profile(original, target, &self.opts.transcode) {
            match self.prepare_incremental(ctx, dir, input_bytes, original, target, budget) {
                Ok(prepared) => incremental = Some(prepared),
                Err(f) if f.is_transcode(T::StreamingIneligible) => {}
                Err(f) => return Err(Step::Failed(f)),
            }
        }
        let (mut transformed, transform_report, output) = match incremental {
            Some(prepared) => {
                let Incremental {
                    file,
                    report,
                    bytes,
                    offset,
                } = prepared;
                (file.dataset, report, Some((bytes, offset)))
            }
            None => {
                let (object, report) = transcode::transcode_data_set(
                    dataset,
                    original.uid,
                    target.uid,
                    &self.opts.transcode,
                )
                .map_err(|e| Step::Failed(Fail::transcode(e)))?;
                (object, report, None)
            }
        };
        report.lossy_verification = transform_report.lossy_verification;
        candidate.lossy = transform_report.lossy;
        if transform_report.lossy {
            if !self.opts.transcode.allow_lossy || self.opts.fallback_original {
                return Err(Step::Return(Fail::transcode(transcode::Error::new(
                    T::LossyDisallowed,
                    transcode::ERR_LOSSY_DISALLOWED,
                ))));
            }
            let mut uid = transformed
                .get_uid(TAG_SOP_INSTANCE_UID)
                .unwrap_or_default();
            if !is_valid_uid(uid.as_bytes()) || uid == input.sop_instance_uid {
                return Err(Step::Return(Fail::kind(Kind::StoreInvalidSource)));
            }
            if let Some(expected) = expected {
                if expected.derived_uid.is_empty() {
                    return Err(Step::Return(Fail::kind(Kind::StoreSourceChanged)));
                }
                uid = expected.derived_uid.clone();
                transformed.put(Element {
                    header: ElementHeader {
                        tag: TAG_SOP_INSTANCE_UID,
                        vr: Vr::UI,
                        length: 0,
                        length_set: false,
                    },
                    value: Value::Strings(vec![uid.clone()]),
                });
            }
            plan.derived_uid = uid;
        }
        let (bytes, offset, size) = match output {
            Some((bytes, offset)) => {
                let size = (bytes.len() - offset) as i64;
                (bytes, offset, size)
            }
            None => {
                let written = object::writer::write_data_set(&transformed, target.uid)
                    .map_err(|e| Step::Failed(Fail::object(e)))?;
                let mut out = Vec::with_capacity(written.len());
                let mut limit = limits.max_output_bytes;
                if spool_write(&mut out, &written, &mut limit, budget).is_err() {
                    return Err(Step::Failed(Fail::spool()));
                }
                let size = out.len() as i64;
                (out, 0, size)
            }
        };
        let file = file_of(transformed, *target);
        plan.output = new_file_store_source(Some(Arc::new(file)), size)
            .inspect(ctx)
            .map_err(|e| Step::Failed(Fail::of(&e)))?;
        Ok(Some((Arc::from(bytes), offset)))
    }
    fn prepare_incremental(
        &self,
        ctx: &Ctx,
        dir: &SpoolDir,
        input_bytes: &[u8],
        original: &Syntax,
        target: &Syntax,
        budget: &mut Budget,
    ) -> std::result::Result<Incremental, Fail> {
        if budget.remaining <= 0 {
            return Err(Fail::spool());
        }
        let limits = self.opts.transcode.limits;
        let input_path = dir.0.join("source.dataset");
        let output_path = dir.0.join("target.dataset");
        std::fs::write(&input_path, input_bytes).map_err(|_| Fail::default())?;
        let mut options = self.opts.transcode.clone();
        options.write_options = Default::default();
        let measured = transcode::transcode_data_set_path(
            &input_path,
            &output_path,
            original.uid,
            target.uid,
            &options,
            transcode::DataSetPathOptions {
                source_offset: 0,
                max_temporary_bytes: budget.remaining,
            },
        )
        .map_err(Fail::transcode)?;
        let data = std::fs::read(&output_path).map_err(|_| Fail::default())?;
        let size = data.len() as i64;
        if size > budget.remaining {
            return Err(Fail::spool());
        }
        budget.remaining -= size;
        let read_options = ReadFileOptions {
            max_total_bytes: limits.max_output_bytes,
            max_element_bytes: limits.max_output_bytes,
            max_pixel_data_bytes: limits.max_output_bytes,
            max_elements: limits.max_elements,
            max_fragments: limits.max_fragments,
            max_sequence_depth: limits.max_depth,
            ..self.opts.dictionary_options
        };
        let shared: Rc<[u8]> = Rc::from(data.as_slice());
        let header = object::read_part10_header_mem(&MemFile::new(shared), &read_options)
            .map_err(Fail::object)?;
        if header.data_set_offset < 0
            || header.data_set_offset >= size
            || header.transfer_syntax.uid != target.uid
        {
            return Err(Fail::kind(Kind::StoreInvalidSource));
        }
        let file = object::read_file_with_options(&data, read_options)
            .map_err(|(_, e)| Fail::object(e))?;
        if let Some(e) = context_err(ctx) {
            return Err(Fail::of(&e));
        }
        Ok(Incremental {
            file,
            report: measured.report,
            bytes: data,
            offset: header.data_set_offset as usize,
        })
    }
}
type Payload = (Arc<[u8]>, usize);
enum Step {
    Failed(Fail),
    Return(Fail),
}
fn file_of(dataset: Object, syntax: Syntax) -> File {
    File {
        preamble: Vec::new(),
        meta: Object::default(),
        dataset,
        transfer_syntax: syntax,
        partial_read: None,
        incomplete_pixel_data: None,
    }
}
impl StoreSource for Source {
    fn inspect(&self, ctx: &Ctx) -> Result<StoreDescriptor> {
        self.inspect_source(ctx).map_err(Error::from)
    }
    fn open(&self, ctx: &Ctx) -> Result<OpenedStoreSource> {
        self.open_source(ctx).map_err(Error::from)
    }
}
