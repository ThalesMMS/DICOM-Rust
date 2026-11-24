use super::{SearchRequest, ServerError};
use crate::object::{Element, ElementHeader, Object, Tag, Value, Vr};
use crate::personname::fuzzy::FuzzyAlgorithm;
use crate::qrmatch;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
#[derive(Clone, Debug, Default)]
pub struct SearchMatcherOptions {
    pub limits: qrmatch::Limits,
    pub supported_matching: Vec<Tag>,
    pub lenient_dates: bool,
    pub fuzzy_person_names: Option<FuzzyAlgorithm>,
    pub cancel: Option<Arc<AtomicBool>>,
}
#[derive(Clone, Debug)]
pub struct SearchMatcher {
    identifier: Object,
    options: qrmatch::Options,
    fuzzy: bool,
}
const STUDY_INSTANCE_UID: Tag = Tag {
    group: 0x0020,
    element: 0x000d,
};
const SERIES_INSTANCE_UID: Tag = Tag {
    group: 0x0020,
    element: 0x000e,
};
fn search_filter_tag(key: &str) -> Option<(Tag, Vr)> {
    if key.len() == 8
        && let Ok(tag) = crate::dicomjson::parse_core_tag(key)
    {
        return Some((
            tag,
            crate::object::dict::by_tag(tag).map_or(Vr::UN, |e| e.vr),
        ));
    }
    let (tag, keyword) = crate::object::dict::by_keyword(key)?;
    if keyword != key {
        return None;
    }
    let vr = crate::object::dict::by_tag(tag)?.vr;
    Some((tag, vr))
}
fn search_element(tag: Tag, vr: Vr, mut values: Vec<String>) -> Element {
    if values.is_empty() {
        values.push(String::new());
    }
    Element {
        header: ElementHeader {
            tag,
            vr,
            length: 0,
            length_set: false,
        },
        value: Value::Strings(values),
    }
}
impl SearchMatcher {
    pub fn new(
        req: &SearchRequest,
        options: SearchMatcherOptions,
    ) -> Result<SearchMatcher, ServerError> {
        let mut elements: Vec<Element> = Vec::with_capacity(req.filters.len() + 2);
        if !req.study_instance_uid.is_empty() {
            elements.push(search_element(
                STUDY_INSTANCE_UID,
                Vr::UI,
                vec![req.study_instance_uid.clone()],
            ));
        }
        if !req.series_instance_uid.is_empty() {
            elements.push(search_element(
                SERIES_INSTANCE_UID,
                Vr::UI,
                vec![req.series_instance_uid.clone()],
            ));
        }
        for filter in &req.filters {
            let (tag, vr) = search_filter_tag(&filter.key).ok_or(ServerError::InvalidRequest)?;
            let mut values: Vec<String> = Vec::new();
            for value in &filter.values {
                if vr == Vr::UI {
                    values.extend(
                        value
                            .split([',', '\\'])
                            .filter(|s| !s.is_empty())
                            .map(str::to_string),
                    );
                    continue;
                }
                values.push(value.clone());
            }
            elements.push(search_element(tag, vr, values));
        }
        let mut matcher = SearchMatcher {
            identifier: Object::from_elements(elements),
            options: qrmatch::Options {
                limits: options.limits,
                supported_matching: options.supported_matching.clone(),
                lenient_date_queries: options.lenient_dates,
                cancel: options.cancel.clone(),
                ..qrmatch::Options::default()
            },
            fuzzy: false,
        };
        if req.fuzzy_matching && options.fuzzy_person_names.is_some() {
            matcher.options.fuzzy_person_names = options.fuzzy_person_names;
            matcher.fuzzy = true;
        }
        Ok(matcher)
    }
    pub fn identifier(&self) -> &Object {
        &self.identifier
    }
    pub fn fuzzy_matching_applied(&self) -> bool {
        self.fuzzy
    }
    pub fn matches(&self, candidate: Option<&Object>) -> Result<bool, ServerError> {
        let query = qrmatch::elements_from(self.identifier.elements());
        let candidate = candidate.map(|c| qrmatch::elements_from(c.elements()));
        match qrmatch::match_identifier(Some(&query), candidate.as_deref(), &self.options) {
            Ok(result) => Ok(result.matched),
            Err(qrmatch::Error::InvalidQuery(_)) => Err(ServerError::InvalidRequest),
            Err(e) => Err(ServerError::Other(e.to_string())),
        }
    }
}
