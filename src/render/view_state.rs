#![forbid(unsafe_code)]
use super::affine::GeometryAffine;
use super::error::{ErrorKind, RenderError};
use super::mpr_fusion::{
    FusionOpacityPoint, MAX_MPR_PRESENTATION_RAMP_POINTS, MPR_PRESENTATION_PALETTE_ENTRIES,
    MprFusionState, MprLayerPresentation,
};
use super::vec3::Vec3;
use super::view_contracts::{
    ComponentPresentation, StereoPresentationState, SurfaceProbeState, VolumeFilterState,
    VrFusionState, f32_text, json_f32,
};
use super::vr_geometry::VrDepthImage;
use super::vr_gradient_opacity::VrGradientOpacity;
use super::vr_illumination::{
    MAX_VR_DIRECTIONAL_LIGHTS, VrIlluminationState, validate_vr_illumination,
};
use super::vr_labels::VrLabelMap;
use super::vr_projection::VrValueRange;
use super::vr_raycast::to8;
use super::vr_transfer::{VrLightingMaterial, linear_to_srgb};
use super::vr_transfer_2d::VrTransfer2dState;
use crate::textfmt::float_v;
use std::sync::Arc;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, PartialOrd, Ord)]
pub struct Interpolation(pub u32);
impl Interpolation {
    pub const UNKNOWN: Interpolation = Interpolation(0);
    pub const NEAREST: Interpolation = Interpolation(1);
    pub const LINEAR: Interpolation = Interpolation(2);
    pub const CUBIC: Interpolation = Interpolation(3);
}
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct TransferSample {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}
pub const VIEW_STATE_CONTRACT_VERSION_V1: u32 = 1;
pub const VIEW_STATE_CONTRACT_VERSION_V2: u32 = 2;
pub const VIEW_STATE_CONTRACT_VERSION_V3: u32 = 3;
pub const VIEW_STATE_CONTRACT_VERSION_V4: u32 = 4;
pub const VIEW_STATE_CONTRACT_VERSION_V5: u32 = 5;
pub const VIEW_STATE_CONTRACT_VERSION_V6: u32 = 6;
pub const VIEW_STATE_CONTRACT_VERSION_V7: u32 = 7;
pub const VIEW_STATE_CONTRACT_VERSION_V8: u32 = 8;
pub const VIEW_STATE_CONTRACT_VERSION_V9: u32 = 9;
pub const VIEW_STATE_CONTRACT_VERSION_V10: u32 = 10;
pub const VIEW_STATE_CONTRACT_VERSION_V11: u32 = 11;
pub const VIEW_STATE_CONTRACT_VERSION_V12: u32 = 12;
pub const VIEW_STATE_CONTRACT_VERSION_V13: u32 = 13;
pub const VIEW_STATE_CONTRACT_VERSION_V14: u32 = 14;
pub const VIEW_STATE_CONTRACT_VERSION_V15: u32 = 15;
pub const VIEW_STATE_CONTRACT_VERSION_V16: u32 = 16;
pub const VIEW_STATE_CONTRACT_VERSION_V17: u32 = 17;
pub const VIEW_STATE_CONTRACT_VERSION_V18: u32 = 18;
pub const VIEW_STATE_CONTRACT_VERSION_V19: u32 = 19;
pub const VIEW_STATE_CONTRACT_VERSION_V20: u32 = 20;
pub const VIEW_STATE_CONTRACT_VERSION_V21: u32 = 21;
pub const VIEW_STATE_CONTRACT_VERSION_V22: u32 = 22;
pub const VIEW_STATE_CONTRACT_VERSION_V23: u32 = 23;
pub const VIEW_STATE_CONTRACT_VERSION_V24: u32 = 24;
pub const VIEW_STATE_CONTRACT_VERSION_V25: u32 = 25;
pub const VIEW_STATE_CONTRACT_VERSION_V26: u32 = 26;
pub const VIEW_STATE_CONTRACT_VERSION_V27: u32 = 27;
pub const VIEW_STATE_CONTRACT_VERSION_V28: u32 = 28;
pub const VIEW_STATE_CONTRACT_VERSION_V29: u32 = 29;
pub const VIEW_STATE_CONTRACT_VERSION: u32 = VIEW_STATE_CONTRACT_VERSION_V1;
pub const VIEW_STATE_HEADER_SIZES: [u32; 30] = [
    0, 48, 96, 112, 144, 152, 184, 232, 272, 280, 288, 296, 304, 312, 424, 432, 456, 472, 480, 488,
    488, 520, 560, 752, 752, 760, 920, 1432, 1432, 1432,
];
pub const MAX_VR_ISO_SURFACES: usize = 8;
pub const MAX_VR_LABEL_STYLES: usize = 4096;
pub const MAX_VIEW_LABEL_BYTES: u64 = 512 << 20;
pub const MAX_VIEW_OUTPUT_DIMENSION: u32 = 8192;
pub const MAX_VIEW_OUTPUT_BYTES: u64 = 256 << 20;
pub const MAX_VIEW_CUT_MASK_BYTES: u64 = 64 << 20;
pub const MAX_TRANSFER_LUT_SAMPLES: usize = 4096;
pub const MAX_CLIPPING_PLANES: usize = 8;
pub const ERR_INVALID_VIEW_STATE: &str = "render: invalid view state";
pub(crate) fn invalid_view_state(detail: impl AsRef<str>) -> RenderError {
    RenderError::new(
        ErrorKind::InvalidViewState,
        format!("{ERR_INVALID_VIEW_STATE}: {}", detail.as_ref()),
    )
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ViewKind(pub u32);
impl ViewKind {
    pub const UNKNOWN: ViewKind = ViewKind(0);
    pub const MPR: ViewKind = ViewKind(1);
    pub const VR: ViewKind = ViewKind(2);
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Projection(pub u32);
impl Projection {
    pub const UNKNOWN: Projection = Projection(0);
    pub const PERSPECTIVE: Projection = Projection(1);
    pub const PARALLEL: Projection = Projection(2);
    pub const ENDOSCOPY: Projection = Projection(3);
}
pub const SLAB_NONE: i64 = 0;
pub const SLAB_MIP: i64 = 1;
pub const SLAB_MIN_IP: i64 = 2;
pub const SLAB_AVERAGE: i64 = 3;
pub const VR_MODE_DVR: i64 = 0;
pub const VR_MODE_MIP: i64 = 1;
pub const VR_MODE_MIN_IP: i64 = 2;
pub const VR_MODE_AVERAGE: i64 = 3;
pub const VR_MODE_ISO_SURFACE: i64 = 4;
pub const VR_MODE_RAY_SUM: i64 = 5;
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct CprColumn {
    pub origin_lps: [f64; 3],
    pub step_v_lps: [f64; 3],
    pub slab_axis_lps: [f64; 3],
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MprPresentationMode(pub u32);
impl MprPresentationMode {
    pub const NONE: MprPresentationMode = MprPresentationMode(0);
    pub const GRAY: MprPresentationMode = MprPresentationMode(1);
    pub const FUSION: MprPresentationMode = MprPresentationMode(2);
}
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MprPresentationState {
    pub mode: MprPresentationMode,
    pub base: MprLayerPresentation,
    pub overlay: MprLayerPresentation,
    pub fusion: MprFusionState,
}
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MprViewState {
    pub scalar_validity: bool,
    pub cpr_columns: Vec<CprColumn>,
    pub antialias_2x: bool,
    pub pixel_origin_lps: [f64; 3],
    pub pixel_step_u_lps: [f64; 3],
    pub pixel_step_v_lps: [f64; 3],
    pub interpolation: Interpolation,
    pub slab_mode: i64,
    pub slab_axis_lps: [f64; 3],
    pub slab_thickness_mm: f64,
    pub slab_sample_spacing_mm: f64,
    pub outside_value: f32,
    pub presentation: MprPresentationState,
}
pub type ClippingPlaneLps = [f64; 4];
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct VrIsoSurfaceState {
    pub value: f64,
    pub color: [f64; 3],
    pub opacity: f64,
}
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TransferLut {
    pub domain_min: f64,
    pub domain_max: f64,
    pub samples: Vec<TransferSample>,
}
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct NormalizedCropBox {
    pub enabled: u32,
    pub region_flags: u32,
    pub min: [f64; 3],
    pub max: [f64; 3],
}
#[derive(Debug, Clone, PartialEq, Default)]
pub struct VoxelMask {
    pub dimensions: [u32; 3],
    pub words: Vec<u32>,
}
#[derive(Debug, Clone, PartialEq, Default)]
pub struct VrGeometryState {
    pub enabled: u32,
    pub depth: Vec<f32>,
    pub color: Vec<u32>,
}
pub fn vr_geometry_bytes(w: u32, h: u32) -> u64 {
    u64::from(w) * u64::from(h) * 8
}
pub fn vr_geometry_state_from_depth(depth: Option<&VrDepthImage>) -> VrGeometryState {
    let Some(depth) = depth else {
        return VrGeometryState::default();
    };
    let color = (0..depth.t.len())
        .map(|i| {
            let r = to8(linear_to_srgb(f64::from(depth.r[i])));
            let g = to8(linear_to_srgb(f64::from(depth.g[i])));
            let b = to8(linear_to_srgb(f64::from(depth.b[i])));
            u32::from(r) | u32::from(g) << 8 | u32::from(b) << 16 | 0xff << 24
        })
        .collect();
    VrGeometryState {
        enabled: 1,
        depth: depth.t.clone(),
        color,
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct VrCutPlaneState {
    pub planes: u32,
    pub clipped_voxel_intensity: f64,
    pub cap_opacity: f64,
    pub cap_window: [f64; 2],
}
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct VrScatteringState {
    pub blend: f64,
    pub reach_mm: f64,
    pub anisotropy: f64,
    pub light_direction_lps: [f64; 3],
}
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct VrLabelStyleState {
    pub rgb: [f64; 3],
    pub opacity: f64,
    pub blend: f64,
    pub visible: u32,
}
#[derive(Debug, Clone, Default)]
pub struct VrLabelState {
    pub generation: u64,
    pub dimensions: [u32; 3],
    pub bits: u32,
    pub only_inside: u32,
    pub styles: Vec<VrLabelStyleState>,
    pub volume: Option<Arc<VrLabelMap>>,
}
impl PartialEq for VrLabelState {
    fn eq(&self, other: &Self) -> bool {
        self.generation == other.generation
            && self.dimensions == other.dimensions
            && self.bits == other.bits
            && self.only_inside == other.only_inside
            && self.styles == other.styles
            && match (&self.volume, &other.volume) {
                (None, None) => true,
                (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                _ => false,
            }
    }
}
impl VrLabelState {
    fn is_zero(&self) -> bool {
        self.generation == 0
            && self.dimensions == [0; 3]
            && self.bits == 0
            && self.only_inside == 0
            && self.styles.is_empty()
            && self.volume.is_none()
    }
}
pub fn vr_label_state_from_map(m: Option<&Arc<VrLabelMap>>, generation: u64) -> VrLabelState {
    let Some(m) = m.filter(|_| generation != 0) else {
        return VrLabelState::default();
    };
    let (cols, rows, depth) = m.dimensions();
    let styles = m.styles();
    let styles = &styles[..styles.len().min(MAX_VR_LABEL_STYLES)];
    VrLabelState {
        generation,
        dimensions: [cols as u32, rows as u32, depth as u32],
        bits: m.storage_bits(),
        only_inside: u32::from(m.only_inside()),
        styles: styles
            .iter()
            .map(|s| VrLabelStyleState {
                rgb: [s.color.r, s.color.g, s.color.b],
                opacity: s.opacity,
                blend: s.blend,
                visible: u32::from(s.visible),
            })
            .collect(),
        volume: Some(m.clone()),
    }
}
#[derive(Debug, Clone, PartialEq, Default)]
pub struct VrViewState {
    pub fusion: VrFusionState,
    pub transfer_2d: VrTransfer2dState,
    pub stereo: StereoPresentationState,
    pub position_lps: [f64; 3],
    pub focal_point_lps: [f64; 3],
    pub view_up_lps: [f64; 3],
    pub projection: Projection,
    pub vertical_fov_radians: f64,
    pub parallel_scale_mm: f64,
    pub near_mm: f64,
    pub far_mm: f64,
    pub mode: i64,
    pub projection_range: VrValueRange,
    pub ray_sum_reference_mm: f64,
    pub sample_distance_mm: f64,
    pub opacity_unit_distance_mm: f64,
    pub image_sample_distance: f64,
    pub shading_enabled: u32,
    pub opacity_normals: bool,
    pub illumination: VrIlluminationState,
    pub ambient: f64,
    pub diffuse: f64,
    pub specular: f64,
    pub specular_power: f64,
    pub gradient_opacity_scale: f64,
    pub background_rgba: [f64; 4],
    pub jitter_enabled: u32,
    pub clipping_planes_lps: Vec<ClippingPlaneLps>,
    pub crop: NormalizedCropBox,
    pub cut_mask: VoxelMask,
    pub transfer_lut: TransferLut,
    pub labels: VrLabelState,
    pub scattering: VrScatteringState,
    pub cut: VrCutPlaneState,
    pub geometry: VrGeometryState,
    pub gradient_opacity: VrGradientOpacity,
    pub iso_surfaces: Vec<VrIsoSurfaceState>,
    pub surface_probe: SurfaceProbeState,
}
impl VrViewState {
    pub fn effective_opacity_unit_distance_mm(&self) -> f64 {
        if !finite(self.opacity_unit_distance_mm) || self.opacity_unit_distance_mm <= 0.0 {
            return super::vr_transfer::DEFAULT_VR_OPACITY_UNIT_DISTANCE_MM;
        }
        self.opacity_unit_distance_mm
    }
}
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ViewState {
    pub components: ComponentPresentation,
    pub volume_filter: VolumeFilterState,
    pub contract_version: u32,
    pub struct_size: u32,
    pub kind: ViewKind,
    pub volume_generation: u64,
    pub view_generation: u64,
    pub presentation_generation: u64,
    pub output_width: u32,
    pub output_height: u32,
    pub mpr: MprViewState,
    pub vr: VrViewState,
}
impl ViewState {
    pub fn overlay_source(&self) -> (u64, [u32; 3]) {
        if self.kind == ViewKind::VR && self.vr.surface_probe.enabled() {
            return (
                self.vr.surface_probe.source_generation,
                self.vr.surface_probe.source_dimensions,
            );
        }
        if self.kind == ViewKind::VR && self.vr.fusion.enabled() {
            return (
                self.vr.fusion.overlay_generation,
                self.vr.fusion.overlay_dimensions,
            );
        }
        if self.kind == ViewKind::MPR && self.mpr.presentation.mode == MprPresentationMode::FUSION {
            return (
                self.mpr.presentation.fusion.overlay_generation,
                self.mpr.presentation.fusion.overlay_dimensions,
            );
        }
        (0, [0; 3])
    }
}
pub(crate) fn finite(value: f64) -> bool {
    !value.is_nan() && !value.is_infinite()
}
pub(crate) fn finite3(v: [f64; 3]) -> bool {
    finite(v[0]) && finite(v[1]) && finite(v[2])
}
fn finite4(v: [f64; 4]) -> bool {
    finite(v[0]) && finite(v[1]) && finite(v[2]) && finite(v[3])
}
pub(crate) fn norm3(v: [f64; 3]) -> f64 {
    crate::fmath::hypot(crate::fmath::hypot(v[0], v[1]), v[2])
}
fn max_abs3(v: [f64; 3]) -> f64 {
    crate::fmath::max(v[0].abs(), crate::fmath::max(v[1].abs(), v[2].abs()))
}
fn normalize3(v: [f64; 3]) -> Option<[f64; 3]> {
    let scale = max_abs3(v);
    if scale == 0.0 || !finite(scale) {
        return None;
    }
    let scaled = [v[0] / scale, v[1] / scale, v[2] / scale];
    let length = norm3(scaled);
    let result = [scaled[0] / length, scaled[1] / length, scaled[2] / length];
    finite3(result).then_some(result)
}
fn cross3_finite(a: [f64; 3], b: [f64; 3]) -> Option<[f64; 3]> {
    let r = [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ];
    finite3(r).then_some(r)
}
fn scaled_add3(base: [f64; 3], direction: [f64; 3], scale: f64) -> Option<[f64; 3]> {
    let mut result = [0.0; 3];
    for c in 0..3 {
        let product = direction[c] * scale;
        result[c] = base[c] + product;
        if !finite(product) || !finite(result[c]) {
            return None;
        }
    }
    Some(result)
}
fn sub3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
type Fail = RenderError;
fn fail(detail: impl AsRef<str>) -> Fail {
    invalid_view_state(detail)
}
fn v3_text(v: [f64; 3]) -> String {
    format!("[{} {} {}]", float_v(v[0]), float_v(v[1]), float_v(v[2]))
}
pub fn validate_view_state(state: &ViewState) -> Result<(), RenderError> {
    let version = state.contract_version;
    let Some(minimum_header) = (1..=29)
        .contains(&version)
        .then(|| VIEW_STATE_HEADER_SIZES[version as usize])
    else {
        return Err(fail(format!("contract version {version}")));
    };
    if state.struct_size < minimum_header {
        return Err(fail(format!(
            "struct size {} below V{version} minimum {minimum_header}",
            state.struct_size
        )));
    }
    state.components.validate()?;
    state.vr.surface_probe.validate()?;
    if state.vr.surface_probe.enabled()
        && (version < VIEW_STATE_CONTRACT_VERSION_V27
            || state.kind != ViewKind::VR
            || state.vr.fusion.enabled()
            || state.components.mode != super::view_contracts::ComponentPresentationMode::SCALAR)
    {
        return Err(fail(
            "surface probe requires V27 scalar VR and its own secondary source",
        ));
    }
    let no_filter = state.volume_filter == VolumeFilterState::default();
    if state.components.mode != super::view_contracts::ComponentPresentationMode::SCALAR
        && (version < VIEW_STATE_CONTRACT_VERSION_V26
            || state.mpr.scalar_validity
            || state.mpr.antialias_2x
            || !state.mpr.cpr_columns.is_empty()
            || state.mpr.presentation.mode != MprPresentationMode::NONE
            || !no_filter)
    {
        return Err(fail(
            "component presentation requires V26 plain MPR/VR without scalar-only features",
        ));
    }
    if state.volume_generation == 0 || state.view_generation == 0 {
        return Err(fail("zero volume or view generation"));
    }
    if state.output_width == 0
        || state.output_height == 0
        || state.output_width > MAX_VIEW_OUTPUT_DIMENSION
        || state.output_height > MAX_VIEW_OUTPUT_DIMENSION
    {
        return Err(fail(format!(
            "output dimensions {}x{} outside 1..{MAX_VIEW_OUTPUT_DIMENSION}",
            state.output_width, state.output_height
        )));
    }
    let pixels = u64::from(state.output_width) * u64::from(state.output_height);
    if pixels > MAX_VIEW_OUTPUT_BYTES / 4 {
        return Err(fail(format!(
            "output byte size exceeds {MAX_VIEW_OUTPUT_BYTES}"
        )));
    }
    let mpr = &state.mpr;
    if mpr.antialias_2x
        && (version < VIEW_STATE_CONTRACT_VERSION_V15
            || state.kind != ViewKind::MPR
            || mpr.slab_mode != SLAB_NONE
            || mpr.interpolation == Interpolation::NEAREST
            || mpr.presentation.mode == MprPresentationMode::FUSION)
    {
        return Err(fail("Antialias2x requires V15 single-plane non-label MPR"));
    }
    if !mpr.cpr_columns.is_empty() {
        if version < VIEW_STATE_CONTRACT_VERSION_V18
            || state.kind != ViewKind::MPR
            || mpr.antialias_2x
        {
            return Err(fail(
                "CPR columns require V18 scalar MPR without averaged levels",
            ));
        }
        if (mpr.presentation.mode == MprPresentationMode::FUSION || !no_filter)
            && version < VIEW_STATE_CONTRACT_VERSION_V28
        {
            return Err(fail("filtered or fused CPR requires V28"));
        }
        validate_cpr_columns(mpr, state.output_width, state.output_height)?;
    }
    if mpr.scalar_validity {
        if version < VIEW_STATE_CONTRACT_VERSION_V19
            || state.kind != ViewKind::MPR
            || mpr.presentation.mode != MprPresentationMode::NONE
            || mpr.antialias_2x
        {
            return Err(fail(
                "scalar validity requires V19 scalar MPR without presentation or averaged levels",
            ));
        }
        if !no_filter
            && (version < VIEW_STATE_CONTRACT_VERSION_V29
                || state.volume_filter.mode
                    == super::view_contracts::VolumeFilterMode::VESSEL_ENHANCE)
        {
            return Err(fail(
                "filtered scalar validity requires V29 and a modality-preserving filter",
            ));
        }
        if pixels > MAX_VIEW_OUTPUT_BYTES / 8 {
            return Err(fail("scalar validity output exceeds byte budget"));
        }
    }
    if state.volume_filter.mode == super::view_contracts::VolumeFilterMode::MEDIAN_3X3X3
        && version < VIEW_STATE_CONTRACT_VERSION_V20
    {
        return Err(fail("median filter requires V20"));
    }
    if state.volume_filter.mode == super::view_contracts::VolumeFilterMode::VESSEL_ENHANCE
        && version < VIEW_STATE_CONTRACT_VERSION_V24
    {
        return Err(fail("vessel enhancement requires V24"));
    }
    let table = &state.vr.transfer_2d;
    if state.kind != ViewKind::VR
        && (table.enabled()
            || !table.samples.is_empty()
            || table.intensity_min != 0.0
            || table.intensity_max != 0.0
            || table.gradient_max != 0.0)
    {
        return Err(fail("2D transfer classification requires a VR view"));
    }
    state.vr.fusion.validate()?;
    if state.vr.fusion.enabled()
        && (state.kind != ViewKind::VR
            || state.vr.mode != VR_MODE_DVR
            || version < VIEW_STATE_CONTRACT_VERSION_V23)
    {
        return Err(fail("volume fusion requires DVR V23"));
    }
    if !no_filter {
        if version < VIEW_STATE_CONTRACT_VERSION_V17 {
            return Err(fail("volume filter requires V17"));
        }
        state.volume_filter.validate()?;
        if mpr.antialias_2x
            || mpr.presentation.mode == MprPresentationMode::FUSION
            || state.vr.geometry.enabled != 0
        {
            return Err(fail(
                "volume filter does not combine with averaged MPR levels, fusion or preprojected geometry",
            ));
        }
    }
    if state.vr.stereo != StereoPresentationState::default() {
        if state.kind != ViewKind::VR || version < VIEW_STATE_CONTRACT_VERSION_V16 {
            return Err(fail("stereo requires V16 VR"));
        }
        state.vr.stereo.validate()?;
        if state.vr.stereo.mode != super::view_contracts::StereoMode::MONO
            && (state.vr.geometry.enabled != 0 || state.vr.image_sample_distance != 1.0)
        {
            return Err(fail(
                "stereo requires per-eye GPU rendering without preprojected geometry or legacy image LOD",
            ));
        }
        if state.vr.stereo.mode == super::view_contracts::StereoMode::SIDE_BY_SIDE
            && state.output_width < 2
        {
            return Err(fail("side-by-side needs two output pixels"));
        }
    }
    match state.kind {
        ViewKind::MPR => {
            validate_mpr_presentation(&mpr.presentation, version)?;
            validate_mpr_view_state(mpr, state.output_width, state.output_height)
        }
        ViewKind::VR => {
            validate_mpr_presentation(&mpr.presentation, VIEW_STATE_CONTRACT_VERSION_V1)?;
            validate_vr_view_state(&state.vr, version, state.output_width, state.output_height)
        }
        kind => Err(fail(format!("unsupported view kind {}", kind.0))),
    }
}
fn validate_cpr_columns(state: &MprViewState, width: u32, height: u32) -> Result<(), Fail> {
    if state.cpr_columns.len() != width as usize {
        return Err(fail("CPR column count does not match output width"));
    }
    for column in &state.cpr_columns {
        let step_norm = norm3(column.step_v_lps);
        let axis_norm = norm3(column.slab_axis_lps);
        if !finite3(column.origin_lps)
            || !finite3(column.step_v_lps)
            || !finite3(column.slab_axis_lps)
            || !finite(step_norm)
            || step_norm <= 0.0
            || !finite(axis_norm)
            || axis_norm < 1.0 - 1e-6
            || axis_norm > 1.0 + 1e-6
        {
            return Err(fail("invalid CPR column basis"));
        }
        for axis in 0..3 {
            let extent = column.origin_lps[axis] + f64::from(height - 1) * column.step_v_lps[axis];
            let half = state.slab_thickness_mm * 0.5 * column.slab_axis_lps[axis];
            if !finite(extent) || !finite(extent + half) || !finite(extent - half) {
                return Err(fail("CPR extent overflows"));
            }
        }
    }
    Ok(())
}
fn validate_mpr_view_state(state: &MprViewState, width: u32, height: u32) -> Result<(), Fail> {
    if !finite3(state.pixel_origin_lps)
        || !finite3(state.pixel_step_u_lps)
        || !finite3(state.pixel_step_v_lps)
    {
        return Err(fail("non-finite MPR basis"));
    }
    let u = normalize3(state.pixel_step_u_lps);
    let v = normalize3(state.pixel_step_v_lps);
    let degenerate = match (u, v) {
        (Some(u), Some(v)) => cross3_finite(u, v).is_none_or(|c| norm3(c) <= 1e-12),
        _ => true,
    };
    if degenerate {
        return Err(fail("degenerate MPR basis"));
    }
    match state.interpolation {
        Interpolation::NEAREST | Interpolation::LINEAR | Interpolation::CUBIC => {}
        other => {
            return Err(fail(format!("unsupported interpolation {}", other.0)));
        }
    }
    if !finite(f64::from(state.outside_value))
        || !finite(state.slab_thickness_mm)
        || state.slab_thickness_mm < 0.0
    {
        return Err(fail("invalid MPR outside value or slab thickness"));
    }
    match state.slab_mode {
        SLAB_NONE => {}
        SLAB_MIP | SLAB_MIN_IP | SLAB_AVERAGE => {
            let slab_norm = norm3(state.slab_axis_lps);
            if !finite3(state.slab_axis_lps) || !finite(slab_norm) || (slab_norm - 1.0).abs() > 1e-6
            {
                return Err(fail("slab axis is not finite unit length"));
            }
            if !finite(state.slab_sample_spacing_mm) || state.slab_sample_spacing_mm <= 0.0 {
                return Err(fail("invalid slab sample spacing"));
            }
        }
        other => return Err(fail(format!("unsupported slab mode {other}"))),
    }
    validate_mpr_derived_geometry(state, width, height)
}
fn validate_mpr_derived_geometry(
    state: &MprViewState,
    width: u32,
    height: u32,
) -> Result<(), Fail> {
    let last_x = f64::from(width.wrapping_sub(1));
    let last_y = f64::from(height.wrapping_sub(1));
    let xs = [0.0, crate::fmath::max(0.0, last_x - 1.0), last_x];
    let ys = [0.0, crate::fmath::max(0.0, last_y - 1.0), last_y];
    let offsets: Vec<f64> = if state.slab_mode != SLAB_NONE {
        vec![
            -state.slab_thickness_mm / 2.0,
            state.slab_thickness_mm / 2.0,
        ]
    } else {
        vec![0.0]
    };
    for x in xs {
        for y in ys {
            let Some(point) = scaled_add3(state.pixel_origin_lps, state.pixel_step_u_lps, x) else {
                return Err(fail("MPR U extent is non-finite"));
            };
            let Some(point) = scaled_add3(point, state.pixel_step_v_lps, y) else {
                return Err(fail("MPR V extent is non-finite"));
            };
            for offset in &offsets {
                let mut sample = point;
                if state.slab_mode != SLAB_NONE {
                    match scaled_add3(point, state.slab_axis_lps, *offset) {
                        Some(s) => sample = s,
                        None => return Err(fail("MPR slab extent is non-finite")),
                    }
                }
                if x < last_x {
                    match scaled_add3(sample, state.pixel_step_u_lps, 1.0) {
                        Some(next) if next != sample => {}
                        _ => return Err(fail("MPR U step collapses within the derived extent")),
                    }
                }
                if y < last_y {
                    match scaled_add3(sample, state.pixel_step_v_lps, 1.0) {
                        Some(next) if next != sample => {}
                        _ => return Err(fail("MPR V step collapses within the derived extent")),
                    }
                }
            }
        }
    }
    Ok(())
}
fn iso_surface_text(s: &VrIsoSurfaceState) -> String {
    format!(
        "{{Value:{} Color:{} Opacity:{}}}",
        float_v(s.value),
        v3_text(s.color),
        float_v(s.opacity)
    )
}
fn validate_vr_view_state(
    state: &VrViewState,
    version: u32,
    width: u32,
    height: u32,
) -> Result<(), Fail> {
    if let Err(e) = state.transfer_2d.validate() {
        return Err(fail(format!("invalid VR 2D transfer table: {e}")));
    }
    if state.transfer_2d.enabled()
        && (version < VIEW_STATE_CONTRACT_VERSION_V22 || state.mode != VR_MODE_DVR)
    {
        return Err(fail("2D transfer table requires DVR V22"));
    }
    if !finite3(state.position_lps)
        || !finite3(state.focal_point_lps)
        || !finite3(state.view_up_lps)
    {
        return Err(fail("non-finite VR camera"));
    }
    let direction = sub3(state.focal_point_lps, state.position_lps);
    let degenerate = match (normalize3(direction), normalize3(state.view_up_lps)) {
        (Some(d), Some(u)) => {
            !finite3(direction) || cross3_finite(d, u).is_none_or(|c| norm3(c) <= 1e-12)
        }
        _ => true,
    };
    if degenerate {
        return Err(fail("degenerate VR camera"));
    }
    match state.projection {
        Projection::PERSPECTIVE | Projection::ENDOSCOPY => {
            if state.projection == Projection::ENDOSCOPY && version < VIEW_STATE_CONTRACT_VERSION_V2
            {
                return Err(fail("endoscopy projection requires V2"));
            }
            if !finite(state.vertical_fov_radians)
                || state.vertical_fov_radians <= 0.0
                || state.vertical_fov_radians >= std::f64::consts::PI
            {
                return Err(fail("invalid perspective FOV"));
            }
        }
        Projection::PARALLEL => {
            if !finite(state.parallel_scale_mm) || state.parallel_scale_mm <= 0.0 {
                return Err(fail("invalid parallel scale"));
            }
        }
        other => return Err(fail(format!("unsupported projection {}", other.0))),
    }
    if !finite(state.near_mm)
        || !finite(state.far_mm)
        || state.near_mm <= 0.0
        || state.near_mm >= state.far_mm
    {
        return Err(fail("invalid near/far planes"));
    }
    match state.mode {
        VR_MODE_RAY_SUM => {
            if version < VIEW_STATE_CONTRACT_VERSION_V21
                || !finite(state.ray_sum_reference_mm)
                || state.ray_sum_reference_mm <= 0.0
                || !state.iso_surfaces.is_empty()
            {
                return Err(fail(
                    "Ray Sum requires V21 and a positive reference distance",
                ));
            }
        }
        VR_MODE_DVR | VR_MODE_MIP | VR_MODE_MIN_IP | VR_MODE_AVERAGE => {
            if !state.iso_surfaces.is_empty() {
                return Err(fail("VR iso-surfaces require the iso-surface mode"));
            }
        }
        VR_MODE_ISO_SURFACE => {
            if version < VIEW_STATE_CONTRACT_VERSION_V11 {
                return Err(fail("VR iso-surface mode requires V11"));
            }
            if state.iso_surfaces.is_empty() || state.iso_surfaces.len() > MAX_VR_ISO_SURFACES {
                return Err(fail(format!(
                    "VR iso-surface count {} outside 1..{MAX_VR_ISO_SURFACES}",
                    state.iso_surfaces.len()
                )));
            }
            for surface in &state.iso_surfaces {
                if !finite(surface.value)
                    || !finite(surface.opacity)
                    || surface.opacity <= 0.0
                    || surface.opacity > 1.0
                {
                    return Err(fail(format!(
                        "invalid VR iso-surface {}",
                        iso_surface_text(surface)
                    )));
                }
                for channel in surface.color {
                    if !finite(channel) || !(0.0..=1.0).contains(&channel) {
                        return Err(fail(format!(
                            "invalid VR iso-surface colour {}",
                            v3_text(surface.color)
                        )));
                    }
                }
            }
        }
        other => return Err(fail(format!("unsupported VR mode {other}"))),
    }
    if state.mode != VR_MODE_RAY_SUM && state.ray_sum_reference_mm != 0.0 {
        return Err(fail("reference distance requires Ray Sum"));
    }
    if state.projection_range.enabled
        && (version < VIEW_STATE_CONTRACT_VERSION_V21
            || (state.mode != VR_MODE_AVERAGE && state.mode != VR_MODE_RAY_SUM)
            || !state.projection_range.valid())
    {
        return Err(fail("invalid or unsupported projection range"));
    }
    if !finite(state.sample_distance_mm)
        || state.sample_distance_mm <= 0.0
        || !finite(state.image_sample_distance)
        || state.image_sample_distance <= 0.0
    {
        return Err(fail("invalid VR sampling distance"));
    }
    let unit = state.opacity_unit_distance_mm;
    if version >= VIEW_STATE_CONTRACT_VERSION_V3 {
        if !finite(unit) || unit <= 0.0 {
            return Err(fail("invalid VR opacity unit distance"));
        }
    } else if unit != 0.0 && (!finite(unit) || unit <= 0.0) {
        return Err(fail("invalid VR opacity unit distance"));
    }
    if version >= VIEW_STATE_CONTRACT_VERSION_V4 {
        if !finite4(state.background_rgba) || state.background_rgba[3] != 1.0 {
            return Err(fail("invalid opaque VR background"));
        }
        if state.background_rgba[..3]
            .iter()
            .any(|c| *c < 0.0 || *c > 1.0)
        {
            return Err(fail("VR background channel outside 0..1"));
        }
    } else if state.background_rgba != [0.0; 4] {
        return Err(fail("VR background requires V4"));
    }
    if state.shading_enabled > 1 {
        return Err(fail(format!(
            "invalid shading flag {}",
            state.shading_enabled
        )));
    }
    if state.opacity_normals
        && (version < VIEW_STATE_CONTRACT_VERSION_V13 || state.mode != VR_MODE_DVR)
    {
        return Err(fail("opacity normals require V13 DVR"));
    }
    if state.illumination != VrIlluminationState::default() {
        if version < VIEW_STATE_CONTRACT_VERSION_V14 || state.mode != VR_MODE_DVR {
            return Err(fail("illumination requires V14 DVR"));
        }
        let material = VrLightingMaterial {
            ambient: state.ambient,
            diffuse: state.diffuse,
            specular: state.specular,
            specular_power: state.specular_power,
        };
        if let Err(e) = validate_vr_illumination(&state.illumination, &material) {
            return Err(fail(format!("illumination: {e}")));
        }
    }
    if state.jitter_enabled > 1 {
        return Err(fail(format!(
            "invalid jitter flag {}",
            state.jitter_enabled
        )));
    }
    if state.jitter_enabled != 0 && version < VIEW_STATE_CONTRACT_VERSION_V5 {
        return Err(fail("VR jitter requires V5"));
    }
    let scattering = state.scattering;
    if scattering != VrScatteringState::default() {
        if version < VIEW_STATE_CONTRACT_VERSION_V7 {
            return Err(fail("VR scattering requires V7"));
        }
        let d = scattering.light_direction_lps;
        let light = Vec3::new(d[0], d[1], d[2]);
        if !finite(scattering.blend)
            || scattering.blend <= 0.0
            || scattering.blend > 2.0
            || !finite(scattering.reach_mm)
            || scattering.reach_mm <= 0.0
            || !finite(scattering.anisotropy)
            || scattering.anisotropy.abs() >= 1.0
            || !finite(light.x)
            || !finite(light.y)
            || !finite(light.z)
            || (light.length() - 1.0).abs() > 1e-6
        {
            return Err(fail(format!(
                "invalid VR scattering {{Blend:{} ReachMM:{} Anisotropy:{} LightDirectionLPS:{}}}",
                float_v(scattering.blend),
                float_v(scattering.reach_mm),
                float_v(scattering.anisotropy),
                v3_text(d)
            )));
        }
    }
    if !state.gradient_opacity.0.is_empty() {
        if version < VIEW_STATE_CONTRACT_VERSION_V10 {
            return Err(fail("VR gradient opacity function requires V10"));
        }
        if let Err(e) = state.gradient_opacity.validate() {
            return Err(fail(e.to_string()));
        }
        if state.gradient_opacity_scale != 0.0 {
            return Err(fail("VR gradient opacity function and scale are exclusive"));
        }
    }
    let geometry = &state.geometry;
    if geometry.enabled != 0 || !geometry.depth.is_empty() || !geometry.color.is_empty() {
        if version < VIEW_STATE_CONTRACT_VERSION_V9 {
            return Err(fail("VR geometry requires V9"));
        }
        if geometry.enabled != 1 {
            return Err(fail(format!(
                "invalid VR geometry flag {}",
                geometry.enabled
            )));
        }
        let pixels = u64::from(width) * u64::from(height);
        if geometry.depth.len() as u64 != pixels || geometry.color.len() as u64 != pixels {
            return Err(fail(format!(
                "VR geometry of {}/{} pixels for a {width}x{height} output",
                geometry.depth.len(),
                geometry.color.len()
            )));
        }
        for t in &geometry.depth {
            if t.is_nan() || *t <= 0.0 {
                return Err(fail(format!("invalid VR geometry depth {}", f32_text(*t))));
            }
        }
    }
    let cut = state.cut;
    if cut != VrCutPlaneState::default() {
        if version < VIEW_STATE_CONTRACT_VERSION_V8 {
            return Err(fail("VR cut planes require V8"));
        }
        let shift = state.clipping_planes_lps.len() as u32;
        let high = if shift >= 32 { 0 } else { cut.planes >> shift };
        if cut.planes == 0
            || high != 0
            || !finite(cut.clipped_voxel_intensity)
            || !finite(cut.cap_opacity)
            || cut.cap_opacity < 0.0
            || cut.cap_opacity > 1.0
            || (cut.cap_opacity > 0.0
                && (!finite(cut.cap_window[0])
                    || !finite(cut.cap_window[1])
                    || cut.cap_window[1] <= cut.cap_window[0]))
        {
            return Err(fail(format!(
                "invalid VR cut planes {{Planes:{} ClippedVoxelIntensity:{} CapOpacity:{} CapWindow:[{} {}]}}",
                cut.planes,
                float_v(cut.clipped_voxel_intensity),
                float_v(cut.cap_opacity),
                float_v(cut.cap_window[0]),
                float_v(cut.cap_window[1])
            )));
        }
    }
    for value in [
        state.ambient,
        state.diffuse,
        state.specular,
        state.specular_power,
        state.gradient_opacity_scale,
    ] {
        if !finite(value) || value < 0.0 {
            return Err(fail("invalid shading or gradient-opacity coefficient"));
        }
    }
    if state.clipping_planes_lps.len() > MAX_CLIPPING_PLANES {
        return Err(fail(format!(
            "too many clipping planes: {}",
            state.clipping_planes_lps.len()
        )));
    }
    for plane in &state.clipping_planes_lps {
        if !finite4(*plane) || max_abs3([plane[0], plane[1], plane[2]]) == 0.0 {
            return Err(fail("invalid clipping plane"));
        }
    }
    if version < VIEW_STATE_CONTRACT_VERSION_V2 {
        let crop = &state.crop;
        if crop.enabled != 0
            || crop.region_flags != 0
            || crop.min != [0.0; 3]
            || crop.max != [0.0; 3]
            || state.cut_mask.dimensions != [0; 3]
            || !state.cut_mask.words.is_empty()
        {
            return Err(fail("crop or cut mask requires V2"));
        }
    } else {
        validate_normalized_crop(&state.crop)?;
        if state.crop.region_flags != 0 && version < VIEW_STATE_CONTRACT_VERSION_V25 {
            return Err(fail("crop regions require V25"));
        }
        validate_voxel_mask(&state.cut_mask)?;
    }
    validate_vr_label_state(&state.labels, version)?;
    let lut = &state.transfer_lut;
    if !finite(lut.domain_min) || !finite(lut.domain_max) || lut.domain_min >= lut.domain_max {
        return Err(fail("invalid transfer domain"));
    }
    if lut.samples.len() < 2 || lut.samples.len() > MAX_TRANSFER_LUT_SAMPLES {
        return Err(fail(format!(
            "transfer sample count {} outside 2..{MAX_TRANSFER_LUT_SAMPLES}",
            lut.samples.len()
        )));
    }
    if lut.samples.iter().any(|s| !transfer_sample_unit(s)) {
        return Err(fail("invalid transfer sample"));
    }
    Ok(())
}
pub(crate) fn transfer_sample_unit(s: &TransferSample) -> bool {
    [s.r, s.g, s.b, s.a]
        .iter()
        .all(|v| v.is_finite() && (0.0..=1.0).contains(v))
}
fn validate_vr_label_state(labels: &VrLabelState, version: u32) -> Result<(), Fail> {
    if labels.generation == 0 {
        if labels.dimensions != [0; 3]
            || labels.bits != 0
            || labels.only_inside != 0
            || !labels.styles.is_empty()
            || labels.volume.is_some()
        {
            return Err(fail("VR labels without a generation"));
        }
        return Ok(());
    }
    if version < VIEW_STATE_CONTRACT_VERSION_V6 {
        return Err(fail("VR labels require V6"));
    }
    if labels.dimensions.contains(&0) {
        return Err(fail("empty VR label dimensions"));
    }
    if labels.bits != 8 && labels.bits != 16 {
        return Err(fail(format!("VR label bits {}", labels.bits)));
    }
    let mut voxels = u64::from(labels.dimensions[0]);
    for dimension in &labels.dimensions[1..] {
        if voxels > u64::MAX / u64::from(*dimension) {
            return Err(fail("VR label dimensions overflow"));
        }
        voxels *= u64::from(*dimension);
    }
    if voxels > MAX_VIEW_LABEL_BYTES / u64::from(labels.bits / 8) {
        return Err(fail(format!(
            "VR label volume over {MAX_VIEW_LABEL_BYTES} bytes"
        )));
    }
    if labels.only_inside > 1 {
        return Err(fail(format!(
            "invalid VR label only-inside flag {}",
            labels.only_inside
        )));
    }
    if labels.styles.len() > MAX_VR_LABEL_STYLES {
        return Err(fail(format!(
            "{} VR label styles over {MAX_VR_LABEL_STYLES}",
            labels.styles.len()
        )));
    }
    for style in &labels.styles {
        for value in [
            style.rgb[0],
            style.rgb[1],
            style.rgb[2],
            style.opacity,
            style.blend,
        ] {
            if !finite(value) || !(0.0..=1.0).contains(&value) {
                return Err(fail("invalid VR label style"));
            }
        }
        if style.visible > 1 {
            return Err(fail(format!(
                "invalid VR label visibility {}",
                style.visible
            )));
        }
    }
    if let Some(m) = &labels.volume {
        let (cols, rows, depth) = m.dimensions();
        if [cols as u32, rows as u32, depth as u32] != labels.dimensions
            || m.storage_bits() > labels.bits
        {
            return Err(fail("VR label volume does not match its header"));
        }
    }
    Ok(())
}
fn validate_normalized_crop(crop: &NormalizedCropBox) -> Result<(), Fail> {
    if crop.region_flags & !super::vr_clip::VR_CROP_ALL_REGIONS != 0 {
        return Err(fail("invalid crop region mask"));
    }
    if crop.enabled > 1 {
        return Err(fail(format!("invalid crop flag {}", crop.enabled)));
    }
    if crop.enabled == 0 {
        if crop.region_flags != 0 || crop.min != [0.0; 3] || crop.max != [0.0; 3] {
            return Err(fail("disabled crop carries bounds"));
        }
        return Ok(());
    }
    if !finite3(crop.min) || !finite3(crop.max) {
        return Err(fail("non-finite crop bounds"));
    }
    for axis in 0..3 {
        if crop.min[axis] < 0.0 || crop.max[axis] > 1.0 || crop.min[axis] > crop.max[axis] {
            return Err(fail("invalid normalized crop bounds"));
        }
    }
    Ok(())
}
fn validate_voxel_mask(mask: &VoxelMask) -> Result<(), Fail> {
    if mask.dimensions == [0; 3] && mask.words.is_empty() {
        return Ok(());
    }
    if mask.dimensions.contains(&0) {
        return Err(fail("cut mask has zero dimension"));
    }
    let mut voxels = u64::from(mask.dimensions[0]);
    for dimension in &mask.dimensions[1..] {
        if voxels > u64::MAX / u64::from(*dimension) {
            return Err(fail("cut mask dimensions overflow"));
        }
        voxels *= u64::from(*dimension);
    }
    let words = (voxels - 1) / 32 + 1;
    if words > MAX_VIEW_CUT_MASK_BYTES / 4 || mask.words.len() as u64 != words {
        return Err(fail(format!(
            "cut mask word count {} does not match {voxels} voxels",
            mask.words.len()
        )));
    }
    Ok(())
}
pub(crate) fn validate_mpr_presentation(
    state: &MprPresentationState,
    version: u32,
) -> Result<(), RenderError> {
    if state.mode == MprPresentationMode::NONE {
        if !layer_zero(&state.base) || !layer_zero(&state.overlay) || !fusion_zero(&state.fusion) {
            return Err(fail("MPR presentation without a mode"));
        }
        return Ok(());
    }
    if version < VIEW_STATE_CONTRACT_VERSION_V12 {
        return Err(fail("MPR presentation requires V12"));
    }
    validate_presentation_layer(&state.base)?;
    match state.mode {
        MprPresentationMode::GRAY => {
            if !layer_zero(&state.overlay) || !fusion_zero(&state.fusion) {
                return Err(fail("gray MPR presentation carries an overlay"));
            }
            return Ok(());
        }
        MprPresentationMode::FUSION => {}
        other => {
            return Err(fail(format!(
                "unsupported MPR presentation mode {}",
                other.0
            )));
        }
    }
    validate_presentation_layer(&state.overlay)?;
    let fusion = &state.fusion;
    if fusion.overlay_generation == 0 || fusion.overlay_dimensions.contains(&0) {
        return Err(fail("fused MPR presentation without an overlay volume"));
    }
    let affine = fusion.target_to_overlay_index.0;
    if affine.iter().any(|v| !finite(*v)) {
        return Err(fail("non-finite fused MPR overlay mapping"));
    }
    if affine[12] != 0.0 || affine[13] != 0.0 || affine[14] != 0.0 || affine[15] != 1.0 {
        return Err(fail("fused MPR overlay mapping is not affine"));
    }
    if !(0..=3).contains(&fusion.composite) {
        return Err(fail(format!(
            "unsupported fused MPR composite {}",
            fusion.composite
        )));
    }
    if fusion.checker_size_px == 0
        || !finite(fusion.swipe_position)
        || fusion.swipe_position <= 0.0
        || fusion.swipe_position >= 1.0
        || !finite(fusion.difference_range)
        || fusion.difference_range <= 0.0
    {
        return Err(fail("invalid fused MPR composite parameters"));
    }
    if fusion.composite == 3 && fusion.difference_palette.len() != MPR_PRESENTATION_PALETTE_ENTRIES
    {
        return Err(fail(format!(
            "fused MPR difference needs a {MPR_PRESENTATION_PALETTE_ENTRIES}-entry palette"
        )));
    }
    if !fusion.difference_palette.is_empty()
        && fusion.difference_palette.len() != MPR_PRESENTATION_PALETTE_ENTRIES
    {
        return Err(fail(format!(
            "fused MPR difference palette of {} entries",
            fusion.difference_palette.len()
        )));
    }
    Ok(())
}
fn validate_presentation_layer(layer: &MprLayerPresentation) -> Result<(), Fail> {
    if !finite(layer.window_center) || !finite(layer.window_width) || layer.window_width <= 0.0 {
        return Err(fail("invalid MPR presentation window"));
    }
    if layer.window_function > 2 || layer.invert > 1 || layer.transparent_below > 1 {
        return Err(fail("invalid MPR presentation flags"));
    }
    if !finite(layer.alpha) || layer.alpha < 0.0 || layer.alpha > 1.0 || layer.bands > 256 {
        return Err(fail("invalid MPR presentation alpha or bands"));
    }
    if layer.opacity_ramp.len() > MAX_MPR_PRESENTATION_RAMP_POINTS {
        return Err(fail(format!(
            "{} MPR presentation ramp points over {MAX_MPR_PRESENTATION_RAMP_POINTS}",
            layer.opacity_ramp.len()
        )));
    }
    for (i, point) in layer.opacity_ramp.iter().enumerate() {
        if !finite(point.position)
            || !finite(point.opacity)
            || (i > 0 && point.position < layer.opacity_ramp[i - 1].position)
        {
            return Err(fail("invalid MPR presentation opacity ramp"));
        }
    }
    if !layer.palette.is_empty() && layer.palette.len() != MPR_PRESENTATION_PALETTE_ENTRIES {
        return Err(fail(format!(
            "MPR presentation palette of {} entries",
            layer.palette.len()
        )));
    }
    if layer.palette.iter().any(|p| p >> 24 != 0) {
        return Err(fail("invalid MPR presentation palette entry"));
    }
    Ok(())
}
fn layer_zero(layer: &MprLayerPresentation) -> bool {
    layer.window_center == 0.0
        && layer.window_width == 0.0
        && layer.window_function == 0
        && layer.invert == 0
        && layer.alpha == 0.0
        && layer.transparent_below == 0
        && layer.bands == 0
        && layer.opacity_ramp.is_empty()
        && layer.palette.is_empty()
}
fn fusion_zero(fusion: &MprFusionState) -> bool {
    fusion.overlay_generation == 0
        && fusion.overlay_dimensions == [0; 3]
        && fusion.target_to_overlay_index == GeometryAffine::default()
        && fusion.composite == 0
        && fusion.checker_size_px == 0
        && fusion.swipe_position == 0.0
        && fusion.difference_range == 0.0
        && fusion.difference_palette.is_empty()
}
pub(crate) fn mpr_presentation_rgba(state: &MprViewState) -> bool {
    state.presentation.mode != MprPresentationMode::NONE
}
pub fn freeze_view_state(state: &ViewState) -> Result<ViewState, RenderError> {
    validate_view_state(state)?;
    Ok(state.clone())
}
pub(crate) fn json_f64(out: &mut String, f: f64) {
    let abs = f.abs();
    let exponent = abs != 0.0 && !(1e-6..1e21).contains(&abs);
    let mut text = crate::numtext::format_float(f, if exponent { 'e' } else { 'f' }, -1);
    if exponent {
        let b = text.as_bytes();
        let n = b.len();
        if n >= 4 && b[n - 4] == b'e' && (b[n - 3] == b'-' || b[n - 3] == b'+') && b[n - 2] == b'0'
        {
            text.remove(n - 2);
        }
    }
    out.push_str(&text);
}
pub(crate) struct JsonObject<'a> {
    out: &'a mut String,
    first: bool,
}
impl<'a> JsonObject<'a> {
    pub(crate) fn new(out: &'a mut String) -> JsonObject<'a> {
        out.push('{');
        JsonObject { out, first: true }
    }
    pub(crate) fn key(&mut self, name: &str) -> &mut String {
        if !self.first {
            self.out.push(',');
        }
        self.first = false;
        self.out.push('"');
        self.out.push_str(name);
        self.out.push_str("\":");
        self.out
    }
    pub(crate) fn f64(&mut self, name: &str, v: f64) {
        json_f64(self.key(name), v);
    }
    pub(crate) fn f32(&mut self, name: &str, v: f32) {
        json_f32(self.key(name), v);
    }
    pub(crate) fn int(&mut self, name: &str, v: impl std::fmt::Display) {
        let out = self.key(name);
        out.push_str(&v.to_string());
    }
    pub(crate) fn bool(&mut self, name: &str, v: bool) {
        self.key(name).push_str(if v { "true" } else { "false" });
    }
    pub(crate) fn floats(&mut self, name: &str, values: &[f64]) {
        let out = self.key(name);
        json_f64_array(out, values);
    }
    pub(crate) fn uints(&mut self, name: &str, values: &[u32]) {
        let out = self.key(name);
        out.push('[');
        for (i, v) in values.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push_str(&v.to_string());
        }
        out.push(']');
    }
    pub(crate) fn finish(self) {
        self.out.push('}');
    }
}
pub(crate) fn json_f64_array(out: &mut String, values: &[f64]) {
    out.push('[');
    for (i, v) in values.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        json_f64(out, *v);
    }
    out.push(']');
}
pub(crate) fn json_list<T>(out: &mut String, items: &[T], mut item: impl FnMut(&mut String, &T)) {
    if items.is_empty() {
        out.push_str("null");
        return;
    }
    out.push('[');
    for (i, v) in items.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        item(out, v);
    }
    out.push(']');
}
pub(crate) fn json_transfer_sample(out: &mut String, s: &TransferSample) {
    let mut o = JsonObject::new(out);
    o.f32("R", s.r);
    o.f32("G", s.g);
    o.f32("B", s.b);
    o.f32("A", s.a);
    o.finish();
}
pub(crate) fn json_transfer_lut(out: &mut String, lut: &TransferLut) {
    let mut o = JsonObject::new(out);
    o.f64("DomainMin", lut.domain_min);
    o.f64("DomainMax", lut.domain_max);
    json_list(o.key("Samples"), &lut.samples, json_transfer_sample);
    o.finish();
}
fn json_layer(out: &mut String, layer: &MprLayerPresentation) {
    let mut o = JsonObject::new(out);
    o.f64("WindowCenter", layer.window_center);
    o.f64("WindowWidth", layer.window_width);
    o.int("WindowFunction", layer.window_function);
    o.int("Invert", layer.invert);
    o.f64("Alpha", layer.alpha);
    o.int("TransparentBelow", layer.transparent_below);
    o.int("Bands", layer.bands);
    if !layer.opacity_ramp.is_empty() {
        json_list(
            o.key("OpacityRamp"),
            &layer.opacity_ramp,
            |out, p: &FusionOpacityPoint| {
                let mut o = JsonObject::new(out);
                o.f64("Position", p.position);
                o.f64("Opacity", p.opacity);
                o.finish();
            },
        );
    }
    if !layer.palette.is_empty() {
        o.uints("Palette", &layer.palette);
    }
    o.finish();
}
fn json_presentation(out: &mut String, p: &MprPresentationState) {
    let mut o = JsonObject::new(out);
    o.int("Mode", p.mode.0);
    json_layer(o.key("Base"), &p.base);
    if p.overlay != MprLayerPresentation::default() {
        json_layer(o.key("Overlay"), &p.overlay);
    }
    if p.fusion != MprFusionState::default() {
        let f = &p.fusion;
        let out = o.key("Fusion");
        let mut fo = JsonObject::new(out);
        fo.int("OverlayGeneration", f.overlay_generation);
        fo.uints("OverlayDimensions", &f.overlay_dimensions);
        fo.floats("TargetToOverlayIndex", &f.target_to_overlay_index.0);
        fo.int("Composite", f.composite);
        fo.int("CheckerSizePx", f.checker_size_px);
        fo.f64("SwipePosition", f.swipe_position);
        fo.f64("DifferenceRange", f.difference_range);
        if !f.difference_palette.is_empty() {
            fo.uints("DifferencePalette", &f.difference_palette);
        }
        fo.finish();
    }
    o.finish();
}
fn json_mpr(out: &mut String, m: &MprViewState) {
    let mut o = JsonObject::new(out);
    if m.scalar_validity {
        o.bool("ScalarValidity", true);
    }
    if !m.cpr_columns.is_empty() {
        json_list(o.key("CPRColumns"), &m.cpr_columns, |out, c: &CprColumn| {
            let mut o = JsonObject::new(out);
            o.floats("OriginLPS", &c.origin_lps);
            o.floats("StepVLPS", &c.step_v_lps);
            o.floats("SlabAxisLPS", &c.slab_axis_lps);
            o.finish();
        });
    }
    if m.antialias_2x {
        o.bool("Antialias2x", true);
    }
    o.floats("PixelOriginLPS", &m.pixel_origin_lps);
    o.floats("PixelStepULPS", &m.pixel_step_u_lps);
    o.floats("PixelStepVLPS", &m.pixel_step_v_lps);
    o.int("Interpolation", m.interpolation.0);
    o.int("SlabMode", m.slab_mode);
    o.floats("SlabAxisLPS", &m.slab_axis_lps);
    o.f64("SlabThicknessMM", m.slab_thickness_mm);
    o.f64("SlabSampleSpacingMM", m.slab_sample_spacing_mm);
    o.f32("OutsideValue", m.outside_value);
    if m.presentation != MprPresentationState::default() {
        json_presentation(o.key("Presentation"), &m.presentation);
    }
    o.finish();
}
#[allow(clippy::needless_range_loop)]
fn json_vr(out: &mut String, v: &VrViewState) {
    let mut o = JsonObject::new(out);
    if v.fusion != VrFusionState::default() {
        v.fusion.json(o.key("Fusion"));
    }
    if v.transfer_2d != VrTransfer2dState::default() {
        let t = &v.transfer_2d;
        let mut to = JsonObject::new(o.key("Transfer2D"));
        to.int("Width", t.width);
        to.int("Height", t.height);
        to.f64("IntensityMin", t.intensity_min);
        to.f64("IntensityMax", t.intensity_max);
        to.f64("GradientMax", t.gradient_max);
        to.finish();
    }
    o.floats("PositionLPS", &v.position_lps);
    o.floats("FocalPointLPS", &v.focal_point_lps);
    o.floats("ViewUpLPS", &v.view_up_lps);
    o.int("Projection", v.projection.0);
    o.f64("VerticalFOVRadians", v.vertical_fov_radians);
    o.f64("ParallelScaleMM", v.parallel_scale_mm);
    o.f64("NearMM", v.near_mm);
    o.f64("FarMM", v.far_mm);
    o.int("Mode", v.mode);
    if v.ray_sum_reference_mm != 0.0 {
        o.f64("RaySumReferenceMM", v.ray_sum_reference_mm);
    }
    o.f64("SampleDistanceMM", v.sample_distance_mm);
    o.f64("OpacityUnitDistanceMM", v.opacity_unit_distance_mm);
    o.f64("ImageSampleDistance", v.image_sample_distance);
    o.int("ShadingEnabled", v.shading_enabled);
    if v.opacity_normals {
        o.bool("OpacityNormals", true);
    }
    o.f64("Ambient", v.ambient);
    o.f64("Diffuse", v.diffuse);
    o.f64("Specular", v.specular);
    o.f64("SpecularPower", v.specular_power);
    o.f64("GradientOpacityScale", v.gradient_opacity_scale);
    o.floats("BackgroundRGBA", &v.background_rgba);
    o.int("JitterEnabled", v.jitter_enabled);
    json_list(
        o.key("ClippingPlanesLPS"),
        &v.clipping_planes_lps,
        |out, p| json_f64_array(out, p),
    );
    {
        let c = &v.crop;
        let mut co = JsonObject::new(o.key("Crop"));
        co.int("Enabled", c.enabled);
        if c.region_flags != 0 {
            co.int("RegionFlags", c.region_flags);
        }
        co.floats("Min", &c.min);
        co.floats("Max", &c.max);
        co.finish();
    }
    {
        let mut mo = JsonObject::new(o.key("CutMask"));
        mo.uints("Dimensions", &v.cut_mask.dimensions);
        mo.finish();
    }
    json_transfer_lut(o.key("TransferLUT"), &v.transfer_lut);
    if v.surface_probe != SurfaceProbeState::default() {
        v.surface_probe.json(o.key("SurfaceProbe"));
    }
    if v.projection_range != VrValueRange::default() {
        let r = &v.projection_range;
        let mut ro = JsonObject::new(o.key("ProjectionRange"));
        ro.f64("Low", r.low);
        ro.f64("High", r.high);
        ro.bool("Enabled", r.enabled);
        ro.finish();
    }
    if v.stereo != StereoPresentationState::default() {
        v.stereo.json(o.key("Stereo"));
    }
    if !v.labels.is_zero() {
        let l = &v.labels;
        let mut lo = JsonObject::new(o.key("Labels"));
        lo.int("Generation", l.generation);
        lo.uints("Dimensions", &l.dimensions);
        lo.int("Bits", l.bits);
        lo.int("OnlyInside", l.only_inside);
        json_list(lo.key("Styles"), &l.styles, |out, s: &VrLabelStyleState| {
            let mut so = JsonObject::new(out);
            so.floats("RGB", &s.rgb);
            so.f64("Opacity", s.opacity);
            so.f64("Blend", s.blend);
            so.int("Visible", s.visible);
            so.finish();
        });
        lo.finish();
    }
    if v.scattering != VrScatteringState::default() {
        let s = &v.scattering;
        let mut so = JsonObject::new(o.key("Scattering"));
        so.f64("Blend", s.blend);
        so.f64("ReachMM", s.reach_mm);
        so.f64("Anisotropy", s.anisotropy);
        so.floats("LightDirectionLPS", &s.light_direction_lps);
        so.finish();
    }
    if v.cut != VrCutPlaneState::default() {
        let c = &v.cut;
        let mut co = JsonObject::new(o.key("Cut"));
        co.int("Planes", c.planes);
        co.f64("ClippedVoxelIntensity", c.clipped_voxel_intensity);
        co.f64("CapOpacity", c.cap_opacity);
        co.floats("CapWindow", &c.cap_window);
        co.finish();
    }
    if v.geometry != VrGeometryState::default() {
        let mut go = JsonObject::new(o.key("Geometry"));
        go.int("Enabled", v.geometry.enabled);
        go.finish();
    }
    if !v.gradient_opacity.0.is_empty() {
        json_list(o.key("GradientOpacity"), &v.gradient_opacity.0, |out, p| {
            let mut po = JsonObject::new(out);
            po.f64("Magnitude", p.magnitude);
            po.f64("Opacity", p.opacity);
            po.finish();
        });
    }
    if !v.iso_surfaces.is_empty() {
        json_list(o.key("IsoSurfaces"), &v.iso_surfaces, |out, s| {
            let mut so = JsonObject::new(out);
            so.f64("Value", s.value);
            so.floats("Color", &s.color);
            so.f64("Opacity", s.opacity);
            so.finish();
        });
    }
    if v.illumination != VrIlluminationState::default() {
        let il = &v.illumination;
        let mut io = JsonObject::new(o.key("Illumination"));
        io.int("LightCount", il.light_count);
        {
            let out = io.key("Lights");
            out.push('[');
            for i in 0..MAX_VR_DIRECTIONAL_LIGHTS {
                if i > 0 {
                    out.push(',');
                }
                let l = &il.lights[i];
                let mut lo = JsonObject::new(out);
                lo.floats("DirectionCamera", &l.direction_camera);
                lo.f64("Intensity", l.intensity);
                lo.finish();
            }
            out.push(']');
        }
        io.int("ToneMapping", il.tone_mapping.0);
        io.bool("SpecularLookup", il.specular_lookup);
        io.finish();
    }
    o.finish();
}
#[doc(hidden)]
pub fn marshal_view_state_json(state: &ViewState) -> Result<String, String> {
    let mut out = String::new();
    let mut o = JsonObject::new(&mut out);
    if state.components != ComponentPresentation::default() {
        state.components.json(o.key("Components"));
    }
    o.int("ContractVersion", state.contract_version);
    o.int("StructSize", state.struct_size);
    o.int("Kind", state.kind.0);
    o.int("VolumeGeneration", state.volume_generation);
    o.int("ViewGeneration", state.view_generation);
    o.int("PresentationGeneration", state.presentation_generation);
    o.int("OutputWidth", state.output_width);
    o.int("OutputHeight", state.output_height);
    json_mpr(o.key("MPR"), &state.mpr);
    json_vr(o.key("VR"), &state.vr);
    if state.volume_filter != VolumeFilterState::default() {
        let mut fo = JsonObject::new(o.key("VolumeFilter"));
        fo.int("Mode", state.volume_filter.mode.0);
        fo.f64("SigmaMM", state.volume_filter.sigma_mm);
        fo.finish();
    }
    o.finish();
    if let Some(bad) = ["NaN", "+Inf", "-Inf"]
        .iter()
        .filter_map(|t| out.find(t).map(|at| (at, *t)))
        .min()
    {
        return Err(format!("json: unsupported value: {}", bad.1));
    }
    Ok(out)
}
