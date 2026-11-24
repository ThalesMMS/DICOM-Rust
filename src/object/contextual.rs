#![forbid(unsafe_code)]
use super::dict::{self, Context};
use super::types::{DataSet, Element, Tag, Value, Vr};
use crate::dictionary::chained::ChainedDictionary;
use crate::dictionary::dic::ContextualVr;
const TAG_PIXEL_REPRESENTATION: Tag = Tag::new(0x0028, 0x0103);
#[derive(Clone, Copy)]
enum Scope {
    Absent,
    Value(u16),
    Invalid,
}
pub(crate) fn resolve_implicit_contextual_vrs(ds: &mut DataSet, big_endian: bool) {
    resolve_elements(&mut ds.elements, &[], big_endian, None);
}
pub(crate) fn resolve_implicit_contextual_vrs_chained(
    ds: &mut DataSet,
    big_endian: bool,
    chained: Option<&dyn ChainedDictionary>,
) {
    resolve_elements(&mut ds.elements, &[], big_endian, chained);
}
fn resolve_elements(
    elements: &mut [Element],
    ancestors: &[Scope],
    big_endian: bool,
    chained: Option<&dyn ChainedDictionary>,
) {
    let mut scope = vec![pixel_representation(elements, big_endian)];
    scope.extend_from_slice(ancestors);
    for element in elements.iter_mut() {
        resolve_element(element, &scope, big_endian, chained);
    }
}
fn resolve_element(
    element: &mut Element,
    scope: &[Scope],
    big_endian: bool,
    chained: Option<&dyn ChainedDictionary>,
) {
    if let Value::Sequence(items) = &mut element.value {
        for item in items.iter_mut() {
            resolve_elements(&mut item.elements, scope, big_endian, chained);
        }
        return;
    }
    let contextual = match dict::by_tag(element.tag()) {
        Some(entry) => entry.context == Some(Context::Xs),
        None => chained
            .and_then(|c| c.vr_spec(element.tag()))
            .is_some_and(|spec| spec.context == Some(ContextualVr::Xs)),
    };
    if !contextual || element.vr() != Vr::US {
        return;
    }
    for pr in scope {
        match pr {
            Scope::Invalid => return,
            Scope::Absent => continue,
            Scope::Value(1) => {
                element.header.vr = Vr::SS;
                return;
            }
            Scope::Value(_) => return,
        }
    }
}
fn pixel_representation(elements: &[Element], big_endian: bool) -> Scope {
    match uint16_value(elements, TAG_PIXEL_REPRESENTATION, big_endian) {
        Err(()) => Scope::Invalid,
        Ok(None) => Scope::Absent,
        Ok(Some(v)) if v > 1 => Scope::Invalid,
        Ok(Some(v)) => Scope::Value(v),
    }
}
fn uint16_value(elements: &[Element], tag: Tag, big_endian: bool) -> Result<Option<u16>, ()> {
    let Some(element) = elements.iter().find(|e| e.tag() == tag) else {
        return Ok(None);
    };
    match &element.value {
        Value::Raw(raw) if raw.len() >= 2 && raw.len() % 2 == 0 => {
            let b = [raw[0], raw[1]];
            Ok(Some(if big_endian {
                u16::from_be_bytes(b)
            } else {
                u16::from_le_bytes(b)
            }))
        }
        Value::Nil => Ok(None),
        _ => Err(()),
    }
}
