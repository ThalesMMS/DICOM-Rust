#![forbid(unsafe_code)]
pub mod association_profile;
pub mod compile;
pub mod json;
pub mod ldapconfig;
use crate::net::ul::{MAX_AE_TITLE_LENGTH, normalize_ae_title, validate_ae_title};
pub use association_profile::from_association_profile;
pub use compile::{CompiledConnection, compile_connection};
pub use json::{marshal, parse, read_file};
use std::collections::{HashMap, HashSet};
use std::fmt;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cause {
    Invalid,
    ResourceLimit,
}
impl Cause {
    pub fn text(self) -> &'static str {
        match self {
            Cause::Invalid => "dicom netconf: invalid configuration",
            Cause::ResourceLimit => "dicom netconf: resource limit",
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigurationError {
    field: &'static str,
    cause: Cause,
    io: Option<std::io::ErrorKind>,
}
impl ConfigurationError {
    pub fn logical_field(&self) -> &'static str {
        self.field
    }
    pub fn is(&self, cause: Cause) -> bool {
        self.cause == cause
    }
    #[doc(hidden)]
    pub fn io_kind(&self) -> Option<std::io::ErrorKind> {
        self.io
    }
}
impl fmt::Display for ConfigurationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.cause.text(), self.field)
    }
}
impl std::error::Error for ConfigurationError {}
pub(crate) type Result<T> = std::result::Result<T, ConfigurationError>;
pub(crate) fn field_error(field: &'static str, cause: Cause) -> ConfigurationError {
    ConfigurationError {
        field,
        cause,
        io: None,
    }
}
pub(crate) fn io_field_error(field: &'static str, kind: std::io::ErrorKind) -> ConfigurationError {
    ConfigurationError {
        field,
        cause: Cause::Invalid,
        io: Some(kind),
    }
}
pub const ROLE_SCU: &str = "SCU";
pub const ROLE_SCP: &str = "SCP";
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Device {
    pub schema_version: i64,
    pub device_name: String,
    pub description: String,
    pub installed: Option<bool>,
    pub connections: Vec<Connection>,
    pub application_entities: Vec<ApplicationEntity>,
}
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Connection {
    pub id: String,
    pub hostname: String,
    pub port: i64,
    pub installed: Option<bool>,
    pub tls_required: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ApplicationEntity {
    pub ae_title: String,
    pub connection_ids: Vec<String>,
    pub association_initiator: bool,
    pub association_acceptor: bool,
    pub installed: Option<bool>,
    pub transfer_capabilities: Vec<TransferCapability>,
}
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TransferCapability {
    pub sop_class_uid: String,
    pub role: String,
    pub transfer_syntax_uids: Vec<String>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Limits {
    pub max_json_bytes: i64,
    pub max_depth: i64,
    pub max_scalar_bytes: i64,
    pub max_name_bytes: i64,
    pub max_description_bytes: i64,
    pub max_hostname_bytes: i64,
    pub max_connections: i64,
    pub max_aes: i64,
    pub max_capabilities: i64,
    pub max_capabilities_per_ae: i64,
    pub max_transfer_syntaxes: i64,
    pub max_transfer_syntaxes_per_capability: i64,
    pub max_compiled_bytes: i64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Options {
    pub limits: Limits,
}
pub(crate) fn normalized(o: Options) -> std::result::Result<Options, Cause> {
    let mut l = o.limits;
    if l.max_json_bytes < 0 || l.max_json_bytes == i64::MAX {
        return Err(Cause::ResourceLimit);
    }
    if l.max_json_bytes == 0 {
        l.max_json_bytes = 1 << 20;
    }
    let pairs: [(&mut i64, i64); 12] = [
        (&mut l.max_depth, 16),
        (&mut l.max_scalar_bytes, 8192),
        (&mut l.max_name_bytes, 128),
        (&mut l.max_description_bytes, 4096),
        (&mut l.max_hostname_bytes, 253),
        (&mut l.max_connections, 16),
        (&mut l.max_aes, 64),
        (&mut l.max_capabilities, 4096),
        (&mut l.max_capabilities_per_ae, 1024),
        (&mut l.max_transfer_syntaxes, 16384),
        (&mut l.max_transfer_syntaxes_per_capability, 256),
        (&mut l.max_compiled_bytes, 8 << 20),
    ];
    for (value, default) in pairs {
        if *value < 0 || *value > i64::MAX / 1024 {
            return Err(Cause::ResourceLimit);
        }
        if *value == 0 {
            *value = default;
        }
    }
    if l.max_depth > 256 {
        return Err(Cause::ResourceLimit);
    }
    Ok(Options { limits: l })
}
pub(crate) struct Budget {
    used: i64,
    max: i64,
}
impl Budget {
    pub(crate) fn new(max: i64) -> Budget {
        Budget { used: 0, max }
    }
    pub(crate) fn add(&mut self, n: i64) -> std::result::Result<(), Cause> {
        if n < 0 || n > self.max - self.used {
            return Err(Cause::ResourceLimit);
        }
        self.used += n;
        Ok(())
    }
}
const SIZEOF_DEVICE: i64 = 96;
const SIZEOF_CONNECTION: i64 = 56;
const SIZEOF_APPLICATION_ENTITY: i64 = 80;
const SIZEOF_TRANSFER_CAPABILITY: i64 = 56;
fn len(s: &str) -> i64 {
    s.len() as i64
}
fn model_footprint(d: &Device, l: &Limits) -> std::result::Result<(), Cause> {
    let limit = Err(Cause::ResourceLimit);
    if len(&d.device_name) > l.max_name_bytes || len(&d.description) > l.max_description_bytes {
        return limit;
    }
    let (connections, aes) = (
        d.connections.len() as i64,
        d.application_entities.len() as i64,
    );
    if connections > l.max_connections || aes > l.max_aes {
        return limit;
    }
    let mut b = Budget::new(l.max_compiled_bytes);
    b.add(SIZEOF_DEVICE + len(&d.device_name) + len(&d.description) + 64 * (connections + aes))?;
    let (mut caps, mut syntaxes) = (0i64, 0i64);
    for c in &d.connections {
        if len(&c.id) > l.max_name_bytes || len(&c.hostname) > l.max_hostname_bytes {
            return limit;
        }
        b.add(SIZEOF_CONNECTION + len(&c.id) + len(&c.hostname) + 1)?;
    }
    for ae in &d.application_entities {
        if ae.ae_title.len() > MAX_AE_TITLE_LENGTH {
            return limit;
        }
        let (ae_caps, ae_refs) = (
            ae.transfer_capabilities.len() as i64,
            ae.connection_ids.len() as i64,
        );
        if ae_caps > l.max_capabilities_per_ae
            || ae_caps > l.max_capabilities - caps
            || ae_refs > l.max_connections
        {
            return limit;
        }
        caps += ae_caps;
        b.add(SIZEOF_APPLICATION_ENTITY + len(&ae.ae_title) + 1 + 64 * (ae_refs + ae_caps))?;
        for id in &ae.connection_ids {
            if len(id) > l.max_name_bytes {
                return limit;
            }
            b.add(16 + len(id))?;
        }
        for tc in &ae.transfer_capabilities {
            if tc.sop_class_uid.len() > 64 {
                return limit;
            }
            let count = tc.transfer_syntax_uids.len() as i64;
            if count > l.max_transfer_syntaxes_per_capability
                || count > l.max_transfer_syntaxes - syntaxes
            {
                return limit;
            }
            syntaxes += count;
            b.add(
                SIZEOF_TRANSFER_CAPABILITY + len(&tc.sop_class_uid) + len(&tc.role) + 64 * count,
            )?;
            for ts in &tc.transfer_syntax_uids {
                if ts.len() > 64 {
                    return limit;
                }
                b.add(16 + len(ts))?;
            }
        }
    }
    Ok(())
}
pub(crate) fn active(v: Option<bool>, inherited: bool) -> bool {
    v.unwrap_or(inherited)
}
pub(crate) fn text_ok(s: &str, max: i64, empty: bool) -> bool {
    if (!empty && s.is_empty()) || len(s) > max {
        return false;
    }
    !s.chars().any(char::is_control)
}
fn host_ok(s: &str, max: i64) -> bool {
    text_ok(s, max, false)
        && !s
            .chars()
            .any(|r| r as u32 > 127 || r == ' ' || r == '/' || r == '\\')
}
pub(crate) fn is_valid_uid(uid: &str) -> bool {
    crate::object::is_valid_uid(uid.as_bytes())
}
pub fn validate(d: &Device, options: Options) -> Result<()> {
    let o = normalized(options).map_err(|cause| field_error("limits", cause))?;
    validate_normalized(d, &o)
}
pub(crate) fn validate_normalized(d: &Device, o: &Options) -> Result<()> {
    let l = &o.limits;
    model_footprint(d, l).map_err(|cause| field_error("limits.model", cause))?;
    let invalid = |field| Err(field_error(field, Cause::Invalid));
    if d.schema_version != 1
        || !text_ok(&d.device_name, l.max_name_bytes, false)
        || !text_ok(&d.description, l.max_description_bytes, true)
        || d.connections.is_empty()
        || d.application_entities.is_empty()
    {
        return invalid("device");
    }
    let mut connections: HashSet<&str> = HashSet::new();
    for c in &d.connections {
        if !text_ok(&c.id, l.max_name_bytes, false)
            || c.id.trim() != c.id
            || !host_ok(&c.hostname, l.max_hostname_bytes)
            || c.port < 0
            || c.port > 65535
            || !connections.insert(&c.id)
        {
            return invalid("connections[]");
        }
    }
    let mut aes: HashSet<&str> = HashSet::new();
    for ae in &d.application_entities {
        let title = normalize_ae_title(&ae.ae_title);
        if validate_ae_title(title).is_err() || aes.contains(title) || ae.connection_ids.is_empty()
        {
            return invalid("applicationEntities[]");
        }
        aes.insert(title);
        let mut refs: HashSet<&str> = HashSet::new();
        for id in &ae.connection_ids {
            if !connections.contains(id.as_str()) || !refs.insert(id) {
                return invalid("applicationEntities[].connectionIDs[]");
            }
        }
        let mut pairs: HashMap<&str, u8> = HashMap::new();
        for tc in &ae.transfer_capabilities {
            let bit = match tc.role.as_str() {
                ROLE_SCU => 1u8,
                ROLE_SCP => 2u8,
                _ => return invalid("applicationEntities[].transferCapabilities[].role"),
            };
            let entry = pairs.entry(&tc.sop_class_uid).or_insert(0);
            if !is_valid_uid(&tc.sop_class_uid)
                || tc.transfer_syntax_uids.is_empty()
                || *entry & bit != 0
            {
                return invalid("applicationEntities[].transferCapabilities[]");
            }
            *entry |= bit;
            let mut seen: HashSet<&str> = HashSet::new();
            for ts in &tc.transfer_syntax_uids {
                if !is_valid_uid(ts) || !seen.insert(ts) {
                    return invalid(
                        "applicationEntities[].transferCapabilities[].transferSyntaxUIDs[]",
                    );
                }
            }
        }
    }
    Ok(())
}
