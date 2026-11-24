#![forbid(unsafe_code)]
use super::command::*;
use super::controls::context_err;
use super::error::{Error, Kind, Result};
use super::message::*;
use super::normalized::*;
use crate::net::ul::{Association, Ctx};
use crate::object::{self, DataSet, Element, ElementHeader, Object, Syntax, Tag, Value, Vr};
use crate::textfmt::quote;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant, SystemTime};
pub const STORAGE_COMMITMENT_PUSH_MODEL_SOP_CLASS_UID: &str = "1.2.840.10008.1.20.1";
pub const STORAGE_COMMITMENT_PUSH_MODEL_SOP_INSTANCE_UID: &str = "1.2.840.10008.1.20.1.1";
pub const STORAGE_COMMITMENT_ACTION_TYPE_ID: u16 = 1;
pub const STORAGE_COMMITMENT_EVENT_TYPE_SUCCESS: u16 = 1;
pub const STORAGE_COMMITMENT_EVENT_TYPE_FAILURES: u16 = 2;
pub const STORAGE_COMMITMENT_EVENT_TYPE_ID: u16 = STORAGE_COMMITMENT_EVENT_TYPE_SUCCESS;
pub const STATUS_STORAGE_COMMITMENT_PROCESSING_FAILURE: u16 = 0x0110;
pub const STATUS_STORAGE_COMMITMENT_RESOURCE_LIMITATION: u16 = 0x0213;
pub const STORAGE_COMMITMENT_FAILURE_REASON_PROCESSING_FAILURE: u16 = 0x0110;
pub const STORAGE_COMMITMENT_FAILURE_REASON_NO_SUCH_SOP_INSTANCE: u16 = 0x0112;
pub const STORAGE_COMMITMENT_FAILURE_REASON_CLASS_INSTANCE_CONFLICT: u16 = 0x0119;
pub const STORAGE_COMMITMENT_FAILURE_REASON_SOP_CLASS_NOT_SUPPORTED: u16 = 0x0122;
pub const STORAGE_COMMITMENT_FAILURE_REASON_DUPLICATE_TRANSACTION_UID: u16 = 0x0131;
pub const STORAGE_COMMITMENT_FAILURE_REASON_RESOURCE_LIMITATION: u16 = 0x0213;
pub(crate) const DEFAULT_STORAGE_COMMITMENT_MAX_REFERENCES: usize = 10_000;
pub const STORAGE_COMMITMENT_TRANSACTION_UID: Tag = Tag::new(0x0008, 0x1195);
pub const STORAGE_COMMITMENT_REFERENCED_SOP_CLASS: Tag = Tag::new(0x0008, 0x1150);
pub const STORAGE_COMMITMENT_REFERENCED_SOP_INSTANCE: Tag = Tag::new(0x0008, 0x1155);
pub const STORAGE_COMMITMENT_REFERENCED_SOP_SEQUENCE: Tag = Tag::new(0x0008, 0x1199);
pub const STORAGE_COMMITMENT_FAILED_SOP_SEQUENCE: Tag = Tag::new(0x0008, 0x1198);
pub const STORAGE_COMMITMENT_FAILURE_REASON: Tag = Tag::new(0x0008, 0x1197);
pub const ERR_INVALID_REQUEST: &str = "dicom dimse: invalid storage commitment request";
#[doc(hidden)]
pub const ERR_INVALID_RESULT: &str = "dicom dimse: invalid storage commitment result";
pub const ERR_RESOURCE_LIMIT: &str = "dicom dimse: storage commitment resource limit";
#[doc(hidden)]
pub const ERR_TRANSACTION_UNKNOWN: &str =
    "dicom dimse: storage commitment transaction UID not found";
