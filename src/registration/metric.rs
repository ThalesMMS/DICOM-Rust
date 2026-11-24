use super::error::{RegError, Sentinel};
use super::image::Image;
use super::matrix::Jacobian;
use super::transform::Transform;
use crate::render::Vec3;
#[derive(Debug, Clone, Default)]
pub struct MetricDomain<'a> {
    pub fixed_mask: Option<&'a Image>,
    pub moving_mask: Option<&'a Image>,
    pub points: Option<Vec<Vec3>>,
    pub workers: i64,
}
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Sample {
    pub fixed_point: Vec3,
    pub fixed: f64,
    pub moving: f64,
    pub gradient: Vec3,
}
pub(crate) const METRIC_CHUNK: usize = 4096;
#[derive(Debug, Clone)]
pub(crate) struct Evaluator<'a> {
    pub fixed: &'a Image,
    pub moving: &'a Image,
    pub domain: MetricDomain<'a>,
}
impl<'a> Evaluator<'a> {
    pub fn new(
        fixed: Option<&'a Image>,
        moving: Option<&'a Image>,
        domain: MetricDomain<'a>,
    ) -> Result<Evaluator<'a>, RegError> {
        match (fixed, moving) {
            (Some(fixed), Some(moving)) => Ok(Evaluator {
                fixed,
                moving,
                domain,
            }),
            _ => Err(RegError::new(Sentinel::Image, "nil fixed or moving image")),
        }
    }
    pub(crate) fn point_count(&self) -> usize {
        match &self.domain.points {
            Some(p) => p.len(),
            None => self.fixed.data.len(),
        }
    }
    pub(crate) fn fixed_sample(&self, i: usize) -> (Vec3, Option<f64>) {
        let (point, value) = match &self.domain.points {
            Some(points) => {
                let point = points[i];
                match self.fixed.interpolate(point) {
                    Some(v) => (point, v),
                    None => return (point, None),
                }
            }
            None => {
                let size = self.fixed.size;
                let (x, y, z) = (i % size[0], i / size[0] % size[1], i / (size[0] * size[1]));
                let point = self.fixed.point(Vec3::new(x as f64, y as f64, z as f64));
                (point, f64::from(self.fixed.data[i]))
            }
        };
        if let Some(mask) = self.domain.fixed_mask
            && !mask.inside_mask(point)
        {
            return (point, None);
        }
        (point, Some(value))
    }
    pub fn sample_at(
        &self,
        i: usize,
        transform: &dyn Transform,
        gradients: bool,
    ) -> Option<Sample> {
        let (point, fixed) = self.fixed_sample(i);
        let fixed = fixed?;
        let mapped = transform.transform_point(point);
        let index = self.moving.continuous_index(mapped);
        if !self.moving.inside(index) {
            return None;
        }
        if let Some(mask) = self.domain.moving_mask
            && !mask.inside_mask(mapped)
        {
            return None;
        }
        let mut s = Sample {
            fixed_point: point,
            fixed,
            ..Default::default()
        };
        if gradients {
            (s.moving, s.gradient) = self.moving.linear_with_gradient_at(index);
        } else {
            s.moving = self.moving.linear_at(index);
        }
        Some(s)
    }
    pub(crate) fn reduce_chunks<R>(
        &self,
        transform: &dyn Transform,
        gradients: bool,
        mut visit: impl FnMut(&[Sample]) -> R,
        mut merge: impl FnMut(R),
    ) {
        let mut buffer = Vec::with_capacity(METRIC_CHUNK);
        reduce_metric_chunks(self.point_count(), |start, end| {
            buffer.clear();
            for i in start..end {
                if let Some(s) = self.sample_at(i, transform, gradients) {
                    buffer.push(s);
                }
            }
            merge(visit(&buffer));
        });
    }
}
pub(crate) fn reduce_metric_chunks(count: usize, mut chunk: impl FnMut(usize, usize)) {
    let mut start = 0;
    while start < count {
        let end = start + METRIC_CHUNK.min(count - start);
        chunk(start, end);
        start = end;
    }
}
impl Image {
    pub(crate) fn inside_mask(&self, point: Vec3) -> bool {
        let index = self.continuous_index(point);
        let mut at = [0usize; 3];
        for (axis, value) in [index.x, index.y, index.z].into_iter().enumerate() {
            let shifted = value + 0.5;
            if !(shifted >= 0.0 && shifted < self.size[axis] as f64) {
                return false;
            }
            at[axis] = shifted as usize;
        }
        self.at(at[0], at[1], at[2]) != 0.0
    }
    pub(crate) fn linear_with_gradient_at(&self, index: Vec3) -> (f64, Vec3) {
        let (x, y, z) = (index.x as i64, index.y as i64, index.z as i64);
        let (tx, ty, tz) = (index.x - x as f64, index.y - y as f64, index.z - z as f64);
        const MIN_FRACTION: f64 = 8.636168555094445e-78;
        if !(tx > MIN_FRACTION && ty > MIN_FRACTION && tz > MIN_FRACTION)
            || x >= self.size[0] as i64 - 1
            || y >= self.size[1] as i64 - 1
            || z >= self.size[2] as i64 - 1
        {
            return (self.linear_at(index), self.gradient_at(index));
        }
        let row = self.size[0];
        let plane = row * self.size[1];
        let base = x as usize + y as usize * row + z as usize * plane;
        let d = |i: usize| f64::from(self.data[i]);
        let (a, b) = (d(base), d(base + 1));
        let (c, dd) = (d(base + row), d(base + row + 1));
        let (e, f) = (d(base + plane), d(base + plane + 1));
        let (g, h) = (d(base + plane + row), d(base + plane + row + 1));
        let (sx, sy, sz) = (1.0 - tx, 1.0 - ty, 1.0 - tz);
        let (xy00, xy10, xy01, xy11) = (sx * sy, tx * sy, sx * ty, tx * ty);
        let value = xy00 * sz * a
            + xy10 * sz * b
            + xy01 * sz * c
            + xy11 * sz * dd
            + xy00 * tz * e
            + xy10 * tz * f
            + xy01 * tz * g
            + xy11 * tz * h;
        let (yz00, yz10, yz01, yz11) = (sy * sz, ty * sz, sy * tz, ty * tz);
        let (xz00, xz10, xz01, xz11) = (sx * sz, tx * sz, sx * tz, tx * tz);
        let dx = (yz00 * b + yz10 * dd + yz01 * f + yz11 * h)
            - (yz00 * a + yz10 * c + yz01 * e + yz11 * g);
        let dy = (xz00 * c + xz10 * dd + xz01 * g + xz11 * h)
            - (xz00 * a + xz10 * b + xz01 * e + xz11 * f);
        let dz = (xy00 * e + xy10 * f + xy01 * g + xy11 * h)
            - (xy00 * a + xy10 * b + xy01 * c + xy11 * dd);
        (
            value,
            self.physical_to_index
                .transpose()
                .mul_vec(Vec3::new(dx, dy, dz)),
        )
    }
    pub(crate) fn gradient_at(&self, index: Vec3) -> Vec3 {
        let mut per_index = [0f64; 3];
        let position = [index.x, index.y, index.z];
        let at = |p: [f64; 3]| self.linear_at(Vec3::new(p[0], p[1], p[2]));
        for axis in 0..3 {
            let last = (self.size[axis] - 1) as f64;
            let value = position[axis];
            if self.size[axis] == 1 || value < 0.0 || value > last {
                continue;
            }
            let (mut low, mut high) = (position, position);
            let floor = value.floor();
            if floor == value {
                low[axis] = value - 1.0;
                high[axis] = value + 1.0;
                per_index[axis] = (at(high) - at(low)) / 2.0;
                continue;
            }
            low[axis] = floor;
            high[axis] = floor + 1.0;
            per_index[axis] = at(high) - at(low);
        }
        self.physical_to_index.transpose().mul_vec(Vec3::new(
            per_index[0],
            per_index[1],
            per_index[2],
        ))
    }
}
pub(crate) fn accumulate_derivative(
    derivative: &mut [f64],
    jacobian: &mut Jacobian,
    transform: &dyn Transform,
    s: &Sample,
    weight: f64,
) {
    transform.jacobian_with_respect_to_parameters(s.fixed_point, jacobian);
    for (p, d) in derivative.iter_mut().enumerate() {
        *d += weight * project(s.gradient, jacobian, p);
    }
}
pub(crate) fn project(g: Vec3, jacobian: &Jacobian, p: usize) -> f64 {
    g.x * jacobian.at(0, p) + g.y * jacobian.at(1, p) + g.z * jacobian.at(2, p)
}
pub trait Metric {
    fn value(&self, transform: &dyn Transform) -> Result<f64, RegError>;
    fn value_and_derivative(&self, transform: &dyn Transform) -> Result<(f64, Vec<f64>), RegError>;
    fn context_value(
        &self,
        _ctx: Option<&dyn crate::render::parallel::Cancellation>,
        _transform: &dyn Transform,
    ) -> Option<Result<f64, RegError>> {
        None
    }
    fn context_value_and_derivative(
        &self,
        _ctx: Option<&dyn crate::render::parallel::Cancellation>,
        _transform: &dyn Transform,
    ) -> Option<Result<(f64, Vec<f64>), RegError>> {
        None
    }
}
#[derive(Debug, Clone)]
pub struct MeanSquares<'a> {
    evaluator: Evaluator<'a>,
}
impl<'a> MeanSquares<'a> {
    pub fn new(
        fixed: Option<&'a Image>,
        moving: Option<&'a Image>,
        domain: MetricDomain<'a>,
    ) -> Result<MeanSquares<'a>, RegError> {
        Ok(MeanSquares {
            evaluator: Evaluator::new(fixed, moving, domain)?,
        })
    }
    fn evaluate(
        &self,
        transform: &dyn Transform,
        derivatives: bool,
    ) -> Result<(f64, Vec<f64>), RegError> {
        let parameters = transform.number_of_parameters();
        let mut count = 0usize;
        let mut sum = 0.0;
        let mut derivative = vec![0.0; parameters];
        let mut jacobian = Jacobian::default();
        self.evaluator.reduce_chunks(
            transform,
            derivatives,
            |samples| {
                let mut partial_sum = 0.0;
                let mut partial = vec![0.0; if derivatives { parameters } else { 0 }];
                for s in samples {
                    let difference = s.moving - s.fixed;
                    partial_sum += difference * difference;
                    if derivatives {
                        accumulate_derivative(
                            &mut partial,
                            &mut jacobian,
                            transform,
                            s,
                            difference,
                        );
                    }
                }
                (samples.len(), partial_sum, partial)
            },
            |(n, s, partial)| {
                count += n;
                sum += s;
                for (d, p) in derivative.iter_mut().zip(&partial) {
                    *d += p;
                }
            },
        );
        if count == 0 {
            return Err(RegError::bare(Sentinel::TooFewSamples));
        }
        for d in derivative.iter_mut() {
            *d *= 2.0 / count as f64;
        }
        Ok((sum / count as f64, derivative))
    }
}
impl Metric for MeanSquares<'_> {
    fn value(&self, transform: &dyn Transform) -> Result<f64, RegError> {
        self.evaluate(transform, false).map(|r| r.0)
    }
    fn value_and_derivative(&self, transform: &dyn Transform) -> Result<(f64, Vec<f64>), RegError> {
        self.evaluate(transform, true)
    }
}
#[derive(Debug, Clone, Default)]
pub(crate) struct CorrelationPartial {
    pub count: usize,
    pub f: f64,
    pub m: f64,
    pub ff: f64,
    pub mm: f64,
    pub fm: f64,
    pub g: Vec<f64>,
    pub fg: Vec<f64>,
    pub mg: Vec<f64>,
}
impl CorrelationPartial {
    pub fn new(parameters: usize) -> CorrelationPartial {
        CorrelationPartial {
            g: vec![0.0; parameters],
            fg: vec![0.0; parameters],
            mg: vec![0.0; parameters],
            ..Default::default()
        }
    }
}
#[derive(Debug, Clone)]
pub struct Correlation<'a> {
    evaluator: Evaluator<'a>,
}
impl<'a> Correlation<'a> {
    pub fn new(
        fixed: Option<&'a Image>,
        moving: Option<&'a Image>,
        domain: MetricDomain<'a>,
    ) -> Result<Correlation<'a>, RegError> {
        Ok(Correlation {
            evaluator: Evaluator::new(fixed, moving, domain)?,
        })
    }
    fn evaluate(
        &self,
        transform: &dyn Transform,
        derivatives: bool,
    ) -> Result<(f64, Vec<f64>), RegError> {
        let parameters = transform.number_of_parameters();
        let width = if derivatives { parameters } else { 0 };
        let mut total = CorrelationPartial::new(width);
        let mut jacobian = Jacobian::default();
        self.evaluator.reduce_chunks(
            transform,
            derivatives,
            |samples| {
                let mut partial = CorrelationPartial::new(width);
                partial.count = samples.len();
                for s in samples {
                    partial.f += s.fixed;
                    partial.m += s.moving;
                    partial.ff += s.fixed * s.fixed;
                    partial.mm += s.moving * s.moving;
                    partial.fm += s.fixed * s.moving;
                    if derivatives {
                        transform.jacobian_with_respect_to_parameters(s.fixed_point, &mut jacobian);
                        for p in 0..parameters {
                            let g = project(s.gradient, &jacobian, p);
                            partial.g[p] += g;
                            partial.fg[p] += s.fixed * g;
                            partial.mg[p] += s.moving * g;
                        }
                    }
                }
                partial
            },
            |partial| {
                total.count += partial.count;
                total.f += partial.f;
                total.m += partial.m;
                total.ff += partial.ff;
                total.mm += partial.mm;
                total.fm += partial.fm;
                for p in 0..partial.g.len() {
                    total.g[p] += partial.g[p];
                    total.fg[p] += partial.fg[p];
                    total.mg[p] += partial.mg[p];
                }
            },
        );
        if total.count < 2 {
            return Err(RegError::bare(Sentinel::TooFewSamples));
        }
        let n = total.count as f64;
        let (mean_f, mean_m) = (total.f / n, total.m / n);
        let sff = total.ff - n * mean_f * mean_f;
        let smm = total.mm - n * mean_m * mean_m;
        let sfm = total.fm - n * mean_f * mean_m;
        if sff <= 0.0 || smm <= 0.0 {
            return Err(RegError::bare(Sentinel::TooFewSamples));
        }
        let value = -sfm * sfm / (sff * smm);
        if !derivatives {
            return Ok((value, Vec::new()));
        }
        let scale = -2.0 * sfm / (sff * smm);
        let derivative = (0..parameters)
            .map(|p| {
                let sum_fg = total.fg[p] - mean_f * total.g[p];
                let sum_mg = total.mg[p] - mean_m * total.g[p];
                scale * (sum_fg - sfm / smm * sum_mg)
            })
            .collect();
        Ok((value, derivative))
    }
}
impl Metric for Correlation<'_> {
    fn value(&self, transform: &dyn Transform) -> Result<f64, RegError> {
        self.evaluate(transform, false).map(|r| r.0)
    }
    fn value_and_derivative(&self, transform: &dyn Transform) -> Result<(f64, Vec<f64>), RegError> {
        self.evaluate(transform, true)
    }
}
pub struct Objective<'m, 't> {
    pub metric: &'m dyn Metric,
    pub transform: &'t mut dyn Transform,
}
impl Objective<'_, '_> {
    pub fn value(&mut self, x: &[f64]) -> Result<f64, RegError> {
        self.transform.set_parameters(x)?;
        self.metric.value(&*self.transform)
    }
    pub fn value_and_gradient(&mut self, x: &[f64]) -> Result<(f64, Vec<f64>), RegError> {
        self.transform.set_parameters(x)?;
        self.metric.value_and_derivative(&*self.transform)
    }
}
