#![allow(clippy::neg_cmp_op_on_partial_ord)]
use super::resample_grid::{
    CoverageBuilder, DEFAULT_MAX_RESAMPLE_VOXELS, ERR_INVALID_RESAMPLE, ERR_RESAMPLE_TOO_LARGE,
    Grid, GridAxis, GridMap, IndexBox, PointMapper, ResampleCoverage, TargetRows, covered_run,
    for_each_line, map_row,
};
use super::roimask::RasterMask;
use super::{Cancellation, Error, ctx_err};
use crate::fmath;
use crate::render::grid::nearest_index;
use crate::textfmt::float_v;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt;
pub trait LabelSource {
    fn label_grid(&self) -> &Grid;
    fn label_at(&self, x: i64, y: i64, z: i64) -> u16;
    fn label_bounds(&self) -> Option<(IndexBox, bool)> {
        None
    }
    #[doc(hidden)]
    fn single_label(&self) -> Option<u16> {
        None
    }
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LabelVolume {
    pub grid: Grid,
    pub labels: Vec<u16>,
}
impl LabelVolume {
    pub fn new(grid: Grid) -> Result<LabelVolume, Error> {
        grid.validate()?;
        if grid.voxel_count() > DEFAULT_MAX_RESAMPLE_VOXELS * 4 {
            return Err(Error::wrap(
                ERR_RESAMPLE_TOO_LARGE,
                format!("{} voxels", grid.voxel_count()),
            ));
        }
        let labels = vec![0u16; grid.voxel_count() as usize];
        Ok(LabelVolume { grid, labels })
    }
    fn index(&self, x: i64, y: i64, z: i64) -> usize {
        ((z * self.grid.rows + y) * self.grid.columns + x) as usize
    }
    pub fn set(&mut self, x: i64, y: i64, z: i64, label: u16) {
        let at = self.index(x, y, z);
        self.labels[at] = label;
    }
    pub fn count(&self, label: u16) -> i64 {
        self.labels.iter().filter(|&&value| value == label).count() as i64
    }
    pub fn slice_mask(&self, label: u16, z: i64) -> RasterMask {
        let (columns, rows) = (self.grid.columns, self.grid.rows);
        let mut mask = RasterMask::new(columns, rows);
        for y in 0..rows {
            let row = &self.labels
                [((z * rows + y) * columns) as usize..((z * rows + y + 1) * columns) as usize];
            set_label_runs(&mut mask, y, 0, row, label);
        }
        mask
    }
}
impl LabelSource for LabelVolume {
    fn label_grid(&self) -> &Grid {
        &self.grid
    }
    fn label_at(&self, x: i64, y: i64, z: i64) -> u16 {
        self.labels[self.index(x, y, z)]
    }
}
fn set_label_runs(mask: &mut RasterMask, y: i64, x0: i64, row: &[u16], label: u16) {
    let mut start: i64 = -1;
    for i in 0..=row.len() {
        let on = i < row.len() && row[i] == label;
        if on && start < 0 {
            start = i as i64;
        } else if !on && start >= 0 {
            mask.set_run(y, x0 + start, x0 + i as i64);
            start = -1;
        }
    }
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LabelStack {
    grid: Grid,
    slices: Vec<Option<Vec<u16>>>,
    bounds: IndexBox,
    filled: bool,
}
impl LabelStack {
    pub fn new(grid: Grid, slices: Vec<Option<Vec<u16>>>) -> Result<LabelStack, Error> {
        grid.validate()?;
        if slices.len() as i64 != grid.slices {
            return Err(Error::wrap(
                ERR_INVALID_RESAMPLE,
                format!(
                    "{} label slices for {} grid slices",
                    slices.len(),
                    grid.slices
                ),
            ));
        }
        let mut b = BoundsBuilder::default();
        for (z, values) in slices.iter().enumerate() {
            let Some(values) = values else {
                continue;
            };
            if values.len() as i64 != grid.columns * grid.rows {
                return Err(Error::wrap(
                    ERR_INVALID_RESAMPLE,
                    format!(
                        "label slice {z} has {} values, want {}",
                        values.len(),
                        grid.columns * grid.rows
                    ),
                ));
            }
            for (i, &value) in values.iter().enumerate() {
                if value != 0 {
                    let i = i as i64;
                    b.add(i % grid.columns, i / grid.columns, z as i64);
                }
            }
        }
        let (bounds, filled) = b.index_box();
        Ok(LabelStack {
            grid,
            slices,
            bounds,
            filled,
        })
    }
}
impl LabelSource for LabelStack {
    fn label_grid(&self) -> &Grid {
        &self.grid
    }
    fn label_at(&self, x: i64, y: i64, z: i64) -> u16 {
        match &self.slices[z as usize] {
            None => 0,
            Some(values) => values[(y * self.grid.columns + x) as usize],
        }
    }
    fn label_bounds(&self) -> Option<(IndexBox, bool)> {
        Some((self.bounds, self.filled))
    }
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MaskStack {
    grid: Grid,
    masks: Vec<Option<RasterMask>>,
    label: u16,
    bounds: IndexBox,
    filled: bool,
}
impl MaskStack {
    pub fn new(grid: Grid, masks: Vec<Option<RasterMask>>, label: u16) -> Result<MaskStack, Error> {
        grid.validate()?;
        if masks.len() as i64 != grid.slices {
            return Err(Error::wrap(
                ERR_INVALID_RESAMPLE,
                format!("{} masks for {} grid slices", masks.len(), grid.slices),
            ));
        }
        let label = if label == 0 { 1 } else { label };
        let mut b = BoundsBuilder::default();
        for (z, mask) in masks.iter().enumerate() {
            let Some(mask) = mask else {
                continue;
            };
            if mask.columns != grid.columns || mask.rows != grid.rows {
                return Err(Error::wrap(
                    ERR_INVALID_RESAMPLE,
                    format!(
                        "mask {z} is {}x{}, want {}x{}",
                        mask.columns, mask.rows, grid.columns, grid.rows
                    ),
                ));
            }
            mask.for_each_run(|y, run| {
                b.add(run.start, y, z as i64);
                b.add(run.end - 1, y, z as i64);
            });
        }
        let (bounds, filled) = b.index_box();
        Ok(MaskStack {
            grid,
            masks,
            label,
            bounds,
            filled,
        })
    }
}
impl LabelSource for MaskStack {
    fn label_grid(&self) -> &Grid {
        &self.grid
    }
    fn label_at(&self, x: i64, y: i64, z: i64) -> u16 {
        match &self.masks[z as usize] {
            Some(mask) if mask.get(x, y) => self.label,
            _ => 0,
        }
    }
    fn label_bounds(&self) -> Option<(IndexBox, bool)> {
        Some((self.bounds, self.filled))
    }
    fn single_label(&self) -> Option<u16> {
        Some(self.label)
    }
}
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct BoundsBuilder {
    lo: [i64; 3],
    hi: [i64; 3],
    filled: bool,
}
impl BoundsBuilder {
    pub(crate) fn add(&mut self, x: i64, y: i64, z: i64) {
        let p = [x, y, z];
        if !self.filled {
            (self.lo, self.hi, self.filled) = (p, p, true);
            return;
        }
        for i in 0..3 {
            self.lo[i] = self.lo[i].min(p[i]);
            self.hi[i] = self.hi[i].max(p[i]);
        }
    }
    pub(crate) fn index_box(&self) -> (IndexBox, bool) {
        if !self.filled {
            return (IndexBox::default(), false);
        }
        (
            IndexBox {
                min: self.lo,
                max: [self.hi[0] + 1, self.hi[1] + 1, self.hi[2] + 1],
            },
            true,
        )
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct LabelInterpolator(pub i64);
pub const INTERPOLATE_NEAREST: LabelInterpolator = LabelInterpolator(0);
pub const INTERPOLATE_LABEL_GAUSSIAN: LabelInterpolator = LabelInterpolator(1);
pub const DEFAULT_GAUSSIAN_CUTOFF_SIGMAS: f64 = 3.0;
#[derive(Clone, Copy, Default)]
pub struct LabelResampleMode<'a> {
    pub interpolator: LabelInterpolator,
    pub sigma_mm: f64,
    pub cutoff_sigmas: f64,
    pub disable_preservation: bool,
    pub max_voxels: i64,
    pub mapper: Option<PointMapper<'a>>,
}
pub fn nearest_labels<'a>() -> LabelResampleMode<'a> {
    LabelResampleMode::default()
}
pub fn label_gaussian<'a>(sigma_mm: f64) -> LabelResampleMode<'a> {
    LabelResampleMode {
        interpolator: INTERPOLATE_LABEL_GAUSSIAN,
        sigma_mm,
        ..LabelResampleMode::default()
    }
}
impl fmt::Display for LabelResampleMode<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.interpolator == INTERPOLATE_LABEL_GAUSSIAN {
            write!(f, "label Gaussian σ={} mm", float_v(self.sigma_mm))?;
        } else {
            f.write_str("nearest")?;
        }
        if self.mapper.is_some() {
            f.write_str(" through a registration")?;
        }
        Ok(())
    }
}
impl<'a> LabelResampleMode<'a> {
    fn normalized(mut self) -> Result<LabelResampleMode<'a>, Error> {
        match self.interpolator {
            INTERPOLATE_NEAREST => {}
            INTERPOLATE_LABEL_GAUSSIAN => {
                if !(self.sigma_mm > 0.0) || self.sigma_mm.is_infinite() {
                    return Err(Error::wrap(
                        ERR_INVALID_RESAMPLE,
                        format!(
                            "Gaussian sigma {} mm must be positive and finite",
                            float_v(self.sigma_mm)
                        ),
                    ));
                }
                if self.cutoff_sigmas == 0.0 {
                    self.cutoff_sigmas = DEFAULT_GAUSSIAN_CUTOFF_SIGMAS;
                }
                if !(self.cutoff_sigmas > 0.0) || self.cutoff_sigmas.is_infinite() {
                    return Err(Error::wrap(
                        ERR_INVALID_RESAMPLE,
                        format!(
                            "Gaussian cutoff {} sigmas must be positive and finite",
                            float_v(self.cutoff_sigmas)
                        ),
                    ));
                }
            }
            other => {
                return Err(Error::wrap(
                    ERR_INVALID_RESAMPLE,
                    format!("unknown interpolator {}", other.0),
                ));
            }
        }
        if self.max_voxels == 0 {
            self.max_voxels = DEFAULT_MAX_RESAMPLE_VOXELS;
        }
        if self.max_voxels < 0 {
            return Err(Error::wrap(ERR_INVALID_RESAMPLE, "negative voxel budget"));
        }
        Ok(self)
    }
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LabelResampleReport {
    pub coverage: ResampleCoverage,
    pub counts: BTreeMap<u16, i64>,
    pub source_labels: Vec<u16>,
    pub preserved: Vec<u16>,
    pub vanished: Vec<u16>,
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LabelResampleResult {
    pub report: LabelResampleReport,
    pub volume: LabelVolume,
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LabelMaskResult {
    pub report: LabelResampleReport,
    pub grid: Grid,
    pub masks: BTreeMap<u16, Vec<Option<RasterMask>>>,
}
pub fn resample_labels(
    ctx: Option<&dyn Cancellation>,
    src: &dyn LabelSource,
    target: &Grid,
    mode: LabelResampleMode<'_>,
) -> Result<LabelResampleResult, Error> {
    let mode = mode.normalized()?;
    check_target_budget(target, mode.max_voxels)?;
    let mut sink = DenseLabelSink {
        volume: LabelVolume::new(target.clone())?,
    };
    let report = resample_labels_into(ctx, src, target, mode, &mut sink)?;
    Ok(LabelResampleResult {
        report,
        volume: sink.volume,
    })
}
pub fn resample_label_masks(
    ctx: Option<&dyn Cancellation>,
    src: &dyn LabelSource,
    target: &Grid,
    mode: LabelResampleMode<'_>,
) -> Result<LabelMaskResult, Error> {
    let mode = mode.normalized()?;
    check_target_budget(target, mode.max_voxels)?;
    let mut sink = MaskLabelSink {
        columns: target.columns,
        rows: target.rows,
        slices: target.slices,
        masks: BTreeMap::new(),
    };
    let report = resample_labels_into(ctx, src, target, mode, &mut sink)?;
    Ok(LabelMaskResult {
        report,
        grid: target.clone(),
        masks: sink.masks,
    })
}
pub(crate) fn check_target_budget(target: &Grid, budget: i64) -> Result<(), Error> {
    target.validate()?;
    if budget > 0 && target.voxel_count() > budget {
        return Err(Error::wrap(
            ERR_RESAMPLE_TOO_LARGE,
            format!("{} voxels > {}", target.voxel_count(), budget),
        ));
    }
    Ok(())
}
trait LabelSink {
    fn row(&mut self, y: i64, z: i64, x0: i64, labels: &[u16]);
    fn relabel(&mut self, x: i64, y: i64, z: i64, from: u16, to: u16);
}
struct DenseLabelSink {
    volume: LabelVolume,
}
impl LabelSink for DenseLabelSink {
    fn row(&mut self, y: i64, z: i64, x0: i64, labels: &[u16]) {
        let at = self.volume.index(x0, y, z);
        let tail = &mut self.volume.labels[at..];
        let n = labels.len().min(tail.len());
        tail[..n].copy_from_slice(&labels[..n]);
    }
    fn relabel(&mut self, x: i64, y: i64, z: i64, _from: u16, to: u16) {
        self.volume.set(x, y, z, to);
    }
}
struct MaskLabelSink {
    columns: i64,
    rows: i64,
    slices: i64,
    masks: BTreeMap<u16, Vec<Option<RasterMask>>>,
}
impl MaskLabelSink {
    fn mask(&mut self, label: u16, z: i64) -> &mut RasterMask {
        let (columns, rows, slices) = (self.columns, self.rows, self.slices);
        let per_slice = self
            .masks
            .entry(label)
            .or_insert_with(|| vec![None; slices.max(0) as usize]);
        per_slice[z as usize].get_or_insert_with(|| RasterMask::new(columns, rows))
    }
}
impl LabelSink for MaskLabelSink {
    fn row(&mut self, y: i64, z: i64, x0: i64, labels: &[u16]) {
        let mut i = 0;
        while i < labels.len() {
            let label = labels[i];
            let mut j = i + 1;
            while j < labels.len() && labels[j] == label {
                j += 1;
            }
            if label != 0 {
                self.mask(label, z).set_run(y, x0 + i as i64, x0 + j as i64);
            }
            i = j;
        }
    }
    fn relabel(&mut self, x: i64, y: i64, z: i64, from: u16, to: u16) {
        if from != 0 {
            self.mask(from, z).set(x, y, false);
        }
        if to != 0 {
            self.mask(to, z).set(x, y, true);
        }
    }
}
#[derive(Debug, Clone, Copy)]
struct LabelCandidate {
    fraction: f64,
    index: i64,
    x: i64,
    y: i64,
    z: i64,
    current: u16,
}
impl LabelCandidate {
    fn better(&self, o: &LabelCandidate) -> bool {
        self.fraction > o.fraction || self.fraction == o.fraction && self.index < o.index
    }
}
struct LabelReader<'a> {
    src: &'a dyn LabelSource,
    sm: &'a GridMap,
    bounds: IndexBox,
    eval_lo: [f64; 3],
    eval_hi: [f64; 3],
    gaussian: bool,
    kernel: GaussianKernel,
    target_rows: i64,
    target_columns: i64,
    candidates: HashMap<u16, LabelCandidate>,
    gauss: GaussianScratch,
}
impl LabelReader<'_> {
    fn label_at(&mut self, x: i64, y: i64, z: i64, u: f64, v: f64, s: f64) -> u16 {
        let (lo, hi) = (&self.eval_lo, &self.eval_hi);
        if u < lo[0] || u >= hi[0] || v < lo[1] || v >= hi[1] || s < lo[2] || s >= hi[2] {
            return 0;
        }
        if !self.gaussian {
            let axes = &self.sm.axes;
            return self
                .src
                .label_at(axes[0].nearest(u), axes[1].nearest(v), axes[2].nearest(s));
        }
        let label =
            self.kernel
                .evaluate(self.src, self.sm, self.bounds, [u, v, s], &mut self.gauss);
        let index = (z * self.target_rows + y) * self.target_columns + x;
        for (i, &candidate_label) in self.gauss.labels.iter().enumerate() {
            if candidate_label == 0 || self.gauss.total <= 0.0 {
                continue;
            }
            let c = LabelCandidate {
                fraction: self.gauss.weights[i] / self.gauss.total,
                index,
                x,
                y,
                z,
                current: label,
            };
            match self.candidates.get(&candidate_label) {
                Some(old) if !c.better(old) => {}
                _ => {
                    self.candidates.insert(candidate_label, c);
                }
            }
        }
        label
    }
}
fn resample_labels_into(
    ctx: Option<&dyn Cancellation>,
    src: &dyn LabelSource,
    target: &Grid,
    mode: LabelResampleMode<'_>,
    sink: &mut dyn LabelSink,
) -> Result<LabelResampleReport, Error> {
    let source_grid = src.label_grid();
    let sm = GridMap::new(source_grid)?;
    let full = IndexBox {
        min: [0; 3],
        max: [source_grid.columns, source_grid.rows, source_grid.slices],
    };
    let (bounds, has_labels) = src.label_bounds().unwrap_or((full, true));
    let gaussian = mode.interpolator == INTERPOLATE_LABEL_GAUSSIAN;
    let (mut eval_lo, mut eval_hi) = ([0.0f64; 3], [0.0f64; 3]);
    if has_labels {
        (eval_lo, eval_hi) = sm.box_extent(bounds);
        if gaussian {
            let reach = mode.cutoff_sigmas * mode.sigma_mm;
            for i in 0..3 {
                eval_lo[i] = fmath::max(sm.lo[i], eval_lo[i] - reach);
                eval_hi[i] = fmath::min(sm.hi[i], eval_hi[i] + reach);
            }
        }
    }
    let kernel = if gaussian {
        GaussianKernel {
            sigma: mode.sigma_mm,
            reach: mode.cutoff_sigmas * mode.sigma_mm,
        }
    } else {
        GaussianKernel::default()
    };
    let rows = TargetRows::new(&sm, target);
    let mut coverage = CoverageBuilder::new(target);
    let columns = target.columns.max(0) as usize;
    let mut buffer = vec![0u16; columns];
    let (mut coords, mut inside) = (Vec::new(), Vec::new());
    if mode.mapper.is_some() {
        coords = vec![[0.0f64; 3]; columns];
        inside = vec![false; columns];
    }
    let mut counts: BTreeMap<u16, i64> = BTreeMap::new();
    let mut reader = LabelReader {
        src,
        sm: &sm,
        bounds,
        eval_lo,
        eval_hi,
        gaussian,
        kernel,
        target_rows: target.rows,
        target_columns: target.columns,
        candidates: HashMap::new(),
        gauss: GaussianScratch::default(),
    };
    for_each_line(ctx, target.rows * target.slices, |line| {
        let (y, z) = (line % target.rows, line / target.rows);
        let labels: &mut [u16];
        let mut x0 = 0i64;
        if let Some(mapper) = mode.mapper {
            map_row(&sm, target, mapper, y, z, &mut coords, &mut inside);
            coverage.set_mask(y, z, &inside);
            if !has_labels {
                return;
            }
            labels = &mut buffer[..columns];
            for x in 0..columns {
                labels[x] = 0;
                if inside[x] {
                    let c = coords[x];
                    labels[x] = reader.label_at(x as i64, y, z, c[0], c[1], c[2]);
                }
            }
        } else {
            let l = rows.line(y, z);
            let (cx0, cx1) = covered_run(&sm, &l, target.columns);
            coverage.set_row(y, z, cx0, cx1);
            if !has_labels || cx1 <= cx0 {
                return;
            }
            let (r0, r1) = l.column_range(target.columns, eval_lo, eval_hi);
            let (r0, r1) = (r0.max(cx0), r1.min(cx1));
            if r1 <= r0 {
                return;
            }
            labels = &mut buffer[..(r1 - r0) as usize];
            x0 = r0;
            for x in r0..r1 {
                let (u, v, s) = l.at(x);
                labels[(x - r0) as usize] = reader.label_at(x, y, z, u, v, s);
            }
        }
        let mut i = 0;
        while i < labels.len() {
            let mut j = i + 1;
            while j < labels.len() && labels[j] == labels[i] {
                j += 1;
            }
            if labels[i] != 0 {
                *counts.entry(labels[i]).or_insert(0) += (j - i) as i64;
            }
            i = j;
        }
        sink.row(y, z, x0, labels);
    })?;
    let candidates = std::mem::take(&mut reader.candidates);
    let mut report = LabelResampleReport {
        coverage: coverage.finish(),
        counts,
        ..LabelResampleReport::default()
    };
    if has_labels && mode.mapper.is_none() {
        report.source_labels = source_labels_inside(ctx, src, &sm, bounds, target)?;
    }
    let count_of =
        |counts: &BTreeMap<u16, i64>, label: u16| counts.get(&label).copied().unwrap_or(0);
    if gaussian && !mode.disable_preservation {
        let mut taken: HashSet<i64> = HashSet::new();
        for &label in &report.source_labels {
            if count_of(&report.counts, label) > 0 {
                continue;
            }
            let Some(c) = candidates.get(&label) else {
                continue;
            };
            if c.fraction <= 0.0 || taken.contains(&c.index) {
                continue;
            }
            if c.current != 0 && count_of(&report.counts, c.current) <= 1 {
                continue;
            }
            sink.relabel(c.x, c.y, c.z, c.current, label);
            if c.current != 0 {
                *report.counts.entry(c.current).or_insert(0) -= 1;
            }
            report.counts.insert(label, 1);
            taken.insert(c.index);
            report.preserved.push(label);
        }
    }
    report.counts.retain(|_, n| *n != 0);
    for &label in &report.source_labels {
        if count_of(&report.counts, label) == 0 {
            report.vanished.push(label);
        }
    }
    match ctx_err(ctx) {
        Some(err) => Err(err),
        None => Ok(report),
    }
}
fn source_labels_inside(
    ctx: Option<&dyn Cancellation>,
    src: &dyn LabelSource,
    sm: &GridMap,
    bounds: IndexBox,
    target: &Grid,
) -> Result<Vec<u16>, Error> {
    let tm = GridMap::new(target)?;
    let source_grid = &sm.grid;
    let lines = TargetRows::new(&tm, source_grid);
    let single = src.single_label();
    let mut found = false;
    let mut set: BTreeSet<u16> = BTreeSet::new();
    let (box_rows, box_slices) = (bounds.max[1] - bounds.min[1], bounds.max[2] - bounds.min[2]);
    for_each_line(ctx, box_rows.wrapping_mul(box_slices), |line| {
        if single.is_some() && found {
            return;
        }
        let (y, z) = (
            bounds.min[1] + line % box_rows,
            bounds.min[2] + line / box_rows,
        );
        let l = lines.line(y, z);
        let (x0, x1) = l.column_range(source_grid.columns, tm.lo, tm.hi);
        let (x0, x1) = (x0.max(bounds.min[0]), x1.min(bounds.max[0]));
        for x in x0..x1 {
            let (u, v, s) = l.at(x);
            if !tm.inside(u, v, s) {
                continue;
            }
            let label = src.label_at(x, y, z);
            if label != 0 && set.insert(label) && single == Some(label) {
                found = true;
                return;
            }
        }
    })?;
    Ok(set.into_iter().collect())
}
#[derive(Debug, Clone, Copy, Default)]
struct GaussianKernel {
    sigma: f64,
    reach: f64,
}
#[derive(Debug, Clone, Default)]
struct GaussianScratch {
    axis_weights: [Vec<f64>; 3],
    labels: Vec<u16>,
    weights: Vec<f64>,
    total: f64,
}
impl GaussianKernel {
    fn axis_weights(&self, a: &GridAxis, c: f64, out: &mut Vec<f64>) -> i64 {
        let i0 = nearest_index(a.continuous(c - self.reach)).max(0);
        let i1 = nearest_index(a.continuous(c + self.reach)).min(a.n - 1);
        out.clear();
        let scale = 1.0 / (std::f64::consts::SQRT_2 * self.sigma);
        for i in i0..=i1 {
            let (lo, hi) = a.edges(i);
            out.push(0.5 * (fmath::erf((hi - c) * scale) - fmath::erf((lo - c) * scale)));
        }
        i0
    }
    fn evaluate(
        &self,
        src: &dyn LabelSource,
        sm: &GridMap,
        bounds: IndexBox,
        p: [f64; 3],
        scratch: &mut GaussianScratch,
    ) -> u16 {
        let mut start = [0i64; 3];
        for i in 0..3 {
            start[i] = self.axis_weights(&sm.axes[i], p[i], &mut scratch.axis_weights[i]);
        }
        scratch.labels.clear();
        scratch.weights.clear();
        scratch.total = 0.0;
        let mut best = 0u16;
        let mut best_weight = 0.0f64;
        let [wx, wy, wz] = &scratch.axis_weights;
        for (dz, &weight_z) in wz.iter().enumerate() {
            let z = start[2] + dz as i64;
            for (dy, &weight_y) in wy.iter().enumerate() {
                let y = start[1] + dy as i64;
                let weight_zy = weight_z * weight_y;
                for (dx, &weight_x) in wx.iter().enumerate() {
                    let x = start[0] + dx as i64;
                    let w = weight_zy * weight_x;
                    let label = if bounds.contains(x, y, z) {
                        src.label_at(x, y, z)
                    } else {
                        0
                    };
                    scratch.total += w;
                    let slot = match scratch.labels.iter().position(|&l| l == label) {
                        Some(slot) => slot,
                        None => {
                            scratch.labels.push(label);
                            scratch.weights.push(0.0);
                            scratch.labels.len() - 1
                        }
                    };
                    scratch.weights[slot] += w;
                    if scratch.weights[slot] > best_weight {
                        best_weight = scratch.weights[slot];
                        best = label;
                    }
                }
            }
        }
        best
    }
}
