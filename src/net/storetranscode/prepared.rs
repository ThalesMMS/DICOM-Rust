use super::lock;
use super::source::{Prepared, Source, SourceError, SourceOptions, new_source};
#[cfg(feature = "jpeg")]
use crate::codec::jpeg;
#[cfg(feature = "jpeg-lossless")]
use crate::codec::jpeglossless;
#[cfg(feature = "jpeg-ls")]
use crate::codec::jpegls;
use crate::net::dimse::{
    Error, Kind, OpenedStoreSource, Result, StoreDataSetWriter, StoreDescriptor, StoreItemResult,
    StoreSource, StoreSourceCloser, context_err,
};
use crate::net::ul::{CancelToken, Ctx};
use crate::object::transfer::{Syntax, lookup, normalize_uid};
use crate::pixel::transcode::{DecoderRegistry, EncoderCounter, EncoderRegistry, FrameEncoder};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
pub const ERR_PREPARED_CLOSED: &str = "dicom: prepared representation session is closed";
pub const ERR_PREPARED_BINDING: &str =
    "dicom: prepared representation binding unavailable or changed";
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PreparedSessionOptions {
    pub max_retained_bytes: i64,
    pub max_entries: i64,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PreparedSessionStats {
    pub reserved_bytes: i64,
    pub retained_bytes: i64,
    pub peak_bytes: i64,
    pub entries: i64,
    pub closing: bool,
    pub closed: bool,
}
#[derive(Clone)]
pub struct PreparedSession(Arc<SessionInner>);
struct SessionInner {
    ctx: Ctx,
    cancel: CancelToken,
    options: PreparedSessionOptions,
    state: Mutex<SessionState>,
}
#[derive(Default)]
struct SessionState {
    sources: Vec<Arc<Control>>,
    next_id: u64,
    stats: PreparedSessionStats,
    closing: bool,
    ended: bool,
}
impl PreparedSession {
    pub fn new(
        parent: &Ctx,
        mut options: PreparedSessionOptions,
    ) -> std::result::Result<PreparedSession, SourceError> {
        if options.max_retained_bytes < 0 || options.max_entries < 0 {
            return Err(SourceError::options());
        }
        if options.max_retained_bytes == 0 {
            options.max_retained_bytes = 2 << 30;
        }
        if options.max_entries == 0 {
            options.max_entries = 1024;
        }
        let cancel = CancelToken::new();
        Ok(PreparedSession(Arc::new(SessionInner {
            ctx: parent.with_cancel(&cancel),
            cancel,
            options,
            state: Mutex::new(SessionState::default()),
        })))
    }
    pub fn stats(&self) -> PreparedSessionStats {
        self.0.notice_end();
        lock(&self.0.state).stats
    }
    pub fn new_source(
        &self,
        base: Box<dyn StoreSource>,
        mut opts: SourceOptions,
        binding: Option<&PreparedBinding>,
    ) -> std::result::Result<PreparedSource, SourceError> {
        let Some(binding) = binding else {
            return Err(SourceError::options());
        };
        if opts.target.is_empty() {
            return Err(SourceError::options());
        }
        if opts.transcode.decoder.is_some() || opts.transcode.encoders.is_some() {
            return Err(SourceError::prepared_binding());
        }
        self.0.notice_end();
        let token = binding.acquire(&self.0.ctx)?;
        opts.transcode.decoder = Some(DecoderRegistry::BuiltinWithoutJpeg2000);
        opts.transcode.encoders = Some(binding.0.encoders.clone());
        let source = new_source(base, opts)?;
        let cancel = CancelToken::new();
        let mut state = lock(&self.0.state);
        if state.closing || self.0.ctx.err().is_some() {
            return Err(SourceError::prepared_closed());
        }
        if state.sources.len() as i64 >= self.0.options.max_entries {
            return Err(SourceError::resource_limit());
        }
        let control = Arc::new(Control {
            id: state.next_id,
            session: Arc::clone(&self.0),
            ctx: self.0.ctx.with_cancel(&cancel),
            cancel,
            state: Mutex::new(ControlState::default()),
            revalidation: AtomicI64::new(0),
            copy: AtomicI64::new(0),
        });
        state.next_id += 1;
        state.sources.push(Arc::clone(&control));
        state.stats.entries += 1;
        Ok(PreparedSource {
            control,
            source,
            binding: binding.clone(),
            token,
            gate: Mutex::new(()),
        })
    }
    pub fn close(&self, ctx: &Ctx) -> std::result::Result<(), SourceError> {
        let all = {
            let mut state = lock(&self.0.state);
            state.closing = true;
            state.stats.closing = true;
            state.sources.clone()
        };
        self.0.request_close();
        let mut result = Ok(());
        for control in &all {
            if let Err(e) = control.await_cleanup(ctx) {
                result = Err(e);
            }
        }
        let mut state = lock(&self.0.state);
        if result.is_ok()
            && state.stats.entries == 0
            && state.stats.reserved_bytes == 0
            && state.stats.retained_bytes == 0
        {
            state.stats.closed = true;
        }
        state.ended = true;
        result
    }
}
impl SessionInner {
    fn notice_end(&self) {
        if self.ctx.err().is_none() {
            return;
        }
        {
            let mut state = lock(&self.state);
            if state.ended {
                return;
            }
            state.ended = true;
        }
        self.request_close();
    }
    fn request_close(&self) {
        let all = {
            let mut state = lock(&self.state);
            state.closing = true;
            state.stats.closing = true;
            state.sources.clone()
        };
        self.cancel.cancel();
        for control in all {
            control.request_close();
        }
    }
    fn reserve(&self, n: i64) -> std::result::Result<i64, SourceError> {
        let mut state = lock(&self.state);
        if state.closing || self.ctx.err().is_some() {
            return Err(SourceError::prepared_closed());
        }
        let available = self.options.max_retained_bytes
            - state.stats.reserved_bytes
            - state.stats.retained_bytes;
        if available <= 0 {
            return Err(SourceError::resource_limit());
        }
        let n = n.min(available);
        state.stats.reserved_bytes += n;
        let peak = state.stats.reserved_bytes + state.stats.retained_bytes;
        if peak > state.stats.peak_bytes {
            state.stats.peak_bytes = peak;
        }
        Ok(n)
    }
    fn publish(&self, reserved: i64, retained: i64) {
        let mut state = lock(&self.state);
        state.stats.reserved_bytes -= reserved;
        state.stats.retained_bytes += retained;
    }
    fn release_reserved(&self, n: i64) {
        lock(&self.state).stats.reserved_bytes -= n;
    }
    fn release_retained(&self, n: i64) {
        lock(&self.state).stats.retained_bytes -= n;
    }
    fn release_entry(&self, id: u64) {
        let mut state = lock(&self.state);
        if let Some(i) = state.sources.iter().position(|c| c.id == id) {
            state.sources.remove(i);
            state.stats.entries -= 1;
            if state.closing
                && state.stats.entries == 0
                && state.stats.reserved_bytes == 0
                && state.stats.retained_bytes == 0
            {
                state.stats.closed = true;
            }
        }
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BuiltinPreparedBindingOptions {
    pub allow_lossy: bool,
    pub jpeg_quality: i64,
}
#[derive(Clone)]
pub struct PreparedBinding(Arc<BindingInner>);
struct BindingInner {
    closed: Mutex<bool>,
    token: [u8; 32],
    encoders: EncoderRegistry,
    counter: Arc<EncoderCounter>,
}
pub fn new_builtin_prepared_binding(
    mut options: BuiltinPreparedBindingOptions,
) -> std::result::Result<PreparedBinding, SourceError> {
    if options.jpeg_quality == 0 {
        options.jpeg_quality = 90;
    }
    if !(1..=100).contains(&options.jpeg_quality) {
        return Err(SourceError::options());
    }
    let unavailable = |_| SourceError::prepared_binding();
    let mut encoders = EncoderRegistry::new();
    #[cfg(feature = "jpeg-ls")]
    let lossless = jpegls::Encoder::with_options(jpegls::EncoderOptions {
        near: 0,
        allow_lossy: false,
        interleave: 0,
    })
    .map_err(|_| SourceError::prepared_binding())?;
    encoders
        .register_own(FrameEncoder::Rle)
        .map_err(unavailable)?;
    #[cfg(feature = "jpeg-ls")]
    encoders
        .register_own(FrameEncoder::JpegLs(lossless))
        .map_err(unavailable)?;
    #[cfg(feature = "jpeg-lossless")]
    encoders
        .register_own(FrameEncoder::JpegLossless(jpeglossless::Encoder::sv1()))
        .map_err(unavailable)?;
    #[cfg(feature = "jpeg-lossless")]
    let process14 =
        jpeglossless::Encoder::process14(1).map_err(|_| SourceError::prepared_binding())?;
    #[cfg(feature = "jpeg-lossless")]
    encoders
        .register_own(FrameEncoder::JpegLossless(process14))
        .map_err(unavailable)?;
    encoders
        .register_own(FrameEncoder::Deflate(-1))
        .map_err(unavailable)?;
    #[cfg(feature = "jpeg")]
    if options.allow_lossy {
        let baseline = jpeg::Encoder::with_options(jpeg::EncoderOptions {
            quality: options.jpeg_quality as _,
            subsampling: jpeg::Subsampling(0),
            optimize_huffman: false,
        })
        .map_err(|_| SourceError::options())?;
        encoders
            .register_own(FrameEncoder::JpegBaseline(baseline))
            .map_err(unavailable)?;
    }
    let uids = encoders.registered_uids();
    let capabilities: Vec<String> = uids
        .iter()
        .map(|uid| format!("{:?}", encoders.capabilities_of(uid)))
        .collect();
    let description = format!(
        "builtin-prepared-v1|{}|{}|{:?}|{:?}|{:?}",
        options.allow_lossy,
        options.jpeg_quality,
        crate::pixel::pixeldata::registered_codec_uids(),
        uids,
        capabilities
    );
    let counter = Arc::new(EncoderCounter::default());
    Ok(PreparedBinding(Arc::new(BindingInner {
        closed: Mutex::new(false),
        token: Sha256::digest(description.as_bytes()).into(),
        encoders: encoders.with_counter(Arc::clone(&counter)),
        counter,
    })))
}
impl PreparedBinding {
    fn acquire(&self, ctx: &Ctx) -> std::result::Result<[u8; 32], SourceError> {
        if let Some(e) = context_err(ctx) {
            return Err(SourceError::plain(e));
        }
        if *lock(&self.0.closed) {
            return Err(SourceError::prepared_binding());
        }
        Ok(self.0.token)
    }
    pub fn close(&self) {
        *lock(&self.0.closed) = true;
    }
    pub fn encoder_calls(&self) -> u64 {
        self.0.counter.calls()
    }
    pub fn encoder_time(&self) -> Duration {
        self.0.counter.duration()
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PreparedSourceTiming {
    pub original_validation: Duration,
    pub spool_integrity: Duration,
    pub copy: Duration,
}
pub struct PreparedSource {
    control: Arc<Control>,
    source: Source,
    binding: PreparedBinding,
    token: [u8; 32],
    gate: Mutex<()>,
}
struct Control {
    id: u64,
    session: Arc<SessionInner>,
    ctx: Ctx,
    cancel: CancelToken,
    state: Mutex<ControlState>,
    revalidation: AtomicI64,
    copy: AtomicI64,
}
#[derive(Default)]
struct ControlState {
    cache: Option<Arc<Prepared>>,
    report: super::Report,
    retained: i64,
    handles: Vec<Arc<HandleShared>>,
    next_handle: u64,
    operations: i64,
    closing: bool,
    cleaned: bool,
}
impl Control {
    fn begin(&self, ctx: &Ctx) -> std::result::Result<Ctx, SourceError> {
        if let Some(e) = context_err(ctx) {
            return Err(SourceError::plain(e));
        }
        self.session.notice_end();
        let mut state = lock(&self.state);
        if state.closing || self.ctx.err().is_some() {
            return Err(SourceError::prepared_closed());
        }
        state.operations += 1;
        Ok(ctx.with_end_of(&self.ctx))
    }
    fn end(&self) {
        lock(&self.state).operations -= 1;
        self.cleanup();
    }
    fn cleanup(&self) {
        let (cache, retained) = {
            let mut state = lock(&self.state);
            if !state.closing || state.operations != 0 || !state.handles.is_empty() || state.cleaned
            {
                return;
            }
            state.cleaned = true;
            let retained = state.retained;
            state.retained = 0;
            (state.cache.take(), retained)
        };
        if let Some(cache) = cache {
            drop(cache);
            self.session.release_retained(retained);
        }
        self.session.release_entry(self.id);
    }
    fn request_close(&self) {
        let handles = {
            let mut state = lock(&self.state);
            state.closing = true;
            state.handles.clone()
        };
        self.cancel.cancel();
        for handle in handles {
            handle.request_close(self);
        }
        self.cleanup();
    }
    fn await_cleanup(&self, ctx: &Ctx) -> std::result::Result<(), SourceError> {
        loop {
            if lock(&self.state).cleaned {
                return Ok(());
            }
            if let Some(e) = context_err(ctx) {
                return Err(SourceError::plain(e));
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}
fn prepared_same_descriptor(a: &StoreDescriptor, b: &StoreDescriptor) -> bool {
    let empty = BTreeMap::new();
    super::same_descriptor(a, b)
        && a.origin == b.origin
        && a.transfer_syntax_sizes.as_ref().unwrap_or(&empty)
            == b.transfer_syntax_sizes.as_ref().unwrap_or(&empty)
}
impl PreparedSource {
    pub fn report(&self) -> super::Report {
        lock(&self.control.state).report.clone()
    }
    pub fn record_result(&self, result: &StoreItemResult) {
        let mut state = lock(&self.control.state);
        state.report.selected_transfer_syntax_uid = result.negotiated_transfer_syntax_uid.clone();
        state.report.negotiation = result.outcome.as_str().to_string();
    }
    pub fn timing(&self) -> PreparedSourceTiming {
        let d = |a: &AtomicI64| Duration::from_nanos(a.load(Ordering::Relaxed).max(0) as u64);
        PreparedSourceTiming {
            original_validation: d(&self.control.revalidation),
            spool_integrity: Duration::ZERO,
            copy: d(&self.control.copy),
        }
    }
    pub fn close(&self, ctx: &Ctx) -> std::result::Result<(), SourceError> {
        self.control.request_close();
        self.control.await_cleanup(ctx)
    }
    fn check_binding(&self, ctx: &Ctx) -> std::result::Result<(), SourceError> {
        if self.binding.acquire(ctx)? != self.token {
            return Err(SourceError::prepared_binding());
        }
        Ok(())
    }
    fn ensure(&self, ctx: &Ctx) -> std::result::Result<Arc<Prepared>, SourceError> {
        let _gate = lock(&self.gate);
        self.check_binding(ctx)?;
        if let Some(cache) = lock(&self.control.state).cache.clone() {
            return Ok(cache);
        }
        let session = &self.control.session;
        let reserved = session.reserve(self.source.max_spool_bytes())?;
        let (prepared, report) = self.source.prepare_within(ctx, reserved);
        let prepared = match prepared {
            Ok(prepared) => prepared,
            Err(fail) => {
                session.release_reserved(reserved);
                return Err(fail.wrapped());
            }
        };
        let retained = prepared.spool_bytes;
        if retained > reserved {
            session.release_reserved(reserved);
            return Err(SourceError::wrapped_spool_limit());
        }
        session.publish(reserved, retained);
        let prepared = Arc::new(prepared);
        let mut state = lock(&self.control.state);
        state.cache = Some(Arc::clone(&prepared));
        state.report = report;
        state.retained = retained;
        Ok(prepared)
    }
    fn validate_original(
        &self,
        ctx: &Ctx,
        cache: &Prepared,
    ) -> std::result::Result<(), SourceError> {
        let started = Instant::now();
        let result = self.validate_original_inner(ctx, cache);
        self.control
            .revalidation
            .fetch_add(started.elapsed().as_nanos() as i64, Ordering::Relaxed);
        result
    }
    fn validate_original_inner(
        &self,
        ctx: &Ctx,
        cache: &Prepared,
    ) -> std::result::Result<(), SourceError> {
        self.check_binding(ctx)?;
        let plan = &cache.plan;
        let base = self.source.base();
        let input = base.inspect(ctx).map_err(|e| SourceError::wrap(&e))?;
        if !prepared_same_descriptor(&plan.input, &input) {
            return Err(SourceError::plain(Error::sentinel(
                Kind::StoreSourceChanged,
            )));
        }
        let mut opened = base.open(ctx).map_err(|e| SourceError::wrap(&e))?;
        let (Some(close), Some(write)) = (opened.close.take(), opened.write_data_set.clone())
        else {
            if let Some(close) = opened.close.take() {
                let _ = close();
            }
            return Err(SourceError::plain(Error::sentinel(
                Kind::StoreInvalidSource,
            )));
        };
        let checked = (|| {
            if !prepared_same_descriptor(&plan.input, &opened.descriptor) {
                return Err(SourceError::plain(Error::sentinel(
                    Kind::StoreSourceChanged,
                )));
            }
            let Some(original) = lookup(&plan.input.transfer_syntax_uid) else {
                return Err(SourceError::plain(Error::sentinel(
                    Kind::StoreTransferSyntax,
                )));
            };
            let mut hash = Sha256::new();
            let limit = self.source.max_input_bytes();
            let (mut remaining, mut over) = (limit, false);
            let written = {
                let mut sink = |chunk: &[u8]| -> Result<()> {
                    if let Some(e) = context_err(ctx) {
                        return Err(e);
                    }
                    if chunk.len() as i64 > remaining {
                        over = true;
                        return Err(Error::new(super::ERR_RESOURCE_LIMIT));
                    }
                    hash.update(chunk);
                    remaining -= chunk.len() as i64;
                    Ok(())
                };
                write(ctx, &mut sink, &original)
            };
            if over {
                return Err(SourceError::wrapped_spool_limit());
            }
            written.map_err(|e| SourceError::wrap(&e))?;
            let digest: [u8; 32] = hash.finalize().into();
            if digest != plan.digest {
                return Err(SourceError::plain(Error::sentinel(
                    Kind::StoreSourceChanged,
                )));
            }
            Ok(())
        })();
        let closed = close();
        checked?;
        closed.map_err(|e| SourceError::wrap(&e))?;
        self.check_binding(ctx)
    }
    pub fn inspect_source(&self, ctx: &Ctx) -> std::result::Result<StoreDescriptor, SourceError> {
        let ctx = self.control.begin(ctx)?;
        let result = (|| {
            let cache = self.ensure(&ctx)?;
            self.validate_original(&ctx, &cache)?;
            Ok(cache.plan.output.clone())
        })();
        self.control.end();
        result
    }
    pub fn open_source(&self, ctx: &Ctx) -> std::result::Result<OpenedStoreSource, SourceError> {
        let ctx = self.control.begin(ctx)?;
        let result = self.open_inner(&ctx);
        self.control.end();
        result
    }
    fn open_inner(&self, ctx: &Ctx) -> std::result::Result<OpenedStoreSource, SourceError> {
        let cache = self.ensure(ctx)?;
        self.validate_original(ctx, &cache)?;
        let handle = {
            let mut state = lock(&self.control.state);
            if state.closing {
                return Err(SourceError::prepared_closed());
            }
            let handle = Arc::new(HandleShared {
                id: state.next_handle,
                prepared: Mutex::new(Some(Arc::clone(&cache))),
                gate: Mutex::new(()),
                state: Mutex::new(HandleState::default()),
            });
            state.next_handle += 1;
            state.handles.push(Arc::clone(&handle));
            handle
        };
        let descriptor = cache.plan.output.clone();
        let control = Arc::clone(&self.control);
        let write_handle = Arc::clone(&handle);
        let write: StoreDataSetWriter = Arc::new(move |write_ctx, sink, syntax| {
            write_handle.write(&control, write_ctx, sink, syntax)
        });
        let control = Arc::clone(&self.control);
        let close: StoreSourceCloser = Box::new(move || {
            handle.request_close(&control);
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
}
impl StoreSource for PreparedSource {
    fn inspect(&self, ctx: &Ctx) -> Result<StoreDescriptor> {
        self.inspect_source(ctx).map_err(Error::from)
    }
    fn open(&self, ctx: &Ctx) -> Result<OpenedStoreSource> {
        self.open_source(ctx).map_err(Error::from)
    }
}
struct HandleShared {
    id: u64,
    prepared: Mutex<Option<Arc<Prepared>>>,
    gate: Mutex<()>,
    state: Mutex<HandleState>,
}
#[derive(Default)]
struct HandleState {
    closed: bool,
    active: bool,
    released: bool,
    cancel: Option<CancelToken>,
}
impl HandleShared {
    fn release(&self, control: &Control) {
        {
            let mut state = lock(&self.state);
            if state.released || state.active {
                return;
            }
            state.released = true;
        }
        lock(&self.prepared).take();
        lock(&control.state).handles.retain(|h| h.id != self.id);
        control.cleanup();
    }
    fn request_close(&self, control: &Control) {
        let active = {
            let mut state = lock(&self.state);
            state.closed = true;
            if let Some(cancel) = &state.cancel {
                cancel.cancel();
            }
            state.active
        };
        if !active {
            self.release(control);
        }
    }
    fn write(
        &self,
        control: &Control,
        ctx: &Ctx,
        sink: &mut crate::net::dimse::StoreSink<'_>,
        syntax: &Syntax,
    ) -> Result<()> {
        let ctx = control.begin(ctx).map_err(Error::from)?;
        let result = self.write_inner(control, &ctx, sink, syntax);
        control.end();
        result
    }
    fn write_inner(
        &self,
        control: &Control,
        ctx: &Ctx,
        sink: &mut crate::net::dimse::StoreSink<'_>,
        syntax: &Syntax,
    ) -> Result<()> {
        let _gate = lock(&self.gate);
        let cancel = CancelToken::new();
        {
            let mut state = lock(&self.state);
            if state.closed {
                return Err(Error::new(ERR_PREPARED_CLOSED));
            }
            state.active = true;
            state.cancel = Some(cancel.clone());
        }
        let ctx = ctx.with_cancel(&cancel);
        let result = self.copy(control, &ctx, sink, syntax);
        let closed = {
            let mut state = lock(&self.state);
            state.active = false;
            state.cancel = None;
            state.closed
        };
        if closed {
            self.release(control);
        }
        result
    }
    fn copy(
        &self,
        control: &Control,
        ctx: &Ctx,
        sink: &mut crate::net::dimse::StoreSink<'_>,
        syntax: &Syntax,
    ) -> Result<()> {
        let uid = normalize_uid(syntax.uid);
        let Some(prepared) = lock(&self.prepared).clone() else {
            return Err(Error::new(ERR_PREPARED_CLOSED));
        };
        let Some((data, offset)) = prepared.payloads.get(uid) else {
            return Err(Error::sentinel(Kind::StoreTransferSyntax));
        };
        {
            let mut state = lock(&control.state);
            state.report.selected_transfer_syntax_uid = uid.to_string();
            state.report.negotiation = "selected".to_string();
        }
        let started = Instant::now();
        let mut rest = &data[*offset..];
        let mut result = Ok(());
        while !rest.is_empty() {
            if let Some(e) = context_err(ctx) {
                result = Err(SourceError::wrap(&e).into());
                break;
            }
            let n = rest.len().min(64 << 10);
            if let Err(e) = sink(&rest[..n]) {
                result = Err(SourceError::wrap(&e).into());
                break;
            }
            rest = &rest[n..];
        }
        control
            .copy
            .fetch_add(started.elapsed().as_nanos() as i64, Ordering::Relaxed);
        result
    }
}
