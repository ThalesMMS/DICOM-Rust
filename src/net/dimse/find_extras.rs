#![forbid(unsafe_code)]
use super::command::*;
use super::controls::*;
use super::error::{Error, Kind, Result};
use super::find::*;
use super::message::*;
use super::monitor::ScpCancelMonitor;
use super::mwl::{
    DEFAULT_MWL_IDENTIFIER_DEPTH, DEFAULT_MWL_IDENTIFIER_ELEMENTS, DEFAULT_MWL_MAX_MATCHES,
    DEFAULT_MWL_RESPONSE_DEPTH, DEFAULT_MWL_RESPONSE_ELEMENTS, TAG_MWL_SPECIFIC_CHARACTER_SET,
    str_element, string_values, validate_modality_worklist_object_limits,
};
use super::mwl_scp::*;
use super::pdata::{PDataReader, drain_pdata_reader, read_pdata_data_set};
use crate::charset::SpecificCharacterSet;
use crate::net::ul::{Association, Ctx, Nanos};
use crate::object::{Element, Object, Syntax, Tag, Value, Vr, dict};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Instant;
pub(crate) fn required_response_character_set(
    elements: &[Element],
    candidate: Option<&Object>,
) -> Result<Option<Element>> {
    if !response_needs_specific_character_set(elements) {
        return Ok(None);
    }
    let bad = || Error::sentinel(Kind::CFindResponseCharacterSet);
    let Some(candidate) = candidate else {
        return Err(bad());
    };
    let Some(declaration) = candidate.get(TAG_MWL_SPECIFIC_CHARACTER_SET) else {
        return Err(bad());
    };
    if declaration.vr() != Vr::CS {
        return Err(bad());
    }
    let values = string_values(declaration);
    if values.is_empty() {
        return Err(bad());
    }
    let Ok(charset) = SpecificCharacterSet::parse(&values) else {
        return Err(Error::wrap(
            Kind::CFindResponseCharacterSet,
            "unsupported declaration",
        ));
    };
    if !response_text_encoding_valid(elements, &charset) {
        return Err(bad());
    }
    Ok(Some(declaration.clone()))
}
fn response_needs_specific_character_set(elements: &[Element]) -> bool {
    let default = SpecificCharacterSet::default_repertoire();
    for element in elements {
        if element.tag() == TAG_MWL_SPECIFIC_CHARACTER_SET {
            continue;
        }
        if let Value::Sequence(items) = &element.value {
            if items
                .iter()
                .any(|item| response_needs_specific_character_set(&item.elements))
            {
                return true;
            }
            continue;
        }
        if !element.vr().uses_specific_character_set() {
            continue;
        }
        match &element.value {
            Value::Strings(values) => {
                for text in values {
                    let r = if element.vr() == Vr::PN {
                        default.encode_person_name(text)
                    } else {
                        default.encode(text)
                    };
                    if r.is_err() {
                        return true;
                    }
                }
            }
            Value::Raw(raw) if raw.iter().any(|&b| b == 0x1b || b >= 0x80) => return true,
            _ => {}
        }
    }
    false
}
fn response_text_encoding_valid(elements: &[Element], charset: &SpecificCharacterSet) -> bool {
    for element in elements {
        let vr = element.vr();
        if element.tag() == TAG_MWL_SPECIFIC_CHARACTER_SET
            || !vr.uses_specific_character_set() && vr != Vr::SQ
        {
            continue;
        }
        match &element.value {
            Value::Strings(values) => {
                for text in values {
                    let ok = if vr == Vr::PN {
                        charset.encode_person_name(text).is_ok()
                    } else if !vr.uses_text_value_delimiter() {
                        charset.encode_single_value(text).is_ok()
                    } else {
                        charset.encode(text).is_ok()
                    };
                    if !ok {
                        return false;
                    }
                }
            }
            Value::Raw(raw) => {
                if raw.contains(&0x1b) && !character_set_supports_code_extensions(charset) {
                    return false;
                }
                let ok = if vr == Vr::PN {
                    charset.decode_person_name(raw).is_ok()
                } else if !vr.uses_text_value_delimiter() {
                    charset.decode_single_value(raw).is_ok()
                } else {
                    charset.decode(raw).is_ok()
                };
                if !ok {
                    return false;
                }
            }
            Value::Sequence(items)
                if !items
                    .iter()
                    .all(|item| response_text_encoding_valid(&item.elements, charset)) =>
            {
                return false;
            }
            _ => {}
        }
    }
    true
}
fn character_set_supports_code_extensions(charset: &SpecificCharacterSet) -> bool {
    charset
        .names()
        .iter()
        .any(|name| crate::textfmt::to_upper(name.trim()).starts_with("ISO 2022 "))
}
pub(crate) fn model_levels(model: &str) -> Result<&'static [&'static str]> {
    let normalized = crate::textfmt::to_upper(model.trim());
    match normalized.as_str() {
        "STUDY_ROOT" => Ok(&["STUDY", "SERIES", "IMAGE"]),
        "PATIENT_ROOT" => Ok(&["PATIENT", "STUDY", "SERIES", "IMAGE"]),
        _ => Err(Error::new(format!(
            "dicom dimse: unsupported Query/Retrieve model {}",
            crate::textfmt::quote(&normalized)
        ))),
    }
}
pub(crate) fn normalize_query_retrieve_level(level: &str) -> String {
    crate::textfmt::to_upper(level.trim())
}
fn model_keys(model: &str, level: &str, required: bool) -> Result<Vec<String>> {
    model_levels(model)?;
    let study_root = crate::textfmt::to_upper(model.trim()) == "STUDY_ROOT";
    let level = normalize_query_retrieve_level(level);
    let keys: Option<&[&str]> = match (study_root, required, level.as_str()) {
        (true, true, "STUDY") => Some(&["StudyInstanceUID"]),
        (true, true, "SERIES") => Some(&["StudyInstanceUID", "SeriesInstanceUID"]),
        (true, true, "IMAGE") => Some(&["StudyInstanceUID", "SeriesInstanceUID", "SOPInstanceUID"]),
        (true, false, "STUDY") => Some(&[
            "PatientID",
            "PatientName",
            "StudyDate",
            "AccessionNumber",
            "StudyDescription",
        ]),
        (true, false, "SERIES") => Some(&[
            "PatientID",
            "PatientName",
            "StudyDate",
            "Modality",
            "SeriesNumber",
            "SeriesDescription",
        ]),
        (true, false, "IMAGE") => Some(&[
            "PatientID",
            "PatientName",
            "StudyDate",
            "Modality",
            "SOPClassUID",
            "InstanceNumber",
        ]),
        (false, true, "PATIENT") => Some(&["PatientID"]),
        (false, true, "STUDY") => Some(&["PatientID", "StudyInstanceUID"]),
        (false, true, "SERIES") => Some(&["PatientID", "StudyInstanceUID", "SeriesInstanceUID"]),
        (false, true, "IMAGE") => Some(&[
            "PatientID",
            "StudyInstanceUID",
            "SeriesInstanceUID",
            "SOPInstanceUID",
        ]),
        (false, false, "PATIENT") => Some(&["PatientName", "PatientBirthDate", "PatientSex"]),
        (false, false, "STUDY") => Some(&[
            "PatientName",
            "StudyDate",
            "StudyTime",
            "AccessionNumber",
            "StudyID",
            "StudyDescription",
        ]),
        (false, false, "SERIES") => Some(&[
            "PatientName",
            "StudyDate",
            "Modality",
            "SeriesNumber",
            "SeriesDescription",
        ]),
        (false, false, "IMAGE") => Some(&[
            "PatientName",
            "StudyDate",
            "Modality",
            "SOPClassUID",
            "InstanceNumber",
        ]),
        _ => None,
    };
    match keys {
        Some(keys) => Ok(keys.iter().map(|k| k.to_string()).collect()),
        None => Err(Error::new(format!(
            "dicom dimse: model {model} does not support QueryRetrieveLevel {}",
            crate::textfmt::quote(&level)
        ))),
    }
}
#[doc(hidden)]
pub fn query_retrieve_levels_of(model: &str) -> Result<Vec<String>> {
    Ok(model_levels(model)?.iter().map(|l| l.to_string()).collect())
}
pub fn query_retrieve_required_keys(model: &str, level: &str) -> Result<Vec<String>> {
    model_keys(model, level, true)
}
pub fn query_retrieve_optional_keys(model: &str, level: &str) -> Result<Vec<String>> {
    model_keys(model, level, false)
}
pub(crate) fn level_rank(level: &str) -> usize {
    match level {
        "PATIENT" => 0,
        "STUDY" => 1,
        "SERIES" => 2,
        _ => 3,
    }
}
pub(crate) fn unique_key(level: &str) -> Tag {
    match level {
        "PATIENT" => Tag::new(0x0010, 0x0020),
        "STUDY" => Tag::new(0x0020, 0x000D),
        "SERIES" => Tag::new(0x0020, 0x000E),
        _ => Tag::new(0x0008, 0x0018),
    }
}
fn key_level(tag: Tag) -> Option<&'static str> {
    Some(match (tag.group, tag.element) {
        (0x0010, 0x0010)
        | (0x0010, 0x0020)
        | (0x0010, 0x0021)
        | (0x0010, 0x0024)
        | (0x0010, 0x0030)
        | (0x0010, 0x0032)
        | (0x0010, 0x0040)
        | (0x0010, 0x1001)
        | (0x0010, 0x1002)
        | (0x0010, 0x2160)
        | (0x0010, 0x4000)
        | (0x0020, 0x1200)
        | (0x0020, 0x1202)
        | (0x0020, 0x1204) => "PATIENT",
        (0x0008, 0x0020)
        | (0x0008, 0x0030)
        | (0x0008, 0x0050)
        | (0x0008, 0x0051)
        | (0x0008, 0x0061)
        | (0x0008, 0x0062)
        | (0x0008, 0x0090)
        | (0x0008, 0x1030)
        | (0x0008, 0x1032)
        | (0x0008, 0x1060)
        | (0x0008, 0x1080)
        | (0x0010, 0x1010)
        | (0x0010, 0x1020)
        | (0x0010, 0x1030)
        | (0x0010, 0x2180)
        | (0x0010, 0x21B0)
        | (0x0020, 0x000D)
        | (0x0020, 0x0010)
        | (0x0020, 0x1070)
        | (0x0020, 0x1206)
        | (0x0020, 0x1208) => "STUDY",
        (0x0008, 0x0021)
        | (0x0008, 0x0031)
        | (0x0008, 0x0060)
        | (0x0008, 0x103E)
        | (0x0018, 0x0015)
        | (0x0020, 0x000E)
        | (0x0020, 0x0011)
        | (0x0020, 0x1209)
        | (0x0040, 0x0244)
        | (0x0040, 0x0245)
        | (0x0040, 0x0275) => "SERIES",
        (0x0008, 0x0016)
        | (0x0008, 0x0018)
        | (0x0008, 0x0023)
        | (0x0008, 0x0033)
        | (0x0020, 0x0013)
        | (0x0028, 0x0008)
        | (0x0028, 0x0010)
        | (0x0028, 0x0011) => "IMAGE",
        _ => return None,
    })
}
pub fn validate_c_find_identifier(
    model: &str,
    level: &str,
    identifier: Option<&Object>,
    relational: bool,
) -> Result<()> {
    let Some(identifier) = identifier else {
        return Err(Error::wrap(Kind::CFindIdentifierKeys, "missing Identifier"));
    };
    let levels = model_levels(model)?;
    let level = normalize_query_retrieve_level(level);
    if !levels.contains(&level.as_str()) {
        return Err(Error::wrap(
            Kind::CFindIdentifierKeys,
            format!(
                "model {model} has no level {}",
                crate::textfmt::quote(&level)
            ),
        ));
    }
    let rank = level_rank(&level);
    let study_root = levels[0] == "STUDY";
    let mut seen_unique: Vec<&str> = Vec::new();
    for element in identifier.elements() {
        let Some(mut kl) = key_level(element.tag()) else {
            continue;
        };
        if study_root && kl == "PATIENT" {
            kl = "STUDY";
        }
        let key_rank = level_rank(kl);
        if key_rank > rank {
            return Err(Error::wrap(
                Kind::CFindIdentifierKeys,
                format!("{kl} key {} below the {level} level", element.tag()),
            ));
        }
        if key_rank < rank && !relational {
            if element.tag() != unique_key(kl) {
                return Err(Error::wrap(
                    Kind::CFindIdentifierKeys,
                    format!(
                        "non-unique {kl} key {} above the {level} level in a hierarchical query",
                        element.tag()
                    ),
                ));
            }
            if !single_unique_key_value(element) {
                return Err(Error::wrap(
                    Kind::CFindIdentifierKeys,
                    format!(
                        "{kl} Unique Key {} needs a single value in a hierarchical query",
                        element.tag()
                    ),
                ));
            }
            seen_unique.push(kl);
        }
    }
    if relational {
        return Ok(());
    }
    for above in levels {
        if level_rank(above) >= rank {
            break;
        }
        if !seen_unique.contains(above) {
            return Err(Error::wrap(
                Kind::CFindIdentifierKeys,
                format!(
                    "hierarchical query at {level} level without the {above} Unique Key {}",
                    unique_key(above)
                ),
            ));
        }
    }
    Ok(())
}
fn single_unique_key_value(element: &Element) -> bool {
    let values = string_values(element);
    if values.len() != 1 {
        return false;
    }
    let value = values[0].trim();
    !value.is_empty() && !value.contains(['*', '?', '\\'])
}
#[derive(Debug, Clone, Copy)]
pub(crate) struct CFindIdentifierCheck(pub bool);
pub(crate) fn with_c_find_identifier_check(ctx: &Ctx) -> Ctx {
    ctx.with_value(CFindIdentifierCheck(true))
}
pub(crate) fn c_find_identifier_check_enabled(ctx: Option<&Ctx>) -> bool {
    ctx.and_then(|c| c.value::<CFindIdentifierCheck>())
        .is_some_and(|c| c.0)
}
pub fn new_query_retrieve_level_element(level: &str) -> Result<Element> {
    let level = normalize_query_retrieve_level(level);
    if !matches!(level.as_str(), "PATIENT" | "STUDY" | "SERIES" | "IMAGE") {
        return Err(Error::new(format!(
            "dicom dimse: unsupported QueryRetrieveLevel {}",
            crate::textfmt::quote(&level)
        )));
    }
    Ok(str_element(TAG_QUERY_RETRIEVE_LEVEL, Vr::CS, vec![level]))
}
pub(crate) type FindKeys = BTreeMap<String, String>;
pub fn build_study_root_study_find_keys(
    keys: &FindKeys,
    return_keys: &[&str],
) -> Result<Vec<Element>> {
    build_root_find_keys("STUDY_ROOT", "STUDY", keys, return_keys)
}
pub fn build_study_root_series_find_keys(
    keys: &FindKeys,
    return_keys: &[&str],
) -> Result<Vec<Element>> {
    build_root_find_keys("STUDY_ROOT", "SERIES", keys, return_keys)
}
pub fn build_study_root_image_find_keys(
    keys: &FindKeys,
    return_keys: &[&str],
) -> Result<Vec<Element>> {
    build_root_find_keys("STUDY_ROOT", "IMAGE", keys, return_keys)
}
pub fn build_patient_root_patient_find_keys(
    keys: &FindKeys,
    return_keys: &[&str],
) -> Result<Vec<Element>> {
    build_root_find_keys("PATIENT_ROOT", "PATIENT", keys, return_keys)
}
pub fn build_patient_root_study_find_keys(
    keys: &FindKeys,
    return_keys: &[&str],
) -> Result<Vec<Element>> {
    build_root_find_keys("PATIENT_ROOT", "STUDY", keys, return_keys)
}
pub fn build_patient_root_series_find_keys(
    keys: &FindKeys,
    return_keys: &[&str],
) -> Result<Vec<Element>> {
    build_root_find_keys("PATIENT_ROOT", "SERIES", keys, return_keys)
}
pub fn build_patient_root_image_find_keys(
    keys: &FindKeys,
    return_keys: &[&str],
) -> Result<Vec<Element>> {
    build_root_find_keys("PATIENT_ROOT", "IMAGE", keys, return_keys)
}
fn build_root_find_keys(
    model: &str,
    level: &str,
    keys: &FindKeys,
    return_keys: &[&str],
) -> Result<Vec<Element>> {
    query_retrieve_required_keys(model, level)?;
    let mut elements = vec![new_query_retrieve_level_element(level)?];
    elements.extend(build_keyword_elements(keys)?);
    let filtered: FindKeys = return_keys
        .iter()
        .filter(|k| !keys.contains_key(**k))
        .map(|k| (k.to_string(), String::new()))
        .collect();
    elements.extend(build_keyword_elements(&filtered)?);
    Ok(elements)
}
fn build_keyword_elements(keys: &FindKeys) -> Result<Vec<Element>> {
    let mut elements = Vec::with_capacity(keys.len());
    for (keyword, value) in keys {
        let Some((tag, _)) = dict::by_keyword(keyword) else {
            return Err(Error::new(format!(
                "dicom dimse: unknown keyword {}",
                crate::textfmt::quote(keyword)
            )));
        };
        let vr = dict::by_tag(tag).map_or(Vr::UN, |e| e.vr);
        elements.push(str_element(tag, vr, vec![value.clone()]));
    }
    Ok(elements)
}
pub const DEFAULT_FIND_CANCEL_DRAIN_TIMEOUT: Nanos = Nanos(10_000_000_000);
#[derive(Debug, Clone, Default)]
pub struct FindEachOptions {
    pub operation: OperationOptions,
    pub cancel_drain_timeout: Nanos,
}
#[derive(Debug, Clone, Default)]
pub struct FindEachOutcome {
    pub final_response: Option<CFindResponse>,
    pub pending: i64,
    pub cancel_sent: bool,
    pub discarded: i64,
}
pub fn duration_text(d: Nanos) -> String {
    let neg = d.0 < 0;
    let mut u = d.0.unsigned_abs();
    let mut out = String::new();
    fn frac(u: u64, prec: u32) -> (String, u64) {
        let mut digits = String::new();
        let mut v = u;
        let mut print = false;
        for _ in 0..prec {
            let digit = v % 10;
            print = print || digit != 0;
            if print {
                digits.insert(0, char::from(b'0' + digit as u8));
            }
            v /= 10;
        }
        if print {
            digits.insert(0, '.');
        }
        (digits, v)
    }
    if u < 1_000_000_000 {
        let (unit, prec) = match u {
            0 => return "0s".to_string(),
            1..=999 => ("ns", 0),
            1_000..=999_999 => ("\u{b5}s", 3),
            _ => ("ms", 6),
        };
        let (f, rest) = frac(u, prec);
        out.push_str(&rest.to_string());
        out.push_str(&f);
        out.push_str(unit);
    } else {
        let (f, rest) = frac(u, 9);
        u = rest;
        let secs = format!("{}{f}s", u % 60);
        u /= 60;
        if u > 0 {
            let mins = u % 60;
            u /= 60;
            if u > 0 {
                out.push_str(&format!("{u}h"));
            }
            out.push_str(&format!("{mins}m"));
        }
        out.push_str(&secs);
    }
    if neg {
        out.insert(0, '-');
    }
    out
}
impl FindClient<'_> {
    fn validate(&self, req: &CFindRequest) -> Result<String> {
        if self.presentation_ctx_id == 0 {
            return Err(Error::new(
                "dicom dimse: find client presentation context ID is required",
            ));
        }
        let requested = crate::object::transfer::normalize_uid(&req.affected_sop_class_uid);
        let client = crate::object::transfer::normalize_uid(&self.sop_class_uid);
        if !requested.is_empty() && requested != client {
            return Err(Error::new(format!(
                "dicom dimse: C-FIND request AffectedSOPClassUID {} does not match find client SOPClassUID {}",
                crate::textfmt::quote(requested),
                crate::textfmt::quote(client)
            )));
        }
        Ok(if requested.is_empty() {
            client.to_string()
        } else {
            requested.to_string()
        })
    }
    pub fn find_each(
        &self,
        mut options: FindEachOptions,
        mut req: CFindRequest,
        identifier: Option<&Object>,
        each: Option<&mut dyn FnMut(FindResult) -> bool>,
    ) -> (FindEachOutcome, Result<()>) {
        let mut outcome = FindEachOutcome::default();
        let sop = match self.validate(&req) {
            Ok(s) => s,
            Err(e) => return (outcome, Err(e)),
        };
        let Some(each) = each else {
            return (
                outcome,
                Err(Error::new("dicom dimse: nil C-FIND response callback")),
            );
        };
        let Some(identifier) = identifier else {
            return (
                outcome,
                Err(Error::new("dicom dimse: nil C-FIND identifier")),
            );
        };
        req.affected_sop_class_uid = sop;
        options.operation = operation_options_with_default_policy(
            options.operation,
            OperationErrorPolicy::LeaveOpen,
        );
        if options.cancel_drain_timeout.0 <= 0 {
            options.cancel_drain_timeout = DEFAULT_FIND_CANCEL_DRAIN_TIMEOUT;
        }
        let policy = options.operation.error_policy;
        let assoc = self.assoc;
        let fail = |err: Error, uncertain: bool| -> Error {
            if !uncertain {
                return new_operation_error("C-FIND", err, false);
            }
            apply_operation_error_policy(
                Some(assoc),
                policy,
                new_operation_error("C-FIND", err, true),
            )
        };
        let ctx = operation_context(&options.operation);
        if let Some(e) = context_err(&ctx) {
            return (outcome, Err(fail(e, false)));
        }
        let _guard = match begin_association_operation(Some(assoc)) {
            Ok(g) => g,
            Err(e) => return (outcome, Err(fail(e, false))),
        };
        let pc = self.presentation_ctx_id;
        let syntax = self.identifier_syntax;
        if let Err(e) = send_command_set(Some(&ctx), assoc, pc, &req.command_set()) {
            return (outcome, Err(fail(e, true)));
        }
        if let Err(e) = send_data_set(Some(&ctx), assoc, pc, identifier, &syntax) {
            return (outcome, Err(fail(e, true)));
        }
        loop {
            if let Some(e) = context_err(&ctx) {
                return (outcome, Err(fail(e, true)));
            }
            let rctx = operation_response_context(&ctx, options.operation.response_timeout);
            let (rsp, id) = match receive_c_find_response(Some(&rctx), assoc, pc, syntax) {
                Ok(v) => v,
                Err(e) => return (outcome, Err(fail(e, true))),
            };
            if rsp.message_id_being_responded_to != req.message_id {
                return (
                    outcome,
                    Err(fail(
                        Error::classified(
                            Kind::ProtocolViolation,
                            format!(
                                "dicom dimse: C-FIND response message ID {}, want {}",
                                rsp.message_id_being_responded_to, req.message_id
                            ),
                        ),
                        true,
                    )),
                );
            }
            let category = categorize_c_find_status(rsp.status);
            if id.is_none() && category == CFindStatusCategory::Pending {
                return (
                    outcome,
                    Err(fail(
                        Error::classified(
                            Kind::ProtocolViolation,
                            "dicom dimse: pending C-FIND response without an Identifier",
                        ),
                        true,
                    )),
                );
            }
            match category {
                CFindStatusCategory::Pending => {
                    outcome.pending += 1;
                    if each(FindResult {
                        response: rsp,
                        identifier: id,
                    }) {
                        continue;
                    }
                    return self.cancel_find_each(&ctx, &options, &req, outcome);
                }
                CFindStatusCategory::Success => {
                    outcome.final_response = Some(rsp);
                    return (outcome, Ok(()));
                }
                _ => {
                    let err = c_find_status_error(&rsp);
                    outcome.final_response = Some(rsp);
                    return (outcome, err.map_or(Ok(()), Err));
                }
            }
        }
    }
    fn cancel_find_each(
        &self,
        ctx: &Ctx,
        options: &FindEachOptions,
        req: &CFindRequest,
        mut outcome: FindEachOutcome,
    ) -> (FindEachOutcome, Result<()>) {
        let policy = options.operation.error_policy;
        let drain = ctx.with_optional_timeout(options.cancel_drain_timeout);
        let fail = |err: Error| {
            apply_operation_error_policy(
                Some(self.assoc),
                policy,
                new_operation_error("C-FIND", err, true),
            )
        };
        let cancel = CCancelRequest {
            message_id_being_responded_to: req.message_id,
        };
        if let Err(e) = send_command_set(
            Some(&drain),
            self.assoc,
            self.presentation_ctx_id,
            &cancel.command_set(),
        ) {
            return (outcome, Err(fail(e)));
        }
        outcome.cancel_sent = true;
        loop {
            let rctx = operation_response_context(&drain, options.operation.response_timeout);
            let received = receive_c_find_response(
                Some(&rctx),
                self.assoc,
                self.presentation_ctx_id,
                self.identifier_syntax,
            );
            let rsp = match received {
                Ok((rsp, _)) => rsp,
                Err(mut e) => {
                    if drain_expired(&drain) && ctx.err().is_none() {
                        let text = format!(
                            "{} within {}: {}",
                            Kind::FindCancelNotAnswered.text(),
                            duration_text(options.cancel_drain_timeout),
                            e.text()
                        );
                        e = e.relabel(text).also(Kind::FindCancelNotAnswered);
                    }
                    return (outcome, Err(fail(e)));
                }
            };
            if categorize_c_find_status(rsp.status) == CFindStatusCategory::Pending {
                outcome.discarded += 1;
                continue;
            }
            outcome.final_response = Some(rsp);
            return (outcome, Ok(()));
        }
    }
}
fn drain_expired(drain: &Ctx) -> bool {
    drain.err().is_some() || drain.deadline().is_some_and(|d| Instant::now() >= d)
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StreamingCFindLimits {
    pub max_matches: i64,
    pub max_identifier_bytes: i64,
    pub max_identifier_elements: i64,
    pub max_identifier_depth: i64,
    pub max_response_bytes: i64,
    pub max_response_elements: i64,
    pub max_response_depth: i64,
}
impl StreamingCFindLimits {
    pub(crate) fn normalized(mut self) -> Result<StreamingCFindLimits> {
        if self.max_matches < 0
            || self.max_identifier_bytes < 0
            || self.max_identifier_elements < 0
            || self.max_identifier_depth < 0
            || self.max_response_bytes < 0
            || self.max_response_elements < 0
            || self.max_response_depth < 0
        {
            return Err(Error::new(
                "dicom dimse: streaming C-FIND limits must not be negative",
            ));
        }
        if self.max_matches == 0 {
            self.max_matches = DEFAULT_MWL_MAX_MATCHES;
        }
        if self.max_identifier_bytes == 0 {
            self.max_identifier_bytes = MAX_IDENTIFIER_BYTES;
        }
        if self.max_identifier_elements == 0 {
            self.max_identifier_elements = DEFAULT_MWL_IDENTIFIER_ELEMENTS;
        }
        if self.max_identifier_depth == 0 {
            self.max_identifier_depth = DEFAULT_MWL_IDENTIFIER_DEPTH;
        }
        if self.max_response_bytes == 0 {
            self.max_response_bytes = MAX_IDENTIFIER_BYTES;
        }
        if self.max_response_elements == 0 {
            self.max_response_elements = DEFAULT_MWL_RESPONSE_ELEMENTS;
        }
        if self.max_response_depth == 0 {
            self.max_response_depth = DEFAULT_MWL_RESPONSE_DEPTH;
        }
        Ok(self)
    }
}
#[derive(Debug, Clone)]
pub struct StreamingCFindRequest {
    pub request: CFindRequest,
    pub identifier: Object,
    pub presentation_context_id: u8,
    pub identifier_syntax: Syntax,
}
pub type StreamingCFindYield<'a> = dyn FnMut(u16, Option<&Object>) -> Result<()> + 'a;
pub trait StreamingCFindHandler: Send + Sync {
    fn find(
        &self,
        ctx: &Ctx,
        request: &StreamingCFindRequest,
        yield_: &mut StreamingCFindYield<'_>,
    ) -> Result<()>;
}
pub struct StreamingCFindHandlerFunc<F>(pub F);
impl<F> StreamingCFindHandler for StreamingCFindHandlerFunc<F>
where
    F: Fn(&Ctx, &StreamingCFindRequest, &mut StreamingCFindYield<'_>) -> Result<()> + Send + Sync,
{
    fn find(
        &self,
        ctx: &Ctx,
        request: &StreamingCFindRequest,
        yield_: &mut StreamingCFindYield<'_>,
    ) -> Result<()> {
        (self.0)(ctx, request, yield_)
    }
}
#[derive(Clone, Default)]
pub struct StreamingCFindRoute {
    pub sop_class_uid: String,
    pub response_sop_class_uid: String,
    pub handler: Option<Arc<dyn StreamingCFindHandler>>,
    pub limits: StreamingCFindLimits,
}
fn streaming_provider() -> Error {
    Error::sentinel(Kind::StreamingCFindProvider)
}
#[derive(Clone)]
enum WorklistRoute {
    None,
    NoProvider,
    Route(ModalityWorklistCFindRoute),
}
#[derive(Clone)]
pub struct CFindRouter {
    hierarchical: CFindHandler,
    worklist: WorklistRoute,
    streaming: Option<HashMap<String, StreamingCFindRoute>>,
}
impl CFindRouter {
    pub fn from_handler(handler: CFindHandler) -> CFindRouter {
        CFindRouter {
            hierarchical: handler,
            worklist: WorklistRoute::None,
            streaming: None,
        }
    }
    pub(crate) fn hierarchical(&self) -> &CFindHandler {
        &self.hierarchical
    }
    pub(crate) fn serve_modality_worklist(
        &self,
        ctx: &Ctx,
        assoc: &Association,
        pc_id: u8,
        command: &Object,
    ) -> Result<()> {
        match &self.worklist {
            WorklistRoute::None => Err(Error::new(
                "dicom dimse: Modality Worklist C-FIND handler is not configured",
            )),
            WorklistRoute::NoProvider => Err(Error::sentinel(Kind::ModalityWorklistProvider)),
            WorklistRoute::Route(route) => serve_modality_worklist_c_find_command(
                Some(ctx),
                assoc,
                pc_id,
                command,
                route.handler.as_ref(),
                route.options,
            ),
        }
    }
    pub(crate) fn serve_streaming(
        &self,
        ctx: &Ctx,
        assoc: &Association,
        pc_id: u8,
        command: &Object,
        sop_class_uid: &str,
    ) -> (bool, Result<()>) {
        let Some(route) = self.streaming.as_ref().and_then(|r| r.get(sop_class_uid)) else {
            return (false, Ok(()));
        };
        (
            true,
            serve_streaming_c_find_command(Some(ctx), assoc, pc_id, command, route),
        )
    }
}
fn hierarchical_missing() -> Error {
    Error::new("dicom dimse: hierarchical C-FIND handler is not configured")
}
fn failing_handler(err: fn() -> Error) -> CFindHandler {
    Arc::new(move |_: &Ctx, _: &CFindRequestContext| Err(err()))
}
pub fn new_modality_worklist_c_find_router(
    query_retrieve: Option<CFindHandler>,
    worklist: Option<Arc<dyn ModalityWorklistHandler>>,
    options: ModalityWorklistScpOptions,
) -> Result<CFindRouter> {
    let Some(worklist) = worklist else {
        return Err(Error::sentinel(Kind::ModalityWorklistProvider));
    };
    let options = options.normalized()?;
    Ok(CFindRouter {
        hierarchical: query_retrieve.unwrap_or_else(|| failing_handler(hierarchical_missing)),
        worklist: WorklistRoute::Route(ModalityWorklistCFindRoute {
            handler: worklist,
            options,
        }),
        streaming: None,
    })
}
pub fn new_streaming_c_find_router(
    base: Option<CFindRouter>,
    routes: Vec<StreamingCFindRoute>,
) -> Result<CFindRouter> {
    if routes.is_empty() {
        return Err(streaming_provider());
    }
    let mut map: HashMap<String, StreamingCFindRoute> = HashMap::new();
    for mut route in routes {
        if route.sop_class_uid.is_empty()
            || route.handler.is_none()
            || map.contains_key(&route.sop_class_uid)
        {
            return Err(streaming_provider());
        }
        route.limits = route.limits.normalized()?;
        if route.response_sop_class_uid.is_empty() {
            route.response_sop_class_uid = route.sop_class_uid.clone();
        }
        map.insert(route.sop_class_uid.clone(), route);
    }
    let (hierarchical, worklist) = match base {
        None => (
            failing_handler(streaming_provider),
            WorklistRoute::NoProvider,
        ),
        Some(b) => {
            let worklist = match b.worklist {
                WorklistRoute::Route(r) => WorklistRoute::Route(r),
                _ => WorklistRoute::NoProvider,
            };
            (b.hierarchical, worklist)
        }
    };
    Ok(CFindRouter {
        hierarchical,
        worklist,
        streaming: Some(map),
    })
}
#[doc(hidden)]
pub fn serve_streaming_c_find_command_public(
    ctx: Option<&Ctx>,
    assoc: &Association,
    pc_id: u8,
    command: &Object,
    route: &StreamingCFindRoute,
) -> Result<()> {
    if route.sop_class_uid.trim().is_empty() || route.handler.is_none() {
        return Err(streaming_provider());
    }
    let mut route = route.clone();
    route.limits = route.limits.normalized()?;
    if route.response_sop_class_uid.is_empty() {
        route.response_sop_class_uid = route.sop_class_uid.clone();
    }
    serve_streaming_c_find_command(ctx, assoc, pc_id, command, &route)
}
fn serve_streaming_c_find_command(
    ctx: Option<&Ctx>,
    assoc: &Association,
    pc_id: u8,
    command: &Object,
    route: &StreamingCFindRoute,
) -> Result<()> {
    let Some(handler) = route.handler.as_ref() else {
        return Err(streaming_provider());
    };
    let pc = accepted_context_by_id(assoc, pc_id)?;
    let syntax = transfer_syntax_for_accepted_context(&pc)?;
    let request = parse_c_find_request(command)?;
    let limits = route.limits;
    let response_sop = route.response_sop_class_uid.as_str();
    let identifier = match receive_streaming_c_find_data_set(
        ctx,
        assoc,
        pc_id,
        syntax,
        limits.max_identifier_bytes,
        limits.max_identifier_elements,
        limits.max_identifier_depth,
    ) {
        Ok(i) => i,
        Err(err) => {
            let (status, comment) = streaming_c_find_status(&err);
            send_c_find_final(
                ctx,
                assoc,
                pc_id,
                &request,
                response_sop,
                status,
                comment,
                &syntax,
            )?;
            return Err(err);
        }
    };
    if pc.abstract_syntax_uid != route.sop_class_uid
        || request.affected_sop_class_uid != route.sop_class_uid
    {
        send_c_find_final(
            ctx,
            assoc,
            pc_id,
            &request,
            response_sop,
            C_FIND_STATUS_IDENTIFIER_DOES_NOT_MATCH_SOP_CLASS,
            "Identifier does not match SOP Class",
            &syntax,
        )?;
        return Err(Error::sentinel(Kind::StreamingCFindIdentifier));
    }
    let grace = scp_controls(ctx).cancel_grace;
    let (handler_err, op_ctx) = std::thread::scope(|s| {
        let monitor = ScpCancelMonitor::start(
            s,
            ctx,
            assoc,
            pc_id,
            request.message_id,
            Error::sentinel(Kind::CFindCanceled),
            false,
        );
        let op_ctx = monitor.context().clone();
        let streaming_request = StreamingCFindRequest {
            request: request.clone(),
            identifier: identifier.clone(),
            presentation_context_id: pc_id,
            identifier_syntax: syntax,
        };
        let r = run_scp_handler(&op_ctx, Some(assoc), grace, |handler_ctx| {
            let mut matches = 0i64;
            let mut emit = |status: u16, response: Option<&Object>| -> Result<()> {
                let response = match response {
                    Some(r) if status == STATUS_PENDING || status == STATUS_PENDING_WARNING => r,
                    _ => return Err(streaming_provider()),
                };
                if let Some(e) = monitor.operation_error() {
                    return Err(e);
                }
                if matches >= limits.max_matches {
                    return Err(Error::sentinel(Kind::StreamingCFindResourceLimit));
                }
                if let Err(e) = preflight_modality_worklist_response(
                    response,
                    &syntax,
                    limits.max_response_elements,
                    limits.max_response_bytes,
                    limits.max_response_depth,
                ) {
                    if e.is(Kind::ModalityWorklistResourceLimit) {
                        return Err(Error::sentinel(Kind::StreamingCFindResourceLimit));
                    }
                    return Err(streaming_provider());
                }
                send_with_scp_response_context(ctx, Some(assoc), |rc| {
                    send_c_find_response(
                        Some(rc),
                        assoc,
                        pc_id,
                        CFindResponse {
                            affected_sop_class_uid: response_sop.to_string(),
                            message_id_being_responded_to: request.message_id,
                            status,
                            ..Default::default()
                        },
                        Some(response),
                        &syntax,
                    )
                })?;
                matches += 1;
                Ok(())
            };
            handler.find(handler_ctx, &streaming_request, &mut emit)
        })
        .err();
        let monitor_err = monitor.stop();
        (Error::join([r, monitor_err]), op_ctx)
    });
    if let Some(err) = handler_err {
        let (status, comment) = streaming_c_find_status(&err);
        send_c_find_final(
            Some(&op_ctx),
            assoc,
            pc_id,
            &request,
            response_sop,
            status,
            comment,
            &syntax,
        )?;
        return Err(err);
    }
    send_c_find_final(
        ctx,
        assoc,
        pc_id,
        &request,
        response_sop,
        STATUS_SUCCESS,
        "",
        &syntax,
    )
}
fn receive_streaming_c_find_data_set(
    ctx: Option<&Ctx>,
    assoc: &Association,
    pc_id: u8,
    syntax: Syntax,
    max_bytes: i64,
    max_elements: i64,
    max_depth: i64,
) -> Result<Object> {
    let mut reader = PDataReader::typed(
        Some(data_set_read_context(ctx, Some(assoc))),
        Some(assoc),
        pc_id,
        false,
    );
    let value = match read_pdata_data_set(
        &mut reader,
        syntax,
        limit_options(max_bytes, max_elements, max_depth),
    ) {
        Ok(v) => v,
        Err(err) => {
            if should_drain_on_error(&err) {
                drain_pdata_reader(&mut reader)?;
            }
            if is_modality_worklist_limit_error(&err) {
                return Err(Error::sentinel(Kind::StreamingCFindResourceLimit));
            }
            return Err(Error::sentinel(Kind::StreamingCFindIdentifier));
        }
    };
    if validate_modality_worklist_object_limits(Some(&value), max_elements, max_depth).is_err() {
        return Err(Error::sentinel(Kind::StreamingCFindResourceLimit));
    }
    Ok(value)
}
fn streaming_c_find_status(err: &Error) -> (u16, &'static str) {
    if err.is(Kind::CFindCanceled) || err.is_canceled() {
        return (C_FIND_STATUS_CANCEL, "C-FIND canceled");
    }
    if err.is(Kind::StreamingCFindResourceLimit) {
        return (C_FIND_STATUS_OUT_OF_RESOURCES, "Out of resources");
    }
    if err.is(Kind::StreamingCFindIdentifier) {
        return (
            C_FIND_STATUS_IDENTIFIER_DOES_NOT_MATCH_SOP_CLASS,
            "Identifier does not match SOP Class",
        );
    }
    (C_FIND_STATUS_UNABLE_TO_PROCESS, "Unable to process")
}
#[derive(Debug, Clone, Default)]
pub struct StreamingCFindClientOptions {
    pub operation: OperationOptions,
    pub max_matches: i64,
    pub max_response_bytes: i64,
    pub max_response_elements: i64,
    pub max_response_depth: i64,
    pub cancel_drain_timeout: Nanos,
    pub priority: u16,
}
impl StreamingCFindClientOptions {
    fn normalized(mut self) -> Result<StreamingCFindClientOptions> {
        if self.max_matches < 0
            || self.max_response_bytes < 0
            || self.max_response_elements < 0
            || self.max_response_depth < 0
            || self.cancel_drain_timeout.0 < 0
            || self.priority > PRIORITY_LOW
        {
            return Err(Error::sentinel(Kind::StreamingCFindResourceLimit));
        }
        if self.max_matches == 0 {
            self.max_matches = DEFAULT_MWL_MAX_MATCHES;
        }
        if self.max_response_bytes == 0 {
            self.max_response_bytes = MAX_IDENTIFIER_BYTES;
        }
        if self.max_response_elements == 0 {
            self.max_response_elements = DEFAULT_MWL_RESPONSE_ELEMENTS;
        }
        if self.max_response_depth == 0 {
            self.max_response_depth = DEFAULT_MWL_RESPONSE_DEPTH;
        }
        if self.cancel_drain_timeout.0 == 0 {
            self.cancel_drain_timeout = Nanos::from_millis(1000);
        }
        self.operation =
            operation_options_with_default_policy(self.operation, OperationErrorPolicy::Abort);
        Ok(self)
    }
}
#[derive(Debug, Clone, Default)]
pub struct StreamingCFindResult {
    pub final_response: Option<CFindResponse>,
    pub match_count: i64,
    pub cancel_sent: bool,
}
pub fn streaming_c_find_status_error(status: u16) -> Error {
    Error::new(format!(
        "dicom dimse: streaming C-FIND status 0x{status:04X}"
    ))
    .with_detail(super::error::Detail::Status(status))
}
pub struct StreamingCFindClient<'a> {
    assoc: &'a Association,
    pc_id: u8,
    syntax: Syntax,
    request_sop_class_uid: String,
    response_sop_class_uid: String,
    next_id: AtomicU32,
}
pub type StreamingCFindCallback<'a> = dyn FnMut(u16, &Object) -> Result<()> + 'a;
const STREAMING_OPERATION: &str = "streaming C-FIND";
impl<'a> StreamingCFindClient<'a> {
    pub fn new(
        assoc: &'a Association,
        request_sop_class_uid: &str,
        response_sop_class_uid: &str,
    ) -> Result<StreamingCFindClient<'a>> {
        let pc = accepted_context_for_sop_class(assoc, request_sop_class_uid)?;
        let syntax = transfer_syntax_for_accepted_context(&pc)?;
        let response = if response_sop_class_uid.is_empty() {
            request_sop_class_uid
        } else {
            response_sop_class_uid
        };
        Ok(StreamingCFindClient {
            assoc,
            pc_id: pc.id,
            syntax,
            request_sop_class_uid: request_sop_class_uid.to_string(),
            response_sop_class_uid: response.to_string(),
            next_id: AtomicU32::new(0),
        })
    }
    fn next_message_id(&self) -> u16 {
        loop {
            let id = self.next_id.fetch_add(1, Ordering::SeqCst).wrapping_add(1) as u16;
            if id != 0 {
                return id;
            }
        }
    }
    fn fail(&self, policy: OperationErrorPolicy, err: Error) -> Error {
        apply_operation_error_policy(
            Some(self.assoc),
            policy,
            new_operation_error(STREAMING_OPERATION, err, true),
        )
    }
    pub fn find(
        &self,
        ctx: &Ctx,
        identifier: Option<&Object>,
        yield_: Option<&mut StreamingCFindCallback<'_>>,
    ) -> (StreamingCFindResult, Result<()>) {
        self.find_with_options(
            StreamingCFindClientOptions {
                operation: OperationOptions {
                    context: Some(ctx.clone()),
                    ..Default::default()
                },
                ..Default::default()
            },
            identifier,
            yield_,
        )
    }
    pub fn find_with_options(
        &self,
        options: StreamingCFindClientOptions,
        identifier: Option<&Object>,
        yield_: Option<&mut StreamingCFindCallback<'_>>,
    ) -> (StreamingCFindResult, Result<()>) {
        let mut result = StreamingCFindResult::default();
        let (Some(identifier), Some(yield_)) = (identifier, yield_) else {
            return (result, Err(streaming_provider()));
        };
        let options = match options.normalized() {
            Ok(o) => o,
            Err(e) => return (result, Err(e)),
        };
        let ctx = operation_context(&options.operation);
        if let Some(e) = context_err(&ctx) {
            return (
                result,
                Err(new_operation_error(STREAMING_OPERATION, e, false)),
            );
        }
        let _guard = match begin_association_operation(Some(self.assoc)) {
            Ok(g) => g,
            Err(e) => {
                return (
                    result,
                    Err(new_operation_error(STREAMING_OPERATION, e, false)),
                );
            }
        };
        let policy = options.operation.error_policy;
        let request = CFindRequest {
            affected_sop_class_uid: self.request_sop_class_uid.clone(),
            message_id: self.next_message_id(),
            priority: options.priority,
        };
        if let Err(e) = send_command_set(Some(&ctx), self.assoc, self.pc_id, &request.command_set())
        {
            return (result, Err(self.fail(policy, e)));
        }
        if let Err(e) = send_data_set(Some(&ctx), self.assoc, self.pc_id, identifier, &self.syntax)
        {
            return (result, Err(self.fail(policy, e)));
        }
        loop {
            if let Some(e) = context_err(&ctx) {
                return self.cancel_and_drain(result, &request, &options, e);
            }
            let rctx = operation_response_context(&ctx, options.operation.response_timeout);
            let (response, matched) = match self.receive_response(&rctx, &options) {
                Ok(v) => v,
                Err(e) => {
                    if let Some(c) = context_err(&ctx) {
                        return self.cancel_and_drain(result, &request, &options, c);
                    }
                    return (result, Err(self.fail(policy, e)));
                }
            };
            if let Err(e) = self.validate_response(&response, request.message_id) {
                return (result, Err(self.fail(policy, e)));
            }
            let category = categorize_c_find_status(response.status);
            if let Err(e) = validate_response_identifier(category, matched.is_some()) {
                return (result, Err(self.fail(policy, e)));
            }
            match category {
                CFindStatusCategory::Pending => {
                    if result.match_count >= options.max_matches {
                        return self.cancel_and_drain(
                            result,
                            &request,
                            &options,
                            Error::sentinel(Kind::StreamingCFindResourceLimit),
                        );
                    }
                    result.match_count += 1;
                    let Some(matched) = matched else {
                        continue;
                    };
                    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        yield_(response.status, &matched)
                    }));
                    let err = match r {
                        Ok(Ok(())) => None,
                        Ok(Err(e)) => Some(
                            e.relabel(Kind::StreamingCFindCallback.text())
                                .also(Kind::StreamingCFindCallback),
                        ),
                        Err(_) => Some(Error::sentinel(Kind::StreamingCFindCallback)),
                    };
                    if let Some(e) = err {
                        return self.cancel_and_drain(result, &request, &options, e);
                    }
                }
                CFindStatusCategory::Success => {
                    result.final_response = Some(response);
                    return (result, Ok(()));
                }
                CFindStatusCategory::Failure => {
                    let status = response.status;
                    result.final_response = Some(response);
                    return (result, Err(streaming_c_find_status_error(status)));
                }
                CFindStatusCategory::Invalid => {
                    return (
                        result,
                        Err(self.fail(policy, c_find_invalid_status_error(response.status))),
                    );
                }
            }
        }
    }
    fn cancel_and_drain(
        &self,
        mut result: StreamingCFindResult,
        request: &CFindRequest,
        options: &StreamingCFindClientOptions,
        cause: Error,
    ) -> (StreamingCFindResult, Result<()>) {
        let policy = options.operation.error_policy;
        let drain = Ctx::background().with_optional_timeout(options.cancel_drain_timeout);
        let joined = |e: Error| Error::join([Some(cause.clone()), Some(e)]).expect("two errors");
        let cancel = CCancelRequest {
            message_id_being_responded_to: request.message_id,
        };
        if let Err(e) =
            send_command_set(Some(&drain), self.assoc, self.pc_id, &cancel.command_set())
        {
            return (result, Err(self.fail(policy, joined(e))));
        }
        result.cancel_sent = true;
        loop {
            let (response, matched) = match self.receive_response(&drain, options) {
                Ok(v) => v,
                Err(e) => return (result, Err(self.fail(policy, joined(e)))),
            };
            if let Err(e) = self.validate_response(&response, request.message_id) {
                return (result, Err(self.fail(policy, joined(e))));
            }
            let category = categorize_c_find_status(response.status);
            if let Err(e) = validate_response_identifier(category, matched.is_some()) {
                return (result, Err(self.fail(policy, joined(e))));
            }
            match category {
                CFindStatusCategory::Pending => continue,
                CFindStatusCategory::Invalid => {
                    return (
                        result,
                        Err(self.fail(policy, joined(c_find_invalid_status_error(response.status)))),
                    );
                }
                _ => {
                    result.final_response = Some(response);
                    return (
                        result,
                        Err(new_operation_error(STREAMING_OPERATION, cause, false)),
                    );
                }
            }
        }
    }
    fn receive_response(
        &self,
        ctx: &Ctx,
        options: &StreamingCFindClientOptions,
    ) -> Result<(CFindResponse, Option<Object>)> {
        let command = receive_command_set(Some(ctx), self.assoc, self.pc_id)?;
        let response = parse_c_find_response(&command)?;
        if response.command_data_set_type == NO_DATA_SET {
            return Ok((response, None));
        }
        let matched = receive_streaming_c_find_data_set(
            Some(ctx),
            self.assoc,
            self.pc_id,
            self.syntax,
            options.max_response_bytes,
            options.max_response_elements,
            options.max_response_depth,
        )?;
        Ok((response, Some(matched)))
    }
    fn validate_response(&self, response: &CFindResponse, message_id: u16) -> Result<()> {
        if response.message_id_being_responded_to != message_id {
            return Err(streaming_provider());
        }
        if !response.affected_sop_class_uid.is_empty()
            && response.affected_sop_class_uid != self.response_sop_class_uid
        {
            return Err(streaming_provider());
        }
        Ok(())
    }
}
