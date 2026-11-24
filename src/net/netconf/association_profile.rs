use super::compile::registry_options;
use super::{
    ApplicationEntity, Budget, Cause, Options, ROLE_SCP, ROLE_SCU, Result, TransferCapability,
    field_error, normalized, text_ok,
};
use crate::net::ul::{AssociationProfile, CalledAeProfile, CalledAeRegistry, Kind};
pub fn from_association_profile(
    title: &str,
    connection_ids: &[String],
    p: &AssociationProfile,
    options: Options,
) -> Result<ApplicationEntity> {
    const FIELD: &str = "associationProfile";
    let bare = |cause| field_error(FIELD, cause);
    let invalid = || Err(bare(Cause::Invalid));
    let limit = || Err(bare(Cause::ResourceLimit));
    let o = normalized(options).map_err(bare)?;
    let l = &o.limits;
    if !p.contexts.is_empty()
        || !p.extended_negotiation.is_empty()
        || p.rules.is_empty()
        || connection_ids.is_empty()
        || connection_ids.len() as i64 > l.max_connections
    {
        return invalid();
    }
    let rule_count = p.rules.len() as i64;
    if rule_count > l.max_capabilities_per_ae || rule_count > l.max_capabilities {
        return limit();
    }
    if p.role_selections.len() > p.rules.len() {
        return invalid();
    }
    let mut expanded = Budget::new(l.max_compiled_bytes);
    expanded
        .add(256 + title.len() as i64 + 16 * connection_ids.len() as i64)
        .map_err(bare)?;
    for (i, id) in connection_ids.iter().enumerate() {
        if !text_ok(id, l.max_name_bytes, false) || id.trim() != id {
            return invalid();
        }
        if connection_ids[..i].contains(id) {
            return invalid();
        }
        expanded.add(id.len() as i64).map_err(bare)?;
    }
    let (mut cap_count, mut ts_count) = (0i64, 0i64);
    for rule in &p.rules {
        let copies = if rule.roles.is_some_and(|r| r.scu && r.scp) {
            2
        } else {
            1
        };
        let syntaxes = rule.transfer_syntaxes.len() as i64;
        if copies > l.max_capabilities_per_ae - cap_count
            || copies > l.max_capabilities - cap_count
            || syntaxes > l.max_transfer_syntaxes_per_capability
        {
            return limit();
        }
        cap_count += copies;
        for _ in 0..copies {
            if syntaxes > l.max_transfer_syntaxes - ts_count {
                return limit();
            }
            ts_count += syntaxes;
            expanded
                .add(160 + rule.abstract_syntax_uid.len() as i64 + 24 * syntaxes)
                .map_err(bare)?;
            for ts in &rule.transfer_syntaxes {
                expanded.add(ts.len() as i64).map_err(bare)?;
            }
        }
    }
    let mut used = vec![false; p.role_selections.len()];
    for (i, role) in p.role_selections.iter().enumerate() {
        if p.role_selections[..i]
            .iter()
            .any(|earlier| earlier.sop_class_uid == role.sop_class_uid)
        {
            return invalid();
        }
    }
    for r in &p.rules {
        for (i, role) in p.role_selections.iter().enumerate() {
            if role.sop_class_uid == r.abstract_syntax_uid {
                let agrees = r
                    .roles
                    .is_some_and(|roles| roles.scu == role.scu_role && roles.scp == role.scp_role);
                if used[i] || !agrees {
                    return invalid();
                }
                used[i] = true;
            }
        }
    }
    if used.iter().any(|&v| !v) {
        return invalid();
    }
    let mut detached = p.clone();
    detached.role_selections = Vec::new();
    let registry = CalledAeRegistry::new(
        &[CalledAeProfile {
            ae_title: title.to_string(),
            profile: detached,
        }],
        registry_options(&o),
    )
    .map_err(|err| {
        if err.is(Kind::CalledAeRegistryLimit) {
            bare(Cause::ResourceLimit)
        } else {
            bare(Cause::Invalid)
        }
    })?;
    let profile = registry
        .profiles()
        .into_iter()
        .next()
        .ok_or_else(|| bare(Cause::Invalid))?;
    let mut count = 0i64;
    for r in &profile.profile.rules {
        match r.roles {
            None => count += 1,
            Some(roles) => count += roles.scu as i64 + roles.scp as i64,
        }
    }
    if count > l.max_capabilities_per_ae || count > l.max_capabilities {
        return limit();
    }
    let mut capabilities = Vec::with_capacity(count as usize);
    for r in &profile.profile.rules {
        if r.roles.is_none_or(|roles| roles.scu) {
            capabilities.push(TransferCapability {
                sop_class_uid: r.abstract_syntax_uid.clone(),
                role: ROLE_SCP.to_string(),
                transfer_syntax_uids: r.transfer_syntaxes.clone(),
            });
        }
        if r.roles.is_some_and(|roles| roles.scp) {
            capabilities.push(TransferCapability {
                sop_class_uid: r.abstract_syntax_uid.clone(),
                role: ROLE_SCU.to_string(),
                transfer_syntax_uids: r.transfer_syntaxes.clone(),
            });
        }
    }
    Ok(ApplicationEntity {
        ae_title: profile.ae_title,
        connection_ids: connection_ids.to_vec(),
        association_acceptor: true,
        transfer_capabilities: capabilities,
        ..ApplicationEntity::default()
    })
}
