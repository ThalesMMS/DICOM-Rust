pub(super) fn row_runs<T: Send>(
    grid: &mut [T],
    width: usize,
    threads: usize,
    work: impl Fn(usize, &mut [T]) + Sync,
) {
    let rows = grid.len().checked_div(width).unwrap_or(0);
    let threads = threads.min(rows);
    if threads <= 1 {
        work(0, grid);
        return;
    }
    let per = rows.div_ceil(threads);
    let work = &work;
    std::thread::scope(|scope| {
        let mut runs = grid.chunks_mut(per.saturating_mul(width)).enumerate();
        let first = runs.next();
        for (k, run) in runs {
            scope.spawn(move || work(k.saturating_mul(per), run));
        }
        if let Some((_, run)) = first {
            work(0, run);
        }
    });
}
pub(super) fn each<T: Send>(items: &mut [T], threads: usize, work: impl Fn(usize, &mut T) + Sync) {
    row_runs(items, 1, threads, |first, run| {
        for (k, item) in run.iter_mut().enumerate() {
            work(first.saturating_add(k), item);
        }
    });
}
pub(super) fn worth(threads: usize, amount: usize, per_thread: usize) -> usize {
    threads
        .min(amount.checked_div(per_thread).unwrap_or(amount))
        .max(1)
}
