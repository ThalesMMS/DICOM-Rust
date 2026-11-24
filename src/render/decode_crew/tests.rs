use super::run_batches_background;
use crate::render::error::RenderError;
use crate::render::parallel::{CancelToken, Cancellation};
use std::sync::atomic::{AtomicUsize, Ordering};
fn by(size: usize) -> impl FnMut(usize) -> usize {
    move |start| start + size
}
#[test]
fn every_item_runs_once_and_batches_do_not_overlap() {
    const COUNT: usize = 103;
    let runs: Vec<AtomicUsize> = (0..COUNT).map(|_| AtomicUsize::new(0)).collect();
    let (done, running) = (AtomicUsize::new(0), AtomicUsize::new(0));
    let mut batches = Vec::new();
    let item = |i: usize| {
        running.fetch_add(1, Ordering::SeqCst);
        runs[i].fetch_add(1, Ordering::SeqCst);
        std::thread::yield_now();
        done.fetch_add(1, Ordering::SeqCst);
        running.fetch_sub(1, Ordering::SeqCst);
    };
    let result = run_batches_background(None, COUNT, 4, &item, &mut by(10), &mut |start, end| {
        assert_eq!(running.load(Ordering::SeqCst), 0);
        assert_eq!(done.load(Ordering::SeqCst), end);
        assert!(runs[end..].iter().all(|r| r.load(Ordering::SeqCst) == 0));
        batches.push((start, end));
        Ok(())
    });
    assert!(result.is_ok());
    assert!(runs.iter().all(|r| r.load(Ordering::SeqCst) == 1));
    let expected: Vec<(usize, usize)> = (0..COUNT)
        .step_by(10)
        .map(|s| (s, (s + 10).min(COUNT)))
        .collect();
    assert_eq!(batches, expected);
}
#[test]
fn a_batch_end_that_does_not_advance_still_makes_progress() {
    let ran = AtomicUsize::new(0);
    let item = |_: usize| {
        ran.fetch_add(1, Ordering::SeqCst);
    };
    let mut afters = 0;
    let result = run_batches_background(None, 5, 3, &item, &mut |_| 0, &mut |start, end| {
        assert_eq!(end, start + 1);
        afters += 1;
        Ok(())
    });
    assert!(result.is_ok());
    assert_eq!((ran.load(Ordering::SeqCst), afters), (5, 5));
}
#[test]
fn nothing_to_run_reports_the_context() {
    let item = |_: usize| panic!("no item");
    let ok = run_batches_background(None, 0, 0, &item, &mut by(4), &mut |_, _| Ok(()));
    assert!(ok.is_ok());
    let token = CancelToken::new();
    token.cancel();
    let ctx: Option<&dyn Cancellation> = Some(&token);
    let canceled = run_batches_background(ctx, 0, 0, &item, &mut by(4), &mut |_, _| Ok(()));
    assert!(canceled.is_err());
}
#[test]
fn an_error_after_a_batch_ends_the_call() {
    let ran = AtomicUsize::new(0);
    let item = |_: usize| {
        ran.fetch_add(1, Ordering::SeqCst);
    };
    let result = run_batches_background(None, 40, 4, &item, &mut by(8), &mut |_, end| {
        if end == 16 {
            return Err(RenderError::other("stop"));
        }
        Ok(())
    });
    assert_eq!(
        result.err().map(|e| e.to_string()),
        Some("stop".to_string())
    );
    assert_eq!(ran.load(Ordering::SeqCst), 16);
}
#[test]
fn a_cancellation_between_batches_ends_the_call() {
    let token = CancelToken::new();
    let ran = AtomicUsize::new(0);
    let item = |_: usize| {
        ran.fetch_add(1, Ordering::SeqCst);
    };
    let ctx: Option<&dyn Cancellation> = Some(&token);
    let result = run_batches_background(ctx, 40, 4, &item, &mut by(8), &mut |_, end| {
        if end == 24 {
            token.cancel();
        }
        Ok(())
    });
    assert!(result.is_err());
    assert_eq!(ran.load(Ordering::SeqCst), 24);
}
#[test]
fn a_cancellation_by_the_last_after_is_left_to_the_caller() {
    let token = CancelToken::new();
    let item = |_: usize| {};
    let ctx: Option<&dyn Cancellation> = Some(&token);
    let result = run_batches_background(ctx, 16, 2, &item, &mut by(8), &mut |_, end| {
        if end == 16 {
            token.cancel();
        }
        Ok(())
    });
    assert!(result.is_ok());
    assert!(token.err().is_some());
}
#[test]
fn a_cancellation_inside_a_batch_skips_its_remaining_items() {
    let token = CancelToken::new();
    let ran = AtomicUsize::new(0);
    let item = |_: usize| {
        ran.fetch_add(1, Ordering::SeqCst);
        token.cancel();
    };
    let ctx: Option<&dyn Cancellation> = Some(&token);
    let mut afters = 0;
    let result = run_batches_background(ctx, 12, 1, &item, &mut by(6), &mut |_, _| {
        afters += 1;
        Ok(())
    });
    assert!(result.is_err());
    assert_eq!((ran.load(Ordering::SeqCst), afters), (1, 0));
}
