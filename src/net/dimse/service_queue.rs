#![forbid(unsafe_code)]
use super::commitment_workflow::StorageCommitmentTime;
use super::controls::context_err;
use super::error::{Error, Kind, Result};
use crate::net::ul::{self, CancelToken, Ctx, Nanos};
use std::collections::{BTreeMap, VecDeque};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;
pub type ServiceJob = Box<dyn FnOnce(&Ctx) + Send>;
#[derive(Clone, Default)]
pub struct ServiceQueueOptions {
    pub context: Option<Ctx>,
    pub max_concurrent_associations: i64,
    pub max_active_operations: i64,
    pub queue_depth: i64,
    pub enqueue_timeout: Nanos,
    pub max_in_flight_bytes: i64,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ServiceQueueMetrics {
    pub active: i64,
    pub queued: i64,
    pub in_flight_bytes: i64,
    pub peak_in_flight_bytes: i64,
    pub admitted: u64,
    pub completed: u64,
    pub rejected: u64,
    pub job_panics: u64,
}
struct Queued {
    ctx: Ctx,
    job: ServiceJob,
    bytes: i64,
}
#[derive(Default)]
struct State {
    admitted: usize,
    jobs: VecDeque<Queued>,
    closed: bool,
    in_flight_bytes: i64,
    live_workers: usize,
}
struct Inner {
    ctx: Ctx,
    token: CancelToken,
    enqueue_timeout: Option<Duration>,
    capacity: usize,
    max_bytes: i64,
    state: Mutex<State>,
    cv: Condvar,
    active: AtomicI64,
    queued: AtomicI64,
    admitted: AtomicU64,
    completed: AtomicU64,
    rejected: AtomicU64,
    job_panics: AtomicU64,
    peak_bytes: AtomicI64,
}
#[derive(Clone)]
pub struct ServiceQueue {
    inner: Arc<Inner>,
}
const POLL: Duration = Duration::from_millis(2);
fn queue_context_error(ctx: &Ctx) -> Option<Error> {
    match ctx.err() {
        Some(ul::Kind::DeadlineExceeded) => Some(Error::sentinel(Kind::ServiceQueueFull)),
        Some(_) => context_err(ctx),
        None => None,
    }
}
impl ServiceQueue {
    pub fn new(options: ServiceQueueOptions) -> Result<ServiceQueue> {
        if options.max_active_operations <= 0 {
            return Err(Error::new(
                "dicom dimse: MaxActiveOperations must be positive",
            ));
        }
        if options.max_concurrent_associations < 0 {
            return Err(Error::new(
                "dicom dimse: MaxConcurrentAssociations must not be negative",
            ));
        }
        if options.queue_depth < 0 {
            return Err(Error::new("dicom dimse: QueueDepth must not be negative"));
        }
        if options.enqueue_timeout.is_negative() {
            return Err(Error::new(
                "dicom dimse: EnqueueTimeout must not be negative",
            ));
        }
        if options.max_in_flight_bytes < 0 {
            return Err(Error::new(
                "dicom dimse: MaxInFlightBytes must not be negative",
            ));
        }
        let token = CancelToken::new();
        let ctx = options
            .context
            .clone()
            .unwrap_or_default()
            .with_cancel(&token);
        let mut capacity = (options.max_active_operations + options.queue_depth) as usize;
        if options.max_concurrent_associations > 0
            && (options.max_concurrent_associations as usize) < capacity
        {
            capacity = options.max_concurrent_associations as usize;
        }
        let workers = options.max_active_operations as usize;
        let inner = Arc::new(Inner {
            ctx,
            token,
            enqueue_timeout: options.enqueue_timeout.positive(),
            capacity,
            max_bytes: options.max_in_flight_bytes,
            state: Mutex::new(State {
                live_workers: workers,
                ..Default::default()
            }),
            cv: Condvar::new(),
            active: AtomicI64::new(0),
            queued: AtomicI64::new(0),
            admitted: AtomicU64::new(0),
            completed: AtomicU64::new(0),
            rejected: AtomicU64::new(0),
            job_panics: AtomicU64::new(0),
            peak_bytes: AtomicI64::new(0),
        });
        for _ in 0..workers {
            let q = inner.clone();
            std::thread::spawn(move || worker(q));
        }
        Ok(ServiceQueue { inner })
    }
    pub fn enqueue(&self, ctx: Option<&Ctx>, job: ServiceJob) -> Result<()> {
        self.enqueue_bytes(ctx, 0, job)
    }
    pub fn enqueue_bytes(
        &self,
        ctx: Option<&Ctx>,
        estimated_bytes: i64,
        job: ServiceJob,
    ) -> Result<()> {
        let q = &self.inner;
        if estimated_bytes < 0 {
            return Err(Error::new(
                "dicom dimse: estimated in-flight bytes must not be negative",
            ));
        }
        let ctx = ctx.cloned().unwrap_or_default();
        if q.state.lock().unwrap().closed {
            return Err(Error::sentinel(Kind::ServiceQueueClosed));
        }
        let enqueue_ctx = match q.enqueue_timeout {
            Some(t) => ctx.with_timeout(t),
            None => ctx.clone(),
        };
        if let Err(e) = self.acquire(&enqueue_ctx, estimated_bytes) {
            q.rejected.fetch_add(1, Ordering::SeqCst);
            return Err(e);
        }
        let mut st = q.state.lock().unwrap();
        if st.closed {
            release_locked(q, &mut st, estimated_bytes);
            return Err(Error::sentinel(Kind::ServiceQueueClosed));
        }
        q.queued.fetch_add(1, Ordering::SeqCst);
        q.admitted.fetch_add(1, Ordering::SeqCst);
        st.jobs.push_back(Queued {
            ctx: ctx.with_end_of(&q.ctx),
            job,
            bytes: estimated_bytes,
        });
        q.cv.notify_all();
        Ok(())
    }
    fn acquire(&self, ctx: &Ctx, bytes: i64) -> Result<()> {
        let q = &self.inner;
        let check = || queue_context_error(ctx).or_else(|| queue_context_error(&q.ctx));
        if let Some(e) = check() {
            return Err(e);
        }
        let mut st = q.state.lock().unwrap();
        loop {
            if st.admitted < q.capacity {
                if let Some(e) = check() {
                    return Err(e);
                }
                st.admitted += 1;
                break;
            }
            if q.enqueue_timeout.is_none() {
                return Err(Error::sentinel(Kind::ServiceQueueFull));
            }
            if let Some(e) = check() {
                return Err(e);
            }
            st = q.cv.wait_timeout(st, POLL).unwrap().0;
        }
        if q.max_bytes == 0 || bytes == 0 {
            return Ok(());
        }
        if bytes > q.max_bytes {
            st.admitted -= 1;
            q.cv.notify_all();
            return Err(Error::sentinel(Kind::ServiceQueueFull));
        }
        loop {
            if let Some(e) = check() {
                st.admitted -= 1;
                q.cv.notify_all();
                return Err(e);
            }
            if st.in_flight_bytes <= q.max_bytes - bytes {
                st.in_flight_bytes += bytes;
                q.peak_bytes.fetch_max(st.in_flight_bytes, Ordering::SeqCst);
                return Ok(());
            }
            if q.enqueue_timeout.is_none() {
                st.admitted -= 1;
                q.cv.notify_all();
                return Err(Error::sentinel(Kind::ServiceQueueFull));
            }
            st = q.cv.wait_timeout(st, POLL).unwrap().0;
        }
    }
    pub fn close(&self, ctx: Option<&Ctx>) -> Result<()> {
        let q = &self.inner;
        let ctx = ctx.cloned().unwrap_or_default();
        {
            let mut st = q.state.lock().unwrap();
            if !st.closed {
                st.closed = true;
                q.token.cancel();
            }
            q.cv.notify_all();
        }
        let mut st = q.state.lock().unwrap();
        while st.live_workers > 0 {
            if let Some(e) = context_err(&ctx) {
                return Err(e);
            }
            st = q.cv.wait_timeout(st, POLL).unwrap().0;
        }
        Ok(())
    }
    pub fn snapshot(&self) -> ServiceQueueMetrics {
        let q = &self.inner;
        let in_flight_bytes = q.state.lock().unwrap().in_flight_bytes;
        ServiceQueueMetrics {
            active: q.active.load(Ordering::SeqCst),
            queued: q.queued.load(Ordering::SeqCst),
            in_flight_bytes,
            peak_in_flight_bytes: q.peak_bytes.load(Ordering::SeqCst),
            admitted: q.admitted.load(Ordering::SeqCst),
            completed: q.completed.load(Ordering::SeqCst),
            rejected: q.rejected.load(Ordering::SeqCst),
            job_panics: q.job_panics.load(Ordering::SeqCst),
        }
    }
}
fn release_locked(q: &Inner, st: &mut State, bytes: i64) {
    if q.max_bytes != 0 && bytes != 0 {
        st.in_flight_bytes -= bytes;
    }
    st.admitted -= 1;
    q.cv.notify_all();
}
fn worker(q: Arc<Inner>) {
    loop {
        let next = {
            let mut st = q.state.lock().unwrap();
            loop {
                if let Some(j) = st.jobs.pop_front() {
                    break Some(j);
                }
                if st.closed {
                    break None;
                }
                st = q.cv.wait(st).unwrap();
            }
        };
        let Some(queued) = next else {
            break;
        };
        q.queued.fetch_sub(1, Ordering::SeqCst);
        q.active.fetch_add(1, Ordering::SeqCst);
        if q.ctx.err().is_none() && queued.ctx.err().is_none() {
            let ctx = queued.ctx.clone();
            if catch_unwind(AssertUnwindSafe(|| (queued.job)(&ctx))).is_err() {
                q.job_panics.fetch_add(1, Ordering::SeqCst);
            }
        }
        q.active.fetch_sub(1, Ordering::SeqCst);
        q.completed.fetch_add(1, Ordering::SeqCst);
        let mut st = q.state.lock().unwrap();
        release_locked(&q, &mut st, queued.bytes);
    }
    let mut st = q.state.lock().unwrap();
    st.live_workers -= 1;
    q.cv.notify_all();
}
#[derive(Debug, Clone, Copy, Default)]
pub struct StudyInactivityOptions {
    pub timeout: Nanos,
    pub max_studies: i64,
    pub max_bytes: i64,
}
#[derive(Debug)]
struct Activity {
    key: (String, String),
    generation: u64,
    count: u64,
    pending: i64,
    last: StorageCommitmentTime,
    path: String,
    bytes: i64,
}
#[derive(Default)]
struct Tracker {
    states: BTreeMap<(String, String), Activity>,
    next: u64,
    bytes: i64,
}
#[derive(Clone)]
pub struct StudyInactivityTracker {
    options: StudyInactivityOptions,
    inner: Arc<Mutex<Tracker>>,
}
pub struct StudyActivityReservation {
    tracker: StudyInactivityTracker,
    key: (String, String),
    generation: u64,
    done: Mutex<bool>,
    new_generation: bool,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StudyInactiveEvent {
    pub called_ae_title: String,
    pub study_instance_uid: String,
    pub last_path: String,
    pub generation: u64,
    pub count: u64,
    pub last_commit: StorageCommitmentTime,
}
fn input_error() -> Error {
    Error::sentinel(Kind::StudyActivityInput)
}
fn limit_error() -> Error {
    Error::sentinel(Kind::StudyActivityLimit)
}
const NANOS_PER_SECOND: i64 = 1_000_000_000;
impl StudyInactivityTracker {
    pub fn new(mut opts: StudyInactivityOptions) -> Result<StudyInactivityTracker> {
        if opts.timeout.is_zero() {
            opts.timeout = Nanos(30 * NANOS_PER_SECOND);
        }
        if opts.max_studies == 0 {
            opts.max_studies = 128;
        }
        if opts.max_bytes == 0 {
            opts.max_bytes = 1 << 20;
        }
        if opts.timeout.0 <= 0
            || opts.timeout.0 > 24 * 3600 * NANOS_PER_SECOND
            || opts.max_studies <= 0
            || opts.max_studies > 4096
            || opts.max_bytes <= 0
            || opts.max_bytes > 64 << 20
        {
            return Err(input_error());
        }
        Ok(StudyInactivityTracker {
            options: opts,
            inner: Arc::new(Mutex::new(Tracker::default())),
        })
    }
    pub fn reserve(
        &self,
        ctx: Option<&Ctx>,
        called_ae: &str,
        study_uid: &str,
    ) -> Result<StudyActivityReservation> {
        let ctx = ctx.cloned().unwrap_or_default();
        if let Some(e) = context_err(&ctx) {
            return Err(e);
        }
        if called_ae.len() > 16
            || ul::validate_ae_title(called_ae).is_err()
            || called_ae != ul::normalize_ae_title(called_ae)
            || study_uid.len() > 64
            || !crate::object::is_valid_uid(study_uid.as_bytes())
        {
            return Err(input_error());
        }
        let key = (called_ae.to_string(), study_uid.to_string());
        let mut t = self.inner.lock().unwrap();
        if let Some(e) = context_err(&ctx) {
            return Err(e);
        }
        let is_new = !t.states.contains_key(&key);
        if is_new {
            let cost = 8192 + 128 + called_ae.len() as i64 + study_uid.len() as i64;
            if t.states.len() as i64 >= self.options.max_studies
                || cost > self.options.max_bytes - t.bytes
                || t.next == u64::MAX
            {
                return Err(limit_error());
            }
            t.next += 1;
            let generation = t.next;
            t.states.insert(
                key.clone(),
                Activity {
                    key: key.clone(),
                    generation,
                    count: 0,
                    pending: 0,
                    last: StorageCommitmentTime::ZERO,
                    path: String::new(),
                    bytes: cost,
                },
            );
            t.bytes += cost;
        }
        let s = t.states.get_mut(&key).unwrap();
        if s.pending == i64::MAX || s.count == u64::MAX || s.pending as u64 >= u64::MAX - s.count {
            return Err(limit_error());
        }
        s.pending += 1;
        Ok(StudyActivityReservation {
            tracker: self.clone(),
            key,
            generation: s.generation,
            done: Mutex::new(false),
            new_generation: is_new,
        })
    }
    pub fn poll(
        &self,
        ctx: Option<&Ctx>,
        now: StorageCommitmentTime,
    ) -> Result<Vec<StudyInactiveEvent>> {
        let ctx = ctx.cloned().unwrap_or_default();
        if let Some(e) = context_err(&ctx) {
            return Err(e);
        }
        if now.is_zero() {
            return Err(input_error());
        }
        let mut t = self.inner.lock().unwrap();
        let timeout = self.options.timeout.0 as i128;
        let mut due: Vec<(StorageCommitmentTime, u64, (String, String))> = t
            .states
            .values()
            .filter(|s| s.pending == 0 && s.count > 0 && sub_nanos(now, s.last) >= timeout)
            .map(|s| (s.last, s.generation, s.key.clone()))
            .collect();
        due.sort();
        if let Some(e) = context_err(&ctx) {
            return Err(e);
        }
        let mut events = Vec::with_capacity(due.len());
        for (_, _, key) in due {
            let s = t.states.remove(&key).unwrap();
            t.bytes -= s.bytes;
            events.push(StudyInactiveEvent {
                called_ae_title: s.key.0,
                study_instance_uid: s.key.1,
                last_path: s.path,
                generation: s.generation,
                count: s.count,
                last_commit: s.last,
            });
        }
        Ok(events)
    }
}
fn sub_nanos(a: StorageCommitmentTime, b: StorageCommitmentTime) -> i128 {
    (a.unix_nanos() - b.unix_nanos()).clamp(i64::MIN as i128, i64::MAX as i128)
}
impl StudyActivityReservation {
    pub fn new_generation(&self) -> bool {
        self.new_generation
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }
    pub fn generation_active(&self) -> bool {
        let t = self.tracker.inner.lock().unwrap();
        t.states
            .get(&self.key)
            .is_some_and(|s| s.generation == self.generation)
    }
    pub fn commit(&self, published_path: &str, now: StorageCommitmentTime) -> Result<()> {
        if published_path.len() > 8192
            || published_path.is_empty()
            || published_path.contains('\0')
            || now.is_zero()
        {
            return Err(input_error());
        }
        let mut t = self.tracker.inner.lock().unwrap();
        let mut done = self.done.lock().unwrap();
        if *done {
            return Err(input_error());
        }
        let Some(s) = t
            .states
            .get_mut(&self.key)
            .filter(|s| s.generation == self.generation)
        else {
            return Err(input_error());
        };
        if !s.last.is_zero() && now < s.last {
            return Err(input_error());
        }
        s.path = published_path.to_string();
        s.last = now;
        s.count += 1;
        s.pending -= 1;
        *done = true;
        Ok(())
    }
    pub fn abort(&self) {
        let mut t = self.tracker.inner.lock().unwrap();
        let mut done = self.done.lock().unwrap();
        if *done {
            return;
        }
        *done = true;
        let remove = match t
            .states
            .get_mut(&self.key)
            .filter(|s| s.generation == self.generation)
        {
            Some(s) => {
                s.pending -= 1;
                s.pending == 0 && s.count == 0
            }
            None => false,
        };
        if remove && let Some(s) = t.states.remove(&self.key) {
            t.bytes -= s.bytes;
        }
    }
}
