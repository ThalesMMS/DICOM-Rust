#![forbid(unsafe_code)]
use super::admission::{
    ADMISSION_REJECT_WRITE_TIMEOUT, AdmissionDecision, RejectedAssociation, admission_request,
    admit, reject_called_ae_title,
};
use super::aetitle::{normalize_ae_title, validate_ae_title};
use super::called_ae::validate_called_ae_options;
use super::common_ext::common_extended_negotiation_from_user_info;
use super::conn::{Conn, Ctx, CtxReader, association_context_error, ended_context_error};
use super::error::{Error, Kind, Result};
use super::linger::{
    artim_timeout, context_limit_reached, linger_close, provider_abort_reason, unexpected_pdu_error,
};
use super::negotiation::*;
use super::observability::OperationalState;
use super::outcome::{PresentationContextOutcome, presentation_context_outcomes_from_results};
use super::pdu::*;
use super::reader::{
    PDU_READ_GROWTH, parse_pdu_body, read_pdu_with_association_limit, resolve_max_association_pdu,
    validate_read_pdu_length,
};
use super::rules::{
    called_ae_presentation_context_rules, validate_called_ae_presentation_context_rules,
    validate_presentation_context_rules,
};
use super::writer::encode_pdu;
use crate::net::telemetry::{AssociationRole, CommandObservation, Direction, OperationObservation};
use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::net::{SocketAddr, TcpListener, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, PoisonError, RwLock};
use std::time::{Duration, Instant};
pub const DEFAULT_MAX_SEND_PDU: u32 = 128 << 10;
const MIN_SEND_PDU: u32 = PDV_HEADER_SIZE + 1;
fn resolve_max_send_pdu(configured: u32) -> u32 {
    if configured == 0 {
        DEFAULT_MAX_SEND_PDU
    } else {
        configured
    }
}
pub use crate::net::telemetry::AssociationMetrics;
pub(crate) type Lifecycle = OperationalState;
pub(crate) fn new_association_id() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let mut out = String::with_capacity(32);
    for _ in 0..2 {
        let mut h = RandomState::new().build_hasher();
        h.write_u64(COUNTER.fetch_add(1, Ordering::SeqCst));
        h.write_u128(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or_default(),
        );
        out.push_str(&format!("{:016x}", h.finish()));
    }
    out
}
#[derive(Debug)]
pub struct OperationToken {
    id: u64,
    ended: AtomicBool,
}
impl OperationToken {
    pub fn context(&self, ctx: &Ctx) -> Ctx {
        let mut c = ctx.clone();
        c.operation_token = Some(self.id);
        c
    }
    pub fn end(&self, assoc: &Association) {
        if self.ended.swap(true, Ordering::SeqCst) {
            return;
        }
        let mut state = assoc.state.lock().unwrap_or_else(PoisonError::into_inner);
        if state.exclusive_token == Some(self.id) {
            state.exclusive_token = None;
            state.operation_active = false;
        }
    }
}
#[derive(Debug, Default)]
struct OpState {
    operation_active: bool,
    exclusive_token: Option<u64>,
    carryover: Vec<PDataValue>,
}
#[derive(Debug, Default)]
struct ReleaseFlags {
    local_requested: bool,
    remote_requested: bool,
    local_responded: bool,
    remote_responded: bool,
}
pub struct Association {
    conn: RwLock<Option<Arc<Conn>>>,
    read_buffer: Mutex<Vec<u8>>,
    write_lock: Mutex<()>,
    message_gate: (Mutex<bool>, Condvar),
    release_flags: Mutex<ReleaseFlags>,
    state: Mutex<OpState>,
    artim_timeout: Duration,
    max_send_pdu: u32,
    max_association_pdu: u32,
    idle_timeout: Option<Duration>,
    read_progress_timeout: Option<Duration>,
    write_progress_timeout: Option<Duration>,
    release_timeout: OptTimeout,
    lifecycle: Arc<Lifecycle>,
    local_addr: Option<SocketAddr>,
    pub called_ae_title: String,
    pub calling_ae_title: String,
    pub contexts: Vec<PresentationContext>,
    pub accepted_contexts: Vec<AcceptedContext>,
    pub context_outcomes: Vec<PresentationContextOutcome>,
    pub peer_max_pdu: u32,
    pub max_pdu: u32,
    pub protocol_version: u16,
    pub application_context_name: String,
    pub context: Ctx,
    pub user_identity_response: Vec<u8>,
    pub user_identity_responded: bool,
    pub requested_role_selections: Vec<RoleSelectionItem>,
    pub accepted_role_selections: Vec<RoleSelectionItem>,
    pub requested_extended_negotiation: Vec<SopClassExtendedNegotiationItem>,
    pub accepted_extended_negotiation: Vec<SopClassExtendedNegotiationItem>,
    pub requested_common_extended_negotiation: Vec<SopClassCommonExtendedNegotiationItem>,
    pub requested_asynchronous_operations_window: Option<AsynchronousOperationsWindow>,
    pub negotiated_asynchronous_operations_window: AsynchronousOperationsWindow,
    pub asynchronous_operations_negotiated: bool,
    pub is_association_requestor: bool,
}
type OptTimeout = Option<Duration>;
impl std::fmt::Debug for Association {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Association")
            .field("called_ae_title", &self.called_ae_title)
            .field("calling_ae_title", &self.calling_ae_title)
            .field("requestor", &self.is_association_requestor)
            .finish_non_exhaustive()
    }
}
struct Parts {
    conn: Arc<Conn>,
    lifecycle: Arc<Lifecycle>,
    idle: crate::net::ul::conn::Nanos,
    read_progress: crate::net::ul::conn::Nanos,
    write_progress: crate::net::ul::conn::Nanos,
    release: crate::net::ul::conn::Nanos,
    artim: crate::net::ul::conn::Nanos,
    max_send_pdu: u32,
    max_association_pdu: u32,
}
impl Association {
    fn from_parts(p: Parts) -> Association {
        Association {
            local_addr: Some(p.conn.local_addr()),
            conn: RwLock::new(Some(p.conn)),
            read_buffer: Mutex::new(Vec::new()),
            write_lock: Mutex::new(()),
            message_gate: (Mutex::new(false), Condvar::new()),
            release_flags: Mutex::new(ReleaseFlags::default()),
            state: Mutex::new(OpState::default()),
            artim_timeout: artim_timeout(p.artim),
            max_send_pdu: resolve_max_send_pdu(p.max_send_pdu),
            max_association_pdu: resolve_max_association_pdu(p.max_association_pdu),
            idle_timeout: p.idle.positive(),
            read_progress_timeout: p.read_progress.positive(),
            write_progress_timeout: p.write_progress.positive(),
            release_timeout: p.release.positive(),
            lifecycle: p.lifecycle,
            called_ae_title: String::new(),
            calling_ae_title: String::new(),
            contexts: Vec::new(),
            accepted_contexts: Vec::new(),
            context_outcomes: Vec::new(),
            peer_max_pdu: 0,
            max_pdu: 0,
            protocol_version: 0,
            application_context_name: String::new(),
            context: Ctx::background(),
            user_identity_response: Vec::new(),
            user_identity_responded: false,
            requested_role_selections: Vec::new(),
            accepted_role_selections: Vec::new(),
            requested_extended_negotiation: Vec::new(),
            accepted_extended_negotiation: Vec::new(),
            requested_common_extended_negotiation: Vec::new(),
            requested_asynchronous_operations_window: None,
            negotiated_asynchronous_operations_window: AsynchronousOperationsWindow::default(),
            asynchronous_operations_negotiated: false,
            is_association_requestor: false,
        }
    }
    pub fn id(&self) -> &str {
        &self.lifecycle.id
    }
    pub fn telemetry_snapshot(&self) -> AssociationMetrics {
        self.lifecycle.snapshot()
    }
    pub fn record_command_observation(&self, observation: CommandObservation) {
        self.lifecycle
            .observe_command(&self.accepted_contexts, observation);
    }
    pub fn record_operation_observation(&self, observation: OperationObservation) {
        self.lifecycle
            .observe_operation(&self.accepted_contexts, observation);
    }
    pub(crate) fn mark_aborted(&self) {
        self.lifecycle.mark_aborted();
    }
    pub fn observed(&self) -> bool {
        self.lifecycle.enabled()
    }
    #[cfg(feature = "tls")]
    pub fn tls_connection_state(&self) -> Option<super::tls_conn::TlsConnectionState> {
        self.connection()?.tls_connection_state().cloned()
    }
    pub fn remote_addr(&self) -> Option<SocketAddr> {
        self.connection().map(|c| c.peer_addr())
    }
    pub fn local_addr(&self) -> Option<SocketAddr> {
        self.connection().and(self.local_addr)
    }
    pub fn effective_socket_buffers(&self) -> Result<super::socket_buffers::SocketBuffers> {
        match self.connection() {
            Some(conn) => conn.effective_socket_buffers(),
            None => Err(Error::sentinel(Kind::SocketBuffersUnsupported)),
        }
    }
    fn connection(&self) -> Option<Arc<Conn>> {
        self.conn
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
    pub fn effective_asynchronous_operations_window(&self) -> AsynchronousOperationsWindow {
        if !self.asynchronous_operations_negotiated {
            return AsynchronousOperationsWindow {
                maximum_invoked: 1,
                maximum_performed: 1,
            };
        }
        self.negotiated_asynchronous_operations_window
    }
    pub fn send_pdu_limit(&self) -> u32 {
        let mut limit = self.peer_max_pdu;
        if limit == 0 {
            limit = DEFAULT_MAX_PDU;
        }
        let local_cap = if self.max_send_pdu == 0 {
            DEFAULT_MAX_SEND_PDU
        } else {
            self.max_send_pdu
        };
        limit = limit.min(local_cap);
        limit.max(MIN_SEND_PDU)
    }
    pub fn presentation_context_outcomes(&self) -> Vec<PresentationContextOutcome> {
        self.context_outcomes.clone()
    }
    pub fn rejected_presentation_contexts(&self) -> Vec<PresentationContextOutcome> {
        self.context_outcomes
            .iter()
            .filter(|o| !o.accepted())
            .cloned()
            .collect()
    }
    fn ctx_or_default(&self, ctx: Option<&Ctx>) -> Ctx {
        ctx.cloned().unwrap_or_else(|| self.context.clone())
    }
    fn exclusive_operation_allowed(&self, ctx: Option<&Ctx>) -> bool {
        let token = self
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .exclusive_token;
        match token {
            None => true,
            Some(t) => ctx.and_then(|c| c.operation_token) == Some(t),
        }
    }
    pub fn try_begin_exclusive_operation(&self) -> Option<OperationToken> {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if state.operation_active {
            return None;
        }
        let id = NEXT.fetch_add(1, Ordering::SeqCst);
        state.operation_active = true;
        state.exclusive_token = Some(id);
        Some(OperationToken {
            id,
            ended: AtomicBool::new(false),
        })
    }
    pub fn try_begin_operation(&self) -> bool {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if state.operation_active {
            return false;
        }
        state.operation_active = true;
        true
    }
    pub fn end_operation(&self) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if state.exclusive_token.is_none() {
            state.operation_active = false;
        }
    }
    pub fn store_pdata_carryover(&self, ctx: Option<&Ctx>, values: &[PDataValue]) -> Result<()> {
        if values.is_empty() {
            return Ok(());
        }
        if !self.exclusive_operation_allowed(ctx) {
            return Err(Error::sentinel(Kind::AssociationExclusivelyOwned));
        }
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .carryover
            .extend(values.iter().cloned());
        Ok(())
    }
    pub fn take_pdata_carryover(&self, ctx: Option<&Ctx>) -> Result<Vec<PDataValue>> {
        if !self.exclusive_operation_allowed(ctx) {
            return Err(Error::sentinel(Kind::AssociationExclusivelyOwned));
        }
        Ok(std::mem::take(
            &mut self
                .state
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .carryover,
        ))
    }
    pub fn serialize_message_write<T>(
        &self,
        ctx: Option<&Ctx>,
        write: impl FnOnce() -> Result<T>,
    ) -> Result<T> {
        if !self.exclusive_operation_allowed(ctx) {
            return Err(Error::sentinel(Kind::AssociationExclusivelyOwned));
        }
        let ctx = ctx.cloned().unwrap_or_default();
        let (lock, cond) = &self.message_gate;
        let mut busy = lock.lock().unwrap_or_else(PoisonError::into_inner);
        while *busy {
            if let Some(kind) = ctx.err() {
                return Err(Error::wrap_err(
                    Kind::MessageWriteNotStarted,
                    Error::sentinel(kind),
                ));
            }
            busy = cond
                .wait_timeout(busy, Duration::from_millis(20))
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
        *busy = true;
        drop(busy);
        let out = write();
        *lock.lock().unwrap_or_else(PoisonError::into_inner) = false;
        cond.notify_one();
        out
    }
    pub fn send(&self, ctx: Option<&Ctx>, pdu: &Pdu) -> Result<()> {
        if !self.exclusive_operation_allowed(ctx) {
            return Err(Error::sentinel(Kind::AssociationExclusivelyOwned));
        }
        let ctx = self.ctx_or_default(ctx);
        let Some(conn) = self.connection() else {
            return Err(closed_association_error(&ctx));
        };
        self.write_pdu_on(&ctx, &conn, pdu)
    }
    pub fn write_pdu(&self, pdu: &Pdu) -> Result<()> {
        if !self.exclusive_operation_allowed(None) {
            return Err(Error::sentinel(Kind::AssociationExclusivelyOwned));
        }
        let ctx = self.context.clone();
        let Some(conn) = self.connection() else {
            return Err(closed_association_error(&ctx));
        };
        self.write_pdu_on(&ctx, &conn, pdu)
    }
    fn write_pdu_on(&self, ctx: &Ctx, conn: &Conn, pdu: &Pdu) -> Result<()> {
        let _guard = self
            .write_lock
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let progress = ctx.write_progress.or(self.write_progress_timeout);
        let mut written = Written::default();
        let result = match progress {
            None => write_association_pdu_counted(ctx, conn, pdu, &mut written),
            Some(timeout) => write_with_progress(ctx, conn, pdu, timeout, &mut written),
        };
        if self.lifecycle.enabled() {
            let limit =
                (self.lifecycle.max_raw_pdu_bytes.max(0) as usize).max(PDU_HEADER_SIZE as usize);
            let captured = written.captured(limit);
            let n = written.n as i64;
            match &result {
                Err(e) => self
                    .lifecycle
                    .observe_pdu_error(Direction::Outbound, captured, n, n, e),
                Ok(()) => self
                    .lifecycle
                    .observe_pdu(Direction::Outbound, pdu, n, n, captured),
            }
        }
        if result.is_ok() {
            self.observe_lifecycle_pdu(true, pdu);
        }
        result
    }
    pub fn receive(&self, ctx: Option<&Ctx>) -> Result<Pdu> {
        if !self.exclusive_operation_allowed(ctx) {
            return Err(Error::sentinel(Kind::AssociationExclusivelyOwned));
        }
        let ctx = self.ctx_or_default(ctx);
        self.receive_on(&ctx)
    }
    pub fn read_pdu(&self) -> Result<Pdu> {
        if !self.exclusive_operation_allowed(None) {
            return Err(Error::sentinel(Kind::AssociationExclusivelyOwned));
        }
        let ctx = self.context.clone();
        self.receive_on(&ctx)
    }
    fn receive_on(&self, ctx: &Ctx) -> Result<Pdu> {
        let Some(conn) = self.connection() else {
            return Err(closed_association_error(ctx));
        };
        let pdu = self.read_pdu_on(ctx, &conn)?;
        if let Pdu::Abort(a) = pdu {
            self.lifecycle.mark_aborted();
            self.close();
            return Err(Error::abort(a.source, a.reason));
        }
        Ok(pdu)
    }
    fn read_pdu_on(&self, ctx: &Ctx, conn: &Conn) -> Result<Pdu> {
        let _reading = super::conn::PduReadGuard::new(conn);
        let mut buf = self
            .read_buffer
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let initial = buf.len();
        let observe_error = |buf: &[u8], e: &Error| {
            let n = buf.len() as i64;
            self.lifecycle.observe_pdu_error(
                Direction::Inbound,
                buf,
                n,
                n - initial.min(buf.len()) as i64,
                e,
            );
        };
        if let Err(e) = self.fill(&mut buf, ctx, conn, PDU_HEADER_SIZE as usize) {
            observe_error(&buf, &e);
            return Err(e);
        }
        let pdu_type = buf[0];
        let length = u32::from_be_bytes([buf[2], buf[3], buf[4], buf[5]]);
        if let Err(e) = validate_read_pdu_length(
            pdu_type,
            length,
            self.max_pdu,
            resolve_max_association_pdu(self.max_association_pdu),
        ) {
            observe_error(&buf, &e);
            buf.clear();
            return Err(e);
        }
        let total = PDU_HEADER_SIZE as usize + length as usize;
        if let Err(e) = self.fill(&mut buf, ctx, conn, total) {
            observe_error(&buf, &e);
            return Err(e);
        }
        let encoded = std::mem::take(&mut *buf);
        drop(buf);
        let parsed = parse_pdu_body(pdu_type, &encoded[PDU_HEADER_SIZE as usize..total]);
        let n = total as i64;
        let fresh = n - initial.min(total) as i64;
        match &parsed {
            Err(e) => {
                self.lifecycle
                    .observe_pdu_error(Direction::Inbound, &encoded[..total], n, fresh, e)
            }
            Ok(pdu) => {
                self.lifecycle
                    .observe_pdu(Direction::Inbound, pdu, n, fresh, &encoded[..total]);
                self.observe_lifecycle_pdu(false, pdu);
            }
        }
        parsed
    }
    fn fill(&self, buf: &mut Vec<u8>, ctx: &Ctx, conn: &Conn, target: usize) -> Result<()> {
        while buf.len() < target {
            let want = (target - buf.len()).min(PDU_READ_GROWTH.max(buf.len()));
            let start = buf.len();
            buf.resize(start + want, 0);
            let read_ctx = match self.next_read_timeout(ctx, start) {
                Some(t) => ctx.with_timeout(t),
                None => ctx.clone(),
            };
            let result = conn.read_ctx(&mut buf[start..], &read_ctx);
            let n = *result.as_ref().unwrap_or(&0);
            buf.truncate(start + n);
            if buf.len() >= target {
                return Ok(());
            }
            match result {
                Err(e) => return Err(e),
                Ok(0) => return Err(Error::eof()),
                Ok(_) => {}
            }
        }
        Ok(())
    }
    fn next_read_timeout(&self, ctx: &Ctx, buffered: usize) -> Option<Duration> {
        if buffered == 0 && !ctx.active_transfer {
            if ctx.no_idle {
                return None;
            }
            if self.idle_timeout.is_some() {
                return self.idle_timeout;
            }
        }
        if let Some(t) = ctx.read_progress {
            return Some(t);
        }
        if self.read_progress_timeout.is_some() {
            return self.read_progress_timeout;
        }
        if buffered != 0 {
            return self.idle_timeout;
        }
        None
    }
    fn observe_lifecycle_pdu(&self, outbound: bool, pdu: &Pdu) {
        let mut f = self
            .release_flags
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        match pdu {
            Pdu::ReleaseRq => {
                if outbound {
                    f.local_requested = true
                } else {
                    f.remote_requested = true
                }
            }
            Pdu::ReleaseRp => {
                if outbound {
                    f.local_responded = true
                } else {
                    f.remote_responded = true
                }
            }
            Pdu::Abort(_) => {
                drop(f);
                self.lifecycle.mark_aborted();
                return;
            }
            _ => {}
        }
        let completed = if f.local_requested && f.remote_requested {
            f.local_responded && f.remote_responded
        } else if f.local_requested {
            f.remote_responded
        } else if f.remote_requested {
            f.local_responded
        } else {
            false
        };
        drop(f);
        if completed {
            self.lifecycle.mark_released();
        }
    }
    pub fn release(&self, ctx: Option<&Ctx>) -> Result<()> {
        if !self.exclusive_operation_allowed(ctx) {
            return Err(Error::sentinel(Kind::AssociationExclusivelyOwned));
        }
        if self.connection().is_none() {
            return Ok(());
        }
        let base = self.ctx_or_default(ctx);
        let ctx = match self.release_timeout {
            Some(t) => base.with_timeout(t),
            None => base,
        }
        .without_idle_timeout();
        if let Err(e) = self.send(Some(&ctx), &Pdu::ReleaseRq) {
            self.cleanup_release_error(&e);
            return Err(e);
        }
        loop {
            let pdu = match self.receive(Some(&ctx)) {
                Ok(p) => p,
                Err(e) => {
                    self.cleanup_release_error(&e);
                    return Err(e);
                }
            };
            match pdu {
                Pdu::ReleaseRp => {
                    self.close();
                    return Ok(());
                }
                Pdu::PData(_) => continue,
                Pdu::ReleaseRq => {
                    if let Err(e) = self.send(Some(&ctx), &Pdu::ReleaseRp) {
                        self.cleanup_release_error(&e);
                        return Err(e);
                    }
                }
                other => {
                    let _ = self.abort_with_context(Some(&ctx), ABORT_REASON_UNEXPECTED_PDU);
                    self.close();
                    return Err(Error::wrap(
                        Kind::UnexpectedPdu,
                        format!("got {} while waiting for A-RELEASE-RP", other.kind_label()),
                    ));
                }
            }
        }
    }
    fn cleanup_release_error(&self, err: &Error) {
        if err.is(Kind::AssociationTimeout) {
            let ctx = Ctx::background().with_timeout(Duration::from_millis(100));
            let _ = self.abort_with_context(Some(&ctx), ABORT_REASON_NOT_SPECIFIED);
            return;
        }
        self.close();
    }
    pub fn abort(&self, reason: u8) -> Result<()> {
        let ctx = self.context.clone();
        self.abort_with_context(Some(&ctx), reason)
    }
    pub fn abort_with_context(&self, ctx: Option<&Ctx>, reason: u8) -> Result<()> {
        self.abort_from(ctx, ABORT_SOURCE_SERVICE_USER, reason)
    }
    pub fn abort_for_error(&self, ctx: Option<&Ctx>, err: &Error) -> Result<()> {
        match provider_abort_reason(err) {
            Some(reason) => self.abort_from(ctx, ABORT_SOURCE_SERVICE_PROVIDER, reason),
            None => self.abort_from(ctx, ABORT_SOURCE_SERVICE_USER, ABORT_REASON_NOT_SPECIFIED),
        }
    }
    fn abort_from(&self, ctx: Option<&Ctx>, source: u8, reason: u8) -> Result<()> {
        if !self.exclusive_operation_allowed(ctx) {
            return Err(Error::sentinel(Kind::AssociationExclusivelyOwned));
        }
        if self.connection().is_none() {
            return Ok(());
        }
        let result = self.send(ctx, &Pdu::Abort(AbortRq { source, reason }));
        self.close_lingering();
        result
    }
    pub fn close(&self) {
        let conn = self
            .conn
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .carryover
            .clear();
        self.lifecycle.close();
        if let Some(conn) = conn {
            conn.close();
        }
    }
    fn close_lingering(&self) {
        let conn = self
            .conn
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .carryover
            .clear();
        self.lifecycle.close();
        if let Some(conn) = conn {
            linger_close(&conn, self.artim_timeout);
        }
    }
}
fn closed_association_error(ctx: &Ctx) -> Error {
    ended_context_error(ctx).unwrap_or_else(Error::closed)
}
pub(crate) fn read_association_pdu(
    ctx: &Ctx,
    conn: &Conn,
    max_pdu: u32,
    max_association_pdu: u32,
) -> Result<Pdu> {
    let mut reader = CtxReader { conn, ctx };
    read_pdu_with_association_limit(&mut reader, max_pdu, max_association_pdu).map_err(|e| {
        match ended_context_error(ctx) {
            Some(c) => c,
            None => association_context_error(ctx, e),
        }
    })
}
#[derive(Default)]
pub(crate) struct Written {
    encoded: Vec<u8>,
    n: usize,
}
impl Written {
    fn captured(&self, limit: usize) -> &[u8] {
        &self.encoded[..self.n.min(limit).min(self.encoded.len())]
    }
}
fn write_association_pdu_counted(
    ctx: &Ctx,
    conn: &Conn,
    pdu: &Pdu,
    written: &mut Written,
) -> Result<()> {
    written.encoded = encode_pdu(pdu).map_err(|e| match ended_context_error(ctx) {
        Some(c) => c,
        None => e,
    })?;
    let (n, result) = conn.write_counted(&written.encoded, ctx);
    written.n = n;
    result.map_err(|e| match ended_context_error(ctx) {
        Some(c) => c,
        None => association_context_error(ctx, e.prefixed("dicom ul: write PDU")),
    })
}
fn write_with_progress(
    ctx: &Ctx,
    conn: &Conn,
    pdu: &Pdu,
    timeout: Duration,
    written: &mut Written,
) -> Result<()> {
    written.encoded = encode_pdu(pdu)?;
    let total = written.encoded.len();
    let mut offset = 0;
    while offset < total {
        let end = (offset + 32 * 1024).min(total);
        let chunk_ctx = ctx.with_timeout(timeout);
        let (n, result) = conn.write_counted(&written.encoded[offset..end], &chunk_ctx);
        written.n += n;
        offset = end;
        if let Err(e) = result {
            if let Some(c) = ended_context_error(ctx) {
                return Err(c.prefixed("dicom ul: write PDU"));
            }
            let e = if chunk_ctx.err().is_some() {
                Error::wrap(
                    Kind::AssociationTimeout,
                    format!(
                        "write tcp {}->{}: i/o timeout",
                        conn.local_addr(),
                        conn.peer_addr()
                    ),
                )
            } else {
                e
            };
            return Err(e.prefixed("dicom ul: write PDU"));
        }
    }
    Ok(())
}
pub(crate) fn write_observed(
    ctx: &Ctx,
    conn: &Conn,
    pdu: &Pdu,
    lifecycle: &Lifecycle,
) -> Result<()> {
    let mut written = Written::default();
    let result = write_association_pdu_counted(ctx, conn, pdu, &mut written);
    if lifecycle.enabled() {
        let n = written.n as i64;
        let captured = written.captured(PDU_HEADER_SIZE as usize);
        match &result {
            Err(e) => lifecycle.observe_pdu_error(Direction::Outbound, captured, n, n, e),
            Ok(()) => lifecycle.observe_pdu(Direction::Outbound, pdu, n, n, &[]),
        }
    }
    if result.is_ok() {
        match pdu {
            Pdu::AssociateRj(rj) => lifecycle.mark_rejected(Some(rj)),
            Pdu::Abort(_) => lifecycle.mark_aborted(),
            _ => {}
        }
    }
    result
}
struct CountingReader<'a> {
    inner: CtxReader<'a>,
    n: i64,
    prefix: Vec<u8>,
}
impl super::reader::ByteRead for CountingReader<'_> {
    fn read_avail(&mut self, buf: &mut [u8]) -> Result<usize> {
        let n = self.inner.read_avail(buf)?;
        self.n += n as i64;
        let want = (PDU_HEADER_SIZE as usize)
            .saturating_sub(self.prefix.len())
            .min(n);
        self.prefix.extend_from_slice(&buf[..want]);
        Ok(n)
    }
}
pub(crate) fn read_observed(
    ctx: &Ctx,
    conn: &Conn,
    max_pdu: u32,
    max_association_pdu: u32,
    lifecycle: &Lifecycle,
) -> Result<Pdu> {
    let result = if lifecycle.enabled() {
        let mut reader = CountingReader {
            inner: CtxReader { conn, ctx },
            n: 0,
            prefix: Vec::new(),
        };
        let result = read_pdu_with_association_limit(&mut reader, max_pdu, max_association_pdu)
            .map_err(|e| match ended_context_error(ctx) {
                Some(c) => c,
                None => association_context_error(ctx, e),
            });
        match &result {
            Err(e) => lifecycle.observe_pdu_error(
                Direction::Inbound,
                &reader.prefix,
                reader.n,
                reader.n,
                e,
            ),
            Ok(pdu) => lifecycle.observe_pdu(Direction::Inbound, pdu, reader.n, reader.n, &[]),
        }
        result
    } else {
        read_association_pdu(ctx, conn, max_pdu, max_association_pdu)
    };
    let pdu = result?;
    match &pdu {
        Pdu::AssociateRj(rj) => lifecycle.mark_rejected(Some(rj)),
        Pdu::Abort(_) => lifecycle.mark_aborted(),
        _ => {}
    }
    Ok(pdu)
}
fn spawn_detached(f: impl FnOnce() + Send + 'static) {
    std::thread::spawn(f);
}
pub fn dial(address: &str, opts: DialOptions) -> Result<Association> {
    let mut opts = normalize_dial_options(opts);
    apply_allowlist(&mut opts)?;
    validate_dial_options(&opts)?;
    let lifecycle = OperationalState::new(
        opts.observability.as_ref(),
        AssociationRole::Scu,
        new_association_id(),
    )?;
    let base = opts.context.clone().unwrap_or_default();
    let negotiation_ctx = base.with_optional_timeout(opts.negotiation_timeout);
    let (local_address, socket_buffers) = (opts.transport.local_address, opts.socket_buffers);
    let dialed = opts
        .transport
        .dial(&negotiation_ctx, address, |destination, ctx| {
            connect_with(destination, ctx, local_address.as_ref(), &socket_buffers)
        })
        .map_err(|e| association_context_error(&negotiation_ctx, e));
    let conn = match dialed {
        Ok(c) => Arc::new(c),
        Err(e) => {
            lifecycle.close();
            return Err(e);
        }
    };
    lifecycle.set_connection(&conn);
    if opts.transport.http_connect.is_some() {
        lifecycle.set_logical_remote_address(address);
    }
    opts.context = Some(negotiation_ctx.clone());
    #[cfg(feature = "tls")]
    if let Some(tls) = &opts.tls_config
        && let Err(e) = conn.start_tls_client(address, tls, &negotiation_ctx)
    {
        lifecycle.close();
        conn.close();
        return Err(e);
    }
    match negotiate_scu(&conn, &opts, &lifecycle) {
        Ok(mut assoc) => {
            assoc.context = Ctx::background();
            lifecycle.mark_established();
            Ok(assoc)
        }
        Err(err) => {
            #[cfg(feature = "tls")]
            let err = match conn.tls() {
                Some(tls) => tls.refusal(&conn, err),
                None => err,
            };
            let rejected_or_aborted = err.is(Kind::AssociationRejected)
                && err.as_rejection().is_some()
                || matches!(err.detail(), super::error::Detail::Abort(..));
            if rejected_or_aborted {
                lifecycle.close();
                conn.close();
                return Err(err);
            }
            let (source, reason) = match provider_abort_reason(&err) {
                Some(r) => (ABORT_SOURCE_SERVICE_PROVIDER, r),
                None => (ABORT_SOURCE_SERVICE_USER, ABORT_REASON_NOT_SPECIFIED),
            };
            let artim = artim_timeout(opts.artim_timeout);
            let abort = Pdu::Abort(AbortRq { source, reason });
            if context_limit_reached(&negotiation_ctx) {
                let write_ctx = negotiation_ctx
                    .without_cancel()
                    .with_timeout(artim + Duration::from_secs(1));
                let conn = conn.clone();
                lifecycle.close();
                let lifecycle = lifecycle.clone();
                spawn_detached(move || {
                    let _ = write_observed(&write_ctx, &conn, &abort, &lifecycle);
                    linger_close(&conn, artim);
                });
            } else {
                let _ = write_observed(&negotiation_ctx, &conn, &abort, &lifecycle);
                linger_close(&conn, artim);
                lifecycle.close();
            }
            Err(err)
        }
    }
}
pub fn dial_context(ctx: &Ctx, address: &str, mut opts: DialOptions) -> Result<Association> {
    opts.context = Some(ctx.clone());
    dial(address, opts)
}
fn connect_with(
    address: &str,
    ctx: &Ctx,
    local: Option<&SocketAddr>,
    buffers: &super::socket_buffers::SocketBuffers,
) -> Result<Conn> {
    use super::local_endpoint::{LocalEndpointKind, retry_unavailable_source_port};
    let kind_of = |err: &Error| {
        if err.is(Kind::AddressInUse) {
            LocalEndpointKind::AddressInUse
        } else if err.is(Kind::AddressNotAvailable) {
            LocalEndpointKind::AddressNotAvailable
        } else if err.is(Kind::NoBufferSpace) {
            LocalEndpointKind::NoBuffers
        } else {
            LocalEndpointKind::NotLocalEndpoint
        }
    };
    retry_unavailable_source_port(ctx, local, address, kind_of, || {
        connect_once(address, ctx, local, buffers)
    })
}
fn connect_once(
    address: &str,
    ctx: &Ctx,
    local: Option<&SocketAddr>,
    buffers: &super::socket_buffers::SocketBuffers,
) -> Result<Conn> {
    let source = match local {
        Some(l) => l.to_string(),
        None => "<nil>".to_string(),
    };
    let dial_error =
        |message: String| Error::other(format!("dial tcp {source}->{address}: {message}"));
    if ctx.err() == Some(Kind::Canceled) {
        return Err(dial_error("operation was canceled".into()).also(Kind::Canceled));
    }
    let addrs: Vec<SocketAddr> = address
        .to_socket_addrs()
        .map_err(|e| {
            let err = dial_error(e.to_string());
            if is_lookup_failure(&e) {
                err.also(Kind::NameLookup)
            } else {
                err
            }
        })?
        .collect();
    let addrs: Vec<SocketAddr> = addrs
        .into_iter()
        .filter(|a| local.is_none_or(|l| l.is_ipv4() == a.is_ipv4() || l.ip().is_unspecified()))
        .collect();
    if addrs.is_empty() && local.is_some() {
        return Err(Error::other(format!(
            "dial tcp: address {address}: no suitable address found"
        )));
    }
    let mut last = None;
    for addr in addrs {
        let left = match ctx.deadline() {
            Some(d) => {
                let left = d.saturating_duration_since(Instant::now());
                if left.is_zero() {
                    return Err(super::conn::context_deadline_error());
                }
                Some(left)
            }
            None => None,
        };
        let result = super::local_endpoint::connect_socket(&addr, local, buffers, left);
        match result {
            Ok(stream) => {
                return Conn::new(stream).map_err(|e| dial_error(e.to_string()));
            }
            Err(e) => last = Some(e),
        }
    }
    if let Some(c) = ended_context_error(ctx) {
        return Err(c);
    }
    let errno = last.as_ref().is_some_and(|e| {
        e.raw_os_error().is_some()
            && !matches!(
                e.kind(),
                std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
            )
    });
    let marker = last
        .as_ref()
        .and_then(|e| super::local_endpoint::classify_local_endpoint_error(e).marker());
    let code = last.as_ref().and_then(|e| e.raw_os_error());
    let e = last
        .map(|e| e.to_string())
        .unwrap_or_else(|| "no address".into());
    let e = match e.find(" (os error") {
        Some(i) => e[..i].to_string(),
        None => e,
    };
    let err = dial_error(e);
    let err = if errno {
        err.also(Kind::Syscall).with_os_error(code)
    } else {
        err
    };
    Err(match marker {
        Some(kind) => err.also(kind),
        None => err,
    })
}
fn negotiate_scu(
    conn: &Arc<Conn>,
    opts: &DialOptions,
    lifecycle: &Arc<Lifecycle>,
) -> Result<Association> {
    let ctx = opts.context.clone().unwrap_or_default();
    let rq = build_associate_rq(opts);
    write_observed(&ctx, conn, &Pdu::AssociateRq(rq.clone()), lifecycle)?;
    let pdu = read_observed(
        &ctx,
        conn,
        opts.max_pdu,
        opts.max_association_pdu,
        lifecycle,
    )?;
    match pdu {
        Pdu::AssociateAc(ac) => {
            let (accepted, outcomes) =
                process_association_ac(&ac, &rq.presentation_contexts, opts)?;
            let user_info = ac.user_info.clone().unwrap_or_default();
            let identity_response = user_identity_response_from_user_info(&user_info)?;
            let accepted_roles =
                accepted_role_selections_from_user_info(&user_info, &opts.role_selections)?;
            let accepted_ext = accepted_extended_negotiation_from_user_info(
                &user_info,
                &opts.extended_negotiation,
            )?;
            let accepted_async = asynchronous_operations_window_from_user_info(&user_info)?;
            if accepted_async.is_some() && opts.asynchronous_operations_window.is_none() {
                return Err(Error::wrap(
                    Kind::InvalidUserItem,
                    "asynchronous operations window was not requested",
                ));
            }
            if let (Some(a), Some(o)) = (accepted_async, opts.asynchronous_operations_window) {
                validate_accepted_asynchronous_operations_window(a, o)?;
            }
            let mut assoc = Association::from_parts(Parts {
                conn: conn.clone(),
                lifecycle: lifecycle.clone(),
                idle: opts.idle_timeout,
                read_progress: opts.read_progress_timeout,
                write_progress: opts.write_progress_timeout,
                release: opts.release_timeout,
                artim: opts.artim_timeout,
                max_send_pdu: opts.max_send_pdu,
                max_association_pdu: opts.max_association_pdu,
            });
            assoc.called_ae_title = rq.called_ae_title.clone();
            assoc.calling_ae_title = rq.calling_ae_title.clone();
            assoc.contexts = rq.presentation_contexts.clone();
            assoc.accepted_contexts = accepted;
            assoc.context_outcomes = outcomes;
            assoc.peer_max_pdu = max_pdu_from_user_info(&user_info);
            assoc.max_pdu = opts.max_pdu;
            assoc.protocol_version = ac.protocol_version;
            assoc.application_context_name = ac.application_context_name.clone();
            assoc.user_identity_responded = identity_response.is_some();
            assoc.user_identity_response = identity_response.unwrap_or_default();
            assoc.requested_role_selections = opts.role_selections.clone();
            assoc.accepted_role_selections = accepted_roles;
            assoc.requested_extended_negotiation = opts.extended_negotiation.clone();
            assoc.accepted_extended_negotiation = accepted_ext;
            assoc.requested_common_extended_negotiation = opts.common_extended_negotiation.clone();
            assoc.requested_asynchronous_operations_window = opts.asynchronous_operations_window;
            assoc.negotiated_asynchronous_operations_window =
                accepted_async.unwrap_or(AsynchronousOperationsWindow {
                    maximum_invoked: 1,
                    maximum_performed: 1,
                });
            assoc.asynchronous_operations_negotiated = accepted_async.is_some();
            assoc.is_association_requestor = true;
            Ok(assoc)
        }
        Pdu::AssociateRj(rj) => Err(Error::rejection(rj)),
        Pdu::Abort(a) => Err(Error::abort(a.source, a.reason)),
        other => Err(unexpected_pdu_error(&other, "waiting for A-ASSOCIATE-AC")),
    }
}
#[derive(Debug, Clone, Default)]
pub struct ListenOptions {
    pub address: String,
    pub context: Option<Ctx>,
    pub socket_buffers: super::socket_buffers::SocketBuffers,
}
#[derive(Debug)]
pub struct Listener {
    listener: std::sync::Mutex<Option<TcpListener>>,
    addr: Option<SocketAddr>,
    closed: AtomicBool,
    buffers: super::socket_buffers::SocketBuffers,
}
pub fn listen(opts: ListenOptions) -> Result<Listener> {
    let address = if opts.address.is_empty() {
        "127.0.0.1:0".to_string()
    } else {
        opts.address
    };
    opts.socket_buffers.validate()?;
    let buffers = opts.socket_buffers;
    let bound = if buffers.is_zero() {
        TcpListener::bind(&address)
    } else {
        address.to_socket_addrs().and_then(|addrs| {
            let mut last = std::io::Error::new(std::io::ErrorKind::NotFound, "no address");
            for addr in addrs {
                match super::local_endpoint::listen_socket(&addr, &buffers) {
                    Ok(listener) => return Ok(listener),
                    Err(e) => last = e,
                }
            }
            Err(last)
        })
    };
    let listener = bound.map_err(|e| listen_error(&address, &e))?;
    let _ = listener.set_nonblocking(true);
    Ok(Listener {
        addr: listener.local_addr().ok(),
        listener: std::sync::Mutex::new(Some(listener)),
        closed: AtomicBool::new(false),
        buffers,
    })
}
fn listen_error(address: &str, e: &std::io::Error) -> Error {
    let text = e.to_string();
    let text = match text.find(" (os error") {
        Some(i) => text[..i].to_string(),
        None => text,
    };
    let err = Error::other(format!("listen tcp {address}: {text}"));
    if is_lookup_failure(e) {
        return err.also(Kind::NameLookup);
    }
    let Some(code) = e.raw_os_error() else {
        return err;
    };
    let err = err.also(Kind::Syscall).with_os_error(Some(code));
    match super::local_endpoint::classify_local_endpoint_error(e).marker() {
        Some(kind) => err.also(kind),
        None => err,
    }
}
fn is_lookup_failure(e: &std::io::Error) -> bool {
    match e.raw_os_error() {
        Some(code) => cfg!(windows) && (11001..=11004).contains(&code),
        None => e
            .to_string()
            .contains("failed to lookup address information"),
    }
}
impl Listener {
    pub fn local_addr(&self) -> Option<SocketAddr> {
        self.addr
    }
    pub fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
        let socket = self
            .listener
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        drop(socket);
    }
    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }
    #[doc(hidden)]
    pub fn accept_conn(&self, ctx: &Ctx) -> Result<Conn> {
        loop {
            if self.closed.load(Ordering::SeqCst) {
                return Err(Error::closed());
            }
            if let Some(e) = ended_context_error(ctx) {
                return Err(e);
            }
            let accepted = {
                let guard = self.listener.lock().unwrap_or_else(|e| e.into_inner());
                match guard.as_ref() {
                    Some(listener) => listener.accept(),
                    None => return Err(Error::closed()),
                }
            };
            match accepted {
                Ok((stream, _)) => {
                    let _ = stream.set_nonblocking(false);
                    self.buffers.apply(&stream)?;
                    return Conn::new(stream).map_err(|e| Error::other(e.to_string()));
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(2));
                }
                Err(e) => return Err(Error::other(format!("accept tcp: {e}"))),
            }
        }
    }
    pub fn accept_association(&self, opts: AcceptOptions) -> Result<Association> {
        let opts = normalize_accept_options(opts);
        validate_operational_timeouts(
            opts.negotiation_timeout,
            opts.idle_timeout,
            opts.read_progress_timeout,
            opts.write_progress_timeout,
            opts.release_timeout,
        )?;
        let ctx = opts.context.clone().unwrap_or_default();
        let conn = Arc::new(self.accept_conn(&ctx)?);
        accept(conn.clone(), opts).inspect_err(|_| conn.close())
    }
}
pub(crate) fn notify_rejection(
    opts: &AcceptOptions,
    request: Option<&AssociationRq>,
    conn: &Conn,
    rj: &AssociationRj,
) {
    if let Some(cb) = &opts.on_association_rejected {
        let cb = cb.clone();
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| cb()));
    }
    notify_rejection_audit(opts, request, conn, rj);
}
pub(crate) fn notify_rejection_audit(
    opts: &AcceptOptions,
    request: Option<&AssociationRq>,
    conn: &Conn,
    rj: &AssociationRj,
) {
    if let Some(cb) = &opts.on_association_rejection {
        let mut rejected = RejectedAssociation {
            remote_addr: Some(conn.peer_addr()),
            rejection: *rj,
            ..Default::default()
        };
        if let Some(rq) = request {
            if validate_ae_title(&rq.calling_ae_title).is_ok() {
                rejected.calling_ae_title = normalize_ae_title(&rq.calling_ae_title).to_string();
            }
            if validate_ae_title(&rq.called_ae_title).is_ok() {
                rejected.called_ae_title = normalize_ae_title(&rq.called_ae_title).to_string();
            }
        }
        let cb = cb.clone();
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| cb(&rejected)));
    }
}
fn reject_and_close(
    opts: &AcceptOptions,
    ctx: &Ctx,
    conn: &Conn,
    rj: &AssociationRj,
    lifecycle: &Lifecycle,
) -> Option<Error> {
    notify_rejection(opts, opts.rejected_request.as_ref(), conn, rj);
    let err = write_observed(ctx, conn, &Pdu::AssociateRj(*rj), lifecycle).err();
    linger_close(conn, artim_timeout(opts.artim_timeout));
    err
}
fn provider_abort_and_close(
    ctx: &Ctx,
    conn: &Conn,
    err: &Error,
    artim: Duration,
    lifecycle: &Lifecycle,
) {
    let Some(reason) = provider_abort_reason(err) else {
        conn.close();
        return;
    };
    let ctx = if context_limit_reached(ctx) {
        ctx.without_cancel()
            .with_timeout(artim + Duration::from_secs(1))
    } else {
        ctx.clone()
    };
    let _ = write_observed(
        &ctx,
        conn,
        &Pdu::Abort(AbortRq {
            source: ABORT_SOURCE_SERVICE_PROVIDER,
            reason,
        }),
        lifecycle,
    );
    linger_close(conn, artim);
}
fn unexpected_association_response(pdu: &Pdu) -> Pdu {
    match pdu {
        Pdu::ReleaseRq => Pdu::ReleaseRp,
        Pdu::Unknown(_) => Pdu::Abort(AbortRq {
            source: ABORT_SOURCE_SERVICE_PROVIDER,
            reason: ABORT_REASON_UNRECOGNIZED_PDU,
        }),
        _ => Pdu::Abort(AbortRq {
            source: ABORT_SOURCE_SERVICE_PROVIDER,
            reason: ABORT_REASON_UNEXPECTED_PDU,
        }),
    }
}
pub fn accept(conn: Arc<Conn>, opts: AcceptOptions) -> Result<Association> {
    let mut opts = normalize_accept_options(opts);
    validate_called_ae_options(&opts)?;
    validate_operational_timeouts(
        opts.negotiation_timeout,
        opts.idle_timeout,
        opts.read_progress_timeout,
        opts.write_progress_timeout,
        opts.release_timeout,
    )?;
    validate_presentation_context_rules(&opts.presentation_context_rules)?;
    validate_called_ae_presentation_context_rules(&opts.called_ae_presentation_context_rules)?;
    let association_ctx = opts.context.clone().unwrap_or_default();
    let ctx = association_ctx.with_optional_timeout(opts.negotiation_timeout);
    let lifecycle = OperationalState::new(
        opts.observability.as_ref(),
        AssociationRole::Scp,
        new_association_id(),
    )?;
    lifecycle.set_connection(&conn);
    #[cfg(feature = "tls")]
    if let Some(tls) = opts.tls_config.clone()
        && let Err(e) = conn.start_tls_server(&tls, &ctx)
    {
        notify_tls_handshake_failure(&opts, &conn, &e);
        conn.close();
        lifecycle.close();
        return Err(e);
    }
    let result = accept_negotiate(&conn, &mut opts, &ctx, &lifecycle);
    match result {
        Ok(mut assoc) => {
            assoc.context = association_ctx;
            lifecycle.mark_established();
            Ok(assoc)
        }
        Err(e) => {
            lifecycle.close();
            Err(e)
        }
    }
}
#[cfg(feature = "tls")]
fn notify_tls_handshake_failure(opts: &AcceptOptions, conn: &Conn, err: &Error) {
    let Some(cb) = &opts.on_tls_handshake_failure else {
        return;
    };
    let failure = super::tls_conn::TlsHandshakeFailure {
        remote_addr: Some(conn.peer_addr()),
        not_tls: err.tls_failure() == Some(super::error::TlsFailureClass::NotTls),
        err: err.clone(),
    };
    let cb = cb.clone();
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| cb(&failure)));
}
fn no_reason() -> AssociationRj {
    AssociationRj {
        result: ASSOCIATE_RJ_RESULT_PERMANENT,
        source: ASSOCIATE_RJ_SOURCE_SERVICE_USER,
        reason: ASSOCIATE_RJ_REASON_NO_REASON_GIVEN,
    }
}
fn accept_negotiate(
    conn: &Arc<Conn>,
    opts: &mut AcceptOptions,
    ctx: &Ctx,
    lifecycle: &Arc<Lifecycle>,
) -> Result<Association> {
    let artim = artim_timeout(opts.artim_timeout);
    let pdu = match read_observed(ctx, conn, opts.max_pdu, opts.max_association_pdu, lifecycle) {
        Ok(p) => p,
        Err(e) => {
            provider_abort_and_close(ctx, conn, &e, artim, lifecycle);
            return Err(e);
        }
    };
    let rq = match pdu {
        Pdu::AssociateRq(rq) => rq,
        other => {
            let _ = write_observed(
                ctx,
                conn,
                &unexpected_association_response(&other),
                lifecycle,
            );
            linger_close(conn, artim);
            return Err(unexpected_pdu_error(&other, "waiting for A-ASSOCIATE-RQ"));
        }
    };
    opts.rejected_request = Some(rq.clone());
    if let Some(rj) = reject_association_request(&rq, opts) {
        if let Some(e) = reject_and_close(opts, ctx, conn, &rj, lifecycle) {
            return Err(e);
        }
        let rejection = Error::rejection(rj);
        if let Some(cause) = invalid_request_ae_title(&rq) {
            return Err(Error::join([Some(rejection), Some(cause)]).unwrap());
        }
        return Err(rejection);
    }
    if let Some(registry) = opts.called_ae_registry.clone() {
        match registry.lookup(&rq.called_ae_title) {
            None => {
                let rj = reject_called_ae_title();
                if let Some(e) = reject_and_close(opts, ctx, conn, &rj, lifecycle) {
                    return Err(e);
                }
                return Err(Error::rejection(rj));
            }
            Some(selected) => {
                opts.ae_title = selected.ae_title.clone();
                opts.presentation_context_rules = selected.profile.rules.clone();
            }
        }
    }
    if let Some(rules) = called_ae_presentation_context_rules(
        &opts.called_ae_presentation_context_rules,
        &rq.called_ae_title,
    ) {
        opts.presentation_context_rules = rules.to_vec();
    }
    #[allow(unused_mut)]
    let mut request = admission_request(Some(conn.peer_addr()), Some(conn.local_addr()), &rq);
    #[cfg(feature = "tls")]
    {
        request.tls = conn.tls_connection_state().cloned();
    }
    let mut decided_identity: Option<UserIdentityHandler> = None;
    let admission_outcome = if let Some(decider) = opts.admission_decider.clone() {
        let handler: super::admission::AdmitFn<AdmissionDecision> = decider;
        let (rj, value, err) = admit(ctx, handler, request, |d: &AdmissionDecision| d.reject);
        if let Some(v) = value {
            decided_identity = v.user_identity;
        }
        Some((rj, err))
    } else if let Some(handler) = opts.admission.clone() {
        let (rj, _, err) = admit(ctx, handler, request, |r: &Option<AssociationRj>| *r);
        Some((rj, err))
    } else {
        None
    };
    if let Some((Some(rj), admission_err)) = admission_outcome {
        let write_ctx = if context_limit_reached(ctx) {
            ctx.without_cancel()
                .with_timeout(ADMISSION_REJECT_WRITE_TIMEOUT)
        } else {
            ctx.clone()
        };
        let write_err = reject_and_close(opts, &write_ctx, conn, &rj, lifecycle);
        return Err(Error::join([Some(Error::rejection(rj)), admission_err, write_err]).unwrap());
    }
    if decided_identity.is_some() {
        opts.user_identity_handler = decided_identity;
    }
    let user_info = rq.user_info.clone().unwrap_or_default();
    let reject_with = |err: Error| -> Error {
        let _ = reject_and_close(opts, ctx, conn, &no_reason(), lifecycle);
        err
    };
    let (identity_req, identity_result, identity_err) =
        handle_user_identity(&user_info, opts.user_identity_handler.as_ref());
    if let Some(e) = identity_err {
        return Err(reject_with(e));
    }
    let requested_roles = role_selections_from_user_info(&user_info).map_err(&reject_with)?;
    let requested_common =
        common_extended_negotiation_from_user_info(&user_info).map_err(&reject_with)?;
    let (results, accepted) = negotiate_presentation_contexts_with_roles(
        &rq.presentation_contexts,
        opts,
        &requested_roles,
        &requested_common,
    );
    let outcomes = presentation_context_outcomes_from_results(&rq.presentation_contexts, &results);
    if accepted.is_empty() {
        return Err(reject_with(Error::no_accepted(outcomes)));
    }
    let accepted_roles =
        negotiate_role_selections(&requested_roles, opts, &accepted).map_err(&reject_with)?;
    let (requested_ext, accepted_ext, ext_err) = negotiate_extended_negotiation(
        &user_info,
        opts.extended_negotiation_handler.as_ref(),
        &accepted,
    );
    if let Some(e) = ext_err {
        return Err(reject_with(e));
    }
    let requested_async =
        asynchronous_operations_window_from_user_info(&user_info).map_err(&reject_with)?;
    let accepted_async = match (requested_async, opts.asynchronous_operations_window) {
        (Some(r), Some(local)) => Some(negotiate_asynchronous_operations_window(r, local)),
        _ => None,
    };
    let ac = AssociationAc {
        protocol_version: DEFAULT_PROTOCOL_VERSION,
        called_ae_title: rq.called_ae_title.clone(),
        calling_ae_title: rq.calling_ae_title.clone(),
        application_context_name: APPLICATION_CONTEXT_NAME.to_string(),
        presentation_contexts: results,
        user_info: Some(accept_user_info(
            opts.max_pdu,
            &opts.implementation_class_uid,
            &opts.implementation_version_name,
            identity_req.as_ref(),
            &identity_result,
            &accepted_roles,
            &accepted_ext,
            accepted_async,
        )),
    };
    write_observed(ctx, conn, &Pdu::AssociateAc(ac), lifecycle)?;
    let mut assoc = Association::from_parts(Parts {
        conn: conn.clone(),
        lifecycle: lifecycle.clone(),
        idle: opts.idle_timeout,
        read_progress: opts.read_progress_timeout,
        write_progress: opts.write_progress_timeout,
        release: opts.release_timeout,
        artim: opts.artim_timeout,
        max_send_pdu: opts.max_send_pdu,
        max_association_pdu: opts.max_association_pdu,
    });
    assoc.called_ae_title = rq.called_ae_title.clone();
    assoc.calling_ae_title = rq.calling_ae_title.clone();
    assoc.contexts = rq.presentation_contexts.clone();
    assoc.accepted_contexts = accepted;
    assoc.context_outcomes = outcomes;
    assoc.peer_max_pdu = max_pdu_from_user_info(&user_info);
    assoc.max_pdu = opts.max_pdu;
    assoc.protocol_version = DEFAULT_PROTOCOL_VERSION;
    assoc.application_context_name = APPLICATION_CONTEXT_NAME.to_string();
    assoc.requested_role_selections = requested_roles;
    assoc.accepted_role_selections = accepted_roles;
    assoc.requested_extended_negotiation = requested_ext;
    assoc.accepted_extended_negotiation = accepted_ext;
    assoc.requested_common_extended_negotiation = requested_common;
    assoc.requested_asynchronous_operations_window = requested_async;
    assoc.negotiated_asynchronous_operations_window = match accepted_async {
        Some(a) => AsynchronousOperationsWindow {
            maximum_invoked: a.maximum_performed,
            maximum_performed: a.maximum_invoked,
        },
        None => AsynchronousOperationsWindow {
            maximum_invoked: 1,
            maximum_performed: 1,
        },
    };
    assoc.asynchronous_operations_negotiated = accepted_async.is_some();
    assoc.is_association_requestor = false;
    Ok(assoc)
}
