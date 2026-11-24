#![forbid(unsafe_code)]
use crate::textfmt::is_print;
fn peer_rune_safe(r: Option<char>) -> bool {
    match r {
        None => false,
        Some('\\') => false,
        Some(c) if (' '..'\u{7f}').contains(&c) => true,
        Some(c) => c as u32 >= 0x80 && is_print(c),
    }
}
pub fn escape_peer_string(s: &str) -> String {
    escape_peer_bytes(s.as_bytes())
}
pub(crate) fn escape_peer_bytes(s: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(s.len() + 8);
    let mut i = 0;
    while i < s.len() {
        let (r, size) = crate::textfmt::decode_rune(&s[i..]);
        match r {
            None => {
                out.push_str("\\x");
                out.push(HEX[(s[i] >> 4) as usize] as char);
                out.push(HEX[(s[i] & 0x0f) as usize] as char);
            }
            Some('\\') => out.push_str("\\\\"),
            Some('\n') => out.push_str("\\n"),
            Some('\r') => out.push_str("\\r"),
            Some('\t') => out.push_str("\\t"),
            Some(c) if !peer_rune_safe(r) => {
                let v = c as u32;
                if v < 0x80 {
                    out.push_str(&format!("\\x{v:02x}"));
                } else if v <= 0xffff {
                    out.push_str(&format!("\\u{v:04x}"));
                } else {
                    out.push_str(&format!("\\U{v:08x}"));
                }
            }
            Some(c) => out.push(c),
        }
        i += size.max(1);
    }
    out
}
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};
use std::sync::{Arc, Condvar, Mutex, RwLock};
use std::time::{Duration, SystemTime};
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Direction {
    #[default]
    Unset,
    Inbound,
    Outbound,
}
impl Direction {
    pub fn as_str(self) -> &'static str {
        match self {
            Direction::Unset => "",
            Direction::Inbound => "inbound",
            Direction::Outbound => "outbound",
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum AssociationRole {
    #[default]
    Scu,
    Scp,
}
impl AssociationRole {
    pub fn as_str(self) -> &'static str {
        match self {
            AssociationRole::Scu => "scu",
            AssociationRole::Scp => "scp",
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum EventKind {
    #[default]
    PduObserved,
    AssociationEstablished,
    AssociationRejected,
    AssociationReleased,
    AssociationAborted,
    AssociationClosed,
    CommandObserved,
    OperationStarted,
    OperationCompleted,
}
impl EventKind {
    pub fn as_str(self) -> &'static str {
        match self {
            EventKind::PduObserved => "pdu.observed",
            EventKind::AssociationEstablished => "association.established",
            EventKind::AssociationRejected => "association.rejected",
            EventKind::AssociationReleased => "association.released",
            EventKind::AssociationAborted => "association.aborted",
            EventKind::AssociationClosed => "association.closed",
            EventKind::CommandObserved => "dimse.command",
            EventKind::OperationStarted => "dimse.operation.started",
            EventKind::OperationCompleted => "dimse.operation.completed",
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum EndpointMode {
    #[default]
    Omit,
    Plaintext,
    HmacSha256,
    Other(u8),
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EndpointPolicy {
    pub ae_titles: EndpointMode,
    pub addresses: EndpointMode,
    pub hmac_key: Vec<u8>,
}
fn hmac_sha256(key: &[u8], parts: &[&[u8]]) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let mut block = [0u8; 64];
    if key.len() > 64 {
        block[..32].copy_from_slice(&Sha256::digest(key));
    } else {
        block[..key.len()].copy_from_slice(key);
    }
    let mut inner = Sha256::new();
    inner.update(block.map(|b| b ^ 0x36));
    for part in parts {
        inner.update(part);
    }
    let mut outer = Sha256::new();
    outer.update(block.map(|b| b ^ 0x5c));
    outer.update(inner.finalize());
    outer.finalize().into()
}
impl EndpointPolicy {
    pub fn validate(&self) -> Result<(), crate::MessageError> {
        if let EndpointMode::Other(v) = self.ae_titles {
            return Err(format!("dicom telemetry: invalid AE title endpoint mode {v}").into());
        }
        if let EndpointMode::Other(v) = self.addresses {
            return Err(format!("dicom telemetry: invalid address endpoint mode {v}").into());
        }
        if (self.ae_titles == EndpointMode::HmacSha256
            || self.addresses == EndpointMode::HmacSha256)
            && self.hmac_key.is_empty()
        {
            return Err("dicom telemetry: HMAC endpoint redaction requires a key".into());
        }
        Ok(())
    }
    pub fn ae_title(&self, value: &str) -> String {
        self.transform(self.ae_titles, "ae", value)
    }
    pub fn address(&self, value: &str) -> String {
        self.transform(self.addresses, "address", value)
    }
    fn transform(&self, mode: EndpointMode, domain: &str, value: &str) -> String {
        if value.is_empty() {
            return String::new();
        }
        match mode {
            EndpointMode::Plaintext => escape_peer_string(value),
            EndpointMode::HmacSha256 if !self.hmac_key.is_empty() => {
                let mac = hmac_sha256(&self.hmac_key, &[domain.as_bytes(), &[0], value.as_bytes()]);
                let mut out = String::from("hmac-sha256:");
                for b in mac {
                    out.push_str(&format!("{b:02x}"));
                }
                out
            }
            _ => String::new(),
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    pub time: SystemTime,
    pub elapsed: Duration,
    pub kind: EventKind,
    pub association_id: String,
    pub role: AssociationRole,
    pub direction: Direction,
    pub pdu_type: u8,
    pub declared_length: u32,
    pub actual_length: i64,
    pub presentation_context_id: u8,
    pub abstract_syntax_uid: String,
    pub transfer_syntax_uid: String,
    pub local_ae_title: String,
    pub remote_ae_title: String,
    pub local_address: String,
    pub remote_address: String,
    pub malformed: bool,
    pub truncated: bool,
    pub error_class: String,
    pub command_field: u16,
    pub data_set_present: bool,
    pub status: u16,
    pub status_set: bool,
    pub status_category: String,
    pub duration: Duration,
    pub rejection_result: u8,
    pub rejection_source: u8,
    pub rejection_reason: u8,
}
impl Default for Event {
    fn default() -> Self {
        Event {
            time: SystemTime::UNIX_EPOCH,
            elapsed: Duration::ZERO,
            kind: EventKind::default(),
            association_id: String::new(),
            role: AssociationRole::default(),
            direction: Direction::default(),
            pdu_type: 0,
            declared_length: 0,
            actual_length: 0,
            presentation_context_id: 0,
            abstract_syntax_uid: String::new(),
            transfer_syntax_uid: String::new(),
            local_ae_title: String::new(),
            remote_ae_title: String::new(),
            local_address: String::new(),
            remote_address: String::new(),
            malformed: false,
            truncated: false,
            error_class: String::new(),
            command_field: 0,
            data_set_present: false,
            status: 0,
            status_set: false,
            status_category: String::new(),
            duration: Duration::ZERO,
            rejection_result: 0,
            rejection_source: 0,
            rejection_reason: 0,
        }
    }
}
impl Event {
    pub fn of(kind: EventKind) -> Event {
        Event {
            kind,
            ..Default::default()
        }
    }
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CommandObservation {
    pub direction: Direction,
    pub presentation_context_id: u8,
    pub abstract_syntax_uid: String,
    pub transfer_syntax_uid: String,
    pub command_field: u16,
    pub data_set_present: bool,
    pub status: u16,
    pub status_set: bool,
    pub status_category: String,
    pub duration: Duration,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OperationObservation {
    pub completed: bool,
    pub presentation_context_id: u8,
    pub command_field: u16,
    pub duration: Duration,
    pub error_class: String,
}
pub const STATUS_CATEGORY_SUCCESS: &str = "success";
pub const STATUS_CATEGORY_PENDING: &str = "pending";
pub const STATUS_CATEGORY_WARNING: &str = "warning";
pub const STATUS_CATEGORY_FAILURE: &str = "failure";
pub const STATUS_CATEGORY_CANCELED: &str = "canceled";
pub const ERROR_CLASS_TIMEOUT: &str = "timeout";
pub const ERROR_CLASS_PROTOCOL: &str = "protocol";
pub const ERROR_CLASS_CANCELED: &str = "canceled";
pub const ERROR_CLASS_DIMSE_STATUS: &str = "dimse_status";
pub const ERROR_CLASS_HANDLER_OR_TRANSPORT: &str = "handler_or_transport";
pub const ERROR_CLASS_OTHER: &str = "other";
pub trait Sink: Send + Sync {
    fn emit_network_event(&self, event: Event);
}
impl<F: Fn(Event) + Send + Sync> Sink for F {
    fn emit_network_event(&self, event: Event) {
        self(event)
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DispatcherStats {
    pub dropped_events: u64,
    pub sink_panics: u64,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AssociationMetrics {
    pub pdus_inbound: u64,
    pub pdus_outbound: u64,
    pub bytes_inbound: u64,
    pub bytes_outbound: u64,
    pub malformed_pdus: u64,
    pub truncated_pdus: u64,
    pub raw_captured_bytes: u64,
    pub raw_dropped: u64,
    pub raw_sink_panics: u64,
    pub dropped_events: u64,
    pub sink_panics: u64,
    pub commands_inbound: u64,
    pub commands_outbound: u64,
    pub success_responses: u64,
    pub pending_responses: u64,
    pub warning_responses: u64,
    pub failure_responses: u64,
    pub canceled_responses: u64,
    pub timeouts: u64,
    pub protocol_errors: u64,
    pub established: u64,
    pub rejected: u64,
    pub released: u64,
    pub aborted: u64,
    pub closed: u64,
    pub operations_started: u64,
    pub operations_completed: u64,
    pub operation_errors: u64,
    pub canceled_operations: u64,
    pub active_operations: i64,
}
pub(crate) struct Worker<T: Send + 'static> {
    sender: RwLock<Option<SyncSender<T>>>,
    done: Arc<(Mutex<bool>, Condvar)>,
    dropped: AtomicU64,
    panics: Arc<AtomicU64>,
}
impl<T: Send + 'static> Worker<T> {
    pub(crate) fn new(depth: usize, consume: impl Fn(T) + Send + 'static) -> Worker<T> {
        let (sender, receiver): (SyncSender<T>, Receiver<T>) = sync_channel(depth.max(1));
        let done = Arc::new((Mutex::new(false), Condvar::new()));
        let panics = Arc::new(AtomicU64::new(0));
        let (finished, counted) = (done.clone(), panics.clone());
        std::thread::spawn(move || {
            for item in receiver {
                let run = std::panic::AssertUnwindSafe(|| consume(item));
                if std::panic::catch_unwind(run).is_err() {
                    counted.fetch_add(1, Ordering::SeqCst);
                }
            }
            *finished.0.lock().unwrap() = true;
            finished.1.notify_all();
        });
        Worker {
            sender: RwLock::new(Some(sender)),
            done,
            dropped: AtomicU64::new(0),
            panics,
        }
    }
    pub(crate) fn emit(&self, item: T) -> bool {
        let guard = self.sender.read().unwrap();
        let Some(sender) = guard.as_ref() else {
            return false;
        };
        match sender.try_send(item) {
            Ok(()) => true,
            Err(TrySendError::Full(_)) => {
                self.dropped.fetch_add(1, Ordering::SeqCst);
                false
            }
            Err(TrySendError::Disconnected(_)) => false,
        }
    }
    pub(crate) fn close(&self) {
        self.sender.write().unwrap().take();
    }
    pub(crate) fn wait_done(&self, timeout: Duration) -> bool {
        let (lock, cond) = &*self.done;
        let guard = lock.lock().unwrap();
        let (guard, _) = cond
            .wait_timeout_while(guard, timeout, |done| !*done)
            .unwrap();
        *guard
    }
    pub(crate) fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::SeqCst)
    }
    pub(crate) fn panics(&self) -> u64 {
        self.panics.load(Ordering::SeqCst)
    }
}
pub struct Dispatcher {
    worker: Worker<Event>,
}
impl std::fmt::Debug for Dispatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Dispatcher")
            .field("stats", &self.stats())
            .finish()
    }
}
impl Dispatcher {
    pub fn new(sink: Arc<dyn Sink>, queue_depth: i64) -> Arc<Dispatcher> {
        let depth = if queue_depth <= 0 {
            64
        } else {
            queue_depth as usize
        };
        Arc::new(Dispatcher {
            worker: Worker::new(depth, move |event| sink.emit_network_event(event)),
        })
    }
    pub fn emit(&self, mut event: Event) -> bool {
        if event.time == SystemTime::UNIX_EPOCH {
            event.time = SystemTime::now();
        }
        self.worker.emit(event)
    }
    pub fn close(&self) {
        self.worker.close();
    }
    pub fn wait_done(&self, timeout: Duration) -> bool {
        self.worker.wait_done(timeout)
    }
    pub fn stats(&self) -> DispatcherStats {
        DispatcherStats {
            dropped_events: self.worker.dropped(),
            sink_panics: self.worker.panics(),
        }
    }
}
impl Sink for Dispatcher {
    fn emit_network_event(&self, event: Event) {
        let _ = self.emit(event);
    }
}
