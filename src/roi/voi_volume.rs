use super::voi::{
    ERR_EMPTY_VOI, IndexBox, MAX_VOI_VOXELS, VoiMask, VoiPrimitive, VoiRasterOptions,
    VoiStatistics, VoxelGrid, VoxelValueFn, finite_positive, rasterize_voi,
    sphere_radius_for_volume, voxel_grid_from_descriptor,
};
use super::voi_peak::{DEFAULT_PEAK_VOLUME_MM3, PeakOptions, PeakResult};
use super::{Cancellation, Error, ctx_err};
use crate::fmath;
use crate::render::volume::Volume;
use crate::render::volume_snapshot::VolumeSnapshot;
use std::sync::Arc;
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct VolumeVoiOptions {
    pub raster: VoiRasterOptions,
    pub peak: Option<PeakOptions>,
}
#[derive(Debug, Clone, Default)]
pub struct VolumeVoiResult {
    pub mask: Option<VoiMask>,
    pub statistics: VoiStatistics,
    pub peak: Option<PeakResult>,
    pub peak_err: Option<Error>,
}
pub fn volume_voi(
    ctx: Option<&dyn Cancellation>,
    volume: Option<&Arc<Volume>>,
    primitive: Option<Arc<dyn VoiPrimitive>>,
    opts: VolumeVoiOptions,
) -> (VolumeVoiResult, Option<Error>) {
    let fail = |err: Error| (VolumeVoiResult::default(), Some(err));
    let Some(volume) = volume else {
        return fail(Error::msg("roi: volume VOI needs a volume"));
    };
    let snapshot = match volume.acquire_snapshot_context(ctx) {
        Ok(s) => s,
        Err(err) => return fail(Error::from(err)),
    };
    let descriptor = snapshot.descriptor();
    let (grid, err) = voxel_grid_from_descriptor(&descriptor);
    if let Some(err) = err {
        return fail(err);
    }
    let mask = match rasterize_voi(ctx, grid, primitive, opts.raster) {
        Ok(m) => m,
        Err(err) => return fail(err),
    };
    let mut result = VolumeVoiResult {
        mask: Some(mask),
        ..VolumeVoiResult::default()
    };
    let Some(mask) = result.mask.as_ref() else {
        return (result, None);
    };
    if mask.voxels.is_empty() {
        return (result, Some(Error::sentinel(ERR_EMPTY_VOI)));
    }
    let mut margin = 0.0f64;
    if let Some(peak) = &opts.peak {
        let mut volume_mm3 = peak.volume_mm3;
        if volume_mm3 == 0.0 {
            volume_mm3 = DEFAULT_PEAK_VOLUME_MM3;
        }
        if finite_positive(volume_mm3) {
            margin = sphere_radius_for_volume(volume_mm3);
        }
    }
    let mut value_at = match cached_voxel_values(ctx, &snapshot, grid, mask, margin) {
        Ok(f) => f,
        Err(err) => return (result, Some(err)),
    };
    let (statistics, err) = mask.statistics(Some(&mut value_at));
    let mut peak_result: Option<PeakResult> = None;
    let mut peak_err: Option<Error> = None;
    let mut failure = err;
    if failure.is_none()
        && let Some(peak_opts) = opts.peak
    {
        let (peak, err) = mask.peak(ctx, Some(&mut value_at), peak_opts);
        match err {
            Some(err) => {
                if let Some(ctx_error) = ctx_err(ctx) {
                    failure = Some(ctx_error);
                } else {
                    peak_err = Some(err);
                }
            }
            None => peak_result = Some(peak),
        }
    }
    drop(value_at);
    result.statistics = statistics;
    result.peak = peak_result;
    result.peak_err = peak_err;
    (result, failure)
}
fn cached_voxel_values<'a>(
    ctx: Option<&dyn Cancellation>,
    snapshot: &'a VolumeSnapshot,
    grid: VoxelGrid,
    mask: &VoiMask,
    margin_mm: f64,
) -> Result<Box<VoxelValueFn<'a>>, Error> {
    let t = grid.transform()?;
    let mut b = IndexBox {
        lo: [i64::MAX; 3],
        hi: [i64::MIN; 3],
    };
    for v in &mask.voxels {
        for (axis, index) in [v.column, v.row, v.slice].into_iter().enumerate() {
            b.lo[axis] = b.lo[axis].min(index);
            b.hi[axis] = b.hi[axis].max(index);
        }
    }
    let dims = [grid.columns, grid.rows, grid.slices];
    for axis in 0..3 {
        let mut extent =
            fmath::f64_to_i64((margin_mm * t.inverse[axis].length()).ceil()).wrapping_add(1);
        if margin_mm == 0.0 {
            extent = 0;
        }
        b.lo[axis] = b.lo[axis].wrapping_sub(extent).max(0);
        b.hi[axis] = b.hi[axis].wrapping_add(extent).min(dims[axis] - 1);
    }
    let direct = move |i: i64, j: i64, k: i64| -> Option<f64> {
        if !t.in_grid(i, j, k) {
            return None;
        }
        snapshot.modality_at(i as u32, j as u32, k as u32)
    };
    if b.count() > MAX_VOI_VOXELS {
        return Ok(Box::new(direct));
    }
    let (nx, ny) = (b.hi[0] - b.lo[0] + 1, b.hi[1] - b.lo[1] + 1);
    let mut cache: Vec<f64> = Vec::with_capacity(b.count() as usize);
    for k in b.lo[2]..=b.hi[2] {
        if let Some(err) = ctx_err(ctx) {
            return Err(err);
        }
        for j in b.lo[1]..=b.hi[1] {
            for i in b.lo[0]..=b.hi[0] {
                cache.push(
                    snapshot
                        .modality_at(i as u32, j as u32, k as u32)
                        .unwrap_or(f64::NAN),
                );
            }
        }
    }
    Ok(Box::new(move |i: i64, j: i64, k: i64| -> Option<f64> {
        if i < b.lo[0] || j < b.lo[1] || k < b.lo[2] || i > b.hi[0] || j > b.hi[1] || k > b.hi[2] {
            return direct(i, j, k);
        }
        let v = cache[(((k - b.lo[2]) * ny + (j - b.lo[1])) * nx + (i - b.lo[0])) as usize];
        if v.is_nan() { None } else { Some(v) }
    }))
}
