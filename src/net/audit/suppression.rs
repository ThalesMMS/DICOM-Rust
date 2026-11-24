#![forbid(unsafe_code)]
use super::{Error, ErrorKind, Message, Result};
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SuppressionRule {
    pub event_id: String,
    pub action: String,
    pub outcome: String,
}
pub const MAX_SUPPRESSION_RULES: usize = 64;
pub fn validate_suppression(rules: &[SuppressionRule]) -> Result<()> {
    let bad = |detail: &str| Err(Error::wrap(ErrorKind::SyslogConfig, detail));
    if rules.len() > MAX_SUPPRESSION_RULES {
        return bad("suppression limit");
    }
    for r in rules {
        if r.event_id.is_empty() && r.action.is_empty() && r.outcome.is_empty() {
            return bad("empty suppression rule");
        }
        if r.event_id.len() > 32 || r.action.len() > 1 || r.outcome.len() > 8 {
            return bad("suppression field limit");
        }
        if !matches!(r.action.as_str(), "" | "C" | "R" | "U" | "D" | "E") {
            return bad("suppression action");
        }
        if !matches!(r.outcome.as_str(), "" | "0" | "4" | "8" | "12") {
            return bad("suppression outcome");
        }
    }
    Ok(())
}
pub(crate) fn suppressed(rules: &[SuppressionRule], m: &Message) -> bool {
    rules.iter().any(|r| {
        (r.event_id.is_empty() || r.event_id == m.event.id.value)
            && (r.action.is_empty() || r.action == m.event.action_code)
            && (r.outcome.is_empty() || r.outcome == m.event.outcome_indicator)
    })
}
