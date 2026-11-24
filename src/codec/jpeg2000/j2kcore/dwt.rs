use super::geometry::{BAND_HH, BAND_HL, BAND_LH, BAND_LL, TileComponent};
use super::{Result, errorf};
use crate::fmath;
mod fast;
#[cfg(test)]
mod tests;
const DWT_ALPHA: f64 = -1.586134342059924;
const DWT_BETA: f64 = -0.052980118572961;
const DWT_GAMMA: f64 = 0.882911075530934;
const DWT_DELTA: f64 = 0.443506852043971;
const DWT_LOW_SCALE: f64 = 1.230174104914001;
const DWT_HIGH_SCALE: f64 = 1.625786440180958;
#[derive(Debug, Clone, Default)]
pub(super) struct SubbandGrid {
    pub x0: i64,
    pub y0: i64,
    pub x1: i64,
    pub y1: i64,
    pub values: Vec<f32>,
}
impl SubbandGrid {
    pub fn width(&self) -> i64 {
        self.x1.saturating_sub(self.x0)
    }
    fn height(&self) -> i64 {
        self.y1.saturating_sub(self.y0)
    }
    fn index(&self, x: i64, y: i64) -> Option<usize> {
        let (dx, dy) = (x.checked_sub(self.x0)?, y.checked_sub(self.y0)?);
        if dx < 0 || dy < 0 || dx >= self.width() {
            return None;
        }
        usize::try_from(dy.checked_mul(self.width())?.checked_add(dx)?).ok()
    }
    fn at(&self, x: i64, y: i64) -> f32 {
        self.index(x, y)
            .and_then(|k| self.values.get(k))
            .copied()
            .unwrap_or(0.0)
    }
}
pub(super) fn inverse_dwt(
    tc: &TileComponent,
    bands: Vec<[Option<SubbandGrid>; 4]>,
    reversible: bool,
    threads: usize,
) -> Result<Vec<f32>> {
    match fast::inverse(tc, &bands, reversible, threads) {
        Some(samples) => Ok(samples),
        None => inverse_dwt_plain(tc, bands, reversible),
    }
}
fn inverse_dwt_plain(
    tc: &TileComponent,
    mut bands: Vec<[Option<SubbandGrid>; 4]>,
    reversible: bool,
) -> Result<Vec<f32>> {
    if tc.resolutions.is_empty() {
        return Err(errorf("a tile-component with no resolutions"));
    }
    const _: () = assert!(BAND_LL == 0 && BAND_HL == 1 && BAND_LH == 2 && BAND_HH == 3);
    let mut current = match bands.first_mut().and_then(|[ll, ..]| ll.take()) {
        Some(g) => g,
        None => return Err(errorf("resolution 0 has no LL band")),
    };
    for (res, detail) in tc.resolutions.iter().zip(&bands).skip(1) {
        let (w, h) = (res.x1.saturating_sub(res.x0), res.y1.saturating_sub(res.y0));
        let mut next = SubbandGrid {
            x0: res.x0,
            y0: res.y0,
            x1: res.x1,
            y1: res.y1,
            values: vec![0.0; w.saturating_mul(h).max(0) as usize],
        };
        interleave(&mut next, &current, detail);
        synthesize_2d(&mut next, reversible);
        current = next;
    }
    if current.x0 != tc.x0 || current.y0 != tc.y0 || current.x1 != tc.x1 || current.y1 != tc.y1 {
        return Err(errorf(format_args!(
            "the synthesized grid is {}x{} at ({},{}), not the tile-component's",
            current.width(),
            current.height(),
            current.x0,
            current.y0
        )));
    }
    Ok(current.values)
}
fn interleave(dst: &mut SubbandGrid, ll: &SubbandGrid, detail: &[Option<SubbandGrid>; 4]) {
    fn place(dst: &mut SubbandGrid, band: Option<&SubbandGrid>, xob: i64, yob: i64) {
        let Some(band) = band else { return };
        for v in band.y0..band.y1 {
            for u in band.x0..band.x1 {
                let x = u.saturating_mul(2).saturating_add(xob);
                let y = v.saturating_mul(2).saturating_add(yob);
                if x < dst.x0 || x >= dst.x1 || y < dst.y0 || y >= dst.y1 {
                    continue;
                }
                let value = band.at(u, v);
                if let Some(d) = dst.index(x, y).and_then(|k| dst.values.get_mut(k)) {
                    *d = value;
                }
            }
        }
    }
    let [_, hl, lh, hh] = detail;
    place(dst, Some(ll), 0, 0);
    place(dst, hl.as_ref(), 1, 0);
    place(dst, lh.as_ref(), 0, 1);
    place(dst, hh.as_ref(), 1, 1);
}
fn synthesize_2d(g: &mut SubbandGrid, reversible: bool) {
    let (w, h) = (g.width(), g.height());
    if w <= 0 || h <= 0 {
        return;
    }
    let (w, h) = (w as usize, h as usize);
    let (x0, x1, y0, y1) = (g.x0, g.x1, g.y0, g.y1);
    for row in g.values.chunks_exact_mut(w) {
        synthesize_1d(row, x0, x1, reversible);
    }
    let mut column = vec![0f32; h];
    for x in 0..w {
        for (c, &v) in column.iter_mut().zip(g.values.iter().skip(x).step_by(w)) {
            *c = v;
        }
        synthesize_1d(&mut column, y0, y1, reversible);
        for (v, &c) in g.values.iter_mut().skip(x).step_by(w).zip(&column) {
            *v = c;
        }
    }
}
fn synthesize_1d(y: &mut [f32], i0: i64, i1: i64, reversible: bool) {
    let n = i1.saturating_sub(i0);
    if n == 1 {
        if i0 % 2 != 0
            && reversible
            && let Some(v) = y.first_mut()
        {
            *v /= 2.0;
        }
        return;
    }
    if n <= 0 || y.len() as i64 != n {
        return;
    }
    if reversible {
        synthesize53(y, i0, i1);
    } else {
        synthesize97(y, i0, i1);
    }
}
fn extended(y: &[f32], i0: i64, i1: i64, i: i64) -> f32 {
    let n = i1.saturating_sub(i0);
    if n == 1 {
        return y.first().copied().unwrap_or(0.0);
    }
    let period = n.saturating_sub(1).saturating_mul(2);
    let mut k = i.saturating_sub(i0).checked_rem_euclid(period).unwrap_or(0);
    if k >= n {
        k = period.saturating_sub(k);
    }
    y.get(k as usize).copied().unwrap_or(0.0)
}
fn is_even(x: i64) -> bool {
    x % 2 == 0
}
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
fn synthesize53(y: &mut [f32], i0: i64, i1: i64) {
    let n = i1 - i0;
    const PAD: i64 = 2;
    let mut buf = vec![0i32; (n + 2 * PAD) as usize];
    for (k, b) in buf.iter_mut().enumerate() {
        *b = fmath::f64_to_i32(extended(y, i0, i1, i0 - PAD + k as i64) as f64);
    }
    let at = |x: i64| (x - i0 + PAD) as usize;
    for x in (i0 - 1)..=i1 {
        if !is_even(x) {
            continue;
        }
        let d = buf[at(x - 1)].wrapping_add(buf[at(x + 1)]).wrapping_add(2) >> 2;
        buf[at(x)] = buf[at(x)].wrapping_sub(d);
    }
    for x in i0..i1 {
        if is_even(x) {
            continue;
        }
        let d = buf[at(x - 1)].wrapping_add(buf[at(x + 1)]) >> 1;
        buf[at(x)] = buf[at(x)].wrapping_add(d);
    }
    for k in 0..n as usize {
        y[k] = buf[PAD as usize + k] as f32;
    }
}
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
fn synthesize97(y: &mut [f32], i0: i64, i1: i64) {
    let n = i1 - i0;
    const PAD: i64 = 4;
    let mut buf = vec![0f64; (n + 2 * PAD) as usize];
    for (k, b) in buf.iter_mut().enumerate() {
        *b = extended(y, i0, i1, i0 - PAD + k as i64) as f64;
    }
    let at = |x: i64| (x - i0 + PAD) as usize;
    let (lo, hi) = (i0 - PAD, i1 + PAD);
    for x in lo..hi {
        if is_even(x) {
            buf[at(x)] *= DWT_LOW_SCALE;
        } else {
            buf[at(x)] *= DWT_HIGH_SCALE;
        }
    }
    let mut lift = |odd: bool, c: f64, from: i64, to: i64| {
        for x in from..to {
            if is_even(x) == odd {
                continue;
            }
            let v = c * (buf[at(x - 1)] + buf[at(x + 1)]);
            buf[at(x)] -= v;
        }
    };
    lift(false, DWT_DELTA, lo + 1, hi - 1);
    lift(true, DWT_GAMMA, lo + 2, hi - 2);
    lift(false, DWT_BETA, lo + 3, hi - 3);
    lift(true, DWT_ALPHA, lo + 4, hi - 4);
    for k in 0..n as usize {
        y[k] = buf[PAD as usize + k] as f32;
    }
}
