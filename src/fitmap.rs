#![forbid(unsafe_code)]
use crate::codes::{Code, dcm, ucum};
use crate::fmath;
use crate::object::{Object, Tag};
use crate::render::parallel;
use crate::textfmt::float_v;
use std::fmt;
use std::sync::Mutex;
pub const ERR_FIT: &str = "dicom/fitmap: cannot fit map";
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    message: String,
}
impl Error {
    fn new(detail: impl fmt::Display) -> Error {
        Error {
            message: format!("{ERR_FIT}: {detail}"),
        }
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for Error {}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Kind {
    #[default]
    T2,
    T2Star,
    Adc,
}
impl Kind {
    pub fn from_i64(kind: i64) -> Result<Kind, Error> {
        match kind {
            0 => Ok(Kind::T2),
            1 => Ok(Kind::T2Star),
            2 => Ok(Kind::Adc),
            _ => Err(Error::new(format_args!("unknown kind {kind}"))),
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::T2 => "T2",
            Kind::T2Star => "T2*",
            Kind::Adc => "ADC",
        }
    }
    pub fn quantity(self) -> Code<'static> {
        match self {
            Kind::T2 => dcm::T2_113065,
            Kind::T2Star => dcm::T2_113064,
            Kind::Adc => dcm::ApparentDiffusionCoefficient,
        }
    }
    pub fn units(self) -> Code<'static> {
        if self == Kind::Adc {
            ucum::Um2PerS
        } else {
            ucum::Millisecond
        }
    }
    pub fn parameter_name(self) -> &'static str {
        if self == Kind::Adc {
            "b-value (s/mm²)"
        } else {
            "echo time (ms)"
        }
    }
}
impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Options {
    pub mask: Option<Vec<bool>>,
    pub background: f64,
    pub threshold: f64,
    pub weighted: bool,
    pub max_value: f64,
    pub workers: i64,
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FitResult {
    pub kind: Kind,
    pub parameters: Vec<f64>,
    pub values: Vec<f64>,
    pub s0: Vec<f64>,
    pub r_squared: Vec<f64>,
    pub fitted: i64,
    pub skipped: i64,
    pub insufficient: i64,
    pub non_decaying: i64,
    pub out_of_range: i64,
}
fn total_less(a: f64, b: f64) -> std::cmp::Ordering {
    a.partial_cmp(&b).unwrap_or(std::cmp::Ordering::Equal)
}
const FIT_CHUNK: usize = 4096;
struct Chunk {
    values: Vec<f64>,
    s0: Vec<f64>,
    r_squared: Vec<f64>,
    counts: [i64; 5],
}
pub fn fit(
    kind: Kind,
    parameters: &[f64],
    images: &[Vec<f32>],
    opts: &Options,
) -> Result<FitResult, Error> {
    if parameters.len() != images.len() {
        return Err(Error::new(format_args!(
            "{} parameters for {} images",
            parameters.len(),
            images.len()
        )));
    }
    if images.len() < 2 {
        return Err(Error::new(format_args!("{} images", images.len())));
    }
    let voxels = images[0].len();
    for (i, &x) in parameters.iter().enumerate() {
        if !x.is_finite() || x < 0.0 {
            return Err(Error::new(format_args!(
                "{} {}",
                kind.parameter_name(),
                float_v(x)
            )));
        }
        if images[i].len() != voxels {
            return Err(Error::new(format_args!(
                "image {i} has {} voxels, want {voxels}",
                images[i].len()
            )));
        }
    }
    if let Some(mask) = &opts.mask
        && mask.len() != voxels
    {
        return Err(Error::new(format_args!(
            "mask has {} voxels, want {voxels}",
            mask.len()
        )));
    }
    if !opts.background.is_finite()
        || !opts.threshold.is_finite()
        || !opts.max_value.is_finite()
        || opts.workers < 0
    {
        return Err(Error::new(
            "background, threshold and maximum must be finite and workers not negative",
        ));
    }
    let mut order: Vec<usize> = (0..parameters.len()).collect();
    order.sort_by(|&a, &b| total_less(parameters[a], parameters[b]));
    let mut out = FitResult {
        kind,
        ..FitResult::default()
    };
    for &i in &order {
        if out.parameters.last() != Some(&parameters[i]) {
            out.parameters.push(parameters[i]);
        }
    }
    if out.parameters.len() < 2 {
        return Err(Error::new(format_args!(
            "the {}s are not distinct",
            kind.parameter_name()
        )));
    }
    let first = &images[order[0]];
    let chunks = voxels.div_ceil(FIT_CHUNK);
    let results: Vec<Mutex<Option<Chunk>>> = (0..chunks).map(|_| Mutex::new(None)).collect();
    let order = &order;
    let run = parallel::run(None, chunks as i64, opts.workers, 1, || {
        let mut x: Vec<f64> = Vec::with_capacity(images.len());
        let mut y: Vec<f64> = Vec::with_capacity(images.len());
        let mut w: Vec<f64> = Vec::with_capacity(images.len());
        let results = &results;
        move |chunk: i64| {
            let start = chunk as usize * FIT_CHUNK;
            let end = voxels.min(start + FIT_CHUNK);
            let n = end - start;
            let mut part = Chunk {
                values: vec![f64::NAN; n],
                s0: vec![f64::NAN; n],
                r_squared: vec![f64::NAN; n],
                counts: [0; 5],
            };
            for v in start..end {
                let masked = opts.mask.as_ref().is_some_and(|mask| !mask[v]);
                let above = f64::from(first[v]) > opts.threshold;
                if masked || !above {
                    part.counts[0] += 1;
                    continue;
                }
                x.clear();
                y.clear();
                w.clear();
                let mut distinct = false;
                for &i in order {
                    let s = f64::from(images[i][v]) - opts.background;
                    let positive = s > 0.0;
                    if !positive || s.is_infinite() {
                        continue;
                    }
                    if !x.is_empty() && parameters[i] != x[0] {
                        distinct = true;
                    }
                    x.push(parameters[i]);
                    y.push(fmath::log(s));
                    w.push(if opts.weighted { s * s } else { 1.0 });
                }
                if !distinct {
                    part.counts[1] += 1;
                    continue;
                }
                let (rate, intercept, r2) = log_linear(&x, &y, &w);
                let decaying = rate > 0.0;
                if !decaying {
                    part.counts[2] += 1;
                    continue;
                }
                let value = if kind == Kind::Adc {
                    rate * 1e6
                } else {
                    1.0 / rate
                };
                if opts.max_value > 0.0 && value > opts.max_value {
                    part.counts[3] += 1;
                    continue;
                }
                part.values[v - start] = value;
                part.s0[v - start] = fmath::exp(intercept);
                part.r_squared[v - start] = r2;
                part.counts[4] += 1;
            }
            if let Ok(mut slot) = results[chunk as usize].lock() {
                *slot = Some(part);
            }
        }
    });
    if let Err(err) = run {
        return Err(Error::new(err));
    }
    out.values = Vec::with_capacity(voxels);
    out.s0 = Vec::with_capacity(voxels);
    out.r_squared = Vec::with_capacity(voxels);
    for slot in results {
        let part = slot.into_inner().ok().flatten();
        let Some(part) = part else {
            return Err(Error::new("a worker did not finish"));
        };
        out.values.extend(part.values);
        out.s0.extend(part.s0);
        out.r_squared.extend(part.r_squared);
        out.skipped += part.counts[0];
        out.insufficient += part.counts[1];
        out.non_decaying += part.counts[2];
        out.out_of_range += part.counts[3];
        out.fitted += part.counts[4];
    }
    Ok(out)
}
fn log_linear(x: &[f64], y: &[f64], w: &[f64]) -> (f64, f64, f64) {
    let (mut sw, mut sx, mut sy) = (0.0f64, 0.0f64, 0.0f64);
    for i in 0..x.len() {
        sw += w[i];
        sx += w[i] * x[i];
        sy += w[i] * y[i];
    }
    let (mx, my) = (sx / sw, sy / sw);
    let (mut sxx, mut sxy, mut syy) = (0.0f64, 0.0f64, 0.0f64);
    for i in 0..x.len() {
        let (dx, dy) = (x[i] - mx, y[i] - my);
        sxx += w[i] * dx * dx;
        sxy += w[i] * dx * dy;
        syy += w[i] * dy * dy;
    }
    let slope = sxy / sxx;
    let intercept = my - slope * mx;
    let mut r2 = 1.0;
    if syy > 0.0 {
        let mut residual = 0.0f64;
        for i in 0..x.len() {
            let d = y[i] - (intercept + slope * x[i]);
            residual += w[i] * d * d;
        }
        r2 = 1.0 - residual / syy;
    }
    (-slope, intercept, r2)
}
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Frame {
    pub parameter: f64,
    pub position: f64,
    pub index: i64,
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Grouping {
    pub parameters: Vec<f64>,
    pub positions: Vec<f64>,
    pub images: Vec<Vec<i64>>,
    pub dropped: i64,
}
pub fn group(
    frames: &[Frame],
    parameter_tolerance: f64,
    position_tolerance: f64,
) -> Result<Grouping, Error> {
    let tolerances_ok = parameter_tolerance >= 0.0 && position_tolerance >= 0.0;
    if !tolerances_ok {
        return Err(Error::new("negative tolerance"));
    }
    let mut sorted = frames.to_vec();
    for f in &sorted {
        if !f.parameter.is_finite() || !f.position.is_finite() || f.parameter < 0.0 {
            return Err(Error::new(format_args!(
                "frame {} has parameter {} at {} mm",
                f.index,
                float_v(f.parameter),
                float_v(f.position)
            )));
        }
    }
    sorted.sort_by(|a, b| total_less(a.parameter, b.parameter));
    let mut out = Grouping::default();
    let mut groups: Vec<Vec<Frame>> = Vec::new();
    for f in sorted {
        if let (Some(&last), Some(group)) = (out.parameters.last(), groups.last_mut())
            && f.parameter - last <= parameter_tolerance
        {
            group.push(f);
            continue;
        }
        out.parameters.push(f.parameter);
        groups.push(vec![f]);
    }
    if out.parameters.len() < 2 {
        return Err(Error::new(format_args!(
            "{} distinct parameter values",
            out.parameters.len()
        )));
    }
    let slot = |group: &[Frame], position: f64| {
        group
            .iter()
            .position(|f| (f.position - position).abs() <= position_tolerance)
    };
    let mut first = groups[0].clone();
    first.sort_by(|a, b| total_less(a.position, b.position));
    let mut used: Vec<Vec<bool>> = groups.iter().map(|g| vec![false; g.len()]).collect();
    out.images = vec![Vec::new(); groups.len()];
    for f in &first {
        if let Some(&last) = out.positions.last()
            && (f.position - last).abs() <= position_tolerance
        {
            continue;
        }
        let row: Option<Vec<usize>> = groups.iter().map(|g| slot(g, f.position)).collect();
        let Some(row) = row else {
            continue;
        };
        out.positions.push(f.position);
        for (g, &i) in row.iter().enumerate() {
            out.images[g].push(groups[g][i].index);
            used[g][i] = true;
        }
    }
    if out.positions.is_empty() {
        return Err(Error::new("no position is acquired at every parameter"));
    }
    for (g, group) in groups.iter().enumerate() {
        out.dropped += (group.len() - used[g].iter().filter(|&&u| u).count()) as i64;
    }
    Ok(out)
}
const TAG_ECHO_TIME: Tag = Tag::new(0x0018, 0x0081);
const TAG_MR_ECHO_SEQUENCE: Tag = Tag::new(0x0018, 0x9114);
const TAG_EFFECTIVE_ECHO_TIME: Tag = Tag::new(0x0018, 0x9082);
pub fn echo_time(obj: Option<&Object>, frame_index: i64) -> Option<f64> {
    let obj = obj?;
    let usable = |value: f64| value.is_finite() && value >= 0.0;
    if let (Some(item), _, None) = obj.functional_group_item(frame_index, TAG_MR_ECHO_SEQUENCE)
        && let Ok(value) = item.get_float(TAG_EFFECTIVE_ECHO_TIME)
        && usable(value)
    {
        return Some(value);
    }
    obj.get_float(TAG_ECHO_TIME).ok().filter(|&v| usable(v))
}
