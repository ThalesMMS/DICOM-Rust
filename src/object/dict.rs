#![forbid(unsafe_code)]
use super::dict_tables::STD_VR;
use super::types::{Tag, Vr};
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Context {
    Xs,
    Ox,
    Px,
    Lt,
    Up,
}
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Entry {
    pub vr: Vr,
    pub context: Option<Context>,
}
pub fn by_tag(tag: Tag) -> Option<Entry> {
    let key = tag.key();
    let i = STD_VR.binary_search_by_key(&key, |e| e.0).ok()?;
    let (_, vr, context) = STD_VR[i];
    let context = match context {
        1 => Some(Context::Xs),
        2 => Some(Context::Ox),
        3 => Some(Context::Px),
        4 => Some(Context::Lt),
        5 => Some(Context::Up),
        _ => None,
    };
    Some(Entry { vr, context })
}
pub(crate) fn lookup_vr(tag: Tag) -> Vr {
    by_tag(tag).map_or(Vr::UN, |e| e.vr)
}
pub(crate) fn vr_values(entry: Entry) -> &'static [Vr] {
    match entry.context {
        Some(Context::Xs) => &[Vr::US, Vr::SS],
        Some(Context::Ox) | Some(Context::Px) => &[Vr::OB, Vr::OW],
        Some(Context::Lt) => &[Vr::US, Vr::SS, Vr::OW],
        Some(Context::Up) => &[Vr::UL],
        None => std::slice::from_ref(VR_SELF.iter().find(|v| **v == entry.vr).unwrap_or(&Vr::UN)),
    }
}
static VR_SELF: [Vr; 34] = [
    Vr::AE,
    Vr::AS,
    Vr::AT,
    Vr::CS,
    Vr::DA,
    Vr::DS,
    Vr::DT,
    Vr::FL,
    Vr::FD,
    Vr::IS,
    Vr::LO,
    Vr::LT,
    Vr::OB,
    Vr::OD,
    Vr::OF,
    Vr::OL,
    Vr::OV,
    Vr::OW,
    Vr::PN,
    Vr::SH,
    Vr::SL,
    Vr::SQ,
    Vr::SS,
    Vr::SV,
    Vr::ST,
    Vr::TM,
    Vr::UC,
    Vr::UI,
    Vr::UL,
    Vr::UN,
    Vr::UR,
    Vr::US,
    Vr::UT,
    Vr::UV,
];
pub(crate) fn single_vr(tag: Tag) -> Option<Vr> {
    let entry = by_tag(tag)?;
    let values = vr_values(entry);
    if values.len() != 1 || values[0] == Vr::UN {
        return None;
    }
    Some(values[0])
}
pub fn by_keyword(keyword: &str) -> Option<(Tag, &'static str)> {
    use std::collections::HashMap;
    use std::sync::OnceLock;
    static INDEX: OnceLock<HashMap<String, usize>> = OnceLock::new();
    let index = INDEX.get_or_init(|| {
        let mut m = HashMap::new();
        for (i, e) in super::dict_names::STD_NAMES.iter().enumerate() {
            m.entry(e.1.to_lowercase()).or_insert(i);
        }
        m
    });
    let i = *index.get(&keyword.to_lowercase())?;
    let (key, word, _) = super::dict_names::STD_NAMES[i];
    Some((Tag::new((key >> 16) as u16, key as u16), word))
}
pub fn is_private_group(group: u16) -> bool {
    group >= 0x0009 && group != 0xFFFF && group & 1 != 0
}
pub fn is_private_creator(tag: Tag) -> bool {
    is_private_group(tag.group) && (0x0010..=0x00FF).contains(&tag.element)
}
#[doc(hidden)]
pub fn vm(tag: Tag) -> Option<&'static str> {
    let key = tag.key();
    let i = STD_VR.binary_search_by_key(&key, |e| e.0).ok()?;
    Some(super::dict_vm::STD_VM_STRINGS[super::dict_vm::STD_VM[i] as usize])
}
