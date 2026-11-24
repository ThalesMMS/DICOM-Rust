use super::error::{RegError, Sentinel};
use super::image::Image;
use super::matrix::Jacobian;
use super::metric::{Evaluator, Metric, MetricDomain, project, reduce_metric_chunks};
use super::transform::Transform;
use crate::fmath;
use crate::render::Vec3;
const MAX_MATTES_ARRAY_BYTES: i64 = 256 << 20;
pub(crate) fn check_mattes_array_size(rows: i64, columns: i64) -> Result<(), RegError> {
    const FLOAT64_BYTES: i64 = 8;
    if rows < 1 || columns < 0 {
        return Err(RegError::new(
            Sentinel::MattesWorkspaceLimit,
            format!("invalid {rows} × {columns} array dimensions"),
        ));
    }
    if columns > i64::MAX / FLOAT64_BYTES / rows {
        return Err(RegError::new(
            Sentinel::MattesWorkspaceLimit,
            format!("{rows} × {columns} array byte size cannot be represented as int"),
        ));
    }
    if columns > MAX_MATTES_ARRAY_BYTES / FLOAT64_BYTES / rows {
        return Err(RegError::new(
            Sentinel::MattesWorkspaceLimit,
            format!("{rows} × {columns} array exceeds {MAX_MATTES_ARRAY_BYTES} bytes"),
        ));
    }
    Ok(())
}
pub const DEFAULT_MATTES_BINS: i64 = 50;
pub(crate) const EPSILON: f64 = 2.220446049250313e-16;
#[derive(Debug, Clone)]
pub struct MattesMutualInformation<'a> {
    pub(crate) evaluator: Evaluator<'a>,
    bins: usize,
    moving_min: f64,
    moving_max: f64,
    fixed_bin_size: f64,
    moving_bin_size: f64,
    fixed_normal_min: f64,
    moving_norm_min: f64,
}
pub(crate) trait LocalSupport {
    fn local_support(&self) -> bool;
    fn sparse_jacobian_at(&self, point: Vec3, out: &mut SparseJacobian);
}
#[derive(Debug, Clone, Default)]
pub(crate) struct SparseJacobian {
    pub n: usize,
    pub indices: Vec<usize>,
    pub weights: Vec<f64>,
    pub stride: usize,
    pub post: [[f64; 3]; 3],
}
pub(crate) type LocalLookup = fn(&dyn Transform) -> Option<Box<dyn LocalSupport + '_>>;
impl<'a> MattesMutualInformation<'a> {
    pub fn new(
        fixed: Option<&'a Image>,
        moving: Option<&'a Image>,
        domain: MetricDomain<'a>,
        bins: i64,
    ) -> Result<MattesMutualInformation<'a>, RegError> {
        let e = Evaluator::new(fixed, moving, domain)?;
        let bins = if bins == 0 { DEFAULT_MATTES_BINS } else { bins };
        if bins < 5 {
            return Err(RegError::with_text(
                Vec::new(),
                format!("registration: {bins} histogram bins, want at least 5"),
            ));
        }
        check_mattes_array_size(bins, bins)?;
        let (fixed_min, fixed_max) = fixed_range(e.fixed, &e.domain);
        let (moving_min, moving_max) = image_range(e.moving, e.domain.moving_mask);
        if !(fixed_max > fixed_min) || !(moving_max > moving_min) {
            return Err(RegError::new(
                Sentinel::Image,
                "constant intensities; mutual information is undefined",
            ));
        }
        let fixed_bin_size = (fixed_max - fixed_min) / (bins - 4) as f64;
        let moving_bin_size = (moving_max - moving_min) / (bins - 4) as f64;
        Ok(MattesMutualInformation {
            evaluator: e,
            bins: bins as usize,
            moving_min,
            moving_max,
            fixed_bin_size,
            moving_bin_size,
            fixed_normal_min: fixed_min / fixed_bin_size - 2.0,
            moving_norm_min: moving_min / moving_bin_size - 2.0,
        })
    }
    fn bins_of(&self, fixed: f64, moving: f64) -> (f64, usize, usize) {
        let moving_term = moving / self.moving_bin_size - self.moving_norm_min;
        let moving_bin = clamp_bin(moving_term, self.bins);
        let fixed_bin = clamp_bin(
            fixed / self.fixed_bin_size - self.fixed_normal_min,
            self.bins,
        );
        (moving_term, moving_bin, fixed_bin)
    }
    pub(crate) fn evaluate(
        &self,
        transform: &dyn Transform,
        derivatives: bool,
        local: Option<LocalLookup>,
    ) -> Result<(f64, Vec<f64>), RegError> {
        if derivatives
            && let Some(l) = local.and_then(|f| f(transform))
            && l.local_support()
        {
            return self.evaluate_local(transform, l.as_ref());
        }
        let bins = self.bins;
        let mut parameters = 0;
        if derivatives {
            parameters = transform.number_of_parameters();
            check_mattes_array_size((bins * bins) as i64, parameters as i64)?;
        }
        let mut count = 0usize;
        let mut joint = vec![0.0; bins * bins];
        let mut fixed = vec![0.0; bins];
        let mut dtotal = vec![0.0; bins * bins * parameters];
        let mut pjoint = vec![0.0; bins * bins];
        let mut pfixed = vec![0.0; bins];
        let mut pderiv = vec![0.0; bins * bins * parameters];
        let mut jacobian = Jacobian::default();
        let mut projected = vec![0.0; parameters];
        let e = &self.evaluator;
        reduce_metric_chunks(e.point_count(), |start, end| {
            let mut pcount = 0;
            for i in start..end {
                let Some(s) = e.sample_at(i, transform, derivatives) else {
                    continue;
                };
                if s.moving < self.moving_min || s.moving > self.moving_max {
                    continue;
                }
                pcount += 1;
                let (moving_term, moving_bin, fixed_bin) = self.bins_of(s.fixed, s.moving);
                pfixed[fixed_bin] += 1.0;
                if derivatives {
                    transform.jacobian_with_respect_to_parameters(s.fixed_point, &mut jacobian);
                    for (p, v) in projected.iter_mut().enumerate() {
                        *v = project(s.gradient, &jacobian, p);
                    }
                }
                for j in moving_bin - 1..=moving_bin + 2 {
                    let argument = j as f64 - moving_term;
                    let cell = fixed_bin * bins + j;
                    pjoint[cell] += cubic_bspline(argument);
                    if derivatives {
                        let slope = cubic_bspline_derivative(argument);
                        let row = &mut pderiv[cell * parameters..(cell + 1) * parameters];
                        for (r, g) in row.iter_mut().zip(&projected) {
                            *r += slope * g;
                        }
                    }
                }
            }
            count += pcount;
            for (t, p) in joint.iter_mut().zip(pjoint.iter_mut()) {
                *t += *p;
                *p = 0.0;
            }
            for (t, p) in fixed.iter_mut().zip(pfixed.iter_mut()) {
                *t += *p;
                *p = 0.0;
            }
            for (t, p) in dtotal.iter_mut().zip(pderiv.iter_mut()) {
                *t += *p;
                *p = 0.0;
            }
        });
        if count == 0 {
            return Err(RegError::bare(Sentinel::TooFewSamples));
        }
        let (sum, moving_marginal) = self.marginals(&joint)?;
        let mut information = 0.0;
        let mut derivative = vec![0.0; parameters];
        for i in 0..bins {
            let fixed_probability = fixed[i] / sum;
            if fixed_probability <= EPSILON {
                continue;
            }
            let log_fixed = fmath::log(fixed_probability);
            for j in 0..bins {
                let p = joint[i * bins + j] / sum;
                if moving_marginal[j] <= EPSILON || p <= EPSILON {
                    continue;
                }
                let ratio = fmath::log(p / moving_marginal[j]);
                information += p * (ratio - log_fixed);
                if derivatives {
                    let row = &dtotal[(i * bins + j) * parameters..(i * bins + j + 1) * parameters];
                    for (d, r) in derivative.iter_mut().zip(row) {
                        *d += ratio * r;
                    }
                }
            }
        }
        for d in derivative.iter_mut() {
            *d /= self.moving_bin_size * sum;
        }
        Ok((-information, derivative))
    }
    fn marginals(&self, joint: &[f64]) -> Result<(f64, Vec<f64>), RegError> {
        let bins = self.bins;
        let mut sum = 0.0;
        for v in joint {
            sum += v;
        }
        if sum < EPSILON {
            return Err(RegError::bare(Sentinel::TooFewSamples));
        }
        let mut moving_marginal = vec![0.0; bins];
        for i in 0..bins {
            for j in 0..bins {
                moving_marginal[j] += joint[i * bins + j] / sum;
            }
        }
        Ok((sum, moving_marginal))
    }
    fn evaluate_local(
        &self,
        transform: &dyn Transform,
        local: &dyn LocalSupport,
    ) -> Result<(f64, Vec<f64>), RegError> {
        let parameters = transform.number_of_parameters();
        check_mattes_array_size(1, parameters as i64)?;
        let bins = self.bins;
        let mut count = 0usize;
        let mut joint = vec![0.0; bins * bins];
        let mut fixed = vec![0.0; bins];
        self.evaluator.reduce_chunks(
            transform,
            false,
            |samples| {
                let mut pcount = 0;
                let mut pjoint = vec![0.0; bins * bins];
                let mut pfixed = vec![0.0; bins];
                for s in samples {
                    if s.moving < self.moving_min || s.moving > self.moving_max {
                        continue;
                    }
                    pcount += 1;
                    let (moving_term, moving_bin, fixed_bin) = self.bins_of(s.fixed, s.moving);
                    pfixed[fixed_bin] += 1.0;
                    for j in moving_bin - 1..=moving_bin + 2 {
                        pjoint[fixed_bin * bins + j] += cubic_bspline(j as f64 - moving_term);
                    }
                }
                (pcount, pjoint, pfixed)
            },
            |(pcount, pjoint, pfixed)| {
                count += pcount;
                for (t, p) in joint.iter_mut().zip(&pjoint) {
                    *t += p;
                }
                for (t, p) in fixed.iter_mut().zip(&pfixed) {
                    *t += p;
                }
            },
        );
        if count == 0 {
            return Err(RegError::bare(Sentinel::TooFewSamples));
        }
        let (sum, moving_marginal) = self.marginals(&joint)?;
        let mut ratio = vec![0.0; bins * bins];
        let mut information = 0.0;
        for i in 0..bins {
            let fixed_probability = fixed[i] / sum;
            if fixed_probability <= EPSILON {
                continue;
            }
            let log_fixed = fmath::log(fixed_probability);
            for j in 0..bins {
                let p = joint[i * bins + j] / sum;
                if moving_marginal[j] <= EPSILON || p <= EPSILON {
                    continue;
                }
                let r = fmath::log(p / moving_marginal[j]);
                ratio[i * bins + j] = r;
                information += p * (r - log_fixed);
            }
        }
        let mut derivative = vec![0.0; parameters];
        let mut sparse = SparseJacobian::default();
        self.evaluator.reduce_chunks(
            transform,
            true,
            |samples| {
                let mut partial = vec![0.0; parameters];
                for s in samples {
                    if s.moving < self.moving_min || s.moving > self.moving_max {
                        continue;
                    }
                    let (moving_term, moving_bin, fixed_bin) = self.bins_of(s.fixed, s.moving);
                    let mut weight = 0.0;
                    for j in moving_bin - 1..=moving_bin + 2 {
                        let r = ratio[fixed_bin * bins + j];
                        if r != 0.0 {
                            weight += r * cubic_bspline_derivative(j as f64 - moving_term);
                        }
                    }
                    if weight == 0.0 {
                        continue;
                    }
                    local.sparse_jacobian_at(s.fixed_point, &mut sparse);
                    if sparse.n == 0 {
                        continue;
                    }
                    let mut g = [0f64; 3];
                    for (d, gd) in g.iter_mut().enumerate() {
                        *gd = weight
                            * (s.gradient.x * sparse.post[0][d]
                                + s.gradient.y * sparse.post[1][d]
                                + s.gradient.z * sparse.post[2][d]);
                    }
                    for k in 0..sparse.n {
                        let (at, w) = (sparse.indices[k], sparse.weights[k]);
                        partial[at] += w * g[0];
                        partial[sparse.stride + at] += w * g[1];
                        partial[2 * sparse.stride + at] += w * g[2];
                    }
                }
                partial
            },
            |partial| {
                for (d, p) in derivative.iter_mut().zip(&partial) {
                    *d += p;
                }
            },
        );
        for d in derivative.iter_mut() {
            *d /= self.moving_bin_size * sum;
        }
        Ok((-information, derivative))
    }
}
impl Metric for MattesMutualInformation<'_> {
    fn value(&self, transform: &dyn Transform) -> Result<f64, RegError> {
        self.evaluate(transform, false, super::local_lookup())
            .map(|r| r.0)
    }
    fn value_and_derivative(&self, transform: &dyn Transform) -> Result<(f64, Vec<f64>), RegError> {
        self.evaluate(transform, true, super::local_lookup())
    }
}
fn image_range(im: &Image, mask: Option<&Image>) -> (f64, f64) {
    let (mut low, mut high) = (f64::INFINITY, f64::NEG_INFINITY);
    for (i, &v) in im.data.iter().enumerate() {
        if let Some(mask) = mask {
            let (x, y, z) = (
                i % im.size[0],
                i / im.size[0] % im.size[1],
                i / (im.size[0] * im.size[1]),
            );
            if !mask.inside_mask(im.point(Vec3::new(x as f64, y as f64, z as f64))) {
                continue;
            }
        }
        low = fmath::min(low, f64::from(v));
        high = fmath::max(high, f64::from(v));
    }
    (low, high)
}
fn fixed_range(fixed: &Image, domain: &MetricDomain) -> (f64, f64) {
    let Some(points) = &domain.points else {
        return image_range(fixed, domain.fixed_mask);
    };
    let (mut low, mut high) = (f64::INFINITY, f64::NEG_INFINITY);
    for &point in points {
        let index = fixed.continuous_index(point);
        let x = fmath::f64_to_i64(index.x.round());
        let y = fmath::f64_to_i64(index.y.round());
        let z = fmath::f64_to_i64(index.z.round());
        let size = fixed.size;
        if x < 0
            || y < 0
            || z < 0
            || x >= size[0] as i64
            || y >= size[1] as i64
            || z >= size[2] as i64
        {
            continue;
        }
        let v = f64::from(fixed.at(x as usize, y as usize, z as usize));
        low = fmath::min(low, v);
        high = fmath::max(high, v);
    }
    (low, high)
}
fn clamp_bin(term: f64, bins: usize) -> usize {
    let bin = fmath::f64_to_i64(term);
    if bin < 2 {
        return 2;
    }
    if bin > bins as i64 - 3 {
        return bins - 3;
    }
    bin as usize
}
pub(crate) fn cubic_bspline(u: f64) -> f64 {
    let a = u.abs();
    if a < 1.0 {
        (4.0 - 6.0 * a * a + 3.0 * a * a * a) / 6.0
    } else if a < 2.0 {
        (8.0 - 12.0 * a + 6.0 * a * a - a * a * a) / 6.0
    } else {
        0.0
    }
}
pub(crate) fn cubic_bspline_derivative(u: f64) -> f64 {
    if (0.0..1.0).contains(&u) {
        -2.0 * u + 1.5 * u * u
    } else if u > -1.0 && u < 0.0 {
        -2.0 * u - 1.5 * u * u
    } else if (1.0..2.0).contains(&u) {
        -2.0 + 2.0 * u - 0.5 * u * u
    } else if u > -2.0 && u <= -1.0 {
        2.0 + 2.0 * u + 0.5 * u * u
    } else {
        0.0
    }
}
