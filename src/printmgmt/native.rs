use super::*;
use crate::display::{DisplayLut, Lut};
use crate::object::DataSet;
pub const PRESENTATION_LUT_SOP_CLASS_UID: &str = "1.2.840.10008.5.1.1.23";
pub const ANNOTATION_BOX_SOP_CLASS_UID: &str = "1.2.840.10008.5.1.1.15";
pub const MAX_NATIVE_LUTS: i64 = 64;
pub const MAX_NATIVE_ANNOTATION_BOXES: i64 = 64;
pub const MAX_NATIVE_LUT_BYTES: i64 = 512 << 10;
pub const TAG_PRESENTATION_LUT_SEQUENCE: Tag = Tag::new(0x2050, 0x0010);
pub const TAG_PRESENTATION_LUT_SHAPE: Tag = Tag::new(0x2050, 0x0020);
pub const TAG_LUT_DESCRIPTOR: Tag = Tag::new(0x0028, 0x3002);
pub const TAG_LUT_DATA: Tag = Tag::new(0x0028, 0x3006);
pub const TAG_LUT_EXPLANATION: Tag = Tag::new(0x0028, 0x3003);
pub const TAG_REFERENCED_ANNOTATION_BOX_SEQUENCE: Tag = Tag::new(0x2010, 0x0520);
pub const TAG_ANNOTATION_POSITION: Tag = Tag::new(0x2030, 0x0010);
pub const TAG_ANNOTATION_TEXT: Tag = Tag::new(0x2030, 0x0020);
pub const TAG_SPECIFIC_CHARACTER_SET: Tag = Tag::new(0x0008, 0x0005);
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AnnotationFormat {
    pub id: String,
    pub slots: i64,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NativeLimits {
    pub max_presentation_luts: i64,
    pub max_lut_bytes: i64,
    pub max_annotation_boxes: i64,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NativeServiceOptions {
    pub presentation_lut: bool,
    pub annotation_formats: Vec<AnnotationFormat>,
    pub limits: NativeLimits,
}
pub(crate) fn normalize_native_options(
    backend: &dyn Backend,
    p: Option<&NativeServiceOptions>,
) -> Result<Option<NativeServiceOptions>> {
    let Some(p) = p else {
        return Ok(None);
    };
    let mut out = p.clone();
    let wants = p.presentation_lut || !p.annotation_formats.is_empty();
    if wants {
        let capable = backend.native_capabilities();
        let refuse = match capable {
            None => true,
            Some((luts, annotations)) => {
                (p.presentation_lut && !luts) || (!p.annotation_formats.is_empty() && !annotations)
            }
        };
        if refuse {
            return Err(Kind::NativeUnsupported.into());
        }
    }
    let l = &mut out.limits;
    if l.max_presentation_luts == 0 {
        l.max_presentation_luts = 16;
    }
    if l.max_lut_bytes == 0 {
        l.max_lut_bytes = 128 << 10;
    }
    if l.max_annotation_boxes == 0 {
        l.max_annotation_boxes = 16;
    }
    if l.max_presentation_luts < 1
        || l.max_presentation_luts > MAX_NATIVE_LUTS
        || l.max_lut_bytes < 1
        || l.max_lut_bytes > MAX_NATIVE_LUT_BYTES
        || l.max_annotation_boxes < 1
        || l.max_annotation_boxes > MAX_NATIVE_ANNOTATION_BOXES
        || p.annotation_formats.len() > 32
    {
        return Err(Kind::InvalidAttributes.into());
    }
    let max_boxes = l.max_annotation_boxes;
    let mut seen = std::collections::HashSet::new();
    for f in &out.annotation_formats {
        if !valid_native_cs(&f.id)
            || f.id.is_empty()
            || f.slots < 1
            || f.slots > max_boxes
            || !seen.insert(f.id.clone())
        {
            return Err(invalid(&[TAG_ANNOTATION_DISPLAY_FORMAT_ID]));
        }
    }
    Ok(Some(out))
}
pub(crate) fn valid_native_cs(s: &str) -> bool {
    !s.is_empty() && s.len() <= 16 && s.trim_matches(' ') == s && cs_pattern(s)
}
fn validate_native_density(d: &PrintDensity) -> Result<()> {
    let bad = || {
        invalid(&[
            TAG_MIN_DENSITY,
            TAG_MAX_DENSITY,
            TAG_ILLUMINATION,
            TAG_REFLECTED_AMBIENT_LIGHT,
        ])
    };
    for v in [
        d.min_density,
        d.max_density,
        d.illumination,
        d.reflected_ambient_light,
    ] {
        if v.is_nan() || v.is_infinite() || v < 0.0 {
            return Err(bad());
        }
    }
    if d.max_density <= d.min_density
        || d.illumination <= 0.0
        || d.luminance(d.min_density)
            .partial_cmp(&d.luminance(d.max_density))
            != Some(std::cmp::Ordering::Greater)
        || d.luminance(d.max_density) <= 0.0
    {
        return Err(bad());
    }
    Ok(())
}
impl FilmBoxPresentation {
    pub fn native_print_density(&self) -> Result<PrintDensity> {
        for v in [
            self.min_density,
            self.max_density,
            self.illumination,
            self.reflected_ambient_light,
        ] {
            if !(0..=65535).contains(&v) {
                return Err(invalid(&[
                    TAG_MIN_DENSITY,
                    TAG_MAX_DENSITY,
                    TAG_ILLUMINATION,
                    TAG_REFLECTED_AMBIENT_LIGHT,
                ]));
            }
        }
        let mut d = DEFAULT_PRINT_DENSITY;
        if self.min_density_set || self.min_density > 0 {
            d.min_density = self.min_density as f64 / 100.0;
        }
        if self.max_density_set || self.max_density > 0 {
            d.max_density = self.max_density as f64 / 100.0;
        }
        if self.illumination_set || self.illumination > 0 {
            d.illumination = self.illumination as f64;
        }
        if self.reflected_ambient_light_set || self.reflected_ambient_light > 0 {
            d.reflected_ambient_light = self.reflected_ambient_light as f64;
        }
        validate_native_density(&d)?;
        Ok(d)
    }
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AnnotationBox {
    pub sop_instance_uid: String,
    pub position: i64,
    pub text: String,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Annotation {
    pub position: i64,
    pub text: String,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AnnotationBoxReference {
    pub sop_instance_uid: String,
    pub position: i64,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FilmBoxReferences {
    pub image_boxes: Vec<ImageBoxReference>,
    pub annotation_boxes: Vec<AnnotationBoxReference>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PresentationLutInstance {
    pub sop_instance_uid: String,
    pub lut: PresentationLut,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NativeFilmOptions {
    pub presentation_lut: Option<PresentationLut>,
    pub annotation_display_format_id: String,
    pub annotations: Vec<Annotation>,
}
impl NativeFilmOptions {
    pub fn specified(&self) -> bool {
        self.presentation_lut.is_some()
            || !self.annotation_display_format_id.is_empty()
            || !self.annotations.is_empty()
    }
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PresentationLut {
    pub shape: String,
    pub input_bits: i64,
    pub output_bits: i64,
    pub entries: Vec<u16>,
    pub explanation: String,
}
impl PresentationLut {
    pub fn validate(&self) -> Result<()> {
        if !self.shape.is_empty() {
            if (self.shape != "IDENTITY" && self.shape != "LIN OD")
                || self.input_bits != 0
                || self.output_bits != 0
                || !self.entries.is_empty()
                || !self.explanation.is_empty()
            {
                return Err(invalid(&[
                    TAG_PRESENTATION_LUT_SHAPE,
                    TAG_PRESENTATION_LUT_SEQUENCE,
                ]));
            }
            return Ok(());
        }
        if (self.input_bits != 8 && self.input_bits != 12)
            || self.output_bits < 10
            || self.output_bits > 16
            || self.entries.len() as i64 != 1i64 << self.input_bits
        {
            return Err(invalid(&[TAG_LUT_DESCRIPTOR, TAG_LUT_DATA]));
        }
        if !valid_native_text(&self.explanation, "") {
            return Err(invalid(&[TAG_LUT_EXPLANATION]));
        }
        let maximum = 1u32 << self.output_bits;
        if self.entries.iter().any(|v| u32::from(*v) >= maximum) {
            return Err(invalid(&[TAG_LUT_DATA]));
        }
        Ok(())
    }
    pub(crate) fn validate_image(&self, image: Option<&Image>) -> Result<()> {
        let Some(image) = image else {
            return Ok(());
        };
        self.validate()?;
        if image.color
            || (image.bits != 8 && image.bits != 12)
            || (self.shape.is_empty() && self.input_bits != image.bits)
        {
            return Err(invalid(&[TAG_REFERENCED_PRESENTATION_LUT, TAG_BITS_STORED]));
        }
        Ok(())
    }
    pub fn display_lut(
        &self,
        input_bits: i64,
        density: &PrintDensity,
        calibration: Option<&DisplayLut>,
    ) -> Result<Lut> {
        self.validate()?;
        if input_bits != 8 && input_bits != 12 {
            return Err(invalid(&[TAG_BITS_STORED]));
        }
        let other = |e: &dyn std::fmt::Display| Error::Other(e.to_string());
        match self.shape.as_str() {
            "LIN OD" => {
                validate_native_density(density)?;
                match calibration {
                    Some(c) => density
                        .lin_od_presentation_lut_for(Some(c), input_bits)
                        .map_err(|e| other(&e)),
                    None => density
                        .lin_od_presentation_lut(input_bits)
                        .map_err(|e| other(&e)),
                }
            }
            "IDENTITY" => {
                let count = 1u32 << input_bits;
                let entries = (0..count)
                    .map(|i| (i * 65535 / (count - 1)) as u16)
                    .collect();
                Lut::new(&[count as i64, 0, 16], entries).map_err(|e| other(&e))
            }
            _ => {
                if input_bits != self.input_bits {
                    return Err(invalid(&[TAG_LUT_DESCRIPTOR, TAG_BITS_STORED]));
                }
                Lut::new(
                    &[self.entries.len() as i64, 0, self.output_bits],
                    self.entries.clone(),
                )
                .map_err(|e| other(&e))
            }
        }
    }
}
pub fn build_presentation_lut(p: &PresentationLut) -> Result<Object> {
    p.validate()?;
    if !p.shape.is_empty() {
        return Ok(Object::from_elements(vec![text(
            TAG_PRESENTATION_LUT_SHAPE,
            Vr::CS,
            &p.shape,
        )]));
    }
    let raw: Vec<u8> = p.entries.iter().flat_map(|v| v.to_le_bytes()).collect();
    let descriptor: Vec<u8> = [p.entries.len() as u16, 0, p.output_bits as u16]
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .collect();
    let mut fields = vec![
        raw_element(TAG_LUT_DESCRIPTOR, Vr::US, descriptor),
        raw_element(TAG_LUT_DATA, Vr::OW, raw),
    ];
    if !p.explanation.is_empty() {
        fields.push(text(TAG_LUT_EXPLANATION, Vr::LO, &p.explanation));
    }
    Ok(Object::from_elements(vec![sequence_of(
        TAG_PRESENTATION_LUT_SEQUENCE,
        vec![DataSet {
            elements: fields,
            item_offset: None,
        }],
    )]))
}
pub(crate) fn parse_presentation_lut_budget(
    ds: Option<&Object>,
    budget: i64,
) -> Result<PresentationLut> {
    let mut p = PresentationLut::default();
    let missing = |tags: &[Tag]| {
        failure(
            STATUS_MISSING_ATTRIBUTE,
            "",
            Kind::InvalidAttributes.into(),
            tags,
        )
    };
    let Some(ds) = ds else {
        return Err(missing(&[
            TAG_PRESENTATION_LUT_SHAPE,
            TAG_PRESENTATION_LUT_SEQUENCE,
        ]));
    };
    let (shape, has_shape) = presentation_text(ds, TAG_PRESENTATION_LUT_SHAPE, Vr::CS)?;
    let table = ds.get(TAG_PRESENTATION_LUT_SEQUENCE);
    let has_table = table.is_some();
    if !has_shape && !has_table {
        return Err(missing(&[
            TAG_PRESENTATION_LUT_SHAPE,
            TAG_PRESENTATION_LUT_SEQUENCE,
        ]));
    }
    if has_shape == has_table {
        return Err(invalid(&[
            TAG_PRESENTATION_LUT_SHAPE,
            TAG_PRESENTATION_LUT_SEQUENCE,
        ]));
    }
    if has_shape {
        if shape.is_empty() {
            return Err(failure(
                STATUS_MISSING_ATTRIBUTE_VALUE,
                "",
                Kind::InvalidAttributes.into(),
                &[TAG_PRESENTATION_LUT_SHAPE],
            ));
        }
        p.shape = shape;
        p.validate()?;
        return Ok(p);
    }
    let e = table.unwrap();
    match &e.value {
        Value::Sequence(items) if e.vr() == Vr::SQ && items.len() == 1 => {}
        _ => return Err(invalid(&[TAG_PRESENTATION_LUT_SEQUENCE])),
    }
    let items = ds
        .get_sequence(TAG_PRESENTATION_LUT_SEQUENCE)
        .unwrap_or_default();
    let Some(item) = items.first() else {
        return Err(invalid(&[TAG_PRESENTATION_LUT_SEQUENCE]));
    };
    let desc = native_us_values(item, TAG_LUT_DESCRIPTOR, 3)?;
    p.input_bits = match desc[0] {
        256 => 8,
        4096 => 12,
        _ => return Err(invalid(&[TAG_LUT_DESCRIPTOR])),
    };
    p.output_bits = desc[2] as i64;
    if desc[1] != 0 || p.output_bits < 10 || p.output_bits > 16 {
        return Err(invalid(&[TAG_LUT_DESCRIPTOR]));
    }
    let Some(data) = item.get(TAG_LUT_DATA) else {
        return Err(missing(&[TAG_LUT_DATA]));
    };
    if data.vr() != Vr::OW {
        return Err(invalid(&[TAG_LUT_DATA]));
    }
    let count = desc[0] as i64;
    if 2 * count > budget {
        return Err(failure(
            STATUS_RESOURCE_LIMITATION,
            "",
            Kind::InvalidAttributes.into(),
            &[TAG_LUT_DATA],
        ));
    }
    match &data.value {
        Value::Raw(v) => {
            if v.len() as i64 != 2 * count {
                return Err(invalid(&[TAG_LUT_DATA]));
            }
            let big = item.value_big_endian();
            p.entries = v
                .as_chunks::<2>()
                .0
                .iter()
                .map(|c| {
                    if big {
                        u16::from_be_bytes([c[0], c[1]])
                    } else {
                        u16::from_le_bytes([c[0], c[1]])
                    }
                })
                .collect();
        }
        _ => return Err(invalid(&[TAG_LUT_DATA])),
    }
    let (value, present) = native_text(item, TAG_LUT_EXPLANATION, "")?;
    if present {
        p.explanation = value;
    }
    p.validate()?;
    Ok(p)
}
pub(crate) fn native_us_values(ds: &Object, tag: Tag, count: usize) -> Result<Vec<u16>> {
    let Some(e) = ds.get(tag) else {
        return Err(failure(
            STATUS_MISSING_ATTRIBUTE,
            "",
            Kind::InvalidAttributes.into(),
            &[tag],
        ));
    };
    if e.vr() != Vr::US {
        return Err(invalid(&[tag]));
    }
    if let Value::Raw(v) = &e.value
        && v.len() == 2 * count
    {
        let big = ds.value_big_endian();
        return Ok(v
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| {
                if big {
                    u16::from_be_bytes([c[0], c[1]])
                } else {
                    u16::from_le_bytes([c[0], c[1]])
                }
            })
            .collect());
    }
    Err(invalid(&[tag]))
}
pub(crate) fn presentation_lut_reference(uid: &str) -> Element {
    sequence_of(
        TAG_REFERENCED_PRESENTATION_LUT,
        vec![reference_item(PRESENTATION_LUT_SOP_CLASS_UID, uid)],
    )
}
