#![forbid(unsafe_code)]
use super::aetitle::normalize_ae_title;
use super::conn::Conn;
use super::error::{Error, Kind, Result};
use super::negotiation::AcceptedContext;
use super::pdu::*;
use crate::net::telemetry::{
    self, AssociationMetrics, AssociationRole, CommandObservation, Direction, Dispatcher,
    EndpointPolicy, Event, EventKind, OperationObservation, Sink, Worker, escape_peer_string,
};
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant, SystemTime};
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawPduCapture {
    pub time: SystemTime,
    pub association_id: String,
    pub role: AssociationRole,
    pub direction: Direction,
    pub pdu_type: u8,
    pub original_bytes: i64,
    pub data: Vec<u8>,
    pub truncated: bool,
}
pub trait RawPduSink: Send + Sync {
    fn capture_raw_pdu(&self, capture: RawPduCapture);
}
impl<F: Fn(RawPduCapture) + Send + Sync> RawPduSink for F {
    fn capture_raw_pdu(&self, capture: RawPduCapture) {
        self(capture)
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RawPduDispatcherStats {
    pub dropped: u64,
    pub panics: u64,
}
pub struct RawPduDispatcher {
    worker: Worker<RawPduCapture>,
}
impl std::fmt::Debug for RawPduDispatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RawPduDispatcher")
            .field("stats", &self.stats())
            .finish()
    }
}
impl RawPduDispatcher {
    pub fn new(sink: Arc<dyn RawPduSink>, queue_depth: i64) -> Arc<RawPduDispatcher> {
        let depth = if queue_depth <= 0 {
            8
        } else {
            queue_depth as usize
        };
        Arc::new(RawPduDispatcher {
            worker: Worker::new(depth, move |capture| sink.capture_raw_pdu(capture)),
        })
    }
    pub fn emit(&self, capture: RawPduCapture) -> bool {
        self.worker.emit(capture)
    }
    pub fn close(&self) {
        self.worker.close();
    }
    pub fn wait_done(&self, timeout: Duration) -> bool {
        self.worker.wait_done(timeout)
    }
    pub fn stats(&self) -> RawPduDispatcherStats {
        RawPduDispatcherStats {
            dropped: self.worker.dropped(),
            panics: self.worker.panics(),
        }
    }
}
#[derive(Clone, Default)]
pub struct ObservabilityOptions {
    pub sink: Option<Arc<dyn Sink>>,
    pub dispatcher: Option<Arc<Dispatcher>>,
    pub event_queue_depth: i64,
    pub endpoint_policy: EndpointPolicy,
    pub raw_pdu_sink: Option<Arc<dyn RawPduSink>>,
    pub raw_pdu_dispatcher: Option<Arc<RawPduDispatcher>>,
    pub max_captured_pdu_bytes: i64,
    pub max_captured_association_bytes: i64,
    pub raw_pdu_queue_depth: i64,
}
impl std::fmt::Debug for ObservabilityOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ObservabilityOptions")
            .field("sink", &self.sink.is_some())
            .field("dispatcher", &self.dispatcher.is_some())
            .field("endpoint_policy", &self.endpoint_policy)
            .field("raw_pdu_sink", &self.raw_pdu_sink.is_some())
            .finish_non_exhaustive()
    }
}
pub(crate) fn validate_observability_options(options: Option<&ObservabilityOptions>) -> Result<()> {
    let Some(o) = options else {
        return Ok(());
    };
    if o.event_queue_depth < 0 {
        return Err(Error::other(
            "dicom ul: observability event queue depth must not be negative",
        ));
    }
    if o.sink.is_some() && o.dispatcher.is_some() {
        return Err(Error::other(
            "dicom ul: observability Sink and Dispatcher are mutually exclusive",
        ));
    }
    if o.raw_pdu_sink.is_some() && o.raw_pdu_dispatcher.is_some() {
        return Err(Error::other(
            "dicom ul: raw PDU sink and dispatcher are mutually exclusive",
        ));
    }
    o.endpoint_policy.validate().map_err(Error::other)?;
    if o.raw_pdu_queue_depth < 0 {
        return Err(Error::other(
            "dicom ul: raw PDU queue depth must not be negative",
        ));
    }
    if (o.raw_pdu_sink.is_some() || o.raw_pdu_dispatcher.is_some())
        && (o.max_captured_pdu_bytes <= 0 || o.max_captured_association_bytes <= 0)
    {
        return Err(Error::other(
            "dicom ul: raw PDU capture requires positive per-PDU and per-association byte limits",
        ));
    }
    Ok(())
}
#[derive(Default)]
struct Endpoints {
    local_ae_title: String,
    remote_ae_title: String,
    local_address: String,
    remote_address: String,
    closed: bool,
    raw_bytes_remaining: i64,
}
pub(crate) struct OperationalState {
    pub(crate) id: String,
    role: AssociationRole,
    started: Instant,
    events: Option<Arc<Dispatcher>>,
    owned_events: bool,
    monitoring: bool,
    endpoint_policy: EndpointPolicy,
    raw_dispatcher: Option<Arc<RawPduDispatcher>>,
    owned_raw_dispatcher: bool,
    pub(crate) max_raw_pdu_bytes: i64,
    state: RwLock<Endpoints>,
    capture: Mutex<()>,
    pdus_inbound: AtomicU64,
    pdus_outbound: AtomicU64,
    bytes_inbound: AtomicU64,
    bytes_outbound: AtomicU64,
    malformed_pdus: AtomicU64,
    truncated_pdus: AtomicU64,
    timeouts: AtomicU64,
    protocol_errors: AtomicU64,
    dropped_events: AtomicU64,
    raw_captured_bytes: AtomicU64,
    raw_dropped: AtomicU64,
    established: AtomicU64,
    rejected: AtomicU64,
    released: AtomicU64,
    aborted: AtomicU64,
    closed_associations: AtomicU64,
    commands_inbound: AtomicU64,
    commands_outbound: AtomicU64,
    success_responses: AtomicU64,
    pending_responses: AtomicU64,
    warning_responses: AtomicU64,
    failure_responses: AtomicU64,
    canceled_responses: AtomicU64,
    operations_started: AtomicU64,
    operations_completed: AtomicU64,
    operation_errors: AtomicU64,
    canceled_operations: AtomicU64,
    active_operations: AtomicI64,
}
impl std::fmt::Debug for OperationalState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OperationalState")
            .field("id", &self.id)
            .field("monitoring", &self.monitoring)
            .finish_non_exhaustive()
    }
}
fn once(counter: &AtomicU64) -> bool {
    counter
        .compare_exchange(0, 1, Ordering::SeqCst, Ordering::SeqCst)
        .is_ok()
}
fn add(counter: &AtomicU64, n: u64) {
    counter.fetch_add(n, Ordering::SeqCst);
}
fn accepted_syntaxes(contexts: &[AcceptedContext], id: u8) -> (String, String) {
    contexts
        .iter()
        .find(|c| c.id == id)
        .map(|c| (c.abstract_syntax_uid.clone(), c.transfer_syntax_uid.clone()))
        .unwrap_or_default()
}
impl OperationalState {
    pub(crate) fn new(
        options: Option<&ObservabilityOptions>,
        role: AssociationRole,
        id: String,
    ) -> Result<Arc<OperationalState>> {
        validate_observability_options(options)?;
        let default_options = ObservabilityOptions::default();
        let o = options.unwrap_or(&default_options);
        let mut events = o.dispatcher.clone();
        let mut owned_events = false;
        if events.is_none()
            && let Some(sink) = &o.sink
        {
            events = Some(Dispatcher::new(sink.clone(), o.event_queue_depth));
            owned_events = true;
        }
        let mut raw_dispatcher = o.raw_pdu_dispatcher.clone();
        let mut owned_raw_dispatcher = false;
        if raw_dispatcher.is_none()
            && let Some(sink) = &o.raw_pdu_sink
        {
            raw_dispatcher = Some(RawPduDispatcher::new(sink.clone(), o.raw_pdu_queue_depth));
            owned_raw_dispatcher = true;
        }
        Ok(Arc::new(OperationalState {
            id,
            role,
            started: Instant::now(),
            monitoring: events.is_some() || raw_dispatcher.is_some(),
            events,
            owned_events,
            endpoint_policy: o.endpoint_policy.clone(),
            raw_dispatcher,
            owned_raw_dispatcher,
            max_raw_pdu_bytes: o.max_captured_pdu_bytes,
            state: RwLock::new(Endpoints {
                raw_bytes_remaining: o.max_captured_association_bytes,
                ..Default::default()
            }),
            capture: Mutex::new(()),
            pdus_inbound: AtomicU64::new(0),
            pdus_outbound: AtomicU64::new(0),
            bytes_inbound: AtomicU64::new(0),
            bytes_outbound: AtomicU64::new(0),
            malformed_pdus: AtomicU64::new(0),
            truncated_pdus: AtomicU64::new(0),
            timeouts: AtomicU64::new(0),
            protocol_errors: AtomicU64::new(0),
            dropped_events: AtomicU64::new(0),
            raw_captured_bytes: AtomicU64::new(0),
            raw_dropped: AtomicU64::new(0),
            established: AtomicU64::new(0),
            rejected: AtomicU64::new(0),
            released: AtomicU64::new(0),
            aborted: AtomicU64::new(0),
            closed_associations: AtomicU64::new(0),
            commands_inbound: AtomicU64::new(0),
            commands_outbound: AtomicU64::new(0),
            success_responses: AtomicU64::new(0),
            pending_responses: AtomicU64::new(0),
            warning_responses: AtomicU64::new(0),
            failure_responses: AtomicU64::new(0),
            canceled_responses: AtomicU64::new(0),
            operations_started: AtomicU64::new(0),
            operations_completed: AtomicU64::new(0),
            operation_errors: AtomicU64::new(0),
            canceled_operations: AtomicU64::new(0),
            active_operations: AtomicI64::new(0),
        }))
    }
    pub(crate) fn enabled(&self) -> bool {
        self.monitoring
    }
    fn emit(&self, mut event: Event) {
        let Some(events) = &self.events else {
            return;
        };
        if event.time == SystemTime::UNIX_EPOCH {
            event.time = SystemTime::now();
        }
        event.elapsed = self.started.elapsed();
        event.association_id = self.id.clone();
        event.role = self.role;
        event.abstract_syntax_uid = escape_peer_string(&event.abstract_syntax_uid);
        event.transfer_syntax_uid = escape_peer_string(&event.transfer_syntax_uid);
        {
            let s = self.state.read().unwrap();
            event.local_ae_title = s.local_ae_title.clone();
            event.remote_ae_title = s.remote_ae_title.clone();
            event.local_address = s.local_address.clone();
            event.remote_address = s.remote_address.clone();
        }
        if !events.emit(event) {
            add(&self.dropped_events, 1);
        }
    }
    pub(crate) fn set_connection(&self, conn: &Conn) {
        if !self.monitoring {
            return;
        }
        let local = self.endpoint_policy.address(&conn.local_addr().to_string());
        let remote = self.endpoint_policy.address(&conn.peer_addr().to_string());
        let mut s = self.state.write().unwrap();
        s.local_address = local;
        s.remote_address = remote;
    }
    pub(crate) fn set_logical_remote_address(&self, address: &str) {
        let remote = self.endpoint_policy.address(address);
        self.state.write().unwrap().remote_address = remote;
    }
    fn set_ae_titles(&self, called: &str, calling: &str) {
        let (local, remote) = match self.role {
            AssociationRole::Scp => (called, calling),
            AssociationRole::Scu => (calling, called),
        };
        let local = self.endpoint_policy.ae_title(normalize_ae_title(local));
        let remote = self.endpoint_policy.ae_title(normalize_ae_title(remote));
        let mut s = self.state.write().unwrap();
        s.local_ae_title = local;
        s.remote_ae_title = remote;
    }
    pub(crate) fn close(&self) {
        if once(&self.closed_associations) {
            self.emit(Event::of(EventKind::AssociationClosed));
        }
        if !self.monitoring {
            return;
        }
        let mut s = self.state.write().unwrap();
        if !s.closed {
            s.closed = true;
            if self.owned_events
                && let Some(events) = &self.events
            {
                events.close();
            }
            if self.owned_raw_dispatcher
                && let Some(raw) = &self.raw_dispatcher
            {
                raw.close();
            }
        }
    }
    pub(crate) fn mark_established(&self) {
        if once(&self.established) {
            self.emit(Event::of(EventKind::AssociationEstablished));
        }
    }
    pub(crate) fn mark_rejected(&self, rejection: Option<&AssociationRj>) {
        if once(&self.rejected) {
            let mut event = Event::of(EventKind::AssociationRejected);
            if let Some(rj) = rejection {
                event.rejection_result = rj.result;
                event.rejection_source = rj.source;
                event.rejection_reason = rj.reason;
            }
            self.emit(event);
        }
    }
    pub(crate) fn mark_released(&self) {
        if once(&self.released) {
            self.emit(Event::of(EventKind::AssociationReleased));
        }
    }
    pub(crate) fn mark_aborted(&self) {
        if once(&self.aborted) {
            self.emit(Event::of(EventKind::AssociationAborted));
        }
    }
    pub(crate) fn observe_command(
        &self,
        contexts: &[AcceptedContext],
        mut observation: CommandObservation,
    ) {
        if !self.enabled() {
            return;
        }
        match observation.direction {
            Direction::Inbound => add(&self.commands_inbound, 1),
            Direction::Outbound => add(&self.commands_outbound, 1),
            Direction::Unset => {}
        }
        if observation.status_set {
            let category = match observation.status_category.as_str() {
                telemetry::STATUS_CATEGORY_SUCCESS => {
                    add(&self.success_responses, 1);
                    telemetry::STATUS_CATEGORY_SUCCESS
                }
                telemetry::STATUS_CATEGORY_PENDING => {
                    add(&self.pending_responses, 1);
                    telemetry::STATUS_CATEGORY_PENDING
                }
                telemetry::STATUS_CATEGORY_WARNING => {
                    add(&self.warning_responses, 1);
                    telemetry::STATUS_CATEGORY_WARNING
                }
                telemetry::STATUS_CATEGORY_CANCELED => {
                    add(&self.canceled_responses, 1);
                    telemetry::STATUS_CATEGORY_CANCELED
                }
                _ => {
                    add(&self.failure_responses, 1);
                    telemetry::STATUS_CATEGORY_FAILURE
                }
            };
            observation.status_category = category.to_string();
        } else {
            observation.status_category.clear();
        }
        let (abstract_syntax_uid, transfer_syntax_uid) =
            accepted_syntaxes(contexts, observation.presentation_context_id);
        self.emit(Event {
            kind: EventKind::CommandObserved,
            direction: observation.direction,
            presentation_context_id: observation.presentation_context_id,
            abstract_syntax_uid,
            transfer_syntax_uid,
            command_field: observation.command_field,
            data_set_present: observation.data_set_present,
            status: observation.status,
            status_set: observation.status_set,
            status_category: observation.status_category,
            duration: observation.duration,
            ..Default::default()
        });
    }
    pub(crate) fn observe_operation(
        &self,
        contexts: &[AcceptedContext],
        observation: OperationObservation,
    ) {
        if !self.enabled() {
            return;
        }
        let error_class = match observation.error_class.as_str() {
            ""
            | telemetry::ERROR_CLASS_TIMEOUT
            | telemetry::ERROR_CLASS_PROTOCOL
            | telemetry::ERROR_CLASS_CANCELED
            | telemetry::ERROR_CLASS_DIMSE_STATUS
            | telemetry::ERROR_CLASS_HANDLER_OR_TRANSPORT => observation.error_class.clone(),
            _ => telemetry::ERROR_CLASS_OTHER.to_string(),
        };
        let mut kind = EventKind::OperationStarted;
        if observation.completed {
            kind = EventKind::OperationCompleted;
            add(&self.operations_completed, 1);
            self.active_operations.fetch_sub(1, Ordering::SeqCst);
            if !error_class.is_empty() {
                add(&self.operation_errors, 1);
                match error_class.as_str() {
                    telemetry::ERROR_CLASS_TIMEOUT => add(&self.timeouts, 1),
                    telemetry::ERROR_CLASS_PROTOCOL => add(&self.protocol_errors, 1),
                    telemetry::ERROR_CLASS_CANCELED => add(&self.canceled_operations, 1),
                    _ => {}
                }
            }
        } else {
            add(&self.operations_started, 1);
            self.active_operations.fetch_add(1, Ordering::SeqCst);
        }
        let (abstract_syntax_uid, transfer_syntax_uid) =
            accepted_syntaxes(contexts, observation.presentation_context_id);
        self.emit(Event {
            kind,
            presentation_context_id: observation.presentation_context_id,
            command_field: observation.command_field,
            duration: observation.duration,
            error_class,
            abstract_syntax_uid,
            transfer_syntax_uid,
            ..Default::default()
        });
    }
    fn count_bytes(&self, direction: Direction, byte_count: i64) {
        if byte_count > 0 {
            match direction {
                Direction::Inbound => add(&self.bytes_inbound, byte_count as u64),
                _ => add(&self.bytes_outbound, byte_count as u64),
            }
        }
    }
    pub(crate) fn observe_pdu(
        &self,
        direction: Direction,
        pdu: &Pdu,
        actual_length: i64,
        byte_count: i64,
        encoded: &[u8],
    ) {
        if !self.enabled() {
            return;
        }
        self.count_bytes(direction, byte_count);
        if let Pdu::AssociateRq(rq) = pdu {
            self.set_ae_titles(&rq.called_ae_title, &rq.calling_ae_title);
        }
        match direction {
            Direction::Inbound => add(&self.pdus_inbound, 1),
            _ => add(&self.pdus_outbound, 1),
        }
        let mut event = Event {
            kind: EventKind::PduObserved,
            direction,
            pdu_type: pdu.pdu_type(),
            actual_length,
            declared_length: declared_pdu_size(actual_length),
            ..Default::default()
        };
        if matches!(pdu, Pdu::Unknown(_)) {
            event.error_class = "unsupported_pdu".into();
            add(&self.protocol_errors, 1);
        }
        if let Pdu::PData(data) = pdu {
            event.presentation_context_id = common_presentation_context_id(&data.values);
        }
        self.emit(event);
        self.capture_raw_pdu(direction, pdu, actual_length, encoded);
    }
    pub(crate) fn observe_pdu_error(
        &self,
        direction: Direction,
        encoded_prefix: &[u8],
        actual_length: i64,
        byte_count: i64,
        err: &Error,
    ) {
        if !self.enabled() {
            return;
        }
        self.count_bytes(direction, byte_count);
        let mut event = Event {
            kind: EventKind::PduObserved,
            direction,
            actual_length,
            ..Default::default()
        };
        if let Some(first) = encoded_prefix.first() {
            event.pdu_type = *first;
        }
        if encoded_prefix.len() >= PDU_HEADER_SIZE as usize {
            event.declared_length = u32::from_be_bytes([
                encoded_prefix[2],
                encoded_prefix[3],
                encoded_prefix[4],
                encoded_prefix[5],
            ]);
        }
        let timeout = err.is(Kind::AssociationTimeout) || err.is(Kind::DeadlineExceeded);
        let canceled = err.is(Kind::Canceled);
        let eof = err.is(Kind::UnexpectedEof) || err.is(Kind::Eof);
        let outbound = direction == Direction::Outbound;
        let class = if outbound && timeout {
            add(&self.timeouts, 1);
            "write_timeout"
        } else if outbound && canceled {
            "write_canceled"
        } else if outbound {
            "transport_write"
        } else if eof && actual_length == 0 {
            "connection_closed"
        } else if eof {
            event.truncated = true;
            add(&self.truncated_pdus, 1);
            "truncated_pdu"
        } else if err.is(Kind::PduTooLarge) {
            event.malformed = true;
            add(&self.malformed_pdus, 1);
            add(&self.protocol_errors, 1);
            "oversized_pdu"
        } else if timeout {
            add(&self.timeouts, 1);
            "read_timeout"
        } else if canceled {
            "read_canceled"
        } else if is_malformed_pdu_error(err) {
            event.malformed = true;
            add(&self.malformed_pdus, 1);
            add(&self.protocol_errors, 1);
            "malformed_pdu"
        } else {
            "transport_read"
        };
        event.error_class = class.into();
        self.emit(event);
    }
    fn capture_raw_pdu(&self, direction: Direction, pdu: &Pdu, actual_length: i64, encoded: &[u8]) {
        let Some(raw) = &self.raw_dispatcher else {
            return;
        };
        if !matches!(pdu, Pdu::PData(_)) || encoded.is_empty() {
            return;
        }
        let _order = self.capture.lock().unwrap();
        let mut s = self.state.write().unwrap();
        if s.closed || s.raw_bytes_remaining <= 0 {
            return;
        }
        let limit = self
            .max_raw_pdu_bytes
            .min(s.raw_bytes_remaining)
            .min(encoded.len() as i64);
        if limit <= 0 {
            return;
        }
        let data = encoded[..limit as usize].to_vec();
        s.raw_bytes_remaining -= data.len() as i64;
        drop(s);
        let size = data.len();
        let capture = RawPduCapture {
            time: SystemTime::now(),
            association_id: self.id.clone(),
            role: self.role,
            direction,
            pdu_type: PDU_DATA_TF,
            original_bytes: actual_length,
            truncated: actual_length > size as i64,
            data,
        };
        if raw.emit(capture) {
            add(&self.raw_captured_bytes, size as u64);
        } else {
            add(&self.raw_dropped, 1);
        }
    }
    pub(crate) fn snapshot(&self) -> AssociationMetrics {
        let load = |c: &AtomicU64| c.load(Ordering::SeqCst);
        let sink_panics = match (&self.events, self.owned_events) {
            (Some(events), true) => events.stats().sink_panics,
            _ => 0,
        };
        let raw_sink_panics = match (&self.raw_dispatcher, self.owned_raw_dispatcher) {
            (Some(raw), true) => raw.stats().panics,
            _ => 0,
        };
        AssociationMetrics {
            pdus_inbound: load(&self.pdus_inbound),
            pdus_outbound: load(&self.pdus_outbound),
            bytes_inbound: load(&self.bytes_inbound),
            bytes_outbound: load(&self.bytes_outbound),
            malformed_pdus: load(&self.malformed_pdus),
            truncated_pdus: load(&self.truncated_pdus),
            raw_captured_bytes: load(&self.raw_captured_bytes),
            raw_dropped: load(&self.raw_dropped),
            raw_sink_panics,
            dropped_events: load(&self.dropped_events),
            sink_panics,
            commands_inbound: load(&self.commands_inbound),
            commands_outbound: load(&self.commands_outbound),
            success_responses: load(&self.success_responses),
            pending_responses: load(&self.pending_responses),
            warning_responses: load(&self.warning_responses),
            failure_responses: load(&self.failure_responses),
            canceled_responses: load(&self.canceled_responses),
            timeouts: load(&self.timeouts),
            protocol_errors: load(&self.protocol_errors),
            established: load(&self.established),
            rejected: load(&self.rejected),
            released: load(&self.released),
            aborted: load(&self.aborted),
            closed: load(&self.closed_associations),
            operations_started: load(&self.operations_started),
            operations_completed: load(&self.operations_completed),
            operation_errors: load(&self.operation_errors),
            canceled_operations: load(&self.canceled_operations),
            active_operations: self.active_operations.load(Ordering::SeqCst),
        }
    }
}
fn is_malformed_pdu_error(err: &Error) -> bool {
    [
        Kind::InvalidPdu,
        Kind::InvalidPduSize,
        Kind::LengthOverflow,
        Kind::UnsupportedPdu,
        Kind::InvalidPduField,
        Kind::MissingPduField,
        Kind::InvalidPduItem,
        Kind::InvalidUserItem,
        Kind::InvalidPcid,
    ]
    .into_iter()
    .any(|k| err.is(k))
}
fn declared_pdu_size(actual_length: i64) -> u32 {
    if actual_length <= i64::from(PDU_HEADER_SIZE) {
        return 0;
    }
    let length = actual_length - i64::from(PDU_HEADER_SIZE);
    u32::try_from(length).unwrap_or(0)
}
fn common_presentation_context_id(values: &[PDataValue]) -> u8 {
    let Some(first) = values.first() else {
        return 0;
    };
    let id = first.presentation_context_id;
    if values.iter().any(|v| v.presentation_context_id != id) {
        return 0;
    }
    id
}
