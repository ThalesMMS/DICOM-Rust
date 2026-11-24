#![forbid(unsafe_code)]
use crate::net::ul;
use crate::object;
use std::fmt;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    ProtocolViolation,
    CEchoStatus,
    PresentationContextMismatch,
    CommandSetTooLarge,
    OperationInProgress,
    OperationCanceled,
    OperationTimeout,
    AssociationStateUncertain,
    UnexpectedCommand,
    AssociationReleased,
    CancelGraceExceeded,
    ScpHandlerPanic,
    StoreAccessDenied,
    CEchoAssociation,
    CEchoNoAcceptedPresentationContext,
    CEchoRequest,
    CEchoRelease,
    RejectedDataSetDrainLimit,
    DataSetSpool,
    StoreInvalidOptions,
    CFindCanceled,
    CMoveCanceled,
    CGetCanceled,
    CFindIdentifierKeys,
    CGetIdentifierKeys,
    CFindInvalidStatus,
    CFindResponseCharacterSet,
    FindCancelNotAnswered,
    CGetStorageRoleNotAccepted,
    MoveDestinationUnknown,
    MoveDestinationRefused,
    NormalizedStatus,
    NormalizedPresentationContext,
    NormalizedScpStatusHandled,
    StorageCommitmentTransactionNotFound,
    StorageCommitmentTransactionConflict,
    StorageCommitmentConcurrentUpdate,
    StorageCommitmentResourceLimit,
    StorageCommitmentInvalidRequest,
    StorageCommitmentInvalidResult,
    StorageCommitmentCallbackUnknown,
    StorageCommitmentDeliveryExhausted,
    StorageCommitmentFileStoreInvalidRecord,
    StorageCommitmentFileStoreClosed,
    StorageCommitmentStatus,
    ModalityWorklistIdentifier,
    ModalityWorklistResourceLimit,
    ModalityWorklistProvider,
    ModalityWorklistStatus,
    ModalityWorklistCallback,
    ModalityWorklistUnknownAeTitle,
    StreamingCFindProvider,
    StreamingCFindResourceLimit,
    StreamingCFindIdentifier,
    StreamingCFindCallback,
    StoreCoercion,
    StoreCoercionPixels,
    StoreCoercionIdentity,
    StoreInvalidSource,
    StoreResourceLimit,
    StoreSourceChanged,
    StoreTransferSyntax,
    StoreTransferSyntaxPolicy,
    StorePresentationContextRejected,
    StoreAssociation,
    StoreRemoteFailure,
    StoreUncertain,
    StoreTimeout,
    StoreSessionClosed,
    StoreCallback,
    RetrieveSourceInstance,
    ParserResourceLimit,
    NetstoreInvalidIdentity,
    NetstoreUnsafeDirectory,
    NetstoreCollisionLimit,
    StudyActivityLimit,
    StudyActivityInput,
    ServiceQueueFull,
    ServiceQueueClosed,
    CMoveBatchContract,
    NonPatientIdentifier,
    NonPatientProvider,
    AsyncSessionClosed,
    AsyncSessionReleasing,
    AsyncWindowExceeded,
    AsyncMessageIdExhausted,
    AsyncOperationNotCancelable,
    AsyncOperationComplete,
    AsyncOperationsActive,
    AsyncTerminalResponseMissing,
    AsyncResponseQueueFull,
    AsyncResourceLimit,
    AsyncReleaseNotRequestor,
    AsyncRoleNotAccepted,
    AsyncRequestHandler,
    UnknownTransferSyntax,
    UnsupportedTransferSyntax,
    Other,
}
impl Kind {
    pub fn text(self) -> &'static str {
        match self {
            Kind::ProtocolViolation => "dicom dimse: protocol violation",
            Kind::CEchoStatus => "dicom dimse: C-ECHO status",
            Kind::PresentationContextMismatch => "dicom dimse: presentation context mismatch",
            Kind::CommandSetTooLarge => "dicom dimse: command set too large",
            Kind::OperationInProgress => "dicom dimse: operation already in progress",
            Kind::OperationCanceled => "dicom dimse: operation canceled",
            Kind::OperationTimeout => "dicom dimse: operation timeout",
            Kind::AssociationStateUncertain => "dicom dimse: association state uncertain",
            Kind::UnexpectedCommand => "dicom dimse: unexpected command",
            Kind::AssociationReleased => "dicom dimse: association released",
            Kind::CancelGraceExceeded => "dicom dimse: cancel grace exceeded",
            Kind::ScpHandlerPanic => "dicom dimse: SCP handler panic",
            Kind::StoreAccessDenied => "dicom dimse: access denied",
            Kind::CEchoAssociation => "dicom dimse: C-ECHO association",
            Kind::CEchoNoAcceptedPresentationContext => {
                "dicom dimse: C-ECHO verification presentation context not accepted"
            }
            Kind::CEchoRequest => "dicom dimse: C-ECHO request",
            Kind::CEchoRelease => "dicom dimse: C-ECHO release",
            Kind::RejectedDataSetDrainLimit => {
                "dicom dimse: rejected data set exceeds the drain limit"
            }
            Kind::DataSetSpool => "dicom dimse: data set spool",
            Kind::StoreInvalidOptions => "dicom dimse: invalid C-STORE session options",
            Kind::CFindCanceled => "dicom dimse: C-FIND canceled",
            Kind::CMoveCanceled => "dicom dimse: C-MOVE canceled",
            Kind::CGetCanceled => "dicom dimse: C-GET canceled",
            Kind::CFindIdentifierKeys => {
                "dicom dimse: C-FIND Identifier keys do not fit the Query/Retrieve level"
            }
            Kind::CGetIdentifierKeys => "dicom dimse: invalid C-GET Identifier keys",
            Kind::CFindInvalidStatus => "dicom dimse: invalid C-FIND status",
            Kind::CFindResponseCharacterSet => {
                "dicom dimse: invalid C-FIND response Specific Character Set"
            }
            Kind::FindCancelNotAnswered => "dicom dimse: no final C-FIND response after C-CANCEL",
            Kind::CGetStorageRoleNotAccepted => "dicom dimse: C-GET storage SCP role not accepted",
            Kind::MoveDestinationUnknown => "dicom dimse: Move Destination unknown",
            Kind::MoveDestinationRefused => "dicom dimse: Move Destination refused by policy",
            Kind::NormalizedStatus => "dicom dimse: normalized operation status",
            Kind::NormalizedPresentationContext => {
                "dicom dimse: incompatible normalized presentation context"
            }
            Kind::NormalizedScpStatusHandled => "dicom dimse: normalized SCP status",
            Kind::StorageCommitmentTransactionNotFound => {
                "dicom dimse: storage commitment transaction not found"
            }
            Kind::StorageCommitmentTransactionConflict => {
                "dicom dimse: storage commitment transaction conflict"
            }
            Kind::StorageCommitmentConcurrentUpdate => {
                "dicom dimse: storage commitment transaction changed concurrently"
            }
            Kind::StorageCommitmentResourceLimit => {
                "dicom dimse: storage commitment resource limit"
            }
            Kind::StorageCommitmentInvalidRequest => {
                "dicom dimse: invalid storage commitment request"
            }
            Kind::StorageCommitmentInvalidResult => {
                "dicom dimse: invalid storage commitment result"
            }
            Kind::StorageCommitmentCallbackUnknown => {
                "dicom dimse: storage commitment callback is unknown"
            }
            Kind::StorageCommitmentDeliveryExhausted => {
                "dicom dimse: storage commitment delivery exhausted"
            }
            Kind::StorageCommitmentFileStoreInvalidRecord => {
                "dicom dimse: invalid storage commitment file record"
            }
            Kind::StorageCommitmentFileStoreClosed => {
                "dicom dimse: storage commitment file store is closed"
            }
            Kind::StorageCommitmentStatus => "dicom dimse: storage commitment status",
            Kind::ModalityWorklistIdentifier => {
                "dicom dimse: MWL Identifier does not match SOP Class"
            }
            Kind::ModalityWorklistResourceLimit => "dicom dimse: MWL resource limit exceeded",
            Kind::ModalityWorklistProvider => "dicom dimse: MWL provider failed",
            Kind::ModalityWorklistStatus => "dicom dimse: MWL C-FIND returned non-success status",
            Kind::ModalityWorklistCallback => "dicom dimse: MWL match callback failed",
            Kind::ModalityWorklistUnknownAeTitle => {
                "dicom dimse: no MWL directory for the AE title"
            }
            Kind::StreamingCFindProvider => "dicom dimse: streaming C-FIND provider failed",
            Kind::StreamingCFindResourceLimit => {
                "dicom dimse: streaming C-FIND resource limit exceeded"
            }
            Kind::StreamingCFindIdentifier => "dicom dimse: streaming C-FIND Identifier invalid",
            Kind::StreamingCFindCallback => "dicom dimse: streaming C-FIND callback failed",
            Kind::StoreCoercion => "dicom dimse: attribute coercion failed",
            Kind::StoreCoercionPixels => {
                "dicom dimse: attribute coercion changed protected pixel attributes"
            }
            Kind::StoreCoercionIdentity => {
                "dicom dimse: attribute coercion has inconsistent SOP identity"
            }
            Kind::StoreInvalidSource => "dicom dimse: invalid C-STORE source",
            Kind::StoreResourceLimit => "dicom dimse: C-STORE resource limit exceeded",
            Kind::StoreSourceChanged => "dicom dimse: C-STORE source changed",
            Kind::StoreTransferSyntax => "dicom dimse: C-STORE transfer syntax is not writable",
            Kind::StoreTransferSyntaxPolicy => {
                "dicom dimse: no writable C-STORE transfer syntax in the allowlist"
            }
            Kind::StorePresentationContextRejected => {
                "dicom dimse: C-STORE presentation context was not accepted"
            }
            Kind::StoreAssociation => "dicom dimse: C-STORE association failed",
            Kind::StoreRemoteFailure => "dicom dimse: remote C-STORE failure",
            Kind::StoreUncertain => "dicom dimse: C-STORE outcome is uncertain",
            Kind::StoreTimeout => "dicom dimse: C-STORE timed out waiting for the peer",
            Kind::StoreSessionClosed => "dicom dimse: C-STORE session is closed",
            Kind::StoreCallback => "dicom dimse: C-STORE progress callback failed",
            Kind::RetrieveSourceInstance => {
                "dicom dimse: retrieve source did not provide the instance"
            }
            Kind::ParserResourceLimit => "dicom: parser resource limit",
            Kind::NetstoreInvalidIdentity => "netstore: invalid SOP identity",
            Kind::NetstoreUnsafeDirectory => "netstore: output directory changed or unsafe",
            Kind::NetstoreCollisionLimit => "netstore: unique instance limit reached",
            Kind::StudyActivityLimit => "dicom dimse: study activity limit exceeded",
            Kind::StudyActivityInput => "dicom dimse: invalid study activity metadata",
            Kind::ServiceQueueFull => "dicom dimse: service queue full",
            Kind::ServiceQueueClosed => "dicom dimse: service queue closed",
            Kind::CMoveBatchContract => "dicom dimse: invalid C-MOVE batch contract",
            Kind::NonPatientIdentifier => "dicom dimse: invalid non-patient Identifier",
            Kind::NonPatientProvider => "dicom dimse: invalid non-patient catalog result",
            Kind::AsyncSessionClosed => "dicom dimse: asynchronous session closed",
            Kind::AsyncSessionReleasing => "dicom dimse: asynchronous session is releasing",
            Kind::AsyncWindowExceeded => "dicom dimse: asynchronous operations window exceeded",
            Kind::AsyncMessageIdExhausted => "dicom dimse: asynchronous message IDs exhausted",
            Kind::AsyncOperationNotCancelable => {
                "dicom dimse: asynchronous operation is not cancelable"
            }
            Kind::AsyncOperationComplete => "dicom dimse: asynchronous operation already complete",
            Kind::AsyncOperationsActive => "dicom dimse: asynchronous operations remain active",
            Kind::AsyncTerminalResponseMissing => {
                "dicom dimse: asynchronous handler returned before terminal response"
            }
            Kind::AsyncResponseQueueFull => "dicom dimse: asynchronous response queue full",
            Kind::AsyncResourceLimit => "dicom dimse: asynchronous session resource limit exceeded",
            Kind::AsyncReleaseNotRequestor => {
                "dicom dimse: only the association requestor may initiate release"
            }
            Kind::AsyncRoleNotAccepted => "dicom dimse: local DIMSE role not accepted",
            Kind::AsyncRequestHandler => "dicom dimse: asynchronous request handler failed",
            Kind::UnknownTransferSyntax => "dicom: unknown transfer syntax",
            Kind::UnsupportedTransferSyntax => "dicom: unsupported transfer syntax",
            Kind::Other => "error",
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cause {
    Dimse(Kind),
    Ul(ul::Kind),
    Obj(object::Kind),
}
#[derive(Debug, Clone, Default)]
pub enum Detail {
    #[default]
    None,
    UnexpectedCommand(u16, u16),
    Operation(String, bool),
    MissingPresentationContext(String, Vec<ul::PresentationContextOutcome>),
    ScpStatus(&'static str, u16, String),
    Status(u16),
    NormalizedHandler(String),
    NormalizedPresentationContext(u8, String, String),
    StorageCommitmentDelivery(&'static str, u16, bool, Box<Detail>),
    NormalizedActionResponseHook(NormalizedActionResponseHook),
    Store(String, i64, bool, Box<Detail>),
}
#[derive(Clone)]
pub struct NormalizedActionResponseHook(pub NormalizedActionResponseObserver);
pub type NormalizedActionResponseObserver = std::sync::Arc<
    dyn Fn(&ul::Ctx, &super::command::NormalizedResponse, bool) -> Option<Error> + Send + Sync,
>;
impl fmt::Debug for NormalizedActionResponseHook {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("NormalizedActionResponseHook")
    }
}
impl NormalizedActionResponseHook {
    pub fn into_error(self) -> Error {
        Error::new("dicom dimse: normalized action response hook")
            .with_detail(Detail::NormalizedActionResponseHook(self))
    }
}
#[derive(Debug, Clone)]
pub struct Error {
    text: String,
    causes: Vec<Cause>,
    ul_detail: Box<ul::Detail>,
    obj: Option<Box<object::Error>>,
    detail: Box<Detail>,
    os_error: Option<i32>,
    failed_suboperations: u16,
}
pub type Result<T> = std::result::Result<T, Error>;
const OBJECT_KINDS: &[object::Kind] = &[
    object::Kind::Eof,
    object::Kind::UnexpectedEof,
    object::Kind::MaxTotalBytes,
    object::Kind::ContainerOverrun,
    object::Kind::IncompletePixelData,
    object::Kind::IncompletePixelDataOptions,
    object::Kind::PartialRead,
    object::Kind::FileMeta,
    object::Kind::DataSet,
    object::Kind::MissingTransferSyntax,
    object::Kind::UnknownTransferSyntax,
    object::Kind::UnsupportedTransferSyntax,
    object::Kind::DeflateCorrupt,
];
impl Error {
    fn bare(text: String, causes: Vec<Cause>) -> Error {
        Error {
            text,
            causes,
            ul_detail: Box::new(ul::Detail::None),
            obj: None,
            detail: Box::new(Detail::None),
            os_error: None,
            failed_suboperations: 0,
        }
    }
    pub(crate) fn bare_with(text: impl Into<String>, causes: Vec<Cause>) -> Error {
        Error::bare(text.into(), causes)
    }
    pub fn new(text: impl Into<String>) -> Error {
        Error::bare(text.into(), vec![Cause::Dimse(Kind::Other)])
    }
    pub fn sentinel(kind: Kind) -> Error {
        Error::bare(kind.text().to_string(), vec![Cause::Dimse(kind)])
    }
    pub fn wrap(kind: Kind, message: impl fmt::Display) -> Error {
        Error::bare(
            format!("{}: {message}", kind.text()),
            vec![Cause::Dimse(kind)],
        )
    }
    pub fn classified(kind: Kind, text: impl Into<String>) -> Error {
        Error::bare(text.into(), vec![Cause::Dimse(kind)])
    }
    pub fn prefixed(mut self, prefix: impl fmt::Display) -> Error {
        self.text = format!("{prefix}: {}", self.text);
        self
    }
    pub fn wrap_err(kind: Kind, err: Error) -> Error {
        let mut err = err.prefixed(kind.text());
        err.causes.insert(0, Cause::Dimse(kind));
        err
    }
    pub fn wrap_msg_err(kind: Kind, message: impl fmt::Display, err: Error) -> Error {
        let mut err = err.prefixed(format!("{}: {message}", kind.text()));
        err.causes.insert(0, Cause::Dimse(kind));
        err
    }
    pub fn with_text(text: impl Into<String>) -> Error {
        Error::new(text)
    }
    pub(crate) fn with_failed_suboperations(mut self, failed: u16) -> Error {
        self.failed_suboperations = failed;
        self
    }
    pub(crate) fn failed_suboperations(&self) -> u16 {
        self.failed_suboperations
    }
    pub(crate) fn relabel(mut self, text: impl Into<String>) -> Error {
        self.text = text.into();
        self
    }
    pub fn also(mut self, kind: Kind) -> Error {
        if !self.is(kind) {
            self.causes.push(Cause::Dimse(kind));
        }
        self
    }
    pub(crate) fn also_cause(mut self, cause: Cause) -> Error {
        if !self.causes.contains(&cause) {
            self.causes.push(cause);
        }
        self
    }
    pub fn with_ul_detail(mut self, detail: ul::Detail) -> Error {
        self.ul_detail = Box::new(detail);
        self
    }
    pub fn with_detail(mut self, detail: Detail) -> Error {
        self.detail = Box::new(detail);
        self
    }
    pub fn join(errors: impl IntoIterator<Item = Option<Error>>) -> Option<Error> {
        let mut out: Option<Error> = None;
        for err in errors.into_iter().flatten() {
            out = Some(match out {
                None => err,
                Some(mut joined) => {
                    joined.text.push('\n');
                    joined.text.push_str(&err.text);
                    for c in err.causes {
                        if !joined.causes.contains(&c) {
                            joined.causes.push(c);
                        }
                    }
                    if matches!(*joined.ul_detail, ul::Detail::None) {
                        joined.ul_detail = err.ul_detail;
                    }
                    if joined.obj.is_none() {
                        joined.obj = err.obj;
                    }
                    if matches!(*joined.detail, Detail::None) {
                        joined.detail = err.detail;
                    }
                    if joined.os_error.is_none() {
                        joined.os_error = err.os_error;
                    }
                    joined
                }
            });
        }
        out
    }
    pub fn is(&self, kind: Kind) -> bool {
        self.causes.contains(&Cause::Dimse(kind))
    }
    pub fn is_ul(&self, kind: ul::Kind) -> bool {
        self.causes.contains(&Cause::Ul(kind))
            || match kind {
                ul::Kind::Eof => self.causes.contains(&Cause::Obj(object::Kind::Eof)),
                ul::Kind::UnexpectedEof => self
                    .causes
                    .contains(&Cause::Obj(object::Kind::UnexpectedEof)),
                _ => false,
            }
    }
    pub fn is_obj(&self, kind: object::Kind) -> bool {
        self.causes.contains(&Cause::Obj(kind))
    }
    pub fn is_canceled(&self) -> bool {
        self.is_ul(ul::Kind::Canceled)
    }
    pub fn is_deadline(&self) -> bool {
        self.is_ul(ul::Kind::DeadlineExceeded)
    }
    pub fn causes(&self) -> &[Cause] {
        &self.causes
    }
    pub fn detail(&self) -> &Detail {
        &self.detail
    }
    pub fn ul_detail(&self) -> &ul::Detail {
        &self.ul_detail
    }
    #[doc(hidden)]
    pub fn object_error(&self) -> Option<&object::Error> {
        self.obj.as_deref()
    }
    pub fn parse_error(&self) -> Option<object::error::ParseInfo> {
        self.obj.as_deref().and_then(|e| e.parse_info())
    }
    pub fn wraps_parse_error(&self) -> bool {
        self.parse_error().is_some()
    }
    pub fn os_error(&self) -> Option<i32> {
        self.os_error
    }
    pub fn with_os_error(mut self, code: Option<i32>) -> Error {
        self.os_error = code;
        self
    }
    pub fn text(&self) -> &str {
        &self.text
    }
    pub fn to_ul(&self) -> ul::Error {
        let kinds: Vec<ul::Kind> = self
            .causes
            .iter()
            .filter_map(|c| match c {
                Cause::Ul(k) => Some(*k),
                Cause::Obj(object::Kind::Eof) => Some(ul::Kind::Eof),
                Cause::Obj(object::Kind::UnexpectedEof) => Some(ul::Kind::UnexpectedEof),
                _ => None,
            })
            .collect();
        let kinds = if kinds.is_empty() {
            vec![ul::Kind::Other]
        } else {
            kinds
        };
        ul::Error::with_kinds(self.text.clone(), kinds).with_os_error(self.os_error)
    }
}
impl From<ul::Error> for Error {
    fn from(err: ul::Error) -> Error {
        Error {
            text: err.text().to_string(),
            causes: err.kinds().iter().map(|k| Cause::Ul(*k)).collect(),
            ul_detail: Box::new(err.detail().clone()),
            obj: None,
            detail: Box::new(Detail::None),
            os_error: err.os_error(),
            failed_suboperations: 0,
        }
    }
}
impl From<object::Error> for Error {
    fn from(err: object::Error) -> Error {
        let mut causes: Vec<Cause> = OBJECT_KINDS
            .iter()
            .filter(|k| err.is(**k))
            .map(|k| Cause::Obj(*k))
            .collect();
        if causes.is_empty() {
            causes.push(Cause::Obj(object::Kind::Other));
        }
        Error {
            text: err.message().to_string(),
            causes,
            ul_detail: Box::new(ul::Detail::None),
            obj: Some(Box::new(err)),
            detail: Box::new(Detail::None),
            os_error: None,
            failed_suboperations: 0,
        }
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}
impl std::error::Error for Error {}
