#![forbid(unsafe_code)]
mod object;
#[rustfmt::skip]
mod tables;
mod temporal;
mod text;
use crate::object::{Tag, Vr};
pub use crate::personname::fuzzy::FuzzyAlgorithm;
pub use object::{Element, Value, elements_from};
use object::{Object, SPECIFIC_CHARACTER_SET, string_values};
use std::collections::HashSet;
use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tables::MULTI_VALUED;
pub use temporal::{TimezoneOffset, TimezoneOffsetError, parse_timezone_offset};
use text::{all_values_empty, equal_fold, trim_text_value_bytes, value_rune_len};
const QUERY_RETRIEVE_LEVEL: Tag = Tag::new(0x0008, 0x0052);
const RETRIEVE_AE_TITLE: Tag = Tag::new(0x0008, 0x0054);
const TIMEZONE_OFFSET_FROM_UTC: Tag = Tag::new(0x0008, 0x0201);
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    ResourceLimit,
    Canceled,
    InvalidQuery(String),
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::ResourceLimit => f.write_str("dicom qrmatch: resource limit exceeded"),
            Error::Canceled => f.write_str("dicom qrmatch: canceled"),
            Error::InvalidQuery(detail) => write!(f, "dicom qrmatch: invalid query: {detail}"),
        }
    }
}
impl std::error::Error for Error {}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Limits {
    pub max_values: usize,
    pub max_sequence_items: usize,
    pub max_sequence_depth: usize,
    pub max_wildcard_steps: usize,
    pub max_value_bytes: usize,
}
impl Limits {
    pub fn defaults() -> Self {
        Limits {
            max_values: 128,
            max_sequence_items: 64,
            max_sequence_depth: 8,
            max_wildcard_steps: 250_000,
            max_value_bytes: 16_384,
        }
    }
    fn normalized(self) -> Self {
        let d = Self::defaults();
        let or = |v: usize, default: usize| if v == 0 { default } else { v };
        Limits {
            max_values: or(self.max_values, d.max_values),
            max_sequence_items: or(self.max_sequence_items, d.max_sequence_items),
            max_sequence_depth: or(self.max_sequence_depth, d.max_sequence_depth),
            max_wildcard_steps: or(self.max_wildcard_steps, d.max_wildcard_steps),
            max_value_bytes: or(self.max_value_bytes, d.max_value_bytes),
        }
    }
}
#[derive(Debug, Clone, Default)]
pub struct Options {
    pub limits: Limits,
    pub supported_matching: Vec<Tag>,
    pub lenient_date_queries: bool,
    pub fuzzy_person_names: Option<FuzzyAlgorithm>,
    pub combined_date_time: bool,
    pub timezone_adjustment: bool,
    pub candidate_timezone: Option<i32>,
    pub empty_value_matching: bool,
    pub multiple_value_matching: bool,
    pub cancel: Option<Arc<AtomicBool>>,
}
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MatchResult {
    pub matched: bool,
    pub unsupported_optional: Vec<Tag>,
}
pub fn wildcard_allowed(vr: Vr) -> bool {
    use Vr::*;
    matches!(vr, AE | CS | LO | LT | PN | SH | ST | UC | UR | UT)
}
pub fn match_identifier(
    query: Option<&[Element]>,
    candidate: Option<&[Element]>,
    options: &Options,
) -> Result<MatchResult, Error> {
    let Some(query) = query else {
        return Err(Error::InvalidQuery("nil query".to_string()));
    };
    let Some(candidate) = candidate else {
        return Ok(MatchResult::default());
    };
    let query = Object::new(query, None);
    let candidate = Object::new(candidate, None);
    let mut state = State {
        cancel: options.cancel.as_deref(),
        limits: options.limits.normalized(),
        supported: None,
        wildcard_steps: 0,
        unsupported: Vec::new(),
        lenient_dates: options.lenient_date_queries,
        fuzzy_names: options.fuzzy_person_names,
        combined_date_time: options.combined_date_time,
        query_zone: None,
        candidate_zone: None,
        empty_value_matching: options.empty_value_matching,
        multiple_value_matching: options.multiple_value_matching,
    };
    if options.timezone_adjustment {
        let (query_zone, candidate_zone) =
            timezone_adjustment(&query, &candidate, options.candidate_timezone)?;
        state.query_zone = query_zone;
        state.candidate_zone = candidate_zone;
    }
    if !options.supported_matching.is_empty() {
        state.supported = Some(options.supported_matching.iter().copied().collect());
    }
    state.check_canceled()?;
    let matched = state.match_object(&query, &candidate, 0)?;
    Ok(MatchResult {
        matched,
        unsupported_optional: state.unsupported,
    })
}
fn timezone_adjustment(
    query: &Object<'_>,
    candidate: &Object<'_>,
    fallback: Option<i32>,
) -> Result<(Option<i32>, Option<i32>), Error> {
    let Some(query_offset) = query.get_string(TIMEZONE_OFFSET_FROM_UTC) else {
        return Ok((None, None));
    };
    if query_offset.trim().is_empty() {
        return Ok((None, None));
    }
    let query_zone = parse_timezone_offset(&query_offset)
        .map_err(|e| Error::InvalidQuery(e.to_string()))?
        .seconds;
    let mut candidate_zone = fallback;
    if let Some(offset) = candidate.get_string(TIMEZONE_OFFSET_FROM_UTC)
        && let Ok(zone) = parse_timezone_offset(&offset)
    {
        candidate_zone = Some(zone.seconds);
    }
    Ok((Some(query_zone), Some(candidate_zone.unwrap_or(query_zone))))
}
enum ValuesError {
    Undecodable,
    Failed(Error),
}
impl From<Error> for ValuesError {
    fn from(e: Error) -> Self {
        ValuesError::Failed(e)
    }
}
struct State<'c> {
    cancel: Option<&'c AtomicBool>,
    limits: Limits,
    supported: Option<HashSet<Tag>>,
    wildcard_steps: usize,
    unsupported: Vec<Tag>,
    lenient_dates: bool,
    fuzzy_names: Option<FuzzyAlgorithm>,
    combined_date_time: bool,
    query_zone: Option<i32>,
    candidate_zone: Option<i32>,
    empty_value_matching: bool,
    multiple_value_matching: bool,
}
fn is_control_tag(tag: Tag) -> bool {
    tag == QUERY_RETRIEVE_LEVEL || tag == RETRIEVE_AE_TITLE || tag == SPECIFIC_CHARACTER_SET
}
fn normalize_query_value(vr: Vr, value: &str) -> &str {
    match vr {
        Vr::UI => value.trim_end_matches([' ', '\0']),
        Vr::PN => value.trim().trim_end_matches(['^', ' ']),
        _ => value.trim().trim_end_matches(' '),
    }
}
fn is_empty_matching_key(vr: Vr, values: &[String]) -> bool {
    use Vr::*;
    values.len() == 1
        && values[0] == "\"\""
        && matches!(
            vr,
            AE | CS | DA | DT | LO | LT | PN | SH | ST | TM | UC | UR | UT
        )
}
fn multiple_value_matching_key(element: &Element) -> bool {
    use Vr::*;
    if !matches!(element.vr, AE | AS | AT | CS | LO | PN | SH | UC) {
        return false;
    }
    let key = element.tag.key();
    match MULTI_VALUED.binary_search_by_key(&key, |&(tag, _)| tag) {
        Ok(i) => MULTI_VALUED[i].1 == element.vr.as_str(),
        Err(_) => false,
    }
}
fn validate_multiple_matching_values(element: &Element, values: &[String]) -> Result<(), Error> {
    for value in values {
        if normalize_query_value(element.vr, value).is_empty() || value.contains(['*', '?']) {
            return Err(Error::InvalidQuery(format!(
                "empty or wildcard value in multiple value matching key {}",
                element.tag
            )));
        }
    }
    Ok(())
}
const COMBINED_DATE_TIME_PAIRS: [(Tag, Tag); 8] = [
    (Tag::new(0x0008, 0x0012), Tag::new(0x0008, 0x0013)),
    (Tag::new(0x0008, 0x0020), Tag::new(0x0008, 0x0030)),
    (Tag::new(0x0008, 0x0021), Tag::new(0x0008, 0x0031)),
    (Tag::new(0x0008, 0x0022), Tag::new(0x0008, 0x0032)),
    (Tag::new(0x0008, 0x0023), Tag::new(0x0008, 0x0033)),
    (Tag::new(0x0010, 0x0030), Tag::new(0x0010, 0x0032)),
    (Tag::new(0x0040, 0x0002), Tag::new(0x0040, 0x0003)),
    (Tag::new(0x0040, 0x0244), Tag::new(0x0040, 0x0245)),
];
impl State<'_> {
    fn check_canceled(&self) -> Result<(), Error> {
        match self.cancel {
            Some(flag) if flag.load(Ordering::Relaxed) => Err(Error::Canceled),
            _ => Ok(()),
        }
    }
    fn is_supported(&self, tag: Tag) -> bool {
        self.supported.as_ref().is_none_or(|s| s.contains(&tag))
    }
    fn skip_unsupported(&mut self, tag: Tag) -> bool {
        if self.is_supported(tag) {
            return false;
        }
        if !self.unsupported.contains(&tag) {
            self.unsupported.push(tag);
        }
        true
    }
    fn check_values(&self, values: &[String]) -> Result<(), Error> {
        if values.len() > self.limits.max_values
            || value_rune_len(values) > self.limits.max_value_bytes
        {
            return Err(Error::ResourceLimit);
        }
        Ok(())
    }
    fn check_character_set_limit(&self, owner: &Object<'_>) -> Result<(), Error> {
        let Some(element) = owner.get(SPECIFIC_CHARACTER_SET) else {
            return Ok(());
        };
        match &element.value {
            Value::Raw(raw) if raw.len() > self.limits.max_value_bytes => Err(Error::ResourceLimit),
            Value::Strings(codes) => {
                if codes.len() > self.limits.max_values {
                    return Err(Error::ResourceLimit);
                }
                let mut remaining = self.limits.max_value_bytes;
                for code in codes {
                    if code.len() > remaining {
                        return Err(Error::ResourceLimit);
                    }
                    remaining -= code.len();
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }
    fn check_tag_value_length(&self, length: u32) -> Result<(), Error> {
        let length = u64::from(length);
        if length > self.limits.max_value_bytes as u64 || length / 4 > self.limits.max_values as u64
        {
            return Err(Error::ResourceLimit);
        }
        Ok(())
    }
    fn matching_values(
        &self,
        owner: Option<&Object<'_>>,
        element: &Element,
    ) -> Result<Vec<String>, ValuesError> {
        if let Value::Raw(raw) = &element.value
            && text::is_string_like(element.vr)
        {
            if raw.len() > self.limits.max_value_bytes {
                return Err(Error::ResourceLimit.into());
            }
            if let Some(owner) = owner
                && text::uses_specific_character_set(element.vr)
            {
                if trim_text_value_bytes(element.vr, raw).is_empty() {
                    return Ok(Vec::new());
                }
                self.check_character_set_limit(owner)?;
                let values = owner
                    .decode_text_values(element)
                    .map_err(|()| ValuesError::Undecodable)?;
                self.check_values(&values)?;
                return Ok(values);
            }
        }
        let values = string_values(element);
        self.check_values(&values)?;
        Ok(values)
    }
    fn match_object(
        &mut self,
        query: &Object<'_>,
        candidate: &Object<'_>,
        depth: usize,
    ) -> Result<bool, Error> {
        if depth > self.limits.max_sequence_depth {
            return Err(Error::ResourceLimit);
        }
        self.check_canceled()?;
        let (combined, mut matched) = self.match_combined_date_time(query, candidate)?;
        for &element in query.elements() {
            self.check_canceled()?;
            if is_control_tag(element.tag) || combined.contains(&element.tag) {
                continue;
            }
            if element.tag == TIMEZONE_OFFSET_FROM_UTC && self.query_zone.is_some() {
                continue;
            }
            if !self.match_query_element(query, element, candidate, depth)? {
                matched = false;
            }
        }
        Ok(matched)
    }
    fn match_query_element(
        &mut self,
        query: &Object<'_>,
        element: &Element,
        candidate: &Object<'_>,
        depth: usize,
    ) -> Result<bool, Error> {
        if element.vr == Vr::SQ {
            return self.match_sequence(query, element, candidate, depth);
        }
        if self.multiple_value_matching
            && element.vr == Vr::AT
            && multiple_value_matching_key(element)
        {
            return self.match_attribute_tags(query, element, candidate);
        }
        let query_scope = self.is_supported(element.tag).then_some(query);
        let query_values = match self.matching_values(query_scope, element) {
            Ok(values) => values,
            Err(ValuesError::Undecodable) => {
                return Err(Error::InvalidQuery(format!(
                    "cannot decode matching text for {}",
                    element.tag
                )));
            }
            Err(ValuesError::Failed(e)) => return Err(e),
        };
        let multiple = self.multiple_value_matching
            && query_values.len() > 1
            && multiple_value_matching_key(element);
        if !multiple && all_values_empty(&query_values) {
            return Ok(true);
        }
        if self.skip_unsupported(element.tag) {
            return Ok(true);
        }
        if multiple {
            validate_multiple_matching_values(element, &query_values)?;
        }
        let empty = self.empty_value_matching && is_empty_matching_key(element.vr, &query_values);
        let Some(candidate_element) = candidate.get(element.tag) else {
            return Ok(empty);
        };
        let candidate_values = match self.matching_values(Some(candidate), candidate_element) {
            Ok(values) => values,
            Err(ValuesError::Undecodable) => return Ok(false),
            Err(ValuesError::Failed(e)) => return Err(e),
        };
        if empty {
            return Ok(candidate_values.is_empty()
                || candidate_values.len() == 1 && candidate_values[0].is_empty());
        }
        self.match_values(element.vr, &query_values, &candidate_values, multiple)
    }
    fn match_sequence(
        &mut self,
        query: &Object<'_>,
        element: &Element,
        candidate: &Object<'_>,
        depth: usize,
    ) -> Result<bool, Error> {
        let Value::Sequence(items) = &element.value else {
            return Ok(true);
        };
        if items.is_empty() || self.all_sequence_items_universal(items, depth + 1)? {
            return Ok(true);
        }
        if self.skip_unsupported(element.tag) {
            return Ok(true);
        }
        let Some(candidate_element) = candidate.get(element.tag) else {
            return Ok(false);
        };
        let Value::Sequence(candidate_items) = &candidate_element.value else {
            return Ok(false);
        };
        if candidate_items.is_empty() {
            return Ok(false);
        }
        if candidate_items.len() > self.limits.max_sequence_items {
            return Err(Error::ResourceLimit);
        }
        self.check_character_set_limit(query)?;
        self.check_character_set_limit(candidate)?;
        let query_items = query.get_sequence(element.tag);
        let candidate_items = candidate.get_sequence(element.tag);
        for query_item in &query_items {
            let mut item_matched = false;
            for candidate_item in &candidate_items {
                if self.match_object(query_item, candidate_item, depth + 1)? {
                    item_matched = true;
                    break;
                }
            }
            if !item_matched {
                return Ok(false);
            }
        }
        Ok(true)
    }
    fn match_values(
        &mut self,
        vr: Vr,
        query_values: &[String],
        candidate_values: &[String],
        require_all: bool,
    ) -> Result<bool, Error> {
        if vr == Vr::UI && query_values.len() > 1 {
            let wanted: HashSet<&str> = query_values
                .iter()
                .map(|q| normalize_query_value(Vr::UI, q))
                .collect();
            return Ok(candidate_values
                .iter()
                .any(|c| wanted.contains(normalize_query_value(Vr::UI, c))));
        }
        'queries: for query in query_values {
            self.check_canceled()?;
            let query = normalize_query_value(vr, query);
            if query.is_empty() {
                continue;
            }
            for candidate in candidate_values {
                if self.match_one_value(vr, query, normalize_query_value(vr, candidate))? {
                    if require_all {
                        continue 'queries;
                    }
                    return Ok(true);
                }
            }
            if require_all {
                return Ok(false);
            }
        }
        Ok(require_all)
    }
    fn match_one_value(&mut self, vr: Vr, query: &str, candidate: &str) -> Result<bool, Error> {
        match vr {
            Vr::DT => {
                return temporal::match_temporal_in(
                    vr,
                    query,
                    candidate,
                    self.lenient_dates,
                    self.query_zone,
                    self.candidate_zone,
                );
            }
            Vr::DA | Vr::TM => {
                return temporal::match_temporal_in(
                    vr,
                    query,
                    candidate,
                    self.lenient_dates,
                    None,
                    None,
                );
            }
            _ => {}
        }
        if wildcard_allowed(vr) && query.contains(['*', '?']) {
            let (pattern, value) = if vr == Vr::PN {
                (
                    crate::textfmt::to_upper(query),
                    crate::textfmt::to_upper(candidate),
                )
            } else {
                (query.to_string(), candidate.to_string())
            };
            let pattern: Vec<char> = pattern.chars().collect();
            let value: Vec<char> = value.chars().collect();
            return self.match_wildcard(&pattern, &value);
        }
        if vr == Vr::PN {
            if equal_fold(query, candidate) {
                return Ok(true);
            }
            return Ok(self
                .fuzzy_names
                .is_some_and(|f| crate::personname::fuzzy::matches(f, query, candidate)));
        }
        Ok(query == candidate)
    }
    fn count_wildcard_step(&mut self) -> Result<(), Error> {
        self.wildcard_steps += 1;
        if self.wildcard_steps > self.limits.max_wildcard_steps {
            return Err(Error::ResourceLimit);
        }
        Ok(())
    }
    fn match_wildcard(&mut self, pattern: &[char], value: &[char]) -> Result<bool, Error> {
        let (mut p, mut v) = (0, 0);
        let mut star: Option<usize> = None;
        let mut star_value = 0;
        while v < value.len() {
            self.count_wildcard_step()?;
            if p < pattern.len() && (pattern[p] == '?' || pattern[p] == value[v]) {
                p += 1;
                v += 1;
                continue;
            }
            if p < pattern.len() && pattern[p] == '*' {
                star = Some(p);
                star_value = v;
                p += 1;
                continue;
            }
            let Some(s) = star else {
                return Ok(false);
            };
            p = s + 1;
            star_value += 1;
            v = star_value;
        }
        while p < pattern.len() && pattern[p] == '*' {
            self.count_wildcard_step()?;
            p += 1;
        }
        Ok(p == pattern.len())
    }
    fn all_sequence_items_universal(
        &mut self,
        items: &[Vec<Element>],
        depth: usize,
    ) -> Result<bool, Error> {
        if depth > self.limits.max_sequence_depth || items.len() > self.limits.max_sequence_items {
            return Err(Error::ResourceLimit);
        }
        for item in items {
            for element in item {
                self.check_canceled()?;
                if is_control_tag(element.tag) {
                    continue;
                }
                if self.multiple_value_matching
                    && element.vr == Vr::AT
                    && multiple_value_matching_key(element)
                    && element.calculated_length() != 0
                {
                    return Ok(false);
                }
                if element.vr == Vr::SQ {
                    if let Value::Sequence(nested) = &element.value
                        && !nested.is_empty()
                        && !self.all_sequence_items_universal(nested, depth + 1)?
                    {
                        return Ok(false);
                    }
                    continue;
                }
                let values = match self.matching_values(None, element) {
                    Ok(values) => values,
                    Err(ValuesError::Failed(e)) => return Err(e),
                    Err(ValuesError::Undecodable) => return Ok(false),
                };
                if !all_values_empty(&values)
                    || self.multiple_value_matching
                        && values.len() > 1
                        && multiple_value_matching_key(element)
                {
                    return Ok(false);
                }
            }
        }
        Ok(true)
    }
    fn match_attribute_tags(
        &mut self,
        query: &Object<'_>,
        element: &Element,
        candidate: &Object<'_>,
    ) -> Result<bool, Error> {
        let length = element.calculated_length();
        if length == 0 {
            return Ok(true);
        }
        if self.skip_unsupported(element.tag) {
            return Ok(true);
        }
        self.check_tag_value_length(length)?;
        let Some(wanted) = query.get_tags(element.tag) else {
            return Err(Error::InvalidQuery(format!(
                "invalid AT matching key {}",
                element.tag
            )));
        };
        let Some(value) = candidate.get(element.tag) else {
            return Ok(false);
        };
        if value.vr != Vr::AT {
            return Ok(false);
        }
        self.check_tag_value_length(value.calculated_length())?;
        let Some(values) = candidate.get_tags(element.tag) else {
            return Ok(false);
        };
        for tag in &wanted {
            self.check_canceled()?;
            if !values.contains(tag) {
                return Ok(false);
            }
        }
        Ok(true)
    }
    fn match_combined_date_time(
        &mut self,
        query: &Object<'_>,
        candidate: &Object<'_>,
    ) -> Result<(Vec<Tag>, bool), Error> {
        let mut handled = Vec::new();
        if !self.combined_date_time {
            return Ok((handled, true));
        }
        let mut matched = true;
        for (date_tag, time_tag) in COMBINED_DATE_TIME_PAIRS {
            let (Some(date_element), Some(time_element)) =
                (query.get(date_tag), query.get(time_tag))
            else {
                continue;
            };
            let date_values = string_values(date_element);
            let time_values = string_values(time_element);
            if date_values.len() != 1 || time_values.len() != 1 {
                continue;
            }
            if self.empty_value_matching
                && (is_empty_matching_key(Vr::DA, &date_values)
                    || is_empty_matching_key(Vr::TM, &time_values))
            {
                continue;
            }
            let date_query = date_values[0].trim();
            let time_query = time_values[0].trim();
            if date_query.is_empty()
                || time_query.is_empty()
                || date_query.contains(['*', '?'])
                || time_query.contains(['*', '?'])
            {
                continue;
            }
            if date_query.contains('-') != time_query.contains('-') {
                continue;
            }
            if !self.is_supported(date_tag) || !self.is_supported(time_tag) {
                continue;
            }
            handled.push(date_tag);
            handled.push(time_tag);
            if !self.match_date_time_pair(date_tag, time_tag, date_query, time_query, candidate)? {
                matched = false;
            }
        }
        Ok((handled, matched))
    }
    fn match_date_time_pair(
        &self,
        date_tag: Tag,
        time_tag: Tag,
        date_query: &str,
        time_query: &str,
        candidate: &Object<'_>,
    ) -> Result<bool, Error> {
        let (lower, upper) = temporal::combined_query_window(
            date_query,
            time_query,
            self.lenient_dates,
            self.query_zone.unwrap_or(0),
        )?;
        let Some(date_element) = candidate.get(date_tag) else {
            return Ok(false);
        };
        let mut candidate_time = String::new();
        if let Some(time_element) = candidate.get(time_tag) {
            for value in string_values(time_element) {
                let value = value.trim();
                if !value.is_empty() {
                    candidate_time = value.to_string();
                    break;
                }
            }
        }
        let candidate_dates = string_values(date_element);
        self.check_values(&candidate_dates)?;
        let zone = self.candidate_zone.unwrap_or(0);
        for value in &candidate_dates {
            let Some((start, end)) =
                temporal::candidate_date_time_window(value.trim(), &candidate_time, zone)
            else {
                continue;
            };
            if lower.as_ref().is_some_and(|l| end.before(l)) {
                continue;
            }
            if upper.as_ref().is_some_and(|u| start.after(u)) {
                continue;
            }
            return Ok(true);
        }
        Ok(false)
    }
}
#[cfg_attr(not(feature = "dicomweb"), allow(dead_code))]
pub(crate) fn simple_fold_char(r: char) -> char {
    match tables::SIMPLE_FOLD.binary_search_by_key(&(r as u32), |&(from, _)| from) {
        Ok(i) => char::from_u32(tables::SIMPLE_FOLD[i].1).unwrap_or(r),
        Err(_) => r,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn s(tag: Tag, vr: Vr, values: &[&str]) -> Element {
        Element::strings(tag, vr, values.iter().copied())
    }
    fn matched(query: &[Element], candidate: &[Element], options: &Options) -> bool {
        match_identifier(Some(query), Some(candidate), options)
            .unwrap()
            .matched
    }
    #[test]
    fn single_values_and_wildcards() {
        let o = Options::default();
        let pid = Tag::new(0x0010, 0x0020);
        let pn = Tag::new(0x0010, 0x0010);
        assert!(matched(
            &[s(pid, Vr::LO, &[""])],
            &[s(pid, Vr::LO, &["R001"])],
            &o
        ));
        assert!(!matched(
            &[s(pid, Vr::LO, &["r001"])],
            &[s(pid, Vr::LO, &["R001"])],
            &o
        ));
        assert!(matched(
            &[s(pn, Vr::PN, &["rec*"])],
            &[s(pn, Vr::PN, &["RECEIVE^P"])],
            &o
        ));
        assert!(matched(
            &[s(pn, Vr::PN, &["doe^john^^"])],
            &[s(pn, Vr::PN, &["DOE^JOHN"])],
            &o
        ));
    }
    #[test]
    fn date_ranges_and_errors() {
        let o = Options::default();
        let da = Tag::new(0x0008, 0x0020);
        assert!(matched(
            &[s(da, Vr::DA, &["20260101-"])],
            &[s(da, Vr::DA, &["20261231"])],
            &o
        ));
        assert!(!matched(
            &[s(da, Vr::DA, &["-20251231"])],
            &[s(da, Vr::DA, &["20260101"])],
            &o
        ));
        let err = match_identifier(
            Some(&[s(da, Vr::DA, &["2026"])]),
            Some(&[s(da, Vr::DA, &["20260604"])]),
            &o,
        )
        .unwrap_err();
        assert_eq!(
            err.to_string(),
            "dicom qrmatch: invalid query: invalid DA matching value"
        );
    }
    #[test]
    fn unsupported_keys_are_reported() {
        let o = Options {
            supported_matching: vec![Tag::new(0x0010, 0x0020)],
            ..Options::default()
        };
        let q = [
            s(Tag::new(0x0010, 0x0020), Vr::LO, &["R1"]),
            s(Tag::new(0x0032, 0x1060), Vr::LO, &["X"]),
        ];
        let r = match_identifier(
            Some(&q),
            Some(&[s(Tag::new(0x0010, 0x0020), Vr::LO, &["R1"])]),
            &o,
        )
        .unwrap();
        assert!(r.matched);
        assert_eq!(r.unsupported_optional, vec![Tag::new(0x0032, 0x1060)]);
    }
}
