use crate::net::ul::{Ctx, Kind as CtxKind};
use std::collections::VecDeque;
use std::fmt;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::Duration;
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MemoryAdmissionStats {
    pub capacity: i64,
    pub held: i64,
    pub peak: i64,
    pub granted: i64,
    pub waited: i64,
    pub oversized: i64,
    pub canceled: i64,
    pub queued: i64,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdmissionError {
    Invalid(String),
    Context(CtxKind),
}
impl fmt::Display for AdmissionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AdmissionError::Invalid(detail) => {
                write!(f, "dicom: invalid memory admission: {detail}")
            }
            AdmissionError::Context(CtxKind::Canceled) => f.write_str("context canceled"),
            AdmissionError::Context(_) => f.write_str("context deadline exceeded"),
        }
    }
}
impl std::error::Error for AdmissionError {}
#[derive(Debug)]
struct Waiter {
    id: u64,
    bytes: i64,
}
#[derive(Debug, Default)]
struct State {
    held: i64,
    waiters: VecDeque<Waiter>,
    granted: Vec<u64>,
    next_id: u64,
    stats: MemoryAdmissionStats,
}
#[derive(Debug)]
struct Shared {
    capacity: i64,
    state: Mutex<State>,
    changed: Condvar,
}
#[derive(Debug, Clone)]
pub struct MemoryAdmission(Arc<Shared>);
#[derive(Debug)]
pub struct AdmissionGrant {
    shared: Arc<Shared>,
    bytes: i64,
    released: bool,
}
impl AdmissionGrant {
    pub fn release(&mut self) {
        if self.released {
            return;
        }
        self.released = true;
        let mut s = lock(&self.shared);
        s.held = s.held.saturating_sub(self.bytes);
        wake(&self.shared, &mut s);
    }
}
impl Drop for AdmissionGrant {
    fn drop(&mut self) {
        self.release();
    }
}
fn lock(shared: &Shared) -> MutexGuard<'_, State> {
    shared.state.lock().unwrap_or_else(|e| e.into_inner())
}
fn fits(capacity: i64, s: &State, bytes: i64) -> bool {
    s.held == 0 || bytes <= capacity.saturating_sub(s.held)
}
fn grant(capacity: i64, s: &mut State, bytes: i64) {
    s.held = s.held.saturating_add(bytes);
    s.stats.granted = s.stats.granted.saturating_add(1);
    if bytes > capacity {
        s.stats.oversized = s.stats.oversized.saturating_add(1);
    }
    s.stats.peak = s.stats.peak.max(s.held);
}
fn wake(shared: &Shared, s: &mut State) {
    let mut any = false;
    while let Some(w) = s.waiters.front() {
        if !fits(shared.capacity, s, w.bytes) {
            break;
        }
        let (id, bytes) = (w.id, w.bytes);
        s.waiters.pop_front();
        grant(shared.capacity, s, bytes);
        s.granted.push(id);
        any = true;
    }
    if any {
        shared.changed.notify_all();
    }
}
impl MemoryAdmission {
    pub fn new(capacity: i64) -> Result<MemoryAdmission, AdmissionError> {
        if capacity < 1 {
            return Err(AdmissionError::Invalid(format!("capacity {capacity}")));
        }
        Ok(MemoryAdmission(Arc::new(Shared {
            capacity,
            state: Mutex::new(State {
                stats: MemoryAdmissionStats {
                    capacity,
                    ..MemoryAdmissionStats::default()
                },
                ..State::default()
            }),
            changed: Condvar::new(),
        })))
    }
    fn grant_handle(&self, bytes: i64) -> AdmissionGrant {
        AdmissionGrant {
            shared: self.0.clone(),
            bytes,
            released: false,
        }
    }
    pub fn acquire(&self, ctx: &Ctx, bytes: i64) -> Result<AdmissionGrant, AdmissionError> {
        if bytes < 1 {
            return Err(AdmissionError::Invalid(format!("request of {bytes} bytes")));
        }
        if let Some(kind) = ctx.err() {
            return Err(AdmissionError::Context(kind));
        }
        let shared = &self.0;
        let mut s = lock(shared);
        if s.waiters.is_empty() && fits(shared.capacity, &s, bytes) {
            grant(shared.capacity, &mut s, bytes);
            return Ok(self.grant_handle(bytes));
        }
        let id = s.next_id;
        s.next_id = s.next_id.wrapping_add(1);
        s.waiters.push_back(Waiter { id, bytes });
        s.stats.waited = s.stats.waited.saturating_add(1);
        loop {
            if let Some(i) = s.granted.iter().position(|g| *g == id) {
                s.granted.swap_remove(i);
                return Ok(self.grant_handle(bytes));
            }
            if let Some(kind) = ctx.err() {
                if let Some(i) = s.waiters.iter().position(|w| w.id == id) {
                    s.waiters.remove(i);
                    s.stats.canceled = s.stats.canceled.saturating_add(1);
                    wake(shared, &mut s);
                    return Err(AdmissionError::Context(kind));
                }
                if let Some(i) = s.granted.iter().position(|g| *g == id) {
                    s.granted.swap_remove(i);
                }
                drop(s);
                self.grant_handle(bytes).release();
                return Err(AdmissionError::Context(kind));
            }
            s = shared
                .changed
                .wait_timeout(s, Duration::from_millis(2))
                .map(|(guard, _)| guard)
                .unwrap_or_else(|e| e.into_inner().0);
        }
    }
    pub fn stats(&self) -> MemoryAdmissionStats {
        let s = lock(&self.0);
        MemoryAdmissionStats {
            held: s.held,
            queued: s.waiters.len() as i64,
            ..s.stats
        }
    }
}