#[doc(hidden)]
pub const ERR_TRANSACTION_TIMEOUT: &str = "dicom dimse: storage commitment transaction timed out";
pub(crate) fn invalid_request() -> Error {
    Error::sentinel(Kind::StorageCommitmentInvalidRequest)
}
pub(crate) fn invalid_result() -> Error {
    Error::sentinel(Kind::StorageCommitmentInvalidResult)
}
pub(crate) fn resource_limit() -> Error {
    Error::sentinel(Kind::StorageCommitmentResourceLimit)
}
pub fn classify_storage_commitment_status(status: u16) -> &'static str {
    if status == 0 { "success" } else { "failure" }
}
pub fn check_storage_commitment_status(op: &str, status: u16) -> Option<Error> {
    if status == 0 {
        return None;
    }
    Some(
        Error::new(format!(
            "dicom dimse: {op}: storage commitment status 0x{status:04X} (failure)"
        ))
        .also(Kind::StorageCommitmentStatus)
        .with_detail(super::error::Detail::Status(status)),
    )
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NActionRequest {
    pub requested_sop_class_uid: String,
    pub requested_sop_instance_uid: String,
    pub message_id: u16,
    pub action_type_id: u16,
}
impl NActionRequest {
    pub fn command_set(&self) -> Vec<Element> {
        NormalizedRequest {
            command_field: N_ACTION_RQ,
            sop_class_uid: self.requested_sop_class_uid.clone(),
            message_id: self.message_id,
            command_data_set_type: DATA_SET_PRESENT,
            sop_instance_uid: self.requested_sop_instance_uid.clone(),
            type_id: self.action_type_id,
            ..Default::default()
        }
        .command_set()
    }
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CommitmentResponse {
    pub affected_sop_class_uid: String,
    pub affected_sop_instance_uid: String,
    pub message_id_being_responded_to: u16,
    pub status: u16,
    pub has_reply: bool,
}
pub type NActionResponse = CommitmentResponse;
pub type NEventReportResponse = CommitmentResponse;
impl CommitmentResponse {
    fn command_set_for(&self, field: u16, type_id: u16) -> Vec<Element> {
        NormalizedResponse {
            command_field: field,
            affected_sop_class_uid: self.affected_sop_class_uid.clone(),
            message_id_being_responded_to: self.message_id_being_responded_to,
            command_data_set_type: if self.has_reply {
                DATA_SET_PRESENT
            } else {
                NO_DATA_SET
            },
            status: self.status,
            affected_sop_instance_uid: self.affected_sop_instance_uid.clone(),
            type_id: self.has_reply.then_some(type_id),
            status_fields: None,
        }
        .command_set()
    }
    #[doc(hidden)]
    pub fn action_command_set(&self) -> Vec<Element> {
        self.command_set_for(N_ACTION_RSP, STORAGE_COMMITMENT_ACTION_TYPE_ID)
    }
    #[doc(hidden)]
    pub fn event_report_command_set(&self) -> Vec<Element> {
        self.command_set_for(N_EVENT_REPORT_RSP, STORAGE_COMMITMENT_EVENT_TYPE_ID)
    }
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NEventReportRequest {
    pub affected_sop_class_uid: String,
    pub affected_sop_instance_uid: String,
    pub message_id: u16,
    pub event_type_id: u16,
}
impl NEventReportRequest {
    pub fn command_set(&self) -> Vec<Element> {
        NormalizedRequest {
            command_field: N_EVENT_REPORT_RQ,
            sop_class_uid: self.affected_sop_class_uid.clone(),
            message_id: self.message_id,
            command_data_set_type: DATA_SET_PRESENT,
            sop_instance_uid: self.affected_sop_instance_uid.clone(),
            type_id: self.event_type_id,
            ..Default::default()
        }
        .command_set()
    }
}
fn want_uid(service: &str, what: &str, got: &str, want: &str) -> Result<()> {
    if got != want {
        return Err(Error::new(format!(
            "dicom dimse: {service} {what} {}, want {}",
            quote(got),
            quote(want)
        )));
    }
    Ok(())
}
fn check_action_request(class: &str, instance: &str, type_id: u16) -> Result<()> {
    want_uid(
        "N-ACTION request",
        "RequestedSOPClassUID",
        class,
        STORAGE_COMMITMENT_PUSH_MODEL_SOP_CLASS_UID,
    )?;
    want_uid(
        "N-ACTION request",
        "RequestedSOPInstanceUID",
        instance,
        STORAGE_COMMITMENT_PUSH_MODEL_SOP_INSTANCE_UID,
    )?;
    if type_id != STORAGE_COMMITMENT_ACTION_TYPE_ID {
        return Err(Error::new(format!(
            "dicom dimse: N-ACTION request ActionTypeID {type_id}, want StorageCommitmentActionTypeID {STORAGE_COMMITMENT_ACTION_TYPE_ID}"
        )));
    }
    Ok(())
}
fn check_response(service: &str, class: &str, instance: &str) -> Result<()> {
    want_uid(
        service,
        "AffectedSOPClassUID",
        class,
        STORAGE_COMMITMENT_PUSH_MODEL_SOP_CLASS_UID,
    )?;
    want_uid(
        service,
        "AffectedSOPInstanceUID",
        instance,
        STORAGE_COMMITMENT_PUSH_MODEL_SOP_INSTANCE_UID,
    )
}
pub(crate) fn valid_event_type(t: u16) -> bool {
    t == STORAGE_COMMITMENT_EVENT_TYPE_SUCCESS || t == STORAGE_COMMITMENT_EVENT_TYPE_FAILURES
}
fn check_event_request(class: &str, instance: &str, type_id: u16) -> Result<()> {
    want_uid(
        "N-EVENT-REPORT request",
        "AffectedSOPClassUID",
        class,
        STORAGE_COMMITMENT_PUSH_MODEL_SOP_CLASS_UID,
    )?;
    want_uid(
        "N-EVENT-REPORT request",
        "AffectedSOPInstanceUID",
        instance,
        STORAGE_COMMITMENT_PUSH_MODEL_SOP_INSTANCE_UID,
    )?;
    if !valid_event_type(type_id) {
        return Err(Error::new(
            "dicom dimse: N-EVENT-REPORT request has unsupported EventTypeID",
        ));
    }
    Ok(())
}
pub fn parse_n_action_request(command: &Object) -> Result<NActionRequest> {
    let g = parse_normalized_request(command, N_ACTION_RQ)?;
    check_action_request(&g.sop_class_uid, &g.sop_instance_uid, g.type_id)?;
    if g.command_data_set_type == NO_DATA_SET {
        return Err(Error::new(format!(
            "dicom dimse: N-ACTION request dataset type 0x{:04X}, want dataset present",
            g.command_data_set_type
        )));
    }
    Ok(NActionRequest {
        requested_sop_class_uid: g.sop_class_uid,
        requested_sop_instance_uid: g.sop_instance_uid,
        message_id: g.message_id,
        action_type_id: g.type_id,
    })
}
fn to_commitment_response(g: NormalizedResponse) -> CommitmentResponse {
    CommitmentResponse {
        affected_sop_class_uid: g.affected_sop_class_uid,
        affected_sop_instance_uid: g.affected_sop_instance_uid,
        message_id_being_responded_to: g.message_id_being_responded_to,
        status: g.status,
        has_reply: g.command_data_set_type != NO_DATA_SET,
    }
}
pub fn parse_n_action_response(command: &Object) -> Result<NActionResponse> {
    let g = parse_normalized_response(command, N_ACTION_RSP)?;
    check_response(
        "N-ACTION response",
        &g.affected_sop_class_uid,
        &g.affected_sop_instance_uid,
    )?;
    Ok(to_commitment_response(g))
}
pub fn parse_n_event_report_request(command: &Object) -> Result<NEventReportRequest> {
    let g = parse_normalized_request(command, N_EVENT_REPORT_RQ)?;
    check_event_request(&g.sop_class_uid, &g.sop_instance_uid, g.type_id)?;
    if g.command_data_set_type == NO_DATA_SET {
        return Err(Error::new(format!(
            "dicom dimse: N-EVENT-REPORT request dataset type 0x{:04X}, want dataset present",
            g.command_data_set_type
        )));
    }
    Ok(NEventReportRequest {
        affected_sop_class_uid: g.sop_class_uid,
        affected_sop_instance_uid: g.sop_instance_uid,
        message_id: g.message_id,
        event_type_id: g.type_id,
    })
}
pub fn parse_n_event_report_response(command: &Object) -> Result<NEventReportResponse> {
    let g = parse_normalized_response(command, N_EVENT_REPORT_RSP)?;
    check_response(
        "N-EVENT-REPORT response",
        &g.affected_sop_class_uid,
        &g.affected_sop_instance_uid,
    )?;
    Ok(to_commitment_response(g))
}
pub fn send_n_action_request(assoc: &Association, pc_id: u8, req: &NActionRequest) -> Result<()> {
    check_action_request(
        &req.requested_sop_class_uid,
        &req.requested_sop_instance_uid,
        req.action_type_id,
    )?;
    send_command_set(None, assoc, pc_id, &req.command_set())
}
pub fn receive_n_action_request(assoc: &Association, pc_id: u8) -> Result<NActionRequest> {
    parse_n_action_request(&receive_command_set(None, assoc, pc_id)?)
}
pub fn send_n_action_response(assoc: &Association, pc_id: u8, rsp: &NActionResponse) -> Result<()> {
    check_response(
        "N-ACTION response",
        &rsp.affected_sop_class_uid,
        &rsp.affected_sop_instance_uid,
    )?;
    send_command_set(None, assoc, pc_id, &rsp.action_command_set())
}
pub fn receive_n_action_response(assoc: &Association, pc_id: u8) -> Result<NActionResponse> {
    parse_n_action_response(&receive_command_set(None, assoc, pc_id)?)
}
pub fn send_n_event_report_request(
    assoc: &Association,
    pc_id: u8,
    req: &NEventReportRequest,
) -> Result<()> {
    check_event_request(
        &req.affected_sop_class_uid,
        &req.affected_sop_instance_uid,
        req.event_type_id,
    )?;
    send_command_set(None, assoc, pc_id, &req.command_set())
}
pub fn receive_n_event_report_request(
    assoc: &Association,
    pc_id: u8,
) -> Result<NEventReportRequest> {
    parse_n_event_report_request(&receive_command_set(None, assoc, pc_id)?)
}
pub fn send_n_event_report_response(
    assoc: &Association,
    pc_id: u8,
    rsp: &NEventReportResponse,
) -> Result<()> {
    check_response(
        "N-EVENT-REPORT response",
        &rsp.affected_sop_class_uid,
        &rsp.affected_sop_instance_uid,
    )?;
    send_command_set(None, assoc, pc_id, &rsp.event_report_command_set())
}
pub fn receive_n_event_report_response(
    assoc: &Association,
    pc_id: u8,
) -> Result<NEventReportResponse> {
    parse_n_event_report_response(&receive_command_set(None, assoc, pc_id)?)
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StorageCommitmentSopReference {
    pub sop_class_uid: String,
    pub sop_instance_uid: String,
    pub failure_reason: u16,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StorageCommitmentActionInformation {
    pub transaction_uid: String,
    pub referenced_sops: Vec<StorageCommitmentSopReference>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StorageCommitmentEventInformation {
    pub transaction_uid: String,
    pub referenced_sops: Vec<StorageCommitmentSopReference>,
    pub failed_sops: Vec<StorageCommitmentSopReference>,
}
pub(crate) fn valid_uid(v: &str) -> bool {
    object::is_valid_uid(v.as_bytes())
}
pub(crate) fn valid_storage_commitment_failure_reason(reason: u16) -> bool {
    matches!(
        reason,
        STORAGE_COMMITMENT_FAILURE_REASON_PROCESSING_FAILURE
            | STORAGE_COMMITMENT_FAILURE_REASON_NO_SUCH_SOP_INSTANCE
            | STORAGE_COMMITMENT_FAILURE_REASON_RESOURCE_LIMITATION
            | STORAGE_COMMITMENT_FAILURE_REASON_SOP_CLASS_NOT_SUPPORTED
            | STORAGE_COMMITMENT_FAILURE_REASON_CLASS_INSTANCE_CONFLICT
            | STORAGE_COMMITMENT_FAILURE_REASON_DUPLICATE_TRANSACTION_UID
    )
}
pub(crate) fn normalize_storage_commitment_references(
    references: &[StorageCommitmentSopReference],
    failed: bool,
    limit: usize,
) -> Result<Vec<StorageCommitmentSopReference>> {
    if references.is_empty() || references.len() > limit {
        return Err(resource_limit());
    }
    let mut result: Vec<StorageCommitmentSopReference> = references.to_vec();
    let mut seen = HashSet::new();
    for r in &mut result {
        r.sop_class_uid = r.sop_class_uid.trim().to_string();
        r.sop_instance_uid = r.sop_instance_uid.trim().to_string();
        if !valid_uid(&r.sop_class_uid) || !valid_uid(&r.sop_instance_uid) {
            return Err(invalid_request());
        }
        if !seen.insert(r.sop_instance_uid.clone()) {
            return Err(invalid_request());
        }
        if failed {
            if !valid_storage_commitment_failure_reason(r.failure_reason) {
                return Err(invalid_result());
            }
        } else if r.failure_reason != 0 {
            return Err(invalid_result());
        }
    }
    result.sort_by(|a, b| {
        (a.sop_class_uid.as_str(), a.sop_instance_uid.as_str())
            .cmp(&(b.sop_class_uid.as_str(), b.sop_instance_uid.as_str()))
    });
    Ok(result)
}
fn ui(tag: Tag, value: &str) -> Element {
    Element {
        header: ElementHeader {
            tag,
            vr: Vr::UI,
            length: 0,
            length_set: false,
        },
        value: Value::Strings(vec![value.to_string()]),
    }
}
fn sop_sequence_element(
    tag: Tag,
    refs: &[StorageCommitmentSopReference],
    include_failure_reason: bool,
) -> Element {
    let items = refs
        .iter()
        .map(|r| {
            let mut elements = vec![
                ui(STORAGE_COMMITMENT_REFERENCED_SOP_CLASS, &r.sop_class_uid),
                ui(
                    STORAGE_COMMITMENT_REFERENCED_SOP_INSTANCE,
                    &r.sop_instance_uid,
                ),
            ];
            if include_failure_reason {
                let raw = r.failure_reason.to_le_bytes().to_vec();
                elements.push(Element {
                    header: ElementHeader::new(STORAGE_COMMITMENT_FAILURE_REASON, Vr::US, 2),
                    value: Value::Raw(raw),
                });
            }
            DataSet {
                elements,
                item_offset: None,
            }
        })
        .collect();
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
pub fn build_storage_commitment_action_information(
    transaction_uid: &str,
    refs: &[StorageCommitmentSopReference],
) -> Result<Object> {
    let transaction_uid = transaction_uid.trim();
    if !valid_uid(transaction_uid) {
        return Err(Error::new(
            "dicom dimse: Storage Commitment TransactionUID is required",
        ));
    }
    let normalized = normalize_storage_commitment_references(
        refs,
        false,
        DEFAULT_STORAGE_COMMITMENT_MAX_REFERENCES,
    )
    .map_err(|_| {
        Error::new("dicom dimse: Storage Commitment referenced SOP sequence is required")
    })?;
    Ok(Object::from_elements(vec![
        ui(STORAGE_COMMITMENT_TRANSACTION_UID, transaction_uid),
        sop_sequence_element(
            STORAGE_COMMITMENT_REFERENCED_SOP_SEQUENCE,
            &normalized,
            false,
        ),
    ]))
}
pub fn parse_storage_commitment_action_information(
    data_set: Option<&Object>,
) -> Result<StorageCommitmentActionInformation> {
    let Some(ds) = data_set else {
        return Err(Error::new(
            "dicom dimse: missing Storage Commitment action information",
        ));
    };
    let transaction_uid = single_uid(ds, STORAGE_COMMITMENT_TRANSACTION_UID)
        .map_err(|_| Error::new("dicom dimse: missing Storage Commitment TransactionUID"))?;
    let refs = sop_references(ds, STORAGE_COMMITMENT_REFERENCED_SOP_SEQUENCE, false)?;
    if refs.is_empty() {
        return Err(Error::new(
            "dicom dimse: missing Storage Commitment ReferencedSOPSequence",
        ));
    }
    Ok(StorageCommitmentActionInformation {
        transaction_uid,
        referenced_sops: refs,
    })
}
pub fn build_storage_commitment_event_information(
    transaction_uid: &str,
    committed: &[StorageCommitmentSopReference],
    failed: &[StorageCommitmentSopReference],
) -> Result<Object> {
    let transaction_uid = transaction_uid.trim();
    if !valid_uid(transaction_uid) {
        return Err(Error::new(
            "dicom dimse: Storage Commitment TransactionUID is required",
        ));
    }
    if committed.is_empty() && failed.is_empty() {
        return Err(Error::new(
            "dicom dimse: Storage Commitment event information requires at least one referenced or failed SOP",
        ));
    }
    let mut committed = committed.to_vec();
    let mut failed = failed.to_vec();
    if !committed.is_empty() {
        committed = normalize_storage_commitment_references(
            &committed,
            false,
            DEFAULT_STORAGE_COMMITMENT_MAX_REFERENCES,
        )
        .map_err(|_| invalid_result())?;
    }
    if !failed.is_empty() {
        failed = normalize_storage_commitment_references(
            &failed,
            true,
            DEFAULT_STORAGE_COMMITMENT_MAX_REFERENCES,
        )
        .map_err(|_| invalid_result())?;
    }
    let seen: HashSet<&str> = committed
        .iter()
        .map(|r| r.sop_instance_uid.as_str())
        .collect();
    if failed
        .iter()
        .any(|r| seen.contains(r.sop_instance_uid.as_str()))
    {
        return Err(invalid_result());
    }
    let mut elements = vec![ui(STORAGE_COMMITMENT_TRANSACTION_UID, transaction_uid)];
    if !committed.is_empty() {
        elements.push(sop_sequence_element(
            STORAGE_COMMITMENT_REFERENCED_SOP_SEQUENCE,
            &committed,
            false,
        ));
    }
    if !failed.is_empty() {
        elements.push(sop_sequence_element(
            STORAGE_COMMITMENT_FAILED_SOP_SEQUENCE,
            &failed,
            true,
        ));
    }
    Ok(Object::from_elements(elements))
}
pub fn parse_storage_commitment_event_information(
    data_set: Option<&Object>,
) -> Result<StorageCommitmentEventInformation> {
    let Some(ds) = data_set else {
        return Err(Error::new(
            "dicom dimse: missing Storage Commitment event information",
        ));
    };
    let transaction_uid = single_uid(ds, STORAGE_COMMITMENT_TRANSACTION_UID)
        .map_err(|_| Error::new("dicom dimse: missing Storage Commitment TransactionUID"))?;
    let committed = sop_references(ds, STORAGE_COMMITMENT_REFERENCED_SOP_SEQUENCE, false)?;
    let failed = sop_references(ds, STORAGE_COMMITMENT_FAILED_SOP_SEQUENCE, true)?;
    if committed.is_empty() && failed.is_empty() {
        return Err(Error::new(
            "dicom dimse: Storage Commitment event information missing referenced and failed SOP sequences",
        ));
    }
    let mut seen = HashSet::new();
    for r in committed.iter().chain(failed.iter()) {
        if !seen.insert(r.sop_instance_uid.as_str()) {
            return Err(invalid_result());
        }
    }
    Ok(StorageCommitmentEventInformation {
        transaction_uid,
        referenced_sops: committed,
        failed_sops: failed,
    })
}
fn string_values(e: &Element) -> Vec<String> {
    use crate::object::text::{trim_text_value, trim_text_value_bytes};
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
fn single_uid(ds: &Object, tag: Tag) -> Result<String> {
    let Some(element) = ds.get(tag) else {
        return Err(invalid_request());
    };
    if element.vr() != Vr::UI {
        return Err(invalid_request());
    }
    let values = string_values(element);
    if values.len() != 1 {
        return Err(invalid_request());
    }
    let value = values[0].trim().to_string();
    if !valid_uid(&value) {
        return Err(invalid_request());
    }
    Ok(value)
}
fn command_like_uint16(ds: &Object, tag: Tag) -> Result<u16> {
    let missing = || {
        Error::new(format!(
            "dicom dimse: missing Storage Commitment element {tag}"
        ))
    };
    let Some(element) = ds.get(tag) else {
        return Err(missing());
    };
    if element.vr() != Vr::US {
        return Err(missing());
    }
    let Some(raw) = ds.get_raw(tag) else {
        return Err(missing());
    };
    if raw.len() != 2 {
        return Err(Error::new(format!(
            "dicom dimse: Storage Commitment element {tag} has invalid byte value"
        )));
    }
    Ok(u16::from_le_bytes([raw[0], raw[1]]))
}
fn sop_references(
    ds: &Object,
    tag: Tag,
    include_failure_reason: bool,
) -> Result<Vec<StorageCommitmentSopReference>> {
    if let Some(e) = ds.get(tag)
        && e.vr() != Vr::SQ
    {
        return Err(invalid_result());
    }
    let Some(items) = ds.get_sequence(tag) else {
        return Ok(Vec::new());
    };
    if items.len() > DEFAULT_STORAGE_COMMITMENT_MAX_REFERENCES {
        return Err(resource_limit());
    }
    let mut refs = Vec::with_capacity(items.len());
    let mut seen = HashSet::new();
    for (i, item) in items.iter().enumerate() {
        let class = single_uid(item, STORAGE_COMMITMENT_REFERENCED_SOP_CLASS).map_err(|_| {
            Error::new(format!(
                "dicom dimse: Storage Commitment sequence item {i} missing ReferencedSOPClassUID"
            ))
        })?;
        let instance = single_uid(item, STORAGE_COMMITMENT_REFERENCED_SOP_INSTANCE).map_err(|_| {
            Error::new(format!(
                "dicom dimse: Storage Commitment sequence item {i} missing ReferencedSOPInstanceUID"
            ))
        })?;
        if !seen.insert(instance.clone()) {
            return Err(invalid_result());
        }
        let mut r = StorageCommitmentSopReference {
            sop_class_uid: class,
            sop_instance_uid: instance,
            failure_reason: 0,
        };
        if include_failure_reason {
            let reason = command_like_uint16(item, STORAGE_COMMITMENT_FAILURE_REASON)?;
            if !valid_storage_commitment_failure_reason(reason) {
                return Err(invalid_result());
            }
            r.failure_reason = reason;
        }
        refs.push(r);
    }
    Ok(refs)
}
#[derive(Debug, Clone)]
pub struct StorageCommitmentActionContext {
    pub request: NActionRequest,
    pub action_information: Object,
    pub transaction_uid: String,
    pub referenced_sops: Vec<StorageCommitmentSopReference>,
    pub presentation_context_id: u8,
    pub action_syntax: Syntax,
}
pub type StorageCommitmentActionHandler =
    Arc<dyn Fn(&Ctx, &StorageCommitmentActionContext) -> (u16, Option<Error>) + Send + Sync>;
#[derive(Debug, Clone, Copy, Default)]
pub struct StorageCommitmentScpOptions {
    pub max_data_set_bytes: i64,
}
pub(crate) fn storage_commitment_requestor_can_act_as_scu(assoc: &Association) -> bool {
    for role in &assoc.accepted_role_selections {
        if role.sop_class_uid == STORAGE_COMMITMENT_PUSH_MODEL_SOP_CLASS_UID {
            return role.scu_role;
        }
    }
    true
}
pub(crate) fn validate_action_command_status(request: &NormalizedRequest) -> Result<u16> {
    if request.sop_class_uid != STORAGE_COMMITMENT_PUSH_MODEL_SOP_CLASS_UID {
        return fail_with(STATUS_NO_SUCH_SOP_CLASS);
    }
    if request.sop_instance_uid != STORAGE_COMMITMENT_PUSH_MODEL_SOP_INSTANCE_UID {
        return fail_with(STATUS_NO_SUCH_SOP_INSTANCE);
    }
    if request.type_id != STORAGE_COMMITMENT_ACTION_TYPE_ID {
        return fail_with(STATUS_NO_SUCH_ACTION_TYPE);
    }
    if request.command_data_set_type == NO_DATA_SET {
        return fail_with(STATUS_STORAGE_COMMITMENT_PROCESSING_FAILURE);
    }
    Ok(STATUS_SUCCESS)
}
fn fail_with(status: u16) -> Result<u16> {
    Err(invalid_request().with_detail(super::error::Detail::Status(status)))
}
fn send_action_response(
    ctx: &Ctx,
    assoc: &Association,
    pc_id: u8,
    req: &NActionRequest,
    status: u16,
) -> Result<()> {
    let response = CommitmentResponse {
        affected_sop_class_uid: req.requested_sop_class_uid.clone(),
        affected_sop_instance_uid: req.requested_sop_instance_uid.clone(),
        message_id_being_responded_to: req.message_id,
        status,
        has_reply: false,
    };
    send_command_set(Some(ctx), assoc, pc_id, &response.action_command_set())
}
pub fn serve_storage_commitment_scp(
    ctx: Option<&Ctx>,
    assoc: &Association,
    pc_id: u8,
    handler: Option<&StorageCommitmentActionHandler>,
    options: StorageCommitmentScpOptions,
) -> Result<()> {
    let ctx = ctx.cloned().unwrap_or_default();
    let Some(handler) = handler else {
        return Err(Error::new(
            "dicom dimse: nil Storage Commitment action handler",
        ));
    };
    if !storage_commitment_requestor_can_act_as_scu(assoc) {
        return Err(invalid_request());
    }
    let pc = accepted_context_by_id(assoc, pc_id)?;
    if pc.abstract_syntax_uid != STORAGE_COMMITMENT_PUSH_MODEL_SOP_CLASS_UID {
        return Err(Error::sentinel(Kind::PresentationContextMismatch));
    }
    let syntax = transfer_syntax_for_accepted_context(&pc)?;
    let mut max = options.max_data_set_bytes;
    if max < 0 {
        return Err(resource_limit());
    }
    if max == 0 {
        max = MAX_NORMALIZED_DATA_SET_BYTES;
    }
    let command = receive_command_set(Some(&ctx), assoc, pc_id)?;
    let normalized = parse_normalized_request(&command, N_ACTION_RQ)?;
    let req = NActionRequest {
        requested_sop_class_uid: normalized.sop_class_uid.clone(),
        requested_sop_instance_uid: normalized.sop_instance_uid.clone(),
        message_id: normalized.message_id,
        action_type_id: normalized.type_id,
    };
    if let Err(e) = validate_action_command_status(&normalized) {
        let status = match e.detail() {
            super::error::Detail::Status(s) => *s,
            _ => STATUS_STORAGE_COMMITMENT_PROCESSING_FAILURE,
        };
        let _ = send_action_response(&ctx, assoc, pc_id, &req, status);
        return Err(e.with_detail(super::error::Detail::None));
    }
    let action_info = match receive_data_set_limits(
        Some(&ctx),
        assoc,
        pc_id,
        syntax,
        max,
        MAX_NORMALIZED_DATA_SET_ELEMENTS,
        MAX_NORMALIZED_DATA_SET_DEPTH,
    ) {
        Ok((o, _)) => o,
        Err(e) => {
            let _ = send_action_response(
                &ctx,
                assoc,
                pc_id,
                &req,
                STATUS_STORAGE_COMMITMENT_PROCESSING_FAILURE,
            );
            return Err(e);
        }
    };
    let parsed = match parse_storage_commitment_action_information(Some(&action_info)) {
        Ok(p) => p,
        Err(e) => {
            let _ = send_action_response(
                &ctx,
                assoc,
                pc_id,
                &req,
                STATUS_STORAGE_COMMITMENT_PROCESSING_FAILURE,
            );
            return Err(e);
        }
    };
    let action = StorageCommitmentActionContext {
        request: req.clone(),
        action_information: action_info,
        transaction_uid: parsed.transaction_uid,
        referenced_sops: parsed.referenced_sops,
        presentation_context_id: pc_id,
        action_syntax: syntax,
    };
    let (mut status, handler_err) = handler(&ctx, &action);
    if handler_err.is_some() && status == STATUS_SUCCESS {
        status = STATUS_STORAGE_COMMITMENT_PROCESSING_FAILURE;
    }
    send_action_response(&ctx, assoc, pc_id, &req, status)?;
    match handler_err {
        Some(e) => Err(e),
        None => Ok(()),
    }
}
#[derive(Debug, Clone)]
pub struct StorageCommitmentEvent {
    pub transaction_uid: String,
    pub received_at: SystemTime,
    pub request_message_id: u16,
}
struct Tracked {
    request_message_id: u16,
    event: Option<StorageCommitmentEvent>,
    closed: bool,
}
#[derive(Default)]
pub struct StorageCommitmentTransactionTracker {
    m: Mutex<HashMap<String, Tracked>>,
    cond: Condvar,
}
impl StorageCommitmentTransactionTracker {
    pub fn new() -> StorageCommitmentTransactionTracker {
        StorageCommitmentTransactionTracker::default()
    }
    pub fn track(&self, transaction_uid: &str, request_message_id: u16) -> Result<()> {
        if transaction_uid.is_empty() {
            return Err(Error::new(
                "dicom dimse: storage commitment Track requires non-empty transaction UID",
            ));
        }
        let mut m = self.m.lock().unwrap();
        if m.contains_key(transaction_uid) {
            return Err(Error::new(format!(
                "dicom dimse: storage commitment transaction UID already tracked: {transaction_uid}"
            )));
        }
        m.insert(
            transaction_uid.to_string(),
            Tracked {
                request_message_id,
                event: None,
                closed: false,
            },
        );
        Ok(())
    }
    pub fn wait(
        &self,
        ctx: Option<&Ctx>,
        transaction_uid: &str,
        timeout: Duration,
    ) -> Result<StorageCommitmentEvent> {
        if transaction_uid.is_empty() {
            return Err(Error::new(
                "dicom dimse: storage commitment Wait requires non-empty transaction UID",
            ));
        }
        let ctx = ctx.cloned().unwrap_or_default();
        let deadline = (!timeout.is_zero()).then(|| Instant::now() + timeout);
        let mut m = self.m.lock().unwrap();
        if !m.contains_key(transaction_uid) {
            return Err(Error::new(ERR_TRANSACTION_UNKNOWN));
        }
        loop {
            if let Some(ev) = m.get_mut(transaction_uid).and_then(|t| t.event.take()) {
                m.remove(transaction_uid);
                return Ok(ev);
            }
            if deadline.is_some_and(|d| Instant::now() >= d) {
                m.remove(transaction_uid);
                return Err(Error::new(ERR_TRANSACTION_TIMEOUT));
            }
            if let Some(e) = context_err(&ctx) {
                m.remove(transaction_uid);
                return Err(e);
            }
            m = self
                .cond
                .wait_timeout(m, Duration::from_millis(2))
                .unwrap()
                .0;
        }
    }
    pub fn deliver(&self, transaction_uid: &str) -> Result<bool> {
        if transaction_uid.is_empty() {
            return Err(Error::new(
                "dicom dimse: storage commitment Deliver requires non-empty transaction UID",
            ));
        }
        let mut m = self.m.lock().unwrap();
        let Some(t) = m.get_mut(transaction_uid) else {
            return Err(Error::new(ERR_TRANSACTION_UNKNOWN));
        };
        if t.closed {
            return Err(Error::new(format!(
                "dicom dimse: storage commitment transaction already delivered: {transaction_uid}"
            )));
        }
        t.closed = true;
        t.event = Some(StorageCommitmentEvent {
            transaction_uid: transaction_uid.to_string(),
            received_at: SystemTime::now(),
            request_message_id: t.request_message_id,
        });
        self.cond.notify_all();
        Ok(true)
    }
}
