#![forbid(unsafe_code)]
use super::error::RenderError;
use super::parallel::{Cancellation, ctx_err, parallel_rows_worker_context};
use super::vec3::Vec3;
use super::volume::Sampler;
use super::vr_gradient_opacity::VrGradientOpacity;
use super::vr_labels::{VR_BLOCK_LABEL_OPACITY, VR_BLOCK_VISIBLE_LABEL, VrLabelMap};
use super::vr_projection::VrValueRange;
use super::vr_raycast::normalized_vr_data_coordinate;
use super::vr_transfer::{
    VrLut, VrMode, VrPreset, VrTransferDomain, VrTransferFunction, clamp_unit, finite,
};
use super::vr_transfer_2d::VrTransfer2d;
use super::window::{WindowLevel, windowed_unit};
use crate::fmath;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
const VR_BLOCK_SHIFT: i64 = 3;
const VR_BLOCK_SIZE: i64 = 1 << VR_BLOCK_SHIFT;
const VR_LEAP_MARGIN: f64 = 1e-6;
type Ctx<'a> = Option<&'a dyn Cancellation>;
#[derive(Debug)]
pub(crate) struct VrBlockGrid {
    pub(crate) nx: i64,
    pub(crate) ny: i64,
    pub(crate) nz: i64,
    pub(crate) min: Vec<f32>,
    pub(crate) max: Vec<f32>,
}
fn next_down32(x: f32) -> f32 {
    if x == 0.0 {
        return -f32::from_bits(1);
    }
    if x > 0.0 {
        f32::from_bits(x.to_bits() - 1)
    } else {
        f32::from_bits(x.to_bits() + 1)
    }
}
fn next_up32(x: f32) -> f32 {
    if x == 0.0 {
        return f32::from_bits(1);
    }
    if x > 0.0 {
        f32::from_bits(x.to_bits() + 1)
    } else {
        f32::from_bits(x.to_bits() - 1)
    }
}
pub(crate) fn widen(minimum: f64, maximum: f64) -> (f64, f64) {
    (
        minimum - 1e-9 * (1.0 + minimum.abs()),
        maximum + 1e-9 * (1.0 + maximum.abs()),
    )
}
fn interpolation_bounds(lo: f64, hi: f64) -> (f64, f64) {
    if lo == hi && finite(lo) {
        return (lo, hi);
    }
    widen(lo, hi)
}
pub(crate) fn build_vr_block_grid(ctx: Ctx<'_>, s: &Sampler) -> Result<VrBlockGrid, RenderError> {
    let blocks = |n: i64| (n + VR_BLOCK_SIZE - 1) >> VR_BLOCK_SHIFT;
    let (nx, ny, nz) = (blocks(s.cols), blocks(s.rows), blocks(s.depth));
    let count = (nx * ny * nz) as usize;
    let min_cells: Vec<AtomicU32> = (0..count).map(|_| AtomicU32::new(0)).collect();
    let max_cells: Vec<AtomicU32> = (0..count).map(|_| AtomicU32::new(0)).collect();
    let span = |b: i64, n: i64| {
        (
            b << VR_BLOCK_SHIFT,
            ((b << VR_BLOCK_SHIFT) + VR_BLOCK_SIZE).min(n - 1),
        )
    };
    let values = s.f32_values();
    parallel_rows_worker_context(ctx, nz, || {
        |bz: i64| {
            let (z0, z1) = span(bz, s.depth);
            for by in 0..ny {
                let (y0, y1) = span(by, s.rows);
                for bx in 0..nx {
                    let (x0, x1) = span(bx, s.cols);
                    let (mut minimum, mut maximum) = (f64::INFINITY, f64::NEG_INFINITY);
                    let mut finite_block = true;
                    let mut z = z0;
                    while z <= z1 && finite_block {
                        let mut y = y0;
                        while y <= y1 && finite_block {
                            if let Some(values) = values.filter(|v| !v.is_empty()) {
                                let base = ((z * s.rows + y) * s.cols) as usize;
                                for v32 in &values[base + x0 as usize..=base + x1 as usize] {
                                    let v = f64::from(*v32);
                                    if v.is_nan() || v.is_infinite() {
                                        finite_block = false;
                                        break;
                                    }
                                    if v < minimum {
                                        minimum = v;
                                    }
                                    if v > maximum {
                                        maximum = v;
                                    }
                                }
                                y += 1;
                                continue;
                            }
                            for x in x0..=x1 {
                                let v = s.value_or_zero(x, y, z);
                                if !finite(v) {
                                    finite_block = false;
                                    break;
                                }
                                minimum = fmath::min(minimum, v);
                                maximum = fmath::max(maximum, v);
                            }
                            y += 1;
                        }
                        z += 1;
                    }
                    let i = ((bz * ny + by) * nx + bx) as usize;
                    if !finite_block {
                        min_cells[i].store(f32::NAN.to_bits(), Ordering::Relaxed);
                        max_cells[i].store(f32::NAN.to_bits(), Ordering::Relaxed);
                        continue;
                    }
                    let mut lo = minimum as f32;
                    if f64::from(lo) > minimum {
                        lo = next_down32(lo);
                    }
                    let mut hi = maximum as f32;
                    if f64::from(hi) < maximum {
                        hi = next_up32(hi);
                    }
                    min_cells[i].store(lo.to_bits(), Ordering::Relaxed);
                    max_cells[i].store(hi.to_bits(), Ordering::Relaxed);
                }
            }
        }
    })?;
    let load = |cells: Vec<AtomicU32>| -> Vec<f32> {
        cells
            .into_iter()
            .map(|c| f32::from_bits(c.into_inner()))
            .collect()
    };
    Ok(VrBlockGrid {
        nx,
        ny,
        nz,
        min: load(min_cells),
        max: load(max_cells),
    })
}
impl VrBlockGrid {
    pub(crate) fn physical_gradient_bounds(
        &self,
        ctx: Ctx<'_>,
        s: &Sampler,
    ) -> Result<Vec<f64>, RenderError> {
        if let Some(e) = ctx_err(ctx) {
            return Err(e);
        }
        let bounds: Vec<AtomicU64> = (0..self.min.len()).map(|_| AtomicU64::new(0)).collect();
        let span = |b: i64, n: i64| {
            (
                (b * VR_BLOCK_SIZE - 1).max(0),
                (n - 1).min(b * VR_BLOCK_SIZE + VR_BLOCK_SIZE + 1),
            )
        };
        let mut factor = 0.0;
        for (direction, spacing) in [
            (s.vol.axis_x, s.vol.col_spacing),
            (s.vol.axis_y, s.vol.row_spacing),
            (s.vol.normal, s.vol.slice_spacing),
        ] {
            if spacing > 0.0 {
                factor += 0.5 * direction.length() / spacing;
            }
        }
        let (nx, ny) = (self.nx, self.ny);
        parallel_rows_worker_context(ctx, self.nz, || {
            |bz: i64| {
                let (z0, z1) = span(bz, s.depth);
                for by in 0..ny {
                    let (y0, y1) = span(by, s.rows);
                    for bx in 0..nx {
                        if ctx_err(ctx).is_some() {
                            return;
                        }
                        let (x0, x1) = span(bx, s.cols);
                        let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
                        for z in z0..=z1 {
                            for y in y0..=y1 {
                                for x in x0..=x1 {
                                    let value = s.value_or_zero(x, y, z);
                                    if !finite(value) {
                                        lo = f64::NEG_INFINITY;
                                        hi = f64::INFINITY;
                                        break;
                                    }
                                    lo = fmath::min(lo, value);
                                    hi = fmath::max(hi, value);
                                }
                            }
                        }
                        let index = ((bz * ny + by) * nx + bx) as usize;
                        if lo == hi {
                            continue;
                        }
                        let (lo, hi) = widen(lo, hi);
                        let mut bound = (hi - lo) * factor;
                        if !finite(bound) || factor <= 0.0 {
                            bound = f64::INFINITY;
                        }
                        if bound == 0.0 {
                            bound = f64::from_bits(1);
                        }
                        bounds[index].store(bound.to_bits(), Ordering::Relaxed);
                    }
                }
            }
        })?;
        Ok(bounds
            .into_iter()
            .map(|c| f64::from_bits(c.into_inner()))
            .collect())
    }
}
#[derive(Debug)]
pub(crate) struct VrSpaceLeap<'a> {
    pub(crate) grid: &'a VrBlockGrid,
    scale: Vec3,
    pub(crate) empty: Option<Vec<bool>>,
    window: WindowLevel,
    monotone: bool,
    value_range: VrValueRange,
}
fn gradient_opacity_zero_until(function: &VrGradientOpacity, maximum: f64) -> bool {
    if !finite(maximum) || maximum < 0.0 || function.at(0.0) != 0.0 || function.at(maximum) != 0.0 {
        return false;
    }
    !function
        .0
        .iter()
        .any(|p| p.magnitude >= 0.0 && p.magnitude <= maximum && p.opacity != 0.0)
}
impl<'a> VrSpaceLeap<'a> {
    pub(crate) fn new(grid: &'a VrBlockGrid, s: &Sampler) -> VrSpaceLeap<'a> {
        VrSpaceLeap {
            grid,
            scale: Vec3::new(
                (s.cols - 1) as f64,
                (s.rows - 1) as f64,
                (s.depth - 1) as f64,
            ),
            empty: None,
            window: WindowLevel::default(),
            monotone: false,
            value_range: VrValueRange::default(),
        }
    }
    pub(crate) fn for_dvr(
        &mut self,
        tf: &VrTransferFunction,
        lut: &VrLut,
        window: &WindowLevel,
        data_min: f64,
        data_max: f64,
    ) {
        let n = lut.len();
        if n == 0 {
            return;
        }
        let mut opaque = vec![0i64; n + 1];
        for i in 0..n {
            opaque[i + 1] = opaque[i];
            if !(lut.at(i as i64).a == 0.0) {
                opaque[i + 1] += 1;
            }
        }
        let coordinate = |v: f64| {
            if tf.domain() == VrTransferDomain::Normalized {
                return tf.window_coordinate(v, window);
            }
            normalized_vr_data_coordinate(v, data_min, data_max)
        };
        let index = |c: f64| -> i64 {
            if c <= 0.0 {
                return 0;
            }
            if c >= 1.0 {
                return n as i64 - 1;
            }
            fmath::f64_to_i64((c * (n - 1) as f64).floor())
        };
        let mut empty = vec![false; self.grid.min.len()];
        for (i, cell) in empty.iter_mut().enumerate() {
            let (lo, hi) = (f64::from(self.grid.min[i]), f64::from(self.grid.max[i]));
            if !finite(lo) || !finite(hi) {
                continue;
            }
            let (lo, hi) = interpolation_bounds(lo, hi);
            let (c0, c1) = (coordinate(lo), coordinate(hi));
            if !finite(c0) || !finite(c1) || c0 > c1 {
                continue;
            }
            let first = index(c0);
            let mut last = index(c1);
            if c1 > 0.0 && c1 < 1.0 {
                last = fmath::f64_to_i64((c1 * (n - 1) as f64).ceil());
            }
            if last > n as i64 - 1 {
                last = n as i64 - 1;
            }
            *cell = opaque[last as usize + 1] - opaque[first as usize] == 0;
        }
        self.empty = Some(empty);
    }
    pub(crate) fn for_dvr_2d(&mut self, transfer: &VrTransfer2d, gradients: Option<&[f64]>) {
        let (width, _) = transfer.size();
        let n = width as usize;
        let column_max = transfer.column_max_alpha();
        let mut opaque = vec![0i64; n + 1];
        for (i, alpha) in column_max.iter().enumerate() {
            opaque[i + 1] = opaque[i];
            if !(*alpha == 0.0) {
                opaque[i + 1] += 1;
            }
        }
        let mut empty = vec![false; self.grid.min.len()];
        for (i, cell) in empty.iter_mut().enumerate() {
            let (lo, hi) = (f64::from(self.grid.min[i]), f64::from(self.grid.max[i]));
            if !finite(lo) || !finite(hi) {
                continue;
            }
            let (lo, hi) = interpolation_bounds(lo, hi);
            let first = fmath::f64_to_i64(transfer.column_position(lo));
            let mut last = fmath::f64_to_i64(transfer.column_position(hi).ceil());
            if last > width - 1 {
                last = width - 1;
            }
            *cell = opaque[last as usize + 1] - opaque[first as usize] == 0;
        }
        self.empty = Some(empty);
        if let Some(bounds) = gradients {
            self.limit_transfer_2d_gradients(transfer, bounds);
        }
    }
    pub(crate) fn for_projection(&mut self, window: &WindowLevel) {
        self.window = window.clone();
        self.monotone = window.width > 0.0 && finite(window.center) && finite(window.width);
    }
    pub(crate) fn for_value_range(&mut self, value_range: VrValueRange) {
        self.value_range = value_range;
    }
    pub(crate) fn limit_gradient_opacity(&mut self, preset: &VrPreset, bounds: Option<&[f64]>) {
        let (Some(empty), Some(bounds)) = (self.empty.as_mut(), bounds) else {
            return;
        };
        if bounds.len() != empty.len() {
            return;
        }
        for (i, maximum) in bounds.iter().enumerate() {
            if !preset.gradient_opacity.is_empty() {
                if gradient_opacity_zero_until(&preset.gradient_opacity, *maximum) {
                    empty[i] = true;
                }
            } else if preset.gradient_opacity_scale > 0.0 && *maximum == 0.0 {
                empty[i] = true;
            }
        }
    }
    pub(crate) fn limit_labels(&mut self, labels: Option<&VrLabelMap>) {
        let (Some(empty), Some(m)) = (self.empty.as_mut(), labels) else {
            return;
        };
        let flags = m.block_flags();
        if flags.len() != empty.len() {
            return;
        }
        for (i, f) in flags.iter().enumerate() {
            if m.only_inside() && f & VR_BLOCK_VISIBLE_LABEL == 0 {
                empty[i] = true;
                continue;
            }
            if f & VR_BLOCK_LABEL_OPACITY != 0 {
                empty[i] = false;
            }
        }
    }
    fn limit_transfer_2d_gradients(&mut self, t: &VrTransfer2d, bounds: &[f64]) {
        let Some(empty) = self.empty.as_mut() else {
            return;
        };
        if bounds.len() != empty.len() {
            return;
        }
        let (width, height) = t.size();
        let (w, h) = (width as usize, height as usize);
        let stride = w + 1;
        let mut prefix = vec![0i64; (h + 1) * stride];
        for y in 0..h {
            let mut row = 0i64;
            for x in 0..w {
                if t.at(x as i64, y as i64).a != 0.0 {
                    row += 1;
                }
                prefix[(y + 1) * stride + x + 1] = prefix[y * stride + x + 1] + row;
            }
        }
        for (i, maximum) in bounds.iter().enumerate() {
            let (gmin, gmax) = (f64::from(self.grid.min[i]), f64::from(self.grid.max[i]));
            if !finite(*maximum) || !finite(gmin) || !finite(gmax) {
                continue;
            }
            let (lo, hi) = interpolation_bounds(gmin, gmax);
            let first = fmath::f64_to_i64(t.column_position(lo)) as usize;
            let last = fmath::f64_to_i64(t.column_position(hi).ceil()).min(width - 1) as usize;
            let row = fmath::f64_to_i64(
                (clamp_unit(*maximum / t.gradient_max()) * (height - 1) as f64).ceil(),
            )
            .min(height - 1) as usize;
            let count = prefix[(row + 1) * stride + last + 1] - prefix[(row + 1) * stride + first];
            if count == 0 {
                empty[i] = true;
            }
        }
    }
    pub(crate) fn block(&self, tex: Vec3) -> i64 {
        let x = fmath::f64_to_i64((tex.x * self.scale.x).floor());
        let y = fmath::f64_to_i64((tex.y * self.scale.y).floor());
        let z = fmath::f64_to_i64((tex.z * self.scale.z).floor());
        if x < 0 || y < 0 || z < 0 {
            return -1;
        }
        let (x, y, z) = (
            x >> VR_BLOCK_SHIFT,
            y >> VR_BLOCK_SHIFT,
            z >> VR_BLOCK_SHIFT,
        );
        if x >= self.grid.nx || y >= self.grid.ny || z >= self.grid.nz {
            return -1;
        }
        (z * self.grid.ny + y) * self.grid.nx + x
    }
    fn skips_projection(&self, block: i64, mode: VrMode, acc: f64) -> bool {
        if !self.monotone {
            return false;
        }
        let (lo, hi) = (
            f64::from(self.grid.min[block as usize]),
            f64::from(self.grid.max[block as usize]),
        );
        if lo.is_nan() {
            return false;
        }
        let (lo, hi) = widen(lo, hi);
        match mode {
            VrMode::MIP => windowed_unit(hi, &self.window) <= acc,
            VrMode::MIN_IP => windowed_unit(lo, &self.window) >= acc,
            _ => false,
        }
    }
    fn outside_value_range(&self, block: i64) -> bool {
        if !self.value_range.enabled {
            return false;
        }
        let (lo, hi) = (
            f64::from(self.grid.min[block as usize]),
            f64::from(self.grid.max[block as usize]),
        );
        if lo.is_nan() {
            return false;
        }
        let (lo, hi) = widen(lo, hi);
        hi < self.value_range.low || lo > self.value_range.high
    }
    pub(crate) fn skips_projection_sample(
        &self,
        block: i64,
        mode: VrMode,
        acc: f64,
        count: i64,
    ) -> bool {
        match mode {
            VrMode::MIP | VrMode::MIN_IP => count > 0 && self.skips_projection(block, mode, acc),
            VrMode::AVERAGE | VrMode::RAY_SUM => self.outside_value_range(block),
            _ => false,
        }
    }
    pub(crate) fn leap(&self, block: i64, a: Vec3, b: Vec3, t: f64, t1: f64, step: f64) -> f64 {
        let bx = block % self.grid.nx;
        let by = (block / self.grid.nx) % self.grid.ny;
        let bz = block / (self.grid.nx * self.grid.ny);
        let (mut t_in, mut t_out) = (f64::NEG_INFINITY, f64::INFINITY);
        for axis in [
            [a.x, b.x, bx as f64],
            [a.y, b.y, by as f64],
            [a.z, b.z, bz as f64],
        ] {
            let lower = axis[2] * VR_BLOCK_SIZE as f64 + VR_LEAP_MARGIN;
            let upper = (axis[2] + 1.0) * VR_BLOCK_SIZE as f64 - VR_LEAP_MARGIN;
            if axis[1] == 0.0 {
                if axis[0] < lower || axis[0] > upper {
                    return t;
                }
                continue;
            }
            let (mut enter, mut exit) = ((lower - axis[0]) / axis[1], (upper - axis[0]) / axis[1]);
            if enter > exit {
                std::mem::swap(&mut enter, &mut exit);
            }
            t_in = fmath::max(t_in, enter);
            t_out = fmath::min(t_out, exit);
        }
        if t < t_in {
            return t;
        }
        let mut t = t;
        loop {
            let next = t + step;
            if next >= t_out || next > t1 {
                return t;
            }
            t = next;
        }
    }
    pub(crate) fn ray_voxel(&self, o: Vec3, d: Vec3, box_min: Vec3, size: Vec3) -> (Vec3, Vec3) {
        let a = Vec3::new(
            (o.x - box_min.x) / size.x * self.scale.x,
            (o.y - box_min.y) / size.y * self.scale.y,
            (o.z - box_min.z) / size.z * self.scale.z,
        );
        let b = Vec3::new(
            d.x / size.x * self.scale.x,
            d.y / size.y * self.scale.y,
            d.z / size.z * self.scale.z,
        );
        (a, b)
    }
}
