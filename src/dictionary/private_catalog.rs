use super::private::{PrivateEntry, is_private_group, private_creator_id};
use crate::object::Vr;
use std::collections::HashMap;
use std::fmt;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrivateDefinitionError {
    pub entry: usize,
    pub duplicate: bool,
}
impl fmt::Display for PrivateDefinitionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let what = if self.duplicate {
            "duplicate entry"
        } else {
            "entry"
        };
        write!(
            f,
            "dicom: invalid private dictionary definition: {what} {}",
            self.entry
        )
    }
}
impl std::error::Error for PrivateDefinitionError {}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PrivateDefinition {
    pub creator: String,
    pub group: u16,
    pub offset: u8,
    pub vr: String,
    pub keyword: String,
    pub name: String,
    pub vm: String,
    pub retired: bool,
}
impl From<&PrivateEntry> for PrivateDefinition {
    fn from(e: &PrivateEntry) -> PrivateDefinition {
        PrivateDefinition {
            creator: e.creator.to_string(),
            group: e.group,
            offset: e.offset,
            vr: e.vr.to_string(),
            keyword: e.keyword.to_string(),
            name: e.name.to_string(),
            vm: e.vm.to_string(),
            retired: e.retired,
        }
    }
}
#[derive(Clone, Debug, Default)]
pub struct PrivateCatalog {
    entries: Vec<PrivateDefinition>,
    by_key: HashMap<(String, u16, u8), Vr>,
}
impl PrivateCatalog {
    pub fn new(entries: &[PrivateDefinition]) -> Result<PrivateCatalog, PrivateDefinitionError> {
        let mut catalog = PrivateCatalog::default();
        for (i, e) in entries.iter().enumerate() {
            let invalid = PrivateDefinitionError {
                entry: i,
                duplicate: false,
            };
            let Ok(creator) = private_creator_id(&e.creator) else {
                return Err(invalid);
            };
            let vr = match Vr::parse(e.vr.as_bytes()) {
                Some(vr) if vr != Vr::UN && is_private_group(e.group) => vr,
                _ => return Err(invalid),
            };
            let key = (creator.to_string(), e.group, e.offset);
            if catalog.by_key.contains_key(&key) {
                return Err(PrivateDefinitionError {
                    entry: i,
                    duplicate: true,
                });
            }
            catalog.by_key.insert(key, vr);
            let mut entry = e.clone();
            entry.creator = creator.to_string();
            catalog.entries.push(entry);
        }
        Ok(catalog)
    }
    pub fn by_private(&self, creator: &str, group: u16, offset: u8) -> Option<Vr> {
        let creator = private_creator_id(creator).ok()?;
        self.by_key
            .get(&(creator.to_string(), group, offset))
            .copied()
    }
    pub fn entries(&self) -> &[PrivateDefinition] {
        &self.entries
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn def(creator: &str, group: u16, offset: u8, vr: &str) -> PrivateDefinition {
        PrivateDefinition {
            creator: creator.into(),
            group,
            offset,
            vr: vr.into(),
            ..PrivateDefinition::default()
        }
    }
    #[test]
    fn construction() {
        let c = PrivateCatalog::new(&[def(" ACME ", 0x0029, 0x10, "LO")]).unwrap();
        assert_eq!(c.by_private("ACME", 0x0029, 0x10), Some(Vr::LO));
        assert_eq!(c.by_private("ACME  ", 0x0029, 0x10), Some(Vr::LO));
        assert_eq!(c.by_private("acme", 0x0029, 0x10), None);
        assert_eq!(c.entries()[0].creator, "ACME");
        for bad in [
            def("", 0x0029, 1, "LO"),
            def("A", 0x0028, 1, "LO"),
            def("A", 0x0029, 1, "UN"),
            def("A", 0x0029, 1, "XX"),
        ] {
            assert_eq!(
                PrivateCatalog::new(&[bad]).unwrap_err().to_string(),
                "dicom: invalid private dictionary definition: entry 0"
            );
        }
        let twice = [def("A", 0x0029, 1, "LO"), def("A ", 0x0029, 1, "SH")];
        assert_eq!(
            PrivateCatalog::new(&twice).unwrap_err().to_string(),
            "dicom: invalid private dictionary definition: duplicate entry 1"
        );
    }
}
