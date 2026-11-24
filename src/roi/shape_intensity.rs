use super::distance3d::VolumeMask;
use super::pdqsort;
use super::polygon_raster::finite;
use super::resample_labels::LabelVolume;
use super::segmentation::Segmentation3D;
use super::shape_stats::ERR_INVALID_SHAPE_INPUT;
use super::{Cancellation, Error, ctx_err};
use crate::fmath;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct IntensityMoments {
    pub label: u16,
    pub count: i64,
    pub non_finite: i64,
    pub min: f64,
    pub max: f64,
    pub sum: f64,
    pub mean: f64,
    pub variance: f64,
    pub standard_deviation: f64,
}
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct IntensityAccumulator {
    pub(crate) count: i64,
    pub(crate) non_finite: i64,
    pub(crate) min: f64,
    pub(crate) max: f64,
    pub(crate) sum: f64,
    pub(crate) compensate: f64,
    pub(crate) shift: f64,
    pub(crate) mean: f64,
    pub(crate) m2: f64,
}
impl IntensityAccumulator {
    pub fn add(&mut self, v: f64) {
        if !finite(v) {
            self.non_finite += 1;
            return;
        }
        self.count += 1;
        if self.count == 1 {
            self.min = v;
            self.max = v;
            self.shift = v;
        } else if v < self.min {
            self.min = v;
        } else if v > self.max {
            self.max = v;
        }
        let t = self.sum + v;
        if self.sum.abs() >= v.abs() {
            self.compensate += (self.sum - t) + v;
        } else {
            self.compensate += (v - t) + self.sum;
        }
        self.sum = t;
        let d = v - self.shift;
        let delta = d - self.mean;
        self.mean += delta / self.count as f64;
        self.m2 += delta * (d - self.mean);
    }
    pub fn moments(&self) -> IntensityMoments {
        let mut m = IntensityMoments {
            count: self.count,
            non_finite: self.non_finite,
            ..IntensityMoments::default()
        };
        if self.count == 0 {
            return m;
        }
        m.min = self.min;
        m.max = self.max;
        m.sum = self.sum + self.compensate;
        m.mean = m.sum / self.count as f64;
        if self.count > 1 {
            m.variance = fmath::max(0.0, self.m2 / (self.count - 1) as f64);
            m.standard_deviation = m.variance.sqrt();
        }
        m
    }
}
pub fn new_intensity_moments(values: &[f64]) -> IntensityMoments {
    let mut a = IntensityAccumulator::default();
    for &v in values {
        a.add(v);
    }
    a.moments()
}
#[derive(Debug, Clone, Default)]
pub struct IntensityStatistics {
    pub label: u16,
    pub count: i64,
    pub non_finite: i64,
    pub min: f64,
    pub max: f64,
    pub sum: f64,
    pub mean: f64,
    pub variance: f64,
    pub standard_deviation: f64,
    pub median: f64,
    pub(crate) order: Option<Arc<IntensityOrder>>,
}
#[derive(Debug)]
pub(crate) struct IntensityOrder {
    state: Mutex<OrderState>,
    capacity: i64,
}
#[derive(Debug)]
struct OrderState {
    values: Vec<f64>,
    sorted: bool,
}
impl IntensityOrder {
    pub(crate) fn new(values: Vec<f64>, capacity: i64) -> IntensityOrder {
        IntensityOrder {
            state: Mutex::new(OrderState {
                values,
                sorted: false,
            }),
            capacity,
        }
    }
    pub(crate) fn new_sorted(values: Vec<f64>, capacity: i64) -> IntensityOrder {
        IntensityOrder {
            state: Mutex::new(OrderState {
                values,
                sorted: true,
            }),
            capacity,
        }
    }
    pub(crate) fn len(&self) -> usize {
        self.lock().values.len()
    }
    pub(crate) fn capacity(&self) -> i64 {
        self.capacity
    }
    fn lock(&self) -> std::sync::MutexGuard<'_, OrderState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
    pub(crate) fn with_sorted<R>(&self, f: impl FnOnce(&[f64]) -> R) -> R {
        let mut state = self.lock();
        if !state.sorted {
            pdqsort::sort_float64s(&mut state.values);
            state.sorted = true;
        }
        f(&state.values)
    }
}
impl IntensityStatistics {
    pub fn moments(&self) -> IntensityMoments {
        IntensityMoments {
            label: self.label,
            count: self.count,
            non_finite: self.non_finite,
            min: self.min,
            max: self.max,
            sum: self.sum,
            mean: self.mean,
            variance: self.variance,
            standard_deviation: self.standard_deviation,
        }
    }
    pub fn percentile(&self, p: f64) -> Option<f64> {
        let order = self.order.as_ref()?;
        if order.len() == 0 || !(0.0..=100.0).contains(&p) {
            return None;
        }
        Some(order.with_sorted(|sorted| {
            let rank = p / 100.0 * (sorted.len() - 1) as f64;
            let i = fmath::f64_to_i64(rank.floor());
            if i >= sorted.len() as i64 - 1 {
                return sorted[sorted.len() - 1];
            }
            let i = i as usize;
            interpolate_rank(sorted[i], sorted[i + 1], rank - i as f64)
        }))
    }
    pub fn percentiles(&self, ps: &[f64]) -> Vec<f64> {
        ps.iter()
            .map(|&p| self.percentile(p).unwrap_or(f64::NAN))
            .collect()
    }
}
pub fn new_intensity_statistics(values: &[f64]) -> IntensityStatistics {
    let mut a = IntensityAccumulator::default();
    for &v in values {
        a.add(v);
    }
    let capacity = a.count;
    let owned: Vec<f64> = values.iter().copied().filter(|v| finite(*v)).collect();
    intensity_from_owned(owned, capacity, a.moments())
}
pub(crate) fn intensity_from_owned(
    mut values: Vec<f64>,
    capacity: i64,
    m: IntensityMoments,
) -> IntensityStatistics {
    let mut median = 0.0;
    if !values.is_empty() {
        median = selected_median(&mut values);
    }
    IntensityStatistics {
        label: m.label,
        count: m.count,
        non_finite: m.non_finite,
        min: m.min,
        max: m.max,
        sum: m.sum,
        mean: m.mean,
        variance: m.variance,
        standard_deviation: m.standard_deviation,
        median,
        order: Some(Arc::new(IntensityOrder::new(values, capacity))),
    }
}
pub(crate) fn selected_median(values: &mut [f64]) -> f64 {
    let n = values.len();
    let rank = 0.5 * (n - 1) as f64;
    let i = rank.floor() as usize;
    select_kth(values, i);
    if i >= n - 1 {
        return values[i];
    }
    let lo = values[i];
    let mut hi = values[i + 1];
    for &v in &values[i + 2..] {
        hi = fmath::min(hi, v);
    }
    interpolate_rank(lo, hi, rank - i as f64)
}
pub(crate) fn interpolate_rank(lo: f64, hi: f64, f: f64) -> f64 {
    if f == 0.0 || lo == hi {
        return lo;
    }
    lo + f * (hi - lo)
}
pub(crate) fn select_kth(values: &mut [f64], k: usize) {
    if values.is_empty() {
        return;
    }
    let (mut lo, mut hi) = (0usize, values.len() - 1);
    let mut budget = 2 * (usize::BITS - values.len().leading_zeros());
    while hi > lo {
        if budget == 0 {
            pdqsort::sort_float64s(&mut values[lo..=hi]);
            return;
        }
        budget -= 1;
        let mid = lo + (hi - lo) / 2;
        if values[mid] < values[lo] {
            values.swap(mid, lo);
        }
        if values[hi] < values[lo] {
            values.swap(hi, lo);
        }
        if values[hi] < values[mid] {
            values.swap(hi, mid);
        }
        let pivot = values[mid];
        let (mut i, mut j) = (lo as isize, hi as isize);
        while i <= j {
            while values[i as usize] < pivot {
                i += 1;
            }
            while values[j as usize] > pivot {
                j -= 1;
            }
            if i <= j {
                values.swap(i as usize, j as usize);
                i += 1;
                j -= 1;
            }
        }
        let k = k as isize;
        if k <= j {
            hi = j as usize;
        } else if k >= i {
            lo = i as usize;
        } else {
            return;
        }
    }
}
const GO_SIZE_CLASSES: [usize; 68] = [
    8, 16, 24, 32, 48, 64, 80, 96, 112, 128, 144, 160, 176, 192, 208, 224, 240, 256, 288, 320, 352,
    384, 416, 448, 480, 512, 576, 640, 704, 768, 896, 1024, 1152, 1280, 1408, 1536, 1792, 2048,
    2304, 2688, 3072, 3200, 3456, 4096, 4864, 5120, 5376, 6144, 6528, 6784, 6912, 8192, 9472, 9728,
    10240, 10880, 12288, 13568, 14336, 16384, 18432, 19072, 20480, 21760, 24576, 27264, 28672,
    32768,
];
pub(crate) fn grown_capacity(old_cap: usize, new_len: usize) -> usize {
    let double = old_cap + old_cap;
    let mut new_cap = if new_len > double {
        new_len
    } else if old_cap < 256 {
        double
    } else {
        let mut c = old_cap;
        loop {
            c += (c + 3 * 256) >> 2;
            if c >= new_len {
                break c;
            }
        }
    };
    let bytes = new_cap * 8;
    let rounded = if bytes <= 32768 - 8 {
        GO_SIZE_CLASSES
            .iter()
            .copied()
            .find(|class| *class >= bytes)
            .unwrap_or(bytes)
    } else {
        (bytes + 8191) & !8191
    };
    new_cap = rounded / 8;
    new_cap
}
#[derive(Debug, Default)]
struct GrownSlice {
    values: Vec<f64>,
    capacity: usize,
}
impl GrownSlice {
    fn append(&mut self, v: f64) {
        if self.values.len() == self.capacity {
            self.capacity = grown_capacity(self.capacity, self.values.len() + 1);
        }
        self.values.push(v);
    }
}
impl VolumeMask {
    fn check_mask_values(&self, values: &[f64]) -> Result<(), Error> {
        self.validate()?;
        if values.len() != self.voxels.len() {
            return Err(Error::wrap(
                ERR_INVALID_SHAPE_INPUT,
                format!("{} values for {} voxels", values.len(), self.voxels.len()),
            ));
        }
        Ok(())
    }
    pub fn intensity_moments(&self, values: &[f64]) -> Result<IntensityMoments, Error> {
        self.check_mask_values(values)?;
        let mut a = IntensityAccumulator::default();
        for (i, &on) in self.voxels.iter().enumerate() {
            if on {
                a.add(values[i]);
            }
        }
        Ok(a.moments())
    }
    pub fn intensity_statistics(&self, values: &[f64]) -> Result<IntensityStatistics, Error> {
        self.check_mask_values(values)?;
        let mut a = IntensityAccumulator::default();
        for (i, &on) in self.voxels.iter().enumerate() {
            if on {
                a.add(values[i]);
            }
        }
        let mut inside: Vec<f64> = Vec::with_capacity(a.count as usize);
        for (i, &on) in self.voxels.iter().enumerate() {
            if on && finite(values[i]) {
                inside.push(values[i]);
            }
        }
        Ok(intensity_from_owned(inside, a.count, a.moments()))
    }
}
impl Segmentation3D {
    fn for_each_value(
        &self,
        mut value_at: impl FnMut(i64, i64, i64) -> Option<f64>,
        mut f: impl FnMut(f64),
    ) {
        for z in self.slices() {
            self.masks[&z].for_each_pixel(|x, y| {
                if let Some(v) = value_at(x, y, z) {
                    f(v);
                }
            });
        }
    }
    pub fn intensity_moments(
        &self,
        value_at: impl FnMut(i64, i64, i64) -> Option<f64>,
    ) -> IntensityMoments {
        let mut a = IntensityAccumulator::default();
        self.for_each_value(value_at, |v| a.add(v));
        a.moments()
    }
    pub fn intensity_statistics(
        &self,
        value_at: impl FnMut(i64, i64, i64) -> Option<f64>,
    ) -> IntensityStatistics {
        let mut a = IntensityAccumulator::default();
        let mut inside = GrownSlice::default();
        self.for_each_value(value_at, |v| {
            a.add(v);
            if finite(v) {
                inside.append(v);
            }
        });
        intensity_from_owned(inside.values, inside.capacity as i64, a.moments())
    }
}
fn check_label_values<'a>(
    v: Option<&'a LabelVolume>,
    values: &[f64],
) -> Result<&'a LabelVolume, Error> {
    let Some(v) = v.filter(|v| {
        v.labels.len() as i64 == v.grid.voxel_count() && v.grid.columns > 0 && v.grid.rows > 0
    }) else {
        return Err(Error::wrap(ERR_INVALID_SHAPE_INPUT, "label volume size"));
    };
    if values.len() != v.labels.len() {
        return Err(Error::wrap(
            ERR_INVALID_SHAPE_INPUT,
            format!("{} values for {} voxels", values.len(), v.labels.len()),
        ));
    }
    Ok(v)
}
fn label_accumulators(
    ctx: Option<&dyn Cancellation>,
    v: Option<&LabelVolume>,
    values: &[f64],
) -> Result<BTreeMap<u16, IntensityAccumulator>, Error> {
    let v = check_label_values(v, values)?;
    let mut by_label: BTreeMap<u16, IntensityAccumulator> = BTreeMap::new();
    let slice = (v.grid.columns * v.grid.rows) as usize;
    for (i, &label) in v.labels.iter().enumerate() {
        if i % slice == 0
            && let Some(err) = ctx_err(ctx)
        {
            return Err(err);
        }
        if label == 0 {
            continue;
        }
        by_label.entry(label).or_default().add(values[i]);
    }
    Ok(by_label)
}
pub fn label_intensity_moments(
    ctx: Option<&dyn Cancellation>,
    v: Option<&LabelVolume>,
    values: &[f64],
) -> Result<Vec<IntensityMoments>, Error> {
    let by_label = label_accumulators(ctx, v, values)?;
    Ok(by_label
        .iter()
        .map(|(label, a)| {
            let mut m = a.moments();
            m.label = *label;
            m
        })
        .collect())
}
pub fn label_intensity_statistics(
    ctx: Option<&dyn Cancellation>,
    v: Option<&LabelVolume>,
    values: &[f64],
) -> Result<Vec<IntensityStatistics>, Error> {
    let counts = label_accumulators(ctx, v, values)?;
    let Some(v) = v else {
        return Ok(Vec::new());
    };
    let mut by_label: BTreeMap<u16, Vec<f64>> = counts
        .iter()
        .map(|(label, a)| (*label, Vec::with_capacity(a.count as usize)))
        .collect();
    for (i, &label) in v.labels.iter().enumerate() {
        if label != 0
            && finite(values[i])
            && let Some(list) = by_label.get_mut(&label)
        {
            list.push(values[i]);
        }
    }
    let mut out = Vec::with_capacity(counts.len());
    for (label, a) in &counts {
        let mut m = a.moments();
        m.label = *label;
        let owned = by_label.remove(label).unwrap_or_default();
        out.push(intensity_from_owned(owned, a.count, m));
    }
    Ok(out)
}
