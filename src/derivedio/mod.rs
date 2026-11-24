#![forbid(unsafe_code)]
use crate::object::element_setters::{
    fd_element, fl_element, format_ds, format_is, sl_element, ul_element, us_element,
};
use crate::object::{
    DataSet, Element, ElementHeader, Error, File, Kind, Object, Tag, UNDEFINED_LENGTH, Value, Vr,
    transfer,
};
pub const TAG_MEDIA_STORAGE_SOP_CLASS_UID: Tag = Tag::new(0x0002, 0x0002);
pub const TAG_MEDIA_STORAGE_SOP_INSTANCE_UID: Tag = Tag::new(0x0002, 0x0003);
pub const TAG_TRANSFER_SYNTAX_UID: Tag = Tag::new(0x0002, 0x0010);
pub const TAG_SOP_CLASS_UID: Tag = Tag::new(0x0008, 0x0016);
pub const TAG_SOP_INSTANCE_UID: Tag = Tag::new(0x0008, 0x0018);
pub const TAG_CONTENT_DATE: Tag = Tag::new(0x0008, 0x0023);
pub const TAG_CONTENT_TIME: Tag = Tag::new(0x0008, 0x0033);
pub const TAG_MODALITY: Tag = Tag::new(0x0008, 0x0060);
pub const TAG_SERIES_DESCR: Tag = Tag::new(0x0008, 0x103E);
pub const TAG_REF_SOP_CLASS_UID: Tag = Tag::new(0x0008, 0x1150);
pub const TAG_REF_SOP_INSTANCE_UID: Tag = Tag::new(0x0008, 0x1155);
pub const TAG_REF_FRAME_NUMBER: Tag = Tag::new(0x0008, 0x1160);
pub const TAG_STUDY_INSTANCE_UID: Tag = Tag::new(0x0020, 0x000D);
pub const TAG_SERIES_INSTANCE_UID: Tag = Tag::new(0x0020, 0x000E);
pub const TAG_FRAME_OF_REFERENCE_UID: Tag = Tag::new(0x0020, 0x0052);
pub const TAG_SAMPLES_PER_PIXEL: Tag = Tag::new(0x0028, 0x0002);
pub const TAG_PHOTOMETRIC_INTERPRETATION: Tag = Tag::new(0x0028, 0x0004);
pub const TAG_NUMBER_OF_FRAMES: Tag = Tag::new(0x0028, 0x0008);
pub const TAG_ROWS: Tag = Tag::new(0x0028, 0x0010);
pub const TAG_COLUMNS: Tag = Tag::new(0x0028, 0x0011);
pub const TAG_BITS_ALLOCATED: Tag = Tag::new(0x0028, 0x0100);
pub const TAG_BITS_STORED: Tag = Tag::new(0x0028, 0x0101);
pub const TAG_HIGH_BIT: Tag = Tag::new(0x0028, 0x0102);
pub const TAG_PIXEL_REPRESENTATION: Tag = Tag::new(0x0028, 0x0103);
pub const TAG_PIXEL_DATA: Tag = Tag::new(0x7FE0, 0x0010);
pub const TAG_SERIES_NUMBER: Tag = Tag::new(0x0020, 0x0011);
pub const TAG_INSTANCE_NUMBER: Tag = Tag::new(0x0020, 0x0013);
pub const TAG_CONTENT_LABEL: Tag = Tag::new(0x0070, 0x0080);
pub const TAG_CONTENT_DESCRIPTION: Tag = Tag::new(0x0070, 0x0081);
pub const TAG_CONTENT_CREATOR_NAME: Tag = Tag::new(0x0070, 0x0084);
pub fn object(elements: Vec<Element>) -> Object {
    Object::from_elements(elements)
}
pub fn file(
    sop_class_uid: &str,
    sop_instance_uid: &str,
    dataset: Option<Object>,
) -> Result<File, Error> {
    if sop_class_uid.trim().is_empty() {
        return Err(Error::sentinel(Kind::MissingSopClassUid));
    }
    if sop_instance_uid.trim().is_empty() {
        return Err(Error::sentinel(Kind::MissingSopInstanceUid));
    }
    let Some(dataset) = dataset else {
        return Err(Error::msg("dicom derived: dataset is required"));
    };
    let syntax = transfer::EXPLICIT_VR_LITTLE_ENDIAN;
    Ok(File {
        preamble: Vec::new(),
        meta: object(vec![
            ui(TAG_MEDIA_STORAGE_SOP_CLASS_UID, sop_class_uid),
            ui(TAG_MEDIA_STORAGE_SOP_INSTANCE_UID, sop_instance_uid),
            ui(TAG_TRANSFER_SYNTAX_UID, syntax.uid),
        ]),
        dataset,
        transfer_syntax: syntax,
        partial_read: None,
        incomplete_pixel_data: None,
    })
}
fn strings_value(tag: Tag, vr: Vr, values: Vec<String>) -> Element {
    Element {
        header: ElementHeader {
            tag,
            vr,
            length: 0,
            length_set: false,
        },
        value: Value::Strings(values),
    }
}
pub fn str_elem(tag: Tag, vr: Vr, value: &str) -> Element {
    strings_value(tag, vr, vec![value.to_string()])
}
pub fn strings(tag: Tag, vr: Vr, values: &[String]) -> Element {
    strings_value(tag, vr, values.to_vec())
}
pub fn ui(tag: Tag, value: &str) -> Element {
    str_elem(tag, Vr::UI, value)
}
pub fn cs(tag: Tag, value: &str) -> Element {
    str_elem(tag, Vr::CS, value)
}
pub fn lo(tag: Tag, value: &str) -> Element {
    str_elem(tag, Vr::LO, value)
}
pub fn sh(tag: Tag, value: &str) -> Element {
    str_elem(tag, Vr::SH, value)
}
pub fn is(tag: Tag, values: &[i64]) -> Element {
    let out = values
        .iter()
        .map(|&v| format_is(v).unwrap_or_default())
        .collect();
    strings_value(tag, Vr::IS, out)
}
pub fn ds(tag: Tag, values: &[f64]) -> Element {
    let out = values
        .iter()
        .map(|&v| format_ds(v).unwrap_or_default())
        .collect();
    strings_value(tag, Vr::DS, out)
}
pub fn us(tag: Tag, values: &[u16]) -> Element {
    us_element(tag, false, values)
}
pub fn ul(tag: Tag, values: &[u32]) -> Element {
    ul_element(tag, false, values)
}
pub fn sl(tag: Tag, values: &[i32]) -> Element {
    sl_element(tag, false, values)
}
pub fn fl(tag: Tag, values: &[f64]) -> Element {
    let single: Vec<f32> = values.iter().map(|&v| v as f32).collect();
    fl_element(tag, false, &single)
}
pub fn fd(tag: Tag, values: &[f64]) -> Element {
    fd_element(tag, false, values)
}
pub fn raw(tag: Tag, vr: Vr, data: Vec<u8>) -> Element {
    Element {
        header: ElementHeader::new(tag, vr, data.len() as u32),
        value: Value::Raw(data),
    }
}
pub fn seq(tag: Tag, items: Vec<DataSet>) -> Element {
    Element {
        header: ElementHeader {
            tag,
            vr: Vr::SQ,
            length: UNDEFINED_LENGTH,
            length_set: true,
        },
        value: Value::Sequence(items),
    }
}
pub fn data_set(mut elements: Vec<Element>) -> DataSet {
    elements.sort_by_key(|e| (e.tag().group, e.tag().element));
    DataSet {
        elements,
        item_offset: None,
    }
}
fn trim_space_nul(s: &str) -> &str {
    s.trim_matches(|c: char| c == '\0' || c.is_whitespace())
}
pub fn clean_string(obj: &Object, tag: Tag) -> String {
    match obj.get_string(tag) {
        Some(v) => trim_space_nul(&v).to_string(),
        None => String::new(),
    }
}
pub fn clean_uid(obj: &Object, tag: Tag) -> String {
    match obj.get_uid(tag) {
        Some(v) => v.trim().to_string(),
        None => String::new(),
    }
}
pub fn int(obj: &Object, tag: Tag) -> i64 {
    ints(Some(obj), tag)
        .and_then(|v| v.first().copied())
        .unwrap_or(0)
}
pub fn ints(obj: Option<&Object>, tag: Tag) -> Option<Vec<i64>> {
    lookup_ints(obj, tag).ok()
}
pub fn lookup_ints(obj: Option<&Object>, tag: Tag) -> Result<Vec<i64>, Error> {
    match obj {
        None => Err(Error::msg(format!("dicom derived: nil object for {tag}"))),
        Some(o) => o.get_ints(tag),
    }
}
pub fn floats(obj: Option<&Object>, tag: Tag) -> Option<Vec<f64>> {
    lookup_floats(obj, tag).ok()
}
pub fn lookup_floats(obj: Option<&Object>, tag: Tag) -> Result<Vec<f64>, Error> {
    match obj {
        None => Err(Error::msg(format!("dicom derived: nil object for {tag}"))),
        Some(o) => o.get_floats(tag),
    }
}
pub fn sequence(obj: &Object, tag: Tag) -> Vec<Object> {
    obj.get_sequence(tag).unwrap_or_default()
}
pub fn uint16_bytes(values: &[u16]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}
pub fn uint16s(data: &[u8]) -> Vec<u16> {
    data.as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect()
}
pub fn same_position(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-3
}
pub fn derived_uid(parts: &[&str]) -> String {
    crate::deid::uid::name_uid(parts)
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Identification {
    pub series_number: i64,
    pub instance_number: i64,
    pub content_label: String,
    pub content_description: String,
    pub content_creator_name: String,
}
impl Identification {
    pub fn elements(&self, default_label: &str) -> Result<Vec<Element>, String> {
        let label = if self.content_label.is_empty() {
            default_label
        } else {
            &self.content_label
        };
        if !valid_code_string(label) {
            return Err(format!(
                "dicom derived: Content Label {} is not a CS value",
                crate::textfmt::quote(label)
            ));
        }
        if self.series_number < 0 || self.instance_number < 0 {
            return Err("dicom derived: negative Series or Instance Number".to_string());
        }
        Ok(vec![
            is(TAG_SERIES_NUMBER, &[self.series_number.max(1)]),
            is(TAG_INSTANCE_NUMBER, &[self.instance_number.max(1)]),
            cs(TAG_CONTENT_LABEL, label),
            lo(TAG_CONTENT_DESCRIPTION, &self.content_description),
            str_elem(TAG_CONTENT_CREATOR_NAME, Vr::PN, &self.content_creator_name),
        ])
    }
}
pub fn read_identification(obj: &Object) -> Identification {
    Identification {
        series_number: int(obj, TAG_SERIES_NUMBER),
        instance_number: int(obj, TAG_INSTANCE_NUMBER),
        content_label: clean_string(obj, TAG_CONTENT_LABEL),
        content_description: clean_string(obj, TAG_CONTENT_DESCRIPTION),
        content_creator_name: clean_string(obj, TAG_CONTENT_CREATOR_NAME),
    }
}
pub fn valid_code_string(value: &str) -> bool {
    if value.is_empty() || value.len() > 16 || value.trim().is_empty() {
        return false;
    }
    value
        .chars()
        .all(|r| r.is_ascii_uppercase() || r.is_ascii_digit() || r == ' ' || r == '_')
}
