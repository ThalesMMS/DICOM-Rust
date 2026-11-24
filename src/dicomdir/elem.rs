#![forbid(unsafe_code)]
#![cfg_attr(
    not(test),
    warn(clippy::indexing_slicing, clippy::arithmetic_side_effects)
)]
use super::TAG_RECORD_IN_USE_FLAG;
use crate::object::text::{normalize_uid, trim_text_value, trim_text_value_bytes};
use crate::object::{DataSet, Element, ElementHeader, Object, Tag, Value, Vr};
pub(crate) fn calculated_length(e: &Element) -> Option<u32> {
    match &e.value {
        Value::Raw(raw) => Some(raw.len() as u32),
        Value::Strings(s) => {
            let mut total: usize = s.iter().map(String::len).fold(0, usize::saturating_add);
            total = total.saturating_add(s.len().saturating_sub(1));
            Some(total.saturating_add(total % 2) as u32)
        }
        _ => None,
    }
}
pub(crate) fn string_value(e: &Element) -> String {
    match &e.value {
        Value::Raw(raw) => trim_text_value(e.vr(), &String::from_utf8_lossy(raw)).to_string(),
        Value::Strings(s) => s
            .first()
            .map(|v| trim_text_value(e.vr(), v).to_string())
            .unwrap_or_default(),
        _ => String::new(),
    }
}
pub(crate) fn string_values(e: &Element) -> Vec<String> {
    match &e.value {
        Value::Raw(raw) => {
            let parts: Vec<&[u8]> = raw.split(|b| *b == b'\\').collect();
            if let [only] = parts.as_slice()
                && trim_text_value_bytes(e.vr(), only).is_empty()
            {
                return Vec::new();
            }
            parts
                .iter()
                .map(|p| String::from_utf8_lossy(trim_text_value_bytes(e.vr(), p)).into_owned())
                .collect()
        }
        Value::Strings(s) => s
            .iter()
            .map(|v| trim_text_value(e.vr(), v).to_string())
            .collect(),
        _ => Vec::new(),
    }
}
pub(crate) fn raw_element(tag: Tag, vr: Vr, data: Vec<u8>) -> Element {
    Element {
        header: ElementHeader::new(tag, vr, data.len() as u32),
        value: Value::Raw(data),
    }
}
pub(crate) fn value_element(tag: Tag, vr: Vr, value: Value) -> Element {
    Element {
        header: ElementHeader {
            tag,
            vr,
            length: 0,
            length_set: false,
        },
        value,
    }
}
pub(crate) fn text_element(tag: Tag, vr: Vr, value: &str) -> Element {
    text_values_element(tag, vr, vec![value.to_string()])
}
pub(crate) fn text_values_element(tag: Tag, vr: Vr, values: Vec<String>) -> Element {
    value_element(tag, vr, Value::Strings(values))
}
pub(crate) fn sequence_element(tag: Tag, items: Vec<DataSet>) -> Element {
    value_element(tag, Vr::SQ, Value::Sequence(items))
}
pub(crate) fn ui_element(tag: Tag, value: &str) -> Element {
    let mut raw = normalize_uid(value).as_bytes().to_vec();
    if !raw.len().is_multiple_of(2) {
        raw.push(0);
    }
    raw_element(tag, Vr::UI, raw)
}
pub(crate) fn ul_element(tag: Tag, value: u32) -> Element {
    raw_element(tag, Vr::UL, value.to_le_bytes().to_vec())
}
pub(crate) fn us_element(tag: Tag, value: u16) -> Element {
    raw_element(tag, Vr::US, value.to_le_bytes().to_vec())
}
pub(crate) fn uint32_value(obj: &Object, tag: Tag) -> Option<u32> {
    let e = obj.get(tag)?;
    let raw = e.raw()?;
    if e.vr() != Vr::UL {
        return None;
    }
    let b: [u8; 4] = raw.try_into().ok()?;
    Some(if obj.value_big_endian() {
        u32::from_be_bytes(b)
    } else {
        u32::from_le_bytes(b)
    })
}
pub(crate) fn uint16_value(obj: &Object, tag: Tag) -> Option<u16> {
    let e = obj.get(tag)?;
    if e.vr() != Vr::US {
        return None;
    }
    let raw = e.raw()?;
    let b: [u8; 2] = raw.try_into().ok()?;
    Some(if obj.value_big_endian() {
        u16::from_be_bytes(b)
    } else {
        u16::from_le_bytes(b)
    })
}
pub(crate) fn record_in_use(record: &Object) -> bool {
    let Some(e) = record.get(TAG_RECORD_IN_USE_FLAG) else {
        return true;
    };
    match e.raw() {
        Some(&[b0, b1]) if e.vr() == Vr::US => {
            let b = [b0, b1];
            let v = if record.value_big_endian() {
                u16::from_be_bytes(b)
            } else {
                u16::from_le_bytes(b)
            };
            v != 0
        }
        _ => false,
    }
}
pub(crate) fn string_with_vr(item: &Object, tag: Tag, vr: Vr) -> Option<String> {
    let e = item.get(tag)?;
    if e.vr() != vr {
        return None;
    }
    let values = item.get_strings(tag)?;
    if values.is_empty() {
        return if calculated_length(e) == Some(0) {
            Some(String::new())
        } else {
            None
        };
    }
    if values.len() != 1 {
        return None;
    }
    values.into_iter().next()
}
pub(crate) fn strings_with_vr(item: &Object, tag: Tag, vr: Vr) -> Option<Vec<String>> {
    let e = item.get(tag)?;
    if e.vr() != vr {
        return None;
    }
    item.get_strings(tag)
}
pub(crate) fn required_string_with_vr(item: &Object, tag: Tag, vr: Vr) -> Option<String> {
    string_with_vr(item, tag, vr).filter(|v| !v.is_empty())
}
pub(crate) fn required_uid(item: &Object, tag: Tag) -> (String, bool) {
    let (value, ok) = match string_with_vr(item, tag, Vr::UI) {
        Some(v) => (v, true),
        None => (String::new(), false),
    };
    let value = normalize_uid(&value).to_string();
    let valid = ok && valid_uid(&value);
    (value, valid)
}
pub(crate) fn valid_uid(uid: &str) -> bool {
    crate::object::is_valid_uid(uid.as_bytes())
}
pub(crate) fn non_ascii(values: &[String]) -> bool {
    values.iter().any(|v| v.chars().any(|r| r as u32 > 127))
}
