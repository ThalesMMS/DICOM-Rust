use super::modules::{self, ConstraintSource};
use super::priority::{dataset_condition, has_priority};
use super::{Error, Kind, Options, Report, validate_with_external};
use crate::object::Object;
use std::collections::{HashMap, HashSet};
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ConditionState {
    #[default]
    Unknown,
    False,
    True,
}
impl ConditionState {
    pub(super) fn from_bool(required: bool) -> ConditionState {
        if required {
            ConditionState::True
        } else {
            ConditionState::False
        }
    }
    pub fn value(self) -> u8 {
        match self {
            ConditionState::Unknown => 0,
            ConditionState::False => 1,
            ConditionState::True => 2,
        }
    }
}
pub type ExternalFamily = &'static str;
pub const PERFORMED_PROCEDURE_STEP_PARTICIPATION: ExternalFamily =
    "performed-procedure-step-participation";
pub const CONTRIBUTING_SOP_INSTANCE_CONSISTENCY: ExternalFamily =
    "contributing-sop-instance-consistency";
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExternalFact {
    pub family: String,
    pub rule: String,
    pub path: String,
    pub state: u8,
    pub evidence_sha256: String,
}
pub const CONTRADICTORY_EXTERNAL_CONTEXT: Kind = "contradictory-external-context";
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RuleSource {
    pub rule: String,
    pub source: ConstraintSource,
    pub cross_references: Vec<String>,
    pub external_family: String,
}
fn public_rule_source(s: &modules::RuleSource) -> RuleSource {
    RuleSource {
        rule: s.rule.to_string(),
        source: s.source,
        cross_references: s.cross_references.iter().map(|x| x.to_string()).collect(),
        external_family: s.external_family.to_string(),
    }
}
#[derive(Clone, Debug, Default)]
pub struct ExternalContext {
    facts: HashMap<String, Vec<ExternalFact>>,
}
fn fact_key(family: &str, rule: &str, path: &str) -> String {
    format!("{family}\u{0}{rule}\u{0}{path}")
}
impl ExternalContext {
    pub fn new(facts: &[ExternalFact]) -> Result<ExternalContext, Error> {
        if facts.len() > 256 {
            return Err(Error::ExternalContext);
        }
        let mut c = ExternalContext::default();
        let mut total = 0;
        for f in facts {
            if f.family != PERFORMED_PROCEDURE_STEP_PARTICIPATION
                && f.family != CONTRIBUTING_SOP_INSTANCE_CONSISTENCY
            {
                return Err(Error::ExternalContext);
            }
            if f.state > 2
                || f.rule.is_empty()
                || f.rule.len() > 512
                || f.path.is_empty()
                || f.path.len() > 1024
                || f.evidence_sha256.len() != 64
            {
                return Err(Error::ExternalContext);
            }
            for text in [&f.rule, &f.path] {
                if text.chars().any(|c| !(' '..='~').contains(&c)) {
                    return Err(Error::ExternalContext);
                }
            }
            if !f
                .evidence_sha256
                .chars()
                .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
            {
                return Err(Error::ExternalContext);
            }
            total += f.rule.len() + f.path.len() + f.evidence_sha256.len() + f.family.len();
            if total > 65536 {
                return Err(Error::ExternalContext);
            }
            c.facts
                .entry(fact_key(&f.family, &f.rule, &f.path))
                .or_default()
                .push(f.clone());
        }
        Ok(c)
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ConditionCheck {
    pub rule: String,
    pub path: String,
    pub module: String,
    pub source: RuleSource,
    pub state: ConditionState,
    pub mode: &'static str,
    pub reason: &'static str,
    pub evidence_sha256: Vec<String>,
}
#[derive(Clone, Debug, Default)]
pub struct ContextReport {
    pub report: Report,
    pub validated: bool,
    pub conditions: Vec<ConditionCheck>,
    pub local: i64,
    pub external: i64,
    pub pending: i64,
    pub truncated: i64,
}
impl ContextReport {
    pub fn complete(&self) -> bool {
        self.validated
            && self.pending == 0
            && self.truncated == 0
            && self.report.truncated == 0
            && self.report.errors().is_empty()
    }
}
pub(crate) struct ExternalValidation {
    context: Option<ExternalContext>,
    checks: Vec<ConditionCheck>,
    seen: HashSet<String>,
    local: i64,
    external: i64,
    pending: i64,
    dropped: i64,
    max: i64,
}
pub fn validate_with_external_context(
    obj: &Object,
    opts: &Options,
    context: Option<&ExternalContext>,
    max_checks: i64,
) -> (ContextReport, Option<Error>) {
    if !(0..=65536).contains(&max_checks) {
        return (ContextReport::default(), Some(Error::ExternalContext));
    }
    let max = if max_checks == 0 { 10000 } else { max_checks };
    let mut v = ExternalValidation {
        context: context.cloned(),
        checks: Vec::new(),
        seen: HashSet::new(),
        local: 0,
        external: 0,
        pending: 0,
        dropped: 0,
        max,
    };
    let (report, err) = validate_with_external(obj, opts, Some(&mut v));
    (
        ContextReport {
            report,
            validated: err.is_none(),
            conditions: v.checks,
            local: v.local,
            external: v.external,
            pending: v.pending,
            truncated: v.dropped,
        },
        err,
    )
}
impl ExternalValidation {
    fn record(&mut self, check: ConditionCheck) {
        let key = format!("{}\u{0}{}\u{0}{}", check.rule, check.path, check.module);
        if self.seen.contains(&key) {
            return;
        }
        if self.checks.len() as i64 >= self.max {
            self.dropped += 1;
            return;
        }
        self.seen.insert(key);
        match check.mode {
            "local" => self.local += 1,
            "external" => self.external += 1,
            _ => self.pending += 1,
        }
        self.checks.push(check);
    }
    pub(super) fn evaluate(
        &mut self,
        text: &str,
        source: &modules::RuleSource,
        path: &str,
        module: &str,
        item: &Object,
        root: &Object,
    ) -> (bool, bool, bool, bool) {
        let mut check = ConditionCheck {
            rule: source.rule.to_string(),
            path: path.to_string(),
            module: module.to_string(),
            source: public_rule_source(source),
            state: ConditionState::Unknown,
            mode: "pending",
            reason: "unsupported-context-family",
            evidence_sha256: Vec::new(),
        };
        if check.rule.is_empty() {
            check.rule = "unbound-condition".into();
            check.reason = "missing-normative-source";
        }
        let (state, forbidden) = dataset_condition(text, source, item, root);
        if state != ConditionState::Unknown {
            check.state = state;
            check.mode = "local";
            check.reason = "dataset-condition";
            self.record(check);
            return (state == ConditionState::True, true, forbidden, false);
        }
        if has_priority(source.rule) {
            check.reason = "indeterminate-dataset-condition";
            self.record(check);
            return (false, false, false, false);
        }
        let family = source.external_family;
        if family == PERFORMED_PROCEDURE_STEP_PARTICIPATION
            || family == CONTRIBUTING_SOP_INSTANCE_CONSISTENCY
        {
            check.reason = "missing-external-context";
            if let Some(context) = &self.context {
                let facts = context
                    .facts
                    .get(&fact_key(family, source.rule, path))
                    .cloned()
                    .unwrap_or_default();
                let mut known = 0u8;
                let mut conflict = false;
                for f in &facts {
                    check.evidence_sha256.push(f.evidence_sha256.clone());
                    if f.state != 0 {
                        if known != 0 && known != f.state {
                            conflict = true;
                        }
                        known = f.state;
                    }
                }
                if conflict {
                    check.reason = "contradictory-external-context";
                    self.record(check);
                    return (false, false, false, true);
                }
                if known != 0 {
                    check.state = if known == 2 {
                        ConditionState::True
                    } else {
                        ConditionState::False
                    };
                    check.mode = "external";
                    check.reason = "caller-resolved-condition";
                    self.record(check);
                    return (known == 2, true, false, false);
                }
            }
        }
        self.record(check);
        (false, false, false, false)
    }
}
pub fn external_condition_coverage() -> Vec<(String, i64)> {
    let mut counts: HashMap<String, i64> = HashMap::new();
    for table in modules::rule_tables() {
        let rows = modules::rows(table).unwrap_or_default();
        for index in 0..rows.len() {
            if let Some(source) = modules::row_source(table, index as i32)
                && !source.external_family.trim().is_empty()
            {
                *counts
                    .entry(source.external_family.to_string())
                    .or_default() += 1;
            }
        }
    }
    let mut out: Vec<(String, i64)> = counts.into_iter().collect();
    out.sort();
    out
}
