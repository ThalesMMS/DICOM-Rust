#![forbid(unsafe_code)]
use super::error::{ErrorKind, RenderError};
use super::frame::{RenderImage, display_gray_mapped};
use super::mpr::Plane;
use super::mpr_render::{
    MAX_OBLIQUE_SLAB_SAMPLES, MprLimits, SlabMode, invalid_field, limited_field,
    validate_mpr_reslice,
};
use super::mpr_scalar::{ScalarPlane, gather_rows};
use super::parallel::{Cancellation, ctx_err};
use super::vec3::Vec3;
use super::volume::Volume;
use super::window::{
    DEFAULT_WINDOW_CENTER, DEFAULT_WINDOW_WIDTH, PreparedVoi, WindowLevel, normalize_window,
    prepare_window,
};
use crate::display::{PaletteColorLut, cool_warm_lut};
use std::fmt;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
pub const ERR_FUSION_MAPPING: &str = "dicom/render: fused MPR mapping failed";
pub const ERR_FUSION_NO_OVERLAP: &str = "dicom/render: fused MPR overlay has no sampled overlap";
pub const ERR_MAPPER_OUTSIDE_DOMAIN: &str = "dicom/render: target point outside the mapper domain";
pub const ERR_MPR_PRESENTATION_UNSUPPORTED: &str = "render: MPR presentation unsupported";
#[doc(hidden)]
pub fn mapper_outside_domain() -> RenderError {
    RenderError::new(ErrorKind::MapperOutsideDomain, ERR_MAPPER_OUTSIDE_DOMAIN)
}
#[derive(Debug, Clone, PartialEq)]
pub enum FusionError {
    Render(RenderError),
    Mapping(RenderError),
    NoOverlap,
}
impl FusionError {
    pub fn is(&self, kind: ErrorKind) -> bool {
        match self {
            FusionError::Render(e) => e.is(kind),
            FusionError::Mapping(cause) => kind == ErrorKind::FusionMapping || cause.is(kind),
            FusionError::NoOverlap => kind == ErrorKind::FusionNoOverlap,
        }
    }
}
impl fmt::Display for FusionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FusionError::Render(e) => write!(f, "{e}"),
            FusionError::Mapping(cause) => write!(f, "{ERR_FUSION_MAPPING}: {cause}"),
            FusionError::NoOverlap => f.write_str(ERR_FUSION_NO_OVERLAP),
        }
    }
}
impl std::error::Error for FusionError {}
impl From<RenderError> for FusionError {
    fn from(e: RenderError) -> FusionError {
        FusionError::Render(e)
    }
}
pub type PatientPointMapper<'a> = &'a (dyn Fn(Vec3) -> Result<Vec3, RenderError> + Sync);
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct FusedMprSlab {
    pub mode: i64,
    pub axis: Vec3,
    pub thickness_mm: f64,
    pub sample_spacing_mm: f64,
}
#[derive(Clone, Copy, Default)]
pub struct FusedMprRequest<'a> {
    pub base: Option<&'a Arc<Volume>>,
    pub overlay: Option<&'a Arc<Volume>>,
    pub target_plane: Plane,
    pub width: i64,
    pub height: i64,
    pub target_to_source: Option<PatientPointMapper<'a>>,
    pub slab: FusedMprSlab,
    pub limits: MprLimits,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FusedMprSamplingReport {
    pub target_samples: i64,
    pub base_in_bounds: i64,
    pub overlay_in_bounds: i64,
}
#[derive(Debug, Clone, PartialEq)]
pub struct FusedScalarPlanes {
    pub base: ScalarPlane,
    pub overlay: ScalarPlane,
    pub report: FusedMprSamplingReport,
}
impl FusedScalarPlanes {
    pub fn bytes(&self) -> i64 {
        self.base.bytes() + self.overlay.bytes()
    }
    pub fn composite(
        &self,
        ctx: Option<&dyn Cancellation>,
        base: &FusionLayerPresentation,
        overlay: &FusionLayerPresentation,
    ) -> Result<RenderImage, RenderError> {
        let layers = FusedScalarLayers {
            planes: vec![self.base.clone(), self.overlay.clone()],
            reports: Vec::new(),
        };
        layers.composite(
            ctx,
            &[base.clone(), overlay.clone()],
            &FusionCompositeMode::default(),
        )
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct FusionOpacityPoint {
    pub position: f64,
    pub opacity: f64,
}
pub const FUSION_BLEND: i64 = 0;
pub const FUSION_CHECKERBOARD: i64 = 1;
pub const FUSION_SWIPE: i64 = 2;
pub const FUSION_DIFFERENCE: i64 = 3;
pub const DEFAULT_FUSION_CHECKER_SIZE_PX: i64 = 32;
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FusionCompositeMode {
    pub kind: i64,
    pub checker_size_px: i64,
    pub swipe_position: f64,
    pub difference_range: f64,
    pub difference_palette: Option<PaletteColorLut>,
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FusionLayerPresentation {
    pub window: WindowLevel,
    pub palette: Option<PaletteColorLut>,
    pub alpha: f64,
    pub opacity_ramp: Vec<FusionOpacityPoint>,
    pub transparent_below: bool,
    pub bands: i64,
}
pub fn default_fusion_presentation() -> (FusionLayerPresentation, FusionLayerPresentation) {
    (
        FusionLayerPresentation {
            alpha: 1.0,
            ..Default::default()
        },
        FusionLayerPresentation {
            alpha: 0.5,
            ..Default::default()
        },
    )
}
fn clamp_unit(value: f64) -> f64 {
    if value < 0.0 || value.is_nan() {
        0.0
    } else if value > 1.0 || value.is_infinite() {
        1.0
    } else {
        value
    }
}
fn default_scalar_voi() -> WindowLevel {
    WindowLevel::new(DEFAULT_WINDOW_CENTER, DEFAULT_WINDOW_WIDTH)
}
fn round_byte(v: f64) -> u8 {
    crate::fmath::f64_to_u8(v.round())
}
struct LayerRenderer<'a> {
    mapper: PreparedVoi,
    photometric: &'a str,
    presentation: &'a FusionLayerPresentation,
    alpha: f64,
    lower: f64,
    window: WindowLevel,
}
impl<'a> LayerRenderer<'a> {
    fn new(plane: &'a ScalarPlane, presentation: &'a FusionLayerPresentation) -> Self {
        let window = normalize_window(&presentation.window, plane.default_voi());
        LayerRenderer {
            mapper: prepare_window(&window),
            photometric: plane.photometric(),
            presentation,
            alpha: clamp_unit(presentation.alpha),
            lower: window.center - window.width / 2.0,
            window,
        }
    }
    fn sample(&self, value: f64) -> (u8, u8, u8, f64) {
        let p = self.presentation;
        if p.transparent_below && value < self.lower {
            return (0, 0, 0, 0.0);
        }
        let mut gray = display_gray_mapped(value, &self.mapper, self.photometric);
        let coordinate = f64::from(gray) / 255.0;
        if p.bands > 1 {
            let band = (p.bands - 1).min(crate::fmath::f64_to_i64(coordinate * p.bands as f64));
            gray = round_byte(band as f64 * 255.0 / (p.bands - 1) as f64);
        }
        let alpha = self.alpha * fusion_ramp_opacity(&p.opacity_ramp, coordinate);
        match &p.palette {
            None => (gray, gray, gray, alpha),
            Some(palette) => {
                let (r, g, b) = palette.at(i64::from(gray));
                (r, g, b, alpha)
            }
        }
    }
}
fn fusion_ramp_opacity(ramp: &[FusionOpacityPoint], x: f64) -> f64 {
    if ramp.is_empty() {
        return 1.0;
    }
    if x <= ramp[0].position {
        return clamp_unit(ramp[0].opacity);
    }
    for w in ramp.windows(2) {
        let (a, b) = (w[0], w[1]);
        if x <= b.position {
            if b.position <= a.position {
                return clamp_unit(b.opacity);
            }
            let t = (x - a.position) / (b.position - a.position);
            return clamp_unit(a.opacity + (b.opacity - a.opacity) * t);
        }
    }
    clamp_unit(ramp[ramp.len() - 1].opacity)
}
fn validate_fusion_presentation(p: &FusionLayerPresentation) -> Result<(), RenderError> {
    if !p.alpha.is_finite() {
        return Err(invalid_field("LayerAlpha"));
    }
    if p.bands < 0 || p.bands > 256 {
        return Err(invalid_field("LayerBands"));
    }
    for (i, point) in p.opacity_ramp.iter().enumerate() {
        if !point.position.is_finite()
            || !point.opacity.is_finite()
            || (i > 0 && point.position < p.opacity_ramp[i - 1].position)
        {
            return Err(invalid_field("LayerOpacityRamp"));
        }
    }
    Ok(())
}
fn composite_fusion_layer(
    dst: (f64, f64, f64, f64),
    source: (u8, u8, u8),
    source_a: f64,
) -> (f64, f64, f64, f64) {
    let source_a = clamp_unit(source_a);
    let one_minus = 1.0 - source_a;
    (
        f64::from(source.0) / 255.0 * source_a + dst.0 * one_minus,
        f64::from(source.1) / 255.0 * source_a + dst.1 * one_minus,
        f64::from(source.2) / 255.0 * source_a + dst.2 * one_minus,
        source_a + dst.3 * one_minus,
    )
}
#[derive(Clone, Copy)]
pub struct FusedMprOverlay<'a> {
    pub volume: Option<&'a Arc<Volume>>,
    pub target_to_source: Option<PatientPointMapper<'a>>,
}
#[derive(Clone, Default)]
pub struct FusedMprLayersRequest<'a> {
    pub base: Option<&'a Arc<Volume>>,
    pub overlays: Vec<FusedMprOverlay<'a>>,
    pub target_plane: Plane,
    pub width: i64,
    pub height: i64,
    pub slab: FusedMprSlab,
    pub limits: MprLimits,
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FusedScalarLayers {
    pub planes: Vec<ScalarPlane>,
    pub reports: Vec<FusedMprSamplingReport>,
}
impl FusedScalarLayers {
    pub fn bytes(&self) -> i64 {
        self.planes.iter().map(ScalarPlane::bytes).sum()
    }
    pub fn composite(
        &self,
        ctx: Option<&dyn Cancellation>,
        layers: &[FusionLayerPresentation],
        mode: &FusionCompositeMode,
    ) -> Result<RenderImage, RenderError> {
        if self.planes.is_empty() || layers.len() != self.planes.len() {
            return Err(invalid_field("FusedScalarLayers"));
        }
        let (width, height) = self.planes[0].dimensions();
        let n = width * height;
        for plane in &self.planes {
            if (plane.values.len() as i64) < n || (plane.valid.len() as i64) < n {
                return Err(invalid_field("FusedScalarLayers"));
            }
            if plane.dimensions() != (width, height) {
                return Err(invalid_field("FusedScalarPlaneDimensions"));
            }
        }
        for layer in layers {
            validate_fusion_presentation(layer)?;
        }
        if mode.kind != FUSION_BLEND && self.planes.len() < 2 {
            return Err(invalid_field("FusionComparisonLayers"));
        }
        if let Some(e) = ctx_err(ctx) {
            return Err(e);
        }
        let renderers: Vec<LayerRenderer<'_>> = self
            .planes
            .iter()
            .zip(layers)
            .map(|(p, l)| LayerRenderer::new(p, l))
            .collect();
        let checker = if mode.checker_size_px <= 0 {
            DEFAULT_FUSION_CHECKER_SIZE_PX
        } else {
            mode.checker_size_px
        };
        let mut swipe = mode.swipe_position;
        if !swipe.is_finite() || swipe <= 0.0 || swipe >= 1.0 {
            swipe = 0.5;
        }
        let swipe_x = crate::fmath::f64_to_i64((swipe * width as f64).round());
        let mut difference_range = mode.difference_range;
        if !difference_range.is_finite() || difference_range <= 0.0 {
            difference_range = renderers[1 % renderers.len()].window.width;
        }
        let cool_warm;
        let difference_palette = match &mode.difference_palette {
            Some(p) => p,
            None => {
                cool_warm = cool_warm_lut();
                &cool_warm
            }
        };
        let planes = &self.planes;
        let opaque = |layer: usize, sample: usize| -> Option<(u8, u8, u8)> {
            let plane = &planes[layer];
            if plane.valid[sample] == 0 {
                return None;
            }
            let (r, g, b, _) = renderers[layer].sample(plane.values[sample]);
            Some((r, g, b))
        };
        let w = width as usize;
        let rows = gather_rows::<u8, _, _>(ctx, height, w * 4, || {
            |y: i64, out: &mut [u8]| {
                for x in 0..width {
                    let sample = (y * width + x) as usize;
                    let mut c = (0.0, 0.0, 0.0, 0.0);
                    if mode.kind == FUSION_BLEND {
                        for (i, plane) in planes.iter().enumerate() {
                            if plane.valid[sample] == 0 {
                                continue;
                            }
                            let (r, g, b, a) = renderers[i].sample(plane.values[sample]);
                            if a > 0.0 {
                                c = composite_fusion_layer(c, (r, g, b), a);
                            }
                        }
                    } else {
                        let (mut first, mut second) = (0, 1);
                        match mode.kind {
                            FUSION_CHECKERBOARD if (x / checker + y / checker) % 2 == 1 => {
                                (first, second) = (1, 0);
                            }
                            FUSION_SWIPE if x >= swipe_x => {
                                (first, second) = (1, 0);
                            }
                            FUSION_DIFFERENCE
                                if planes[0].valid[sample] != 0 && planes[1].valid[sample] != 0 =>
                            {
                                let d = planes[1].values[sample] - planes[0].values[sample];
                                let index8 = crate::fmath::f64_to_i64(
                                    (clamp_unit(0.5 + d / (2.0 * difference_range)) * 255.0)
                                        .round(),
                                );
                                let (r, g, b) = difference_palette.at(index8);
                                c = (
                                    f64::from(r) / 255.0,
                                    f64::from(g) / 255.0,
                                    f64::from(b) / 255.0,
                                    1.0,
                                );
                            }
                            _ => {}
                        }
                        if c.3 == 0.0
                            && let Some((r, g, b)) =
                                opaque(first, sample).or_else(|| opaque(second, sample))
                        {
                            c = (
                                f64::from(r) / 255.0,
                                f64::from(g) / 255.0,
                                f64::from(b) / 255.0,
                                1.0,
                            );
                        }
                    }
                    let (red, green, blue, alpha) = c;
                    if alpha > 0.0 {
                        let i = x as usize * 4;
                        out[i] = round_byte(clamp_unit(red / alpha) * 255.0);
                        out[i + 1] = round_byte(clamp_unit(green / alpha) * 255.0);
                        out[i + 2] = round_byte(clamp_unit(blue / alpha) * 255.0);
                        out[i + 3] = round_byte(clamp_unit(alpha) * 255.0);
                    }
                }
            }
        })?;
        Ok(RenderImage::Rgba {
            width: w,
            height: height as usize,
            pixels: rows.concat(),
        })
    }
}
pub fn reslice_fused_mpr_layers(
    ctx: Option<&dyn Cancellation>,
    request: &FusedMprLayersRequest<'_>,
) -> (Option<FusedScalarLayers>, Option<FusionError>) {
    if request.overlays.is_empty() {
        return (None, Some(invalid_field("OverlayVolume").into()));
    }
    let mut result = FusedScalarLayers::default();
    let mut overlap = false;
    for overlay in &request.overlays {
        let (planes, err) = reslice_fused_mpr_scalar(
            ctx,
            &FusedMprRequest {
                base: request.base,
                overlay: overlay.volume,
                target_plane: request.target_plane,
                width: request.width,
                height: request.height,
                target_to_source: overlay.target_to_source,
                slab: request.slab,
                limits: request.limits,
            },
        );
        if let Some(e) = &err
            && *e != FusionError::NoOverlap
        {
            return (None, err);
        }
        let Some(planes) = planes else {
            return (None, err);
        };
        if result.planes.is_empty() {
            result.planes.push(planes.base);
        }
        result.planes.push(planes.overlay);
        result.reports.push(planes.report);
        overlap = overlap || err.is_none();
    }
    if !overlap {
        return (Some(result), Some(FusionError::NoOverlap));
    }
    (Some(result), None)
}
pub fn reslice_fused_mpr_scalar(
    ctx: Option<&dyn Cancellation>,
    request: &FusedMprRequest<'_>,
) -> (Option<FusedScalarPlanes>, Option<FusionError>) {
    if let Some(e) = ctx_err(ctx) {
        return (None, Some(e.into()));
    }
    if request.base.is_none() {
        return (None, Some(invalid_field("BaseVolume").into()));
    }
    if request.overlay.is_none() {
        return (None, Some(invalid_field("OverlayVolume").into()));
    }
    match reslice_fused_layers(ctx, request, 18) {
        Err(e) => (None, Some(e)),
        Ok((base, overlay, report)) => {
            let no_overlap = report.overlay_in_bounds == 0;
            let result = FusedScalarPlanes {
                base: base.unwrap_or_else(|| overlay.clone()),
                overlay,
                report,
            };
            (Some(result), no_overlap.then_some(FusionError::NoOverlap))
        }
    }
}
#[derive(Clone, Copy, Default)]
pub struct MappedMprRequest<'a> {
    pub source: Option<&'a Arc<Volume>>,
    pub target_plane: Plane,
    pub width: i64,
    pub height: i64,
    pub target_to_source: Option<PatientPointMapper<'a>>,
    pub slab: FusedMprSlab,
    pub limits: MprLimits,
}
#[derive(Debug, Clone, PartialEq)]
pub struct MappedScalarPlane {
    pub plane: ScalarPlane,
    pub report: FusedMprSamplingReport,
}
pub fn reslice_mapped_mpr_scalar(
    ctx: Option<&dyn Cancellation>,
    request: &MappedMprRequest<'_>,
) -> (Option<MappedScalarPlane>, Option<FusionError>) {
    if let Some(e) = ctx_err(ctx) {
        return (None, Some(e.into()));
    }
    if request.source.is_none() {
        return (None, Some(invalid_field("SourceVolume").into()));
    }
    let fused = FusedMprRequest {
        base: None,
        overlay: request.source,
        target_plane: request.target_plane,
        width: request.width,
        height: request.height,
        target_to_source: request.target_to_source,
        slab: request.slab,
        limits: request.limits,
    };
    match reslice_fused_layers(ctx, &fused, 9) {
        Err(e) => (None, Some(e)),
        Ok((_, plane, report)) => {
            let no_overlap = report.overlay_in_bounds == 0;
            (
                Some(MappedScalarPlane { plane, report }),
                no_overlap.then_some(FusionError::NoOverlap),
            )
        }
    }
}
type FusedRow = (Vec<(f64, u8)>, Vec<(f64, u8)>);
fn reslice_fused_layers(
    ctx: Option<&dyn Cancellation>,
    request: &FusedMprRequest<'_>,
    retained_bytes_per_pixel: i64,
) -> Result<(Option<ScalarPlane>, ScalarPlane, FusedMprSamplingReport), FusionError> {
    let (axis, first_offset, step, sample_count) = fused_slab_sampling(request)?;
    let mode = SlabMode::from_code(request.slab.mode).unwrap_or_default();
    let limits = validate_mpr_reslice(
        &request.target_plane,
        request.width,
        request.height,
        sample_count,
        retained_bytes_per_pixel,
        request.limits,
    )?;
    let base_reader = match request.base {
        Some(base) => Some(base.acquire_reader()?),
        None => None,
    };
    let Some(overlay) = request.overlay else {
        return Err(invalid_field("OverlayVolume").into());
    };
    let overlay_reader = overlay.acquire_reader()?;
    let mut base_plane = match request.base {
        Some(base) => Some(ScalarPlane::new(
            request.width,
            request.height,
            sample_count,
            base.photometric(),
            &default_scalar_voi(),
            limits,
        )?),
        None => None,
    };
    let mut overlay_plane = ScalarPlane::new(
        request.width,
        request.height,
        sample_count,
        overlay.photometric(),
        &default_scalar_voi(),
        limits,
    )?;
    let failed = AtomicBool::new(false);
    let mapping_err: Mutex<Option<RenderError>> = Mutex::new(None);
    let base_in = AtomicI64::new(0);
    let overlay_in = AtomicI64::new(0);
    let denom_w = (request.width - 1).max(1) as f64;
    let denom_h = (request.height - 1).max(1) as f64;
    let width = request.width;
    let rows = gather_rows::<FusedRow, _, _>(ctx, request.height, 1, || {
        |y: i64, out: &mut [FusedRow]| {
            let t = y as f64 / denom_h;
            let (mut row_base, mut row_overlay) = (0i64, 0i64);
            let mut base_row = vec![(0.0, 0u8); width as usize];
            let mut overlay_row = vec![(0.0, 0u8); width as usize];
            'pixels: for x in 0..width {
                if failed.load(Ordering::SeqCst) {
                    break;
                }
                let center = request.target_plane.at(x as f64 / denom_w, t);
                let mut base_acc = Acc::new(mode);
                let mut overlay_acc = Acc::new(mode);
                for sample in 0..sample_count {
                    if failed.load(Ordering::SeqCst) {
                        break 'pixels;
                    }
                    let target = center.add(axis.scale(first_offset + sample as f64 * step));
                    if let Some(reader) = &base_reader
                        && let Some(value) = reader.sample_patient(target)
                    {
                        base_acc.add(value);
                        row_base += 1;
                    }
                    let mut source = target;
                    if let Some(mapper) = request.target_to_source {
                        match mapper(target) {
                            Err(e) if e.is(ErrorKind::MapperOutsideDomain) => continue,
                            Ok(mapped) if mapped.finite() => source = mapped,
                            other => {
                                let cause = match other {
                                    Err(e) => e,
                                    Ok(_) => invalid_field("TargetToSource"),
                                };
                                if let Ok(mut slot) = mapping_err.lock()
                                    && slot.is_none()
                                {
                                    *slot = Some(cause);
                                }
                                failed.store(true, Ordering::SeqCst);
                                break 'pixels;
                            }
                        }
                    }
                    if let Some(value) = overlay_reader.sample_patient(source) {
                        overlay_acc.add(value);
                        row_overlay += 1;
                    }
                }
                if let Some(v) = base_acc.finish() {
                    base_row[x as usize] = (v, 1);
                }
                if let Some(v) = overlay_acc.finish() {
                    overlay_row[x as usize] = (v, 1);
                }
            }
            base_in.fetch_add(row_base, Ordering::SeqCst);
            overlay_in.fetch_add(row_overlay, Ordering::SeqCst);
            out[0] = (base_row, overlay_row);
        }
    });
    if let Ok(mut slot) = mapping_err.lock()
        && let Some(cause) = slot.take()
    {
        return Err(FusionError::Mapping(cause));
    }
    let rows = rows?;
    let w = width as usize;
    for (y, mut row) in rows.into_iter().enumerate() {
        let (base_row, overlay_row) = row.pop().unwrap_or_default();
        for (x, (v, ok)) in overlay_row.into_iter().enumerate() {
            overlay_plane.values[y * w + x] = v;
            overlay_plane.valid[y * w + x] = ok;
        }
        if let Some(plane) = base_plane.as_mut() {
            for (x, (v, ok)) in base_row.into_iter().enumerate() {
                plane.values[y * w + x] = v;
                plane.valid[y * w + x] = ok;
            }
        }
    }
    let report = FusedMprSamplingReport {
        target_samples: request.width * request.height * sample_count,
        base_in_bounds: base_in.load(Ordering::SeqCst),
        overlay_in_bounds: overlay_in.load(Ordering::SeqCst),
    };
    Ok((base_plane, overlay_plane, report))
}
struct Acc {
    mode: SlabMode,
    acc: f64,
    count: i64,
}
impl Acc {
    fn new(mode: SlabMode) -> Acc {
        Acc {
            mode,
            acc: 0.0,
            count: 0,
        }
    }
    fn add(&mut self, value: f64) {
        match self.mode {
            SlabMode::MinIp => {
                if self.count == 0 || value < self.acc {
                    self.acc = value;
                }
            }
            SlabMode::Average | SlabMode::Sum => self.acc += value,
            _ => {
                if self.count == 0 || value > self.acc {
                    self.acc = value;
                }
            }
        }
        self.count += 1;
    }
    fn finish(&self) -> Option<f64> {
        if self.count == 0 {
            return None;
        }
        if self.mode == SlabMode::Average {
            return Some(self.acc / self.count as f64);
        }
        Some(self.acc)
    }
}
fn fused_slab_sampling(
    request: &FusedMprRequest<'_>,
) -> Result<(Vec3, f64, f64, i64), RenderError> {
    let Some(mode) = SlabMode::from_code(request.slab.mode) else {
        return Err(invalid_field("SlabMode"));
    };
    let thickness = request.slab.thickness_mm;
    let mut spacing = request.slab.sample_spacing_mm;
    if !thickness.is_finite() || thickness < 0.0 {
        return Err(invalid_field("SlabThicknessMM"));
    }
    if !spacing.is_finite() || spacing < 0.0 {
        return Err(invalid_field("SlabSampleSpacingMM"));
    }
    let mut axis = request.slab.axis;
    if axis.length() == 0.0 {
        axis = request.target_plane.u.cross(request.target_plane.v);
    }
    if !axis.finite() || axis.length() == 0.0 {
        return Err(invalid_field("SlabAxis"));
    }
    let axis = axis.normalize();
    if mode == SlabMode::None || thickness == 0.0 {
        return Ok((axis, 0.0, 0.0, 1));
    }
    if spacing == 0.0 {
        spacing = smallest_positive_volume_spacing(&[request.base, request.overlay]);
    }
    if spacing <= 0.0 {
        return Err(invalid_field("SlabSampleSpacingMM"));
    }
    let intervals_float = (thickness / spacing).ceil();
    if intervals_float.is_infinite() || intervals_float >= MAX_OBLIQUE_SLAB_SAMPLES as f64 {
        return Err(limited_field("SlabSamples"));
    }
    let intervals = crate::fmath::f64_to_i64(intervals_float).max(1);
    Ok((
        axis,
        -thickness / 2.0,
        thickness / intervals as f64,
        intervals + 1,
    ))
}
fn smallest_positive_volume_spacing(volumes: &[Option<&Arc<Volume>>]) -> f64 {
    let mut spacing = f64::INFINITY;
    for v in volumes.iter().flatten() {
        for candidate in [v.col_spacing, v.row_spacing, v.slice_spacing] {
            if candidate > 0.0 && candidate.is_finite() && candidate < spacing {
                spacing = candidate;
            }
        }
    }
    if spacing == f64::INFINITY {
        0.0
    } else {
        spacing
    }
}
impl ScalarPlane {
    pub fn composite_over(
        &self,
        ctx: Option<&dyn Cancellation>,
        base: &RenderImage,
        layer: &FusionLayerPresentation,
    ) -> Result<RenderImage, RenderError> {
        let n = self.width * self.height;
        if (self.values.len() as i64) < n || (self.valid.len() as i64) < n {
            return Err(invalid_field("ScalarPlane"));
        }
        let (bw, bh) = base.size();
        if matches!(base, RenderImage::Nil) {
            return Err(invalid_field("ScalarPlane"));
        }
        if bw as i64 != self.width || bh as i64 != self.height {
            return Err(invalid_field("BaseImageDimensions"));
        }
        validate_fusion_presentation(layer)?;
        if let Some(e) = ctx_err(ctx) {
            return Err(e);
        }
        let renderer = LayerRenderer::new(self, layer);
        let w = self.width as usize;
        let rows = gather_rows::<u8, _, _>(ctx, self.height, w * 4, || {
            |y: i64, out: &mut [u8]| {
                for x in 0..w {
                    let sample = y as usize * w + x;
                    let (br, bg, bb, ba) = match base {
                        RenderImage::Gray { pixels, .. } => {
                            let v = pixels[sample];
                            (v, v, v, 255)
                        }
                        RenderImage::Rgba { pixels, .. } => (
                            pixels[sample * 4],
                            pixels[sample * 4 + 1],
                            pixels[sample * 4 + 2],
                            pixels[sample * 4 + 3],
                        ),
                        RenderImage::Nil => (0, 0, 0, 0),
                    };
                    let (mut r, mut g, mut b, mut layer_alpha) = (0, 0, 0, 0.0);
                    if self.valid[sample] != 0 {
                        (r, g, b, layer_alpha) = renderer.sample(self.values[sample]);
                    }
                    let i = x * 4;
                    if layer_alpha <= 0.0 {
                        out[i..i + 4].copy_from_slice(&[br, bg, bb, ba]);
                        continue;
                    }
                    let (red, green, blue, alpha) = composite_fusion_layer(
                        (
                            f64::from(br) / 255.0,
                            f64::from(bg) / 255.0,
                            f64::from(bb) / 255.0,
                            f64::from(ba) / 255.0,
                        ),
                        (r, g, b),
                        layer_alpha,
                    );
                    out[i] = round_byte(clamp_unit(red) * 255.0);
                    out[i + 1] = round_byte(clamp_unit(green) * 255.0);
                    out[i + 2] = round_byte(clamp_unit(blue) * 255.0);
                    out[i + 3] = round_byte(clamp_unit(alpha) * 255.0);
                }
            }
        })?;
        Ok(RenderImage::Rgba {
            width: w,
            height: self.height as usize,
            pixels: rows.concat(),
        })
    }
}
pub const MAX_MPR_PRESENTATION_RAMP_POINTS: usize = 64;
pub const MPR_PRESENTATION_PALETTE_ENTRIES: usize = 256;
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MprLayerPresentation {
    pub window_center: f64,
    pub window_width: f64,
    pub window_function: u32,
    pub invert: u32,
    pub alpha: f64,
    pub transparent_below: u32,
    pub bands: u32,
    pub opacity_ramp: Vec<FusionOpacityPoint>,
    pub palette: Vec<u32>,
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MprFusionState {
    pub overlay_generation: u64,
    pub overlay_dimensions: [u32; 3],
    pub target_to_overlay_index: super::affine::GeometryAffine,
    pub composite: i64,
    pub checker_size_px: u32,
    pub swipe_position: f64,
    pub difference_range: f64,
    pub difference_palette: Vec<u32>,
}
pub fn new_mpr_layer_presentation(
    layer: &FusionLayerPresentation,
    photometric: &str,
) -> Result<MprLayerPresentation, RenderError> {
    if layer.window.lut.is_some() {
        return Err(RenderError::new(
            ErrorKind::MprPresentationUnsupported,
            ERR_MPR_PRESENTATION_UNSUPPORTED,
        ));
    }
    validate_fusion_presentation(layer)?;
    let window = normalize_window(&layer.window, &default_scalar_voi());
    let result = MprLayerPresentation {
        window_center: window.center,
        window_width: window.width,
        window_function: window.function as u32,
        invert: u32::from(super::frame::normalized_photometric(photometric) == "MONOCHROME1"),
        alpha: clamp_unit(layer.alpha),
        transparent_below: u32::from(layer.transparent_below),
        bands: layer.bands as u32,
        opacity_ramp: layer.opacity_ramp.clone(),
        palette: pack_mpr_presentation_palette(layer.palette.as_ref()),
    };
    if result.opacity_ramp.len() > MAX_MPR_PRESENTATION_RAMP_POINTS {
        return Err(RenderError::new(
            ErrorKind::MprPresentationUnsupported,
            ERR_MPR_PRESENTATION_UNSUPPORTED,
        ));
    }
    Ok(result)
}
pub fn pack_mpr_presentation_palette(palette: Option<&PaletteColorLut>) -> Vec<u32> {
    let Some(palette) = palette else {
        return Vec::new();
    };
    (0..MPR_PRESENTATION_PALETTE_ENTRIES as i64)
        .map(|i| {
            let (r, g, b) = palette.at(i);
            u32::from(r) | u32::from(g) << 8 | u32::from(b) << 16
        })
        .collect()
}
pub fn new_mpr_fusion_composite(
    mode: &FusionCompositeMode,
    overlay: &MprLayerPresentation,
) -> MprFusionState {
    let checker = if mode.checker_size_px <= 0 {
        DEFAULT_FUSION_CHECKER_SIZE_PX
    } else {
        mode.checker_size_px
    };
    let mut swipe = mode.swipe_position;
    if !swipe.is_finite() || swipe <= 0.0 || swipe >= 1.0 {
        swipe = 0.5;
    }
    let mut range = mode.difference_range;
    if !range.is_finite() || range <= 0.0 {
        range = overlay.window_width;
    }
    let palette = match &mode.difference_palette {
        Some(p) => pack_mpr_presentation_palette(Some(p)),
        None => pack_mpr_presentation_palette(Some(&cool_warm_lut())),
    };
    MprFusionState {
        composite: mode.kind,
        checker_size_px: checker as u32,
        swipe_position: swipe,
        difference_range: range,
        difference_palette: palette,
        ..Default::default()
    }
}
pub fn fused_slab_plan(
    request: &FusedMprRequest<'_>,
) -> Result<(Vec3, f64, f64, i64), RenderError> {
    fused_slab_sampling(request)
}
