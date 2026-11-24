#![forbid(unsafe_code)]
use super::command::*;
use super::controls::{context_err, data_set_write_context};
use super::error::{Error, Kind, Result};
use super::message::*;
use super::normalized::STATUS_UNRECOGNIZED_OPERATION;
use super::storage_catalog::default_storage_sop_class_uids;
use super::store::{STATUS_C_STORE_DATA_SET_DOES_NOT_MATCH, c_store_context_matches};
use crate::net::ul::{self, Association, CancelToken, Ctx, Nanos, OperationToken, Pdu};
use crate::object::{Element, Object, Syntax, Tag};
use std::collections::{HashMap, HashSet, VecDeque};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};
const DEFAULT_ASYNC_UNLIMITED_LIMIT: usize = 64;
const DEFAULT_ASYNC_RESPONSE_QUEUE: i64 = 32;
const DEFAULT_ASYNC_DATA_SET_BYTES: i64 = 1 << 30;
const DEFAULT_ASYNC_QUEUED_BYTES: i64 = 1 << 30;
const DEFAULT_ASYNC_DATA_SET_ELEMENTS: i64 = 1_000_000;
const DEFAULT_ASYNC_DATA_SET_DEPTH: i64 = 128;
const DEFAULT_ASYNC_CANCEL_DRAIN: Nanos = Nanos(5_000_000_000);
const DEFAULT_ASYNC_PENDING_REQUESTS: i64 = 256;
const MAX_ASYNC_OPERATION_LIMIT: i64 = 65_535;
const MAX_ASYNC_RESPONSE_QUEUE: i64 = 65_535;
const MAX_ASYNC_DATA_SET_DEPTH: i64 = 1_024;
const POLL: Duration = Duration::from_millis(2);
pub const STATUS_DUPLICATE_INVOCATION: u16 = 0x0210;
pub type AsyncRequestHandler =
    Arc<dyn Fn(&Ctx, &AsyncSession, AsyncMessage) -> Result<()> + Send + Sync>;
