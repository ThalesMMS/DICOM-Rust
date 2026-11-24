use super::levelset::{
    ERR_LEVEL_SET_EMPTY, ERR_LEVEL_SET_INVALID_INPUT, ERR_LEVEL_SET_RESOURCE_LIMIT, LevelSetLimits,
    LevelSetVolume, check,
};
use super::roimask::{MaskRun, RasterMask};
use super::{Cancellation, Error};
use crate::fmath;
use std::collections::BTreeMap;
#[derive(Clone, Copy, Default)]
pub struct ChanVeseOptions<'a> {
    pub region_origin: [i64; 3],
    pub region_size: [i64; 3],
    pub limits: LevelSetLimits,
    pub mu: f64,
    pub nu: f64,
    pub lambda_inside: f64,
    pub lambda_outside: f64,
    pub epsilon_mm: f64,
    pub gradient_epsilon: f64,
    pub max_iterations: i64,
    pub stable_iterations: i64,
    pub maximum_rms_change_mm: f64,
    pub maximum_time_step: f64,
    pub max_work: u64,
    pub progress: Option<&'a dyn Fn(ChanVeseIteration)>,
}
pub fn default_chan_vese_options<'a>() -> ChanVeseOptions<'a> {
    ChanVeseOptions {
        mu: 0.2,
        lambda_inside: 1.0,
        lambda_outside: 1.0,
        max_iterations: 200,
        stable_iterations: 10,
        maximum_rms_change_mm: 1e-4,
        maximum_time_step: 1.0,
        ..Default::default()
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ChanVeseIteration {
    pub iteration: i64,
    pub energy: f64,
    pub inside_mean: f64,
    pub outside_mean: f64,
    pub rms_change_mm: f64,
    pub time_step: f64,
    pub inside_voxels: i64,
    pub available_voxels: i64,
}
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ChanVeseResult {
    pub masks: BTreeMap<i64, RasterMask>,
    pub region_origin: [i64; 3],
    pub region_size: [i64; 3],
    pub history: Vec<ChanVeseIteration>,
    pub iterations: i64,
    pub stop_reason: String,
    pub converged: bool,
    pub estimated_peak_bytes: u64,
    pub touches_region_boundary: bool,
    pub touches_volume_boundary: bool,
}
struct CvState {
    size: [usize; 3],
    stride: [usize; 3],
    spacing: [f64; 3],
    measure: f64,
    image: Vec<f64>,
    phi: Vec<f64>,
    h: Vec<f64>,
    speed: Vec<f64>,
    flux: [Vec<f64>; 3],
    available: Vec<bool>,
}
fn finite(x: f64) -> bool {
    x.is_finite()
}
fn bound_add(a: u64, b: u64) -> Option<u64> {
    a.checked_add(b)
}
fn bound_mul(a: u64, b: u64) -> Option<u64> {
    a.checked_mul(b)
}
fn limit() -> Error {
    Error::sentinel(ERR_LEVEL_SET_RESOURCE_LIMIT)
}
fn invalid(s: &str) -> Error {
    Error::wrap(ERR_LEVEL_SET_INVALID_INPUT, format!("Chan-Vese {s}"))
}
pub fn chan_vese(
    ctx: Option<&dyn Cancellation>,
    v: LevelSetVolume,
    initial: &BTreeMap<i64, Option<RasterMask>>,
    o: ChanVeseOptions,
) -> Result<ChanVeseResult, Error> {
    let mut o = o;
    check(ctx)?;
    let sizes = [v.columns, v.rows, v.slices];
    let Some(value_at) = v.value_at else {
        return Err(invalid("missing image"));
    };
    if sizes.iter().any(|&n| n <= 0) {
        return Err(invalid("size"));
    }
    for (a, &h) in v.spacing_mm.iter().enumerate() {
        if a == 2 && v.slices == 1 {
            continue;
        }
        if !finite(h) || h <= 0.0 {
            return Err(invalid("spacing"));
        }
    }
    if o.region_size == [0; 3] {
        if o.region_origin != [0; 3] {
            return Err(invalid("region origin without size"));
        }
        o.region_size = sizes;
    }
    for a in 0..3 {
        let n = o.region_size[a];
        if n <= 0 || o.region_origin[a] < 0 || o.region_origin[a] > sizes[a] - n {
            return Err(invalid("region"));
        }
    }
    if o.max_iterations == 0 {
        o.max_iterations = 200;
    }
    if o.stable_iterations == 0 {
        o.stable_iterations = 10;
    }
    if o.maximum_time_step == 0.0 {
        o.maximum_time_step = 1.0;
    }
    if o.maximum_rms_change_mm == 0.0 {
        o.maximum_rms_change_mm = 1e-4;
    }
    let mut hmin = fmath::min(v.spacing_mm[0], v.spacing_mm[1]);
    if v.slices > 1 {
        hmin = fmath::min(hmin, v.spacing_mm[2]);
    }
    if o.epsilon_mm == 0.0 {
        o.epsilon_mm = hmin;
    }
    if o.gradient_epsilon == 0.0 {
        o.gradient_epsilon = 1e-6 / hmin;
    }
    if o.lambda_inside == 0.0 && o.lambda_outside == 0.0 {
        o.lambda_inside = 1.0;
        o.lambda_outside = 1.0;
    }
    for x in [
        o.mu,
        o.nu,
        o.lambda_inside,
        o.lambda_outside,
        o.epsilon_mm,
        o.gradient_epsilon,
        o.maximum_time_step,
        o.maximum_rms_change_mm,
    ] {
        if !finite(x) {
            return Err(invalid("nonfinite parameter"));
        }
    }
    if o.mu < 0.0
        || o.lambda_inside < 0.0
        || o.lambda_outside < 0.0
        || o.epsilon_mm <= 0.0
        || o.gradient_epsilon <= 0.0
        || o.maximum_time_step <= 0.0
        || o.maximum_rms_change_mm < 0.0
        || o.max_iterations < 1
        || o.max_iterations > 100000
        || o.stable_iterations < 1
        || o.stable_iterations > 100000
    {
        return Err(invalid("parameter range"));
    }
    let limits = o.limits.normalize()?;
    let mut n: u64 = 1;
    for &x in &o.region_size {
        n = bound_mul(n, x as u64).ok_or_else(limit)?;
    }
    if n > limits.max_voxels as u64 {
        return Err(limit());
    }
    let work = bound_mul(n, (o.max_iterations as u64 + 1) * 8);
    if o.max_work == 0 {
        o.max_work = 2_000_000_000;
    }
    let mut work = match work {
        Some(w) if w <= o.max_work => w,
        _ => return Err(limit()),
    };
    let mut bytes = bound_mul(n, 112).ok_or_else(limit)?;
    let rowbytes = bound_mul(v.rows as u64, o.region_size[2] as u64 * 24).ok_or_else(limit)?;
    for extra in [
        rowbytes,
        o.region_size[2] as u64 * 128,
        (o.max_iterations as u64 + 1) * 96,
        4096,
    ] {
        bytes = bound_add(bytes, extra).ok_or_else(limit)?;
    }
    if bytes > limits.max_bytes as u64 {
        return Err(limit());
    }
    let (ro, rs) = (o.region_origin, o.region_size);
    let mut input_work = bound_mul(initial.len() as u64, 2).ok_or_else(limit)?;
    for (&z, m) in initial {
        check(ctx)?;
        let m = match m {
            Some(m)
                if z >= 0
                    && z < v.slices
                    && m.columns == v.columns
                    && m.rows == v.rows
                    && m.runs.len() as i64 == m.rows =>
            {
                m
            }
            _ => return Err(invalid("mask grid/storage")),
        };
        if z < ro[2] || z >= ro[2] + rs[2] {
            continue;
        }
        for y in ro[1]..ro[1] + rs[1] {
            check(ctx)?;
            let run_work = bound_mul(m.runs[y as usize].len() as u64, 2).ok_or_else(limit)?;
            input_work = match bound_add(input_work, run_work + 1) {
                Some(w) if w <= o.max_work => w,
                _ => return Err(limit()),
            };
        }
    }
    work = match bound_add(work, input_work) {
        Some(w) if w <= o.max_work => w,
        _ => return Err(limit()),
    };
    let _ = work;
    for (&z, m) in initial {
        check(ctx)?;
        if z < ro[2] || z >= ro[2] + rs[2] {
            continue;
        }
        let Some(m) = m else { continue };
        for y in ro[1]..ro[1] + rs[1] {
            let mut end = 0;
            for (k, run) in m.runs[y as usize].iter().enumerate() {
                if k % 256 == 0 {
                    check(ctx)?;
                }
                if run.start < end || run.start < 0 || run.end <= run.start || run.end > v.columns {
                    return Err(invalid("mask runs"));
                }
                end = run.end;
            }
        }
    }
    let nn = n as usize;
    let size = [rs[0] as usize, rs[1] as usize, rs[2] as usize];
    let mut s = CvState {
        size,
        stride: [1, size[0], size[0] * size[1]],
        spacing: v.spacing_mm,
        measure: 0.0,
        image: vec![0.0; nn],
        phi: vec![0.0; nn],
        h: vec![0.0; nn],
        speed: vec![0.0; nn],
        flux: [vec![0.0; nn], vec![0.0; nn], vec![0.0; nn]],
        available: vec![false; nn],
    };
    if v.slices == 1 {
        s.spacing[2] = 1.0;
    }
    s.measure = s.spacing[0] * s.spacing[1];
    if v.slices > 1 {
        s.measure *= s.spacing[2];
    }
    if !finite(s.measure) || s.measure <= 0.0 {
        return Err(invalid("voxel measure"));
    }
    let (mut inside, mut total) = (0i64, 0i64);
    let empty: Vec<MaskRun> = Vec::new();
    for z in 0..size[2] {
        for y in 0..size[1] {
            let runs: &[MaskRun] = match initial.get(&(z as i64 + ro[2])) {
                Some(Some(m)) => &m.runs[y + ro[1] as usize],
                _ => &empty,
            };
            let mut cursor = 0usize;
            for x in 0..size[0] {
                let i = (z * size[1] + y) * size[0] + x;
                if i.is_multiple_of(256) {
                    check(ctx)?;
                }
                let (xx, yy, zz) = (x as i64 + ro[0], y as i64 + ro[1], z as i64 + ro[2]);
                let got = value_at(xx, yy, zz);
                check(ctx)?;
                let Some(value) = got else {
                    continue;
                };
                if !finite(value) || value.abs() > f64::from(f32::MAX) {
                    return Err(invalid("available value nonfinite/overflow"));
                }
                s.available[i] = true;
                s.image[i] = value;
                s.phi[i] = -2.0 * o.epsilon_mm;
                total += 1;
                while cursor < runs.len() && runs[cursor].end <= xx {
                    if cursor.is_multiple_of(256) {
                        check(ctx)?;
                    }
                    cursor += 1;
                }
                if cursor < runs.len() && runs[cursor].start <= xx {
                    s.phi[i] = 2.0 * o.epsilon_mm;
                    inside += 1;
                }
            }
        }
    }
    if inside == 0 {
        return Err(Error::wrap(ERR_LEVEL_SET_EMPTY, "Chan-Vese inside empty"));
    }
    if inside == total {
        return Err(invalid("outside empty"));
    }
    let mut r = ChanVeseResult {
        region_origin: ro,
        region_size: rs,
        estimated_peak_bytes: bytes,
        history: Vec::with_capacity(o.max_iterations as usize + 1),
        stop_reason: "iterations".into(),
        ..Default::default()
    };
    let stat = s.evaluate(ctx, &o, true)?;
    r.history.push(stat);
    let mut stable = 0i64;
    for it in 1..=o.max_iterations {
        let mut maxspeed = 0.0f64;
        for i in 0..nn {
            if i % 256 == 0 {
                check(ctx)?;
            }
            if !s.available[i] {
                continue;
            }
            let d = s.phi[i] / o.epsilon_mm;
            let delta = 1.0 / (std::f64::consts::PI * o.epsilon_mm * (1.0 + d * d));
            s.speed[i] *= delta;
            if !finite(s.speed[i]) {
                return Err(invalid("evolution overflow"));
            }
            maxspeed = fmath::max(maxspeed, s.speed[i].abs());
        }
        let mut dt = o.maximum_time_step;
        if maxspeed > 0.0 {
            dt = fmath::min(dt, 0.45 * hmin / maxspeed);
        }
        let (mut change, mut changed) = (0.0f64, 0i64);
        for i in 0..nn {
            if i % 256 == 0 {
                check(ctx)?;
            }
            if !s.available[i] {
                continue;
            }
            let old = s.phi[i];
            let step = dt * s.speed[i];
            s.phi[i] += step;
            if !finite(s.phi[i]) {
                return Err(invalid("level-set overflow"));
            }
            change += step * step;
            if (old >= 0.0) != (s.phi[i] >= 0.0) {
                changed += 1;
            }
        }
        let mut stat = s.evaluate(ctx, &o, true)?;
        stat.iteration = it;
        stat.time_step = dt;
        stat.rms_change_mm = (change / total as f64).sqrt();
        r.history.push(stat);
        r.iterations = it;
        if let Some(progress) = o.progress {
            progress(stat);
        }
        check(ctx)?;
        if changed == 0 {
            stable += 1;
        } else {
            stable = 0;
        }
        if stat.inside_voxels == 0 {
            r.stop_reason = "empty-inside".into();
        } else if stat.inside_voxels == total {
            r.stop_reason = "empty-outside".into();
        } else if stat.rms_change_mm <= o.maximum_rms_change_mm {
            r.stop_reason = "rms".into();
            r.converged = true;
        } else if stable >= o.stable_iterations
            && stat.rms_change_mm <= 10.0 * o.maximum_rms_change_mm
        {
            r.stop_reason = "stable-mask".into();
            r.converged = true;
        }
        if r.stop_reason != "iterations" {
            break;
        }
    }
    for z in 0..size[2] {
        check(ctx)?;
        let mut m = RasterMask::new(v.columns, v.rows);
        for y in 0..size[1] {
            check(ctx)?;
            let mut start: i64 = -1;
            for x in 0..size[0] {
                if x % 256 == 0 {
                    check(ctx)?;
                }
                let i = (z * size[1] + y) * size[0] + x;
                let on = s.available[i] && s.phi[i] >= 0.0;
                if on {
                    if start < 0 {
                        start = x as i64 + ro[0];
                    }
                    for (a, c) in [x, y, z].into_iter().enumerate() {
                        if !(a == 2 && v.slices == 1) && (c == 0 || c == size[a] - 1) {
                            r.touches_region_boundary = true;
                            let g = c as i64 + ro[a];
                            if g == 0 || g == sizes[a] - 1 {
                                r.touches_volume_boundary = true;
                            }
                        }
                    }
                }
                if start >= 0 && (!on || x + 1 == size[0]) {
                    let mut end = x as i64 + ro[0];
                    if on {
                        end += 1;
                    }
                    m.runs[y + ro[1] as usize].push(MaskRun { start, end });
                    start = -1;
                }
            }
        }
        r.masks.insert(z as i64 + ro[2], m);
    }
    check(ctx)?;
    Ok(r)
}
impl CvState {
    fn evaluate(
        &mut self,
        ctx: Option<&dyn Cancellation>,
        o: &ChanVeseOptions,
        forces: bool,
    ) -> Result<ChanVeseIteration, Error> {
        let mut st = ChanVeseIteration::default();
        let (mut sum_in, mut sum_out, mut mass_in, mut mass_out) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
        let (mut anchor, mut have_anchor) = (0.0f64, false);
        for i in 0..self.phi.len() {
            if i % 256 == 0 {
                check(ctx)?;
            }
            let p = self.phi[i];
            if !self.available[i] {
                self.h[i] = 0.0;
                continue;
            }
            if !finite(p) {
                return Err(Error::wrap(
                    ERR_LEVEL_SET_INVALID_INPUT,
                    "Chan-Vese level set",
                ));
            }
            let h = 0.5 + fmath::atan(p / o.epsilon_mm) / std::f64::consts::PI;
            self.h[i] = h;
            mass_in += h;
            mass_out += 1.0 - h;
            if !have_anchor {
                anchor = self.image[i];
                have_anchor = true;
            }
            sum_in += h * (self.image[i] - anchor);
            sum_out += (1.0 - h) * (self.image[i] - anchor);
            st.available_voxels += 1;
            if p >= 0.0 {
                st.inside_voxels += 1;
            }
        }
        if mass_in <= 0.0 || mass_out <= 0.0 {
            return Err(Error::wrap(
                ERR_LEVEL_SET_INVALID_INPUT,
                "Chan-Vese weighted phase empty",
            ));
        }
        st.inside_mean = anchor + sum_in / mass_in;
        st.outside_mean = anchor + sum_out / mass_out;
        for i in 0..self.h.len() {
            if i % 256 == 0 {
                check(ctx)?;
            }
            for f in self.flux.iter_mut() {
                f[i] = 0.0;
            }
            if !self.available[i] {
                self.speed[i] = 0.0;
                continue;
            }
            let coords = [
                i % self.size[0],
                (i / self.size[0]) % self.size[1],
                i / self.stride[2],
            ];
            let mut magnitude = o.gradient_epsilon * o.gradient_epsilon;
            for a in 0..3 {
                if coords[a] + 1 < self.size[a] && self.available[i + self.stride[a]] {
                    let g = (self.h[i + self.stride[a]] - self.h[i]) / self.spacing[a];
                    self.flux[a][i] = g;
                    magnitude += g * g;
                }
            }
            let norm = magnitude.sqrt();
            for f in self.flux.iter_mut() {
                f[i] /= norm;
            }
            let (din, dout) = (
                self.image[i] - st.inside_mean,
                self.image[i] - st.outside_mean,
            );
            let (fi, fo) = (o.lambda_inside * din * din, o.lambda_outside * dout * dout);
            st.energy += (o.mu * (norm - o.gradient_epsilon)
                + o.nu * self.h[i]
                + fi * self.h[i]
                + fo * (1.0 - self.h[i]))
                * self.measure;
            self.speed[i] = -o.nu - fi + fo;
        }
        if !finite(st.energy) || !finite(st.inside_mean) || !finite(st.outside_mean) {
            return Err(Error::wrap(
                ERR_LEVEL_SET_INVALID_INPUT,
                "Chan-Vese energy overflow",
            ));
        }
        if forces {
            for i in 0..self.h.len() {
                if i % 256 == 0 {
                    check(ctx)?;
                }
                if !self.available[i] {
                    continue;
                }
                let coords = [
                    i % self.size[0],
                    (i / self.size[0]) % self.size[1],
                    i / self.stride[2],
                ];
                let mut div = 0.0f64;
                for a in 0..3 {
                    if self.size[a] <= 1 {
                        continue;
                    }
                    div += self.flux[a][i] / self.spacing[a];
                    if coords[a] > 0 && self.available[i - self.stride[a]] {
                        div -= self.flux[a][i - self.stride[a]] / self.spacing[a];
                    }
                }
                self.speed[i] += o.mu * div;
            }
        }
        check(ctx)?;
        Ok(st)
    }
}
