#![forbid(unsafe_code)]
use super::error::{ErrorKind, RenderError};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
pub trait Cancellation: Sync {
    fn err(&self) -> Option<RenderError>;
}
#[derive(Debug, Default)]
pub struct CancelToken {
    canceled: AtomicBool,
    deadline: Option<Instant>,
}
impl CancelToken {
    pub fn new() -> CancelToken {
        CancelToken::default()
    }
    pub fn with_deadline(deadline: Instant) -> CancelToken {
        CancelToken {
            canceled: AtomicBool::new(false),
            deadline: Some(deadline),
        }
    }
    pub fn cancel(&self) {
        self.canceled.store(true, Ordering::SeqCst);
    }
}
impl Cancellation for CancelToken {
    fn err(&self) -> Option<RenderError> {
        if self.canceled.load(Ordering::SeqCst) {
            return Some(RenderError::new(ErrorKind::Canceled, "context canceled"));
        }
        if self.deadline.is_some_and(|d| Instant::now() >= d) {
            return Some(RenderError::new(
                ErrorKind::DeadlineExceeded,
                "context deadline exceeded",
            ));
        }
        None
    }
}
pub fn ctx_err(ctx: Option<&dyn Cancellation>) -> Option<RenderError> {
    ctx.and_then(|c| c.err())
}
static EXPLICIT_BUDGET: AtomicI64 = AtomicI64::new(-1);
struct Slots {
    in_use: AtomicI64,
    peak: AtomicI64,
}
static SLOTS: Slots = Slots::new();
fn default_parallelism() -> i64 {
    static N: OnceLock<i64> = OnceLock::new();
    *N.get_or_init(|| std::thread::available_parallelism().map_or(1, |n| n.get() as i64))
}
pub fn budget() -> i64 {
    let b = EXPLICIT_BUDGET.load(Ordering::SeqCst);
    if b >= 0 {
        return b;
    }
    (default_parallelism() - 1).max(0)
}
pub fn set_budget(n: i64) -> i64 {
    EXPLICIT_BUDGET.swap(if n < 0 { -1 } else { n }, Ordering::SeqCst)
}
pub(crate) fn default_workers() -> i64 {
    budget() + 1
}
pub fn workers(requested: i64) -> i64 {
    if requested > 0 {
        requested
    } else {
        default_workers()
    }
}
pub fn in_use() -> i64 {
    SLOTS.in_use()
}
pub fn peak() -> i64 {
    SLOTS.peak.load(Ordering::SeqCst)
}
#[doc(hidden)]
pub fn reset_peak() {
    SLOTS.peak.store(SLOTS.in_use(), Ordering::SeqCst);
}
impl Slots {
    const fn new() -> Slots {
        Slots {
            in_use: AtomicI64::new(0),
            peak: AtomicI64::new(0),
        }
    }
    fn in_use(&self) -> i64 {
        self.in_use.load(Ordering::SeqCst)
    }
    fn try_reserve(&self, n: i64) -> i64 {
        if n <= 0 {
            return 0;
        }
        loop {
            let used = self.in_use.load(Ordering::SeqCst);
            let free = budget() - used;
            if free <= 0 {
                return 0;
            }
            let got = n.min(free);
            if self
                .in_use
                .compare_exchange(used, used + got, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                self.peak.fetch_max(used + got, Ordering::SeqCst);
                return got;
            }
        }
    }
    fn release(&self, n: i64) {
        if n > 0 {
            self.in_use.fetch_sub(n, Ordering::SeqCst);
        }
    }
}
pub struct Reservation {
    helpers: i64,
    released: bool,
}
impl Reservation {
    pub fn workers(&self) -> i64 {
        self.helpers + 1
    }
    pub fn done(mut self) {
        self.release_once();
    }
    fn release_once(&mut self) {
        if !self.released {
            self.released = true;
            SLOTS.release(self.helpers);
        }
    }
}
impl Drop for Reservation {
    fn drop(&mut self) {
        self.release_once();
    }
}
pub fn reserve(want: i64) -> Reservation {
    Reservation {
        helpers: SLOTS.try_reserve(want - 1),
        released: false,
    }
}
pub fn run<F, W>(
    ctx: Option<&dyn Cancellation>,
    count: i64,
    workers: i64,
    chunk: i64,
    new_worker: F,
) -> Result<(), RenderError>
where
    F: Fn() -> W + Sync,
    W: FnMut(i64),
{
    run_inner(&SLOTS, ctx, count, workers, chunk, &new_worker, false)
}
pub(crate) fn run_background<F, W>(
    ctx: Option<&dyn Cancellation>,
    count: i64,
    workers: i64,
    chunk: i64,
    new_worker: F,
) -> Result<(), RenderError>
where
    F: Fn() -> W + Sync,
    W: FnMut(i64),
{
    run_inner(&SLOTS, ctx, count, workers, chunk, &new_worker, true)
}
#[doc(hidden)]
pub fn do_items<F>(
    ctx: Option<&dyn Cancellation>,
    count: i64,
    workers: i64,
    f: F,
) -> Result<(), RenderError>
where
    F: Fn(i64) + Sync,
{
    run(ctx, count, workers, 1, || &f)
}
fn as_result(ctx: Option<&dyn Cancellation>) -> Result<(), RenderError> {
    match ctx_err(ctx) {
        Some(e) => Err(e),
        None => Ok(()),
    }
}
fn run_inner<F, W>(
    slots: &Slots,
    ctx: Option<&dyn Cancellation>,
    count: i64,
    workers: i64,
    chunk: i64,
    new_worker: &F,
    background: bool,
) -> Result<(), RenderError>
where
    F: Fn() -> W + Sync,
    W: FnMut(i64),
{
    if count <= 0 {
        return as_result(ctx);
    }
    let workers = self::workers(workers).min(count).max(1);
    let chunk = if chunk <= 0 {
        (count / (workers * 8)).max(1)
    } else {
        chunk
    };
    let chunks = (count + chunk - 1) / chunk;
    let wanted = workers.min(chunks) - 1;
    let next = AtomicI64::new(0);
    let canceled = || ctx.is_some_and(|c| c.err().is_some());
    let claim = |worker: &mut Option<W>| -> bool {
        let end = next.fetch_add(chunk, Ordering::SeqCst) + chunk;
        let start = end - chunk;
        if start >= count {
            return false;
        }
        let item_fn = worker.get_or_insert_with(new_worker);
        for item in start..end.min(count) {
            if canceled() {
                return false;
            }
            item_fn(item);
        }
        true
    };
    let running = AtomicI64::new(0);
    std::thread::scope(|scope| {
        let helper = || {
            struct Leave<'a>(&'a AtomicI64, &'a Slots);
            impl Drop for Leave<'_> {
                fn drop(&mut self) {
                    self.0.fetch_sub(1, Ordering::SeqCst);
                    self.1.release(1);
                }
            }
            let _leave = Leave(&running, slots);
            let mut worker: Option<W> = None;
            while !canceled() {
                if (background && interactive()) || !claim(&mut worker) {
                    break;
                }
            }
        };
        let spawn = || {
            if background && interactive() {
                return;
            }
            let got = slots.try_reserve(wanted - running.load(Ordering::SeqCst));
            running.fetch_add(got, Ordering::SeqCst);
            for _ in 0..got {
                scope.spawn(helper);
            }
        };
        spawn();
        let mut worker: Option<W> = None;
        while !canceled() {
            if background && running.load(Ordering::SeqCst) < wanted {
                spawn();
            }
            if !claim(&mut worker) {
                break;
            }
        }
    });
    as_result(ctx)
}
const INTERACTIVE_QUIET: Duration = Duration::from_millis(150);
static INTERACTIVE_HOLDS: AtomicI64 = AtomicI64::new(0);
fn interactive_until() -> &'static Mutex<Option<Instant>> {
    static UNTIL: OnceLock<Mutex<Option<Instant>>> = OnceLock::new();
    UNTIL.get_or_init(|| Mutex::new(None))
}
pub struct InteractiveGuard {
    ended: bool,
}
impl InteractiveGuard {
    pub fn end(mut self) {
        self.end_once();
    }
    fn end_once(&mut self) {
        if !self.ended {
            self.ended = true;
            if let Ok(mut until) = interactive_until().lock() {
                *until = Some(Instant::now() + INTERACTIVE_QUIET);
            }
            INTERACTIVE_HOLDS.fetch_sub(1, Ordering::SeqCst);
        }
    }
}
impl Drop for InteractiveGuard {
    fn drop(&mut self) {
        self.end_once();
    }
}
#[doc(hidden)]
pub fn begin_interactive() -> InteractiveGuard {
    INTERACTIVE_HOLDS.fetch_add(1, Ordering::SeqCst);
    InteractiveGuard { ended: false }
}
pub fn interactive() -> bool {
    if INTERACTIVE_HOLDS.load(Ordering::SeqCst) > 0 {
        return true;
    }
    interactive_until()
        .lock()
        .ok()
        .and_then(|until| *until)
        .is_some_and(|until| Instant::now() < until)
}
const PARALLEL_ROWS_MIN_ROWS: i64 = 32;
const PARALLEL_ROW_CHUNKS_PER_WORKER: i64 = 8;
pub(crate) fn parallel_rows<F>(row_count: i64, render_row: F)
where
    F: Fn(i64) + Sync,
{
    run_parallel_rows(None, row_count, &|| &render_row);
}
pub(crate) fn parallel_rows_worker_context<F, W>(
    ctx: Option<&dyn Cancellation>,
    row_count: i64,
    new_worker: F,
) -> Result<(), RenderError>
where
    F: Fn() -> W + Sync,
    W: FnMut(i64),
{
    run_parallel_rows(ctx, row_count, &new_worker);
    as_result(ctx)
}
fn run_parallel_rows<F, W>(ctx: Option<&dyn Cancellation>, row_count: i64, new_worker: &F)
where
    F: Fn() -> W + Sync,
    W: FnMut(i64),
{
    if row_count <= 0 {
        return;
    }
    let mut workers = default_workers();
    if row_count < PARALLEL_ROWS_MIN_ROWS {
        workers = 1;
    }
    let workers = workers.min(row_count);
    let _ = run_inner(
        &SLOTS,
        ctx,
        row_count,
        workers,
        parallel_row_chunk(row_count, workers),
        new_worker,
        false,
    );
}
fn parallel_row_chunk(row_count: i64, workers: i64) -> i64 {
    let chunks = workers * PARALLEL_ROW_CHUNKS_PER_WORKER;
    ((row_count + chunks - 1) / chunks).max(1)
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU64;
    fn run_on<F, W>(slots: &Slots, count: i64, new_worker: F) -> Result<(), RenderError>
    where
        F: Fn() -> W + Sync,
        W: FnMut(i64),
    {
        run_inner(slots, None, count, 0, 0, &new_worker, false)
    }
    #[test]
    fn every_item_runs_once() {
        let slots = Slots::new();
        let hits: Vec<AtomicU64> = (0..1000).map(|_| AtomicU64::new(0)).collect();
        run_on(&slots, 1000, || {
            |i: i64| {
                hits[i as usize].fetch_add(1, Ordering::SeqCst);
            }
        })
        .unwrap();
        assert!(hits.iter().all(|h| h.load(Ordering::SeqCst) == 1));
        assert_eq!(slots.in_use(), 0, "every helper slot went back");
    }
    #[test]
    fn the_process_count_includes_other_calls() {
        let held = reserve(3);
        if held.workers() == 1 {
            return;
        }
        let slots = Slots::new();
        run_on(&slots, 100, || |_: i64| {}).unwrap();
        assert_eq!(slots.in_use(), 0);
        assert!(in_use() >= held.workers() - 1);
        held.done();
    }
    #[test]
    fn a_panicking_helper_returns_its_slot() {
        if budget() == 0 {
            return;
        }
        let slots = Slots::new();
        let caller = std::thread::current().id();
        let helper_ran = AtomicBool::new(false);
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            run_on(&slots, 64, || {
                |_: i64| {
                    if std::thread::current().id() != caller {
                        helper_ran.store(true, Ordering::SeqCst);
                        panic!("a helper's item panics");
                    }
                    let start = Instant::now();
                    while !helper_ran.load(Ordering::SeqCst)
                        && start.elapsed() < Duration::from_secs(10)
                    {
                        std::thread::yield_now();
                    }
                }
            })
        }));
        assert!(outcome.is_err(), "the helper's panic reaches the caller");
        assert!(helper_ran.load(Ordering::SeqCst));
        assert_eq!(slots.in_use(), 0, "the panicking helper's slot went back");
    }
    #[test]
    fn canceled_context_reports() {
        let token = CancelToken::new();
        token.cancel();
        let err = do_items(Some(&token), 10, 2, |_| {}).unwrap_err();
        assert!(err.is(ErrorKind::Canceled));
        assert_eq!(err.to_string(), "context canceled");
    }
}