#[derive(Clone, Default)]
pub struct AsyncSessionOptions {
    pub max_invoked_operations: i64,
    pub max_performed_operations: i64,
    pub response_queue_depth: i64,
    pub max_data_set_bytes: i64,
    pub max_queued_message_bytes: i64,
    pub max_data_set_elements: i64,
    pub max_data_set_depth: i64,
    pub cancel_drain_timeout: Nanos,
    pub max_pending_requests: i64,
    pub c_get_storage_sop_class_uids: Vec<String>,
    pub handlers: HashMap<u16, AsyncRequestHandler>,
}
impl std::fmt::Debug for AsyncSessionOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AsyncSessionOptions")
            .field("max_invoked_operations", &self.max_invoked_operations)
            .field("max_performed_operations", &self.max_performed_operations)
            .field("response_queue_depth", &self.response_queue_depth)
            .field("handlers", &self.handlers.len())
            .finish_non_exhaustive()
    }
}
#[derive(Debug, Clone, Default)]
pub struct AsyncMessage {
    pub presentation_context_id: u8,
    pub command_field: u16,
    pub message_id: u16,
    pub command: Option<Object>,
    pub data_set: Option<Object>,
    message_bytes: i64,
    incoming_generation: u64,
}
#[derive(Debug, Clone, Default)]
pub struct AsyncRequest {
    pub presentation_context_id: u8,
    pub command: Vec<Element>,
    pub data_set: Option<Object>,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AsyncSessionMetrics {
    pub active_invoked: i64,
    pub peak_invoked: i64,
    pub active_performed: i64,
    pub peak_performed: i64,
    pub queued_message_bytes: i64,
    pub peak_queued_message_bytes: i64,
    pub window_violations: u64,
    pub duplicate_invocations: u64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AsyncReleasePolicy {
    Wait,
    RejectIfActive,
    Abort,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct HandlerSession(u64);
#[derive(Clone)]
struct Incoming {
    cancel: CancelToken,
    ctx: Ctx,
    request_field: u16,
    pc_id: u8,
    cancelable: bool,
    finishing: bool,
    write_started: bool,
    write_completed: bool,
    generation: u64,
    response_mu: Arc<Mutex<()>>,
    slot_held: bool,
    message_bytes: i64,
}
#[derive(Default)]
struct State {
    handlers: HashMap<u16, AsyncRequestHandler>,
    operations: HashMap<u16, Arc<OpInner>>,
    incoming: HashMap<u16, Incoming>,
    pending_incoming: i64,
    next_message_id: u16,
    next_incoming_id: u64,
    metrics: AsyncSessionMetrics,
    queued_message_bytes: i64,
    invoked_used: usize,
    performed_used: usize,
    closed: bool,
    releasing: bool,
    release_sent: bool,
    close_err: Option<Error>,
    done: bool,
    release_done: bool,
    release_err: Option<Error>,
}
#[derive(Default)]
struct AbortState {
    started: bool,
    done: bool,
    err: Option<Error>,
}
struct Inner {
    id: u64,
    assoc: Arc<Association>,
    owner: OperationToken,
    ctx: Ctx,
    cancel: CancelToken,
    options: AsyncSessionOptions,
    wire_performed_unlimited: bool,
    c_get_storage: HashSet<String>,
    invoked_limit: usize,
    performed_limit: usize,
    st: Mutex<State>,
    cond: Condvar,
    shutdown_lock: Mutex<bool>,
    abort: Mutex<AbortState>,
    abort_cond: Condvar,
}
#[derive(Clone)]
pub struct AsyncSession {
    inner: Arc<Inner>,
}
impl std::fmt::Debug for AsyncSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AsyncSession")
            .field("id", &self.inner.id)
            .finish_non_exhaustive()
    }
}
struct OpState {
    responses: VecDeque<AsyncMessage>,
    capacity: usize,
    err: Option<Error>,
    finished: bool,
    context_cancel: Option<CancelToken>,
    terminal_seen: bool,
    discarding: bool,
    queued_bytes: i64,
    accounting_transferred: bool,
}
struct OpInner {
    session: AsyncSession,
    message_id: u16,
    pc_id: u8,
    response_field: u16,
    cancelable: bool,
    cancel_mu: Mutex<bool>,
    drain_started: AtomicBool,
    mu: Mutex<OpState>,
    cond: Condvar,
}
#[derive(Clone)]
pub struct AsyncOperation {
    inner: Arc<OpInner>,
}
impl std::fmt::Debug for AsyncOperation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AsyncOperation")
            .field("message_id", &self.inner.message_id)
            .finish_non_exhaustive()
    }
}
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}
fn wait<'a, T>(cond: &Condvar, guard: MutexGuard<'a, T>) -> MutexGuard<'a, T> {
    match cond.wait_timeout(guard, POLL) {
        Ok((g, _)) => g,
        Err(p) => p.into_inner().0,
    }
}
fn ul_wrap(kind: ul::Kind, message: &str) -> Error {
    Error::from(ul::Error::wrap(kind, message))
}
fn eof() -> Error {
    Error::from(ul::Error::eof())
}
fn ctx_or_background(ctx: Option<&Ctx>) -> Ctx {
    ctx.cloned().unwrap_or_default()
}
pub fn new_async_session(
    assoc: Option<Arc<Association>>,
    options: AsyncSessionOptions,
) -> Result<AsyncSession> {
    let Some(assoc) = assoc else {
        return Err(Error::new("dicom dimse: nil association"));
    };
    let Some(owner) = assoc.try_begin_exclusive_operation() else {
        return Err(Error::sentinel(Kind::OperationInProgress));
    };
    let (options, invoked, performed) = match normalize_async_session_options(&assoc, options) {
        Ok(v) => v,
        Err(e) => {
            owner.end(&assoc);
            return Err(e);
        }
    };
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let cancel = CancelToken::new();
    let ctx = assoc.context.with_cancel(&cancel);
    let handlers = options.handlers.clone();
    let c_get_storage = options
        .c_get_storage_sop_class_uids
        .iter()
        .cloned()
        .collect();
    let state = State {
        handlers,
        next_message_id: 1,
        ..Default::default()
    };
    let session = AsyncSession {
        inner: Arc::new(Inner {
            id: NEXT.fetch_add(1, Ordering::SeqCst),
            wire_performed_unlimited: assoc
                .effective_asynchronous_operations_window()
                .maximum_performed
                == 0,
            assoc,
            owner,
            ctx,
            cancel,
            options,
            c_get_storage,
            invoked_limit: invoked,
            performed_limit: performed,
            st: Mutex::new(state),
            cond: Condvar::new(),
            shutdown_lock: Mutex::new(false),
            abort: Mutex::new(AbortState::default()),
            abort_cond: Condvar::new(),
        }),
    };
    let reader = session.clone();
    std::thread::spawn(move || reader.read_loop());
    Ok(session)
}
fn normalize_async_session_options(
    assoc: &Association,
    mut o: AsyncSessionOptions,
) -> Result<(AsyncSessionOptions, usize, usize)> {
    if o.max_invoked_operations < 0
        || o.max_performed_operations < 0
        || o.response_queue_depth < 0
        || o.max_data_set_bytes < 0
        || o.max_queued_message_bytes < 0
        || o.max_data_set_elements < 0
        || o.max_data_set_depth < 0
        || o.cancel_drain_timeout.is_negative()
        || o.max_pending_requests < 0
    {
        return Err(Error::new(
            "dicom dimse: asynchronous session limits must not be negative",
        ));
    }
    if o.max_invoked_operations > MAX_ASYNC_OPERATION_LIMIT
        || o.max_performed_operations > MAX_ASYNC_OPERATION_LIMIT
    {
        return Err(Error::new(format!(
            "dicom dimse: asynchronous local operation limit exceeds {MAX_ASYNC_OPERATION_LIMIT}"
        )));
    }
    if o.response_queue_depth > MAX_ASYNC_RESPONSE_QUEUE {
        return Err(Error::new(format!(
            "dicom dimse: asynchronous response queue exceeds {MAX_ASYNC_RESPONSE_QUEUE}"
        )));
    }
    if o.max_data_set_depth > MAX_ASYNC_DATA_SET_DEPTH {
        return Err(Error::new(format!(
            "dicom dimse: asynchronous dataset depth exceeds {MAX_ASYNC_DATA_SET_DEPTH}"
        )));
    }
    let default = |v: &mut i64, d: i64| {
        if *v == 0 {
            *v = d;
        }
    };
    default(&mut o.response_queue_depth, DEFAULT_ASYNC_RESPONSE_QUEUE);
    default(&mut o.max_data_set_bytes, DEFAULT_ASYNC_DATA_SET_BYTES);
    default(&mut o.max_queued_message_bytes, DEFAULT_ASYNC_QUEUED_BYTES);
    default(
        &mut o.max_data_set_elements,
        DEFAULT_ASYNC_DATA_SET_ELEMENTS,
    );
    default(&mut o.max_data_set_depth, DEFAULT_ASYNC_DATA_SET_DEPTH);
    if o.cancel_drain_timeout.is_zero() {
        o.cancel_drain_timeout = DEFAULT_ASYNC_CANCEL_DRAIN;
    }
    default(&mut o.max_pending_requests, DEFAULT_ASYNC_PENDING_REQUESTS);
    if o.c_get_storage_sop_class_uids.is_empty() {
        o.c_get_storage_sop_class_uids = default_storage_sop_class_uids();
    }
    let window = assoc.effective_asynchronous_operations_window();
    let invoked = effective_async_local_limit(window.maximum_invoked, o.max_invoked_operations);
    let performed = if window.maximum_performed == 0 {
        effective_async_local_limit(0, o.max_performed_operations)
    } else {
        let performed = window.maximum_performed as usize;
        if o.max_performed_operations > 0 && (o.max_performed_operations as usize) < performed {
            return Err(Error::new(format!(
                "dicom dimse: local performed limit {} is below negotiated peer window {performed}",
                o.max_performed_operations
            )));
        }
        performed
    };
    if invoked == 0 || performed == 0 {
        return Err(Error::new(
            "dicom dimse: asynchronous local limits must be positive",
        ));
    }
    Ok((o, invoked, performed))
}
fn effective_async_local_limit(negotiated: u16, configured: i64) -> usize {
    if negotiated == 0 {
        if configured > 0 {
            return configured as usize;
        }
        return DEFAULT_ASYNC_UNLIMITED_LIMIT;
    }
    let mut limit = negotiated as usize;
    if configured > 0 && (configured as usize) < limit {
        limit = configured as usize;
    }
    limit
}
fn command_object(elements: &[Element]) -> Object {
    Object::from_elements(elements.to_vec())
}
fn command_field_from_elements(elements: &[Element]) -> Result<u16> {
    command_uint16(Some(&command_object(elements)), COMMAND_FIELD)
}
fn command_uint16_from_elements(elements: &[Element], tag: Tag) -> Result<u16> {
    command_uint16(Some(&command_object(elements)), tag)
}
fn async_command_allows_pending(field: u16) -> bool {
    matches!(field, C_FIND_RQ | C_MOVE_RQ | C_GET_RQ)
}
fn async_command_status_is_pending(field: u16, status: u16) -> bool {
    match field {
        C_FIND_RQ => status == STATUS_PENDING || status == STATUS_PENDING_WARNING,
        C_MOVE_RQ | C_GET_RQ => status == STATUS_PENDING,
        _ => false,
    }
}
fn async_resource_limit_status(field: u16) -> u16 {
    match field {
        C_STORE_RQ | C_FIND_RQ => 0xA700,
        C_MOVE_RQ | C_GET_RQ => 0xA701,
        _ => STATUS_PROCESSING_FAILURE,
    }
}
fn command_elements_with_message_id(
    elements: &[Element],
    tag: Tag,
    message_id: u16,
) -> Result<Vec<Element>> {
    let mut cloned = elements.to_vec();
    let mut found = false;
    for e in cloned.iter_mut() {
        if e.tag() != tag {
            continue;
        }
        if found {
            return Err(Error::new(format!(
                "dicom dimse: duplicate command element {tag}"
            )));
        }
        *e = us_element(tag, message_id);
        found = true;
    }
    if !found {
        cloned.push(us_element(tag, message_id));
    }
    Ok(cloned)
}
fn response_direction(field: u16) -> bool {
    field & 0x8000 != 0 || field == C_CANCEL_RQ
}
fn async_message_id(command: &Object, field: u16) -> Result<u16> {
    let (wanted, forbidden) = if response_direction(field) {
        (MESSAGE_ID_BEING_RESPONDED_TO, MESSAGE_ID)
    } else {
        (MESSAGE_ID, MESSAGE_ID_BEING_RESPONDED_TO)
    };
    if command.get(forbidden).is_some() {
        return Err(Error::new(format!(
            "dicom dimse: command field 0x{field:04X} contains forbidden correlation element {forbidden}"
        )));
    }
    if command.get(wanted).is_none() {
        return Err(Error::new(format!(
            "dicom dimse: command field 0x{field:04X} missing correlation element {wanted}"
        )));
    }
    command_uint16(Some(command), wanted)
}
fn validate_outgoing_correlation(command: &Object, field: u16) -> Result<()> {
    let forbidden = if response_direction(field) {
        MESSAGE_ID
    } else {
        MESSAGE_ID_BEING_RESPONDED_TO
    };
    if command.get(forbidden).is_some() {
        return Err(Error::new(format!(
            "dicom dimse: command field 0x{field:04X} contains forbidden correlation element {forbidden}"
        )));
    }
    Ok(())
}
fn message_id_or_zero(command: &Object) -> u16 {
    let Ok(field) = command_uint16(Some(command), COMMAND_FIELD) else {
        return 0;
    };
    async_message_id(command, field).unwrap_or(0)
}
fn validate_async_command_presentation_context(
    assoc: &Association,
    pc_id: u8,
    command: &Object,
) -> Result<()> {
    let pc = accepted_context_by_id(assoc, pc_id)?;
    for tag in [AFFECTED_SOP_CLASS_UID, REQUESTED_SOP_CLASS_UID] {
        if command.get(tag).is_none() {
            continue;
        }
        let Some(uid) = command.get_uid(tag).filter(|u| !u.is_empty()) else {
            return Err(Error::new(format!(
                "dicom dimse: invalid command SOP Class UID element {tag}"
            )));
        };
        if uid != pc.abstract_syntax_uid {
            return Err(Error::wrap(
                Kind::PresentationContextMismatch,
                format!(
                    "command SOP Class {} on context {pc_id} for {}",
                    crate::textfmt::quote(&uid),
                    crate::textfmt::quote(&pc.abstract_syntax_uid)
                ),
            ));
        }
    }
    Ok(())
}
fn accepted_role_selection(assoc: &Association, sop: &str) -> Option<ul::RoleSelectionItem> {
    assoc
        .accepted_role_selections
        .iter()
        .find(|r| r.sop_class_uid == sop)
        .cloned()
}
fn automatic_response_command(request: &AsyncMessage, status: u16) -> Vec<Element> {
    let mut command = Vec::new();
    let empty = Object::default();
    let req = request.command.as_ref().unwrap_or(&empty);
    let uid = |tag: Tag| req.get_uid(tag).filter(|u| !u.is_empty());
    if let Some(u) = uid(AFFECTED_SOP_CLASS_UID).or_else(|| uid(REQUESTED_SOP_CLASS_UID)) {
        command.push(ui_element(AFFECTED_SOP_CLASS_UID, &u));
    }
    command.push(us_element(COMMAND_FIELD, request.command_field | 0x8000));
    command.push(us_element(
        MESSAGE_ID_BEING_RESPONDED_TO,
        request.message_id,
    ));
    command.push(us_element(COMMAND_DATA_SET_TYPE, NO_DATA_SET));
    command.push(us_element(STATUS, status));
    if let Some(u) = uid(AFFECTED_SOP_INSTANCE_UID).or_else(|| uid(REQUESTED_SOP_INSTANCE_UID)) {
        command.push(ui_element(AFFECTED_SOP_INSTANCE_UID, &u));
    }
    for tag in [EVENT_TYPE_ID, ACTION_TYPE_ID] {
        if let Ok(v) = command_uint16(Some(req), tag) {
            command.push(us_element(tag, v));
        }
    }
    command
}
fn handler_error(err: Option<Error>) -> Error {
    match err {
        Some(e) => e
            .relabel(Kind::AsyncRequestHandler.text())
            .also(Kind::AsyncRequestHandler),
        None => Error::classified(Kind::AsyncRequestHandler, Kind::AsyncRequestHandler.text()),
    }
}
impl AsyncSession {
    fn st(&self) -> MutexGuard<'_, State> {
        lock(&self.inner.st)
    }
    fn signal(&self) {
        self.inner.cond.notify_all();
    }
    fn assoc(&self) -> &Association {
        &self.inner.assoc
    }
    fn serialize<T>(&self, ctx: &Ctx, write: impl FnOnce() -> Result<T>) -> Result<T> {
        let mut inner: Option<Error> = None;
        let out = self
            .assoc()
            .serialize_message_write(Some(ctx), || match write() {
                Ok(v) => Ok(Some(v)),
                Err(e) => {
                    inner = Some(e);
                    Ok(None)
                }
            });
        match out {
            Ok(Some(v)) => Ok(v),
            Ok(None) => Err(inner.expect("write error recorded")),
            Err(e) => Err(Error::from(e)),
        }
    }
    fn wire_ctx(&self, ctx: &Ctx) -> Ctx {
        self.inner.owner.context(ctx)
    }
    pub fn handle(&self, command_field: u16, handler: Option<AsyncRequestHandler>) {
        let mut st = self.st();
        match handler {
            None => {
                st.handlers.remove(&command_field);
            }
            Some(h) => {
                st.handlers.insert(command_field, h);
            }
        }
    }
    pub fn snapshot(&self) -> AsyncSessionMetrics {
        self.st().metrics
    }
    pub fn err(&self) -> Option<Error> {
        self.st().close_err.clone()
    }
    pub fn is_done(&self) -> bool {
        self.st().done
    }
    pub fn wait_done(&self, timeout: Duration) -> bool {
        let end = Instant::now() + timeout;
        let mut st = self.st();
        while !st.done {
            if Instant::now() >= end {
                return false;
            }
            st = wait(&self.inner.cond, st);
        }
        true
    }
    fn session_error(&self) -> Error {
        self.st()
            .close_err
            .clone()
            .unwrap_or_else(|| Error::sentinel(Kind::AsyncSessionClosed))
    }
    fn read_loop(self) {
        loop {
            let ctx = self.wire_ctx(&self.inner.ctx);
            let received = receive_dimse_command_or_control(&ctx, self.assoc());
            let (pc_id, command, command_bytes) = match received {
                Err(e) => {
                    self.shutdown(e);
                    return;
                }
                Ok(CommandOrControl::Command(pc, c, n)) => (pc, c, n),
                Ok(CommandOrControl::Control(pdu)) => {
                    self.control(pdu);
                    return;
                }
            };
            let field = match command_uint16(Some(&command), COMMAND_FIELD) {
                Ok(f) => f,
                Err(e) => {
                    self.shutdown(e);
                    return;
                }
            };
            let request_direction = field & 0x8000 == 0 && field != C_CANCEL_RQ;
            let (mut prepared, mut automatic, mut generation) = (false, 0u16, 0u64);
            if request_direction {
                match self.prepare_received_request(pc_id, &command) {
                    Ok(v) => (prepared, automatic, generation) = v,
                    Err(e) => {
                        self.shutdown(e);
                        return;
                    }
                }
            }
            let mid = message_id_or_zero(&command);
            let mut message = match self.assemble_async_message(pc_id, command, command_bytes) {
                Ok(m) => m,
                Err(e) => {
                    if prepared {
                        self.finish_incoming_operation(mid, field, generation);
                    }
                    self.shutdown(e);
                    return;
                }
            };
            message.incoming_generation = generation;
            if prepared
                && !self.attach_incoming_message_bytes(
                    message.message_id,
                    message.command_field,
                    generation,
                    message.message_bytes,
                )
            {
                self.release_message_bytes(message.message_bytes);
                self.shutdown(Error::sentinel(Kind::AsyncOperationComplete));
                return;
            }
            let bytes = message.message_bytes;
            let r = if request_direction {
                if automatic != 0 {
                    let r = self.send_automatic_response(&message, automatic);
                    self.release_message_bytes(bytes);
                    r
                } else {
                    self.dispatch_prepared_request(message)
                }
            } else {
                self.route_message(message)
            };
            if let Err(e) = r {
                if !prepared && automatic == 0 {
                    self.release_message_bytes(bytes);
                }
                self.shutdown(e);
                return;
            }
            prepared = false;
            automatic = 0;
            generation = 0;
            let _ = (prepared, automatic, generation);
        }
    }
    fn control(&self, pdu: Pdu) {
        match pdu {
            Pdu::ReleaseRq => {
                if self.assoc().is_association_requestor {
                    self.shutdown(ul_wrap(
                        ul::Kind::UnexpectedPdu,
                        "association acceptor attempted A-RELEASE-RQ",
                    ));
                    return;
                }
                {
                    let mut st = self.st();
                    st.releasing = true;
                }
                self.signal();
                if self.wait_for_peer_invocations_confirmed().is_err() {
                    self.shutdown(ul_wrap(
                        ul::Kind::UnexpectedPdu,
                        "A-RELEASE-RQ received with outstanding DIMSE operations",
                    ));
                    return;
                }
                let wire = self.wire_ctx(&self.inner.ctx);
                let err = match self.serialize(&wire, || {
                    self.assoc()
                        .send(Some(&wire), &Pdu::ReleaseRp)
                        .map_err(Error::from)
                }) {
                    Ok(()) => Error::sentinel(Kind::AssociationReleased),
                    Err(e) => e,
                };
                self.shutdown(err);
            }
            Pdu::ReleaseRp => {
                let expected = {
                    let st = self.st();
                    st.release_sent && self.assoc().is_association_requestor
                };
                if !expected {
                    self.shutdown(ul_wrap(ul::Kind::UnexpectedPdu, "unsolicited A-RELEASE-RP"));
                    return;
                }
                self.shutdown_with_release(Error::sentinel(Kind::AssociationReleased), None);
            }
            Pdu::Abort(a) => {
                self.shutdown(Error::from(ul::Error::abort(a.source, a.reason)));
            }
            other => {
                self.shutdown(Error::from(ul::unexpected_pdu_error(
                    &other,
                    "at a DIMSE message boundary",
                )));
            }
        }
    }
    fn wait_for_peer_invocations_confirmed(&self) -> Result<()> {
        let mut st = self.st();
        loop {
            let active = st.incoming.values().any(|i| !i.finishing);
            if active {
                return Err(Error::sentinel(Kind::AsyncOperationsActive));
            }
            if st.incoming.is_empty() {
                return Ok(());
            }
            if st.done {
                drop(st);
                return Err(self.session_error());
            }
            st = wait(&self.inner.cond, st);
        }
    }
    fn assemble_async_message(
        &self,
        pc_id: u8,
        command: Object,
        command_bytes: i64,
    ) -> Result<AsyncMessage> {
        if command_bytes <= 0 {
            return Err(Error::new(format!(
                "dicom dimse: invalid assembled command size {command_bytes}"
            )));
        }
        self.reserve_message_bytes(command_bytes)?;
        let mut message = AsyncMessage {
            message_bytes: command_bytes,
            ..Default::default()
        };
        let r = self.assemble_into(&mut message, pc_id, command);
        if let Err(e) = r {
            self.release_message_bytes(message.message_bytes);
            return Err(e);
        }
        Ok(message)
    }
    fn assemble_into(&self, message: &mut AsyncMessage, pc_id: u8, command: Object) -> Result<()> {
        let field = command_uint16(Some(&command), COMMAND_FIELD)?;
        let message_id = async_message_id(&command, field)?;
        let data_set_type = command_uint16(Some(&command), COMMAND_DATA_SET_TYPE)?;
        message.presentation_context_id = pc_id;
        message.command_field = field;
        message.message_id = message_id;
        validate_async_command_presentation_context(self.assoc(), pc_id, &command)?;
        message.command = Some(command);
        let ctx = self.wire_ctx(&self.inner.ctx);
        if data_set_type != NO_DATA_SET {
            let syntax = accepted_transfer_syntax(self.assoc(), pc_id)?;
            let max_bytes = self.available_data_set_bytes();
            if max_bytes <= 0 {
                return Err(Error::sentinel(Kind::AsyncResourceLimit));
            }
            let (data_set, size) = receive_data_set_limits(
                Some(&ctx),
                self.assoc(),
                pc_id,
                syntax,
                max_bytes,
                self.inner.options.max_data_set_elements,
                self.inner.options.max_data_set_depth,
            )?;
            message.data_set = Some(data_set);
            self.reserve_message_bytes(size)?;
            message.message_bytes += size;
        }
        let trailing = self.assoc().take_pdata_carryover(Some(&ctx))?;
        if !trailing.is_empty() {
            return Err(ul_wrap(
                ul::Kind::UnexpectedPdu,
                "multiple DIMSE messages in one P-DATA-TF",
            ));
        }
        Ok(())
    }
    fn available_data_set_bytes(&self) -> i64 {
        let st = self.st();
        let remaining = self.inner.options.max_queued_message_bytes - st.queued_message_bytes;
        remaining.min(self.inner.options.max_data_set_bytes)
    }
    fn reserve_message_bytes(&self, size: i64) -> Result<()> {
        if size <= 0 {
            return Ok(());
        }
        let mut st = self.st();
        if size > self.inner.options.max_queued_message_bytes - st.queued_message_bytes {
            return Err(Error::sentinel(Kind::AsyncResourceLimit));
        }
        st.queued_message_bytes += size;
        st.metrics.queued_message_bytes = st.queued_message_bytes;
        if st.queued_message_bytes > st.metrics.peak_queued_message_bytes {
            st.metrics.peak_queued_message_bytes = st.queued_message_bytes;
        }
        Ok(())
    }
    fn release_message_bytes(&self, size: i64) {
        if size <= 0 {
            return;
        }
        let mut st = self.st();
        st.queued_message_bytes = (st.queued_message_bytes - size).max(0);
        st.metrics.queued_message_bytes = st.queued_message_bytes;
    }
    fn route_message(&self, message: AsyncMessage) -> Result<()> {
        if message.command_field == C_CANCEL_RQ {
            if message.data_set.is_some() {
                return Err(Error::wrap(
                    Kind::UnexpectedCommand,
                    "C-CANCEL-RQ carries a dataset",
                ));
            }
            let incoming = self.st().incoming.get(&message.message_id).cloned();
            if let Some(i) = incoming
                && i.pc_id == message.presentation_context_id
                && i.cancelable
            {
                i.cancel.cancel();
            }
            self.release_message_bytes(message.message_bytes);
            return Ok(());
        }
        if message.command_field & 0x8000 != 0 {
            return self.route_response(message);
        }
        Err(Error::wrap(
            Kind::UnexpectedCommand,
            format!("unprepared request field 0x{:04X}", message.command_field),
        ))
    }
    fn route_response(&self, message: AsyncMessage) -> Result<()> {
        let operation = self.st().operations.get(&message.message_id).cloned();
        let Some(op) = operation else {
            return Err(super::scp::unexpected_command_error(
                message.command_field,
                message.message_id,
            ));
        };
        if message.command_field != op.response_field {
            return Err(Error::wrap(
                Kind::UnexpectedCommand,
                format!(
                    "response field 0x{:04X} for request expecting 0x{:04X}",
                    message.command_field, op.response_field
                ),
            ));
        }
        if message.presentation_context_id != op.pc_id {
            return Err(Error::wrap(
                Kind::PresentationContextMismatch,
                format!(
                    "response presentation context {} for request on context {}",
                    message.presentation_context_id, op.pc_id
                ),
            ));
        }
        let status = command_uint16(message.command.as_ref(), STATUS)?;
        let request_field = op.response_field & !0x8000;
        let pending = async_command_status_is_pending(request_field, status);
        if (status == STATUS_PENDING || status == STATUS_PENDING_WARNING) && !pending {
            return Err(Error::wrap(
                Kind::UnexpectedCommand,
                format!(
                    "pending status 0x{status:04X} for command field 0x{:04X}",
                    op.response_field
                ),
            ));
        }
        let terminal = !pending;
        let message_id = message.message_id;
        let bytes = message.message_bytes;
        let cancel_guard = terminal.then(|| lock(&op.cancel_mu));
        let (discarding, newly_discarding, queued) = {
            let mut o = lock(&op.mu);
            let mut discarding = o.discarding;
            let mut newly = false;
            let mut queued = false;
            if !discarding {
                if o.responses.len() < o.capacity {
                    o.responses.push_back(message);
                    o.queued_bytes += bytes;
                    queued = true;
                } else {
                    o.discarding = true;
                    discarding = true;
                    newly = true;
                }
            }
            if terminal {
                o.terminal_seen = true;
            }
            (discarding, newly, queued)
        };
        op.cond.notify_all();
        drop(cancel_guard);
        if newly_discarding && !terminal {
            let session = self.clone();
            let operation = AsyncOperation { inner: op.clone() };
            std::thread::spawn(move || {
                let ctx = Ctx::background().with_timeout(Duration::from_secs(1));
                if let Err(e) = operation.cancel(Some(&ctx))
                    && !e.is(Kind::AsyncOperationComplete)
                    && !e.is(Kind::AsyncSessionClosed)
                {
                    session.shutdown(e);
                }
            });
        }
        if discarding || !queued {
            self.release_message_bytes(bytes);
        }
        if terminal {
            if discarding || !queued {
                self.finish_operation(
                    message_id,
                    Some(Error::sentinel(Kind::AsyncResponseQueueFull)),
                );
            } else {
                self.finish_operation(message_id, None);
            }
        }
        Ok(())
    }
    fn attach_incoming_message_bytes(
        &self,
        message_id: u16,
        request_field: u16,
        generation: u64,
        size: i64,
    ) -> bool {
        if size <= 0 {
            return true;
        }
        let mut st = self.st();
        match st.incoming.get_mut(&message_id) {
            Some(c) if c.request_field == request_field && c.generation == generation => {
                c.message_bytes += size;
                true
            }
            _ => false,
        }
    }
    fn prepare_incoming_request(&self, pc_id: u8, command: &Object) -> Result<(bool, u16, u64)> {
        let field = command_uint16(Some(command), COMMAND_FIELD)?;
        let message_id = async_message_id(command, field)?;
        validate_async_command_presentation_context(self.assoc(), pc_id, command)?;
        if !self.local_may_perform(pc_id) {
            return Err(Error::sentinel(Kind::AsyncRoleNotAccepted));
        }
        'retry: loop {
            let mut st = self.st();
            if st.release_sent || st.closed {
                return Err(Error::sentinel(Kind::AsyncSessionReleasing));
            }
            let duplicate = st.incoming.get(&message_id).cloned();
            if let Some(current) = &duplicate {
                if current.finishing && current.write_completed {
                    if self.wait_changed(st) {
                        continue 'retry;
                    }
                    return Err(self.session_error());
                }
                st.metrics.duplicate_invocations += 1;
            }
            if duplicate.is_none()
                && self.inner.wire_performed_unlimited
                && st.incoming.len() as i64 >= self.inner.options.max_pending_requests
            {
                return Ok((false, async_resource_limit_status(field), 0));
            }
            drop(st);
            if duplicate.is_some() {
                return Ok((false, STATUS_DUPLICATE_INVOCATION, 0));
            }
            let mut slot_held = false;
            if !self.inner.wire_performed_unlimited {
                if !self.acquire_performed_slot_for_request() {
                    let mut st = self.st();
                    if st.release_sent || st.closed {
                        return Err(Error::sentinel(Kind::AsyncSessionReleasing));
                    }
                    st.metrics.window_violations += 1;
                    return Err(Error::sentinel(Kind::AsyncWindowExceeded));
                }
                slot_held = true;
            }
            let mut st = self.st();
            if st.release_sent || st.closed {
                if slot_held {
                    st.performed_used -= 1;
                }
                return Err(Error::sentinel(Kind::AsyncSessionReleasing));
            }
            if let Some(current) = st.incoming.get(&message_id) {
                let finishing = current.finishing && current.write_completed;
                if !finishing {
                    st.metrics.duplicate_invocations += 1;
                }
                if slot_held {
                    st.performed_used -= 1;
                }
                if finishing {
                    if self.wait_changed(st) {
                        continue 'retry;
                    }
                    return Err(self.session_error());
                }
                return Ok((false, STATUS_DUPLICATE_INVOCATION, 0));
            }
            st.next_incoming_id = st.next_incoming_id.wrapping_add(1);
            if st.next_incoming_id == 0 {
                st.next_incoming_id = 1;
            }
            let generation = st.next_incoming_id;
            let cancel = CancelToken::new();
            let ctx = self
                .inner
                .ctx
                .with_value(HandlerSession(self.inner.id))
                .with_cancel(&cancel);
            st.incoming.insert(
                message_id,
                Incoming {
                    cancel,
                    ctx,
                    request_field: field,
                    pc_id,
                    cancelable: async_command_allows_pending(field),
                    finishing: false,
                    write_started: false,
                    write_completed: false,
                    generation,
                    response_mu: Arc::new(Mutex::new(())),
                    slot_held,
                    message_bytes: 0,
                },
            );
            st.metrics.active_performed += 1;
            if st.metrics.active_performed > st.metrics.peak_performed {
                st.metrics.peak_performed = st.metrics.active_performed;
            }
            drop(st);
            self.signal();
            return Ok((true, 0, generation));
        }
    }
    fn wait_changed(&self, st: MutexGuard<'_, State>) -> bool {
        if st.done {
            return false;
        }
        let st = wait(&self.inner.cond, st);
        !st.done
    }
    fn prepare_received_request(&self, pc_id: u8, command: &Object) -> Result<(bool, u16, u64)> {
        self.st().pending_incoming += 1;
        self.signal();
        let r = self.prepare_incoming_request(pc_id, command);
        {
            let mut st = self.st();
            if st.pending_incoming > 0 {
                st.pending_incoming -= 1;
            }
        }
        self.signal();
        r
    }
    fn dispatch_prepared_request(&self, message: AsyncMessage) -> Result<()> {
        let (incoming, handler) = {
            let st = self.st();
            (
                st.incoming.get(&message.message_id).cloned(),
                st.handlers.get(&message.command_field).cloned(),
            )
        };
        let Some(incoming) = incoming.filter(|i| {
            i.request_field == message.command_field
                && i.pc_id == message.presentation_context_id
                && i.generation == message.incoming_generation
        }) else {
            return Err(Error::sentinel(Kind::AsyncOperationComplete));
        };
        if message.command_field == C_STORE_RQ {
            let pc = accepted_context_by_id(self.assoc(), message.presentation_context_id)?;
            let class = message
                .command
                .as_ref()
                .and_then(|c| c.get_uid(AFFECTED_SOP_CLASS_UID))
                .unwrap_or_default();
            if !c_store_context_matches(&class, &pc) {
                return self.respond_automatically_to_incoming(
                    &message,
                    STATUS_C_STORE_DATA_SET_DOES_NOT_MATCH,
                );
            }
        }
        let session = self.clone();
        std::thread::spawn(move || {
            session.run_incoming(message.clone(), incoming.clone(), handler);
            incoming.cancel.cancel();
            session.finish_incoming_operation(
                message.message_id,
                message.command_field,
                message.incoming_generation,
            );
        });
        Ok(())
    }
    fn run_incoming(
        &self,
        message: AsyncMessage,
        incoming: Incoming,
        handler: Option<AsyncRequestHandler>,
    ) {
        if !incoming.slot_held {
            let mut st = self.st();
            loop {
                if st.performed_used < self.inner.performed_limit {
                    st.performed_used += 1;
                    let same = match st.incoming.get_mut(&message.message_id) {
                        Some(c)
                            if c.request_field == message.command_field
                                && c.generation == message.incoming_generation =>
                        {
                            c.slot_held = true;
                            true
                        }
                        _ => {
                            st.performed_used -= 1;
                            false
                        }
                    };
                    if !same {
                        return;
                    }
                    break;
                }
                if st.done {
                    return;
                }
                st = wait(&self.inner.cond, st);
            }
        }
        let taken = self.take_incoming_message_bytes(
            message.message_id,
            message.command_field,
            message.incoming_generation,
        );
        self.release_message_bytes(taken);
        let Some(handler) = handler else {
            if let Err(e) =
                self.respond_automatically_to_incoming(&message, STATUS_UNRECOGNIZED_OPERATION)
            {
                self.shutdown(e);
            }
            return;
        };
        let generation = message.incoming_generation;
        let message_id = message.message_id;
        let outcome = catch_unwind(AssertUnwindSafe(|| handler(&incoming.ctx, self, message)));
        let err = match outcome {
            Ok(Ok(())) => None,
            Ok(Err(e)) => Some(handler_error(Some(e))),
            Err(_) => Some(handler_error(None)),
        };
        if let Some(e) = err {
            self.shutdown(e);
            return;
        }
        let active = self
            .st()
            .incoming
            .get(&message_id)
            .is_some_and(|c| c.generation == generation);
        if active {
            self.shutdown(Error::sentinel(Kind::AsyncTerminalResponseMissing));
        }
    }
    fn take_incoming_message_bytes(&self, message_id: u16, field: u16, generation: u64) -> i64 {
        let mut st = self.st();
        match st.incoming.get_mut(&message_id) {
            Some(c) if c.request_field == field && c.generation == generation => {
                std::mem::take(&mut c.message_bytes)
            }
            _ => 0,
        }
    }
    fn local_may_invoke(&self, pc_id: u8) -> bool {
        let Ok(pc) = accepted_context_by_id(self.assoc(), pc_id) else {
            return false;
        };
        let role = accepted_role_selection(self.assoc(), &pc.abstract_syntax_uid);
        if self.assoc().is_association_requestor {
            return role.is_none_or(|r| r.scu_role);
        }
        role.is_some_and(|r| r.scp_role)
    }
    fn local_may_perform(&self, pc_id: u8) -> bool {
        let Ok(pc) = accepted_context_by_id(self.assoc(), pc_id) else {
            return false;
        };
        let role = accepted_role_selection(self.assoc(), &pc.abstract_syntax_uid);
        if self.assoc().is_association_requestor {
            return role.is_some_and(|r| r.scp_role);
        }
        role.is_none_or(|r| r.scu_role)
    }
    fn local_has_reverse_store_role(&self, query_sop_class_uid: &str) -> bool {
        self.assoc().accepted_contexts.iter().any(|pc| {
            pc.abstract_syntax_uid != query_sop_class_uid
                && self.inner.c_get_storage.contains(&pc.abstract_syntax_uid)
                && self.local_may_perform(pc.id)
        })
    }
    fn acquire_performed_slot_for_request(&self) -> bool {
        let mut st = self.st();
        loop {
            if st.performed_used < self.inner.performed_limit {
                st.performed_used += 1;
                return true;
            }
            let finishing = st.incoming.values().any(|i| i.finishing && i.write_started);
            if !finishing || st.done {
                return false;
            }
            st = wait(&self.inner.cond, st);
        }
    }
    fn send_automatic_response(&self, request: &AsyncMessage, status: u16) -> Result<()> {
        let ctx = self.inner.ctx.clone();
        self.send_message(
            Some(&ctx),
            request.presentation_context_id,
            &automatic_response_command(request, status),
            None,
        )
    }
    fn respond_automatically_to_incoming(&self, request: &AsyncMessage, status: u16) -> Result<()> {
        let ctx = self.inner.ctx.clone();
        self.respond(
            Some(&ctx),
            request,
            &automatic_response_command(request, status),
            None,
        )
    }
    fn mark_incoming(&self, message_id: u16, field: u16, generation: u64, completed: bool) {
        let mut st = self.st();
        if let Some(c) = st.incoming.get_mut(&message_id)
            && c.request_field == field
            && c.generation == generation
            && c.finishing
        {
            if completed {
                c.write_completed = true;
            } else {
                c.write_started = true;
            }
            drop(st);
            self.signal();
        }
    }
    fn finish_incoming_operation(&self, message_id: u16, field: u16, generation: u64) -> bool {
        let mut st = self.st();
        let Some(current) = st
            .incoming
            .get(&message_id)
            .filter(|c| c.request_field == field && c.generation == generation)
            .cloned()
        else {
            return false;
        };
        st.incoming.remove(&message_id);
        st.metrics.active_performed -= 1;
        if current.slot_held {
            st.performed_used -= 1;
        }
        drop(st);
        self.signal();
        self.release_message_bytes(current.message_bytes);
        true
    }
    pub fn invoke(&self, ctx: Option<&Ctx>, request: AsyncRequest) -> Result<AsyncOperation> {
        self.invoke_with(ctx, request, None)
    }
    fn invoke_with(
        &self,
        ctx: Option<&Ctx>,
        request: AsyncRequest,
        encode: Option<&DataSetStreamFn<'_>>,
    ) -> Result<AsyncOperation> {
        let ctx = ctx_or_background(ctx);
        if request.data_set.is_some() && encode.is_some() {
            return Err(Error::new(
                "dicom dimse: C-STORE dataset and encoded writer are mutually exclusive",
            ));
        }
        let field = command_field_from_elements(&request.command)?;
        if field & 0x8000 != 0 || field == C_CANCEL_RQ {
            return Err(Error::new(format!(
                "dicom dimse: command field 0x{field:04X} is not an invocable request"
            )));
        }
        let has_data_set = request.data_set.is_some() || encode.is_some();
        self.validate_outgoing_command(
            request.presentation_context_id,
            &request.command,
            has_data_set,
        )?;
        if !self.local_may_invoke(request.presentation_context_id) {
            return Err(Error::sentinel(Kind::AsyncRoleNotAccepted));
        }
        self.acquire_invoked_slot(&ctx)?;
        let op = {
            let mut st = self.st();
            if st.closed || st.releasing {
                let releasing = st.releasing;
                st.invoked_used -= 1;
                drop(st);
                if releasing {
                    return Err(Error::sentinel(Kind::AsyncSessionReleasing));
                }
                return Err(self.session_error());
            }
            let message_id = match allocate_message_id(&mut st) {
                Ok(id) => id,
                Err(e) => {
                    st.invoked_used -= 1;
                    return Err(e);
                }
            };
            let op = Arc::new(OpInner {
                session: self.clone(),
                message_id,
                pc_id: request.presentation_context_id,
                response_field: field | 0x8000,
                cancelable: async_command_allows_pending(field),
                cancel_mu: Mutex::new(false),
                drain_started: AtomicBool::new(false),
                mu: Mutex::new(OpState {
                    responses: VecDeque::new(),
                    capacity: self.inner.options.response_queue_depth as usize,
                    err: None,
                    finished: false,
                    context_cancel: None,
                    terminal_seen: false,
                    discarding: false,
                    queued_bytes: 0,
                    accounting_transferred: false,
                }),
                cond: Condvar::new(),
            });
            st.operations.insert(message_id, op.clone());
            st.metrics.active_invoked += 1;
            if st.metrics.active_invoked > st.metrics.peak_invoked {
                st.metrics.peak_invoked = st.metrics.active_invoked;
            }
            op
        };
        self.signal();
        let message_id = op.message_id;
        let command = command_elements_with_message_id(&request.command, MESSAGE_ID, message_id);
        let sent = match &command {
            Err(e) => Err(e.clone()),
            Ok(command) => match encode {
                Some(encode) => self.send_encoded_message(
                    &ctx,
                    request.presentation_context_id,
                    command,
                    encode,
                ),
                None => self.send_message(
                    Some(&ctx),
                    request.presentation_context_id,
                    command,
                    request.data_set.as_ref(),
                ),
            },
        };
        if let Err(e) = sent {
            if command.is_err() || e.is_ul(ul::Kind::MessageWriteNotStarted) {
                self.finish_operation(message_id, Some(e.clone()));
            } else {
                self.shutdown(e.clone());
            }
            return Err(e);
        }
        let operation = AsyncOperation { inner: op };
        if ctx.can_end() {
            let session = self.clone();
            let watched = operation.clone();
            std::thread::spawn(move || session.watch_operation_context(ctx, watched));
        }
        Ok(operation)
    }
    fn acquire_invoked_slot(&self, ctx: &Ctx) -> Result<()> {
        let mut st = self.st();
        loop {
            if st.closed || st.releasing {
                let releasing = st.releasing;
                drop(st);
                if releasing {
                    return Err(Error::sentinel(Kind::AsyncSessionReleasing));
                }
                return Err(self.session_error());
            }
            if st.invoked_used < self.inner.invoked_limit {
                st.invoked_used += 1;
                return Ok(());
            }
            if let Some(e) = context_err(ctx) {
                return Err(e);
            }
            if st.done {
                drop(st);
                return Err(self.session_error());
            }
            st = wait(&self.inner.cond, st);
        }
    }
    fn finish_operation(&self, message_id: u16, err: Option<Error>) {
        let Some(op) = self.st().operations.get(&message_id).cloned() else {
            return;
        };
        let _cancel = lock(&op.cancel_mu);
        {
            let mut st = self.st();
            match st.operations.get(&message_id) {
                Some(current) if Arc::ptr_eq(current, &op) => {}
                _ => return,
            }
            st.operations.remove(&message_id);
            st.metrics.active_invoked -= 1;
            st.invoked_used -= 1;
        }
        self.signal();
        op.finish(err);
    }
    pub fn start_c_echo(&self, ctx: Option<&Ctx>) -> Result<AsyncOperation> {
        let Some(pc) = accepted_verification_context(self.assoc()) else {
            return Err(Error::new(
                "dicom dimse: verification presentation context not accepted",
            ));
        };
        self.invoke(
            ctx,
            AsyncRequest {
                presentation_context_id: pc.id,
                command: CEchoRequest::default().command_set(),
                data_set: None,
            },
        )
    }
    pub fn start_c_find(
        &self,
        ctx: Option<&Ctx>,
        pc_id: u8,
        request: &CFindRequest,
        identifier: Option<Object>,
    ) -> Result<AsyncOperation> {
        self.invoke(ctx, req(pc_id, request.command_set(), identifier))
    }
    pub fn start_c_move(
        &self,
        ctx: Option<&Ctx>,
        pc_id: u8,
        request: &CMoveRequest,
        identifier: Option<Object>,
    ) -> Result<AsyncOperation> {
        self.invoke(ctx, req(pc_id, request.command_set(), identifier))
    }
    pub fn start_c_get(
        &self,
        ctx: Option<&Ctx>,
        pc_id: u8,
        request: &CGetRequest,
        identifier: Option<Object>,
    ) -> Result<AsyncOperation> {
        if !self.local_has_reverse_store_role(&request.affected_sop_class_uid) {
            return Err(Error::sentinel(Kind::CGetStorageRoleNotAccepted));
        }
        if !self.st().handlers.contains_key(&C_STORE_RQ) {
            return Err(Error::new(
                "dicom dimse: C-GET requires a reverse C-STORE handler",
            ));
        }
        self.invoke(ctx, req(pc_id, request.command_set(), identifier))
    }
    pub fn start_c_store(
        &self,
        ctx: Option<&Ctx>,
        pc_id: u8,
        request: &CStoreRequest,
        data_set: Option<Object>,
    ) -> Result<AsyncOperation> {
        let ctx = self.suboperation_context(ctx);
        self.invoke(ctx.as_ref(), req(pc_id, request.command_set(), data_set))
    }
    pub fn start_c_store_encoded(
        &self,
        ctx: Option<&Ctx>,
        pc_id: u8,
        request: &CStoreRequest,
        encode: Option<&DataSetEncoderFn<'_>>,
    ) -> Result<AsyncOperation> {
        let Some(encode) = encode else {
            return Err(Error::new("dicom dimse: C-STORE dataset writer is nil"));
        };
        self.start_c_store_streamed(
            ctx,
            pc_id,
            request,
            Some(
                &|write_ctx: &Ctx, sink: &mut dyn FnMut(&[u8]) -> Result<()>, syntax: &Syntax| {
                    let bytes = encode(write_ctx, syntax)?;
                    sink(&bytes)
                },
            ),
        )
    }
    pub(crate) fn start_c_store_streamed(
        &self,
        ctx: Option<&Ctx>,
        pc_id: u8,
        request: &CStoreRequest,
        write: Option<&DataSetStreamFn<'_>>,
    ) -> Result<AsyncOperation> {
        let Some(write) = write else {
            return Err(Error::new("dicom dimse: C-STORE dataset writer is nil"));
        };
        let ctx = self.suboperation_context(ctx);
        self.invoke_with(
            ctx.as_ref(),
            req(pc_id, request.command_set(), None),
            Some(write),
        )
    }
    pub fn start_normalized(
        &self,
        ctx: Option<&Ctx>,
        pc_id: u8,
        command: Vec<Element>,
        data_set: Option<Object>,
    ) -> Result<AsyncOperation> {
        self.invoke(ctx, req(pc_id, command, data_set))
    }
    pub fn respond(
        &self,
        ctx: Option<&Ctx>,
        request: &AsyncMessage,
        command: &[Element],
        data_set: Option<&Object>,
    ) -> Result<()> {
        let field = command_field_from_elements(command)?;
        if field & 0x8000 == 0 {
            return Err(Error::new(format!(
                "dicom dimse: command field 0x{field:04X} is not a response"
            )));
        }
        if field != request.command_field | 0x8000 {
            return Err(Error::new(format!(
                "dicom dimse: response field 0x{field:04X} does not match request field 0x{:04X}",
                request.command_field
            )));
        }
        let status = command_uint16_from_elements(command, STATUS)?;
        let matches = |i: &Incoming| {
            i.request_field == request.command_field
                && i.pc_id == request.presentation_context_id
                && i.generation == request.incoming_generation
        };
        let Some(incoming) = self
            .st()
            .incoming
            .get(&request.message_id)
            .filter(|i| matches(i))
            .cloned()
        else {
            return Err(Error::sentinel(Kind::AsyncOperationComplete));
        };
        let _response = lock(&incoming.response_mu);
        let active = self
            .st()
            .incoming
            .get(&request.message_id)
            .is_some_and(|i| matches(i) && !i.finishing);
        if !active {
            return Err(Error::sentinel(Kind::AsyncOperationComplete));
        }
        let command = command_elements_with_message_id(
            command,
            MESSAGE_ID_BEING_RESPONDED_TO,
            request.message_id,
        )?;
        let pending = async_command_status_is_pending(request.command_field, status);
        if (status == STATUS_PENDING || status == STATUS_PENDING_WARNING) && !pending {
            return Err(Error::new(format!(
                "dicom dimse: pending status 0x{status:04X} is invalid for command field 0x{:04X}",
                request.command_field
            )));
        }
        let terminal = !pending;
        let ctx = self.response_context(ctx);
        let syntax = self.validate_outgoing_command(
            request.presentation_context_id,
            &command,
            data_set.is_some(),
        )?;
        let (mid, rf, generation) = (
            request.message_id,
            request.command_field,
            request.incoming_generation,
        );
        if terminal {
            let mut st = self.st();
            match st.incoming.get_mut(&mid) {
                Some(c) if c.request_field == rf && c.generation == generation && !c.finishing => {
                    c.finishing = true;
                }
                _ => return Err(Error::sentinel(Kind::AsyncOperationComplete)),
            }
            drop(st);
            self.signal();
        }
        let r = self.write_validated_message(
            &ctx,
            request.presentation_context_id,
            &command,
            data_set,
            &syntax,
            || {
                if terminal {
                    self.mark_incoming(mid, rf, generation, false);
                }
            },
            || {
                if terminal {
                    self.mark_incoming(mid, rf, generation, true);
                    self.finish_incoming_operation(mid, rf, generation);
                }
            },
        );
        if let Err(e) = r {
            if terminal && e.is_ul(ul::Kind::MessageWriteNotStarted) {
                let mut st = self.st();
                if let Some(c) = st.incoming.get_mut(&mid)
                    && c.request_field == rf
                    && c.generation == generation
                {
                    c.finishing = false;
                    c.write_started = false;
                    c.write_completed = false;
                    drop(st);
                    self.signal();
                }
                return Err(e);
            }
            self.shutdown(e.clone());
            return Err(e);
        }
        Ok(())
    }
    fn send_message(
        &self,
        ctx: Option<&Ctx>,
        pc_id: u8,
        command: &[Element],
        data_set: Option<&Object>,
    ) -> Result<()> {
        let ctx = ctx_or_background(ctx);
        let syntax = self.validate_outgoing_command(pc_id, command, data_set.is_some())?;
        self.write_validated_message(&ctx, pc_id, command, data_set, &syntax, || {}, || {})
    }
    fn send_encoded_message(
        &self,
        ctx: &Ctx,
        pc_id: u8,
        command: &[Element],
        encode: &DataSetStreamFn<'_>,
    ) -> Result<()> {
        let syntax = self.validate_outgoing_command(pc_id, command, true)?;
        let ctx = self.wire_ctx(ctx);
        self.serialize(&ctx, || {
            let write_ctx = data_set_write_context(Some(&ctx)).unwrap_or_else(|| ctx.clone());
            send_message(Some(&ctx), self.assoc(), pc_id, command, &syntax, |w| {
                encode(
                    &write_ctx,
                    &mut |chunk: &[u8]| w.write(chunk).map(|_| ()),
                    &syntax,
                )
            })
        })
    }
    fn validate_outgoing_command(
        &self,
        pc_id: u8,
        command: &[Element],
        has_data_set: bool,
    ) -> Result<Syntax> {
        let field = command_field_from_elements(command)?;
        let object = command_object(command);
        validate_outgoing_correlation(&object, field)?;
        validate_async_command_presentation_context(self.assoc(), pc_id, &object)?;
        let data_set_type = command_uint16_from_elements(command, COMMAND_DATA_SET_TYPE)?;
        if data_set_type == NO_DATA_SET && has_data_set {
            return Err(Error::new(
                "dicom dimse: command forbids accompanying dataset",
            ));
        }
        if data_set_type != NO_DATA_SET && !has_data_set {
            return Err(Error::new(
                "dicom dimse: command requires accompanying dataset",
            ));
        }
        accepted_transfer_syntax(self.assoc(), pc_id)
    }
    #[allow(clippy::too_many_arguments)]
    fn write_validated_message(
        &self,
        ctx: &Ctx,
        pc_id: u8,
        command: &[Element],
        data_set: Option<&Object>,
        syntax: &Syntax,
        before: impl FnOnce(),
        after: impl FnOnce(),
    ) -> Result<()> {
        let ctx = self.wire_ctx(ctx);
        self.serialize(&ctx, || {
            before();
            send_message_data_set(Some(&ctx), self.assoc(), pc_id, command, data_set, syntax)?;
            after();
            Ok(())
        })
    }
    fn is_handler_context(&self, ctx: &Ctx) -> bool {
        ctx.value::<HandlerSession>() == Some(&HandlerSession(self.inner.id))
    }
    fn response_context(&self, ctx: Option<&Ctx>) -> Ctx {
        match ctx {
            Some(c) if self.is_handler_context(c) => {
                self.suboperation_context(Some(c)).unwrap_or_default()
            }
            Some(c) => c.clone(),
            None => self.inner.ctx.clone(),
        }
    }
    fn suboperation_context(&self, ctx: Option<&Ctx>) -> Option<Ctx> {
        match ctx {
            Some(c) if self.is_handler_context(c) => Some(match c.deadline() {
                Some(d) => self.inner.ctx.with_deadline(d),
                None => self.inner.ctx.clone(),
            }),
            other => other.cloned(),
        }
    }
    fn watch_operation_context(&self, ctx: Ctx, operation: AsyncOperation) {
        let op = &operation.inner;
        {
            let mut o = lock(&op.mu);
            loop {
                if o.finished {
                    return;
                }
                if ctx.err().is_some() {
                    break;
                }
                o = wait(&op.cond, o);
            }
        }
        let control = Ctx::background().with_timeout(Duration::from_secs(1));
        if op.cancelable {
            if let Err(e) = operation.cancel(Some(&control))
                && !e.is(Kind::AsyncOperationComplete)
                && !e.is(Kind::AsyncSessionClosed)
                && !e.is_ul(ul::Kind::Closed)
            {
                self.shutdown(e);
            }
            return;
        }
        let _cancel = lock(&op.cancel_mu);
        let terminal_seen = lock(&op.mu).terminal_seen;
        if terminal_seen {
            return;
        }
        let _ = self.abort(Some(&control));
    }
    fn start_cancel_drain_timer(&self, operation: &AsyncOperation) {
        if operation.inner.drain_started.swap(true, Ordering::SeqCst) {
            return;
        }
        let session = self.clone();
        let op = operation.inner.clone();
        let timeout = self
            .inner
            .options
            .cancel_drain_timeout
            .positive()
            .unwrap_or_default();
        std::thread::spawn(move || {
            let end = Instant::now() + timeout;
            loop {
                if lock(&op.mu).finished || session.is_done() {
                    return;
                }
                if Instant::now() >= end {
                    break;
                }
                let o = lock(&op.mu);
                drop(wait(&op.cond, o));
            }
            let ctx = Ctx::background().with_timeout(Duration::from_secs(1));
            let _ = session.abort(Some(&ctx));
        });
    }
    pub fn release(&self, ctx: Option<&Ctx>, policy: AsyncReleasePolicy) -> Result<()> {
        if policy == AsyncReleasePolicy::Abort {
            return self.abort(ctx);
        }
        if !self.assoc().is_association_requestor {
            return Err(Error::sentinel(Kind::AsyncReleaseNotRequestor));
        }
        let ctx = ctx_or_background(ctx);
        {
            let mut st = self.st();
            loop {
                if st.closed {
                    drop(st);
                    return Err(self.session_error());
                }
                if !st.releasing {
                    st.releasing = true;
                    break;
                }
                if st.release_done {
                    return st.release_err.clone().map_or(Ok(()), Err);
                }
                if st.done {
                    drop(st);
                    return Err(self.session_error());
                }
                if let Some(e) = context_err(&ctx) {
                    return Err(e);
                }
                st = wait(&self.inner.cond, st);
            }
        }
        self.signal();
        if policy == AsyncReleasePolicy::RejectIfActive {
            let active = {
                let mut st = self.st();
                let active = !st.operations.is_empty()
                    || !st.incoming.is_empty()
                    || st.pending_incoming != 0;
                if active {
                    st.releasing = false;
                } else {
                    st.release_sent = true;
                }
                active
            };
            if active {
                self.signal();
                return Err(Error::sentinel(Kind::AsyncOperationsActive));
            }
        } else if let Err(e) = self.wait_idle_and_commit_release(&ctx) {
            {
                let mut st = self.st();
                if !st.closed {
                    st.releasing = false;
                    st.release_sent = false;
                }
            }
            self.signal();
            return Err(e);
        }
        let wire = self.wire_ctx(&ctx);
        if let Err(e) = self.serialize(&wire, || {
            self.assoc()
                .send(Some(&wire), &Pdu::ReleaseRq)
                .map_err(Error::from)
        }) {
            if e.is_ul(ul::Kind::MessageWriteNotStarted) {
                let aborting = lock(&self.inner.abort).started;
                {
                    let mut st = self.st();
                    if !st.closed && !aborting {
                        st.releasing = false;
                        st.release_sent = false;
                    }
                }
                self.signal();
                return Err(e);
            }
            self.shutdown(e.clone());
            return Err(e);
        }
        let mut st = self.st();
        loop {
            if st.release_done {
                return st.release_err.clone().map_or(Ok(()), Err);
            }
            if let Some(e) = context_err(&ctx) {
                drop(st);
                let cleanup = Ctx::background().with_timeout(Duration::from_secs(1));
                let _ = self.abort(Some(&cleanup));
                return Err(e);
            }
            st = wait(&self.inner.cond, st);
        }
    }
    fn wait_idle_and_commit_release(&self, ctx: &Ctx) -> Result<()> {
        let mut st = self.st();
        loop {
            if st.operations.is_empty() && st.incoming.is_empty() && st.pending_incoming == 0 {
                st.release_sent = true;
                drop(st);
                self.signal();
                return Ok(());
            }
            if let Some(e) = context_err(ctx) {
                return Err(e);
            }
            if st.done {
                drop(st);
                return Err(self.session_error());
            }
            st = wait(&self.inner.cond, st);
        }
    }
    pub fn abort(&self, ctx: Option<&Ctx>) -> Result<()> {
        let ctx = ctx_or_background(ctx);
        let mut a = lock(&self.inner.abort);
        if a.started {
            loop {
                if a.done {
                    return a.err.clone().map_or(Ok(()), Err);
                }
                if let Some(e) = context_err(&ctx) {
                    return Err(e);
                }
                a = match self.inner.abort_cond.wait_timeout(a, POLL) {
                    Ok((g, _)) => g,
                    Err(p) => p.into_inner().0,
                };
            }
        }
        let closed = {
            let mut st = self.st();
            let closed = st.closed;
            if !closed {
                st.releasing = true;
            }
            closed
        };
        self.signal();
        if closed {
            return Ok(());
        }
        a.started = true;
        drop(a);
        let wire = self.wire_ctx(&ctx);
        let sent = self.serialize(&wire, || {
            self.assoc()
                .send(
                    Some(&wire),
                    &Pdu::Abort(ul::AbortRq {
                        source: ul::ABORT_SOURCE_SERVICE_USER,
                        reason: ul::ABORT_REASON_NOT_SPECIFIED,
                    }),
                )
                .map_err(Error::from)
        });
        self.assoc().close();
        self.shutdown(Error::sentinel(Kind::AsyncSessionClosed));
        let result = sent.err();
        let mut a = lock(&self.inner.abort);
        a.err = result.clone();
        a.done = true;
        drop(a);
        self.inner.abort_cond.notify_all();
        result.map_or(Ok(()), Err)
    }
    pub fn close(&self) -> Result<()> {
        let ctx = Ctx::background().with_timeout(Duration::from_secs(1));
        self.abort(Some(&ctx))
    }
    fn shutdown(&self, err: Error) {
        self.shutdown_with_release(err.clone(), Some(err));
    }
    fn shutdown_with_release(&self, err: Error, release_err: Option<Error>) {
        let mut once = lock(&self.inner.shutdown_lock);
        if *once {
            return;
        }
        *once = true;
        self.inner.cancel.cancel();
        self.assoc().close();
        let (operations, incoming) = {
            let mut st = self.st();
            st.closed = true;
            st.close_err = Some(err.clone());
            let operations: Vec<Arc<OpInner>> = st.operations.drain().map(|(_, o)| o).collect();
            let incoming: Vec<Incoming> = st.incoming.drain().map(|(_, i)| i).collect();
            st.metrics.active_invoked = 0;
            st.metrics.active_performed = 0;
            st.queued_message_bytes = 0;
            st.metrics.queued_message_bytes = 0;
            st.invoked_used -= operations.len();
            for i in &incoming {
                if i.slot_held {
                    st.performed_used -= 1;
                }
            }
            (operations, incoming)
        };
        self.signal();
        for op in operations {
            let _ = op.transfer_response_accounting();
            op.finish(Some(err.clone()));
        }
        for i in incoming {
            i.cancel.cancel();
        }
        self.inner.owner.end(&self.inner.assoc);
        {
            let mut st = self.st();
            st.done = true;
            if !st.release_done {
                st.release_err = release_err;
                st.release_done = true;
            }
        }
        self.signal();
        drop(once);
    }
}
pub(crate) type DataSetEncoderFn<'a> = dyn Fn(&Ctx, &Syntax) -> Result<Vec<u8>> + 'a;
pub(crate) type DataSetStreamFn<'a> =
    dyn Fn(&Ctx, &mut dyn FnMut(&[u8]) -> Result<()>, &Syntax) -> Result<()> + 'a;
