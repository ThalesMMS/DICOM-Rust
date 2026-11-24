#![allow(clippy::neg_cmp_op_on_partial_ord)]
use crate::fmath;
use crate::roi::{Error, RasterMask, Segmentation3D};
use crate::textfmt::float_v;
pub const DEFAULT_BINS: i64 = 256;
pub const MAX_BINS: i64 = 1 << 20;
pub const ERR_NO_SAMPLES: &str = "autothreshold: no finite samples";
pub const ERR_SINGLE_VALUE: &str = "autothreshold: all samples fall in one bin";
pub const ERR_NOT_BIMODAL: &str = "autothreshold: histogram does not smooth to two peaks";
pub const ERR_TOO_MANY_BINS: &str = "autothreshold: too many bins for this method";
pub type Values<'a> = &'a dyn Fn(&mut dyn FnMut(f64) -> bool);
pub type ValueAt<'a> = &'a dyn Fn(i64, i64) -> Option<f64>;
pub(crate) type VoxelValueAt<'a> = &'a dyn Fn(i64, i64, i64) -> Option<f64>;
pub fn slice_values(values: &[f64]) -> impl Fn(&mut dyn FnMut(f64) -> bool) + '_ {
    move |emit| {
        for &v in values {
            if !emit(v) {
                return;
            }
        }
    }
}
pub fn grid_values<'a>(
    columns: i64,
    rows: i64,
    value_at: Option<ValueAt<'a>>,
) -> impl Fn(&mut dyn FnMut(f64) -> bool) + 'a {
    move |emit| {
        let Some(value_at) = value_at else {
            return;
        };
        for y in 0..rows {
            for x in 0..columns {
                if let Some(v) = value_at(x, y)
                    && !emit(v)
                {
                    return;
                }
            }
        }
    }
}
pub fn mask_values<'a>(
    mask: Option<&'a RasterMask>,
    value_at: Option<ValueAt<'a>>,
) -> impl Fn(&mut dyn FnMut(f64) -> bool) + 'a {
    move |emit| {
        if let Some(value_at) = value_at {
            mask_values_into(mask, &|x, y| value_at(x, y), emit);
        }
    }
}
fn mask_values_into(
    mask: Option<&RasterMask>,
    value_at: &dyn Fn(i64, i64) -> Option<f64>,
    emit: &mut dyn FnMut(f64) -> bool,
) -> bool {
    let Some(mask) = mask else {
        return true;
    };
    for y in 0..mask.rows {
        for run in mask.runs(y) {
            for x in run.start..run.end {
                if let Some(v) = value_at(x, y)
                    && !emit(v)
                {
                    return false;
                }
            }
        }
    }
    true
}
pub fn segmentation_values<'a>(
    seg: Option<&'a Segmentation3D>,
    value_at: Option<VoxelValueAt<'a>>,
) -> impl Fn(&mut dyn FnMut(f64) -> bool) + 'a {
    move |emit| {
        let (Some(seg), Some(value_at)) = (seg, value_at) else {
            return;
        };
        for slice in seg.slices() {
            let at = |x: i64, y: i64| value_at(x, y, slice);
            if !mask_values_into(seg.mask_at(slice), &at, emit) {
                return;
            }
        }
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct HistogramOptions {
    pub bins: i64,
    pub bin_width: f64,
    pub lower: f64,
    pub upper: f64,
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Histogram {
    pub counts: Vec<f64>,
    pub lower: f64,
    pub upper: f64,
    pub samples: i64,
    pub excluded: i64,
    pub min_value: f64,
    pub max_value: f64,
}
pub fn new_histogram(
    values: Option<Values<'_>>,
    opts: HistogramOptions,
) -> Result<Histogram, Error> {
    let Some(values) = values else {
        return Err(Error::sentinel(ERR_NO_SAMPLES));
    };
    let explicit = opts.upper > opts.lower;
    if opts.bins < 0 || !opts.bin_width.is_finite() || opts.bin_width < 0.0 {
        return Err(Error::msg(format!(
            "autothreshold: invalid bins {} or bin width {}",
            opts.bins,
            float_v(opts.bin_width)
        )));
    }
    if explicit && opts.bin_width > 0.0 {
        return Err(Error::msg(
            "autothreshold: BinWidth cannot be combined with an explicit range",
        ));
    }
    if explicit
        && (opts.lower.is_infinite()
            || opts.upper.is_infinite()
            || (opts.upper - opts.lower).is_infinite())
    {
        return Err(Error::msg("autothreshold: histogram range must be finite"));
    }
    let mut bins = opts.bins;
    if bins == 0 {
        bins = DEFAULT_BINS;
    }
    if bins > MAX_BINS {
        return Err(Error::wrap(
            ERR_TOO_MANY_BINS,
            format!("{bins} bins requested, at most {MAX_BINS}"),
        ));
    }
    let mut h = Histogram {
        min_value: f64::INFINITY,
        max_value: f64::NEG_INFINITY,
        ..Histogram::default()
    };
    let (mut lower, mut upper) = (opts.lower, opts.upper);
    if !explicit {
        (lower, upper) = observed_range(values);
        if lower > upper {
            return Err(Error::sentinel(ERR_NO_SAMPLES));
        }
        if opts.bin_width > 0.0 {
            let first = (lower / opts.bin_width).round();
            let last = (upper / opts.bin_width).round();
            let n = last - first + 1.0;
            if n > MAX_BINS as f64 || n.is_nan() {
                return Err(Error::wrap(
                    ERR_TOO_MANY_BINS,
                    format!(
                        "bin width {} needs {} bins",
                        float_v(opts.bin_width),
                        float_v(n)
                    ),
                ));
            }
            bins = fmath::f64_to_i64(last - first).wrapping_add(1);
            lower = (first - 0.5) * opts.bin_width;
            upper = lower + bins as f64 * opts.bin_width;
        } else if upper == lower {
            bins = 1;
            (lower, upper) = (lower - 0.5, upper + 0.5);
        } else if (upper - lower).is_infinite() {
            return Err(Error::msg("autothreshold: sample range overflows"));
        }
    }
    h.lower = lower;
    h.upper = upper;
    h.counts = vec![0.0; bins as usize];
    let scale = bins as f64 / (upper - lower);
    values(&mut |v: f64| {
        if !v.is_finite() || v < lower || v > upper {
            h.excluded += 1;
            return true;
        }
        let mut i = fmath::f64_to_i64((v - lower) * scale);
        if i >= bins {
            i = bins - 1;
        } else if i < 0 {
            i = 0;
        }
        h.counts[i as usize] += 1.0;
        h.samples += 1;
        h.min_value = fmath::min(h.min_value, v);
        h.max_value = fmath::max(h.max_value, v);
        true
    });
    if h.samples == 0 {
        return Err(Error::sentinel(ERR_NO_SAMPLES));
    }
    Ok(h)
}
fn observed_range(values: Values<'_>) -> (f64, f64) {
    let (mut lower, mut upper) = (f64::INFINITY, f64::NEG_INFINITY);
    values(&mut |v: f64| {
        if v.is_finite() {
            lower = fmath::min(lower, v);
            upper = fmath::max(upper, v);
        }
        true
    });
    (lower, upper)
}
pub fn new_histogram_from_counts(
    counts: &[f64],
    lower: f64,
    upper: f64,
) -> Result<Histogram, Error> {
    if counts.is_empty() || counts.len() as i64 > MAX_BINS {
        return Err(Error::msg(format!(
            "autothreshold: {} bins, want 1..{}",
            counts.len(),
            MAX_BINS
        )));
    }
    if !(upper > lower)
        || lower.is_infinite()
        || upper.is_infinite()
        || (upper - lower).is_infinite()
    {
        return Err(Error::msg(format!(
            "autothreshold: invalid histogram range [{}, {}]",
            float_v(lower),
            float_v(upper)
        )));
    }
    let mut total = 0.0;
    for (i, &c) in counts.iter().enumerate() {
        if !c.is_finite() || c < 0.0 {
            return Err(Error::msg(format!(
                "autothreshold: bin {} count {} is not a finite non-negative number",
                i,
                float_v(c)
            )));
        }
        total += c;
    }
    if total == 0.0 {
        return Err(Error::sentinel(ERR_NO_SAMPLES));
    }
    Ok(Histogram {
        counts: counts.to_vec(),
        lower,
        upper,
        samples: fmath::f64_to_i64(fmath::min(total.round(), 2147483647.0)),
        excluded: 0,
        min_value: f64::NAN,
        max_value: f64::NAN,
    })
}
impl Histogram {
    pub fn bins(&self) -> i64 {
        self.counts.len() as i64
    }
    pub fn bin_width(&self) -> f64 {
        (self.upper - self.lower) / self.counts.len() as f64
    }
    pub fn bin_lower(&self, i: i64) -> f64 {
        if i == self.counts.len() as i64 {
            return self.upper;
        }
        self.lower + i as f64 * self.bin_width()
    }
    pub fn bin_center(&self, i: i64) -> f64 {
        self.lower + (i as f64 + 0.5) * self.bin_width()
    }
    pub fn bin_of(&self, v: f64) -> i64 {
        let i = fmath::f64_to_i64(((v - self.lower) / self.bin_width()).floor());
        if i < 0 {
            return 0;
        }
        if i >= self.counts.len() as i64 {
            return self.counts.len() as i64 - 1;
        }
        i
    }
    pub fn total(&self) -> f64 {
        let mut total = 0.0;
        for &c in &self.counts {
            total += c;
        }
        total
    }
    pub(super) fn span(&self) -> Result<(usize, usize), Error> {
        let mut bounds: Option<(usize, usize)> = None;
        for (i, &c) in self.counts.iter().enumerate() {
            if c > 0.0 {
                bounds = Some((bounds.map_or(i, |b| b.0), i));
            }
        }
        match bounds {
            None => Err(Error::sentinel(ERR_NO_SAMPLES)),
            Some((lo, hi)) if lo == hi => Err(Error::sentinel(ERR_SINGLE_VALUE)),
            Some(bounds) => Ok(bounds),
        }
    }
}
