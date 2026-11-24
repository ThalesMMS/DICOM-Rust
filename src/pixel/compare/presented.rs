use super::images::PresentedImagePolicy;
use super::{Accumulator, Error, Metrics, ctx_err};
use crate::fmath;
use crate::net::ul::Ctx;
use crate::render::qualification::RgbaFrameErrorMetricsV1;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rect {
    pub x0: i64,
    pub y0: i64,
    pub x1: i64,
    pub y1: i64,
}
impl Rect {
    pub fn new(x0: i64, y0: i64, x1: i64, y1: i64) -> Rect {
        Rect { x0, y0, x1, y1 }
    }
    pub fn is_empty(&self) -> bool {
        self.x0 >= self.x1 || self.y0 >= self.y1
    }
    fn within(&self, outer: &Rect) -> bool {
        self.is_empty()
            || (outer.x0 <= self.x0
                && self.x1 <= outer.x1
                && outer.y0 <= self.y0
                && self.y1 <= outer.y1)
    }
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PresentedFrame {
    pub width: i64,
    pub height: i64,
    pub components: i64,
    pub depth: i64,
    pub encoding: String,
    pub values: Vec<f64>,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SsimOptions {
    pub window: i64,
    pub kernel: String,
    pub sigma: f64,
    pub k1: f64,
    pub k2: f64,
    pub range: f64,
    pub border: String,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ComparisonRegion {
    pub name: String,
    pub bounds: Rect,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PresentedOptions {
    pub ssim: SsimOptions,
    pub difference: String,
    pub scale: f64,
    pub max_pixels: i64,
    pub max_working_bytes: i64,
    pub mask: Vec<bool>,
    pub regions: Vec<ComparisonRegion>,
}
pub fn default_presented_options() -> PresentedOptions {
    PresentedOptions {
        ssim: SsimOptions {
            window: 7,
            kernel: "gaussian".into(),
            sigma: 1.5,
            k1: 0.01,
            k2: 0.03,
            range: 1.0,
            border: "reflect".into(),
        },
        difference: "absolute".into(),
        scale: 1.0,
        max_pixels: 4 << 20,
        max_working_bytes: 128 << 20,
        mask: Vec::new(),
        regions: Vec::new(),
    }
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PresentedStatistics {
    pub name: String,
    pub bounds: Rect,
    pub numerical: Metrics,
    pub ssim_samples: i64,
    pub mean_ssim: f64,
    pub minimum_ssim: f64,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PresentedResult {
    pub schema: String,
    pub width: i64,
    pub height: i64,
    pub components: i64,
    pub depth: i64,
    pub encoding: String,
    pub options: PresentedOptions,
    pub difference: Vec<f64>,
    pub ssim: Vec<f64>,
    pub valid: Vec<bool>,
    pub summary: PresentedStatistics,
    pub regions: Vec<PresentedStatistics>,
    pub image_policy: Option<PresentedImagePolicy>,
    pub raw_rgba: Option<RgbaFrameErrorMetricsV1>,
    pub logical_working_bytes: i64,
}
pub(super) fn finite(v: f64) -> bool {
    v.is_finite()
}
fn presented_shape(f: &PresentedFrame) -> Result<i64, Error> {
    let Some(n) = f
        .width
        .checked_mul(f.height)
        .filter(|_| f.width > 0 && f.height > 0)
    else {
        return Err(Error::Mismatch(Some("extent")));
    };
    if (f.components != 1 && f.components != 3)
        || (f.depth != 8 && f.depth != 16)
        || n.checked_mul(f.components).is_none()
    {
        return Err(Error::Mismatch(Some("depth/components")));
    }
    if (f.components == 1 && f.encoding != "gray-pvalue")
        || (f.components == 3 && f.encoding != "srgb-code" && f.encoding != "linear-rgb")
        || f.values.len() as i64 != n.saturating_mul(f.components)
    {
        return Err(Error::Mismatch(Some("encoding/length")));
    }
    Ok(n)
}
pub(super) fn presented_budget(
    width: i64,
    height: i64,
    components: i64,
    o: &PresentedOptions,
    extra_per_pixel: i64,
) -> Result<i64, Error> {
    let Some(n) = width
        .checked_mul(height)
        .filter(|_| width > 0 && height > 0 && (components == 1 || components == 3))
    else {
        return Err(Error::Mismatch(None));
    };
    let s = &o.ssim;
    if s.window < 1
        || s.window > 31
        || s.window % 2 == 0
        || (s.kernel != "uniform" && s.kernel != "gaussian")
        || !finite(s.sigma)
        || s.sigma < 0.0
        || (s.kernel == "gaussian" && s.sigma <= 0.0)
        || !finite(s.k1)
        || !finite(s.k2)
        || s.k1 < 1e-6
        || s.k2 < 1e-6
        || s.k1 > 1.0
        || s.k2 > 1.0
        || s.k1 * s.k1 == 0.0
        || s.k2 * s.k2 == 0.0
        || s.range != 1.0
        || (s.border != "crop" && s.border != "reflect" && s.border != "valid")
    {
        return Err(Error::Configuration(Some("SSIM")));
    }
    if (o.difference != "signed" && o.difference != "absolute")
        || !finite(o.scale)
        || o.scale <= 0.0
        || o.max_pixels <= 0
        || o.max_working_bytes <= 0
        || o.regions.len() > 256
        || (!o.mask.is_empty() && o.mask.len() as i64 != n)
    {
        return Err(Error::Configuration(Some("difference/mask/budget")));
    }
    let per_pixel = components
        .saturating_mul(16)
        .saturating_add(2)
        .saturating_add(extra_per_pixel);
    let room = (i64::MAX - 8192 - 256 * 1024)
        .checked_div(per_pixel)
        .unwrap_or(0);
    if per_pixel <= 0 || n > room {
        return Err(Error::Limit);
    }
    let bytes = n
        .saturating_mul(per_pixel)
        .saturating_add(s.window.saturating_mul(s.window).saturating_mul(8))
        .saturating_add((o.regions.len() as i64).saturating_mul(1024));
    if n > o.max_pixels || bytes > o.max_working_bytes {
        return Err(Error::Limit);
    }
    let bounds = Rect::new(0, 0, width, height);
    let mut seen = std::collections::HashSet::new();
    for r in &o.regions {
        if r.name.is_empty()
            || r.name.len() > 256
            || !seen.insert(r.name.as_str())
            || r.bounds.is_empty()
            || !r.bounds.within(&bounds)
        {
            return Err(Error::Configuration(Some("region")));
        }
    }
    Ok(bytes)
}
pub fn compare_presented(
    ctx: &Ctx,
    reference: &PresentedFrame,
    candidate: &PresentedFrame,
    o: &PresentedOptions,
) -> Result<PresentedResult, Error> {
    ctx_err(ctx)?;
    presented_shape(reference)?;
    let bytes = presented_budget(
        reference.width,
        reference.height,
        reference.components,
        o,
        0,
    )?;
    presented_shape(candidate)?;
    if reference.width != candidate.width
        || reference.height != candidate.height
        || reference.components != candidate.components
        || reference.depth != candidate.depth
        || reference.encoding != candidate.encoding
    {
        return Err(Error::Mismatch(None));
    }
    for (i, (&v, &u)) in reference.values.iter().zip(&candidate.values).enumerate() {
        if i % 4096 == 0 {
            ctx_err(ctx)?;
        }
        if !finite(v) || !(0.0..=1.0).contains(&v) || !finite(u) || !(0.0..=1.0).contains(&u) {
            return Err(Error::Mismatch(Some("sample outside normalized range")));
        }
    }
    let (width, height) = (reference.width as usize, reference.height as usize);
    let n = width.saturating_mul(height);
    let c = reference.components as usize;
    let mut r = PresentedResult {
        schema: "presented-comparison-v1".into(),
        width: reference.width,
        height: reference.height,
        components: reference.components,
        depth: reference.depth,
        encoding: reference.encoding.clone(),
        options: o.clone(),
        difference: vec![0.0; n.saturating_mul(c)],
        ssim: vec![0.0; n.saturating_mul(c)],
        valid: vec![false; n],
        logical_working_bytes: bytes,
        ..PresentedResult::default()
    };
    let weights = super::ssim::weights(&o.ssim);
    let radius = (o.ssim.window / 2) as usize;
    let absolute = o.difference == "absolute";
    let valid_border = o.ssim.border == "valid";
    for y in 0..height {
        ctx_err(ctx)?;
        for x in 0..width {
            if x % 32 == 0 {
                ctx_err(ctx)?;
            }
            let p = y.saturating_mul(width).saturating_add(x);
            if !o.mask.is_empty() && !o.mask.get(p).copied().unwrap_or(false) {
                continue;
            }
            let first = p.saturating_mul(c);
            let span = first..first.saturating_add(c);
            if let (Some(cv), Some(rv), Some(dv)) = (
                candidate.values.get(span.clone()),
                reference.values.get(span.clone()),
                r.difference.get_mut(span.clone()),
            ) {
                for ((out, &u), &v) in dv.iter_mut().zip(cv).zip(rv) {
                    let d = u - v;
                    *out = if absolute { d.abs() } else { d };
                }
            }
            if valid_border
                && (x < radius
                    || y < radius
                    || x.saturating_add(radius) >= width
                    || y.saturating_add(radius) >= height)
            {
                continue;
            }
            if let Some(v) = r.valid.get_mut(p) {
                *v = true;
            }
            if let Some(out) = r.ssim.get_mut(span) {
                for (ch, s) in out.iter_mut().enumerate() {
                    *s = super::ssim::local(reference, candidate, o, &weights, x, y, ch);
                }
            }
        }
    }
    let all = Rect::new(0, 0, r.width, r.height);
    r.summary = presented_statistics(ctx, reference, candidate, &r, "all", all)?;
    if r.summary.numerical.samples == 0 || r.summary.ssim_samples == 0 {
        return Err(Error::Configuration(Some("empty mask or valid interior")));
    }
    let mut regions = Vec::with_capacity(o.regions.len());
    for region in &o.regions {
        regions.push(presented_statistics(
            ctx,
            reference,
            candidate,
            &r,
            &region.name,
            region.bounds,
        )?);
    }
    r.regions = regions;
    ctx_err(ctx)?;
    Ok(r)
}
fn presented_statistics(
    ctx: &Ctx,
    a: &PresentedFrame,
    b: &PresentedFrame,
    r: &PresentedResult,
    name: &str,
    rect: Rect,
) -> Result<PresentedStatistics, Error> {
    let mut stats = PresentedStatistics {
        name: name.into(),
        bounds: rect,
        minimum_ssim: 1.0,
        ..PresentedStatistics::default()
    };
    let mut acc = Accumulator::default();
    let mut sum = 0.0;
    let c = r.components as usize;
    let mask = &r.options.mask;
    for y in rect.y0..rect.y1 {
        ctx_err(ctx)?;
        for x in rect.x0..rect.x1 {
            let p = y.saturating_mul(r.width).saturating_add(x) as usize;
            if !mask.is_empty() && !mask.get(p).copied().unwrap_or(false) {
                continue;
            }
            let valid = r.valid.get(p).copied().unwrap_or(false);
            let first = p.saturating_mul(c);
            for i in first..first.saturating_add(c) {
                let (Some(&va), Some(&vb)) = (a.values.get(i), b.values.get(i)) else {
                    continue;
                };
                acc.add(va, vb);
                if valid {
                    let v = r.ssim.get(i).copied().unwrap_or(0.0);
                    sum += v;
                    stats.ssim_samples = stats.ssim_samples.saturating_add(1);
                    stats.minimum_ssim = fmath::min(stats.minimum_ssim, v);
                }
            }
        }
    }
    stats.numerical = acc.metrics();
    stats.numerical.frames = 1;
    if stats.ssim_samples > 0 {
        stats.mean_ssim = sum / stats.ssim_samples as f64;
    } else {
        stats.minimum_ssim = 0.0;
    }
    Ok(stats)
}
