#![forbid(unsafe_code)]
use super::cpr_path_assistant::finite_vec3;
use super::error::{ErrorKind, RenderError};
use super::frame::RenderImage;
use super::mpr_fusion::{FusionError, PatientPointMapper};
use super::mpr_render::invalid_field;
use super::parallel::{Cancellation, ctx_err};
use super::vec3::Vec3;
use super::volume::{Sampler, Volume};
use super::volume_sampler::VolumeReader;
use super::vr_clip::{VrClip, clip_keep};
use super::vr_labels::VrLabelMap;
use super::vr_prepared::VrPreparedVolume;
use super::vr_raycast::{VrRenderError, dvr_transfer_sample, render_vr_prepared_inner};
use super::vr_settings::VrRenderSettings;
use super::vr_space_leap::{VrSpaceLeap, build_vr_block_grid};
use super::vr_transfer::{
    TfAlphaStop, TfColorStop, VrLut, VrMode, VrTransferDomain, VrTransferFunction,
    correct_vr_alpha, finite,
};
use super::window::{WindowLevel, normalize_window};
use crate::display::PaletteColorLut;
use crate::fmath;
use std::fmt;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
type Ctx<'a> = Option<&'a dyn Cancellation>;
pub const ERR_FUSED_VR_UNSUPPORTED_MODE: &str =
    "dicom/render: fused volume rendering requires DVR mode";
