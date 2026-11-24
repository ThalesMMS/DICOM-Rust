#![forbid(unsafe_code)]
use crate::dictionary::reservations;
use crate::fmath;
use crate::numtext::parse_float;
use crate::object::{Element, Object, Tag, Vr};
use crate::textfmt::{float_v, to_upper};
use std::fmt;
pub const ERR_INVALID_MAPPING: &str = "dicom: invalid real world value mapping";
pub const ERR_INCOMPLETE_MAPPING: &str =
    "dicom: real world value mapping lacks first/last value mapped";
const TAG_REAL_WORLD_VALUE_MAPPING_SEQUENCE: Tag = Tag::new(0x0040, 0x9096);
const TAG_FIRST_VALUE_MAPPED: Tag = Tag::new(0x0040, 0x9216);
const TAG_LAST_VALUE_MAPPED: Tag = Tag::new(0x0040, 0x9211);
const TAG_DOUBLE_FIRST_VALUE_MAPPED: Tag = Tag::new(0x0040, 0x9214);
const TAG_DOUBLE_LAST_VALUE_MAPPED: Tag = Tag::new(0x0040, 0x9213);
const TAG_REAL_WORLD_VALUE_LUT_DATA: Tag = Tag::new(0x0040, 0x9212);
const TAG_REAL_WORLD_VALUE_INTERCEPT: Tag = Tag::new(0x0040, 0x9224);
const TAG_REAL_WORLD_VALUE_SLOPE: Tag = Tag::new(0x0040, 0x9225);
const TAG_LUT_EXPLANATION: Tag = Tag::new(0x0028, 0x3003);
const TAG_LUT_LABEL: Tag = Tag::new(0x0040, 0x9210);
const TAG_MEASUREMENT_UNITS_CODE_SEQUENCE: Tag = Tag::new(0x0040, 0x08EA);
const TAG_QUANTITY_DEFINITION_SEQUENCE: Tag = Tag::new(0x0040, 0x9220);
const TAG_VALUE_TYPE: Tag = Tag::new(0x0040, 0xA040);
const TAG_CONCEPT_NAME_CODE_SEQUENCE: Tag = Tag::new(0x0040, 0xA043);
const TAG_CONCEPT_CODE_SEQUENCE: Tag = Tag::new(0x0040, 0xA168);
const TAG_CODE_VALUE: Tag = Tag::new(0x0008, 0x0100);
const TAG_CODING_SCHEME_DESIGNATOR: Tag = Tag::new(0x0008, 0x0102);
const TAG_CODE_MEANING: Tag = Tag::new(0x0008, 0x0104);
const TAG_PER_FRAME_FUNCTIONAL_GROUPS: Tag = Tag::new(0x5200, 0x9230);
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Code {
    pub code_value: String,
    pub coding_scheme_designator: String,
    pub code_meaning: String,
}
pub type Unit = Code;
impl Code {
    pub fn is_empty(&self) -> bool {
        self.code_value.is_empty()
            && self.coding_scheme_designator.is_empty()
            && self.code_meaning.is_empty()
    }
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Mapping {
    pub first_value_mapped: f64,
    pub last_value_mapped: f64,
    pub slope: f64,
    pub intercept: f64,
    pub lut: Option<Vec<f64>>,
    pub label: String,
    pub explanation: String,
    pub units: Unit,
    pub quantity: Code,
    pub range_missing: bool,
}
impl Mapping {
    pub fn map(&self, stored: f64) -> Option<f64> {
        if stored.is_nan()
            || !self.range_missing
                && (stored < self.first_value_mapped || stored > self.last_value_mapped)
        {
            return None;
        }
        if let Some(lut) = &self.lut {
            let index = fmath::f64_to_i64(stored - self.first_value_mapped);
            if index < 0 || index >= lut.len() as i64 {
                return None;
            }
            return Some(lut[index as usize]);
        }
        Some(stored * self.slope + self.intercept)
    }
    pub fn name(&self) -> &str {
        [&self.label, &self.explanation, &self.quantity.code_meaning]
            .into_iter()
            .find(|name| !name.is_empty())
            .map_or("RWV", String::as_str)
    }
    pub fn unit_text(&self) -> &str {
        let units = &self.units;
        if equal_fold_ucum(&units.coding_scheme_designator) && !units.code_value.is_empty() {
            if units.code_value == "1" {
                return "";
            }
            return &units.code_value;
        }
        if !units.code_meaning.is_empty() {
            return &units.code_meaning;
        }
        &units.code_value
    }
}
fn equal_fold_ucum(s: &str) -> bool {
    s.eq_ignore_ascii_case("UCUM")
}
#[derive(Debug, Clone, PartialEq)]
pub struct Value {
    pub index: usize,
    pub mapping: Mapping,
    pub value: f64,
}
pub fn values_for(mappings: &[Mapping], stored: f64) -> Vec<Value> {
    mappings
        .iter()
        .enumerate()
        .filter_map(|(index, mapping)| {
            let value = mapping.map(stored)?;
            value.is_finite().then(|| Value {
                index,
                mapping: mapping.clone(),
                value,
            })
        })
        .collect()
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    InvalidMapping,
    IncompleteMapping,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemError {
    pub index: usize,
    pub kind: ErrorKind,
    pub message: String,
}
impl fmt::Display for ItemError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "item {}: {}", self.index, self.message)
    }
}
impl std::error::Error for ItemError {}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub items: Vec<ItemError>,
}
impl Error {
    pub fn is(&self, kind: ErrorKind) -> bool {
        self.items.iter().any(|item| item.kind == kind)
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, item) in self.items.iter().enumerate() {
            if i > 0 {
                f.write_str("\n")?;
            }
            write!(f, "{item}")?;
        }
        Ok(())
    }
}
impl std::error::Error for Error {}
pub fn mappings_at(obj: Option<&Object>, frame_index: i64) -> (Vec<Mapping>, Option<Error>) {
    let mut out = Vec::new();
    let mut errs = Vec::new();
    for (index, item) in mapping_items(obj, frame_index).iter().enumerate() {
        match mapping_from_item(item) {
            Ok(mapping) => out.push(mapping),
            Err(failure) => {
                let (mapping, kind, message) = *failure;
                errs.push(ItemError {
                    index,
                    kind,
                    message,
                });
                if mapping.range_missing {
                    out.push(mapping);
                }
            }
        }
    }
    let err = (!errs.is_empty()).then_some(Error { items: errs });
    (out, err)
}
fn mapping_items(obj: Option<&Object>, frame_index: i64) -> Vec<Object> {
    let Some(obj) = obj else {
        return Vec::new();
    };
    let (items, _, _) =
        obj.functional_group_items(frame_index, TAG_REAL_WORLD_VALUE_MAPPING_SEQUENCE);
    if !items.is_empty() {
        return items;
    }
    obj.get_sequence(TAG_REAL_WORLD_VALUE_MAPPING_SEQUENCE)
        .unwrap_or_default()
}
type ItemFailure = Box<(Mapping, ErrorKind, String)>;
fn slope_intercept(item: &Object, m: Mapping) -> Result<(Mapping, f64, f64), ItemFailure> {
    let slope = first_number(item, &[TAG_REAL_WORLD_VALUE_SLOPE]);
    let intercept = first_number(item, &[TAG_REAL_WORLD_VALUE_INTERCEPT]);
    match (slope, intercept) {
        (Some(slope), Some(intercept)) => Ok((m, slope, intercept)),
        _ => Err(Box::new((
            m,
            ErrorKind::InvalidMapping,
            format!("{ERR_INVALID_MAPPING}: neither LUT nor slope and intercept"),
        ))),
    }
}
fn mapping_from_item(item: &Object) -> Result<Mapping, ItemFailure> {
    let mut m = Mapping::default();
    let first = first_number(
        item,
        &[TAG_DOUBLE_FIRST_VALUE_MAPPED, TAG_FIRST_VALUE_MAPPED],
    );
    let last = first_number(item, &[TAG_DOUBLE_LAST_VALUE_MAPPED, TAG_LAST_VALUE_MAPPED]);
    let lut = numbers(item, TAG_REAL_WORLD_VALUE_LUT_DATA).filter(|lut| !lut.is_empty());
    let (first, last) = match (first, last) {
        (Some(first), Some(last)) if last >= first => (first, last),
        (None, None) if lut.is_none() => {
            let (mut m, slope, intercept) = slope_intercept(item, m)?;
            m.slope = slope;
            m.intercept = intercept;
            m.range_missing = true;
            m.first_value_mapped = f64::NEG_INFINITY;
            m.last_value_mapped = f64::INFINITY;
            fill_description(&mut m, item);
            return Err(Box::new((
                m,
                ErrorKind::IncompleteMapping,
                ERR_INCOMPLETE_MAPPING.to_string(),
            )));
        }
        _ => {
            return Err(Box::new((
                m,
                ErrorKind::InvalidMapping,
                format!("{ERR_INVALID_MAPPING}: first/last value mapped"),
            )));
        }
    };
    m.first_value_mapped = first;
    m.last_value_mapped = last;
    if let Some(lut) = lut {
        if lut.len() as f64 != last - first + 1.0 {
            let message = format!(
                "{ERR_INVALID_MAPPING}: {} LUT entries for {}..{}",
                lut.len(),
                float_v(first),
                float_v(last)
            );
            return Err(Box::new((m, ErrorKind::InvalidMapping, message)));
        }
        m.lut = Some(lut);
    } else {
        let (mapping, slope, intercept) = slope_intercept(item, m)?;
        m = mapping;
        m.slope = slope;
        m.intercept = intercept;
    }
    fill_description(&mut m, item);
    Ok(m)
}
fn fill_description(m: &mut Mapping, item: &Object) {
    m.label = trimmed(item, TAG_LUT_LABEL);
    m.explanation = trimmed(item, TAG_LUT_EXPLANATION);
    m.units = first_code(item, TAG_MEASUREMENT_UNITS_CODE_SEQUENCE);
    m.quantity = quantity(item);
}
fn quantity(item: &Object) -> Code {
    let rows = item
        .get_sequence(TAG_QUANTITY_DEFINITION_SEQUENCE)
        .unwrap_or_default();
    for row in &rows {
        let name = first_code(row, TAG_CONCEPT_NAME_CODE_SEQUENCE);
        if name.code_value != "246205007" && name.code_value != "G-C1C6" {
            continue;
        }
        let value_type = trimmed(row, TAG_VALUE_TYPE);
        if !value_type.is_empty() && value_type != "CODE" {
            continue;
        }
        let code = first_code(row, TAG_CONCEPT_CODE_SEQUENCE);
        if !code.is_empty() {
            return code;
        }
    }
    Code::default()
}
fn first_code(o: &Object, tag: Tag) -> Code {
    let Some(items) = o.get_sequence(tag) else {
        return Code::default();
    };
    let Some(item) = items.first() else {
        return Code::default();
    };
    Code {
        code_value: trimmed(item, TAG_CODE_VALUE),
        coding_scheme_designator: trimmed(item, TAG_CODING_SCHEME_DESIGNATOR),
        code_meaning: trimmed(item, TAG_CODE_MEANING),
    }
}
fn trimmed(o: &Object, tag: Tag) -> String {
    o.get_string(tag).unwrap_or_default().trim().to_string()
}
fn first_number(o: &Object, tags: &[Tag]) -> Option<f64> {
    tags.iter().find_map(|&tag| {
        let values = numbers(o, tag)?;
        values.first().copied().filter(|v| v.is_finite())
    })
}
fn numbers(o: &Object, tag: Tag) -> Option<Vec<f64>> {
    numbers_as(o, tag, None)
}
fn numbers_as(o: &Object, tag: Tag, un_vr: Option<Vr>) -> Option<Vec<f64>> {
    let element = o.get(tag)?;
    let mut vr = element.vr();
    if vr == Vr::UN
        && let Some(assumed) = un_vr
    {
        vr = assumed;
    }
    if matches!(vr, Vr::DS | Vr::IS) {
        return string_values(o, element)?
            .iter()
            .map(|value| parse_float(value.trim()).ok())
            .collect();
    }
    let raw = element.raw()?;
    let big = o.value_big_endian();
    let width = match vr {
        Vr::FD => 8,
        Vr::FL | Vr::UL | Vr::SL => 4,
        Vr::US | Vr::SS => 2,
        _ => return None,
    };
    if raw.len() % width != 0 {
        return None;
    }
    let u16_at = |b: &[u8]| {
        let bytes = [b[0], b[1]];
        if big {
            u16::from_be_bytes(bytes)
        } else {
            u16::from_le_bytes(bytes)
        }
    };
    let u32_at = |b: &[u8]| {
        let bytes = [b[0], b[1], b[2], b[3]];
        if big {
            u32::from_be_bytes(bytes)
        } else {
            u32::from_le_bytes(bytes)
        }
    };
    let u64_at = |b: &[u8]| {
        let mut bytes = [0u8; 8];
        bytes.copy_from_slice(&b[..8]);
        if big {
            u64::from_be_bytes(bytes)
        } else {
            u64::from_le_bytes(bytes)
        }
    };
    Some(
        raw.chunks_exact(width)
            .map(|b| match vr {
                Vr::FD => f64::from_bits(u64_at(b)),
                Vr::FL => f64::from(f32::from_bits(u32_at(b))),
                Vr::US => f64::from(u16_at(b)),
                Vr::SS => f64::from(u16_at(b) as i16),
                Vr::UL => f64::from(u32_at(b)),
                _ => f64::from(u32_at(b) as i32),
            })
            .collect(),
    )
}
fn string_values(o: &Object, element: &Element) -> Option<Vec<String>> {
    if let Some(values) = o.get_strings(element.tag()) {
        return Some(values);
    }
    let raw = element.raw()?;
    let text = String::from_utf8_lossy(raw);
    Some(
        text.trim_end_matches([' ', '\0'])
            .split('\\')
            .map(str::to_string)
            .collect(),
    )
}
pub const PHILIPS_GROUP: u16 = 0x2005;
pub const PHILIPS_RESCALE_CREATOR: &str = "Philips MR Imaging DD 005";
pub const PHILIPS_RESCALE_INTERCEPT_OFFSET: u8 = 0x09;
pub const PHILIPS_RESCALE_SLOPE_OFFSET: u8 = 0x0A;
pub const PHILIPS_SCALE_SLOPE_CREATOR: &str = "Philips MR Imaging DD 001";
pub const PHILIPS_SCALE_SLOPE_OFFSET: u8 = 0x0E;
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PhilipsScale {
    pub rescale_intercept: f64,
    pub rescale_slope: f64,
    pub scale_slope: f64,
    pub has_scale_slope: bool,
}
impl PhilipsScale {
    pub fn display_value(&self, stored: f64) -> f64 {
        stored * self.rescale_slope + self.rescale_intercept
    }
    pub fn floating_point_value(&self, stored: f64) -> Option<f64> {
        if !self.has_scale_slope || self.rescale_slope == 0.0 {
            return None;
        }
        Some(self.display_value(stored) / (self.rescale_slope * self.scale_slope))
    }
}
pub fn philips_scale_at(obj: Option<&Object>, frame_index: i64) -> Option<PhilipsScale> {
    let obj = obj?;
    if frame_index >= 0
        && let Some(frames) = obj.get_sequence(TAG_PER_FRAME_FUNCTIONAL_GROUPS)
        && let Some(frame) = frames.get(frame_index as usize)
        && let Some(scale) = philips_scale_in(frame)
    {
        return Some(scale);
    }
    philips_scale_in(obj)
}
fn philips_scale_in(o: &Object) -> Option<PhilipsScale> {
    let intercept = private_number(
        o,
        PHILIPS_RESCALE_CREATOR,
        PHILIPS_RESCALE_INTERCEPT_OFFSET,
        Vr::DS,
    );
    let slope = private_number(
        o,
        PHILIPS_RESCALE_CREATOR,
        PHILIPS_RESCALE_SLOPE_OFFSET,
        Vr::DS,
    );
    let (intercept, mut slope) = (intercept?, slope?);
    if slope == 0.0 {
        slope = 1.0;
    }
    let mut scale = PhilipsScale {
        rescale_intercept: intercept,
        rescale_slope: slope,
        ..PhilipsScale::default()
    };
    if let Some(ss) = private_number(
        o,
        PHILIPS_SCALE_SLOPE_CREATOR,
        PHILIPS_SCALE_SLOPE_OFFSET,
        Vr::FL,
    ) && ss != 0.0
    {
        scale.scale_slope = ss;
        scale.has_scale_slope = true;
    }
    Some(scale)
}
fn private_number(o: &Object, creator: &str, offset: u8, vr: Vr) -> Option<f64> {
    let tag = private_tag(o, creator, offset)?;
    numbers_as(o, tag, Some(vr))?.first().copied()
}
fn private_tag(o: &Object, creator: &str, offset: u8) -> Option<Tag> {
    let upper = to_upper(creator);
    for name in [creator, upper.as_str()] {
        if let Ok(tag) = reservations::private_tag(o, PHILIPS_GROUP, name, offset) {
            return Some(tag);
        }
    }
    let mut block: Option<u16> = None;
    for element in o.elements() {
        let tag = element.tag();
        if tag.group != PHILIPS_GROUP || tag.element < 0x0010 || tag.element > 0x00FF {
            continue;
        }
        let Some(values) = string_values(o, element) else {
            continue;
        };
        if values.len() != 1 {
            continue;
        }
        let name = values[0].trim();
        if name == creator || name == upper {
            if block.is_some() {
                return None;
            }
            block = Some(tag.element);
        }
    }
    block.map(|block| Tag::new(PHILIPS_GROUP, (block << 8) | u16::from(offset)))
}
