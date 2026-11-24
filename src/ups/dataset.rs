use super::*;
use crate::net::dimse;
use crate::net::ul::Ctx;
use crate::object::text::{trim_text_value, trim_text_value_bytes};
use crate::object::{self, Element, ElementHeader, Value, Vr, transfer};
use crate::validation::{Mode, ValidationOptions, validate_data_set_with};
pub(crate) const MAX_UPS_STATUS_RECIPIENTS: i64 = 1_000_000;
pub(crate) const TAG_CODE_VALUE: Tag = Tag::new(0x0008, 0x0100);
pub(crate) const TAG_CODING_SCHEME_DESIGNATOR: Tag = Tag::new(0x0008, 0x0102);
pub(crate) const TAG_CODING_SCHEME_VERSION: Tag = Tag::new(0x0008, 0x0103);
pub(crate) const TAG_CODE_MEANING: Tag = Tag::new(0x0008, 0x0104);
pub(crate) const TAG_LONG_CODE_VALUE: Tag = Tag::new(0x0008, 0x0119);
pub(crate) const TAG_URN_CODE_VALUE: Tag = Tag::new(0x0008, 0x0120);
#[derive(Debug, Clone, Default)]
pub struct ScheduledStepAttributes {
    pub priority: String,
    pub procedure_step_label: String,
    pub worklist_label: String,
    pub start_date_time: String,
    pub input_readiness_state: String,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Code {
    pub value: String,
    pub scheme: String,
    pub meaning: String,
}
#[derive(Debug, Clone, Default)]
pub struct PerformedProcedureAttributes {
    pub station: Code,
    pub workitem: Code,
    pub start_date_time: String,
    pub end_date_time: String,
}
pub fn new_data_set(elements: Vec<Element>) -> Object {
    Object::from_elements(elements)
}
pub fn string_element(tag: Tag, vr: Vr, values: &[&str]) -> Element {
    Element {
        header: ElementHeader {
            tag,
            vr,
            length: 0,
            length_set: false,
        },
        value: Value::Strings(values.iter().map(|v| v.to_string()).collect()),
    }
}
pub(crate) fn string_element_owned(tag: Tag, vr: Vr, values: Vec<String>) -> Element {
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
pub fn empty_sequence(tag: Tag) -> Element {
    sequence_element(tag, Vec::new())
}
pub(crate) fn sequence_element(tag: Tag, items: Vec<DataSet>) -> Element {
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
pub(crate) fn item_of(elements: Vec<Element>) -> DataSet {
    DataSet {
        elements,
        item_offset: None,
    }
}
pub fn build_scheduled_step(a: &ScheduledStepAttributes) -> Result<Object> {
    if a.priority.trim().is_empty()
        || a.procedure_step_label.trim().is_empty()
        || a.start_date_time.trim().is_empty()
        || a.input_readiness_state.trim().is_empty()
    {
        return Err(Kind::InvalidDataSet.into());
    }
    Ok(new_data_set(vec![
        string_element(TAG_TRANSACTION_UID, Vr::UI, &[""]),
        string_element(TAG_SOP_CLASS_UID, Vr::UI, &[PUSH_SOP_CLASS_UID]),
        string_element(
            TAG_SCHEDULED_PROCEDURE_STEP_PRIORITY,
            Vr::CS,
            &[&a.priority],
        ),
        string_element(TAG_PROCEDURE_STEP_LABEL, Vr::LO, &[&a.procedure_step_label]),
        string_element(TAG_WORKLIST_LABEL, Vr::LO, &[&a.worklist_label]),
        empty_sequence(TAG_SCHEDULED_PROCESSING_PARAMETERS_SEQUENCE),
        empty_sequence(TAG_SCHEDULED_STATION_NAME_CODE_SEQUENCE),
        empty_sequence(TAG_SCHEDULED_STATION_CLASS_CODE_SEQUENCE),
        empty_sequence(TAG_SCHEDULED_STATION_GEOGRAPHIC_LOCATION_CODE_SEQUENCE),
        empty_sequence(TAG_SCHEDULED_HUMAN_PERFORMERS_SEQUENCE),
        string_element(
            TAG_SCHEDULED_PROCEDURE_STEP_START_DATE_TIME,
            Vr::DT,
            &[&a.start_date_time],
        ),
        empty_sequence(TAG_SCHEDULED_WORKITEM_CODE_SEQUENCE),
        string_element(TAG_COMMENTS_ON_SCHEDULED_PROCEDURE_STEP, Vr::LT, &[""]),
        string_element(
            TAG_INPUT_READINESS_STATE,
            Vr::CS,
            &[&a.input_readiness_state],
        ),
        empty_sequence(TAG_INPUT_INFORMATION_SEQUENCE),
        string_element(TAG_STUDY_INSTANCE_UID, Vr::UI, &[""]),
        string_element(TAG_PATIENT_NAME, Vr::PN, &[""]),
        string_element(TAG_PATIENT_ID, Vr::LO, &[""]),
        string_element(TAG_ISSUER_OF_PATIENT_ID, Vr::LO, &[""]),
        empty_sequence(TAG_ISSUER_OF_PATIENT_ID_QUALIFIERS_SEQUENCE),
        empty_sequence(TAG_OTHER_PATIENT_IDS_SEQUENCE),
        string_element(TAG_PATIENT_BIRTH_DATE, Vr::DA, &[""]),
        string_element(TAG_PATIENT_SEX, Vr::CS, &[""]),
        string_element(TAG_ADMISSION_ID, Vr::LO, &[""]),
        empty_sequence(TAG_ISSUER_OF_ADMISSION_ID_SEQUENCE),
        string_element(TAG_ADMITTING_DIAGNOSES_DESCRIPTION, Vr::LO, &[""]),
        empty_sequence(TAG_ADMITTING_DIAGNOSES_CODE_SEQUENCE),
        empty_sequence(TAG_REFERENCED_REQUEST_SEQUENCE),
        string_element(TAG_PROCEDURE_STEP_STATE, Vr::CS, &[STATE_SCHEDULED]),
        empty_sequence(TAG_PROCEDURE_STEP_PROGRESS_INFORMATION_SEQUENCE),
        empty_sequence(TAG_UNIFIED_PROCEDURE_STEP_PERFORMED_PROCEDURE_SEQUENCE),
    ]))
}
pub fn build_performed_procedure(a: &PerformedProcedureAttributes) -> Result<Object> {
    validate_code(&a.station)?;
    validate_code(&a.workitem)?;
    if a.start_date_time.is_empty() || a.end_date_time.is_empty() {
        return Err(Kind::InvalidDataSet.into());
    }
    let item = item_of(vec![
        code_sequence(TAG_PERFORMED_STATION_NAME_CODE_SEQUENCE, &a.station),
        string_element(
            TAG_PERFORMED_PROCEDURE_STEP_START_DATE_TIME,
            Vr::DT,
            &[&a.start_date_time],
        ),
        code_sequence(TAG_PERFORMED_WORKITEM_CODE_SEQUENCE, &a.workitem),
        string_element(
            TAG_PERFORMED_PROCEDURE_STEP_END_DATE_TIME,
            Vr::DT,
            &[&a.end_date_time],
        ),
        empty_sequence(TAG_OUTPUT_INFORMATION_SEQUENCE),
    ]);
    Ok(new_data_set(vec![sequence_element(
        TAG_UNIFIED_PROCEDURE_STEP_PERFORMED_PROCEDURE_SEQUENCE,
        vec![item],
    )]))
}
pub fn build_discontinuation_progress(
    reason: &Code,
    cancellation_date_time: &str,
) -> Result<Object> {
    validate_code(reason)?;
    let cancellation = cancellation_date_time.trim();
    if crate::dcmtime::parse_datetime(cancellation).is_err() {
        return Err(Kind::InvalidDataSet.into());
    }
    let item = item_of(vec![
        string_element(
            TAG_PROCEDURE_STEP_CANCELLATION_DATE_TIME,
            Vr::DT,
            &[cancellation],
        ),
        code_sequence(
            TAG_PROCEDURE_STEP_DISCONTINUATION_REASON_CODE_SEQUENCE,
            reason,
        ),
    ]);
    Ok(new_data_set(vec![sequence_element(
        TAG_PROCEDURE_STEP_PROGRESS_INFORMATION_SEQUENCE,
        vec![item],
    )]))
}
fn validate_code(code: &Code) -> Result<()> {
    if code.value.trim().is_empty()
        || code.scheme.trim().is_empty()
        || code.meaning.trim().is_empty()
    {
        return Err(Kind::InvalidDataSet.into());
    }
    Ok(())
}
pub(crate) fn code_sequence(tag: Tag, code: &Code) -> Element {
    sequence_element(
        tag,
        vec![item_of(vec![
            string_element(TAG_CODE_VALUE, Vr::SH, &[&code.value]),
            string_element(TAG_CODING_SCHEME_DESIGNATOR, Vr::SH, &[&code.scheme]),
            string_element(TAG_CODE_MEANING, Vr::LO, &[&code.meaning]),
        ])],
    )
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Limits {
    pub max_data_set_bytes: i64,
    pub max_data_set_elements: i64,
    pub max_data_set_depth: i64,
    pub max_cas_attempts: i64,
    pub max_status_recipients: i64,
    pub max_subscription_filter_scanned: i64,
}
pub(crate) fn normalize_limits(mut l: Limits) -> Result<Limits> {
    if l.max_data_set_bytes < 0
        || l.max_data_set_elements < 0
        || l.max_data_set_depth < 0
        || l.max_cas_attempts < 0
        || l.max_status_recipients < 0
        || l.max_status_recipients > MAX_UPS_STATUS_RECIPIENTS
        || l.max_subscription_filter_scanned < 0
        || l.max_subscription_filter_scanned > MAX_QUERY_SCANNED_STEPS
    {
        return Err(Kind::ResourceLimit.into());
    }
    if l.max_data_set_bytes == 0 {
        l.max_data_set_bytes = 4 << 20;
    }
    if l.max_data_set_elements == 0 {
        l.max_data_set_elements = 16_384;
    }
    if l.max_data_set_depth == 0 {
        l.max_data_set_depth = 32;
    }
    if l.max_cas_attempts == 0 {
        l.max_cas_attempts = 64;
    }
    if l.max_status_recipients == 0 {
        l.max_status_recipients = 10_000;
    }
    if l.max_subscription_filter_scanned == 0 {
        l.max_subscription_filter_scanned = 100_000;
    }
    Ok(l)
}
pub(crate) fn default_limits() -> Limits {
    normalize_limits(Limits::default()).unwrap()
}
pub(crate) fn ctx_err(ctx: &Ctx) -> Result<()> {
    match dimse::context_err(ctx) {
        Some(e) => Err(Error::Dimse(e)),
        None => Ok(()),
    }
}
struct CloneBudget<'a> {
    ctx: &'a Ctx,
    bytes: i64,
    elements: i64,
    max_bytes: i64,
    max_elems: i64,
    max_depth: i64,
}
pub(crate) fn clone_data_set(ctx: &Ctx, ds: &DataSet, limits: &Limits) -> Result<DataSet> {
    let mut b = CloneBudget {
        ctx,
        bytes: 0,
        elements: 0,
        max_bytes: limits.max_data_set_bytes,
        max_elems: limits.max_data_set_elements,
        max_depth: limits.max_data_set_depth,
    };
    b.clone_data_set(ds, 0)
}
impl CloneBudget<'_> {
    fn clone_data_set(&mut self, ds: &DataSet, depth: i64) -> Result<DataSet> {
        ctx_err(self.ctx)?;
        if depth > self.max_depth || ds.elements.len() as i64 > self.max_elems - self.elements {
            return Err(Kind::ResourceLimit.into());
        }
        let mut out = DataSet {
            elements: Vec::with_capacity(ds.elements.len()),
            item_offset: ds.item_offset,
        };
        for e in &ds.elements {
            self.elements += 1;
            let value = self.clone_value(&e.value, depth)?;
            out.elements.push(Element {
                header: e.header,
                value,
            });
        }
        Ok(out)
    }
    fn reserve(&mut self, count: i64) -> Result<()> {
        if count < 0 || count > self.max_bytes - self.bytes {
            return Err(Kind::ResourceLimit.into());
        }
        self.bytes += count;
        Ok(())
    }
    fn clone_value(&mut self, value: &Value, depth: i64) -> Result<Value> {
        match value {
            Value::Raw(raw) => {
                self.reserve(raw.len() as i64)?;
                Ok(Value::Raw(raw.clone()))
            }
            Value::Strings(values) => {
                if values.len() as i64 > self.max_elems - self.elements {
                    return Err(Kind::ResourceLimit.into());
                }
                for v in values {
                    self.reserve(v.len() as i64)?;
                }
                Ok(Value::Strings(values.clone()))
            }
            Value::Sequence(items) => {
                if depth + 2 > self.max_depth || items.len() as i64 > self.max_elems - self.elements
                {
                    return Err(Kind::ResourceLimit.into());
                }
                let mut out = Vec::with_capacity(items.len());
                for item in items {
                    self.elements += 1;
                    out.push(self.clone_data_set(item, depth + 2)?);
                }
                Ok(Value::Sequence(out))
            }
            Value::Nil | Value::Fragments { .. } | Value::BulkData(_) => {
                Err(Kind::InvalidDataSet.into())
            }
        }
    }
}
pub(crate) fn validate_data_set(ctx: &Ctx, ds: &DataSet, limits: &Limits) -> Result<()> {
    ctx_err(ctx).map_err(|_| Error::from(Kind::InvalidDataSet))?;
    let object = Object::from_data_set(ds.clone());
    let opts = ValidationOptions {
        mode: Mode::Strict,
        dictionary: true,
        max_findings: 1,
        stop_first: true,
        max_depth: limits.max_data_set_depth,
        max_elements: limits.max_data_set_elements,
        big_endian: object.value_big_endian(),
        ..Default::default()
    };
    let (_, err) = validate_data_set_with(&object.to_data_set(), &opts);
    match err {
        None => Ok(()),
        Some(crate::validation::Error::Limit(_)) => Err(Kind::ResourceLimit.into()),
        Some(_) => Err(Kind::InvalidDataSet.into()),
    }
}
pub(crate) fn validate_data_set_encoding(ctx: &Ctx, ds: &DataSet) -> Result<()> {
    ctx_err(ctx)?;
    let object = Object::from_data_set(ds.clone());
    if object::writer::write_data_set(&object, transfer::EXPLICIT_VR_LITTLE_ENDIAN.uid).is_err() {
        return Err(Kind::InvalidDataSet.into());
    }
    ctx_err(ctx)
}
pub(crate) fn data_set_element(ds: &DataSet, tag: Tag) -> Option<&Element> {
    ds.elements.iter().rev().find(|e| e.tag() == tag)
}
pub(crate) fn data_set_string(ds: &DataSet, tag: Tag) -> Option<String> {
    data_set_element(ds, tag).map(element_string_value)
}
pub(crate) fn put_element(ds: &mut DataSet, element: Element) {
    match ds.elements.iter_mut().find(|e| e.tag() == element.tag()) {
        Some(slot) => *slot = element,
        None => ds.elements.push(element),
    }
}
pub(crate) fn remove_element(ds: &mut DataSet, tag: Tag) {
    if let Some(i) = ds.elements.iter().position(|e| e.tag() == tag) {
        ds.elements.remove(i);
    }
}
pub(crate) fn element_string_value(e: &Element) -> String {
    match &e.value {
        Value::Raw(raw) => trim_text_value(e.vr(), &String::from_utf8_lossy(raw)).to_string(),
        Value::Strings(s) => s
            .first()
            .map(|v| trim_text_value(e.vr(), v).to_string())
            .unwrap_or_default(),
        _ => String::new(),
    }
}
#[doc(hidden)]
pub fn element_string_values(e: &Element) -> Vec<String> {
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
pub(crate) fn sequence_items(e: &Element) -> Option<&Vec<DataSet>> {
    match &e.value {
        Value::Sequence(items) => Some(items),
        _ => None,
    }
}
