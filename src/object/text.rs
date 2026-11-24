#![forbid(unsafe_code)]
use super::types::Vr;
use crate::charset::SpecificCharacterSet;
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TextOptions {
    pub allow_unsupported_charset_fallback: bool,
    pub fallback_character_set: SpecificCharacterSet,
    pub charset_fallback: CharsetFallbackCases,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CharsetFallbackCases(pub u8);
impl CharsetFallbackCases {
    pub const UNDECLARED: CharsetFallbackCases = CharsetFallbackCases(1);
    #[doc(hidden)]
    pub const UNDECODABLE: CharsetFallbackCases = CharsetFallbackCases(2);
    pub fn contains(self, other: CharsetFallbackCases) -> bool {
        self.0 & other.0 != 0
    }
}
impl std::ops::BitOr for CharsetFallbackCases {
    type Output = CharsetFallbackCases;
    fn bitor(self, rhs: CharsetFallbackCases) -> CharsetFallbackCases {
        CharsetFallbackCases(self.0 | rhs.0)
    }
}
impl Vr {
    pub fn uses_specific_character_set(self) -> bool {
        matches!(
            self,
            Vr::LO | Vr::LT | Vr::PN | Vr::SH | Vr::ST | Vr::UC | Vr::UT
        )
    }
    pub fn uses_text_value_delimiter(self) -> bool {
        !matches!(self, Vr::LT | Vr::ST | Vr::UT | Vr::UR)
    }
    pub fn padding_byte(self) -> u8 {
        if self.is_string_like() && self != Vr::UI {
            b' '
        } else {
            0
        }
    }
}
fn space_trimmed(vr: Vr) -> bool {
    vr != Vr::UI && vr.is_string_like()
}
pub fn normalize_uid(uid: &str) -> &str {
    uid.trim_end_matches([' ', '\0'])
}
pub fn trim_text_value_bytes(vr: Vr, data: &[u8]) -> &[u8] {
    let space_only = space_trimmed(vr);
    let mut end = data.len();
    while end > 0 && (data[end - 1] == b' ' || !space_only && data[end - 1] == 0) {
        end -= 1;
    }
    &data[..end]
}
pub fn trim_text_value(vr: Vr, s: &str) -> &str {
    if space_trimmed(vr) {
        s.trim_end_matches(' ')
    } else {
        normalize_uid(s)
    }
}
pub fn clean_text_value(vr: Vr, s: &str) -> String {
    let s = s.replace('\0', "");
    trim_text_value(vr, &s).trim().to_string()
}
pub fn split_text_multiplicity(vr: Vr, s: &str) -> Vec<String> {
    let s = trim_text_value(vr, s);
    if s.is_empty() {
        return Vec::new();
    }
    if !vr.uses_text_value_delimiter() {
        return vec![s.to_string()];
    }
    s.split('\\')
        .map(|p| trim_text_value(vr, p).to_string())
        .collect()
}
pub(crate) fn raw_string_values(vr: Vr, raw: &[u8]) -> Vec<Vec<u8>> {
    let parts: Vec<&[u8]> = raw.split(|&b| b == b'\\').collect();
    if parts.len() == 1 && trim_text_value_bytes(vr, parts[0]).is_empty() {
        return Vec::new();
    }
    parts
        .iter()
        .map(|p| trim_text_value_bytes(vr, p).to_vec())
        .collect()
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PersonName {
    pub family_name: String,
    pub given_name: String,
    pub middle_name: String,
    pub name_prefix: String,
    pub name_suffix: String,
    component_groups: String,
}
impl PersonName {
    pub fn parse(s: &str) -> PersonName {
        let s = s.trim();
        let (alphabetic, remainder) = match s.split_once('=') {
            Some((a, r)) => (a, r),
            None => (s, ""),
        };
        let parts: Vec<&str> = alphabetic.trim().split('^').collect();
        let part = |i: usize| parts.get(i).map_or(String::new(), |p| p.trim().to_string());
        let mut pn = PersonName {
            family_name: part(0),
            given_name: part(1),
            middle_name: part(2),
            name_prefix: part(3),
            name_suffix: part(4),
            component_groups: String::new(),
        };
        if !remainder.is_empty() || s.contains('=') {
            pn.component_groups = format!("={remainder}");
        }
        pn
    }
    pub fn is_empty(&self) -> bool {
        self.family_name.is_empty()
            && self.given_name.is_empty()
            && self.middle_name.is_empty()
            && self.name_prefix.is_empty()
            && self.name_suffix.is_empty()
            && self.component_groups.is_empty()
    }
    pub fn to_dicom_string(&self) -> String {
        let parts = [
            &self.family_name,
            &self.given_name,
            &self.middle_name,
            &self.name_prefix,
            &self.name_suffix,
        ];
        let Some(last) = parts.iter().rposition(|p| !p.is_empty()) else {
            return self.component_groups.clone();
        };
        let joined: Vec<&str> = parts[..=last].iter().map(|p| p.as_str()).collect();
        joined.join("^") + &self.component_groups
    }
}
impl std::fmt::Display for PersonName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let parts: Vec<&str> = [
            &self.name_prefix,
            &self.given_name,
            &self.middle_name,
            &self.family_name,
            &self.name_suffix,
        ]
        .iter()
        .filter(|p| !p.is_empty())
        .map(|p| p.as_str())
        .collect();
        f.write_str(&parts.join(" "))
    }
}
