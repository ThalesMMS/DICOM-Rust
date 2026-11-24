use super::overlap::{PositionKey, frame_position};
use super::write::{
    label_map_bits_allocated, validate_frame_payloads, validate_unsigned_short_fields,
    validate_write_dimensions, write_document,
};
use super::{
    Document, Error, Frame, Kind, LABEL_MAP_SEGMENTATION_STORAGE, MAX_READ_FRAMES, ReferencedImage,
    SEGMENTATION_STORAGE, SEGMENTATION_TYPE_FRACTIONAL, SEGMENTS_OVERLAP_NO, SEGMENTS_OVERLAP_YES,
    Segment, segmentation_type, set_packed_bit_range, validate_representable_segmentation_type,
};
use crate::derivedio as dio;
use crate::object::{Element, Object, UNDEFINED_LENGTH, Value, Vr, writer};
use crate::roi::RasterMask;
use std::collections::{HashMap, HashSet};
#[derive(Debug, Clone, Default)]
pub struct FramePayload {
    pub mask: Option<RasterMask>,
    pub fractional: Vec<u8>,
    pub label_map: Vec<u16>,
}
pub trait FrameSource {
    fn frame(&mut self, index: i64) -> Result<FramePayload, Error>;
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StreamWriteOptions {
    pub max_frames: i64,
    pub max_frame_reads: i64,
    pub max_pixel_buffer_bytes: i64,
    pub max_metadata_bytes: i64,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StreamWriteResult {
    pub frames: i64,
    pub frame_reads: i64,
    pub pixel_buffer_bytes: i64,
    pub metadata_reservation_bytes: i64,
}
#[derive(Debug, Clone, Copy)]
pub struct DocumentFrames<'a>(pub &'a Document);
impl FrameSource for DocumentFrames<'_> {
    fn frame(&mut self, index: i64) -> Result<FramePayload, Error> {
        if index < 0 || index as usize >= self.0.frames.len() {
            return Err(Error::sentinel(Kind::InvalidObject));
        }
        let f = &self.0.frames[index as usize];
        Ok(FramePayload {
            mask: f.mask.clone(),
            fractional: f.fractional.clone(),
            label_map: f.label_map.clone(),
        })
    }
}
fn limit() -> Error {
    Error::sentinel(Kind::ResourceLimitExceeded)
}
pub fn write_frames(
    doc: &Document,
    source: &mut dyn FrameSource,
    limits: StreamWriteOptions,
) -> (StreamWriteResult, Result<Vec<u8>, Error>) {
    let mut result = StreamWriteResult::default();
    let out = write_frames_impl(doc, source, limits, &mut result);
    (result, out)
}
#[doc(hidden)]
pub fn write_frames_context(
    ctx: Option<&dyn crate::roi::Cancellation>,
    doc: &Document,
    source: &mut dyn FrameSource,
    limits: StreamWriteOptions,
) -> (StreamWriteResult, Result<Vec<u8>, Error>) {
    if let Some(e) = ctx.and_then(|c| c.err()) {
        return (StreamWriteResult::default(), Err(Error::other(e)));
    }
    write_frames(doc, source, limits)
}
fn write_frames_impl(
    doc: &Document,
    source: &mut dyn FrameSource,
    limits: StreamWriteOptions,
    result: &mut StreamWriteResult,
) -> Result<Vec<u8>, Error> {
    if limits.max_frames <= 0
        || limits.max_frames > MAX_READ_FRAMES
        || limits.max_frame_reads <= 0
        || limits.max_pixel_buffer_bytes <= 0
        || limits.max_metadata_bytes <= 0
    {
        return Err(Error::wrap(
            Kind::ResourceLimitExceeded,
            "source, destination and finite stream budgets required",
        ));
    }
    validate_write_dimensions(doc)?;
    let frame_count = doc.frames.len() as i64;
    if frame_count > limits.max_frames || frame_count > limits.max_frame_reads / 2 {
        return Err(limit());
    }
    result.metadata_reservation_bytes =
        stream_metadata_reservation(doc, limits.max_metadata_bytes)?;
    let mut header = doc.clone();
    for frame in &mut header.frames {
        frame.mask = None;
        frame.fractional = Vec::new();
        frame.label_map = Vec::new();
    }
    let class = if header.sop_class_uid.is_empty() {
        SEGMENTATION_STORAGE.to_string()
    } else {
        header.sop_class_uid.clone()
    };
    validate_representable_segmentation_type(&class, &header.segmentation_type)?;
    validate_unsigned_short_fields(&header)?;
    let pixels = doc.rows * doc.columns;
    let packed_bytes = (pixels + 7) / 8;
    let mut bits = 1;
    if class == LABEL_MAP_SEGMENTATION_STORAGE {
        bits = label_map_bits_allocated(&header)?;
    } else if segmentation_type(&header) == SEGMENTATION_TYPE_FRACTIONAL {
        bits = 8;
    }
    let frame_bytes = (pixels * bits + 7) / 8;
    result.pixel_buffer_bytes = frame_bytes + 2 * packed_bytes + (32 << 10);
    if result.pixel_buffer_bytes > limits.max_pixel_buffer_bytes {
        return Err(limit());
    }
    let mut stream = SegFrameStream {
        header: &header,
        source,
        class: &class,
        bits,
        reads: 0,
        max_reads: limits.max_frame_reads,
    };
    let outcome = stream.run(frame_bytes as usize, packed_bytes as usize, pixels);
    result.frame_reads = stream.reads;
    let bytes = outcome?;
    result.frames = frame_count;
    Ok(bytes)
}
struct SegFrameStream<'a> {
    header: &'a Document,
    source: &'a mut dyn FrameSource,
    class: &'a str,
    bits: i64,
    reads: i64,
    max_reads: i64,
}
impl SegFrameStream<'_> {
    fn run(
        &mut self,
        frame_bytes: usize,
        packed_bytes: usize,
        pixels: i64,
    ) -> Result<Vec<u8>, Error> {
        let frame_count = self.header.frames.len();
        for i in 0..frame_count {
            self.read(i)?;
        }
        let overlap = if self.class == LABEL_MAP_SEGMENTATION_STORAGE {
            SEGMENTS_OVERLAP_NO
        } else {
            self.overlap(packed_bytes)?
        };
        let mut file = write_document(self.header, false, overlap)?;
        let total = if self.bits == 1 {
            ((pixels * frame_count as i64 + 7) / 8) as usize
        } else {
            frame_bytes * frame_count
        };
        let mut data = vec![0u8; total + total % 2];
        let mut buffer = vec![0u8; frame_bytes];
        let vr = if self.bits > 8 { Vr::OW } else { Vr::OB };
        let length = data.len();
        for i in 0..frame_count {
            let f = self.read(i).map_err(|e| {
                e.with_prefix(format!(
                    "dicom: write value for tag (7FE0,0010) with VR {vr} with length {length}"
                ))
            })?;
            buffer.fill(0);
            match self.bits {
                1 => {
                    self.presence(&f, &mut buffer);
                    let base = i as i64 * pixels;
                    for bit in 0..pixels {
                        if buffer[(bit / 8) as usize] >> (bit % 8) & 1 != 0 {
                            let at = base + bit;
                            data[(at / 8) as usize] |= 1 << (at % 8);
                        }
                    }
                }
                8 if self.class == LABEL_MAP_SEGMENTATION_STORAGE => {
                    for (out, &value) in buffer.iter_mut().zip(&f.label_map) {
                        *out = value as u8;
                    }
                }
                8 => {
                    let n = buffer.len().min(f.fractional.len());
                    buffer[..n].copy_from_slice(&f.fractional[..n]);
                }
                _ => {
                    for (out, &value) in buffer.as_chunks_mut::<2>().0.iter_mut().zip(&f.label_map)
                    {
                        out.copy_from_slice(&value.to_le_bytes());
                    }
                }
            }
            if self.bits != 1 {
                data[i * frame_bytes..(i + 1) * frame_bytes].copy_from_slice(&buffer);
            }
        }
        file.dataset.put(dio::raw(dio::TAG_PIXEL_DATA, vr, data));
        writer::write_file(&file).map_err(Error::other)
    }
    fn read(&mut self, i: usize) -> Result<Frame, Error> {
        if self.reads >= self.max_reads {
            return Err(limit());
        }
        self.reads += 1;
        let payload = self.source.frame(i as i64)?;
        let mut f = self.header.frames[i].clone();
        f.mask = payload.mask;
        f.fractional = payload.fractional;
        f.label_map = payload.label_map;
        let mut check = self.header.clone();
        check.frames = vec![f];
        validate_frame_payloads(&check, self.class)?;
        let f = check.frames.remove(0);
        if self.class == LABEL_MAP_SEGMENTATION_STORAGE
            && self.bits == 8
            && f.label_map.iter().any(|&v| v > 255)
        {
            return Err(Error::sentinel(Kind::InvalidObject));
        }
        Ok(f)
    }
    fn presence(&self, frame: &Frame, buffer: &mut [u8]) {
        buffer.fill(0);
        if segmentation_type(self.header) == SEGMENTATION_TYPE_FRACTIONAL {
            for (i, &value) in frame.fractional.iter().enumerate() {
                if value > 0 && i / 8 < buffer.len() {
                    buffer[i / 8] |= 1 << (i % 8);
                }
            }
        } else if let Some(mask) = &frame.mask {
            for y in 0..mask.rows {
                for run in mask.runs(y) {
                    set_packed_bit_range(
                        buffer,
                        y * self.header.columns + run.start,
                        y * self.header.columns + run.end,
                    );
                }
            }
        }
    }
    fn overlap(&mut self, bytes: usize) -> Result<&'static str, Error> {
        let frames = &self.header.frames;
        let mut groups: HashMap<PositionKey, Vec<usize>> = HashMap::new();
        let mut order: Vec<PositionKey> = Vec::new();
        for (i, f) in frames.iter().enumerate() {
            let key = frame_position(f);
            if !groups.contains_key(&key) {
                order.push(key.clone());
            }
            groups.entry(key).or_default().push(i);
        }
        let (mut a, mut b) = (vec![0u8; bytes], vec![0u8; bytes]);
        for key in &order {
            let group = &groups[key];
            let segments: HashSet<i64> = group.iter().map(|&i| frames[i].segment_number).collect();
            if segments.len() < 2 {
                continue;
            }
            for (offset, &i) in group.iter().enumerate() {
                let rest = &group[offset + 1..];
                if !rest
                    .iter()
                    .any(|&j| frames[i].segment_number != frames[j].segment_number)
                {
                    continue;
                }
                let first = self.read(i)?;
                self.presence(&first, &mut a);
                for &j in rest {
                    if frames[i].segment_number == frames[j].segment_number {
                        continue;
                    }
                    let second = self.read(j)?;
                    self.presence(&second, &mut b);
                    if a.iter().zip(&b).any(|(x, y)| x & y != 0) {
                        return Ok(SEGMENTS_OVERLAP_YES);
                    }
                }
            }
        }
        Ok(SEGMENTS_OVERLAP_NO)
    }
}
struct Reservation {
    used: i64,
    limit: i64,
}
impl Reservation {
    fn add(&mut self, n: i64) -> Result<(), Error> {
        if n < 0 || self.used > self.limit || n > (self.limit - self.used) / 16 {
            return Err(limit());
        }
        self.used += n * 16;
        Ok(())
    }
    fn text(&mut self, s: &str) -> Result<(), Error> {
        self.add(s.len() as i64)
    }
    fn segment(&mut self, s: &Segment) -> Result<(), Error> {
        let a = &s.algorithm_identification;
        for text in [
            &s.label,
            &s.algorithm_type,
            &s.algorithm_name,
            &a.family.code_value,
            &a.family.coding_scheme_designator,
            &a.family.code_meaning,
            &a.name_code.code_value,
            &a.name_code.coding_scheme_designator,
            &a.name_code.code_meaning,
            &a.name,
            &a.version,
            &a.parameters,
            &a.source,
            &s.segmented_property_category.code_value,
            &s.segmented_property_category.coding_scheme_designator,
            &s.segmented_property_category.code_meaning,
            &s.segmented_property_type.code_value,
            &s.segmented_property_type.coding_scheme_designator,
            &s.segmented_property_type.code_meaning,
        ] {
            self.text(text)?;
        }
        Ok(())
    }
    fn reference(&mut self, r: &ReferencedImage) -> Result<(), Error> {
        self.text(&r.series_instance_uid)?;
        self.text(&r.sop_class_uid)?;
        self.text(&r.sop_instance_uid)
    }
    fn walk(&mut self, elements: &[Element], depth: i64, nodes: &mut i64) -> Result<(), Error> {
        if depth > 32 {
            return Err(limit());
        }
        for element in elements {
            if element.tag() == dio::TAG_PIXEL_DATA {
                continue;
            }
            *nodes += 1;
            if *nodes > 100_000 {
                return Err(limit());
            }
            self.add(256)?;
            match &element.value {
                Value::Nil => {
                    if element.header.length_set && element.header.length != UNDEFINED_LENGTH {
                        self.add(i64::from(element.header.length))?;
                    }
                }
                Value::Raw(raw) => self.add(raw.len() as i64)?,
                Value::Strings(values) => {
                    for item in values {
                        self.add(item.len() as i64)?;
                    }
                }
                Value::Sequence(items) => {
                    for item in items {
                        self.walk(&item.elements, depth + 1, nodes)?;
                    }
                }
                Value::Fragments { .. } | Value::BulkData(_) => {}
            }
        }
        Ok(())
    }
}
fn stream_metadata_reservation(doc: &Document, limit_bytes: i64) -> Result<i64, Error> {
    let mut r = Reservation {
        used: (64 << 10) + doc.frames.len() as i64 * (32 << 10),
        limit: limit_bytes,
    };
    if r.used > r.limit {
        return Err(limit());
    }
    for text in [
        &doc.sop_class_uid,
        &doc.sop_instance_uid,
        &doc.study_instance_uid,
        &doc.series_instance_uid,
        &doc.frame_of_reference_uid,
        &doc.identification.content_label,
        &doc.identification.content_description,
        &doc.identification.content_creator_name,
        &doc.content_date,
        &doc.content_time,
        &doc.segmentation_type,
        &doc.fractional_type,
        &doc.segments_overlap,
    ] {
        r.text(text)?;
    }
    r.text(doc.geometry.primary_issue.as_str())?;
    for issue in doc.geometry.issues() {
        r.text(issue.as_str())?;
    }
    if let Some(equipment) = &doc.common.equipment {
        r.text(&equipment.manufacturer)?;
        r.text(&equipment.model_name)?;
        r.text(&equipment.device_serial_number)?;
        r.text(&equipment.software_versions)?;
    }
    for segment in &doc.segments {
        r.add(4096)?;
        r.segment(segment)?;
    }
    for reference in &doc.referenced_images {
        r.add(4096 + reference.frames.len() as i64 * 8)?;
        r.reference(reference)?;
    }
    for frame in &doc.frames {
        r.add(frame.referenced_sop_instance_uid.len() as i64)?;
    }
    if let Some(source) = &doc.common.source {
        let mut nodes = 0;
        r.walk(source_elements(source), 0, &mut nodes)?;
    }
    Ok(r.used)
}
fn source_elements(source: &Object) -> &[Element] {
    source.elements()
}
