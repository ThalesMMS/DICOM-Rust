use super::error::{RegError, Sentinel};
use super::image::Image;
use super::matrix::{Jacobian, finite, finite_slice, finite_vec};
use super::metric::{
    CorrelationPartial, Evaluator, Metric, MetricDomain, Sample, accumulate_derivative, project,
};
use super::transform::Transform;
use crate::fmath::rand::Rand;
use crate::render::Vec3;
use crate::render::parallel::{Cancellation, ctx_err};
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MetricLimits {
    pub components: i64,
    pub points: i64,
    pub channels: i64,
    pub parameters: i64,
    pub workers: i64,
    pub bytes: u64,
}
pub(crate) fn multi() -> RegError {
    RegError::bare(Sentinel::MultiMetric)
}
impl MetricLimits {
    pub(crate) fn checked(self) -> Result<MetricLimits, RegError> {
        let mut l = self;
        if l.components < 0
            || l.components > 4096
            || l.points < 0
            || l.channels < 0
            || l.parameters < 0
            || l.workers < 0
            || l.parameters > 4096
        {
            return Err(multi());
        }
        if l.components == 0 {
            l.components = 64;
        }
        if l.points == 0 {
            l.points = 1 << 24;
        }
        if l.channels == 0 {
            l.channels = 64;
        }
        if l.parameters == 0 {
            l.parameters = 4096;
        }
        if l.workers == 0 {
            l.workers = 64;
        }
        if l.bytes == 0 {
            l.bytes = 256 << 20;
        }
        Ok(l)
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct ComponentInfo {
    pub frame: String,
    pub units: String,
    pub points: i64,
    pub channels: i64,
    pub parameters: i64,
    pub bytes: u64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ImageMetricKind(pub i64);
impl ImageMetricKind {
    pub const MEAN_SQUARES: ImageMetricKind = ImageMetricKind(0);
    pub const CORRELATION: ImageMetricKind = ImageMetricKind(1);
}
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ImageComponentOptions {
    pub kind: ImageMetricKind,
    pub frame_of_reference: String,
    pub units: String,
    pub sample_count: i64,
    pub seed: i64,
    pub limits: MetricLimits,
}
#[derive(Debug, Clone)]
struct Owned {
    fixed: Image,
    moving: Image,
    fixed_mask: Option<Image>,
    moving_mask: Option<Image>,
    points: Option<Vec<Vec3>>,
    workers: i64,
}
#[derive(Debug, Clone)]
pub struct ImageMetricComponent {
    owned: Owned,
    kind: ImageMetricKind,
    pub(crate) info: ComponentInfo,
}
fn check(ctx: Option<&dyn Cancellation>) -> Result<(), RegError> {
    match ctx_err(ctx) {
        Some(e) => Err(e.into()),
        None => Ok(()),
    }
}
impl ImageMetricComponent {
    pub fn new(
        ctx: Option<&dyn Cancellation>,
        fixed: Option<&Image>,
        moving: Option<&Image>,
        domain: &MetricDomain,
        o: &ImageComponentOptions,
    ) -> Result<ImageMetricComponent, RegError> {
        let l = o.limits.checked()?;
        let (Some(fixed), Some(moving)) = (fixed, moving) else {
            return Err(multi());
        };
        if ctx.is_none()
            || o.frame_of_reference.is_empty()
            || o.frame_of_reference.len() > 1024
            || o.units.len() > 256
            || o.units.is_empty()
            || o.kind.0 < 0
            || o.kind.0 > 1
            || o.sample_count < 0
            || domain.workers < 0
            || domain.workers > l.workers
        {
            return Err(multi());
        }
        check(ctx)?;
        let count = domain.points.as_ref().map_or(fixed.data.len(), |p| p.len()) as i64;
        if count == 0 || count > l.points || o.sample_count > l.points {
            return Err(multi());
        }
        if let Some(points) = &domain.points {
            for (i, v) in points.iter().enumerate() {
                if i % 256 == 0 {
                    check(ctx)?;
                }
                if !finite_vec(*v) {
                    return Err(multi());
                }
            }
        }
        let output_count = if o.sample_count > 0 {
            o.sample_count
        } else {
            count
        };
        let mut bytes = 4096u64 + l.parameters as u64 * 128;
        for im in [
            Some(fixed),
            Some(moving),
            domain.fixed_mask,
            domain.moving_mask,
        ]
        .into_iter()
        .flatten()
        {
            if im.data.is_empty() || im.data.len() as u64 > l.bytes / 4 {
                return Err(multi());
            }
            let n = im.data.len() as u64 * 4;
            if bytes > l.bytes || n > l.bytes - bytes {
                return Err(multi());
            }
            bytes += n;
        }
        let sampled = domain.points.is_some() || o.sample_count > 0;
        let mut point_bytes = 0u64;
        if sampled {
            if output_count as u64 > l.bytes / 24 {
                return Err(multi());
            }
            point_bytes = output_count as u64 * 24;
        }
        if bytes > l.bytes || point_bytes > l.bytes - bytes {
            return Err(multi());
        }
        bytes += point_bytes;
        let copy = |im: &Image| -> Result<Image, RegError> {
            for (i, v) in im.data.iter().enumerate() {
                if i % 256 == 0 {
                    check(ctx)?;
                }
                if !finite(f64::from(*v)) {
                    return Err(multi());
                }
            }
            Ok(im.clone())
        };
        let f = copy(fixed)?;
        let mv = copy(moving)?;
        let fm = domain.fixed_mask.map(copy).transpose()?;
        let mm = domain.moving_mask.map(copy).transpose()?;
        let mut points = None;
        if sampled {
            let mut out = Vec::with_capacity(output_count as usize);
            let mut rng = Rand::new(o.seed);
            for i in 0..output_count {
                if i % 256 == 0 {
                    check(ctx)?;
                }
                let j = if o.sample_count > 0 {
                    rng.intn(count).unwrap_or(0)
                } else {
                    i
                } as usize;
                let p = match &domain.points {
                    Some(points) => points[j],
                    None => {
                        let s = fixed.size;
                        fixed.point(Vec3::new(
                            (j % s[0]) as f64,
                            (j / s[0] % s[1]) as f64,
                            (j / (s[0] * s[1])) as f64,
                        ))
                    }
                };
                if !finite_vec(p) {
                    return Err(multi());
                }
                out.push(p);
            }
            points = Some(out);
        }
        check(ctx)?;
        Ok(ImageMetricComponent {
            owned: Owned {
                fixed: f,
                moving: mv,
                fixed_mask: fm,
                moving_mask: mm,
                points,
                workers: domain.workers,
            },
            kind: o.kind,
            info: ComponentInfo {
                frame: o.frame_of_reference.clone(),
                units: o.units.clone(),
                points: output_count,
                channels: 1,
                parameters: l.parameters,
                bytes,
            },
        })
    }
    fn evaluator(&self) -> Evaluator<'_> {
        let o = &self.owned;
        Evaluator {
            fixed: &o.fixed,
            moving: &o.moving,
            domain: MetricDomain {
                fixed_mask: o.fixed_mask.as_ref(),
                moving_mask: o.moving_mask.as_ref(),
                points: o.points.clone(),
                workers: o.workers,
            },
        }
    }
    pub fn value_context(
        &self,
        ctx: Option<&dyn Cancellation>,
        t: &dyn Transform,
    ) -> Result<f64, RegError> {
        self.evaluate_component(ctx, t, false)
            .map(|r| r.0)
            .map_err(|e| e.0)
    }
    pub fn value_and_derivative_context(
        &self,
        ctx: Option<&dyn Cancellation>,
        t: &dyn Transform,
    ) -> Result<(f64, Vec<f64>), RegError> {
        self.evaluate_component(ctx, t, true)
            .map(|r| (r.0, r.1))
            .map_err(|e| e.0)
    }
    pub(crate) fn evaluate_component(
        &self,
        ctx: Option<&dyn Cancellation>,
        t: &dyn Transform,
        derivatives: bool,
    ) -> Result<(f64, Vec<f64>, i64), (RegError, i64)> {
        let p = t.number_of_parameters();
        if ctx.is_none() || p < 1 || p as i64 > self.info.parameters {
            return Err((multi(), 0));
        }
        let e = self.evaluator();
        let mut g = vec![0.0; p];
        let mut j = Jacobian::default();
        let mut moments = CorrelationPartial::new(
            if derivatives && self.kind == ImageMetricKind::CORRELATION {
                p
            } else {
                0
            },
        );
        let mut sum = 0.0;
        let mut n = 0i64;
        let ck = |n: i64| check(ctx).map_err(|e| (e, n));
        for i in 0..e.point_count() {
            if i % 64 == 0 {
                ck(n)?;
            }
            let Some(s) = sample_at(&e, i, t, derivatives).map_err(|err| (err, n))? else {
                continue;
            };
            if !finite_slice(&[s.fixed, s.moving]) || derivatives && !finite_vec(s.gradient) {
                return Err((multi(), n));
            }
            n += 1;
            if self.kind == ImageMetricKind::MEAN_SQUARES {
                let diff = s.moving - s.fixed;
                sum += diff * diff;
                if derivatives {
                    accumulate_derivative(&mut g, &mut j, t, &s, 2.0 * diff);
                }
                continue;
            }
            moments.f += s.fixed;
            moments.m += s.moving;
            moments.ff += s.fixed * s.fixed;
            moments.mm += s.moving * s.moving;
            moments.fm += s.fixed * s.moving;
            if derivatives {
                t.jacobian_with_respect_to_parameters(s.fixed_point, &mut j);
                for k in 0..p {
                    let v = project(s.gradient, &j, k);
                    moments.g[k] += v;
                    moments.fg[k] += s.fixed * v;
                    moments.mg[k] += s.moving * v;
                }
            }
        }
        ck(n)?;
        if n == 0 {
            return Err((RegError::bare(Sentinel::TooFewSamples), n));
        }
        let mut value = sum / n as f64;
        if self.kind == ImageMetricKind::MEAN_SQUARES {
            for v in g.iter_mut() {
                *v /= n as f64;
            }
        } else {
            let nf = n as f64;
            let (f, mv) = (moments.f / nf, moments.m / nf);
            let (ff, mm, fm) = (
                moments.ff - nf * f * f,
                moments.mm - nf * mv * mv,
                moments.fm - nf * f * mv,
            );
            if n < 2 || ff <= 0.0 || mm <= 0.0 {
                return Err((RegError::bare(Sentinel::TooFewSamples), n));
            }
            value = -fm * fm / (ff * mm);
            if derivatives {
                let scale = -2.0 * fm / (ff * mm);
                for (k, v) in g.iter_mut().enumerate() {
                    *v = scale
                        * (moments.fg[k]
                            - f * moments.g[k]
                            - fm / mm * (moments.mg[k] - mv * moments.g[k]));
                }
            }
        }
        if !finite(value) || !finite_slice(&g) {
            return Err((
                RegError::new(Sentinel::MultiMetric, "nonfinite image evaluation"),
                n,
            ));
        }
        if !derivatives {
            g = Vec::new();
        }
        Ok((value, g, n))
    }
}
fn sample_at(
    e: &Evaluator,
    i: usize,
    t: &dyn Transform,
    gradients: bool,
) -> Result<Option<Sample>, RegError> {
    let (point, fixed) = match &e.domain.points {
        Some(points) => {
            let point = points[i];
            if !finite_vec(point) {
                return Err(multi());
            }
            let index = e.fixed.continuous_index(point);
            if !finite_vec(index) {
                return Err(multi());
            }
            if !e.fixed.inside(index) {
                return Ok(None);
            }
            (point, e.fixed.linear_at(index))
        }
        None => {
            let size = e.fixed.size;
            let point = e.fixed.point(Vec3::new(
                (i % size[0]) as f64,
                (i / size[0] % size[1]) as f64,
                (i / (size[0] * size[1])) as f64,
            ));
            if !finite_vec(point) {
                return Err(multi());
            }
            (point, f64::from(e.fixed.data[i]))
        }
    };
    if !mask_at(e.domain.fixed_mask, point)? {
        return Ok(None);
    }
    let mapped = t.transform_point(point);
    if !finite_vec(mapped) {
        return Err(multi());
    }
    let index = e.moving.continuous_index(mapped);
    if !finite_vec(index) {
        return Err(multi());
    }
    if !e.moving.inside(index) {
        return Ok(None);
    }
    if !mask_at(e.domain.moving_mask, mapped)? {
        return Ok(None);
    }
    let mut out = Sample {
        fixed_point: point,
        fixed,
        moving: e.moving.linear_at(index),
        ..Default::default()
    };
    if gradients {
        out.gradient = e.moving.gradient_at(index);
    }
    Ok(Some(out))
}
fn mask_at(im: Option<&Image>, point: Vec3) -> Result<bool, RegError> {
    let Some(im) = im else {
        return Ok(true);
    };
    let index = im.continuous_index(point);
    if !finite_vec(index) {
        return Err(multi());
    }
    if !im.inside(index) {
        return Ok(false);
    }
    Ok(im.at(
        (index.x + 0.5) as usize,
        (index.y + 0.5) as usize,
        (index.z + 0.5) as usize,
    ) != 0.0)
}
impl Metric for ImageMetricComponent {
    fn context_value(
        &self,
        ctx: Option<&dyn Cancellation>,
        transform: &dyn Transform,
    ) -> Option<Result<f64, RegError>> {
        Some(self.value_context(ctx, transform))
    }
    fn context_value_and_derivative(
        &self,
        ctx: Option<&dyn Cancellation>,
        transform: &dyn Transform,
    ) -> Option<Result<(f64, Vec<f64>), RegError>> {
        Some(self.value_and_derivative_context(ctx, transform))
    }
    fn value(&self, t: &dyn Transform) -> Result<f64, RegError> {
        let token = crate::render::parallel::CancelToken::new();
        self.value_context(Some(&token), t)
    }
    fn value_and_derivative(&self, t: &dyn Transform) -> Result<(f64, Vec<f64>), RegError> {
        let token = crate::render::parallel::CancelToken::new();
        self.value_and_derivative_context(Some(&token), t)
    }
}
