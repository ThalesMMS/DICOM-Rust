use super::condition::parse_condition;
use super::modules::{self, ConstraintSource, ValueConstraint, ValuePredicate};
use super::{
    ADDITIONAL_DEFINED_TERM, INVALID_CONTEXT_MULTIPLICITY, INVALID_ENUMERATED_VALUE, Kind,
    MANUAL_VALUE_CONSTRAINT, Node, Validator, empty_element, kind_severity, tree,
};
use crate::numtext::format_float;
use crate::object::{DataSet, Element, ElementHeader, Object, Tag, Value, Vr};
use crate::validation::{Severity, ValidationOptions, validate_data_set_with};
use std::collections::HashSet;
pub const MAX_CONSTRAINT_VALUE_BYTES: i64 = 1 << 20;
impl Validator<'_> {
    fn add_value(
        &mut self,
        kind: Kind,
        module: &str,
        path: &str,
        n: &Node,
        c: &ValueConstraint,
        reason: &str,
    ) {
        let key = format!("{kind} {path} {}", c.rule);
        if !self.seen.insert(key) {
            return;
        }
        if self.report.findings.len() as i64 >= self.max {
            self.report.truncated += 1;
            return;
        }
        let mut severity = kind_severity(kind);
        if self.inferred && severity == Severity::Error {
            severity = Severity::Warning;
        }
        self.report.findings.push(super::Finding {
            kind,
            severity,
            module: module.to_string(),
            path: path.to_string(),
            tag: n.tag,
            typ: n.typ.to_string(),
            condition: n.condition.to_string(),
            rule: c.rule.to_string(),
            source: Some(c.source),
            message: reason.to_string(),
        });
    }
    pub(super) fn check_value_constraints(
        &mut self,
        n: &Node,
        item: &Object,
        module: &str,
        path: &str,
    ) {
        if n.constraints.is_empty() {
            return;
        }
        let decoded = constraint_values(item, n.tag);
        if let Some((values, _)) = &decoded
            && values.iter().all(|v| v.trim().is_empty())
        {
            return;
        }
        let root = self.root;
        for c in &n.constraints {
            if c.kind == "manual" {
                self.add_value(MANUAL_VALUE_CONSTRAINT, module, path, n, c, c.reason);
                continue;
            }
            let (active, known) = predicate_value(&c.predicate, item, root);
            if !known {
                self.add_value(
                    MANUAL_VALUE_CONSTRAINT,
                    module,
                    path,
                    n,
                    c,
                    "constraint predicate could not be evaluated",
                );
                continue;
            }
            if !active {
                continue;
            }
            let Some((values, vr)) = &decoded else {
                self.add_value(
                    MANUAL_VALUE_CONSTRAINT,
                    module,
                    path,
                    n,
                    c,
                    "value was not decoded for bounded constraint validation; encoding and dictionary VM remain separate",
                );
                continue;
            };
            if c.kind == "count" {
                let tag = Tag::new((c.count_tag >> 16) as u16, c.count_tag as u16);
                match root.get_sequence(tag) {
                    Some(seq) if !seq.is_empty() => {
                        if values.len() != seq.len() {
                            self.add_value(
                                INVALID_CONTEXT_MULTIPLICITY,
                                module,
                                path,
                                n,
                                c,
                                "value count does not match the root dimension sequence item count",
                            );
                        }
                    }
                    _ => self.add_value(
                        MANUAL_VALUE_CONSTRAINT,
                        module,
                        path,
                        n,
                        c,
                        "context sequence is absent, empty or not decoded",
                    ),
                }
                continue;
            }
            let mut wanted = HashSet::new();
            let mut valid = true;
            for term in &c.values {
                match canonical_value(*vr, term) {
                    Some(v) => {
                        wanted.insert(v);
                    }
                    None => {
                        valid = false;
                        break;
                    }
                }
            }
            if !valid {
                self.add_value(
                    MANUAL_VALUE_CONSTRAINT,
                    module,
                    path,
                    n,
                    c,
                    "literal normalization for this VR is not formalized",
                );
                continue;
            }
            let components: &[String] = if c.position > 0 {
                let p = c.position as usize;
                if p > values.len() {
                    continue;
                }
                &values[p - 1..p]
            } else {
                values
            };
            let mut invalid = false;
            for value in components {
                if value.trim().is_empty() {
                    invalid = true;
                    continue;
                }
                match canonical_value(*vr, value) {
                    None => {
                        valid = false;
                        break;
                    }
                    Some(v) => {
                        if !wanted.contains(&v) {
                            invalid = true;
                        }
                    }
                }
            }
            if !valid {
                self.add_value(
                    MANUAL_VALUE_CONSTRAINT,
                    module,
                    path,
                    n,
                    c,
                    "value normalization requires encoding validation",
                );
                continue;
            }
            if invalid {
                if c.kind == "defined" {
                    self.add_value(
                        ADDITIONAL_DEFINED_TERM,
                        module,
                        path,
                        n,
                        c,
                        "component is an additional defined term; the source list is extensible",
                    );
                } else {
                    self.add_value(
                        INVALID_ENUMERATED_VALUE,
                        module,
                        path,
                        n,
                        c,
                        "component is outside the scoped enumerated values",
                    );
                }
            }
        }
    }
    pub(super) fn check_functional_group_values(&mut self, module: &str) {
        let root = self.root;
        let macros = [
            value_only_tree("Frame Content Macro"),
            value_only_tree("Frame Usefulness Functional Group Macro"),
        ];
        for tag in [Tag::new(0x5200, 0x9229), Tag::new(0x5200, 0x9230)] {
            let items = root.get_sequence(tag).unwrap_or_default();
            for (index, item) in items.iter().enumerate() {
                for m in &macros {
                    self.check_fg_values(m, item, &format!("{tag}[{}]/", index + 1), module);
                }
            }
        }
    }
    fn check_fg_values(&mut self, nodes: &[Node], item: &Object, prefix: &str, module: &str) {
        let root = self.root;
        for n in nodes {
            if n.group {
                if !n.condition.is_empty() {
                    match parse_condition(n.condition) {
                        Some(c) if c.eval(item, root) => {}
                        _ => continue,
                    }
                }
                self.check_fg_values(&n.children, item, prefix, module);
                continue;
            }
            if n.opaque {
                continue;
            }
            let Some(element) = item.get(n.tag) else {
                continue;
            };
            let path = format!("{prefix}{}", n.tag);
            if !empty_element(item, element) {
                self.check_value_constraints(n, item, module, &path);
            }
            if !n.children.is_empty() {
                let items = item.get_sequence(n.tag).unwrap_or_default();
                for (index, child) in items.iter().enumerate() {
                    self.check_fg_values(
                        &n.children,
                        child,
                        &format!("{path}[{}]/", index + 1),
                        module,
                    );
                }
            }
        }
    }
}
fn value_only_tree(table: &str) -> Vec<Node> {
    let mut nodes: Vec<Node> = tree(table).as_ref().clone();
    for c in modules::constraints(table, -1) {
        if c.parent_path.len() != c.source.depth as usize {
            continue;
        }
        let mut level: &mut Vec<Node> = &mut nodes;
        let mut found = true;
        for &parent in &c.parent_path {
            let tag = Tag::new((parent >> 16) as u16, parent as u16);
            match level
                .iter()
                .position(|n| !n.group && !n.opaque && n.tag == tag)
            {
                Some(i) => level = &mut level[i].children,
                None => {
                    found = false;
                    break;
                }
            }
        }
        if !found {
            continue;
        }
        let tag = Tag::new((c.source.tag >> 16) as u16, c.source.tag as u16);
        match level.iter_mut().find(|n| !n.group && n.tag == tag) {
            Some(n) => n.constraints.push(c),
            None => level.push(Node {
                tag,
                typ: c.attribute_type,
                constraints: vec![c],
                ..Node::default()
            }),
        }
    }
    nodes
}
fn calculated_length(e: &Element) -> Option<i64> {
    match &e.value {
        Value::Raw(raw) => Some(raw.len() as i64),
        Value::Strings(values) => Some(crate::object::element_length(&Element {
            header: ElementHeader {
                length_set: false,
                ..e.header
            },
            value: Value::Strings(values.clone()),
        }) as i64),
        _ => None,
    }
}
fn constraint_values(obj: &Object, tag: Tag) -> Option<(Vec<String>, Vr)> {
    let element = obj.get(tag)?;
    let vr = element.vr();
    if vr == Vr::SQ || vr == Vr::UN {
        return None;
    }
    let length = calculated_length(element)?;
    if length == crate::object::UNDEFINED_LENGTH as i64 || length > MAX_CONSTRAINT_VALUE_BYTES {
        return None;
    }
    let mut ds = DataSet {
        elements: vec![element.clone()],
        item_offset: None,
    };
    if vr.uses_specific_character_set() {
        let charset = obj.character_set().ok()?;
        ds.elements.insert(
            0,
            Element {
                header: ElementHeader {
                    tag: Tag::new(0x0008, 0x0005),
                    vr: Vr::CS,
                    length: 0,
                    length_set: false,
                },
                value: Value::Strings(charset.names()),
            },
        );
    }
    let (result, err) = validate_data_set_with(
        &ds,
        &ValidationOptions {
            dictionary: true,
            big_endian: obj.value_big_endian(),
            max_findings: 32,
            ..ValidationOptions::default()
        },
    );
    if err.is_some() || result.report.has_errors() {
        return None;
    }
    if vr.is_string_like() {
        return obj.get_strings(tag).map(|v| (v, vr));
    }
    let values: Vec<String> = match vr {
        Vr::US | Vr::SS | Vr::UL | Vr::SL | Vr::UV | Vr::SV => obj
            .get_ints(tag)
            .ok()?
            .iter()
            .map(|n| n.to_string())
            .collect(),
        Vr::FL | Vr::FD => obj
            .get_floats(tag)
            .ok()?
            .iter()
            .map(|n| format_float(*n, 'g', -1))
            .collect(),
        Vr::AT => obj
            .get_tags(tag)?
            .iter()
            .map(|t| format!("{:04X}{:04X}H", t.group, t.element))
            .collect(),
        _ => return None,
    };
    (!values.is_empty()).then_some((values, vr))
}
fn canonical_value(vr: Vr, value: &str) -> Option<String> {
    let value = value.trim();
    match vr {
        Vr::DS | Vr::FL | Vr::FD => {
            let n = crate::numtext::parse_float(value).ok()?;
            Some(format_float(n, 'g', -1))
        }
        Vr::IS | Vr::US | Vr::SS | Vr::UL | Vr::SL | Vr::UV | Vr::SV => {
            if (vr == Vr::US || vr == Vr::UL)
                && let Some(hex) = value.strip_suffix('H')
            {
                return parse_hex_u32(hex).map(|n| n.to_string());
            }
            crate::numtext::parse_int(value).ok().map(|n| n.to_string())
        }
        Vr::AT => {
            let hex = value.strip_suffix('H').unwrap_or(value);
            if hex.len() != 8 {
                return None;
            }
            parse_hex_u32(hex).map(|n| format!("{n:08X}H"))
        }
        _ => vr.is_string_like().then(|| value.to_string()),
    }
}
fn parse_hex_u32(s: &str) -> Option<u32> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    u32::from_str_radix(s, 16).ok()
}
fn predicate_value(p: &ValuePredicate, item: &Object, root: &Object) -> (bool, bool) {
    match p.operator {
        "" => return (true, true),
        "and" | "or" => {
            let mut unknown = false;
            for child in &p.children {
                let (value, known) = predicate_value(child, item, root);
                if !known {
                    unknown = true;
                    continue;
                }
                if p.operator == "and" && !value {
                    return (false, true);
                }
                if p.operator == "or" && value {
                    return (true, true);
                }
            }
            if unknown {
                return (false, false);
            }
            return (p.operator == "and", true);
        }
        _ => {}
    }
    let tag = Tag::new((p.tag >> 16) as u16, p.tag as u16);
    let holder = if !item.has(tag) && !super::condition::same(root, item) && root.has(tag) {
        root
    } else {
        item
    };
    if p.operator == "present" {
        return (holder.has(tag), true);
    }
    if p.operator != "one-of" || !holder.has(tag) {
        return (false, false);
    }
    let Some((mut values, vr)) = constraint_values(holder, tag) else {
        return (false, false);
    };
    if p.position > 0 {
        let pos = p.position as usize;
        if pos > values.len() {
            return (false, false);
        }
        values = vec![values[pos - 1].clone()];
    }
    for want in &p.values {
        let Some(normalized) = canonical_value(vr, want) else {
            return (false, false);
        };
        for value in &values {
            let Some(got) = canonical_value(vr, value) else {
                return (false, false);
            };
            if got == normalized {
                return (true, true);
            }
        }
    }
    (false, true)
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValueRule {
    pub rule: String,
    pub reason: String,
    pub source: ConstraintSource,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ValueCoverage {
    pub compiled: i64,
    pub manual: Vec<ValueRule>,
}
pub fn value_constraint_coverage() -> ValueCoverage {
    let mut result = ValueCoverage::default();
    for (_, _, constraints) in modules::constraint_tables() {
        for c in constraints {
            if c.kind != "manual" {
                result.compiled += 1;
            } else {
                result.manual.push(ValueRule {
                    rule: c.rule.to_string(),
                    reason: c.reason.to_string(),
                    source: c.source,
                });
            }
        }
    }
    result
}
