pub mod events;
pub mod http;
pub mod media;
pub mod part10;
pub mod part10_metadata;
pub mod qido;
pub mod searchmatch;
pub mod spool;
pub mod stow;
pub mod transcode;
pub mod ups;
pub mod wado;
pub use self::events::{
    WorkitemEvent, WorkitemEventHub, WorkitemEventHubOptions, decode_workitem_event,
    encode_workitem_event, same_origin_or_absent,
};
use self::http::{Handler, HttpConfig, Request, ResponseWriter, ServeHandle, http_error};
pub use self::part10::{format_bulk_data_path, parse_bulk_data_path};
pub use self::part10_metadata::{
    DEFAULT_PART10_BULK_DATA_THRESHOLD, Part10MetadataOptions, part10_bulk_data, part10_metadata,
};
pub use self::qido::{
    MetadataBackend, MetadataLevel, MetadataRequest, SearchBackend, SearchFilter, SearchLevel,
    SearchRequest, SearchResult,
};
pub use self::searchmatch::{SearchMatcher, SearchMatcherOptions};
pub use self::transcode::{PreparedSource, RetrieveTranscodePolicy, SourceOpen, SourceReader};
use super::mimeheader::Header;
use super::urlrules::{self, Url};
use crate::dicomjson::Dataset;
use crate::net::ul::{CancelToken, Ctx, Kind, Nanos};
use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant, SystemTime};
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ServerError {
    InvalidServerOptions(String),
    InvalidRequest,
    Unauthorized,
    Forbidden,
    NotFound,
    Conflict,
    Unsupported,
    ResourceLimit,
    Backend,
    ReadLeaseDenied,
    Canceled,
    DeadlineExceeded,
    MaxBytes(i64),
    Status {
        status: u16,
        code: String,
        err: Box<ServerError>,
    },
    Other(String),
}
impl ServerError {
    pub fn status(status: u16, code: &str, err: ServerError) -> ServerError {
        ServerError::Status {
            status,
            code: code.to_string(),
            err: Box::new(err),
        }
    }
    pub fn is(&self, target: &ServerError) -> bool {
        if self == target {
            return true;
        }
        match self {
            ServerError::Status { err, .. } => err.is(target),
            ServerError::ReadLeaseDenied => *target == ServerError::Forbidden,
            ServerError::InvalidServerOptions(_) => {
                matches!(target, ServerError::InvalidServerOptions(t) if t.is_empty())
            }
            _ => false,
        }
    }
    fn ctx(kind: Kind) -> ServerError {
        match kind {
            Kind::Canceled => ServerError::Canceled,
            _ => ServerError::DeadlineExceeded,
        }
    }
}
impl fmt::Display for ServerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ServerError::InvalidServerOptions(detail) if detail.is_empty() => {
                f.write_str("dicomweb: invalid server options")
            }
            ServerError::InvalidServerOptions(detail) => {
                write!(f, "dicomweb: invalid server options: {detail}")
            }
            ServerError::InvalidRequest => f.write_str("dicomweb: invalid request"),
            ServerError::Unauthorized => f.write_str("dicomweb: unauthorized"),
            ServerError::Forbidden => f.write_str("dicomweb: forbidden"),
            ServerError::NotFound => f.write_str("dicomweb: resource not found"),
            ServerError::Conflict => f.write_str("dicomweb: conflict"),
            ServerError::Unsupported => f.write_str("dicomweb: unsupported operation"),
            ServerError::ResourceLimit => f.write_str("dicomweb: resource limit exceeded"),
            ServerError::Backend => f.write_str("dicomweb: backend failure"),
            ServerError::ReadLeaseDenied => f.write_str("dicomweb: forbidden: read lease denied"),
            ServerError::Canceled => f.write_str("context canceled"),
            ServerError::DeadlineExceeded => f.write_str("context deadline exceeded"),
            ServerError::MaxBytes(_) => f.write_str("http: request body too large"),
            ServerError::Status { .. } => f.write_str("DICOMweb request failed"),
            ServerError::Other(text) => f.write_str(text),
        }
    }
}
impl std::error::Error for ServerError {}
const STATUS_CLIENT_CLOSED_REQUEST: u16 = 499;
pub(crate) fn status_for_server_error(err: &ServerError) -> u16 {
    if let ServerError::Status { status, .. } = err {
        return *status;
    }
    if matches!(err, ServerError::MaxBytes(_)) {
        return 413;
    }
    let is = |t: ServerError| err.is(&t);
    if is(ServerError::Canceled) {
        STATUS_CLIENT_CLOSED_REQUEST
    } else if is(ServerError::DeadlineExceeded) {
        504
    } else if is(ServerError::Unauthorized) {
        401
    } else if is(ServerError::Forbidden) {
        403
    } else if is(ServerError::NotFound) {
        404
    } else if is(ServerError::Conflict) {
        409
    } else if is(ServerError::ResourceLimit) {
        413
    } else if is(ServerError::InvalidRequest) {
        400
    } else if is(ServerError::Unsupported) {
        406
    } else {
        500
    }
}
pub(crate) fn error_code_for_server(err: &ServerError) -> String {
    if let ServerError::Status { code, .. } = err
        && !code.is_empty()
    {
        return code.clone();
    }
    let is = |t: ServerError| err.is(&t);
    if is(ServerError::ReadLeaseDenied) {
        "read_lease_denied"
    } else if is(ServerError::Canceled) {
        "canceled"
    } else if is(ServerError::DeadlineExceeded) {
        "deadline"
    } else if is(ServerError::ResourceLimit) {
        "resource_limit"
    } else if is(ServerError::InvalidRequest) {
        "invalid_request"
    } else if is(ServerError::Unsupported) {
        "unsupported"
    } else if is(ServerError::NotFound) {
        "not_found"
    } else {
        "backend_failure"
    }
    .to_string()
}
pub(crate) fn classify_backend_error(err: ServerError) -> ServerError {
    if err.is(&ServerError::ReadLeaseDenied) {
        return ServerError::ReadLeaseDenied;
    }
    for t in [
        ServerError::Canceled,
        ServerError::DeadlineExceeded,
        ServerError::NotFound,
        ServerError::Conflict,
        ServerError::Unsupported,
        ServerError::ResourceLimit,
        ServerError::InvalidRequest,
    ] {
        if err.is(&t) {
            return err;
        }
    }
    ServerError::Backend
}
pub(crate) fn classify_multipart_error(err: ServerError) -> ServerError {
    if matches!(err, ServerError::MaxBytes(_)) {
        return ServerError::status(413, "request_too_large", ServerError::ResourceLimit);
    }
    if err.is(&ServerError::ResourceLimit) {
        return ServerError::status(413, "request_too_large", err);
    }
    if err.is(&ServerError::Canceled) || err.is(&ServerError::DeadlineExceeded) {
        return err;
    }
    ServerError::InvalidRequest
}
fn context_error_code(err: &ServerError) -> &'static str {
    if err.is(&ServerError::DeadlineExceeded) {
        "deadline"
    } else {
        "canceled"
    }
}
fn authorization_error_code(err: &ServerError) -> &'static str {
    if err.is(&ServerError::Unauthorized) {
        "unauthorized"
    } else {
        "forbidden"
    }
}
pub(crate) fn write_server_error(w: &mut dyn ResponseWriter, err: &ServerError) {
    let status = status_for_server_error(err);
    let message = if status == STATUS_CLIENT_CLOSED_REQUEST {
        "request canceled"
    } else {
        "DICOMweb request failed"
    };
    if status == 503 && w.header().get("Retry-After").is_empty() {
        w.header().set("Retry-After", "1");
    }
    http_error(w, message, status);
}
pub fn ctx_err(ctx: &Ctx) -> Result<(), ServerError> {
    match ctx.err() {
        Some(kind) => Err(ServerError::ctx(kind)),
        None => Ok(()),
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Operation {
    SearchStudies,
    SearchSeries,
    SearchInstances,
    RetrieveStudy,
    RetrieveSeries,
    RetrieveInstance,
    RetrieveMetadata,
    RetrieveFrames,
    RetrieveRendered,
    RetrieveThumbnail,
    RetrieveBulkData,
    RetrieveWadoUri,
    StoreInstances,
    CreateWorkitem,
    RetrieveWorkitem,
    UpdateWorkitem,
    ChangeWorkitemState,
    RequestWorkitemCancellation,
    SearchWorkitems,
    SubscribeWorkitem,
    UnsubscribeWorkitem,
    SuspendWorkitemSubscription,
    OpenEventChannel,
    Unknown,
}
impl Operation {
    pub fn as_str(self) -> &'static str {
        match self {
            Operation::SearchStudies => "search_studies",
            Operation::SearchSeries => "search_series",
            Operation::SearchInstances => "search_instances",
            Operation::RetrieveStudy => "retrieve_study",
            Operation::RetrieveSeries => "retrieve_series",
            Operation::RetrieveInstance => "retrieve_instance",
            Operation::RetrieveMetadata => "retrieve_metadata",
            Operation::RetrieveFrames => "retrieve_frames",
            Operation::RetrieveRendered => "retrieve_rendered",
            Operation::RetrieveThumbnail => "retrieve_thumbnail",
            Operation::RetrieveBulkData => "retrieve_bulk_data",
            Operation::RetrieveWadoUri => "retrieve_wado_uri",
            Operation::StoreInstances => "store_instances",
            Operation::CreateWorkitem => "create_workitem",
            Operation::RetrieveWorkitem => "retrieve_workitem",
            Operation::UpdateWorkitem => "update_workitem",
            Operation::ChangeWorkitemState => "change_workitem_state",
            Operation::RequestWorkitemCancellation => "request_workitem_cancellation",
            Operation::SearchWorkitems => "search_workitems",
            Operation::SubscribeWorkitem => "subscribe_workitem",
            Operation::UnsubscribeWorkitem => "unsubscribe_workitem",
            Operation::SuspendWorkitemSubscription => "suspend_workitem_subscription",
            Operation::OpenEventChannel => "open_event_channel",
            Operation::Unknown => "unknown",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuditEvent {
    pub operation: Operation,
    pub request_id: String,
    pub status_code: u16,
    pub item_count: usize,
    pub duration: Duration,
    pub error_code: String,
}
pub type Authorizer =
    Arc<dyn Fn(&Ctx, &Request, Operation) -> Result<(), ServerError> + Send + Sync>;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ServerLimits {
    pub max_request_bytes: i64,
    pub max_part_bytes: i64,
    pub max_metadata_bytes: i64,
    pub max_decoded_part_bytes: i64,
    pub max_response_bytes: i64,
    pub max_request_uri_bytes: i64,
    pub max_header_bytes: i64,
    pub max_part_header_bytes: i64,
    pub max_query_value_bytes: i64,
    pub max_uid_bytes: i64,
    pub max_json_value_bytes: i64,
    pub max_json_values: i64,
    pub max_json_depth: i64,
    pub max_parts: i64,
    pub max_query_fields: i64,
    pub max_results: i64,
    pub max_offset: i64,
    pub max_frames: i64,
    pub max_rendered_pixels: i64,
    pub max_concurrent_requests: i64,
    pub max_duration: Nanos,
    pub max_queued_requests: i64,
    pub max_queue_wait: Nanos,
}
pub fn default_server_limits() -> ServerLimits {
    ServerLimits {
        max_request_bytes: 2 << 30,
        max_part_bytes: 1 << 30,
        max_metadata_bytes: 64 << 20,
        max_decoded_part_bytes: 2 << 30,
        max_response_bytes: 64 << 20,
        max_request_uri_bytes: 16 << 10,
        max_header_bytes: 1 << 20,
        max_part_header_bytes: 64 << 10,
        max_query_value_bytes: 8 << 10,
        max_uid_bytes: 64,
        max_json_value_bytes: 64 << 20,
        max_json_values: 1_000_000,
        max_json_depth: 64,
        max_parts: 10_000,
        max_query_fields: 256,
        max_results: 100_000,
        max_offset: 10_000_000,
        max_frames: 10_000,
        max_rendered_pixels: 64 << 20,
        max_concurrent_requests: 64,
        max_duration: Nanos::from_secs(300),
        max_queued_requests: 0,
        max_queue_wait: Nanos::ZERO,
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HttpServerOptions {
    pub read_header_timeout: Nanos,
    pub read_timeout: Nanos,
    pub write_timeout: Nanos,
    pub idle_timeout: Nanos,
    pub max_header_bytes: i64,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StorePolicy(pub String);
impl StorePolicy {
    pub(crate) fn reject_duplicate() -> StorePolicy {
        StorePolicy("reject_duplicate".into())
    }
    pub fn idempotent() -> StorePolicy {
        StorePolicy("idempotent".into())
    }
}
pub trait Backend: Send + Sync {
    fn search_backend(&self) -> Option<&dyn SearchBackend> {
        None
    }
    fn metadata_backend(&self) -> Option<&dyn MetadataBackend> {
        None
    }
    fn retrieve_backend(&self) -> Option<&dyn wado::RetrieveBackend> {
        None
    }
    fn frame_backend(&self) -> Option<&dyn wado::FrameBackend> {
        None
    }
    fn bulk_data_backend(&self) -> Option<&dyn wado::BulkDataBackend> {
        None
    }
    fn store_backend(&self) -> Option<&dyn stow::StoreBackend> {
        None
    }
    fn workitem_backend(&self) -> Option<&dyn ups::WorkitemBackend> {
        None
    }
}
pub type Middleware = Arc<dyn Fn(Handler) -> Option<Handler> + Send + Sync>;
pub(crate) type RequestIdFn = Arc<dyn Fn(&Request) -> String + Send + Sync>;
pub type AuditFn = Arc<dyn Fn(&Ctx, &AuditEvent) + Send + Sync>;
#[derive(Clone, Default)]
pub struct ServerOptions {
    pub backend: Option<Arc<dyn Backend>>,
    pub workitems: Option<Arc<dyn ups::WorkitemBackend>>,
    pub renderer: Option<Arc<dyn wado::Renderer>>,
    pub enable_wado_uri: bool,
    pub enable_bulk_data_ranges: bool,
    pub limits: ServerLimits,
    pub service_root: String,
    pub authorize: Option<Authorizer>,
    pub allow_unauthenticated: bool,
    pub authorize_read_lease: Option<ReadLeaseAuthorizer>,
    pub middleware: Option<Middleware>,
    pub request_id: Option<RequestIdFn>,
    pub audit: Option<AuditFn>,
    pub store_policy: StorePolicy,
    pub spool_directory: String,
    pub spool_retention_age: Nanos,
    pub spool_aggregate_quota_bytes: i64,
    pub http: HttpServerOptions,
    pub workitem_events: Option<Arc<events::WorkitemEventHub>>,
    pub retrieve_transcode: Option<transcode::RetrieveTranscodePolicy>,
}
const DEFAULT_SPOOL_RETENTION_AGE: Nanos = Nanos(24 * 3600 * 1_000_000_000);
const DEFAULT_SPOOL_AGGREGATE_QUOTA_BYTES: i64 = 4 << 30;
pub(crate) fn normalize_server_limits(limits: ServerLimits) -> Result<ServerLimits, ServerError> {
    let d = default_server_limits();
    let mut l = limits;
    let ints = [
        l.max_request_bytes,
        l.max_part_bytes,
        l.max_metadata_bytes,
        l.max_decoded_part_bytes,
        l.max_response_bytes,
        l.max_request_uri_bytes,
        l.max_header_bytes,
        l.max_part_header_bytes,
        l.max_query_value_bytes,
        l.max_uid_bytes,
        l.max_json_value_bytes,
        l.max_json_values,
        l.max_json_depth,
        l.max_parts,
        l.max_query_fields,
        l.max_results,
        l.max_offset,
        l.max_frames,
        l.max_rendered_pixels,
        l.max_concurrent_requests,
        l.max_duration.0,
        l.max_queued_requests,
        l.max_queue_wait.0,
    ];
    if ints.iter().any(|v| *v < 0) {
        return Err(ServerError::InvalidServerOptions(String::new()));
    }
    macro_rules! dflt {
        ($($f:ident),*) => { $( if l.$f == 0 { l.$f = d.$f; } )* };
    }
    dflt!(
        max_request_bytes,
        max_part_bytes,
        max_metadata_bytes,
        max_decoded_part_bytes,
        max_response_bytes,
        max_request_uri_bytes,
        max_header_bytes,
        max_part_header_bytes,
        max_query_value_bytes,
        max_uid_bytes,
        max_json_value_bytes,
        max_json_values,
        max_json_depth,
        max_parts,
        max_query_fields,
        max_results,
        max_offset,
        max_frames,
        max_rendered_pixels,
        max_concurrent_requests
    );
    if l.max_duration.is_zero() {
        l.max_duration = d.max_duration;
    }
    if l.max_queued_requests == 0 || l.max_queue_wait.is_zero() {
        l.max_queued_requests = 0;
        l.max_queue_wait = Nanos::ZERO;
    }
    l.max_queue_wait = l.max_queue_wait.min(l.max_duration);
    if l.max_part_bytes > l.max_request_bytes {
        l.max_part_bytes = l.max_request_bytes;
    }
    if l.max_metadata_bytes > l.max_part_bytes {
        l.max_metadata_bytes = l.max_part_bytes;
    }
    Ok(l)
}
fn normalize_store_policy(policy: &StorePolicy) -> Result<StorePolicy, ServerError> {
    if policy.0.trim().is_empty() {
        return Ok(StorePolicy::reject_duplicate());
    }
    match policy.0.as_str() {
        "reject_duplicate" | "idempotent" => Ok(policy.clone()),
        _ => Err(ServerError::InvalidServerOptions(String::new())),
    }
}
fn validate_service_root(root: &str) -> Result<(), ServerError> {
    if root.is_empty() {
        return Ok(());
    }
    let invalid = || {
        Err(ServerError::InvalidServerOptions(
            "invalid service root".into(),
        ))
    };
    if !root.starts_with('/')
        || root == "/"
        || root.ends_with('/')
        || root.contains("//")
        || root.contains(['?', '#', '%'])
    {
        return invalid();
    }
    for segment in root[1..].split('/') {
        if segment.is_empty()
            || segment == "."
            || segment == ".."
            || urlrules::path_escape(segment) != segment
        {
            return invalid();
        }
    }
    Ok(())
}
#[derive(Clone, Default)]
pub struct ReadLease {
    pub expires_at: Option<SystemTime>,
    pub validate: Option<LeaseValidateFn>,
}
pub type LeaseValidateFn = Arc<dyn Fn(&Ctx) -> Result<(), ServerError> + Send + Sync>;
pub type ReadLeaseAuthorizer =
    Arc<dyn Fn(&Ctx, &Request, Operation) -> Result<ReadLease, ServerError> + Send + Sync>;
pub(crate) struct RequestReadLease {
    lease: ReadLease,
    cancel: CancelToken,
    denied: AtomicBool,
    deadline_is_lease: bool,
}
#[derive(Clone)]
struct LeaseKey(Arc<RequestReadLease>);
fn is_studies_service_read(op: Operation) -> bool {
    matches!(
        op,
        Operation::SearchStudies
            | Operation::SearchSeries
            | Operation::SearchInstances
            | Operation::RetrieveStudy
            | Operation::RetrieveSeries
            | Operation::RetrieveInstance
            | Operation::RetrieveMetadata
            | Operation::RetrieveFrames
            | Operation::RetrieveRendered
            | Operation::RetrieveThumbnail
            | Operation::RetrieveBulkData
            | Operation::RetrieveWadoUri
    )
}
fn acquire_read_lease(
    acquire: &ReadLeaseAuthorizer,
    r: &Request,
    op: Operation,
) -> Result<ReadLease, ServerError> {
    match catch_unwind(AssertUnwindSafe(|| acquire(r.context(), r, op))) {
        Ok(Ok(lease)) if lease.expires_at.is_some() => Ok(lease),
        _ => Err(ServerError::ReadLeaseDenied),
    }
}
pub fn check_read_lease(ctx: &Ctx) -> Result<(), ServerError> {
    match ctx.value::<LeaseKey>() {
        Some(lease) => lease.0.check(ctx),
        None => ctx_err(ctx),
    }
}
impl RequestReadLease {
    fn check(&self, ctx: &Ctx) -> Result<(), ServerError> {
        self.context_error(ctx)?;
        if !self.valid_at(SystemTime::now()) {
            return Err(self.deny());
        }
        let valid = match &self.lease.validate {
            None => true,
            Some(validate) => {
                matches!(catch_unwind(AssertUnwindSafe(|| validate(ctx))), Ok(Ok(())))
            }
        };
        self.context_error(ctx)?;
        if !valid || !self.valid_at(SystemTime::now()) {
            return Err(self.deny());
        }
        Ok(())
    }
    fn deny(&self) -> ServerError {
        self.denied.store(true, Ordering::SeqCst);
        self.cancel.cancel();
        ServerError::ReadLeaseDenied
    }
    fn context_error(&self, ctx: &Ctx) -> Result<(), ServerError> {
        if self.denied.load(Ordering::SeqCst) {
            return Err(ServerError::ReadLeaseDenied);
        }
        match ctx.err() {
            Some(Kind::DeadlineExceeded) if self.deadline_is_lease => {
                Err(ServerError::ReadLeaseDenied)
            }
            Some(kind) => Err(ServerError::ctx(kind)),
            None => Ok(()),
        }
    }
    fn valid_at(&self, now: SystemTime) -> bool {
        self.lease.expires_at.is_some_and(|e| now < e)
    }
}
fn lease_check(lease: Option<&RequestReadLease>, ctx: &Ctx) -> Result<(), ServerError> {
    match lease {
        Some(l) => l.check(ctx),
        None => ctx_err(ctx),
    }
}
fn request_lease(ctx: &Ctx) -> Option<Arc<RequestReadLease>> {
    ctx.value::<LeaseKey>().map(|k| k.0.clone())
}
struct Slots {
    used: Mutex<i64>,
    max: i64,
    freed: Condvar,
}
impl Slots {
    fn new(max: i64) -> Slots {
        Slots {
            used: Mutex::new(0),
            max,
            freed: Condvar::new(),
        }
    }
    fn try_acquire(&self) -> bool {
        let Ok(mut used) = self.used.lock() else {
            return false;
        };
        if *used < self.max {
            *used += 1;
            true
        } else {
            false
        }
    }
    fn acquire_until(&self, deadline: Instant, ctx: &Ctx) -> Result<bool, ServerError> {
        let Ok(mut used) = self.used.lock() else {
            return Ok(false);
        };
        loop {
            if *used < self.max {
                *used += 1;
                return Ok(true);
            }
            ctx_err(ctx)?;
            let now = Instant::now();
            if now >= deadline {
                return Ok(false);
            }
            let wait = (deadline - now).min(Duration::from_millis(10));
            used = match self.freed.wait_timeout(used, wait) {
                Ok((g, _)) => g,
                Err(_) => return Ok(false),
            };
        }
    }
    fn release(&self) {
        if let Ok(mut used) = self.used.lock() {
            *used -= 1;
            self.freed.notify_one();
        }
    }
}
pub(crate) struct Inner {
    pub(crate) options: ServerOptions,
    pub(crate) limits: ServerLimits,
    handler: Mutex<Option<Handler>>,
    sem: Slots,
    queue: Slots,
    running: Mutex<Option<Arc<ServeHandle>>>,
    pub(crate) retrieve: Option<Arc<transcode::PolicyState>>,
}
#[derive(Clone)]
pub struct Server {
    inner: Arc<Inner>,
}
impl Server {
    pub fn new(mut options: ServerOptions) -> Result<Server, ServerError> {
        if options.backend.is_none() && options.workitems.is_none() {
            return Err(ServerError::InvalidServerOptions(
                "backend is required".into(),
            ));
        }
        let limits = normalize_server_limits(options.limits)?;
        options.store_policy = normalize_store_policy(&options.store_policy)?;
        let retrieve_policy = transcode::normalize_policy(
            options.retrieve_transcode.as_ref(),
            &limits,
            options.enable_wado_uri,
        )?;
        if options.retrieve_transcode.is_some() {
            options.retrieve_transcode = retrieve_policy.clone();
        }
        if options.spool_retention_age.is_negative() || options.spool_aggregate_quota_bytes < 0 {
            return Err(ServerError::InvalidServerOptions(
                "invalid spool retention policy".into(),
            ));
        }
        if options.spool_retention_age.is_zero() {
            options.spool_retention_age = DEFAULT_SPOOL_RETENTION_AGE;
        }
        if options.spool_aggregate_quota_bytes == 0 {
            options.spool_aggregate_quota_bytes = DEFAULT_SPOOL_AGGREGATE_QUOTA_BYTES;
        }
        validate_service_root(&options.service_root)?;
        spool::scavenge(&options, &limits, SystemTime::now())?;
        let middleware = options.middleware.clone();
        let inner = Arc::new(Inner {
            sem: Slots::new(limits.max_concurrent_requests),
            queue: Slots::new(limits.max_queued_requests),
            retrieve: retrieve_policy.map(|p| Arc::new(transcode::PolicyState::new(p, &limits))),
            options,
            limits,
            handler: Mutex::new(None),
            running: Mutex::new(None),
        });
        let weak = Arc::downgrade(&inner);
        let mut handler: Handler = Arc::new(move |w: &mut dyn ResponseWriter, r: &mut Request| {
            if let Some(inner) = weak.upgrade() {
                inner.serve_core(w, r);
            }
        });
        if let Some(middleware) = middleware {
            match middleware(handler) {
                Some(h) => handler = h,
                None => {
                    return Err(ServerError::InvalidServerOptions(
                        "middleware returned nil handler".into(),
                    ));
                }
            }
        }
        if let Ok(mut slot) = inner.handler.lock() {
            *slot = Some(handler);
        }
        Ok(Server { inner })
    }
    pub fn handler(&self) -> Handler {
        let server = self.clone();
        Arc::new(move |w: &mut dyn ResponseWriter, r: &mut Request| server.serve_http(w, r))
    }
    pub fn serve_http(&self, w: &mut dyn ResponseWriter, r: &mut Request) {
        let handler = self.inner.handler.lock().ok().and_then(|h| h.clone());
        match handler {
            Some(h) => h(w, r),
            None => http_error(w, "DICOMweb service unavailable", 503),
        }
    }
    pub fn serve(&self, listener: std::net::TcpListener) -> Result<(), ServerError> {
        self.serve_secured(listener, |listener, handler, config, handle| {
            http::serve(listener, handler, config, handle)
        })
    }
    #[cfg(feature = "tls")]
    #[doc(hidden)]
    pub fn serve_tls(
        &self,
        listener: std::net::TcpListener,
        tls: Arc<rustls::ServerConfig>,
    ) -> Result<(), ServerError> {
        self.serve_secured(listener, move |listener, handler, config, handle| {
            http::serve_tls(listener, tls, handler, config, handle)
        })
    }
    #[cfg(feature = "tls")]
    pub fn serve_tls_profile(
        &self,
        listener: std::net::TcpListener,
        tls: &crate::net::ul::TlsServerConfig,
    ) -> Result<(), ServerError> {
        let tls = tls.clone();
        self.serve_secured(listener, move |listener, handler, config, handle| {
            http::serve_tls_profile(listener, &tls, handler, config, handle)
        })
    }
    fn serve_secured(
        &self,
        listener: std::net::TcpListener,
        run: impl FnOnce(
            std::net::TcpListener,
            Handler,
            HttpConfig,
            &ServeHandle,
        ) -> std::io::Result<()>,
    ) -> Result<(), ServerError> {
        let o = self.inner.options.http;
        let positive = |d: Nanos, dflt: Nanos| {
            if d.0 <= 0 { dflt } else { d }
        };
        let to_std = |d: Nanos| d.positive().unwrap_or(Duration::ZERO);
        let config = HttpConfig {
            read_header_timeout: to_std(positive(o.read_header_timeout, Nanos::from_secs(10))),
            read_timeout: to_std(positive(o.read_timeout, self.inner.limits.max_duration)),
            write_timeout: to_std(positive(o.write_timeout, self.inner.limits.max_duration)),
            idle_timeout: to_std(positive(o.idle_timeout, Nanos::from_secs(60))),
            max_header_bytes: if o.max_header_bytes <= 0 {
                self.inner.limits.max_header_bytes as usize
            } else {
                o.max_header_bytes as usize
            },
        };
        let handle = Arc::new(ServeHandle::new());
        {
            let Ok(mut running) = self.inner.running.lock() else {
                return Err(ServerError::Backend);
            };
            if running.is_some() {
                return Err(ServerError::InvalidServerOptions(
                    "server is already running".into(),
                ));
            }
            *running = Some(handle.clone());
        }
        let result = run(listener, self.handler(), config, &handle);
        if result.is_err() {
            handle.close_connections();
        }
        if let Ok(mut running) = self.inner.running.lock()
            && running.as_ref().is_some_and(|h| Arc::ptr_eq(h, &handle))
        {
            *running = None;
        }
        result.map_err(|e| ServerError::Other(e.to_string()))
    }
    pub fn shutdown(&self, timeout: Option<Duration>) -> Result<(), ServerError> {
        if let Some(hub) = &self.inner.options.workitem_events {
            hub.close();
        }
        let handle = self.inner.running.lock().ok().and_then(|r| r.clone());
        let Some(handle) = handle else {
            return Ok(());
        };
        handle.stop();
        let deadline = timeout.map(|t| Instant::now() + t);
        while handle.active() > 0 {
            if deadline.is_some_and(|d| Instant::now() >= d) {
                handle.close_connections();
                return Err(ServerError::DeadlineExceeded);
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        Ok(())
    }
}
pub(crate) struct ServerWriter<'a> {
    pub(crate) inner: &'a mut dyn ResponseWriter,
    pub(crate) status: u16,
    pub(crate) written: bool,
    pub(crate) remaining: i64,
    #[allow(dead_code)]
    pub(crate) limit: i64,
    pub(crate) read_ctx: Option<Ctx>,
}
impl ServerWriter<'_> {
    fn clear_protected_read_headers(&mut self) {
        if self.read_ctx.is_none() {
            return;
        }
        self.read_ctx = None;
        for h in [
            "Accept-Ranges",
            "Content-Length",
            "Content-Location",
            "Content-Range",
            "Content-Type",
            "Warning",
        ] {
            self.inner.header().del(h);
        }
    }
}
impl ResponseWriter for ServerWriter<'_> {
    fn header(&mut self) -> &mut Header {
        self.inner.header()
    }
    fn write_header(&mut self, status: u16) {
        if self.written {
            return;
        }
        self.status = status;
        self.written = true;
        self.inner.write_header(status);
    }
    fn write(&mut self, data: &[u8]) -> Result<usize, ServerError> {
        if let Some(ctx) = &self.read_ctx {
            check_read_lease(ctx)?;
        }
        if data.len() as i64 > self.remaining {
            return Err(ServerError::ResourceLimit);
        }
        if !self.written {
            self.write_header(200);
        }
        let n = self.inner.write(data)?;
        self.remaining -= n as i64;
        Ok(n)
    }
    fn flush(&mut self) {
        self.inner.flush();
    }
    fn abort(&mut self) {
        self.inner.abort();
    }
    fn hijack(&mut self) -> Option<http::Hijacked> {
        self.inner.hijack()
    }
}
fn safe_request_id(callback: &RequestIdFn, r: &Request) -> String {
    let Ok(value) = catch_unwind(AssertUnwindSafe(|| callback(r))) else {
        return String::new();
    };
    let value = value.trim_matches(char::is_whitespace).as_bytes();
    let value = &value[..value.len().min(64)];
    let end = value
        .iter()
        .position(|&c| !(c.is_ascii_alphanumeric() || c == b'-' || c == b'_' || c == b'.'))
        .unwrap_or(value.len());
    String::from_utf8_lossy(&value[..end]).into_owned()
}
fn safe_authorize(
    callback: &Authorizer,
    ctx: &Ctx,
    r: &Request,
    op: Operation,
) -> Result<(), ServerError> {
    match catch_unwind(AssertUnwindSafe(|| callback(ctx, r, op))) {
        Err(_) => Err(ServerError::Forbidden),
        Ok(Ok(())) => Ok(()),
        Ok(Err(e)) if e.is(&ServerError::Unauthorized) || e.is(&ServerError::Forbidden) => Err(e),
        Ok(Err(_)) => Err(ServerError::Forbidden),
    }
}
struct CoreState {
    item_count: usize,
    error_code: String,
    abort_read: bool,
}
impl Inner {
    pub(crate) fn service_path(&self, resource: &str) -> String {
        format!("{}{resource}", self.options.service_root)
    }
    pub(crate) fn service_uri(&self, r: &Request) -> String {
        let scheme = if r.tls { "https" } else { "http" };
        let host = if r.host.is_empty() {
            "dicomweb"
        } else {
            &r.host
        };
        format!("{scheme}://{host}{}", self.options.service_root)
    }
    fn serve_core(&self, w: &mut dyn ResponseWriter, r: &mut Request) {
        let started = Instant::now();
        let mut writer = ServerWriter {
            inner: w,
            status: 200,
            written: false,
            remaining: self.limits.max_response_bytes,
            limit: self.limits.max_response_bytes,
            read_ctx: None,
        };
        let mut request_id = String::new();
        if let Some(cb) = &self.options.request_id {
            request_id = safe_request_id(cb, r);
            if !request_id.is_empty() {
                writer.header().set("X-Request-ID", &request_id);
            }
        }
        let operation = route_operation(&r.method, &r.url);
        let retrieve_like = matches!(
            operation,
            Operation::RetrieveRendered | Operation::RetrieveThumbnail | Operation::RetrieveWadoUri
        ) || (matches!(
            operation,
            Operation::RetrieveStudy
                | Operation::RetrieveSeries
                | Operation::RetrieveInstance
                | Operation::RetrieveFrames
        ) && self.retrieve_transcode_enabled());
        if retrieve_like {
            writer.header().set("Cache-Control", "private, no-store");
            writer.header().set("Vary", "Accept");
            writer.header().set("X-Content-Type-Options", "nosniff");
        }
        let mut state = CoreState {
            item_count: 0,
            error_code: String::new(),
            abort_read: false,
        };
        let outcome = catch_unwind(AssertUnwindSafe(|| {
            self.serve_core_body(&mut writer, r, operation, &mut state);
        }));
        if outcome.is_err() {
            state.error_code = "panic".into();
            if !writer.written {
                writer.clear_protected_read_headers();
                write_server_error(
                    &mut writer,
                    &ServerError::status(500, "backend_failure", ServerError::Backend),
                );
            }
        }
        if let Some(audit) = &self.options.audit {
            let event = AuditEvent {
                operation,
                request_id,
                status_code: writer.status,
                item_count: state.item_count,
                duration: started.elapsed(),
                error_code: state.error_code.clone(),
            };
            let _ = catch_unwind(AssertUnwindSafe(|| audit(r.context(), &event)));
        }
        if state.abort_read {
            writer.abort();
        }
    }
    pub(crate) fn retrieve_transcode_enabled(&self) -> bool {
        self.options.retrieve_transcode.is_some()
    }
    fn acquire_request_slot(&self, ctx: &Ctx) -> Result<(), ServerError> {
        let busy = ServerError::status(503, "busy", ServerError::ResourceLimit);
        if self.sem.try_acquire() {
            return Ok(());
        }
        if !self.queue.try_acquire() {
            return Err(busy);
        }
        let wait = self
            .limits
            .max_queue_wait
            .positive()
            .unwrap_or(Duration::ZERO);
        let result = self.sem.acquire_until(Instant::now() + wait, ctx);
        self.queue.release();
        match result {
            Ok(true) => Ok(()),
            Ok(false) => Err(busy),
            Err(e) => Err(e),
        }
    }
    fn serve_core_body(
        &self,
        writer: &mut ServerWriter<'_>,
        r: &mut Request,
        operation: Operation,
        state: &mut CoreState,
    ) {
        if r.request_uri.len() as i64 > self.limits.max_request_uri_bytes {
            state.error_code = "invalid_request".into();
            write_server_error(
                writer,
                &ServerError::status(414, &state.error_code, ServerError::InvalidRequest),
            );
            return;
        }
        if request_header_bytes(&r.header) > self.limits.max_header_bytes {
            state.error_code = "request_headers_too_large".into();
            write_server_error(
                writer,
                &ServerError::status(431, &state.error_code, ServerError::ResourceLimit),
            );
            return;
        }
        if let Err(err) = self.acquire_request_slot(r.context()) {
            state.error_code = error_code_for_server(&err);
            write_server_error(writer, &err);
            return;
        }
        struct Release<'a>(&'a Slots);
        impl Drop for Release<'_> {
            fn drop(&mut self) {
                self.0.release();
            }
        }
        let _release = Release(&self.sem);
        let mut ctx = r.context().clone();
        if let Some(d) = self.limits.max_duration.positive() {
            ctx = ctx.with_timeout(d);
        }
        if let Err(err) = ctx_err(&ctx) {
            state.error_code = context_error_code(&err).into();
            write_server_error(writer, &err);
            return;
        }
        r.set_context(ctx.clone());
        if operation == Operation::Unknown {
            let allowed = allowed_methods(&r.url.path);
            if !allowed.is_empty() {
                writer.header().set("Allow", &allowed.join(", "));
                state.error_code = "method_not_allowed".into();
                write_server_error(
                    writer,
                    &ServerError::status(405, &state.error_code, ServerError::Unsupported),
                );
                return;
            }
            state.error_code = "not_found".into();
            write_server_error(
                writer,
                &ServerError::status(404, &state.error_code, ServerError::NotFound),
            );
            return;
        }
        match &self.options.authorize {
            None => {
                if !self.options.allow_unauthenticated {
                    state.error_code = "unauthorized".into();
                    writer
                        .header()
                        .set("WWW-Authenticate", "Bearer realm=\"dicomweb\"");
                    write_server_error(writer, &ServerError::Unauthorized);
                    return;
                }
            }
            Some(authorize) => {
                if let Err(err) = safe_authorize(authorize, &ctx, r, operation) {
                    state.error_code = authorization_error_code(&err).into();
                    write_server_error(writer, &err);
                    return;
                }
            }
        }
        let mut lease: Option<Arc<RequestReadLease>> = None;
        if let Some(acquire) = &self.options.authorize_read_lease
            && is_studies_service_read(operation)
        {
            writer.header().set("Cache-Control", "private, no-store");
            let grant = match acquire_read_lease(acquire, r, operation) {
                Ok(g) => g,
                Err(err) => {
                    state.error_code = "read_lease_denied".into();
                    write_server_error(writer, &err);
                    return;
                }
            };
            let cancel = CancelToken::new();
            let expires = grant.expires_at.unwrap_or(SystemTime::UNIX_EPOCH);
            let until = expires
                .duration_since(SystemTime::now())
                .unwrap_or(Duration::ZERO);
            let lease_deadline = Instant::now() + until;
            let deadline_is_lease = r.context().deadline().is_none_or(|d| lease_deadline < d);
            let request_lease = Arc::new(RequestReadLease {
                lease: grant,
                cancel: cancel.clone(),
                denied: AtomicBool::new(false),
                deadline_is_lease,
            });
            let lease_ctx = r
                .context()
                .with_deadline(lease_deadline)
                .with_cancel(&cancel)
                .with_value(LeaseKey(request_lease.clone()));
            if let Err(err) = check_read_lease(&lease_ctx) {
                state.error_code = error_code_for_server(&err);
                write_server_error(writer, &err);
                return;
            }
            writer.read_ctx = Some(lease_ctx.clone());
            r.set_context(lease_ctx);
            lease = Some(request_lease);
        }
        let (count, mut err) = match self.dispatch(writer, r, operation) {
            Ok(n) => (n, None),
            Err((n, e)) => (n, Some(e)),
        };
        state.item_count = count;
        if let Some(l) = &lease
            && !writer.written
            && let Err(e) = l.check(r.context())
        {
            err = Some(e);
        }
        if let Some(l) = &lease {
            let denied = l.denied.load(Ordering::SeqCst)
                || (err.is_some()
                    && l.context_error(r.context()) == Err(ServerError::ReadLeaseDenied))
                || err
                    .as_ref()
                    .is_some_and(|e| e.is(&ServerError::ReadLeaseDenied));
            if denied {
                err = Some(ServerError::ReadLeaseDenied);
                state.abort_read = writer.written;
                if !writer.written {
                    state.item_count = 0;
                }
            }
        }
        if let Some(err) = err {
            state.error_code = error_code_for_server(&err);
            if !writer.written {
                writer.clear_protected_read_headers();
                write_server_error(writer, &err);
            }
        }
    }
    fn dispatch(
        &self,
        w: &mut ServerWriter<'_>,
        r: &mut Request,
        operation: Operation,
    ) -> Result<usize, (usize, ServerError)> {
        if is_workitem_operation(operation) {
            return self.serve_workitems(w, r, operation);
        }
        if request_accept_header(r).trim().is_empty() {
            return Err((
                0,
                ServerError::status(406, "not_acceptable", ServerError::Unsupported),
            ));
        }
        match operation {
            Operation::SearchStudies | Operation::SearchSeries | Operation::SearchInstances => {
                self.serve_search(w, r, operation)
            }
            Operation::RetrieveMetadata => self.serve_metadata(w, r),
            Operation::RetrieveWadoUri => self.serve_wado_uri(w, r),
            Operation::RetrieveStudy | Operation::RetrieveSeries | Operation::RetrieveInstance => {
                self.serve_retrieve(w, r, operation)
            }
            Operation::RetrieveFrames => self.serve_frames(w, r),
            Operation::RetrieveRendered | Operation::RetrieveThumbnail => {
                self.serve_rendered(w, r, operation == Operation::RetrieveThumbnail)
            }
            Operation::RetrieveBulkData => self.serve_bulk_data(w, r),
            Operation::StoreInstances => self.serve_store(w, r),
            _ => Err((0, ServerError::NotFound)),
        }
    }
}
pub(crate) fn request_header_bytes(header: &Header) -> i64 {
    header
        .0
        .iter()
        .map(|(k, values)| k.len() + values.iter().map(String::len).sum::<usize>())
        .sum::<usize>() as i64
}
pub(crate) fn request_accept_header(r: &Request) -> String {
    r.header.values("Accept").join(",")
}
pub(crate) fn resource_parts(path: &str) -> Vec<String> {
    let trimmed = path.trim_matches('/');
    if trimmed.is_empty() {
        return Vec::new();
    }
    trimmed.split('/').map(str::to_string).collect()
}
pub(crate) fn request_resource_parts(u: &Url) -> Vec<String> {
    let raw = resource_parts(&u.escaped_path());
    let mut parts = Vec::with_capacity(raw.len());
    for segment in raw {
        match urlrules::path_unescape(&segment) {
            Ok(decoded) => parts.push(decoded),
            Err(_) => return resource_parts(&u.path),
        }
    }
    parts
}
fn workitem_route_operation(method: &str, parts: &[String]) -> Operation {
    let p = |i: usize| parts.get(i).map_or("", String::as_str);
    if parts.len() == 2 && p(0) == "subscribers" {
        return if method == "GET" {
            Operation::OpenEventChannel
        } else {
            Operation::Unknown
        };
    }
    if parts.is_empty() || p(0) != "workitems" {
        return Operation::Unknown;
    }
    match (parts.len(), method) {
        (1, "GET") => Operation::SearchWorkitems,
        (1, "POST") => Operation::CreateWorkitem,
        (2, "GET") => Operation::RetrieveWorkitem,
        (2, "POST") => Operation::UpdateWorkitem,
        (3, "PUT") if p(2) == "state" => Operation::ChangeWorkitemState,
        (3, "POST") if p(2) == "cancelrequest" => Operation::RequestWorkitemCancellation,
        (4, "POST") if p(2) == "subscribers" => Operation::SubscribeWorkitem,
        (4, "DELETE") if p(2) == "subscribers" => Operation::UnsubscribeWorkitem,
        (5, "POST") if p(2) == "subscribers" && p(4) == "suspend" => {
            Operation::SuspendWorkitemSubscription
        }
        _ => Operation::Unknown,
    }
}
fn workitem_allowed_methods(parts: &[String]) -> Option<Vec<&'static str>> {
    let p = |i: usize| parts.get(i).map_or("", String::as_str);
    if parts.len() == 2 && p(0) == "subscribers" {
        return Some(vec!["GET"]);
    }
    if parts.is_empty() || p(0) != "workitems" {
        return None;
    }
    match parts.len() {
        1 | 2 => Some(vec!["GET", "POST"]),
        3 if p(2) == "state" => Some(vec!["PUT"]),
        3 if p(2) == "cancelrequest" => Some(vec!["POST"]),
        4 if p(2) == "subscribers" => Some(vec!["POST", "DELETE"]),
        5 if p(2) == "subscribers" && p(4) == "suspend" => Some(vec!["POST"]),
        _ => None,
    }
}
pub(crate) fn is_workitem_operation(op: Operation) -> bool {
    matches!(
        op,
        Operation::CreateWorkitem
            | Operation::RetrieveWorkitem
            | Operation::UpdateWorkitem
            | Operation::ChangeWorkitemState
            | Operation::RequestWorkitemCancellation
            | Operation::SearchWorkitems
            | Operation::SubscribeWorkitem
            | Operation::UnsubscribeWorkitem
            | Operation::SuspendWorkitemSubscription
            | Operation::OpenEventChannel
    )
}
fn allowed_methods(path: &str) -> Vec<&'static str> {
    let parts = resource_parts(path);
    if let Some(methods) = workitem_allowed_methods(&parts) {
        return methods;
    }
    if (parts.len() == 1 || parts.len() == 2) && parts[0] == "studies" {
        return vec!["GET", "POST"];
    }
    let probe = Url {
        path: path.to_string(),
        ..Url::default()
    };
    if route_operation("GET", &probe) != Operation::Unknown {
        return vec!["GET"];
    }
    Vec::new()
}
pub(crate) fn route_operation(method: &str, url: &Url) -> Operation {
    let op = workitem_route_operation(method, &request_resource_parts(url));
    if op != Operation::Unknown {
        return op;
    }
    let parts = resource_parts(&url.path);
    let op = workitem_route_operation(method, &parts);
    if op != Operation::Unknown {
        return op;
    }
    let p = |i: usize| parts.get(i).map_or("", String::as_str);
    let n = parts.len();
    if method == "POST" && n >= 1 && p(0) == "studies" && n <= 2 {
        return Operation::StoreInstances;
    }
    if method != "GET" {
        return Operation::Unknown;
    }
    if n >= 2 && p(0) == "bulkdata" {
        return Operation::RetrieveBulkData;
    }
    if n == 1 {
        match p(0) {
            "wado" => return Operation::RetrieveWadoUri,
            "studies" => return Operation::SearchStudies,
            "series" => return Operation::SearchSeries,
            "instances" => return Operation::SearchInstances,
            _ => {}
        }
    }
    if n == 2 && p(0) == "studies" {
        return Operation::RetrieveStudy;
    }
    let rendered = |s: &str| match s {
        "rendered" => Some(Operation::RetrieveRendered),
        "thumbnail" => Some(Operation::RetrieveThumbnail),
        _ => None,
    };
    if n == 3 && p(0) == "studies" {
        match p(2) {
            "series" => return Operation::SearchSeries,
            "instances" => return Operation::SearchInstances,
            "metadata" => return Operation::RetrieveMetadata,
            s => {
                if let Some(op) = rendered(s) {
                    return op;
                }
            }
        }
    }
    if n == 4 && p(0) == "studies" && p(2) == "series" {
        return Operation::RetrieveSeries;
    }
    if n == 5 && p(0) == "studies" && p(2) == "series" {
        match p(4) {
            "instances" => return Operation::SearchInstances,
            "metadata" => return Operation::RetrieveMetadata,
            s => {
                if let Some(op) = rendered(s) {
                    return op;
                }
            }
        }
    }
    let instance = n >= 6 && p(0) == "studies" && p(2) == "series" && p(4) == "instances";
    if n == 6 && instance {
        return Operation::RetrieveInstance;
    }
    if n == 7 && instance {
        if p(6) == "metadata" {
            return Operation::RetrieveMetadata;
        }
        if let Some(op) = rendered(p(6)) {
            return op;
        }
    }
    if n == 8 && instance && p(6) == "frames" {
        return Operation::RetrieveFrames;
    }
    if n == 9
        && instance
        && p(6) == "frames"
        && let Some(op) = rendered(p(8))
    {
        return op;
    }
    Operation::Unknown
}
impl Inner {
    pub(crate) fn validate_uid(&self, uid: &str) -> Result<(), ServerError> {
        if uid.len() as i64 > self.limits.max_uid_bytes
            || !crate::object::is_valid_uid(uid.as_bytes())
        {
            return Err(ServerError::InvalidRequest);
        }
        Ok(())
    }
    pub(crate) fn instance_location(&self, r: &super::InstanceRef) -> Result<String, ServerError> {
        for uid in [
            &r.study_instance_uid,
            &r.series_instance_uid,
            &r.sop_instance_uid,
        ] {
            self.validate_uid(uid)?;
        }
        Ok(self.service_path(&format!(
            "/studies/{}/series/{}/instances/{}",
            urlrules::path_escape(&r.study_instance_uid),
            urlrules::path_escape(&r.series_instance_uid),
            urlrules::path_escape(&r.sop_instance_uid)
        )))
    }
}
pub(crate) fn parse_frames(value: &str, maximum: i64) -> Result<Vec<i64>, ServerError> {
    let parts: Vec<&str> = value.split(',').collect();
    if parts.len() as i64 > maximum {
        return Err(ServerError::ResourceLimit);
    }
    let mut frames = Vec::with_capacity(parts.len());
    for raw in parts {
        let frame = match crate::dicomjson::sizedparse::parse_int(raw, 31) {
            Ok(f) if f > 0 && !frames.contains(&f) => f,
            _ => return Err(ServerError::InvalidRequest),
        };
        frames.push(frame);
    }
    Ok(frames)
}
pub type ServerDataset = Dataset;
pub(crate) fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}
pub(crate) fn lease_of(ctx: &Ctx) -> Option<Arc<RequestReadLease>> {
    request_lease(ctx)
}
pub(crate) fn lease_check_opt(
    lease: Option<&Arc<RequestReadLease>>,
    ctx: &Ctx,
) -> Result<(), ServerError> {
    lease_check(lease.map(|l| l.as_ref()), ctx)
}
