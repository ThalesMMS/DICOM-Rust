#![forbid(unsafe_code)]
use super::aetitle::{normalize_ae_title, validate_ae_title};
use super::error::{Error, Kind, Result};
use super::negotiation::AcceptOptions;
use super::pdu::RoleSelectionItem;
use crate::textfmt::quote;
use std::collections::HashSet;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RequestorRoles {
    pub scu: bool,
    pub scp: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PresentationContextRule {
    pub abstract_syntax_uid: String,
    pub transfer_syntaxes: Vec<String>,
    pub roles: Option<RequestorRoles>,
}
pub(crate) fn validate_presentation_context_rules(rules: &[PresentationContextRule]) -> Result<()> {
    let mut seen = HashSet::new();
    for rule in rules {
        let uid = rule.abstract_syntax_uid.trim();
        if uid.is_empty() {
            return Err(Error::wrap(
                Kind::InvalidPresentationContextRule,
                "empty abstract syntax",
            ));
        }
        if !seen.insert(uid.to_string()) {
            return Err(Error::wrap(
                Kind::InvalidPresentationContextRule,
                format!("duplicate abstract syntax {}", quote(uid)),
            ));
        }
        if let Some(roles) = rule.roles
            && !roles.scu
            && !roles.scp
        {
            return Err(Error::wrap(
                Kind::InvalidPresentationContextRule,
                format!("{} allows no role", quote(uid)),
            ));
        }
    }
    Ok(())
}
pub(crate) fn validate_called_ae_presentation_context_rules(
    by_title: &[(String, Vec<PresentationContextRule>)],
) -> Result<()> {
    let mut seen = HashSet::new();
    for (key, rules) in by_title {
        let title = normalize_ae_title(key);
        if let Err(e) = validate_ae_title(title) {
            return Err(Error::wrap_err(
                Kind::InvalidPresentationContextRule,
                e.prefixed("called AE"),
            ));
        }
        if !seen.insert(title.to_string()) {
            return Err(Error::wrap(
                Kind::InvalidPresentationContextRule,
                format!("duplicate called AE {}", quote(title)),
            ));
        }
        validate_presentation_context_rules(rules)
            .map_err(|e| e.prefixed(format!("called AE {}", quote(title))))?;
    }
    Ok(())
}
pub(crate) fn called_ae_presentation_context_rules<'a>(
    by_title: &'a [(String, Vec<PresentationContextRule>)],
    called_ae: &str,
) -> Option<&'a [PresentationContextRule]> {
    if by_title.is_empty() {
        return None;
    }
    if let Some((_, rules)) = by_title.iter().find(|(k, _)| k == called_ae) {
        return Some(rules);
    }
    let title = normalize_ae_title(called_ae);
    by_title
        .iter()
        .find(|(k, _)| normalize_ae_title(k) == title)
        .map(|(_, rules)| rules.as_slice())
}
pub(crate) fn presentation_context_rule<'a>(
    opts: &'a AcceptOptions,
    abstract_syntax_uid: &str,
) -> Option<&'a PresentationContextRule> {
    opts.presentation_context_rules
        .iter()
        .find(|rule| rule.abstract_syntax_uid.trim() == abstract_syntax_uid)
}
pub(crate) fn rule_role_accepts(
    policy: RequestorRoles,
    proposed: Option<&RoleSelectionItem>,
    always_accept_default: bool,
) -> bool {
    match proposed {
        None => policy.scu || always_accept_default,
        Some(p) => (p.scu_role && policy.scu) || (p.scp_role && policy.scp),
    }
}
pub(crate) fn requested_role_for<'a>(
    requested: &'a [RoleSelectionItem],
    sop_class_uid: &str,
) -> Option<&'a RoleSelectionItem> {
    requested.iter().find(|r| r.sop_class_uid == sop_class_uid)
}