const VR_BLOCK_SIZE: i64 = 8;
#[derive(Clone, Default)]
pub struct VrFusionLayer<'a> {
    pub volume: Option<Arc<Volume>>,
    pub primary_to_secondary: Option<PatientPointMapper<'a>>,
    pub tf: VrTransferFunction,
    pub window: WindowLevel,
    pub opacity: f64,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct VrFusionReport {
    pub secondary_samples: i64,
    pub secondary_in_bounds: i64,
}
#[derive(Debug, Clone, PartialEq)]
pub enum VrFusionError {
    Vr(VrRenderError),
    UnsupportedMode,
    Fusion(FusionError),
}
impl VrFusionError {
    pub fn is(&self, kind: ErrorKind) -> bool {
        match self {
            VrFusionError::Vr(VrRenderError::Render(e)) => e.is(kind),
            VrFusionError::Fusion(e) => e.is(kind),
            _ => false,
        }
    }
}
impl fmt::Display for VrFusionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VrFusionError::Vr(e) => e.fmt(f),
            VrFusionError::UnsupportedMode => f.write_str(ERR_FUSED_VR_UNSUPPORTED_MODE),
            VrFusionError::Fusion(e) => e.fmt(f),
        }
    }
}
impl std::error::Error for VrFusionError {}
pub fn new_vr_fusion_palette_transfer_function(
    palette: Option<&PaletteColorLut>,
    opacity_unit_distance_mm: f64,
) -> VrTransferFunction {
    const ENTRIES: i64 = 256;
    let colors: Vec<TfColorStop> = (0..ENTRIES)
        .map(|index| {
            let position = index as f64 / (ENTRIES - 1) as f64;
            let (mut r, mut g, mut b) = (position, position, position);
            if let Some(p) = palette {
                let (red, green, blue) = p.at(index);
                r = f64::from(red) / 255.0;
                g = f64::from(green) / 255.0;
                b = f64::from(blue) / 255.0;
            }
            TfColorStop {
                hu: position,
                r,
                g,
                b,
            }
        })
        .collect();
    let alpha = [
        TfAlphaStop { hu: 0.0, a: 0.0 },
        TfAlphaStop { hu: 1.0, a: 1.0 },
    ];
    VrTransferFunction::new_normalized(&colors, &alpha, opacity_unit_distance_mm)
}
pub(crate) struct FusionFrame<'a> {
    pub(crate) reader: VolumeReader,
    pub(crate) mapper: Option<PatientPointMapper<'a>>,
    pub(crate) tf: VrTransferFunction,
    pub(crate) lut: VrLut,
    pub(crate) window: WindowLevel,
    pub(crate) data_min: f64,
    pub(crate) data_max: f64,
    pub(crate) opacity: f64,
    unit_distance: f64,
    canceled: AtomicBool,
    mapping_err: Mutex<Option<RenderError>>,
    samples: AtomicI64,
    in_bounds: AtomicI64,
}
impl<'a> FusionFrame<'a> {
    fn new(
        layer: &VrFusionLayer<'a>,
        volume: &Arc<Volume>,
        reader: VolumeReader,
    ) -> FusionFrame<'a> {
        let mut tf = layer.tf.clone();
        if tf.color_stops().is_empty() && tf.alpha_stops().is_empty() {
            tf = new_vr_fusion_palette_transfer_function(None, 0.0);
        }
        let window = normalize_window(&layer.window, &volume.recommended_window());
        let (mut data_min, mut data_max) = (0.0, 1.0);
        let lut = if tf.domain() == VrTransferDomain::Hu {
            if let Some((minimum, maximum)) = volume.hu_range() {
                (data_min, data_max) = (minimum, maximum);
            }
            tf.bake_lut(data_min, data_max, tf.preferred_lut_size())
        } else {
            tf.bake_window_lut(&window, tf.preferred_lut_size())
        };
        let unit_distance = tf.opacity_unit_distance_mm();
        FusionFrame {
            reader,
            mapper: layer.primary_to_secondary,
            tf,
            lut,
            window,
            data_min,
            data_max,
            opacity: layer.opacity,
            unit_distance,
            canceled: AtomicBool::new(false),
            mapping_err: Mutex::new(None),
            samples: AtomicI64::new(0),
            in_bounds: AtomicI64::new(0),
        }
    }
    fn fail(&self, err: Option<RenderError>) {
        if let Ok(mut slot) = self.mapping_err.lock()
            && slot.is_none()
        {
            *slot = Some(err.unwrap_or_else(|| invalid_field("PrimaryToSecondary")));
            self.canceled.store(true, Ordering::SeqCst);
        }
    }
    fn report(&self) -> VrFusionReport {
        VrFusionReport {
            secondary_samples: self.samples.load(Ordering::SeqCst),
            secondary_in_bounds: self.in_bounds.load(Ordering::SeqCst),
        }
    }
    pub(crate) fn worker<'f>(&'f self, primary: &'f Volume) -> FusionWorker<'f, 'a> {
        FusionWorker {
            frame: self,
            primary,
            samples: 0,
            in_bounds: 0,
            failed: false,
        }
    }
    pub(crate) fn limit_leap(
        &self,
        ctx: Ctx<'_>,
        leap: &mut VrSpaceLeap<'_>,
        primary: &Sampler,
    ) -> Result<(), RenderError> {
        let secondary = &self.reader.sampler;
        if primary.vol.gantry_tilt_correction().applied
            || secondary.vol.gantry_tilt_correction().applied
        {
            if let Some(empty) = leap.empty.as_mut() {
                empty.iter_mut().for_each(|e| *e = false);
            }
            return Ok(());
        }
        let sv = &secondary.vol;
        let spacing = fmath::min(
            sv.col_spacing,
            fmath::min(sv.row_spacing, sv.effective_slice_spacing()),
        );
        if !finite(spacing) || spacing <= 0.0 {
            if let Some(empty) = leap.empty.as_mut() {
                empty.iter_mut().for_each(|e| *e = false);
            }
            return Ok(());
        }
        let grid = build_vr_block_grid(ctx, secondary)?;
        let mut overlay = VrSpaceLeap::new(&grid, secondary);
        overlay.for_dvr(
            &self.tf,
            &self.lut,
            &self.window,
            self.data_min,
            self.data_max,
        );
        let mut overlay_empty = overlay.empty.take().unwrap_or_default();
        if self.opacity == 0.0 {
            overlay_empty.iter_mut().for_each(|e| *e = true);
        }
        let (gnx, gny, gnz) = (leap.grid.nx, leap.grid.ny, leap.grid.nz);
        let mut no_blocks: Vec<bool> = Vec::new();
        let empty = match leap.empty.as_mut() {
            Some(e) => e.as_mut_slice(),
            None => no_blocks.as_mut_slice(),
        };
        let (gnx, gny, gnz) = if empty.is_empty() {
            (0, 0, 0)
        } else {
            (gnx, gny, gnz)
        };
        for bz in 0..gnz {
            if let Some(e) = ctx_err(ctx) {
                return Err(e);
            }
            for by in 0..gny {
                for bx in 0..gnx {
                    if ctx_err(ctx).is_some() {
                        break;
                    }
                    let index = ((bz * gny + by) * gnx + bx) as usize;
                    if !empty[index] {
                        continue;
                    }
                    let inf = f64::INFINITY;
                    let mut lo = Vec3::new(inf, inf, inf);
                    let mut hi = Vec3::new(-inf, -inf, -inf);
                    let mut valid = true;
                    let mut largest = fmath::max(
                        sv.origin.x.abs(),
                        fmath::max(sv.origin.y.abs(), sv.origin.z.abs()),
                    );
                    for z in 0..2 {
                        for y in 0..2 {
                            for x in 0..2 {
                                let point = Vec3::new(
                                    (primary.cols - 1).min((bx + x) * VR_BLOCK_SIZE) as f64,
                                    (primary.rows - 1).min((by + y) * VR_BLOCK_SIZE) as f64,
                                    (primary.depth - 1).min((bz + z) * VR_BLOCK_SIZE) as f64,
                                );
                                let patient = primary.vol.voxel_to_patient(point);
                                largest = fmath::max(
                                    largest,
                                    fmath::max(
                                        patient.x.abs(),
                                        fmath::max(patient.y.abs(), patient.z.abs()),
                                    ),
                                );
                                let point = sv.patient_to_voxel(patient);
                                if !finite_vec3(point) {
                                    valid = false;
                                    continue;
                                }
                                lo = Vec3::new(
                                    fmath::min(lo.x, point.x),
                                    fmath::min(lo.y, point.y),
                                    fmath::min(lo.z, point.z),
                                );
                                hi = Vec3::new(
                                    fmath::max(hi.x, point.x),
                                    fmath::max(hi.y, point.y),
                                    fmath::max(hi.z, point.z),
                                );
                            }
                        }
                    }
                    if !valid {
                        empty[index] = false;
                        continue;
                    }
                    let margin = 1e-8 + 1e-12 * (1.0 + largest) / spacing;
                    if !finite(margin) {
                        empty[index] = false;
                        continue;
                    }
                    let lo = lo.sub(Vec3::new(margin, margin, margin));
                    let hi = hi.add(Vec3::new(margin, margin, margin));
                    let maximum = Vec3::new(
                        (secondary.cols - 1) as f64,
                        (secondary.rows - 1) as f64,
                        (secondary.depth - 1) as f64,
                    );
                    if hi.x < 0.0
                        || hi.y < 0.0
                        || hi.z < 0.0
                        || lo.x > maximum.x
                        || lo.y > maximum.y
                        || lo.z > maximum.z
                    {
                        continue;
                    }
                    if lo.x < 0.0
                        || lo.y < 0.0
                        || lo.z < 0.0
                        || hi.x > maximum.x
                        || hi.y > maximum.y
                        || hi.z > maximum.z
                    {
                        empty[index] = false;
                        continue;
                    }
                    let block = |v: f64| fmath::f64_to_i64(v.floor()) / VR_BLOCK_SIZE;
                    let (x0, y0, z0) = (block(lo.x), block(lo.y), block(lo.z));
                    let (x1, y1, z1) = (block(hi.x), block(hi.y), block(hi.z));
                    let mut all_empty = true;
                    'scan: for z in z0..=z1 {
                        for y in y0..=y1 {
                            for x in x0..=x1 {
                                if !overlay_empty[((z * grid.ny + y) * grid.nx + x) as usize] {
                                    all_empty = false;
                                    break 'scan;
                                }
                            }
                        }
                    }
                    empty[index] = all_empty;
                }
            }
        }
        match ctx_err(ctx) {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }
}
pub(crate) struct FusionWorker<'f, 'a> {
    frame: &'f FusionFrame<'a>,
    primary: &'f Volume,
    samples: i64,
    pub(crate) in_bounds: i64,
    failed: bool,
}
impl Drop for FusionWorker<'_, '_> {
    fn drop(&mut self) {
        self.frame.samples.fetch_add(self.samples, Ordering::SeqCst);
        self.frame
            .in_bounds
            .fetch_add(self.in_bounds, Ordering::SeqCst);
    }
}
impl FusionWorker<'_, '_> {
    pub(crate) fn sample(&mut self, tex: Vec3, step_len: f64) -> super::vr_transfer::Rgba {
        let none = super::vr_transfer::Rgba::default();
        if self.failed {
            return none;
        }
        let frame = self.frame;
        self.samples += 1;
        let mut patient = self
            .primary
            .voxel_to_patient(self.primary.texture_to_voxel(tex));
        if let Some(mapper) = frame.mapper {
            match mapper(patient) {
                Err(e) if e.is(ErrorKind::MapperOutsideDomain) => return none,
                Err(e) => {
                    self.failed = true;
                    frame.fail(Some(e));
                    return none;
                }
                Ok(mapped) if !finite_vec3(mapped) => {
                    self.failed = true;
                    frame.fail(None);
                    return none;
                }
                Ok(mapped) => patient = mapped,
            }
        }
        let Some(value) = frame.reader.sample_patient(patient) else {
            return none;
        };
        self.in_bounds += 1;
        let mut rgba = dvr_transfer_sample(
            &frame.tf,
            &frame.lut,
            value,
            &frame.window,
            frame.data_min,
            frame.data_max,
        );
        rgba.a = correct_vr_alpha(rgba.a, frame.opacity, step_len, frame.unit_distance);
        rgba
    }
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn probe_skipped(
        &mut self,
        ctx: Ctx<'_>,
        o: Vec3,
        d: Vec3,
        box_min: Vec3,
        size: Vec3,
        first: f64,
        last: f64,
        step: f64,
        clip: Option<&VrClip>,
        labels: Option<&VrLabelMap>,
    ) {
        let mut t = first;
        let mut index = 0i64;
        while t <= last {
            if index & 63 == 0 && ctx_err(ctx).is_some() {
                return;
            }
            let point = o.add(d.scale(t));
            let tex = Vec3::new(
                (point.x - box_min.x) / size.x,
                (point.y - box_min.y) / size.y,
                (point.z - box_min.z) / size.z,
            );
            if clip_keep(clip, tex) && labels.is_none_or(|l| l.keeps(tex)) {
                let _ = self.sample(tex, step);
                return;
            }
            t += step;
            index += 1;
        }
    }
}
pub(crate) fn mix_vr_fusion_sample(
    r: f64,
    g: f64,
    b: f64,
    alpha: f64,
    secondary: super::vr_transfer::Rgba,
) -> (f64, f64, f64, f64) {
    let mut total = alpha + secondary.a;
    if total <= 0.0 {
        return (r, g, b, 0.0);
    }
    let inv = 1.0 / total;
    let r = (alpha * r + secondary.a * secondary.r) * inv;
    let g = (alpha * g + secondary.a * secondary.g) * inv;
    let b = (alpha * b + secondary.a * secondary.b) * inv;
    if total > 1.0 {
        total = 1.0;
    }
    (r, g, b, total)
}
struct FusionCancel<'c> {
    parent: Ctx<'c>,
    canceled: &'c AtomicBool,
}
impl Cancellation for FusionCancel<'_> {
    fn err(&self) -> Option<RenderError> {
        if let Some(e) = ctx_err(self.parent) {
            return Some(e);
        }
        if self.canceled.load(Ordering::SeqCst) {
            return Some(RenderError::new(ErrorKind::Canceled, "context canceled"));
        }
        None
    }
}
pub fn render_fused_vr_prepared(
    ctx: Ctx<'_>,
    prepared: Option<&VrPreparedVolume>,
    secondary: &VrFusionLayer<'_>,
    settings: &VrRenderSettings<'_>,
) -> (Option<RenderImage>, VrFusionReport, Option<VrFusionError>) {
    let VrRenderSettings {
        camera: cam,
        preset,
        window,
        shading,
        clip,
        quality,
    } = *settings;
    let report = VrFusionReport::default();
    if preset.mode != VrMode::DVR {
        let (img, err) = super::vr_raycast::render_vr_prepared(ctx, prepared, settings);
        if let Some(e) = err {
            return (img, report, Some(VrFusionError::Vr(e)));
        }
        return (img, report, Some(VrFusionError::UnsupportedMode));
    }
    let Some(volume) = secondary.volume.as_ref() else {
        return (
            None,
            report,
            Some(VrFusionError::Vr(VrRenderError::Render(invalid_field(
                "FusionVolume",
            )))),
        );
    };
    if !finite(secondary.opacity) || secondary.opacity < 0.0 || secondary.opacity > 1.0 {
        return (
            None,
            report,
            Some(VrFusionError::Vr(VrRenderError::Render(invalid_field(
                "FusionOpacity",
            )))),
        );
    }
    if secondary.opacity == 0.0 {
        let (img, err) = super::vr_raycast::render_vr_prepared(ctx, prepared, settings);
        return (img, report, err.map(VrFusionError::Vr));
    }
    let reader = match volume.acquire_reader() {
        Ok(r) => r,
        Err(e) => {
            return (
                None,
                report,
                Some(VrFusionError::Vr(VrRenderError::Render(e))),
            );
        }
    };
    let frame = FusionFrame::new(secondary, volume, reader);
    let cancel = FusionCancel {
        parent: ctx,
        canceled: &frame.canceled,
    };
    let (img, err) = render_vr_prepared_inner(
        Some(&cancel),
        prepared,
        cam,
        preset,
        window,
        shading,
        clip,
        quality,
        Some(&frame),
    );
    let report = frame.report();
    let mapping = frame
        .mapping_err
        .lock()
        .ok()
        .and_then(|mut slot| slot.take());
    if let Some(cause) = mapping {
        return (
            None,
            report,
            Some(VrFusionError::Fusion(FusionError::Mapping(cause))),
        );
    }
    if let Some(err) = err {
        if let Some(e) = ctx_err(ctx) {
            return (
                img,
                report,
                Some(VrFusionError::Vr(VrRenderError::Render(e))),
            );
        }
        return (img, report, Some(VrFusionError::Vr(err)));
    }
    if report.secondary_in_bounds == 0 {
        return (
            img,
            report,
            Some(VrFusionError::Fusion(FusionError::NoOverlap)),
        );
    }
    (img, report, None)
}
pub fn render_fused_vr_prepared_preview(
    ctx: Ctx<'_>,
    prepared: Option<&VrPreparedVolume>,
    secondary: &VrFusionLayer<'_>,
    settings: &VrRenderSettings<'_>,
) -> (Option<RenderImage>, VrFusionReport, Option<VrFusionError>) {
    let mut preset = settings.preset.clone();
    preset.gradient_opacity_scale = 0.0;
    preset.gradient_opacity = Default::default();
    let settings = VrRenderSettings {
        preset: &preset,
        shading: false,
        ..*settings
    };
    render_fused_vr_prepared(ctx, prepared, secondary, &settings)
}
