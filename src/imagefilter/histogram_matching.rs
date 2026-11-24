use super::volume::{Ctx, ErrorKind, FilterError, Volume, ctx_err};
use crate::fmath;
use crate::numtext::format_float;
use crate::textfmt::float_v;
pub const DEFAULT_HISTOGRAM_MATCH_LEVELS: i64 = 1024;
pub const DEFAULT_HISTOGRAM_MATCH_POINTS: i64 = 7;
pub const MAX_HISTOGRAM_MATCH_LEVELS: i64 = 1 << 20;
pub const MAX_HISTOGRAM_MATCH_POINTS: i64 = 1 << 12;
fn invalid(detail: impl AsRef<str>) -> FilterError {
    FilterError::new(
        ErrorKind::InvalidHistogramMatch,
        format!(
            "imagefilter: invalid histogram matching input: {}",
            detail.as_ref()
        ),
    )
}
fn prefixed(prefix: &str, e: FilterError) -> FilterError {
    FilterError::new(e.kind(), format!("{prefix}: {}", e.message()))
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct HistogramOptions {
    pub levels: i64,
    pub include_background: bool,
}
impl HistogramOptions {
    fn resolved_levels(&self) -> Result<usize, FilterError> {
        let levels = if self.levels == 0 {
            DEFAULT_HISTOGRAM_MATCH_LEVELS
        } else {
            self.levels
        };
        if !(1..=MAX_HISTOGRAM_MATCH_LEVELS).contains(&levels) {
            return Err(invalid(format!(
                "Levels={} outside 1..{MAX_HISTOGRAM_MATCH_LEVELS}",
                self.levels
            )));
        }
        Ok(levels as usize)
    }
}
#[derive(Debug, Clone, PartialEq, Default)]
pub struct IntensityHistogram {
    pub min: f64,
    pub max: f64,
    pub mean: f64,
    pub threshold: f64,
    pub counts: Vec<u64>,
    pub samples: u64,
    pub counted: u64,
}
pub fn new_intensity_histogram(
    samples: &[f32],
    opts: HistogramOptions,
) -> Result<IntensityHistogram, FilterError> {
    let levels = opts.resolved_levels()?;
    let mut h = IntensityHistogram {
        min: f64::INFINITY,
        max: f64::NEG_INFINITY,
        ..Default::default()
    };
    let mut sum = 0.0;
    for s in samples {
        let v = f64::from(*s);
        if !v.is_finite() {
            continue;
        }
        h.samples += 1;
        sum += v;
        h.min = fmath::min(h.min, v);
        h.max = fmath::max(h.max, v);
    }
    if h.samples == 0 {
        return Err(invalid("no finite samples"));
    }
    if !(h.max > h.min) {
        return Err(invalid(format!("every sample equals {}", float_v(h.min))));
    }
    h.mean = sum / h.samples as f64;
    h.mean = fmath::min(fmath::max(h.mean, h.min), h.max);
    h.threshold = if opts.include_background {
        h.min
    } else {
        h.mean
    };
    if !(h.max > h.threshold) {
        return Err(invalid(format!(
            "no intensity range above the threshold {}",
            float_v(h.threshold)
        )));
    }
    h.counts = vec![0; levels];
    let scale = levels as f64 / (h.max - h.threshold);
    for s in samples {
        let v = f64::from(*s);
        if !(v >= h.threshold && v <= h.max) {
            continue;
        }
        let mut bin = fmath::f64_to_i64((v - h.threshold) * scale);
        if bin >= levels as i64 {
            bin = levels as i64 - 1;
        }
        h.counts[bin as usize] += 1;
        h.counted += 1;
    }
    Ok(h)
}
impl IntensityHistogram {
    fn validate(&self) -> Result<(), FilterError> {
        for v in [self.min, self.max, self.threshold] {
            if !v.is_finite() {
                return Err(invalid("non-finite histogram bounds"));
            }
        }
        if !(self.min <= self.threshold && self.threshold < self.max) {
            return Err(invalid(format!(
                "histogram bounds min {}, threshold {}, max {}",
                float_v(self.min),
                float_v(self.threshold),
                float_v(self.max)
            )));
        }
        if self.counts.is_empty() || self.counts.len() as i64 > MAX_HISTOGRAM_MATCH_LEVELS {
            return Err(invalid(format!("{} histogram bins", self.counts.len())));
        }
        let mut total: u64 = 0;
        for c in &self.counts {
            total = total.wrapping_add(*c);
        }
        if total == 0 {
            return Err(invalid("empty histogram"));
        }
        Ok(())
    }
    fn bin_min(&self, i: usize) -> f64 {
        self.threshold + i as f64 * (self.max - self.threshold) / self.counts.len() as f64
    }
    fn bin_max(&self, i: usize) -> f64 {
        if i == self.counts.len() - 1 {
            return self.max;
        }
        self.bin_min(i + 1)
    }
    pub fn quantile(&self, p: f64) -> f64 {
        if self.counts.is_empty() {
            return f64::NAN;
        }
        let p = fmath::min(fmath::max(p, 0.0), 1.0);
        let mut total = 0.0;
        for c in &self.counts {
            total += *c as f64;
        }
        if total == 0.0 {
            return f64::NAN;
        }
        let size = self.counts.len();
        let mut cumulated = 0.0;
        let (mut fn_, mut pn_prev);
        if p < 0.5 {
            let (mut n, mut pn) = (0usize, 0.0);
            loop {
                fn_ = self.counts[n] as f64;
                cumulated += fn_;
                pn_prev = pn;
                pn = cumulated / total;
                n += 1;
                if !(n < size && pn < p) {
                    break;
                }
            }
            let (lo, hi) = (self.bin_min(n - 1), self.bin_max(n - 1));
            if fn_ == 0.0 {
                return lo;
            }
            return lo + ((p - pn_prev) / (fn_ / total)) * (hi - lo);
        }
        let (mut n, mut m, mut pn) = (size as i64 - 1, 0usize, 1.0);
        loop {
            fn_ = self.counts[n as usize] as f64;
            cumulated += fn_;
            pn_prev = pn;
            pn = 1.0 - cumulated / total;
            n -= 1;
            m += 1;
            if !(m < size && pn > p) {
                break;
            }
        }
        let (lo, hi) = (
            self.bin_min((n + 1) as usize),
            self.bin_max((n + 1) as usize),
        );
        if fn_ == 0.0 {
            return hi;
        }
        hi - ((pn_prev - p) / (fn_ / total)) * (hi - lo)
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct HistogramMatchOptions {
    pub histogram: HistogramOptions,
    pub match_points: i64,
    pub workers: i64,
}
fn match_points(points: i64) -> Result<usize, FilterError> {
    let p = if points == 0 {
        DEFAULT_HISTOGRAM_MATCH_POINTS
    } else {
        points
    };
    if !(1..=MAX_HISTOGRAM_MATCH_POINTS).contains(&p) {
        return Err(invalid(format!(
            "MatchPoints={points} outside 1..{MAX_HISTOGRAM_MATCH_POINTS}"
        )));
    }
    Ok(p as usize)
}
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct HistogramLandmark {
    pub fraction: f64,
    pub source: f64,
    pub reference: f64,
}
#[derive(Debug, Clone, PartialEq)]
pub struct HistogramMatch {
    source: Vec<f64>,
    reference: Vec<f64>,
    fractions: Vec<f64>,
    gradients: Vec<f64>,
    source_min: f64,
    source_max: f64,
    ref_min: f64,
    ref_max: f64,
    lower_grad: f64,
    upper_grad: f64,
    source_hist: IntensityHistogram,
    ref_hist: IntensityHistogram,
}
fn slope(rise: f64, run: f64) -> f64 {
    if run.abs() <= 4.0 * 5e-324 || run.abs() <= 1e-12 * fmath::max(1.0, rise.abs()) {
        return 0.0;
    }
    rise / run
}
pub fn new_histogram_match(
    source: Option<&IntensityHistogram>,
    reference: Option<&IntensityHistogram>,
    points: i64,
) -> Result<HistogramMatch, FilterError> {
    let points = match_points(points)?;
    let nil = || invalid("nil histogram");
    let source = source
        .ok_or_else(nil)
        .and_then(|s| s.validate().map(|_| s))
        .map_err(|e| prefixed("source", e))?;
    let reference = reference
        .ok_or_else(nil)
        .and_then(|s| s.validate().map(|_| s))
        .map_err(|e| prefixed("reference", e))?;
    let n = points + 2;
    let mut m = HistogramMatch {
        source: vec![0.0; n],
        reference: vec![0.0; n],
        fractions: vec![0.0; n],
        gradients: vec![0.0; n - 1],
        source_min: source.min,
        source_max: source.max,
        ref_min: reference.min,
        ref_max: reference.max,
        lower_grad: 0.0,
        upper_grad: 0.0,
        source_hist: source.clone(),
        ref_hist: reference.clone(),
    };
    m.source[0] = source.threshold;
    m.reference[0] = reference.threshold;
    m.source[n - 1] = source.max;
    m.reference[n - 1] = reference.max;
    m.fractions[n - 1] = 1.0;
    let delta = 1.0 / (points + 1) as f64;
    for j in 1..=points {
        let p = j as f64 * delta;
        m.fractions[j] = p;
        m.source[j] = source.quantile(p);
        m.reference[j] = reference.quantile(p);
    }
    for j in 1..n {
        m.source[j] = fmath::max(m.source[j], m.source[j - 1]);
        m.reference[j] = fmath::max(m.reference[j], m.reference[j - 1]);
    }
    for j in 0..n - 1 {
        m.gradients[j] = slope(
            m.reference[j + 1] - m.reference[j],
            m.source[j + 1] - m.source[j],
        );
    }
    m.lower_grad = slope(m.reference[0] - m.ref_min, m.source[0] - m.source_min);
    Ok(m)
}
pub fn new_histogram_match_from_samples(
    source: &[f32],
    reference: &[f32],
    opts: HistogramMatchOptions,
) -> Result<HistogramMatch, FilterError> {
    let points = match_points(opts.match_points)?;
    let src = new_intensity_histogram(source, opts.histogram).map_err(|e| prefixed("source", e))?;
    let rf =
        new_intensity_histogram(reference, opts.histogram).map_err(|e| prefixed("reference", e))?;
    new_histogram_match(Some(&src), Some(&rf), points as i64)
}
impl HistogramMatch {
    pub fn map(&self, v: f64) -> f64 {
        if !v.is_finite() {
            return v;
        }
        let n = self.source.len();
        let j = self.source.partition_point(|s| !(v < *s));
        if j == 0 {
            self.ref_min + (v - self.source_min) * self.lower_grad
        } else if j == n {
            self.ref_max + (v - self.source_max) * self.upper_grad
        } else {
            self.reference[j - 1] + (v - self.source[j - 1]) * self.gradients[j - 1]
        }
    }
    pub fn landmarks(&self) -> Vec<HistogramLandmark> {
        (0..self.source.len())
            .map(|i| HistogramLandmark {
                fraction: self.fractions[i],
                source: self.source[i],
                reference: self.reference[i],
            })
            .collect()
    }
    pub fn match_points(&self) -> i64 {
        self.source.len() as i64 - 2
    }
    pub fn source_histogram(&self) -> &IntensityHistogram {
        &self.source_hist
    }
    pub fn reference_histogram(&self) -> &IntensityHistogram {
        &self.ref_hist
    }
    pub fn summary(&self) -> String {
        let g = |x: f64| format_float(x, 'g', 6);
        format!(
            "histogram match: {} landmarks, source [{}, {}] threshold {} -> reference [{}, {}] threshold {}",
            self.match_points(),
            g(self.source_min),
            g(self.source_max),
            g(self.source[0]),
            g(self.ref_min),
            g(self.ref_max),
            g(self.reference[0])
        )
    }
    pub fn apply(&self, ctx: Ctx<'_>, dst: &mut [f32], src: &[f32]) -> Result<(), FilterError> {
        if dst.len() != src.len() {
            return Err(invalid(format!(
                "{} destination and {} source samples",
                dst.len(),
                src.len()
            )));
        }
        const CHUNK: usize = 1 << 16;
        for c in 0..src.len().div_ceil(CHUNK) {
            ctx_err(ctx)?;
            let (lo, hi) = (c * CHUNK, ((c + 1) * CHUNK).min(src.len()));
            for i in lo..hi {
                dst[i] = self.map(f64::from(src[i])) as f32;
            }
        }
        ctx_err(ctx)
    }
    #[doc(hidden)]
    pub fn apply_in_place(&self, ctx: Ctx<'_>, buf: &mut [f32]) -> Result<(), FilterError> {
        let src = buf.to_vec();
        self.apply(ctx, buf, &src)
    }
}
#[doc(hidden)]
pub const HISTOGRAM_MATCH_NONE: &str = "histogram match: none";
pub fn match_histograms(
    ctx: Ctx<'_>,
    source: &Volume,
    reference: &Volume,
    opts: HistogramMatchOptions,
) -> Result<(Volume, HistogramMatch), FilterError> {
    source.validate().map_err(|e| prefixed("source", e))?;
    reference.validate().map_err(|e| prefixed("reference", e))?;
    let m = new_histogram_match_from_samples(&source.data, &reference.data, opts)?;
    let mut out = Volume {
        dims: source.dims,
        spacing_mm: source.spacing_mm,
        data: vec![0.0; source.data.len()],
    };
    m.apply(ctx, &mut out.data, &source.data)?;
    Ok((out, m))
}
