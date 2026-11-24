use super::*;
use crate::dcmtime::{self, PrecisionLevel, ZonedTime};
use crate::net::dimse::{
    StreamingCFindClient, StreamingCFindClientOptions, StreamingCFindHandler, StreamingCFindLimits,
    StreamingCFindRequest, StreamingCFindResult, StreamingCFindRoute, StreamingCFindYield,
};
use crate::net::ul::{Association, Ctx};
use crate::object::{Element, Value, Vr, dict};
use crate::validation::{Mode, ValidationOptions, validate_element_with};
use std::collections::HashMap;
use std::sync::Arc;
pub(crate) const MAX_QUERY_SCANNED_STEPS: i64 = 1_000_000;
pub(super) const MAX_QUERY_PAGE_SIZE: i64 = 4_096;
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct QueryKey {
    pub present: bool,
    pub values: Vec<String>,
}
pub fn match_values(values: &[&str]) -> QueryKey {
    QueryKey {
        present: true,
        values: values.iter().map(|v| v.to_string()).collect(),
    }
}
pub fn return_key() -> QueryKey {
    match_values(&[])
}
pub fn match_date_time_range(from: &str, through: &str) -> Result<QueryKey> {
    let (from, through) = (from.trim(), through.trim());
    if from.is_empty() && through.is_empty() {
        return Ok(return_key());
    }
    let lower = parse_temporal_value(Vr::DT, from, "");
    let upper = parse_temporal_value(Vr::DT, through, "");
    if (!from.is_empty() && lower.is_none())
        || (!through.is_empty() && upper.is_none())
        || matches!((&lower, &upper), (Some(l), Some(u)) if l.start > u.end)
    {
        return Err(Kind::QueryIdentifier.into());
    }
    Ok(match_values(&[&format!("{from}-{through}")]))
}
#[derive(Debug, Clone, Default)]
pub struct Query {
    pub keys: HashMap<Tag, QueryKey>,
}
#[derive(Debug, Clone, Default)]
pub struct ParsedQuery {
    pub keys: HashMap<Tag, QueryKey>,
    pub(super) order: Vec<Tag>,
}
impl ParsedQuery {
    pub fn has_matching_key(&self) -> bool {
        self.keys.iter().any(|(tag, key)| {
            *tag != TAG_SPECIFIC_CHARACTER_SET
                && *tag != TAG_TIMEZONE_OFFSET_FROM_UTC
                && key.present
                && !key.values.is_empty()
        })
    }
    pub fn matches_identifier(&self, identifier: Option<&Object>) -> bool {
        let Some(identifier) = identifier else {
            return false;
        };
        if !self.has_matching_key() {
            return false;
        }
        if let Some(cs) = identifier.get(TAG_SPECIFIC_CHARACTER_SET)
            && cs.vr() != Vr::CS
        {
            return false;
        }
        if identifier.character_set().is_err() {
            return false;
        }
        let query_tz = query_timezone_offset(self);
        let mut candidate_tz = String::new();
        if let Some(tz) = identifier.get(TAG_TIMEZONE_OFFSET_FROM_UTC) {
            let values = identifier.lookup_strings(TAG_TIMEZONE_OFFSET_FROM_UTC);
            let Ok(values) = values else {
                return false;
            };
            if tz.vr() != Vr::SH || values.len() > 1 {
                return false;
            }
            if values.len() == 1 && !values[0].is_empty() {
                if !valid_ups_timezone_offset(&values[0]) {
                    return false;
                }
                candidate_tz = values[0].clone();
            }
        }
        for (tag, key) in &self.keys {
            let tag = *tag;
            if !key.present
                || tag == TAG_SPECIFIC_CHARACTER_SET
                || tag == TAG_TIMEZONE_OFFSET_FROM_UTC
            {
                continue;
            }
            let element = identifier.get(tag);
            if element.is_none() && key.values.is_empty() {
                continue;
            }
            let vr = query_entry(tag);
            let (Some(element), Some(vr)) = (element, vr) else {
                return false;
            };
            if element.vr() != vr {
                return false;
            }
            if vr == Vr::SQ {
                if !matches!(element.value, Value::Sequence(_)) || !key.values.is_empty() {
                    return false;
                }
                continue;
            }
            let Ok(values) = identifier.lookup_strings(tag) else {
                return false;
            };
            if values.len() > 1 {
                return false;
            }
            if values.is_empty() || values[0].is_empty() {
                if !key.values.is_empty() {
                    return false;
                }
                continue;
            }
            let opts = ValidationOptions {
                mode: Mode::Strict,
                stop_first: true,
                ..Default::default()
            };
            let (_, err) =
                validate_element_with(&string_element(tag, vr, &[values[0].as_str()]), &opts);
            if err.is_some() {
                return false;
            }
            if !key.values.is_empty()
                && !query_values_match(vr, &key.values, &values, &query_tz, &candidate_tz)
            {
                return false;
            }
        }
        true
    }
}
pub fn build_query_identifier(query: &Query) -> Result<Object> {
    let mut tags: Vec<Tag> = query
        .keys
        .iter()
        .filter(|(_, k)| k.present)
        .map(|(t, _)| *t)
        .collect();
    tags.sort();
    let mut elements = Vec::with_capacity(tags.len());
    for tag in tags {
        let key = &query.keys[&tag];
        let Some(vr) = query_entry(tag) else {
            return Err(Kind::QueryIdentifier.into());
        };
        if vr == Vr::SQ {
            if !key.values.is_empty() {
                return Err(Kind::QueryIdentifier.into());
            }
            elements.push(empty_sequence(tag));
            continue;
        }
        if tag == TAG_TIMEZONE_OFFSET_FROM_UTC
            && (key.values.len() != 1 || !valid_ups_timezone_offset(&key.values[0]))
        {
            return Err(Kind::QueryIdentifier.into());
        }
        if tag == TAG_SPECIFIC_CHARACTER_SET
            && (key.values.len() != 1 || key.values[0].trim().is_empty())
        {
            return Err(Kind::QueryIdentifier.into());
        }
        if key.values.is_empty() {
            elements.push(string_element(tag, vr, &[""]));
        } else {
            elements.push(string_element_owned(tag, vr, key.values.clone()));
        }
    }
    Ok(new_data_set(with_query_character_set(elements)))
}
pub fn parse_query_identifier(identifier: Option<&Object>) -> Result<ParsedQuery> {
    let Some(identifier) = identifier else {
        return Err(Kind::QueryIdentifier.into());
    };
    let mut parsed = ParsedQuery::default();
    for element in identifier.elements() {
        let tag = element.tag();
        let Some(vr) = query_entry(tag).filter(|vr| *vr == element.vr()) else {
            return Err(Kind::QueryIdentifier.into());
        };
        let mut key = QueryKey {
            present: true,
            values: Vec::new(),
        };
        if vr == Vr::SQ {
            match &element.value {
                Value::Sequence(items) if items.is_empty() => {}
                _ => return Err(Kind::QueryIdentifier.into()),
            }
        } else {
            key.values = identifier
                .lookup_strings(tag)
                .map_err(|_| Error::from(Kind::QueryIdentifier))?;
            if key.values.len() == 1 && key.values[0].is_empty() {
                key.values = Vec::new();
            }
        }
        if tag == TAG_TIMEZONE_OFFSET_FROM_UTC
            && (key.values.len() != 1 || !valid_ups_timezone_offset(&key.values[0]))
        {
            return Err(Kind::QueryIdentifier.into());
        }
        if tag == TAG_SPECIFIC_CHARACTER_SET
            && (identifier.character_set().is_err()
                || key.values.len() != 1
                || key.values[0].trim().is_empty())
        {
            return Err(Kind::QueryIdentifier.into());
        }
        parsed.keys.insert(tag, key);
        parsed.order.push(tag);
    }
    Ok(parsed)
}
pub(crate) fn valid_ups_timezone_offset(value: &str) -> bool {
    let b = value.as_bytes();
    if b.len() != 5 || (b[0] != b'+' && b[0] != b'-') {
        return false;
    }
    if value == "-0000" {
        return false;
    }
    if !b[1..].iter().all(|c| c.is_ascii_digit()) {
        return false;
    }
    let hours = (b[1] - b'0') as i64 * 10 + (b[2] - b'0') as i64;
    let minutes = (b[3] - b'0') as i64 * 10 + (b[4] - b'0') as i64;
    let max_hours = if b[0] == b'-' { 12 } else { 14 };
    minutes < 60 && (hours < max_hours || (hours == max_hours && minutes == 0))
}
#[derive(Debug, Clone, Copy, Default)]
pub struct QueryScpOptions {
    pub limits: StreamingCFindLimits,
    pub max_scanned_steps: i64,
    pub page_size: i64,
}
impl QueryScpOptions {
    fn normalized(mut self) -> Result<QueryScpOptions> {
        if self.max_scanned_steps < 0
            || self.page_size < 0
            || self.max_scanned_steps > MAX_QUERY_SCANNED_STEPS
            || self.page_size > MAX_QUERY_PAGE_SIZE
        {
            return Err(Kind::ResourceLimit.into());
        }
        if self.max_scanned_steps == 0 {
            self.max_scanned_steps = 100_000;
        }
        if self.page_size == 0 {
            self.page_size = 128;
        }
        if self.page_size > self.max_scanned_steps {
            self.page_size = self.max_scanned_steps;
        }
        Ok(self)
    }
}
impl Service {
    pub fn query_routes(&self, options: QueryScpOptions) -> Result<Vec<StreamingCFindRoute>> {
        let options = options.normalized()?;
        let handler: Arc<dyn StreamingCFindHandler> = Arc::new(QueryHandler {
            service: self.clone(),
            options,
        });
        Ok(
            [PULL_SOP_CLASS_UID, WATCH_SOP_CLASS_UID, QUERY_SOP_CLASS_UID]
                .iter()
                .map(|uid| StreamingCFindRoute {
                    sop_class_uid: uid.to_string(),
                    response_sop_class_uid: uid.to_string(),
                    handler: Some(handler.clone()),
                    limits: options.limits,
                })
                .collect(),
        )
    }
}
struct QueryHandler {
    service: Service,
    options: QueryScpOptions,
}
impl StreamingCFindHandler for QueryHandler {
    fn find(
        &self,
        ctx: &Ctx,
        request: &StreamingCFindRequest,
        yield_: &mut StreamingCFindYield<'_>,
    ) -> dimse::Result<()> {
        let Ok(query) = parse_query_identifier(Some(&request.identifier)) else {
            return Err(dimse::Error::sentinel(
                dimse::Kind::StreamingCFindIdentifier,
            ));
        };
        if !query.has_matching_key() {
            return Ok(());
        }
        let max = self.options.max_scanned_steps;
        let mut after = 0u64;
        let mut scanned = 0i64;
        while scanned < max {
            let limit = self.options.page_size.min(max - scanned);
            let mut probe = limit;
            if scanned + limit == max {
                probe += 1;
            }
            let steps = self.service.inner.store.list_steps(
                ctx,
                &StepQuery {
                    after_sequence: after,
                    limit: probe,
                    ..Default::default()
                },
            );
            let Ok(mut steps) = steps else {
                return Err(dimse::Error::sentinel(dimse::Kind::StreamingCFindProvider));
            };
            if steps.is_empty() {
                return Ok(());
            }
            let got = steps.len() as i64;
            let has_more = got > limit;
            if has_more {
                steps.truncate(limit as usize);
            }
            for step in &steps {
                if let Some(e) = dimse::context_err(ctx) {
                    return Err(e);
                }
                scanned += 1;
                after = step.sequence;
                if !query_matches_step(&query, step) {
                    continue;
                }
                let (projected, warning) = project_query_result(&query, step);
                let status = if warning {
                    dimse::STATUS_PENDING_WARNING
                } else {
                    dimse::STATUS_PENDING
                };
                yield_(status, Some(&projected))?;
            }
            if has_more {
                return Err(dimse::Error::sentinel(
                    dimse::Kind::StreamingCFindResourceLimit,
                ));
            }
            if got < probe {
                return Ok(());
            }
        }
        Err(dimse::Error::sentinel(
            dimse::Kind::StreamingCFindResourceLimit,
        ))
    }
}
pub(super) fn query_matches_step(query: &ParsedQuery, step: &Step) -> bool {
    let query_tz = query_timezone_offset(query);
    let candidate_tz =
        data_set_string(&step.attributes, TAG_TIMEZONE_OFFSET_FROM_UTC).unwrap_or_default();
    let mut candidate = StepQueryMatcher::new(&step.attributes);
    for (tag, key) in &query.keys {
        if *tag == TAG_SPECIFIC_CHARACTER_SET || *tag == TAG_TIMEZONE_OFFSET_FROM_UTC {
            continue;
        }
        if key.values.is_empty() {
            continue;
        }
        if !candidate.matches(*tag, &key.values, &query_tz, &candidate_tz) {
            return false;
        }
    }
    true
}
pub(crate) struct StepQueryMatcher<'a> {
    attributes: &'a DataSet,
    text_scope: Option<Object>,
}
impl<'a> StepQueryMatcher<'a> {
    pub(crate) fn new(attributes: &'a DataSet) -> Self {
        StepQueryMatcher {
            attributes,
            text_scope: None,
        }
    }
    pub(crate) fn matches(
        &mut self,
        tag: Tag,
        requested: &[String],
        query_tz: &str,
        candidate_tz: &str,
    ) -> bool {
        let Some(element) = data_set_element(self.attributes, tag) else {
            return false;
        };
        let values = if matches!(element.value, Value::Raw(_))
            && element.vr().uses_specific_character_set()
        {
            let scope = self
                .text_scope
                .get_or_insert_with(|| Object::from_data_set(self.attributes.clone()));
            match scope.lookup_strings(tag) {
                Ok(v) => v,
                Err(_) => return false,
            }
        } else {
            element_string_values(element)
        };
        query_values_match(element.vr(), requested, &values, query_tz, candidate_tz)
    }
}
fn query_timezone_offset(query: &ParsedQuery) -> String {
    match query.keys.get(&TAG_TIMEZONE_OFFSET_FROM_UTC) {
        Some(k) if k.present && k.values.len() == 1 => k.values[0].clone(),
        _ => String::new(),
    }
}
fn query_values_match(
    vr: Vr,
    requested: &[String],
    candidates: &[String],
    query_tz: &str,
    candidate_tz: &str,
) -> bool {
    requested.iter().any(|r| {
        candidates
            .iter()
            .any(|c| query_value_match_with_timezone(vr, r, c, query_tz, candidate_tz))
    })
}
fn query_value_match_with_timezone(
    vr: Vr,
    request: &str,
    candidate: &str,
    query_tz: &str,
    candidate_tz: &str,
) -> bool {
    if vr == Vr::DA || vr == Vr::DT || vr == Vr::TM {
        let Some(cand) = parse_temporal_value(vr, candidate, candidate_tz) else {
            return false;
        };
        if let Some((lower, upper)) = parse_temporal_range(vr, request, query_tz) {
            if let (Some(l), Some(u)) = (&lower, &upper)
                && l.start > u.end
            {
                return false;
            }
            return lower.as_ref().is_none_or(|l| cand.end >= l.start)
                && upper.as_ref().is_none_or(|u| cand.start <= u.end);
        }
        let Some(req) = parse_temporal_value(vr, request, query_tz) else {
            return false;
        };
        return cand.end >= req.start && cand.start <= req.end;
    }
    if request.contains(['*', '?']) && vr != Vr::UI {
        return wildcard_match(request, candidate);
    }
    if vr == Vr::CS {
        return simple_fold_eq(request, candidate);
    }
    request == candidate
}
fn simple_fold_eq(a: &str, b: &str) -> bool {
    let mut x = a.chars();
    let mut y = b.chars();
    loop {
        match (x.next(), y.next()) {
            (None, None) => return true,
            (Some(c), Some(d)) => {
                if c == d {
                    continue;
                }
                if !simple_fold_equal(c, d) {
                    return false;
                }
            }
            _ => return false,
        }
    }
}
fn simple_fold_equal(c: char, d: char) -> bool {
    let lower = |c: char| c.to_lowercase().collect::<String>();
    let upper = |c: char| c.to_uppercase().collect::<String>();
    lower(c) == lower(d) || upper(c) == upper(d)
}
#[derive(Debug, Clone, Copy)]
pub(crate) struct TemporalValue {
    start: i128,
    end: i128,
}
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}
fn instant(z: &ZonedTime) -> i128 {
    let (y, m, d) = z.date();
    let secs = days_from_civil(y, m, d) as i128 * 86_400
        + (z.hour() * 3600 + z.minute() * 60 + z.second()) as i128
        - z.offset() as i128;
    secs * 1_000_000_000 + z.nanosecond() as i128
}
fn fields(z: &ZonedTime) -> (i64, i64, i64, i64, i64, i64, i64, i64) {
    let (y, m, d) = z.date();
    (
        y,
        m,
        d,
        z.hour(),
        z.minute(),
        z.second(),
        z.nanosecond(),
        z.offset(),
    )
}
fn add_date(z: &ZonedTime, years: i64, months: i64, days: i64) -> ZonedTime {
    let (y, m, d, h, mi, s, ns, off) = fields(z);
    ZonedTime::from_fields(y + years, m + months, d + days, h, mi, s, ns, off)
}
fn add_nanos(z: &ZonedTime, nanos: i64) -> ZonedTime {
    let (y, m, d, h, mi, s, ns, off) = fields(z);
    let total = ns as i128 + nanos as i128;
    let secs = total.div_euclid(1_000_000_000) as i64;
    let rest = total.rem_euclid(1_000_000_000) as i64;
    ZonedTime::from_fields(y, m, d, h, mi, s + secs, rest, off)
}
pub(crate) fn parse_temporal_value(vr: Vr, value: &str, default_tz: &str) -> Option<TemporalValue> {
    let (start, precision) = match vr {
        Vr::DA => {
            let p = dcmtime::parse_date(value).ok()?;
            if p.is_nema {
                return None;
            }
            (p.time, p.precision)
        }
        Vr::TM => {
            let p = dcmtime::parse_time(value).ok()?;
            let mut start = p.time;
            if !default_tz.is_empty() {
                let offset = timezone_offset_seconds(default_tz)?;
                start = add_nanos(&start, -offset * 1_000_000_000);
            }
            (start, p.precision)
        }
        Vr::DT => {
            let p = dcmtime::parse_datetime(value).ok()?;
            let mut start = p.time;
            if p.no_offset && !default_tz.is_empty() {
                let offset = timezone_offset_seconds(default_tz)?;
                let (y, m, d, h, mi, s, ns, _) = fields(&start);
                start = ZonedTime::from_fields(y, m, d, h, mi, s, ns, offset);
            }
            (start, p.precision)
        }
        _ => return None,
    };
    let end = temporal_precision_end(&start, precision, vr);
    Some(TemporalValue {
        start: instant(&start),
        end,
    })
}
pub(crate) fn parse_temporal_range(
    vr: Vr,
    value: &str,
    default_tz: &str,
) -> Option<(Option<TemporalValue>, Option<TemporalValue>)> {
    for (i, b) in value.bytes().enumerate() {
        if b != b'-' {
            continue;
        }
        let (left, right) = (&value[..i], &value[i + 1..]);
        let mut lower = None;
        let mut upper = None;
        if !left.is_empty() {
            match parse_temporal_value(vr, left, default_tz) {
                Some(p) => lower = Some(p),
                None => continue,
            }
        }
        if !right.is_empty() {
            match parse_temporal_value(vr, right, default_tz) {
                Some(p) => upper = Some(p),
                None => continue,
            }
        }
        if lower.is_some() || upper.is_some() {
            return Some((lower, upper));
        }
    }
    None
}
fn timezone_offset_seconds(value: &str) -> Option<i64> {
    if !valid_ups_timezone_offset(value) {
        return None;
    }
    let b = value.as_bytes();
    let d = |i: usize| (b[i] - b'0') as i64;
    let offset = (d(1) * 10 + d(2)) * 3600 + (d(3) * 10 + d(4)) * 60;
    Some(if b[0] == b'-' { -offset } else { offset })
}
fn temporal_precision_end(start: &ZonedTime, precision: PrecisionLevel, vr: Vr) -> i128 {
    if vr == Vr::DA && (precision == PrecisionLevel::DAY || precision == PrecisionLevel::FULL) {
        return instant(&add_date(start, 0, 0, 1)) - 1;
    }
    const MS: i64 = 1_000_000;
    let exclusive = match precision {
        PrecisionLevel::YEAR => add_date(start, 1, 0, 0),
        PrecisionLevel::MONTH => add_date(start, 0, 1, 0),
        PrecisionLevel::DAY => add_date(start, 0, 0, 1),
        PrecisionLevel::HOURS => add_nanos(start, 3600 * 1_000_000_000),
        PrecisionLevel::MINUTES => add_nanos(start, 60 * 1_000_000_000),
        PrecisionLevel::SECONDS => add_nanos(start, 1_000_000_000),
        PrecisionLevel::MS1 => add_nanos(start, 100 * MS),
        PrecisionLevel::MS2 => add_nanos(start, 10 * MS),
        PrecisionLevel::MS3 => add_nanos(start, MS),
        PrecisionLevel::MS4 => add_nanos(start, 100_000),
        PrecisionLevel::MS5 => add_nanos(start, 10_000),
        _ => add_nanos(start, 1_000),
    };
    instant(&exclusive) - 1
}
fn wildcard_match(pattern: &str, value: &str) -> bool {
    let value: Vec<char> = value.chars().collect();
    let mut previous = vec![false; value.len() + 1];
    previous[0] = true;
    for token in pattern.chars() {
        let mut next = vec![false; value.len() + 1];
        if token == '*' {
            next[0] = previous[0];
            for i in 0..value.len() {
                next[i + 1] = next[i] || previous[i + 1];
            }
        } else {
            for (i, c) in value.iter().enumerate() {
                next[i + 1] = previous[i] && (token == '?' || token == *c);
            }
        }
        previous = next;
    }
    previous[value.len()]
}
pub(super) fn project_query_result(query: &ParsedQuery, step: &Step) -> (Object, bool) {
    let mut elements: Vec<Element> = Vec::with_capacity(query.order.len() + 2);
    let mut warning = false;
    let mut temporal = false;
    for &tag in &query.order {
        if tag == TAG_SPECIFIC_CHARACTER_SET || tag == TAG_TIMEZONE_OFFSET_FROM_UTC {
            continue;
        }
        let Some(e) = data_set_element(&step.attributes, tag) else {
            warning = true;
            continue;
        };
        elements.push(e.clone());
        if matches!(e.vr(), Vr::DA | Vr::DT | Vr::TM) {
            temporal = true;
        }
    }
    if query_elements_need_character_set(&elements)
        && let Some(cs) = data_set_element(&step.attributes, TAG_SPECIFIC_CHARACTER_SET)
        && !contains_tag(&elements, TAG_SPECIFIC_CHARACTER_SET)
    {
        elements.insert(0, cs.clone());
    }
    if temporal
        && let Some(tz) = data_set_element(&step.attributes, TAG_TIMEZONE_OFFSET_FROM_UTC)
        && !contains_tag(&elements, TAG_TIMEZONE_OFFSET_FROM_UTC)
    {
        elements.insert(0, tz.clone());
    }
    (new_data_set(elements), warning)
}
pub(crate) fn with_query_character_set(mut elements: Vec<Element>) -> Vec<Element> {
    if !contains_tag(&elements, TAG_SPECIFIC_CHARACTER_SET)
        && query_elements_need_character_set(&elements)
    {
        elements.insert(
            0,
            string_element(TAG_SPECIFIC_CHARACTER_SET, Vr::CS, &["ISO_IR 192"]),
        );
    }
    elements
}
fn query_elements_need_character_set(elements: &[Element]) -> bool {
    elements.iter().any(|e| {
        if let Some(raw) = e.raw()
            && raw.iter().any(|b| *b >= 0x80 || *b == 0x1b)
        {
            return true;
        }
        element_string_values(e).iter().any(|v| !v.is_ascii())
    })
}
fn contains_tag(elements: &[Element], tag: Tag) -> bool {
    elements.iter().any(|e| e.tag() == tag)
}
pub(super) fn is_query_tag(tag: Tag) -> bool {
    QUERY_TAGS.contains(&tag)
}
pub(crate) fn query_entry(tag: Tag) -> Option<Vr> {
    if !QUERY_TAGS.contains(&tag) {
        return None;
    }
    dict::by_tag(tag).map(|e| e.vr)
}
const QUERY_TAGS: [Tag; 26] = [
    TAG_SPECIFIC_CHARACTER_SET,
    TAG_TIMEZONE_OFFSET_FROM_UTC,
    TAG_SOP_CLASS_UID,
    TAG_SOP_INSTANCE_UID,
    TAG_STUDY_INSTANCE_UID,
    TAG_PATIENT_NAME,
    TAG_PATIENT_ID,
    TAG_ISSUER_OF_PATIENT_ID,
    TAG_PATIENT_BIRTH_DATE,
    TAG_PATIENT_SEX,
    TAG_ADMISSION_ID,
    TAG_SCHEDULED_PROCEDURE_STEP_PRIORITY,
    TAG_PROCEDURE_STEP_LABEL,
    TAG_WORKLIST_LABEL,
    TAG_SCHEDULED_STATION_NAME_CODE_SEQUENCE,
    TAG_SCHEDULED_STATION_CLASS_CODE_SEQUENCE,
    TAG_SCHEDULED_STATION_GEOGRAPHIC_LOCATION_CODE_SEQUENCE,
    TAG_SCHEDULED_HUMAN_PERFORMERS_SEQUENCE,
    TAG_SCHEDULED_PROCEDURE_STEP_START_DATE_TIME,
    TAG_EXPECTED_COMPLETION_DATE_TIME,
    TAG_SCHEDULED_WORKITEM_CODE_SEQUENCE,
    TAG_INPUT_READINESS_STATE,
    TAG_SCHEDULED_PROCEDURE_STEP_MODIFICATION_DATE_TIME,
    TAG_PROCEDURE_STEP_STATE,
    TAG_PROCEDURE_STEP_PROGRESS_INFORMATION_SEQUENCE,
    TAG_UNIFIED_PROCEDURE_STEP_PERFORMED_PROCEDURE_SEQUENCE,
];
#[derive(Debug, Clone)]
pub struct QueryMatch<'a> {
    pub status: u16,
    pub identifier: &'a Object,
}
pub(crate) type QueryYield<'a> = dyn FnMut(QueryMatch<'_>) -> dimse::Result<()> + 'a;
pub struct QueryClient<'a> {
    client: StreamingCFindClient<'a>,
}
impl<'a> QueryClient<'a> {
    pub fn new(assoc: &'a Association, sop_class_uid: &str) -> Result<QueryClient<'a>> {
        if ![PULL_SOP_CLASS_UID, WATCH_SOP_CLASS_UID, QUERY_SOP_CLASS_UID].contains(&sop_class_uid)
        {
            return Err(Kind::QueryIdentifier.into());
        }
        let client = StreamingCFindClient::new(assoc, sop_class_uid, sop_class_uid)?;
        Ok(QueryClient { client })
    }
    pub fn find(
        &self,
        ctx: &Ctx,
        query: &Query,
        yield_: Option<&mut QueryYield<'_>>,
    ) -> (StreamingCFindResult, Result<()>) {
        let mut options = StreamingCFindClientOptions::default();
        options.operation.context = Some(ctx.clone());
        self.find_with_options(options, query, yield_)
    }
    pub fn find_with_options(
        &self,
        options: StreamingCFindClientOptions,
        query: &Query,
        yield_: Option<&mut QueryYield<'_>>,
    ) -> (StreamingCFindResult, Result<()>) {
        let Some(yield_) = yield_ else {
            return (
                StreamingCFindResult::default(),
                Err(Kind::QueryIdentifier.into()),
            );
        };
        let identifier = match build_query_identifier(query) {
            Ok(i) => i,
            Err(e) => return (StreamingCFindResult::default(), Err(e)),
        };
        let mut callback =
            |status: u16, identifier: &Object| yield_(QueryMatch { status, identifier });
        let (result, r) =
            self.client
                .find_with_options(options, Some(&identifier), Some(&mut callback));
        (result, r.map_err(Error::Dimse))
    }
}
