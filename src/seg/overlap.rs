use super::samples::sample_mask;
use super::{Document, Error, Frame, LABEL_MAP_SEGMENTATION_STORAGE, valid_slice_geometry};
use crate::fmath;
use crate::roi::RasterMask;
use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet, HashMap};
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SegmentOverlap {
    pub first: i64,
    pub second: i64,
    pub pixels: i64,
    pub positions: i64,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OverlapReport {
    pub segments: Vec<i64>,
    pub overlaps: Vec<SegmentOverlap>,
}
impl OverlapReport {
    pub fn overlapping(&self, a: i64, b: i64) -> bool {
        let (a, b) = if a > b { (b, a) } else { (a, b) };
        self.overlaps.iter().any(|o| o.first == a && o.second == b)
    }
    pub fn disjoint_groups(&self) -> Vec<Vec<i64>> {
        let mut groups: Vec<Vec<i64>> = Vec::new();
        for &segment in &self.segments {
            let slot = groups
                .iter()
                .position(|group| !group.iter().any(|&m| self.overlapping(segment, m)));
            match slot {
                Some(index) => groups[index].push(segment),
                None => groups.push(vec![segment]),
            }
        }
        groups
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct PositionKey {
    kind: u8,
    x: i64,
    y: i64,
    z: i64,
    uid: String,
    frame: i64,
}
pub(crate) fn frame_position(frame: &Frame) -> PositionKey {
    let mut key = PositionKey {
        kind: 3,
        x: 0,
        y: 0,
        z: 0,
        uid: String::new(),
        frame: 0,
    };
    if valid_slice_geometry(&frame.geometry) {
        let quantize = |value: f64| fmath::f64_to_i64((value * 1000.0).round());
        let origin = frame.geometry.origin;
        key.kind = 1;
        key.x = quantize(origin.x);
        key.y = quantize(origin.y);
        key.z = quantize(origin.z);
    } else if !frame.referenced_sop_instance_uid.is_empty() {
        key.kind = 2;
        key.uid = frame.referenced_sop_instance_uid.clone();
        key.frame = frame.referenced_frame_number;
    } else {
        key.frame = frame.slice_index;
    }
    key
}
pub(crate) fn presence_mask<'a>(doc: &Document, frame: &'a Frame) -> Option<Cow<'a, RasterMask>> {
    if !frame.fractional.is_empty() {
        return Some(Cow::Owned(sample_mask(
            doc.columns,
            doc.rows,
            &frame.fractional,
            |v| v > 0,
        )));
    }
    frame.mask.as_ref().map(Cow::Borrowed)
}
pub fn overlaps(doc: &Document) -> Result<OverlapReport, Error> {
    let mut present: BTreeSet<i64> = BTreeSet::new();
    if doc.sop_class_uid == LABEL_MAP_SEGMENTATION_STORAGE {
        for frame in &doc.frames {
            for value in &frame.label_map {
                present.insert(i64::from(*value));
            }
        }
        return Ok(OverlapReport {
            segments: present.into_iter().collect(),
            overlaps: Vec::new(),
        });
    }
    let mut positions: HashMap<PositionKey, BTreeMap<i64, RasterMask>> = HashMap::new();
    for frame in &doc.frames {
        let Some(mask) = presence_mask(doc, frame) else {
            continue;
        };
        if mask.is_empty() {
            continue;
        }
        present.insert(frame.segment_number);
        let group = positions.entry(frame_position(frame)).or_default();
        match group.get_mut(&frame.segment_number) {
            None => {
                group.insert(frame.segment_number, mask.into_owned());
            }
            Some(existing) => existing.union(&mask),
        }
    }
    let mut pixels: BTreeMap<(i64, i64), i64> = BTreeMap::new();
    let mut position_counts: HashMap<(i64, i64), i64> = HashMap::new();
    for group in positions.values() {
        if group.len() < 2 {
            continue;
        }
        let mut at_position: BTreeSet<(i64, i64)> = BTreeSet::new();
        let mut events: Vec<(i64, i64, i64)> = Vec::new();
        let mut active: Vec<i64> = Vec::new();
        for y in 0..doc.rows {
            events.clear();
            for (&segment, mask) in group {
                for run in mask.runs(y) {
                    events.push((run.start, 1, segment));
                    events.push((run.end, -1, segment));
                }
            }
            if events.len() < 4 {
                continue;
            }
            events.sort_by_key(|&(x, delta, _)| (x, delta));
            active.clear();
            let mut previous = events[0].0;
            for &(x, delta, segment) in &events {
                let width = x - previous;
                if width > 0 && active.len() > 1 {
                    for i in 0..active.len() {
                        for j in i + 1..active.len() {
                            let key = (active[i], active[j]);
                            *pixels.entry(key).or_insert(0) += width;
                            at_position.insert(key);
                        }
                    }
                }
                previous = x;
                let index = active.partition_point(|&s| s < segment);
                if delta > 0 {
                    active.insert(index, segment);
                } else if index < active.len() && active[index] == segment {
                    active.remove(index);
                }
            }
        }
        for key in at_position {
            *position_counts.entry(key).or_insert(0) += 1;
        }
    }
    Ok(OverlapReport {
        segments: present.into_iter().collect(),
        overlaps: pixels
            .into_iter()
            .map(|((first, second), count)| SegmentOverlap {
                first,
                second,
                pixels: count,
                positions: position_counts.get(&(first, second)).copied().unwrap_or(0),
            })
            .collect(),
    })
}
