use super::condition::parse_condition;
use super::modules;
use super::priority::has_priority;
use std::collections::HashMap;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ConditionCount {
    pub total: i64,
    pub evaluated: i64,
}
impl ConditionCount {
    pub fn fraction(&self) -> f64 {
        if self.total == 0 {
            return 0.0;
        }
        self.evaluated as f64 / self.total as f64
    }
    fn add(&mut self, evaluated: bool) {
        self.total += 1;
        if evaluated {
            self.evaluated += 1;
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConditionUse {
    pub text: String,
    pub rows: i64,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Coverage {
    pub attributes: ConditionCount,
    pub includes: ConditionCount,
    pub modules: ConditionCount,
    pub distinct: ConditionCount,
    pub type1c: ConditionCount,
    pub type2c: ConditionCount,
    pub unevaluated: Vec<ConditionUse>,
}
pub fn condition_coverage() -> Coverage {
    let mut coverage = Coverage::default();
    let mut uses: HashMap<String, i64> = HashMap::new();
    let mut note = |text: &str, rule: &str| -> bool {
        let supported = parse_condition(text).is_some() || has_priority(rule);
        let count = uses.entry(text.to_string()).or_insert(0);
        if !supported {
            *count += 1;
        }
        supported
    };
    for table in modules::rule_tables() {
        let rows = modules::rows(table).unwrap_or_default();
        for (index, row) in rows.iter().enumerate() {
            if row.condition.is_empty() {
                continue;
            }
            let source = modules::row_source(table, index as i32).unwrap_or_default();
            if !row.include.is_empty() {
                let s = note(row.condition, source.rule);
                coverage.includes.add(s);
            } else if row.typ == "1C" || row.typ == "2C" {
                let s = note(row.condition, source.rule);
                coverage.attributes.add(s);
                if row.typ == "1C" {
                    coverage.type1c.add(s);
                } else {
                    coverage.type2c.add(s);
                }
            }
        }
    }
    for name in modules::iods() {
        let uses_of = modules::modules(name).unwrap_or_default();
        for (index, u) in uses_of.iter().enumerate() {
            if u.usage == "C" && !u.condition.is_empty() {
                let source = modules::module_source(name, index as i32).unwrap_or_default();
                let s = note(u.condition, source.rule);
                coverage.modules.add(s);
            }
        }
    }
    for (text, count) in uses {
        coverage.distinct.add(count == 0);
        if count != 0 {
            coverage
                .unevaluated
                .push(ConditionUse { text, rows: count });
        }
    }
    coverage
        .unevaluated
        .sort_by(|a, b| b.rows.cmp(&a.rows).then_with(|| a.text.cmp(&b.text)));
    coverage
}
