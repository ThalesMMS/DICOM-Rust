#![forbid(unsafe_code)]
use super::error::Error;
use super::obj::{Object, TAG_SPECIFIC_CHARACTER_SET};
use super::patient_identity::{element_empty, elements_equal, previous_value, string_values};
use super::types::{DataSet, Element, ElementHeader, Tag, Value, Vr};
use chrono::{DateTime, FixedOffset};
use std::collections::{HashMap, HashSet};
pub(crate) const TAG_ORIGINAL_ATTRIBUTES_SEQUENCE: Tag = Tag::new(0x0400, 0x0561);
const TAG_MODIFIED_ATTRIBUTES_SEQUENCE: Tag = Tag::new(0x0400, 0x0550);
const TAG_ATTRIBUTE_MODIFICATION_DT: Tag = Tag::new(0x0400, 0x0562);
const TAG_MODIFYING_SYSTEM: Tag = Tag::new(0x0400, 0x0563);
const TAG_SOURCE_OF_PREVIOUS_VALUES: Tag = Tag::new(0x0400, 0x0564);
const TAG_REASON_FOR_MODIFICATION: Tag = Tag::new(0x0400, 0x0565);
pub const MODIFICATION_COERCE: &str = "COERCE";
pub const MODIFICATION_CORRECT: &str = "CORRECT";
pub const ERR_ORIGINAL_ATTRIBUTES: &str = "dicom: invalid Original Attributes record";
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UpdatePolicy(pub i64);
impl UpdatePolicy {
    pub const OVERWRITE: UpdatePolicy = UpdatePolicy(0);
    pub const SUPPLEMENT: UpdatePolicy = UpdatePolicy(1);
    pub const MERGE: UpdatePolicy = UpdatePolicy(2);
    pub const REPLACE: UpdatePolicy = UpdatePolicy(3);
}
#[derive(Clone, Debug, Default)]
pub struct OriginalAttributesRecord {
    pub modified: Vec<Element>,
    pub reason: String,
    pub system: String,
    pub source: String,
    pub time: Option<DateTime<FixedOffset>>,
}
pub trait AttributeCoercion {
    fn coerce(&self, obj: &mut Object) -> Result<Vec<Element>, Error>;
}
pub struct CoercionFunc<F: Fn(&mut Object) -> Result<Vec<Element>, Error>>(pub F);
impl<F: Fn(&mut Object) -> Result<Vec<Element>, Error>> AttributeCoercion for CoercionFunc<F> {
    fn coerce(&self, obj: &mut Object) -> Result<Vec<Element>, Error> {
        (self.0)(obj)
    }
}
pub struct MergeAttributes<'a> {
    pub attributes: Option<&'a Object>,
    pub policy: UpdatePolicy,
}
impl AttributeCoercion for MergeAttributes<'_> {
    fn coerce(&self, obj: &mut Object) -> Result<Vec<Element>, Error> {
        match self.attributes {
            None => Ok(Vec::new()),
            Some(src) => update_with_policy(obj, src, self.policy),
        }
    }
}
pub struct NullifyAttributes {
    pub tags: Vec<Tag>,
}
impl AttributeCoercion for NullifyAttributes {
    fn coerce(&self, obj: &mut Object) -> Result<Vec<Element>, Error> {
        let mut modified = Vec::new();
        for &tag in &self.tags {
            let Some(element) = obj.get(tag).cloned() else {
                continue;
            };
            if element_empty(&element) {
                continue;
            }
            let empty = previous_value(None, &element);
            modified.push(element);
            obj.put(empty);
        }
        Ok(modified)
    }
}
pub struct RemapUids {
    pub uids: HashMap<String, String>,
}
impl RemapUids {
    fn remap(&self, element: &Element) -> Option<Element> {
        if let Value::Sequence(items) = &element.value {
            let mut changed = false;
            let mut new_items = Vec::with_capacity(items.len());
            for item in items {
                let mut elements = Vec::with_capacity(item.elements.len());
                for child in &item.elements {
                    match self.remap(child) {
                        Some(remapped) => {
                            changed = true;
                            elements.push(remapped);
                        }
                        None => elements.push(child.clone()),
                    }
                }
                new_items.push(DataSet {
                    elements,
                    item_offset: item.item_offset,
                });
            }
            if !changed {
                return None;
            }
            let mut element = element.clone();
            element.value = Value::Sequence(new_items);
            return Some(element);
        }
        if element.vr() != Vr::UI {
            return None;
        }
        let mut changed = false;
        let out: Vec<String> = string_values(element)
            .into_iter()
            .map(|value| {
                let trimmed = value.trim_end_matches(['\0', ' ']).to_string();
                match self.uids.get(&trimmed) {
                    Some(replacement) if *replacement != trimmed => {
                        changed = true;
                        replacement.clone()
                    }
                    _ => trimmed,
                }
            })
            .collect();
        if !changed {
            return None;
        }
        let mut element = element.clone();
        element.value = Value::Strings(out);
        element.header.length = 0;
        Some(element)
    }
}
impl AttributeCoercion for RemapUids {
    fn coerce(&self, obj: &mut Object) -> Result<Vec<Element>, Error> {
        if self.uids.is_empty() {
            return Ok(Vec::new());
        }
        let mut modified = Vec::new();
        for element in obj.sorted_elements() {
            if element.tag() == TAG_ORIGINAL_ATTRIBUTES_SEQUENCE {
                continue;
            }
            if let Some(remapped) = self.remap(&element) {
                modified.push(element);
                obj.put(remapped);
            }
        }
        Ok(modified)
    }
}
fn iso2022_single_byte(term: &str) -> Option<&'static str> {
    Some(match term {
        "ISO 2022 IR 6" => "",
        "ISO 2022 IR 100" => "ISO_IR 100",
        "ISO 2022 IR 101" => "ISO_IR 101",
        "ISO 2022 IR 109" => "ISO_IR 109",
        "ISO 2022 IR 110" => "ISO_IR 110",
        "ISO 2022 IR 144" => "ISO_IR 144",
        "ISO 2022 IR 127" => "ISO_IR 127",
        "ISO 2022 IR 126" => "ISO_IR 126",
        "ISO 2022 IR 138" => "ISO_IR 138",
        "ISO 2022 IR 148" => "ISO_IR 148",
        "ISO 2022 IR 13" => "ISO_IR 13",
        "ISO 2022 IR 166" => "ISO_IR 166",
        "ISO 2022 IR 203" => "ISO_IR 203",
        _ => return None,
    })
}
fn trim_unicode_space(s: &str) -> &str {
    s.trim_matches(char::is_whitespace)
}
fn new_string_element(tag: Tag, vr: Vr, value: &str) -> Element {
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
pub struct TrimIso2022CharacterSet;
impl AttributeCoercion for TrimIso2022CharacterSet {
    fn coerce(&self, obj: &mut Object) -> Result<Vec<Element>, Error> {
        let Some(element) = obj.get(TAG_SPECIFIC_CHARACTER_SET).cloned() else {
            return Ok(Vec::new());
        };
        let values = string_values(&element);
        let (mut target, mut extended) = ("", 0);
        for value in &values {
            let value = trim_unicode_space(value);
            if value.is_empty() || value == "ISO 2022 IR 6" {
                continue;
            }
            let Some(replacement) = iso2022_single_byte(value) else {
                return Ok(Vec::new());
            };
            target = replacement;
            extended += 1;
        }
        if extended > 1 || values.is_empty() || contains_escape(obj.elements()) {
            return Ok(Vec::new());
        }
        let trimmed = new_string_element(TAG_SPECIFIC_CHARACTER_SET, Vr::CS, target);
        if elements_equal(&element, &trimmed) {
            return Ok(Vec::new());
        }
        obj.put(trimmed);
        Ok(vec![element])
    }
}
fn contains_escape(elements: &[Element]) -> bool {
    elements.iter().any(|element| match &element.value {
        Value::Sequence(items) => items.iter().any(|item| contains_escape(&item.elements)),
        Value::Strings(values) => values.iter().any(|v| v.contains('\x1b')),
        Value::Raw(raw) => {
            matches!(
                element.vr(),
                Vr::SH | Vr::LO | Vr::ST | Vr::LT | Vr::UT | Vr::PN | Vr::UC
            ) && raw.contains(&0x1b)
        }
        _ => false,
    })
}
pub fn coerce_attributes(
    obj: &mut Object,
    record: OriginalAttributesRecord,
    coercions: &[Option<&dyn AttributeCoercion>],
) -> Result<Vec<Element>, Error> {
    let mut modified: Vec<Element> = Vec::new();
    let mut seen = HashSet::new();
    for coercion in coercions.iter().flatten() {
        for element in coercion.coerce(obj)? {
            if seen.insert(element.tag()) {
                modified.push(element);
            }
        }
    }
    let record = OriginalAttributesRecord {
        modified: modified.clone(),
        ..record
    };
    append_original_attributes(obj, &record)?;
    Ok(modified)
}
pub fn update_with_policy(
    obj: &mut Object,
    src: &Object,
    policy: UpdatePolicy,
) -> Result<Vec<Element>, Error> {
    let mut modified = Vec::new();
    for element in src.sorted_elements() {
        let tag = element.tag();
        if tag == TAG_ORIGINAL_ATTRIBUTES_SEQUENCE || tag.group == 0x0002 {
            continue;
        }
        let existing = obj.get(tag).cloned();
        let present = existing.is_some();
        match policy {
            UpdatePolicy::SUPPLEMENT if present => continue,
            UpdatePolicy::MERGE if existing.as_ref().is_some_and(|e| !element_empty(e)) => {
                continue;
            }
            UpdatePolicy::REPLACE if !present => continue,
            UpdatePolicy::OVERWRITE
            | UpdatePolicy::SUPPLEMENT
            | UpdatePolicy::MERGE
            | UpdatePolicy::REPLACE => {}
            UpdatePolicy(other) => {
                return Err(Error::msg(format!("dicom: unknown update policy {other}")));
            }
        }
        if let Some(existing) = &existing
            && elements_equal(existing, &element)
        {
            continue;
        }
        modified.push(previous_value(existing.as_ref(), &element));
        obj.put(element);
    }
    Ok(modified)
}
pub fn modified_attributes(before: &Object, after: &Object) -> Vec<Element> {
    let mut seen = HashSet::new();
    let mut modified = Vec::new();
    for element in before.sorted_elements() {
        let tag = element.tag();
        seen.insert(tag);
        if tag == TAG_ORIGINAL_ATTRIBUTES_SEQUENCE {
            continue;
        }
        match after.get(tag) {
            Some(current) if elements_equal(&element, current) => {}
            _ => modified.push(element),
        }
    }
    for element in after.sorted_elements() {
        let tag = element.tag();
        if !seen.contains(&tag) && tag != TAG_ORIGINAL_ATTRIBUTES_SEQUENCE {
            modified.push(previous_value(None, &element));
        }
    }
    modified.sort_by_key(|e| e.tag());
    modified
}
fn format_dt(at: &DateTime<FixedOffset>) -> String {
    let offset = at.offset().local_minus_utc();
    let sign = if offset < 0 { '-' } else { '+' };
    let minutes = offset.abs() / 60;
    format!(
        "{}{sign}{:02}{:02}",
        at.format("%Y%m%d%H%M%S%.6f"),
        minutes / 60,
        minutes % 60
    )
}
pub fn append_original_attributes(
    obj: &mut Object,
    record: &OriginalAttributesRecord,
) -> Result<(), Error> {
    if record.modified.is_empty() {
        return Ok(());
    }
    let reason = trim_unicode_space(&record.reason);
    if reason != MODIFICATION_COERCE && reason != MODIFICATION_CORRECT {
        return Err(Error::msg(format!(
            "{ERR_ORIGINAL_ATTRIBUTES}: reason {} is not COERCE or CORRECT",
            crate::textfmt::quote(&record.reason)
        )));
    }
    let system = trim_unicode_space(&record.system);
    if system.is_empty() || system.len() > 64 {
        return Err(Error::msg(format!(
            "{ERR_ORIGINAL_ATTRIBUTES}: Modifying System must hold 1 to 64 characters"
        )));
    }
    if record.source.len() > 64 {
        return Err(Error::msg(format!(
            "{ERR_ORIGINAL_ATTRIBUTES}: Source of Previous Values is longer than 64 characters"
        )));
    }
    let at = record
        .time
        .unwrap_or_else(|| chrono::Local::now().fixed_offset());
    let mut modified = record.modified.clone();
    modified.sort_by_key(|e| e.tag());
    if let Some(charset) = obj.get(TAG_SPECIFIC_CHARACTER_SET)
        && !modified
            .iter()
            .any(|e| e.tag() == TAG_SPECIFIC_CHARACTER_SET)
    {
        modified.insert(0, charset.clone());
    }
    let sequence = |tag: Tag, items: Vec<DataSet>| Element {
        header: ElementHeader {
            tag,
            vr: Vr::SQ,
            length: 0,
            length_set: false,
        },
        value: Value::Sequence(items),
    };
    let item = DataSet {
        elements: vec![
            sequence(
                TAG_MODIFIED_ATTRIBUTES_SEQUENCE,
                vec![DataSet {
                    elements: modified,
                    item_offset: None,
                }],
            ),
            new_string_element(TAG_ATTRIBUTE_MODIFICATION_DT, Vr::DT, &format_dt(&at)),
            new_string_element(TAG_MODIFYING_SYSTEM, Vr::LO, system),
            new_string_element(
                TAG_SOURCE_OF_PREVIOUS_VALUES,
                Vr::LO,
                trim_unicode_space(&record.source),
            ),
            new_string_element(TAG_REASON_FOR_MODIFICATION, Vr::CS, reason),
        ],
        item_offset: None,
    };
    let mut items = Vec::new();
    if let Some(existing) = obj.get(TAG_ORIGINAL_ATTRIBUTES_SEQUENCE) {
        let Value::Sequence(existing) = &existing.value else {
            return Err(Error::msg(format!(
                "{ERR_ORIGINAL_ATTRIBUTES}: existing Original Attributes Sequence is not a sequence"
            )));
        };
        items.extend(existing.iter().cloned());
    }
    items.push(item);
    obj.put(sequence(TAG_ORIGINAL_ATTRIBUTES_SEQUENCE, items));
    Ok(())
}
