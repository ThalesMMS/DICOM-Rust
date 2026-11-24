#![forbid(unsafe_code)]
mod error;
mod read;
pub mod temporal_map;
use crate::fmath;
use crate::numtext::format_float;
use crate::render::stablesort::slice_stable;
pub use error::Error;
pub use read::{read, read_frame_metadata};
use std::collections::HashMap;
pub use temporal_map::{
    MovingMeanOptions, TemporalError, TemporalErrorKind, TemporalFrameInfo, TemporalGrid,
    TemporalLimits, TemporalMap, TemporalMapOptions, TemporalMapProvenance, TemporalSource,
    compute_temporal_map, moving_temporal_mean,
};
pub type Duration = i64;
const MICROSECOND: Duration = 1_000;
const MILLISECOND: Duration = 1_000_000;
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Instant(pub i128);
impl Default for Instant {
    fn default() -> Self {
        Instant(GO_ZERO_TIME)
    }
}
const GO_ZERO_TIME: i128 = -62_135_596_800 * 1_000_000_000;
impl Instant {
    pub fn is_zero(self) -> bool {
        self.0 == GO_ZERO_TIME
    }
    #[allow(clippy::should_implement_trait)]
    pub fn sub(self, u: Instant) -> Duration {
        (self.0 - u.0).clamp(i64::MIN as i128, i64::MAX as i128) as i64
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AcquisitionTimestamp {
    pub time: Option<crate::dcmtime::ZonedTime>,
    pub utc_offset_known: bool,
}
impl AcquisitionTimestamp {
    pub fn is_zero(&self) -> bool {
        self.time.is_none_or(|t| t.is_zero())
    }
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FrameMetadata {
    pub frame_index: i64,
    pub temporal_position: i64,
    pub has_temporal_position: bool,
    pub number_of_temporal_positions: i64,
    pub has_number_of_temporal_positions: bool,
    pub stack_id: String,
    pub in_stack_position: i64,
    pub has_in_stack_position: bool,
    pub offset: Duration,
    pub has_offset: bool,
    pub duration: Duration,
    pub has_duration: bool,
    pub actual_duration: Duration,
    pub has_actual_duration: bool,
    pub trigger: Duration,
    pub has_trigger: bool,
    pub phase: f64,
    pub has_phase: bool,
    pub acquisition_time: Instant,
    pub has_acquisition_time: bool,
    pub acquisition_stamp: AcquisitionTimestamp,
    pub spatial_position: f64,
    pub has_spatial_position: bool,
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Stack {
    pub id: String,
    pub frame_indices: Vec<i64>,
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TimePoint {
    pub ordinal: i64,
    pub temporal_position: i64,
    pub has_position: bool,
    pub offset: Duration,
    pub has_offset: bool,
    pub duration: Duration,
    pub has_duration: bool,
    pub trigger: Duration,
    pub has_trigger: bool,
    pub phase: f64,
    pub has_phase: bool,
    pub used_occurrence_fallback: bool,
    pub stacks: Vec<Stack>,
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Timeline {
    pub frames: Vec<FrameMetadata>,
    pub points: Option<Vec<TimePoint>>,
    pub dynamic: bool,
    pub irregular: bool,
    pub gated: bool,
    pub multiple_stacks: bool,
    pub used_occurrence_fallback: bool,
}
impl Timeline {
    pub fn points(&self) -> &[TimePoint] {
        self.points.as_deref().unwrap_or(&[])
    }
}
pub fn build(input: &[FrameMetadata]) -> Timeline {
    let frames = input.to_vec();
    let mut timeline = Timeline {
        frames: frames.clone(),
        ..Default::default()
    };
    if frames.is_empty() {
        return timeline;
    }
    let duplicate_spatial = has_duplicate_spatial_position(&frames);
    let mut has_explicit_position = false;
    let mut needs_explicit_offset_lookup = false;
    let mut has_acquisition_time = false;
    let mut earliest = Instant::default();
    for frame in &frames {
        has_explicit_position = has_explicit_position || frame.has_temporal_position;
        needs_explicit_offset_lookup =
            needs_explicit_offset_lookup || (!frame.has_temporal_position && frame.has_offset);
        if frame.has_acquisition_time {
            has_acquisition_time = true;
            if earliest.is_zero() || frame.acquisition_time < earliest {
                earliest = frame.acquisition_time;
            }
        }
        timeline.gated = timeline.gated || frame.has_trigger || frame.has_phase;
    }
    let mut explicit_offset_index = Vec::new();
    if has_explicit_position && needs_explicit_offset_lookup {
        let mut explicit: HashMap<i64, Duration> = HashMap::new();
        for frame in &frames {
            if frame.has_temporal_position && frame.has_offset {
                explicit.insert(frame.temporal_position, frame.offset);
            }
        }
        explicit_offset_index = new_temporal_offset_index(&explicit);
    }
    struct PointBuilder {
        point: TimePoint,
        frames: Vec<FrameMetadata>,
    }
    let mut occurrence: HashMap<String, i64> = HashMap::new();
    let mut builders: HashMap<String, PointBuilder> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    for frame in &frames {
        let spatial_key = frame_spatial_key(frame);
        let entry = occurrence.entry(spatial_key).or_insert(0);
        let ordinal = *entry;
        *entry = ordinal + 1;
        let (key, position, has_position, used_fallback) = temporal_key_with_provenance(
            frame,
            has_explicit_position,
            &explicit_offset_index,
            duplicate_spatial && has_acquisition_time,
            ordinal,
        );
        let builder = match builders.get_mut(&key) {
            Some(b) => {
                if used_fallback {
                    b.point.used_occurrence_fallback = true;
                }
                b
            }
            None => {
                order.push(key.clone());
                builders.entry(key.clone()).or_insert(PointBuilder {
                    point: TimePoint {
                        temporal_position: position,
                        has_position,
                        used_occurrence_fallback: used_fallback,
                        ..Default::default()
                    },
                    frames: Vec::new(),
                })
            }
        };
        builder.frames.push(frame.clone());
        merge_timing(&mut builder.point, frame, earliest, has_acquisition_time);
    }
    slice_stable(&mut order, |a, b| {
        less_time_point(&builders[a].point, &builders[b].point, a, b)
    });
    let mut points = Vec::with_capacity(order.len());
    for (ordinal, key) in order.iter().enumerate() {
        let builder = builders.get_mut(key).expect("builder of an ordered key");
        builder.point.ordinal = ordinal as i64;
        builder.point.stacks = build_stacks(&builder.frames);
        timeline.used_occurrence_fallback =
            timeline.used_occurrence_fallback || builder.point.used_occurrence_fallback;
        if builder.point.stacks.len() > 1 {
            timeline.multiple_stacks = true;
        }
        points.push(builder.point.clone());
    }
    timeline.dynamic = points.len() > 1;
    timeline.irregular = irregular_timing(&points);
    timeline.points = Some(points);
    timeline
}
fn new_temporal_offset_index(offsets: &HashMap<i64, Duration>) -> Vec<(Duration, i64)> {
    let mut entries: Vec<(Duration, i64)> = offsets.iter().map(|(p, o)| (*o, *p)).collect();
    entries.sort();
    entries.dedup_by(|b, a| a.0 == b.0);
    entries
}
fn nearest_offset(index: &[(Duration, i64)], target: Duration) -> Option<i64> {
    let insertion = index.partition_point(|e| e.0 < target);
    let mut best: Option<(i64, u64)> = None;
    let mut consider = |entry_index: usize| {
        let Some(entry) = index.get(entry_index) else {
            return;
        };
        let delta = duration_distance(target, entry.0);
        if delta > MICROSECOND as u64 {
            return;
        }
        match best {
            Some((position, best_delta))
                if !(delta < best_delta || (delta == best_delta && entry.1 < position)) => {}
            _ => best = Some((entry.1, delta)),
        }
    };
    if insertion > 0 {
        consider(insertion - 1);
    }
    consider(insertion);
    best.map(|b| b.0)
}
fn duration_distance(a: Duration, b: Duration) -> u64 {
    if a >= b {
        (a as u64).wrapping_sub(b as u64)
    } else {
        (b as u64).wrapping_sub(a as u64)
    }
}
fn temporal_key_with_provenance(
    frame: &FrameMetadata,
    has_explicit: bool,
    explicit_offsets: &[(Duration, i64)],
    duplicate_spatial: bool,
    occurrence: i64,
) -> (String, i64, bool, bool) {
    if frame.has_temporal_position {
        return (
            format!("position:{}", frame.temporal_position),
            frame.temporal_position,
            true,
            false,
        );
    }
    if has_explicit {
        if frame.has_offset
            && let Some(position) = nearest_offset(explicit_offsets, frame.offset)
        {
            return (format!("position:{position}"), position, true, false);
        }
        let position = occurrence + 1;
        return (format!("position:{position}"), position, true, true);
    }
    if frame.has_offset {
        return (format!("offset:{}", frame.offset), 0, false, false);
    }
    if frame.has_trigger {
        return (format!("trigger:{}", frame.trigger), 0, false, false);
    }
    if frame.has_phase {
        return (
            format!("phase:{}", format_float(frame.phase, 'g', -1)),
            0,
            false,
            false,
        );
    }
    if duplicate_spatial {
        return (format!("occurrence:{occurrence:09}"), 0, false, true);
    }
    ("static".into(), 0, false, false)
}
fn build_stacks(frames: &[FrameMetadata]) -> Vec<Stack> {
    let mut by_id: HashMap<String, Vec<FrameMetadata>> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    for frame in frames {
        let mut id = frame.stack_id.trim_matches(is_unicode_space).to_string();
        if id.is_empty() {
            id = "default".into();
        }
        if !by_id.contains_key(&id) {
            order.push(id.clone());
        }
        by_id.entry(id).or_default().push(frame.clone());
    }
    order.sort();
    order
        .into_iter()
        .map(|id| {
            let mut items = by_id.remove(&id).unwrap_or_default();
            slice_stable(&mut items, |a, b| {
                if a.has_in_stack_position
                    && b.has_in_stack_position
                    && a.in_stack_position != b.in_stack_position
                {
                    return a.in_stack_position < b.in_stack_position;
                }
                if a.has_spatial_position
                    && b.has_spatial_position
                    && a.spatial_position != b.spatial_position
                {
                    return a.spatial_position < b.spatial_position;
                }
                a.frame_index < b.frame_index
            });
            Stack {
                id,
                frame_indices: items.iter().map(|f| f.frame_index).collect(),
            }
        })
        .collect()
}
fn merge_timing(
    point: &mut TimePoint,
    frame: &FrameMetadata,
    earliest: Instant,
    has_acquisition_time: bool,
) {
    if frame.has_offset && (!point.has_offset || frame.offset < point.offset) {
        point.offset = frame.offset;
        point.has_offset = true;
    } else if !frame.has_offset && has_acquisition_time && frame.has_acquisition_time {
        let offset = frame.acquisition_time.sub(earliest);
        if offset >= 0 && (!point.has_offset || offset < point.offset) {
            point.offset = offset;
            point.has_offset = true;
        }
    }
    if frame.has_duration && (!point.has_duration || frame.duration > point.duration) {
        point.duration = frame.duration;
        point.has_duration = true;
    }
    if frame.has_trigger && !point.has_trigger {
        point.trigger = frame.trigger;
        point.has_trigger = true;
    }
    if frame.has_phase && !point.has_phase {
        point.phase = frame.phase;
        point.has_phase = true;
    }
}
fn less_time_point(a: &TimePoint, b: &TimePoint, a_key: &str, b_key: &str) -> bool {
    if a.has_position && b.has_position && a.temporal_position != b.temporal_position {
        return a.temporal_position < b.temporal_position;
    }
    if a.has_position != b.has_position {
        return a.has_position;
    }
    if a.has_offset && b.has_offset && a.offset != b.offset {
        return a.offset < b.offset;
    }
    if a.has_offset != b.has_offset {
        return a.has_offset;
    }
    if a.has_trigger && b.has_trigger && a.trigger != b.trigger {
        return a.trigger < b.trigger;
    }
    if a.has_phase && b.has_phase && a.phase != b.phase {
        return a.phase < b.phase;
    }
    a_key < b_key
}
fn irregular_timing(points: &[TimePoint]) -> bool {
    if points.len() < 3 {
        return false;
    }
    let mut prior: Duration = 0;
    let mut have_prior = false;
    for index in 1..points.len() {
        if !points[index - 1].has_offset || !points[index].has_offset {
            continue;
        }
        let delta = points[index].offset.wrapping_sub(points[index - 1].offset);
        if delta <= 0 {
            return true;
        }
        if have_prior && abs_duration(delta.wrapping_sub(prior)) > MILLISECOND {
            return true;
        }
        prior = delta;
        have_prior = true;
    }
    false
}
fn abs_duration(value: Duration) -> Duration {
    if value < 0 {
        value.wrapping_neg()
    } else {
        value
    }
}
fn is_unicode_space(c: char) -> bool {
    c.is_whitespace()
}
fn spatial_millis(position: f64) -> i64 {
    fmath::f64_to_i64((position * 1000.0).round())
}
fn has_duplicate_spatial_position(frames: &[FrameMetadata]) -> bool {
    let mut counts: HashMap<String, i64> = HashMap::new();
    for frame in frames {
        if !frame.has_spatial_position {
            continue;
        }
        let key = format!(
            "{}:{}",
            frame.stack_id.trim_matches(is_unicode_space),
            spatial_millis(frame.spatial_position)
        );
        let count = counts.entry(key).or_insert(0);
        *count += 1;
        if *count > 1 {
            return true;
        }
    }
    false
}
fn frame_spatial_key(frame: &FrameMetadata) -> String {
    let id = frame.stack_id.trim_matches(is_unicode_space);
    if frame.has_spatial_position {
        return format!("{id}:z:{}", spatial_millis(frame.spatial_position));
    }
    if frame.has_in_stack_position {
        return format!("{id}:i:{}", frame.in_stack_position);
    }
    format!("{id}:frame:{}", frame.frame_index)
}
