#![forbid(unsafe_code)]
use super::aetitle::{normalize_ae_title, validate_ae_title};
use super::error::{Error, Kind, Result};
use super::negotiation::AcceptOptions;
use super::pdu::{PresentationContext, RoleSelectionItem, SopClassExtendedNegotiationItem};
use super::rules::PresentationContextRule;
use std::collections::{HashMap, HashSet};
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AssociationProfile {
    pub contexts: Vec<PresentationContext>,
    pub rules: Vec<PresentationContextRule>,
    pub role_selections: Vec<RoleSelectionItem>,
    pub extended_negotiation: Vec<SopClassExtendedNegotiationItem>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CalledAeProfile {
    pub ae_title: String,
    pub profile: AssociationProfile,
}
#[derive(Debug, Clone, Copy, Default)]
pub struct CalledAeRegistryOptions {
    pub max_aes: i64,
    pub max_rules: i64,
    pub max_rules_per_ae: i64,
    pub max_transfer_syntaxes: i64,
    pub max_transfer_syntaxes_per_rule: i64,
    pub max_compiled_bytes: i64,
}
#[derive(Debug, Clone)]
pub struct CalledAeRegistry {
    entries: Vec<CalledAeProfile>,
    by_title: HashMap<String, usize>,
}
pub(crate) fn is_valid_uid(uid: &str) -> bool {
    let b = uid.as_bytes();
    if b.len() < 3 || b.len() > 64 {
        return false;
    }
    let mut component_index = 0;
    let mut start = 0;
    let mut root = 0u8;
    for index in 0..=b.len() {
        if index < b.len() && b[index] != b'.' {
            if !b[index].is_ascii_digit() {
                return false;
            }
            continue;
        }
        let len = index - start;
        if len == 0 || (len > 1 && b[start] == b'0') {
            return false;
        }
        match component_index {
            0 => {
                if len != 1 || b[start] > b'2' {
                    return false;
                }
                root = b[start];
            }
            1 if root != b'2' => {
                let mut arc = (b[start] - b'0') as i32;
                if len == 2 {
                    arc = arc * 10 + (b[start + 1] - b'0') as i32;
                }
                if len > 2 || arc > 39 {
                    return false;
                }
            }
            _ => {}
        }
        component_index += 1;
        start = index + 1;
    }
    component_index >= 2
}
const MAX_LIMIT: i64 = i64::MAX / 1024;
fn registry_options(o: CalledAeRegistryOptions) -> Result<CalledAeRegistryOptions> {
    let mut o = o;
    let defaults = [64, 4096, 1024, 16384, 256, 8 << 20];
    let fields = [
        &mut o.max_aes,
        &mut o.max_rules,
        &mut o.max_rules_per_ae,
        &mut o.max_transfer_syntaxes,
        &mut o.max_transfer_syntaxes_per_rule,
        &mut o.max_compiled_bytes,
    ];
    for (v, d) in fields.into_iter().zip(defaults) {
        if *v < 0 || *v > MAX_LIMIT {
            return Err(Error::sentinel(Kind::CalledAeRegistryLimit));
        }
        if *v == 0 {
            *v = d;
        }
    }
    Ok(o)
}
fn registry_roles_agree(p: &AssociationProfile) -> bool {
    if p.role_selections.len() > p.rules.len() {
        return false;
    }
    for (i, role) in p.role_selections.iter().enumerate() {
        if !is_valid_uid(&role.sop_class_uid) || (!role.scu_role && !role.scp_role) {
            return false;
        }
        if p.role_selections[..i]
            .iter()
            .any(|r| r.sop_class_uid == role.sop_class_uid)
        {
            return false;
        }
        match p
            .rules
            .iter()
            .find(|r| r.abstract_syntax_uid == role.sop_class_uid)
        {
            Some(rule) => match rule.roles {
                Some(r) if r.scu == role.scu_role && r.scp == role.scp_role => {}
                _ => return false,
            },
            None => return false,
        }
    }
    true
}
impl CalledAeRegistry {
    pub fn new(
        input: &[CalledAeProfile],
        options: CalledAeRegistryOptions,
    ) -> Result<CalledAeRegistry> {
        let o = registry_options(options)?;
        let limit = || Error::sentinel(Kind::CalledAeRegistryLimit);
        let invalid = || Error::sentinel(Kind::CalledAeRegistry);
        if input.is_empty() {
            return Err(invalid());
        }
        if input.len() as i64 > o.max_aes {
            return Err(limit());
        }
        let (mut used, mut rules, mut syntax) = (0i64, 0i64, 0i64);
        let mut add = |n: i64| -> Result<()> {
            if n < 0 || n > o.max_compiled_bytes - used {
                return Err(Error::sentinel(Kind::CalledAeRegistryLimit));
            }
            used += n;
            Ok(())
        };
        for e in input {
            add(256 + e.ae_title.len() as i64)?;
            let n = e.profile.rules.len() as i64;
            if n > o.max_rules_per_ae || n > o.max_rules - rules {
                return Err(limit());
            }
            rules += n;
            if !e.profile.contexts.is_empty()
                || !e.profile.extended_negotiation.is_empty()
                || e.profile.rules.is_empty()
                || validate_ae_title(normalize_ae_title(&e.ae_title)).is_err()
            {
                return Err(invalid());
            }
            if !registry_roles_agree(&e.profile) {
                return Err(invalid());
            }
            for r in &e.profile.rules {
                let t = r.transfer_syntaxes.len() as i64;
                if t > o.max_transfer_syntaxes_per_rule || t > o.max_transfer_syntaxes - syntax {
                    return Err(limit());
                }
                syntax += t;
                add(160 + r.abstract_syntax_uid.len() as i64 + 80 * t)?;
                for ts in &r.transfer_syntaxes {
                    add(ts.len() as i64)?;
                }
            }
        }
        let mut registry = CalledAeRegistry {
            entries: Vec::with_capacity(input.len()),
            by_title: HashMap::new(),
        };
        for (i, e) in input.iter().enumerate() {
            let title = normalize_ae_title(&e.ae_title).to_string();
            if registry.by_title.contains_key(&title) {
                return Err(invalid());
            }
            let mut seen = HashSet::new();
            for r in &e.profile.rules {
                if !is_valid_uid(&r.abstract_syntax_uid)
                    || !seen.insert(r.abstract_syntax_uid.as_str())
                    || r.transfer_syntaxes.is_empty()
                    || r.roles.is_some_and(|roles| !roles.scu && !roles.scp)
                {
                    return Err(invalid());
                }
                let mut ts_seen = HashSet::new();
                for ts in &r.transfer_syntaxes {
                    if !is_valid_uid(ts) || !ts_seen.insert(ts.as_str()) {
                        return Err(invalid());
                    }
                }
            }
            registry.entries.push(CalledAeProfile {
                ae_title: title.clone(),
                profile: AssociationProfile {
                    rules: e.profile.rules.clone(),
                    ..Default::default()
                },
            });
            registry.by_title.insert(title, i);
        }
        Ok(registry)
    }
    pub fn ae_titles(&self) -> Vec<String> {
        self.entries.iter().map(|e| e.ae_title.clone()).collect()
    }
    pub fn profiles(&self) -> Vec<CalledAeProfile> {
        self.entries.clone()
    }
    pub(crate) fn lookup(&self, title: &str) -> Option<&CalledAeProfile> {
        self.by_title
            .get(normalize_ae_title(title))
            .map(|&i| &self.entries[i])
    }
    pub(crate) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}
pub(crate) fn validate_called_ae_options(o: &AcceptOptions) -> Result<()> {
    let Some(registry) = &o.called_ae_registry else {
        return Ok(());
    };
    if registry.is_empty()
        || !o.supported_abstract_syntaxes.is_empty()
        || !o.supported_transfer_syntaxes.is_empty()
        || !o.presentation_context_rules.is_empty()
        || !o.role_selections.is_empty()
        || o.accept_any_abstract_syntax
        || o.always_accept_default_role
        || o.common_extended_negotiation_handler.is_some()
        || o.require_matching_called_ae
        || !o.called_ae_presentation_context_rules.is_empty()
    {
        return Err(Error::sentinel(Kind::CalledAeRegistry));
    }
    Ok(())
}
