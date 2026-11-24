#![forbid(unsafe_code)]
use super::atna::{XML_HEADER, encode_message};
use super::durable_disk::{DiskState, RecordMeta};
use super::suppression::suppressed;
use super::syslog::{
    Link, MAX_SYSLOG_UDP_MESSAGE, SYSLOG_UDP, Wake, check_transport, dial_link, send_link,
    syslog_record,
};
use super::{
    CallbackError, CancelToken, ClinicalPolicy, Ctx, Error, ErrorKind, Message, Nanos,
    OUTCOME_SUCCESS, Result, SuppressionRule, SyslogDial, SyslogTls, Time, ctx_err,
    split_host_port, timetext, validate_suppression,
};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::File;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};
#[derive(Clone, Default)]
pub struct AuditDestination {
    pub id: String,
    pub network: String,
    pub address: String,
    pub tls: Option<SyslogTls>,
    pub fingerprint: String,
    pub dial: Option<SyslogDial>,
}
pub(crate) type DurableClock = Arc<dyn Fn() -> Time + Send + Sync>;
pub type DurableFault = Arc<dyn Fn(&str) -> std::result::Result<(), CallbackError> + Send + Sync>;
#[derive(Clone, Default)]
pub struct DurableOptions {
    pub directory: String,
    pub destinations: Vec<AuditDestination>,
    pub app_name: String,
    pub hostname: String,
    pub max_messages: i64,
    pub max_bytes: i64,
    pub max_record_bytes: i64,
    pub max_age: Nanos,
    pub retry_interval: Nanos,
    pub max_retry_interval: Nanos,
    pub send_timeout: Nanos,
    pub suppression: Vec<SuppressionRule>,
    pub source_identity: String,
    pub initial_source_cursor: u64,
    pub resume_source: bool,
    pub now: Option<DurableClock>,
    pub fault: Option<DurableFault>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DurableStats {
    pub pending: i64,
    pub pending_bytes: i64,
    pub source_cursor: u64,
    pub source_paused: bool,
    pub expired: u64,
    pub suppressed: u64,
    pub sent: u64,
    pub failed: u64,
    pub rejected: u64,
    pub source_retirements: u64,
    pub persistence_failures: u64,
    pub corruptions: u64,
    pub destinations: BTreeMap<String, i64>,
}
const SECOND: i64 = 1_000_000_000;
fn normalize_durable(mut o: DurableOptions) -> Result<DurableOptions> {
    let config = |code: &str| Error::spool(code, ErrorKind::SyslogConfig);
    if o.directory.is_empty()
        || o.directory.len() > 4096
        || o.app_name.len() > 48
        || o.hostname.len() > 255
        || o.destinations.len() > 16
        || o.source_identity.len() > 256
    {
        return Err(config("configuration"));
    }
    if o.max_messages == 0 {
        o.max_messages = 1000;
    }
    if o.max_bytes == 0 {
        o.max_bytes = 16 << 20;
    }
    if o.max_record_bytes == 0 {
        o.max_record_bytes = 4 << 20;
    }
    if o.max_age.is_zero() {
        o.max_age = Nanos(30 * 24 * 3600 * SECOND);
    }
    if o.retry_interval.is_zero() {
        o.retry_interval = Nanos(5 * SECOND);
    }
    if o.max_retry_interval.is_zero() {
        o.max_retry_interval = Nanos(60 * SECOND);
    }
    if o.send_timeout.is_zero() {
        o.send_timeout = Nanos(10 * SECOND);
    }
    if o.max_messages < 1
        || o.max_messages > 100_000
        || o.max_bytes < 1
        || o.max_bytes > 1 << 40
        || o.max_record_bytes < 1
        || o.max_record_bytes > 64 << 20
        || o.max_record_bytes > o.max_bytes
        || o.max_age.0 < SECOND
        || o.max_age.0 > 3650 * 24 * 3600 * SECOND
        || o.retry_interval.0 < 1_000_000
        || o.max_retry_interval < o.retry_interval
        || o.max_retry_interval.0 > 3600 * SECOND
        || o.send_timeout.0 < 1_000_000
        || o.send_timeout.0 > 60 * SECOND
    {
        return Err(config("limits"));
    }
    validate_suppression(&o.suppression)?;
    let mut seen: Vec<&str> = Vec::new();
    for d in &o.destinations {
        if !valid_destination_id(&d.id)
            || seen.contains(&d.id.as_str())
            || d.address.len() > 1024
            || d.fingerprint.len() > 256
        {
            return Err(config("destination_configuration"));
        }
        seen.push(&d.id);
        if split_host_port(&d.address).is_none()
            || check_transport(&d.network, &d.address, d.tls.is_some()).is_err()
        {
            return Err(config("destination_configuration"));
        }
        if d.tls.is_some() && (d.fingerprint.len() != 64 || !valid_hex_digest(&d.fingerprint)) {
            return Err(config("tls_policy_fingerprint"));
        }
    }
    Ok(o)
}
pub(crate) fn valid_destination_id(id: &str) -> bool {
    (1..=64).contains(&id.len())
        && id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
}
pub(crate) fn valid_hex_digest(s: &str) -> bool {
    s.len().is_multiple_of(2) && s.bytes().all(|c| c.is_ascii_hexdigit())
}
pub(crate) fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}
pub(crate) fn destination_fingerprint(d: &AuditDestination) -> String {
    let mut h = Sha256::new();
    h.update(format!("{}\0{}\0{}", d.network, d.address, d.fingerprint));
    #[cfg(feature = "tls")]
    if let Some(c) = &d.tls {
        let profile = c.policy.is_some();
        let suites: Vec<String> = c.cipher_suites().iter().map(u16::to_string).collect();
        h.update(format!(
            "\0{}/{}/{}/{}",
            c.server_name(),
            c.min_version(),
            c.max_version(),
            profile
        ));
        h.update(format!(
            "/[{}]/[]/[]/false/0/0/false/{}/false/false/false/false/false/false",
            suites.join(" "),
            profile
        ));
        for der in c.certificate_ders() {
            h.update(der);
        }
    }
    hex(&h.finalize())
}
struct Core {
    pub(crate) state: DiskState,
    lock: Option<File>,
    closed: bool,
    poisoned: bool,
    persistence_failures: u64,
    corruptions: u64,
    active: usize,
}
struct Worker {
    destination: AuditDestination,
    gate: Mutex<bool>,
    gate_free: Condvar,
    wake: Wake,
    conn: Mutex<Option<Arc<Link>>>,
}
#[derive(Default)]
struct RunState {
    cancel: Option<CancelToken>,
    started: bool,
    running: bool,
}
pub struct DurableLogger {
    pub(crate) options: DurableOptions,
    core: Mutex<Core>,
    idle: Condvar,
    workers: BTreeMap<String, Worker>,
    lifecycle: CancelToken,
    run: Mutex<RunState>,
    run_done: Condvar,
    close_result: OnceLock<Result<()>>,
}
pub(crate) struct Spool<'a> {
    pub(crate) options: &'a DurableOptions,
    pub(crate) state: &'a mut DiskState,
    pub(crate) poisoned: &'a mut bool,
    pub(crate) persistence_failures: &'a mut u64,
    pub(crate) recovery: &'a Ctx,
}
fn clock(options: &DurableOptions) -> Time {
    let t = match &options.now {
        Some(now) => now(),
        None => Time::now_local(),
    };
    Time::from_unix_utc(t.unix(), t.nanosecond())
}
fn fault(options: &DurableOptions, stage: &str) -> std::io::Result<()> {
    match &options.fault {
        Some(fault) if fault(stage).is_err() => Err(std::io::Error::other("fault")),
        _ => Ok(()),
    }
}
impl Spool<'_> {
    pub(crate) fn now(&self) -> Time {
        clock(self.options)
    }
    pub(crate) fn fault(&self, stage: &str) -> std::io::Result<()> {
        fault(self.options, stage)
    }
    pub(crate) fn disk_failure(&mut self) -> Error {
        *self.persistence_failures += 1;
        *self.poisoned = true;
        Error::spool("persistence", ErrorKind::DurableSpool)
    }
}
fn severity_of(m: &Message) -> i64 {
    if m.event.outcome_indicator != OUTCOME_SUCCESS {
        4
    } else {
        5
    }
}
impl DurableLogger {
    pub fn open(options: DurableOptions) -> Result<DurableLogger> {
        DurableLogger::open_context(&Ctx::background(), options)
    }
    pub fn open_context(ctx: &Ctx, options: DurableOptions) -> Result<DurableLogger> {
        if let Some(e) = ctx_err(ctx) {
            return Err(e);
        }
        let options = normalize_durable(options)?;
        let mut state = DiskState::default();
        let mut poisoned = false;
        let mut persistence_failures = 0;
        let mut lock = None;
        let opened = Spool {
            options: &options,
            state: &mut state,
            poisoned: &mut poisoned,
            persistence_failures: &mut persistence_failures,
            recovery: ctx,
        }
        .open_disk(&mut lock);
        let failed = match opened {
            Err(e) => Some(e),
            Ok(()) => ctx_err(ctx),
        };
        if let Some(e) = failed {
            if let Some(lock) = lock {
                let _ = super::privatefs::unlock_spool(lock);
            }
            return Err(e);
        }
        let workers = options
            .destinations
            .iter()
            .map(|d| {
                (
                    d.id.clone(),
                    Worker {
                        destination: d.clone(),
                        gate: Mutex::new(false),
                        gate_free: Condvar::new(),
                        wake: Wake::default(),
                        conn: Mutex::new(None),
                    },
                )
            })
            .collect();
        Ok(DurableLogger {
            options,
            core: Mutex::new(Core {
                state,
                lock,
                closed: false,
                poisoned,
                persistence_failures,
                corruptions: 0,
                active: 0,
            }),
            idle: Condvar::new(),
            workers,
            lifecycle: CancelToken::new(),
            run: Mutex::new(RunState::default()),
            run_done: Condvar::new(),
            close_result: OnceLock::new(),
        })
    }
    fn lock(&self) -> MutexGuard<'_, Core> {
        self.core.lock().unwrap_or_else(|e| e.into_inner())
    }
    fn spool<'a>(&'a self, core: &'a mut Core, recovery: &'a Ctx) -> Spool<'a> {
        Spool {
            options: &self.options,
            state: &mut core.state,
            poisoned: &mut core.poisoned,
            persistence_failures: &mut core.persistence_failures,
            recovery,
        }
    }
    fn usable(&self, core: &Core) -> Result<()> {
        if core.closed || self.lifecycle.is_canceled() {
            return Err(Error::sentinel(ErrorKind::SpoolClosed));
        }
        if core.poisoned {
            return Err(Error::spool(
                "persistence_uncertain",
                ErrorKind::DurableSpool,
            ));
        }
        Ok(())
    }
    fn signal(&self) {
        for w in self.workers.values() {
            w.wake.signal();
        }
    }
    fn marshal(&self, ctx: &Ctx, m: &Message) -> Result<Vec<u8>> {
        if let Some(e) = ctx_err(ctx) {
            return Err(e);
        }
        let limit = self.options.max_record_bytes;
        let full = || Error::spool("record_limit", ErrorKind::SpoolFull);
        if m.participants.len() as i64 > limit
            || m.sources.len() as i64 > limit
            || m.objects.len() as i64 > limit
        {
            return Err(full());
        }
        if m.validate().is_err() {
            return Err(Error::spool("message", ErrorKind::AuditMessage));
        }
        let mut out = String::from(XML_HEADER);
        out.push_str(&encode_message(m));
        if out.len() as i64 > limit {
            return Err(full());
        }
        Ok(out.into_bytes())
    }
    pub fn log(&self, m: &Message) -> Result<()> {
        self.log_source(&Ctx::background(), 0, m)
    }
    pub fn log_source(&self, ctx: &Ctx, source: u64, m: &Message) -> Result<()> {
        if suppressed(&self.options.suppression, m) {
            return self.advance(ctx, source, true);
        }
        let payload = self.marshal(ctx, m)?;
        let at = timetext::parse_rfc3339(&m.event.date_time).unwrap_or_default();
        self.log_raw_source(ctx, source, severity_of(m), &at, &payload)
    }
    pub fn log_clinical(&self, policy: &ClinicalPolicy, m: &Message) -> Result<()> {
        self.log_clinical_context(&Ctx::background(), policy, m)
    }
    pub fn log_clinical_context(
        &self,
        ctx: &Ctx,
        policy: &ClinicalPolicy,
        m: &Message,
    ) -> Result<()> {
        if let Some(e) = ctx_err(ctx) {
            return Err(e);
        }
        if suppressed(&self.options.suppression, m) {
            return self.advance(ctx, 0, true);
        }
        let mut p = policy.normalize()?;
        p.limits.max_message_bytes = p
            .limits
            .max_message_bytes
            .min(self.options.max_record_bytes);
        let payload = m.marshal_clinical(&p)?;
        let at = timetext::parse_rfc3339(&m.event.date_time).unwrap_or_default();
        self.log_raw_source(ctx, 0, severity_of(m), &at, &payload)
    }
    pub fn log_raw(&self, severity: i64, at: &Time, payload: &[u8]) -> Result<()> {
        self.log_raw_source(&Ctx::background(), 0, severity, at, payload)
    }
    pub fn log_raw_source(
        &self,
        ctx: &Ctx,
        source: u64,
        severity: i64,
        at: &Time,
        payload: &[u8],
    ) -> Result<()> {
        if !(0..=7).contains(&severity) {
            return Err(Error::spool(
                "record_configuration",
                ErrorKind::SyslogConfig,
            ));
        }
        if let Some(e) = ctx_err(ctx) {
            return Err(e);
        }
        let o = &self.options;
        let full = || Error::spool("record_limit", ErrorKind::SpoolFull);
        if payload.len() as i64 > o.max_record_bytes {
            return Err(full());
        }
        let at = if at.is_zero() { clock(o) } else { *at };
        let record = syslog_record(&o.hostname, &o.app_name, severity, &at, payload);
        if record.len() as i64 > o.max_record_bytes {
            return Err(full());
        }
        if record.len() > MAX_SYSLOG_UDP_MESSAGE
            && o.destinations.iter().any(|d| d.network == SYSLOG_UDP)
        {
            return Err(Error::spool("udp_record_limit", ErrorKind::SpoolFull));
        }
        let background = Ctx::background();
        let mut guard = self.lock();
        self.usable(&guard)?;
        if source > 0 && source <= guard.state.source_cursor {
            return Ok(());
        }
        let mut spool = self.spool(&mut guard, &background);
        spool.expire()?;
        let size: i64 = spool.state.records.iter().map(|r| r.bytes).sum();
        if spool.state.records.len() as i64 >= o.max_messages
            || record.len() as i64 > o.max_bytes - size
        {
            let mut next = spool.state.clone();
            next.rejected += 1;
            spool.commit(next)?;
            return Err(Error::sentinel(ErrorKind::SpoolFull));
        }
        if self.workers.is_empty() {
            return Err(Error::spool(
                "no_active_destination",
                ErrorKind::SyslogConfig,
            ));
        }
        if spool.state.next == u64::MAX {
            return Err(Error::spool("sequence_limit", ErrorKind::DurableSpool));
        }
        let meta = RecordMeta {
            sequence: spool.state.next,
            source,
            created: spool.now(),
            created_utc: true,
            bytes: record.len() as i64,
            digest: hex(&Sha256::digest(&record)),
            targets: self.workers.keys().cloned().collect(),
        };
        if spool.write_record(&meta, &record).is_err() {
            return Err(spool.disk_failure());
        }
        let mut next = spool.state.clone();
        next.next += 1;
        next.records.push(meta);
        if source > 0 {
            next.source_cursor = source;
        }
        spool.commit(next)?;
        drop(guard);
        self.signal();
        Ok(())
    }
    pub fn advance_source(&self, ctx: &Ctx, source: u64) -> Result<()> {
        self.advance(ctx, source, false)
    }
    pub fn pause_source(&self, ctx: &Ctx, through: u64) -> Result<()> {
        if let Some(e) = ctx_err(ctx) {
            return Err(e);
        }
        let background = Ctx::background();
        let mut guard = self.lock();
        self.usable(&guard)?;
        let mut next = guard.state.clone();
        next.source_paused = true;
        if through > next.source_cursor {
            next.source_cursor = through;
            next.source_retirements += 1;
        }
        self.spool(&mut guard, &background).commit(next)
    }
    fn advance(&self, ctx: &Ctx, source: u64, is_suppressed: bool) -> Result<()> {
        if let Some(e) = ctx_err(ctx) {
            return Err(e);
        }
        let background = Ctx::background();
        let mut guard = self.lock();
        self.usable(&guard)?;
        if source > 0 && source <= guard.state.source_cursor {
            return Ok(());
        }
        let mut next = guard.state.clone();
        if source > 0 {
            next.source_cursor = source;
        }
        if is_suppressed {
            next.suppressed += 1;
        } else if source > 0 {
            next.source_retirements += 1;
        }
        self.spool(&mut guard, &background).commit(next)
    }
    pub fn source_cursor(&self) -> u64 {
        self.lock().state.source_cursor
    }
    pub fn stats(&self) -> DurableStats {
        let core = self.lock();
        let s = &core.state;
        let mut stats = DurableStats {
            pending: s.records.len() as i64,
            source_cursor: s.source_cursor,
            source_paused: s.source_paused,
            expired: s.expired,
            suppressed: s.suppressed,
            sent: s.sent,
            failed: s.failed,
            rejected: s.rejected,
            source_retirements: s.source_retirements,
            persistence_failures: core.persistence_failures,
            corruptions: core.corruptions,
            ..Default::default()
        };
        for r in &s.records {
            stats.pending_bytes += r.bytes;
            for id in &r.targets {
                if s.cursor(id) < r.sequence {
                    *stats.destinations.entry(id.clone()).or_insert(0) += 1;
                }
            }
        }
        stats
    }
    fn close_conn(&self, w: &Worker) {
        let conn = w.conn.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(conn) = conn {
            conn.close();
        }
    }
    fn connection(&self, w: &Worker, ctx: &Ctx) -> Result<Arc<Link>> {
        if let Some(conn) = w.conn.lock().unwrap_or_else(|e| e.into_inner()).clone() {
            return Ok(conn);
        }
        let d = &w.destination;
        let conn = dial_link(&d.network, &d.address, d.tls.as_ref(), d.dial.as_ref(), ctx)?;
        let mut slot = w.conn.lock().unwrap_or_else(|e| e.into_inner());
        if ctx.err().is_some() || self.lifecycle.is_canceled() {
            drop(slot);
            conn.close();
            return Err(Error::sentinel(ErrorKind::SpoolClosed));
        }
        *slot = Some(conn.clone());
        Ok(conn)
    }
    fn flush_worker(&self, w: &Worker, ctx: &Ctx) -> Result<()> {
        {
            let mut core = self.lock();
            self.usable(&core)?;
            core.active += 1;
        }
        struct Active<'a>(&'a DurableLogger);
        impl Drop for Active<'_> {
            fn drop(&mut self) {
                self.0.lock().active -= 1;
                self.0.idle.notify_all();
            }
        }
        let _active = Active(self);
        let ctx = ctx.with_cancel(&self.lifecycle);
        {
            let mut busy = w.gate.lock().unwrap_or_else(|e| e.into_inner());
            loop {
                if let Some(e) = ctx_err(&ctx) {
                    return Err(e);
                }
                if !*busy {
                    *busy = true;
                    break;
                }
                busy = w
                    .gate_free
                    .wait_timeout(busy, Duration::from_millis(5))
                    .unwrap_or_else(|e| e.into_inner())
                    .0;
            }
        }
        struct Gate<'a>(&'a Worker);
        impl Drop for Gate<'_> {
            fn drop(&mut self) {
                *self.0.gate.lock().unwrap_or_else(|e| e.into_inner()) = false;
                self.0.gate_free.notify_one();
            }
        }
        let _gate = Gate(w);
        let background = Ctx::background();
        let id = &w.destination.id;
        loop {
            if let Some(e) = ctx_err(&ctx) {
                return Err(e);
            }
            let (meta, record) = {
                let mut core = self.lock();
                self.usable(&core)?;
                let mut spool = self.spool(&mut core, &background);
                spool.expire()?;
                let cursor = spool.state.cursor(id);
                let Some(meta) = spool
                    .state
                    .records
                    .iter()
                    .find(|r| r.targets.contains(id) && cursor < r.sequence)
                    .cloned()
                else {
                    return Ok(());
                };
                match spool.read_record(&meta) {
                    Ok(record) => (meta, record),
                    Err(_) => {
                        core.corruptions += 1;
                        core.poisoned = true;
                        return Err(Error::spool("record_integrity", ErrorKind::SpoolCorrupt));
                    }
                }
            };
            let timeout = self
                .options
                .send_timeout
                .positive()
                .unwrap_or(Duration::from_secs(10));
            let send_ctx = ctx.with_timeout(timeout);
            let sent = self.connection(w, &send_ctx).and_then(|conn| {
                if fault(&self.options, "send").is_err() {
                    return Err(Error::transport("fault"));
                }
                send_link(&w.destination.network, &conn, &record, &send_ctx)
            });
            if sent.is_err() {
                self.close_conn(w);
                {
                    let mut core = self.lock();
                    if self.usable(&core).is_ok() {
                        let mut next = core.state.clone();
                        next.failed += 1;
                        self.spool(&mut core, &background).commit(next)?;
                    } else {
                        core.state.failed += 1;
                    }
                }
                if let Some(e) = ctx_err(&ctx) {
                    return Err(e);
                }
                return Err(Error::spool("transport", ErrorKind::DurableSpool));
            }
            let mut core = self.lock();
            self.usable(&core)?;
            let mut spool = self.spool(&mut core, &background);
            if spool.fault("checkpoint").is_err() {
                return Err(spool.disk_failure());
            }
            let mut next = spool.state.clone();
            let d = next.destinations.entry(id.clone()).or_default();
            d.cursor = d.cursor.max(meta.sequence);
            next.sent += 1;
            spool.commit(next)?;
            spool.expire()?;
        }
    }
    pub fn flush(&self, ctx: &Ctx) -> Result<()> {
        let results: Vec<Result<()>> = std::thread::scope(|scope| {
            let handles: Vec<_> = self
                .workers
                .values()
                .map(|w| scope.spawn(move || self.flush_worker(w, ctx)))
                .collect();
            handles
                .into_iter()
                .map(|h| {
                    h.join()
                        .unwrap_or_else(|_| Err(Error::spool("transport", ErrorKind::DurableSpool)))
                })
                .collect()
        });
        match Error::join(results.into_iter().filter_map(|r| r.err()).collect()) {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }
    pub fn run(&self, ctx: &Ctx) -> Error {
        let cancel = CancelToken::new();
        {
            let mut run = self.run.lock().unwrap_or_else(|e| e.into_inner());
            if run.started {
                return Error::spool("already_running", ErrorKind::DurableSpool);
            }
            if let Err(e) = self.usable(&self.lock()) {
                return e;
            }
            run.cancel = Some(cancel.clone());
            run.started = true;
            run.running = true;
        }
        let ctx = ctx.with_cancel(&cancel);
        let retry = self
            .options
            .retry_interval
            .positive()
            .unwrap_or(Duration::from_secs(5));
        let max_retry = self
            .options
            .max_retry_interval
            .positive()
            .unwrap_or(Duration::from_secs(60));
        let background = Ctx::background();
        std::thread::scope(|scope| {
            for w in self.workers.values() {
                let ctx = &ctx;
                scope.spawn(move || {
                    let mut wait = retry;
                    loop {
                        let failed = self.flush_worker(w, ctx).is_err();
                        if ctx.err().is_some() {
                            break;
                        }
                        let delay = if failed {
                            let delay = wait;
                            wait = (wait * 2).min(max_retry);
                            delay
                        } else {
                            wait = retry;
                            retry
                        };
                        let timer = Instant::now() + delay;
                        if failed {
                            while ctx.err().is_none() && Instant::now() < timer {
                                std::thread::sleep(
                                    Duration::from_millis(10)
                                        .min(timer.saturating_duration_since(Instant::now())),
                                );
                            }
                        } else {
                            w.wake.wait(ctx, Some(timer));
                        }
                        if ctx.err().is_some() {
                            break;
                        }
                    }
                    self.close_conn(w);
                });
            }
            let mut tick = Instant::now() + retry;
            while ctx.err().is_none() {
                std::thread::sleep(Duration::from_millis(10));
                if Instant::now() >= tick {
                    tick += retry;
                    let mut core = self.lock();
                    if self.usable(&core).is_ok() {
                        let _ = self.spool(&mut core, &background).expire();
                    }
                }
            }
            for w in self.workers.values() {
                self.close_conn(w);
            }
        });
        {
            let mut run = self.run.lock().unwrap_or_else(|e| e.into_inner());
            run.running = false;
        }
        self.run_done.notify_all();
        ctx_err(&ctx).unwrap_or_else(|| Error::sentinel(ErrorKind::Canceled))
    }
    pub fn close(&self) -> Result<()> {
        self.close_result
            .get_or_init(|| self.close_internal())
            .clone()
    }
    fn close_internal(&self) -> Result<()> {
        self.lifecycle.cancel();
        let cancel = self
            .run
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .cancel
            .clone();
        if let Some(cancel) = cancel {
            cancel.cancel();
        }
        for w in self.workers.values() {
            self.close_conn(w);
        }
        {
            let mut core = self.lock();
            if core.closed {
                return Ok(());
            }
            core.closed = true;
        }
        {
            let mut run = self.run.lock().unwrap_or_else(|e| e.into_inner());
            while run.running {
                run = self
                    .run_done
                    .wait_timeout(run, Duration::from_millis(20))
                    .unwrap_or_else(|e| e.into_inner())
                    .0;
            }
        }
        let mut core = self.lock();
        while core.active > 0 {
            core = self
                .idle
                .wait_timeout(core, Duration::from_millis(20))
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
        if let Some(lock) = core.lock.take()
            && super::privatefs::unlock_spool(lock).is_err()
        {
            return Err(Error::spool("unlock", ErrorKind::DurableSpool));
        }
        Ok(())
    }
}
impl Drop for DurableLogger {
    fn drop(&mut self) {
        let _ = self.close();
    }
}
impl DiskState {
    pub(crate) fn cursor(&self, id: &str) -> u64 {
        self.destinations.get(id).map_or(0, |d| d.cursor)
    }
}
