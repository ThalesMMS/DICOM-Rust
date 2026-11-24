#![forbid(unsafe_code)]
mod dataset;
mod dicomweb;
mod filter;
mod memory;
mod normalized;
mod query;
mod service;
mod uri;
mod watch;
use crate::net::dimse;
use crate::object::{DataSet, Object, Tag};
pub use dataset::*;
pub use dicomweb::*;
pub use filter::*;
pub use memory::*;
pub use normalized::*;
pub use query::*;
pub use service::*;
use std::fmt;
pub use watch::*;
pub type Time = dimse::StorageCommitmentTime;
pub const PUSH_SOP_CLASS_UID: &str = "1.2.840.10008.5.1.4.34.6.1";
pub const WATCH_SOP_CLASS_UID: &str = "1.2.840.10008.5.1.4.34.6.2";
pub const PULL_SOP_CLASS_UID: &str = "1.2.840.10008.5.1.4.34.6.3";
pub const EVENT_SOP_CLASS_UID: &str = "1.2.840.10008.5.1.4.34.6.4";
pub const QUERY_SOP_CLASS_UID: &str = "1.2.840.10008.5.1.4.34.6.5";
pub const GLOBAL_SUBSCRIPTION_SOP_INSTANCE_UID: &str = "1.2.840.10008.5.1.4.34.5";
pub const FILTERED_GLOBAL_SUBSCRIPTION_SOP_INSTANCE_UID: &str = "1.2.840.10008.5.1.4.34.5.1";
pub type State = String;
pub const STATE_SCHEDULED: &str = "SCHEDULED";
pub const STATE_IN_PROGRESS: &str = "IN PROGRESS";
pub const STATE_COMPLETED: &str = "COMPLETED";
pub const STATE_CANCELED: &str = "CANCELED";
pub const STATUS_SUCCESS: u16 = 0x0000;
pub const STATUS_REQUESTED_OPTIONAL_UNSUPPORTED: u16 = 0x0001;
pub const STATUS_NO_SUCH_ATTRIBUTE: u16 = 0x0105;
pub const STATUS_INVALID_ATTRIBUTE_VALUE: u16 = 0x0106;
pub const STATUS_DUPLICATE_SOP_INSTANCE: u16 = 0x0111;
pub const STATUS_NO_SUCH_SOP_INSTANCE: u16 = 0x0112;
pub const STATUS_NO_SUCH_ARGUMENT: u16 = 0x0114;
pub const STATUS_INVALID_ARGUMENT_VALUE: u16 = 0x0115;
pub const STATUS_INVALID_OBJECT_INSTANCE: u16 = 0x0117;
pub const STATUS_MISSING_ATTRIBUTE: u16 = 0x0120;
pub const STATUS_MISSING_ATTRIBUTE_VALUE: u16 = 0x0121;
pub const STATUS_NOT_AUTHORIZED: u16 = 0x0124;
pub const STATUS_DELETION_LOCK_NOT_GRANTED: u16 = 0xB301;
pub const STATUS_ALREADY_CANCELED: u16 = 0xB304;
pub const STATUS_COERCED_INVALID_VALUES: u16 = 0xB305;
pub const STATUS_ALREADY_COMPLETED: u16 = 0xB306;
pub const STATUS_MAY_NO_LONGER_BE_UPDATED: u16 = 0xC300;
pub const STATUS_INCORRECT_TRANSACTION_UID: u16 = 0xC301;
pub const STATUS_ALREADY_IN_PROGRESS: u16 = 0xC302;
pub const STATUS_ONLY_SCHEDULED_VIA_CREATE: u16 = 0xC303;
pub const STATUS_FINAL_STATE_REQUIREMENTS_NOT_MET: u16 = 0xC304;
pub const STATUS_UPS_NOT_FOUND: u16 = 0xC307;
pub const STATUS_RECEIVING_AE_UNKNOWN: u16 = 0xC308;
pub const STATUS_CREATE_STATE_NOT_SCHEDULED: u16 = 0xC309;
pub const STATUS_NOT_IN_PROGRESS: u16 = 0xC310;
pub const STATUS_ALREADY_COMPLETED_CANCEL_REQUEST: u16 = 0xC311;
pub const STATUS_PERFORMER_CANNOT_BE_CONTACTED: u16 = 0xC312;
pub const STATUS_PERFORMER_CHOOSES_NOT_TO_CANCEL: u16 = 0xC313;
pub const STATUS_ACTION_NOT_APPROPRIATE: u16 = 0xC314;
pub const STATUS_EVENT_REPORTS_UNSUPPORTED: u16 = 0xC315;
pub const STATUS_RESOURCE_LIMITATION: u16 = 0x0213;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    NotFound,
    Conflict,
    ConcurrentUpdate,
    ResourceLimit,
    InvalidDataSet,
    InvalidState,
    DeliveryFailed,
    Repository,
    QueryIdentifier,
    NoChange,
}
impl Kind {
    pub fn text(self) -> &'static str {
        match self {
            Kind::NotFound => "dicom ups: record not found",
            Kind::Conflict => "dicom ups: record conflict",
            Kind::ConcurrentUpdate => "dicom ups: concurrent update",
            Kind::ResourceLimit => "dicom ups: resource limit",
            Kind::InvalidDataSet => "dicom ups: invalid dataset",
            Kind::InvalidState => "dicom ups: invalid state",
            Kind::DeliveryFailed => "dicom ups: event delivery failed",
            Kind::Repository => "dicom ups: repository operation failed",
            Kind::QueryIdentifier => "dicom ups: query Identifier invalid",
            Kind::NoChange => "dicom ups: no effective change",
        }
    }
}
#[derive(Debug, Clone)]
pub enum Error {
    Sentinel(Kind),
    Repository(Box<Error>),
    Status {
        operation: String,
        status: u16,
        err: Option<Box<Error>>,
    },
    Delivery {
        class: DeliveryFailureClass,
        status: u16,
        retryable: bool,
        err: Option<Box<Error>>,
    },
    Filter {
        status: u16,
        err: Box<Error>,
    },
    Dimse(dimse::Error),
    Join(Vec<Error>),
    Other(String),
}
pub type Result<T> = std::result::Result<T, Error>;
impl Error {
    pub fn sentinel(kind: Kind) -> Error {
        Error::Sentinel(kind)
    }
    pub fn is(&self, kind: Kind) -> bool {
        match self {
            Error::Sentinel(k) => *k == kind,
            Error::Repository(e) => kind == Kind::Repository || e.is(kind),
            Error::Status { err, .. } => err.as_ref().is_some_and(|e| e.is(kind)),
            Error::Delivery { err, .. } => {
                kind == Kind::DeliveryFailed || err.as_ref().is_some_and(|e| e.is(kind))
            }
            Error::Filter { err, .. } => err.is(kind),
            Error::Join(list) => list.iter().any(|e| e.is(kind)),
            Error::Dimse(_) | Error::Other(_) => false,
        }
    }
    pub fn is_canceled(&self) -> bool {
        self.any_dimse(|e| e.is_canceled())
    }
    pub fn is_deadline(&self) -> bool {
        self.any_dimse(|e| e.is_deadline())
    }
    #[doc(hidden)]
    pub fn any_dimse(&self, f: impl Fn(&dimse::Error) -> bool + Copy) -> bool {
        match self {
            Error::Dimse(e) => f(e),
            Error::Repository(e) | Error::Filter { err: e, .. } => e.any_dimse(f),
            Error::Status { err, .. } | Error::Delivery { err, .. } => {
                err.as_ref().is_some_and(|e| e.any_dimse(f))
            }
            Error::Join(list) => list.iter().any(|e| e.any_dimse(f)),
            _ => false,
        }
    }
    pub fn dimse_error(&self) -> Option<&dimse::Error> {
        match self {
            Error::Dimse(e) => Some(e),
            Error::Repository(e) | Error::Filter { err: e, .. } => e.dimse_error(),
            Error::Status { err, .. } | Error::Delivery { err, .. } => {
                err.as_ref().and_then(|e| e.dimse_error())
            }
            Error::Join(list) => list.iter().find_map(|e| e.dimse_error()),
            _ => None,
        }
    }
    pub fn status_error(&self) -> Option<(&str, u16)> {
        match self {
            Error::Status {
                operation, status, ..
            } => Some((operation, *status)),
            Error::Repository(e) | Error::Filter { err: e, .. } => e.status_error(),
            Error::Delivery { err, .. } => err.as_ref().and_then(|e| e.status_error()),
            Error::Join(list) => list.iter().find_map(|e| e.status_error()),
            _ => None,
        }
    }
    pub fn delivery_error(&self) -> Option<(DeliveryFailureClass, u16, bool)> {
        match self {
            Error::Delivery {
                class,
                status,
                retryable,
                ..
            } => Some((*class, *status, *retryable)),
            Error::Repository(e) | Error::Filter { err: e, .. } => e.delivery_error(),
            Error::Status { err, .. } => err.as_ref().and_then(|e| e.delivery_error()),
            Error::Join(list) => list.iter().find_map(|e| e.delivery_error()),
            _ => None,
        }
    }
    pub fn is_repository_error(&self) -> bool {
        match self {
            Error::Repository(_) => true,
            Error::Status { err, .. } | Error::Delivery { err, .. } => {
                err.as_ref().is_some_and(|e| e.is_repository_error())
            }
            Error::Filter { err, .. } => err.is_repository_error(),
            Error::Join(list) => list.iter().any(|e| e.is_repository_error()),
            _ => false,
        }
    }
    pub fn join(errors: Vec<Error>) -> Option<Error> {
        match errors.len() {
            0 => None,
            _ => Some(Error::Join(errors)),
        }
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Sentinel(k) => f.write_str(k.text()),
            Error::Repository(_) => f.write_str(Kind::Repository.text()),
            Error::Status {
                operation, status, ..
            } => write!(
                f,
                "dicom ups: {operation} failed with status 0x{status:04X}"
            ),
            Error::Delivery { class, .. } => {
                write!(f, "dicom ups: event delivery failed ({})", class.as_str())
            }
            Error::Filter { .. } => f.write_str(Kind::InvalidDataSet.text()),
            Error::Dimse(e) => f.write_str(e.text()),
            Error::Join(list) => {
                let texts: Vec<String> = list.iter().map(|e| e.to_string()).collect();
                f.write_str(&texts.join("\n"))
            }
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
impl From<Kind> for Error {
    fn from(k: Kind) -> Error {
        Error::Sentinel(k)
    }
}
pub fn is_status(err: &Error, status: u16) -> bool {
    err.status_error().is_some_and(|(_, s)| s == status)
}
pub(crate) fn status_error(operation: &str, status: u16, err: Error) -> Error {
    Error::Status {
        operation: operation.to_string(),
        status,
        err: Some(Box::new(err)),
    }
}
pub(crate) fn to_dimse(e: Error) -> dimse::Error {
    match e {
        Error::Dimse(d) => d,
        other => dimse::Error::new(other.to_string()),
    }
}
#[derive(Debug, Clone, Default)]
pub struct Step {
    pub sop_instance_uid: String,
    pub state: State,
    pub transaction_uid: String,
    pub attributes: DataSet,
    pub version: u64,
    pub sequence: u64,
    pub created_at: Time,
    pub updated_at: Time,
}
#[derive(Debug, Clone, Default)]
pub struct StepQuery {
    pub after_sequence: u64,
    pub limit: i64,
    pub states: Vec<State>,
}
pub type EventType = u16;
pub const EVENT_STATE_REPORT: EventType = 1;
pub const EVENT_CANCEL_REQUESTED: EventType = 2;
pub const EVENT_PROGRESS_REPORT: EventType = 3;
pub const EVENT_SCP_STATUS_CHANGE: EventType = 4;
pub const EVENT_UPS_ASSIGNED: EventType = 5;
#[derive(Debug, Clone, Default)]
pub struct Event {
    pub id: String,
    pub event_type: EventType,
    pub sop_instance_uid: String,
    pub step_version: u64,
    pub information: DataSet,
    pub direct_receiving_ae: String,
    pub created_at: Time,
}
#[derive(Debug, Clone, Default)]
pub struct EventQuery {
    pub sop_instance_uid: String,
    pub after_id: String,
    pub limit: i64,
}
#[derive(Debug, Clone)]
pub struct ReceivedEvent<'a> {
    pub sop_instance_uid: String,
    pub event_type: EventType,
    pub information: &'a Object,
}
pub trait EventHandler: Send + Sync {
    fn handle_ups_event(&self, ctx: &crate::net::ul::Ctx, event: &ReceivedEvent<'_>) -> Result<()>;
}
pub struct EventHandlerFunc<F>(pub F);
impl<F> EventHandler for EventHandlerFunc<F>
where
    F: Fn(&crate::net::ul::Ctx, &ReceivedEvent<'_>) -> Result<()> + Send + Sync,
{
    fn handle_ups_event(&self, ctx: &crate::net::ul::Ctx, event: &ReceivedEvent<'_>) -> Result<()> {
        (self.0)(ctx, event)
    }
}
#[derive(Debug, Clone, Default)]
pub struct StepMutation {
    pub expected_version: u64,
    pub next: Step,
}
#[derive(Debug, Clone, Default)]
pub struct CommitRequest {
    pub step: Option<StepMutation>,
    pub subscription: Option<SubscriptionMutation>,
    pub events: Vec<Event>,
}
#[derive(Debug, Clone, Default)]
pub struct CommitResult {
    pub step: Option<Step>,
    pub subscription: Option<Subscription>,
}
pub const SCP_STATUS_RESTARTED: &str = "RESTARTED";
pub const SCP_STATUS_GOING_DOWN: &str = "GOING DOWN";
pub const LIST_STATUS_WARM_START: &str = "WARM START";
pub const LIST_STATUS_COLD_START: &str = "COLD START";
#[derive(Debug, Clone, Default)]
pub struct ScpStatusChange {
    pub status: String,
    pub subscription_list_status: String,
    pub ups_list_status: String,
}
#[derive(Debug, Clone, Default)]
pub struct CreateRequest<'a> {
    pub sop_instance_uid: String,
    pub attributes: Option<&'a Object>,
}
#[derive(Debug, Clone, Default)]
pub struct SetRequest<'a> {
    pub sop_instance_uid: String,
    pub transaction_uid: String,
    pub modifications: Option<&'a Object>,
}
#[derive(Debug, Clone, Default)]
pub struct ChangeStateRequest {
    pub sop_instance_uid: String,
    pub state: State,
    pub transaction_uid: String,
}
#[derive(Debug, Clone, Default)]
pub struct CancelRequest<'a> {
    pub sop_instance_uid: String,
    pub requesting_ae_title: String,
    pub information: Option<&'a Object>,
}
const fn tag(g: u16, e: u16) -> Tag {
    Tag::new(g, e)
}
pub const TAG_SPECIFIC_CHARACTER_SET: Tag = tag(0x0008, 0x0005);
pub const TAG_TIMEZONE_OFFSET_FROM_UTC: Tag = tag(0x0008, 0x0201);
pub const TAG_SOP_CLASS_UID: Tag = tag(0x0008, 0x0016);
pub const TAG_SOP_INSTANCE_UID: Tag = tag(0x0008, 0x0018);
pub const TAG_TRANSACTION_UID: Tag = tag(0x0008, 0x1195);
pub const TAG_STUDY_INSTANCE_UID: Tag = tag(0x0020, 0x000D);
pub const TAG_PATIENT_NAME: Tag = tag(0x0010, 0x0010);
pub const TAG_PATIENT_ID: Tag = tag(0x0010, 0x0020);
pub const TAG_ISSUER_OF_PATIENT_ID: Tag = tag(0x0010, 0x0021);
pub const TAG_ISSUER_OF_PATIENT_ID_QUALIFIERS_SEQUENCE: Tag = tag(0x0010, 0x0024);
pub const TAG_OTHER_PATIENT_IDS_SEQUENCE: Tag = tag(0x0010, 0x1002);
pub const TAG_UNIVERSAL_ENTITY_ID: Tag = tag(0x0040, 0x0032);
pub const TAG_UNIVERSAL_ENTITY_ID_TYPE: Tag = tag(0x0040, 0x0033);
pub const TAG_IDENTIFIER_TYPE_CODE: Tag = tag(0x0040, 0x0035);
pub const TAG_ASSIGNING_FACILITY_SEQUENCE: Tag = tag(0x0040, 0x0036);
pub const TAG_ASSIGNING_JURISDICTION_CODE_SEQUENCE: Tag = tag(0x0040, 0x0039);
pub const TAG_ASSIGNING_AGENCY_OR_DEPARTMENT_CODE_SEQUENCE: Tag = tag(0x0040, 0x003A);
pub const TAG_LOCAL_NAMESPACE_ENTITY_ID: Tag = tag(0x0040, 0x0031);
pub const TAG_PATIENT_BIRTH_DATE: Tag = tag(0x0010, 0x0030);
pub const TAG_PATIENT_SEX: Tag = tag(0x0010, 0x0040);
pub const TAG_ADMISSION_ID: Tag = tag(0x0038, 0x0010);
pub const TAG_ISSUER_OF_ADMISSION_ID_SEQUENCE: Tag = tag(0x0038, 0x0014);
pub const TAG_ADMITTING_DIAGNOSES_DESCRIPTION: Tag = tag(0x0008, 0x1080);
pub const TAG_ADMITTING_DIAGNOSES_CODE_SEQUENCE: Tag = tag(0x0008, 0x1084);
pub const TAG_REFERENCED_REQUEST_SEQUENCE: Tag = tag(0x0040, 0xA370);
pub const TAG_REPLACED_PROCEDURE_STEP_SEQUENCE: Tag = tag(0x0074, 0x1224);
pub const TAG_ACCESSION_NUMBER: Tag = tag(0x0008, 0x0050);
pub const TAG_ISSUER_OF_ACCESSION_NUMBER_SEQUENCE: Tag = tag(0x0008, 0x0051);
pub const TAG_ORDER_PLACER_IDENTIFIER_SEQUENCE: Tag = tag(0x0040, 0x0026);
pub const TAG_ORDER_FILLER_IDENTIFIER_SEQUENCE: Tag = tag(0x0040, 0x0027);
pub const TAG_REQUESTED_PROCEDURE_ID: Tag = tag(0x0040, 0x1001);
pub const TAG_REQUESTED_PROCEDURE_DESCRIPTION: Tag = tag(0x0032, 0x1060);
pub const TAG_REQUESTED_PROCEDURE_CODE_SEQUENCE: Tag = tag(0x0032, 0x1064);
pub const TAG_SCHEDULED_PROCEDURE_STEP_PRIORITY: Tag = tag(0x0074, 0x1200);
pub const TAG_PROCEDURE_STEP_LABEL: Tag = tag(0x0074, 0x1204);
pub const TAG_WORKLIST_LABEL: Tag = tag(0x0074, 0x1202);
pub const TAG_SCHEDULED_PROCESSING_PARAMETERS_SEQUENCE: Tag = tag(0x0074, 0x1210);
pub const TAG_VALUE_TYPE: Tag = tag(0x0040, 0xA040);
pub const TAG_CONCEPT_NAME_CODE_SEQUENCE: Tag = tag(0x0040, 0xA043);
pub const TAG_DATE_TIME: Tag = tag(0x0040, 0xA120);
pub const TAG_DATE: Tag = tag(0x0040, 0xA121);
pub const TAG_TIME: Tag = tag(0x0040, 0xA122);
pub const TAG_PERSON_NAME: Tag = tag(0x0040, 0xA123);
pub const TAG_UID: Tag = tag(0x0040, 0xA124);
pub const TAG_TEXT_VALUE: Tag = tag(0x0040, 0xA160);
pub const TAG_CONCEPT_CODE_SEQUENCE: Tag = tag(0x0040, 0xA168);
pub const TAG_NUMERIC_VALUE: Tag = tag(0x0040, 0xA30A);
pub const TAG_MEASUREMENT_UNITS_CODE_SEQUENCE: Tag = tag(0x0040, 0x08EA);
pub const TAG_SCHEDULED_STATION_NAME_CODE_SEQUENCE: Tag = tag(0x0040, 0x4025);
pub const TAG_SCHEDULED_STATION_CLASS_CODE_SEQUENCE: Tag = tag(0x0040, 0x4026);
pub const TAG_SCHEDULED_STATION_GEOGRAPHIC_LOCATION_CODE_SEQUENCE: Tag = tag(0x0040, 0x4027);
pub const TAG_SCHEDULED_HUMAN_PERFORMERS_SEQUENCE: Tag = tag(0x0040, 0x4034);
pub const TAG_HUMAN_PERFORMER_CODE_SEQUENCE: Tag = tag(0x0040, 0x4009);
pub const TAG_HUMAN_PERFORMER_ORGANIZATION: Tag = tag(0x0040, 0x4036);
pub const TAG_HUMAN_PERFORMER_NAME: Tag = tag(0x0040, 0x4037);
pub const TAG_SCHEDULED_PROCEDURE_STEP_START_DATE_TIME: Tag = tag(0x0040, 0x4005);
pub const TAG_SCHEDULED_WORKITEM_CODE_SEQUENCE: Tag = tag(0x0040, 0x4018);
pub const TAG_COMMENTS_ON_SCHEDULED_PROCEDURE_STEP: Tag = tag(0x0040, 0x0400);
pub const TAG_INPUT_READINESS_STATE: Tag = tag(0x0040, 0x4041);
pub const TAG_INPUT_INFORMATION_SEQUENCE: Tag = tag(0x0040, 0x4021);
pub const TAG_TYPE_OF_INSTANCES: Tag = tag(0x0040, 0xE020);
pub const TAG_REFERENCED_SOP_SEQUENCE: Tag = tag(0x0008, 0x1199);
pub const TAG_REFERENCED_SOP_CLASS_UID: Tag = tag(0x0008, 0x1150);
pub const TAG_REFERENCED_SOP_INSTANCE_UID: Tag = tag(0x0008, 0x1155);
pub const TAG_DICOM_RETRIEVAL_SEQUENCE: Tag = tag(0x0040, 0xE021);
pub const TAG_DICOM_MEDIA_RETRIEVAL_SEQUENCE: Tag = tag(0x0040, 0xE022);
pub const TAG_WADO_RETRIEVAL_SEQUENCE: Tag = tag(0x0040, 0xE023);
pub const TAG_XDS_RETRIEVAL_SEQUENCE: Tag = tag(0x0040, 0xE024);
pub const TAG_WADO_RS_RETRIEVAL_SEQUENCE: Tag = tag(0x0040, 0xE025);
pub const TAG_RETRIEVE_AE_TITLE: Tag = tag(0x0008, 0x0054);
pub const TAG_STORAGE_MEDIA_FILE_SET_ID: Tag = tag(0x0088, 0x0130);
pub const TAG_STORAGE_MEDIA_FILE_SET_UID: Tag = tag(0x0088, 0x0140);
pub const TAG_RETRIEVE_URI: Tag = tag(0x0040, 0xE010);
pub const TAG_RETRIEVE_URL: Tag = tag(0x0008, 0x1190);
pub const TAG_REPOSITORY_UNIQUE_ID: Tag = tag(0x0040, 0xE030);
pub const TAG_HOME_COMMUNITY_ID: Tag = tag(0x0040, 0xE031);
pub const TAG_SCHEDULED_PROCEDURE_STEP_MODIFICATION_DATE_TIME: Tag = tag(0x0040, 0x4010);
pub const TAG_EXPECTED_COMPLETION_DATE_TIME: Tag = tag(0x0040, 0x4011);
pub const TAG_PROCEDURE_STEP_STATE: Tag = tag(0x0074, 0x1000);
pub const TAG_PROCEDURE_STEP_PROGRESS_INFORMATION_SEQUENCE: Tag = tag(0x0074, 0x1002);
pub const TAG_PROCEDURE_STEP_PROGRESS: Tag = tag(0x0074, 0x1004);
pub const TAG_PROCEDURE_STEP_PROGRESS_DESCRIPTION: Tag = tag(0x0074, 0x1006);
pub const TAG_PROCEDURE_STEP_PROGRESS_PARAMETERS_SEQUENCE: Tag = tag(0x0074, 0x1007);
pub const TAG_CONTENT_ITEM_MODIFIER_SEQUENCE: Tag = tag(0x0040, 0x0441);
pub const TAG_PROCEDURE_STEP_COMMUNICATIONS_URI_SEQUENCE: Tag = tag(0x0074, 0x1008);
pub const TAG_PROCEDURE_STEP_CANCELLATION_DATE_TIME: Tag = tag(0x0040, 0x4052);
pub const TAG_PROCEDURE_STEP_DISCONTINUATION_REASON_CODE_SEQUENCE: Tag = tag(0x0074, 0x100E);
pub const TAG_UNIFIED_PROCEDURE_STEP_PERFORMED_PROCEDURE_SEQUENCE: Tag = tag(0x0074, 0x1216);
pub const TAG_ACTUAL_HUMAN_PERFORMERS_SEQUENCE: Tag = tag(0x0040, 0x4035);
pub const TAG_PERFORMED_STATION_NAME_CODE_SEQUENCE: Tag = tag(0x0040, 0x4028);
pub const TAG_PERFORMED_PROCEDURE_STEP_START_DATE_TIME: Tag = tag(0x0040, 0x4050);
pub const TAG_PERFORMED_WORKITEM_CODE_SEQUENCE: Tag = tag(0x0040, 0x4019);
pub const TAG_PERFORMED_PROCEDURE_STEP_END_DATE_TIME: Tag = tag(0x0040, 0x4051);
pub const TAG_PERFORMED_PROCESSING_PARAMETERS_SEQUENCE: Tag = tag(0x0074, 0x1212);
pub const TAG_OUTPUT_INFORMATION_SEQUENCE: Tag = tag(0x0040, 0x4033);
pub const TAG_REASON_FOR_CANCELLATION: Tag = tag(0x0074, 0x1238);
pub const TAG_CONTACT_URI: Tag = tag(0x0074, 0x100A);
pub const TAG_CONTACT_DISPLAY_NAME: Tag = tag(0x0074, 0x100C);
pub const TAG_REQUESTING_AE: Tag = tag(0x0074, 0x1236);
pub const TAG_RECEIVING_AE: Tag = tag(0x0074, 0x1234);
pub const TAG_DELETION_LOCK: Tag = tag(0x0074, 0x1230);
pub const TAG_SCP_STATUS: Tag = tag(0x0074, 0x1242);
pub const TAG_SUBSCRIPTION_LIST_STATUS: Tag = tag(0x0074, 0x1244);
pub const TAG_UNIFIED_PROCEDURE_STEP_LIST_STATUS: Tag = tag(0x0074, 0x1246);
