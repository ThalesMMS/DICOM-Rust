use super::client::{Session, and_filter, equality, with_session};
use super::dn::{Dn, dn_key, equal_fold, parse_dn};
use super::model::{Entry, admit_entry, ctx_error, single};
use super::{Config, Error, Options, normalized, validated};
use crate::net::ul::Ctx;
use std::collections::HashSet;
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AuthenticationOptions {
    pub group_base_dn: String,
    pub max_groups: i64,
    pub limits: Options,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Identity {
    pub dn: String,
    pub groups: Vec<String>,
}
pub fn identity_dn_key(value: &str) -> Result<String, Error> {
    if value.is_empty() || value.len() > 4096 || value.chars().any(char::is_control) {
        return Err(Error::Invalid);
    }
    dn_key(value.as_bytes()).map_err(|_| Error::Invalid)
}
fn identity_key_bytes(value: &[u8]) -> Result<String, Error> {
    let text = std::str::from_utf8(value).map_err(|_| Error::Invalid)?;
    identity_dn_key(text)
}
#[derive(Default)]
struct Budget {
    requests: i64,
    entries: i64,
    bytes: i64,
}
fn search_identity(
    session: &mut Session,
    (base, root): (&[u8], &Dn),
    filter: Vec<u8>,
    attributes: &[&str],
    max_entries: i64,
    o: &Options,
    budget: &mut Budget,
) -> Result<Vec<Entry>, Error> {
    if let Some(e) = ctx_error(&session.ctx) {
        return Err(e);
    }
    if budget.requests == o.max_pages {
        return Err(Error::Limit);
    }
    budget.requests += 1;
    let limit = max_entries.min(o.max_entries - budget.entries);
    let result = session
        .search(base, limit + 1, filter, attributes, None)
        .map_err(|e| session.normalize(e))?;
    if let Some(e) = ctx_error(&session.ctx) {
        return Err(e);
    }
    if result.referrals != 0 {
        return Err(Error::Referral);
    }
    if !result.controls.is_empty() {
        return Err(Error::Protocol);
    }
    if result.entries.len() as i64 > limit {
        return Err(Error::Limit);
    }
    for entry in &result.entries {
        let weight = admit_entry(entry, root, o)?;
        if weight > o.max_model_bytes - budget.bytes {
            return Err(Error::Limit);
        }
        budget.bytes += weight;
    }
    budget.entries += result.entries.len() as i64;
    Ok(result.entries)
}
fn verified_group_names(
    ctx: &Ctx,
    entries: &[Entry],
    user_key: &str,
) -> Result<Vec<String>, Error> {
    let mut names = Vec::with_capacity(entries.len());
    let mut seen = HashSet::new();
    for entry in entries {
        if let Some(e) = ctx_error(ctx) {
            return Err(e);
        }
        let key = identity_key_bytes(&entry.dn).map_err(|_| Error::Invalid)?;
        if seen.contains(&key) {
            return Err(Error::Invalid);
        }
        let group_class = entry
            .values("objectClass")
            .iter()
            .any(|v| equal_fold(v, b"groupOfNames"));
        let mut member_found = false;
        for value in entry.values("member") {
            member_found = identity_key_bytes(value)? == user_key || member_found;
        }
        if !group_class || !member_found {
            return Err(Error::Invalid);
        }
        seen.insert(key);
        names.push(String::from_utf8_lossy(&entry.dn).into_owned());
    }
    names.sort();
    Ok(names)
}
pub fn authenticate(
    ctx: &Ctx,
    c: &Config,
    username: &str,
    password: &str,
    options: &AuthenticationOptions,
) -> Result<Identity, Error> {
    if username.is_empty()
        || password.is_empty()
        || username.len() > 4096
        || password.len() > 4096
        || username.chars().any(char::is_control)
    {
        return Err(Error::Authentication);
    }
    let max_groups = if options.max_groups == 0 {
        128
    } else {
        options.max_groups
    };
    if !(1..=256).contains(&max_groups) {
        return Err(Error::Limit);
    }
    if c.allow_anonymous_cleartext
        || !options.limits.device_name.is_empty()
        || !options.limits.ae_title.is_empty()
    {
        return Err(Error::Invalid);
    }
    let limits = normalized(options.limits.clone())?;
    identity_dn_key(&options.group_base_dn)?;
    let group_root = parse_dn(options.group_base_dn.as_bytes()).map_err(|_| Error::Invalid)?;
    let (endpoint, user_root) = validated(c)?;
    with_session(ctx, c, &endpoint, &limits, |session| {
        let mut budget = Budget::default();
        let users = search_identity(
            session,
            (c.base_dn.as_bytes(), &user_root),
            equality("uid", username.as_bytes()),
            &["uid"],
            2,
            &limits,
            &mut budget,
        )?;
        if users.len() != 1 {
            return Err(Error::Identity);
        }
        let user = &users[0];
        match single(user, "uid", true) {
            Ok(uid) if uid == username => {}
            _ => return Err(Error::Identity),
        }
        let user_key = identity_key_bytes(&user.dn)?;
        if let Some(e) = ctx_error(&session.ctx) {
            return Err(e);
        }
        let user_dn = String::from_utf8_lossy(&user.dn).into_owned();
        session
            .bind(&user_dn, password)
            .map_err(|e| session.normalize(e))?;
        let filter = and_filter(&[
            equality("objectClass", b"groupOfNames"),
            equality("member", &user.dn),
        ]);
        let groups = search_identity(
            session,
            (options.group_base_dn.as_bytes(), &group_root),
            filter,
            &["objectClass", "member"],
            max_groups,
            &limits,
            &mut budget,
        )?;
        let groups = verified_group_names(&session.ctx, &groups, &user_key)?;
        Ok(Identity {
            dn: user_dn,
            groups,
        })
    })
}
