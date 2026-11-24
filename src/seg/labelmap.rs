use super::overlap::{OverlapReport, frame_position, overlaps};
use super::{
    ALGORITHM_MANUAL, Document, Error, Frame, Kind, LABEL_MAP_SEGMENTATION_STORAGE,
    SEGMENTATION_STORAGE, SEGMENTATION_TYPE_BINARY, SEGMENTATION_TYPE_LABEL_MAP, Segment, invalid,
    normalize_segment, segmentation_type,
};
use crate::roi::RasterMask;
use std::collections::{BTreeMap, HashMap};
#[derive(Debug, Clone, Default)]
pub struct LabelMapOptions {
    pub sop_instance_uid: String,
    pub series_instance_uid: String,
    pub bits_allocated: i64,
    pub background: Option<Segment>,
}
fn first_non_empty(a: &str, b: &str) -> String {
    if a.is_empty() { b } else { a }.to_string()
}
pub fn to_label_map(
    doc: &Document,
    opts: &LabelMapOptions,
) -> Result<(Document, OverlapReport), (OverlapReport, Error)> {
    let none = OverlapReport::default;
    if doc.sop_class_uid == LABEL_MAP_SEGMENTATION_STORAGE
        || segmentation_type(doc) != SEGMENTATION_TYPE_BINARY
    {
        return Err((
            none(),
            Error::wrap(
                Kind::UnsupportedSegmentationType,
                format!("{} segmentation is not BINARY", segmentation_type(doc)),
            ),
        ));
    }
    if opts.sop_instance_uid.is_empty() {
        return Err((
            none(),
            invalid("the label map needs its own SOP Instance UID"),
        ));
    }
    let report = overlaps(doc).map_err(|e| (none(), e))?;
    if !report.overlaps.is_empty() {
        let n = report.overlaps.len();
        return Err((
            report,
            Error::wrap(
                Kind::OverlappingSegments,
                format!("{n} segment pairs share pixels"),
            ),
        ));
    }
    let pixels = doc.rows.wrapping_mul(doc.columns);
    if pixels <= 0 {
        return Err((
            report,
            invalid(format!("Rows={} Columns={}", doc.rows, doc.columns)),
        ));
    }
    let mut background = opts.background.clone().unwrap_or_else(|| Segment {
        label: "Background".to_string(),
        algorithm_type: ALGORITHM_MANUAL.to_string(),
        ..Segment::default()
    });
    background.number = 0;
    let mut out = Document {
        sop_class_uid: LABEL_MAP_SEGMENTATION_STORAGE.to_string(),
        sop_instance_uid: opts.sop_instance_uid.clone(),
        study_instance_uid: doc.study_instance_uid.clone(),
        series_instance_uid: first_non_empty(&opts.series_instance_uid, &doc.series_instance_uid),
        frame_of_reference_uid: doc.frame_of_reference_uid.clone(),
        rows: doc.rows,
        columns: doc.columns,
        segmentation_type: SEGMENTATION_TYPE_LABEL_MAP.to_string(),
        label_map_bits_allocated: opts.bits_allocated,
        segments: vec![normalize_segment(&background, 0)],
        referenced_images: doc.referenced_images.clone(),
        geometry: doc.geometry.clone(),
        ..Document::default()
    };
    out.segments[0].number = 0;
    for (i, segment) in doc.segments.iter().enumerate() {
        let segment = normalize_segment(segment, i as i64 + 1);
        if segment.number == 0 {
            return Err((
                report,
                invalid("segment number 0 is the label-map background"),
            ));
        }
        out.segments.push(segment);
    }
    let mut index = HashMap::new();
    for frame in &doc.frames {
        let frame_index = *index.entry(frame_position(frame)).or_insert_with(|| {
            out.frames.push(Frame {
                slice_index: frame.slice_index,
                referenced_sop_instance_uid: frame.referenced_sop_instance_uid.clone(),
                referenced_frame_number: frame.referenced_frame_number,
                geometry: frame.geometry,
                label_map: vec![0; pixels as usize],
                ..Frame::default()
            });
            out.frames.len() - 1
        });
        let Some(mask) = &frame.mask else {
            continue;
        };
        if frame.segment_number <= 0 || frame.segment_number > 0xFFFF {
            return Err((
                report,
                invalid(format!(
                    "frame segment number {} is outside US",
                    frame.segment_number
                )),
            ));
        }
        let labels = &mut out.frames[frame_index].label_map;
        mask.for_each_run(|y, run| {
            for x in run.start..run.end {
                labels[(y * doc.columns + x) as usize] = frame.segment_number as u16;
            }
        });
    }
    out.frames.sort_by_key(|f| f.slice_index);
    Ok((out, report))
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BinaryOptions {
    pub sop_instance_uid: String,
    pub series_instance_uid: String,
    pub include_zero: bool,
}
pub fn to_binary(
    doc: &Document,
    opts: &BinaryOptions,
) -> Result<(Document, BTreeMap<i64, i64>), Error> {
    if doc.sop_class_uid != LABEL_MAP_SEGMENTATION_STORAGE {
        return Err(Error::wrap(
            Kind::UnsupportedSegmentationType,
            format!("{} segmentation is not a label map", segmentation_type(doc)),
        ));
    }
    if opts.sop_instance_uid.is_empty() {
        return Err(invalid(
            "the binary segmentation needs its own SOP Instance UID",
        ));
    }
    if doc.columns <= 0 || doc.rows <= 0 {
        return Err(invalid(format!(
            "Rows={} Columns={}",
            doc.rows, doc.columns
        )));
    }
    let mut segments = doc.segments.clone();
    segments.sort_by_key(|s| s.number);
    let mut numbers: BTreeMap<i64, i64> = BTreeMap::new();
    let mut out = Document {
        sop_class_uid: SEGMENTATION_STORAGE.to_string(),
        sop_instance_uid: opts.sop_instance_uid.clone(),
        study_instance_uid: doc.study_instance_uid.clone(),
        series_instance_uid: first_non_empty(&opts.series_instance_uid, &doc.series_instance_uid),
        frame_of_reference_uid: doc.frame_of_reference_uid.clone(),
        rows: doc.rows,
        columns: doc.columns,
        segmentation_type: SEGMENTATION_TYPE_BINARY.to_string(),
        referenced_images: doc.referenced_images.clone(),
        geometry: doc.geometry.clone(),
        ..Document::default()
    };
    for mut segment in segments {
        if segment.number == 0 && !opts.include_zero {
            continue;
        }
        if numbers.contains_key(&segment.number) {
            return Err(invalid(format!(
                "segment number {} is not unique",
                segment.number
            )));
        }
        let number = out.segments.len() as i64 + 1;
        numbers.insert(segment.number, number);
        segment.number = number;
        out.segments.push(segment);
    }
    let mut frames: Vec<(i64, usize, Frame)> = Vec::new();
    for (order, frame) in doc.frames.iter().enumerate() {
        let mut masks: BTreeMap<i64, RasterMask> = BTreeMap::new();
        for (index, &value) in frame.label_map.iter().enumerate() {
            let Some(&number) = numbers.get(&i64::from(value)) else {
                if value != 0 || opts.include_zero {
                    return Err(invalid(format!("label-map value {value} has no segment")));
                }
                continue;
            };
            let index = index as i64;
            masks
                .entry(number)
                .or_insert_with(|| RasterMask::new(doc.columns, doc.rows))
                .set(index % doc.columns, index / doc.columns, true);
        }
        for (number, mask) in masks {
            frames.push((
                number,
                order,
                Frame {
                    segment_number: number,
                    slice_index: frame.slice_index,
                    referenced_sop_instance_uid: frame.referenced_sop_instance_uid.clone(),
                    referenced_frame_number: frame.referenced_frame_number,
                    geometry: frame.geometry,
                    mask: Some(mask),
                    ..Frame::default()
                },
            ));
        }
    }
    frames.sort_by_key(|(segment, order, _)| (*segment, *order));
    out.frames = frames.into_iter().map(|(_, _, f)| f).collect();
    Ok((out, numbers))
}
