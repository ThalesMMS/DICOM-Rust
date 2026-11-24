use super::{
    Ctx, RenderError, Sampler, Volume, VrPrefilterId, VrPreparedVolume, check, invalid_snapshot,
    sampler_modality_values, smooth_gaussian_3d,
};
use std::sync::{Arc, OnceLock};
pub const VR_DISPLAY_GAUSSIAN: VrPrefilterId = VrPrefilterId(0x80);
pub fn prepare_vr_volume_gaussian(
    volume: &Arc<Volume>,
    ctx: Ctx<'_>,
    sigma_mm: f64,
) -> Result<Arc<VrPreparedVolume>, RenderError> {
    check(ctx)?;
    let grid = volume.regular_grid()?;
    grid.payload()?;
    if grid.components() != 1 {
        return Err(invalid_snapshot(
            "a display filter of a multi-component volume",
        ));
    }
    let sampler = Sampler::direct(grid.clone())?;
    let source = sampler_modality_values(ctx, &sampler)?;
    let dims = [sampler.cols, sampler.rows, sampler.depth];
    let spacing = [
        grid.col_spacing,
        grid.row_spacing,
        grid.effective_slice_spacing(),
    ];
    let values = smooth_gaussian_3d(ctx, &source, dims, spacing, sigma_mm)?;
    check(ctx)?;
    Ok(Arc::new(VrPreparedVolume {
        prefilter: VR_DISPLAY_GAUSSIAN,
        volume: grid,
        values: Some(Arc::new(values)),
        hu_range: OnceLock::new(),
        blocks: OnceLock::new(),
    }))
}
