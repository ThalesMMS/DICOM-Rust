use super::reference::{
    ContentItemIdentifier, DiagnosticFinding, DiagnosticSeverity, ValidationMode, ValidationReport,
    append_path, identifier_string,
};
use super::relationship_tables::{RELATIONSHIP_CONSTRAINTS_EDITION, RELATIONSHIP_IODS};
use super::{ContentItem, Document, SrError, clean, value};
use crate::textfmt::quote;
#[derive(Debug)]
pub(crate) enum RelationshipTerm {
    Any(&'static str),
    List(&'static [&'static str]),
}
impl RelationshipTerm {
    fn matches(&self, v: &str) -> bool {
        match self {
            RelationshipTerm::Any(except) => v != *except,
            RelationshipTerm::List(list) => list.contains(&v),
        }
    }
}
#[derive(Debug)]
pub(crate) struct RelationshipRule {
    pub sources: RelationshipTerm,
    pub relationship: RelationshipTerm,
    pub targets: RelationshipTerm,
}
#[derive(Debug)]
pub(crate) struct RelationshipIod {
    pub sop_class_uid: &'static str,
    pub name: &'static str,
    pub table: &'static str,
    pub by_reference: &'static str,
    pub by_reference_relationships: &'static [&'static str],
    pub rules: &'static [RelationshipRule],
}
pub(super) fn lookup_relationship_iod(sop_class_uid: &str) -> Option<&'static RelationshipIod> {
    let uid = clean(sop_class_uid);
    RELATIONSHIP_IODS.iter().find(|i| i.sop_class_uid == uid)
}
impl RelationshipIod {
    pub(super) fn allows(&self, source: &str, relationship: &str, target: &str) -> bool {
        self.rules.iter().any(|r| {
            r.sources.matches(source)
                && r.relationship.matches(relationship)
                && r.targets.matches(target)
        })
    }
    pub(super) fn allows_by_reference(&self, relationship: &str) -> bool {
        let listed = self.by_reference_relationships.contains(&relationship);
        match self.by_reference {
            "all" => true,
            "all-except" => !listed,
            "only" => listed,
            _ => false,
        }
    }
    pub(super) fn rule_id(&self) -> String {
        format!(
            "PS3.3-{RELATIONSHIP_CONSTRAINTS_EDITION}.Table.{}",
            self.table
        )
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RelationshipConstraint {
    pub sop_class_uid: String,
    pub iod_name: String,
    pub table: String,
    pub edition: String,
}
pub fn relationship_constraint_for(sop_class_uid: &str) -> Option<RelationshipConstraint> {
    let iod = lookup_relationship_iod(sop_class_uid)?;
    Some(RelationshipConstraint {
        sop_class_uid: iod.sop_class_uid.into(),
        iod_name: iod.name.into(),
        table: iod.table.into(),
        edition: RELATIONSHIP_CONSTRAINTS_EDITION.into(),
    })
}
pub fn relationship_allowed(
    sop_class_uid: &str,
    source: &str,
    relationship: &str,
    target: &str,
    by_reference: bool,
) -> (bool, bool) {
    let Some(iod) = lookup_relationship_iod(sop_class_uid) else {
        return (false, false);
    };
    if by_reference && !iod.allows_by_reference(relationship) {
        return (false, true);
    }
    (iod.allows(source, relationship, target), true)
}
pub const RELATIONSHIP_CODE_FORBIDDEN: &str = "sr.relationship.forbidden";
pub const RELATIONSHIP_CODE_MISSING: &str = "sr.relationship.missing";
pub const RELATIONSHIP_CODE_UNKNOWN: &str = "sr.relationship.unknown";
pub const RELATIONSHIP_CODE_UNKNOWN_VALUE_TYPE: &str = "sr.relationship.unknown_value_type";
pub const RELATIONSHIP_CODE_RESOURCE_LIMIT: &str = "sr.relationship.resource_limit";
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RelationshipOptions {
    pub mode: ValidationMode,
    pub max_depth: i64,
    pub max_items: i64,
    pub max_findings: i64,
}
pub fn default_relationship_options() -> RelationshipOptions {
    RelationshipOptions {
        mode: ValidationMode::STRICT,
        max_depth: super::read::MAX_CONTENT_TREE_DEPTH,
        max_items: 100_000,
        max_findings: 1_024,
    }
}
pub(super) fn known_value_type(v: &str) -> bool {
    matches!(
        v,
        value::CONTAINER
            | value::TEXT
            | value::CODE
            | value::IMAGE
            | value::NUM
            | value::DATETIME
            | value::DATE
            | value::TIME
            | value::UIDREF
            | value::PNAME
            | value::COMPOSITE
            | value::WAVEFORM
            | value::SCOORD
            | value::SCOORD3D
            | value::TCOORD
            | value::TABLE
    )
}
fn known_relationship(r: &str) -> bool {
    matches!(
        r,
        super::RELATIONSHIP_CONTAINS
            | super::RELATIONSHIP_HAS_OBS_CONTEXT
            | super::RELATIONSHIP_HAS_ACQ_CONTEXT
            | super::RELATIONSHIP_HAS_CONCEPT_MOD
            | super::RELATIONSHIP_HAS_PROPERTIES
            | super::RELATIONSHIP_INFERRED_FROM
            | super::RELATIONSHIP_SELECTED_FROM
    )
}
pub fn validate_relationships(
    document: Option<&Document>,
    options: RelationshipOptions,
) -> (ValidationReport, Option<SrError>) {
    let defaults = default_relationship_options();
    let mut options = options;
    if options.mode.0 == 0 {
        options.mode = defaults.mode;
    }
    if options.max_depth == 0 {
        options.max_depth = defaults.max_depth;
    }
    if options.max_items == 0 {
        options.max_items = defaults.max_items;
    }
    if options.max_findings == 0 {
        options.max_findings = defaults.max_findings;
    }
    if (options.mode != ValidationMode::STRICT && options.mode != ValidationMode::WARN)
        || options.max_depth < 1
        || options.max_items < 1
        || options.max_findings < 1
    {
        return (
            ValidationReport::default(),
            Some(SrError::relationship(": invalid options")),
        );
    }
    let Some(document) = document else {
        return (
            ValidationReport::default(),
            Some(SrError::invalid_document(": document is nil")),
        );
    };
    let Some(iod) = lookup_relationship_iod(&document.sop_class_uid) else {
        return (
            ValidationReport::default(),
            Some(SrError::unconstrained(&format!(
                ": {}",
                quote(clean(&document.sop_class_uid))
            ))),
        );
    };
    let mut v = RelWalk {
        iod,
        options,
        report: ValidationReport::default(),
        items: 0,
    };
    v.walk(&[1], value::CONTAINER, &document.content, 1);
    let report = v.report;
    if options.mode == ValidationMode::STRICT && report.has_errors() {
        let f = &report.findings[0];
        let err = SrError::relationship(&format!(
            ": {} at {} ({} {} {}, {})",
            f.code,
            identifier_string(&f.path),
            f.source_value_type,
            f.relationship_type,
            f.target_value_type,
            iod.name
        ));
        return (report, Some(err));
    }
    (report, None)
}
struct RelWalk {
    iod: &'static RelationshipIod,
    options: RelationshipOptions,
    report: ValidationReport,
    items: i64,
}
impl RelWalk {
    fn add(
        &mut self,
        code: &str,
        path: &[u32],
        source: &str,
        relationship: &str,
        target: &str,
        message: &str,
    ) {
        if self.report.findings.len() as i64 >= self.options.max_findings {
            self.report.truncated = true;
            return;
        }
        let mut severity = DiagnosticSeverity::Error;
        if (self.options.mode == ValidationMode::WARN && code != RELATIONSHIP_CODE_RESOURCE_LIMIT)
            || code == RELATIONSHIP_CODE_UNKNOWN_VALUE_TYPE
        {
            severity = DiagnosticSeverity::Warning;
        }
        self.report.findings.push(DiagnosticFinding {
            path: path.to_vec(),
            target: ContentItemIdentifier::new(),
            rule_id: self.iod.rule_id(),
            code: code.into(),
            severity,
            message: message.into(),
            sop_class_uid: self.iod.sop_class_uid.into(),
            source_value_type: source.into(),
            relationship_type: relationship.into(),
            target_value_type: target.into(),
        });
    }
    fn walk(&mut self, parent: &[u32], source: &str, children: &[ContentItem], depth: i64) -> bool {
        if depth > self.options.max_depth {
            self.add(
                RELATIONSHIP_CODE_RESOURCE_LIMIT,
                parent,
                source,
                "",
                "",
                "content tree exceeds the validation depth limit",
            );
            return false;
        }
        for (index, item) in children.iter().enumerate() {
            self.items += 1;
            let path = append_path(parent, index as u32 + 1);
            if self.items > self.options.max_items {
                self.add(
                    RELATIONSHIP_CODE_RESOURCE_LIMIT,
                    &path,
                    source,
                    "",
                    "",
                    "content tree exceeds the validation item limit",
                );
                return false;
            }
            if !item.referenced_content_item_identifier.is_empty() {
                continue;
            }
            let relationship = clean(&item.relationship_type);
            let vt = item.value_type.as_str();
            if !known_value_type(vt) {
                self.add(
                    RELATIONSHIP_CODE_UNKNOWN_VALUE_TYPE,
                    &path,
                    source,
                    relationship,
                    vt,
                    "content item value type is not a defined term",
                );
            } else if relationship.is_empty() {
                self.add(
                    RELATIONSHIP_CODE_MISSING,
                    &path,
                    source,
                    "",
                    vt,
                    "content item has no relationship type",
                );
            } else if !known_relationship(relationship) {
                self.add(
                    RELATIONSHIP_CODE_UNKNOWN,
                    &path,
                    source,
                    relationship,
                    vt,
                    "relationship type is not a defined term",
                );
            } else if !self.iod.allows(source, relationship, vt) {
                self.add(
                    RELATIONSHIP_CODE_FORBIDDEN,
                    &path,
                    source,
                    relationship,
                    vt,
                    "relationship is not allowed between these value types by the IOD",
                );
            }
            if !self.walk(&path, vt, &item.children, depth + 1) {
                return false;
            }
        }
        true
    }
}
