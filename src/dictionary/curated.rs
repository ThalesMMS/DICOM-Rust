use super::curated_tables::{LICENSE_NOTICE, RECORDS};
use super::private::{PrivateEntry, private_creator_id};
use std::collections::HashMap;
use std::sync::OnceLock;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Origin {
    pub project: &'static str,
    pub version: &'static str,
    pub commit: &'static str,
    pub source_url: &'static str,
    pub source_sha256: &'static str,
    pub source_line: u32,
    pub definition_sha256: &'static str,
    pub license: &'static str,
    pub license_url: &'static str,
    pub restrictions: &'static str,
    pub verification: &'static str,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Record {
    pub definition: PrivateEntry,
    pub origin: Origin,
}
type Key = (&'static str, u16, u8);
fn index() -> &'static HashMap<Key, usize> {
    static INDEX: OnceLock<HashMap<Key, usize>> = OnceLock::new();
    INDEX.get_or_init(|| {
        RECORDS
            .iter()
            .enumerate()
            .map(|(i, r)| {
                (
                    (
                        r.definition.creator,
                        r.definition.group,
                        r.definition.offset,
                    ),
                    i,
                )
            })
            .collect()
    })
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Catalog;
impl Catalog {
    pub fn new() -> Catalog {
        Catalog
    }
    pub fn lookup(&self, creator: &str, group: u16, offset: u8) -> Option<&'static Record> {
        let creator = private_creator_id(creator).ok()?;
        index().get(&(creator, group, offset)).map(|&i| &RECORDS[i])
    }
    pub fn by_private(
        &self,
        creator: &str,
        group: u16,
        offset: u8,
    ) -> Option<&'static PrivateEntry> {
        self.lookup(creator, group, offset).map(|r| &r.definition)
    }
    pub fn entries(&self) -> &'static [Record] {
        &RECORDS
    }
}
pub fn license_notice() -> &'static str {
    LICENSE_NOTICE
}
#[cfg(test)]
mod tests {
    use super::super::private::is_private_group;
    use super::*;
    #[test]
    fn records_are_valid_and_unique() {
        let catalog = Catalog::new();
        assert_eq!(index().len(), RECORDS.len());
        for record in catalog.entries() {
            let d = &record.definition;
            assert_eq!(private_creator_id(d.creator), Ok(d.creator));
            assert!(is_private_group(d.group));
            assert!(
                crate::object::Vr::parse(d.vr.as_bytes()).is_some() && d.vr != "UN",
                "{d:?}"
            );
            assert_eq!(
                catalog.lookup(&format!(" {} ", d.creator), d.group, d.offset),
                Some(record)
            );
        }
        assert_eq!(
            catalog
                .by_private("FDMS 1.0", 0x0009, 0x04)
                .map(|e| e.tag()),
            Some(crate::object::Tag::new(0x0009, 0x0004))
        );
        assert!(catalog.lookup("fdms 1.0", 0x0009, 0x04).is_none());
        assert!(license_notice().contains("OFFIS"));
    }
}
