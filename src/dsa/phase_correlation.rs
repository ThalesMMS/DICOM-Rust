use super::{Error, ErrorKind, Shift, apply_shift, fmax, fmin, frame_size_matches};
use crate::fmath;
use crate::numeric::fft::{self, Complex};
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PeakInterpolation(pub i64);
impl PeakInterpolation {
    pub const PARABOLIC: PeakInterpolation = PeakInterpolation(0);
    pub const COSINE: PeakInterpolation = PeakInterpolation(1);
    pub const WEIGHTED_MEAN_PHASE: PeakInterpolation = PeakInterpolation(2);
    pub const NONE: PeakInterpolation = PeakInterpolation(3);
}
impl std::fmt::Display for PeakInterpolation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.0 {
            0 => f.write_str("parabolic"),
            1 => f.write_str("cosine"),
            2 => f.write_str("weighted-mean-phase"),
            3 => f.write_str("none"),
            other => write!(f, "PeakInterpolation({other})"),
        }
    }
}
const MIN_PHASE_CORRELATION_SIZE: i64 = 8;
const PHASE_REFINEMENT_ITERATIONS: usize = 4;
#[derive(Debug, Clone, Copy, Default)]
pub struct PhaseCorrelationOptions {
    pub peak: PeakInterpolation,
    pub max_shift: f64,
}
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Registration {
    pub shift: Shift,
    pub dx: f64,
    pub dy: f64,
    pub peak: f64,
}
pub fn phase_correlate(
    reference: &[f64],
    moving: &[f64],
    columns: i64,
    rows: i64,
    opts: PhaseCorrelationOptions,
) -> Result<Registration, Error> {
    if !frame_size_matches(columns, rows, &[reference, moving]) {
        return Err(Error::wrap(
            ErrorKind::FrameSizeMismatch,
            format!(
                "{} and {} values for {columns}x{rows}",
                reference.len(),
                moving.len()
            ),
        ));
    }
    if columns < MIN_PHASE_CORRELATION_SIZE || rows < MIN_PHASE_CORRELATION_SIZE {
        return Err(Error::wrap(
            ErrorKind::FrameTooSmall,
            format!("{columns}x{rows}"),
        ));
    }
    if opts.peak.0 < PeakInterpolation::PARABOLIC.0 || opts.peak.0 > PeakInterpolation::NONE.0 {
        return Err(Error::with_message(
            ErrorKind::Other,
            format!("dsa: unknown peak interpolation {}", opts.peak.0),
        ));
    }
    let (width, height) = (
        fft::next_power_of_two(columns),
        fft::next_power_of_two(rows),
    );
    const COMPLEX128_BYTES: i64 = 16;
    if width > i64::MAX / height / COMPLEX128_BYTES {
        return Err(Error::wrap(
            ErrorKind::FrameSizeMismatch,
            format!("padded spectrum of {width}x{height} exceeds addressable size"),
        ));
    }
    let a = tapered_spectrum(reference, columns, rows, width, height)?;
    let mut b = tapered_spectrum(moving, columns, rows, width, height)?;
    let (limit_x, limit_y) = (
        search_limit(opts.max_shift, width, columns),
        search_limit(opts.max_shift, height, rows),
    );
    let (mut dx, mut dy, peak) =
        correlation_peak(&a, &b, width, height, limit_x, limit_y, opts.peak);
    if opts.peak != PeakInterpolation::NONE {
        for _ in 0..PHASE_REFINEMENT_ITERATIONS {
            let shifted = apply_shift(moving, columns, rows, Shift::from_displacement(dx, dy))?;
            b = tapered_spectrum(&shifted, columns, rows, width, height)?;
            let (rx, ry, _) = correlation_peak(&a, &b, width, height, 1, 1, opts.peak);
            dx += rx;
            dy += ry;
            if rx.abs() < 0.005 && ry.abs() < 0.005 {
                break;
            }
        }
    }
    Ok(Registration {
        shift: Shift::from_displacement(dx, dy),
        dx,
        dy,
        peak,
    })
}
fn cmul(a: Complex, b: Complex) -> Complex {
    Complex::new(a.re * b.re - a.im * b.im, a.re * b.im + a.im * b.re)
}
fn inf2one(f: f64) -> f64 {
    let g: f64 = if f.is_infinite() { 1.0 } else { 0.0 };
    g.copysign(f)
}
fn cdiv(n: Complex, m: Complex) -> Complex {
    let (mut e, mut f);
    if m.re.abs() >= m.im.abs() {
        let ratio = m.im / m.re;
        let denom = m.re + ratio * m.im;
        e = (n.re + n.im * ratio) / denom;
        f = (n.im - n.re * ratio) / denom;
    } else {
        let ratio = m.re / m.im;
        let denom = m.im + ratio * m.re;
        e = (n.re * ratio + n.im) / denom;
        f = (n.im * ratio - n.re) / denom;
    }
    if e.is_nan() && f.is_nan() {
        let (mut a, mut b) = (n.re, n.im);
        let (mut c, mut d) = (m.re, m.im);
        if c == 0.0 && d == 0.0 && (!a.is_nan() || !b.is_nan()) {
            e = f64::INFINITY.copysign(c) * a;
            f = f64::INFINITY.copysign(c) * b;
        } else if (a.is_infinite() || b.is_infinite()) && c.is_finite() && d.is_finite() {
            a = inf2one(a);
            b = inf2one(b);
            e = f64::INFINITY * (a * c + b * d);
            f = f64::INFINITY * (b * c - a * d);
        } else if (c.is_infinite() || d.is_infinite()) && a.is_finite() && b.is_finite() {
            c = inf2one(c);
            d = inf2one(d);
            e = 0.0 * (a * c + b * d);
            f = 0.0 * (b * c - a * d);
        }
    }
    Complex::new(e, f)
}
fn correlation_peak(
    a: &[Complex],
    b: &[Complex],
    width: i64,
    height: i64,
    limit_x: i64,
    limit_y: i64,
    method: PeakInterpolation,
) -> (f64, f64, f64) {
    let mut cross = vec![Complex::default(); a.len()];
    let mut surface = vec![Complex::default(); a.len()];
    for i in 0..a.len() {
        let c = cmul(a[i], Complex::new(b[i].re, -b[i].im));
        cross[i] = c;
        let magnitude = fmath::hypot(c.re, c.im);
        if magnitude > 1e-12 {
            surface[i] = cdiv(c, Complex::new(magnitude, 0.0));
        }
    }
    fft::transform_2d(&mut surface, width as usize, height as usize, true);
    let (mut peak_x, mut peak_y, mut peak) = (0i64, 0i64, f64::NEG_INFINITY);
    for y in -limit_y..=limit_y {
        let row = wrap_index(y, height) * width;
        for x in -limit_x..=limit_x {
            let value = surface[(row + wrap_index(x, width)) as usize].re;
            if value > peak {
                (peak_x, peak_y, peak) = (x, y, value);
            }
        }
    }
    let at = |x: i64, y: i64| -> f64 {
        surface[(wrap_index(y, height) * width + wrap_index(x, width)) as usize].re
    };
    let (mut dx, mut dy) = (peak_x as f64, peak_y as f64);
    if method == PeakInterpolation::PARABOLIC {
        dx += parabolic_offset(at(peak_x - 1, peak_y), peak, at(peak_x + 1, peak_y));
        dy += parabolic_offset(at(peak_x, peak_y - 1), peak, at(peak_x, peak_y + 1));
    } else if method == PeakInterpolation::COSINE {
        dx += cosine_offset(at(peak_x - 1, peak_y), peak, at(peak_x + 1, peak_y));
        dy += cosine_offset(at(peak_x, peak_y - 1), peak, at(peak_x, peak_y + 1));
    } else if method == PeakInterpolation::WEIGHTED_MEAN_PHASE {
        let (rx, ry) = phase_slope_offset(&cross, width, height, peak_x, peak_y);
        dx += rx;
        dy += ry;
    }
    (dx, dy, peak)
}
fn tapered_spectrum(
    frame: &[f64],
    columns: i64,
    rows: i64,
    width: i64,
    height: i64,
) -> Result<Vec<Complex>, Error> {
    let (mut mean, mut gx, mut gy) = (0.0f64, 0.0f64, 0.0f64);
    let (cx, cy) = ((columns - 1) as f64 / 2.0, (rows - 1) as f64 / 2.0);
    for y in 0..rows {
        for x in 0..columns {
            let value = frame[(y * columns + x) as usize];
            if value.is_nan() || value.is_infinite() {
                return Err(Error::with_message(
                    ErrorKind::Other,
                    "dsa: phase correlation frame has a non-finite value".to_string(),
                ));
            }
            mean += value;
            gx += value * (x as f64 - cx);
            gy += value * (y as f64 - cy);
        }
    }
    let count = frame.len() as f64;
    mean /= count;
    gx /= count * (columns as f64 * columns as f64 - 1.0) / 12.0;
    gy /= count * (rows as f64 * rows as f64 - 1.0) / 12.0;
    let (window_x, window_y) = (hann_window(columns), hann_window(rows));
    let mut out = vec![Complex::default(); (width * height) as usize];
    for y in 0..rows {
        for x in 0..columns {
            let plane = mean + gx * (x as f64 - cx) + gy * (y as f64 - cy);
            out[(y * width + x) as usize] = Complex::new(
                (frame[(y * columns + x) as usize] - plane)
                    * window_x[x as usize]
                    * window_y[y as usize],
                0.0,
            );
        }
    }
    fft::transform_2d(&mut out, width as usize, height as usize, false);
    Ok(out)
}
fn hann_window(n: i64) -> Vec<f64> {
    (0..n)
        .map(|i| 0.5 - 0.5 * fmath::cos(std::f64::consts::TAU * (i as f64 + 0.5) / n as f64))
        .collect()
}
fn search_limit(max_shift: f64, padded: i64, size: i64) -> i64 {
    let mut limit = size / 4;
    if max_shift > 0.0 && max_shift < limit as f64 {
        limit = fmath::f64_to_i64(max_shift.ceil());
    }
    limit.max(1).min(padded / 2 - 1)
}
fn parabolic_offset(y0: f64, y1: f64, y2: f64) -> f64 {
    let denominator = y0 - 2.0 * y1 + y2;
    if denominator >= 0.0 {
        return 0.0;
    }
    clamp_half((y0 - y2) / (2.0 * denominator))
}
fn cosine_offset(y0: f64, y1: f64, y2: f64) -> f64 {
    if y1 <= 0.0 {
        return 0.0;
    }
    let ratio = (y0 + y2) / (2.0 * y1);
    const EDGE: f64 = 0.999999999;
    let ratio = fmin(fmax(ratio, -EDGE), EDGE);
    let omega = fmath::acos(ratio);
    clamp_half(fmath::atan((y2 - y0) / (2.0 * y1 * fmath::sin(omega))) / omega)
}
fn phase_slope_offset(
    cross: &[Complex],
    width: i64,
    height: i64,
    peak_x: i64,
    peak_y: i64,
) -> (f64, f64) {
    let (mut sxx, mut sxy, mut syy, mut sxp, mut syp) = (0.0f64, 0.0f64, 0.0f64, 0.0f64, 0.0f64);
    for ky in -height / 4..=height / 4 {
        let row = wrap_index(ky, height) * width;
        for kx in -width / 4..=width / 4 {
            if kx == 0 && ky == 0 {
                continue;
            }
            let c = cross[(row + wrap_index(kx, width)) as usize];
            let weight = fmath::hypot(c.re, c.im);
            if weight == 0.0 {
                continue;
            }
            let fx = std::f64::consts::TAU * kx as f64 / width as f64;
            let fy = std::f64::consts::TAU * ky as f64 / height as f64;
            let rotated = cmul(c, cexp_imaginary(fx * peak_x as f64 + fy * peak_y as f64));
            let phase = fmath::atan2(rotated.im, rotated.re);
            sxx += weight * fx * fx;
            sxy += weight * fx * fy;
            syy += weight * fy * fy;
            sxp -= weight * fx * phase;
            syp -= weight * fy * phase;
        }
    }
    let determinant = sxx * syy - sxy * sxy;
    if determinant <= 0.0 {
        return (0.0, 0.0);
    }
    let rx = (sxp * syy - syp * sxy) / determinant;
    let ry = (syp * sxx - sxp * sxy) / determinant;
    (fmin(fmax(rx, -1.0), 1.0), fmin(fmax(ry, -1.0), 1.0))
}
fn cexp_imaginary(t: f64) -> Complex {
    let (s, c) = fmath::sincos(t);
    Complex::new(1.0 * c, 1.0 * s)
}
fn clamp_half(value: f64) -> f64 {
    if value.is_nan() {
        return 0.0;
    }
    fmin(fmax(value, -0.5), 0.5)
}
fn wrap_index(i: i64, n: i64) -> i64 {
    let mut i = i % n;
    if i < 0 {
        i += n;
    }
    i
}
