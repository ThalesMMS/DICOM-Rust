mod ber;
mod client;
mod dn;
mod identity;
mod model;
use super::{ConfigurationError, Options as ModelOptions};
use crate::net::dicomweb::urlrules;
use crate::net::ul::Nanos;
pub use client::read;
pub use identity::{AuthenticationOptions, Identity, authenticate, identity_dn_key};
use std::fmt;
pub const NORMATIVE_EDITION: &str = "DICOM PS3.15 2025e Annex H";
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    Invalid,
    Limit,
    Authentication,
    Transport,
    Protocol,
    Referral,
    Identity,
    Canceled,
    DeadlineExceeded,
    Netconf(ConfigurationError),
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            Error::Invalid => "LDAP directory configuration or schema is invalid",
            Error::Limit => "LDAP directory resource limit exceeded",
            Error::Authentication => "LDAP directory authentication failed",
            Error::Transport => "LDAP directory connection or TLS failed",
            Error::Protocol => "LDAP directory response is invalid",
            Error::Referral => "LDAP directory referral is not allowed",
            Error::Identity => "LDAP directory user identity is missing or ambiguous",
            Error::Canceled => "context canceled",
            Error::DeadlineExceeded => "context deadline exceeded",
            Error::Netconf(e) => return e.fmt(f),
        };
        f.write_str(text)
    }
}
impl std::error::Error for Error {}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TlsConfig {
    pub root_certificates: Vec<Vec<u8>>,
    pub system_roots: bool,
    pub server_name: String,
    pub insecure_skip_verify: bool,
    pub verify_peer_certificate: bool,
    pub verify_connection: bool,
}
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Config {
    pub url: String,
    pub base_dn: String,
    pub bind_dn: String,
    pub password: String,
    pub tls: Option<TlsConfig>,
    pub allow_anonymous_cleartext: bool,
}
const REDACTED: &str = "LDAP configuration (credentials redacted)";
impl fmt::Display for Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(REDACTED)
    }
}
impl fmt::Debug for Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(REDACTED)
    }
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Options {
    pub timeout: Nanos,
    pub page_size: i64,
    pub max_pages: i64,
    pub max_devices: i64,
    pub max_entries: i64,
    pub max_depth: i64,
    pub max_packet_bytes: i64,
    pub max_wire_bytes: i64,
    pub max_entry_bytes: i64,
    pub max_model_bytes: i64,
    pub max_ber_depth: i64,
    pub max_ber_nodes: i64,
    pub device_name: String,
    pub ae_title: String,
    pub model: ModelOptions,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Diagnostic {
    pub code: String,
    pub object: String,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Device {
    pub id: String,
    pub dn: String,
    pub model: super::Device,
    pub diagnostics: Vec<Diagnostic>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub devices: Vec<Device>,
    pub edition: String,
    pub skipped: Vec<Diagnostic>,
}
pub(crate) fn normalized(mut o: Options) -> Result<Options, Error> {
    if o.timeout.0 == 0 {
        o.timeout = Nanos::from_secs(20);
    }
    if o.timeout.0 < 1_000_000 || o.timeout.0 > 300_000_000_000 {
        return Err(Error::Limit);
    }
    let fields: [(&mut i64, i64, i64); 11] = [
        (&mut o.page_size, 64, 256),
        (&mut o.max_pages, 128, 512),
        (&mut o.max_devices, 32, 128),
        (&mut o.max_entries, 4096, 16384),
        (&mut o.max_depth, 8, 32),
        (&mut o.max_packet_bytes, 1 << 20, 16 << 20),
        (&mut o.max_wire_bytes, 16 << 20, 64 << 20),
        (&mut o.max_entry_bytes, 64 << 10, 1 << 20),
        (&mut o.max_model_bytes, 8 << 20, 32 << 20),
        (&mut o.max_ber_depth, 16, 32),
        (&mut o.max_ber_nodes, 8192, 65536),
    ];
    for (value, def, max) in fields {
        if *value == 0 {
            *value = def;
        }
        if *value < 1 || *value > max {
            return Err(Error::Limit);
        }
    }
    if o.device_name.len() > 128 || o.ae_title.len() > 16 {
        return Err(Error::Limit);
    }
    Ok(o)
}
pub(crate) struct Endpoint {
    pub scheme: String,
    pub host: String,
    pub port: String,
}
fn port_ok(port: &str) -> bool {
    let mut n: u64 = 0;
    for c in port.bytes() {
        if !c.is_ascii_digit() {
            return false;
        }
        n = (n * 10 + u64::from(c - b'0')).min(1 << 32);
    }
    n <= 65535
}
pub(crate) fn validated(c: &Config) -> Result<(Endpoint, dn::Dn), Error> {
    if c.url.len() > 2048
        || c.base_dn.len() > 4096
        || c.bind_dn.len() > 4096
        || c.password.len() > 65535
    {
        return Err(Error::Limit);
    }
    let u = urlrules::parse(&c.url).map_err(|_| Error::Invalid)?;
    if u.user.is_some()
        || !u.opaque.is_empty()
        || !u.raw_query.is_empty()
        || !u.fragment.is_empty()
        || (!u.path.is_empty() && u.path != "/")
        || (u.scheme != "ldap" && u.scheme != "ldaps")
        || u.hostname().is_empty()
    {
        return Err(Error::Invalid);
    }
    let host = u.hostname();
    if host.contains([' ', '/', '\\', '\r', '\n', '\0']) || host.len() > 253 {
        return Err(Error::Invalid);
    }
    if !u.port().is_empty() && !port_ok(u.port()) {
        return Err(Error::Invalid);
    }
    let root = dn::parse_dn(c.base_dn.as_bytes()).map_err(|_| Error::Invalid)?;
    if root.rdns.is_empty() || root.rdns.len() > 32 {
        return Err(Error::Invalid);
    }
    if !c.bind_dn.is_empty() && dn::parse_dn(c.bind_dn.as_bytes()).is_err() {
        return Err(Error::Invalid);
    }
    if c.password.is_empty() != c.bind_dn.is_empty() {
        return Err(Error::Invalid);
    }
    if c.allow_anonymous_cleartext
        && (u.scheme != "ldap" || !c.bind_dn.is_empty() || !c.password.is_empty())
    {
        return Err(Error::Invalid);
    }
    if let Some(t) = &c.tls
        && (t.insecure_skip_verify || t.verify_peer_certificate || t.verify_connection)
    {
        return Err(Error::Invalid);
    }
    let endpoint = Endpoint {
        scheme: u.scheme.clone(),
        host: host.to_string(),
        port: u.port().to_string(),
    };
    Ok((endpoint, root))
}
pub fn validate_config(c: &Config, o: &Options) -> Result<(), Error> {
    normalized(o.clone())?;
    validated(c).map(|_| ())
}
