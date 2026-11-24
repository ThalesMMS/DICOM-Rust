#![forbid(unsafe_code)]
use super::error::RenderError;
use super::frame::RenderImage;
use super::parallel::{Cancellation, ctx_err, parallel_rows_worker_context};
use super::vec3::Vec3;
use super::volume::{Sampler, TextureSample, Volume};
use super::vr_camera::{VrCamera, VrCameraProjection};
use super::vr_clip::{VrClip, clip_keep, clip_ray};
use super::vr_fusion::{FusionFrame, FusionWorker, mix_vr_fusion_sample};
use super::vr_geometry::{
    VrFragment, clip_geometry_depth, composite_fragments_before, composite_vr_fragments, depth_at,
    depth_fragments,
};
use super::vr_gradient_opacity::VrGradientOpacity;
use super::vr_labels::{VrLabelMap, mix_vr_label};
use super::vr_prepared::VrPreparedVolume;
use super::vr_quality::{
    VrAdaptiveStep, VrAdaptiveStepper, VrQuality, VrRayGuard, VrRaySamplingPlan, VrSamplingError,
    resolve_vr_ray_sampling, vr_ray_jitter,
};
use super::vr_scattering::{
    VrScattering, henyey_greenstein, vr_scattering_coefficient, vr_scattering_light,
};
use super::vr_settings::VrRenderSettings;
use super::vr_space_leap::VrSpaceLeap;
use super::vr_transfer::{
    Rgba, VrLut, VrMode, VrPreset, VrTransferDomain, VrTransferFunction, clamp_unit,
    correct_vr_alpha, finite, lerp, linear_to_srgb, srgb_to_linear, vr_mip_color, vr_window_bounds,
};
use super::vr_volume::VR_OPACITY_THRESHOLD;
use super::window::{WindowLevel, normalize_window, windowed_unit};
use crate::fmath;
use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
type Ctx<'a> = Option<&'a dyn Cancellation>;
#[derive(Debug, Clone, PartialEq)]
pub enum VrRenderError {
    Render(RenderError),
    Sampling(VrSamplingError),
    InvalidValueRange,
}
impl fmt::Display for VrRenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VrRenderError::Render(e) => e.fmt(f),
            VrRenderError::Sampling(e) => e.fmt(f),
            VrRenderError::InvalidValueRange => {
                f.write_str(super::vr_projection::ERR_INVALID_VR_VALUE_RANGE)
            }
        }
    }
}
impl std::error::Error for VrRenderError {}
impl From<RenderError> for VrRenderError {
    fn from(e: RenderError) -> Self {
        VrRenderError::Render(e)
    }
}
pub(crate) fn normalized_vr_data_coordinate(value: f64, minimum: f64, maximum: f64) -> f64 {
    if !finite(value) || !finite(minimum) || !finite(maximum) || maximum <= minimum {
        return 0.0;
    }
    clamp_unit((value - minimum) / (maximum - minimum))
}
pub(crate) fn dvr_transfer_sample(
    tf: &VrTransferFunction,
    lut: &VrLut,
    hu: f64,
    window: &WindowLevel,
    data_min: f64,
    data_max: f64,
) -> Rgba {
    if tf.domain() == VrTransferDomain::Normalized {
        return lut.lookup(tf.window_coordinate(hu, window));
    }
    let density = normalized_vr_data_coordinate(hu, data_min, data_max);
    let mut rgba = lut.lookup(density);
    rgba.a *= windowed_unit(hu, window);
    rgba
}
pub(crate) fn to8(v: f64) -> u8 {
    if v <= 0.0 {
        return 0;
    }
    if v >= 1.0 {
        return 255;
    }
    fmath::f64_to_u8(v * 255.0 + 0.5)
}
pub fn adjust_vr_window(wl: &WindowLevel, dx: f64, dy: f64) -> WindowLevel {
    let mut wl = wl.clone();
    let step = fmath::max(wl.width.abs() / 100.0, 1.0);
    wl.center -= dy * step;
    wl.width += dx * step;
    if wl.width < 1.0 {
        wl.width = 1.0;
    }
    wl
}
pub(crate) fn intersect_aabb(o: Vec3, d: Vec3, bmin: Vec3, bmax: Vec3) -> Option<(f64, f64)> {
    let (mut t0, mut t1) = (f64::NEG_INFINITY, f64::INFINITY);
    let mut axis = |oa: f64, da: f64, mn: f64, mx: f64| -> bool {
        if da.abs() < 1e-12 {
            return oa >= mn && oa <= mx;
        }
        let inv = 1.0 / da;
        let (mut ta, mut tb) = ((mn - oa) * inv, (mx - oa) * inv);
        if ta > tb {
            std::mem::swap(&mut ta, &mut tb);
        }
        if ta > t0 {
            t0 = ta;
        }
        if tb < t1 {
            t1 = tb;
        }
        t0 <= t1
    };
    if !axis(o.x, d.x, bmin.x, bmax.x)
        || !axis(o.y, d.y, bmin.y, bmax.y)
        || !axis(o.z, d.z, bmin.z, bmax.z)
    {
        return None;
    }
    if t1 < 0.0 {
        return None;
    }
    if t0 < 0.0 {
        t0 = 0.0;
    }
    Some((t0, t1))
}
pub(crate) struct RayGenerator {
    origin: Vec3,
    right: Vec3,
    up: Vec3,
    forward: Vec3,
    projection: VrCameraProjection,
    distance: f64,
    aspect_tan: f64,
    tan_half_fov: f64,
    inv_width: f64,
    inv_height: f64,
}
pub(crate) struct RayRow {
    origin_base: Vec3,
    dir_base: Vec3,
}
impl RayGenerator {
    pub(crate) fn new(cam: &VrCamera, w: i64, h: i64) -> RayGenerator {
        let (right, up, forward) = cam.basis();
        let aspect = if h > 0 { w as f64 / h as f64 } else { 1.0 };
        let inv_width = if w > 0 { 1.0 / w as f64 } else { 1.0 };
        let inv_height = if h > 0 { 1.0 / h as f64 } else { 1.0 };
        let tan_half_fov = fmath::tan(cam.fov_y / 2.0);
        let projection = cam.projection.normalized();
        let origin = if projection == VrCameraProjection::ENDOSCOPY {
            cam.target
        } else {
            cam.position()
        };
        RayGenerator {
            origin,
            right,
            up,
            forward,
            projection,
            distance: cam.distance,
            aspect_tan: aspect * tan_half_fov,
            tan_half_fov,
            inv_width,
            inv_height,
        }
    }
    pub(crate) fn row_base(&self, py: i64) -> RayRow {
        let ndcy = (1.0 - 2.0 * (py as f64 + 0.5) * self.inv_height) * self.tan_half_fov;
        if self.projection == VrCameraProjection::PARALLEL {
            return RayRow {
                origin_base: self.origin.add(self.up.scale(ndcy * self.distance)),
                dir_base: self.forward,
            };
        }
        RayRow {
            origin_base: self.origin,
            dir_base: self.forward.add(self.up.scale(ndcy)),
        }
    }
    pub(crate) fn ray(&self, px: i64, row: &RayRow) -> (Vec3, Vec3) {
        let ndcx = (2.0 * (px as f64 + 0.5) * self.inv_width - 1.0) * self.aspect_tan;
        if self.projection == VrCameraProjection::PARALLEL {
            return (
                row.origin_base.add(self.right.scale(ndcx * self.distance)),
                self.forward,
            );
        }
        (
            row.origin_base,
            row.dir_base.add(self.right.scale(ndcx)).normalize(),
        )
    }
}
fn tex_to_index(s: &Sampler, tex: Vec3) -> Vec3 {
    Vec3::new(
        tex.x * (s.cols - 1) as f64,
        tex.y * (s.rows - 1) as f64,
        tex.z * (s.depth - 1) as f64,
    )
}
pub(crate) fn texture_at(s: &Sampler, tex: Vec3) -> Option<f64> {
    s.trilinear_at(tex_to_index(s, tex))
}
fn texture_sample(s: &Sampler, tex: Vec3) -> Option<TextureSample> {
    s.voxel_sample(tex_to_index(s, tex))
}
fn clamp_index(s: &Sampler, p: Vec3) -> Vec3 {
    Vec3::new(
        fmath::max(0.0, fmath::min((s.cols - 1) as f64, p.x)),
        fmath::max(0.0, fmath::min((s.rows - 1) as f64, p.y)),
        fmath::max(0.0, fmath::min((s.depth - 1) as f64, p.z)),
    )
}
fn value_clamped(s: &Sampler, col: i64, row: i64, slice: i64) -> f64 {
    s.value_or_zero(
        col.min(s.cols - 1).max(0),
        row.min(s.rows - 1).max(0),
        slice.min(s.depth - 1).max(0),
    )
}
const AXIS_OFFSETS: [Vec3; 6] = [
    Vec3 {
        x: 1.0,
        y: 0.0,
        z: 0.0,
    },
    Vec3 {
        x: -1.0,
        y: 0.0,
        z: 0.0,
    },
    Vec3 {
        x: 0.0,
        y: 1.0,
        z: 0.0,
    },
    Vec3 {
        x: 0.0,
        y: -1.0,
        z: 0.0,
    },
    Vec3 {
        x: 0.0,
        y: 0.0,
        z: 1.0,
    },
    Vec3 {
        x: 0.0,
        y: 0.0,
        z: -1.0,
    },
];
fn gradient_index_at(s: &Sampler, tex: Vec3) -> Vec3 {
    let index = tex_to_index(s, tex);
    let v = AXIS_OFFSETS.map(|offset| {
        s.trilinear_at(clamp_index(s, index.add(offset)))
            .unwrap_or(0.0)
    });
    Vec3::new(v[0] - v[1], v[2] - v[3], v[4] - v[5])
}
fn index_to_tex(s: &Sampler, q: Vec3) -> Vec3 {
    let axis = |v: f64, n: i64| if n <= 1 { 0.0 } else { v / (n - 1) as f64 };
    Vec3::new(axis(q.x, s.cols), axis(q.y, s.rows), axis(q.z, s.depth))
}
pub(crate) fn clipped_gradient_index(
    s: &Sampler,
    tex: Vec3,
    clip: Option<&VrClip>,
    clipped_value: f64,
) -> Option<Vec3> {
    let clip = clip?;
    let index = tex_to_index(s, tex);
    let neighbours = AXIS_OFFSETS.map(|offset| clamp_index(s, index.add(offset)));
    if !neighbours
        .iter()
        .any(|q| clip.cut_away(index_to_tex(s, *q)))
    {
        return None;
    }
    let v = neighbours.map(|q| {
        if clip.cut_away(index_to_tex(s, q)) {
            clipped_value
        } else {
            s.trilinear_at(q).unwrap_or(0.0)
        }
    });
    Some(Vec3::new(v[0] - v[1], v[2] - v[3], v[4] - v[5]))
}
fn gradient_opacity_vector(s: &Sampler, g: Vec3) -> Vec3 {
    Vec3::new(
        g.x * (s.cols - 1) as f64 / s.cols.max(2) as f64,
        g.y * (s.rows - 1) as f64 / s.rows.max(2) as f64,
        g.z * (s.depth - 1) as f64 / s.depth.max(2) as f64,
    )
}
#[derive(Default)]
struct GradientCell {
    valid: bool,
    x0: i64,
    y0: i64,
    z0: i64,
    gradients: [[[Vec3; 2]; 2]; 2],
}
fn lerp_vec3(a: Vec3, b: Vec3, fraction: f64) -> Vec3 {
    Vec3::new(
        lerp(a.x, b.x, fraction),
        lerp(a.y, b.y, fraction),
        lerp(a.z, b.z, fraction),
    )
}
impl GradientCell {
    fn gradient_index(&mut self, s: &Sampler, sample: &TextureSample) -> Vec3 {
        if !self.valid || self.x0 != sample.x0 || self.y0 != sample.y0 || self.z0 != sample.z0 {
            self.prepare(s, sample);
        }
        let g = &self.gradients;
        let g00 = lerp_vec3(g[0][0][0], g[0][0][1], sample.fx);
        let g10 = lerp_vec3(g[0][1][0], g[0][1][1], sample.fx);
        let g01 = lerp_vec3(g[1][0][0], g[1][0][1], sample.fx);
        let g11 = lerp_vec3(g[1][1][0], g[1][1][1], sample.fx);
        lerp_vec3(
            lerp_vec3(g00, g10, sample.fy),
            lerp_vec3(g01, g11, sample.fy),
            sample.fz,
        )
    }
    fn prepare(&mut self, s: &Sampler, sample: &TextureSample) {
        self.valid = true;
        (self.x0, self.y0, self.z0) = (sample.x0, sample.y0, sample.z0);
        let sc = &sample.c;
        let c = [
            [[sc[0], sc[1]], [sc[2], sc[3]]],
            [[sc[4], sc[5]], [sc[6], sc[7]]],
        ];
        let (x0, y0, z0) = (sample.x0, sample.y0, sample.z0);
        for z in 0..2usize {
            for y in 0..2usize {
                let left = value_clamped(s, x0 - 1, y0 + y as i64, z0 + z as i64);
                let right = value_clamped(s, x0 + 2, y0 + y as i64, z0 + z as i64);
                self.gradients[z][y][0].x = c[z][y][1] - left;
                self.gradients[z][y][1].x = right - c[z][y][0];
            }
        }
        for z in 0..2usize {
            for x in 0..2usize {
                let below = value_clamped(s, x0 + x as i64, y0 - 1, z0 + z as i64);
                let above = value_clamped(s, x0 + x as i64, y0 + 2, z0 + z as i64);
                self.gradients[z][0][x].y = c[z][1][x] - below;
                self.gradients[z][1][x].y = above - c[z][0][x];
            }
        }
        for y in 0..2usize {
            for x in 0..2usize {
                let behind = value_clamped(s, x0 + x as i64, y0 + y as i64, z0 - 1);
                let in_front = value_clamped(s, x0 + x as i64, y0 + y as i64, z0 + 2);
                self.gradients[0][y][x].z = c[1][y][x] - behind;
                self.gradients[1][y][x].z = in_front - c[0][y][x];
            }
        }
    }
}
#[allow(clippy::too_many_arguments)]
fn dvr_index_gradient(
    s: &Sampler,
    cell: Option<&mut GradientCell>,
    sample: &TextureSample,
    tex: Vec3,
    clip: Option<&VrClip>,
    cut_gradients: bool,
    use_cell: bool,
    clipped_value: f64,
) -> Vec3 {
    if cut_gradients && let Some(gradient) = clipped_gradient_index(s, tex, clip, clipped_value) {
        return gradient;
    }
    if use_cell {
        return match cell {
            Some(cell) => cell.gradient_index(s, sample),
            None => Vec3::ZERO,
        };
    }
    gradient_index_at(s, tex)
}
fn clip_cut_away(clip: Option<&VrClip>, tex: Vec3) -> bool {
    clip.is_some_and(|c| c.cut_away(tex))
}
fn clip_has_cut_planes(clip: Option<&VrClip>) -> bool {
    clip.is_some_and(|c| c.has_cut_planes())
}
struct Scene<'a> {
    sampler: &'a Sampler,
    lut: &'a VrLut,
    preset: &'a VrPreset,
    window: &'a WindowLevel,
    data_min: f64,
    data_max: f64,
    clip: Option<&'a VrClip>,
    labels: Option<&'a VrLabelMap>,
    box_min: Vec3,
    size: Vec3,
}
impl Scene<'_> {
    fn tex(&self, pw: Vec3) -> Vec3 {
        Vec3::new(
            (pw.x - self.box_min.x) / self.size.x,
            (pw.y - self.box_min.y) / self.size.y,
            (pw.z - self.box_min.z) / self.size.z,
        )
    }
    fn transfer(&self, value: f64) -> Rgba {
        dvr_transfer_sample(
            &self.preset.tf,
            self.lut,
            value,
            self.window,
            self.data_min,
            self.data_max,
        )
    }
    fn opacity_gradient(&self, tex: Vec3, clipped_value: f64) -> Vec3 {
        let s = self.sampler;
        let index = tex_to_index(s, tex);
        let alpha_at = |offset: Vec3| -> f64 {
            let q = clamp_index(s, index.add(offset));
            let neighbour = index_to_tex(s, q);
            let mut value = s.trilinear_at(q).unwrap_or(0.0);
            if clip_cut_away(self.clip, neighbour) {
                value = clipped_value;
            }
            if let Some(t2d) = &self.preset.transfer_2d {
                let gradient = dvr_index_gradient(
                    s,
                    None,
                    &TextureSample::default(),
                    neighbour,
                    self.clip,
                    clip_has_cut_planes(self.clip),
                    false,
                    clipped_value,
                );
                return t2d
                    .lookup(value, s.vol.physical_gradient_magnitude(gradient))
                    .a;
            }
            self.transfer(value).a
        };
        let v = AXIS_OFFSETS.map(alpha_at);
        Vec3::new(v[0] - v[1], v[2] - v[3], v[4] - v[5])
    }
    fn gradient_opacity_factor(&self, index_gradient: Vec3) -> f64 {
        let preset = self.preset;
        if !preset.gradient_opacity.is_empty() {
            return preset
                .gradient_opacity
                .at(self.sampler.vol.physical_gradient_magnitude(index_gradient));
        }
        if preset.gradient_opacity_scale > 0.0 {
            return clamp_unit(
                gradient_opacity_vector(self.sampler, index_gradient).length()
                    / preset.gradient_opacity_scale,
            );
        }
        1.0
    }
}
fn uses_gradient_opacity(preset: &VrPreset) -> bool {
    !preset.gradient_opacity.is_empty() || preset.gradient_opacity_scale > 0.0
}
struct Projection {
    mode: VrMode,
    window: WindowLevel,
    value_range: super::vr_projection::VrValueRange,
    colored_mip: bool,
    ray_sum_reference_mm: f64,
}
impl Projection {
    fn new(preset: &VrPreset, window: &WindowLevel, diagonal_mm: f64) -> Projection {
        Projection {
            mode: preset.mode,
            window: window.clone(),
            ray_sum_reference_mm: diagonal_mm,
            colored_mip: preset.colored_mip && preset.mode == VrMode::MIP,
            value_range: if preset.uses_value_range() {
                preset.projection_range
            } else {
                Default::default()
            },
        }
    }
    fn display(&self, acc: f64, count: i64) -> f64 {
        match self.mode {
            VrMode::AVERAGE => acc / count as f64,
            VrMode::RAY_SUM => {
                if !(self.ray_sum_reference_mm > 0.0) {
                    return 0.0;
                }
                windowed_unit(acc / self.ray_sum_reference_mm, &self.window)
            }
            _ => acc,
        }
    }
}
pub(crate) const VR_CANCELLATION_CHECK_STRIDE: i64 = 32;
#[allow(clippy::too_many_arguments)]
fn march_projection(
    ctx: Ctx<'_>,
    scene: &Scene<'_>,
    o: Vec3,
    d: Vec3,
    t0: f64,
    t1: f64,
    step_len: f64,
    projection: &Projection,
    leap: Option<&VrSpaceLeap<'_>>,
    guard: &mut VrRayGuard,
) -> Option<f64> {
    let s = scene.sampler;
    let mut acc = 0.0f64;
    let mut count = 0i64;
    let mut sample_index = 0i64;
    let mode = projection.mode;
    if mode == VrMode::MIN_IP {
        acc = 1.0;
    }
    let (t0, t1) = clip_ray(
        scene.clip,
        o,
        d,
        scene.box_min,
        scene.size,
        t0,
        t1,
        step_len,
    )?;
    let (leap_a, leap_b) = match leap {
        Some(l) => l.ray_voxel(o, d, scene.box_min, scene.size),
        None => (Vec3::ZERO, Vec3::ZERO),
    };
    let mut t = t0;
    while t <= t1 {
        if !guard.take() {
            return None;
        }
        if sample_index % VR_CANCELLATION_CHECK_STRIDE == 0 && ctx_err(ctx).is_some() {
            return None;
        }
        sample_index += 1;
        let tex = scene.tex(o.add(d.scale(t)));
        if let Some(l) = leap {
            let block = l.block(tex);
            if block >= 0 && l.skips_projection_sample(block, mode, acc, count) {
                t = l.leap(block, leap_a, leap_b, t, t1, step_len);
                t += step_len;
                continue;
            }
        }
        if !clip_keep(scene.clip, tex) || !scene.labels.is_none_or(|l| l.keeps(tex)) {
            t += step_len;
            continue;
        }
        let hu = match texture_at(s, tex) {
            Some(hu) if projection.value_range.contains(hu) => hu,
            _ => {
                t += step_len;
                continue;
            }
        };
        match mode {
            VrMode::MIN_IP => {
                let dv = windowed_unit(hu, &projection.window);
                if count == 0 || dv < acc {
                    acc = dv;
                }
            }
            VrMode::AVERAGE => acc += windowed_unit(hu, &projection.window),
            VrMode::RAY_SUM => acc += hu * step_len,
            _ => {
                if projection.colored_mip {
                    if count == 0 || hu > acc {
                        acc = hu;
                    }
                } else {
                    let dv = windowed_unit(hu, &projection.window);
                    if count == 0 || dv > acc {
                        acc = dv;
                    }
                }
            }
        }
        count += 1;
        t += step_len;
    }
    if count == 0 {
        return None;
    }
    Some(projection.display(acc, count))
}
fn vr_cap_gray(value: f64, window: &WindowLevel) -> f64 {
    let (lower, upper) = vr_window_bounds(window);
    if !(upper > lower) {
        return 0.0;
    }
    srgb_to_linear(clamp_unit((value - lower) / (upper - lower)))
}
struct ShadowVolume {
    nx: i64,
    ny: i64,
    nz: i64,
    t: Vec<f32>,
}
impl ShadowVolume {
    fn at(&self, tex: Vec3) -> f64 {
        let fx = clamp_unit(tex.x) * (self.nx - 1) as f64;
        let fy = clamp_unit(tex.y) * (self.ny - 1) as f64;
        let fz = clamp_unit(tex.z) * (self.nz - 1) as f64;
        let x0 = fmath::f64_to_i64(fx).min(self.nx - 2);
        let y0 = fmath::f64_to_i64(fy).min(self.ny - 2);
        let z0 = fmath::f64_to_i64(fz).min(self.nz - 2);
        let (ax, ay, az) = (fx - x0 as f64, fy - y0 as f64, fz - z0 as f64);
        let v =
            |x: i64, y: i64, z: i64| f64::from(self.t[((z * self.ny + y) * self.nx + x) as usize]);
        let c00 = lerp(v(x0, y0, z0), v(x0 + 1, y0, z0), ax);
        let c10 = lerp(v(x0, y0 + 1, z0), v(x0 + 1, y0 + 1, z0), ax);
        let c01 = lerp(v(x0, y0, z0 + 1), v(x0 + 1, y0, z0 + 1), ax);
        let c11 = lerp(v(x0, y0 + 1, z0 + 1), v(x0 + 1, y0 + 1, z0 + 1), ax);
        lerp(lerp(c00, c10, ay), lerp(c01, c11, ay), az)
    }
}
struct ScatterFrame {
    settings: VrScattering,
    to_light: Vec3,
    shadow: ShadowVolume,
    gradient_scale: f64,
}
const MAX_VR_SHADOW_LATTICE: i64 = 64;
const MAX_VR_SHADOW_STEPS: f64 = 64.0;
fn vr_shadow_transmittance(
    scene: &Scene<'_>,
    origin: Vec3,
    to_light: Vec3,
    step: f64,
    reach: f64,
    unit: f64,
) -> f64 {
    let mut transmittance = 1.0f64;
    let mut distance = step;
    while distance <= reach {
        let tex = scene.tex(origin.add(to_light.scale(distance)));
        if tex.x < 0.0 || tex.y < 0.0 || tex.z < 0.0 || tex.x > 1.0 || tex.y > 1.0 || tex.z > 1.0 {
            break;
        }
        if clip_keep(scene.clip, tex)
            && scene.labels.is_none_or(|l| l.keeps(tex))
            && let Some(value) = texture_at(scene.sampler, tex)
        {
            let alpha = scene.transfer(value).a;
            transmittance *= 1.0 - correct_vr_alpha(alpha, 1.0, step, unit);
            if transmittance < 1.0 / 255.0 {
                return 0.0;
            }
        }
        distance += step;
    }
    transmittance
}
impl ScatterFrame {
    #[allow(clippy::too_many_arguments)]
    fn new(
        ctx: Ctx<'_>,
        scene: &Scene<'_>,
        shading: bool,
        quality: &VrQuality,
        cam: &VrCamera,
        step_len: f64,
    ) -> Result<Option<ScatterFrame>, RenderError> {
        let preset = scene.preset;
        let settings = preset.scattering.normalized();
        if !quality.scattering
            || !shading
            || preset.mode != VrMode::DVR
            || !settings.enabled()
            || preset.transfer_2d.is_some()
        {
            return Ok(None);
        }
        let (right, up, forward) = cam.basis();
        let to_light = vr_scattering_light(
            forward,
            right,
            up,
            settings.light_azimuth,
            settings.light_elevation,
        );
        let (mut lower, mut upper) = (scene.data_min, scene.data_max);
        if preset.tf.domain() == VrTransferDomain::Normalized {
            (lower, upper) = vr_window_bounds(scene.window);
        }
        let gradient_scale = if upper > lower {
            0.5 / (upper - lower)
        } else {
            0.0
        };
        let vol = &scene.sampler.vol;
        let lattice = |n: i64| n.clamp(2, MAX_VR_SHADOW_LATTICE);
        let (nx, ny, nz) = (lattice(vol.cols), lattice(vol.rows), lattice(vol.depth));
        let cells: Vec<AtomicU32> = (0..nx * ny * nz).map(|_| AtomicU32::new(0)).collect();
        let size = scene.size;
        let reach = settings.reach * size.length();
        let step = fmath::max(step_len, reach / MAX_VR_SHADOW_STEPS);
        let unit = preset.tf.opacity_unit_distance_mm();
        let cells_ref = &cells;
        parallel_rows_worker_context(ctx, nz, || {
            move |z: i64| {
                for y in 0..ny {
                    for x in 0..nx {
                        let tex = Vec3::new(
                            x as f64 / (nx - 1) as f64,
                            y as f64 / (ny - 1) as f64,
                            z as f64 / (nz - 1) as f64,
                        );
                        let origin = scene.box_min.add(Vec3::new(
                            tex.x * size.x,
                            tex.y * size.y,
                            tex.z * size.z,
                        ));
                        let value =
                            vr_shadow_transmittance(scene, origin, to_light, step, reach, unit);
                        cells_ref[((z * ny + y) * nx + x) as usize]
                            .store((value as f32).to_bits(), Ordering::Relaxed);
                    }
                }
            }
        })?;
        Ok(Some(ScatterFrame {
            settings,
            to_light,
            shadow: ShadowVolume {
                nx,
                ny,
                nz,
                t: cells
                    .into_iter()
                    .map(|c| f32::from_bits(c.into_inner()))
                    .collect(),
            },
            gradient_scale,
        }))
    }
    #[allow(clippy::too_many_arguments)]
    fn shade(
        &self,
        surface: (f64, f64, f64),
        color: (f64, f64, f64),
        alpha: f64,
        tex: Vec3,
        ray_dir: Vec3,
        index_gradient: Vec3,
        diffuse: f64,
    ) -> (f64, f64, f64) {
        let secondary = self.shadow.at(tex)
            * henyey_greenstein(
                self.settings.anisotropy,
                self.to_light.dot(ray_dir.normalize()),
            )
            * diffuse;
        let coef = vr_scattering_coefficient(
            self.settings.blend,
            index_gradient.length() * self.gradient_scale,
            alpha,
        );
        let keep = 1.0 - coef;
        (
            keep * surface.0 + coef * secondary * color.0,
            keep * surface.1 + coef * secondary * color.1,
            keep * surface.2 + coef * secondary * color.2,
        )
    }
}
#[allow(clippy::too_many_arguments)]
fn march_dvr(
    ctx: Ctx<'_>,
    scene: &Scene<'_>,
    cell: &mut GradientCell,
    o: Vec3,
    d: Vec3,
    t0: f64,
    t1: f64,
    step_len: f64,
    shading: bool,
    leap: Option<&VrSpaceLeap<'_>>,
    scatter: Option<&ScatterFrame>,
    mut stepper: Option<&mut VrAdaptiveStepper>,
    mut fusion: Option<&mut FusionWorker<'_, '_>>,
    fragments: &[VrFragment],
    guard: &mut VrRayGuard,
) -> (f64, f64, f64, f64) {
    let mut fragments = fragments;
    let s = scene.sampler;
    let preset = scene.preset;
    let clip = scene.clip;
    let transfer_2d = preset.transfer_2d.as_deref();
    let gradient_opacity = transfer_2d.is_none() && uses_gradient_opacity(preset);
    let need_gradient = shading || gradient_opacity || transfer_2d.is_some();
    let use_gradient_cell = gradient_opacity || transfer_2d.is_some();
    let (mut acc_r, mut acc_g, mut acc_b, mut acc_a) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
    let Some((t0, t1)) = clip_ray(clip, o, d, scene.box_min, scene.size, t0, t1, step_len) else {
        return composite_fragments_before(
            &mut fragments,
            f64::INFINITY,
            (acc_r, acc_g, acc_b, acc_a),
        );
    };
    let leap = leap.filter(|l| l.empty.is_some());
    let (leap_a, leap_b) = match leap {
        Some(l) => l.ray_voxel(o, d, scene.box_min, scene.size),
        None => (Vec3::ZERO, Vec3::ZERO),
    };
    let cut_gradients = need_gradient && clip_has_cut_planes(clip);
    let mut clipped_value = scene.data_min;
    if let Some(t2d) = transfer_2d {
        clipped_value = t2d.intensity_range().0;
    }
    let cap_opacity = clip.map_or(0.0, |c| c.cap_opacity());
    let mut cap_pending = cap_opacity > 0.0;
    let mut sample_index = 0i64;
    if let Some(st) = stepper.as_deref_mut() {
        st.begin_ray();
    }
    let mut step = step_len;
    let mut t = t0;
    while t <= t1 {
        if !guard.take() {
            return (0.0, 0.0, 0.0, 0.0);
        }
        if sample_index % VR_CANCELLATION_CHECK_STRIDE == 0 && ctx_err(ctx).is_some() {
            return (acc_r, acc_g, acc_b, acc_a);
        }
        sample_index += 1;
        let tex = scene.tex(o.add(d.scale(t)));
        if cap_pending && clip_keep(clip, tex) {
            cap_pending = false;
            let previous = scene.tex(o.add(d.scale(t - step_len)));
            if let Some(c) = clip
                && c.caps_at(previous)
                && let Some(value) = texture_at(s, tex)
            {
                let gray = vr_cap_gray(value, &c.cap_window_or(scene.window));
                let alpha = cap_opacity;
                (acc_r, acc_g, acc_b, acc_a) =
                    composite_fragments_before(&mut fragments, t, (acc_r, acc_g, acc_b, acc_a));
                let inv = 1.0 - acc_a;
                acc_r += inv * alpha * gray;
                acc_g += inv * alpha * gray;
                acc_b += inv * alpha * gray;
                acc_a += inv * alpha;
                if acc_a > VR_OPACITY_THRESHOLD {
                    break;
                }
            }
        }
        if let Some(l) = leap
            && !cap_pending
        {
            let block = l.block(tex);
            if block >= 0 && l.empty.as_ref().is_some_and(|e| e[block as usize]) {
                let last = l.leap(block, leap_a, leap_b, t, t1, step);
                if let Some(worker) = fusion.as_deref_mut()
                    && worker.in_bounds == 0
                {
                    worker.probe_skipped(
                        ctx,
                        o,
                        d,
                        scene.box_min,
                        scene.size,
                        t,
                        last,
                        step,
                        clip,
                        scene.labels,
                    );
                }
                t = last;
                if let Some(st) = stepper.as_deref_mut() {
                    st.reset();
                }
                step = step_len;
                t += step;
                continue;
            }
        }
        if !clip_keep(clip, tex) {
            if let Some(st) = stepper.as_deref_mut() {
                st.reset();
            }
            step = step_len;
            t += step;
            continue;
        }
        let mut label_style = None;
        if let Some(labels) = scene.labels {
            label_style = labels.visible_style(labels.label_at_tex(tex));
            if label_style.is_none() && labels.only_inside() {
                t += step;
                continue;
            }
        }
        let label_opacity = label_style.is_some_and(|st| st.blend > 0.0 && st.opacity > 0.0);
        let mut cell_sample = TextureSample::default();
        let sampled = if use_gradient_cell {
            texture_sample(s, tex).map(|sample| {
                cell_sample = sample;
                sample.value()
            })
        } else {
            texture_at(s, tex)
        };
        let Some(hu) = sampled else {
            if let Some(st) = stepper.as_deref_mut() {
                st.reset();
            }
            step = step_len;
            t += step;
            continue;
        };
        let mut rgba = Rgba::default();
        let mut index_gradient = Vec3::ZERO;
        let mut have_gradient = false;
        if let Some(t2d) = transfer_2d {
            if t2d.max_alpha_at(hu) > 0.0 {
                index_gradient = dvr_index_gradient(
                    s,
                    Some(cell),
                    &cell_sample,
                    tex,
                    clip,
                    cut_gradients,
                    use_gradient_cell,
                    clipped_value,
                );
                have_gradient = true;
                rgba = t2d.lookup(hu, s.vol.physical_gradient_magnitude(index_gradient));
            }
        } else {
            rgba = scene.transfer(hu);
        }
        let mut sample_len = step_len;
        if let Some(st) = stepper.as_deref_mut() {
            let (accept, length, next, resume) = st.observe(t, rgba);
            step = next;
            if !accept {
                t = resume;
                t += step;
                continue;
            }
            sample_len = length;
        }
        let mut alpha = rgba.a;
        let mut secondary = Rgba::default();
        if let Some(worker) = fusion.as_deref_mut() {
            secondary = worker.sample(tex, step_len);
        }
        if alpha <= 0.0 && !label_opacity {
            if secondary.a <= 0.0 {
                t += step;
                continue;
            }
            (acc_r, acc_g, acc_b, acc_a) =
                composite_fragments_before(&mut fragments, t, (acc_r, acc_g, acc_b, acc_a));
            let inv = 1.0 - acc_a;
            acc_r += inv * secondary.a * secondary.r;
            acc_g += inv * secondary.a * secondary.g;
            acc_b += inv * secondary.a * secondary.b;
            acc_a += inv * secondary.a;
            if acc_a > VR_OPACITY_THRESHOLD {
                break;
            }
            t += step;
            continue;
        }
        let (mut cr, mut cg, mut cb) = (rgba.r, rgba.g, rgba.b);
        if need_gradient {
            if !have_gradient
                && (!preset.opacity_normals
                    || gradient_opacity
                    || transfer_2d.is_some()
                    || scatter.is_some())
            {
                index_gradient = dvr_index_gradient(
                    s,
                    Some(cell),
                    &cell_sample,
                    tex,
                    clip,
                    cut_gradients,
                    use_gradient_cell,
                    clipped_value,
                );
            }
            if gradient_opacity {
                alpha *= scene.gradient_opacity_factor(index_gradient);
            }
            if let Some(style) = label_style.take() {
                (cr, cg, cb, alpha) = mix_vr_label(&style, cr, cg, cb, alpha);
            }
            if shading {
                let material = preset.effective_lighting_material();
                let mut shading_gradient = index_gradient;
                if preset.opacity_normals {
                    shading_gradient = scene.opacity_gradient(tex, clipped_value);
                }
                let (lit, specular) = s.vol.shade_vr_with_gradient(shading_gradient, d, &material);
                let mut shaded = (
                    cr * lit + specular,
                    cg * lit + specular,
                    cb * lit + specular,
                );
                if let Some(frame) = scatter {
                    shaded = frame.shade(
                        shaded,
                        (cr, cg, cb),
                        alpha,
                        tex,
                        d,
                        index_gradient,
                        material.diffuse,
                    );
                }
                (cr, cg, cb) = shaded;
            }
        }
        if let Some(style) = label_style.take() {
            (cr, cg, cb, alpha) = mix_vr_label(&style, cr, cg, cb, alpha);
        }
        alpha = correct_vr_alpha(alpha, 1.0, sample_len, preset.tf.opacity_unit_distance_mm());
        if secondary.a > 0.0 {
            (cr, cg, cb, alpha) = mix_vr_fusion_sample(cr, cg, cb, alpha, secondary);
        }
        if alpha <= 0.0 {
            t += step;
            continue;
        }
        (acc_r, acc_g, acc_b, acc_a) =
            composite_fragments_before(&mut fragments, t, (acc_r, acc_g, acc_b, acc_a));
        let inv = 1.0 - acc_a;
        acc_r += inv * alpha * cr;
        acc_g += inv * alpha * cg;
        acc_b += inv * alpha * cb;
        acc_a += inv * alpha;
        if acc_a > VR_OPACITY_THRESHOLD {
            break;
        }
        t += step;
    }
    composite_fragments_before(&mut fragments, f64::INFINITY, (acc_r, acc_g, acc_b, acc_a))
}
fn vr_transfer_setup(
    prepared: &VrPreparedVolume,
    preset: &VrPreset,
    window: &WindowLevel,
) -> (VrLut, WindowLevel, f64, f64) {
    let (mut data_min, mut data_max) = (0.0, 1.0);
    if (preset.mode == VrMode::DVR || preset.mode == VrMode::MIP && preset.colored_mip)
        && preset.tf.domain() == VrTransferDomain::Hu
        && let Some((minimum, maximum)) = prepared.hu_range()
    {
        (data_min, data_max) = (minimum, maximum);
    }
    let effective = normalize_window(window, &prepared.volume.recommended_window());
    let lut = if preset.tf.domain() == VrTransferDomain::Hu {
        preset
            .tf
            .bake_lut(data_min, data_max, preset.tf.preferred_lut_size())
    } else {
        preset
            .tf
            .bake_window_lut(&effective, preset.tf.preferred_lut_size())
    };
    (lut, effective, data_min, data_max)
}
fn new_render_leap<'a>(
    ctx: Ctx<'_>,
    prepared: &'a VrPreparedVolume,
    scene: &Scene<'_>,
) -> Result<Option<VrSpaceLeap<'a>>, RenderError> {
    let preset = scene.preset;
    if preset.mode == VrMode::MIP && preset.colored_mip {
        return Ok(None);
    }
    if preset.mode != VrMode::DVR
        && preset.mode != VrMode::MIP
        && preset.mode != VrMode::MIN_IP
        && !preset.uses_value_range()
    {
        return Ok(None);
    }
    let grid = prepared.block_grid(ctx, scene.sampler)?;
    let mut leap = VrSpaceLeap::new(grid, scene.sampler);
    let cut_gradients = clip_has_cut_planes(scene.clip);
    let mut gradients = None;
    if preset.mode == VrMode::DVR
        && !cut_gradients
        && (preset.transfer_2d.is_some() || uses_gradient_opacity(preset))
    {
        gradients = Some(grid.physical_gradient_bounds(ctx, scene.sampler)?);
    }
    if preset.mode == VrMode::DVR {
        if let Some(t2d) = &preset.transfer_2d {
            leap.for_dvr_2d(t2d, gradients.as_deref());
        } else {
            leap.for_dvr(
                &preset.tf,
                scene.lut,
                scene.window,
                scene.data_min,
                scene.data_max,
            );
            leap.limit_gradient_opacity(preset, gradients.as_deref());
        }
    } else if preset.mode == VrMode::MIP || preset.mode == VrMode::MIN_IP {
        leap.for_projection(scene.window);
    } else {
        leap.for_value_range(preset.projection_range);
    }
    if preset.mode == VrMode::DVR {
        leap.limit_labels(scene.labels);
    }
    Ok(Some(leap))
}
pub(crate) fn viewport_size(quality: &VrQuality) -> (usize, usize) {
    if quality.width <= 0 || quality.height <= 0 {
        return (256, 256);
    }
    (quality.width as usize, quality.height as usize)
}
fn srgb_pixel(c: Rgba) -> [u8; 4] {
    [
        to8(linear_to_srgb(c.r)),
        to8(linear_to_srgb(c.g)),
        to8(linear_to_srgb(c.b)),
        255,
    ]
}
fn viewport_image(quality: &VrQuality, background: Rgba) -> RenderImage {
    let (w, h) = viewport_size(quality);
    let pixel = srgb_pixel(background);
    let mut pixels = Vec::with_capacity(w * h * 4);
    for _ in 0..w * h {
        pixels.extend_from_slice(&pixel);
    }
    RenderImage::Rgba {
        width: w,
        height: h,
        pixels,
    }
}
pub(crate) struct RenderCancel<'a> {
    parent: Ctx<'a>,
    aborted: AtomicBool,
}
impl<'a> RenderCancel<'a> {
    pub(crate) fn new(parent: Ctx<'a>) -> RenderCancel<'a> {
        RenderCancel {
            parent,
            aborted: AtomicBool::new(false),
        }
    }
    pub(crate) fn abort(&self) {
        self.aborted.store(true, Ordering::Relaxed);
    }
}
impl Cancellation for RenderCancel<'_> {
    fn err(&self) -> Option<RenderError> {
        if let Some(e) = ctx_err(self.parent) {
            return Some(e);
        }
        if self.aborted.load(Ordering::Relaxed) {
            return Some(RenderError::new(
                super::error::ErrorKind::Canceled,
                "context canceled",
            ));
        }
        None
    }
}
pub(crate) fn volume_vr_ray_sampling(
    vol: &Volume,
    quality: &VrQuality,
) -> Result<VrRaySamplingPlan, VrSamplingError> {
    resolve_vr_ray_sampling(
        [vol.col_spacing, vol.row_spacing, vol.slice_spacing],
        vol.physical_size().length(),
        quality,
    )
}
pub fn render_vr_prepared(
    ctx: Ctx<'_>,
    prepared: Option<&VrPreparedVolume>,
    settings: &VrRenderSettings<'_>,
) -> (Option<RenderImage>, Option<VrRenderError>) {
    let s = settings;
    render_vr_prepared_inner(
        ctx, prepared, s.camera, s.preset, s.window, s.shading, s.clip, s.quality, None,
    )
}
#[allow(clippy::too_many_arguments)]
pub(crate) fn render_vr_prepared_inner(
    ctx: Ctx<'_>,
    prepared: Option<&VrPreparedVolume>,
    cam: &VrCamera,
    preset: &VrPreset,
    window: &WindowLevel,
    shading: bool,
    clip: Option<&VrClip>,
    quality: &VrQuality,
    fusion: Option<&FusionFrame<'_>>,
) -> (Option<RenderImage>, Option<VrRenderError>) {
    let (w, h) = viewport_size(quality);
    let background = preset.effective_background();
    let blank = || {
        Some(viewport_image(
            &VrQuality {
                width: w as i64,
                height: h as i64,
                ..Default::default()
            },
            background,
        ))
    };
    let Some(prepared) = prepared else {
        return (blank(), None);
    };
    if let Some(e) = ctx_err(ctx) {
        return (blank(), Some(e.into()));
    }
    if !preset.projection_range.valid() {
        return (blank(), Some(VrRenderError::InvalidValueRange));
    }
    let vol = &prepared.volume;
    let size = vol.physical_size();
    if size.x <= 0.0 || size.y <= 0.0 || size.z <= 0.0 {
        return (blank(), None);
    }
    let labels = clip.and_then(|c| c.active_labels(vol));
    let mut sampling_quality = *quality;
    if preset.mode != VrMode::DVR || fusion.is_some() || labels.is_some() {
        sampling_quality.adaptive_step = VrAdaptiveStep::default();
    }
    let plan = match volume_vr_ray_sampling(vol, &sampling_quality) {
        Ok(plan) => plan,
        Err(e) => return (None, Some(VrRenderError::Sampling(e))),
    };
    let cancel = RenderCancel {
        parent: ctx,
        aborted: AtomicBool::new(false),
    };
    let rctx: Ctx<'_> = Some(&cancel);
    let sampler = match prepared.new_sampler() {
        Ok(s) => s,
        Err(e) => return (blank(), Some(e.into())),
    };
    let box_min = size.scale(-0.5);
    let box_max = size.scale(0.5);
    let (lut, window, data_min, data_max) = vr_transfer_setup(prepared, preset, window);
    let diag = size.length();
    let step_len = plan.sample_distance_mm;
    let ray_gen = RayGenerator::new(cam, w as i64, h as i64);
    let projection = Projection::new(preset, &window, diag);
    let scene = Scene {
        sampler: &sampler,
        lut: &lut,
        preset,
        window: &window,
        data_min,
        data_max,
        clip,
        labels,
        box_min,
        size,
    };
    let mut leap = None;
    if clip.is_none_or(|c| !c.full_rays) && fusion.is_none_or(|f| f.mapper.is_none()) {
        leap = match new_render_leap(rctx, prepared, &scene) {
            Ok(l) => l,
            Err(e) => return (blank(), Some(e.into())),
        };
        if let (Some(l), Some(f)) = (leap.as_mut(), fusion)
            && let Err(e) = f.limit_leap(rctx, l, &sampler)
        {
            return (blank(), Some(e.into()));
        }
    }
    let scatter = match ScatterFrame::new(rctx, &scene, shading, quality, cam, step_len) {
        Ok(s) => s,
        Err(e) => return (blank(), Some(e.into())),
    };
    let geometry = match clip_geometry_depth(rctx, clip, cam, w as i64, h as i64) {
        Ok(g) => g,
        Err(e) => return (blank(), Some(e.into())),
    };
    let geometry_ref = geometry.as_ref();
    let adaptive = if labels.is_some() || fusion.is_some() {
        VrAdaptiveStep::default()
    } else {
        quality.adaptive_step
    };
    let background_pixel = srgb_pixel(background);
    let rows: Vec<OnceLock<Vec<u8>>> = (0..h).map(|_| OnceLock::new()).collect();
    let budget_error: Mutex<Option<VrSamplingError>> = Mutex::new(None);
    let fail = |e: Option<VrSamplingError>| {
        if let Ok(mut slot) = budget_error.lock()
            && slot.is_none()
        {
            *slot = e;
        }
        cancel.aborted.store(true, Ordering::Relaxed);
    };
    let (leap_ref, scatter_ref, scene_ref) = (leap.as_ref(), scatter.as_ref(), &scene);
    let sampler_vol = sampler.vol.clone();
    let (rows_ref, ray_gen_ref, projection_ref, fail_ref) = (&rows, &ray_gen, &projection, &fail);
    let outcome = parallel_rows_worker_context(rctx, h as i64, || {
        let mut cell = GradientCell::default();
        let mut stepper = adaptive
            .enabled()
            .then(|| VrAdaptiveStepper::new(adaptive, step_len));
        let mut fusion_worker = fusion.map(|f| f.worker(&sampler_vol));
        move |py: i64| {
            let mut out = Vec::with_capacity(w * 4);
            for _ in 0..w {
                out.extend_from_slice(&background_pixel);
            }
            let mut set =
                |px: usize, pixel: [u8; 4]| out[px * 4..px * 4 + 4].copy_from_slice(&pixel);
            let row_base = ray_gen_ref.row_base(py);
            let mut complete = true;
            for px in 0..w {
                if ctx_err(rctx).is_some() {
                    complete = false;
                    break;
                }
                let (o, d) = ray_gen_ref.ray(px as i64, &row_base);
                let hit = intersect_aabb(o, d, box_min, box_max);
                let (mut t0, mut t1) = hit.unwrap_or((0.0, 0.0));
                if quality.jitter {
                    t0 += vr_ray_jitter(px as i64, py) * step_len;
                }
                let fragments = depth_fragments(geometry_ref, px as i64, py);
                let mut pixel_background = background;
                if let Some((t_geometry, geometry_color)) = depth_at(geometry_ref, px as i64, py) {
                    pixel_background = geometry_color;
                    t1 = fmath::min(t1, t_geometry);
                    if hit.is_none() || t0 > t1 {
                        set(
                            px,
                            srgb_pixel(composite_vr_fragments(&fragments, geometry_color)),
                        );
                        continue;
                    }
                }
                let mut guard = VrRayGuard {
                    plan,
                    ..Default::default()
                };
                if preset.mode != VrMode::DVR {
                    let mut value = None;
                    if hit.is_some() {
                        value = march_projection(
                            rctx,
                            scene_ref,
                            o,
                            d,
                            t0,
                            t1,
                            step_len,
                            projection_ref,
                            leap_ref,
                            &mut guard,
                        );
                    }
                    if guard.err.is_some() {
                        fail_ref(guard.err.take());
                        complete = false;
                        break;
                    }
                    let Some(mut v) = value else {
                        if pixel_background != background || !fragments.is_empty() {
                            set(
                                px,
                                srgb_pixel(composite_vr_fragments(&fragments, pixel_background)),
                            );
                        }
                        continue;
                    };
                    if projection_ref.colored_mip {
                        let mapped = vr_mip_color(scene_ref.lut, v, preset.inverse);
                        set(px, srgb_pixel(composite_vr_fragments(&fragments, mapped)));
                        continue;
                    }
                    if preset.inverse {
                        v = 1.0 - v;
                    }
                    if !fragments.is_empty() {
                        let gray = srgb_to_linear(clamp_unit(v));
                        let base = Rgba {
                            r: gray,
                            g: gray,
                            b: gray,
                            a: 1.0,
                        };
                        set(px, srgb_pixel(composite_vr_fragments(&fragments, base)));
                        continue;
                    }
                    let gy = to8(v);
                    set(px, [gy, gy, gy, 255]);
                    continue;
                }
                if hit.is_none() {
                    if !fragments.is_empty() {
                        set(
                            px,
                            srgb_pixel(composite_vr_fragments(&fragments, background)),
                        );
                    }
                    continue;
                }
                let (mut r, mut g, mut b, a) = march_dvr(
                    rctx,
                    scene_ref,
                    &mut cell,
                    o,
                    d,
                    t0,
                    t1,
                    step_len,
                    shading,
                    leap_ref,
                    scatter_ref,
                    stepper.as_mut(),
                    fusion_worker.as_mut(),
                    &fragments,
                    &mut guard,
                );
                if guard.err.is_some() {
                    fail_ref(guard.err.take());
                    complete = false;
                    break;
                }
                r += (1.0 - a) * pixel_background.r;
                g += (1.0 - a) * pixel_background.g;
                b += (1.0 - a) * pixel_background.b;
                set(
                    px,
                    [
                        to8(linear_to_srgb(r)),
                        to8(linear_to_srgb(g)),
                        to8(linear_to_srgb(b)),
                        255,
                    ],
                );
            }
            let _ = complete;
            let _ = rows_ref[py as usize].set(out);
        }
    });
    if let Ok(mut slot) = budget_error.lock()
        && let Some(e) = slot.take()
    {
        return (None, Some(VrRenderError::Sampling(e)));
    }
    let mut pixels = Vec::with_capacity(w * h * 4);
    for row in rows {
        match row.into_inner() {
            Some(row) => pixels.extend_from_slice(&row),
            None => {
                for _ in 0..w {
                    pixels.extend_from_slice(&background_pixel);
                }
            }
        }
    }
    let image = RenderImage::Rgba {
        width: w,
        height: h,
        pixels,
    };
    (Some(image), outcome.err().map(VrRenderError::Render))
}
pub fn render_vr(
    ctx: Ctx<'_>,
    vol: Option<&Arc<Volume>>,
    settings: &VrRenderSettings<'_>,
) -> (Option<RenderImage>, Option<VrRenderError>) {
    let background = settings.preset.effective_background();
    let Some(vol) = vol else {
        return (Some(viewport_image(settings.quality, background)), None);
    };
    match vol.prepare_vr_volume(ctx, settings.preset.prefilter) {
        Err(e) => (
            Some(viewport_image(settings.quality, background)),
            Some(e.into()),
        ),
        Ok(prepared) => render_vr_prepared(ctx, Some(&prepared), settings),
    }
}
pub fn render_vr_preview(
    ctx: Ctx<'_>,
    vol: Option<&Arc<Volume>>,
    cam: &VrCamera,
    preset: &VrPreset,
    window: &WindowLevel,
    clip: Option<&VrClip>,
    quality: &VrQuality,
) -> (Option<RenderImage>, Option<VrRenderError>) {
    let mut preset = preset.clone();
    preset.gradient_opacity_scale = 0.0;
    preset.gradient_opacity = VrGradientOpacity::default();
    let settings = VrRenderSettings {
        camera: cam,
        preset: &preset,
        window,
        shading: false,
        clip,
        quality,
    };
    render_vr(ctx, vol, &settings)
}
pub fn render_vr_prepared_preview(
    ctx: Ctx<'_>,
    prepared: Option<&VrPreparedVolume>,
    cam: &VrCamera,
    preset: &VrPreset,
    window: &WindowLevel,
    clip: Option<&VrClip>,
    quality: &VrQuality,
) -> (Option<RenderImage>, Option<VrRenderError>) {
    let mut preset = preset.clone();
    preset.gradient_opacity_scale = 0.0;
    preset.gradient_opacity = VrGradientOpacity::default();
    let settings = VrRenderSettings {
        camera: cam,
        preset: &preset,
        window,
        shading: false,
        clip,
        quality,
    };
    render_vr_prepared(ctx, prepared, &settings)
}
