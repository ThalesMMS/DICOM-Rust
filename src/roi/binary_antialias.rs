use super::binary_antialias_distance::{anti_alias_geometry, anti_alias_initial};
use super::distance3d::{VolumeMask, ctx_check};
use super::levelset::LevelSetStencil;
use super::{Cancellation, Error, ctx_err};
use crate::fmath;
use crate::render::GeometryAffine;
pub const ERR_BINARY_ANTI_ALIAS: &str = "roi: invalid binary anti-alias request";
pub const ERR_BINARY_ANTI_ALIAS_LIMIT: &str = "roi: binary anti-alias resource limit";
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct BinaryAntiAliasOptions {
    pub max_iterations: i64,
    pub maximum_rms_change_mm: f64,
    pub time_step_mm2: f64,
    pub band_width_mm: f64,
    pub sign_margin_mm: f64,
    pub max_voxels: u64,
    pub max_voxel_iterations: u64,
    pub max_bytes: u64,
    pub workers: i64,
}
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct BinaryAntiAliasIteration {
    pub iteration: i64,
    pub rms_change_mm: f64,
    pub clamped_voxels: i64,
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BinaryAntiAliasReport {
    pub options: BinaryAntiAliasOptions,
    pub iterations: i64,
    pub converged: bool,
    pub stop_reason: String,
    pub logical_bytes: u64,
    pub band_voxels: i64,
    pub inside_voxels: i64,
    pub history: Vec<BinaryAntiAliasIteration>,
}
#[derive(Debug, Clone, PartialEq)]
pub struct BinaryAntiAliasResult {
    pub size: [i64; 3],
    pub phi: Vec<f32>,
    pub initial_phi: Vec<f32>,
    pub original: VolumeMask,
    pub report: BinaryAntiAliasReport,
    affine: GeometryAffine,
}
impl BinaryAntiAliasResult {
    pub fn dims(&self) -> [i64; 3] {
        self.size
    }
    pub fn index_to_patient(&self) -> GeometryAffine {
        self.affine
    }
    pub fn read_slice(
        &self,
        ctx: Option<&dyn Cancellation>,
        z: i64,
        dst: &mut [f32],
    ) -> Result<(), Error> {
        ctx_check(ctx)?;
        let plane = self.size[0].wrapping_mul(self.size[1]);
        if z < 0
            || z >= self.size[2]
            || (dst.len() as i64) < plane
            || self.phi.len() as i64 != plane.wrapping_mul(self.size[2])
        {
            return Err(Error::sentinel(ERR_BINARY_ANTI_ALIAS));
        }
        let plane = plane as usize;
        let z = z as usize;
        dst[..plane].copy_from_slice(&self.phi[z * plane..(z + 1) * plane]);
        ctx_check(ctx)
    }
}
pub(crate) fn anti_alias_finite(v: f64) -> bool {
    v.is_finite()
}
fn anti_alias_reserve(total: &mut u64, v: u64, limit: u64) -> bool {
    if *total > limit || v > limit - *total {
        return false;
    }
    *total += v;
    true
}
pub fn binary_anti_alias(
    ctx: Option<&dyn Cancellation>,
    src: &VolumeMask,
    options: BinaryAntiAliasOptions,
    mut progress: Option<&mut dyn FnMut(BinaryAntiAliasIteration)>,
) -> Result<BinaryAntiAliasResult, Error> {
    let invalid = || Error::sentinel(ERR_BINARY_ANTI_ALIAS);
    let limit = || Error::sentinel(ERR_BINARY_ANTI_ALIAS_LIMIT);
    let mut o = options;
    ctx_check(ctx)?;
    let (size, h, origin, n) = anti_alias_geometry(ctx, src)?;
    let (mut hmin, mut hmax) = (fmath::min(h[0], h[1]), fmath::max(h[0], h[1]));
    let mut dims = 2i64;
    if size[2] > 1 {
        dims = 3;
        hmin = fmath::min(hmin, h[2]);
        hmax = fmath::max(hmax, h[2]);
    }
    if o.max_iterations == 0 {
        o.max_iterations = 100;
    }
    if o.max_voxels == 0 {
        o.max_voxels = 16 << 20;
    }
    if o.max_bytes == 0 {
        o.max_bytes = 512 << 20;
    }
    if o.max_voxel_iterations == 0 {
        o.max_voxel_iterations = 1 << 31;
    }
    if o.maximum_rms_change_mm == 0.0 {
        o.maximum_rms_change_mm = 0.005 * hmin;
    }
    if o.band_width_mm == 0.0 {
        o.band_width_mm = 3.0 * hmax;
    }
    if o.sign_margin_mm == 0.0 {
        o.sign_margin_mm = 1e-4 * hmin;
    }
    let max_dt = hmin * hmin / (4.0 * dims as f64);
    if o.time_step_mm2 == 0.0 {
        o.time_step_mm2 = max_dt;
    }
    if o.max_iterations < 1 || o.max_iterations > 100000 || o.workers < 0 || o.workers > 64 {
        return Err(invalid());
    }
    for v in [
        o.maximum_rms_change_mm,
        o.time_step_mm2,
        o.band_width_mm,
        o.sign_margin_mm,
        max_dt,
    ] {
        if !anti_alias_finite(v) || v <= 0.0 {
            return Err(invalid());
        }
    }
    if o.time_step_mm2 > max_dt
        || o.sign_margin_mm >= 0.5 * hmin
        || o.band_width_mm < hmax
        || o.sign_margin_mm as f32 <= 0.0
        || (o.sign_margin_mm as f32).is_infinite()
    {
        return Err(invalid());
    }
    let voxels = n as u64;
    if voxels > o.max_voxels
        || voxels > i32::MAX as u64
        || voxels > o.max_voxel_iterations / o.max_iterations as u64
    {
        return Err(limit());
    }
    let mut bytes = 0u64;
    let max_axis = size[0].max(size[1]).max(size[2]);
    let axis_count = size[0] as u64 + size[1] as u64 + size[2] as u64;
    for v in [
        voxels * 26,
        axis_count * 16,
        max_axis as u64 * 40 + 8,
        o.max_iterations as u64 * 24,
    ] {
        if !anti_alias_reserve(&mut bytes, v, o.max_bytes) {
            return Err(limit());
        }
    }
    let mut inside = 0i64;
    for (i, &v) in src.voxels.iter().enumerate() {
        if i & 4095 == 0 {
            ctx_check(ctx)?;
        }
        if v {
            inside += 1;
        }
    }
    if inside == 0 || inside as usize == n {
        return Err(Error::wrap(
            ERR_BINARY_ANTI_ALIAS,
            "mask needs inside and outside samples",
        ));
    }
    let original = src.clone();
    let usize_size = [size[0] as usize, size[1] as usize, size[2] as usize];
    let mut phi = anti_alias_initial(ctx, &original, usize_size, h, hmin)?;
    let initial = phi.clone();
    let mut next = phi.clone();
    let mut band: Vec<usize> = Vec::with_capacity(n);
    for (i, &v) in phi.iter().enumerate() {
        if i & 4095 == 0 {
            ctx_check(ctx)?;
        }
        if f64::from(v).abs() <= o.band_width_mm {
            band.push(i);
        }
    }
    if band.is_empty() {
        return Err(invalid());
    }
    let stride = [1, usize_size[0], usize_size[0] * usize_size[1]];
    let mut report = BinaryAntiAliasReport {
        options: o,
        iterations: 0,
        converged: false,
        stop_reason: "iteration-limit".to_string(),
        logical_bytes: bytes,
        band_voxels: band.len() as i64,
        inside_voxels: inside,
        history: Vec::new(),
    };
    let affine = GeometryAffine([
        h[0], 0.0, 0.0, origin[0], 0.0, h[1], 0.0, origin[1], 0.0, 0.0, h[2], origin[2], 0.0, 0.0,
        0.0, 1.0,
    ]);
    const CHUNK: usize = 256;
    for iteration in 1..=o.max_iterations {
        let mut nonfinite = false;
        {
            let state = LevelSetStencil {
                phi: &phi,
                size: usize_size,
                stride,
                h,
                dims,
            };
            let mut start = 0usize;
            'claims: while ctx_err(ctx).is_none() {
                if start >= band.len() {
                    break;
                }
                let end = (start + CHUNK).min(band.len());
                for (k, &i) in band.iter().enumerate().take(end).skip(start) {
                    if ctx_err(ctx).is_some() {
                        break 'claims;
                    }
                    if k & 63 == 0 && ctx_err(ctx).is_some() {
                        continue;
                    }
                    let rate = state.curvature_rate(i);
                    if !anti_alias_finite(f64::from(rate)) {
                        nonfinite = true;
                        continue;
                    }
                    let mut v = f64::from(phi[i]) + o.time_step_mm2 * f64::from(rate);
                    if original.voxels[i] {
                        v = fmath::max(v, o.sign_margin_mm);
                    } else {
                        v = fmath::min(v, -o.sign_margin_mm);
                    }
                    if !anti_alias_finite(v) || (v as f32).is_infinite() {
                        nonfinite = true;
                        continue;
                    }
                    next[i] = v as f32;
                }
                start += CHUNK;
            }
            ctx_check(ctx)?;
        }
        if nonfinite {
            return Err(Error::wrap(ERR_BINARY_ANTI_ALIAS, "nonfinite evolution"));
        }
        let mut squared = 0.0f64;
        let mut clamped = 0i64;
        for (k, &i) in band.iter().enumerate() {
            if k & 4095 == 0 {
                ctx_check(ctx)?;
            }
            let d = f64::from(next[i]) - f64::from(phi[i]);
            squared += d * d;
            if f64::from(next[i]).abs() <= o.sign_margin_mm * 1.000001 {
                clamped += 1;
            }
        }
        let rms = (squared / band.len() as f64).sqrt();
        if !anti_alias_finite(rms) {
            return Err(invalid());
        }
        std::mem::swap(&mut phi, &mut next);
        report.iterations = iteration;
        let p = BinaryAntiAliasIteration {
            iteration,
            rms_change_mm: rms,
            clamped_voxels: clamped,
        };
        report.history.push(p);
        if let Some(progress) = progress.as_mut() {
            progress(p);
        }
        ctx_check(ctx)?;
        if rms <= o.maximum_rms_change_mm {
            report.converged = true;
            report.stop_reason = "rms-tolerance".to_string();
            break;
        }
    }
    ctx_check(ctx)?;
    Ok(BinaryAntiAliasResult {
        size,
        phi,
        initial_phi: initial,
        original,
        report,
        affine,
    })
}
