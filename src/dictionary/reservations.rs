use super::private::{is_private_group, private_creator_id};
use crate::object::{Element, Object, Tag, Value, Vr};
use std::collections::HashMap;
use std::fmt;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrivateError {
    Creator,
    Conflict,
    Missing,
    ResourceLimit,
    BlocksExhausted,
}
impl fmt::Display for PrivateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            PrivateError::Creator => "dicom: invalid private creator reservation",
            PrivateError::Conflict => "dicom: conflicting private creator reservations",
            PrivateError::Missing => "dicom: private creator reservation missing",
            PrivateError::ResourceLimit => "dicom: private reservation resource limit exceeded",
            PrivateError::BlocksExhausted => "dicom: no unused private block available",
        })
    }
}
impl std::error::Error for PrivateError {}
pub const DEFAULT_MAX_PRIVATE_CREATORS: usize = 4096;
pub fn is_private_creator_tag(tag: Tag) -> bool {
    is_private_group(tag.group) && (0x0010..=0x00FF).contains(&tag.element)
}
pub fn is_private_data_tag(tag: Tag) -> bool {
    is_private_group(tag.group) && tag.element >= 0x1000
}
#[derive(Clone, Debug, Default)]
struct Reservation {
    creator: String,
    valid: bool,
}
#[derive(Clone, Debug, Default)]
pub struct PrivateReservations {
    blocks: HashMap<Tag, Reservation>,
    creators: HashMap<(String, u16), Tag>,
    conflicts: HashMap<u16, bool>,
    max: usize,
}
impl PrivateReservations {
    pub fn new(max_creators: usize) -> PrivateReservations {
        PrivateReservations {
            max: if max_creators == 0 {
                DEFAULT_MAX_PRIVATE_CREATORS
            } else {
                max_creators
            },
            ..Default::default()
        }
    }
    pub fn observe(&mut self, e: &Element) -> Result<(), PrivateError> {
        let tag = e.tag();
        if !is_private_creator_tag(tag) {
            return Err(PrivateError::Creator);
        }
        if self.blocks.contains_key(&tag) {
            self.conflicts.insert(tag.group, true);
            return Err(PrivateError::Conflict);
        }
        if self.blocks.len() >= self.max {
            return Err(PrivateError::ResourceLimit);
        }
        self.blocks.insert(tag, Reservation::default());
        if e.vr() != Vr::LO {
            return Err(PrivateError::Creator);
        }
        let raw: String = match &e.value {
            Value::Raw(raw) => String::from_utf8_lossy(raw).into_owned(),
            Value::Strings(values) => {
                if values.len() != 1 {
                    return Err(PrivateError::Creator);
                }
                values[0].clone()
            }
            _ => return Err(PrivateError::Creator),
        };
        let creator = private_creator_id(&raw)
            .map_err(|_| PrivateError::Creator)?
            .to_string();
        let key = (creator.clone(), tag.group);
        if self.creators.contains_key(&key) {
            self.conflicts.insert(tag.group, true);
            return Err(PrivateError::Conflict);
        }
        self.creators.insert(key, tag);
        self.blocks.insert(
            tag,
            Reservation {
                creator,
                valid: true,
            },
        );
        Ok(())
    }
    pub fn creator(&self, tag: Tag) -> Result<String, PrivateError> {
        if !is_private_data_tag(tag) {
            return Err(PrivateError::Creator);
        }
        if self.conflicts.contains_key(&tag.group) {
            return Err(PrivateError::Conflict);
        }
        match self.blocks.get(&Tag::new(tag.group, tag.element >> 8)) {
            None => Err(PrivateError::Missing),
            Some(r) if !r.valid => Err(PrivateError::Creator),
            Some(r) => Ok(r.creator.clone()),
        }
    }
    pub fn block(&self, group: u16, creator: &str) -> Result<Option<u8>, PrivateError> {
        if !is_private_group(group) {
            return Err(PrivateError::Creator);
        }
        let id = private_creator_id(creator).map_err(|_| PrivateError::Creator)?;
        if self.conflicts.contains_key(&group) {
            return Err(PrivateError::Conflict);
        }
        Ok(self
            .creators
            .get(&(id.to_string(), group))
            .map(|t| t.element as u8))
    }
    pub fn from_elements(
        elements: &[Element],
        max: usize,
    ) -> Result<PrivateReservations, PrivateError> {
        let mut s = PrivateReservations::new(max);
        for e in elements {
            if is_private_creator_tag(e.tag())
                && let Err(PrivateError::ResourceLimit) = s.observe(e)
            {
                return Err(PrivateError::ResourceLimit);
            }
        }
        Ok(s)
    }
}
pub(crate) fn object_reservations(o: &Object) -> Result<PrivateReservations, PrivateError> {
    let mut s = PrivateReservations::from_elements(o.elements(), o.len())?;
    for &tag in o.private_duplicate_slots() {
        if let Some(e) = o.get(tag) {
            let _ = s.observe(e);
        }
    }
    Ok(s)
}
#[doc(hidden)]
pub fn private_tag(o: &Object, group: u16, creator: &str, offset: u8) -> Result<Tag, PrivateError> {
    let s = object_reservations(o)?;
    match s.block(group, creator)? {
        Some(block) => Ok(Tag::new(group, (block as u16) << 8 | offset as u16)),
        None => Err(PrivateError::Missing),
    }
}
#[doc(hidden)]
pub fn private_creator(o: &Object, tag: Tag) -> Result<String, PrivateError> {
    object_reservations(o)?.creator(tag)
}
pub fn reserve_private_creator(
    o: &mut Object,
    group: u16,
    creator: &str,
) -> Result<u8, PrivateError> {
    if !is_private_group(group) {
        return Err(PrivateError::Creator);
    }
    let id = private_creator_id(creator)
        .map_err(|_| PrivateError::Creator)?
        .to_string();
    let s = object_reservations(o)?;
    if let Some(block) = s.block(group, &id)? {
        return Ok(block);
    }
    let mut occupied = [false; 256];
    for e in o.elements() {
        let tag = e.tag();
        if tag.group != group {
            continue;
        }
        if is_private_creator_tag(tag) {
            occupied[tag.element as usize] = true;
            PrivateReservations::new(1).observe(e)?;
        } else if is_private_data_tag(tag) {
            occupied[(tag.element >> 8) as usize] = true;
        }
    }
    for (candidate, used) in occupied.iter().enumerate().skip(16) {
        if !used {
            o.put(Element {
                header: crate::object::ElementHeader {
                    tag: Tag::new(group, candidate as u16),
                    vr: Vr::LO,
                    length: 0,
                    length_set: false,
                },
                value: Value::Strings(vec![id]),
            });
            return Ok(candidate as u8);
        }
    }
    Err(PrivateError::BlocksExhausted)
}
