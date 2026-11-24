#![forbid(unsafe_code)]
use super::error::{Detail, Error, Kind, Result};
use crate::net::ul::{self, Association, Ctx, Nanos};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
pub(crate) const DEFAULT_SCP_FAILURE_RESPONSE_TIMEOUT: Duration = Duration::from_secs(1);
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ScpControls {
    pub command_progress_timeout: Nanos,
    pub data_set_progress_timeout: Nanos,
    pub operation_timeout: Nanos,
    pub cancel_grace: Nanos,
}
impl ScpControls {
    pub fn validate(&self) -> Result<()> {
        if self.command_progress_timeout.is_negative() {
            return Err(Error::new(
                "dicom dimse: command progress timeout must not be negative",
            ));
        }
        if self.data_set_progress_timeout.is_negative() {
            return Err(Error::new(
                "dicom dimse: dataset progress timeout must not be negative",
            ));
        }
        if self.operation_timeout.is_negative() {
            return Err(Error::new(
                "dicom dimse: operation timeout must not be negative",
            ));
        }
        if self.cancel_grace.is_negative() {
            return Err(Error::new("dicom dimse: cancel grace must not be negative"));
        }
        Ok(())
    }
}
#[derive(Clone)]
struct ResponseCtx(Ctx);
#[derive(Clone, Default)]
pub(crate) struct CancelCause(Arc<Mutex<Option<Error>>>);
impl CancelCause {
    pub fn set(&self, err: Error) {
        let mut g = self.0.lock().unwrap();
        if g.is_none() {
            *g = Some(err);
        }
    }
    pub fn get(&self) -> Option<Error> {
        self.0.lock().unwrap().clone()
    }
}
pub(crate) fn with_scp_controls(ctx: &Ctx, controls: ScpControls) -> Ctx {
    ctx.with_value(controls)
}
pub fn scp_controls(ctx: Option<&Ctx>) -> ScpControls {
    ctx.and_then(|c| c.value::<ScpControls>().copied())
        .unwrap_or_default()
}
pub(crate) fn with_scp_response_context(ctx: &Ctx, response: &Ctx) -> Ctx {
    ctx.with_value(ResponseCtx(response.clone()))
}
pub(crate) fn context_cause(ctx: &Ctx) -> Option<Error> {
    if let Some(c) = ctx.value::<CancelCause>().and_then(CancelCause::get) {
        return Some(c);
    }
    ctx.err().map(|k| Error::from(ul::Error::sentinel(k)))
}
pub fn context_err(ctx: &Ctx) -> Option<Error> {
    ctx.err().map(|k| Error::from(ul::Error::sentinel(k)))
}
pub(crate) fn scp_response_context(ctx: Option<&Ctx>, assoc: Option<&Association>) -> Ctx {
    let controls = scp_controls(ctx);
    let timeout = controls
        .cancel_grace
        .positive()
        .unwrap_or(DEFAULT_SCP_FAILURE_RESPONSE_TIMEOUT);
    if let Some(ctx) = ctx {
        if ctx.err().is_none() {
            return ctx.clone();
        }
        if let Some(ResponseCtx(response)) = ctx.value::<ResponseCtx>() {
            return with_scp_controls(&response.with_timeout(timeout), controls);
        }
    }
    let base = assoc.map(|a| a.context.clone()).unwrap_or_default();
    with_scp_controls(&base.with_timeout(timeout), controls)
}
pub(crate) fn send_with_scp_response_context<T>(
    ctx: Option<&Ctx>,
    assoc: Option<&Association>,
    send: impl FnOnce(&Ctx) -> Result<T>,
) -> Result<T> {
    let response = scp_response_context(ctx, assoc);
    send(&response)
}
pub(crate) fn command_read_context(ctx: Option<&Ctx>) -> Option<Ctx> {
    let timeout = scp_controls(ctx).command_progress_timeout;
    match (ctx, timeout.positive()) {
        (Some(c), Some(_)) => Some(c.with_read_progress_timeout(timeout)),
        (None, Some(_)) => Some(Ctx::background().with_read_progress_timeout(timeout)),
        (c, None) => c.cloned(),
    }
}
pub(crate) fn command_write_context(ctx: Option<&Ctx>) -> Option<Ctx> {
    let timeout = scp_controls(ctx).command_progress_timeout;
    match (ctx, timeout.positive()) {
        (Some(c), Some(_)) => Some(c.with_write_progress_timeout(timeout)),
        (c, _) => c.cloned(),
    }
}
pub(crate) fn data_set_read_context(ctx: Option<&Ctx>, assoc: Option<&Association>) -> Ctx {
    let base = match (ctx, assoc) {
        (Some(c), _) => c.clone(),
        (None, Some(a)) => a.context.clone(),
        (None, None) => Ctx::background(),
    };
    let base = base.with_active_transfer();
    let timeout = scp_controls(Some(&base)).data_set_progress_timeout;
    base.with_read_progress_timeout(timeout)
}
pub(crate) fn data_set_write_context(ctx: Option<&Ctx>) -> Option<Ctx> {
    let timeout = scp_controls(ctx).data_set_progress_timeout;
    match (ctx, timeout.positive()) {
        (Some(c), Some(_)) => Some(c.with_write_progress_timeout(timeout)),
        (c, _) => c.cloned(),
    }
}
pub(crate) fn mixed_message_write_context(ctx: &Ctx) -> Ctx {
    let controls = scp_controls(Some(ctx));
    let mut timeout = controls.command_progress_timeout;
    let data = controls.data_set_progress_timeout;
    if data.0 > 0 && (timeout.0 <= 0 || data.0 < timeout.0) {
        timeout = data;
    }
    if timeout.0 > 0 {
        return ctx.with_write_progress_timeout(timeout);
    }
    ctx.clone()
}
fn call_safely<T>(ctx: &Ctx, handler: impl FnOnce(&Ctx) -> Result<T>) -> Result<T> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| handler(ctx))) {
        Ok(r) => r,
        Err(_) => Err(Error::sentinel(Kind::ScpHandlerPanic)),
    }
}
pub(crate) fn run_scp_handler<T: Send>(
    ctx: &Ctx,
    assoc: Option<&Association>,
    grace: Nanos,
    handler: impl FnOnce(&Ctx) -> Result<T> + Send,
) -> Result<T> {
    let Some(grace) = grace.positive() else {
        return call_safely(ctx, handler);
    };
    std::thread::scope(|s| {
        let (tx, rx) = std::sync::mpsc::channel();
        let child = ctx.clone();
        s.spawn(move || {
            let _ = tx.send(call_safely(&child, handler));
        });
        loop {
            if let Ok(r) = rx.recv_timeout(Duration::from_millis(5)) {
                return r;
            }
            if ctx.err().is_some() {
                break;
            }
        }
        let end = Instant::now() + grace;
        while Instant::now() < end {
            if let Ok(r) = rx.recv_timeout(Duration::from_millis(5)) {
                return r;
            }
        }
        if let Some(a) = assoc {
            let abort_ctx = Ctx::background().with_timeout(Duration::from_millis(100));
            let _ = a.abort_with_context(Some(&abort_ctx), ul::ABORT_REASON_NOT_SPECIFIED);
        }
        let cause = context_cause(ctx);
        Err(Error::join([cause, Some(Error::sentinel(Kind::CancelGraceExceeded))]).unwrap())
    })
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum OperationErrorPolicy {
    #[default]
    Default,
    LeaveOpen,
    Release,
    Abort,
}
#[derive(Debug, Clone, Default)]
pub struct OperationOptions {
    pub context: Option<Ctx>,
    pub overall_timeout: Nanos,
    pub response_timeout: Nanos,
    pub error_policy: OperationErrorPolicy,
}
pub(crate) struct OperationGuard<'a>(Option<&'a Association>);
impl Drop for OperationGuard<'_> {
    fn drop(&mut self) {
        if let Some(a) = self.0.take() {
            a.end_operation();
        }
    }
}
pub(crate) fn begin_association_operation(
    assoc: Option<&Association>,
) -> Result<OperationGuard<'_>> {
    match assoc {
        None => Ok(OperationGuard(None)),
        Some(a) => {
            if !a.try_begin_operation() {
                return Err(Error::sentinel(Kind::OperationInProgress));
            }
            Ok(OperationGuard(Some(a)))
        }
    }
}
pub(crate) fn operation_context(options: &OperationOptions) -> Ctx {
    let ctx = options.context.clone().unwrap_or_default();
    ctx.with_optional_timeout(options.overall_timeout)
}
pub(crate) fn operation_options_with_default_policy(
    mut options: OperationOptions,
    default: OperationErrorPolicy,
) -> OperationOptions {
    if options.error_policy == OperationErrorPolicy::Default {
        options.error_policy = default;
    }
    options
}
pub(crate) fn operation_response_context(ctx: &Ctx, timeout: Nanos) -> Ctx {
    ctx.with_optional_timeout(timeout)
}
pub(crate) fn classify_operation_error(err: Error) -> Error {
    if err.is_canceled() {
        if err.is(Kind::OperationCanceled) {
            return err;
        }
        return Error::wrap_err(Kind::OperationCanceled, err);
    }
    if err.is_deadline() || err.is_ul(ul::Kind::AssociationTimeout) {
        if err.is(Kind::OperationTimeout) {
            return err;
        }
        return Error::wrap_err(Kind::OperationTimeout, err);
    }
    err
}
pub(crate) fn new_operation_error(operation: &str, err: Error, uncertain: bool) -> Error {
    let inner = classify_operation_error(err);
    let prefix = if operation.is_empty() {
        "DIMSE operation"
    } else {
        operation
    };
    let text = if uncertain {
        format!(
            "dicom dimse: {prefix}: {}; association state uncertain",
            inner.text()
        )
    } else {
        format!("dicom dimse: {prefix}: {}", inner.text())
    };
    let mut e = inner
        .relabel(text)
        .with_detail(Detail::Operation(operation.to_string(), uncertain));
    if uncertain {
        e = e.also(Kind::AssociationStateUncertain);
    }
    e
}
pub(crate) fn apply_operation_error_policy(
    assoc: Option<&Association>,
    policy: OperationErrorPolicy,
    err: Error,
) -> Error {
    let Some(assoc) = assoc else { return err };
    let cleanup = Ctx::background().with_timeout(Duration::from_secs(1));
    match policy {
        OperationErrorPolicy::Release => {
            if let Err(e) = assoc.release(Some(&cleanup)) {
                let text = format!("{}; release association failed: {}", err.text(), e.text());
                return err.relabel(text);
            }
        }
        OperationErrorPolicy::Abort => {
            if let Err(e) = assoc.abort_for_error(Some(&cleanup), &err.to_ul()) {
                let text = format!("{}; abort association failed: {}", err.text(), e.text());
                return err.relabel(text);
            }
        }
        _ => {}
    }
    err
}
pub type AccessCheck = dyn Fn(&Ctx) -> Result<()> + Send + Sync;
pub(crate) fn check_store_access(ctx: &Ctx, check: Option<&AccessCheck>) -> Result<()> {
    let Some(check) = check else { return Ok(()) };
    if let Some(e) = context_err(ctx) {
        return Err(e);
    }
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| check(ctx)));
    if let Some(e) = context_err(ctx) {
        return Err(e);
    }
    match r {
        Ok(Ok(())) => Ok(()),
        _ => Err(Error::sentinel(Kind::StoreAccessDenied)),
    }
}
#[derive(Debug, Clone, Copy)]
pub struct ElapsedTimer {
    pub started: Instant,
}
impl ElapsedTimer {
    pub fn start() -> ElapsedTimer {
        ElapsedTimer {
            started: Instant::now(),
        }
    }
    pub fn elapsed(&self) -> Duration {
        let e = self.started.elapsed();
        if e.is_zero() {
            Duration::from_nanos(1)
        } else {
            e
        }
    }
}
