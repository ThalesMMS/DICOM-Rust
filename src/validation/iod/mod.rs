mod condition;
mod coverage;
mod external;
mod functional_groups;
pub mod modules;
mod priority;
#[rustfmt::skip]
#[allow(clippy::all)]
mod tables;
mod values;
use super::Severity;
use crate::object::{Element, Object, Tag, Vr, element_length};
pub use coverage::{ConditionCount, ConditionUse, Coverage, condition_coverage};
use external::ExternalValidation;
pub use external::{
    CONTRADICTORY_EXTERNAL_CONTEXT, ConditionCheck, ConditionState, ContextReport, ExternalContext,
    ExternalFact, ExternalFamily, RuleSource as PublicRuleSource, external_condition_coverage,
    validate_with_external_context,
};
pub use functional_groups::{
    FUNCTIONAL_GROUP_CONFLICT, FunctionalGroupOptions, INVALID_FUNCTIONAL_GROUP,
    MANUAL_FUNCTIONAL_GROUP, MAX_FUNCTIONAL_GROUP_METADATA_BYTES, MISSING_FUNCTIONAL_GROUP,
    validate_functional_group_data_set, validate_functional_groups,
    validate_with_functional_groups,
};
pub use modules::ConstraintSource;
use modules::{RuleSource, ValueConstraint};
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::{Arc, Mutex, OnceLock};
pub use values::{MAX_CONSTRAINT_VALUE_BYTES, ValueCoverage, ValueRule, value_constraint_coverage};
pub const STANDARD_VERSION: &str = modules::STANDARD_VERSION;
pub type Kind = &'static str;
pub const MISSING_TYPE1: Kind = "missing-type-1";
pub const EMPTY_TYPE1: Kind = "empty-type-1";
pub const MISSING_TYPE2: Kind = "missing-type-2";
pub const MISSING_TYPE1C: Kind = "missing-type-1c";
pub const EMPTY_TYPE1C: Kind = "empty-type-1c";
pub const MISSING_TYPE2C: Kind = "missing-type-2c";
pub const UNEXPECTED_CONDITIONAL: Kind = "unexpected-conditional";
pub const MANUAL_CONDITION: Kind = "manual-condition";
pub const INVALID_ENUMERATED_VALUE: Kind = "invalid-enumerated-value";
pub const ADDITIONAL_DEFINED_TERM: Kind = "additional-defined-term";
pub const INVALID_CONTEXT_MULTIPLICITY: Kind = "invalid-context-multiplicity";
pub const MANUAL_VALUE_CONSTRAINT: Kind = "manual-value-constraint";
fn kind_severity(k: Kind) -> Severity {
    match k {
        UNEXPECTED_CONDITIONAL | ADDITIONAL_DEFINED_TERM => Severity::Warning,
        MANUAL_CONDITION | MANUAL_VALUE_CONSTRAINT => Severity::Info,
        _ => Severity::Error,
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    UnknownIod(String),
    ExternalContext,
    FunctionalGroupLimit,
    FunctionalGroupOptions,
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::UnknownIod(d) if d.is_empty() => {
                f.write_str("dicom/validation/iod: unknown IOD")
            }
            Error::UnknownIod(d) => write!(f, "dicom/validation/iod: unknown IOD: {d}"),
            Error::ExternalContext => f.write_str("dicom/validation/iod: invalid external context"),
            Error::FunctionalGroupLimit => {
                f.write_str("dicom/validation/iod: functional group resource limit")
            }
            Error::FunctionalGroupOptions => {
                f.write_str("dicom/validation/iod: invalid functional group options")
            }
        }
    }
}
impl std::error::Error for Error {}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    pub kind: Kind,
    pub severity: Severity,
    pub module: String,
    pub path: String,
    pub tag: Tag,
    pub typ: String,
    pub condition: String,
    pub rule: String,
    pub source: Option<ConstraintSource>,
    pub message: String,
}
impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} {}: {} (Type {}, {})",
            self.severity.as_str(),
            self.kind,
            self.path,
            self.typ,
            self.module
        )
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModuleCheck {
    pub module: String,
    pub usage: String,
    pub checked: bool,
    pub reason: String,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub iod: String,
    pub standard_version: String,
    pub modules: Vec<ModuleCheck>,
    pub findings: Vec<Finding>,
    pub truncated: i64,
}
impl Report {
    pub fn errors(&self) -> Vec<Finding> {
        self.findings
            .iter()
            .filter(|f| f.severity == Severity::Error)
            .cloned()
            .collect()
    }
}
#[derive(Clone, Debug, Default)]
pub struct Options {
    pub iod: String,
    pub max_findings: i64,
    pub check_values: bool,
}
pub fn validate(obj: &Object, opts: &Options) -> (Report, Option<Error>) {
    validate_with_external(obj, opts, None)
}
fn version_report() -> Report {
    Report {
        standard_version: STANDARD_VERSION.to_string(),
        ..Report::default()
    }
}
pub(crate) fn retired(tag: Tag) -> bool {
    tables::RETIRED.binary_search(&tag.key()).is_ok()
}
fn validate_with_external(
    obj: &Object,
    opts: &Options,
    external: Option<&mut ExternalValidation>,
) -> (Report, Option<Error>) {
    let mut name = opts.iod.clone();
    if name.is_empty() {
        let sop_class = obj.get_uid(Tag::new(0x0008, 0x0016)).unwrap_or_default();
        match iod_for_sop_class(&sop_class) {
            Some(n) => name = n,
            None => {
                return (
                    version_report(),
                    Some(Error::UnknownIod(format!(
                        "SOP Class {}",
                        crate::textfmt::quote(&sop_class)
                    ))),
                );
            }
        }
    }
    let Some(uses) = modules::modules(&name) else {
        return (
            version_report(),
            Some(Error::UnknownIod(crate::textfmt::quote(&name))),
        );
    };
    let max = if opts.max_findings <= 0 {
        1000
    } else {
        opts.max_findings
    };
    let mut v = Validator {
        root: obj,
        report: Report {
            iod: name.clone(),
            standard_version: STANDARD_VERSION.to_string(),
            ..Report::default()
        },
        max,
        seen: HashSet::new(),
        inferred: false,
        check_values: opts.check_values,
        external,
    };
    let mut mandatory = HashSet::new();
    for u in &uses {
        if u.usage == "M" {
            top_tags(&tree(&module_table(u.module)), &mut |t| {
                mandatory.insert(t);
            });
        }
    }
    for (index, u) in uses.iter().enumerate() {
        let mut check = ModuleCheck {
            module: u.module.to_string(),
            usage: u.usage.to_string(),
            checked: false,
            reason: String::new(),
        };
        if u.usage == "M" {
            check.checked = true;
            check.reason = "mandatory".into();
        } else {
            let mut skip_presence = false;
            if u.usage == "C" {
                let source = modules::module_source(&name, index as i32).unwrap_or_default();
                let mut state = ConditionState::Unknown;
                if let Some(ext) = v.external.as_deref_mut() {
                    let (required, known, _, _) = ext.evaluate(
                        u.condition,
                        &source,
                        &format!("module/{}", u.module),
                        u.module,
                        obj,
                        obj,
                    );
                    if known {
                        state = ConditionState::from_bool(required);
                    }
                } else {
                    state = priority::dataset_condition(u.condition, &source, obj, obj).0;
                }
                if state == ConditionState::True {
                    check.checked = true;
                    check.reason = "condition true".into();
                    skip_presence = true;
                } else {
                    match state {
                        ConditionState::False => check.reason = "condition false".into(),
                        ConditionState::Unknown if priority::has_priority(source.rule) => {
                            check.reason = "condition unknown".into();
                        }
                        _ => {}
                    }
                    if state == ConditionState::False
                        && source.rule != priority::SEG_FRAME_OF_REFERENCE_RULE
                    {
                        skip_presence = true;
                    }
                }
            }
            if !skip_presence {
                top_tags(&tree(&module_table(u.module)), &mut |tag| {
                    if !check.checked && !mandatory.contains(&tag) && obj.has(tag) {
                        check.checked = true;
                        check.reason = format!("present ({tag})");
                    }
                });
                if !check.checked && check.reason.is_empty() {
                    check.reason = "absent".into();
                }
            }
        }
        let checked = check.checked;
        let inferred = check.reason.starts_with("present");
        v.report.modules.push(check);
        if checked {
            v.inferred = inferred;
            let table = module_table(u.module);
            v.check(&tree(&table), obj, "", u.module);
            if opts.check_values && table == "Multi-frame Functional Groups Module" {
                v.check_functional_group_values(u.module);
            }
        }
    }
    (v.report, None)
}
fn module_table(module: &str) -> String {
    if modules::rows(module).is_some() {
        let with = format!("{module} Module");
        if modules::rows(&with).is_some() {
            return with;
        }
        return module.to_string();
    }
    format!("{module} Module")
}
fn iod_index() -> &'static HashMap<String, &'static str> {
    static INDEX: OnceLock<HashMap<String, &'static str>> = OnceLock::new();
    INDEX.get_or_init(|| {
        modules::iods()
            .into_iter()
            .map(|n| (n.to_lowercase(), n))
            .collect()
    })
}
pub fn iod_for_sop_class(sop_class_uid: &str) -> Option<String> {
    if sop_class_uid == "1.2.840.10008.5.1.4.1.1.66.7" {
        return Some("Segmentation".into());
    }
    let entry = crate::dictionary::uid::by_uid(sop_class_uid)?;
    let mut name: &str = entry.name;
    for suffix in [" - For Presentation", " - For Processing"] {
        name = name.strip_suffix(suffix).unwrap_or(name);
    }
    let name = name.strip_suffix(" Storage").unwrap_or(name);
    let index = iod_index();
    let candidates = [
        name.to_string(),
        name.strip_suffix(" Instance").unwrap_or(name).to_string(),
        name.replacen("Secondary Capture", "SC", 1),
    ];
    for c in &candidates {
        if let Some(iod) = index.get(&c.to_lowercase()) {
            return Some(iod.to_string());
        }
    }
    let words: Vec<&str> = name.split_whitespace().collect();
    for i in 1..words.len().saturating_sub(1) {
        if let Some(iod) = index.get(&words[i..].join(" ").to_lowercase()) {
            return Some(iod.to_string());
        }
    }
    None
}
#[derive(Clone, Debug, Default)]
pub(crate) struct Node {
    tag: Tag,
    opaque: bool,
    group: bool,
    typ: &'static str,
    condition: &'static str,
    children: Vec<Node>,
    constraints: Vec<ValueConstraint>,
    source: RuleSource,
}
fn tree(table: &str) -> Arc<Vec<Node>> {
    static TREES: OnceLock<Mutex<HashMap<String, Arc<Vec<Node>>>>> = OnceLock::new();
    let trees = TREES.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(t) = trees.lock().unwrap().get(table) {
        return t.clone();
    }
    let nodes = Arc::new(build_tree(table, &mut HashSet::new()));
    trees
        .lock()
        .unwrap()
        .insert(table.to_string(), nodes.clone());
    nodes
}
fn build_tree(table: &str, open: &mut HashSet<String>) -> Vec<Node> {
    let Some(rows) = modules::rows(table) else {
        return Vec::new();
    };
    if open.contains(table) {
        return Vec::new();
    }
    open.insert(table.to_string());
    fn build(
        rows: &[modules::Row],
        table: &str,
        open: &mut HashSet<String>,
        mut i: usize,
        depth: usize,
    ) -> (Vec<Node>, usize) {
        let mut out: Vec<Node> = Vec::new();
        while i < rows.len() && rows[i].depth as usize >= depth {
            let r = rows[i];
            let row_index = i;
            if r.depth as usize > depth {
                let (children, next) = build(rows, table, open, i, r.depth as usize);
                i = next;
                if let Some(last) = out.last_mut() {
                    last.children.extend(children);
                }
                continue;
            }
            i += 1;
            if !r.include.is_empty() {
                let source = modules::row_source(table, row_index as i32).unwrap_or_default();
                out.push(Node {
                    group: true,
                    condition: r.condition,
                    children: build_tree(r.include, open),
                    source,
                    ..Node::default()
                });
                continue;
            }
            let mut n = Node {
                tag: Tag::new((r.tag >> 16) as u16, r.tag as u16),
                opaque: r.tag == 0,
                typ: r.typ,
                condition: r.condition,
                constraints: modules::constraints(table, row_index as i32),
                source: modules::row_source(table, row_index as i32).unwrap_or_default(),
                ..Node::default()
            };
            if i < rows.len() && rows[i].depth as usize > depth {
                let (children, next) = build(rows, table, open, i, depth + 1);
                n.children = children;
                i = next;
            }
            out.push(n);
        }
        (out, i)
    }
    let (nodes, _) = build(&rows, table, open, 0, 0);
    open.remove(table);
    nodes
}
fn top_tags(nodes: &[Node], f: &mut dyn FnMut(Tag)) {
    for n in nodes {
        if n.group && n.condition.is_empty() {
            top_tags(&n.children, f);
        } else if !n.group && !n.opaque {
            f(n.tag);
        }
    }
}
pub(crate) struct Validator<'a> {
    root: &'a Object,
    report: Report,
    max: i64,
    seen: HashSet<String>,
    inferred: bool,
    check_values: bool,
    external: Option<&'a mut ExternalValidation>,
}
impl<'a> Validator<'a> {
    fn add(&mut self, kind: Kind, module: &str, path: &str, n: &Node) {
        let key = format!("{kind} {path}");
        if !self.seen.insert(key) {
            return;
        }
        if self.report.findings.len() as i64 >= self.max {
            self.report.truncated += 1;
            return;
        }
        let mut severity = kind_severity(kind);
        if self.inferred && severity == Severity::Error && kind != CONTRADICTORY_EXTERNAL_CONTEXT {
            severity = Severity::Warning;
        }
        let mut finding = Finding {
            kind,
            severity,
            module: module.to_string(),
            path: path.to_string(),
            tag: n.tag,
            typ: n.typ.to_string(),
            condition: n.condition.to_string(),
            rule: String::new(),
            source: None,
            message: String::new(),
        };
        if self.external.is_some() {
            finding.rule = n.source.rule.to_string();
            finding.source = Some(n.source.source);
            if kind == CONTRADICTORY_EXTERNAL_CONTEXT {
                finding.message = "Caller evidence contradicts the condition resolution".into();
            }
        }
        self.report.findings.push(finding);
    }
    fn check(&mut self, nodes: &[Node], item: &Object, prefix: &str, module: &str) {
        let root = self.root;
        for n in nodes {
            if n.group {
                if !n.condition.is_empty() {
                    if let Some(ext) = self.external.as_deref_mut() {
                        let (required, known, _, conflict) =
                            ext.evaluate(n.condition, &n.source, prefix, module, item, root);
                        if conflict {
                            self.add(CONTRADICTORY_EXTERNAL_CONTEXT, module, prefix, n);
                        }
                        if !known || !required {
                            continue;
                        }
                    } else {
                        match condition::parse_condition(n.condition) {
                            Some(c) if c.eval(item, root) => {}
                            _ => continue,
                        }
                    }
                }
                self.check(&n.children, item, prefix, module);
                continue;
            }
            if n.opaque {
                continue;
            }
            let path = format!("{prefix}{}", n.tag);
            let element = item.get(n.tag);
            let present = element.is_some();
            let empty = element.is_some_and(|e| empty_element(item, e));
            match n.typ {
                "1" => {
                    if !present {
                        self.add(MISSING_TYPE1, module, &path, n);
                    } else if empty {
                        self.add(EMPTY_TYPE1, module, &path, n);
                    }
                }
                "2" => {
                    if !present {
                        self.add(MISSING_TYPE2, module, &path, n);
                    }
                }
                "1C" | "2C" => {
                    let (required, ok, forbidden, conflict);
                    if let Some(ext) = self.external.as_deref_mut() {
                        (required, ok, forbidden, conflict) =
                            ext.evaluate(n.condition, &n.source, &path, module, item, root);
                    } else {
                        let (state, disallowed) =
                            priority::dataset_condition(n.condition, &n.source, item, root);
                        required = state == ConditionState::True;
                        ok = state != ConditionState::Unknown;
                        forbidden = disallowed;
                        conflict = false;
                    }
                    if conflict {
                        self.add(CONTRADICTORY_EXTERNAL_CONTEXT, module, &path, n);
                    }
                    if !ok {
                        if !present
                            || self.external.is_some()
                            || priority::has_priority(n.source.rule)
                        {
                            self.add(MANUAL_CONDITION, module, &path, n);
                        }
                    } else if required {
                        if !present {
                            let kind = if n.typ == "1C" {
                                MISSING_TYPE1C
                            } else {
                                MISSING_TYPE2C
                            };
                            self.add(kind, module, &path, n);
                        } else if empty && n.typ == "1C" {
                            self.add(EMPTY_TYPE1C, module, &path, n);
                        }
                    } else if present && forbidden {
                        self.add(UNEXPECTED_CONDITIONAL, module, &path, n);
                    }
                }
                _ => {}
            }
            if self.check_values && present && !empty {
                self.check_value_constraints(n, item, module, &path);
            }
            if present && !n.children.is_empty() {
                let items = item.get_sequence(n.tag).unwrap_or_default();
                for (i, child) in items.iter().enumerate() {
                    self.check(&n.children, child, &format!("{path}[{}]/", i + 1), module);
                }
            }
        }
    }
}
pub(crate) fn empty_element(obj: &Object, element: &Element) -> bool {
    if element.vr() == Vr::SQ {
        return obj
            .get_sequence(element.tag())
            .is_none_or(|items| items.is_empty());
    }
    element_length(element) == 0
}
pub fn sort_findings(findings: &mut [Finding]) {
    findings.sort_by(|a, b| a.path.cmp(&b.path).then(a.kind.cmp(b.kind)));
}
