use super::{
    Error, Kind, TAG_BITS_ALLOCATED, TAG_BITS_STORED, TAG_COLUMNS,
    TAG_CONCATENATION_FRAME_OFFSET_NUMBER, TAG_CONCATENATION_UID, TAG_FRAME_OF_REFERENCE_UID,
    TAG_IN_CONCATENATION_NUMBER, TAG_IN_CONCATENATION_TOTAL_NUMBER, TAG_NUMBER_OF_FRAMES,
    TAG_PER_FRAME_FUNCTIONAL_GROUPS_SEQUENCE, TAG_PHOTOMETRIC_INTERPRETATION,
    TAG_PIXEL_REPRESENTATION, TAG_REPRESENTATIVE_FRAME_NUMBER, TAG_ROWS, TAG_SAMPLES_PER_PIXEL,
    TAG_SERIES_INSTANCE_UID, TAG_SHARED_FUNCTIONAL_GROUPS_SEQUENCE, TAG_SOP_CLASS_UID,
    TAG_SOP_INSTANCE_UID, TAG_SOP_INSTANCE_UID_OF_CONCATENATION_SOURCE, TAG_STUDY_INSTANCE_UID,
};
use crate::derivedio;
use crate::object::geometry::FrameGeometry;
use crate::object::inherit::new_uid;
use crate::object::is_valid_uid;
use crate::object::writer::WriteFileOptions;
use crate::object::{
    DataSet, Element, ElementHeader, File, Object, Tag, UNDEFINED_LENGTH, Value, Vr,
};
use crate::siemens::trim_space;
use crate::textfmt::quote;
use std::collections::{HashMap, HashSet};
const TAG_EXTENDED_OFFSET_TABLE: Tag = Tag::new(0x7FE0, 0x0001);
const TAG_EXTENDED_OFFSET_TABLE_LENGTHS: Tag = Tag::new(0x7FE0, 0x0002);
const TAG_ENCAPSULATED_TOTAL_LENGTH: Tag = Tag::new(0x7FE0, 0x0003);
const MAX_CONCATENATION_INSTANCES: usize = (1 << 16) - 1;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConcatenationGroup {
    pub uid: String,
    pub files: Vec<usize>,
}
pub fn group_concatenations(files: &[Option<&File>]) -> (Vec<ConcatenationGroup>, Vec<usize>) {
    let mut index: HashMap<String, usize> = HashMap::new();
    let mut groups: Vec<ConcatenationGroup> = Vec::new();
    let mut others = Vec::new();
    for (i, file) in files.iter().enumerate() {
        let uid = file
            .map(|f| clean_uid(&f.dataset, TAG_CONCATENATION_UID))
            .unwrap_or_default();
        if uid.is_empty() {
            others.push(i);
            continue;
        }
        let position = *index.entry(uid.clone()).or_insert_with(|| {
            groups.push(ConcatenationGroup {
                uid,
                files: Vec::new(),
            });
            groups.len() - 1
        });
        groups[position].files.push(i);
    }
    (groups, others)
}
#[derive(Clone, Copy, Debug)]
pub struct ConcatenationInstance<'a> {
    pub file: &'a File,
    pub file_index: usize,
    pub number: i64,
    pub frame_offset: i64,
    pub frames: i64,
}
#[derive(Clone, Debug)]
pub struct Concatenation<'a> {
    pub uid: String,
    pub source_sop_instance_uid: String,
    pub sop_class_uid: String,
    pub instances: Vec<ConcatenationInstance<'a>>,
    frames: i64,
}
const CONSISTENT_ATTRIBUTES: [(Tag, &str); 12] = [
    (TAG_SOP_CLASS_UID, "SOP Class UID"),
    (TAG_STUDY_INSTANCE_UID, "Study Instance UID"),
    (TAG_SERIES_INSTANCE_UID, "Series Instance UID"),
    (
        TAG_SOP_INSTANCE_UID_OF_CONCATENATION_SOURCE,
        "SOP Instance UID of Concatenation Source",
    ),
    (TAG_FRAME_OF_REFERENCE_UID, "Frame of Reference UID"),
    (TAG_ROWS, "Rows"),
    (TAG_COLUMNS, "Columns"),
    (TAG_SAMPLES_PER_PIXEL, "Samples per Pixel"),
    (TAG_PHOTOMETRIC_INTERPRETATION, "Photometric Interpretation"),
    (TAG_BITS_ALLOCATED, "Bits Allocated"),
    (TAG_BITS_STORED, "Bits Stored"),
    (TAG_PIXEL_REPRESENTATION, "Pixel Representation"),
];
pub fn new_concatenation<'a>(files: &[Option<&'a File>]) -> Result<Concatenation<'a>, Error> {
    if files.is_empty() {
        return Err(Error::wrap(Kind::ConcatenationIncomplete, "no instance"));
    }
    let mut problems: Vec<Error> = Vec::new();
    let mut inconsistent =
        |text: String| problems.push(Error::wrap(Kind::ConcatenationInconsistent, text));
    let mut concatenation = Concatenation {
        uid: String::new(),
        source_sop_instance_uid: String::new(),
        sop_class_uid: String::new(),
        instances: Vec::new(),
        frames: 0,
    };
    let mut total = 0i64;
    let mut numbers: HashMap<i64, usize> = HashMap::new();
    let mut first: Option<&Object> = None;
    for (position, file) in files.iter().enumerate() {
        let Some(file) = *file else {
            return Err(Error::wrap(
                Kind::ConcatenationInconsistent,
                format!("instance {position} has no data set"),
            ));
        };
        let dataset = &file.dataset;
        let uid = clean_uid(dataset, TAG_CONCATENATION_UID);
        match first {
            None => {
                first = Some(dataset);
                concatenation.uid = uid.clone();
                concatenation.source_sop_instance_uid =
                    clean_uid(dataset, TAG_SOP_INSTANCE_UID_OF_CONCATENATION_SOURCE);
                concatenation.sop_class_uid = clean_uid(dataset, TAG_SOP_CLASS_UID);
                if uid.is_empty() {
                    return Err(Error::wrap(
                        Kind::ConcatenationInconsistent,
                        "instance without Concatenation UID",
                    ));
                }
            }
            Some(first) => {
                if uid != concatenation.uid {
                    inconsistent(format!(
                        "instance {position} has Concatenation UID {}, not {}",
                        quote(&uid),
                        quote(&concatenation.uid)
                    ));
                }
                for (tag, name) in CONSISTENT_ATTRIBUTES {
                    if !same_value(first, dataset, tag) {
                        inconsistent(format!("instance {position} differs in {name}"));
                    }
                }
            }
        }
        let number = match int_value(dataset, TAG_IN_CONCATENATION_NUMBER) {
            Some(n) if n >= 1 => n,
            _ => {
                inconsistent(format!(
                    "instance {position} has no valid In-concatenation Number"
                ));
                continue;
            }
        };
        if let Some(previous) = numbers.get(&number) {
            inconsistent(format!(
                "instances {previous} and {position} both have In-concatenation Number {number}"
            ));
            continue;
        }
        numbers.insert(number, position);
        if let Some(value) = int_value(dataset, TAG_IN_CONCATENATION_TOTAL_NUMBER) {
            if total != 0 && value != total {
                inconsistent(format!(
                    "instance {position} has In-concatenation Total Number {value}, not {total}"
                ));
            }
            total = total.max(value);
        }
        let frames = int_value(dataset, TAG_NUMBER_OF_FRAMES).unwrap_or(0);
        if frames < 1 {
            inconsistent(format!("instance {position} has no valid Number of Frames"));
        }
        if let Some(items) = dataset.get_sequence(TAG_PER_FRAME_FUNCTIONAL_GROUPS_SEQUENCE)
            && items.len() as i64 != frames
        {
            inconsistent(format!(
                "instance {position} has {} per-frame functional groups for {frames} frames",
                items.len()
            ));
        }
        let offset = int_value(dataset, TAG_CONCATENATION_FRAME_OFFSET_NUMBER);
        if offset.is_none() {
            inconsistent(format!(
                "instance {position} has no Concatenation Frame Offset Number"
            ));
        }
        concatenation.instances.push(ConcatenationInstance {
            file,
            file_index: position,
            number,
            frame_offset: offset.unwrap_or(0),
            frames,
        });
    }
    if concatenation.source_sop_instance_uid.is_empty() {
        inconsistent("no SOP Instance UID of Concatenation Source".to_string());
    }
    if !problems.is_empty() {
        return Err(Error::join(problems));
    }
    concatenation.instances.sort_by_key(|i| i.number);
    let expected = total.max(concatenation.instances[concatenation.instances.len() - 1].number);
    let mut missing: Vec<String> = Vec::new();
    for number in 1..=expected {
        if !numbers.contains_key(&number) {
            missing.push(number.to_string());
        }
    }
    if !missing.is_empty() {
        return Err(Error::wrap(
            Kind::ConcatenationIncomplete,
            format!(
                "{} of {expected} instances, missing In-concatenation Number {}",
                concatenation.instances.len(),
                missing.join(", ")
            ),
        ));
    }
    for instance in &concatenation.instances {
        if instance.frame_offset != concatenation.frames {
            return Err(Error::wrap(
                Kind::ConcatenationInconsistent,
                format!(
                    "instance {} has Concatenation Frame Offset Number {}, want {}",
                    instance.number, instance.frame_offset, concatenation.frames
                ),
            ));
        }
        concatenation.frames += instance.frames;
    }
    Ok(concatenation)
}
impl<'a> Concatenation<'a> {
    pub fn number_of_frames(&self) -> i64 {
        self.frames
    }
    pub fn locate(&self, frame: i64) -> Result<(ConcatenationInstance<'a>, i64), Error> {
        if frame < 0 || frame >= self.frames {
            return Err(Error::wrap(
                Kind::ConcatenationIncomplete,
                format!("frame {frame} outside the {} frames", self.frames),
            ));
        }
        let position = self
            .instances
            .partition_point(|i| i.frame_offset + i.frames <= frame);
        let instance = self.instances[position];
        Ok((instance, frame - instance.frame_offset))
    }
    pub fn functional_group_item(
        &self,
        frame: i64,
        macro_tag: Tag,
    ) -> Result<(Option<Object>, &'static str, Option<crate::object::Error>), Error> {
        let (instance, local) = self.locate(frame)?;
        Ok(instance
            .file
            .dataset
            .functional_group_item(local, macro_tag))
    }
    pub fn frame_geometry_at(&self, frame: i64) -> Result<FrameGeometry, Error> {
        let (instance, local) = self.locate(frame)?;
        Ok(instance.file.dataset.frame_geometry_at(local))
    }
}
pub(crate) fn clean_uid(obj: &Object, tag: Tag) -> String {
    let value = obj.get_string(tag).unwrap_or_default();
    trim_space(&value).trim_end_matches('\0').to_string()
}
pub(crate) fn int_value(obj: &Object, tag: Tag) -> Option<i64> {
    derivedio::ints(Some(obj), tag)?.first().copied()
}
fn same_value(a: &Object, b: &Object, tag: Tag) -> bool {
    if a.has(tag) != b.has(tag) {
        return false;
    }
    if !a.has(tag) {
        return true;
    }
    if let Some(x) = derivedio::ints(Some(a), tag)
        && !x.is_empty()
    {
        let y = derivedio::ints(Some(b), tag).unwrap_or_default();
        return y.len() == x.len() && y[0] == x[0];
    }
    clean_uid(a, tag) == clean_uid(b, tag)
}
#[derive(Clone, Debug, Default)]
pub struct SplitOptions {
    pub max_frames: i64,
    pub max_pixel_bytes: i64,
    pub concatenation_uid: String,
    pub sop_instance_uids: Vec<String>,
    pub write: WriteFileOptions,
}
#[derive(Clone, Debug)]
pub struct ConcatenationPart {
    pub number: i64,
    pub first_frame: i64,
    pub frames: i64,
    pub file: File,
}
fn split_err(detail: impl AsRef<str>) -> Error {
    Error::wrap(Kind::ConcatenationSplit, detail)
}
fn split_uid(uid: &str) -> Result<String, Error> {
    if uid.is_empty() {
        return Ok(new_uid());
    }
    if !is_valid_uid(uid.as_bytes()) {
        return Err(split_err(format!("invalid UID {}", quote(uid))));
    }
    Ok(uid.to_string())
}
fn split_text(tag: Tag, vr: Vr, value: &str) -> Element {
    Element {
        header: ElementHeader {
            tag,
            vr,
            length: 0,
            length_set: false,
        },
        value: Value::Strings(vec![value.to_string()]),
    }
}
fn u16_element(tag: Tag, big: bool, v: u16) -> Element {
    let raw = if big {
        v.to_be_bytes().to_vec()
    } else {
        v.to_le_bytes().to_vec()
    };
    derivedio::raw(tag, Vr::US, raw)
}
pub fn plan_concatenation(
    source: Option<&File>,
    frame_lengths: &[i64],
    opts: &SplitOptions,
) -> Result<Vec<ConcatenationPart>, Error> {
    let Some(source) = source else {
        return Err(split_err("nil source"));
    };
    if opts.max_frames < 0
        || opts.max_pixel_bytes < 0
        || (opts.max_frames == 0 && opts.max_pixel_bytes == 0)
    {
        return Err(split_err("set MaxFrames or MaxPixelBytes"));
    }
    let dataset = &source.dataset;
    let source_uid = clean_uid(dataset, TAG_SOP_INSTANCE_UID);
    if source_uid.is_empty() {
        return Err(split_err("source has no SOP Instance UID"));
    }
    if !clean_uid(dataset, TAG_CONCATENATION_UID).is_empty() {
        return Err(split_err(
            "source is already an instance of a Concatenation",
        ));
    }
    if !dataset.has(TAG_SHARED_FUNCTIONAL_GROUPS_SEQUENCE) {
        return Err(split_err(
            "source has no Shared Functional Groups Sequence (Multi-frame Functional Groups Module)",
        ));
    }
    let frames = int_value(dataset, TAG_NUMBER_OF_FRAMES).unwrap_or(0);
    if frames < 1 || frames != frame_lengths.len() as i64 {
        return Err(split_err(format!(
            "Number of Frames {frames} for {} stored frames",
            frame_lengths.len()
        )));
    }
    let mut per_frame: Option<&[DataSet]> = None;
    if let Some(element) = dataset.get(TAG_PER_FRAME_FUNCTIONAL_GROUPS_SEQUENCE) {
        match &element.value {
            Value::Sequence(items) if items.len() as i64 == frames => per_frame = Some(items),
            _ => {
                return Err(split_err(
                    "Per-Frame Functional Groups Sequence does not have one item per frame",
                ));
            }
        }
    }
    for (i, &length) in frame_lengths.iter().enumerate() {
        if length <= 0 {
            return Err(split_err(format!("frame {} has length {length}", i + 1)));
        }
    }
    let mut spans: Vec<(usize, usize)> = Vec::new();
    let mut bytes = 0i64;
    for (i, &length) in frame_lengths.iter().enumerate() {
        if let Some(last) = spans.last_mut()
            && (opts.max_frames == 0 || (last.1 as i64) < opts.max_frames)
            && (opts.max_pixel_bytes == 0 || length <= opts.max_pixel_bytes - bytes)
        {
            last.1 += 1;
            if opts.max_pixel_bytes > 0 {
                bytes += length;
            }
            continue;
        }
        spans.push((i, 1));
        bytes = length;
    }
    if spans.len() > MAX_CONCATENATION_INSTANCES {
        return Err(split_err(format!(
            "{} instances exceed In-concatenation Number {MAX_CONCATENATION_INSTANCES}",
            spans.len()
        )));
    }
    if !opts.sop_instance_uids.is_empty() && opts.sop_instance_uids.len() != spans.len() {
        return Err(split_err(format!(
            "{} SOP Instance UIDs for {} instances",
            opts.sop_instance_uids.len(),
            spans.len()
        )));
    }
    let concatenation_uid = split_uid(&opts.concatenation_uid)?;
    let representative = int_value(dataset, TAG_REPRESENTATIVE_FRAME_NUMBER);
    let big = dataset.value_big_endian();
    let mut parts = Vec::with_capacity(spans.len());
    let mut seen: HashSet<String> = HashSet::new();
    seen.insert(source_uid.clone());
    seen.insert(concatenation_uid.clone());
    for (index, &(first, count)) in spans.iter().enumerate() {
        let uid = match opts.sop_instance_uids.get(index) {
            Some(uid) => split_uid(uid)?,
            None => split_uid("")?,
        };
        if !seen.insert(uid.clone()) {
            return Err(split_err(format!("UID {uid} is used twice")));
        }
        let mut elements: Vec<Element> = Vec::with_capacity(dataset.len() + 8);
        for element in dataset.elements() {
            let tag = element.tag();
            if tag == Tag::PIXEL_DATA
                || tag == TAG_EXTENDED_OFFSET_TABLE
                || tag == TAG_EXTENDED_OFFSET_TABLE_LENGTHS
                || tag == TAG_ENCAPSULATED_TOTAL_LENGTH
                || tag == TAG_PER_FRAME_FUNCTIONAL_GROUPS_SEQUENCE
                || tag == TAG_NUMBER_OF_FRAMES
                || tag == TAG_SOP_INSTANCE_UID
                || tag == TAG_REPRESENTATIVE_FRAME_NUMBER
                || tag == TAG_CONCATENATION_UID
                || tag == TAG_IN_CONCATENATION_NUMBER
                || tag == TAG_IN_CONCATENATION_TOTAL_NUMBER
                || tag == TAG_CONCATENATION_FRAME_OFFSET_NUMBER
                || tag == TAG_SOP_INSTANCE_UID_OF_CONCATENATION_SOURCE
            {
                continue;
            }
            elements.push(element.clone());
        }
        elements.push(split_text(TAG_SOP_INSTANCE_UID, Vr::UI, &uid));
        elements.push(split_text(TAG_NUMBER_OF_FRAMES, Vr::IS, &count.to_string()));
        elements.push(split_text(
            TAG_CONCATENATION_UID,
            Vr::UI,
            &concatenation_uid,
        ));
        elements.push(split_text(
            TAG_SOP_INSTANCE_UID_OF_CONCATENATION_SOURCE,
            Vr::UI,
            &source_uid,
        ));
        elements.push(u16_element(
            TAG_IN_CONCATENATION_NUMBER,
            big,
            (index + 1) as u16,
        ));
        elements.push(u16_element(
            TAG_IN_CONCATENATION_TOTAL_NUMBER,
            big,
            spans.len() as u16,
        ));
        let offset = first as u32;
        elements.push(derivedio::raw(
            TAG_CONCATENATION_FRAME_OFFSET_NUMBER,
            Vr::UL,
            if big {
                offset.to_be_bytes().to_vec()
            } else {
                offset.to_le_bytes().to_vec()
            },
        ));
        if let Some(items) = per_frame {
            elements.push(Element {
                header: ElementHeader {
                    tag: TAG_PER_FRAME_FUNCTIONAL_GROUPS_SEQUENCE,
                    vr: Vr::SQ,
                    length: UNDEFINED_LENGTH,
                    length_set: true,
                },
                value: Value::Sequence(items[first..first + count].to_vec()),
            });
        }
        if let Some(representative) = representative
            && representative > first as i64
            && representative <= (first + count) as i64
        {
            elements.push(u16_element(
                TAG_REPRESENTATIVE_FRAME_NUMBER,
                big,
                (representative - first as i64) as u16,
            ));
        }
        let mut part = Object::from_elements(elements);
        part.set_value_big_endian(big);
        let mut file = File {
            preamble: Vec::new(),
            meta: Object::new(),
            dataset: part,
            transfer_syntax: source.transfer_syntax,
            partial_read: None,
            incomplete_pixel_data: None,
        };
        file.rebuild_file_meta_with_options(&opts.write)
            .map_err(|e| split_err(format!("instance {} file meta: {e}", index + 1)))?;
        parts.push(ConcatenationPart {
            number: (index + 1) as i64,
            first_frame: first as i64,
            frames: count as i64,
            file,
        });
    }
    Ok(parts)
}
