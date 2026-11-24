#![allow(clippy::neg_cmp_op_on_partial_ord)]
use super::write::{HeaderSpec, WriteOptions, write_encoded};
use super::{Error, Header, Sentinel, atoi, parse_float, truncate, wrap};
use crate::dicomcolor::{cielab_to_rgb, rgb_to_cielab};
use crate::fmath;
use crate::metaimage::lossy_text;
use crate::nifti::label_table::{fields, trim_space};
use crate::nifti::{self, ByteStream, NewVolumeOptions, ReadLimits, Terminal, Volume};
use crate::object::error::quote_bytes;
use crate::render::GeometryAffine;
use crate::roi::RasterMask;
pub use crate::seg::CodedEntry;
use crate::textfmt::float_v;
use std::collections::{BTreeMap, BTreeSet};
pub type SegmentDef = crate::seg::Segment;
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SlicerSegment {
    pub index: i64,
    pub id: String,
    pub name: String,
    pub label_value: i64,
    pub layer: i64,
    pub color: [f64; 3],
    pub tags: String,
    pub category: CodedEntry,
    pub typ: CodedEntry,
}
impl Header {
    pub fn slicer_segments(&self) -> Result<Vec<SlicerSegment>, Error> {
        let mut indices = BTreeSet::new();
        for key in self.key_values.keys() {
            let Some(rest) = key.strip_prefix(b"Segment") else {
                continue;
            };
            let Some(cut) = rest.iter().position(|&b| b == b'_') else {
                continue;
            };
            if let Some(index) = atoi(&rest[..cut])
                && (0..1 << 16).contains(&index)
            {
                indices.insert(index);
            }
        }
        let mut out = Vec::with_capacity(indices.len());
        for index in indices {
            let prefix = format!("Segment{index}_");
            let value = |name: &str| -> &[u8] {
                let key = format!("{prefix}{name}");
                self.key_values
                    .get(key.as_bytes())
                    .map_or(&[][..], |v| trim_space(v))
            };
            let mut segment = SlicerSegment {
                index,
                id: lossy_text(value("ID")),
                name: lossy_text(value("Name")),
                tags: lossy_text(value("Tags")),
                label_value: 1,
                ..SlicerSegment::default()
            };
            let text = value("LabelValue");
            if !text.is_empty() {
                match atoi(text) {
                    Some(label) if (1..=u16::MAX as i64).contains(&label) => {
                        segment.label_value = label;
                    }
                    _ => {
                        return Err(wrap(
                            Sentinel::MalformedHeader,
                            format!("{prefix}LabelValue {}", quote_bytes(&truncate(text))),
                        ));
                    }
                }
            }
            let text = value("Layer");
            if !text.is_empty() {
                match atoi(text) {
                    Some(layer) if layer >= 0 => segment.layer = layer,
                    _ => {
                        return Err(wrap(
                            Sentinel::MalformedHeader,
                            format!("{prefix}Layer {}", quote_bytes(&truncate(text))),
                        ));
                    }
                }
            }
            let parts = fields(value("Color"));
            if parts.len() == 3 {
                for (i, part) in parts.into_iter().enumerate() {
                    match parse_float(part) {
                        Some(c) if !(c < 0.0 || c > 1.0) => segment.color[i] = c,
                        _ => {
                            return Err(wrap(Sentinel::MalformedHeader, format!("{prefix}Color")));
                        }
                    }
                }
            }
            (segment.category, segment.typ) = terminology_entry(&segment.tags);
            out.push(segment);
        }
        Ok(out)
    }
}
fn terminology_entry(tags: &str) -> (CodedEntry, CodedEntry) {
    for tag in tags.split('|') {
        let Some(value) = tag.strip_prefix("TerminologyEntry:") else {
            continue;
        };
        let parts: Vec<&str> = value.split('~').collect();
        let code = |text: &str| -> CodedEntry {
            let fields: Vec<&str> = text.splitn(3, '^').collect();
            if fields.len() != 3 || fields[0].is_empty() || fields[1].is_empty() {
                return CodedEntry::default();
            }
            CodedEntry {
                coding_scheme_designator: fields[0].to_string(),
                code_value: fields[1].to_string(),
                code_meaning: fields[2].to_string(),
            }
        };
        if parts.len() > 2 {
            return (code(parts[1]), code(parts[2]));
        }
    }
    (CodedEntry::default(), CodedEntry::default())
}
impl SlicerSegment {
    pub fn segment(&self) -> SegmentDef {
        let mut rgb = [0i64; 3];
        for (slot, &c) in rgb.iter_mut().zip(&self.color) {
            *slot = fmath::f64_to_i64((c * 255.0).round());
        }
        SegmentDef {
            number: 0,
            label: self.name.clone(),
            algorithm_type: crate::seg::ALGORITHM_MANUAL.into(),
            recommended_display_cielab: rgb_to_cielab(rgb),
            segmented_property_category: self.category.clone(),
            segmented_property_type: self.typ.clone(),
            ..SegmentDef::default()
        }
    }
    fn slicer_tags(&self) -> String {
        if !self.tags.trim().is_empty() {
            return self.tags.clone();
        }
        let tags = "Segmentation.Status:inprogress|".to_string();
        if self.category.code_value.is_empty() || self.typ.code_value.is_empty() {
            return tags;
        }
        let clean = |s: &str| -> String {
            s.chars()
                .map(|c| match c {
                    '~' | '^' | '|' | '\n' | '\r' => ' ',
                    c => c,
                })
                .collect()
        };
        let code = |c: &CodedEntry| {
            format!(
                "{}^{}^{}",
                clean(&c.coding_scheme_designator),
                clean(&c.code_value),
                clean(&c.code_meaning)
            )
        };
        format!(
            "{tags}TerminologyEntry:{SLICER_CATEGORY_TYPE_CONTEXT}~{}~{}~^^~{SLICER_ANATOMIC_CONTEXT}~^^~^^|",
            code(&self.category),
            code(&self.typ)
        )
    }
}
const SLICER_CATEGORY_TYPE_CONTEXT: &str = "Segmentation category and type - DICOM master list";
const SLICER_ANATOMIC_CONTEXT: &str = "Anatomic codes - DICOM master list";
const SLICER_FALLBACK_COLORS: [[i64; 3]; 8] = [
    [128, 174, 128],
    [241, 214, 145],
    [177, 122, 101],
    [111, 184, 210],
    [216, 101, 79],
    [221, 130, 101],
    [144, 238, 144],
    [192, 104, 88],
];
pub fn new_slicer_segment(segment: &SegmentDef, label_value: i64, layer: i64) -> SlicerSegment {
    let mut name = segment.label.trim().to_string();
    if name.is_empty() {
        name = format!("Segment {}", segment.number);
    }
    let mut rgb = SLICER_FALLBACK_COLORS[((segment.number.max(1) - 1) % 8) as usize];
    if segment.recommended_display_cielab != [0u16; 3] {
        rgb = cielab_to_rgb(segment.recommended_display_cielab);
    }
    let mut out = SlicerSegment {
        name,
        label_value,
        layer,
        category: segment.segmented_property_category.clone(),
        typ: segment.segmented_property_type.clone(),
        ..SlicerSegment::default()
    };
    for (slot, &c) in out.color.iter_mut().zip(&rgb) {
        *slot = fmath::min(fmath::max(c as f64 / 255.0, 0.0), 1.0);
    }
    out
}
pub fn write_slicer_segmentation(
    labelmap: &Volume,
    segments: &[SlicerSegment],
    options: &WriteOptions,
) -> Result<Vec<u8>, Error> {
    let malformed = |detail: String| wrap(Sentinel::MalformedHeader, detail);
    let max_label = match labelmap.header.datatype {
        nifti::DATATYPE_UINT8 => u8::MAX as i64,
        nifti::DATATYPE_UINT16 => u16::MAX as i64,
        other => {
            return Err(wrap(
                Sentinel::Unsupported,
                format!("segmentation datatype {other} (want unsigned 8- or 16-bit labels)"),
            ));
        }
    };
    if labelmap.scaling.applied {
        return Err(wrap(Sentinel::Unsupported, "value scaling"));
    }
    if segments.is_empty() {
        return Err(malformed("no segments".to_string()));
    }
    let d = labelmap.dimensions;
    let layers = d[3].max(1);
    let mut by_slot: BTreeMap<(i64, i64), usize> = BTreeMap::new();
    let mut ids: BTreeSet<String> = BTreeSet::new();
    let mut written: Vec<SlicerSegment> = Vec::with_capacity(segments.len());
    for (index, segment) in segments.iter().enumerate() {
        let mut segment = segment.clone();
        segment.index = index as i64;
        segment.id = segment.id.trim().to_string();
        if segment.id.is_empty() {
            segment.id = format!("Segment_{}", index + 1);
        }
        if segment.name.trim().is_empty() {
            segment.name = segment.id.clone();
        }
        if ids.contains(&segment.id) {
            return Err(malformed(format!(
                "duplicate segment ID {}",
                quote_bytes(&truncate(segment.id.as_bytes()))
            )));
        }
        if segment.layer < 0 || segment.layer >= layers {
            return Err(malformed(format!(
                "segment {index} layer {} of {layers}",
                segment.layer
            )));
        }
        if segment.label_value < 1 || segment.label_value > max_label {
            return Err(malformed(format!(
                "segment {index} label value {}",
                segment.label_value
            )));
        }
        if segment.color.iter().any(|c| !(0.0..=1.0).contains(c)) {
            return Err(malformed(format!(
                "segment {index} colour [{} {} {}]",
                float_v(segment.color[0]),
                float_v(segment.color[1]),
                float_v(segment.color[2])
            )));
        }
        let key = (segment.layer, segment.label_value);
        if by_slot.contains_key(&key) {
            return Err(malformed(format!(
                "two segments use label {} of layer {}",
                segment.label_value, segment.layer
            )));
        }
        by_slot.insert(key, index);
        ids.insert(segment.id.clone());
        written.push(segment);
    }
    let mut extents = vec![[i64::MAX, -1, i64::MAX, -1, i64::MAX, -1]; written.len()];
    for layer in 0..layers {
        let mut voxel = labelmap.index(0, 0, 0, layer) as usize;
        for k in 0..d[2] {
            for j in 0..d[1] {
                for i in 0..d[0] {
                    let label = labelmap.stored(voxel) as i64;
                    voxel += 1;
                    if label == 0 {
                        continue;
                    }
                    let Some(&index) = by_slot.get(&(layer, label)) else {
                        return Err(malformed(format!(
                            "label {label} of layer {layer} has no segment"
                        )));
                    };
                    let e = &mut extents[index];
                    (e[0], e[1]) = (e[0].min(i), e[1].max(i));
                    (e[2], e[3]) = (e[2].min(j), e[3].max(j));
                    (e[4], e[5]) = (e[4].min(k), e[5].max(k));
                }
            }
        }
    }
    let mut key_values: BTreeMap<String, String> = BTreeMap::new();
    for (key, value) in [
        ("Segmentation_MasterRepresentation", "Binary labelmap"),
        (
            "Segmentation_ContainedRepresentationNames",
            "Binary labelmap|",
        ),
        ("Segmentation_ReferenceImageExtentOffset", "0 0 0"),
    ] {
        key_values.insert(key.to_string(), value.to_string());
    }
    for (index, segment) in written.iter().enumerate() {
        let prefix = format!("Segment{index}_");
        let e = extents[index];
        let extent = if e[1] >= 0 {
            format!("{} {} {} {} {} {}", e[0], e[1], e[2], e[3], e[4], e[5])
        } else {
            "0 -1 0 -1 0 -1".to_string()
        };
        let color: Vec<String> = segment.color.iter().map(|&c| float_v(c)).collect();
        for (key, value) in [
            ("ID", segment.id.clone()),
            ("Name", segment.name.clone()),
            ("Color", color.join(" ")),
            ("ColorAutoGenerated", "0".to_string()),
            ("NameAutoGenerated", "0".to_string()),
            ("LabelValue", segment.label_value.to_string()),
            ("Layer", segment.layer.to_string()),
            ("Extent", extent),
            ("Tags", segment.slicer_tags()),
        ] {
            key_values.insert(format!("{prefix}{key}"), value);
        }
    }
    for (key, value) in &options.key_values {
        if key_values.contains_key(key) || key.starts_with("Segment") {
            return Err(malformed(format!(
                "key {} collides with the segmentation keys",
                quote_bytes(&truncate(key.as_bytes()))
            )));
        }
        key_values.insert(key.clone(), value.clone());
    }
    let options = WriteOptions {
        raw: options.raw,
        key_values,
        fourth_axis_kind: "list".to_string(),
    };
    let header = HeaderSpec {
        datatype: labelmap.header.datatype,
        dimensions: d,
        index_to_lps: labelmap.geometry.index_to_lps,
        options: &options,
        list_first: true,
    }
    .header()?;
    if layers > 1 {
        let writes = interleaved_layers(labelmap, layers as usize);
        return Ok(write_encoded(
            header,
            options.raw,
            writes.iter().map(Vec::as_slice),
        ));
    }
    Ok(write_encoded(
        header,
        options.raw,
        labelmap.payload().chunks(nifti::PAYLOAD_CHUNK_BYTES),
    ))
}
fn interleaved_layers(labelmap: &Volume, layers: usize) -> Vec<Vec<u8>> {
    let width = labelmap.bytes_per_voxel();
    let spatial = labelmap.len() / layers;
    let mut writes = Vec::new();
    let mut buffer: Vec<u8> = Vec::with_capacity(64 << 10);
    for n in 0..spatial {
        for layer in 0..layers {
            let value = labelmap.stored(n + layer * spatial) as u16;
            if width == 1 {
                buffer.push(value as u8);
            } else {
                buffer.extend_from_slice(&value.to_le_bytes());
            }
        }
        if buffer.len() >= 63 << 10 {
            writes.push(std::mem::replace(&mut buffer, Vec::with_capacity(64 << 10)));
        }
    }
    writes.push(buffer);
    writes
}
#[derive(Debug, Clone, Default)]
pub struct SlicerSegmentMask {
    pub segment: SegmentDef,
    pub slices: Vec<Option<RasterMask>>,
}
struct LabelLayer {
    data: Vec<u8>,
    last_label: i64,
}
struct LabelLayers {
    layers: Vec<LabelLayer>,
    width: usize,
    voxels: usize,
    limits: ReadLimits,
}
impl LabelLayers {
    fn place(
        &mut self,
        dimensions: &[i64; 3],
        slices: &[Option<RasterMask>],
    ) -> Result<(usize, i64), Error> {
        let mut index = self.layers.len();
        let width = self.width;
        for (candidate, layer) in self.layers.iter_mut().enumerate() {
            if layer.last_label == u16::MAX as i64 {
                continue;
            }
            if !visit_slicer_mask(dimensions, slices, &mut layer.data, width, 0)? {
                index = candidate;
                break;
            }
        }
        if index == self.layers.len() {
            self.check_size(index + 1, self.width)?;
            self.layers.push(LabelLayer {
                data: vec![0u8; self.voxels * self.width],
                last_label: 0,
            });
        }
        let label = self.layers[index].last_label + 1;
        if self.width == 1 && label > u8::MAX as i64 {
            self.widen()?;
        }
        let width = self.width;
        let layer = &mut self.layers[index];
        visit_slicer_mask(dimensions, slices, &mut layer.data, width, label as u16)?;
        layer.last_label = label;
        Ok((index, label))
    }
    fn check_size(&self, layers: usize, width: usize) -> Result<(), Error> {
        let byte_limit = self.limits.max_payload_bytes.min(i64::MAX as u64);
        if layers as u64 > self.limits.max_voxels / self.voxels as u64
            || layers as u64 > byte_limit / width as u64 / self.voxels as u64
        {
            return Err(wrap(
                Sentinel::LimitExceeded,
                format!(
                    "{layers} Slicer layers of {} voxels at {width} bytes per voxel",
                    self.voxels
                ),
            ));
        }
        Ok(())
    }
    fn widen(&mut self) -> Result<(), Error> {
        self.check_size(self.layers.len(), 2)?;
        for layer in &mut self.layers {
            let mut widened = vec![0u8; self.voxels * 2];
            for (j, &label) in layer.data.iter().enumerate() {
                widened[j * 2] = label;
            }
            layer.data = widened;
        }
        self.width = 2;
        Ok(())
    }
}
fn visit_slicer_mask(
    dimensions: &[i64; 3],
    slices: &[Option<RasterMask>],
    data: &mut [u8],
    width: usize,
    label: u16,
) -> Result<bool, Error> {
    let (columns, rows) = (dimensions[0], dimensions[1]);
    for (z, mask) in slices.iter().enumerate() {
        let Some(mask) = mask else {
            continue;
        };
        if mask.columns != columns || mask.rows != rows {
            return Err(wrap(
                Sentinel::MalformedHeader,
                format!(
                    "slice {z} mask size {}x{}, want {columns}x{rows}",
                    mask.columns, mask.rows
                ),
            ));
        }
        for y in 0..rows {
            let row = (z as i64 * rows + y) * columns;
            for run in mask.runs(y) {
                if run.start < 0 || run.end > columns || run.start >= run.end {
                    return Err(wrap(
                        Sentinel::MalformedHeader,
                        format!("slice {z} row {y} mask run [{},{})", run.start, run.end),
                    ));
                }
                for x in run.start..run.end {
                    let offset = (row + x) as usize * width;
                    if label == 0 {
                        if data[offset] != 0 || (width == 2 && data[offset + 1] != 0) {
                            return Ok(true);
                        }
                    } else if width == 1 {
                        data[offset] = label as u8;
                    } else {
                        data[offset..offset + 2].copy_from_slice(&label.to_le_bytes());
                    }
                }
            }
        }
    }
    Ok(false)
}
pub fn pack_slicer_segmentation(
    dimensions: [i64; 3],
    index_to_lps: GeometryAffine,
    masks: &[SlicerSegmentMask],
    limits: ReadLimits,
) -> Result<(Volume, Vec<SlicerSegment>), Error> {
    if masks.is_empty() || !index_to_lps.finite() {
        return Err(wrap(
            Sentinel::MalformedHeader,
            "missing segments or invalid LPS affine",
        ));
    }
    if masks.len() > 1 << 16 {
        return Err(wrap(Sentinel::LimitExceeded, "too many Slicer segments"));
    }
    let mut limits = limits;
    let defaults = nifti::default_read_limits();
    if limits.max_voxels == 0 {
        limits.max_voxels = defaults.max_voxels;
    }
    if limits.max_payload_bytes == 0 {
        limits.max_payload_bytes = defaults.max_payload_bytes;
    }
    let mut packed = LabelLayers {
        layers: Vec::new(),
        width: 1,
        voxels: 1,
        limits,
    };
    let sample_limit = limits
        .max_voxels
        .min(limits.max_payload_bytes)
        .min(i64::MAX as u64);
    for (axis, &size) in dimensions.iter().enumerate() {
        if size < 1 || size > i32::MAX as i64 {
            return Err(wrap(
                Sentinel::MalformedHeader,
                format!("size {size} of axis {axis}"),
            ));
        }
        if size as u64 > sample_limit / packed.voxels as u64 {
            return Err(wrap(
                Sentinel::LimitExceeded,
                format!(
                    "Slicer grid dimensions [{} {} {}]",
                    dimensions[0], dimensions[1], dimensions[2]
                ),
            ));
        }
        packed.voxels *= size as usize;
    }
    let mut segments = Vec::with_capacity(masks.len());
    for (index, mask) in masks.iter().enumerate() {
        if !mask.slices.is_empty() && mask.slices.len() as i64 != dimensions[2] {
            return Err(wrap(
                Sentinel::MalformedHeader,
                format!(
                    "segment {index} has {} slices, want {}",
                    mask.slices.len(),
                    dimensions[2]
                ),
            ));
        }
        let (layer, label) = packed
            .place(&dimensions, &mask.slices)
            .map_err(|err| match err {
                Error::Nrrd { sentinel, message } => Error::Nrrd {
                    sentinel,
                    message: format!("Slicer segment {index}: {message}"),
                },
                other => other,
            })?;
        let mut segment = new_slicer_segment(&mask.segment, label, layer as i64);
        segment.index = index as i64;
        segments.push(segment);
    }
    let datatype = if packed.width == 2 {
        nifti::DATATYPE_UINT16
    } else {
        nifti::DATATYPE_UINT8
    };
    let layer_count = packed.layers.len() as i64;
    let mut data = Vec::with_capacity(packed.voxels * packed.width * packed.layers.len());
    for layer in packed.layers {
        data.extend_from_slice(&layer.data);
    }
    let volume = nifti::new_volume(
        &mut ByteStream::new(data, Terminal::Eof),
        &NewVolumeOptions {
            datatype,
            dimensions: [dimensions[0], dimensions[1], dimensions[2], layer_count],
            index_to_lps,
            big_endian: false,
            limits,
        },
    )
    .map_err(Error::Volume)?;
    Ok((volume, segments))
}
#[derive(Debug, Clone, PartialEq)]
pub enum SegDocumentError {
    Nrrd(Error),
    Seg(nifti::segconv::SegError),
}
impl std::fmt::Display for SegDocumentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SegDocumentError::Nrrd(e) => e.fmt(f),
            SegDocumentError::Seg(e) => e.fmt(f),
        }
    }
}
impl std::error::Error for SegDocumentError {}
impl super::Image {
    pub fn seg_document(
        &self,
        layer: i64,
        options: &nifti::segconv::SegOptions,
    ) -> Result<
        (crate::seg::Document, nifti::segconv::SegReport),
        Box<nifti::segconv::SegDocumentFailure<SegDocumentError>>,
    > {
        use nifti::segconv::SegDocumentFailure;
        let report = nifti::segconv::SegReport::default();
        let segments = self.header.slicer_segments().map_err(|e| {
            Box::new(SegDocumentFailure {
                error: SegDocumentError::Nrrd(e),
                report: report.clone(),
            })
        })?;
        if layer < 0 || layer >= self.volume.dimensions[3] {
            return Err(Box::new(SegDocumentFailure {
                error: SegDocumentError::Nrrd(wrap(
                    Sentinel::MalformedHeader,
                    format!("layer {layer} of {}", self.volume.dimensions[3]),
                )),
                report,
            }));
        }
        let mut options = options.clone();
        if !segments.is_empty() {
            let mut map = BTreeMap::new();
            for segment in &segments {
                if segment.layer == layer {
                    map.insert(segment.label_value, segment.segment());
                }
            }
            options.segments = Some(map);
        }
        options.time_index = layer;
        self.volume.seg_document(&options).map_err(|f| {
            Box::new(SegDocumentFailure {
                error: SegDocumentError::Seg(f.error),
                report: f.report,
            })
        })
    }
}
