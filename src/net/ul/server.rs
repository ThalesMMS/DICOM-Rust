#![forbid(unsafe_code)]
use super::association::{
    Association, Listener, notify_rejection_audit, read_observed, write_observed,
};
use super::called_ae::validate_called_ae_options;
use super::conn::{CancelToken, Conn, Ctx, Nanos};
use super::error::{Error, Kind, Result};
use super::linger::{artim_timeout, linger_close, provider_abort_reason};
use super::negotiation::{AcceptOptions, normalize_accept_options, validate_operational_timeouts};
use super::observability::{OperationalState, RawPduDispatcher, validate_observability_options};
use super::pdu::*;
use crate::net::telemetry::{AssociationRole, Dispatcher};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::{Duration, Instant};
pub const DEFAULT_SERVER_MAX_CONCURRENT_ASSOCIATIONS: usize = 64;
pub const DEFAULT_SERVER_NEGOTIATION_TIMEOUT: Nanos = Nanos(10_000_000_000);
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SaturationPolicy {
    #[default]
    Backpressure,
    Reject,
}
pub type AssociationHandler = Arc<dyn Fn(&Ctx, &Association) -> Result<()> + Send + Sync>;
#[derive(Clone, Default)]
pub struct AssociationServerOptions {
    pub accept: AcceptOptions,
    pub max_concurrent_associations: i64,
    pub saturation_policy: SaturationPolicy,
    pub handler: Option<AssociationHandler>,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AssociationServerMetrics {
    pub negotiating: i64,
    pub active: i64,
    pub accepted: u64,
    pub rejected: u64,
    pub negotiation_panics: u64,
    pub handler_errors: u64,
    pub handler_panics: u64,
    pub released: u64,
    pub aborted: u64,
    pub closed: u64,
    pub dropped_events: u64,
    pub sink_panics: u64,
    pub raw_dropped: u64,
    pub raw_sink_panics: u64,
}
#[derive(Default)]
struct Counters {
    negotiating: AtomicI64,
    active: AtomicI64,
    accepted: AtomicU64,
    rejected: AtomicU64,
    negotiation_panics: AtomicU64,
    handler_errors: AtomicU64,
    handler_panics: AtomicU64,
    released: AtomicU64,
    aborted: AtomicU64,
    closed: AtomicU64,
}
struct Entry {
    conn: Arc<Conn>,
    assoc: Option<Arc<Association>>,
}
struct Shared {
    counters: Counters,
    slots: (Mutex<usize>, Condvar),
    max: usize,
    connections: Mutex<HashMap<u64, Entry>>,
    next_id: AtomicU64,
    running: (Mutex<usize>, Condvar),
    stopped: AtomicBool,
    serving: AtomicBool,
    cancel: Mutex<CancelToken>,
    done: (Mutex<bool>, Condvar),
    owned_dispatcher: Option<Arc<Dispatcher>>,
    owned_raw_dispatcher: Option<Arc<RawPduDispatcher>>,
}
pub struct AssociationServer {
    listener: Arc<Listener>,
    options: AssociationServerOptions,
    shared: Arc<Shared>,
}
impl AssociationServer {
    pub fn new(
        listener: Arc<Listener>,
        mut options: AssociationServerOptions,
    ) -> Result<AssociationServer> {
        if options.handler.is_none() {
            return Err(Error::other("dicom ul: nil association handler"));
        }
        if options.max_concurrent_associations < 0 {
            return Err(Error::other(
                "dicom ul: maximum concurrent associations must not be negative",
            ));
        }
        let max = if options.max_concurrent_associations == 0 {
            DEFAULT_SERVER_MAX_CONCURRENT_ASSOCIATIONS
        } else {
            options.max_concurrent_associations as usize
        };
        options.accept = normalize_accept_options(options.accept);
        validate_called_ae_options(&options.accept)?;
        if options.accept.negotiation_timeout.is_zero() {
            options.accept.negotiation_timeout = DEFAULT_SERVER_NEGOTIATION_TIMEOUT;
        }
        let a = &options.accept;
        validate_operational_timeouts(
            a.negotiation_timeout,
            a.idle_timeout,
            a.read_progress_timeout,
            a.write_progress_timeout,
            a.release_timeout,
        )?;
        validate_observability_options(options.accept.observability.as_ref())?;
        let mut owned_dispatcher = None;
        let mut owned_raw_dispatcher = None;
        if let Some(o) = &mut options.accept.observability {
            if o.dispatcher.is_none()
                && let Some(sink) = o.sink.take()
            {
                let dispatcher = Dispatcher::new(sink, o.event_queue_depth);
                o.dispatcher = Some(dispatcher.clone());
                owned_dispatcher = Some(dispatcher);
            }
            if o.raw_pdu_dispatcher.is_none()
                && let Some(sink) = o.raw_pdu_sink.take()
            {
                let dispatcher = RawPduDispatcher::new(sink, o.raw_pdu_queue_depth);
                o.raw_pdu_dispatcher = Some(dispatcher.clone());
                owned_raw_dispatcher = Some(dispatcher);
            }
        }
        Ok(AssociationServer {
            listener,
            options,
            shared: Arc::new(Shared {
                counters: Counters::default(),
                slots: (Mutex::new(0), Condvar::new()),
                max,
                connections: Mutex::new(HashMap::new()),
                next_id: AtomicU64::new(1),
                running: (Mutex::new(0), Condvar::new()),
                stopped: AtomicBool::new(false),
                serving: AtomicBool::new(false),
                cancel: Mutex::new(CancelToken::new()),
                done: (Mutex::new(false), Condvar::new()),
                owned_dispatcher,
                owned_raw_dispatcher,
            }),
        })
    }
    pub fn serve(&self, ctx: Option<&Ctx>) -> Result<()> {
        if self.shared.serving.load(Ordering::SeqCst) {
            return Err(Error::other("dicom ul: association server already serving"));
        }
        if self.shared.stopped.load(Ordering::SeqCst) {
            return Err(Error::sentinel(Kind::AssociationServerStopped));
        }
        if self.shared.serving.swap(true, Ordering::SeqCst) {
            return Err(Error::other("dicom ul: association server already serving"));
        }
        let token = self
            .shared
            .cancel
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        let base = ctx.cloned().unwrap_or_default();
        let runtime = base.with_cancel(&token);
        let result = std::thread::scope(|s| {
            let watcher_done = Arc::new(AtomicBool::new(false));
            {
                let watcher_done = watcher_done.clone();
                let runtime = runtime.clone();
                s.spawn(move || {
                    while !watcher_done.load(Ordering::SeqCst) {
                        if runtime.err().is_some() {
                            self.force_stop();
                            return;
                        }
                        std::thread::sleep(Duration::from_millis(5));
                    }
                });
            }
            let r = self.accept_loop(&runtime);
            let graceful = r.as_ref().err().is_some_and(|e| e.is(Kind::Closed))
                && self.shared.stopped.load(Ordering::SeqCst);
            let canceled = r.as_ref().err().is_some_and(|e| e.is(Kind::Canceled));
            if r.is_err() && !graceful && !canceled {
                self.force_stop();
            }
            self.wait_connections();
            watcher_done.store(true, Ordering::SeqCst);
            match r {
                Err(_) if graceful || canceled => Ok(()),
                other => other,
            }
        });
        self.finish();
        result
    }
    fn acquire_slot(&self, ctx: &Ctx) -> Result<()> {
        let (lock, cond) = &self.shared.slots;
        let mut used = lock.lock().unwrap_or_else(PoisonError::into_inner);
        while *used >= self.shared.max {
            if let Some(k) = ctx.err() {
                return Err(Error::sentinel(k));
            }
            used = cond
                .wait_timeout(used, Duration::from_millis(5))
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
        *used += 1;
        Ok(())
    }
    fn try_acquire_slot(&self) -> bool {
        let mut used = self
            .shared
            .slots
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if *used >= self.shared.max {
            return false;
        }
        *used += 1;
        true
    }
    fn release_slot(shared: &Shared) {
        let (lock, cond) = &shared.slots;
        let mut used = lock.lock().unwrap_or_else(PoisonError::into_inner);
        *used = used.saturating_sub(1);
        cond.notify_all();
    }
    fn accept_loop(&self, ctx: &Ctx) -> Result<()> {
        loop {
            if self.options.saturation_policy == SaturationPolicy::Backpressure {
                self.acquire_slot(ctx)?;
                match self.listener.accept_conn(ctx) {
                    Ok(conn) => self.start_connection(ctx, Arc::new(conn)),
                    Err(e) => {
                        Self::release_slot(&self.shared);
                        return Err(e);
                    }
                }
                continue;
            }
            let conn = Arc::new(self.listener.accept_conn(ctx)?);
            if self.try_acquire_slot() {
                self.start_connection(ctx, conn);
            } else {
                self.reject_saturated(ctx, &conn);
            }
        }
    }
    fn start_connection(&self, ctx: &Ctx, conn: Arc<Conn>) {
        let id = self.shared.next_id.fetch_add(1, Ordering::SeqCst);
        self.shared
            .connections
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(
                id,
                Entry {
                    conn: conn.clone(),
                    assoc: None,
                },
            );
        *self
            .shared
            .running
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner) += 1;
        let shared = self.shared.clone();
        let mut accept = self.options.accept.clone();
        accept.context = Some(ctx.clone());
        let original = accept.on_association_rejected.clone();
        let counting = shared.clone();
        accept.on_association_rejected = Some(Arc::new(move || {
            counting.counters.rejected.fetch_add(1, Ordering::SeqCst);
            if let Some(cb) = &original {
                cb();
            }
        }));
        let handler = self.options.handler.clone().expect("checked in new");
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            serve_connection(&shared, id, conn.clone(), accept, &handler, &ctx);
            shared
                .connections
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .remove(&id);
            conn.close();
            Self::release_slot(&shared);
            let (lock, cond) = &shared.running;
            *lock.lock().unwrap_or_else(PoisonError::into_inner) -= 1;
            cond.notify_all();
        });
    }
    fn wait_connections(&self) {
        let (lock, cond) = &self.shared.running;
        let mut n = lock.lock().unwrap_or_else(PoisonError::into_inner);
        while *n > 0 {
            n = cond.wait(n).unwrap_or_else(PoisonError::into_inner);
        }
    }
    fn reject_saturated(&self, ctx: &Ctx, conn: &Arc<Conn>) {
        let options = &self.options.accept;
        let ctx = ctx.with_optional_timeout(options.negotiation_timeout);
        let Ok(state) = OperationalState::new(
            options.observability.as_ref(),
            AssociationRole::Scp,
            super::association::new_association_id(),
        ) else {
            conn.close();
            return;
        };
        state.set_connection(conn);
        #[cfg(feature = "tls")]
        if let Some(tls) = &options.tls_config
            && conn.start_tls_server(tls, &ctx).is_err()
        {
            state.close();
            conn.close();
            return;
        }
        let Ok(pdu) = read_observed(
            &ctx,
            conn,
            options.max_pdu,
            options.max_association_pdu,
            &state,
        ) else {
            state.close();
            conn.close();
            return;
        };
        let Pdu::AssociateRq(rq) = pdu else {
            state.close();
            conn.close();
            return;
        };
        let rj = AssociationRj {
            result: ASSOCIATE_RJ_RESULT_TRANSIENT,
            source: ASSOCIATE_RJ_SOURCE_SERVICE_PROVIDER_PRESENTATION,
            reason: ASSOCIATE_RJ_REASON_LOCAL_LIMIT_EXCEEDED,
        };
        self.shared.counters.rejected.fetch_add(1, Ordering::SeqCst);
        notify_rejection_audit(options, Some(&rq), conn, &rj);
        let _ = write_observed(&ctx, conn, &Pdu::AssociateRj(rj), &state);
        linger_close(conn, artim_timeout(options.artim_timeout));
        state.close();
    }
    pub fn shutdown(&self, ctx: Option<&Ctx>) -> Result<()> {
        self.shared.stopped.store(true, Ordering::SeqCst);
        self.listener.close();
        if !self.shared.serving.load(Ordering::SeqCst) {
            self.finish();
            return Ok(());
        }
        let ctx = ctx.cloned().unwrap_or_default();
        let (lock, cond) = &self.shared.done;
        let mut done = lock.lock().unwrap_or_else(PoisonError::into_inner);
        while !*done {
            if let Some(k) = ctx.err() {
                drop(done);
                self.force_stop();
                return Err(Error::sentinel(k));
            }
            done = cond
                .wait_timeout(done, Duration::from_millis(5))
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
        Ok(())
    }
    fn force_stop(&self) {
        self.shared.stopped.store(true, Ordering::SeqCst);
        self.listener.close();
        let targets: Vec<(Arc<Conn>, Option<Arc<Association>>)> = self
            .shared
            .connections
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .values()
            .map(|e| (e.conn.clone(), e.assoc.clone()))
            .collect();
        let abort_ctx =
            Ctx::background().with_deadline(Instant::now() + Duration::from_millis(100));
        for (conn, assoc) in targets {
            if let Some(a) = assoc {
                let _ = a.abort_with_context(Some(&abort_ctx), ABORT_REASON_NOT_SPECIFIED);
                a.mark_aborted();
            }
            conn.close();
        }
        self.shared
            .cancel
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .cancel();
    }
    fn finish(&self) {
        self.shared
            .cancel
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .cancel();
        if let Some(dispatcher) = &self.shared.owned_dispatcher {
            dispatcher.close();
        }
        if let Some(dispatcher) = &self.shared.owned_raw_dispatcher {
            dispatcher.close();
        }
        let (lock, cond) = &self.shared.done;
        *lock.lock().unwrap_or_else(PoisonError::into_inner) = true;
        cond.notify_all();
    }
    pub fn snapshot(&self) -> AssociationServerMetrics {
        let c = &self.shared.counters;
        let events = self
            .shared
            .owned_dispatcher
            .as_ref()
            .map(|d| d.stats())
            .unwrap_or_default();
        let raw = self
            .shared
            .owned_raw_dispatcher
            .as_ref()
            .map(|d| d.stats())
            .unwrap_or_default();
        AssociationServerMetrics {
            dropped_events: events.dropped_events,
            sink_panics: events.sink_panics,
            raw_dropped: raw.dropped,
            raw_sink_panics: raw.panics,
            negotiating: c.negotiating.load(Ordering::SeqCst),
            active: c.active.load(Ordering::SeqCst),
            accepted: c.accepted.load(Ordering::SeqCst),
            rejected: c.rejected.load(Ordering::SeqCst),
            negotiation_panics: c.negotiation_panics.load(Ordering::SeqCst),
            handler_errors: c.handler_errors.load(Ordering::SeqCst),
            handler_panics: c.handler_panics.load(Ordering::SeqCst),
            released: c.released.load(Ordering::SeqCst),
            aborted: c.aborted.load(Ordering::SeqCst),
            closed: c.closed.load(Ordering::SeqCst),
        }
    }
}
fn serve_connection(
    shared: &Shared,
    id: u64,
    conn: Arc<Conn>,
    accept: AcceptOptions,
    handler: &AssociationHandler,
    ctx: &Ctx,
) {
    let c = &shared.counters;
    c.negotiating.fetch_add(1, Ordering::SeqCst);
    let accepted = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        super::association::accept(conn.clone(), accept)
    }));
    c.negotiating.fetch_sub(1, Ordering::SeqCst);
    let assoc = match accepted {
        Err(_) => {
            c.negotiation_panics.fetch_add(1, Ordering::SeqCst);
            return;
        }
        Ok(Err(_)) => return,
        Ok(Ok(a)) => Arc::new(a),
    };
    if let Some(e) = shared
        .connections
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get_mut(&id)
    {
        e.assoc = Some(assoc.clone());
    }
    c.active.fetch_add(1, Ordering::SeqCst);
    c.accepted.fetch_add(1, Ordering::SeqCst);
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| handler(ctx, &assoc)));
    match r {
        Err(_) => {
            c.handler_panics.fetch_add(1, Ordering::SeqCst);
            let abort_ctx = Ctx::background().with_timeout(Duration::from_millis(100));
            let _ = assoc.abort_with_context(Some(&abort_ctx), ABORT_REASON_NOT_SPECIFIED);
        }
        Ok(Err(err)) => {
            if !err.is(Kind::Canceled) {
                c.handler_errors.fetch_add(1, Ordering::SeqCst);
            }
            if provider_abort_reason(&err).is_some() && assoc.remote_addr().is_some() {
                let abort_ctx = Ctx::background().with_timeout(Duration::from_secs(1));
                let _ = assoc.abort_for_error(Some(&abort_ctx), &err);
            }
        }
        Ok(Ok(())) => {}
    }
    assoc.close();
    let lifecycle = assoc.telemetry_snapshot();
    if lifecycle.aborted > 0 {
        c.aborted.fetch_add(1, Ordering::SeqCst);
    } else if lifecycle.released > 0 {
        c.released.fetch_add(1, Ordering::SeqCst);
    } else {
        c.closed.fetch_add(1, Ordering::SeqCst);
    }
    c.active.fetch_sub(1, Ordering::SeqCst);
}
