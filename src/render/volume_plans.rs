#![forbid(unsafe_code)]
use super::error::{ErrorKind, RenderError};
use super::geometry::{GeometryDisposition, VolumeGeometry};
use super::vec3::Vec3;
use super::view_state::{
    Interpolation, MprPresentationMode, SLAB_NONE, ViewKind, ViewState, validate_view_state,
};
use super::volume::{RegularGridGeometry, Volume, plan_regular_grid_geometry};
use super::volume_store::{
    VOLUME_SNAPSHOT_CONTRACT_VERSION, VOLUME_SNAPSHOT_HEADER_SIZE_V1, VolumeDerivation,
    VolumeDescriptor, VolumeSampleDomain, VolumeScalarFormat, validate_descriptor,
};
use super::vr_histogram::MAX_VR_JOINT_HISTOGRAM_SAMPLES;
use crate::fmath;
const MAXIMUM_VR_HISTOGRAM_BINS: i64 = 65536;
fn invalid_snapshot() -> RenderError {
    RenderError::new(
        ErrorKind::InvalidVolumeSnapshot,
        "render: invalid volume snapshot",
    )
}
fn invalid_snapshot_detail(detail: &str) -> RenderError {
    RenderError::new(
        ErrorKind::InvalidVolumeSnapshot,
        format!("render: invalid volume snapshot: {detail}"),
    )
}
fn non_correctable(detail: &str) -> RenderError {
    RenderError::new(
        ErrorKind::NonCorrectableGeometry,
        format!("render: non-correctable volume geometry: {detail}"),
    )
}
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct VolumeHistogramPlan {
    pub dimensions: [u32; 3],
    pub lattice: [u32; 3],
    pub stride: u32,
    pub voxels: u64,
    pub samples: u64,
    pub cells: u64,
    pub gradient_covector: [f64; 9],
}
pub fn plan_volume_histogram(
    d: &VolumeDescriptor,
    intensity_bins: i64,
    gradient_bins: i64,
) -> Result<VolumeHistogramPlan, RenderError> {
    validate_descriptor(d)?;
    if d.components != 1
        || !(1..=MAXIMUM_VR_HISTOGRAM_BINS).contains(&intensity_bins)
        || !(0..=4096).contains(&gradient_bins)
        || gradient_bins == 1
        || gradient_bins > 0 && !(2..=4096).contains(&intensity_bins)
    {
        return Err(invalid_snapshot());
    }
    let mut p = VolumeHistogramPlan {
        dimensions: d.dimensions,
        lattice: d.dimensions,
        stride: 1,
        cells: (intensity_bins as u64).wrapping_mul(gradient_bins.max(1) as u64),
        ..VolumeHistogramPlan::default()
    };
    p.voxels = u64::from(d.dimensions[0]) * u64::from(d.dimensions[1]);
    if p.voxels > u64::MAX / u64::from(d.dimensions[2]) {
        return Err(invalid_snapshot());
    }
    p.voxels *= u64::from(d.dimensions[2]);
    p.samples = p.voxels;
    if gradient_bins > 0 {
        let stride = fmath::max(
            1.0,
            fmath::cbrt(p.voxels as f64 / MAX_VR_JOINT_HISTOGRAM_SAMPLES as f64).floor(),
        );
        p.stride = stride as u32;
        loop {
            p.samples = 1;
            for i in 0..3 {
                let n = u64::from(d.dimensions[i]);
                p.lattice[i] = ((n + u64::from(p.stride) - 1) / u64::from(p.stride)) as u32;
                p.samples = p.samples.wrapping_mul(u64::from(p.lattice[i]));
            }
            if p.samples <= MAX_VR_JOINT_HISTOGRAM_SAMPLES {
                break;
            }
            p.stride += 1;
        }
    }
    for row in 0..3 {
        for column in 0..3 {
            p.gradient_covector[row * 3 + column] =
                0.5 * d.patient_lps_to_index.0[column * 4 + row];
        }
    }
    Ok(p)
}
pub const MAX_VOLUME_REGULARIZATION_SLICES: i64 = 65536;
#[derive(Debug, Clone, PartialEq, Default)]
pub struct VolumeRegularizationPlan {
    pub source_dimensions: [u32; 3],
    pub target: RegularGridGeometry,
    pub column_step: Vec3,
    pub row_step: Vec3,
    pub slice_origins: Vec<Vec3>,
}
fn finite_bounds(values: &[f64]) -> bool {
    values.iter().all(|v| v.is_finite())
}
pub fn plan_volume_regularization(
    geometry: &VolumeGeometry,
) -> Result<VolumeRegularizationPlan, RenderError> {
    let slices = geometry.slices();
    if geometry.disposition == GeometryDisposition::Unsupported
        || slices.is_empty()
        || slices.len() as i64 > MAX_VOLUME_REGULARIZATION_SLICES
    {
        return Err(non_correctable("unsupported acquired geometry"));
    }
    let grid = plan_regular_grid_geometry(geometry)?;
    if grid.dimensions[2] > MAX_VOLUME_REGULARIZATION_SLICES {
        return Err(invalid_snapshot_detail("regularization plane bound"));
    }
    let first = slices[0];
    let mut positions = Vec::with_capacity(slices.len());
    let mut origins = Vec::with_capacity(slices.len());
    for (z, slice) in slices.iter().enumerate() {
        if slice.columns != first.columns || slice.rows != first.rows {
            return Err(non_correctable("differing acquired grids"));
        }
        let position = slice.origin.sub(first.origin).dot(geometry.normal);
        positions.push(position);
        origins.push(slice.origin);
        if !position.is_finite() || (z > 0 && position <= positions[z - 1]) {
            return Err(non_correctable("non-increasing acquired positions"));
        }
    }
    let source = Volume::acquired_lattice(
        first.columns,
        first.rows,
        first.origin,
        geometry.row_dir,
        geometry.col_dir,
        geometry.normal,
        first.col_spacing,
        first.row_spacing,
        geometry.mean_spacing,
        geometry.gantry_tilted,
        positions,
        origins,
    );
    let origin = source.patient_to_voxel(grid.origin);
    let column_step = source
        .patient_to_voxel(grid.origin.add(source.axis_x.scale(source.col_spacing)))
        .sub(origin);
    let row_step = source
        .patient_to_voxel(grid.origin.add(source.axis_y.scale(source.row_spacing)))
        .sub(origin);
    let slice_origins = (0..grid.dimensions[2].max(0))
        .map(|z| {
            source.patient_to_voxel(
                grid.origin
                    .add(source.normal.scale(z as f64 * grid.slice_spacing)),
            )
        })
        .collect();
    let plan = VolumeRegularizationPlan {
        source_dimensions: [source.cols as u32, source.rows as u32, source.depth as u32],
        target: grid,
        column_step,
        row_step,
        slice_origins,
    };
    plan.validate()?;
    Ok(plan)
}
impl VolumeRegularizationPlan {
    pub fn validate(&self) -> Result<(), RenderError> {
        let invalid = || invalid_snapshot_detail("invalid regularization plan");
        let n = self.slice_origins.len() as i64;
        if n == 0 || n > MAX_VOLUME_REGULARIZATION_SLICES || n != self.target.dimensions[2] {
            return Err(invalid());
        }
        for (axis, &n) in self.source_dimensions.iter().enumerate() {
            if n == 0
                || (axis < 2 && n > u32::from(u16::MAX))
                || (axis == 2 && i64::from(n) > MAX_VOLUME_REGULARIZATION_SLICES)
            {
                return Err(invalid());
            }
        }
        for (axis, &n) in self.target.dimensions.iter().enumerate() {
            if n <= 0
                || (axis < 2 && n > i64::from(u16::MAX))
                || (axis == 2 && n > MAX_VOLUME_REGULARIZATION_SLICES)
            {
                return Err(invalid());
            }
        }
        let points = [self.column_step, self.row_step]
            .into_iter()
            .chain(self.slice_origins.iter().copied());
        for point in points {
            if !finite_bounds(&[point.x, point.y, point.z]) {
                return Err(invalid());
            }
        }
        if self.column_step.length() == 0.0 || self.row_step.length() == 0.0 {
            return Err(invalid());
        }
        self.descriptor(2, 1).map(|_| ())
    }
    pub fn descriptor(
        &self,
        generation: u64,
        parent: u64,
    ) -> Result<VolumeDescriptor, RenderError> {
        let dims = self.target.dimensions;
        if generation == 0
            || parent == 0
            || generation == parent
            || dims[0] <= 0
            || dims[1] <= 0
            || dims[2] <= 0
            || dims[0] > i64::from(u16::MAX)
            || dims[1] > i64::from(u16::MAX)
            || dims[2] > MAX_VOLUME_REGULARIZATION_SLICES
        {
            return Err(invalid_snapshot());
        }
        let row = dims[0] as u64 * 4;
        let slice = row * dims[1] as u64;
        let d = VolumeDescriptor {
            contract_version: VOLUME_SNAPSHOT_CONTRACT_VERSION,
            header_size: VOLUME_SNAPSHOT_HEADER_SIZE_V1,
            volume_generation: generation,
            parent_generation: parent,
            derivation: VolumeDerivation::REGULARIZED,
            dimensions: [dims[0] as u32, dims[1] as u32, dims[2] as u32],
            components: 1,
            scalar_format: VolumeScalarFormat::F32_MODALITY_LE,
            sample_domain: VolumeSampleDomain::MODALITY,
            row_stride_bytes: row,
            slice_stride_bytes: slice,
            byte_length: slice * dims[2] as u64,
            rescale_slope: 1.0,
            rescale_intercept: 0.0,
            spacing_mm: [
                self.target.col_spacing,
                self.target.row_spacing,
                self.target.slice_spacing,
            ],
            index_to_patient_lps: self.target.voxel_to_patient_affine,
            patient_lps_to_index: self.target.patient_to_voxel_affine,
        };
        validate_descriptor(&d)?;
        Ok(d)
    }
    pub fn freeze(&self) -> Result<VolumeRegularizationPlan, RenderError> {
        self.validate()?;
        Ok(self.clone())
    }
}
pub fn mpr_bin_shrink_2x_descriptor(
    source: &VolumeDescriptor,
    view: &ViewState,
) -> (VolumeDescriptor, bool) {
    if validate_descriptor(source).is_err() || validate_view_state(view).is_err() {
        return (*source, false);
    }
    let m = &view.mpr;
    if !m.antialias_2x
        || view.kind != ViewKind::MPR
        || m.slab_mode != SLAB_NONE
        || m.interpolation == Interpolation::NEAREST
        || m.presentation.mode == MprPresentationMode::FUSION
    {
        return (*source, false);
    }
    if view.output_width < 2 || view.output_height < 2 {
        return (*source, false);
    }
    if source.dimensions.iter().any(|&n| n < 4 || n % 2 != 0) {
        return (*source, false);
    }
    let a = &source.index_to_patient_lps.0;
    let mut largest = 0.0;
    for axis in 0..3 {
        let spacing = fmath::hypot(fmath::hypot(a[axis], a[axis + 4]), a[axis + 8]);
        if spacing <= 0.0 || !spacing.is_finite() {
            return (*source, false);
        }
        largest = fmath::max(largest, spacing);
    }
    let norm = |p: [f64; 3]| fmath::hypot(fmath::hypot(p[0], p[1]), p[2]);
    if norm(m.pixel_step_u_lps) < 2.0 * largest || norm(m.pixel_step_v_lps) < 2.0 * largest {
        return (*source, false);
    }
    let mut out = *source;
    for axis in 0..3 {
        out.dimensions[axis] /= 2;
        out.spacing_mm[axis] *= 2.0;
    }
    out.components = 1;
    out.scalar_format = VolumeScalarFormat::F32_MODALITY_LE;
    out.sample_domain = VolumeSampleDomain::MODALITY;
    out.rescale_slope = 1.0;
    out.rescale_intercept = 0.0;
    out.row_stride_bytes = u64::from(out.dimensions[0]) * 4;
    out.slice_stride_bytes = out.row_stride_bytes * u64::from(out.dimensions[1]);
    out.byte_length = out.slice_stride_bytes * u64::from(out.dimensions[2]);
    let s_i2p = &source.index_to_patient_lps.0;
    let s_p2i = &source.patient_lps_to_index.0;
    for row in 0..3 {
        let i = row * 4;
        out.index_to_patient_lps.0[i + 3] += 0.5 * (s_i2p[i] + s_i2p[i + 1] + s_i2p[i + 2]);
        for col in 0..3 {
            out.index_to_patient_lps.0[i + col] *= 2.0;
            out.patient_lps_to_index.0[i + col] *= 0.5;
        }
        out.patient_lps_to_index.0[i + 3] = s_p2i[i + 3] * 0.5 - 0.25;
    }
    (out, true)
}
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct VolumeVesselScale {
    pub sigma_mm: f64,
    pub kernels: [[crate::imagefilter::RecursiveGaussianKernel; 3]; 3],
    pub z_margin: u32,
    pub z_slab_slices: u32,
}
#[derive(Debug, Clone, PartialEq, Default)]
pub struct VolumeVesselEnhancementPlan {
    pub dimensions: [u32; 3],
    pub image_dimensions: u32,
    pub alpha: f64,
    pub beta: f64,
    pub ramp_low: f64,
    pub ramp_high: f64,
    pub scales: Vec<VolumeVesselScale>,
}
#[allow(clippy::needless_range_loop)]
pub fn plan_volume_vessel_enhancement(
    d: &VolumeDescriptor,
) -> Result<VolumeVesselEnhancementPlan, RenderError> {
    use super::vr_vessel_enhance as ve;
    validate_descriptor(d)?;
    if d.components != 1 {
        return Err(invalid_snapshot_detail(
            "vessel enhancement requires scalar samples",
        ));
    }
    let mut p = VolumeVesselEnhancementPlan {
        dimensions: d.dimensions,
        image_dimensions: if d.dimensions[2] == 1 { 2 } else { 3 },
        alpha: 0.5,
        beta: 0.5,
        ramp_low: ve::VESSEL_ENHANCE_RAMP_LOW,
        ramp_high: ve::VESSEL_ENHANCE_RAMP_HIGH,
        scales: Vec::new(),
    };
    let dims = d.dimensions.map(i64::from);
    let slab =
        crate::imagefilter::plan_vesselness_slab_slices(dims, ve::VESSEL_ENHANCE_WORKING_BYTES)
            .map_err(ve::filter_error)?;
    for sigma in ve::vessel_enhance_sigmas_mm(d.spacing_mm) {
        let mut s = VolumeVesselScale {
            sigma_mm: sigma,
            z_slab_slices: slab as u32,
            ..VolumeVesselScale::default()
        };
        for axis in 0..3 {
            if d.dimensions[axis] == 1 {
                continue;
            }
            let samples = sigma / d.spacing_mm[axis];
            for order in 0..3 {
                let k = crate::imagefilter::plan_recursive_gaussian(
                    samples,
                    order as i64,
                    fmath::pow(samples, order as f64),
                )
                .map_err(|e| {
                    invalid_snapshot_detail(&format!(
                        "vessel scale {} axis {axis}: {e}",
                        crate::textfmt::float_v(sigma)
                    ))
                })?;
                s.kernels[axis][order] = k;
            }
        }
        if d.dimensions[2] > 1 {
            s.z_margin = fmath::min(
                f64::from(d.dimensions[2] - 1),
                (crate::imagefilter::VESSELNESS_SLAB_MARGIN_SIGMAS * sigma / d.spacing_mm[2])
                    .ceil(),
            ) as u32;
        }
        p.scales.push(s);
    }
    Ok(p)
}
