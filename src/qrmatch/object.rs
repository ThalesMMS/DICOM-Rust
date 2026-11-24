use super::text::{
    is_string_like, lossy_text, split_text_multiplicity, trim_text_value, trim_text_value_bytes,
    uses_specific_character_set, uses_text_value_delimiter,
};
use crate::charset::SpecificCharacterSet;
use crate::object::{Tag, Vr};
pub fn elements_from(elements: &[crate::object::Element]) -> Vec<Element> {
    use crate::object::Value as Source;
    elements
        .iter()
        .map(|e| Element {
            tag: e.tag(),
            vr: e.vr(),
            value: match &e.value {
                Source::Strings(s) => Value::Strings(s.clone()),
                Source::Raw(r) => Value::Raw(r.clone()),
                Source::Sequence(items) => {
                    Value::Sequence(items.iter().map(|i| elements_from(&i.elements)).collect())
                }
                _ => Value::Raw(Vec::new()),
            },
        })
        .collect()
}
pub(super) const SPECIFIC_CHARACTER_SET: Tag = Tag::new(0x0008, 0x0005);
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Strings(Vec<String>),
    Raw(Vec<u8>),
    Tags(Vec<Tag>),
    Sequence(Vec<Vec<Element>>),
}
#[derive(Debug, Clone, PartialEq)]
pub struct Element {
    pub tag: Tag,
    pub vr: Vr,
    pub value: Value,
}
impl Element {
    pub fn strings<S: Into<String>>(tag: Tag, vr: Vr, values: impl IntoIterator<Item = S>) -> Self {
        Element {
            tag,
            vr,
            value: Value::Strings(values.into_iter().map(Into::into).collect()),
        }
    }
    pub(super) fn calculated_length(&self) -> u32 {
        match &self.value {
            Value::Raw(raw) => raw.len() as u32,
            Value::Strings(values) => {
                let mut total: usize = values.iter().map(String::len).sum();
                if !values.is_empty() {
                    total += values.len() - 1;
                }
                if total % 2 == 1 {
                    total += 1;
                }
                total as u32
            }
            Value::Tags(tags) => {
                let total = tags.len() as u64 * 4;
                if total >= u64::from(u32::MAX) {
                    0
                } else {
                    total as u32
                }
            }
            Value::Sequence(_) => u32::MAX,
        }
    }
    pub(super) fn string_values(&self) -> Vec<String> {
        match &self.value {
            Value::Raw(raw) => {
                let parts: Vec<&[u8]> = raw.split(|&b| b == b'\\').collect();
                if parts.len() == 1 && trim_text_value_bytes(self.vr, parts[0]).is_empty() {
                    return Vec::new();
                }
                parts
                    .iter()
                    .map(|p| lossy_text(trim_text_value_bytes(self.vr, p)))
                    .collect()
            }
            Value::Strings(values) => values
                .iter()
                .map(|v| trim_text_value(self.vr, v).to_string())
                .collect(),
            _ => Vec::new(),
        }
    }
}
pub(super) fn string_values(element: &Element) -> Vec<String> {
    if !uses_text_value_delimiter(element.vr) {
        if let Value::Raw(raw) = &element.value {
            return vec![trim_text_value(element.vr, &lossy_text(raw)).to_string()];
        }
        return element.string_values();
    }
    let values = element.string_values();
    if values.is_empty() {
        return vec![String::new()];
    }
    values
        .iter()
        .flat_map(|v| v.split('\\').map(str::to_string))
        .collect()
}
fn is_private_creator(tag: Tag) -> bool {
    tag.group >= 0x0009
        && tag.group != 0xFFFF
        && tag.group & 1 == 1
        && (0x0010..=0x00FF).contains(&tag.element)
}
pub(super) type Charset = Result<SpecificCharacterSet, ()>;
fn decode_raw(vr: Vr, raw: &[u8], charset: &SpecificCharacterSet) -> Result<Vec<String>, ()> {
    let trimmed = trim_text_value_bytes(vr, raw);
    let text = if vr == Vr::PN {
        charset.decode_person_name(trimmed)
    } else if !uses_text_value_delimiter(vr) {
        charset.decode_single_value(trimmed)
    } else {
        charset.decode(trimmed)
    }
    .map_err(|_| ())?;
    Ok(split_text_multiplicity(vr, &text))
}
pub(super) struct Object<'a> {
    elements: Vec<&'a Element>,
    charset: Charset,
}
impl<'a> Object<'a> {
    pub(super) fn new(elements: &'a [Element], inherited: Option<&Charset>) -> Self {
        let mut kept: Vec<&'a Element> = Vec::with_capacity(elements.len());
        for element in elements {
            kept.retain(|e| e.tag != element.tag);
            kept.push(element);
        }
        let mut object = Object {
            elements: kept,
            charset: Ok(SpecificCharacterSet::default()),
        };
        if let Some(inherited) = inherited {
            object.charset = inherited.clone();
        }
        if let Some(scs) = object.get(SPECIFIC_CHARACTER_SET) {
            object.charset = Self::declared_charset(scs);
        }
        object
    }
    fn declared_charset(element: &Element) -> Charset {
        if !is_string_like(element.vr) {
            return Err(());
        }
        let default = SpecificCharacterSet::default();
        let values = match &element.value {
            Value::Strings(values) => values
                .iter()
                .map(|v| trim_text_value(element.vr, v).to_string())
                .collect(),
            Value::Raw(raw) => {
                let text = default
                    .decode(trim_text_value_bytes(element.vr, raw))
                    .map_err(|_| ())?;
                split_text_multiplicity(element.vr, &text)
            }
            _ => return Err(()),
        };
        SpecificCharacterSet::parse(&values).map_err(|_| ())
    }
    pub(super) fn elements(&self) -> &[&'a Element] {
        &self.elements
    }
    pub(super) fn get(&self, tag: Tag) -> Option<&'a Element> {
        self.elements.iter().find(|e| e.tag == tag).copied()
    }
    pub(super) fn decode_text_values(&self, element: &Element) -> Result<Vec<String>, ()> {
        if !is_string_like(element.vr) {
            return Err(());
        }
        match &element.value {
            Value::Strings(values) => Ok(values
                .iter()
                .map(|v| trim_text_value(element.vr, v).to_string())
                .collect()),
            Value::Raw(raw) => {
                if is_private_creator(element.tag) || !uses_specific_character_set(element.vr) {
                    return decode_raw(element.vr, raw, &SpecificCharacterSet::default());
                }
                let charset = self.charset.as_ref().map_err(|_| ())?;
                decode_raw(element.vr, raw, charset)
            }
            _ => Err(()),
        }
    }
    pub(super) fn get_string(&self, tag: Tag) -> Option<String> {
        let element = self.get(tag)?;
        let values = self.decode_text_values(element).ok()?;
        Some(values.into_iter().next().unwrap_or_default())
    }
    pub(super) fn get_sequence(&self, tag: Tag) -> Vec<Object<'a>> {
        match self.get(tag).map(|e| &e.value) {
            Some(Value::Sequence(items)) => items
                .iter()
                .map(|item| Object::new(item, Some(&self.charset)))
                .collect(),
            _ => Vec::new(),
        }
    }
    pub(super) fn get_tags(&self, tag: Tag) -> Option<Vec<Tag>> {
        let element = self.get(tag)?;
        if element.vr != Vr::AT {
            return None;
        }
        match &element.value {
            Value::Tags(tags) => (!tags.is_empty()).then(|| tags.clone()),
            Value::Raw(raw) if !raw.is_empty() && raw.len() % 4 == 0 => Some(
                raw.chunks(4)
                    .map(|c| {
                        Tag::new(
                            u16::from_le_bytes([c[0], c[1]]),
                            u16::from_le_bytes([c[2], c[3]]),
                        )
                    })
                    .collect(),
            ),
            _ => None,
        }
    }
}
