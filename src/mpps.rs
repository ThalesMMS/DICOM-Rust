#![forbid(unsafe_code)]
use crate::net::dimse::{
    self, AssociationScpOptions, NormalizedClient, NormalizedOperationOptions, NormalizedRequest,
    NormalizedResponse, NormalizedScpOptions, NormalizedScpResult, NormalizedStatusFields,
};
use crate::net::ul::{AcceptedContext, Association, Ctx, PresentationContext};
use crate::object::{
    self, DataSet, Element, ElementHeader, Object, Tag, UNDEFINED_LENGTH, Value, Vr,
};
use regex::Regex;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};
pub const SOP_CLASS_UID: &str = "1.2.840.10008.3.1.2.3.3";
pub const STATUS_IN_PROGRESS: &str = "IN PROGRESS";
pub const STATUS_COMPLETED: &str = "COMPLETED";
pub const STATUS_DISCONTINUED: &str = "DISCONTINUED";
pub const DIMSE_STATUS_SUCCESS: u16 = 0x0000;
pub const DIMSE_STATUS_INVALID_ATTRIBUTE_VALUE: u16 = 0x0106;
pub const DIMSE_STATUS_PROCESSING_FAILURE: u16 = 0x0110;
pub const DIMSE_STATUS_DUPLICATE_SOP_INSTANCE: u16 = 0x0111;
pub const DIMSE_STATUS_NO_SUCH_SOP_INSTANCE: u16 = 0x0112;
pub const DIMSE_STATUS_MISSING_ATTRIBUTE: u16 = 0x0120;
pub const DIMSE_STATUS_MISSING_ATTRIBUTE_VALUE: u16 = 0x0121;
pub const DIMSE_STATUS_RESOURCE_LIMITATION: u16 = 0x0213;
pub const DIMSE_STATUS_INVALID_OBJECT_INSTANCE: u16 = 0x0117;
pub const ERROR_ID_NO_LONGER_UPDATABLE: u16 = 0xA710;
pub const ERROR_COMMENT_NO_LONGER_UPDATABLE: &str =
    "Performed Procedure Step Object may no longer be updated";
