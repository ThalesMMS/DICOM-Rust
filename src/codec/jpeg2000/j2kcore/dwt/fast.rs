use super::super::geometry::{BAND_HH, BAND_HL, BAND_LH, BAND_LL, TileComponent};
use super::super::par;
use super::{
    DWT_ALPHA, DWT_BETA, DWT_DELTA, DWT_GAMMA, DWT_HIGH_SCALE, DWT_LOW_SCALE, SubbandGrid,
};
struct Level<'a> {
    x0: i64,
    y0: i64,
    w: usize,
    h: usize,
    low_width: usize,
    high_width: usize,
    hl: &'a [f32],
    lh: &'a [f32],
    hh: &'a [f32],
}
fn ceil_half(v: i64) -> i64 {
    v.saturating_add(1).div_euclid(2)
}
fn floor_half(v: i64) -> i64 {
    v.div_euclid(2)
}
fn band_values(band: Option<&SubbandGrid>, rect: (i64, i64, i64, i64)) -> Option<&[f32]> {
    let (x0, y0, x1, y1) = rect;
    let area = (x1.saturating_sub(x0).max(0) as usize)
        .checked_mul(y1.saturating_sub(y0).max(0) as usize)?;
    match band {
        None if area == 0 => Some(&[]),
        None => None,
        Some(g) if area == 0 => (g.width() <= 0 || g.height() <= 0).then_some(&[] as &[f32]),
        Some(g) => ((g.x0, g.y0, g.x1, g.y1) == rect && g.values.len() == area)
            .then_some(g.values.as_slice()),
    }
}
pub(super) fn inverse(
    tc: &TileComponent,
    bands: &[[Option<SubbandGrid>; 4]],
    reversible: bool,
    threads: usize,
) -> Option<Vec<f32>> {
    let count = tc.resolutions.len();
    if count < 2 || bands.len() < count {
        return None;
    }
    let last = tc.resolutions.last()?;
    if (last.x0, last.y0, last.x1, last.y1) != (tc.x0, tc.y0, tc.x1, tc.y1) {
        return None;
    }
    let first = tc.resolutions.first()?;
    let ll = band_values(
        bands.first()?.first()?.as_ref(),
        (first.x0, first.y0, first.x1, first.y1),
    )?;
    if ll.is_empty() {
        return None;
    }
    let mut levels = Vec::with_capacity(count);
    const _: () = assert!(BAND_LL == 0 && BAND_HL == 1 && BAND_LH == 2 && BAND_HH == 3);
    for (pair, [_, hl, lh, hh]) in tc.resolutions.windows(2).zip(bands.iter().skip(1)) {
        let [below, res] = pair else { return None };
        let (x0, y0, x1, y1) = (res.x0, res.y0, res.x1, res.y1);
        let (w, h) = (x1.saturating_sub(x0), y1.saturating_sub(y0));
        if x1 <= x0 || y1 <= y0 || w > 1 << 20 || h > 1 << 20 {
            return None;
        }
        if (w == 1 && x0 % 2 != 0) || (h == 1 && y0 % 2 != 0) {
            return None;
        }
        let low = (ceil_half(x0), ceil_half(y0), ceil_half(x1), ceil_half(y1));
        if (below.x0, below.y0, below.x1, below.y1) != low {
            return None;
        }
        levels.push(Level {
            x0,
            y0,
            w: w as usize,
            h: h as usize,
            low_width: low.2.saturating_sub(low.0) as usize,
            high_width: floor_half(x1).saturating_sub(floor_half(x0)) as usize,
            hl: band_values(hl.as_ref(), (floor_half(x0), low.1, floor_half(x1), low.3))?,
            lh: band_values(lh.as_ref(), (low.0, floor_half(y0), low.2, floor_half(y1)))?,
            hh: band_values(
                hh.as_ref(),
                (
                    floor_half(x0),
                    floor_half(y0),
                    floor_half(x1),
                    floor_half(y1),
                ),
            )?,
        });
    }
    Some(if reversible {
        inverse_53(&levels, ll, threads)
    } else {
        inverse_97(&levels, ll, threads)
    })
}
#[inline(always)]
fn to_i32(v: f32) -> i32 {
    if (-2_147_483_648.0..2_147_483_648.0).contains(&v) {
        v as i32
    } else {
        i32::MIN
    }
}
#[inline(always)]
fn through_f32(v: i32) -> i32 {
    to_i32(v as f32)
}
fn phases(start: i64) -> (usize, usize) {
    let even = start.rem_euclid(2) as usize;
    (even, even ^ 1)
}
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
fn source_row(level: &Level<'_>, j: usize) -> (usize, bool) {
    let y = level.y0 + j as i64;
    if y.rem_euclid(2) == 0 {
        ((floor_half(y) - ceil_half(level.y0)) as usize, false)
    } else {
        ((floor_half(y) - floor_half(level.y0)) as usize, true)
    }
}
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
fn row_of(values: &[f32], row: usize, width: usize) -> &[f32] {
    &values[row * width..(row + 1) * width]
}
const SAMPLES_PER_THREAD: usize = 1 << 17;
const MOST_THREADS: usize = 8;
fn level_threads(threads: usize, samples: usize) -> usize {
    par::worth(threads.min(MOST_THREADS), samples, SAMPLES_PER_THREAD)
}
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
fn inverse_53(levels: &[Level<'_>], ll: &[f32], threads: usize) -> Vec<f32> {
    let mut current: Vec<i32> = ll.iter().map(|&v| to_i32(v)).collect();
    for level in levels {
        let (w, h) = (level.w, level.h);
        let threads = level_threads(threads, w * h);
        let (even, odd) = phases(level.x0);
        let mut next = vec![0i32; w * h];
        let below = current.as_slice();
        let (low_width, high_width) = (level.low_width, level.high_width);
        par::row_runs(&mut next, w, threads, |first, run| {
            let mut evens = vec![0i32; low_width + 2];
            let mut odds = vec![0i32; high_width + 2];
            for (j, line) in run.chunks_exact_mut(w).enumerate() {
                let (row, high_pass) = source_row(level, first + j);
                let (low, high) = (&mut evens[1..=low_width], &mut odds[1..=high_width]);
                if high_pass {
                    for (d, &s) in low.iter_mut().zip(row_of(level.lh, row, low_width)) {
                        *d = to_i32(s);
                    }
                    for (d, &s) in high.iter_mut().zip(row_of(level.hh, row, high_width)) {
                        *d = to_i32(s);
                    }
                } else {
                    let stored = &below[row * low_width..(row + 1) * low_width];
                    if w >= 2 {
                        for (d, &s) in low.iter_mut().zip(stored) {
                            *d = through_f32(s);
                        }
                    } else {
                        low.copy_from_slice(stored);
                    }
                    for (d, &s) in high.iter_mut().zip(row_of(level.hl, row, high_width)) {
                        *d = to_i32(s);
                    }
                }
                if w >= 2 {
                    lift_53_line(&mut evens, &mut odds, even);
                }
                let (low, high) = (&evens[1..=low_width], &odds[1..=high_width]);
                if h >= 2 {
                    for (d, &s) in line[even..].iter_mut().step_by(2).zip(low) {
                        *d = through_f32(s);
                    }
                    for (d, &s) in line[odd..].iter_mut().step_by(2).zip(high) {
                        *d = through_f32(s);
                    }
                } else {
                    for (d, &s) in line[even..].iter_mut().step_by(2).zip(low) {
                        *d = s;
                    }
                    for (d, &s) in line[odd..].iter_mut().step_by(2).zip(high) {
                        *d = s;
                    }
                }
            }
        });
        if h >= 2 {
            let (even, odd) = phases(level.y0);
            lift_rows(&mut next, w, even, threads, |d, a, b| {
                *d = d.wrapping_sub(a.wrapping_add(b).wrapping_add(2) >> 2)
            });
            lift_rows(&mut next, w, odd, threads, |d, a, b| {
                *d = d.wrapping_add(a.wrapping_add(b) >> 1)
            });
        }
        current = next;
    }
    let mut samples = vec![0f32; current.len()];
    let stored = current.as_slice();
    let threads = level_threads(threads, stored.len());
    par::row_runs(&mut samples, 1, threads, |first, run| {
        for (d, &s) in run.iter_mut().zip(&stored[first..]) {
            *d = s as f32;
        }
    });
    samples
}
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
fn extend<T: Copy>(half: &mut [T]) {
    let n = half.len();
    half[0] = half[1];
    half[n - 1] = half[n - 2];
}
#[inline(always)]
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
fn lift_line<T: Copy>(updated: &mut [T], read: &[T], first: usize, step: impl Fn(&mut T, T, T)) {
    let n = updated.len() - 2;
    let before = &read[first..first + n];
    let after = &read[first + 1..first + 1 + n];
    for ((d, &a), &b) in updated[1..=n].iter_mut().zip(before).zip(after) {
        step(d, a, b);
    }
}
fn lift_53_line(evens: &mut [i32], odds: &mut [i32], even: usize) {
    extend(odds);
    lift_line(evens, odds, even, |d, a, b| {
        *d = d.wrapping_sub(a.wrapping_add(b).wrapping_add(2) >> 2)
    });
    extend(evens);
    lift_line(odds, evens, even ^ 1, |d, a, b| {
        *d = d.wrapping_add(a.wrapping_add(b) >> 1)
    });
}
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
fn lift_rows<T: Copy + Send + Sync>(
    grid: &mut [T],
    w: usize,
    first: usize,
    threads: usize,
    step: impl Fn(&mut T, T, T) + Sync,
) {
    let h = grid.len() / w;
    if threads > 1 {
        let mut updated: Vec<(usize, &mut [T])> = Vec::with_capacity(h / 2 + 1);
        let mut read: Vec<&[T]> = Vec::with_capacity(h / 2 + 1);
        for (i, row) in grid.chunks_exact_mut(w).enumerate() {
            if i % 2 == first {
                updated.push((i, row));
            } else {
                read.push(row);
            }
        }
        let read = read.as_slice();
        par::each(&mut updated, threads, |_, (i, row)| {
            let above = read[(if *i > 0 { *i - 1 } else { *i + 1 }) / 2];
            let below = read[(if *i + 1 < h { *i + 1 } else { *i - 1 }) / 2];
            for ((d, &a), &b) in row.iter_mut().zip(above).zip(below) {
                step(d, a, b);
            }
        });
        return;
    }
    let mut i = first;
    while i < h {
        let (before, rest) = grid.split_at_mut(i * w);
        let (row, after) = rest.split_at_mut(w);
        let above = if i > 0 {
            &before[(i - 1) * w..]
        } else {
            &after[..w]
        };
        let below = if i + 1 < h {
            &after[..w]
        } else {
            &before[(i - 1) * w..]
        };
        for ((d, &a), &b) in row.iter_mut().zip(above).zip(below) {
            step(d, a, b);
        }
        i += 2;
    }
}
fn strip_width(h: usize) -> usize {
    32_768usize.checked_div(h).unwrap_or(32_768).clamp(8, 256)
}
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
fn lift_97_strip(grid: &[f32], w: usize, x: usize, n: usize, y0: i64, strip: &mut Vec<f64>) {
    let (even, odd) = phases(y0);
    strip.clear();
    strip.resize(grid.len() / w * n, 0.0);
    for (j, (buffer, row)) in strip
        .chunks_exact_mut(n)
        .zip(grid.chunks_exact(w))
        .enumerate()
    {
        let scale = if j % 2 == even {
            DWT_LOW_SCALE
        } else {
            DWT_HIGH_SCALE
        };
        for (b, &v) in buffer.iter_mut().zip(&row[x..x + n]) {
            *b = v as f64 * scale;
        }
    }
    for (first, c) in [
        (even, DWT_DELTA),
        (odd, DWT_GAMMA),
        (even, DWT_BETA),
        (odd, DWT_ALPHA),
    ] {
        lift_rows(strip, n, first, 1, |d, a, b| *d -= c * (a + b));
    }
}
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
fn inverse_97(levels: &[Level<'_>], ll: &[f32], threads: usize) -> Vec<f32> {
    let mut current: Vec<f32> = ll.to_vec();
    for level in levels {
        let (w, h) = (level.w, level.h);
        let threads = level_threads(threads, w * h);
        let (even, odd) = phases(level.x0);
        let mut next = vec![0f32; w * h];
        let below = current.as_slice();
        let (low_width, high_width) = (level.low_width, level.high_width);
        par::row_runs(&mut next, w, threads, |first, run| {
            let mut evens = vec![0f64; low_width + 2];
            let mut odds = vec![0f64; high_width + 2];
            for (j, out) in run.chunks_exact_mut(w).enumerate() {
                let (row, high_pass) = source_row(level, first + j);
                let (low, high) = if high_pass {
                    (
                        row_of(level.lh, row, low_width),
                        row_of(level.hh, row, high_width),
                    )
                } else {
                    (
                        row_of(below, row, low_width),
                        row_of(level.hl, row, high_width),
                    )
                };
                if w < 2 {
                    out[0] = low[0];
                    continue;
                }
                for (d, &s) in evens[1..=low_width].iter_mut().zip(low) {
                    *d = s as f64 * DWT_LOW_SCALE;
                }
                for (d, &s) in odds[1..=high_width].iter_mut().zip(high) {
                    *d = s as f64 * DWT_HIGH_SCALE;
                }
                for (c_even, c_odd) in [(DWT_DELTA, DWT_GAMMA), (DWT_BETA, DWT_ALPHA)] {
                    extend(&mut odds);
                    lift_line(&mut evens, &odds, even, |d, a, b| *d -= c_even * (a + b));
                    extend(&mut evens);
                    lift_line(&mut odds, &evens, odd, |d, a, b| *d -= c_odd * (a + b));
                }
                for (o, &v) in out[even..].iter_mut().step_by(2).zip(&evens[1..=low_width]) {
                    *o = v as f32;
                }
                for (o, &v) in out[odd..].iter_mut().step_by(2).zip(&odds[1..=high_width]) {
                    *o = v as f32;
                }
            }
        });
        if h >= 2 {
            let columns = strip_width(h).min(w);
            if threads > 1 {
                let mut strips: Vec<(usize, usize, Vec<f32>)> = (0..w)
                    .step_by(columns)
                    .map(|x| (x, columns.min(w - x), Vec::new()))
                    .collect();
                let grid = next.as_slice();
                par::each(&mut strips, threads, |_, (x, n, lifted)| {
                    let mut strip = Vec::new();
                    lift_97_strip(grid, w, *x, *n, level.y0, &mut strip);
                    *lifted = strip.iter().map(|&v| v as f32).collect();
                });
                let strips = strips.as_slice();
                par::row_runs(&mut next, w, threads, |first, run| {
                    for (j, row) in run.chunks_exact_mut(w).enumerate() {
                        for (x, n, lifted) in strips {
                            row[*x..*x + *n]
                                .copy_from_slice(&lifted[(first + j) * *n..(first + j + 1) * *n]);
                        }
                    }
                });
            } else {
                let mut strip = Vec::new();
                let mut x = 0;
                while x < w {
                    let n = columns.min(w - x);
                    lift_97_strip(&next, w, x, n, level.y0, &mut strip);
                    for (buffer, row) in strip.chunks_exact(n).zip(next.chunks_exact_mut(w)) {
                        for (o, &v) in row[x..x + n].iter_mut().zip(buffer) {
                            *o = v as f32;
                        }
                    }
                    x += n;
                }
            }
        }
        current = next;
    }
    current
}
