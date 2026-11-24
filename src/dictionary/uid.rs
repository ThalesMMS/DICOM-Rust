use super::uid_tables::{ENTRIES, KEYWORD_INDEX, STORAGE_BY_UID, STORAGE_SOP_CLASSES};
use crate::textfmt::quote;
use std::collections::HashSet;
use std::fmt;
use std::str::FromStr;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Type(pub u8);
const TYPE_LABELS: [&str; 12] = [
    "SOP Class",
    "Meta SOP Class",
    "Transfer Syntax",
    "Well-known SOP Instance",
    "DICOM UIDs as a Coding Scheme",
    "Coding Scheme",
    "Application Context Name",
    "Service Class",
    "Application Hosting Model",
    "Mapping Resource",
    "LDAP OID",
    "Synchronization Frame of Reference",
];
impl Type {
    pub const SOP_CLASS: Type = Type(0);
    pub const META_SOP_CLASS: Type = Type(1);
    pub const TRANSFER_SYNTAX: Type = Type(2);
    pub const WELL_KNOWN_SOP_INSTANCE: Type = Type(3);
    pub const DICOM_UID_AS_CODING_SCHEME: Type = Type(4);
    pub const CODING_SCHEME: Type = Type(5);
    pub const APPLICATION_CONTEXT_NAME: Type = Type(6);
    pub const SERVICE_CLASS: Type = Type(7);
    pub const APPLICATION_HOSTING_MODEL: Type = Type(8);
    pub const MAPPING_RESOURCE: Type = Type(9);
    pub const LDAP_OID: Type = Type(10);
    pub const SYNCHRONIZATION_FRAME_OF_REFERENCE: Type = Type(11);
    pub fn label(self) -> Option<&'static str> {
        TYPE_LABELS.get(usize::from(self.0)).copied()
    }
}
impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.label() {
            Some(label) => f.write_str(label),
            None => write!(f, "Type({})", self.0),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseTypeError {
    pub value: String,
}
impl fmt::Display for ParseTypeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown UID type {}", quote(&self.value))
    }
}
impl std::error::Error for ParseTypeError {}
pub fn parse_type(value: &str) -> Result<Type, ParseTypeError> {
    let label = value.trim();
    match TYPE_LABELS.iter().position(|&l| l == label) {
        Some(i) => Ok(Type(i as u8)),
        None => Err(ParseTypeError {
            value: value.to_owned(),
        }),
    }
}
impl FromStr for Type {
    type Err = ParseTypeError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        parse_type(s)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Entry {
    pub uid: &'static str,
    pub name: &'static str,
    pub keyword: &'static str,
    pub typ: Type,
    pub retired: bool,
}
pub fn normalize_uid(uid: &str) -> &str {
    uid.trim_end_matches([' ', '\0'])
}
fn lower_text(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c == '\u{130}' {
                return 'i';
            }
            let mut lower = c.to_lowercase();
            match (lower.next(), lower.next()) {
                (Some(l), None) => l,
                _ => c,
            }
        })
        .collect()
}
pub fn by_uid(uid: &str) -> Option<&'static Entry> {
    let key = normalize_uid(uid);
    ENTRIES
        .binary_search_by(|e| e.uid.cmp(key))
        .ok()
        .map(|i| &ENTRIES[i])
}
pub fn by_keyword(keyword: &str) -> Option<&'static Entry> {
    let key = lower_text(keyword);
    KEYWORD_INDEX
        .binary_search_by(|&(k, _)| k.cmp(key.as_str()))
        .ok()
        .map(|i| &ENTRIES[usize::from(KEYWORD_INDEX[i].1)])
}
pub fn entries() -> &'static [Entry] {
    &ENTRIES
}
pub fn entries_by_type(typ: Type) -> Vec<&'static Entry> {
    ENTRIES.iter().filter(|e| e.typ == typ).collect()
}
pub trait Registry {
    fn by_uid(&self, uid: &str) -> Option<&Entry>;
    fn by_keyword(&self, keyword: &str) -> Option<&Entry>;
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StandardRegistry;
pub static DICTIONARY: StandardRegistry = StandardRegistry;
impl Registry for StandardRegistry {
    fn by_uid(&self, uid: &str) -> Option<&Entry> {
        by_uid(uid)
    }
    fn by_keyword(&self, keyword: &str) -> Option<&Entry> {
        by_keyword(keyword)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StorageCategory {
    Image,
    Waveform,
    Presentation,
    StructuredReport,
    Document,
    Segmentation,
    Registration,
    Derived,
    Rt,
    Measurement,
    Surface,
    Raw,
    Protocol,
    NonPatient,
    Security,
    Ndt,
    Legacy,
}
impl StorageCategory {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Image => "image",
            Self::Waveform => "waveform",
            Self::Presentation => "presentation",
            Self::StructuredReport => "sr",
            Self::Document => "document",
            Self::Segmentation => "segmentation",
            Self::Registration => "registration",
            Self::Derived => "derived",
            Self::Rt => "rt",
            Self::Measurement => "measurement",
            Self::Surface => "surface",
            Self::Raw => "raw",
            Self::Protocol => "protocol",
            Self::NonPatient => "nonpatient",
            Self::Security => "security",
            Self::Ndt => "ndt",
            Self::Legacy => "legacy",
        }
    }
}
impl fmt::Display for StorageCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct StorageSopClass {
    pub uid: &'static str,
    pub name: &'static str,
    pub keyword: &'static str,
    pub category: StorageCategory,
    pub modality: &'static str,
    pub source: &'static str,
    pub retired: bool,
}
pub const STORAGE_CATALOG_EDITION: &str = super::uid_tables::STORAGE_CATALOG_EDITION;
pub const MAX_SCU_STORAGE_CONTEXTS: usize = 126;
pub fn storage_sop_classes() -> &'static [StorageSopClass] {
    &STORAGE_SOP_CLASSES
}
pub fn lookup_storage_sop_class(uid: &str) -> Option<&'static StorageSopClass> {
    let key = normalize_uid(uid);
    STORAGE_BY_UID
        .binary_search_by(|&(u, _)| u.cmp(key))
        .ok()
        .map(|i| &STORAGE_SOP_CLASSES[usize::from(STORAGE_BY_UID[i].1)])
}
pub fn is_storage(uid: &str) -> bool {
    lookup_storage_sop_class(uid).is_some()
}
pub fn storage_sop_class_uids() -> Vec<&'static str> {
    STORAGE_SOP_CLASSES.iter().map(|c| c.uid).collect()
}
const SCU_CATEGORIES: [StorageCategory; 12] = [
    StorageCategory::Image,
    StorageCategory::StructuredReport,
    StorageCategory::Rt,
    StorageCategory::Segmentation,
    StorageCategory::Registration,
    StorageCategory::Presentation,
    StorageCategory::Document,
    StorageCategory::Derived,
    StorageCategory::Waveform,
    StorageCategory::Measurement,
    StorageCategory::Surface,
    StorageCategory::Raw,
];
struct Subset {
    out: Vec<String>,
    seen: HashSet<String>,
}
impl Subset {
    fn add(&mut self, uid: &str) -> bool {
        if self.seen.contains(uid) {
            return true;
        }
        if self.out.len() == MAX_SCU_STORAGE_CONTEXTS {
            return false;
        }
        self.seen.insert(uid.to_owned());
        self.out.push(uid.to_owned());
        true
    }
    fn fill(&mut self, category: StorageCategory, with_modality: impl Fn(&str) -> bool) -> bool {
        STORAGE_SOP_CLASSES
            .iter()
            .filter(|c| {
                !c.retired
                    && c.category == category
                    && !c.name.contains("For Processing")
                    && with_modality(c.modality)
            })
            .all(|c| self.add(c.uid))
    }
}
pub fn storage_scu_subset<S: AsRef<str>>(preferred: &[S]) -> Vec<String> {
    let mut subset = Subset {
        out: Vec::with_capacity(MAX_SCU_STORAGE_CONTEXTS),
        seen: HashSet::new(),
    };
    for uid in preferred {
        let uid = normalize_uid(uid.as_ref().trim());
        if !uid.is_empty() && !subset.add(uid) {
            return subset.out;
        }
    }
    for category in SCU_CATEGORIES {
        let filled = if category == StorageCategory::Rt {
            subset.fill(category, |modality| !modality.is_empty())
        } else {
            subset.fill(category, |_| true)
        };
        if !filled {
            return subset.out;
        }
        if category == StorageCategory::Derived
            && !subset.fill(StorageCategory::Rt, |modality| modality.is_empty())
        {
            return subset.out;
        }
    }
    subset.out
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn registry_round_trips() {
        assert!(ENTRIES.windows(2).all(|w| w[0].uid < w[1].uid));
        assert!(KEYWORD_INDEX.windows(2).all(|w| w[0].0 < w[1].0));
        for entry in entries() {
            assert_eq!(by_uid(entry.uid), Some(entry));
            assert_eq!(by_uid(&format!("{} \0", entry.uid)), Some(entry));
            let found = by_keyword(&entry.keyword.to_uppercase()).unwrap();
            assert!(found.keyword.eq_ignore_ascii_case(entry.keyword));
        }
        for unknown in ["", "0.0", " 1.2.840.10008.1.1", "9.9.9"] {
            assert_eq!(by_uid(unknown), None);
        }
        assert_eq!(
            DICTIONARY.by_keyword("VERİFİCATİON").map(|e| e.uid),
            Some("1.2.840.10008.1.1")
        );
    }
    #[test]
    fn types() {
        for t in 0..=255u8 {
            let typ = Type(t);
            match typ.label() {
                Some(label) => {
                    assert_eq!(parse_type(&format!(" {label}\t")), Ok(typ));
                }
                None => assert_eq!(typ.to_string(), format!("Type({t})")),
            }
        }
        assert_eq!(entries_by_type(Type(255)), Vec::<&Entry>::new());
        assert_eq!(
            parse_type("sop class").unwrap_err().to_string(),
            "unknown UID type \"sop class\""
        );
    }
    #[test]
    fn storage_catalog() {
        assert_eq!(STORAGE_BY_UID.len(), STORAGE_SOP_CLASSES.len());
        for class in storage_sop_classes() {
            assert_eq!(lookup_storage_sop_class(class.uid), Some(class));
            assert!(is_storage(&format!("{}\0", class.uid)));
        }
        let subset = storage_scu_subset::<&str>(&[]);
        assert_eq!(subset.len(), MAX_SCU_STORAGE_CONTEXTS);
        assert_eq!(storage_scu_subset(&[" 1.2.3 \0", "", "1.2.3"])[0], "1.2.3");
    }
}
