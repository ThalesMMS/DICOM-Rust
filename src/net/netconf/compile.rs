use super::{
    Budget, Cause, Device, Options, ROLE_SCP, Result, active, field_error, normalized,
    validate_normalized,
};
use crate::net::ul::{
    AssociationProfile, CalledAeProfile, CalledAeRegistry, CalledAeRegistryOptions, Kind,
    PresentationContextRule, RequestorRoles, normalize_ae_title,
};
use std::collections::HashMap;
#[derive(Debug, Clone)]
pub struct CompiledConnection {
    address: String,
    tls_required: bool,
    registry: CalledAeRegistry,
}
impl CompiledConnection {
    pub fn address(&self) -> &str {
        &self.address
    }
    pub fn tls_required(&self) -> bool {
        self.tls_required
    }
    pub fn registry(&self) -> &CalledAeRegistry {
        &self.registry
    }
    pub fn ae_titles(&self) -> Vec<String> {
        self.registry.ae_titles()
    }
    pub fn profiles(&self) -> Vec<CalledAeProfile> {
        self.registry.profiles()
    }
}
fn join_host_port(host: &str, port: i64) -> String {
    if host.contains(':') || host.contains('%') {
        return format!("[{host}]:{port}");
    }
    format!("{host}:{port}")
}
pub(crate) fn registry_options(o: &Options) -> CalledAeRegistryOptions {
    CalledAeRegistryOptions {
        max_aes: o.limits.max_aes,
        max_rules: o.limits.max_capabilities,
        max_rules_per_ae: o.limits.max_capabilities_per_ae,
        max_transfer_syntaxes: o.limits.max_transfer_syntaxes,
        max_transfer_syntaxes_per_rule: o.limits.max_transfer_syntaxes_per_capability,
        max_compiled_bytes: o.limits.max_compiled_bytes,
    }
}
pub fn compile_connection(d: &Device, id: &str, options: Options) -> Result<CompiledConnection> {
    const FIELD: &str = "connections.selection";
    let bare = |cause| field_error(FIELD, cause);
    let o = normalized(options).map_err(bare)?;
    validate_normalized(d, &o)?;
    let id = if id.is_empty() {
        if d.connections.len() != 1 {
            return Err(bare(Cause::Invalid));
        }
        d.connections[0].id.as_str()
    } else {
        id
    };
    let selected = d.connections.iter().find(|c| c.id == id);
    let installed = active(d.installed, true);
    let Some(selected) = selected.filter(|c| installed && active(c.installed, installed)) else {
        return Err(bare(Cause::Invalid));
    };
    let chosen: Vec<_> = d
        .application_entities
        .iter()
        .filter(|a| {
            active(a.installed, installed)
                && a.association_acceptor
                && a.connection_ids.iter().any(|x| x == id)
        })
        .collect();
    let count = chosen.len() as i64;
    if count == 0 {
        return Err(bare(Cause::Invalid));
    }
    let mut backing = Budget::new(o.limits.max_compiled_bytes);
    backing.add(256 + 256 * count).map_err(bare)?;
    for a in &chosen {
        backing.add(a.ae_title.len() as i64).map_err(bare)?;
        for t in &a.transfer_capabilities {
            backing
                .add(
                    256 + 160
                        + 2 * t.sop_class_uid.len() as i64
                        + 96 * t.transfer_syntax_uids.len() as i64,
                )
                .map_err(bare)?;
            for ts in &t.transfer_syntax_uids {
                backing.add(ts.len() as i64).map_err(bare)?;
            }
        }
    }
    let mut profiles = Vec::with_capacity(chosen.len());
    for a in &chosen {
        let mut positions: HashMap<&str, usize> = HashMap::new();
        let mut rules: Vec<PresentationContextRule> = Vec::new();
        for t in &a.transfer_capabilities {
            let pos = match positions.get(t.sop_class_uid.as_str()) {
                Some(&pos) => {
                    if rules[pos].transfer_syntaxes != t.transfer_syntax_uids {
                        return Err(bare(Cause::Invalid));
                    }
                    pos
                }
                None => {
                    positions.insert(&t.sop_class_uid, rules.len());
                    rules.push(PresentationContextRule {
                        abstract_syntax_uid: t.sop_class_uid.clone(),
                        transfer_syntaxes: t.transfer_syntax_uids.clone(),
                        roles: Some(RequestorRoles::default()),
                    });
                    rules.len() - 1
                }
            };
            let roles = rules[pos].roles.get_or_insert_with(RequestorRoles::default);
            if t.role == ROLE_SCP {
                roles.scu = true;
            } else {
                roles.scp = true;
            }
        }
        profiles.push(CalledAeProfile {
            ae_title: normalize_ae_title(&a.ae_title).to_string(),
            profile: AssociationProfile {
                rules,
                ..AssociationProfile::default()
            },
        });
    }
    let registry = CalledAeRegistry::new(&profiles, registry_options(&o)).map_err(|err| {
        if err.is(Kind::CalledAeRegistryLimit) {
            bare(Cause::ResourceLimit)
        } else {
            bare(Cause::Invalid)
        }
    })?;
    Ok(CompiledConnection {
        address: join_host_port(&selected.hostname, selected.port),
        tls_required: selected.tls_required,
        registry,
    })
}
