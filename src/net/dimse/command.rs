#![forbid(unsafe_code)]
#![cfg_attr(
    not(test),
    warn(clippy::indexing_slicing, clippy::arithmetic_side_effects)
)]
use super::error::{Error, Kind, Result};
use crate::net::ul;
use crate::object::{self, Element, ElementHeader, Object, Tag, Value, Vr};
pub const COMMAND_GROUP_LENGTH: Tag = Tag::new(0x0000, 0x0000);
pub const AFFECTED_SOP_CLASS_UID: Tag = Tag::new(0x0000, 0x0002);
pub const AFFECTED_SOP_INSTANCE_UID: Tag = Tag::new(0x0000, 0x1000);
pub const COMMAND_FIELD: Tag = Tag::new(0x0000, 0x0100);
pub const MESSAGE_ID: Tag = Tag::new(0x0000, 0x0110);
pub const MESSAGE_ID_BEING_RESPONDED_TO: Tag = Tag::new(0x0000, 0x0120);
pub const PRIORITY: Tag = Tag::new(0x0000, 0x0700);
pub const COMMAND_DATA_SET_TYPE: Tag = Tag::new(0x0000, 0x0800);
pub const STATUS: Tag = Tag::new(0x0000, 0x0900);
pub const OFFENDING_ELEMENT: Tag = Tag::new(0x0000, 0x0901);
pub const ERROR_COMMENT: Tag = Tag::new(0x0000, 0x0902);
pub const ERROR_ID: Tag = Tag::new(0x0000, 0x0903);
pub const FAILED_SOP_INSTANCE_UID_LIST: Tag = Tag::new(0x0008, 0x0058);
pub const MOVE_DESTINATION: Tag = Tag::new(0x0000, 0x0600);
pub const NUMBER_OF_REMAINING_SUBOPERATIONS: Tag = Tag::new(0x0000, 0x1020);
pub const NUMBER_OF_COMPLETED_SUBOPERATIONS: Tag = Tag::new(0x0000, 0x1021);
pub const NUMBER_OF_FAILED_SUBOPERATIONS: Tag = Tag::new(0x0000, 0x1022);
pub const NUMBER_OF_WARNING_SUBOPERATIONS: Tag = Tag::new(0x0000, 0x1023);
pub const MOVE_ORIGINATOR_APPLICATION_ENTITY_TITLE: Tag = Tag::new(0x0000, 0x1030);
pub const MOVE_ORIGINATOR_MESSAGE_ID: Tag = Tag::new(0x0000, 0x1031);
pub const REQUESTED_SOP_CLASS_UID: Tag = Tag::new(0x0000, 0x0003);
pub const REQUESTED_SOP_INSTANCE_UID: Tag = Tag::new(0x0000, 0x1001);
pub const ATTRIBUTE_IDENTIFIER_LIST: Tag = Tag::new(0x0000, 0x1005);
pub const ACTION_TYPE_ID: Tag = Tag::new(0x0000, 0x1008);
pub const EVENT_TYPE_ID: Tag = Tag::new(0x0000, 0x1002);
pub const C_STORE_RQ: u16 = 0x0001;
pub const C_STORE_RSP: u16 = 0x8001;
pub const C_CANCEL_RQ: u16 = 0x0FFF;
pub const C_ECHO_RQ: u16 = 0x0030;
pub const C_ECHO_RSP: u16 = 0x8030;
pub const C_FIND_RQ: u16 = 0x0020;
pub const C_FIND_RSP: u16 = 0x8020;
pub const C_GET_RQ: u16 = 0x0010;
pub const C_GET_RSP: u16 = 0x8010;
pub const C_MOVE_RQ: u16 = 0x0021;
pub const C_MOVE_RSP: u16 = 0x8021;
pub const N_ACTION_RQ: u16 = 0x0130;
pub const N_ACTION_RSP: u16 = 0x8130;
pub const N_EVENT_REPORT_RQ: u16 = 0x0100;
pub const N_EVENT_REPORT_RSP: u16 = 0x8100;
pub const N_GET_RQ: u16 = 0x0110;
pub const N_GET_RSP: u16 = 0x8110;
pub const N_SET_RQ: u16 = 0x0120;
pub const N_SET_RSP: u16 = 0x8120;
pub const N_CREATE_RQ: u16 = 0x0140;
pub const N_CREATE_RSP: u16 = 0x8140;
pub const N_DELETE_RQ: u16 = 0x0150;
pub const N_DELETE_RSP: u16 = 0x8150;
pub const DATA_SET_PRESENT: u16 = 0x0000;
pub const NO_DATA_SET: u16 = 0x0101;
pub const DATA_SET_ABSENT: u16 = NO_DATA_SET;
pub const STATUS_SUCCESS: u16 = 0x0000;
pub const STATUS_PENDING: u16 = 0xFF00;
pub const STATUS_PENDING_WARNING: u16 = 0xFF01;
pub const C_FIND_STATUS_CANCEL: u16 = 0xFE00;
pub const C_FIND_STATUS_OUT_OF_RESOURCES: u16 = 0xA700;
pub const C_FIND_STATUS_UNABLE_TO_PROCESS: u16 = 0xC000;
pub const STATUS_PROCESSING_FAILURE: u16 = 0x0110;
pub const STATUS_C_MOVE_CANCEL: u16 = 0xFE00;
pub const STATUS_C_MOVE_SUB_OPERATIONS_COMPLETE_ONE_OR_MORE_FAILURES: u16 = 0xB000;
pub const STATUS_C_MOVE_MOVE_DESTINATION_UNKNOWN: u16 = 0xA801;
pub const STATUS_C_MOVE_IDENTIFIER_DOES_NOT_MATCH_SOP_CLASS: u16 = 0xA900;
pub const STATUS_C_MOVE_UNABLE_TO_PROCESS: u16 = 0xC000;
pub const STATUS_C_GET_CANCEL: u16 = 0xFE00;
pub const STATUS_C_GET_SUB_OPERATIONS_COMPLETE_ONE_OR_MORE_FAILURES: u16 = 0xB000;
pub const STATUS_C_GET_IDENTIFIER_DOES_NOT_MATCH_SOP_CLASS: u16 = 0xA900;
pub const STATUS_C_GET_UNABLE_TO_PROCESS: u16 = 0xC000;
pub const PRIORITY_LOW: u16 = 0x0002;
pub const PRIORITY_MEDIUM: u16 = 0x0000;
pub const PRIORITY_HIGH: u16 = 0x0001;
pub const VERIFICATION_SOP_CLASS_UID: &str = "1.2.840.10008.1.1";
pub const MAX_IDENTIFIER_BYTES: i64 = 4 << 20;
pub fn command_uint16(command: Option<&Object>, tag: Tag) -> Result<u16> {
    let Some(command) = command else {
        return Err(Error::new("dicom dimse: nil command set"));
    };
    let Some(raw) = command.get(tag).and_then(|e| e.raw()) else {
        return Err(Error::new(format!(
            "dicom dimse: missing command element {tag}"
        )));
    };
    let &[a, b] = raw else {
        return Err(Error::new(format!(
            "dicom dimse: command element {tag} has {} byte value, want exactly 2",
            raw.len()
        )));
    };
    Ok(u16::from_le_bytes([a, b]))
}
fn cu16(command: &Object, tag: Tag) -> Result<u16> {
    command_uint16(Some(command), tag)
}
pub(crate) fn optional_command_uint16(command: &Object, tag: Tag) -> Result<Option<u16>> {
    if command.get(tag).is_none() {
        return Ok(None);
    }
    cu16(command, tag).map(Some)
}
#[doc(hidden)]
pub fn command_uid(command: &Object, tag: Tag) -> Result<String> {
    match command.get_uids(tag) {
        Some(uids) if uids.len() == 1 => Ok(uids.into_iter().next().unwrap()),
        Some(uids) if !uids.is_empty() => Err(Error::new(format!(
            "dicom dimse: command UID element {tag} requires one value"
        ))),
        _ => Err(Error::new(format!(
            "dicom dimse: missing command UID element {tag}"
        ))),
    }
}
pub(crate) fn optional_command_uid(command: &Object, tag: Tag) -> Result<(String, bool)> {
    if command.get(tag).is_none() {
        return Ok((String::new(), false));
    }
    match command.get_uid(tag) {
        Some(uid) if !uid.is_empty() => Ok((uid, true)),
        _ => Err(Error::new(format!(
            "dicom dimse: invalid command UID element {tag}"
        ))),
    }
}
pub(crate) fn optional_command_tags(command: &Object, tag: Tag) -> Result<Option<Vec<Tag>>> {
    let Some(elem) = command.get(tag) else {
        return Ok(None);
    };
    if elem.vr() != Vr::AT {
        return Err(Error::new(format!(
            "dicom dimse: command element {tag} has VR {}, want AT",
            elem.vr()
        )));
    }
    let Some(raw) = elem.raw() else {
        return Err(Error::new(format!(
            "dicom dimse: command element {tag} is not raw AT data"
        )));
    };
    if raw.len() % 4 != 0 {
        return Err(Error::new(format!(
            "dicom dimse: command element {tag} has {} byte value, want a multiple of 4",
            raw.len()
        )));
    }
    Ok(Some(
        raw.as_chunks::<4>()
            .0
            .iter()
            .map(|&[g0, g1, e0, e1]| {
                Tag::new(u16::from_le_bytes([g0, g1]), u16::from_le_bytes([e0, e1]))
            })
            .collect(),
    ))
}
pub(crate) fn optional_command_uids(command: &Object, tag: Tag) -> Result<Option<Vec<String>>> {
    let Some(elem) = command.get(tag) else {
        return Ok(None);
    };
    if elem.vr() != Vr::UI {
        return Err(Error::new(format!(
            "dicom dimse: command element {tag} has VR {}, want UI",
            elem.vr()
        )));
    }
    match command.get_uids(tag) {
        Some(v) => Ok(Some(v)),
        None => Err(Error::new(format!(
            "dicom dimse: command element {tag} is not a valid UI list"
        ))),
    }
}
fn raw_element(tag: Tag, vr: Vr, raw: Vec<u8>) -> Element {
    Element {
        header: ElementHeader::new(tag, vr, raw.len() as u32),
        value: Value::Raw(raw),
    }
}
fn string_element(tag: Tag, vr: Vr, values: Vec<String>) -> Element {
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
pub fn us_element(tag: Tag, value: u16) -> Element {
    raw_element(tag, Vr::US, value.to_le_bytes().to_vec())
}
#[doc(hidden)]
pub fn ul_element(tag: Tag, value: u32) -> Element {
    raw_element(tag, Vr::UL, value.to_le_bytes().to_vec())
}
pub(crate) fn ui_element(tag: Tag, value: &str) -> Element {
    string_element(tag, Vr::UI, vec![value.to_string()])
}
pub(crate) fn ui_list_element(tag: Tag, values: &[String]) -> Element {
    string_element(tag, Vr::UI, values.to_vec())
}
pub fn text_element(tag: Tag, vr: Vr, value: &str) -> Element {
    string_element(tag, vr, vec![value.to_string()])
}
#[doc(hidden)]
pub fn at_element(tag: Tag, values: &[Tag]) -> Element {
    let mut raw = Vec::with_capacity(values.len().saturating_mul(4));
    for v in values {
        raw.extend_from_slice(&v.group.to_le_bytes());
        raw.extend_from_slice(&v.element.to_le_bytes());
    }
    raw_element(tag, Vr::AT, raw)
}
fn command_uid_element(elem: Element) -> Element {
    match (&elem.value, elem.header.vr) {
        (Value::Strings(values), Vr::UI) => {
            let mut raw = values.join("\\").into_bytes();
            if raw.len() % 2 != 0 {
                raw.push(0);
            }
            raw_element(elem.tag(), Vr::UI, raw)
        }
        _ => elem,
    }
}
pub fn encode_command_set(elements: &[Element]) -> Result<Vec<u8>> {
    let without: Vec<Element> = elements
        .iter()
        .filter(|e| e.tag() != COMMAND_GROUP_LENGTH)
        .cloned()
        .map(command_uid_element)
        .collect();
    let obj = Object::from_elements(without.clone());
    let command = object::writer::write_data_set(&obj, ul::IMPLICIT_VR_LITTLE_ENDIAN)
        .map_err(|e| Error::from(e).prefixed("dicom dimse: encode command set"))?;
    if command.len() as u64 > u32::MAX as u64 {
        return Err(Error::new(format!(
            "dicom dimse: command group length {} exceeds uint32",
            command.len()
        )));
    }
    let mut with = vec![ul_element(COMMAND_GROUP_LENGTH, command.len() as u32)];
    with.extend(without);
    let obj = Object::from_elements(with);
    object::writer::write_data_set(&obj, ul::IMPLICIT_VR_LITTLE_ENDIAN)
        .map_err(|e| Error::from(e).prefixed("dicom dimse: encode command set with group length"))
}
pub fn decode_command_set(data: &[u8]) -> Result<Object> {
    object::read_data_set_bytes(data, object::transfer::IMPLICIT_VR_LITTLE_ENDIAN)
        .map_err(|e| Error::from(e).prefixed("dicom dimse: decode command set"))
}
fn protocol_violation(text: String) -> Error {
    Error::classified(Kind::ProtocolViolation, text)
}
fn want_field(command: &Object, want: u16, name: &str, violation: bool) -> Result<()> {
    let field = cu16(command, COMMAND_FIELD)?;
    if field != want {
        let text = format!("dicom dimse: command field 0x{field:04X}, want {name} 0x{want:04X}");
        return Err(if violation {
            protocol_violation(text)
        } else {
            Error::new(text)
        });
    }
    Ok(())
}
fn get_string(command: &Object, tag: Tag) -> Option<String> {
    command.get_string(tag)
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CEchoRequest {
    pub message_id: u16,
}
impl CEchoRequest {
    pub fn command_set(&self) -> Vec<Element> {
        vec![
            ui_element(AFFECTED_SOP_CLASS_UID, VERIFICATION_SOP_CLASS_UID),
            us_element(COMMAND_FIELD, C_ECHO_RQ),
            us_element(MESSAGE_ID, self.message_id),
            us_element(COMMAND_DATA_SET_TYPE, NO_DATA_SET),
        ]
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CEchoResponse {
    pub message_id_being_responded_to: u16,
    pub status: u16,
}
impl CEchoResponse {
    pub fn command_set(&self) -> Vec<Element> {
        vec![
            ui_element(AFFECTED_SOP_CLASS_UID, VERIFICATION_SOP_CLASS_UID),
            us_element(COMMAND_FIELD, C_ECHO_RSP),
            us_element(
                MESSAGE_ID_BEING_RESPONDED_TO,
                self.message_id_being_responded_to,
            ),
            us_element(COMMAND_DATA_SET_TYPE, NO_DATA_SET),
            us_element(STATUS, self.status),
        ]
    }
}
pub fn parse_c_echo_response(command: &Object) -> Result<CEchoResponse> {
    want_field(command, C_ECHO_RSP, "C-ECHO-RSP", true)?;
    let message_id = cu16(command, MESSAGE_ID_BEING_RESPONDED_TO)?;
    let dst = cu16(command, COMMAND_DATA_SET_TYPE)?;
    if dst != NO_DATA_SET {
        return Err(protocol_violation(format!(
            "dicom dimse: C-ECHO response dataset type 0x{dst:04X}, want no dataset 0x{NO_DATA_SET:04X}"
        )));
    }
    let status = cu16(command, STATUS)?;
    Ok(CEchoResponse {
        message_id_being_responded_to: message_id,
        status,
    })
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CCancelRequest {
    pub message_id_being_responded_to: u16,
}
impl CCancelRequest {
    pub fn command_set(&self) -> Vec<Element> {
        vec![
            us_element(COMMAND_FIELD, C_CANCEL_RQ),
            us_element(
                MESSAGE_ID_BEING_RESPONDED_TO,
                self.message_id_being_responded_to,
            ),
            us_element(COMMAND_DATA_SET_TYPE, NO_DATA_SET),
        ]
    }
}
pub fn parse_c_cancel_request(command: &Object) -> Result<CCancelRequest> {
    want_field(command, C_CANCEL_RQ, "C-CANCEL-RQ", false)?;
    let msg_id = cu16(command, MESSAGE_ID_BEING_RESPONDED_TO)?;
    let dst = cu16(command, COMMAND_DATA_SET_TYPE)?;
    if dst != NO_DATA_SET {
        return Err(Error::new(format!(
            "dicom dimse: C-CANCEL request dataset type 0x{dst:04X}, want no dataset 0x{NO_DATA_SET:04X}"
        )));
    }
    Ok(CCancelRequest {
        message_id_being_responded_to: msg_id,
    })
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CStoreRequest {
    pub affected_sop_class_uid: String,
    pub message_id: u16,
    pub priority: u16,
    pub affected_sop_instance_uid: String,
    pub move_originator_ae_title: String,
    pub move_originator_message_id: Option<u16>,
}
impl CStoreRequest {
    pub fn command_set(&self) -> Vec<Element> {
        let mut e = vec![
            ui_element(AFFECTED_SOP_CLASS_UID, &self.affected_sop_class_uid),
            us_element(COMMAND_FIELD, C_STORE_RQ),
            us_element(MESSAGE_ID, self.message_id),
            us_element(PRIORITY, self.priority),
            us_element(COMMAND_DATA_SET_TYPE, DATA_SET_PRESENT),
            ui_element(AFFECTED_SOP_INSTANCE_UID, &self.affected_sop_instance_uid),
        ];
        if !self.move_originator_ae_title.is_empty() {
            e.push(text_element(
                MOVE_ORIGINATOR_APPLICATION_ENTITY_TITLE,
                Vr::AE,
                &self.move_originator_ae_title,
            ));
        }
        if let Some(id) = self.move_originator_message_id {
            e.push(us_element(MOVE_ORIGINATOR_MESSAGE_ID, id));
        }
        e
    }
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CStoreResponse {
    pub affected_sop_class_uid: String,
    pub message_id_being_responded_to: u16,
    pub affected_sop_instance_uid: String,
    pub status: u16,
    pub error_comment: String,
}
impl CStoreResponse {
    pub fn command_set(&self) -> Vec<Element> {
        let mut e = vec![
            ui_element(AFFECTED_SOP_CLASS_UID, &self.affected_sop_class_uid),
            us_element(COMMAND_FIELD, C_STORE_RSP),
            us_element(
                MESSAGE_ID_BEING_RESPONDED_TO,
                self.message_id_being_responded_to,
            ),
            us_element(COMMAND_DATA_SET_TYPE, NO_DATA_SET),
            us_element(STATUS, self.status),
            ui_element(AFFECTED_SOP_INSTANCE_UID, &self.affected_sop_instance_uid),
        ];
        if !self.error_comment.is_empty() {
            e.push(text_element(ERROR_COMMENT, Vr::LO, &self.error_comment));
        }
        e
    }
}
pub fn parse_c_store_request(command: &Object) -> Result<CStoreRequest> {
    want_field(command, C_STORE_RQ, "C-STORE-RQ", false)?;
    let sop_class_uid = command_uid(command, AFFECTED_SOP_CLASS_UID)?;
    let message_id = cu16(command, MESSAGE_ID)?;
    let priority = cu16(command, PRIORITY)?;
    let dst = cu16(command, COMMAND_DATA_SET_TYPE)?;
    if dst == NO_DATA_SET {
        return Err(Error::new(format!(
            "dicom dimse: C-STORE request dataset type 0x{dst:04X}, want dataset present"
        )));
    }
    let sop_instance_uid = command_uid(command, AFFECTED_SOP_INSTANCE_UID)?;
    let mut req = CStoreRequest {
        affected_sop_class_uid: sop_class_uid,
        message_id,
        priority,
        affected_sop_instance_uid: sop_instance_uid,
        ..Default::default()
    };
    if let Some(o) = get_string(command, MOVE_ORIGINATOR_APPLICATION_ENTITY_TITLE) {
        req.move_originator_ae_title = ul::normalize_ae_title(&o).to_string();
    }
    req.move_originator_message_id = optional_command_uint16(command, MOVE_ORIGINATOR_MESSAGE_ID)?;
    Ok(req)
}
pub fn parse_c_store_response(command: &Object) -> Result<CStoreResponse> {
    want_field(command, C_STORE_RSP, "C-STORE-RSP", true)?;
    let sop_class_uid = command_uid(command, AFFECTED_SOP_CLASS_UID)?;
    let message_id = cu16(command, MESSAGE_ID_BEING_RESPONDED_TO)?;
    let dst = cu16(command, COMMAND_DATA_SET_TYPE)?;
    if dst != NO_DATA_SET {
        return Err(Error::new(format!(
            "dicom dimse: C-STORE response dataset type 0x{dst:04X}, want no dataset 0x{NO_DATA_SET:04X}"
        )));
    }
    let status = cu16(command, STATUS)?;
    let sop_instance_uid = command_uid(command, AFFECTED_SOP_INSTANCE_UID)?;
    Ok(CStoreResponse {
        affected_sop_class_uid: sop_class_uid,
        message_id_being_responded_to: message_id,
        affected_sop_instance_uid: sop_instance_uid,
        status,
        error_comment: get_string(command, ERROR_COMMENT).unwrap_or_default(),
    })
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CFindRequest {
    pub affected_sop_class_uid: String,
    pub message_id: u16,
    pub priority: u16,
}
impl CFindRequest {
    pub fn command_set(&self) -> Vec<Element> {
        vec![
            ui_element(AFFECTED_SOP_CLASS_UID, &self.affected_sop_class_uid),
            us_element(COMMAND_FIELD, C_FIND_RQ),
            us_element(MESSAGE_ID, self.message_id),
            us_element(PRIORITY, self.priority),
            us_element(COMMAND_DATA_SET_TYPE, DATA_SET_PRESENT),
        ]
    }
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CFindResponse {
    pub affected_sop_class_uid: String,
    pub message_id_being_responded_to: u16,
    pub status: u16,
    pub command_data_set_type: u16,
    pub error_comment: String,
    pub offending_element: Option<Tag>,
    pub failed_sop_instance_uid_list: Option<Vec<String>>,
}
impl CFindResponse {
    pub fn command_set(&self) -> Vec<Element> {
        let mut e = vec![
            ui_element(AFFECTED_SOP_CLASS_UID, &self.affected_sop_class_uid),
            us_element(COMMAND_FIELD, C_FIND_RSP),
            us_element(
                MESSAGE_ID_BEING_RESPONDED_TO,
                self.message_id_being_responded_to,
            ),
            us_element(COMMAND_DATA_SET_TYPE, self.command_data_set_type),
            us_element(STATUS, self.status),
        ];
        if !self.error_comment.is_empty() {
            e.push(text_element(ERROR_COMMENT, Vr::LO, &self.error_comment));
        }
        if let Some(t) = self.offending_element {
            e.push(at_element(OFFENDING_ELEMENT, &[t]));
        }
        if let Some(list) = &self.failed_sop_instance_uid_list
            && !list.is_empty()
        {
            e.push(ui_list_element(FAILED_SOP_INSTANCE_UID_LIST, list));
        }
        e
    }
}
pub fn parse_c_find_request(command: &Object) -> Result<CFindRequest> {
    want_field(command, C_FIND_RQ, "C-FIND-RQ", false)?;
    let message_id = cu16(command, MESSAGE_ID)?;
    let priority = cu16(command, PRIORITY)?;
    let dst = cu16(command, COMMAND_DATA_SET_TYPE)?;
    if dst == NO_DATA_SET {
        return Err(Error::new(
            "dicom dimse: C-FIND request does not contain an Identifier dataset",
        ));
    }
    if priority > PRIORITY_LOW {
        return Err(Error::new(format!(
            "dicom dimse: C-FIND request priority 0x{priority:04X} is invalid"
        )));
    }
    let Some(uid) = get_string(command, AFFECTED_SOP_CLASS_UID) else {
        return Err(Error::new(format!(
            "dicom dimse: missing or invalid command element {AFFECTED_SOP_CLASS_UID}"
        )));
    };
    Ok(CFindRequest {
        affected_sop_class_uid: uid,
        message_id,
        priority,
    })
}
pub fn parse_c_find_response(command: &Object) -> Result<CFindResponse> {
    want_field(command, C_FIND_RSP, "C-FIND-RSP", true)?;
    let message_id = cu16(command, MESSAGE_ID_BEING_RESPONDED_TO)?;
    let dst = cu16(command, COMMAND_DATA_SET_TYPE)?;
    let status = cu16(command, STATUS)?;
    let mut affected = String::new();
    if command.get(AFFECTED_SOP_CLASS_UID).is_some() {
        match get_string(command, AFFECTED_SOP_CLASS_UID) {
            Some(s) => affected = s,
            None => {
                return Err(Error::new(format!(
                    "dicom dimse: invalid command element {AFFECTED_SOP_CLASS_UID}"
                )));
            }
        }
    }
    let mut resp = CFindResponse {
        affected_sop_class_uid: affected,
        message_id_being_responded_to: message_id,
        status,
        command_data_set_type: dst,
        error_comment: get_string(command, ERROR_COMMENT).unwrap_or_default(),
        ..Default::default()
    };
    if let Some(tags) = optional_command_tags(command, OFFENDING_ELEMENT)? {
        resp.offending_element = tags.first().copied();
    }
    resp.failed_sop_instance_uid_list =
        optional_command_uids(command, FAILED_SOP_INSTANCE_UID_LIST)?;
    Ok(resp)
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CFindStatusCategory {
    Invalid,
    Pending,
    Success,
    Failure,
}
impl CFindStatusCategory {
    pub fn as_str(self) -> &'static str {
        match self {
            CFindStatusCategory::Pending => "pending",
            CFindStatusCategory::Success => "success",
            CFindStatusCategory::Failure => "failure",
            CFindStatusCategory::Invalid => "invalid",
        }
    }
}
pub fn categorize_c_find_status(status: u16) -> CFindStatusCategory {
    match status {
        STATUS_PENDING | STATUS_PENDING_WARNING => return CFindStatusCategory::Pending,
        STATUS_SUCCESS => return CFindStatusCategory::Success,
        C_FIND_STATUS_CANCEL | 0x0001 | 0x0122 | 0x0124 | 0x0210 | 0x0211 | 0x0212 => {
            return CFindStatusCategory::Failure;
        }
        _ => {}
    }
    if (0xA000..=0xCFFF).contains(&status) {
        return CFindStatusCategory::Failure;
    }
    CFindStatusCategory::Invalid
}
pub fn c_find_status_error(resp: &CFindResponse) -> Option<Error> {
    match categorize_c_find_status(resp.status) {
        CFindStatusCategory::Pending | CFindStatusCategory::Success => None,
        CFindStatusCategory::Invalid => Some(Error::classified(
            Kind::CFindInvalidStatus,
            format!("{} 0x{:04X}", Kind::CFindInvalidStatus.text(), resp.status),
        )),
        CFindStatusCategory::Failure => Some(Error::new(if resp.error_comment.is_empty() {
            format!(
                "dicom dimse: C-FIND failed with status 0x{:04X}",
                resp.status
            )
        } else {
            format!(
                "dicom dimse: C-FIND failed with status 0x{:04X}: {}",
                resp.status, resp.error_comment
            )
        })),
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CMoveStatus {
    Pending,
    Success,
    Cancel,
    Warning,
    Failure,
}
impl CMoveStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            CMoveStatus::Pending => "pending",
            CMoveStatus::Success => "success",
            CMoveStatus::Cancel => "cancel",
            CMoveStatus::Warning => "warning",
            CMoveStatus::Failure => "failure",
        }
    }
}
pub fn classify_c_move_status(status: u16) -> CMoveStatus {
    match status {
        0xFF00 | 0xFF01 => CMoveStatus::Pending,
        0x0000 => CMoveStatus::Success,
        0xFE00 => CMoveStatus::Cancel,
        s if s & 0xF000 == 0xB000 => CMoveStatus::Warning,
        _ => CMoveStatus::Failure,
    }
}
pub(crate) fn retrieve_status_allows_identifier(status: u16) -> bool {
    matches!(
        classify_c_move_status(status),
        CMoveStatus::Cancel | CMoveStatus::Warning | CMoveStatus::Failure
    )
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SubOperationCounts {
    pub remaining: Option<u16>,
    pub completed: Option<u16>,
    pub failed: Option<u16>,
    pub warning: Option<u16>,
}
impl SubOperationCounts {
    fn push(&self, e: &mut Vec<Element>) {
        if let Some(v) = self.remaining {
            e.push(us_element(NUMBER_OF_REMAINING_SUBOPERATIONS, v));
        }
        if let Some(v) = self.completed {
            e.push(us_element(NUMBER_OF_COMPLETED_SUBOPERATIONS, v));
        }
        if let Some(v) = self.failed {
            e.push(us_element(NUMBER_OF_FAILED_SUBOPERATIONS, v));
        }
        if let Some(v) = self.warning {
            e.push(us_element(NUMBER_OF_WARNING_SUBOPERATIONS, v));
        }
    }
    fn parse(command: &Object) -> Result<SubOperationCounts> {
        Ok(SubOperationCounts {
            remaining: optional_command_uint16(command, NUMBER_OF_REMAINING_SUBOPERATIONS)?,
            completed: optional_command_uint16(command, NUMBER_OF_COMPLETED_SUBOPERATIONS)?,
            failed: optional_command_uint16(command, NUMBER_OF_FAILED_SUBOPERATIONS)?,
            warning: optional_command_uint16(command, NUMBER_OF_WARNING_SUBOPERATIONS)?,
        })
    }
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CMoveRequest {
    pub affected_sop_class_uid: String,
    pub message_id: u16,
    pub priority: u16,
    pub move_destination: String,
    pub move_originator_ae_title: String,
    pub move_originator_message_id: Option<u16>,
}
impl CMoveRequest {
    pub fn command_set(&self) -> Vec<Element> {
        let mut e = vec![
            ui_element(AFFECTED_SOP_CLASS_UID, &self.affected_sop_class_uid),
            us_element(COMMAND_FIELD, C_MOVE_RQ),
            us_element(MESSAGE_ID, self.message_id),
            text_element(MOVE_DESTINATION, Vr::AE, &self.move_destination),
            us_element(PRIORITY, self.priority),
            us_element(COMMAND_DATA_SET_TYPE, DATA_SET_PRESENT),
        ];
        if !self.move_originator_ae_title.is_empty() {
            e.push(text_element(
                MOVE_ORIGINATOR_APPLICATION_ENTITY_TITLE,
                Vr::AE,
                &self.move_originator_ae_title,
            ));
        }
        if let Some(id) = self.move_originator_message_id {
            e.push(us_element(MOVE_ORIGINATOR_MESSAGE_ID, id));
        }
        e
    }
}
#[derive(Debug, Clone, Default)]
pub struct RetrieveResponse {
    pub affected_sop_class_uid: String,
    pub message_id_being_responded_to: u16,
    pub status: u16,
    pub error_comment: String,
    pub identifier: Option<Object>,
    pub counts: SubOperationCounts,
    pub failed_sop_instance_uid_list: Option<Vec<String>>,
}
pub type CMoveResponse = RetrieveResponse;
pub type CGetResponse = RetrieveResponse;
impl RetrieveResponse {
    pub fn command_set(&self, field: u16) -> Vec<Element> {
        let dst = if self.response_identifier().is_some()
            && retrieve_status_allows_identifier(self.status)
        {
            DATA_SET_PRESENT
        } else {
            NO_DATA_SET
        };
        let mut e = vec![
            ui_element(AFFECTED_SOP_CLASS_UID, &self.affected_sop_class_uid),
            us_element(COMMAND_FIELD, field),
            us_element(
                MESSAGE_ID_BEING_RESPONDED_TO,
                self.message_id_being_responded_to,
            ),
            us_element(COMMAND_DATA_SET_TYPE, dst),
            us_element(STATUS, self.status),
        ];
        self.counts.push(&mut e);
        if !self.error_comment.is_empty() {
            e.push(text_element(ERROR_COMMENT, Vr::LO, &self.error_comment));
        }
        e
    }
    pub(crate) fn response_identifier(&self) -> Option<Object> {
        retrieve_response_identifier(
            self.identifier.as_ref(),
            self.status,
            self.failed_sop_instance_uid_list.as_deref().unwrap_or(&[]),
        )
    }
}
pub(crate) fn retrieve_response_identifier(
    identifier: Option<&Object>,
    status: u16,
    failed: &[String],
) -> Option<Object> {
    if failed.is_empty() || !retrieve_status_allows_identifier(status) {
        return identifier.cloned();
    }
    let valid: Vec<String> = failed
        .iter()
        .filter(|u| object::is_valid_uid(u.as_bytes()))
        .cloned()
        .collect();
    if valid.is_empty() {
        return identifier.cloned();
    }
    let mut kept: Vec<Element> = identifier
        .map(|i| i.elements().to_vec())
        .unwrap_or_default()
        .into_iter()
        .filter(|e| e.tag() != FAILED_SOP_INSTANCE_UID_LIST)
        .collect();
    kept.push(ui_list_element(FAILED_SOP_INSTANCE_UID_LIST, &valid));
    Some(Object::from_elements(kept))
}
pub fn parse_c_move_request(command: &Object) -> Result<CMoveRequest> {
    want_field(command, C_MOVE_RQ, "C-MOVE-RQ", false)?;
    let sop_class_uid = command_uid(command, AFFECTED_SOP_CLASS_UID)?;
    let message_id = cu16(command, MESSAGE_ID)?;
    let dest = match get_string(command, MOVE_DESTINATION) {
        Some(d) if !d.is_empty() => d,
        _ => {
            return Err(Error::new(format!(
                "dicom dimse: missing command element {MOVE_DESTINATION}"
            )));
        }
    };
    let priority = cu16(command, PRIORITY)?;
    let dst = cu16(command, COMMAND_DATA_SET_TYPE)?;
    if dst == NO_DATA_SET {
        return Err(Error::new("dicom dimse: C-MOVE request requires a dataset"));
    }
    let mut req = CMoveRequest {
        affected_sop_class_uid: sop_class_uid,
        message_id,
        priority,
        move_destination: ul::normalize_ae_title(&dest).to_string(),
        ..Default::default()
    };
    if let Some(o) = get_string(command, MOVE_ORIGINATOR_APPLICATION_ENTITY_TITLE) {
        req.move_originator_ae_title = ul::normalize_ae_title(&o).to_string();
    }
    req.move_originator_message_id = optional_command_uint16(command, MOVE_ORIGINATOR_MESSAGE_ID)?;
    Ok(req)
}
fn parse_retrieve_response(
    command: &Object,
    field: u16,
    name: &str,
    get: bool,
) -> Result<RetrieveResponse> {
    want_field(command, field, name, true)?;
    let (sop_class_uid, _) = optional_command_uid(command, AFFECTED_SOP_CLASS_UID)?;
    let msg_id = cu16(command, MESSAGE_ID_BEING_RESPONDED_TO)?;
    let dst = cu16(command, COMMAND_DATA_SET_TYPE)?;
    let status = cu16(command, STATUS)?;
    let mut identifier = None;
    if dst != NO_DATA_SET {
        let class = classify_c_move_status(status);
        if class != CMoveStatus::Success && class != CMoveStatus::Pending {
            identifier = Some(Object::new());
        }
    }
    let counts = SubOperationCounts::parse(command)?;
    let error_comment = get_string(command, ERROR_COMMENT).unwrap_or_default();
    let failed = if get {
        optional_command_uids(command, FAILED_SOP_INSTANCE_UID_LIST)?
    } else {
        None
    };
    Ok(RetrieveResponse {
        affected_sop_class_uid: sop_class_uid,
        message_id_being_responded_to: msg_id,
        status,
        error_comment,
        identifier,
        counts,
        failed_sop_instance_uid_list: failed,
    })
}
pub fn parse_c_move_response(command: &Object) -> Result<CMoveResponse> {
    parse_retrieve_response(command, C_MOVE_RSP, "C-MOVE-RSP", false)
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CGetRequest {
    pub affected_sop_class_uid: String,
    pub message_id: u16,
    pub priority: u16,
}
impl CGetRequest {
    pub fn command_set(&self) -> Vec<Element> {
        vec![
            ui_element(AFFECTED_SOP_CLASS_UID, &self.affected_sop_class_uid),
            us_element(COMMAND_FIELD, C_GET_RQ),
            us_element(MESSAGE_ID, self.message_id),
            us_element(PRIORITY, self.priority),
            us_element(COMMAND_DATA_SET_TYPE, DATA_SET_PRESENT),
        ]
    }
}
pub fn parse_c_get_request(command: &Object) -> Result<CGetRequest> {
    want_field(command, C_GET_RQ, "C-GET-RQ", false)?;
    let sop_class_uid = command_uid(command, AFFECTED_SOP_CLASS_UID)?;
    let message_id = cu16(command, MESSAGE_ID)?;
    let priority = cu16(command, PRIORITY)?;
    let dst = cu16(command, COMMAND_DATA_SET_TYPE)?;
    if dst == NO_DATA_SET {
        return Err(Error::new("dicom dimse: C-GET request requires a dataset"));
    }
    Ok(CGetRequest {
        affected_sop_class_uid: sop_class_uid,
        message_id,
        priority,
    })
}
pub fn parse_c_get_response(command: &Object) -> Result<CGetResponse> {
    parse_retrieve_response(command, C_GET_RSP, "C-GET-RSP", true)
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NormalizedStatusClass {
    Success,
    Warning,
    Failure,
}
impl NormalizedStatusClass {
    pub fn as_str(self) -> &'static str {
        match self {
            NormalizedStatusClass::Success => "success",
            NormalizedStatusClass::Warning => "warning",
            NormalizedStatusClass::Failure => "failure",
        }
    }
}
pub fn classify_normalized_status(status: u16) -> NormalizedStatusClass {
    match status {
        STATUS_SUCCESS => NormalizedStatusClass::Success,
        0x0001 | 0x0107 | 0x0116 | 0xB000..=0xBFFF => NormalizedStatusClass::Warning,
        _ => NormalizedStatusClass::Failure,
    }
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NormalizedStatusFields {
    pub error_comment: String,
    pub error_id: Option<u16>,
    pub offending_elements: Vec<Tag>,
    pub attribute_identifier_list: Vec<Tag>,
}
pub fn check_normalized_status(
    service: &str,
    status: u16,
    fields: Option<&NormalizedStatusFields>,
) -> Option<Error> {
    let class = classify_normalized_status(status);
    if class == NormalizedStatusClass::Success {
        return None;
    }
    let service = match service.trim() {
        "" => "normalized operation",
        s => s,
    };
    let comment = match fields {
        Some(f) if !f.error_comment.is_empty() => format!(": {}", f.error_comment),
        _ => String::new(),
    };
    Some(
        Error::classified(
            Kind::NormalizedStatus,
            format!(
                "{}: {service} returned status 0x{status:04X} ({}){comment}",
                Kind::NormalizedStatus.text(),
                class.as_str()
            ),
        )
        .with_detail(super::error::Detail::Status(status)),
    )
}
fn append_optional_uid(e: &mut Vec<Element>, tag: Tag, value: &str) {
    if !value.is_empty() {
        e.push(ui_element(tag, value));
    }
}
fn append_optional_us(e: &mut Vec<Element>, tag: Tag, value: Option<u16>) {
    if let Some(v) = value {
        e.push(us_element(tag, v));
    }
}
fn append_status_fields(e: &mut Vec<Element>, fields: Option<&NormalizedStatusFields>) {
    let Some(f) = fields else { return };
    if !f.error_comment.is_empty() {
        e.push(text_element(ERROR_COMMENT, Vr::LO, &f.error_comment));
    }
    append_optional_us(e, ERROR_ID, f.error_id);
    if !f.offending_elements.is_empty() {
        e.push(at_element(OFFENDING_ELEMENT, &f.offending_elements));
    }
    if !f.attribute_identifier_list.is_empty() {
        e.push(at_element(
            ATTRIBUTE_IDENTIFIER_LIST,
            &f.attribute_identifier_list,
        ));
    }
}
#[doc(hidden)]
pub fn parse_normalized_status_fields(command: &Object) -> Result<Option<NormalizedStatusFields>> {
    let mut f = NormalizedStatusFields::default();
    let mut present = false;
    if command.get(ERROR_COMMENT).is_some() {
        match get_string(command, ERROR_COMMENT) {
            Some(c) => f.error_comment = c,
            None => {
                return Err(Error::new(format!(
                    "dicom dimse: command element {ERROR_COMMENT} is not a valid string"
                )));
            }
        }
        present = true;
    }
    if let Some(id) = optional_command_uint16(command, ERROR_ID)? {
        f.error_id = Some(id);
        present = true;
    }
    if let Some(t) = optional_command_tags(command, OFFENDING_ELEMENT)? {
        f.offending_elements = t;
        present = true;
    }
    if let Some(t) = optional_command_tags(command, ATTRIBUTE_IDENTIFIER_LIST)? {
        f.attribute_identifier_list = t;
        present = true;
    }
    Ok(present.then_some(f))
}
fn validate_normalized_field(command: &Object, service: &str, want: u16) -> Result<()> {
    let field = cu16(command, COMMAND_FIELD)?;
    if field != want {
        return Err(Error::new(format!(
            "dicom dimse: {service} command field 0x{field:04X}, want 0x{want:04X}"
        )));
    }
    Ok(())
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NormalizedRequest {
    pub command_field: u16,
    pub sop_class_uid: String,
    pub message_id: u16,
    pub command_data_set_type: u16,
    pub sop_instance_uid: String,
    pub type_id: u16,
    pub attribute_identifier_list: Vec<Tag>,
}
impl NormalizedRequest {
    pub fn command_set(&self) -> Vec<Element> {
        let f = self.command_field;
        match f {
            N_EVENT_REPORT_RQ => vec![
                ui_element(AFFECTED_SOP_CLASS_UID, &self.sop_class_uid),
                us_element(COMMAND_FIELD, f),
                us_element(MESSAGE_ID, self.message_id),
                us_element(COMMAND_DATA_SET_TYPE, self.command_data_set_type),
                ui_element(AFFECTED_SOP_INSTANCE_UID, &self.sop_instance_uid),
                us_element(EVENT_TYPE_ID, self.type_id),
            ],
            N_CREATE_RQ => {
                let mut e = vec![
                    ui_element(AFFECTED_SOP_CLASS_UID, &self.sop_class_uid),
                    us_element(COMMAND_FIELD, f),
                    us_element(MESSAGE_ID, self.message_id),
                    us_element(COMMAND_DATA_SET_TYPE, self.command_data_set_type),
                ];
                append_optional_uid(&mut e, AFFECTED_SOP_INSTANCE_UID, &self.sop_instance_uid);
                e
            }
            _ => {
                let dst = match f {
                    N_GET_RQ | N_DELETE_RQ => NO_DATA_SET,
                    N_SET_RQ => DATA_SET_PRESENT,
                    _ => self.command_data_set_type,
                };
                let mut e = vec![
                    ui_element(REQUESTED_SOP_CLASS_UID, &self.sop_class_uid),
                    us_element(COMMAND_FIELD, f),
                    us_element(MESSAGE_ID, self.message_id),
                    us_element(COMMAND_DATA_SET_TYPE, dst),
                    ui_element(REQUESTED_SOP_INSTANCE_UID, &self.sop_instance_uid),
                ];
                if f == N_ACTION_RQ {
                    e.push(us_element(ACTION_TYPE_ID, self.type_id));
                }
                if f == N_GET_RQ && !self.attribute_identifier_list.is_empty() {
                    e.push(at_element(
                        ATTRIBUTE_IDENTIFIER_LIST,
                        &self.attribute_identifier_list,
                    ));
                }
                e
            }
        }
    }
}
pub(crate) fn normalized_service_name(field: u16) -> &'static str {
    match field {
        N_EVENT_REPORT_RQ => "N-EVENT-REPORT-RQ",
        N_EVENT_REPORT_RSP => "N-EVENT-REPORT-RSP",
        N_GET_RQ => "N-GET-RQ",
        N_GET_RSP => "N-GET-RSP",
        N_SET_RQ => "N-SET-RQ",
        N_SET_RSP => "N-SET-RSP",
        N_ACTION_RQ => "N-ACTION-RQ",
        N_ACTION_RSP => "N-ACTION-RSP",
        N_CREATE_RQ => "N-CREATE-RQ",
        N_CREATE_RSP => "N-CREATE-RSP",
        N_DELETE_RQ => "N-DELETE-RQ",
        N_DELETE_RSP => "N-DELETE-RSP",
        _ => "",
    }
}
#[doc(hidden)]
pub fn parse_normalized_request(command: &Object, field: u16) -> Result<NormalizedRequest> {
    let service = normalized_service_name(field);
    validate_normalized_field(command, service, field)?;
    let mut req = NormalizedRequest {
        command_field: field,
        ..Default::default()
    };
    match field {
        N_EVENT_REPORT_RQ => {
            req.sop_class_uid = command_uid(command, AFFECTED_SOP_CLASS_UID)?;
            req.message_id = cu16(command, MESSAGE_ID)?;
            req.command_data_set_type = cu16(command, COMMAND_DATA_SET_TYPE)?;
            req.sop_instance_uid = command_uid(command, AFFECTED_SOP_INSTANCE_UID)?;
            req.type_id = cu16(command, EVENT_TYPE_ID)?;
        }
        N_CREATE_RQ => {
            req.sop_class_uid = command_uid(command, AFFECTED_SOP_CLASS_UID)?;
            req.message_id = cu16(command, MESSAGE_ID)?;
            req.command_data_set_type = cu16(command, COMMAND_DATA_SET_TYPE)?;
            req.sop_instance_uid = optional_command_uid(command, AFFECTED_SOP_INSTANCE_UID)?.0;
        }
        _ => {
            req.sop_class_uid = command_uid(command, REQUESTED_SOP_CLASS_UID)?;
            req.message_id = cu16(command, MESSAGE_ID)?;
            let dst = cu16(command, COMMAND_DATA_SET_TYPE)?;
            req.command_data_set_type = dst;
            req.sop_instance_uid = command_uid(command, REQUESTED_SOP_INSTANCE_UID)?;
            match field {
                N_GET_RQ | N_DELETE_RQ => {
                    if dst != NO_DATA_SET {
                        return Err(Error::new(format!(
                            "dicom dimse: {service} CommandDataSetType 0x{dst:04X}, want no dataset 0x{NO_DATA_SET:04X}"
                        )));
                    }
                    if field == N_GET_RQ {
                        req.attribute_identifier_list =
                            optional_command_tags(command, ATTRIBUTE_IDENTIFIER_LIST)?
                                .unwrap_or_default();
                    }
                }
                N_SET_RQ => {
                    if dst == NO_DATA_SET {
                        return Err(Error::new(format!(
                            "dicom dimse: {service} requires a Modification List dataset"
                        )));
                    }
                }
                N_ACTION_RQ => {
                    req.type_id = cu16(command, ACTION_TYPE_ID)?;
                }
                _ => {}
            }
        }
    }
    Ok(req)
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NormalizedResponse {
    pub command_field: u16,
    pub affected_sop_class_uid: String,
    pub message_id_being_responded_to: u16,
    pub command_data_set_type: u16,
    pub status: u16,
    pub affected_sop_instance_uid: String,
    pub type_id: Option<u16>,
    pub status_fields: Option<NormalizedStatusFields>,
}
impl NormalizedResponse {
    pub fn command_set(&self) -> Vec<Element> {
        let f = self.command_field;
        let mut e = Vec::new();
        append_optional_uid(&mut e, AFFECTED_SOP_CLASS_UID, &self.affected_sop_class_uid);
        let dst = if f == N_DELETE_RSP {
            NO_DATA_SET
        } else {
            self.command_data_set_type
        };
        e.push(us_element(COMMAND_FIELD, f));
        e.push(us_element(
            MESSAGE_ID_BEING_RESPONDED_TO,
            self.message_id_being_responded_to,
        ));
        e.push(us_element(COMMAND_DATA_SET_TYPE, dst));
        e.push(us_element(STATUS, self.status));
        append_optional_uid(
            &mut e,
            AFFECTED_SOP_INSTANCE_UID,
            &self.affected_sop_instance_uid,
        );
        match f {
            N_EVENT_REPORT_RSP => append_optional_us(&mut e, EVENT_TYPE_ID, self.type_id),
            N_ACTION_RSP => append_optional_us(&mut e, ACTION_TYPE_ID, self.type_id),
            _ => {}
        }
        append_status_fields(&mut e, self.status_fields.as_ref());
        e
    }
}
#[doc(hidden)]
pub fn parse_normalized_response(command: &Object, field: u16) -> Result<NormalizedResponse> {
    let service = normalized_service_name(field);
    validate_normalized_field(command, service, field)?;
    let message_id = cu16(command, MESSAGE_ID_BEING_RESPONDED_TO)?;
    let dst = cu16(command, COMMAND_DATA_SET_TYPE)?;
    let status = cu16(command, STATUS)?;
    let (class_uid, _) = optional_command_uid(command, AFFECTED_SOP_CLASS_UID)?;
    let (instance_uid, _) = optional_command_uid(command, AFFECTED_SOP_INSTANCE_UID)?;
    let mut type_id = None;
    if field == N_EVENT_REPORT_RSP {
        type_id = optional_command_uint16(command, EVENT_TYPE_ID)?;
        if status == STATUS_SUCCESS && dst != NO_DATA_SET && type_id.is_none() {
            return Err(Error::new(format!(
                "dicom dimse: {service} success response dataset requires EventTypeID"
            )));
        }
    }
    let fields = parse_normalized_status_fields(command)?;
    if field == N_ACTION_RSP {
        type_id = optional_command_uint16(command, ACTION_TYPE_ID)?;
        if status == STATUS_SUCCESS && dst != NO_DATA_SET && type_id.is_none() {
            return Err(Error::new(
                "dicom dimse: N-ACTION-RSP success response dataset requires ActionTypeID",
            ));
        }
    }
    if field == N_DELETE_RSP && dst != NO_DATA_SET {
        return Err(Error::new(format!(
            "dicom dimse: N-DELETE-RSP CommandDataSetType 0x{dst:04X}, want no dataset 0x{NO_DATA_SET:04X}"
        )));
    }
    Ok(NormalizedResponse {
        command_field: field,
        affected_sop_class_uid: class_uid,
        message_id_being_responded_to: message_id,
        command_data_set_type: dst,
        status,
        affected_sop_instance_uid: instance_uid,
        type_id,
        status_fields: fields,
    })
}
pub(crate) fn command_status_category(status: u16) -> &'static str {
    match status {
        STATUS_SUCCESS => "success",
        0xFE00 => "canceled",
        0xFF00 | 0xFF01 => "pending",
        0x0001 | 0x0107 | 0x0116 => "warning",
        s if s & 0xF000 == 0xB000 => "warning",
        _ => "failure",
    }
}