const fn tag(g: u16, e: u16) -> Tag {
    Tag::new(g, e)
}
pub const TAG_SPECIFIC_CHARACTER_SET: Tag = tag(0x0008, 0x0005);
pub const TAG_ACCESSION_NUMBER: Tag = tag(0x0008, 0x0050);
pub const TAG_RETRIEVE_AE_TITLE: Tag = tag(0x0008, 0x0054);
pub const TAG_MODALITY: Tag = tag(0x0008, 0x0060);
pub const TAG_CODE_VALUE: Tag = tag(0x0008, 0x0100);
pub const TAG_CODING_SCHEME_DESIGNATOR: Tag = tag(0x0008, 0x0102);
pub const TAG_CODE_MEANING: Tag = tag(0x0008, 0x0104);
pub const TAG_PROCEDURE_CODE_SEQUENCE: Tag = tag(0x0008, 0x1032);
pub const TAG_SERIES_DESCRIPTION: Tag = tag(0x0008, 0x103E);
pub const TAG_PERFORMING_PHYSICIAN_NAME: Tag = tag(0x0008, 0x1050);
pub const TAG_OPERATORS_NAME: Tag = tag(0x0008, 0x1070);
pub const TAG_REFERENCED_STUDY_SEQUENCE: Tag = tag(0x0008, 0x1110);
pub const TAG_REFERENCED_PATIENT_SEQUENCE: Tag = tag(0x0008, 0x1120);
pub const TAG_REFERENCED_IMAGE_SEQUENCE: Tag = tag(0x0008, 0x1140);
pub const TAG_REFERENCED_SOP_CLASS_UID: Tag = tag(0x0008, 0x1150);
pub const TAG_REFERENCED_SOP_INSTANCE_UID: Tag = tag(0x0008, 0x1155);
pub const TAG_PATIENT_NAME: Tag = tag(0x0010, 0x0010);
pub const TAG_PATIENT_ID: Tag = tag(0x0010, 0x0020);
pub const TAG_PATIENT_BIRTH_DATE: Tag = tag(0x0010, 0x0030);
pub const TAG_PATIENT_SEX: Tag = tag(0x0010, 0x0040);
pub const TAG_PROTOCOL_NAME: Tag = tag(0x0018, 0x1030);
pub const TAG_STUDY_INSTANCE_UID: Tag = tag(0x0020, 0x000D);
pub const TAG_SERIES_INSTANCE_UID: Tag = tag(0x0020, 0x000E);
pub const TAG_STUDY_ID: Tag = tag(0x0020, 0x0010);
pub const TAG_REQUESTED_PROCEDURE_DESCRIPTION: Tag = tag(0x0032, 0x1060);
pub const TAG_SCHEDULED_PROCEDURE_STEP_DESCRIPTION: Tag = tag(0x0040, 0x0007);
pub const TAG_SCHEDULED_PROTOCOL_CODE_SEQUENCE: Tag = tag(0x0040, 0x0008);
pub const TAG_SCHEDULED_PROCEDURE_STEP_ID: Tag = tag(0x0040, 0x0009);
pub const TAG_REFERENCED_NON_IMAGE_SEQUENCE: Tag = tag(0x0040, 0x0220);
pub const TAG_PERFORMED_STATION_AE_TITLE: Tag = tag(0x0040, 0x0241);
pub const TAG_PERFORMED_STATION_NAME: Tag = tag(0x0040, 0x0242);
pub const TAG_PERFORMED_LOCATION: Tag = tag(0x0040, 0x0243);
pub const TAG_PERFORMED_PROCEDURE_STEP_START_DATE: Tag = tag(0x0040, 0x0244);
pub const TAG_PERFORMED_PROCEDURE_STEP_START_TIME: Tag = tag(0x0040, 0x0245);
pub const TAG_PERFORMED_PROCEDURE_STEP_END_DATE: Tag = tag(0x0040, 0x0250);
pub const TAG_PERFORMED_PROCEDURE_STEP_END_TIME: Tag = tag(0x0040, 0x0251);
pub const TAG_PERFORMED_PROCEDURE_STEP_STATUS: Tag = tag(0x0040, 0x0252);
pub const TAG_PERFORMED_PROCEDURE_STEP_ID: Tag = tag(0x0040, 0x0253);
pub const TAG_PERFORMED_PROCEDURE_STEP_DESCRIPTION: Tag = tag(0x0040, 0x0254);
pub const TAG_PERFORMED_PROCEDURE_TYPE_DESCRIPTION: Tag = tag(0x0040, 0x0255);
pub const TAG_PERFORMED_PROTOCOL_CODE_SEQUENCE: Tag = tag(0x0040, 0x0260);
pub const TAG_SCHEDULED_STEP_ATTRIBUTES_SEQUENCE: Tag = tag(0x0040, 0x0270);
pub const TAG_COMMENTS_ON_PERFORMED_PROCEDURE_STEP: Tag = tag(0x0040, 0x0280);
pub const TAG_DISCONTINUATION_REASON_CODE_SEQUENCE: Tag = tag(0x0040, 0x0281);
pub const TAG_PERFORMED_SERIES_SEQUENCE: Tag = tag(0x0040, 0x0340);
pub const TAG_REQUESTED_PROCEDURE_ID: Tag = tag(0x0040, 0x1001);
pub const TAG_REASON_FOR_PERFORMED_PROCEDURE_CODES: Tag = tag(0x0040, 0x1012);
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    InvalidDataSet,
    NoSuchInstance,
    Duplicate,
    NotUpdatable,
    FinalState,
    ConcurrentUpdate,
    ResourceLimit,
}
impl Kind {
    pub fn text(self) -> &'static str {
        match self {
            Kind::InvalidDataSet => "mpps: invalid attribute list",
            Kind::NoSuchInstance => "mpps: no such performed procedure step",
            Kind::Duplicate => "mpps: performed procedure step already exists",
            Kind::NotUpdatable => "mpps: performed procedure step may no longer be updated",
            Kind::FinalState => "mpps: final state attributes missing",
            Kind::ConcurrentUpdate => "mpps: concurrent update",
            Kind::ResourceLimit => "mpps: resource limit",
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusError {
    pub status: u16,
    pub tags: Vec<Tag>,
    pub error_comment: String,
    pub error_id: u16,
    pub kind: Kind,
}
impl std::fmt::Display for StatusError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let tags: Vec<String> = self.tags.iter().map(|t| t.to_string()).collect();
        write!(
            f,
            "{} (status {:04X} {})",
            self.kind.text(),
            self.status,
            tags.join(" ")
        )
    }
}
impl std::error::Error for StatusError {}
#[derive(Debug, Clone)]
pub enum Error {
    Sentinel(Kind),
    Wrapped(Kind, String),
    Status(StatusError),
    Dimse(dimse::Error),
    Other(String),
}
pub type Result<T> = std::result::Result<T, Error>;
impl Error {
    pub fn is(&self, kind: Kind) -> bool {
        match self {
            Error::Sentinel(k) | Error::Wrapped(k, _) => *k == kind,
            Error::Status(s) => s.kind == kind,
            _ => false,
        }
    }
    fn status_error(status: u16, tags: Vec<Tag>, kind: Kind) -> Error {
        Error::Status(StatusError {
            status,
            tags,
            error_comment: String::new(),
            error_id: 0,
            kind,
        })
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Sentinel(k) => f.write_str(k.text()),
            Error::Wrapped(k, m) => write!(f, "{}: {m}", k.text()),
            Error::Status(s) => {
                let tags: Vec<String> = s.tags.iter().map(|t| t.to_string()).collect();
                write!(
                    f,
                    "{} (status {:04X} {})",
                    s.kind.text(),
                    s.status,
                    tags.join(" ")
                )
            }
            Error::Dimse(e) => f.write_str(e.text()),
            Error::Other(m) => f.write_str(m),
        }
    }
}
impl std::error::Error for Error {}
impl From<dimse::Error> for Error {
    fn from(e: dimse::Error) -> Error {
        Error::Dimse(e)
    }
}
pub fn status_of(err: Option<&Error>) -> u16 {
    match err {
        None => DIMSE_STATUS_SUCCESS,
        Some(Error::Status(s)) => s.status,
        Some(e) if e.is(Kind::NoSuchInstance) => DIMSE_STATUS_NO_SUCH_SOP_INSTANCE,
        Some(e) if e.is(Kind::Duplicate) => DIMSE_STATUS_DUPLICATE_SOP_INSTANCE,
        Some(e) if e.is(Kind::ResourceLimit) => DIMSE_STATUS_RESOURCE_LIMITATION,
        Some(e) if e.is(Kind::InvalidDataSet) => DIMSE_STATUS_INVALID_ATTRIBUTE_VALUE,
        Some(_) => DIMSE_STATUS_PROCESSING_FAILURE,
    }
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Reference {
    pub sop_class_uid: String,
    pub sop_instance_uid: String,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Code {
    pub value: String,
    pub scheme: String,
    pub meaning: String,
}
#[derive(Debug, Clone, Default)]
pub struct ScheduledStep {
    pub study_instance_uid: String,
    pub referenced_studies: Vec<Reference>,
    pub accession_number: String,
    pub requested_procedure_id: String,
    pub requested_procedure_description: String,
    pub scheduled_procedure_step_id: String,
    pub scheduled_procedure_step_description: String,
    pub scheduled_protocol_codes: Vec<Code>,
}
#[derive(Debug, Clone, Default)]
pub struct CreateAttributes {
    pub specific_character_set: String,
    pub scheduled_steps: Vec<ScheduledStep>,
    pub patient_name: String,
    pub patient_id: String,
    pub patient_birth_date: String,
    pub patient_sex: String,
    pub performed_procedure_step_id: String,
    pub performed_station_ae_title: String,
    pub performed_station_name: String,
    pub performed_location: String,
    pub start_date: String,
    pub start_time: String,
    pub description: String,
    pub type_description: String,
    pub procedure_codes: Vec<Code>,
    pub modality: String,
    pub study_id: String,
    pub performed_protocol_codes: Vec<Code>,
    pub performed_series: Vec<PerformedSeries>,
}
#[derive(Debug, Clone, Default)]
pub struct PerformedSeries {
    pub performing_physician_name: String,
    pub protocol_name: String,
    pub operators_name: String,
    pub series_instance_uid: String,
    pub series_description: String,
    pub retrieve_ae_title: String,
    pub images: Vec<Reference>,
    pub non_images: Vec<Reference>,
}
#[derive(Debug, Clone, Default)]
pub struct Update {
    pub status: String,
    pub end_date: String,
    pub end_time: String,
    pub description: Option<String>,
    pub comments: Option<String>,
    pub performed_series: Option<Vec<PerformedSeries>>,
    pub discontinuation_reason: Option<Code>,
    pub performed_protocols: Option<Vec<Code>>,
    pub procedure_codes: Option<Vec<Code>>,
}
fn text(tag: Tag, vr: Vr, value: &str) -> Element {
    Element {
        header: ElementHeader {
            tag,
            vr,
            length: 0,
            length_set: false,
        },
        value: Value::Strings(if value.is_empty() {
            Vec::new()
        } else {
            vec![value.to_string()]
        }),
    }
}
fn sequence(tag: Tag, items: Vec<DataSet>) -> Element {
    Element {
        header: ElementHeader {
            tag,
            vr: Vr::SQ,
            length: UNDEFINED_LENGTH,
            length_set: true,
        },
        value: Value::Sequence(items),
    }
}
fn item(mut elements: Vec<Element>) -> DataSet {
    elements.sort_by_key(|e| e.tag());
    DataSet {
        elements,
        item_offset: None,
    }
}
fn reference_items(refs: &[Reference]) -> Vec<DataSet> {
    refs.iter()
        .map(|r| {
            item(vec![
                text(TAG_REFERENCED_SOP_CLASS_UID, Vr::UI, &r.sop_class_uid),
                text(TAG_REFERENCED_SOP_INSTANCE_UID, Vr::UI, &r.sop_instance_uid),
            ])
        })
        .collect()
}
fn code_items(codes: &[Code]) -> Vec<DataSet> {
    codes
        .iter()
        .map(|c| {
            item(vec![
                text(TAG_CODE_VALUE, Vr::SH, &c.value),
                text(TAG_CODING_SCHEME_DESIGNATOR, Vr::SH, &c.scheme),
                text(TAG_CODE_MEANING, Vr::LO, &c.meaning),
            ])
        })
        .collect()
}
fn series_items(series: &[PerformedSeries]) -> Vec<DataSet> {
    series
        .iter()
        .map(|s| {
            item(vec![
                text(
                    TAG_PERFORMING_PHYSICIAN_NAME,
                    Vr::PN,
                    &s.performing_physician_name,
                ),
                text(TAG_PROTOCOL_NAME, Vr::LO, &s.protocol_name),
                text(TAG_OPERATORS_NAME, Vr::PN, &s.operators_name),
                text(TAG_SERIES_INSTANCE_UID, Vr::UI, &s.series_instance_uid),
                text(TAG_SERIES_DESCRIPTION, Vr::LO, &s.series_description),
                text(TAG_RETRIEVE_AE_TITLE, Vr::AE, &s.retrieve_ae_title),
                sequence(TAG_REFERENCED_IMAGE_SEQUENCE, reference_items(&s.images)),
                sequence(
                    TAG_REFERENCED_NON_IMAGE_SEQUENCE,
                    reference_items(&s.non_images),
                ),
            ])
        })
        .collect()
}
pub fn build_create(a: &CreateAttributes) -> Result<Object> {
    let steps = a
        .scheduled_steps
        .iter()
        .map(|s| {
            item(vec![
                text(TAG_STUDY_INSTANCE_UID, Vr::UI, &s.study_instance_uid),
                sequence(
                    TAG_REFERENCED_STUDY_SEQUENCE,
                    reference_items(&s.referenced_studies),
                ),
                text(TAG_ACCESSION_NUMBER, Vr::SH, &s.accession_number),
                text(
                    TAG_REQUESTED_PROCEDURE_ID,
                    Vr::SH,
                    &s.requested_procedure_id,
                ),
                text(
                    TAG_REQUESTED_PROCEDURE_DESCRIPTION,
                    Vr::LO,
                    &s.requested_procedure_description,
                ),
                text(
                    TAG_SCHEDULED_PROCEDURE_STEP_ID,
                    Vr::SH,
                    &s.scheduled_procedure_step_id,
                ),
                text(
                    TAG_SCHEDULED_PROCEDURE_STEP_DESCRIPTION,
                    Vr::LO,
                    &s.scheduled_procedure_step_description,
                ),
                sequence(
                    TAG_SCHEDULED_PROTOCOL_CODE_SEQUENCE,
                    code_items(&s.scheduled_protocol_codes),
                ),
            ])
        })
        .collect();
    let mut elements = vec![
        sequence(TAG_SCHEDULED_STEP_ATTRIBUTES_SEQUENCE, steps),
        text(TAG_PATIENT_NAME, Vr::PN, &a.patient_name),
        text(TAG_PATIENT_ID, Vr::LO, &a.patient_id),
        text(TAG_PATIENT_BIRTH_DATE, Vr::DA, &a.patient_birth_date),
        text(TAG_PATIENT_SEX, Vr::CS, &a.patient_sex),
        sequence(TAG_REFERENCED_PATIENT_SEQUENCE, Vec::new()),
        text(
            TAG_PERFORMED_PROCEDURE_STEP_ID,
            Vr::SH,
            &a.performed_procedure_step_id,
        ),
        text(
            TAG_PERFORMED_STATION_AE_TITLE,
            Vr::AE,
            &a.performed_station_ae_title,
        ),
        text(
            TAG_PERFORMED_STATION_NAME,
            Vr::SH,
            &a.performed_station_name,
        ),
        text(TAG_PERFORMED_LOCATION, Vr::SH, &a.performed_location),
        text(
            TAG_PERFORMED_PROCEDURE_STEP_START_DATE,
            Vr::DA,
            &a.start_date,
        ),
        text(
            TAG_PERFORMED_PROCEDURE_STEP_START_TIME,
            Vr::TM,
            &a.start_time,
        ),
        text(
            TAG_PERFORMED_PROCEDURE_STEP_STATUS,
            Vr::CS,
            STATUS_IN_PROGRESS,
        ),
        text(
            TAG_PERFORMED_PROCEDURE_STEP_DESCRIPTION,
            Vr::LO,
            &a.description,
        ),
        text(
            TAG_PERFORMED_PROCEDURE_TYPE_DESCRIPTION,
            Vr::LO,
            &a.type_description,
        ),
        sequence(TAG_PROCEDURE_CODE_SEQUENCE, code_items(&a.procedure_codes)),
        text(TAG_PERFORMED_PROCEDURE_STEP_END_DATE, Vr::DA, ""),
        text(TAG_PERFORMED_PROCEDURE_STEP_END_TIME, Vr::TM, ""),
        text(TAG_MODALITY, Vr::CS, &a.modality),
        text(TAG_STUDY_ID, Vr::SH, &a.study_id),
        sequence(
            TAG_PERFORMED_PROTOCOL_CODE_SEQUENCE,
            code_items(&a.performed_protocol_codes),
        ),
        sequence(
            TAG_PERFORMED_SERIES_SEQUENCE,
            series_items(&a.performed_series),
        ),
    ];
    if !a.specific_character_set.is_empty() {
        elements.push(text(
            TAG_SPECIFIC_CHARACTER_SET,
            Vr::CS,
            &a.specific_character_set,
        ));
    }
    let ds = Object::from_elements(elements);
    validate_create(Some(&ds))?;
    Ok(ds)
}
pub fn build_set(u: &Update) -> Result<Object> {
    let mut elements = Vec::new();
    if !u.status.is_empty() {
        elements.push(text(TAG_PERFORMED_PROCEDURE_STEP_STATUS, Vr::CS, &u.status));
    }
    if !u.end_date.is_empty() || !u.end_time.is_empty() {
        elements.push(text(
            TAG_PERFORMED_PROCEDURE_STEP_END_DATE,
            Vr::DA,
            &u.end_date,
        ));
        elements.push(text(
            TAG_PERFORMED_PROCEDURE_STEP_END_TIME,
            Vr::TM,
            &u.end_time,
        ));
    }
    if let Some(d) = &u.description {
        elements.push(text(TAG_PERFORMED_PROCEDURE_STEP_DESCRIPTION, Vr::LO, d));
    }
    if let Some(c) = &u.comments {
        elements.push(text(TAG_COMMENTS_ON_PERFORMED_PROCEDURE_STEP, Vr::ST, c));
    }
    if let Some(s) = &u.performed_series {
        elements.push(sequence(TAG_PERFORMED_SERIES_SEQUENCE, series_items(s)));
    }
    if let Some(r) = &u.discontinuation_reason {
        elements.push(sequence(
            TAG_DISCONTINUATION_REASON_CODE_SEQUENCE,
            code_items(std::slice::from_ref(r)),
        ));
    }
    if let Some(p) = &u.performed_protocols {
        elements.push(sequence(
            TAG_PERFORMED_PROTOCOL_CODE_SEQUENCE,
            code_items(p),
        ));
    }
    if let Some(p) = &u.procedure_codes {
        elements.push(sequence(TAG_PROCEDURE_CODE_SEQUENCE, code_items(p)));
    }
    if elements.is_empty() {
        return Err(Error::Wrapped(
            Kind::InvalidDataSet,
            "empty modification list".into(),
        ));
    }
    let ds = Object::from_elements(elements);
    validate_modification_shape(Some(&ds))?;
    Ok(ds)
}
pub fn complete(end_date: &str, end_time: &str, series: Vec<PerformedSeries>) -> Result<Object> {
    build_set(&Update {
        status: STATUS_COMPLETED.into(),
        end_date: end_date.into(),
        end_time: end_time.into(),
        performed_series: Some(series),
        ..Default::default()
    })
}
pub fn discontinue(
    end_date: &str,
    end_time: &str,
    series: Vec<PerformedSeries>,
    reason: Option<Code>,
) -> Result<Object> {
    build_set(&Update {
        status: STATUS_DISCONTINUED.into(),
        end_date: end_date.into(),
        end_time: end_time.into(),
        performed_series: Some(series),
        discontinuation_reason: reason,
        ..Default::default()
    })
}
#[derive(Default)]
struct Checker {
    missing: Vec<Tag>,
    empty: Vec<Tag>,
    invalid: Vec<Tag>,
}
fn re(pattern: &'static str) -> Regex {
    Regex::new(pattern).unwrap()
}
fn valid_date(v: &str) -> bool {
    if v.len() != 8 || !v.bytes().all(|b| b.is_ascii_digit()) {
        return false;
    }
    let y: u32 = v[..4].parse().unwrap_or(0);
    let m: u32 = v[4..6].parse().unwrap_or(0);
    let d: u32 = v[6..].parse().unwrap_or(0);
    let leap = (y.is_multiple_of(4) && !y.is_multiple_of(100)) || y.is_multiple_of(400);
    let days = match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return false,
    };
    (1..=days).contains(&d)
}
impl Checker {
    fn err(&self) -> Result<()> {
        if !self.missing.is_empty() {
            return Err(Error::status_error(
                DIMSE_STATUS_MISSING_ATTRIBUTE,
                self.missing.clone(),
                Kind::InvalidDataSet,
            ));
        }
        if !self.empty.is_empty() {
            return Err(Error::status_error(
                DIMSE_STATUS_MISSING_ATTRIBUTE_VALUE,
                self.empty.clone(),
                Kind::InvalidDataSet,
            ));
        }
        if !self.invalid.is_empty() {
            return Err(Error::status_error(
                DIMSE_STATUS_INVALID_ATTRIBUTE_VALUE,
                self.invalid.clone(),
                Kind::InvalidDataSet,
            ));
        }
        Ok(())
    }
    fn value(&mut self, ds: &Object, tag: Tag) -> Option<String> {
        let element = ds.get(tag)?;
        if matches!(element.value, Value::Sequence(_)) {
            self.invalid.push(tag);
            return Some(String::new());
        }
        match ds.lookup_strings(tag) {
            Ok(values) => Some(values.join("\\").trim().to_string()),
            Err(_) => {
                self.invalid.push(tag);
                Some(String::new())
            }
        }
    }
    fn type1(&mut self, ds: &Object, tag: Tag) -> String {
        match self.value(ds, tag) {
            None => {
                self.missing.push(tag);
                String::new()
            }
            Some(v) => {
                if v.is_empty() {
                    self.empty.push(tag);
                }
                v
            }
        }
    }
    fn type2(&mut self, ds: &Object, tag: Tag) -> String {
        match self.value(ds, tag) {
            None => {
                self.missing.push(tag);
                String::new()
            }
            Some(v) => v,
        }
    }
    fn typed(&mut self, ds: &Object, tag: Tag, required: bool) -> String {
        if required {
            self.type1(ds, tag)
        } else {
            self.type2(ds, tag)
        }
    }
    fn uid(&mut self, ds: &Object, tag: Tag, required: bool) {
        let v = self.typed(ds, tag, required);
        if !v.is_empty() && !object::is_valid_uid(v.as_bytes()) {
            self.invalid.push(tag);
        }
    }
    fn date(&mut self, ds: &Object, tag: Tag, required: bool) {
        let v = self.typed(ds, tag, required);
        if !v.is_empty() && !valid_date(&v) {
            self.invalid.push(tag);
        }
    }
    fn clock(&mut self, ds: &Object, tag: Tag, required: bool) {
        let v = self.typed(ds, tag, required);
        if !v.is_empty() && !re(r"^[0-9]{2}([0-9]{2}([0-9]{2}(\.[0-9]{1,6})?)?)?$").is_match(&v) {
            self.invalid.push(tag);
        }
    }
    fn items(&mut self, ds: &Object, tag: Tag, required: bool) -> Vec<Object> {
        let Some(element) = ds.get(tag) else {
            self.missing.push(tag);
            return Vec::new();
        };
        if !matches!(element.value, Value::Sequence(_)) {
            self.invalid.push(tag);
            return Vec::new();
        }
        let items = ds.get_sequence(tag).unwrap_or_default();
        if required && items.is_empty() {
            self.empty.push(tag);
        }
        items
    }
    fn references(&mut self, ds: &Object, tag: Tag) {
        for item in self.items(ds, tag, false) {
            self.uid(&item, TAG_REFERENCED_SOP_CLASS_UID, true);
            self.uid(&item, TAG_REFERENCED_SOP_INSTANCE_UID, true);
        }
    }
    fn codes(&mut self, ds: &Object, tag: Tag) {
        let Some(element) = ds.get(tag) else { return };
        if !matches!(element.value, Value::Sequence(_)) {
            self.invalid.push(tag);
            return;
        }
        for item in ds.get_sequence(tag).unwrap_or_default() {
            self.type1(&item, TAG_CODE_VALUE);
            self.type1(&item, TAG_CODING_SCHEME_DESIGNATOR);
        }
    }
    fn series(&mut self, ds: &Object, required: bool) {
        for item in self.items(ds, TAG_PERFORMED_SERIES_SEQUENCE, required) {
            self.type2(&item, TAG_PERFORMING_PHYSICIAN_NAME);
            self.type1(&item, TAG_PROTOCOL_NAME);
            self.type2(&item, TAG_OPERATORS_NAME);
            self.uid(&item, TAG_SERIES_INSTANCE_UID, true);
            self.type2(&item, TAG_SERIES_DESCRIPTION);
            if self.type2(&item, TAG_RETRIEVE_AE_TITLE).len() > 16 {
                self.invalid.push(TAG_RETRIEVE_AE_TITLE);
            }
            self.references(&item, TAG_REFERENCED_IMAGE_SEQUENCE);
            self.references(&item, TAG_REFERENCED_NON_IMAGE_SEQUENCE);
        }
    }
}
pub fn validate_create(ds: Option<&Object>) -> Result<()> {
    let Some(ds) = ds else {
        return Err(Error::status_error(
            DIMSE_STATUS_MISSING_ATTRIBUTE,
            Vec::new(),
            Kind::InvalidDataSet,
        ));
    };
    let mut c = Checker::default();
    for step in c.items(ds, TAG_SCHEDULED_STEP_ATTRIBUTES_SEQUENCE, true) {
        c.uid(&step, TAG_STUDY_INSTANCE_UID, true);
        c.references(&step, TAG_REFERENCED_STUDY_SEQUENCE);
        c.type2(&step, TAG_ACCESSION_NUMBER);
        c.type2(&step, TAG_REQUESTED_PROCEDURE_ID);
        c.type2(&step, TAG_REQUESTED_PROCEDURE_DESCRIPTION);
        c.type2(&step, TAG_SCHEDULED_PROCEDURE_STEP_ID);
        c.type2(&step, TAG_SCHEDULED_PROCEDURE_STEP_DESCRIPTION);
        if step.get(TAG_SCHEDULED_PROTOCOL_CODE_SEQUENCE).is_none() {
            c.missing.push(TAG_SCHEDULED_PROTOCOL_CODE_SEQUENCE);
        }
        c.codes(&step, TAG_SCHEDULED_PROTOCOL_CODE_SEQUENCE);
    }
    c.type2(ds, TAG_PATIENT_NAME);
    c.type2(ds, TAG_PATIENT_ID);
    c.date(ds, TAG_PATIENT_BIRTH_DATE, false);
    c.type2(ds, TAG_PATIENT_SEX);
    c.references(ds, TAG_REFERENCED_PATIENT_SEQUENCE);
    c.type1(ds, TAG_PERFORMED_PROCEDURE_STEP_ID);
    if c.type1(ds, TAG_PERFORMED_STATION_AE_TITLE).len() > 16 {
        c.invalid.push(TAG_PERFORMED_STATION_AE_TITLE);
    }
    c.type2(ds, TAG_PERFORMED_STATION_NAME);
    c.type2(ds, TAG_PERFORMED_LOCATION);
    c.date(ds, TAG_PERFORMED_PROCEDURE_STEP_START_DATE, true);
    c.clock(ds, TAG_PERFORMED_PROCEDURE_STEP_START_TIME, true);
    let status = c.type1(ds, TAG_PERFORMED_PROCEDURE_STEP_STATUS);
    if !status.is_empty() && status != STATUS_IN_PROGRESS {
        c.invalid.push(TAG_PERFORMED_PROCEDURE_STEP_STATUS);
    }
    c.type2(ds, TAG_PERFORMED_PROCEDURE_STEP_DESCRIPTION);
    c.type2(ds, TAG_PERFORMED_PROCEDURE_TYPE_DESCRIPTION);
    if ds.get(TAG_PROCEDURE_CODE_SEQUENCE).is_none() {
        c.missing.push(TAG_PROCEDURE_CODE_SEQUENCE);
    }
    c.codes(ds, TAG_PROCEDURE_CODE_SEQUENCE);
    c.date(ds, TAG_PERFORMED_PROCEDURE_STEP_END_DATE, false);
    c.clock(ds, TAG_PERFORMED_PROCEDURE_STEP_END_TIME, false);
    let modality = c.type1(ds, TAG_MODALITY);
    if !modality.is_empty() && !re("^[A-Z0-9_ ]{1,16}$").is_match(&modality) {
        c.invalid.push(TAG_MODALITY);
    }
    c.type2(ds, TAG_STUDY_ID);
    if ds.get(TAG_PERFORMED_PROTOCOL_CODE_SEQUENCE).is_none() {
        c.missing.push(TAG_PERFORMED_PROTOCOL_CODE_SEQUENCE);
    }
    c.codes(ds, TAG_PERFORMED_PROTOCOL_CODE_SEQUENCE);
    c.series(ds, false);
    c.err()
}
const SETTABLE_TAGS: [Tag; 12] = [
    TAG_SPECIFIC_CHARACTER_SET,
    TAG_PERFORMED_PROCEDURE_STEP_STATUS,
    TAG_PERFORMED_PROCEDURE_STEP_DESCRIPTION,
    TAG_PERFORMED_PROCEDURE_TYPE_DESCRIPTION,
    TAG_PROCEDURE_CODE_SEQUENCE,
    TAG_REASON_FOR_PERFORMED_PROCEDURE_CODES,
    TAG_PERFORMED_PROCEDURE_STEP_END_DATE,
    TAG_PERFORMED_PROCEDURE_STEP_END_TIME,
    TAG_COMMENTS_ON_PERFORMED_PROCEDURE_STEP,
    TAG_DISCONTINUATION_REASON_CODE_SEQUENCE,
    TAG_PERFORMED_PROTOCOL_CODE_SEQUENCE,
    TAG_PERFORMED_SERIES_SEQUENCE,
];
pub(crate) fn validate_modification_shape(ds: Option<&Object>) -> Result<()> {
    let ds = match ds {
        Some(ds) if !ds.is_empty() => ds,
        _ => {
            return Err(Error::status_error(
                DIMSE_STATUS_INVALID_ATTRIBUTE_VALUE,
                Vec::new(),
                Kind::InvalidDataSet,
            ));
        }
    };
    let mut c = Checker::default();
    for element in ds.elements() {
        if !SETTABLE_TAGS.contains(&element.tag()) {
            c.invalid.push(element.tag());
        }
    }
    if !c.invalid.is_empty() {
        return c.err();
    }
    if ds.get(TAG_PERFORMED_PROCEDURE_STEP_STATUS).is_some() {
        let status = c.type1(ds, TAG_PERFORMED_PROCEDURE_STEP_STATUS);
        if !matches!(
            status.as_str(),
            STATUS_IN_PROGRESS | STATUS_COMPLETED | STATUS_DISCONTINUED | ""
        ) {
            c.invalid.push(TAG_PERFORMED_PROCEDURE_STEP_STATUS);
        }
    }
    if ds.get(TAG_PERFORMED_PROCEDURE_STEP_END_DATE).is_some() {
        c.date(ds, TAG_PERFORMED_PROCEDURE_STEP_END_DATE, false);
    }
    if ds.get(TAG_PERFORMED_PROCEDURE_STEP_END_TIME).is_some() {
        c.clock(ds, TAG_PERFORMED_PROCEDURE_STEP_END_TIME, false);
    }
    for t in [
        TAG_PROCEDURE_CODE_SEQUENCE,
        TAG_REASON_FOR_PERFORMED_PROCEDURE_CODES,
        TAG_DISCONTINUATION_REASON_CODE_SEQUENCE,
        TAG_PERFORMED_PROTOCOL_CODE_SEQUENCE,
    ] {
        c.codes(ds, t);
    }
    if ds.get(TAG_PERFORMED_SERIES_SEQUENCE).is_some() {
        c.series(ds, false);
    }
    let mut invalid = std::mem::take(&mut c.invalid);
    invalid.append(&mut c.missing);
    invalid.append(&mut c.empty);
    c.invalid = invalid;
    c.err()
}
pub fn validate_final(ds: &Object) -> Result<()> {
    let mut c = Checker::default();
    c.date(ds, TAG_PERFORMED_PROCEDURE_STEP_END_DATE, true);
    c.clock(ds, TAG_PERFORMED_PROCEDURE_STEP_END_TIME, true);
    c.series(ds, true);
    let mut tags = c.missing;
    tags.extend(c.empty);
    tags.extend(c.invalid);
    if tags.is_empty() {
        return Ok(());
    }
    Err(Error::Status(StatusError {
        status: DIMSE_STATUS_PROCESSING_FAILURE,
        tags,
        error_comment: "Final State attributes missing or invalid".into(),
        error_id: 0,
        kind: Kind::FinalState,
    }))
}
#[derive(Debug, Clone, Default)]
pub struct Instance {
    pub sop_instance_uid: String,
    pub status: String,
    pub attributes: DataSet,
    pub version: u64,
    pub created_at: Option<SystemTime>,
    pub updated_at: Option<SystemTime>,
    pub create_digest: [u8; 32],
}
impl Instance {
    pub fn data_set(&self) -> Object {
        Object::from_data_set(self.attributes.clone())
    }
}
pub trait Repository: Send + Sync {
    fn create(&self, ctx: &Ctx, instance: Instance) -> Result<bool>;
    fn get(&self, ctx: &Ctx, sop_instance_uid: &str) -> Result<Instance>;
    fn compare_and_swap(
        &self,
        ctx: &Ctx,
        sop_instance_uid: &str,
        expected_version: u64,
        next: Instance,
    ) -> Result<Instance>;
}
fn ctx_err(ctx: &Ctx) -> Result<()> {
    match dimse::context_err(ctx) {
        Some(e) => Err(Error::Dimse(e)),
        None => Ok(()),
    }
}
pub struct MemoryRepository {
    max: usize,
    items: Mutex<HashMap<String, Instance>>,
}
impl MemoryRepository {
    pub fn new(max_instances: i64) -> Result<MemoryRepository> {
        if max_instances < 0 {
            return Err(Error::Sentinel(Kind::ResourceLimit));
        }
        Ok(MemoryRepository {
            max: if max_instances == 0 {
                1000
            } else {
                max_instances as usize
            },
            items: Mutex::new(HashMap::new()),
        })
    }
}
impl Repository for MemoryRepository {
    fn create(&self, ctx: &Ctx, mut instance: Instance) -> Result<bool> {
        ctx_err(ctx)?;
        let mut items = self.items.lock().unwrap();
        if items.contains_key(&instance.sop_instance_uid) {
            return Ok(false);
        }
        if items.len() >= self.max {
            return Err(Error::Sentinel(Kind::ResourceLimit));
        }
        instance.version = 1;
        items.insert(instance.sop_instance_uid.clone(), instance);
        Ok(true)
    }
    fn get(&self, ctx: &Ctx, uid: &str) -> Result<Instance> {
        ctx_err(ctx)?;
        let items = self.items.lock().unwrap();
        items
            .get(uid)
            .cloned()
            .ok_or(Error::Sentinel(Kind::NoSuchInstance))
    }
    fn compare_and_swap(
        &self,
        ctx: &Ctx,
        uid: &str,
        expected: u64,
        mut next: Instance,
    ) -> Result<Instance> {
        ctx_err(ctx)?;
        let mut items = self.items.lock().unwrap();
        let Some(current) = items.get(uid) else {
            return Err(Error::Sentinel(Kind::NoSuchInstance));
        };
        if current.version != expected || next.sop_instance_uid != uid {
            return Err(Error::Sentinel(Kind::ConcurrentUpdate));
        }
        next.version = expected + 1;
        items.insert(uid.to_string(), next.clone());
        Ok(next)
    }
}
#[derive(Debug, Clone, Copy, Default)]
pub struct Limits {
    pub max_data_set_bytes: i64,
    pub max_performed_series: i64,
    pub max_references: i64,
}
pub type Clock = Arc<dyn Fn() -> SystemTime + Send + Sync>;
pub type UidGenerator = Arc<dyn Fn() -> std::result::Result<String, crate::BoxError> + Send + Sync>;
#[derive(Clone, Default)]
pub struct ServiceOptions {
    pub limits: Limits,
    pub clock: Option<Clock>,
    pub uid_generator: Option<UidGenerator>,
    pub accept_identical_create_replay: bool,
}
#[derive(Clone)]
pub struct Service {
    repository: Arc<dyn Repository>,
    limits: Limits,
    clock: Clock,
    uids: UidGenerator,
    replay: bool,
}
impl Service {
    pub fn new(repository: Arc<dyn Repository>, options: ServiceOptions) -> Result<Service> {
        let mut limits = options.limits;
        if limits.max_data_set_bytes < 0
            || limits.max_performed_series < 0
            || limits.max_references < 0
        {
            return Err(Error::Sentinel(Kind::ResourceLimit));
        }
        if limits.max_data_set_bytes == 0 {
            limits.max_data_set_bytes = 1 << 20;
        }
        if limits.max_performed_series == 0 {
            limits.max_performed_series = 256;
        }
        if limits.max_references == 0 {
            limits.max_references = 10000;
        }
        Ok(Service {
            repository,
            limits,
            clock: options.clock.unwrap_or_else(|| Arc::new(SystemTime::now)),
            uids: options
                .uid_generator
                .unwrap_or_else(|| Arc::new(|| Ok(object::inherit::new_uid()))),
            replay: options.accept_identical_create_replay,
        })
    }
    pub fn create(
        &self,
        ctx: &Ctx,
        sop_instance_uid: &str,
        attributes: Option<&Object>,
    ) -> Result<Instance> {
        let mut uid = sop_instance_uid.trim().to_string();
        if uid.is_empty() {
            uid = (self.uids)().map_err(|e| Error::Other(e.to_string()))?;
        }
        if !object::is_valid_uid(uid.as_bytes()) {
            return Err(Error::status_error(
                DIMSE_STATUS_INVALID_OBJECT_INSTANCE,
                Vec::new(),
                Kind::InvalidDataSet,
            ));
        }
        validate_create(attributes)?;
        let attributes = attributes.expect("validated");
        self.check_limits(attributes)?;
        let digest = data_set_digest(attributes)?;
        let now = (self.clock)();
        let instance = Instance {
            sop_instance_uid: uid.clone(),
            status: STATUS_IN_PROGRESS.into(),
            attributes: attributes.to_data_set(),
            version: 0,
            created_at: Some(now),
            updated_at: Some(now),
            create_digest: digest,
        };
        if self.repository.create(ctx, instance)? {
            return self.repository.get(ctx, &uid);
        }
        let existing = self.repository.get(ctx, &uid)?;
        if self.replay && existing.version == 1 && existing.create_digest == digest {
            return Ok(existing);
        }
        Err(Error::status_error(
            DIMSE_STATUS_DUPLICATE_SOP_INSTANCE,
            Vec::new(),
            Kind::Duplicate,
        ))
    }
    pub fn set(
        &self,
        ctx: &Ctx,
        sop_instance_uid: &str,
        modifications: Option<&Object>,
    ) -> Result<Instance> {
        validate_modification_shape(modifications)?;
        let modifications = modifications.expect("validated");
        for attempt in 0..16u32 {
            let current = self.repository.get(ctx, sop_instance_uid)?;
            if current.status != STATUS_IN_PROGRESS {
                return Err(Error::Status(StatusError {
                    status: DIMSE_STATUS_PROCESSING_FAILURE,
                    tags: Vec::new(),
                    error_comment: ERROR_COMMENT_NO_LONGER_UPDATABLE.into(),
                    error_id: ERROR_ID_NO_LONGER_UPDATABLE,
                    kind: Kind::NotUpdatable,
                }));
            }
            let merged = merge_modifications(&current.attributes, modifications)?;
            let merged_object = Object::from_data_set(merged.clone());
            let mut next = current.clone();
            next.attributes = merged;
            next.updated_at = Some((self.clock)());
            if let Ok(status) = modifications.lookup_string(TAG_PERFORMED_PROCEDURE_STEP_STATUS) {
                next.status = status.trim().to_string();
            }
            if next.status == STATUS_COMPLETED || next.status == STATUS_DISCONTINUED {
                validate_final(&merged_object)?;
            }
            self.check_limits(&merged_object)?;
            match self
                .repository
                .compare_and_swap(ctx, sop_instance_uid, current.version, next)
            {
                Err(e) if e.is(Kind::ConcurrentUpdate) => {
                    let pause = Duration::from_micros(u64::from(attempt + 1) * (1 + jitter(500)));
                    sleep_context(ctx, pause)?;
                    continue;
                }
                r => return r,
            }
        }
        Err(Error::status_error(
            DIMSE_STATUS_PROCESSING_FAILURE,
            Vec::new(),
            Kind::ConcurrentUpdate,
        ))
    }
    pub fn get(&self, ctx: &Ctx, sop_instance_uid: &str) -> Result<Instance> {
        self.repository.get(ctx, sop_instance_uid)
    }
    fn check_limits(&self, ds: &Object) -> Result<()> {
        let series = ds
            .get_sequence(TAG_PERFORMED_SERIES_SEQUENCE)
            .unwrap_or_default();
        let limit = || {
            Error::status_error(
                DIMSE_STATUS_RESOURCE_LIMITATION,
                vec![TAG_PERFORMED_SERIES_SEQUENCE],
                Kind::ResourceLimit,
            )
        };
        if series.len() as i64 > self.limits.max_performed_series {
            return Err(limit());
        }
        let references: usize = series
            .iter()
            .map(|item| {
                item.get_sequence(TAG_REFERENCED_IMAGE_SEQUENCE)
                    .map_or(0, |v| v.len())
                    + item
                        .get_sequence(TAG_REFERENCED_NON_IMAGE_SEQUENCE)
                        .map_or(0, |v| v.len())
            })
            .sum();
        if references as i64 > self.limits.max_references {
            return Err(limit());
        }
        Ok(())
    }
    pub fn normalized_options(&self) -> NormalizedScpOptions {
        let create_service = self.clone();
        let set_service = self.clone();
        NormalizedScpOptions {
            max_data_set_bytes: self.limits.max_data_set_bytes,
            presentation_context_policy: Some(Arc::new(|pc: &AcceptedContext, class: &str| {
                if pc.abstract_syntax_uid != SOP_CLASS_UID || class != SOP_CLASS_UID {
                    return Err(dimse::normalized_presentation_context_error(
                        pc.id,
                        &pc.abstract_syntax_uid,
                        class,
                    ));
                }
                Ok(())
            })),
            create_handler: Some(Arc::new(
                move |ctx: &Ctx, req: &NormalizedRequest, ds: Option<&Object>| {
                    let r = create_service.create(ctx, &req.sop_instance_uid, ds);
                    let err = r.as_ref().err();
                    let mut response = NormalizedResponse {
                        status: status_of(err),
                        status_fields: status_fields(err),
                        ..Default::default()
                    };
                    if let Ok(instance) = &r {
                        response.affected_sop_instance_uid = instance.sop_instance_uid.clone();
                    }
                    (
                        NormalizedScpResult {
                            response,
                            data_set: None,
                        },
                        r.err().map(to_dimse),
                    )
                },
            )),
            set_handler: Some(Arc::new(
                move |ctx: &Ctx, req: &NormalizedRequest, ds: Option<&Object>| {
                    let r = set_service.set(ctx, &req.sop_instance_uid, ds);
                    let err = r.as_ref().err();
                    let response = NormalizedResponse {
                        status: status_of(err),
                        status_fields: status_fields(err),
                        ..Default::default()
                    };
                    (
                        NormalizedScpResult {
                            response,
                            data_set: None,
                        },
                        r.err().map(to_dimse),
                    )
                },
            )),
            ..Default::default()
        }
    }
    pub fn association_scp_options(&self) -> AssociationScpOptions {
        AssociationScpOptions {
            normalized_scp: Some(self.normalized_options()),
            ..Default::default()
        }
    }
}
fn to_dimse(e: Error) -> dimse::Error {
    match e {
        Error::Dimse(d) => d,
        other => dimse::Error::new(other.to_string()),
    }
}
fn status_fields(err: Option<&Error>) -> Option<NormalizedStatusFields> {
    let Some(Error::Status(s)) = err else {
        return None;
    };
    let fields = NormalizedStatusFields {
        error_comment: s.error_comment.clone(),
        error_id: (s.error_id != 0).then_some(s.error_id),
        offending_elements: Vec::new(),
        attribute_identifier_list: s.tags.clone(),
    };
    if fields.error_comment.is_empty()
        && fields.error_id.is_none()
        && fields.attribute_identifier_list.is_empty()
    {
        return None;
    }
    Some(fields)
}
fn element_string_value(e: &Element) -> String {
    match &e.value {
        Value::Raw(raw) => {
            String::from_utf8_lossy(crate::object::text::trim_text_value_bytes(e.vr(), raw))
                .into_owned()
        }
        Value::Strings(s) => s
            .first()
            .map(|v| crate::object::text::trim_text_value(e.vr(), v).to_string())
            .unwrap_or_default(),
        _ => String::new(),
    }
}
fn charset_of(ds: &DataSet) -> String {
    ds.elements
        .iter()
        .find(|e| e.tag() == TAG_SPECIFIC_CHARACTER_SET)
        .map(|e| element_string_value(e).trim().to_string())
        .unwrap_or_default()
}
fn merge_modifications(stored: &DataSet, modifications: &Object) -> Result<DataSet> {
    let mut merged = stored.clone();
    for element in modifications.elements() {
        if element.tag() == TAG_SPECIFIC_CHARACTER_SET {
            if charset_of(stored) != element_string_value(element).trim() {
                return Err(Error::status_error(
                    DIMSE_STATUS_INVALID_ATTRIBUTE_VALUE,
                    vec![TAG_SPECIFIC_CHARACTER_SET],
                    Kind::InvalidDataSet,
                ));
            }
            continue;
        }
        match merged
            .elements
            .iter_mut()
            .find(|e| e.tag() == element.tag())
        {
            Some(slot) => *slot = element.clone(),
            None => merged.elements.push(element.clone()),
        }
    }
    merged.elements.sort_by_key(|e| e.tag());
    Ok(merged)
}
fn data_set_digest(ds: &Object) -> Result<[u8; 32]> {
    let bytes = object::writer::write_data_set(ds, object::transfer::EXPLICIT_VR_LITTLE_ENDIAN.uid)
        .map_err(|_| {
            Error::status_error(
                DIMSE_STATUS_INVALID_ATTRIBUTE_VALUE,
                Vec::new(),
                Kind::InvalidDataSet,
            )
        })?;
    Ok(Sha256::digest(&bytes).into())
}
fn sleep_context(ctx: &Ctx, d: Duration) -> Result<()> {
    let end = std::time::Instant::now() + d;
    while std::time::Instant::now() < end {
        ctx_err(ctx)?;
        std::thread::sleep(Duration::from_micros(50).min(end - std::time::Instant::now()));
    }
    ctx_err(ctx)
}
fn jitter(max: u64) -> u64 {
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| u64::from(d.subsec_nanos()));
    nanos % max
}
pub fn presentation_context() -> PresentationContext {
    PresentationContext {
        abstract_syntax_uid: SOP_CLASS_UID.into(),
        transfer_syntax_uids: vec![
            object::transfer::EXPLICIT_VR_LITTLE_ENDIAN.uid.into(),
            object::transfer::IMPLICIT_VR_LITTLE_ENDIAN.uid.into(),
        ],
        ..Default::default()
    }
}
#[derive(Debug, Clone, Default)]
pub struct ExchangeResult {
    pub status: u16,
    pub sop_instance_uid: String,
    pub status_fields: Option<NormalizedStatusFields>,
}
pub struct Client<'a> {
    normalized: NormalizedClient<'a>,
}
impl<'a> Client<'a> {
    pub fn new(assoc: &'a Association) -> Client<'a> {
        Client {
            normalized: NormalizedClient::new(assoc),
        }
    }
    fn options(ctx: Option<&Ctx>) -> NormalizedOperationOptions {
        let mut o = NormalizedOperationOptions::default();
        o.operation.context = ctx.cloned();
        o
    }
    pub fn create(
        &self,
        ctx: Option<&Ctx>,
        sop_instance_uid: &str,
        attributes: Option<&Object>,
    ) -> (ExchangeResult, Result<()>) {
        if let Err(e) = validate_create(attributes) {
            return (ExchangeResult::default(), Err(e));
        }
        let (exchange, r) = self.normalized.create(
            &Client::options(ctx),
            NormalizedRequest {
                sop_class_uid: SOP_CLASS_UID.into(),
                sop_instance_uid: sop_instance_uid.into(),
                command_data_set_type: dimse::DATA_SET_PRESENT,
                ..Default::default()
            },
            attributes,
        );
        let mut result = ExchangeResult {
            sop_instance_uid: sop_instance_uid.into(),
            ..Default::default()
        };
        if let Some(rsp) = exchange.response {
            result.status = rsp.status;
            result.status_fields = rsp.status_fields;
            if !rsp.affected_sop_instance_uid.is_empty() {
                result.sop_instance_uid = rsp.affected_sop_instance_uid;
            }
        }
        (result, r.map_err(Error::Dimse))
    }
    pub fn set(
        &self,
        ctx: Option<&Ctx>,
        sop_instance_uid: &str,
        modifications: Option<&Object>,
    ) -> (ExchangeResult, Result<()>) {
        if let Err(e) = validate_modification_shape(modifications) {
            return (ExchangeResult::default(), Err(e));
        }
        let (exchange, r) = self.normalized.set(
            &Client::options(ctx),
            NormalizedRequest {
                sop_class_uid: SOP_CLASS_UID.into(),
                sop_instance_uid: sop_instance_uid.into(),
                ..Default::default()
            },
            modifications,
        );
        let mut result = ExchangeResult {
            sop_instance_uid: sop_instance_uid.into(),
            ..Default::default()
        };
        if let Some(rsp) = exchange.response {
            result.status = rsp.status;
            result.status_fields = rsp.status_fields;
        }
        (result, r.map_err(Error::Dimse))
    }
}
#[doc(hidden)]
pub fn check_modification_list(modifications: Option<&Object>) -> Result<()> {
    validate_modification_shape(modifications)
}