fn req(pc_id: u8, command: Vec<Element>, data_set: Option<Object>) -> AsyncRequest {
    AsyncRequest {
        presentation_context_id: pc_id,
        command,
        data_set,
    }
}
fn allocate_message_id(st: &mut State) -> Result<u16> {
    for _ in 0..65535 {
        let mut id = st.next_message_id;
        if id == 0 {
            id = 1;
        }
        st.next_message_id = id.wrapping_add(1);
        if !st.operations.contains_key(&id) {
            return Ok(id);
        }
    }
    Err(Error::sentinel(Kind::AsyncMessageIdExhausted))
}
impl OpInner {
    fn finish(&self, err: Option<Error>) {
        let cancel = {
            let mut o = lock(&self.mu);
            if o.finished {
                return;
            }
            o.err = err;
            o.finished = true;
            o.context_cancel.take()
        };
        if let Some(c) = cancel {
            c.cancel();
        }
        self.cond.notify_all();
    }
    fn release_response_accounting(&self, size: i64) {
        if size <= 0 {
            return;
        }
        let release = {
            let mut o = lock(&self.mu);
            let release = !o.accounting_transferred;
            if release {
                o.queued_bytes = (o.queued_bytes - size).max(0);
            }
            release
        };
        if release {
            self.session.release_message_bytes(size);
        }
    }
    fn transfer_response_accounting(&self) -> i64 {
        let mut o = lock(&self.mu);
        if o.accounting_transferred {
            return 0;
        }
        o.accounting_transferred = true;
        std::mem::take(&mut o.queued_bytes)
    }
}
impl AsyncOperation {
    pub fn next(&self, ctx: Option<&Ctx>) -> Result<AsyncMessage> {
        let op = &self.inner;
        let ctx = if op.cancelable {
            ctx_or_background(ctx)
        } else {
            ctx_or_background(op.session.suboperation_context(ctx).as_ref())
        };
        let mut o = lock(&op.mu);
        loop {
            if let Some(m) = o.responses.pop_front() {
                drop(o);
                op.release_response_accounting(m.message_bytes);
                return Ok(m);
            }
            if o.finished {
                return Err(o.err.clone().unwrap_or_else(eof));
            }
            if let Some(e) = context_err(&ctx) {
                return Err(e);
            }
            o = wait(&op.cond, o);
        }
    }
    pub fn wait(&self, ctx: Option<&Ctx>) -> Result<AsyncMessage> {
        let mut terminal: Option<AsyncMessage> = None;
        loop {
            match self.next(ctx) {
                Ok(m) => terminal = Some(m),
                Err(e) => {
                    if e.is_ul(ul::Kind::Eof)
                        && let Some(t) = terminal.filter(|t| t.command.is_some())
                    {
                        return Ok(t);
                    }
                    return Err(e);
                }
            }
        }
    }
    pub fn discard_responses(&self) {
        let op = &self.inner;
        let drained: Vec<AsyncMessage> = {
            let mut o = lock(&op.mu);
            o.discarding = true;
            o.responses.drain(..).collect()
        };
        for m in drained {
            op.release_response_accounting(m.message_bytes);
        }
    }
    pub fn message_id(&self) -> u16 {
        self.inner.message_id
    }
    pub fn is_done(&self) -> bool {
        lock(&self.inner.mu).finished
    }
    pub fn wait_done(&self, timeout: Duration) -> bool {
        let end = Instant::now() + timeout;
        let mut o = lock(&self.inner.mu);
        while !o.finished {
            if Instant::now() >= end {
                return false;
            }
            o = wait(&self.inner.cond, o);
        }
        true
    }
    pub fn cancel(&self, ctx: Option<&Ctx>) -> Result<()> {
        let op = &self.inner;
        if !op.cancelable {
            return Err(Error::sentinel(Kind::AsyncOperationNotCancelable));
        }
        let mut sent = lock(&op.cancel_mu);
        {
            let o = lock(&op.mu);
            if o.terminal_seen || o.finished {
                return Err(Error::sentinel(Kind::AsyncOperationComplete));
            }
        }
        if *sent {
            return Ok(());
        }
        let command = CCancelRequest {
            message_id_being_responded_to: op.message_id,
        }
        .command_set();
        let ctx = ctx_or_background(ctx);
        if let Err(e) = op
            .session
            .send_message(Some(&ctx), op.pc_id, &command, None)
        {
            if !e.is_ul(ul::Kind::MessageWriteNotStarted) {
                op.session.shutdown(e.clone());
            }
            return Err(e);
        }
        *sent = true;
        op.session.start_cancel_drain_timer(self);
        Ok(())
    }
}
