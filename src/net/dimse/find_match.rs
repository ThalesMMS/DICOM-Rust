#![forbid(unsafe_code)]
use super::error::{Error, Result};
use super::find::{CFindRequestContext, TAG_QUERY_RETRIEVE_LEVEL, new_c_find_scp_error};
use super::find_extras::{normalize_query_retrieve_level, required_response_character_set};
use super::mwl::TAG_MWL_SPECIFIC_CHARACTER_SET;
use super::{
    C_FIND_STATUS_UNABLE_TO_PROCESS, QUERY_RETRIEVE_LEVEL_IMAGE, QUERY_RETRIEVE_LEVEL_SERIES,
    QUERY_RETRIEVE_LEVEL_STUDY,
};
use crate::net::ul::Ctx;
use crate::object::{Element, ElementHeader, Object, Tag, Value, Vr};
use crate::personname::fuzzy::FuzzyAlgorithm;
use crate::qrmatch;
use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
const TAG_RETRIEVE_AE_TITLE: Tag = Tag::new(0x0008, 0x0054);
const STUDY_KEYS: [Tag; 15] = [
    Tag::new(0x0010, 0x0010),
    Tag::new(0x0010, 0x0020),
    Tag::new(0x0010, 0x0030),
    Tag::new(0x0010, 0x0040),
    Tag::new(0x0008, 0x0020),
    Tag::new(0x0008, 0x0030),
    Tag::new(0x0008, 0x1030),
    Tag::new(0x0008, 0x0050),
    Tag::new(0x0008, 0x0090),
    Tag::new(0x0008, 0x0080),
    Tag::new(0x0032, 0x000A),
    Tag::new(0x0020, 0x0010),
    Tag::new(0x0020, 0x000D),
    Tag::new(0x0008, 0x0061),
    Tag::new(0x0008, 0x0060),
];
const SERIES_KEYS: [Tag; 5] = [
    Tag::new(0x0020, 0x000E),
    Tag::new(0x0020, 0x0011),
    Tag::new(0x0008, 0x103E),
    Tag::new(0x0008, 0x0021),
    Tag::new(0x0008, 0x0031),
];
const IMAGE_KEYS: [Tag; 3] = [
    Tag::new(0x0008, 0x0016),
    Tag::new(0x0008, 0x0018),
    Tag::new(0x0020, 0x0013),
];
pub fn study_root_find_matching_tags(level: &str) -> Vec<Tag> {
    let mut keys = STUDY_KEYS.to_vec();
    let level = normalize_query_retrieve_level(level);
    if level == QUERY_RETRIEVE_LEVEL_STUDY {
        keys
    } else if level == QUERY_RETRIEVE_LEVEL_SERIES {
        keys.extend(SERIES_KEYS);
        keys
    } else if level == QUERY_RETRIEVE_LEVEL_IMAGE {
        keys.extend(SERIES_KEYS);
        keys.extend(IMAGE_KEYS);
        keys
    } else {
        Vec::new()
    }
}
#[derive(Debug, Clone, Default)]
pub struct MemoryFindIndex {
    pub candidates: Vec<Object>,
    pub retrieve_ae_title: String,
    pub supported_matching: Vec<Tag>,
    pub limits: qrmatch::Limits,
    pub fuzzy_person_names: Option<FuzzyAlgorithm>,
    pub candidate_timezone: Option<i32>,
}
fn match_failure(err: qrmatch::Error) -> Error {
    if err == qrmatch::Error::Canceled {
        return Error::new(err.to_string());
    }
    let text = err.to_string();
    new_c_find_scp_error(
        C_FIND_STATUS_UNABLE_TO_PROCESS,
        &text,
        Some(Error::new(text.clone())),
    )
}
impl MemoryFindIndex {
    pub fn find(
        &self,
        ctx: Option<&Ctx>,
        req: &CFindRequestContext,
        identifier: Option<&Object>,
    ) -> Result<Vec<Object>> {
        let Some(identifier) = identifier else {
            return Err(new_c_find_scp_error(
                C_FIND_STATUS_UNABLE_TO_PROCESS,
                "missing C-FIND Identifier",
                None,
            ));
        };
        let supported = if self.supported_matching.is_empty() {
            study_root_find_matching_tags(&req.query_retrieve_level)
        } else {
            self.supported_matching.clone()
        };
        let negotiated = &req.extended_negotiation;
        let cancel = Arc::new(AtomicBool::new(ctx.is_some_and(|c| c.err().is_some())));
        let mut options = qrmatch::Options {
            limits: self.limits,
            supported_matching: supported.clone(),
            combined_date_time: negotiated.combined_date_time_matching,
            empty_value_matching: negotiated.empty_value_matching,
            multiple_value_matching: negotiated.multiple_value_matching,
            cancel: Some(cancel),
            ..Default::default()
        };
        if negotiated.fuzzy_semantic_matching {
            options.fuzzy_person_names = self.fuzzy_person_names;
        }
        if negotiated.timezone_query_adjustment {
            options.timezone_adjustment = true;
            options.candidate_timezone = self.candidate_timezone;
        }
        let query = qrmatch::elements_from(identifier.elements());
        if self.candidates.is_empty()
            && (options.empty_value_matching || options.multiple_value_matching)
        {
            qrmatch::match_identifier(Some(&query), Some(&[][..]), &options)
                .map_err(match_failure)?;
        }
        let mut matches = Vec::new();
        for candidate in &self.candidates {
            if let Some(ctx) = ctx
                && let Some(kind) = ctx.err()
            {
                return Err(Error::from(crate::net::ul::Error::sentinel(kind)));
            }
            let elements = qrmatch::elements_from(candidate.elements());
            let result = qrmatch::match_identifier(Some(&query), Some(&elements), &options)
                .map_err(match_failure)?;
            if !result.matched {
                continue;
            }
            matches.push(project_find_response(
                Some(identifier),
                Some(candidate),
                &self.retrieve_ae_title,
                &result.unsupported_optional,
            )?);
        }
        Ok(matches)
    }
}
fn text_element(tag: Tag, vr: Vr, value: &str) -> Element {
    Element {
        header: ElementHeader {
            tag,
            vr,
            length: 0,
            length_set: false,
        },
        value: Value::Strings(vec![value.to_string()]),
    }
}
fn empty_element(element: &Element) -> Element {
    if element.vr() == Vr::SQ {
        return Element {
            header: ElementHeader {
                tag: element.tag(),
                vr: Vr::SQ,
                length: 0,
                length_set: false,
            },
            value: Value::Sequence(Vec::new()),
        };
    }
    text_element(element.tag(), element.vr(), "")
}
pub fn project_find_response(
    query: Option<&Object>,
    candidate: Option<&Object>,
    retrieve_ae_title: &str,
    unsupported: &[Tag],
) -> Result<Object> {
    let Some(query) = query else {
        return Err(Error::new("dicom dimse: nil C-FIND Identifier"));
    };
    let mut response = Object::new();
    let mut seen: HashSet<Tag> = HashSet::new();
    let unsupported_keys: HashSet<Tag> = unsupported.iter().copied().collect();
    for element in query.elements() {
        let tag = element.tag();
        seen.insert(tag);
        if tag == TAG_MWL_SPECIFIC_CHARACTER_SET {
            continue;
        }
        if tag == TAG_QUERY_RETRIEVE_LEVEL {
            response.put(element.clone());
            continue;
        }
        if tag == TAG_RETRIEVE_AE_TITLE && !retrieve_ae_title.is_empty() {
            response.put(text_element(
                TAG_RETRIEVE_AE_TITLE,
                Vr::AE,
                retrieve_ae_title,
            ));
            continue;
        }
        if !unsupported_keys.contains(&tag)
            && let Some(value) = candidate.and_then(|c| c.get(tag))
        {
            response.put(value.clone());
            continue;
        }
        response.put(empty_element(element));
    }
    for &tag in unsupported {
        if !seen.contains(&tag) {
            response.put(text_element(tag, Vr::LO, ""));
        }
    }
    if !retrieve_ae_title.is_empty() && !seen.contains(&TAG_RETRIEVE_AE_TITLE) {
        response.put(text_element(
            TAG_RETRIEVE_AE_TITLE,
            Vr::AE,
            retrieve_ae_title,
        ));
    }
    match required_response_character_set(response.elements(), candidate) {
        Ok(Some(character_set)) => response.put(character_set),
        Ok(None) => {}
        Err(err) => {
            return Err(new_c_find_scp_error(
                C_FIND_STATUS_UNABLE_TO_PROCESS,
                "Unable to encode C-FIND response",
                Some(err),
            ));
        }
    }
    Ok(response)
}
