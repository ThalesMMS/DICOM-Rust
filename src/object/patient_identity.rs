#![forbid(unsafe_code)]
use super::error::Error;
use super::obj::{Object, TAG_SPECIFIC_CHARACTER_SET};
use super::text::trim_text_value_bytes;
use super::types::{Element, ElementHeader, Tag, Value, Vr};
use super::writer::encode_element_explicit_le;
const TAG_PATIENTS_NAME: Tag = Tag::new(0x0010, 0x0010);
const TAG_PATIENT_ID: Tag = Tag::new(0x0010, 0x0020);
const TAG_ISSUER_OF_PATIENT_ID: Tag = Tag::new(0x0010, 0x0021);
const TAG_TYPE_OF_PATIENT_ID: Tag = Tag::new(0x0010, 0x0022);
const TAG_ISSUER_QUALIFIERS: Tag = Tag::new(0x0010, 0x0024);
const TAG_PATIENTS_BIRTH_DATE: Tag = Tag::new(0x0010, 0x0030);
const TAG_PATIENTS_SEX: Tag = Tag::new(0x0010, 0x0040);
pub const PATIENT_IDENTITY_TAGS: [Tag; 7] = [
    TAG_PATIENTS_NAME,
    TAG_PATIENT_ID,
    TAG_ISSUER_OF_PATIENT_ID,
    TAG_TYPE_OF_PATIENT_ID,
    TAG_ISSUER_QUALIFIERS,
    TAG_PATIENTS_BIRTH_DATE,
    TAG_PATIENTS_SEX,
];
pub const ERR_PATIENT_IDENTITY_CHARACTER_SET: &str =
    "dicom: patient identity uses a character set the target does not declare";
fn is_type2(tag: Tag) -> bool {
    matches!(
        tag,
        TAG_PATIENTS_NAME | TAG_PATIENT_ID | TAG_PATIENTS_BIRTH_DATE | TAG_PATIENTS_SEX
    )
}
pub(super) fn string_values(element: &Element) -> Vec<String> {
    let vr = element.vr();
    match &element.value {
        Value::Raw(raw) => {
            let parts: Vec<&[u8]> = raw.split(|&b| b == b'\\').collect();
            if parts.len() == 1 && trim_text_value_bytes(vr, parts[0]).is_empty() {
                return Vec::new();
            }
            parts
                .iter()
                .map(|p| String::from_utf8_lossy(trim_text_value_bytes(vr, p)).into_owned())
                .collect()
        }
        Value::Strings(values) => values
            .iter()
            .map(|v| super::text::trim_text_value(vr, v).to_string())
            .collect(),
        _ => Vec::new(),
    }
}
fn character_set_terms(obj: &Object) -> String {
    let Some(element) = obj.get(TAG_SPECIFIC_CHARACTER_SET) else {
        return String::new();
    };
    let terms: Vec<String> = string_values(element)
        .iter()
        .map(|v| v.trim().to_string())
        .collect();
    terms.join("\\").trim_end_matches('\\').to_string()
}
fn is_plain_ascii(value: &[u8]) -> bool {
    value.iter().all(|&b| b < 0x80 && b != 0x1b)
}
fn element_has_non_ascii_text(element: &Element) -> bool {
    match &element.value {
        Value::Sequence(items) => items
            .iter()
            .any(|item| item.elements.iter().any(element_has_non_ascii_text)),
        Value::Strings(values) => values.iter().any(|v| !is_plain_ascii(v.as_bytes())),
        Value::Raw(raw) => !is_plain_ascii(raw),
        _ => false,
    }
}
pub(super) fn previous_value(existing: Option<&Element>, replacement: &Element) -> Element {
    if let Some(existing) = existing {
        return existing.clone();
    }
    let header = ElementHeader {
        tag: replacement.tag(),
        vr: replacement.vr(),
        length: 0,
        length_set: false,
    };
    if replacement.vr() == Vr::SQ {
        return Element {
            header,
            value: Value::Sequence(Vec::new()),
        };
    }
    Element {
        header,
        value: Value::Raw(Vec::new()),
    }
}
pub(super) fn element_empty(element: &Element) -> bool {
    match &element.value {
        Value::Nil => !element.header.length_set || element.header.length == 0,
        Value::Sequence(items) => items.is_empty(),
        Value::Strings(values) => values.iter().all(|v| v.trim().is_empty()),
        Value::Raw(raw) => raw.is_empty(),
        _ => encode_element_explicit_le(element)
            .is_some_and(|encoded| encoded.len() <= 12 && encoded_value_length(&encoded) == 0),
    }
}
fn encoded_value_length(encoded: &[u8]) -> usize {
    if encoded.len() < 8 {
        return 0;
    }
    let long = Vr::parse(&encoded[4..6]).is_some_and(Vr::uses_long_explicit_length);
    if long && encoded.len() >= 12 {
        return u32::from_le_bytes([encoded[8], encoded[9], encoded[10], encoded[11]]) as usize;
    }
    usize::from(u16::from_le_bytes([encoded[6], encoded[7]]))
}
pub(super) fn elements_equal(a: &Element, b: &Element) -> bool {
    if a.tag() != b.tag() || a.vr() != b.vr() {
        return false;
    }
    let (a_nil, b_nil) = (matches!(a.value, Value::Nil), matches!(b.value, Value::Nil));
    if a_nil || b_nil {
        return a_nil && b_nil && a.header.length == b.header.length;
    }
    match (encode_element_explicit_le(a), encode_element_explicit_le(b)) {
        (Some(x), Some(y)) => x == y,
        _ => a.value == b.value,
    }
}
pub fn copy_patient_identity(source: &Object, obj: &mut Object) -> Result<Vec<Element>, Error> {
    if character_set_terms(source) != character_set_terms(obj) {
        for tag in PATIENT_IDENTITY_TAGS {
            if let Some(element) = source.get(tag)
                && element_has_non_ascii_text(element)
            {
                return Err(Error::msg(format!(
                    "{ERR_PATIENT_IDENTITY_CHARACTER_SET}: {tag}"
                )));
            }
        }
    }
    let issuer_before = obj.get_string(TAG_ISSUER_OF_PATIENT_ID).unwrap_or_default();
    let issuer_after = source
        .get_string(TAG_ISSUER_OF_PATIENT_ID)
        .unwrap_or_default();
    let issuer_changes = issuer_before.trim() != issuer_after.trim();
    let mut modified = Vec::new();
    for tag in PATIENT_IDENTITY_TAGS {
        let existing = obj.get(tag).cloned();
        if let Some(replacement) = source.get(tag) {
            if existing
                .as_ref()
                .is_some_and(|e| elements_equal(e, replacement))
            {
                continue;
            }
            modified.push(previous_value(existing.as_ref(), replacement));
            obj.put(replacement.clone());
            continue;
        }
        let Some(existing) = existing else {
            continue;
        };
        if is_type2(tag) {
            if element_empty(&existing) {
                continue;
            }
            obj.put(previous_value(None, &existing));
            modified.push(existing);
        } else if tag == TAG_ISSUER_QUALIFIERS && !issuer_changes {
            continue;
        } else {
            modified.push(existing);
            obj.remove(tag);
        }
    }
    Ok(modified)
}
