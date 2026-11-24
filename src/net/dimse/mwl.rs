#![forbid(unsafe_code)]
use super::command::MAX_IDENTIFIER_BYTES;
use super::error::{Error, Kind, Result};
use super::find::MODALITY_WORKLIST_FIND_SOP_CLASS_UID;
use super::find_extras::required_response_character_set;
use crate::charset::SpecificCharacterSet;
use crate::dcmtime::{self, ZonedTime};
use crate::net::ul;
use crate::object::text::{trim_text_value, trim_text_value_bytes};
use crate::object::{DataSet, Element, ElementHeader, Object, Tag, Value, Vr, dict};
use crate::validation::{Mode, ValidationOptions, validate_data_set_with};
use std::collections::{HashMap, HashSet};
pub(crate) const TAG_MWL_SPECIFIC_CHARACTER_SET: Tag = Tag::new(0x0008, 0x0005);
pub(crate) const TAG_MWL_TIMEZONE_OFFSET_FROM_UTC: Tag = Tag::new(0x0008, 0x0201);
pub(crate) const TAG_MWL_ACCESSION_NUMBER: Tag = Tag::new(0x0008, 0x0050);
pub(crate) const TAG_MWL_MODALITY: Tag = Tag::new(0x0008, 0x0060);
pub(crate) const TAG_MWL_PATIENT_NAME: Tag = Tag::new(0x0010, 0x0010);
pub(crate) const TAG_MWL_PATIENT_ID: Tag = Tag::new(0x0010, 0x0020);
pub(crate) const TAG_MWL_PATIENT_BIRTH_DATE: Tag = Tag::new(0x0010, 0x0030);
pub(crate) const TAG_MWL_PATIENT_SEX: Tag = Tag::new(0x0010, 0x0040);
pub(crate) const TAG_MWL_REQUESTED_PROCEDURE_DESCRIPTION: Tag = Tag::new(0x0032, 0x1060);
pub(crate) const TAG_MWL_SCHEDULED_STATION_AE_TITLE: Tag = Tag::new(0x0040, 0x0001);
pub const TAG_MWL_SPS_START_DATE: Tag = Tag::new(0x0040, 0x0002);
pub const TAG_MWL_SPS_START_TIME: Tag = Tag::new(0x0040, 0x0003);
pub(crate) const TAG_MWL_SCHEDULED_PERFORMING_PHYSICIAN_NAME: Tag = Tag::new(0x0040, 0x0006);
pub const TAG_MWL_SPS_DESCRIPTION: Tag = Tag::new(0x0040, 0x0007);
pub(crate) const TAG_MWL_SCHEDULED_PROTOCOL_CODE_SEQUENCE: Tag = Tag::new(0x0040, 0x0008);
pub const TAG_MWL_SPS_ID: Tag = Tag::new(0x0040, 0x0009);
pub(crate) const TAG_MWL_SCHEDULED_STATION_NAME: Tag = Tag::new(0x0040, 0x0010);
pub const TAG_MWL_SPS_LOCATION: Tag = Tag::new(0x0040, 0x0011);
pub const TAG_MWL_SPS_SEQUENCE: Tag = Tag::new(0x0040, 0x0100);
pub(crate) const TAG_MWL_REQUESTED_PROCEDURE_ID: Tag = Tag::new(0x0040, 0x1001);
pub(crate) const DEFAULT_MWL_MAX_MATCHES: i64 = 10_000;
pub(crate) const DEFAULT_MWL_IDENTIFIER_ELEMENTS: i64 = 256;
pub(crate) const DEFAULT_MWL_IDENTIFIER_DEPTH: i64 = 4;
pub(crate) const DEFAULT_MWL_RESPONSE_ELEMENTS: i64 = 1_024;
pub(crate) const DEFAULT_MWL_RESPONSE_DEPTH: i64 = 8;
pub fn modality_worklist_find_presentation_context() -> ul::PresentationContext {
    ul::PresentationContext::new(
        MODALITY_WORKLIST_FIND_SOP_CLASS_UID,
        &super::find::DEFAULT_FIND_TRANSFER_SYNTAXES,
    )
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MwlKey {
    pub present: bool,
    pub values: Vec<String>,
}
pub fn mwl_match(values: &[&str]) -> MwlKey {
    MwlKey {
        present: true,
        values: values.iter().map(|v| v.to_string()).collect(),
    }
}
pub fn mwl_return_key() -> MwlKey {
    mwl_match(&[""])
}
fn key_of(values: Vec<String>) -> MwlKey {
    MwlKey {
        present: true,
        values,
    }
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MwlSequenceKey {
    pub present: bool,
    pub items: Vec<DataSet>,
}
pub fn mwl_sequence_return_key() -> MwlSequenceKey {
    MwlSequenceKey {
        present: true,
        items: Vec::new(),
    }
}
pub fn mwl_sequence_match(item: DataSet) -> MwlSequenceKey {
    MwlSequenceKey {
        present: true,
        items: vec![item],
    }
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModalityWorklistScheduledProcedureStep {
    pub scheduled_station_ae_title: MwlKey,
    pub modality: MwlKey,
    pub scheduled_procedure_step_start_date: MwlKey,
    pub scheduled_procedure_step_start_time: MwlKey,
    pub scheduled_performing_physician_name: MwlKey,
    pub scheduled_procedure_step_description: MwlKey,
    pub scheduled_procedure_step_id: MwlKey,
    pub scheduled_station_name: MwlKey,
    pub scheduled_procedure_step_location: MwlKey,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModalityWorklistVeterinaryQuery {
    pub patient_species_description: MwlKey,
    pub patient_breed_description: MwlKey,
    pub patient_sex_neutered: MwlKey,
    pub responsible_person: MwlKey,
    pub responsible_person_role: MwlKey,
    pub responsible_organization: MwlKey,
    pub patient_species_code_sequence: MwlSequenceKey,
    pub patient_breed_code_sequence: MwlSequenceKey,
    pub breed_registration_sequence: MwlSequenceKey,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModalityWorklistQuery {
    pub specific_character_set: Vec<String>,
    pub timezone_offset_from_utc: String,
    pub patient_name: MwlKey,
    pub patient_id: MwlKey,
    pub patient_birth_date: MwlKey,
    pub patient_sex: MwlKey,
    pub accession_number: MwlKey,
    pub requested_procedure_id: MwlKey,
    pub requested_procedure_description: MwlKey,
    pub scheduled_procedure_step: Option<ModalityWorklistScheduledProcedureStep>,
    pub issuer_of_patient_id: MwlKey,
    pub study_date: MwlKey,
    pub study_time: MwlKey,
    pub study_instance_uid: MwlKey,
    pub requesting_physician: MwlKey,
    pub referring_physician_name: MwlKey,
    pub admission_id: MwlKey,
    pub requested_procedure_priority: MwlKey,
    pub requested_procedure_code_sequence: MwlSequenceKey,
    pub issuer_of_accession_number_sequence: MwlSequenceKey,
    pub issuer_of_admission_id_sequence: MwlSequenceKey,
    pub issuer_of_patient_id_qualifiers_sequence: MwlSequenceKey,
    pub other_patient_ids_sequence: MwlSequenceKey,
    pub veterinary: Option<ModalityWorklistVeterinaryQuery>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MatchKind {
    Single,
    Wildcard,
    DateRange,
    TimeRange,
    WildcardSensitive,
}
pub(crate) fn str_element(tag: Tag, vr: Vr, values: Vec<String>) -> Element {
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
pub(crate) fn string_values(e: &Element) -> Vec<String> {
    match &e.value {
        Value::Raw(raw) => {
            let parts: Vec<&[u8]> = raw.split(|b| *b == b'\\').collect();
            if parts.len() == 1 && trim_text_value_bytes(e.vr(), parts[0]).is_empty() {
                return Vec::new();
            }
            parts
                .iter()
                .map(|p| String::from_utf8_lossy(trim_text_value_bytes(e.vr(), p)).into_owned())
                .collect()
        }
        Value::Strings(s) => s
            .iter()
            .map(|v| trim_text_value(e.vr(), v).to_string())
            .collect(),
        _ => Vec::new(),
    }
}
pub(crate) fn encoded_length(v: &Value) -> Option<u32> {
    match v {
        Value::Raw(raw) => Some(raw.len() as u32),
        Value::Strings(s) => {
            let mut total: usize = s.iter().map(String::len).sum();
            if !s.is_empty() {
                total += s.len() - 1;
            }
            Some((total + total % 2) as u32)
        }
        _ => None,
    }
}
fn strict_validation_options() -> ValidationOptions {
    ValidationOptions {
        mode: Mode::Strict,
        dictionary: true,
        max_findings: 1,
        max_depth: 2,
        max_elements: 4,
        stop_first: true,
        ..Default::default()
    }
}
pub(crate) fn strict_valid(elements: Vec<Element>) -> bool {
    let (_, err) = validate_data_set_with(
        &DataSet {
            elements,
            item_offset: None,
        },
        &strict_validation_options(),
    );
    err.is_none()
}
fn provider() -> Error {
    Error::sentinel(Kind::ModalityWorklistProvider)
}
fn resource_limit() -> Error {
    Error::sentinel(Kind::ModalityWorklistResourceLimit)
}
pub(crate) fn mwl_identifier_error(tag: Tag, reason: &str) -> Error {
    if tag.is_zero() {
        return Error::wrap(Kind::ModalityWorklistIdentifier, reason);
    }
    Error::wrap(
        Kind::ModalityWorklistIdentifier,
        format!("attribute {tag}: {reason}"),
    )
}
pub fn build_modality_worklist_identifier(query: &ModalityWorklistQuery) -> Result<Object> {
    let mut elements: Vec<Element> = Vec::with_capacity(8);
    if !query.timezone_offset_from_utc.is_empty() && !query_has_temporal_key(query) {
        return Err(Error::new(
            "dicom dimse: MWL Timezone Offset From UTC requires a DA or TM key",
        ));
    }
    if !query.specific_character_set.is_empty() {
        if SpecificCharacterSet::parse(&query.specific_character_set).is_err() {
            return Err(Error::new(
                "dicom dimse: invalid MWL Specific Character Set",
            ));
        }
        let element = str_element(
            TAG_MWL_SPECIFIC_CHARACTER_SET,
            Vr::CS,
            query.specific_character_set.clone(),
        );
        validate_mwl_query_element(&element)?;
        elements.push(element);
    }
    if !query.timezone_offset_from_utc.is_empty() {
        validate_mwl_timezone_offset(&query.timezone_offset_from_utc)?;
        elements.push(str_element(
            TAG_MWL_TIMEZONE_OFFSET_FROM_UTC,
            Vr::SH,
            vec![query.timezone_offset_from_utc.clone()],
        ));
    }
    let top = [
        (
            TAG_MWL_PATIENT_NAME,
            Vr::PN,
            &query.patient_name,
            MatchKind::Wildcard,
        ),
        (
            TAG_MWL_PATIENT_ID,
            Vr::LO,
            &query.patient_id,
            MatchKind::Single,
        ),
        (
            TAG_MWL_PATIENT_BIRTH_DATE,
            Vr::DA,
            &query.patient_birth_date,
            MatchKind::DateRange,
        ),
        (
            TAG_MWL_PATIENT_SEX,
            Vr::CS,
            &query.patient_sex,
            MatchKind::Single,
        ),
        (
            TAG_MWL_ACCESSION_NUMBER,
            Vr::SH,
            &query.accession_number,
            MatchKind::Single,
        ),
        (
            TAG_MWL_REQUESTED_PROCEDURE_ID,
            Vr::SH,
            &query.requested_procedure_id,
            MatchKind::Single,
        ),
        (
            TAG_MWL_REQUESTED_PROCEDURE_DESCRIPTION,
            Vr::LO,
            &query.requested_procedure_description,
            MatchKind::Single,
        ),
    ];
    for (tag, vr, key, kind) in top {
        append_mwl_key(&mut elements, tag, vr, key, false, kind)?;
    }
    append_mwl_optional_keys(&mut elements, query)?;
    if let Some(step) = &query.scheduled_procedure_step {
        let item = build_scheduled_procedure_step(step)?;
        elements.push(Element {
            header: ElementHeader {
                tag: TAG_MWL_SPS_SEQUENCE,
                vr: Vr::SQ,
                length: 0,
                length_set: false,
            },
            value: Value::Sequence(vec![DataSet {
                elements: item,
                item_offset: None,
            }]),
        });
    }
    let result = Object::from_elements(elements.clone());
    if mwl_optional_keys_present(query) {
        validate_modality_worklist_object_limits(
            Some(&result),
            DEFAULT_MWL_IDENTIFIER_ELEMENTS,
            DEFAULT_MWL_IDENTIFIER_DEPTH,
        )?;
        let mut remaining = MAX_IDENTIFIER_BYTES;
        for element in &elements {
            mwl_sequence_input_bytes(element, &mut remaining)?;
        }
    }
    Ok(result)
}
fn query_has_temporal_key(query: &ModalityWorklistQuery) -> bool {
    query.study_date.present
        || query.study_time.present
        || query.scheduled_procedure_step.as_ref().is_some_and(|s| {
            s.scheduled_procedure_step_start_date.present
                || s.scheduled_procedure_step_start_time.present
        })
}
pub(crate) fn validate_mwl_timezone_offset(value: &str) -> Result<()> {
    let bad = || Error::new("dicom dimse: invalid MWL Timezone Offset From UTC");
    let b = value.as_bytes();
    if b.len() != 5 || (b[0] != b'+' && b[0] != b'-') {
        return Err(bad());
    }
    let atoi = |part: &[u8]| {
        std::str::from_utf8(part)
            .ok()
            .and_then(|p| crate::numtext::parse_int(p).ok())
    };
    let (Some(hours), Some(minutes)) = (atoi(&b[1..3]), atoi(&b[3..5])) else {
        return Err(bad());
    };
    if minutes > 59 {
        return Err(bad());
    }
    let mut offset = hours * 60 + minutes;
    if b[0] == b'-' {
        offset = -offset;
    }
    if !(-12 * 60..=14 * 60).contains(&offset) || value == "-0000" {
        return Err(bad());
    }
    Ok(())
}
fn build_scheduled_procedure_step(
    step: &ModalityWorklistScheduledProcedureStep,
) -> Result<Vec<Element>> {
    let mut elements = Vec::with_capacity(9);
    let keys = [
        (
            TAG_MWL_SCHEDULED_STATION_AE_TITLE,
            Vr::AE,
            &step.scheduled_station_ae_title,
            MatchKind::Single,
        ),
        (TAG_MWL_MODALITY, Vr::CS, &step.modality, MatchKind::Single),
        (
            TAG_MWL_SPS_START_DATE,
            Vr::DA,
            &step.scheduled_procedure_step_start_date,
            MatchKind::DateRange,
        ),
        (
            TAG_MWL_SPS_START_TIME,
            Vr::TM,
            &step.scheduled_procedure_step_start_time,
            MatchKind::TimeRange,
        ),
        (
            TAG_MWL_SCHEDULED_PERFORMING_PHYSICIAN_NAME,
            Vr::PN,
            &step.scheduled_performing_physician_name,
            MatchKind::Wildcard,
        ),
        (
            TAG_MWL_SPS_DESCRIPTION,
            Vr::LO,
            &step.scheduled_procedure_step_description,
            MatchKind::Single,
        ),
        (
            TAG_MWL_SPS_ID,
            Vr::SH,
            &step.scheduled_procedure_step_id,
            MatchKind::Single,
        ),
        (
            TAG_MWL_SCHEDULED_STATION_NAME,
            Vr::SH,
            &step.scheduled_station_name,
            MatchKind::Single,
        ),
        (
            TAG_MWL_SPS_LOCATION,
            Vr::SH,
            &step.scheduled_procedure_step_location,
            MatchKind::Single,
        ),
    ];
    for (tag, vr, key, kind) in keys {
        append_mwl_key(&mut elements, tag, vr, key, false, kind)?;
    }
    Ok(elements)
}
pub(crate) fn append_mwl_key(
    elements: &mut Vec<Element>,
    tag: Tag,
    vr: Vr,
    key: &MwlKey,
    allow_many: bool,
    kind: MatchKind,
) -> Result<()> {
    if !key.present {
        return Ok(());
    }
    if key.values.is_empty() {
        return Err(Error::new(format!(
            "dicom dimse: MWL key {tag} is present without a value"
        )));
    }
    if !allow_many && key.values.len() != 1 {
        return Err(Error::new(format!(
            "dicom dimse: MWL key {tag} requires VM 1"
        )));
    }
    for value in &key.values {
        let value = value.trim();
        if value.is_empty() {
            continue;
        }
        match kind {
            MatchKind::Single => {
                if value.contains(['*', '?']) {
                    return Err(Error::new(format!(
                        "dicom dimse: wildcard is not supported for MWL key {tag}"
                    )));
                }
            }
            MatchKind::DateRange => validate_mwl_temporal_query(value, Vr::DA)?,
            MatchKind::TimeRange => validate_mwl_temporal_query(value, Vr::TM)?,
            _ => {}
        }
    }
    let element = str_element(tag, vr, key.values.clone());
    if kind != MatchKind::DateRange && kind != MatchKind::TimeRange {
        validate_mwl_query_element(&element)?;
    }
    elements.push(element);
    Ok(())
}
fn validate_mwl_query_element(element: &Element) -> Result<()> {
    if !strict_valid(vec![element.clone()]) {
        return Err(Error::new(format!(
            "dicom dimse: MWL key {} has invalid value",
            element.tag()
        )));
    }
    Ok(())
}
pub fn match_modality_worklist(
    query: &ModalityWorklistQuery,
    candidate: Option<&Object>,
) -> Result<bool> {
    let Some(candidate) = candidate else {
        return Err(Error::new("dicom dimse: nil MWL candidate"));
    };
    if let Some(offset) = candidate.get_string(TAG_MWL_TIMEZONE_OFFSET_FROM_UTC) {
        if validate_mwl_timezone_offset(&offset).is_err() {
            return Err(Error::new(
                "dicom dimse: invalid MWL candidate Timezone Offset From UTC",
            ));
        }
        if !query.timezone_offset_from_utc.is_empty() && offset != query.timezone_offset_from_utc {
            return Ok(false);
        }
    }
    let top = [
        (
            &query.patient_name,
            TAG_MWL_PATIENT_NAME,
            MatchKind::Wildcard,
        ),
        (&query.patient_id, TAG_MWL_PATIENT_ID, MatchKind::Single),
        (
            &query.patient_birth_date,
            TAG_MWL_PATIENT_BIRTH_DATE,
            MatchKind::DateRange,
        ),
        (&query.patient_sex, TAG_MWL_PATIENT_SEX, MatchKind::Single),
        (
            &query.accession_number,
            TAG_MWL_ACCESSION_NUMBER,
            MatchKind::Single,
        ),
        (
            &query.requested_procedure_id,
            TAG_MWL_REQUESTED_PROCEDURE_ID,
            MatchKind::Single,
        ),
        (
            &query.requested_procedure_description,
            TAG_MWL_REQUESTED_PROCEDURE_DESCRIPTION,
            MatchKind::Single,
        ),
    ];
    for (key, tag, kind) in top {
        if !match_mwl_key(key, candidate, tag, kind)? {
            return Ok(false);
        }
    }
    if !match_mwl_optional_keys(query, candidate)? {
        return Ok(false);
    }
    let Some(step) = &query.scheduled_procedure_step else {
        return Ok(true);
    };
    let items = candidate.get_sequence(TAG_MWL_SPS_SEQUENCE);
    let Some(items) = items.filter(|i| !i.is_empty()) else {
        return Ok(false);
    };
    if items.len() != 1 {
        return Err(Error::new(
            "dicom dimse: MWL candidate requires exactly one Scheduled Procedure Step item",
        ));
    }
    match_scheduled_procedure_step(step, &items[0])
}
fn match_scheduled_procedure_step(
    query: &ModalityWorklistScheduledProcedureStep,
    candidate: &Object,
) -> Result<bool> {
    let combine = key_uses_range_matching(&query.scheduled_procedure_step_start_date)
        && key_uses_range_matching(&query.scheduled_procedure_step_start_time);
    if combine
        && !match_mwl_combined_date_time(
            &query.scheduled_procedure_step_start_date,
            &query.scheduled_procedure_step_start_time,
            candidate,
        )?
    {
        return Ok(false);
    }
    let mut keys = vec![
        (
            &query.scheduled_station_ae_title,
            TAG_MWL_SCHEDULED_STATION_AE_TITLE,
            MatchKind::Single,
        ),
        (&query.modality, TAG_MWL_MODALITY, MatchKind::Single),
        (
            &query.scheduled_performing_physician_name,
            TAG_MWL_SCHEDULED_PERFORMING_PHYSICIAN_NAME,
            MatchKind::Wildcard,
        ),
        (
            &query.scheduled_procedure_step_description,
            TAG_MWL_SPS_DESCRIPTION,
            MatchKind::Single,
        ),
        (
            &query.scheduled_procedure_step_id,
            TAG_MWL_SPS_ID,
            MatchKind::Single,
        ),
        (
            &query.scheduled_station_name,
            TAG_MWL_SCHEDULED_STATION_NAME,
            MatchKind::Single,
        ),
        (
            &query.scheduled_procedure_step_location,
            TAG_MWL_SPS_LOCATION,
            MatchKind::Single,
        ),
    ];
    if !combine {
        keys.push((
            &query.scheduled_procedure_step_start_date,
            TAG_MWL_SPS_START_DATE,
            MatchKind::DateRange,
        ));
        keys.push((
            &query.scheduled_procedure_step_start_time,
            TAG_MWL_SPS_START_TIME,
            MatchKind::TimeRange,
        ));
    }
    for (key, tag, kind) in keys {
        if !match_mwl_key(key, candidate, tag, kind)? {
            return Ok(false);
        }
    }
    Ok(true)
}
pub(crate) fn key_has_matching_value(key: &MwlKey) -> bool {
    key.present && key.values.len() == 1 && !key.values[0].trim().is_empty()
}
fn key_uses_range_matching(key: &MwlKey) -> bool {
    key_has_matching_value(key) && key.values[0].contains('-')
}
type TimeKey = (i64, i64, i64, i64, i64, i64, i64);
fn time_key(t: &ZonedTime) -> TimeKey {
    let (y, m, d) = t.date();
    (y, m, d, t.hour(), t.minute(), t.second(), t.nanosecond())
}
fn combine_date_time(date: &ZonedTime, time: &ZonedTime) -> TimeKey {
    let (y, m, d) = date.date();
    (
        y,
        m,
        d,
        time.hour(),
        time.minute(),
        time.second(),
        time.nanosecond(),
    )
}
fn parse_mwl_temporal(value: &str, vr: Vr) -> std::result::Result<ZonedTime, ()> {
    match vr {
        Vr::DA => dcmtime::parse_date(value).map(|d| d.time).map_err(|_| ()),
        Vr::TM => dcmtime::parse_time(value).map(|t| t.time).map_err(|_| ()),
        _ => Err(()),
    }
}
fn zero_time() -> ZonedTime {
    ZonedTime::from_fields(1, 1, 1, 0, 0, 0, 0, 0)
}
fn match_mwl_combined_date_time(
    date_key: &MwlKey,
    time_key_: &MwlKey,
    candidate: &Object,
) -> Result<bool> {
    if date_key.values.len() != 1 || time_key_.values.len() != 1 {
        return Err(Error::new(
            "dicom dimse: MWL date/time matching requires VM 1",
        ));
    }
    let date_query = date_key.values[0].trim();
    let time_query = time_key_.values[0].trim();
    validate_mwl_temporal_query(date_query, Vr::DA)?;
    validate_mwl_temporal_query(time_query, Vr::TM)?;
    let date_value = candidate.get_string(TAG_MWL_SPS_START_DATE);
    let time_value = candidate.get_string(TAG_MWL_SPS_START_TIME);
    let (Some(date_value), Some(time_value)) = (date_value, time_value) else {
        return Ok(false);
    };
    let Ok(candidate_date) = parse_mwl_temporal(date_value.trim(), Vr::DA) else {
        return Err(Error::new("dicom dimse: invalid MWL candidate DA value"));
    };
    let Ok(candidate_time) = parse_mwl_temporal(time_value.trim(), Vr::TM) else {
        return Err(Error::new("dicom dimse: invalid MWL candidate TM value"));
    };
    let candidate_key = combine_date_time(&candidate_date, &candidate_time);
    let (date_lower, date_upper) = split_mwl_range(date_query);
    let (time_lower, time_upper) = split_mwl_range(time_query);
    if !date_lower.is_empty() {
        let lower_date = parse_mwl_temporal(date_lower, Vr::DA).unwrap_or_else(|_| zero_time());
        let lower_time = if time_lower.is_empty() {
            zero_time()
        } else {
            parse_mwl_temporal(time_lower, Vr::TM).unwrap_or_else(|_| zero_time())
        };
        if candidate_key < combine_date_time(&lower_date, &lower_time) {
            return Ok(false);
        }
    }
    if !date_upper.is_empty() {
        let upper_date = parse_mwl_temporal(date_upper, Vr::DA).unwrap_or_else(|_| zero_time());
        let upper_time = if time_upper.is_empty() {
            ZonedTime::from_fields(1, 1, 1, 23, 59, 59, 999_999_999, 0)
        } else {
            parse_mwl_temporal(time_upper, Vr::TM).unwrap_or_else(|_| zero_time())
        };
        if candidate_key > combine_date_time(&upper_date, &upper_time) {
            return Ok(false);
        }
    }
    Ok(true)
}
fn split_mwl_range(value: &str) -> (&str, &str) {
    match value.split_once('-') {
        Some((a, b)) => (a, b),
        None => (value, value),
    }
}
pub(crate) fn match_mwl_key(
    key: &MwlKey,
    candidate: &Object,
    tag: Tag,
    kind: MatchKind,
) -> Result<bool> {
    if !key.present {
        return Ok(true);
    }
    if key.values.len() != 1 {
        return Err(Error::new(format!(
            "dicom dimse: MWL key {tag} requires VM 1"
        )));
    }
    let query = key.values[0].trim();
    if query.is_empty() {
        return Ok(true);
    }
    let values = candidate.get_strings(tag);
    let Some(values) = values.filter(|v| !v.is_empty()) else {
        return Ok(false);
    };
    for value in &values {
        if match_mwl_value(query, value.trim(), kind)? {
            return Ok(true);
        }
    }
    Ok(false)
}
fn match_mwl_value(query: &str, candidate: &str, kind: MatchKind) -> Result<bool> {
    match kind {
        MatchKind::Single => {
            if query.contains(['*', '?']) {
                return Err(Error::new(
                    "dicom dimse: wildcard is not supported for this MWL key",
                ));
            }
            Ok(query == candidate)
        }
        MatchKind::Wildcard => Ok(match_mwl_wildcard_case(query, candidate, true)),
        MatchKind::WildcardSensitive => Ok(match_mwl_wildcard_case(query, candidate, false)),
        MatchKind::DateRange => match_mwl_temporal_range(query, candidate, Vr::DA),
        MatchKind::TimeRange => match_mwl_temporal_range(query, candidate, Vr::TM),
    }
}
fn match_mwl_wildcard_case(pattern: &str, value: &str, fold: bool) -> bool {
    let (pattern, value) = if fold {
        (
            crate::textfmt::to_upper(pattern),
            crate::textfmt::to_upper(value),
        )
    } else {
        (pattern.to_string(), value.to_string())
    };
    let p: Vec<char> = pattern.chars().collect();
    let v: Vec<char> = value.chars().collect();
    let (mut pi, mut vi) = (0usize, 0usize);
    let mut star: Option<usize> = None;
    let mut star_v = 0usize;
    while vi < v.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == v[vi]) {
            pi += 1;
            vi += 1;
            continue;
        }
        if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            pi += 1;
            star_v = vi;
            continue;
        }
        if let Some(s) = star {
            pi = s + 1;
            star_v += 1;
            vi = star_v;
            continue;
        }
        return false;
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}
fn match_mwl_temporal_range(query: &str, candidate: &str, vr: Vr) -> Result<bool> {
    validate_mwl_temporal_query(query, vr)?;
    let Ok(candidate_time) = parse_mwl_temporal(candidate, vr) else {
        return Err(Error::new(format!(
            "dicom dimse: invalid MWL candidate {vr} value"
        )));
    };
    let c = time_key(&candidate_time);
    if !query.contains('-') {
        let Ok(q) = parse_mwl_temporal(query, vr) else {
            return Err(Error::new(format!(
                "dicom dimse: invalid MWL {vr} matching value"
            )));
        };
        return Ok(time_key(&q) == c);
    }
    let bad_range = || Error::new(format!("dicom dimse: invalid MWL {vr} range"));
    if query.matches('-').count() != 1 {
        return Err(bad_range());
    }
    let (lower, upper) = split_mwl_range(query);
    if !lower.is_empty() {
        let Ok(l) = parse_mwl_temporal(lower, vr) else {
            return Err(bad_range());
        };
        if c < time_key(&l) {
            return Ok(false);
        }
    }
    if !upper.is_empty() {
        let Ok(u) = parse_mwl_temporal(upper, vr) else {
            return Err(bad_range());
        };
        if c > time_key(&u) {
            return Ok(false);
        }
    }
    Ok(true)
}
fn validate_mwl_temporal_query(query: &str, vr: Vr) -> Result<()> {
    if query.contains(['*', '?']) {
        return Err(Error::new(format!(
            "dicom dimse: wildcard is not supported for MWL {vr} matching"
        )));
    }
    if !query.contains('-') {
        if parse_mwl_temporal(query, vr).is_err() {
            return Err(Error::new(format!(
                "dicom dimse: invalid MWL {vr} matching value"
            )));
        }
        return Ok(());
    }
    let bad_range = || Error::new(format!("dicom dimse: invalid MWL {vr} range"));
    if query.matches('-').count() != 1 {
        return Err(bad_range());
    }
    let (lower, upper) = split_mwl_range(query);
    if lower.is_empty() && upper.is_empty() {
        return Err(bad_range());
    }
    let mut lo = None;
    let mut hi = None;
    if !lower.is_empty() {
        lo = Some(time_key(
            &parse_mwl_temporal(lower, vr).map_err(|_| bad_range())?,
        ));
    }
    if !upper.is_empty() {
        hi = Some(time_key(
            &parse_mwl_temporal(upper, vr).map_err(|_| bad_range())?,
        ));
    }
    if let (Some(lo), Some(hi)) = (lo, hi)
        && lo > hi
    {
        return Err(bad_range());
    }
    Ok(())
}
const VET_TAGS: [Tag; 9] = [
    Tag::new(0x0010, 0x2201),
    Tag::new(0x0010, 0x2292),
    Tag::new(0x0010, 0x2203),
    Tag::new(0x0010, 0x2297),
    Tag::new(0x0010, 0x2298),
    Tag::new(0x0010, 0x2299),
    Tag::new(0x0010, 0x2202),
    Tag::new(0x0010, 0x2293),
    Tag::new(0x0010, 0x2294),
];
fn optional_scalars(q: &ModalityWorklistQuery) -> Vec<(Tag, Vr, MatchKind, &MwlKey)> {
    let mut keys = vec![
        (
            Tag::new(0x0010, 0x0021),
            Vr::LO,
            MatchKind::WildcardSensitive,
            &q.issuer_of_patient_id,
        ),
        (
            Tag::new(0x0008, 0x0020),
            Vr::DA,
            MatchKind::DateRange,
            &q.study_date,
        ),
        (
            Tag::new(0x0008, 0x0030),
            Vr::TM,
            MatchKind::TimeRange,
            &q.study_time,
        ),
        (
            Tag::new(0x0020, 0x000D),
            Vr::UI,
            MatchKind::Single,
            &q.study_instance_uid,
        ),
        (
            Tag::new(0x0032, 0x1032),
            Vr::PN,
            MatchKind::Wildcard,
            &q.requesting_physician,
        ),
        (
            Tag::new(0x0008, 0x0090),
            Vr::PN,
            MatchKind::Wildcard,
            &q.referring_physician_name,
        ),
        (
            Tag::new(0x0038, 0x0010),
            Vr::LO,
            MatchKind::WildcardSensitive,
            &q.admission_id,
        ),
        (
            Tag::new(0x0040, 0x1003),
            Vr::SH,
            MatchKind::WildcardSensitive,
            &q.requested_procedure_priority,
        ),
    ];
    if let Some(v) = &q.veterinary {
        keys.extend([
            (
                Tag::new(0x0010, 0x2201),
                Vr::LO,
                MatchKind::WildcardSensitive,
                &v.patient_species_description,
            ),
            (
                Tag::new(0x0010, 0x2292),
                Vr::LO,
                MatchKind::WildcardSensitive,
                &v.patient_breed_description,
            ),
            (
                Tag::new(0x0010, 0x2203),
                Vr::CS,
                MatchKind::WildcardSensitive,
                &v.patient_sex_neutered,
            ),
            (
                Tag::new(0x0010, 0x2297),
                Vr::PN,
                MatchKind::Wildcard,
                &v.responsible_person,
            ),
            (
                Tag::new(0x0010, 0x2298),
                Vr::CS,
                MatchKind::WildcardSensitive,
                &v.responsible_person_role,
            ),
            (
                Tag::new(0x0010, 0x2299),
                Vr::LO,
                MatchKind::WildcardSensitive,
                &v.responsible_organization,
            ),
        ]);
    }
    keys
}
fn optional_scalar_mut(q: &mut ModalityWorklistQuery, tag: Tag) -> Option<&mut MwlKey> {
    match (tag.group, tag.element) {
        (0x0010, 0x0021) => Some(&mut q.issuer_of_patient_id),
        (0x0008, 0x0020) => Some(&mut q.study_date),
        (0x0008, 0x0030) => Some(&mut q.study_time),
        (0x0020, 0x000D) => Some(&mut q.study_instance_uid),
        (0x0032, 0x1032) => Some(&mut q.requesting_physician),
        (0x0008, 0x0090) => Some(&mut q.referring_physician_name),
        (0x0038, 0x0010) => Some(&mut q.admission_id),
        (0x0040, 0x1003) => Some(&mut q.requested_procedure_priority),
        _ => {
            let v = q.veterinary.as_mut()?;
            match (tag.group, tag.element) {
                (0x0010, 0x2201) => Some(&mut v.patient_species_description),
                (0x0010, 0x2292) => Some(&mut v.patient_breed_description),
                (0x0010, 0x2203) => Some(&mut v.patient_sex_neutered),
                (0x0010, 0x2297) => Some(&mut v.responsible_person),
                (0x0010, 0x2298) => Some(&mut v.responsible_person_role),
                (0x0010, 0x2299) => Some(&mut v.responsible_organization),
                _ => None,
            }
        }
    }
}
fn optional_sequences(q: &ModalityWorklistQuery) -> Vec<(Tag, &MwlSequenceKey)> {
    let mut keys = vec![
        (
            Tag::new(0x0032, 0x1064),
            &q.requested_procedure_code_sequence,
        ),
        (
            Tag::new(0x0008, 0x0051),
            &q.issuer_of_accession_number_sequence,
        ),
        (Tag::new(0x0038, 0x0014), &q.issuer_of_admission_id_sequence),
        (
            Tag::new(0x0010, 0x0024),
            &q.issuer_of_patient_id_qualifiers_sequence,
        ),
        (Tag::new(0x0010, 0x1002), &q.other_patient_ids_sequence),
    ];
    if let Some(v) = &q.veterinary {
        keys.extend([
            (Tag::new(0x0010, 0x2202), &v.patient_species_code_sequence),
            (Tag::new(0x0010, 0x2293), &v.patient_breed_code_sequence),
            (Tag::new(0x0010, 0x2294), &v.breed_registration_sequence),
        ]);
    }
    keys
}
fn optional_sequence_mut(q: &mut ModalityWorklistQuery, tag: Tag) -> Option<&mut MwlSequenceKey> {
    match (tag.group, tag.element) {
        (0x0032, 0x1064) => Some(&mut q.requested_procedure_code_sequence),
        (0x0008, 0x0051) => Some(&mut q.issuer_of_accession_number_sequence),
        (0x0038, 0x0014) => Some(&mut q.issuer_of_admission_id_sequence),
        (0x0010, 0x0024) => Some(&mut q.issuer_of_patient_id_qualifiers_sequence),
        (0x0010, 0x1002) => Some(&mut q.other_patient_ids_sequence),
        _ => {
            let v = q.veterinary.as_mut()?;
            match (tag.group, tag.element) {
                (0x0010, 0x2202) => Some(&mut v.patient_species_code_sequence),
                (0x0010, 0x2293) => Some(&mut v.patient_breed_code_sequence),
                (0x0010, 0x2294) => Some(&mut v.breed_registration_sequence),
                _ => None,
            }
        }
    }
}
fn optional_scalar_binding(q: &ModalityWorklistQuery, tag: Tag) -> Option<(Vr, MatchKind)> {
    optional_scalars(q)
        .into_iter()
        .find(|(t, ..)| *t == tag)
        .map(|(_, vr, kind, _)| (vr, kind))
}
pub(crate) fn mwl_optional_keys_present(q: &ModalityWorklistQuery) -> bool {
    q.issuer_of_patient_id.present
        || q.study_date.present
        || q.study_time.present
        || q.study_instance_uid.present
        || q.requesting_physician.present
        || q.referring_physician_name.present
        || q.admission_id.present
        || q.requested_procedure_priority.present
        || q.requested_procedure_code_sequence.present
        || q.issuer_of_accession_number_sequence.present
        || q.issuer_of_admission_id_sequence.present
        || q.issuer_of_patient_id_qualifiers_sequence.present
        || q.other_patient_ids_sequence.present
        || q.veterinary.is_some()
}
fn sq_element(tag: Tag, items: Vec<DataSet>) -> Element {
    Element {
        header: ElementHeader {
            tag,
            vr: Vr::SQ,
            length: 0,
            length_set: false,
        },
        value: Value::Sequence(items),
    }
}
fn append_mwl_optional_keys(
    elements: &mut Vec<Element>,
    query: &ModalityWorklistQuery,
) -> Result<()> {
    if !mwl_optional_keys_present(query) {
        return Ok(());
    }
    for (tag, vr, kind, key) in optional_scalars(query) {
        if !key.present {
            continue;
        }
        elements.push(build_mwl_optional_scalar(tag, vr, kind, key)?);
    }
    for (tag, key) in optional_sequences(query) {
        if !key.present {
            continue;
        }
        let element = sq_element(tag, key.items.clone());
        elements.push(normalize_mwl_sequence(&element, None)?);
    }
    Ok(())
}
fn build_mwl_optional_scalar(tag: Tag, vr: Vr, kind: MatchKind, key: &MwlKey) -> Result<Element> {
    let mut validation = key.clone();
    if kind == MatchKind::WildcardSensitive && key.values.len() == 1 {
        validation.values = vec![key.values[0].replace(['*', '?'], "A")];
        if vr == Vr::UR && key.values[0].contains(['*', '?']) {
            if key.values[0].len() as i64 > MAX_IDENTIFIER_BYTES
                || key.values[0].contains(['\0', '\r', '\n', '\\'])
            {
                return Err(mwl_identifier_error(tag, "invalid matching form"));
            }
            validation.values = vec!["urn:dicom:query".to_string()];
        }
    }
    let mut out = Vec::new();
    if append_mwl_key(&mut out, tag, vr, &validation, false, kind).is_err() {
        return Err(mwl_identifier_error(tag, "invalid matching form"));
    }
    let mut element = out.remove(0);
    element.value = Value::Strings(key.values.clone());
    Ok(element)
}
fn parse_mwl_optional_key(
    parsed: &mut ParsedModalityWorklistIdentifier,
    source: &Object,
    element: &Element,
    options: &ModalityWorklistParseOptions,
) -> (bool, Result<()>) {
    let tag = element.tag();
    if VET_TAGS.contains(&tag) {
        if !options.enable_veterinary_matching {
            return (false, Ok(()));
        }
        if parsed.query.veterinary.is_none() {
            parsed.query.veterinary = Some(ModalityWorklistVeterinaryQuery::default());
        }
    }
    if let Some((vr, kind)) = optional_scalar_binding(&parsed.query, tag) {
        if element.vr() != vr {
            return (true, Err(mwl_identifier_error(tag, "unexpected VR")));
        }
        let Some(mut values) = source.get_strings(tag) else {
            return (true, Err(mwl_identifier_error(tag, "invalid string value")));
        };
        if values.is_empty() {
            values = vec![String::new()];
        }
        let key = key_of(values);
        if let Err(e) = build_mwl_optional_scalar(tag, vr, kind, &key) {
            return (true, Err(e));
        }
        if let Some(slot) = optional_scalar_mut(&mut parsed.query, tag) {
            *slot = key;
        }
        parsed.requested_top_level.push(tag);
        parsed.optional_keys.push(tag);
        return (true, Ok(()));
    }
    let is_sequence = optional_sequences(&parsed.query)
        .iter()
        .any(|(t, _)| *t == tag);
    if is_sequence {
        let normalized = match normalize_mwl_sequence(element, Some(source)) {
            Ok(n) => n,
            Err(e) => return (true, Err(e)),
        };
        let items = match normalized.value {
            Value::Sequence(items) => items,
            _ => Vec::new(),
        };
        if let Some(slot) = optional_sequence_mut(&mut parsed.query, tag) {
            *slot = MwlSequenceKey {
                present: true,
                items,
            };
        }
        parsed.requested_top_level.push(tag);
        parsed.optional_keys.push(tag);
        return (true, Ok(()));
    }
    (false, Ok(()))
}
fn mwl_sequence_schema(tag: Tag) -> Option<HashMap<Tag, Vr>> {
    let code: HashMap<Tag, Vr> = [
        (Tag::new(0x0008, 0x0100), Vr::SH),
        (Tag::new(0x0008, 0x0102), Vr::SH),
        (Tag::new(0x0008, 0x0103), Vr::SH),
        (Tag::new(0x0008, 0x0104), Vr::LO),
        (Tag::new(0x0008, 0x0119), Vr::UC),
        (Tag::new(0x0008, 0x0120), Vr::UR),
    ]
    .into_iter()
    .collect();
    match (tag.group, tag.element) {
        (0x0032, 0x1064) | (0x0010, 0x2202) | (0x0010, 0x2293) | (0x0010, 0x2296) => Some(code),
        (0x0008, 0x0051) | (0x0038, 0x0014) => Some(
            [
                (Tag::new(0x0040, 0x0031), Vr::UT),
                (Tag::new(0x0040, 0x0032), Vr::UT),
                (Tag::new(0x0040, 0x0033), Vr::CS),
            ]
            .into_iter()
            .collect(),
        ),
        (0x0010, 0x0024) => Some(
            [
                (Tag::new(0x0040, 0x0032), Vr::UT),
                (Tag::new(0x0040, 0x0033), Vr::CS),
                (Tag::new(0x0040, 0x0035), Vr::CS),
            ]
            .into_iter()
            .collect(),
        ),
        (0x0010, 0x1002) => Some(
            [
                (TAG_MWL_PATIENT_ID, Vr::LO),
                (Tag::new(0x0010, 0x0021), Vr::LO),
                (Tag::new(0x0010, 0x0024), Vr::SQ),
            ]
            .into_iter()
            .collect(),
        ),
        (0x0010, 0x2294) => Some(
            [
                (Tag::new(0x0010, 0x2295), Vr::LO),
                (Tag::new(0x0010, 0x2296), Vr::SQ),
            ]
            .into_iter()
            .collect(),
        ),
        _ => None,
    }
}
fn mwl_sequence_scalar_kind(tag: Tag) -> MatchKind {
    if tag == TAG_MWL_PATIENT_ID {
        MatchKind::Single
    } else {
        MatchKind::WildcardSensitive
    }
}
fn normalize_mwl_sequence(element: &Element, source: Option<&Object>) -> Result<Element> {
    mwl_sequence_tree_admission(
        element,
        DEFAULT_MWL_IDENTIFIER_ELEMENTS,
        DEFAULT_MWL_IDENTIFIER_DEPTH,
    )?;
    let mut remaining = MAX_IDENTIFIER_BYTES;
    mwl_sequence_input_bytes(element, &mut remaining)?;
    remaining = MAX_IDENTIFIER_BYTES;
    normalize_mwl_sequence_at(element, source, &mut remaining)
}
fn mwl_sequence_tree_admission(element: &Element, max_nodes: i64, max_depth: i64) -> Result<()> {
    fn walk(e: &Element, depth: i64, remaining: &mut i64, max_depth: i64) -> Result<()> {
        *remaining -= 1;
        if *remaining < 0 || depth > max_depth {
            return Err(resource_limit());
        }
        if let Value::Sequence(items) = &e.value {
            if depth + 1 > max_depth || items.len() as i64 > *remaining {
                return Err(resource_limit());
            }
            for item in items {
                *remaining -= 1;
                if depth + 2 > max_depth || item.elements.len() as i64 > *remaining {
                    return Err(resource_limit());
                }
                for child in &item.elements {
                    walk(child, depth + 2, remaining, max_depth)?;
                }
            }
        }
        Ok(())
    }
    let mut remaining = max_nodes;
    walk(element, 0, &mut remaining, max_depth)
}
fn mwl_sequence_input_bytes(element: &Element, remaining: &mut i64) -> Result<()> {
    if let Value::Sequence(items) = &element.value {
        for item in items {
            for child in &item.elements {
                mwl_sequence_input_bytes(child, remaining)?;
            }
        }
        return Ok(());
    }
    if matches!(element.value, Value::Nil) {
        return Err(mwl_identifier_error(element.tag(), "missing query value"));
    }
    match encoded_length(&element.value) {
        Some(length) if (length as i64) <= *remaining => {
            *remaining -= length as i64;
            Ok(())
        }
        _ => Err(resource_limit()),
    }
}
fn normalize_mwl_sequence_at(
    element: &Element,
    source: Option<&Object>,
    remaining: &mut i64,
) -> Result<Element> {
    let tag = element.tag();
    let schema = mwl_sequence_schema(tag);
    let items = match &element.value {
        Value::Sequence(items) => Some(items),
        _ => None,
    };
    let (Some(schema), Some(items)) = (schema, items) else {
        return Err(mwl_identifier_error(
            tag,
            "sequence requires zero or one query item",
        ));
    };
    if element.vr() != Vr::SQ || items.len() > 1 {
        return Err(mwl_identifier_error(
            tag,
            "sequence requires zero or one query item",
        ));
    }
    let mut clone = element.clone();
    clone.value = Value::Sequence(Vec::new());
    if items.is_empty() {
        return Ok(clone);
    }
    let mut seen = HashSet::new();
    for nested in &items[0].elements {
        if !seen.insert(nested.tag()) {
            return Err(mwl_identifier_error(
                nested.tag(),
                "duplicate query attribute",
            ));
        }
    }
    let item = match source {
        Some(source) => match source.get_sequence(tag) {
            Some(mut v) if v.len() == 1 => v.remove(0),
            _ => return Err(mwl_identifier_error(tag, "invalid sequence")),
        },
        None => Object::from_data_set(items[0].clone()),
    };
    let mut children = Vec::with_capacity(item.len());
    for nested in item.elements() {
        let Some(&vr) = schema.get(&nested.tag()) else {
            if dict::by_tag(nested.tag()).is_none() || !modality_worklist_return_key_request(nested)
            {
                return Err(mwl_identifier_error(
                    nested.tag(),
                    "unsupported sequence item attribute or VR",
                ));
            }
            children.push(nested.clone());
            continue;
        };
        if nested.vr() != vr {
            return Err(mwl_identifier_error(nested.tag(), "unexpected VR"));
        }
        if vr == Vr::SQ {
            children.push(normalize_mwl_sequence_at(nested, Some(&item), remaining)?);
            continue;
        }
        let Some(mut values) = item.get_strings(nested.tag()) else {
            return Err(mwl_identifier_error(nested.tag(), "invalid string value"));
        };
        if values.is_empty() {
            values = vec![String::new()];
        }
        for value in &values {
            if value.len() as i64 > *remaining {
                return Err(resource_limit());
            }
            *remaining -= value.len() as i64;
        }
        children.push(build_mwl_optional_scalar(
            nested.tag(),
            vr,
            mwl_sequence_scalar_kind(nested.tag()),
            &key_of(values),
        )?);
    }
    clone.value = Value::Sequence(vec![DataSet {
        elements: children,
        item_offset: None,
    }]);
    Ok(clone)
}
fn match_mwl_optional_keys(query: &ModalityWorklistQuery, candidate: &Object) -> Result<bool> {
    if !mwl_optional_keys_present(query) {
        return Ok(true);
    }
    for (tag, _, kind, key) in optional_scalars(query) {
        if key.present
            && key.values.len() == 1
            && key.values[0] == "*"
            && (kind == MatchKind::Wildcard || kind == MatchKind::WildcardSensitive)
        {
            continue;
        }
        if !match_mwl_key(key, candidate, tag, kind)? {
            return Ok(false);
        }
    }
    for (tag, key) in optional_sequences(query) {
        if !key.present {
            continue;
        }
        let normalized = normalize_mwl_sequence(&sq_element(tag, key.items.clone()), None)?;
        let items = match normalized.value {
            Value::Sequence(items) => items,
            _ => Vec::new(),
        };
        let matching = MwlSequenceKey {
            present: true,
            items,
        };
        if !match_mwl_sequence_key(&matching, candidate, tag)? {
            return Ok(false);
        }
    }
    Ok(true)
}
fn mwl_sequence_has_matching_value_at(key: &MwlSequenceKey, depth: i64) -> bool {
    if depth > DEFAULT_MWL_IDENTIFIER_DEPTH {
        return true;
    }
    if !key.present || key.items.len() != 1 {
        return false;
    }
    for e in &key.items[0].elements {
        if let Value::Sequence(items) = &e.value {
            let nested = MwlSequenceKey {
                present: true,
                items: items.clone(),
            };
            if mwl_sequence_has_matching_value_at(&nested, depth + 2) {
                return true;
            }
        } else {
            for value in string_values(e) {
                if !value.trim().is_empty() && value != "*" {
                    return true;
                }
            }
        }
    }
    false
}
pub(crate) fn mwl_optional_has_matching_key(query: &ModalityWorklistQuery) -> bool {
    if !mwl_optional_keys_present(query) {
        return false;
    }
    for (_, _, _, key) in optional_scalars(query) {
        if key_has_matching_value(key) && key.values[0] != "*" {
            return true;
        }
    }
    optional_sequences(query)
        .into_iter()
        .any(|(_, key)| mwl_sequence_has_matching_value_at(key, 0))
}
fn match_mwl_sequence_key(key: &MwlSequenceKey, candidate: &Object, tag: Tag) -> Result<bool> {
    if !mwl_sequence_has_matching_value_at(key, 0) {
        return Ok(true);
    }
    let items = mwl_bounded_candidate_items(candidate, tag)?;
    for item in &items {
        if match_mwl_sequence_item(&key.items[0], item)? {
            return Ok(true);
        }
    }
    Ok(false)
}
fn mwl_bounded_candidate_items(candidate: &Object, tag: Tag) -> Result<Vec<Object>> {
    let Some(element) = candidate.get(tag) else {
        return Ok(Vec::new());
    };
    let Value::Sequence(items) = &element.value else {
        return Err(provider());
    };
    if element.vr() != Vr::SQ {
        return Err(provider());
    }
    mwl_sequence_tree_admission(
        element,
        DEFAULT_MWL_RESPONSE_ELEMENTS,
        DEFAULT_MWL_RESPONSE_DEPTH,
    )?;
    let mut remaining = MAX_IDENTIFIER_BYTES;
    mwl_sequence_input_bytes(element, &mut remaining)?;
    for item in items {
        let mut seen = HashSet::new();
        for child in &item.elements {
            if !seen.insert(child.tag()) {
                return Err(provider());
            }
        }
    }
    candidate.get_sequence(tag).ok_or_else(provider)
}
fn match_mwl_sequence_item(query: &DataSet, candidate: &Object) -> Result<bool> {
    for e in &query.elements {
        if let Value::Sequence(items) = &e.value {
            let key = MwlSequenceKey {
                present: true,
                items: items.clone(),
            };
            if !match_mwl_sequence_key(&key, candidate, e.tag())? {
                return Ok(false);
            }
            continue;
        }
        let values = string_values(e);
        if values.is_empty()
            || values.len() == 1 && (values[0].trim().is_empty() || values[0] == "*")
        {
            continue;
        }
        if !match_mwl_key(
            &key_of(values),
            candidate,
            e.tag(),
            mwl_sequence_scalar_kind(e.tag()),
        )? {
            return Ok(false);
        }
    }
    Ok(true)
}
fn mwl_optional_sequence_for_tag(q: &ModalityWorklistQuery, tag: Tag) -> Option<MwlSequenceKey> {
    optional_sequences(q)
        .into_iter()
        .find(|(t, key)| *t == tag && key.present)
        .map(|(_, key)| key.clone())
}
fn project_mwl_sequence_key(
    key: &MwlSequenceKey,
    candidate: &Object,
    tag: Tag,
    budget: &mut CloneBudget,
    depth: i64,
) -> Result<Element> {
    let Some(element) = candidate.get(tag) else {
        return Err(provider());
    };
    if key.items.is_empty() || key.items[0].elements.is_empty() {
        return budget.clone_element(element, depth);
    }
    let items = mwl_bounded_candidate_items(candidate, tag)?;
    budget.consume_element()?;
    if depth + 2 > budget.max_depth {
        return Err(resource_limit());
    }
    let mut result = Vec::new();
    for item in &items {
        if !match_mwl_sequence_item(&key.items[0], item)? {
            continue;
        }
        budget.consume_element()?;
        let mut projected = DataSet::default();
        for requested in &key.items[0].elements {
            let Some(value) = item.get(requested.tag()) else {
                continue;
            };
            let cloned = if let Value::Sequence(sq) = &requested.value {
                let nested = MwlSequenceKey {
                    present: true,
                    items: sq.clone(),
                };
                project_mwl_sequence_key(&nested, item, requested.tag(), budget, depth + 2)?
            } else {
                budget.clone_element(value, depth + 2)?
            };
            projected.elements.push(cloned);
        }
        result.push(projected);
    }
    let mut element = element.clone();
    element.value = Value::Sequence(result);
    Ok(element)
}
fn mwl_optional_return_key_missing(
    parsed: &ParsedModalityWorklistIdentifier,
    candidate: Option<&Object>,
) -> bool {
    for tag in &parsed.optional_keys {
        let Some(candidate) = candidate.filter(|c| c.has(*tag)) else {
            return true;
        };
        if let Some(key) = mwl_optional_sequence_for_tag(&parsed.query, *tag)
            && mwl_sequence_requested_key_missing(&key, candidate, *tag)
        {
            return true;
        }
    }
    false
}
fn mwl_sequence_requested_key_missing(key: &MwlSequenceKey, candidate: &Object, tag: Tag) -> bool {
    if key.items.is_empty() || key.items[0].elements.is_empty() {
        return false;
    }
    let Ok(items) = mwl_bounded_candidate_items(candidate, tag) else {
        return true;
    };
    for item in &items {
        match match_mwl_sequence_item(&key.items[0], item) {
            Ok(true) => {}
            _ => continue,
        }
        for requested in &key.items[0].elements {
            if !item.has(requested.tag()) {
                return true;
            }
            if let Value::Sequence(sq) = &requested.value {
                let nested = MwlSequenceKey {
                    present: true,
                    items: sq.clone(),
                };
                if mwl_sequence_requested_key_missing(&nested, item, requested.tag()) {
                    return true;
                }
            }
        }
    }
    false
}
fn modality_worklist_return_keys(tag: Tag) -> bool {
    matches!(
        (tag.group, tag.element),
        (0x0040, 0x0500)
            | (0x2200, 0x0005)
            | (0x0032, 0x1065)
            | (0x0008, 0x1110)
            | (0x0040, 0x1004)
            | (0x0038, 0x0300)
            | (0x0008, 0x1120)
            | (0x0010, 0x0041)
            | (0x0010, 0x0043)
            | (0x0010, 0x0011)
            | (0x0010, 0x0014)
            | (0x0010, 0x0101)
            | (0x0010, 0x1030)
            | (0x0010, 0x1020)
            | (0x0040, 0x3001)
            | (0x0038, 0x0500)
            | (0x0010, 0x21C0)
            | (0x0010, 0x2000)
            | (0x0010, 0x2110)
            | (0x0038, 0x0050)
            | (0x0038, 0x0100)
    )
}
fn modality_worklist_step_return_keys(tag: Tag) -> bool {
    matches!(
        (tag.group, tag.element),
        (0x0018, 0x990C)
            | (0x0018, 0x990D)
            | (0x0040, 0x0012)
            | (0x0032, 0x1070)
            | (0x0040, 0x0020)
    )
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ModalityWorklistParseOptions {
    pub accept_unknown_return_keys: bool,
    pub enable_veterinary_matching: bool,
}
fn modality_worklist_return_key_request(element: &Element) -> bool {
    if let Some(entry) = dict::by_tag(element.tag())
        && entry.vr != element.vr()
    {
        return false;
    }
    modality_worklist_empty_return_value(element, 0)
}
fn modality_worklist_empty_return_value(element: &Element, depth: i64) -> bool {
    if element.vr() != Vr::SQ {
        for value in string_values(element) {
            if !value.is_empty() && value != "*" {
                return false;
            }
        }
        if let Value::Raw(raw) = &element.value
            && !raw.is_empty()
            && !element.vr().is_string_like()
        {
            return false;
        }
        return true;
    }
    let Value::Sequence(items) = &element.value else {
        return false;
    };
    if items.len() > 1 || depth > 2 {
        return false;
    }
    items.iter().all(|item| {
        item.elements
            .iter()
            .all(|nested| modality_worklist_empty_return_value(nested, depth + 1))
    })
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParsedModalityWorklistIdentifier {
    pub query: ModalityWorklistQuery,
    pub unsupported_optional_keys: Vec<Tag>,
    optional_keys: Vec<Tag>,
    requested_top_level: Vec<Tag>,
    requested_step: Vec<Tag>,
}
impl ParsedModalityWorklistIdentifier {
    #[doc(hidden)]
    pub fn requested_top_level(&self) -> &[Tag] {
        &self.requested_top_level
    }
    #[doc(hidden)]
    pub fn requested_step(&self) -> &[Tag] {
        &self.requested_step
    }
    #[doc(hidden)]
    pub fn optional_keys(&self) -> &[Tag] {
        &self.optional_keys
    }
    fn unsupported(&self, tag: Tag) -> bool {
        self.optional_keys.contains(&tag) || self.unsupported_optional_keys.contains(&tag)
    }
}
pub fn parse_modality_worklist_identifier(
    identifier: Option<&Object>,
) -> Result<ParsedModalityWorklistIdentifier> {
    parse_modality_worklist_identifier_with_options(
        identifier,
        ModalityWorklistParseOptions::default(),
    )
}
pub fn parse_modality_worklist_identifier_with_options(
    identifier: Option<&Object>,
    options: ModalityWorklistParseOptions,
) -> Result<ParsedModalityWorklistIdentifier> {
    let Some(identifier) = identifier else {
        return Err(mwl_identifier_error(Tag::default(), "missing Identifier"));
    };
    let mut parsed = ParsedModalityWorklistIdentifier::default();
    for element in identifier.elements() {
        let tag = element.tag();
        let top = |kind: MatchKind, vr: Vr| parse_mwl_key(identifier, tag, vr, kind);
        match tag {
            TAG_MWL_SPECIFIC_CHARACTER_SET => {
                if element.vr() != Vr::CS {
                    return Err(mwl_identifier_error(tag, "unexpected VR"));
                }
                let values = identifier.get_strings(tag).unwrap_or_default();
                if values.is_empty() {
                    return Err(mwl_identifier_error(tag, "invalid character set"));
                }
                parsed.query.specific_character_set = values;
            }
            TAG_MWL_TIMEZONE_OFFSET_FROM_UTC => {
                if element.vr() != Vr::SH {
                    return Err(mwl_identifier_error(tag, "unexpected VR"));
                }
                let values = string_values(element);
                if values.len() != 1 || validate_mwl_timezone_offset(&values[0]).is_err() {
                    return Err(mwl_identifier_error(tag, "invalid timezone offset"));
                }
                parsed.query.timezone_offset_from_utc = values[0].clone();
            }
            TAG_MWL_PATIENT_NAME => {
                parsed.query.patient_name = top(MatchKind::Wildcard, Vr::PN)?;
                parsed.requested_top_level.push(tag);
            }
            TAG_MWL_PATIENT_ID => {
                parsed.query.patient_id = top(MatchKind::Single, Vr::LO)?;
                parsed.requested_top_level.push(tag);
            }
            TAG_MWL_PATIENT_BIRTH_DATE => {
                parsed.query.patient_birth_date = top(MatchKind::DateRange, Vr::DA)?;
                parsed.requested_top_level.push(tag);
            }
            TAG_MWL_PATIENT_SEX => {
                parsed.query.patient_sex = top(MatchKind::Single, Vr::CS)?;
                parsed.requested_top_level.push(tag);
            }
            TAG_MWL_ACCESSION_NUMBER => {
                parsed.query.accession_number = top(MatchKind::Single, Vr::SH)?;
                parsed.requested_top_level.push(tag);
            }
            TAG_MWL_REQUESTED_PROCEDURE_ID => {
                parsed.query.requested_procedure_id = top(MatchKind::Single, Vr::SH)?;
                parsed.requested_top_level.push(tag);
            }
            TAG_MWL_REQUESTED_PROCEDURE_DESCRIPTION => {
                parsed.query.requested_procedure_description = top(MatchKind::Single, Vr::LO)?;
                parsed.requested_top_level.push(tag);
            }
            TAG_MWL_SPS_SEQUENCE => {
                let (step, requested, unsupported) =
                    parse_scheduled_procedure_step(identifier, element, &options)?;
                parsed.query.scheduled_procedure_step = Some(step);
                parsed.requested_step = requested;
                parsed.unsupported_optional_keys.extend(unsupported);
            }
            _ => {
                let (handled, r) =
                    parse_mwl_optional_key(&mut parsed, identifier, element, &options);
                if handled {
                    r?;
                    continue;
                }
                accept_return_key(
                    element,
                    modality_worklist_return_keys(tag),
                    &options,
                    "attribute is outside supported MWL model",
                )?;
                parsed.requested_top_level.push(tag);
                parsed.unsupported_optional_keys.push(tag);
            }
        }
    }
    if !parsed.query.timezone_offset_from_utc.is_empty() && !query_has_temporal_key(&parsed.query) {
        return Err(mwl_identifier_error(
            TAG_MWL_TIMEZONE_OFFSET_FROM_UTC,
            "timezone offset requires a DA or TM key",
        ));
    }
    Ok(parsed)
}
type StepParse = (ModalityWorklistScheduledProcedureStep, Vec<Tag>, Vec<Tag>);
fn parse_scheduled_procedure_step(
    identifier: &Object,
    element: &Element,
    options: &ModalityWorklistParseOptions,
) -> Result<StepParse> {
    if element.vr() != Vr::SQ {
        return Err(mwl_identifier_error(element.tag(), "unexpected VR"));
    }
    let items = identifier.get_sequence(TAG_MWL_SPS_SEQUENCE);
    let Some(items) = items.filter(|i| i.len() <= 1) else {
        return Err(mwl_identifier_error(
            element.tag(),
            "sequence requires zero or one item",
        ));
    };
    let mut step = ModalityWorklistScheduledProcedureStep::default();
    let Some(item) = items.first() else {
        return Ok((step, Vec::new(), Vec::new()));
    };
    let mut requested = Vec::with_capacity(item.len());
    let mut unsupported = Vec::new();
    for nested in item.elements() {
        let tag = nested.tag();
        let parse = |vr: Vr, kind: MatchKind| parse_mwl_key(item, tag, vr, kind);
        match tag {
            TAG_MWL_SCHEDULED_STATION_AE_TITLE => {
                step.scheduled_station_ae_title = parse(Vr::AE, MatchKind::Single)?
            }
            TAG_MWL_MODALITY => step.modality = parse(Vr::CS, MatchKind::Single)?,
            TAG_MWL_SPS_START_DATE => {
                step.scheduled_procedure_step_start_date = parse(Vr::DA, MatchKind::DateRange)?
            }
            TAG_MWL_SPS_START_TIME => {
                step.scheduled_procedure_step_start_time = parse(Vr::TM, MatchKind::TimeRange)?
            }
            TAG_MWL_SCHEDULED_PERFORMING_PHYSICIAN_NAME => {
                step.scheduled_performing_physician_name = parse(Vr::PN, MatchKind::Wildcard)?
            }
            TAG_MWL_SPS_DESCRIPTION => {
                step.scheduled_procedure_step_description = parse(Vr::LO, MatchKind::Single)?
            }
            TAG_MWL_SPS_ID => step.scheduled_procedure_step_id = parse(Vr::SH, MatchKind::Single)?,
            TAG_MWL_SCHEDULED_STATION_NAME => {
                step.scheduled_station_name = parse(Vr::SH, MatchKind::Single)?
            }
            TAG_MWL_SPS_LOCATION => {
                step.scheduled_procedure_step_location = parse(Vr::SH, MatchKind::Single)?
            }
            TAG_MWL_SCHEDULED_PROTOCOL_CODE_SEQUENCE => {
                if nested.vr() != Vr::SQ {
                    return Err(mwl_identifier_error(tag, "unexpected VR"));
                }
                accept_return_key(nested, true, options, "protocol is return-only")?;
                unsupported.push(tag);
                requested.push(tag);
                continue;
            }
            _ => {
                accept_return_key(
                    nested,
                    modality_worklist_step_return_keys(tag),
                    options,
                    "nested attribute is outside supported MWL model",
                )?;
                unsupported.push(tag);
                requested.push(tag);
                continue;
            }
        }
        requested.push(tag);
    }
    Ok((step, requested, unsupported))
}
pub(crate) fn modality_worklist_pending_status(
    parsed: &ParsedModalityWorklistIdentifier,
    candidate: Option<&Object>,
) -> u16 {
    use super::command::{STATUS_PENDING, STATUS_PENDING_WARNING};
    if mwl_optional_return_key_missing(parsed, candidate) {
        return STATUS_PENDING_WARNING;
    }
    for unsupported in &parsed.unsupported_optional_keys {
        let nested = parsed.requested_step.contains(unsupported);
        if !nested {
            if !candidate.is_some_and(|c| c.has(*unsupported)) {
                return STATUS_PENDING_WARNING;
            }
            continue;
        }
        let item = candidate
            .and_then(|c| c.get(TAG_MWL_SPS_SEQUENCE))
            .and_then(|e| match &e.value {
                Value::Sequence(items) if items.len() == 1 => Some(&items[0]),
                _ => None,
            });
        let Some(item) = item else {
            return STATUS_PENDING_WARNING;
        };
        if data_set_element(item, *unsupported).is_none() {
            return STATUS_PENDING_WARNING;
        }
    }
    STATUS_PENDING
}
fn parse_mwl_key(source: &Object, tag: Tag, want: Vr, kind: MatchKind) -> Result<MwlKey> {
    match source.get(tag) {
        Some(e) if e.vr() == want => {}
        _ => return Err(mwl_identifier_error(tag, "unexpected VR")),
    }
    let Some(mut values) = source.get_strings(tag) else {
        return Err(mwl_identifier_error(tag, "invalid string value"));
    };
    if values.is_empty() {
        values = vec![String::new()];
    }
    let key = key_of(values);
    let mut scratch = Vec::new();
    if append_mwl_key(&mut scratch, tag, want, &key, false, kind).is_err() {
        return Err(mwl_identifier_error(tag, "invalid matching form"));
    }
    Ok(key)
}
fn accept_return_key(
    element: &Element,
    standard: bool,
    options: &ModalityWorklistParseOptions,
    outside_reason: &str,
) -> Result<()> {
    if !standard && !options.accept_unknown_return_keys {
        return Err(mwl_identifier_error(element.tag(), outside_reason));
    }
    if !modality_worklist_return_key_request(element) {
        return Err(mwl_identifier_error(
            element.tag(),
            "matching on this return key is not supported",
        ));
    }
    Ok(())
}
pub fn project_modality_worklist_result(
    parsed: &ParsedModalityWorklistIdentifier,
    candidate: Option<&Object>,
) -> Result<Object> {
    project_modality_worklist_result_with_limits(
        parsed,
        candidate,
        DEFAULT_MWL_RESPONSE_ELEMENTS,
        MAX_IDENTIFIER_BYTES,
        DEFAULT_MWL_RESPONSE_DEPTH,
    )
}
pub(crate) fn project_modality_worklist_result_with_limits(
    parsed: &ParsedModalityWorklistIdentifier,
    candidate: Option<&Object>,
    max_elements: i64,
    max_bytes: i64,
    max_depth: i64,
) -> Result<Object> {
    let Some(candidate) = candidate else {
        return Err(Error::new("dicom dimse: nil MWL result"));
    };
    if max_elements <= 0 || max_bytes <= 0 || max_depth <= 0 {
        return Err(resource_limit());
    }
    let character_set = candidate.get(TAG_MWL_SPECIFIC_CHARACTER_SET).cloned();
    let mut budget = CloneBudget {
        remaining_elements: max_elements,
        remaining_bytes: max_bytes,
        max_depth,
        max_multiplicity: max_elements,
        character_set: character_set.clone(),
    };
    let mut elements: Vec<Element> = Vec::with_capacity(parsed.requested_top_level.len() + 2);
    let mut timezone = candidate.get(TAG_MWL_TIMEZONE_OFFSET_FROM_UTC).cloned();
    if let Some(tz) = &timezone
        && !parsed.query.timezone_offset_from_utc.is_empty()
    {
        let values = string_values(tz);
        if values.len() != 1 || values[0] != parsed.query.timezone_offset_from_utc {
            return Err(provider());
        }
    }
    for &tag in &parsed.requested_top_level {
        if let Some(element) = candidate.get(tag) {
            let cloned = match mwl_optional_sequence_for_tag(&parsed.query, tag) {
                Some(key) => project_mwl_sequence_key(&key, candidate, tag, &mut budget, 0)?,
                None => budget.clone_element(element, 0)?,
            };
            elements.push(cloned);
        } else if !parsed.unsupported(tag) {
            return Err(Error::new(format!(
                "dicom dimse: MWL result is missing requested attribute {tag}"
            )));
        }
    }
    if parsed.query.scheduled_procedure_step.is_some() {
        let item = candidate
            .get(TAG_MWL_SPS_SEQUENCE)
            .and_then(|e| match &e.value {
                Value::Sequence(items) if items.len() == 1 => Some(items[0].clone()),
                _ => None,
            });
        let Some(item) = item else {
            return Err(Error::new(
                "dicom dimse: MWL result requires one Scheduled Procedure Step item",
            ));
        };
        budget.consume_element()?;
        if max_depth < 2 {
            return Err(resource_limit());
        }
        budget.consume_element()?;
        let requested = &parsed.requested_step;
        if requested.is_empty() && item.elements.len() as i64 > budget.remaining_elements {
            return Err(resource_limit());
        }
        let mut item_elements = Vec::new();
        if requested.is_empty() {
            for element in &item.elements {
                item_elements.push(budget.clone_element(element, 2)?);
            }
        } else {
            for &tag in requested {
                if let Some(element) = data_set_element(&item, tag) {
                    item_elements.push(budget.clone_element(element, 2)?);
                } else if !parsed.unsupported(tag) {
                    return Err(Error::new(format!(
                        "dicom dimse: MWL result is missing requested nested attribute {tag}"
                    )));
                }
            }
        }
        let projected = DataSet {
            elements: item_elements,
            item_offset: None,
        };
        if requested.is_empty() {
            validate_universal_step(&projected, character_set.as_ref())?;
        } else if requested.contains(&TAG_MWL_SPS_DESCRIPTION) {
            validate_step_description_alternative(&projected, character_set.as_ref())?;
        }
        elements.push(sq_element(TAG_MWL_SPS_SEQUENCE, vec![projected]));
    }
    if response_contains_temporal_attribute(&elements) {
        if timezone.is_none() && !parsed.query.timezone_offset_from_utc.is_empty() {
            timezone = Some(str_element(
                TAG_MWL_TIMEZONE_OFFSET_FROM_UTC,
                Vr::SH,
                vec![parsed.query.timezone_offset_from_utc.clone()],
            ));
        }
        if let Some(tz) = &timezone {
            let cloned = budget.clone_element(tz, 0)?;
            elements.insert(0, cloned);
        }
    }
    let required =
        required_response_character_set(&elements, Some(candidate)).map_err(|_| provider())?;
    if let Some(declaration) = required {
        let cloned = budget.clone_element(&declaration, 0)?;
        elements.insert(0, cloned);
    }
    Ok(Object::from_elements(elements))
}
pub(crate) fn data_set_element(ds: &DataSet, tag: Tag) -> Option<&Element> {
    ds.elements.iter().rev().find(|e| e.tag() == tag)
}
fn validate_universal_step(item: &DataSet, character_set: Option<&Element>) -> Result<()> {
    let tags = [
        TAG_MWL_SCHEDULED_STATION_AE_TITLE,
        TAG_MWL_SPS_START_DATE,
        TAG_MWL_SPS_START_TIME,
        TAG_MWL_MODALITY,
        TAG_MWL_SPS_ID,
        TAG_MWL_SCHEDULED_PERFORMING_PHYSICIAN_NAME,
        TAG_MWL_SCHEDULED_STATION_NAME,
        TAG_MWL_SPS_LOCATION,
    ];
    for tag in tags {
        match data_set_element(item, tag) {
            Some(e) if validate_result_element(e, character_set).is_ok() => {}
            _ => return Err(provider()),
        }
    }
    validate_step_description_alternative(item, character_set)
}
pub(crate) fn validate_step_description_alternative(
    item: &DataSet,
    character_set: Option<&Element>,
) -> Result<()> {
    let description = data_set_element(item, TAG_MWL_SPS_DESCRIPTION);
    let protocol = data_set_element(item, TAG_MWL_SCHEDULED_PROTOCOL_CODE_SEQUENCE);
    let description_present = description.is_some_and(|d| {
        let values = string_values(d);
        validate_result_element(d, character_set).is_ok()
            && values.len() == 1
            && !values[0].trim().is_empty()
    });
    let protocol_present =
        protocol.is_some_and(|p| validate_result_element(p, character_set).is_ok());
    if !description_present && !protocol_present {
        return Err(provider());
    }
    Ok(())
}
fn response_contains_temporal_attribute(elements: &[Element]) -> bool {
    elements.iter().any(|e| {
        if e.tag() == TAG_MWL_SPS_START_TIME || e.tag() == Tag::new(0x0008, 0x0030) {
            return true;
        }
        match &e.value {
            Value::Sequence(items) => items
                .iter()
                .any(|item| response_contains_temporal_attribute(&item.elements)),
            _ => false,
        }
    })
}
pub(crate) struct CloneBudget {
    pub(crate) remaining_elements: i64,
    pub(crate) remaining_bytes: i64,
    pub(crate) max_depth: i64,
    pub(crate) max_multiplicity: i64,
    pub(crate) character_set: Option<Element>,
}
impl CloneBudget {
    pub(crate) fn consume_element(&mut self) -> Result<()> {
        if self.remaining_elements <= 0 {
            return Err(resource_limit());
        }
        self.remaining_elements -= 1;
        Ok(())
    }
    pub(crate) fn consume_bytes(&mut self, length: i64) -> Result<()> {
        if length < 0 || length > self.remaining_bytes {
            return Err(resource_limit());
        }
        self.remaining_bytes -= length;
        Ok(())
    }
    fn clone_element(&mut self, element: &Element, depth: i64) -> Result<Element> {
        self.consume_element()?;
        self.reserve_primitive_value(&element.value)?;
        validate_result_element(element, self.character_set.as_ref())?;
        let mut clone = element.clone();
        match &element.value {
            Value::Strings(_) | Value::Raw(_) => {}
            Value::Sequence(items) => {
                if depth + 1 > self.max_depth {
                    return Err(resource_limit());
                }
                if items.len() as i64 > self.remaining_elements {
                    return Err(resource_limit());
                }
                let mut out = Vec::with_capacity(items.len());
                for item in items {
                    if depth + 2 > self.max_depth {
                        return Err(resource_limit());
                    }
                    self.consume_element()?;
                    if item.elements.len() as i64 > self.remaining_elements {
                        return Err(resource_limit());
                    }
                    let mut elements = Vec::with_capacity(item.elements.len());
                    for nested in &item.elements {
                        elements.push(self.clone_element(nested, depth + 2)?);
                    }
                    out.push(DataSet {
                        elements,
                        item_offset: item.item_offset,
                    });
                }
                clone.value = Value::Sequence(out);
            }
            _ => return Err(provider()),
        }
        Ok(clone)
    }
    pub(crate) fn reserve_primitive_value(&mut self, value: &Value) -> Result<()> {
        match value {
            Value::Sequence(items) => {
                if items.len() as i64 > self.remaining_elements {
                    return Err(resource_limit());
                }
                let mut remaining = self.remaining_elements - items.len() as i64;
                for item in items {
                    if item.elements.len() as i64 > remaining {
                        return Err(resource_limit());
                    }
                    remaining -= item.elements.len() as i64;
                }
                return Ok(());
            }
            Value::Fragments { .. } | Value::BulkData(_) | Value::Nil => return Err(provider()),
            Value::Strings(s) => {
                if s.len() as i64 > self.max_multiplicity {
                    return Err(resource_limit());
                }
            }
            Value::Raw(_) => {}
        }
        let Some(length) = encoded_length(value) else {
            return Err(provider());
        };
        self.consume_bytes(length as i64)
    }
}
pub(crate) fn validate_result_element(
    element: &Element,
    character_set: Option<&Element>,
) -> Result<()> {
    let rule = result_rule(element.tag());
    if let Some((want, _)) = rule
        && element.vr() != want
    {
        return Err(provider());
    }
    let mut validate_known = rule.is_some();
    if rule.is_none()
        && let Some(entry) = dict::by_tag(element.tag())
    {
        if element.vr() != entry.vr {
            return Err(provider());
        }
        validate_known = true;
    }
    if element.vr() == Vr::SQ {
        if element.tag() == TAG_MWL_SCHEDULED_PROTOCOL_CODE_SEQUENCE {
            match &element.value {
                Value::Sequence(items)
                    if !items.is_empty()
                        && validate_code_sequence(items, character_set).is_ok() => {}
                _ => return Err(provider()),
            }
        }
        return Ok(());
    }
    if validate_known {
        let mut elements = vec![element.clone()];
        if let Some(cs) = character_set
            && element.tag() != TAG_MWL_SPECIFIC_CHARACTER_SET
            && element.vr().uses_specific_character_set()
        {
            elements = vec![cs.clone(), element.clone()];
        }
        if !strict_valid(elements) {
            return Err(provider());
        }
    }
    if element.tag() == TAG_MWL_SPECIFIC_CHARACTER_SET {
        let values = string_values(element);
        if values.is_empty() || SpecificCharacterSet::parse(&values).is_err() {
            return Err(provider());
        }
    }
    if element.tag() == TAG_MWL_TIMEZONE_OFFSET_FROM_UTC {
        let values = string_values(element);
        if values.len() != 1 || validate_mwl_timezone_offset(&values[0]).is_err() {
            return Err(provider());
        }
    }
    if rule.is_some_and(|(_, required)| required) {
        let values = string_values(element);
        if values.is_empty() || values.iter().any(|v| v.trim().is_empty()) {
            return Err(provider());
        }
    }
    Ok(())
}
pub(crate) fn validate_code_sequence(
    items: &[DataSet],
    character_set: Option<&Element>,
) -> Result<()> {
    let code_value_tags = [
        (Tag::new(0x0008, 0x0100), Vr::SH),
        (Tag::new(0x0008, 0x0119), Vr::UC),
        (Tag::new(0x0008, 0x0120), Vr::UR),
    ];
    for item in items {
        let mut code_values = 0;
        let mut selected = Tag::default();
        let mut selected_value = String::new();
        for (tag, vr) in code_value_tags {
            if let Some(element) = data_set_element(item, tag) {
                let value = validate_code_element(element, vr, true, character_set)?;
                selected = tag;
                selected_value = value;
                code_values += 1;
            }
        }
        if code_values != 1 {
            return Err(provider());
        }
        let length = selected_value.chars().count();
        let is_uri = code_value_is_uri(&selected_value);
        let refused = match (selected.group, selected.element) {
            (0x0008, 0x0100) => length > 16 || is_uri,
            (0x0008, 0x0119) => length <= 16 || is_uri,
            (0x0008, 0x0120) => !is_uri,
            _ => false,
        };
        if refused {
            return Err(provider());
        }
        let scheme = data_set_element(item, Tag::new(0x0008, 0x0102));
        let scheme_required = selected != Tag::new(0x0008, 0x0120);
        if scheme_required && scheme.is_none() {
            return Err(provider());
        }
        if let Some(scheme) = scheme {
            validate_code_element(scheme, Vr::SH, scheme_required, character_set)?;
        }
        if let Some(version) = data_set_element(item, Tag::new(0x0008, 0x0103)) {
            if scheme.is_none() {
                return Err(provider());
            }
            validate_code_element(version, Vr::SH, false, character_set)?;
        }
        if let Some(meaning) = data_set_element(item, Tag::new(0x0008, 0x0104)) {
            validate_code_element(meaning, Vr::LO, false, character_set)?;
        }
    }
    Ok(())
}
fn validate_code_element(
    element: &Element,
    want: Vr,
    required: bool,
    character_set: Option<&Element>,
) -> Result<String> {
    let values = code_element_values(element, character_set);
    let Some(values) = values else {
        return Err(provider());
    };
    if element.vr() != want || values.len() > 1 {
        return Err(provider());
    }
    if required && (values.len() != 1 || values[0].trim().is_empty()) {
        return Err(provider());
    }
    if !required && (values.is_empty() || values[0].trim().is_empty()) {
        return Ok(String::new());
    }
    let mut elements = vec![element.clone()];
    if let Some(cs) = character_set
        && element.vr().uses_specific_character_set()
    {
        elements = vec![cs.clone(), element.clone()];
    }
    if !strict_valid(elements) {
        return Err(provider());
    }
    Ok(values[0].trim().to_string())
}
fn code_element_values(element: &Element, character_set: Option<&Element>) -> Option<Vec<String>> {
    let mut elements = vec![element.clone()];
    if let Some(cs) = character_set
        && element.vr().uses_specific_character_set()
    {
        elements = vec![cs.clone(), element.clone()];
    }
    Object::from_elements(elements).get_strings(element.tag())
}
fn code_value_is_uri(value: &str) -> bool {
    let value = value.trim();
    let Some(parsed) = split_url(value) else {
        return false;
    };
    if parsed.scheme.is_empty() {
        return false;
    }
    let Some(colon) = value.find(':') else {
        return false;
    };
    if colon == value.len() - 1 {
        return false;
    }
    let remainder = &value[colon + 1..];
    if parsed.scheme.eq_ignore_ascii_case("urn") {
        let Some(separator) = remainder.find(':') else {
            return false;
        };
        if separator == 0 || separator == remainder.len() - 1 {
            return false;
        }
        return valid_urn_namespace(&remainder[..separator]);
    }
    parsed.host_set || !parsed.opaque.is_empty() || parsed.path_set
}
fn valid_urn_namespace(namespace: &str) -> bool {
    if namespace.len() < 2 || namespace.len() > 32 {
        return false;
    }
    let last = namespace.len() - 1;
    namespace
        .char_indices()
        .all(|(i, c)| c.is_ascii_alphanumeric() || (c == '-' && i != 0 && i != last))
}
struct SplitUrl {
    scheme: String,
    opaque: String,
    host_set: bool,
    path_set: bool,
}
fn split_url(raw: &str) -> Option<SplitUrl> {
    let (u, frag) = match raw.split_once('#') {
        Some((u, f)) => (u, Some(f)),
        None => (raw, None),
    };
    let url = split_url_inner(u)?;
    if let Some(f) = frag {
        url_unescape_check(f, false)?;
    }
    Some(url)
}
fn split_url_inner(raw: &str) -> Option<SplitUrl> {
    if raw.bytes().any(|b| b < 0x20 || b == 0x7f) {
        return None;
    }
    let mut url = SplitUrl {
        scheme: String::new(),
        opaque: String::new(),
        host_set: false,
        path_set: false,
    };
    if raw == "*" {
        url.path_set = true;
        return Some(url);
    }
    let (scheme, rest) = url_scheme(raw)?;
    url.scheme = scheme.to_ascii_lowercase();
    let rest = if rest.ends_with('?') && rest.matches('?').count() == 1 {
        &rest[..rest.len() - 1]
    } else {
        rest.split_once('?').map_or(rest, |(a, _)| a)
    };
    if !rest.starts_with('/') {
        if !url.scheme.is_empty() {
            url.opaque = rest.to_string();
            return Some(url);
        }
        let segment = rest.split_once('/').map_or(rest, |(a, _)| a);
        if segment.contains(':') {
            return None;
        }
    }
    let mut path = rest;
    if (!url.scheme.is_empty() || !rest.starts_with("///")) && rest.starts_with("//") {
        let authority_rest = &rest[2..];
        let (authority, p) = match authority_rest.find('/') {
            Some(i) => (&authority_rest[..i], &authority_rest[i..]),
            None => (authority_rest, ""),
        };
        let host = url_authority(authority)?;
        url.host_set = !host.is_empty();
        path = p;
    }
    url_unescape_check(path, false)?;
    url.path_set = !path.is_empty();
    Some(url)
}
fn url_scheme(raw: &str) -> Option<(&str, &str)> {
    for (i, c) in raw.bytes().enumerate() {
        match c {
            b'a'..=b'z' | b'A'..=b'Z' => {}
            b'0'..=b'9' | b'+' | b'-' | b'.' => {
                if i == 0 {
                    return Some(("", raw));
                }
            }
            b':' => {
                if i == 0 {
                    return None;
                }
                return Some((&raw[..i], &raw[i + 1..]));
            }
            _ => return Some(("", raw)),
        }
    }
    Some(("", raw))
}
fn url_authority(authority: &str) -> Option<&str> {
    let (userinfo, host) = match authority.rfind('@') {
        Some(i) => (Some(&authority[..i]), &authority[i + 1..]),
        None => (None, authority),
    };
    url_host(host)?;
    if let Some(userinfo) = userinfo {
        let valid = userinfo.bytes().all(|c| {
            c.is_ascii_alphanumeric()
                || matches!(
                    c,
                    b'-' | b'.'
                        | b'_'
                        | b':'
                        | b'~'
                        | b'!'
                        | b'$'
                        | b'&'
                        | b'\''
                        | b'('
                        | b')'
                        | b'*'
                        | b'+'
                        | b','
                        | b';'
                        | b'='
                        | b'%'
                        | b'@'
                )
        });
        if !valid {
            return None;
        }
        url_unescape_check(userinfo, false)?;
    }
    Some(host)
}
fn url_host(host: &str) -> Option<()> {
    let valid_port = |port: &str| {
        port.is_empty()
            || port
                .strip_prefix(':')
                .is_some_and(|p| p.bytes().all(|c| c.is_ascii_digit()))
    };
    if host.starts_with('[') {
        let i = host.rfind(']')?;
        if !valid_port(&host[i + 1..]) {
            return None;
        }
    } else if let Some(i) = host.rfind(':')
        && !valid_port(&host[i..])
    {
        return None;
    }
    url_unescape_check(host, true)
}
fn url_unescape_check(s: &str, host: bool) -> Option<()> {
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' => {
                if i + 2 >= b.len()
                    || !b[i + 1].is_ascii_hexdigit()
                    || !b[i + 2].is_ascii_hexdigit()
                {
                    return None;
                }
                if host && unhex(b[i + 1]) < 8 && &s[i..i + 3] != "%25" {
                    return None;
                }
                i += 3;
            }
            c if host && c < 0x80 && should_escape_host(c) => return None,
            _ => i += 1,
        }
    }
    Some(())
}
fn unhex(c: u8) -> u8 {
    match c {
        b'0'..=b'9' => c - b'0',
        b'a'..=b'f' => c - b'a' + 10,
        b'A'..=b'F' => c - b'A' + 10,
        _ => 0,
    }
}
fn should_escape_host(c: u8) -> bool {
    if c.is_ascii_alphanumeric() {
        return false;
    }
    !matches!(
        c,
        b'-' | b'_'
            | b'.'
            | b'~'
            | b'!'
            | b'$'
            | b'&'
            | b'\''
            | b'('
            | b')'
            | b'*'
            | b'+'
            | b','
            | b';'
            | b'='
            | b':'
            | b'['
            | b']'
            | b'<'
            | b'>'
            | b'"'
    )
}
fn result_rule(tag: Tag) -> Option<(Vr, bool)> {
    Some(match tag {
        TAG_MWL_SPECIFIC_CHARACTER_SET => (Vr::CS, false),
        TAG_MWL_TIMEZONE_OFFSET_FROM_UTC => (Vr::SH, true),
        TAG_MWL_PATIENT_NAME => (Vr::PN, true),
        TAG_MWL_PATIENT_ID => (Vr::LO, true),
        TAG_MWL_PATIENT_BIRTH_DATE => (Vr::DA, false),
        TAG_MWL_PATIENT_SEX => (Vr::CS, false),
        TAG_MWL_ACCESSION_NUMBER => (Vr::SH, false),
        TAG_MWL_REQUESTED_PROCEDURE_ID => (Vr::SH, true),
        TAG_MWL_REQUESTED_PROCEDURE_DESCRIPTION => (Vr::LO, true),
        TAG_MWL_SPS_SEQUENCE | TAG_MWL_SCHEDULED_PROTOCOL_CODE_SEQUENCE => (Vr::SQ, false),
        TAG_MWL_SCHEDULED_STATION_AE_TITLE => (Vr::AE, true),
        TAG_MWL_MODALITY => (Vr::CS, true),
        TAG_MWL_SPS_START_DATE => (Vr::DA, true),
        TAG_MWL_SPS_START_TIME => (Vr::TM, true),
        TAG_MWL_SCHEDULED_PERFORMING_PHYSICIAN_NAME => (Vr::PN, false),
        TAG_MWL_SPS_DESCRIPTION => (Vr::LO, false),
        TAG_MWL_SPS_ID => (Vr::SH, true),
        TAG_MWL_SCHEDULED_STATION_NAME | TAG_MWL_SPS_LOCATION => (Vr::SH, false),
        _ => return None,
    })
}
pub(crate) fn validate_modality_worklist_object_limits(
    value: Option<&Object>,
    max_elements: i64,
    max_depth: i64,
) -> Result<()> {
    let Some(value) = value else {
        return Err(provider());
    };
    fn walk(elements: &[Element], depth: i64, remaining: &mut i64, max_depth: i64) -> bool {
        if depth > max_depth {
            return false;
        }
        for element in elements {
            *remaining -= 1;
            if *remaining < 0 {
                return false;
            }
            let Value::Sequence(items) = &element.value else {
                continue;
            };
            if depth + 1 > max_depth {
                return false;
            }
            for item in items {
                *remaining -= 1;
                if *remaining < 0 || depth + 2 > max_depth {
                    return false;
                }
                if !walk(&item.elements, depth + 2, remaining, max_depth) {
                    return false;
                }
            }
        }
        true
    }
    let mut remaining = max_elements;
    if max_elements <= 0 || max_depth <= 0 || !walk(value.elements(), 0, &mut remaining, max_depth)
    {
        return Err(resource_limit());
    }
    Ok(())
}
pub(crate) fn modality_worklist_has_matching_key(query: &ModalityWorklistQuery) -> bool {
    if mwl_optional_has_matching_key(query) {
        return true;
    }
    let mut keys = vec![
        &query.patient_name,
        &query.patient_id,
        &query.patient_birth_date,
        &query.patient_sex,
        &query.accession_number,
        &query.requested_procedure_id,
        &query.requested_procedure_description,
    ];
    if let Some(step) = &query.scheduled_procedure_step {
        keys.extend([
            &step.scheduled_station_ae_title,
            &step.modality,
            &step.scheduled_procedure_step_start_date,
            &step.scheduled_procedure_step_start_time,
            &step.scheduled_performing_physician_name,
            &step.scheduled_procedure_step_description,
            &step.scheduled_procedure_step_id,
            &step.scheduled_station_name,
            &step.scheduled_procedure_step_location,
        ]);
    }
    keys.into_iter().any(key_has_matching_value)
}
