use super::{
    CodedEntry, Error, TAG_CODE_MEANING, TAG_CODE_VALUE, TAG_CODING_SCHEME_DESIGNATOR, invalid,
    trim_space,
};
use crate::derivedio as dio;
use crate::object::{DataSet, Element, Object, Tag, Vr};
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AlgorithmIdentification {
    pub family: CodedEntry,
    pub name_code: CodedEntry,
    pub name: String,
    pub version: String,
    pub parameters: String,
    pub source: String,
}
const TAG_SEGMENTATION_ALGORITHM_IDENTIFICATION: Tag = Tag::new(0x0062, 0x0007);
const TAG_ALGORITHM_FAMILY: Tag = Tag::new(0x0066, 0x002F);
const TAG_ALGORITHM_NAME_CODE: Tag = Tag::new(0x0066, 0x0030);
const TAG_ALGORITHM_VERSION: Tag = Tag::new(0x0066, 0x0031);
const TAG_ALGORITHM_PARAMETERS: Tag = Tag::new(0x0066, 0x0032);
const TAG_ALGORITHM_NAME: Tag = Tag::new(0x0066, 0x0036);
const TAG_ALGORITHM_SOURCE: Tag = Tag::new(0x0024, 0x0202);
impl AlgorithmIdentification {
    pub(crate) fn is_zero(&self) -> bool {
        *self == AlgorithmIdentification::default()
    }
    pub fn validate(&self) -> Result<(), Error> {
        if self.is_zero() {
            return Ok(());
        }
        if !algorithm_code(&self.family)
            || !algorithm_text(&self.name, 64, false)
            || trim_space(&self.name).is_empty()
            || !algorithm_text(&self.version, 64, false)
            || trim_space(&self.version).is_empty()
            || !algorithm_text(&self.source, 64, false)
        {
            return Err(invalid(
                "algorithm identification needs valid family, name, version and source text",
            ));
        }
        if !self.name_code.is_zero() && !algorithm_code(&self.name_code) {
            return Err(invalid("invalid algorithm name code"));
        }
        if self.parameters.len() > 10240 || !algorithm_text(&self.parameters, 10240, true) {
            return Err(invalid("invalid or oversized algorithm parameters"));
        }
        Ok(())
    }
}
fn algorithm_code(c: &CodedEntry) -> bool {
    !trim_space(&c.code_value).is_empty()
        && !trim_space(&c.coding_scheme_designator).is_empty()
        && !trim_space(&c.code_meaning).is_empty()
        && algorithm_text(&c.code_value, 16, false)
        && algorithm_text(&c.coding_scheme_designator, 16, false)
        && algorithm_text(&c.code_meaning, 64, false)
}
fn is_control(r: char) -> bool {
    let c = r as u32;
    c < 0x20 || (0x7F..0xA0).contains(&c)
}
fn algorithm_text(s: &str, limit: usize, long: bool) -> bool {
    if s.len() > 4 * limit || s.chars().count() > limit {
        return false;
    }
    !s.chars().any(|r| {
        (r == '\\' && !long) || (is_control(r) && !(long && matches!(r, '\n' | '\r' | '\t')))
    })
}
pub(crate) fn code_sequence(tag: Tag, code: &CodedEntry) -> Element {
    dio::seq(tag, vec![code_item(code)])
}
fn code_item(code: &CodedEntry) -> DataSet {
    dio::data_set(vec![
        dio::sh(TAG_CODE_VALUE, &code.code_value),
        dio::sh(TAG_CODING_SCHEME_DESIGNATOR, &code.coding_scheme_designator),
        dio::lo(TAG_CODE_MEANING, &code.code_meaning),
    ])
}
pub(crate) fn algorithm_identification_sequence(a: &AlgorithmIdentification) -> Element {
    let mut elements = vec![
        code_sequence(TAG_ALGORITHM_FAMILY, &a.family),
        dio::lo(TAG_ALGORITHM_NAME, &a.name),
        dio::lo(TAG_ALGORITHM_VERSION, &a.version),
    ];
    if !a.name_code.is_zero() {
        elements.push(code_sequence(TAG_ALGORITHM_NAME_CODE, &a.name_code));
    }
    if !a.source.is_empty() {
        elements.push(dio::lo(TAG_ALGORITHM_SOURCE, &a.source));
    }
    if !a.parameters.is_empty() {
        elements.push(dio::str_elem(
            TAG_ALGORITHM_PARAMETERS,
            Vr::LT,
            &a.parameters,
        ));
    }
    dio::seq(
        TAG_SEGMENTATION_ALGORITHM_IDENTIFICATION,
        vec![dio::data_set(elements)],
    )
}
fn trim_right_space_nul(s: &str) -> String {
    s.trim_end_matches([' ', '\0']).to_string()
}
pub(crate) fn read_algorithm_identification(
    obj: &Object,
) -> Result<AlgorithmIdentification, Error> {
    let mut a = AlgorithmIdentification::default();
    let Some(element) = obj.get(TAG_SEGMENTATION_ALGORITHM_IDENTIFICATION) else {
        return Ok(a);
    };
    let items = dio::sequence(obj, TAG_SEGMENTATION_ALGORITHM_IDENTIFICATION);
    if element.vr() != Vr::SQ || items.len() != 1 {
        return Err(invalid(
            "SEG profile requires one algorithm identification item",
        ));
    }
    let item = &items[0];
    let name_code_present = item.has(TAG_ALGORITHM_NAME_CODE);
    if dio::sequence(item, TAG_ALGORITHM_FAMILY).len() != 1
        || (name_code_present && dio::sequence(item, TAG_ALGORITHM_NAME_CODE).len() != 1)
    {
        return Err(invalid("algorithm identification code cardinality"));
    }
    a.family = read_algorithm_code(item, TAG_ALGORITHM_FAMILY)?;
    if name_code_present {
        a.name_code = read_algorithm_code(item, TAG_ALGORITHM_NAME_CODE)?;
    }
    for (tag, vr, field) in [
        (TAG_ALGORITHM_NAME, Vr::LO, 0),
        (TAG_ALGORITHM_VERSION, Vr::LO, 1),
        (TAG_ALGORITHM_SOURCE, Vr::LO, 2),
        (TAG_ALGORITHM_PARAMETERS, Vr::LT, 3),
    ] {
        let Some(element) = item.get(tag) else {
            continue;
        };
        let value = match item.get_strings(tag) {
            Some(values) if element.vr() == vr && values.len() == 1 => {
                trim_right_space_nul(&values[0])
            }
            _ => {
                return Err(invalid(format!(
                    "algorithm attribute {tag} has invalid VR/VM"
                )));
            }
        };
        match field {
            0 => a.name = value,
            1 => a.version = value,
            2 => a.source = value,
            _ => a.parameters = value,
        }
    }
    if trim_space(&a.name).is_empty() || trim_space(&a.version).is_empty() {
        return Err(invalid("algorithm identification missing name/version"));
    }
    a.validate()?;
    Ok(a)
}
fn read_algorithm_code(obj: &Object, tag: Tag) -> Result<CodedEntry, Error> {
    let items = dio::sequence(obj, tag);
    if !obj.get(tag).is_some_and(|e| e.vr() == Vr::SQ) || items.len() != 1 {
        return Err(invalid(format!("algorithm code sequence {tag} VR/VM")));
    }
    let mut code = CodedEntry::default();
    for (field_tag, vr, field) in [
        (TAG_CODE_VALUE, Vr::SH, 0),
        (TAG_CODING_SCHEME_DESIGNATOR, Vr::SH, 1),
        (TAG_CODE_MEANING, Vr::LO, 2),
    ] {
        let value = match (items[0].get(field_tag), items[0].get_strings(field_tag)) {
            (Some(e), Some(values)) if e.vr() == vr && values.len() == 1 => {
                trim_right_space_nul(&values[0])
            }
            _ => {
                return Err(invalid(format!(
                    "algorithm code attribute {field_tag} VR/VM"
                )));
            }
        };
        match field {
            0 => code.code_value = value,
            1 => code.coding_scheme_designator = value,
            _ => code.code_meaning = value,
        }
    }
    if !algorithm_code(&code) {
        return Err(invalid("invalid algorithm code"));
    }
    Ok(code)
}
