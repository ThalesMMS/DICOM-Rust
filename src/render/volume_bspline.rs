#![forbid(unsafe_code)]
use super::error::RenderError;
use super::parallel::{Cancellation, parallel_rows_worker_context};
use super::vec3::Vec3;
use super::volume::checked_voxel_count;
use crate::fmath;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
pub const ERR_BSPLINE_SAMPLES: &str = "render: invalid cubic B-spline samples";
fn bspline_error(detail: &str) -> RenderError {
    RenderError::other(format!("{ERR_BSPLINE_SAMPLES}: {detail}"))
}
fn bspline_pole() -> f64 {
    3f64.sqrt() - 2.0
}
fn bspline_horizon() -> usize {
    static H: OnceLock<usize> = OnceLock::new();
    *H.get_or_init(|| {
        fmath::f64_to_i64((fmath::log(1e-12) / fmath::log(bspline_pole().abs())).ceil()) as usize
    })
}
#[derive(Debug, Clone, PartialEq)]
pub struct CubicBSpline {
    min: [i64; 3],
    size: [i64; 3],
    coefficients: Vec<f32>,
}
impl CubicBSpline {
    pub fn new(
        ctx: Option<&dyn Cancellation>,
        min: [i64; 3],
        size: [i64; 3],
        sample: Option<&(dyn Fn(i64, i64, i64) -> f64 + Sync)>,
    ) -> Result<CubicBSpline, RenderError> {
        let Some(sample) = sample else {
            return Err(bspline_error("nil sample reader"));
        };
        let Some(count) = checked_voxel_count(size[0], size[1], size[2]) else {
            return Err(bspline_error(&format!(
                "box [{} {} {}]",
                size[0], size[1], size[2]
            )));
        };
        let (nx, ny, nz) = (size[0] as usize, size[1] as usize, size[2] as usize);
        let plane = nx * ny;
        let coefficients = Mutex::new(vec![0f32; count]);
        let non_finite = AtomicBool::new(false);
        parallel_rows_worker_context(ctx, nz as i64, || {
            let coefficients = &coefficients;
            let non_finite = &non_finite;
            move |z: i64| {
                let z = z as usize;
                let mut line = vec![0f64; nx];
                let mut rows = vec![0f32; plane];
                for y in 0..ny {
                    for (x, v) in line.iter_mut().enumerate() {
                        *v = sample(min[0] + x as i64, min[1] + y as i64, min[2] + z as i64);
                    }
                    if line.iter().any(|v| !v.is_finite()) {
                        non_finite.store(true, Ordering::SeqCst);
                        return;
                    }
                    bspline_prefilter_line(&mut line);
                    for (x, v) in line.iter().enumerate() {
                        rows[y * nx + x] = *v as f32;
                    }
                }
                if let Ok(mut c) = coefficients.lock() {
                    c[z * plane..(z + 1) * plane].copy_from_slice(&rows);
                }
            }
        })?;
        if non_finite.load(Ordering::SeqCst) {
            return Err(bspline_error("non-finite sample"));
        }
        let mut coefficients = coefficients.into_inner().unwrap_or_default();
        let planes: Vec<OnceLock<Vec<f32>>> = (0..nz).map(|_| OnceLock::new()).collect();
        {
            let source = &coefficients;
            parallel_rows_worker_context(ctx, nz as i64, || {
                let planes = &planes;
                move |z: i64| {
                    let z = z as usize;
                    let mut out = source[z * plane..(z + 1) * plane].to_vec();
                    let mut line = vec![0f64; ny];
                    for x in 0..nx {
                        for (y, v) in line.iter_mut().enumerate() {
                            *v = f64::from(out[y * nx + x]);
                        }
                        bspline_prefilter_line(&mut line);
                        for (y, v) in line.iter().enumerate() {
                            out[y * nx + x] = *v as f32;
                        }
                    }
                    let _ = planes[z].set(out);
                }
            })?;
        }
        for (z, p) in planes.into_iter().enumerate() {
            if let Some(out) = p.into_inner() {
                coefficients[z * plane..(z + 1) * plane].copy_from_slice(&out);
            }
        }
        let rows: Vec<OnceLock<Vec<f32>>> = (0..ny).map(|_| OnceLock::new()).collect();
        {
            let source = &coefficients;
            parallel_rows_worker_context(ctx, ny as i64, || {
                let rows = &rows;
                move |y: i64| {
                    let y = y as usize;
                    let mut out = vec![0f32; nx * nz];
                    let mut line = vec![0f64; nz];
                    for x in 0..nx {
                        for (z, v) in line.iter_mut().enumerate() {
                            *v = f64::from(source[z * plane + y * nx + x]);
                        }
                        bspline_prefilter_line(&mut line);
                        for (z, v) in line.iter().enumerate() {
                            out[z * nx + x] = *v as f32;
                        }
                    }
                    let _ = rows[y].set(out);
                }
            })?;
        }
        for (y, r) in rows.into_iter().enumerate() {
            if let Some(out) = r.into_inner() {
                for z in 0..nz {
                    coefficients[z * plane + y * nx..z * plane + y * nx + nx]
                        .copy_from_slice(&out[z * nx..(z + 1) * nx]);
                }
            }
        }
        Ok(CubicBSpline {
            min,
            size,
            coefficients,
        })
    }
    #[allow(clippy::needless_range_loop)]
    pub fn at(&self, p: Vec3) -> f64 {
        if self.coefficients.is_empty() {
            return 0.0;
        }
        let (nx, ny, nz) = (self.size[0], self.size[1], self.size[2]);
        let (bx, wx) = bspline_weights(p.x - self.min[0] as f64);
        let (by, wy) = bspline_weights(p.y - self.min[1] as f64);
        let (bz, wz) = bspline_weights(p.z - self.min[2] as f64);
        let mut xi = [0usize; 4];
        for (i, slot) in xi.iter_mut().enumerate() {
            *slot = mirror_index(bx.wrapping_sub(1).wrapping_add(i as i64), nx) as usize;
        }
        let mut sum = 0.0;
        for k in 0..4 {
            if wz[k] == 0.0 {
                continue;
            }
            let plane_offset =
                (mirror_index(bz.wrapping_sub(1).wrapping_add(k as i64), nz) * nx * ny) as usize;
            let mut plane = 0.0;
            for j in 0..4 {
                if wy[j] == 0.0 {
                    continue;
                }
                let row = &self.coefficients[plane_offset
                    + (mirror_index(by.wrapping_sub(1).wrapping_add(j as i64), ny) * nx)
                        as usize..];
                plane += wy[j]
                    * (wx[0] * f64::from(row[xi[0]])
                        + wx[1] * f64::from(row[xi[1]])
                        + wx[2] * f64::from(row[xi[2]])
                        + wx[3] * f64::from(row[xi[3]]));
            }
            sum += wz[k] * plane;
        }
        sum
    }
}
pub(crate) fn bspline_prefilter_line(c: &mut [f64]) {
    let n = c.len();
    if n < 2 {
        return;
    }
    let z = bspline_pole();
    let gain = (1.0 - z) * (1.0 - 1.0 / z);
    for v in c.iter_mut() {
        *v *= gain;
    }
    c[0] = bspline_initial_causal(c, z);
    for i in 1..n {
        c[i] += z * c[i - 1];
    }
    c[n - 1] = (z / (z * z - 1.0)) * (z * c[n - 2] + c[n - 1]);
    for i in (0..n - 1).rev() {
        c[i] = z * (c[i + 1] - c[i]);
    }
}
fn bspline_initial_causal(c: &[f64], z: f64) -> f64 {
    let n = c.len();
    let horizon = bspline_horizon();
    if horizon < n {
        let (mut sum, mut zk) = (c[0], z);
        for v in c.iter().take(horizon).skip(1) {
            sum += zk * v;
            zk *= z;
        }
        return sum;
    }
    let mut zk = z;
    let mut z2n = fmath::pow(z, (n - 1) as f64);
    let mut sum = c[0] + z2n * c[n - 1];
    z2n *= z2n / z;
    for v in c.iter().take(n - 1).skip(1) {
        sum += (zk + z2n) * v;
        zk *= z;
        z2n /= z;
    }
    sum / (1.0 - zk * zk)
}
fn bspline_weights(v: f64) -> (i64, [f64; 4]) {
    let base = fmath::f64_to_i64(v.floor());
    let t = v - base as f64;
    let s = 1.0 - t;
    let (t2, t3) = (t * t, t * t * t);
    (
        base,
        [
            s * s * s / 6.0,
            (4.0 - 6.0 * t2 + 3.0 * t3) / 6.0,
            (1.0 + 3.0 * t + 3.0 * t2 - 3.0 * t3) / 6.0,
            t3 / 6.0,
        ],
    )
}
fn mirror_index(i: i64, n: i64) -> i64 {
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
    i
}
