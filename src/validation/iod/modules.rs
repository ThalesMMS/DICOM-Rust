use super::tables::{
    ATTR_TABLES, ATTRS, CONSTRAINTS, IODS, LIST, MODULE_SOURCES, PREDICATES, ROW_SOURCES,
    ROW_TABLES, ROWS, S, SOURCES, USES,
};
pub use super::tables::{SOURCE_SHA256, STANDARD_VERSION};
fn s(i: u32) -> &'static str {
    S[i as usize]
}
fn list(start: u32, len: u32) -> &'static [u32] {
    &LIST[start as usize..(start + len) as usize]
}
fn strings(start: u32, len: u32) -> Vec<&'static str> {
    list(start, len).iter().map(|&i| s(i)).collect()
}
fn find_table(tables: &'static [(u32, u32, u32)], name: &str) -> Option<&'static (u32, u32, u32)> {
    tables
        .binary_search_by(|t| s(t.0).cmp(name))
        .ok()
        .map(|i| &tables[i])
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Attribute {
    pub tag: u32,
    pub typ: &'static str,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModuleUsage {
    pub module: &'static str,
    pub usage: &'static str,
    pub condition: &'static str,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Row {
    pub depth: u8,
    pub tag: u32,
    pub typ: &'static str,
    pub include: &'static str,
    pub condition: &'static str,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ConstraintSource {
    pub table: &'static str,
    pub table_id: &'static str,
    pub row: i32,
    pub index: i32,
    pub depth: u8,
    pub tag: u32,
    pub section: &'static str,
    pub edition: &'static str,
    pub sha256: &'static str,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ValuePredicate {
    pub operator: &'static str,
    pub tag: u32,
    pub position: i32,
    pub values: Vec<&'static str>,
    pub children: Vec<ValuePredicate>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ValueConstraint {
    pub kind: &'static str,
    pub position: i32,
    pub values: Vec<&'static str>,
    pub predicate: ValuePredicate,
    pub count_tag: u32,
    pub parent_path: Vec<u32>,
    pub attribute_type: &'static str,
    pub rule: &'static str,
    pub reason: &'static str,
    pub source: ConstraintSource,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RuleSource {
    pub rule: &'static str,
    pub source: ConstraintSource,
    pub cross_references: Vec<&'static str>,
    pub external_family: &'static str,
}
fn source(i: u32) -> ConstraintSource {
    let (table, table_id, row, index, depth, tag, section, edition, sha) = SOURCES[i as usize];
    ConstraintSource {
        table: s(table),
        table_id: s(table_id),
        row,
        index,
        depth,
        tag,
        section: s(section),
        edition: s(edition),
        sha256: s(sha),
    }
}
pub fn rows(table: &str) -> Option<Vec<Row>> {
    let table = table.trim();
    let t = find_table(ROW_TABLES, table)
        .or_else(|| find_table(ROW_TABLES, &format!("{table} Module")))?;
    Some(
        ROWS[t.1 as usize..(t.1 + t.2) as usize]
            .iter()
            .map(|&(depth, tag, typ, include, condition)| Row {
                depth,
                tag,
                typ: s(typ),
                include: s(include),
                condition: s(condition),
            })
            .collect(),
    )
}
pub fn modules(iod: &str) -> Option<Vec<ModuleUsage>> {
    let t = find_table(IODS, iod)?;
    Some(
        USES[t.1 as usize..(t.1 + t.2) as usize]
            .iter()
            .map(|&(module, usage, condition)| ModuleUsage {
                module: s(module),
                usage: s(usage),
                condition: s(condition),
            })
            .collect(),
    )
}
pub fn attributes(module: &str) -> Option<Vec<Attribute>> {
    let key = module.trim().to_lowercase();
    let t = find_table(ATTR_TABLES, &format!("{key} module"))
        .or_else(|| find_table(ATTR_TABLES, &key))?;
    Some(
        ATTRS[t.1 as usize..(t.1 + t.2) as usize]
            .iter()
            .map(|&(tag, typ)| Attribute { tag, typ: s(typ) })
            .collect(),
    )
}
pub(crate) fn iods() -> Vec<&'static str> {
    IODS.iter().map(|t| s(t.0)).collect()
}
pub(crate) fn rule_tables() -> Vec<&'static str> {
    ROW_TABLES.iter().map(|t| s(t.0)).collect()
}
fn predicate(i: u32) -> ValuePredicate {
    let (op, tag, position, vs, vl, cs, cl) = PREDICATES[i as usize];
    ValuePredicate {
        operator: s(op),
        tag,
        position,
        values: strings(vs, vl),
        children: list(cs, cl).iter().map(|&c| predicate(c)).collect(),
    }
}
type ConstraintRow = (
    u32,
    i32,
    u32,
    i32,
    u32,
    u32,
    u32,
    u32,
    u32,
    u32,
    u32,
    u32,
    u32,
    u32,
);
fn constraint(row: &ConstraintRow) -> ValueConstraint {
    let &(_, _, kind, position, vs, vl, pred, count_tag, ps, pl, attr_type, rule, reason, src) =
        row;
    ValueConstraint {
        kind: s(kind),
        position,
        values: strings(vs, vl),
        predicate: predicate(pred),
        count_tag,
        parent_path: list(ps, pl).to_vec(),
        attribute_type: s(attr_type),
        rule: s(rule),
        reason: s(reason),
        source: source(src),
    }
}
pub fn constraints(table: &str, index: i32) -> Vec<ValueConstraint> {
    let start = CONSTRAINTS.partition_point(|c| (s(c.0), c.1) < (table, index));
    CONSTRAINTS[start..]
        .iter()
        .take_while(|c| s(c.0) == table && c.1 == index)
        .map(constraint)
        .collect()
}
pub(crate) fn constraint_tables() -> Vec<(&'static str, i32, Vec<ValueConstraint>)> {
    let mut out: Vec<(&'static str, i32, Vec<ValueConstraint>)> = Vec::new();
    for c in CONSTRAINTS {
        let table = s(c.0);
        match out.last_mut() {
            Some(last) if last.0 == table && last.1 == c.1 => last.2.push(constraint(c)),
            _ => out.push((table, c.1, vec![constraint(c)])),
        }
    }
    out
}
fn rule_source(
    rows: &'static [(u32, i32, u32, u32, u32, u32, u32)],
    key: &str,
    index: i32,
) -> Option<RuleSource> {
    let i = rows
        .binary_search_by(|r| (s(r.0), r.1).cmp(&(key, index)))
        .ok()?;
    let (_, _, rule, src, xs, xl, family) = rows[i];
    Some(RuleSource {
        rule: s(rule),
        source: source(src),
        cross_references: strings(xs, xl),
        external_family: s(family),
    })
}
pub(crate) fn row_source(table: &str, index: i32) -> Option<RuleSource> {
    rule_source(ROW_SOURCES, table, index)
}
pub(crate) fn module_source(iod: &str, index: i32) -> Option<RuleSource> {
    rule_source(MODULE_SOURCES, iod, index)
}
