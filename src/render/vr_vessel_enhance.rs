#![forbid(unsafe_code)]
use super::error::{ErrorKind, RenderError};
use super::parallel::{Cancellation, ctx_err};
use super::volume::checked_voxel_count;
use crate::fmath;
use crate::imagefilter::{
    FilterError, HessianStructure, VesselnessMeasure, VesselnessOptions, Volume as FilterVolume,
    vesselness, vesselness_sigmas,
};
pub const VESSEL_ENHANCE_MIN_SIGMA_MM: f64 = 0.75;
pub const VESSEL_ENHANCE_MAX_SIGMA_MM: f64 = 8.0;
pub const VESSEL_ENHANCE_SIGMA_STEPS: i64 = 5;
pub const VESSEL_ENHANCE_RAMP_LOW: f64 = 0.2;
pub const VESSEL_ENHANCE_RAMP_HIGH: f64 = 0.6;
pub const VESSEL_ENHANCE_WORKING_BYTES: i64 = 384 << 20;
pub fn vessel_enhance_sigmas_mm(spacing_mm: [f64; 3]) -> Vec<f64> {
    let mut finest = f64::INFINITY;
    for s in spacing_mm {
        if s > 0.0 {
            finest = fmath::min(finest, s);
        }
    }
    let mut minimum = VESSEL_ENHANCE_MIN_SIGMA_MM;
    if finest != f64::INFINITY {
        minimum = fmath::max(minimum, finest);
    }
    if minimum >= VESSEL_ENHANCE_MAX_SIGMA_MM {
        return vec![VESSEL_ENHANCE_MAX_SIGMA_MM];
    }
    vesselness_sigmas(
        minimum,
        VESSEL_ENHANCE_MAX_SIGMA_MM,
        VESSEL_ENHANCE_SIGMA_STEPS,
        false,
    )
    .unwrap_or_else(|_| vec![VESSEL_ENHANCE_MAX_SIGMA_MM])
}
pub fn vessel_enhance_weight(vesselness: f64) -> f64 {
    fmath::max(
        0.0,
        fmath::min(1.0, (vesselness - VESSEL_ENHANCE_RAMP_LOW) / 0.4),
    )
}
pub(crate) fn filter_error(e: FilterError) -> RenderError {
    let kind = if e.is(crate::imagefilter::ErrorKind::Canceled) {
        ErrorKind::Canceled
    } else if e.is(crate::imagefilter::ErrorKind::DeadlineExceeded) {
        ErrorKind::DeadlineExceeded
    } else {
        ErrorKind::Other
    };
    RenderError::new(kind, e.message())
}
fn check_input(values: &[f32], dims: [i64; 3]) -> Result<(), RenderError> {
    let count = checked_voxel_count(dims[0], dims[1], dims[2]);
    if dims[0] <= 0 || dims[1] <= 0 || dims[2] <= 0 || count != Some(values.len()) {
        return Err(RenderError::new(
            ErrorKind::InvalidVolumeSnapshot,
            format!(
                "render: invalid volume snapshot: prefilter volume of {} values for dimensions [{} {} {}]",
                values.len(),
                dims[0],
                dims[1],
                dims[2]
            ),
        ));
    }
    if values.iter().any(|v| v.is_nan() || v.is_infinite()) {
        return Err(RenderError::new(
            ErrorKind::InvalidVolumeSnapshot,
            "render: invalid volume snapshot: invalid source voxel",
        ));
    }
    Ok(())
}
pub fn vessel_enhance(
    ctx: Option<&dyn Cancellation>,
    values: &[f32],
    dims: [i64; 3],
    spacing_mm: [f64; 3],
) -> Result<Vec<f32>, RenderError> {
    check_input(values, dims)?;
    for axis in 0..3 {
        let s = spacing_mm[axis];
        if dims[axis] > 1 && !(s > 0.0 && s <= f64::MAX) {
            return Err(RenderError::other(format!(
                "render: invalid voxel spacing {} mm",
                crate::textfmt::float_v(s)
            )));
        }
    }
    let src = FilterVolume {
        dims,
        spacing_mm,
        data: values.to_vec(),
    };
    let result = vesselness(
        ctx,
        &src,
        &VesselnessOptions {
            measure: VesselnessMeasure::FRANGI,
            structure: HessianStructure::TUBE,
            sigmas_mm: vessel_enhance_sigmas_mm(spacing_mm),
            gamma: -1.0,
            max_working_bytes: VESSEL_ENHANCE_WORKING_BYTES,
            ..VesselnessOptions::default()
        },
    )
    .map_err(filter_error)?;
    let mut out = result.response.data;
    let floor = values.iter().fold(values[0], |a, &v| a.min(v));
    if let Some(e) = ctx_err(ctx) {
        return Err(e);
    }
    for (o, &v) in out.iter_mut().zip(values) {
        let w = vessel_enhance_weight(f64::from(*o)) as f32;
        *o = floor + (v - floor) * w;
    }
    Ok(out)
}
