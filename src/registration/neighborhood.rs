use super::error::{RegError, Sentinel};
use super::image::Image;
use super::matrix::{Jacobian, finite, finite_vec};
use super::metric::{Evaluator, Metric, MetricDomain, Sample, project};
use super::transform::Transform;
use crate::render::Vec3;
use crate::render::parallel::{Cancellation, ctx_err};
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct NeighborhoodCorrelationOptions {
    pub radius: [i64; 3],
    pub radius_mm: [f64; 3],
    pub minimum_variance: f64,
    pub max_centers: i64,
    pub max_neighborhood_samples: i64,
    pub max_working_bytes: i64,
    pub max_work: i64,
}
#[derive(Debug, Clone)]
pub struct NeighborhoodCorrelation<'a> {
    evaluator: Evaluator<'a>,
    pub(crate) options: NeighborhoodCorrelationOptions,
    pub(crate) radius: [i64; 3],
    pub(crate) neighborhood: i64,
    center_bytes: i64,
}
fn nc() -> RegError {
    RegError::bare(Sentinel::NeighborhoodCorrelation)
}
fn limit() -> RegError {
    RegError::bare(Sentinel::NeighborhoodCorrelationLimit)
}
fn check(ctx: Option<&dyn Cancellation>) -> Result<(), RegError> {
    match ctx_err(ctx) {
        Some(e) => Err(e.into()),
        None => Ok(()),
    }
}
impl<'a> NeighborhoodCorrelation<'a> {
    pub fn new(
        fixed: Option<&'a Image>,
        moving: Option<&'a Image>,
        domain: MetricDomain<'a>,
        options: NeighborhoodCorrelationOptions,
    ) -> Result<NeighborhoodCorrelation<'a>, RegError> {
        let e = Evaluator::new(fixed, moving, domain)?;
        let mut o = options;
        if !finite(o.minimum_variance)
            || o.minimum_variance < 0.0
            || o.max_centers < 0
            || o.max_neighborhood_samples < 0
            || o.max_working_bytes < 0
            || o.max_work < 0
            || e.domain.workers < 0
        {
            return Err(nc());
        }
        if o.max_centers == 0 {
            o.max_centers = 1_000_000;
        }
        if o.max_neighborhood_samples == 0 {
            o.max_neighborhood_samples = 4096;
        }
        if o.max_working_bytes == 0 {
            o.max_working_bytes = 16 << 20;
        }
        if o.max_work == 0 {
            o.max_work = 100_000_000;
        }
        if e.point_count() as i64 > o.max_centers {
            return Err(limit());
        }
        let mut radius = o.radius;
        if o.radius_mm != [0.0; 3] {
            if radius != [0; 3] {
                return Err(nc());
            }
            let spacing = [e.fixed.spacing.x, e.fixed.spacing.y, e.fixed.spacing.z];
            for a in 0..3 {
                let mm = o.radius_mm[a];
                if !finite(mm) || mm < 0.0 {
                    return Err(nc());
                }
                let samples = (mm / spacing[a]).floor();
                if !finite(samples) || samples > ((o.max_neighborhood_samples - 1) / 2) as f64 {
                    return Err(limit());
                }
                radius[a] = samples as i64;
            }
        }
        if radius == [0; 3] {
            return Err(nc());
        }
        let mut count = 1i64;
        for &r in &radius {
            if r < 0 {
                return Err(nc());
            }
            if r > (o.max_neighborhood_samples - 1) / 2 {
                return Err(limit());
            }
            let width = 2 * r + 1;
            if count > o.max_neighborhood_samples / width {
                return Err(limit());
            }
            count *= width;
        }
        let points = e.domain.points.as_ref().map_or(0, |p| p.len()) as i64;
        if points > o.max_working_bytes / 24 {
            return Err(limit());
        }
        let center_bytes = points * 24;
        if count > (o.max_working_bytes - center_bytes) / 64
            || e.point_count() as i64 > o.max_work / count
        {
            return Err(limit());
        }
        if let Some(p) = &e.domain.points
            && p.iter().any(|v| !finite_vec(*v))
        {
            return Err(nc());
        }
        Ok(NeighborhoodCorrelation {
            evaluator: e,
            options: o,
            radius,
            neighborhood: count,
            center_bytes,
        })
    }
    pub fn value_context(
        &self,
        ctx: Option<&dyn Cancellation>,
        transform: &dyn Transform,
    ) -> Result<f64, RegError> {
        self.evaluate(ctx, transform, false).map(|r| r.0)
    }
    pub fn value_and_derivative_context(
        &self,
        ctx: Option<&dyn Cancellation>,
        transform: &dyn Transform,
    ) -> Result<(f64, Vec<f64>), RegError> {
        self.evaluate(ctx, transform, true)
    }
    fn evaluate(
        &self,
        ctx: Option<&dyn Cancellation>,
        transform: &dyn Transform,
        derivatives: bool,
    ) -> Result<(f64, Vec<f64>), RegError> {
        check(ctx)?;
        let parameters = if derivatives {
            transform.number_of_parameters() as i64
        } else {
            0
        };
        if !(0..=4096).contains(&parameters) {
            return Err(limit());
        }
        let o = &self.options;
        if self.neighborhood > o.max_work / (parameters + 1) {
            return Err(limit());
        }
        let work_per_center = self.neighborhood * (parameters + 1);
        if self.evaluator.point_count() as i64 > o.max_work / work_per_center
            || parameters > (o.max_working_bytes - self.center_bytes - self.neighborhood * 64) / 40
        {
            return Err(limit());
        }
        let parameters = parameters as usize;
        let mut samples: Vec<Sample> = Vec::with_capacity(self.neighborhood as usize);
        let mut derivative = vec![0.0; parameters];
        let mut jacobian = Jacobian::default();
        let mut total = 0.0;
        let mut eligible = 0i64;
        let e = &self.evaluator;
        let r = self.radius;
        for center in 0..e.point_count() {
            check(ctx)?;
            let (point, value) = e.fixed_sample(center);
            if value.is_none() {
                continue;
            }
            if self.pair(point, transform, false)?.is_none() {
                continue;
            }
            let index = e.fixed.continuous_index(point);
            samples.clear();
            let (mut mean_f, mut mean_m) = (0.0, 0.0);
            for z in -r[2]..=r[2] {
                for y in -r[1]..=r[1] {
                    for x in -r[0]..=r[0] {
                        check(ctx)?;
                        let neighbor = e
                            .fixed
                            .point(index.add(Vec3::new(x as f64, y as f64, z as f64)));
                        let Some(s) = self.pair(neighbor, transform, derivatives)? else {
                            continue;
                        };
                        samples.push(s);
                        let n = samples.len() as f64;
                        mean_f += (s.fixed - mean_f) / n;
                        mean_m += (s.moving - mean_m) / n;
                    }
                }
            }
            if samples.len() < 2 {
                continue;
            }
            let (mut ff, mut mm, mut fm) = (0.0, 0.0, 0.0);
            for s in &samples {
                let (f, m) = (s.fixed - mean_f, s.moving - mean_m);
                ff += f * f;
                mm += m * m;
                fm += f * m;
            }
            let n = samples.len() as f64;
            if ff / n <= o.minimum_variance || mm / n <= o.minimum_variance {
                continue;
            }
            let rho = (fm / ff.sqrt()) / mm.sqrt();
            if !finite(rho) {
                return Err(nc());
            }
            total -= rho * rho;
            eligible += 1;
            if !derivatives {
                continue;
            }
            let scale = -2.0 * (fm / ff) / mm;
            for s in &samples {
                check(ctx)?;
                transform.jacobian_with_respect_to_parameters(s.fixed_point, &mut jacobian);
                if jacobian.columns != parameters || jacobian.values.len() != 3 * parameters {
                    return Err(nc());
                }
                let factor = scale * ((s.fixed - mean_f) - (fm / mm) * (s.moving - mean_m));
                for (p, d) in derivative.iter_mut().enumerate() {
                    let g = project(s.gradient, &jacobian, p);
                    if !finite(g) {
                        return Err(nc());
                    }
                    *d += factor * g;
                }
            }
        }
        check(ctx)?;
        if eligible == 0 {
            return Err(RegError::bare(Sentinel::TooFewSamples));
        }
        for d in derivative.iter_mut() {
            *d /= eligible as f64;
            if !finite(*d) {
                return Err(nc());
            }
        }
        Ok((total / eligible as f64, derivative))
    }
    fn pair(
        &self,
        point: Vec3,
        transform: &dyn Transform,
        gradients: bool,
    ) -> Result<Option<Sample>, RegError> {
        let e = &self.evaluator;
        let Some(fixed) = e.fixed.interpolate(point) else {
            return Ok(None);
        };
        if !mask_at(e.domain.fixed_mask, point)? {
            return Ok(None);
        }
        let mapped = transform.transform_point(point);
        if !finite_vec(mapped) {
            return Err(nc());
        }
        if !mask_at(e.domain.moving_mask, mapped)? {
            return Ok(None);
        }
        let index = e.moving.continuous_index(mapped);
        if !e.moving.inside(index) {
            return Ok(None);
        }
        let moving = e.moving.linear_at(index);
        if !finite(fixed) || !finite(moving) {
            return Err(RegError::new(
                Sentinel::NeighborhoodCorrelation,
                "nonfinite pair",
            ));
        }
        let mut s = Sample {
            fixed_point: point,
            fixed,
            moving,
            ..Default::default()
        };
        if gradients {
            s.gradient = e.moving.gradient_at(index);
            if !finite_vec(s.gradient) {
                return Err(nc());
            }
        }
        Ok(Some(s))
    }
}
pub(crate) fn mask_at(mask: Option<&Image>, point: Vec3) -> Result<bool, RegError> {
    let Some(mask) = mask else {
        return Ok(true);
    };
    let index = mask.continuous_index(point);
    if !mask.inside(index) {
        return Ok(false);
    }
    let at = |v: f64| (v + 0.5).floor() as usize;
    let value = f64::from(mask.at(at(index.x), at(index.y), at(index.z)));
    if !finite(value) {
        return Err(nc());
    }
    Ok(value != 0.0)
}
impl Metric for NeighborhoodCorrelation<'_> {
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
    fn value(&self, transform: &dyn Transform) -> Result<f64, RegError> {
        self.value_context(None, transform)
    }
    fn value_and_derivative(&self, transform: &dyn Transform) -> Result<(f64, Vec<f64>), RegError> {
        self.value_and_derivative_context(None, transform)
    }
}
