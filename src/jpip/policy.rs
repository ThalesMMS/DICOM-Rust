use super::{ChainError, Error, ErrorKind, Sentinel};
use crate::net::dicomweb::BearerTokenSource;
use crate::net::dicomweb::urlrules::{self, Url};
use crate::textfmt;
use std::collections::HashMap;
use std::sync::Arc;
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Rule {
    pub scheme: String,
    pub host: String,
    pub port: String,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BasicCredential {
    pub origin: String,
    pub username: String,
    pub password: String,
}
#[derive(Clone, Default)]
pub struct BearerCredential {
    pub origin: String,
    pub source: Option<Arc<dyn BearerTokenSource>>,
}
#[derive(Clone, Default)]
pub struct Policy {
    pub rules: Vec<Rule>,
    pub credentials: Vec<BasicCredential>,
    pub bearer_credentials: Vec<BearerCredential>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct NormalizedRule {
    scheme: String,
    host: String,
    port: String,
}
#[derive(Clone, Default)]
pub(crate) struct Credential {
    pub username: String,
    pub password: String,
    pub bearer: Option<Arc<dyn BearerTokenSource>>,
}
#[derive(Clone, Default)]
pub(crate) struct CompiledPolicy {
    rules: Vec<NormalizedRule>,
    credentials: HashMap<String, Credential>,
}
pub fn rule_from_url(raw: &str) -> Result<Rule, ChainError> {
    let u = parse_endpoint_url(raw)?;
    Ok(Rule {
        scheme: u.scheme.clone(),
        host: u.hostname().to_string(),
        port: effective_port(&u.scheme, u.port()).to_string(),
    })
}
pub fn rule_for_host(scheme: &str, host: &str, port: &str) -> Result<Rule, ChainError> {
    let rule = Rule {
        scheme: scheme.into(),
        host: host.into(),
        port: port.into(),
    };
    normalize_rule(&rule)?;
    Ok(rule)
}
pub(crate) fn compile_policy(policy: &Policy) -> Result<CompiledPolicy, ChainError> {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Kind {
        Basic,
        Bearer,
    }
    let mut out = CompiledPolicy::default();
    let mut kinds: HashMap<String, Kind> = HashMap::new();
    for candidate in &policy.rules {
        let rule = normalize_rule(candidate)?;
        if !out.rules.contains(&rule) {
            out.rules.push(rule);
        }
    }
    let origin_only = |u: &Url| {
        (u.path.is_empty() || u.path == "/") && u.raw_query.is_empty() && u.fragment.is_empty()
    };
    for candidate in &policy.credentials {
        let u = parse_endpoint_url(&candidate.origin)
            .map_err(|e| prefixed("jpip: credential origin: ", e))?;
        if !origin_only(&u) {
            return Err(ChainError::plain(
                "jpip: credential origin must not include a path, query, or fragment",
            ));
        }
        let origin = canonical_origin(&u);
        if kinds.get(&origin) == Some(&Kind::Bearer) {
            return Err(ChainError::plain(
                "jpip: an origin cannot use both Basic and bearer credentials",
            ));
        }
        kinds.insert(origin.clone(), Kind::Basic);
        out.credentials.insert(
            origin,
            Credential {
                username: candidate.username.clone(),
                password: candidate.password.clone(),
                bearer: None,
            },
        );
    }
    for candidate in &policy.bearer_credentials {
        let u = parse_endpoint_url(&candidate.origin)
            .map_err(|e| prefixed("jpip: bearer credential origin: ", e))?;
        if !origin_only(&u) {
            return Err(ChainError::plain(
                "jpip: bearer credential origin must not include a path, query, or fragment",
            ));
        }
        let Some(source) = &candidate.source else {
            return Err(ChainError::plain(
                "jpip: bearer credential source is required",
            ));
        };
        let origin = canonical_origin(&u);
        if kinds.get(&origin) == Some(&Kind::Basic) {
            return Err(ChainError::plain(
                "jpip: an origin cannot use both Basic and bearer credentials",
            ));
        }
        kinds.insert(origin.clone(), Kind::Bearer);
        out.credentials.insert(
            origin,
            Credential {
                bearer: Some(source.clone()),
                ..Credential::default()
            },
        );
    }
    Ok(out)
}
fn prefixed(prefix: &str, e: ChainError) -> ChainError {
    let out = ChainError::plain(format!("{prefix}{e}"));
    if e.is(Sentinel::InvalidRequest) {
        return out.with(Sentinel::InvalidRequest);
    }
    out
}
fn normalize_rule(candidate: &Rule) -> Result<NormalizedRule, ChainError> {
    let scheme = candidate.scheme.trim().to_lowercase();
    if scheme != "http" && scheme != "https" {
        return Err(ChainError::plain(
            "jpip: policy scheme must be http or https",
        ));
    }
    let mut host = candidate.host.trim().to_string();
    let mut candidate_port = candidate.port.clone();
    if host.contains("://") || host.contains(['/', '?', '#', '@']) {
        return Err(ChainError::plain(
            "jpip: policy host must be a hostname or IP literal",
        ));
    }
    if let Some((parsed_host, parsed_port)) = split_host_port(&host) {
        if !candidate_port.trim().is_empty() {
            return Err(ChainError::plain("jpip: policy port specified twice"));
        }
        host = parsed_host;
        candidate_port = parsed_port;
    }
    let host = normalize_hostname(&host);
    if host.is_empty() {
        return Err(ChainError::plain("jpip: policy host is required"));
    }
    let mut port = candidate_port.trim().to_string();
    if !port.is_empty() {
        match port.parse::<i64>() {
            Ok(number) if (1..=65535).contains(&number) => port = number.to_string(),
            _ => {
                return Err(ChainError::plain(format!(
                    "jpip: invalid policy port {}",
                    textfmt::quote(&port)
                )));
            }
        }
    }
    Ok(NormalizedRule { scheme, host, port })
}
pub(crate) fn split_host_port(hostport: &str) -> Option<(String, String)> {
    let b = hostport.as_bytes();
    let i = hostport.rfind(':')?;
    let (mut j, mut k) = (0, 0);
    let host;
    if b[0] == b'[' {
        let end = hostport.find(']')?;
        if end + 1 == hostport.len() || end + 1 != i {
            return None;
        }
        host = &hostport[1..end];
        j = 1;
        k = end + 1;
    } else {
        host = &hostport[..i];
        if host.contains(':') {
            return None;
        }
    }
    if hostport[j..].contains('[') || hostport[k..].contains(']') {
        return None;
    }
    Some((host.to_string(), hostport[i + 1..].to_string()))
}
fn join_host_port(host: &str, port: &str) -> String {
    if host.contains(':') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    }
}
pub(crate) fn parse_endpoint_url(raw: &str) -> Result<Url, ChainError> {
    let Ok(mut u) = urlrules::parse(raw.trim()) else {
        return Err(ChainError::plain(format!(
            "jpip: parse endpoint URL: {}",
            Sentinel::InvalidRequest.text()
        ))
        .with(Sentinel::InvalidRequest));
    };
    u.scheme = u.scheme.to_lowercase();
    if u.scheme != "http" && u.scheme != "https" {
        return Err(ChainError::plain(
            "jpip: endpoint URL must use http or https",
        ));
    }
    if u.host.is_empty() || u.hostname().is_empty() {
        return Err(ChainError::plain("jpip: endpoint URL must include a host"));
    }
    if u.user.is_some() {
        return Err(ChainError::plain(
            "jpip: endpoint URL must not contain user information",
        ));
    }
    if !u.fragment.is_empty() {
        return Err(ChainError::plain(
            "jpip: endpoint URL must not contain a fragment",
        ));
    }
    Ok(u)
}
impl CompiledPolicy {
    pub fn authorize(&self, u: &Url) -> Result<Credential, Error> {
        if u.user.is_some() || !u.fragment.is_empty() {
            return Err(Error::sentinel(
                ErrorKind::PolicyDenied,
                "authorize",
                Sentinel::PolicyDenied,
            ));
        }
        let host = normalize_hostname(u.hostname());
        let scheme = u.scheme.to_lowercase();
        let port = effective_port(&scheme, u.port());
        for rule in &self.rules {
            if rule.scheme != scheme
                || rule.host != host
                || !rule.port.is_empty() && effective_port(&rule.scheme, &rule.port) != port
            {
                continue;
            }
            return Ok(self
                .credentials
                .get(&canonical_origin(u))
                .cloned()
                .unwrap_or_default());
        }
        Err(Error::sentinel(
            ErrorKind::PolicyDenied,
            "authorize",
            Sentinel::PolicyDenied,
        ))
    }
}
fn effective_port<'a>(scheme: &str, port: &'a str) -> &'a str {
    if !port.is_empty() {
        return port;
    }
    match scheme.to_lowercase().as_str() {
        "http" => "80",
        "https" => "443",
        _ => "",
    }
}
fn normalize_hostname(host: &str) -> String {
    host.trim().trim_matches(['[', ']']).to_lowercase()
}
fn canonical_origin(u: &Url) -> String {
    let host = normalize_hostname(u.hostname());
    let port = effective_port(&u.scheme, u.port());
    format!(
        "{}://{}",
        u.scheme.to_lowercase(),
        join_host_port(&host, port)
    )
}
