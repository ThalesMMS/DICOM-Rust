use super::*;
use crate::object::Vr;
use std::collections::{BTreeMap, HashMap};
const MAX_SUBSCRIPTION_MATCHING_KEYS: usize = 16;
const MAX_SUBSCRIPTION_VALUES_PER_KEY: usize = 16;
const MAX_SUBSCRIPTION_MATCHING_VALUE_BYTES: usize = 4 << 10;
pub type MatchingKeys = BTreeMap<String, Vec<String>>;
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SubscriptionMatchingKey {
    pub tag: Tag,
    pub values: Vec<String>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SubscriptionFilter {
    pub keys: Vec<SubscriptionMatchingKey>,
}
impl SubscriptionFilter {
    pub fn matches(&self, step: &Step) -> bool {
        subscription_filter_matches(Some(self), step)
    }
}
fn filter_error(status: u16, kind: Kind) -> Error {
    Error::Filter {
        status,
        err: Box::new(kind.into()),
    }
}
pub(crate) fn subscription_filter_status(err: &Error) -> u16 {
    match err {
        Error::Filter { status, .. } => *status,
        _ => STATUS_INVALID_ARGUMENT_VALUE,
    }
}
pub fn parse_tag(s: &str) -> Option<Tag> {
    let s = s.trim();
    if s.len() == 8 && !s.contains(',') {
        let v = u32::from_str_radix(s, 16).ok()?;
        if s.starts_with('+') {
            return None;
        }
        return Some(Tag::new((v >> 16) as u16, v as u16));
    }
    let open = s.starts_with('(');
    let close = s.ends_with(')');
    if open != close {
        return None;
    }
    let s = if open { &s[1..s.len() - 1] } else { s };
    let parts: Vec<&str> = s.split(',').collect();
    if parts.len() != 2 {
        return None;
    }
    let hex16 = |p: &str| {
        let p = p.trim();
        if p.is_empty() || p.starts_with('+') {
            return None;
        }
        u16::from_str_radix(p, 16).ok()
    };
    Some(Tag::new(hex16(parts[0])?, hex16(parts[1])?))
}
pub(crate) fn compile_subscription_filter(keys: &MatchingKeys) -> Result<SubscriptionFilter> {
    let invalid = |status| filter_error(status, Kind::InvalidDataSet);
    let limit = || filter_error(STATUS_RESOURCE_LIMITATION, Kind::ResourceLimit);
    if keys.is_empty() {
        return Err(invalid(STATUS_INVALID_ARGUMENT_VALUE));
    }
    if keys.len() > MAX_SUBSCRIPTION_MATCHING_KEYS {
        return Err(limit());
    }
    let mut by_tag: HashMap<Tag, Vec<String>> = HashMap::new();
    let mut total = 0usize;
    for (encoded, values) in keys {
        let Some(tag) = parse_tag(encoded) else {
            return Err(invalid(STATUS_NO_SUCH_ARGUMENT));
        };
        if by_tag.contains_key(&tag) {
            return Err(invalid(STATUS_INVALID_ARGUMENT_VALUE));
        }
        let Some(vr) = query_entry(tag) else {
            return Err(invalid(STATUS_NO_SUCH_ARGUMENT));
        };
        if vr == Vr::SQ || values.is_empty() {
            return Err(invalid(STATUS_INVALID_ARGUMENT_VALUE));
        }
        if values.len() > MAX_SUBSCRIPTION_VALUES_PER_KEY {
            return Err(limit());
        }
        for v in values {
            if v.is_empty() {
                return Err(invalid(STATUS_INVALID_ARGUMENT_VALUE));
            }
            total += v.len();
            if total > MAX_SUBSCRIPTION_MATCHING_VALUE_BYTES {
                return Err(limit());
            }
            if !valid_subscription_matching_value(vr, v) {
                return Err(invalid(STATUS_INVALID_ARGUMENT_VALUE));
            }
        }
        by_tag.insert(tag, values.clone());
    }
    if let Some(v) = by_tag.get(&TAG_SPECIFIC_CHARACTER_SET)
        && (v.len() != 1 || v[0].trim().is_empty())
    {
        return Err(invalid(STATUS_INVALID_ARGUMENT_VALUE));
    }
    if let Some(v) = by_tag.get(&TAG_TIMEZONE_OFFSET_FROM_UTC)
        && (v.len() != 1 || !valid_ups_timezone_offset(&v[0]))
    {
        return Err(invalid(STATUS_INVALID_ARGUMENT_VALUE));
    }
    let mut predicates = by_tag.len();
    if by_tag.contains_key(&TAG_SPECIFIC_CHARACTER_SET) {
        predicates -= 1;
    }
    if by_tag.contains_key(&TAG_TIMEZONE_OFFSET_FROM_UTC) {
        predicates -= 1;
    }
    if predicates == 0 {
        return Err(invalid(STATUS_INVALID_ARGUMENT_VALUE));
    }
    let mut tags: Vec<Tag> = by_tag.keys().copied().collect();
    tags.sort();
    Ok(SubscriptionFilter {
        keys: tags
            .into_iter()
            .map(|tag| SubscriptionMatchingKey {
                tag,
                values: by_tag.remove(&tag).unwrap_or_default(),
            })
            .collect(),
    })
}
fn valid_subscription_matching_value(vr: Vr, value: &str) -> bool {
    if vr == Vr::UI && value.contains(['*', '?']) {
        return false;
    }
    if vr != Vr::DA && vr != Vr::DT && vr != Vr::TM {
        return true;
    }
    if parse_temporal_range(vr, value, "").is_some() {
        return true;
    }
    parse_temporal_value(vr, value, "").is_some()
}
pub(crate) fn subscription_filter_matches(
    filter: Option<&SubscriptionFilter>,
    step: &Step,
) -> bool {
    let Some(filter) = filter.filter(|f| !f.keys.is_empty()) else {
        return false;
    };
    let query_tz = filter
        .keys
        .iter()
        .find(|k| k.tag == TAG_TIMEZONE_OFFSET_FROM_UTC && k.values.len() == 1)
        .map(|k| k.values[0].clone())
        .unwrap_or_default();
    let candidate_tz =
        data_set_string(&step.attributes, TAG_TIMEZONE_OFFSET_FROM_UTC).unwrap_or_default();
    let mut candidate = StepQueryMatcher::new(&step.attributes);
    for key in &filter.keys {
        if key.tag == TAG_SPECIFIC_CHARACTER_SET || key.tag == TAG_TIMEZONE_OFFSET_FROM_UTC {
            continue;
        }
        if !candidate.matches(key.tag, &key.values, &query_tz, &candidate_tz) {
            return false;
        }
    }
    true
}
