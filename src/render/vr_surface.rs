#![forbid(unsafe_code)]
use super::frame::RenderImage;
use super::parallel::{Cancellation, ctx_err, parallel_rows_worker_context};
use super::vec3::{Vec3, WorldPoint};
use super::volume::{Sampler, Volume};
use super::vr_camera::VrCamera;
use super::vr_clip::{VrClip, clip_keep};
use super::vr_geometry::{
    VrFragment, clip_geometry_depth, composite_vr_fragments, depth_at, depth_fragments,
};
use super::vr_prepared::VrPreparedVolume;
use super::vr_quality::{VrAdaptiveStep, VrQuality, VrRayGuard, VrSamplingError};
use super::vr_raycast::{
    RayGenerator, RenderCancel, VR_CANCELLATION_CHECK_STRIDE, VrRenderError,
    clipped_gradient_index, intersect_aabb, normalized_vr_data_coordinate, render_vr, texture_at,
    to8, viewport_size, volume_vr_ray_sampling,
};
use super::vr_settings::{VrRenderSettings, VrSurfaceSettings};
use super::vr_space_leap::{VrBlockGrid, VrSpaceLeap, widen};
use super::vr_transfer::{
    Rgba, VrLightingMaterial, VrPrefilterId, VrTransferDomain, correct_vr_alpha, linear_to_srgb,
    srgb_to_linear,
};
use super::vr_volume::headlight_shading;
use super::window::{normalize_window, windowed_unit};
use crate::fmath;
use std::sync::{Arc, Mutex, OnceLock};
type Ctx<'a> = Option<&'a dyn Cancellation>;
pub mod iso_layers;
const VR_BISECTION_STEPS: usize = 4;
fn bisect_vr_ray(mut before: f64, mut hit: f64, mut inside: impl FnMut(f64) -> bool) -> f64 {
    for _ in 0..VR_BISECTION_STEPS {
        let mid = before + (hit - before) / 2.0;
        if inside(mid) {
            hit = mid;
        } else {
            before = mid;
        }
    }
    hit
}
fn skips_vr_surface_block(grid: &VrBlockGrid, block: i64, iso: f64) -> bool {
    let (lo, hi) = (
        f64::from(grid.min[block as usize]),
        f64::from(grid.max[block as usize]),
    );
    if lo.is_nan() || hi.is_nan() {
        return false;
    }
    let (_, hi) = widen(lo, hi);
    hi < iso
}
#[allow(clippy::too_many_arguments)]
fn advance_vr_surface_leap(
    ctx: Ctx<'_>,
    leap: &VrSpaceLeap<'_>,
    block: i64,
    a: Vec3,
    b: Vec3,
    mut t: f64,
    t1: f64,
    step: f64,
    guard: &mut VrRayGuard,
    sample_index: &mut i64,
) -> Option<f64> {
    let grid = leap.grid;
    let bx = block % grid.nx;
    let by = (block / grid.nx) % grid.ny;
    let bz = block / (grid.nx * grid.ny);
    let (mut t_in, mut t_out) = (f64::NEG_INFINITY, f64::INFINITY);
    for axis in [
        [a.x, b.x, bx as f64],
        [a.y, b.y, by as f64],
        [a.z, b.z, bz as f64],
    ] {
        let lower = axis[2] * 8.0 + 1e-6;
        let upper = (axis[2] + 1.0) * 8.0 - 1e-6;
        if axis[1] == 0.0 {
            if axis[0] < lower || axis[0] > upper {
                return Some(t);
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
        return Some(t);
    }
    loop {
        let next = t + step;
        if next <= t || next >= t_out || next > t1 {
            return Some(t);
        }
        if !guard.take() {
            return None;
        }
        if *sample_index % VR_CANCELLATION_CHECK_STRIDE == 0 && ctx_err(ctx).is_some() {
            return None;
        }
        *sample_index += 1;
        t = next;
    }
}
#[allow(clippy::too_many_arguments)]
fn first_vr_surface_hit(
    ctx: Ctx<'_>,
    sampler: &Sampler,
    leap: Option<&VrSpaceLeap<'_>>,
    origin: Vec3,
    direction: Vec3,
    t0: f64,
    t1: f64,
    step_len: f64,
    box_min: Vec3,
    size: Vec3,
    iso: f64,
    clip: Option<&VrClip>,
    guard: &mut VrRayGuard,
) -> Option<Vec3> {
    if step_len <= 0.0 {
        return None;
    }
    let tex_at = |t: f64| {
        let point = origin.add(direction.scale(t));
        Vec3::new(
            (point.x - box_min.x) / size.x,
            (point.y - box_min.y) / size.y,
            (point.z - box_min.z) / size.z,
        )
    };
    let solid = |t: f64| {
        let tex = tex_at(t);
        if !clip_keep(clip, tex) {
            return false;
        }
        texture_at(sampler, tex).is_some_and(|value| value >= iso)
    };
    let mut sample_index = 0i64;
    let (leap_a, leap_b) = match leap {
        Some(l) => l.ray_voxel(origin, direction, box_min, size),
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
        if let Some(l) = leap {
            let block = l.block(tex_at(t));
            if block >= 0 && skips_vr_surface_block(l.grid, block, iso) {
                t = advance_vr_surface_leap(
                    ctx,
                    l,
                    block,
                    leap_a,
                    leap_b,
                    t,
                    t1,
                    step_len,
                    guard,
                    &mut sample_index,
                )?;
                t += step_len;
                continue;
            }
        }
        if solid(t) {
            if sample_index > 1 {
                t = bisect_vr_ray(t - step_len, t, solid);
            }
            return Some(tex_at(t));
        }
        t += step_len;
    }
    None
}
pub(crate) fn gradient_at(s: &Sampler, tex: Vec3) -> Vec3 {
    let dx = 1.0 / s.cols.max(2) as f64;
    let dy = 1.0 / s.rows.max(2) as f64;
    let dz = 1.0 / s.depth.max(2) as f64;
    let clamped = |x: f64, y: f64, z: f64| {
        let unit = |v: f64| fmath::max(0.0, fmath::min(1.0, v));
        texture_at(s, Vec3::new(unit(x), unit(y), unit(z))).unwrap_or(0.0)
    };
    let hx1 = clamped(tex.x + dx, tex.y, tex.z);
    let hx0 = clamped(tex.x - dx, tex.y, tex.z);
    let hy1 = clamped(tex.x, tex.y + dy, tex.z);
    let hy0 = clamped(tex.x, tex.y - dy, tex.z);
    let hz1 = clamped(tex.x, tex.y, tex.z + dz);
    let hz0 = clamped(tex.x, tex.y, tex.z - dz);
    Vec3::new(hx1 - hx0, hy1 - hy0, hz1 - hz0)
}
fn shade_with_gradient(grad: Vec3, view_dir: Vec3) -> (f64, f64) {
    let material = VrLightingMaterial::legacy();
    let g = grad.length();
    if g < 1e-6 {
        return (material.ambient, 0.0);
    }
    let n = grad.scale(-1.0 / g);
    headlight_shading(n, view_dir.scale(-1.0).normalize(), &material)
}
fn surface_image(quality: &VrQuality) -> RenderImage {
    let (w, h) = viewport_size(quality);
    RenderImage::Rgba {
        width: w,
        height: h,
        pixels: vec![0u8; w * h * 4],
    }
}
pub fn render_vr_surface(
    ctx: Ctx<'_>,
    vol: Option<&Arc<Volume>>,
    settings: &VrSurfaceSettings<'_>,
) -> (Option<RenderImage>, Option<VrRenderError>) {
    let quality = settings.quality;
    let Some(vol) = vol else {
        return (Some(surface_image(quality)), None);
    };
    if let Some(e) = ctx_err(ctx) {
        return (Some(surface_image(quality)), Some(e.into()));
    }
    match vol.prepare_vr_volume(ctx, VrPrefilterId::NONE) {
        Err(e) => (Some(surface_image(quality)), Some(e.into())),
        Ok(prepared) => render_vr_surface_prepared(ctx, Some(&prepared), settings),
    }
}
pub fn render_vr_surface_prepared(
    ctx: Ctx<'_>,
    prepared: Option<&VrPreparedVolume>,
    settings: &VrSurfaceSettings<'_>,
) -> (Option<RenderImage>, Option<VrRenderError>) {
    let VrSurfaceSettings {
        camera: cam,
        iso,
        color: surface_color,
        shading,
        clip,
        quality,
    } = *settings;
    let (w, h) = viewport_size(quality);
    let blank = || Some(surface_image(quality));
    let Some(prepared) = prepared else {
        return (blank(), None);
    };
    if let Some(e) = ctx_err(ctx) {
        return (blank(), Some(e.into()));
    }
    let vol = &prepared.volume;
    let size = vol.physical_size();
    if size.x <= 0.0 || size.y <= 0.0 || size.z <= 0.0 {
        return (blank(), None);
    }
    let mut sampling_quality = *quality;
    sampling_quality.adaptive_step = VrAdaptiveStep::default();
    let plan = match volume_vr_ray_sampling(vol, &sampling_quality) {
        Ok(plan) => plan,
        Err(e) => return (None, Some(VrRenderError::Sampling(e))),
    };
    let cancel = RenderCancel::new(ctx);
    let rctx: Ctx<'_> = Some(&cancel);
    let sampler = match prepared.new_sampler() {
        Ok(s) => s,
        Err(e) => return (blank(), Some(e.into())),
    };
    let mut leap = None;
    if clip.is_none_or(|c| !c.full_rays) {
        if let Some(e) = ctx_err(rctx) {
            return (blank(), Some(e.into()));
        }
        let grid = match prepared.block_grid(rctx, &sampler) {
            Ok(g) => g,
            Err(e) => return (blank(), Some(e.into())),
        };
        if let Some(e) = ctx_err(rctx) {
            return (blank(), Some(e.into()));
        }
        leap = Some(VrSpaceLeap::new(grid, &sampler));
    }
    let mut surface_color = surface_color;
    if surface_color[3] == 0 {
        surface_color[3] = 255;
    }
    let box_min = size.scale(-0.5);
    let box_max = size.scale(0.5);
    let step_len = plan.sample_distance_mm;
    let ray_gen = RayGenerator::new(cam, w as i64, h as i64);
    let cut_gradients = shading && clip.is_some_and(|c| c.has_cut_planes());
    let mut clipped_value = iso - 1.0;
    if let Some((hu_min, _)) = prepared.hu_range() {
        clipped_value = fmath::min(clipped_value, hu_min);
    }
    let geometry = match clip_geometry_depth(rctx, clip, cam, w as i64, h as i64) {
        Ok(g) => g,
        Err(e) => return (blank(), Some(e.into())),
    };
    let geometry_ref = geometry.as_ref();
    let rows: Vec<OnceLock<Vec<u8>>> = (0..h).map(|_| OnceLock::new()).collect();
    let budget_error: Mutex<Option<VrSamplingError>> = Mutex::new(None);
    let (rows_ref, leap_ref, sampler_ref, ray_gen_ref) = (&rows, leap.as_ref(), &sampler, &ray_gen);
    let (budget_ref, cancel_ref) = (&budget_error, &cancel);
    let outcome = parallel_rows_worker_context(rctx, h as i64, || {
        move |py: i64| {
            let mut out = vec![0u8; w * 4];
            let row_base = ray_gen_ref.row_base(py);
            for px in 0..w {
                if ctx_err(rctx).is_some() {
                    break;
                }
                let (o, d) = ray_gen_ref.ray(px as i64, &row_base);
                let hit = intersect_aabb(o, d, box_min, box_max);
                let geometry_hit = depth_at(geometry_ref, px as i64, py);
                let fragments = depth_fragments(geometry_ref, px as i64, py);
                let mut set_composite = |t_end: f64, base: Rgba| {
                    let mut front: &[VrFragment] = &fragments;
                    while let Some(last) = front.last()
                        && last.t >= t_end
                    {
                        front = &front[..front.len() - 1];
                    }
                    let c = composite_vr_fragments(front, base);
                    if c.a <= 0.0 {
                        return;
                    }
                    out[px * 4..px * 4 + 4].copy_from_slice(&[
                        to8(linear_to_srgb(c.r / c.a)),
                        to8(linear_to_srgb(c.g / c.a)),
                        to8(linear_to_srgb(c.b / c.a)),
                        to8(c.a),
                    ]);
                };
                let (t0, mut t1) = hit.unwrap_or((0.0, 0.0));
                if let Some((t_geometry, _)) = geometry_hit {
                    t1 = fmath::min(t1, t_geometry);
                }
                let draw_background = |set: &mut dyn FnMut(f64, Rgba)| {
                    if let Some((_, color)) = geometry_hit {
                        set(f64::INFINITY, color);
                    } else if !fragments.is_empty() {
                        set(f64::INFINITY, Rgba::default());
                    }
                };
                if hit.is_none() || t0 > t1 {
                    draw_background(&mut set_composite);
                    continue;
                }
                let mut guard = VrRayGuard {
                    plan,
                    ..Default::default()
                };
                let hit = first_vr_surface_hit(
                    rctx,
                    sampler_ref,
                    leap_ref,
                    o,
                    d,
                    t0,
                    t1,
                    step_len,
                    box_min,
                    size,
                    iso,
                    clip,
                    &mut guard,
                );
                if guard.err.is_some() {
                    if let Ok(mut slot) = budget_ref.lock()
                        && slot.is_none()
                    {
                        *slot = guard.err.take();
                    }
                    cancel_ref.abort();
                    break;
                }
                let Some(tex) = hit else {
                    draw_background(&mut set_composite);
                    continue;
                };
                let (mut lit, mut specular) = (1.0, 0.0);
                if shading {
                    let mut gradient = None;
                    if cut_gradients {
                        gradient = clipped_gradient_index(sampler_ref, tex, clip, clipped_value);
                    }
                    let gradient = gradient.unwrap_or_else(|| gradient_at(sampler_ref, tex));
                    (lit, specular) = shade_with_gradient(gradient, d);
                }
                let channel = |c: u8| to8(f64::from(c) / 255.0 * lit + specular);
                let surface = [
                    channel(surface_color[0]),
                    channel(surface_color[1]),
                    channel(surface_color[2]),
                    surface_color[3],
                ];
                if !fragments.is_empty() {
                    let hit_point =
                        box_min.add(Vec3::new(tex.x * size.x, tex.y * size.y, tex.z * size.z));
                    let alpha = f64::from(surface[3]) / 255.0;
                    set_composite(
                        hit_point.sub(o).dot(d),
                        Rgba {
                            r: srgb_to_linear(f64::from(surface[0]) / 255.0) * alpha,
                            g: srgb_to_linear(f64::from(surface[1]) / 255.0) * alpha,
                            b: srgb_to_linear(f64::from(surface[2]) / 255.0) * alpha,
                            a: alpha,
                        },
                    );
                    continue;
                }
                out[px * 4..px * 4 + 4].copy_from_slice(&surface);
            }
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
            None => pixels.extend(std::iter::repeat_n(0u8, w * 4)),
        }
    }
    (
        Some(RenderImage::Rgba {
            width: w,
            height: h,
            pixels,
        }),
        outcome.err().map(VrRenderError::Render),
    )
}
const VR_PICK_ALPHA_THRESHOLD: f64 = 0.5;
pub fn pick_vr(
    vol: Option<&Arc<Volume>>,
    settings: &VrRenderSettings<'_>,
    sx: i64,
    sy: i64,
    w: i64,
    h: i64,
) -> Option<WorldPoint> {
    let VrRenderSettings {
        camera: cam,
        preset,
        window,
        clip,
        ..
    } = *settings;
    let vol = vol?;
    if w <= 0 || h <= 0 {
        return None;
    }
    let size = vol.physical_size();
    if size.x <= 0.0 || size.y <= 0.0 || size.z <= 0.0 {
        return None;
    }
    let box_min = size.scale(-0.5);
    let box_max = size.scale(0.5);
    let (o, d) = cam.ray(sx, sy, w, h);
    let (t0, t1) = intersect_aabb(o, d, box_min, box_max)?;
    let (data_min, data_max) = vol.hu_range().unwrap_or((0.0, 1.0));
    let window = normalize_window(window, &vol.recommended_window());
    let lut = if preset.tf.domain() == VrTransferDomain::Hu {
        preset
            .tf
            .bake_lut(data_min, data_max, preset.tf.preferred_lut_size())
    } else {
        preset
            .tf
            .bake_window_lut(&window, preset.tf.preferred_lut_size())
    };
    let step_len = size.length() / 256.0;
    let tex_at = |t: f64| {
        let pw = o.add(d.scale(t));
        Vec3::new(
            (pw.x - box_min.x) / size.x,
            (pw.y - box_min.y) / size.y,
            (pw.z - box_min.z) / size.z,
        )
    };
    let alpha_at = |t: f64| -> f64 {
        let tex = tex_at(t);
        if !clip_keep(clip, tex) {
            return 0.0;
        }
        let Some(hu) = vol.sample_hu_texture(tex) else {
            return 0.0;
        };
        if preset.tf.domain() == VrTransferDomain::Normalized {
            return lut.lookup(preset.tf.window_coordinate(hu, &window)).a;
        }
        lut.lookup(normalized_vr_data_coordinate(hu, data_min, data_max))
            .a
            * windowed_unit(hu, &window)
    };
    let unit = preset.tf.opacity_unit_distance_mm();
    let mut acc_a = 0.0f64;
    let mut first = true;
    let mut t = t0;
    while t <= t1 {
        let alpha = alpha_at(t);
        let next = acc_a + (1.0 - acc_a) * correct_vr_alpha(alpha, 1.0, step_len, unit);
        if next >= VR_PICK_ALPHA_THRESHOLD {
            if !first {
                let (before, start) = (acc_a, t - step_len);
                t = bisect_vr_ray(start, t, |tt| {
                    before + (1.0 - before) * correct_vr_alpha(alpha_at(tt), 1.0, tt - start, unit)
                        >= VR_PICK_ALPHA_THRESHOLD
                });
            }
            return Some(vol.voxel_to_patient(vol.texture_to_voxel(tex_at(t))));
        }
        acc_a = next;
        first = false;
        t += step_len;
    }
    None
}
pub fn vr_length_mm(a: WorldPoint, b: WorldPoint) -> f64 {
    let (dx, dy, dz) = (a.x - b.x, a.y - b.y, a.z - b.z);
    (dx * dx + dy * dy + dz * dz).sqrt()
}
pub fn render_vr_movie(
    vol: Option<&Arc<Volume>>,
    settings: &VrRenderSettings<'_>,
    arc_deg: f64,
    frames: i64,
) -> Vec<Option<RenderImage>> {
    let frames = frames.max(1);
    let arc = arc_deg * std::f64::consts::PI / 180.0;
    let cam = settings.camera;
    (0..frames)
        .map(|i| {
            let mut c = *cam;
            c.yaw = cam.yaw + arc * i as f64 / frames as f64;
            let frame = VrRenderSettings {
                camera: &c,
                ..*settings
            };
            render_vr(None, vol, &frame).0
        })
        .collect()
}
