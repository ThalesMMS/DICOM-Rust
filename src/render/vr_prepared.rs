#![forbid(unsafe_code)]
use super::error::{ErrorKind, RenderError};
use super::parallel::{Cancellation, ctx_err, parallel_rows_worker_context};
use super::volume::{Sampler, Volume, checked_voxel_count};
use super::vr_space_leap::{VrBlockGrid, build_vr_block_grid};
use super::vr_transfer::{VrPrefilterId, finite};
use crate::fmath;
use crate::textfmt::float_v;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, OnceLock};
type Ctx<'a> = Option<&'a dyn Cancellation>;
pub mod display_filter;
pub const VR_GAUSSIAN_PREFILTER_SIGMA_MM: f64 = 1.0;
const VR_GAUSSIAN_RADIUS_FACTOR: f64 = 3.0;
fn invalid_snapshot(detail: &str) -> RenderError {
    RenderError::new(
        ErrorKind::InvalidVolumeSnapshot,
        format!("render: invalid volume snapshot: {detail}"),
    )
}
fn check(ctx: Ctx<'_>) -> Result<(), RenderError> {
    match ctx_err(ctx) {
        Some(e) => Err(e),
        None => Ok(()),
    }
}
fn check_prefilter_input(values: &[f32], dims: [i64; 3]) -> Result<(), RenderError> {
    let count = checked_voxel_count(dims[0], dims[1], dims[2]);
    if dims[0] <= 0 || dims[1] <= 0 || dims[2] <= 0 || count != Some(values.len()) {
        return Err(invalid_snapshot(&format!(
            "prefilter volume of {} values for dimensions [{} {} {}]",
            values.len(),
            dims[0],
            dims[1],
            dims[2]
        )));
    }
    if values.iter().any(|v| v.is_nan() || v.is_infinite()) {
        return Err(invalid_snapshot("invalid source voxel"));
    }
    Ok(())
}
fn atomic_copy(values: &[f32]) -> Vec<AtomicU32> {
    values.iter().map(|v| AtomicU32::new(v.to_bits())).collect()
}
fn atomic_values(cells: Vec<AtomicU32>) -> Vec<f32> {
    cells
        .into_iter()
        .map(|c| f32::from_bits(c.into_inner()))
        .collect()
}
pub fn smooth_gaussian_3d(
    ctx: Ctx<'_>,
    values: &[f32],
    dims: [i64; 3],
    spacing_mm: [f64; 3],
    sigma_mm: f64,
) -> Result<Vec<f32>, RenderError> {
    check_prefilter_input(values, dims)?;
    if !(sigma_mm > 0.0) || sigma_mm.is_infinite() {
        return Err(RenderError::other(format!(
            "render: invalid Gaussian sigma {} mm",
            float_v(sigma_mm)
        )));
    }
    let out = atomic_copy(values);
    let strides = [1i64, dims[0], dims[0] * dims[1]];
    for axis in 0..3 {
        if !(spacing_mm[axis] > 0.0) {
            return Err(RenderError::other(format!(
                "render: invalid voxel spacing {} mm",
                float_v(spacing_mm[axis])
            )));
        }
        let sigma = sigma_mm / spacing_mm[axis];
        let mut radius = fmath::f64_to_i64(sigma * VR_GAUSSIAN_RADIUS_FACTOR);
        let n = dims[axis];
        if sigma < 0.01 || radius < 1 || n < 2 {
            continue;
        }
        radius = radius.min(n - 1);
        let weights: Vec<f64> = (0..2 * radius + 1)
            .map(|k| {
                let x = (k - radius) as f64;
                fmath::exp(-x * x / (2.0 * sigma * sigma))
            })
            .collect();
        let (a, b) = ((axis + 1) % 3, (axis + 2) % 3);
        let out_ref = &out;
        let weights_ref = &weights;
        parallel_rows_worker_context(ctx, dims[b], || {
            let mut line = vec![0.0f64; n as usize];
            move |j: i64| {
                for i in 0..dims[a] {
                    let start = i * strides[a] + j * strides[b];
                    for k in 0..n {
                        line[k as usize] = f64::from(f32::from_bits(
                            out_ref[(start + k * strides[axis]) as usize].load(Ordering::Relaxed),
                        ));
                    }
                    for k in 0..n {
                        let (lo, hi) = ((k - radius).max(0), (k + radius).min(n - 1));
                        let (mut sum, mut weight) = (0.0f64, 0.0f64);
                        for m in lo..=hi {
                            let w = weights_ref[(m - k + radius) as usize];
                            sum += w * line[m as usize];
                            weight += w;
                        }
                        out_ref[(start + k * strides[axis]) as usize]
                            .store(((sum / weight) as f32).to_bits(), Ordering::Relaxed);
                    }
                }
            }
        })?;
    }
    Ok(atomic_values(out))
}
pub fn median_3x3x3(ctx: Ctx<'_>, values: &[f32], dims: [i64; 3]) -> Result<Vec<f32>, RenderError> {
    check_prefilter_input(values, dims)?;
    let out: Vec<AtomicU32> = (0..values.len()).map(|_| AtomicU32::new(0)).collect();
    let (nx, ny, nz) = (dims[0], dims[1], dims[2]);
    let out_ref = &out;
    parallel_rows_worker_context(ctx, nz, || {
        let mut hood = [0f32; 27];
        move |z: i64| {
            for y in 0..ny {
                for x in 0..nx {
                    let mut count = 0usize;
                    for zz in (z - 1).max(0)..=(z + 1).min(nz - 1) {
                        for yy in (y - 1).max(0)..=(y + 1).min(ny - 1) {
                            let row = (zz * ny + yy) * nx;
                            for xx in (x - 1).max(0)..=(x + 1).min(nx - 1) {
                                hood[count] = values[(row + xx) as usize];
                                count += 1;
                            }
                        }
                    }
                    let h = &mut hood[..count];
                    h.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                    let mut median = h[count / 2];
                    if count.is_multiple_of(2) {
                        let low = h[count / 2 - 1];
                        median = low + (median - low) / 2.0;
                    }
                    out_ref[((z * ny + y) * nx + x) as usize]
                        .store(median.to_bits(), Ordering::Relaxed);
                }
            }
        }
    })?;
    Ok(atomic_values(out))
}
fn sampler_modality_values(ctx: Ctx<'_>, sampler: &Sampler) -> Result<Vec<f32>, RenderError> {
    let Some(count) = checked_voxel_count(sampler.cols, sampler.rows, sampler.depth) else {
        return Err(invalid_snapshot("prepared volume is too large"));
    };
    if let Some(values) = sampler.f32_values()
        && values.len() == count
    {
        return Ok(values.to_vec());
    }
    let mut values = vec![0f32; count];
    for z in 0..sampler.depth {
        check(ctx)?;
        for y in 0..sampler.rows {
            for x in 0..sampler.cols {
                let Some(value) = sampler.value_at(x, y, z) else {
                    return Err(invalid_snapshot("invalid source voxel"));
                };
                values[((z * sampler.rows + y) * sampler.cols + x) as usize] = value as f32;
            }
        }
    }
    Ok(values)
}
pub(crate) const BASIC_SMOOTH_5X5_KERNEL: [[f64; 5]; 5] = [
    [1.0, 1.0, 1.0, 1.0, 1.0],
    [1.0, 4.0, 4.0, 4.0, 1.0],
    [1.0, 4.0, 12.0, 4.0, 1.0],
    [1.0, 4.0, 4.0, 4.0, 1.0],
    [1.0, 1.0, 1.0, 1.0, 1.0],
];
fn basic_smooth_5x5(ctx: Ctx<'_>, sampler: &Sampler) -> Result<Vec<f32>, RenderError> {
    if sampler.cols <= 0 || sampler.rows <= 0 || sampler.depth <= 0 {
        return Err(RenderError::new(
            ErrorKind::InvalidVolumeSnapshot,
            "render: invalid volume snapshot",
        ));
    }
    let Some(count) = checked_voxel_count(sampler.cols, sampler.rows, sampler.depth) else {
        return Err(invalid_snapshot("prepared volume is too large"));
    };
    let values: Vec<AtomicU32> = (0..count).map(|_| AtomicU32::new(0)).collect();
    let invalid = AtomicBool::new(false);
    let (values_ref, invalid_ref) = (&values, &invalid);
    parallel_rows_worker_context(ctx, sampler.depth, || {
        move |z: i64| {
            for y in 0..sampler.rows {
                if invalid_ref.load(Ordering::Relaxed) || ctx_err(ctx).is_some() {
                    return;
                }
                for x in 0..sampler.cols {
                    let mut sum = 0.0f64;
                    for ky in -2i64..=2 {
                        let sy = (y + ky).max(0).min(sampler.rows - 1);
                        for kx in -2i64..=2 {
                            let sx = (x + kx).max(0).min(sampler.cols - 1);
                            match sampler.value_at(sx, sy, z) {
                                Some(value) if finite(value) => {
                                    sum += value
                                        * BASIC_SMOOTH_5X5_KERNEL[(ky + 2) as usize]
                                            [(kx + 2) as usize];
                                }
                                _ => {
                                    invalid_ref.store(true, Ordering::Relaxed);
                                    return;
                                }
                            }
                        }
                    }
                    values_ref[((z * sampler.rows + y) * sampler.cols + x) as usize]
                        .store(((sum / 60.0) as f32).to_bits(), Ordering::Relaxed);
                }
            }
        }
    })?;
    if invalid.load(Ordering::Relaxed) {
        return Err(invalid_snapshot("invalid source voxel"));
    }
    Ok(atomic_values(values))
}
#[derive(Debug)]
pub struct VrPreparedVolume {
    pub prefilter: VrPrefilterId,
    pub volume: Arc<Volume>,
    values: Option<Arc<Vec<f32>>>,
    hu_range: OnceLock<(f64, f64)>,
    blocks: OnceLock<VrBlockGrid>,
}
impl Volume {
    pub fn prepare_vr_volume(
        self: &Arc<Self>,
        ctx: Ctx<'_>,
        prefilter: VrPrefilterId,
    ) -> Result<Arc<VrPreparedVolume>, RenderError> {
        check(ctx)?;
        if prefilter.0 > VrPrefilterId::VESSEL_ENHANCE.0 {
            return Err(RenderError::other(format!(
                "render: unsupported VR prefilter {}",
                prefilter.0
            )));
        }
        let grid = self.regular_grid()?;
        grid.payload()?;
        let mut values = None;
        if prefilter != VrPrefilterId::NONE {
            let sampler = Sampler::direct(grid.clone())?;
            let filtered = match prefilter {
                VrPrefilterId::GAUSSIAN_1MM | VrPrefilterId::MEDIAN_3X3X3 => {
                    let source = sampler_modality_values(ctx, &sampler)?;
                    let dims = [sampler.cols, sampler.rows, sampler.depth];
                    if prefilter == VrPrefilterId::MEDIAN_3X3X3 {
                        median_3x3x3(ctx, &source, dims)?
                    } else {
                        let spacing = [
                            grid.col_spacing,
                            grid.row_spacing,
                            grid.effective_slice_spacing(),
                        ];
                        smooth_gaussian_3d(
                            ctx,
                            &source,
                            dims,
                            spacing,
                            VR_GAUSSIAN_PREFILTER_SIGMA_MM,
                        )?
                    }
                }
                VrPrefilterId::VESSEL_ENHANCE => {
                    let source = sampler_modality_values(ctx, &sampler)?;
                    let dims = [sampler.cols, sampler.rows, sampler.depth];
                    let spacing = [
                        grid.col_spacing,
                        grid.row_spacing,
                        grid.effective_slice_spacing(),
                    ];
                    super::vr_vessel_enhance::vessel_enhance(ctx, &source, dims, spacing)?
                }
                _ => basic_smooth_5x5(ctx, &sampler)?,
            };
            check(ctx)?;
            let components = u64::from(grid.components());
            if components != 1 {
                let row = grid.cols as u64 * 4;
                return Err(invalid_snapshot(&format!(
                    "row stride {} below packed row {}",
                    row,
                    row * components
                )));
            }
            values = Some(Arc::new(filtered));
        }
        check(ctx)?;
        Ok(Arc::new(VrPreparedVolume {
            prefilter,
            volume: grid,
            values,
            hu_range: OnceLock::new(),
            blocks: OnceLock::new(),
        }))
    }
}
impl VrPreparedVolume {
    pub(crate) fn new_sampler(&self) -> Result<Sampler, RenderError> {
        match &self.values {
            Some(values) => Ok(Sampler::shared(self.volume.clone(), values.clone())),
            None => Sampler::direct(self.volume.clone()),
        }
    }
    pub fn hu_range(&self) -> Option<(f64, f64)> {
        if self.prefilter == VrPrefilterId::NONE {
            return self.volume.hu_range();
        }
        if let Some(range) = self.hu_range.get() {
            return Some(*range);
        }
        let sampler = self.new_sampler().ok()?;
        let (mut minimum, mut maximum) = (f64::INFINITY, f64::NEG_INFINITY);
        let mut found = false;
        for z in 0..sampler.depth {
            for y in 0..sampler.rows {
                for x in 0..sampler.cols {
                    let Some(value) = sampler.value_at(x, y, z) else {
                        continue;
                    };
                    if !finite(value) {
                        continue;
                    }
                    minimum = fmath::min(minimum, value);
                    maximum = fmath::max(maximum, value);
                    found = true;
                }
            }
        }
        if !found {
            return None;
        }
        if maximum <= minimum {
            minimum -= 1.0;
            maximum += 1.0;
        }
        Some(*self.hu_range.get_or_init(|| (minimum, maximum)))
    }
    pub(crate) fn block_grid(
        &self,
        ctx: Ctx<'_>,
        sampler: &Sampler,
    ) -> Result<&VrBlockGrid, RenderError> {
        if let Some(grid) = self.blocks.get() {
            return Ok(grid);
        }
        let grid = build_vr_block_grid(ctx, sampler)?;
        Ok(self.blocks.get_or_init(|| grid))
    }
    pub fn prefiltered_values(&self) -> Option<&[f32]> {
        self.values.as_deref().map(|v| v.as_slice())
    }
}
