#![forbid(unsafe_code)]
pub const PDU_ASSOCIATE_RQ: u8 = 0x01;
pub const PDU_ASSOCIATE_AC: u8 = 0x02;
pub const PDU_ASSOCIATE_RJ: u8 = 0x03;
pub const PDU_DATA_TF: u8 = 0x04;
pub const PDU_RELEASE_RQ: u8 = 0x05;
pub const PDU_RELEASE_RP: u8 = 0x06;
pub const PDU_ABORT: u8 = 0x07;
pub const MAX_PRESENTATION_CONTEXTS: usize = 128;
pub const PDU_HEADER_SIZE: u32 = 6;
pub const PDV_HEADER_SIZE: u32 = 6;
pub const DEFAULT_MAX_PDU: u32 = 16_384 - PDU_HEADER_SIZE;
pub const DEFAULT_MAX_ASSOCIATION_PDU: u32 = 1 << 20;
pub const MINIMUM_PDU_SIZE: u32 = 1_024 - PDU_HEADER_SIZE;
pub const APPLICATION_CONTEXT_NAME: &str = "1.2.840.10008.3.1.1.1";
pub const IMPLEMENTATION_CLASS_UID: &str = "1.2.826.0.1.3680043.10.543.1";
pub const IMPLEMENTATION_VERSION_NAME: &str = "DCMRUST";
pub const MAX_AE_TITLE_LENGTH: usize = 16;
pub const DEFAULT_PROTOCOL_VERSION: u16 = 1;
pub const IMPLICIT_VR_LITTLE_ENDIAN: &str = "1.2.840.10008.1.2";
pub const EXPLICIT_VR_LITTLE_ENDIAN: &str = "1.2.840.10008.1.2.1";
pub const ITEM_APPLICATION_CONTEXT: u8 = 0x10;
pub const ITEM_PRESENTATION_CONTEXT_RQ: u8 = 0x20;
pub const ITEM_PRESENTATION_CONTEXT_AC: u8 = 0x21;
pub const ITEM_ABSTRACT_SYNTAX: u8 = 0x30;
pub const ITEM_TRANSFER_SYNTAX: u8 = 0x40;
pub const ITEM_USER_INFORMATION: u8 = 0x50;
pub const SUB_ITEM_MAX_LENGTH: u8 = 0x51;
pub const SUB_ITEM_IMPLEMENTATION_CLASS_UID: u8 = 0x52;
pub const SUB_ITEM_ASYNCHRONOUS_OPERATIONS: u8 = 0x53;
pub const SUB_ITEM_ROLE_SELECTION: u8 = 0x54;
pub const SUB_ITEM_IMPLEMENTATION_VERSION_NAME: u8 = 0x55;
pub const SUB_ITEM_SOP_CLASS_EXTENDED_NEGOTIATION: u8 = 0x56;
pub const SUB_ITEM_SOP_CLASS_COMMON_EXTENDED_NEGOTIATION: u8 = 0x57;
pub const SUB_ITEM_USER_IDENTITY: u8 = 0x58;
pub const SUB_ITEM_USER_IDENTITY_RESPONSE: u8 = 0x59;
pub const ASSOCIATE_RJ_RESULT_PERMANENT: u8 = 1;
pub const ASSOCIATE_RJ_RESULT_TRANSIENT: u8 = 2;
pub const ASSOCIATE_RJ_SOURCE_SERVICE_USER: u8 = 1;
pub const ASSOCIATE_RJ_SOURCE_SERVICE_PROVIDER_ACSE: u8 = 2;
pub const ASSOCIATE_RJ_SOURCE_SERVICE_PROVIDER_PRESENTATION: u8 = 3;
pub const ASSOCIATE_RJ_REASON_NO_REASON_GIVEN: u8 = 1;
pub const ASSOCIATE_RJ_REASON_APPLICATION_CONTEXT_NAME_NOT_SUPPORTED: u8 = 2;
pub const ASSOCIATE_RJ_REASON_PROTOCOL_VERSION_NOT_SUPPORTED: u8 = 2;
pub const ASSOCIATE_RJ_REASON_CALLING_AE_TITLE_NOT_RECOGNIZED: u8 = 3;
pub const ASSOCIATE_RJ_REASON_CALLED_AE_TITLE_NOT_RECOGNIZED: u8 = 7;
pub const ASSOCIATE_RJ_REASON_LOCAL_LIMIT_EXCEEDED: u8 = 2;
pub const PRESENTATION_CONTEXT_ACCEPTANCE: u8 = 0;
pub const PRESENTATION_CONTEXT_USER_REJECTION: u8 = 1;
pub const PRESENTATION_CONTEXT_NO_REASON: u8 = 2;
pub const PRESENTATION_CONTEXT_ABSTRACT_SYNTAX_NOT_SUPPORTED: u8 = 3;
pub const PRESENTATION_CONTEXT_TRANSFER_SYNTAXES_NOT_SUPPORTED: u8 = 4;
pub const ABORT_SOURCE_SERVICE_USER: u8 = 0;
pub const ABORT_SOURCE_RESERVED: u8 = 1;
pub const ABORT_SOURCE_SERVICE_PROVIDER: u8 = 2;
pub const ABORT_REASON_NOT_SPECIFIED: u8 = 0;
pub const ABORT_REASON_UNRECOGNIZED_PDU: u8 = 1;
pub const ABORT_REASON_UNEXPECTED_PDU: u8 = 2;
pub const ABORT_REASON_RESERVED: u8 = 3;
pub const ABORT_REASON_UNRECOGNIZED_PDU_PARAMETER: u8 = 4;
pub const ABORT_REASON_UNEXPECTED_PDU_PARAMETER: u8 = 5;
pub const ABORT_REASON_INVALID_PDU_PARAMETER: u8 = 6;
pub const USER_IDENTITY_USERNAME: u8 = 1;
pub const USER_IDENTITY_USERNAME_PASSWORD: u8 = 2;
pub const USER_IDENTITY_KERBEROS: u8 = 3;
pub const USER_IDENTITY_SAML: u8 = 4;
pub const USER_IDENTITY_JWT: u8 = 5;
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pdu {
    AssociateRq(AssociationRq),
    AssociateAc(AssociationAc),
    AssociateRj(AssociationRj),
    PData(PDataTf),
    ReleaseRq,
    ReleaseRp,
    Abort(AbortRq),
    Unknown(UnknownPdu),
}
impl Pdu {
    pub fn pdu_type(&self) -> u8 {
        match self {
            Pdu::AssociateRq(_) => PDU_ASSOCIATE_RQ,
            Pdu::AssociateAc(_) => PDU_ASSOCIATE_AC,
            Pdu::AssociateRj(_) => PDU_ASSOCIATE_RJ,
            Pdu::PData(_) => PDU_DATA_TF,
            Pdu::ReleaseRq => PDU_RELEASE_RQ,
            Pdu::ReleaseRp => PDU_RELEASE_RP,
            Pdu::Abort(_) => PDU_ABORT,
            Pdu::Unknown(u) => u.pdu_type,
        }
    }
    #[doc(hidden)]
    pub fn kind_label(&self) -> &'static str {
        match self {
            Pdu::AssociateRq(_) => "A-ASSOCIATE-RQ",
            Pdu::AssociateAc(_) => "A-ASSOCIATE-AC",
            Pdu::AssociateRj(_) => "A-ASSOCIATE-RJ",
            Pdu::PData(_) => "P-DATA-TF",
            Pdu::ReleaseRq => "A-RELEASE-RQ",
            Pdu::ReleaseRp => "A-RELEASE-RP",
            Pdu::Abort(_) => "A-ABORT",
            Pdu::Unknown(_) => "unknown PDU",
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownPdu {
    pub pdu_type: u8,
    pub data: Vec<u8>,
}
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AssociationRq {
    pub protocol_version: u16,
    pub called_ae_title: String,
    pub calling_ae_title: String,
    pub application_context_name: String,
    pub presentation_contexts: Vec<PresentationContextProposed>,
    pub user_info: Option<Vec<UserItem>>,
}
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AssociationAc {
    pub protocol_version: u16,
    pub called_ae_title: String,
    pub calling_ae_title: String,
    pub application_context_name: String,
    pub presentation_contexts: Vec<PresentationContextResult>,
    pub user_info: Option<Vec<UserItem>>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AssociationRj {
    pub result: u8,
    pub source: u8,
    pub reason: u8,
}
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PDataTf {
    pub values: Vec<PDataValue>,
}
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PDataValue {
    pub presentation_context_id: u8,
    pub is_command: bool,
    pub is_last: bool,
    pub data: Vec<u8>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AbortRq {
    pub source: u8,
    pub reason: u8,
}
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PresentationContextProposed {
    pub id: u8,
    pub abstract_syntax_uid: String,
    pub transfer_syntax_uids: Vec<String>,
}
pub type PresentationContext = PresentationContextProposed;
impl PresentationContextProposed {
    pub fn new(abstract_syntax_uid: &str, transfer_syntax_uids: &[&str]) -> Self {
        PresentationContextProposed {
            id: 0,
            abstract_syntax_uid: abstract_syntax_uid.to_string(),
            transfer_syntax_uids: transfer_syntax_uids.iter().map(|s| s.to_string()).collect(),
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PresentationContextResult {
    pub id: u8,
    pub result: u8,
    pub transfer_syntax_uid: String,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UserItem {
    MaxLength(u32),
    ImplementationClassUid(String),
    ImplementationVersionName(String),
    AsynchronousOperations(AsynchronousOperationsWindow),
    ExtendedNegotiation(SopClassExtendedNegotiationItem),
    CommonExtendedNegotiation(SopClassCommonExtendedNegotiationItem),
    RoleSelection(RoleSelectionItem),
    UserIdentity(UserIdentityItem),
    UserIdentityResponse(Vec<u8>),
    Unknown(UnknownUserItem),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AsynchronousOperationsWindow {
    pub maximum_invoked: u16,
    pub maximum_performed: u16,
}
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SopClassExtendedNegotiationItem {
    pub sop_class_uid: String,
    pub data: Vec<u8>,
}
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SopClassCommonExtendedNegotiationItem {
    pub sop_class_uid: String,
    pub service_class_uid: String,
    pub related_general_sop_class_uids: Vec<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RoleSelectionItem {
    pub sop_class_uid: String,
    pub scu_role: bool,
    pub scp_role: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UserIdentityItem {
    pub identity_type: u8,
    pub positive_response_requested: bool,
    pub primary_field: Vec<u8>,
    pub secondary_field: Vec<u8>,
}
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UnknownUserItem {
    pub item_type: u8,
    pub data: Vec<u8>,
}
pub(crate) fn valid_presentation_context_id(id: u8) -> bool {
    id != 0 && id % 2 == 1
}
pub(crate) fn valid_peer_presentation_context_id(id: u8) -> bool {
    id != 0
}
pub(crate) fn valid_reject(result: u8, source: u8, reason: u8) -> bool {
    if result != ASSOCIATE_RJ_RESULT_PERMANENT && result != ASSOCIATE_RJ_RESULT_TRANSIENT {
        return false;
    }
    match source {
        ASSOCIATE_RJ_SOURCE_SERVICE_USER => (1..=10).contains(&reason),
        ASSOCIATE_RJ_SOURCE_SERVICE_PROVIDER_ACSE => reason == 1 || reason == 2,
        ASSOCIATE_RJ_SOURCE_SERVICE_PROVIDER_PRESENTATION => reason <= 7,
        _ => false,
    }
}
pub(crate) fn valid_abort(source: u8, reason: u8) -> bool {
    match source {
        ABORT_SOURCE_SERVICE_USER => true,
        ABORT_SOURCE_SERVICE_PROVIDER => reason <= ABORT_REASON_INVALID_PDU_PARAMETER,
        _ => false,
    }
}
