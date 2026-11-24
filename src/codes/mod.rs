#![forbid(unsafe_code)]
use crate::textfmt;
use std::collections::HashSet;
use std::fmt;
#[rustfmt::skip]
pub mod dcm;
#[rustfmt::skip]
pub mod ln;
#[rustfmt::skip]
pub mod sct;
#[rustfmt::skip]
pub mod ucum;
#[rustfmt::skip]
pub mod umls;
#[rustfmt::skip]
mod tables;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Code<'a> {
    pub value: &'a str,
    pub scheme: &'a str,
    pub meaning: &'a str,
}
impl<'a> Code<'a> {
    pub const fn new(value: &'a str, scheme: &'a str, meaning: &'a str) -> Self {
        Code {
            value,
            scheme,
            meaning,
        }
    }
    pub fn equal(&self, other: &Code<'_>) -> bool {
        self.value == other.value && self.scheme == other.scheme
    }
}
impl fmt::Display for Code<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "({}, {}, {})",
            self.value,
            self.scheme,
            textfmt::quote(self.meaning)
        )
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Term {
    pub code: Code<'static>,
    pub retired: bool,
}
impl Term {
    pub const fn new(code: Code<'static>, retired: bool) -> Self {
        Term { code, retired }
    }
}
#[derive(Debug, PartialEq, Eq)]
pub struct ContextGroup {
    pub id: u32,
    pub keyword: &'static str,
    pub name: &'static str,
    pub extensible: bool,
    pub version: &'static str,
    pub uid: &'static str,
    pub retired: bool,
    pub defined_by_reference: bool,
    pub codes: &'static [Term],
    pub includes: &'static [u32],
}
pub fn source() -> (&'static str, &'static str) {
    (tables::EDITION, tables::SHA256)
}
pub fn lookup(scheme: &str, value: &str) -> Option<Term> {
    tables::TERMS
        .binary_search_by(|t| (t.code.scheme, t.code.value).cmp(&(scheme, value)))
        .ok()
        .map(|i| tables::TERMS[i])
}
pub fn context_group_by_id(id: i64) -> Option<&'static ContextGroup> {
    let id = u32::try_from(id).ok()?;
    tables::GROUPS
        .binary_search_by_key(&id, |g| g.id)
        .ok()
        .map(|i| &tables::GROUPS[i])
}
pub fn context_group_by_keyword(keyword: &str) -> Option<&'static ContextGroup> {
    let i = tables::KEYWORDS
        .binary_search_by_key(&keyword, |&(k, _)| k)
        .ok()?;
    context_group_by_id(i64::from(tables::KEYWORDS[i].1))
}
impl ContextGroup {
    pub fn members(&self) -> Vec<Term> {
        fn walk(
            group: &ContextGroup,
            visited: &mut HashSet<u32>,
            seen: &mut HashSet<(&'static str, &'static str)>,
            out: &mut Vec<Term>,
        ) {
            if !visited.insert(group.id) {
                return;
            }
            for term in group.codes {
                if seen.insert((term.code.scheme, term.code.value)) {
                    out.push(*term);
                }
            }
            for &id in group.includes {
                if let Some(included) = context_group_by_id(i64::from(id)) {
                    walk(included, visited, seen, out);
                }
            }
        }
        let mut out = Vec::new();
        walk(self, &mut HashSet::new(), &mut HashSet::new(), &mut out);
        out
    }
    pub fn member(&self, c: &Code<'_>) -> Option<Term> {
        self.members().into_iter().find(|t| t.code.equal(c))
    }
    pub fn contains(&self, c: &Code<'_>) -> (bool, bool) {
        fn walk(
            group: &ContextGroup,
            c: &Code<'_>,
            visited: &mut HashSet<u32>,
            decided: &mut bool,
        ) -> bool {
            if !visited.insert(group.id) {
                return false;
            }
            if group.defined_by_reference {
                *decided = false;
            }
            if group.codes.iter().any(|t| t.code.equal(c)) {
                return true;
            }
            group.includes.iter().any(|&id| {
                context_group_by_id(i64::from(id)).is_some_and(|g| walk(g, c, visited, decided))
            })
        }
        let mut decided = true;
        if walk(self, c, &mut HashSet::new(), &mut decided) {
            return (true, true);
        }
        (false, decided)
    }
}
fn find_named(table: &[(&str, Code<'static>)], name: &str) -> Option<Code<'static>> {
    table
        .binary_search_by_key(&name, |&(n, _)| n)
        .ok()
        .map(|i| table[i].1)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tables_are_sorted() {
        assert!(
            tables::TERMS
                .windows(2)
                .all(|w| (w[0].code.scheme, w[0].code.value) < (w[1].code.scheme, w[1].code.value))
        );
        assert!(tables::GROUPS.windows(2).all(|w| w[0].id < w[1].id));
        assert!(tables::KEYWORDS.windows(2).all(|w| w[0].0 < w[1].0));
    }
    #[test]
    fn named_codes_match_lookup() {
        for code in [
            dcm::ImagingMeasurementReport,
            dcm::Finding,
            dcm::OfInterest,
            sct::FindingSite,
            sct::Area,
            ucum::Mm,
            umls::QualitativeEvaluations,
        ] {
            assert_eq!(lookup(code.scheme, code.value).map(|t| t.code), Some(code));
        }
        let retired = lookup("DCM", "AS").unwrap();
        assert!(retired.retired && retired.code.meaning == "Angioscopy");
        assert!(lookup("DCM", "no-such-code").is_none());
        assert_eq!(dcm::Finding.to_string(), r#"(121071, DCM, "Finding")"#);
        assert!(dcm::Finding.equal(&Code::new("121071", "DCM", "other")));
        assert_eq!(dcm::by_name("Finding"), Some(dcm::Finding));
    }
    #[test]
    fn context_groups_resolve_includes_and_extensibility() {
        let group = context_group_by_id(7464).unwrap();
        assert_eq!(group.keyword, "GeneralRegionOfInterestMeasurementModifier");
        let mean = lookup("SCT", "373098007").unwrap();
        assert_eq!(group.contains(&mean.code), (true, true));
        assert_eq!(group.contains(&dcm::Finding), (false, true));
        assert_eq!(group.members().len(), group.codes.len() + 3);
        assert_eq!(context_group_by_keyword("MinMaxMean").unwrap().id, 3488);
        let units = context_group_by_id(82).unwrap();
        assert!(units.defined_by_reference);
        assert!(!units.contains(&ucum::Mm).1);
        assert!(context_group_by_id(999999).is_none());
    }
    #[test]
    fn member_takes_the_group_meaning() {
        let linear = context_group_by_id(7470).unwrap();
        assert_eq!(
            linear.member(&sct::LongAxis).unwrap().code.meaning,
            "Long Axis"
        );
        assert_eq!(sct::LongAxis.meaning, "Long axis");
        assert!(linear.member(&dcm::Finding).is_none());
    }
}
