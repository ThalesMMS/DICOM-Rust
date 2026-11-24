use super::Duration;
use crate::fmath;
use crate::net::ul::{Ctx, Kind as CtxKind};
use std::fmt;
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TemporalGrid {
    pub size: [i64; 3],
    pub origin: [f64; 3],
    pub spacing: [f64; 3],
    pub direction: [f64; 9],
    pub frame_of_reference_uid: String,
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TemporalFrameInfo {
    pub grid: TemporalGrid,
    pub units: String,
    pub missing: bool,
    pub offset: Duration,
    pub has_offset: bool,
    pub duration: Duration,
    pub has_duration: bool,
    pub source_id: String,
    pub registration: String,
}
pub const MAX_TEMPORAL_UNIT_BYTES: usize = 128;
pub const MAX_TEMPORAL_FRAME_OF_REFERENCE_UID_BYTES: usize = 64;
pub const MAX_TEMPORAL_SOURCE_ID_BYTES: usize = 4 << 20;
pub const MAX_TEMPORAL_REGISTRATION_BYTES: usize = 64 << 10;
pub trait TemporalSource {
    fn frame_count(&self) -> i64;
    fn frame_info(&self, ctx: &Ctx, frame: i64) -> Result<TemporalFrameInfo, crate::BoxError>;
    fn read_frame(&self, ctx: &Ctx, frame: i64, dst: &mut [f64]) -> Result<(), crate::BoxError>;
}
macro_rules! temporal_enum {
    ($(#[$doc:meta])* $name:ident { $($(#[$vdoc:meta])* $variant:ident => $text:literal),+ $(,)? }) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
        pub enum $name {
            #[default]
            $($(#[$vdoc])* $variant),+
        }
        impl $name {
            pub fn as_str(self) -> &'static str {
                match self {
                    $($name::$variant => $text),+
                }
            }
        }
    };
}
temporal_enum!(
    TemporalStatistic {
        Mean => "mean",
        Minimum => "minimum",
        Maximum => "maximum",
        StandardDeviation => "standard-deviation",
    }
);
temporal_enum!(
    TemporalWeighting {
        Sample => "sample",
        Duration => "duration",
    }
);
temporal_enum!(
    TemporalVariance {
        Population => "population",
        Sample => "sample",
    }
);
temporal_enum!(
    MissingFramePolicy {
        Reject => "reject-missing",
        Skip => "skip-missing",
    }
);
temporal_enum!(
    NonFinitePolicy {
        Reject => "reject-nonfinite",
        Skip => "skip-nonfinite",
    }
);
temporal_enum!(
    TemporalWindowAlignment {
        Causal => "causal",
        Centered => "centered",
    }
);
temporal_enum!(
    TemporalWindowBorder {
        Clip => "clip",
        RequireFull => "require-full",
    }
);
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TemporalLimits {
    pub max_frames: i64,
    pub max_voxels: i64,
    pub max_working_bytes: u64,
}
pub fn default_temporal_limits() -> TemporalLimits {
    TemporalLimits {
        max_frames: 10_000,
        max_voxels: 16_777_216,
        max_working_bytes: 1 << 30,
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TemporalMapOptions {
    pub statistic: TemporalStatistic,
    pub weighting: TemporalWeighting,
    pub variance: TemporalVariance,
    pub missing: MissingFramePolicy,
    pub non_finite: NonFinitePolicy,
    pub limits: TemporalLimits,
}
pub fn default_temporal_map_options() -> TemporalMapOptions {
    TemporalMapOptions {
        limits: default_temporal_limits(),
        ..TemporalMapOptions::default()
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MovingMeanOptions {
    pub window: i64,
    pub alignment: TemporalWindowAlignment,
    pub border: TemporalWindowBorder,
    pub weighting: TemporalWeighting,
    pub missing: MissingFramePolicy,
    pub non_finite: NonFinitePolicy,
    pub limits: TemporalLimits,
}
pub fn default_moving_mean_options() -> MovingMeanOptions {
    MovingMeanOptions {
        window: 3,
        limits: default_temporal_limits(),
        ..MovingMeanOptions::default()
    }
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TemporalMapProvenance {
    pub options: TemporalMapOptions,
    pub moving: bool,
    pub window: i64,
    pub alignment: TemporalWindowAlignment,
    pub border: TemporalWindowBorder,
    pub target: i64,
    pub start: i64,
    pub end: i64,
    pub frame_indices: Vec<i64>,
    pub missing_indices: Vec<i64>,
    pub weights: Vec<f64>,
    pub offsets: Vec<Duration>,
    pub has_offsets: Vec<bool>,
    pub durations: Vec<Duration>,
    pub has_durations: Vec<bool>,
    pub sources: Vec<String>,
    pub registrations: Vec<String>,
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TemporalMap {
    pub values: Vec<f64>,
    pub valid: Vec<bool>,
    pub counts: Vec<u32>,
    pub weight_sums: Vec<f64>,
    pub grid: TemporalGrid,
    pub units: String,
    pub provenance: TemporalMapProvenance,
    pub peak_working_bytes: u64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemporalErrorKind {
    Admission,
    NoSamples,
    Window,
    Data,
    Context(CtxKind),
    Source,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemporalError {
    pub kind: TemporalErrorKind,
    text: String,
}
const ERR_ADMISSION: &str = "dicom/dynamic: temporal map admission";
const ERR_DATA: &str = "dicom/dynamic: invalid temporal data";
impl TemporalError {
    fn admission(detail: impl fmt::Display) -> TemporalError {
        TemporalError {
            kind: TemporalErrorKind::Admission,
            text: format!("{ERR_ADMISSION}: {detail}"),
        }
    }
    fn data(detail: impl fmt::Display) -> TemporalError {
        TemporalError {
            kind: TemporalErrorKind::Data,
            text: format!("{ERR_DATA}: {detail}"),
        }
    }
    fn bare(kind: TemporalErrorKind, text: &str) -> TemporalError {
        TemporalError {
            kind,
            text: text.into(),
        }
    }
}
impl fmt::Display for TemporalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}
impl std::error::Error for TemporalError {}
fn ctx_err(ctx: &Ctx) -> Result<(), TemporalError> {
    match ctx.err() {
        None => Ok(()),
        Some(kind) => Err(TemporalError::bare(
            TemporalErrorKind::Context(kind),
            if kind == CtxKind::Canceled {
                "context canceled"
            } else {
                "context deadline exceeded"
            },
        )),
    }
}
fn seconds(d: Duration) -> f64 {
    let (sec, nsec) = (d / 1_000_000_000, d % 1_000_000_000);
    sec as f64 + nsec as f64 / 1e9
}
pub fn compute_temporal_map(
    ctx: &Ctx,
    source: &dyn TemporalSource,
    options: &TemporalMapOptions,
) -> Result<TemporalMap, TemporalError> {
    let count = admission(ctx, source, options)?;
    compute(
        ctx,
        source,
        options,
        TemporalMapProvenance {
            options: *options,
            target: -1,
            end: count,
            ..TemporalMapProvenance::default()
        },
    )
}
pub fn moving_temporal_mean(
    ctx: &Ctx,
    source: &dyn TemporalSource,
    target: i64,
    options: &MovingMeanOptions,
) -> Result<TemporalMap, TemporalError> {
    let map_options = TemporalMapOptions {
        weighting: options.weighting,
        missing: options.missing,
        non_finite: options.non_finite,
        limits: options.limits,
        ..TemporalMapOptions::default()
    };
    let count = admission(ctx, source, &map_options)?;
    if target < 0
        || target >= count
        || options.window <= 0
        || options.window > options.limits.max_frames
    {
        return Err(TemporalError::admission("invalid target or window options"));
    }
    let (mut left, mut right) = (options.window - 1, 0);
    if options.alignment == TemporalWindowAlignment::Centered {
        if options.window % 2 == 0 {
            return Err(TemporalError::admission("centered window must be odd"));
        }
        (left, right) = (options.window / 2, options.window / 2);
    }
    let (crosses_start, crosses_end) = (left > target, right >= count - target);
    if options.border == TemporalWindowBorder::RequireFull && (crosses_start || crosses_end) {
        return Err(TemporalError::bare(
            TemporalErrorKind::Window,
            "dicom/dynamic: incomplete temporal window",
        ));
    }
    let start = if crosses_start { 0 } else { target - left };
    let end = if crosses_end {
        count
    } else {
        target + right + 1
    };
    compute(
        ctx,
        source,
        &map_options,
        TemporalMapProvenance {
            options: map_options,
            moving: true,
            window: options.window,
            alignment: options.alignment,
            border: options.border,
            target,
            start,
            end,
            ..TemporalMapProvenance::default()
        },
    )
}
fn admission(
    ctx: &Ctx,
    source: &dyn TemporalSource,
    options: &TemporalMapOptions,
) -> Result<i64, TemporalError> {
    ctx_err(ctx)?;
    let limits = options.limits;
    if limits.max_frames <= 0
        || limits.max_frames as u64 > u64::from(u32::MAX)
        || limits.max_voxels <= 0
        || limits.max_working_bytes == 0
    {
        return Err(TemporalError::admission(
            "invalid policies or finite limits",
        ));
    }
    let count = source.frame_count();
    if count <= 0 || count > limits.max_frames {
        return Err(TemporalError::admission(format!(
            "frame count {count} exceeds limit {}",
            limits.max_frames
        )));
    }
    Ok(count)
}
const PROVENANCE_BYTES_PER_POSITION: u64 = 2 * 8 + 3 * 8 + 2 + 2 * 16;
fn compute(
    ctx: &Ctx,
    source: &dyn TemporalSource,
    options: &TemporalMapOptions,
    mut provenance: TemporalMapProvenance,
) -> Result<TemporalMap, TemporalError> {
    let positions = (provenance.end - provenance.start) as u64;
    let mut metadata_bytes = positions * PROVENANCE_BYTES_PER_POSITION;
    let limit = options.limits.max_working_bytes;
    if metadata_bytes > limit {
        return Err(TemporalError::admission(format!(
            "provenance needs {metadata_bytes} bytes"
        )));
    }
    let p = positions as usize;
    let pv = &mut provenance;
    pv.frame_indices = Vec::with_capacity(p);
    pv.missing_indices = Vec::with_capacity(p);
    pv.weights = Vec::with_capacity(p);
    pv.offsets = Vec::with_capacity(p);
    pv.has_offsets = Vec::with_capacity(p);
    pv.durations = Vec::with_capacity(p);
    pv.has_durations = Vec::with_capacity(p);
    pv.sources = Vec::with_capacity(p);
    pv.registrations = Vec::with_capacity(p);
    let mut grid = TemporalGrid::default();
    let mut units = String::new();
    let mut voxels = 0usize;
    for frame in pv.start..pv.end {
        ctx_err(ctx)?;
        let info = source.frame_info(ctx, frame).map_err(|e| TemporalError {
            kind: TemporalErrorKind::Source,
            text: format!("frame {frame} metadata: {e}"),
        })?;
        if info.missing {
            if options.missing == MissingFramePolicy::Reject {
                return Err(TemporalError::data(format!("frame {frame} is missing")));
            }
            pv.missing_indices.push(frame);
            continue;
        }
        if info.units.len() > MAX_TEMPORAL_UNIT_BYTES
            || info.grid.frame_of_reference_uid.len() > MAX_TEMPORAL_FRAME_OF_REFERENCE_UID_BYTES
            || info.source_id.len() > MAX_TEMPORAL_SOURCE_ID_BYTES
            || info.registration.len() > MAX_TEMPORAL_REGISTRATION_BYTES
        {
            return Err(TemporalError::admission(format!(
                "frame {frame} metadata text exceeds its field limit"
            )));
        }
        let first_accepted = voxels == 0;
        if first_accepted {
            voxels = grid_voxels(&info.grid, options.limits.max_voxels)?;
            grid = info.grid.clone();
            units = info.units.clone();
            if units.trim().is_empty() {
                return Err(TemporalError::data(format!(
                    "frame {frame} has no explicit units"
                )));
            }
        } else if info.grid != grid || info.units != units {
            return Err(TemporalError::data(format!(
                "frame {frame} grid or units diverge"
            )));
        }
        if info.has_offset && info.offset < 0 {
            return Err(TemporalError::data(format!(
                "frame {frame} has a negative offset"
            )));
        }
        if info.has_duration && info.duration <= 0 {
            return Err(TemporalError::data(format!(
                "frame {frame} has a nonpositive duration"
            )));
        }
        let mut weight = 1.0;
        if options.weighting == TemporalWeighting::Duration {
            if !info.has_duration || info.duration <= 0 {
                return Err(TemporalError::data(format!(
                    "frame {frame} needs a declared positive duration"
                )));
            }
            weight = seconds(info.duration);
        }
        let mut text_bytes = (info.source_id.len() + info.registration.len()) as u64;
        if first_accepted {
            text_bytes += (info.units.len() + info.grid.frame_of_reference_uid.len()) as u64;
        }
        if text_bytes > limit - metadata_bytes {
            return Err(TemporalError::admission(format!(
                "frame {frame} retained metadata exceeds the working-byte limit"
            )));
        }
        metadata_bytes += text_bytes;
        pv.frame_indices.push(frame);
        pv.weights.push(weight);
        pv.offsets.push(info.offset);
        pv.has_offsets.push(info.has_offset);
        pv.durations.push(info.duration);
        pv.has_durations.push(info.has_duration);
        pv.sources.push(info.source_id);
        pv.registrations.push(info.registration);
    }
    if voxels == 0 {
        return Err(TemporalError::bare(
            TemporalErrorKind::NoSamples,
            "dicom/dynamic: no temporal samples",
        ));
    }
    let deviation = options.statistic == TemporalStatistic::StandardDeviation;
    let sample = deviation && options.variance == TemporalVariance::Sample;
    let bytes_per_voxel = 29 + if deviation { 8 } else { 0 } + if sample { 8 } else { 0 };
    if voxels as u64 > (limit - metadata_bytes) / bytes_per_voxel {
        return Err(TemporalError::admission(format!(
            "working arrays exceed {limit} bytes"
        )));
    }
    let working_bytes = metadata_bytes + voxels as u64 * bytes_per_voxel;
    ctx_err(ctx)?;
    let mut values = vec![0.0; voxels];
    let mut counts = vec![0u32; voxels];
    let mut weight_sums = vec![0.0; voxels];
    let mut buffer = vec![0.0; voxels];
    let mut m2 = if deviation {
        vec![0.0; voxels]
    } else {
        Vec::new()
    };
    let mut denominators = if sample {
        vec![0.0; voxels]
    } else {
        Vec::new()
    };
    for (position, &frame) in provenance.frame_indices.iter().enumerate() {
        ctx_err(ctx)?;
        source
            .read_frame(ctx, frame, &mut buffer)
            .map_err(|e| TemporalError {
                kind: TemporalErrorKind::Source,
                text: format!("frame {frame} pixels: {e}"),
            })?;
        ctx_err(ctx)?;
        let weight = provenance.weights[position];
        for (voxel, &value) in buffer.iter().enumerate() {
            if voxel % 4096 == 0 {
                ctx_err(ctx)?;
            }
            if !value.is_finite() {
                if options.non_finite == NonFinitePolicy::Reject {
                    return Err(TemporalError::data(format!(
                        "frame {frame} voxel {voxel} is nonfinite"
                    )));
                }
                continue;
            }
            let old_weight = weight_sums[voxel];
            let new_weight = old_weight + weight;
            if counts[voxel] == 0 {
                values[voxel] = value;
            } else {
                match options.statistic {
                    TemporalStatistic::Minimum => values[voxel] = fmath::min(values[voxel], value),
                    TemporalStatistic::Maximum => values[voxel] = fmath::max(values[voxel], value),
                    _ => {
                        let delta = value - values[voxel];
                        let mean = values[voxel] + delta * (weight / new_weight);
                        if !delta.is_finite() || !mean.is_finite() {
                            return Err(TemporalError::data(format!(
                                "frame {frame} voxel {voxel} recurrence overflow"
                            )));
                        }
                        if deviation {
                            m2[voxel] += delta * (delta * (old_weight * (weight / new_weight)));
                            if !m2[voxel].is_finite() {
                                return Err(TemporalError::data(format!(
                                    "frame {frame} voxel {voxel} variance overflow"
                                )));
                            }
                        }
                        values[voxel] = mean;
                    }
                }
            }
            counts[voxel] += 1;
            weight_sums[voxel] = new_weight;
            if sample {
                denominators[voxel] =
                    (old_weight / new_weight) * (denominators[voxel] + 2.0 * weight);
            }
        }
    }
    let mut valid = vec![false; voxels];
    for voxel in 0..voxels {
        if voxel % 4096 == 0 {
            ctx_err(ctx)?;
        }
        valid[voxel] = counts[voxel] > 0;
        if deviation && valid[voxel] {
            let denominator = if sample {
                denominators[voxel]
            } else {
                weight_sums[voxel]
            };
            if denominator <= 0.0 || (sample && counts[voxel] < 2) {
                valid[voxel] = false;
            } else {
                values[voxel] = (fmath::max(m2[voxel], 0.0) / denominator).sqrt();
                if !values[voxel].is_finite() {
                    return Err(TemporalError::data(format!(
                        "voxel {voxel} standard deviation overflow"
                    )));
                }
            }
        }
        if !valid[voxel] {
            values[voxel] = f64::NAN;
        }
    }
    ctx_err(ctx)?;
    Ok(TemporalMap {
        values,
        valid,
        counts,
        weight_sums,
        grid,
        units,
        provenance,
        peak_working_bytes: working_bytes,
    })
}
fn grid_voxels(grid: &TemporalGrid, limit: i64) -> Result<usize, TemporalError> {
    if grid.frame_of_reference_uid.trim().is_empty() {
        return Err(TemporalError::data(
            "explicit frame of reference is required",
        ));
    }
    let mut voxels: i64 = 1;
    for axis in 0..3 {
        let size = grid.size[axis];
        if size <= 0 || voxels > limit / size {
            return Err(TemporalError::admission(format!(
                "grid exceeds {limit} voxels"
            )));
        }
        voxels *= size;
        if !grid.origin[axis].is_finite()
            || !grid.spacing[axis].is_finite()
            || grid.spacing[axis] <= 0.0
        {
            return Err(TemporalError::data("invalid grid origin or spacing"));
        }
    }
    for axis in 0..3 {
        for other in axis..3 {
            let mut dot = 0.0;
            for row in 0..3 {
                let (a, b) = (
                    grid.direction[row * 3 + axis],
                    grid.direction[row * 3 + other],
                );
                if !a.is_finite() || !b.is_finite() {
                    return Err(TemporalError::data("nonfinite grid direction"));
                }
                dot += a * b;
            }
            let want = if axis == other { 1.0 } else { 0.0 };
            if (dot - want).abs() > 1e-9 {
                return Err(TemporalError::data("grid direction is not orthonormal"));
            }
        }
    }
    Ok(voxels as usize)
}
