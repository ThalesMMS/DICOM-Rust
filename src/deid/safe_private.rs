use super::profile::ProfileAction;
use super::{Error, Kind, trim_space};
use crate::object::{Tag, Vr};
use std::collections::HashMap;
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SafePrivateProvenance {
    pub version: String,
    pub checksum: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SafePrivateRule {
    pub tag: Tag,
    pub creator: String,
    pub vr: Option<Vr>,
    pub vm: i64,
    pub action: Option<ProfileAction>,
}
#[derive(Clone, Debug, Default)]
pub struct SafePrivateRegistry {
    provenance: SafePrivateProvenance,
    rules: Vec<SafePrivateRule>,
    by_key: HashMap<(u16, u8, String), SafePrivateRule>,
}
fn invalid(detail: String) -> Error {
    Error::new(Kind::InvalidSafePrivateRegistry, detail)
}
impl SafePrivateRegistry {
    pub fn new(
        provenance: SafePrivateProvenance,
        rules: Vec<SafePrivateRule>,
    ) -> Result<SafePrivateRegistry, Error> {
        if trim_space(&provenance.version).is_empty() {
            return Err(invalid("provenance version is required".into()));
        }
        let valid_checksum = provenance.checksum.len() == 64
            && provenance.checksum.bytes().all(|b| b.is_ascii_hexdigit());
        if !valid_checksum {
            return Err(invalid(
                "provenance checksum must be a SHA-256 hexadecimal digest".into(),
            ));
        }
        let mut by_key = HashMap::new();
        for (index, rule) in rules.iter().enumerate() {
            let n = index + 1;
            if !rule.tag.is_private() || rule.tag.element < 0x1000 {
                return Err(invalid(format!(
                    "rule {n} must identify a private data element"
                )));
            }
            if trim_space(&rule.creator).is_empty() {
                return Err(invalid(format!("rule {n} requires a private creator")));
            }
            if rule.vr.is_none() || rule.vm <= 0 {
                return Err(invalid(format!(
                    "rule {n} requires an exact VR and positive VM"
                )));
            }
            if !matches!(
                rule.action,
                Some(ProfileAction::Keep | ProfileAction::Clean)
            ) {
                return Err(invalid(format!("rule {n} action must be keep or clean")));
            }
            let key = lookup_key(rule.tag, &rule.creator);
            if by_key.contains_key(&key) {
                return Err(invalid(format!(
                    "rule {n} duplicates an earlier group, relative element, and creator"
                )));
            }
            by_key.insert(key, rule.clone());
        }
        Ok(SafePrivateRegistry {
            provenance,
            rules,
            by_key,
        })
    }
    pub(crate) fn lookup_rule(&self, tag: Tag, creator: &str) -> Option<&SafePrivateRule> {
        if !tag.is_private() || tag.element < 0x1000 {
            return None;
        }
        self.by_key.get(&lookup_key(tag, creator))
    }
    pub fn lookup(&self, tag: Tag, creator: &str) -> Option<ProfileAction> {
        self.lookup_rule(tag, creator).and_then(|r| r.action)
    }
    pub fn provenance(&self) -> &SafePrivateProvenance {
        &self.provenance
    }
    pub fn rules(&self) -> &[SafePrivateRule] {
        &self.rules
    }
    pub fn len(&self) -> usize {
        self.rules.len()
    }
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }
}
fn lookup_key(tag: Tag, creator: &str) -> (u16, u8, String) {
    (
        tag.group,
        tag.element as u8,
        trim_space(creator).to_string(),
    )
}
