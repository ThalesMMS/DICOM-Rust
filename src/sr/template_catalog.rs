use super::template_catalog_tables::STANDARD_TEMPLATE_CATALOG;
pub use super::template_catalog_tables::{
    STANDARD_TEMPLATE_CATALOG_SHA256, STANDARD_TEMPLATE_EDITION, STANDARD_TEMPLATE_SOURCE,
    STANDARD_TEMPLATE_SOURCE_SHA256,
};
use super::template_metadata::{CodeKey, MAX_REGISTRY_STRING, code_key_for_entry};
use super::template_registry::{
    Cardinality, ContextGroupDefinition, ContextGroupKey, DefinitionProvenance,
    MAX_REGISTRY_DEFINITIONS, TEMPLATE_REGISTRY_CODE_MISSING_INCLUDE,
    TEMPLATE_REGISTRY_CODE_RESOURCE_LIMIT, TemplateCondition, TemplateConditionContext,
    TemplateDefinition, TemplateKey, TemplateRegistry, TemplateRow, new_template_registry,
    reference_mode, registry_error, requirement,
};
use super::{Kind, SrError, new_code, value};
use crate::codes;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::{Arc, OnceLock};
pub const STANDARD_TEMPLATE_MAPPING_RESOURCE: &str = "DCMR";
pub const STANDARD_CONTEXT_GROUP_MAPPING_RESOURCE: &str = "DCMR";
#[derive(Debug)]
pub(crate) struct CatalogTemplate {
    pub id: i64,
    pub identifier: &'static str,
    pub name: &'static str,
    pub extensible: bool,
    pub order: &'static str,
    pub root: &'static str,
    pub rows: &'static [CatalogRow],
}
#[derive(Debug)]
pub struct CatalogRow {
    pub label: &'static str,
    pub level: i64,
    pub relationship: &'static str,
    pub by_reference: bool,
    pub value_type: &'static str,
    pub include: i64,
    pub include_identifier: &'static str,
    pub concept: &'static str,
    pub code: (&'static str, &'static str, &'static str),
    pub concept_cid: i64,
    pub concept_strength: &'static str,
    pub parameter: &'static str,
    pub min: i64,
    pub max: i64,
    pub requirement: &'static str,
    pub condition: &'static str,
    pub formal: &'static str,
    pub evaluate: Option<&'static CatalogCondition>,
    pub value_set: &'static str,
    pub value_set_cid: i64,
    pub value_set_strength: &'static str,
}
#[derive(Debug)]
pub struct CatalogCondition {
    pub op: &'static str,
    pub rows: &'static [&'static str],
    pub args: &'static [CatalogCondition],
}
impl CatalogCondition {
    fn eval(&self, count: &dyn Fn(&str) -> i64) -> bool {
        match self.op {
            "present" => self.rows.iter().any(|l| count(l) > 0),
            "absent" | "xor" => self.rows.iter().all(|l| count(l) <= 0),
            "and" => self.args.iter().all(|a| a.eval(count)),
            "or" => self.args.iter().any(|a| a.eval(count)),
            "not" => !self.args[0].eval(count),
            _ => false,
        }
    }
}
impl CatalogTemplate {
    fn canonical_identifier(&self) -> String {
        if !self.identifier.is_empty() {
            return self.identifier.into();
        }
        self.id.to_string()
    }
}
impl CatalogRow {
    fn canonical_include(&self) -> String {
        if !self.include_identifier.is_empty() {
            return self.include_identifier.into();
        }
        if self.include > 0 {
            return self.include.to_string();
        }
        String::new()
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StandardTemplateInfo {
    pub id: i64,
    pub identifier: String,
    pub name: String,
    pub extensible: bool,
    pub order: String,
    pub root: String,
    pub rows: i64,
    pub conditional_rows: i64,
    pub evaluated_conditions: i64,
    pub unresolved_includes: Vec<i64>,
    pub unresolved_include_identifiers: Vec<String>,
    pub recursive_include_rows: Vec<String>,
}
fn index() -> &'static HashMap<String, &'static CatalogTemplate> {
    static INDEX: OnceLock<HashMap<String, &'static CatalogTemplate>> = OnceLock::new();
    INDEX.get_or_init(|| {
        STANDARD_TEMPLATE_CATALOG
            .iter()
            .map(|t| (t.canonical_identifier(), t))
            .collect()
    })
}
fn canonical_identifier(v: &str) -> bool {
    let b = v.as_bytes();
    if b.is_empty() || v.len() > MAX_REGISTRY_STRING || !(b'1'..=b'9').contains(&b[0]) {
        return false;
    }
    let mut end = b.len();
    if b[end - 1].is_ascii_uppercase() {
        end -= 1;
    }
    if !b[..end].iter().all(u8::is_ascii_digit) {
        return false;
    }
    end > 0 && v[..end].parse::<i64>().is_ok_and(|n| n > 0)
}
fn catalog_template(identifier: &str) -> Option<&'static CatalogTemplate> {
    if !canonical_identifier(identifier) {
        return None;
    }
    index().get(identifier).copied()
}
pub fn standard_template_key(id: i64) -> Option<TemplateKey> {
    if id <= 0 {
        return None;
    }
    standard_template_key_by_identifier(&id.to_string())
}
pub fn standard_template_key_by_identifier(identifier: &str) -> Option<TemplateKey> {
    catalog_template(identifier)?;
    Some(TemplateKey {
        mapping_resource: STANDARD_TEMPLATE_MAPPING_RESOURCE.into(),
        identifier: identifier.into(),
        version: STANDARD_TEMPLATE_EDITION.into(),
    })
}
pub fn standard_template_ids() -> Vec<i64> {
    let mut out: Vec<i64> = STANDARD_TEMPLATE_CATALOG
        .iter()
        .filter(|t| t.id > 0)
        .map(|t| t.id)
        .collect();
    out.sort_unstable();
    out
}
pub fn standard_template_identifiers() -> Vec<String> {
    STANDARD_TEMPLATE_CATALOG
        .iter()
        .map(CatalogTemplate::canonical_identifier)
        .collect()
}
pub fn lookup_standard_template(id: i64) -> Option<StandardTemplateInfo> {
    if id <= 0 {
        return None;
    }
    lookup_standard_template_by_identifier(&id.to_string())
}
pub fn lookup_standard_template_by_identifier(identifier: &str) -> Option<StandardTemplateInfo> {
    let t = catalog_template(identifier)?;
    let mut info = StandardTemplateInfo {
        id: t.id,
        identifier: identifier.into(),
        name: t.name.into(),
        extensible: t.extensible,
        order: t.order.into(),
        root: t.root.into(),
        rows: t.rows.len() as i64,
        ..Default::default()
    };
    for row in t.rows {
        if row.requirement == "MC" || row.requirement == "UC" {
            info.conditional_rows += 1;
        }
        if row.requirement == "MC" && row.evaluate.is_some() {
            info.evaluated_conditions += 1;
        }
        let include = row.canonical_include();
        if include == identifier {
            info.recursive_include_rows.push(row.label.into());
        }
        if !include.is_empty() && catalog_template(&include).is_none() {
            info.unresolved_include_identifiers.push(include);
            if row.include > 0 {
                info.unresolved_includes.push(row.include);
            }
        }
    }
    Some(info)
}
pub fn standard_templates(
    ids: &[i64],
) -> Result<(Vec<TemplateDefinition>, Vec<ContextGroupDefinition>), SrError> {
    if ids.len() > MAX_REGISTRY_DEFINITIONS {
        return Err(registry_error(TEMPLATE_REGISTRY_CODE_RESOURCE_LIMIT));
    }
    let identifiers: Vec<String> = ids.iter().map(i64::to_string).collect();
    standard_templates_by_identifier(&identifiers)
}
pub fn standard_templates_by_identifier(
    identifiers: &[String],
) -> Result<(Vec<TemplateDefinition>, Vec<ContextGroupDefinition>), SrError> {
    if identifiers.len() > MAX_REGISTRY_DEFINITIONS {
        return Err(registry_error(TEMPLATE_REGISTRY_CODE_RESOURCE_LIMIT));
    }
    let mut definitions = Vec::new();
    let mut group_ids = BTreeSet::new();
    let mut added = HashSet::new();
    fn add(
        identifier: &str,
        added: &mut HashSet<String>,
        definitions: &mut Vec<TemplateDefinition>,
        group_ids: &mut BTreeSet<i64>,
    ) -> Result<(), SrError> {
        if added.contains(identifier) {
            return Ok(());
        }
        let Some(t) = catalog_template(identifier) else {
            return Err(registry_error(TEMPLATE_REGISTRY_CODE_MISSING_INCLUDE));
        };
        added.insert(identifier.into());
        let (definition, includes, groups) = definition_of(t);
        definitions.push(definition);
        group_ids.extend(groups);
        for include in includes {
            add(&include, added, definitions, group_ids)?;
        }
        Ok(())
    }
    for identifier in identifiers {
        add(identifier, &mut added, &mut definitions, &mut group_ids)?;
    }
    let groups: Vec<i64> = group_ids.into_iter().collect();
    let context_groups = standard_context_groups(&groups)?;
    Ok((definitions, context_groups))
}
pub fn new_standard_template_registry(ids: &[i64]) -> Result<TemplateRegistry, SrError> {
    let (t, g) = standard_templates(ids)?;
    new_template_registry(&t, &g)
}
pub fn new_standard_template_registry_by_identifier(
    identifiers: &[String],
) -> Result<TemplateRegistry, SrError> {
    let (t, g) = standard_templates_by_identifier(identifiers)?;
    new_template_registry(&t, &g)
}
fn definition_of(t: &'static CatalogTemplate) -> (TemplateDefinition, Vec<String>, Vec<i64>) {
    let id = t.canonical_identifier();
    let key = standard_template_key_by_identifier(&id).unwrap_or_default();
    let mut definition = TemplateDefinition {
        key,
        extensible: t.extensible,
        order_significant: t.order == "Significant",
        provenance: DefinitionProvenance {
            source: format!(
                "DICOM PS3.16 {STANDARD_TEMPLATE_EDITION} TID {id} ({STANDARD_TEMPLATE_SOURCE})"
            ),
            checksum: STANDARD_TEMPLATE_CATALOG_SHA256.into(),
        },
        rows: Vec::new(),
    };
    let mut includes = Vec::new();
    let mut groups = Vec::new();
    let top = rows_of(t, 0, t.rows.len(), 0, &mut includes, &mut groups);
    if t.root == "Yes"
        && top.len() == 1
        && top[0].value_types.len() == 1
        && top[0].value_types[0] == value::CONTAINER
    {
        definition.rows = top
            .into_iter()
            .next()
            .map(|r| r.children)
            .unwrap_or_default();
    } else {
        definition.rows = top;
    }
    (definition, includes, groups)
}
fn rows_of(
    t: &'static CatalogTemplate,
    start: usize,
    end: usize,
    level: i64,
    includes: &mut Vec<String>,
    groups: &mut Vec<i64>,
) -> Vec<TemplateRow> {
    let mut out = Vec::new();
    let mut index = start;
    while index < end {
        let mut next = index + 1;
        while next < end && t.rows[next].level > level {
            next += 1;
        }
        if let Some(mut converted) = row_of(t, &t.rows[index], includes, groups) {
            if converted.include.is_none() {
                converted.children = rows_of(t, index + 1, next, level + 1, includes, groups);
            }
            out.push(converted);
        }
        index = next;
    }
    out
}
fn row_of(
    t: &'static CatalogTemplate,
    row: &'static CatalogRow,
    includes: &mut Vec<String>,
    groups: &mut Vec<i64>,
) -> Option<TemplateRow> {
    let id = t.canonical_identifier();
    let mut out = TemplateRow {
        rule_id: catalog_rule_id(&id, row.label),
        cardinality: Cardinality {
            min: row.min,
            max: row.max,
        },
        reference_mode: if row.by_reference {
            reference_mode::BY_REFERENCE
        } else {
            reference_mode::BY_VALUE
        },
        ..Default::default()
    };
    if !row.relationship.is_empty() {
        out.relationship_types = vec![row.relationship.into()];
    }
    out.requirement = match row.requirement {
        "M" => requirement::REQUIRED,
        "MC" => match row.evaluate {
            Some(condition) => {
                out.condition_id = row.formal.into();
                out.condition = Some(catalog_condition(&id, row.label, condition));
                requirement::CONDITIONAL
            }
            None => requirement::OPTIONAL,
        },
        _ => requirement::OPTIONAL,
    };
    let include = row.canonical_include();
    if !include.is_empty() {
        catalog_template(&include)?;
        out.include = standard_template_key_by_identifier(&include);
        out.include_fills_relationship = true;
        if row.relationship.is_empty() {
            out.reference_mode = 0;
        }
        out.allow_recursive_include = include == id;
        includes.push(include);
        return Some(out);
    }
    out.value_types = vec![row.value_type.into()];
    match row.concept {
        "EV" | "DT" => out.concept_names = vec![catalog_code_key(row.code.0, row.code.1)],
        "CID" if row.concept_strength == "D" => {
            if let Some(members) = non_extensible_members(row.concept_cid) {
                out.concept_names = members;
            }
        }
        _ => {}
    }
    if row.value_set_strength == "D"
        && row.value_type == value::CODE
        && let Some(group) = codes::context_group_by_id(row.value_set_cid)
        && !group.extensible
        && !group.defined_by_reference
        && let Some(key) = standard_context_group_key(row.value_set_cid)
    {
        out.context_group = Some(key);
        groups.push(row.value_set_cid);
    }
    Some(out)
}
fn catalog_rule_id(id: &str, label: &str) -> String {
    format!("TID{id}:{label}")
}
fn catalog_condition(
    id: &str,
    label: &str,
    condition: &'static CatalogCondition,
) -> TemplateCondition {
    let own = catalog_rule_id(id, label);
    let id = id.to_string();
    Arc::new(move |input: &TemplateConditionContext| {
        let components: Vec<&str> = input.rule_id.split('.').collect();
        let Some(position) = components.iter().rposition(|c| *c == own) else {
            return Err(format!("dicom/sr: condition of {own} evaluated for another row").into());
        };
        let mut prefix = components[..position].join(".");
        if !prefix.is_empty() {
            prefix.push('.');
        }
        let count = |sibling: &str| -> i64 {
            let target = format!("{prefix}{}", catalog_rule_id(&id, sibling));
            let dotted = format!("{target}.");
            input
                .row_counts
                .iter()
                .filter(|(rule, _)| **rule == target || rule.starts_with(&dotted))
                .map(|(_, c)| *c)
                .sum()
        };
        Ok(condition.eval(&count))
    })
}
fn catalog_code_key(v: &str, scheme: &str) -> CodeKey {
    if v.chars().count() > 16 {
        return CodeKey {
            long_code_value: v.into(),
            coding_scheme_designator: scheme.into(),
            ..Default::default()
        };
    }
    CodeKey {
        code_value: v.into(),
        coding_scheme_designator: scheme.into(),
        ..Default::default()
    }
}
fn non_extensible_members(id: i64) -> Option<Vec<CodeKey>> {
    let group = codes::context_group_by_id(id)?;
    if group.extensible || group.defined_by_reference {
        return None;
    }
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    fn walk(id: i64, seen: &mut HashSet<i64>, out: &mut Vec<CodeKey>) {
        if !seen.insert(id) {
            return;
        }
        let Some(group) = codes::context_group_by_id(id) else {
            return;
        };
        for term in group.codes {
            out.push(catalog_code_key(term.code.value, term.code.scheme));
        }
        for &included in group.includes {
            walk(i64::from(included), seen, out);
        }
    }
    walk(id, &mut seen, &mut out);
    if out.is_empty() { None } else { Some(out) }
}
pub fn standard_context_group_key(id: i64) -> Option<ContextGroupKey> {
    let group = codes::context_group_by_id(id)?;
    if group.version.is_empty() {
        return None;
    }
    Some(ContextGroupKey {
        mapping_resource: STANDARD_CONTEXT_GROUP_MAPPING_RESOURCE.into(),
        identifier: id.to_string(),
        version: group.version.into(),
    })
}
pub fn standard_context_groups(ids: &[i64]) -> Result<Vec<ContextGroupDefinition>, SrError> {
    let (edition, sum) = codes::source();
    let mut out = Vec::new();
    let mut added = HashSet::new();
    fn add(
        id: i64,
        edition: &str,
        sum: &str,
        added: &mut HashSet<i64>,
        out: &mut Vec<ContextGroupDefinition>,
    ) -> Result<(), SrError> {
        if !added.insert(id) {
            return Ok(());
        }
        let Some(group) = codes::context_group_by_id(id) else {
            return Err(SrError::msg(format!(
                "dicom/sr: no PS3.16 context group CID {id}"
            )));
        };
        if group.defined_by_reference {
            return Err(SrError::new(
                Kind::ContextGroupByReference,
                &format!(": CID {id}"),
            ));
        }
        let mut definition = ContextGroupDefinition {
            key: standard_context_group_key(id).unwrap_or_default(),
            extensible: group.extensible,
            provenance: DefinitionProvenance {
                source: format!("DICOM {edition} CID {id}"),
                checksum: sum.into(),
            },
            ..Default::default()
        };
        for term in group.codes {
            let (entry, context) = new_code(term.code.value, term.code.scheme, term.code.meaning);
            definition.codes.push(code_key_for_entry(&entry, &context));
        }
        for &included in group.includes {
            let included = i64::from(included);
            let Some(key) = standard_context_group_key(included) else {
                return Err(SrError::msg(format!(
                    "dicom/sr: CID {id} includes unknown CID {included}"
                )));
            };
            definition.includes.push(key);
            add(included, edition, sum, added, out)?;
        }
        out.push(definition);
        Ok(())
    }
    for &id in ids {
        add(id, edition, sum, &mut added, &mut out)?;
    }
    Ok(out)
}
pub(crate) fn standard_template_catalog() -> &'static [CatalogTemplate] {
    STANDARD_TEMPLATE_CATALOG
}
