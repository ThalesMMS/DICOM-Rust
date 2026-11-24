const LIFT_ALPHA: f64 = -1.586134342059924;
const LIFT_BETA: f64 = -0.052980118572961;
const LIFT_GAMMA: f64 = 0.882911075530934;
const LIFT_DELTA: f64 = 0.443506852043971;
const SCALE_K: f64 = 1.230174104914001;
const SCALE_K_HALF: f64 = SCALE_K / 2.0;
fn mirror(i: i64, n: i64) -> usize {
    if n == 1 {
        return 0;
    }
    let period = 2 * (n - 1);
    let mut i = i % period;
    if i < 0 {
        i += period;
    }
    if i >= n {
        i = period - i;
    }
    i as usize
}
fn forward53(x: &mut [i32], scratch: &mut [i32]) {
    let n = x.len();
    if n < 2 {
        return;
    }
    let ni = n as i64;
    let mut i = 1;
    while i < n {
        let d = x[i - 1].wrapping_add(x[mirror(i as i64 + 1, ni)]) >> 1;
        x[i] = x[i].wrapping_sub(d);
        i += 2;
    }
    let mut i = 0;
    while i < n {
        let d = x[mirror(i as i64 - 1, ni)]
            .wrapping_add(x[mirror(i as i64 + 1, ni)])
            .wrapping_add(2)
            >> 2;
        x[i] = x[i].wrapping_add(d);
        i += 2;
    }
    deinterleave(x, scratch);
}
fn deinterleave<T: Copy>(x: &mut [T], scratch: &mut [T]) {
    let n = x.len();
    let low = n.div_ceil(2);
    scratch[..n].copy_from_slice(x);
    for i in 0..n {
        if i % 2 == 0 {
            x[i / 2] = scratch[i];
        } else {
            x[low + i / 2] = scratch[i];
        }
    }
}
fn forward97(x: &mut [f64], scratch: &mut [f64]) {
    let n = x.len();
    if n < 2 {
        return;
    }
    let ni = n as i64;
    let mut lift = |start: usize, c: f64| {
        let mut i = start;
        while i < n {
            let v = c * (x[mirror(i as i64 - 1, ni)] + x[mirror(i as i64 + 1, ni)]);
            x[i] += v;
            i += 2;
        }
    };
    lift(1, LIFT_ALPHA);
    lift(0, LIFT_BETA);
    lift(1, LIFT_GAMMA);
    lift(0, LIFT_DELTA);
    for (i, v) in x.iter_mut().enumerate() {
        if i % 2 == 0 {
            *v /= SCALE_K;
        } else {
            *v *= SCALE_K_HALF;
        }
    }
    deinterleave(x, scratch);
}
fn inverse97(x: &mut [f64], scratch: &mut [f64]) {
    let n = x.len();
    if n < 2 {
        return;
    }
    let ni = n as i64;
    let low = n.div_ceil(2);
    scratch[..n].copy_from_slice(x);
    for i in 0..n {
        if i % 2 == 0 {
            x[i] = scratch[i / 2] * SCALE_K;
        } else {
            x[i] = scratch[low + i / 2] * 2.0 / SCALE_K;
        }
    }
    let mut lift = |start: usize, c: f64| {
        let mut i = start;
        while i < n {
            let v = c * (x[mirror(i as i64 - 1, ni)] + x[mirror(i as i64 + 1, ni)]);
            x[i] -= v;
            i += 2;
        }
    };
    lift(0, LIFT_DELTA);
    lift(1, LIFT_GAMMA);
    lift(0, LIFT_BETA);
    lift(1, LIFT_ALPHA);
}
pub(super) fn forward_dwt53(plane: &mut [i32], w: usize, h: usize, levels: i64) {
    let mut line = vec![0i32; w.max(h)];
    let mut scratch = vec![0i32; w.max(h)];
    let (mut cw, mut ch) = (w, h);
    for _ in 0..levels {
        for x in 0..cw {
            let col = &mut line[..ch];
            for y in 0..ch {
                col[y] = plane[y * w + x];
            }
            forward53(col, &mut scratch);
            for y in 0..ch {
                plane[y * w + x] = col[y];
            }
        }
        for y in 0..ch {
            forward53(&mut plane[y * w..y * w + cw], &mut scratch);
        }
        cw = cw.div_ceil(2);
        ch = ch.div_ceil(2);
    }
}
pub(super) fn forward_dwt97(plane: &mut [f64], w: usize, h: usize, levels: i64) {
    let mut line = vec![0f64; w.max(h)];
    let mut scratch = vec![0f64; w.max(h)];
    let (mut cw, mut ch) = (w, h);
    for _ in 0..levels {
        for x in 0..cw {
            let col = &mut line[..ch];
            for y in 0..ch {
                col[y] = plane[y * w + x];
            }
            forward97(col, &mut scratch);
            for y in 0..ch {
                plane[y * w + x] = col[y];
            }
        }
        for y in 0..ch {
            forward97(&mut plane[y * w..y * w + cw], &mut scratch);
        }
        cw = cw.div_ceil(2);
        ch = ch.div_ceil(2);
    }
}
pub(super) fn level_size(n: usize, l: i64) -> usize {
    (n + (1usize << l) - 1) >> l
}
pub(super) fn synthesis_norm(level: i64, high: bool) -> f64 {
    let n = 64usize << level;
    let mut x = vec![0f64; n];
    let mut scratch = vec![0f64; n];
    let mut position = (n >> level) / 4;
    if high {
        position += n >> level;
    }
    x[position] = 1.0;
    let mut l = level - 1;
    while l >= 0 {
        let len = level_size(n, l);
        inverse97(&mut x[..len], &mut scratch);
        l -= 1;
    }
    let mut sum = 0.0;
    for v in &x {
        sum += v * v;
    }
    sum.sqrt()
}
pub(super) fn forward_rct(r: &mut [i32], g: &mut [i32], b: &mut [i32]) {
    for i in 0..r.len() {
        let y = r[i].wrapping_add(g[i].wrapping_mul(2)).wrapping_add(b[i]) >> 2;
        let u = b[i].wrapping_sub(g[i]);
        let v = r[i].wrapping_sub(g[i]);
        r[i] = y;
        g[i] = u;
        b[i] = v;
    }
}
pub(super) fn forward_ict(r: &mut [f64], g: &mut [f64], b: &mut [f64]) {
    for i in 0..r.len() {
        let y = 0.299 * r[i] + 0.587 * g[i] + 0.114 * b[i];
        let cb = -0.16875 * r[i] - 0.33126 * g[i] + 0.5 * b[i];
        let cr = 0.5 * r[i] - 0.41869 * g[i] - 0.08131 * b[i];
        r[i] = y;
        g[i] = cb;
        b[i] = cr;
    }
}
