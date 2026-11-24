use super::dn::{Dn, dn_key, equal_fold, lower_text, parent_key, parse_dn};
use super::{Device, Diagnostic, Error, Options};
use crate::net::netconf::{
    self, ApplicationEntity, Connection, Device as Model, TransferCapability,
};
use crate::net::ul::{Ctx, Kind, normalize_ae_title};
use crate::textfmt::to_upper;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Attribute {
    pub name: Vec<u8>,
    pub values: Vec<Vec<u8>>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Entry {
    pub dn: Vec<u8>,
    pub attributes: Vec<Attribute>,
}
impl Entry {
    pub fn values(&self, name: &str) -> &[Vec<u8>] {
        self.attributes
            .iter()
            .find(|a| equal_fold(name.as_bytes(), &a.name))
            .map_or(&[], |a| a.values.as_slice())
    }
}
pub(crate) fn ctx_error(ctx: &Ctx) -> Option<Error> {
    ctx.err().map(|k| match k {
        Kind::Canceled => Error::Canceled,
        _ => Error::DeadlineExceeded,
    })
}
fn hex_sha256(v: &[u8]) -> String {
    Sha256::digest(v)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
pub(crate) fn admit_entry(entry: &Entry, root: &Dn, o: &Options) -> Result<i64, Error> {
    if entry.dn.len() > 4096
        || std::str::from_utf8(&entry.dn).is_err()
        || entry.attributes.len() > 64
    {
        return Err(Error::Limit);
    }
    let dn = parse_dn(&entry.dn).map_err(|_| Error::Invalid)?;
    if !root.equal(&dn) && !root.ancestor_of(&dn) {
        return Err(Error::Invalid);
    }
    if (dn.rdns.len() - root.rdns.len()) as i64 > o.max_depth {
        return Err(Error::Limit);
    }
    let mut weight = 256 + 8 * entry.dn.len() as i64 + 64 * entry.attributes.len() as i64;
    let mut seen = HashSet::new();
    for attr in &entry.attributes {
        if attr.name.len() > 128 || attr.values.len() > 512 {
            return Err(Error::Limit);
        }
        if !seen.insert(lower_text(&attr.name)) {
            return Err(Error::Invalid);
        }
        for value in &attr.values {
            if value.len() > 8192 {
                return Err(Error::Limit);
            }
            weight += 64 + 2 * value.len() as i64;
            if weight > o.max_entry_bytes || weight > o.max_model_bytes {
                return Err(Error::Limit);
            }
        }
    }
    Ok(weight)
}
pub(crate) fn single(entry: &Entry, name: &str, required: bool) -> Result<String, Error> {
    let values = entry.values(name);
    if values.len() > 1 || (required && values.first().is_none_or(|v| v.is_empty())) {
        return Err(Error::Invalid);
    }
    match values.first() {
        None => Ok(String::new()),
        Some(v) => String::from_utf8(v.clone()).map_err(|_| Error::Invalid),
    }
}
fn boolean(entry: &Entry, name: &str, required: bool) -> Result<Option<bool>, Error> {
    let value = single(entry, name, required)?;
    if value.is_empty() {
        return Ok(None);
    }
    match to_upper(&value).as_str() {
        "TRUE" => Ok(Some(true)),
        "FALSE" => Ok(Some(false)),
        _ => Err(Error::Invalid),
    }
}
const CLASSES: [&str; 4] = [
    "dicomDevice",
    "dicomNetworkConnection",
    "dicomNetworkAE",
    "dicomTransferCapability",
];
fn object_class(entry: &Entry) -> Result<&'static str, Error> {
    let mut result = "";
    for value in entry.values("objectClass") {
        for known in CLASSES {
            if equal_fold(value, known.as_bytes()) {
                if !result.is_empty() {
                    return Err(Error::Invalid);
                }
                result = known;
            }
        }
    }
    if result.is_empty() {
        return Err(Error::Invalid);
    }
    Ok(result)
}
fn numeric_port(value: &str) -> Result<i64, Error> {
    if value.is_empty() {
        return Ok(0);
    }
    match crate::numtext::parse_int(value) {
        Ok(n) if (1..=65535).contains(&n) => Ok(n),
        _ => Err(Error::Invalid),
    }
}
fn key(v: &[u8]) -> Result<String, Error> {
    dn_key(v).map_err(|_| Error::Invalid)
}
fn lossy(v: &[u8]) -> String {
    crate::textfmt::runes(v).into_iter().collect()
}
pub(crate) fn map_device(
    ctx: &Ctx,
    source: &Entry,
    children: &[Entry],
    o: &Options,
) -> Result<Device, Error> {
    if object_class(source) != Ok("dicomDevice") {
        return Err(Error::Invalid);
    }
    let device_key = key(&source.dn)?;
    let mut result = Device {
        id: hex_sha256(device_key.as_bytes()),
        dn: lossy(&source.dn),
        model: Model {
            schema_version: 1,
            ..Model::default()
        },
        diagnostics: Vec::new(),
    };
    result.model.device_name = single(source, "dicomDeviceName", true)?;
    result.model.description = single(source, "dicomDescription", false)?;
    result.model.installed = boolean(source, "dicomInstalled", true)?;
    let mut connections: HashMap<String, String> = HashMap::new();
    let mut seen = HashSet::new();
    let mut aes = Vec::new();
    let mut capabilities = Vec::new();
    for child in children {
        if let Some(e) = ctx_error(ctx) {
            return Err(e);
        }
        let child_key = dn_key(&child.dn).map_err(|_| Error::Invalid)?;
        if !seen.insert(child_key.clone()) {
            return Err(Error::Invalid);
        }
        let class = object_class(child)?;
        let parent = parent_key(&child.dn).map_err(|_| Error::Invalid)?;
        match class {
            "dicomNetworkConnection" => {
                if parent != device_key {
                    return Err(Error::Invalid);
                }
                let mut connection = Connection {
                    id: format!("ldap-{}", hex_sha256(child_key.as_bytes())),
                    ..Connection::default()
                };
                connections.insert(child_key, connection.id.clone());
                connection.hostname = single(child, "dicomHostname", true)?;
                let port = single(child, "dicomPort", false)?;
                connection.port = numeric_port(&port)?;
                if port.is_empty() {
                    result.diagnostics.push(Diagnostic {
                        code: "port-absent-initiator-only".into(),
                        object: connection.id.clone(),
                    });
                }
                connection.installed = boolean(child, "dicomInstalled", false)?;
                connection.tls_required = !child.values("dicomTLSCipherSuite").is_empty();
                result.model.connections.push(connection);
            }
            "dicomNetworkAE" => {
                if parent != device_key {
                    return Err(Error::Invalid);
                }
                aes.push(child);
            }
            "dicomTransferCapability" => capabilities.push(child),
            _ => return Err(Error::Invalid),
        }
    }
    if aes.is_empty() && capabilities.is_empty() {
        result.model.schema_version = 0;
        result.model.connections = Vec::new();
        return Ok(result);
    }
    let known: HashSet<String> = aes.iter().filter_map(|e| dn_key(&e.dn).ok()).collect();
    for entry in &capabilities {
        match parent_key(&entry.dn) {
            Ok(parent) if known.contains(&parent) => {}
            _ => return Err(Error::Invalid),
        }
    }
    for entry in &aes {
        if let Some(e) = ctx_error(ctx) {
            return Err(e);
        }
        let title = single(entry, "dicomAETitle", true)?;
        let mut ae = ApplicationEntity {
            ae_title: normalize_ae_title(&title).to_string(),
            ..ApplicationEntity::default()
        };
        for reference in entry.values("dicomNetworkConnectionReference") {
            match dn_key(reference).ok().and_then(|k| connections.get(&k)) {
                Some(id) if !id.is_empty() => ae.connection_ids.push(id.clone()),
                _ => return Err(Error::Invalid),
            }
        }
        let initiator = boolean(entry, "dicomAssociationInitiator", true)?;
        let acceptor = boolean(entry, "dicomAssociationAcceptor", true)?;
        ae.association_initiator = initiator == Some(true);
        ae.association_acceptor = acceptor == Some(true);
        ae.installed = boolean(entry, "dicomInstalled", false)?;
        let ae_key = dn_key(&entry.dn).unwrap_or_default();
        for capability in &capabilities {
            if parent_key(&capability.dn).unwrap_or_default() != ae_key {
                continue;
            }
            let sop_class_uid = single(capability, "dicomSOPClass", true)?;
            let role = single(capability, "dicomTransferRole", true)?;
            ae.transfer_capabilities.push(TransferCapability {
                sop_class_uid,
                role: to_upper(&role),
                transfer_syntax_uids: capability
                    .values("dicomTransferSyntax")
                    .iter()
                    .map(|v| lossy(v))
                    .collect(),
            });
        }
        if ae.transfer_capabilities.is_empty() {
            result.diagnostics.push(Diagnostic {
                code: "transfer-capabilities-absent".into(),
                object: ae.ae_title.clone(),
            });
        }
        result.model.application_entities.push(ae);
    }
    if let Some(e) = ctx_error(ctx) {
        return Err(e);
    }
    netconf::validate(&result.model, o.model).map_err(Error::Netconf)?;
    if !o.ae_title.is_empty() {
        result
            .model
            .application_entities
            .retain(|ae| ae.ae_title == o.ae_title);
    }
    Ok(result)
}
