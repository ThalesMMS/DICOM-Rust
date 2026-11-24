#![forbid(unsafe_code)]
use super::error::RenderError;
use super::parallel::{Cancellation, Reservation, ctx_err, interactive, reserve, workers};
use std::sync::{Condvar, Mutex, MutexGuard};
struct State {
    next: usize,
    limit: usize,
    active: usize,
    helpers: usize,
    closed: bool,
}
struct Crew {
    state: Mutex<State>,
    work: Condvar,
    idle: Condvar,
}
impl Crew {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
    fn claim(state: &mut State) -> Option<usize> {
        (state.next < state.limit).then(|| {
            let item = state.next;
            state.next += 1;
            state.active += 1;
            item
        })
    }
    fn finished(&self) {
        let mut state = self.lock();
        state.active -= 1;
        if state.active == 0 {
            self.idle.notify_all();
        }
    }
}
struct InFlight<'a>(&'a Crew);
impl Drop for InFlight<'_> {
    fn drop(&mut self) {
        self.0.finished();
    }
}
struct Close<'a>(&'a Crew);
impl Drop for Close<'_> {
    fn drop(&mut self) {
        self.0.lock().closed = true;
        self.0.work.notify_all();
    }
}
struct Seat<'a> {
    crew: &'a Crew,
    _slot: Reservation,
}
impl Drop for Seat<'_> {
    fn drop(&mut self) {
        self.crew.lock().helpers -= 1;
    }
}
pub(crate) fn run_batches_background(
    ctx: Option<&dyn Cancellation>,
    count: usize,
    requested: i64,
    item: &(dyn Fn(usize) + Sync),
    batch_end: &mut dyn FnMut(usize) -> usize,
    after: &mut dyn FnMut(usize, usize) -> Result<(), RenderError>,
) -> Result<(), RenderError> {
    if count == 0 {
        return ctx_err(ctx).map_or(Ok(()), Err);
    }
    let wanted = usize::try_from(workers(requested))
        .unwrap_or(1)
        .min(count)
        .saturating_sub(1);
    let crew = Crew {
        state: Mutex::new(State {
            next: 0,
            limit: 0,
            active: 0,
            helpers: 0,
            closed: false,
        }),
        work: Condvar::new(),
        idle: Condvar::new(),
    };
    let crew = &crew;
    let run = |index: usize| {
        let _in_flight = InFlight(crew);
        if ctx_err(ctx).is_none() {
            item(index);
        }
    };
    let run = &run;
    std::thread::scope(|scope| {
        let _close = Close(crew);
        let helper = move |seat: Seat<'_>| {
            let _seat = seat;
            loop {
                let claimed = {
                    let mut state = crew.lock();
                    loop {
                        if state.closed || interactive() {
                            break None;
                        }
                        if let Some(index) = Crew::claim(&mut state) {
                            break Some(index);
                        }
                        state = crew.work.wait(state).unwrap_or_else(|e| e.into_inner());
                    }
                };
                match claimed {
                    Some(index) => run(index),
                    None => return,
                }
            }
        };
        let recruit = |open: usize| {
            if interactive() {
                return;
            }
            loop {
                {
                    let state = crew.lock();
                    if state.helpers >= wanted.min(open.saturating_sub(1)) {
                        return;
                    }
                }
                let slot = reserve(2);
                if slot.workers() < 2 {
                    return;
                }
                crew.lock().helpers += 1;
                let seat = Seat { crew, _slot: slot };
                scope.spawn(move || helper(seat));
            }
        };
        let mut start = 0;
        while start < count {
            if let Some(e) = ctx_err(ctx) {
                return Err(e);
            }
            let end = batch_end(start).clamp(start + 1, count);
            recruit(end - start);
            crew.lock().limit = end;
            crew.work.notify_all();
            loop {
                let claimed = Crew::claim(&mut crew.lock());
                match claimed {
                    Some(index) => run(index),
                    None => break,
                }
                recruit(end - start);
            }
            {
                let mut state = crew.lock();
                while state.active > 0 {
                    state = crew.idle.wait(state).unwrap_or_else(|e| e.into_inner());
                }
            }
            if let Some(e) = ctx_err(ctx) {
                return Err(e);
            }
            after(start, end)?;
            start = end;
        }
        Ok(())
    })
}
#[cfg(test)]
mod tests;
