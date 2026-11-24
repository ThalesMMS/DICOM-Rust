use super::*;
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VrIsoLayer {
    pub value: f64,
    pub opacity: f64,
    pub color: [f64; 3],
}
pub const MAX_VR_ISO_LAYERS: usize = 8;
fn refused(message: &str) -> VrRenderError {
    VrRenderError::Render(crate::render::error::RenderError::new(
        crate::render::error::ErrorKind::InvalidViewState,
        message,
    ))
}
pub fn render_vr_iso_layers_prepared(
    ctx: Ctx<'_>,
    prepared: &VrPreparedVolume,
    cam: &VrCamera,
    layers: &[VrIsoLayer],
    shading: bool,
    clip: Option<&VrClip>,
    quality: &VrQuality,
) -> Result<RenderImage, VrRenderError> {
    if layers.is_empty() || layers.len() > MAX_VR_ISO_LAYERS {
        return Err(refused("VR iso-surface count outside 1..8"));
    }
    if layers.iter().any(|l| {
        !l.value.is_finite()
            || !(l.opacity > 0.0 && l.opacity <= 1.0)
            || l.color.iter().any(|c| !(0.0..=1.0).contains(c))
    }) {
        return Err(refused("invalid VR iso-surface"));
    }
    if clip.is_some_and(|c| c.geometry().is_some()) {
        return Err(refused("VR iso-surface list with geometry"));
    }
    let (w, h) = viewport_size(quality);
    let vol = &prepared.volume;
    let size = vol.physical_size();
    if size.x <= 0.0 || size.y <= 0.0 || size.z <= 0.0 {
        return Ok(surface_image(quality));
    }
    let mut sampling_quality = *quality;
    sampling_quality.adaptive_step = VrAdaptiveStep::default();
    let plan = volume_vr_ray_sampling(vol, &sampling_quality).map_err(VrRenderError::Sampling)?;
    let cancel = RenderCancel::new(ctx);
    let rctx: Ctx<'_> = Some(&cancel);
    let sampler = prepared.new_sampler()?;
    let lowest = layers
        .iter()
        .map(|l| l.value)
        .fold(f64::INFINITY, fmath::min);
    let mut leap = None;
    if clip.is_none_or(|c| !c.full_rays) {
        let grid = prepared.block_grid(rctx, &sampler)?;
        leap = Some(VrSpaceLeap::new(grid, &sampler));
    }
    let box_min = size.scale(-0.5);
    let box_max = size.scale(0.5);
    let step_len = plan.sample_distance_mm;
    let ray_gen = RayGenerator::new(cam, w as i64, h as i64);
    let cut_gradients = shading && clip.is_some_and(|c| c.has_cut_planes());
    let mut clipped_value = lowest - 1.0;
    if let Some((hu_min, _)) = prepared.hu_range() {
        clipped_value = fmath::min(clipped_value, hu_min);
    }
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
                let Some((t0, t1)) = intersect_aabb(o, d, box_min, box_max) else {
                    continue;
                };
                if t0 > t1 {
                    continue;
                }
                let mut guard = VrRayGuard {
                    plan,
                    ..Default::default()
                };
                let ray = Ray {
                    sampler: sampler_ref,
                    leap: leap_ref,
                    origin: o,
                    direction: d,
                    box_min,
                    size,
                    clip,
                };
                let acc = ray.march(rctx, layers, lowest, t0, t1, step_len, &mut guard, |tex| {
                    if !shading {
                        return (1.0, 0.0);
                    }
                    let mut gradient = None;
                    if cut_gradients {
                        gradient = clipped_gradient_index(sampler_ref, tex, clip, clipped_value);
                    }
                    let gradient = gradient.unwrap_or_else(|| gradient_at(sampler_ref, tex));
                    shade_with_gradient(gradient, d)
                });
                if guard.err.is_some() {
                    if let Ok(mut slot) = budget_ref.lock()
                        && slot.is_none()
                    {
                        *slot = guard.err.take();
                    }
                    cancel_ref.abort();
                    break;
                }
                let Some(acc) = acc else {
                    continue;
                };
                if acc[3] <= 0.0 {
                    continue;
                }
                out[px * 4..px * 4 + 4].copy_from_slice(&[
                    to8(acc[0] / acc[3]),
                    to8(acc[1] / acc[3]),
                    to8(acc[2] / acc[3]),
                    to8(acc[3]),
                ]);
            }
            let _ = rows_ref[py as usize].set(out);
        }
    });
    if let Ok(mut slot) = budget_error.lock()
        && let Some(e) = slot.take()
    {
        return Err(VrRenderError::Sampling(e));
    }
    if let Err(e) = outcome {
        return Err(VrRenderError::Render(e));
    }
    let mut pixels = Vec::with_capacity(w * h * 4);
    for row in rows {
        match row.into_inner() {
            Some(row) => pixels.extend_from_slice(&row),
            None => pixels.extend(std::iter::repeat_n(0u8, w * 4)),
        }
    }
    Ok(RenderImage::Rgba {
        width: w,
        height: h,
        pixels,
    })
}
struct Ray<'a> {
    sampler: &'a Sampler,
    leap: Option<&'a VrSpaceLeap<'a>>,
    origin: Vec3,
    direction: Vec3,
    box_min: Vec3,
    size: Vec3,
    clip: Option<&'a VrClip>,
}
impl Ray<'_> {
    fn tex_at(&self, t: f64) -> Vec3 {
        let point = self.origin.add(self.direction.scale(t));
        Vec3::new(
            (point.x - self.box_min.x) / self.size.x,
            (point.y - self.box_min.y) / self.size.y,
            (point.z - self.box_min.z) / self.size.z,
        )
    }
    fn value(&self, t: f64) -> Option<f64> {
        let tex = self.tex_at(t);
        if !clip_keep(self.clip, tex) {
            return None;
        }
        texture_at(self.sampler, tex)
    }
    #[allow(clippy::too_many_arguments)]
    fn march(
        &self,
        ctx: Ctx<'_>,
        layers: &[VrIsoLayer],
        lowest: f64,
        t0: f64,
        t1: f64,
        step_len: f64,
        guard: &mut VrRayGuard,
        light: impl Fn(Vec3) -> (f64, f64),
    ) -> Option<[f64; 4]> {
        let mut acc = [0.0f64; 4];
        if step_len <= 0.0 {
            return Some(acc);
        }
        let (leap_a, leap_b) = match self.leap {
            Some(l) => l.ray_voxel(self.origin, self.direction, self.box_min, self.size),
            None => (Vec3::ZERO, Vec3::ZERO),
        };
        let opaque = 1.0 - 1.0 / 255.0;
        let mut previous = 0u32;
        let mut sample_index = 0i64;
        let mut t = t0;
        while t <= t1 {
            if !guard.take() {
                return None;
            }
            if sample_index % VR_CANCELLATION_CHECK_STRIDE == 0 && ctx_err(ctx).is_some() {
                return None;
            }
            sample_index += 1;
            if let Some(l) = self.leap {
                let block = l.block(self.tex_at(t));
                if block >= 0 && skips_vr_surface_block(l.grid, block, lowest) {
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
                    previous = 0;
                    t += step_len;
                    continue;
                }
            }
            let value = self.value(t);
            let mut solid = 0u32;
            if let Some(value) = value {
                for (i, layer) in layers.iter().enumerate() {
                    if value >= layer.value {
                        solid |= 1 << i;
                    }
                }
            }
            let mut rising = solid & !previous;
            previous = solid;
            if rising != 0 {
                let mut hits = [0.0f64; MAX_VR_ISO_LAYERS];
                for (i, layer) in layers.iter().enumerate() {
                    if rising & (1 << i) != 0 {
                        hits[i] = if sample_index > 1 {
                            bisect_vr_ray(t - step_len, t, |m| {
                                self.value(m).is_some_and(|v| v >= layer.value)
                            })
                        } else {
                            t
                        };
                    }
                }
                while rising != 0 && acc[3] <= opaque {
                    let nearest = (0..layers.len())
                        .filter(|i| rising & (1 << i) != 0)
                        .min_by(|a, b| hits[*a].total_cmp(&hits[*b]))
                        .unwrap_or(0);
                    rising &= !(1 << nearest);
                    let (lit, specular) = light(self.tex_at(hits[nearest]));
                    let layer = &layers[nearest];
                    let color = layer
                        .color
                        .map(|c| fmath::max(0.0, fmath::min(1.0, c * lit + specular)));
                    let alpha = layer.opacity;
                    let rest = 1.0 - acc[3];
                    acc = [
                        acc[0] + rest * alpha * color[0],
                        acc[1] + rest * alpha * color[1],
                        acc[2] + rest * alpha * color[2],
                        acc[3] + rest * alpha,
                    ];
                }
                if acc[3] > opaque {
                    break;
                }
            }
            t += step_len;
        }
        Some(acc)
    }
}
