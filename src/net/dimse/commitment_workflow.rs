#![forbid(unsafe_code)]
use super::command::*;
use super::commitment::*;
use super::controls::context_err;
use super::error::{Detail, Error, Kind, Result};
use super::normalized::*;
use crate::net::ul::{self, Association, Ctx, Nanos};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, SystemTime};
pub(crate) const DEFAULT_STORAGE_COMMITMENT_MAX_DATA_SET_SIZE: i64 = MAX_NORMALIZED_DATA_SET_BYTES;
pub(crate) const DEFAULT_STORAGE_COMMITMENT_LEASE_DURATION: Nanos = Nanos(30_000_000_000);
pub(crate) const MINIMUM_STORAGE_COMMITMENT_LEASE_DURATION: Nanos = Nanos(1_000_000_000);
pub(crate) const MAXIMUM_STORAGE_COMMITMENT_LEASE_DURATION: Nanos = Nanos((1 << 62) - 1);
pub(crate) const DEFAULT_STORAGE_COMMITMENT_MAX_ATTEMPTS: i64 = 5;
pub(crate) const DEFAULT_STORAGE_COMMITMENT_INITIAL_BACKOFF: Nanos = Nanos(1_000_000_000);
pub(crate) const DEFAULT_STORAGE_COMMITMENT_MAX_BACKOFF: Nanos = Nanos(60_000_000_000);
pub(crate) const DEFAULT_MEMORY_COMMITMENT_TRANSACTIONS: i64 = 10_000;
pub const STORAGE_COMMITMENT_DIRECTION_REQUESTOR: &str = "requestor";
pub const STORAGE_COMMITMENT_DIRECTION_COMMITTER: &str = "committer";
pub const STORAGE_COMMITMENT_STATE_REQUEST_PENDING: &str = "request_pending";
pub const STORAGE_COMMITMENT_STATE_ACCEPTANCE_PREPARED: &str = "acceptance_prepared";
pub const STORAGE_COMMITMENT_STATE_ACCEPTED: &str = "accepted";
pub const STORAGE_COMMITMENT_STATE_PROCESSING: &str = "processing";
pub const STORAGE_COMMITMENT_STATE_REQUEST_REJECTED: &str = "request_rejected";
pub const STORAGE_COMMITMENT_STATE_RESULT_READY: &str = "result_ready";
pub const STORAGE_COMMITMENT_STATE_DELIVERING: &str = "delivering";
pub const STORAGE_COMMITMENT_STATE_DELIVERY_FAILED: &str = "delivery_failed";
pub const STORAGE_COMMITMENT_STATE_DELIVERY_EXHAUSTED: &str = "delivery_exhausted";
pub const STORAGE_COMMITMENT_STATE_DELIVERED: &str = "delivered";
pub const STORAGE_COMMITMENT_STATE_RESULT_RECEIVED: &str = "result_received";
pub const STORAGE_COMMITMENT_DELIVERY_SAME_ASSOCIATION: &str = "same_association";
pub const STORAGE_COMMITMENT_DELIVERY_CALLBACK: &str = "callback";
pub const STORAGE_COMMITMENT_DELIVERY_MANUAL: &str = "manual";
pub const STORAGE_COMMITMENT_FAILURE_NONE: &str = "";
pub const STORAGE_COMMITMENT_FAILURE_PROCESSING: &str = "processing";
pub const STORAGE_COMMITMENT_FAILURE_CALLBACK_UNKNOWN: &str = "callback_unknown";
pub const STORAGE_COMMITMENT_FAILURE_CALLBACK_OFFLINE: &str = "callback_offline";
pub const STORAGE_COMMITMENT_FAILURE_TLS: &str = "tls";
pub const STORAGE_COMMITMENT_FAILURE_TIMEOUT: &str = "timeout";
pub const STORAGE_COMMITMENT_FAILURE_CANCELED: &str = "canceled";
pub const STORAGE_COMMITMENT_FAILURE_ASSOCIATION_REJECTED: &str = "association_rejected";
pub const STORAGE_COMMITMENT_FAILURE_DIMSE_STATUS: &str = "dimse_status";
pub const STORAGE_COMMITMENT_FAILURE_PROTOCOL: &str = "protocol";
const UNIX_TO_ZERO_NANOS: i128 = 62_135_596_800 * 1_000_000_000;
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StorageCommitmentTime(i128);
impl StorageCommitmentTime {
    pub const ZERO: StorageCommitmentTime = StorageCommitmentTime(0);
    pub fn from_unix_nanos(nanos: i128) -> StorageCommitmentTime {
        StorageCommitmentTime(nanos + UNIX_TO_ZERO_NANOS)
    }
    pub fn unix_nanos(self) -> i128 {
        self.0 - UNIX_TO_ZERO_NANOS
    }
    pub fn now() -> StorageCommitmentTime {
        StorageCommitmentTime::from_system(SystemTime::now())
    }
    pub fn from_system(t: SystemTime) -> StorageCommitmentTime {
        let nanos = match t.duration_since(SystemTime::UNIX_EPOCH) {
            Ok(d) => d.as_nanos() as i128,
            Err(e) => -(e.duration().as_nanos() as i128),
        };
        StorageCommitmentTime::from_unix_nanos(nanos)
    }
    pub fn is_zero(self) -> bool {
        self.0 == 0
    }
    #[allow(clippy::should_implement_trait)]
    pub fn add(self, d: Nanos) -> StorageCommitmentTime {
        StorageCommitmentTime(self.0 + d.0 as i128)
    }
    pub fn before(self, other: StorageCommitmentTime) -> bool {
        self.0 < other.0
    }
    pub fn after(self, other: StorageCommitmentTime) -> bool {
        self.0 > other.0
    }
    pub fn nanos_since_zero(self) -> i128 {
        self.0
    }
    pub fn from_nanos_since_zero(nanos: i128) -> StorageCommitmentTime {
        StorageCommitmentTime(nanos)
    }
}
pub(crate) type StorageCommitmentClock = Arc<dyn Fn() -> StorageCommitmentTime + Send + Sync>;
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StorageCommitmentResult {
    pub transaction_uid: String,
    pub referenced_sops: Vec<StorageCommitmentSopReference>,
    pub failed_sops: Vec<StorageCommitmentSopReference>,
    pub produced_at: StorageCommitmentTime,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StorageCommitmentTransaction {
    pub transaction_uid: String,
    pub direction: String,
    pub state: String,
    pub delivery_mode: String,
    pub requestor_ae_title: String,
    pub committer_ae_title: String,
    pub referenced_sops: Vec<StorageCommitmentSopReference>,
    pub request_message_id: u16,
    pub request_digest: [u8; 32],
    pub result_digest: [u8; 32],
    pub result: Option<StorageCommitmentResult>,
    pub version: u64,
    pub created_at: StorageCommitmentTime,
    pub updated_at: StorageCommitmentTime,
    pub completed_at: StorageCommitmentTime,
    pub processing_tries: i64,
    pub delivery_attempts: i64,
    pub next_attempt_at: StorageCommitmentTime,
    pub lease_token: String,
    pub lease_until: StorageCommitmentTime,
    pub last_failure: String,
    pub last_dimse_status: u16,
}
impl StorageCommitmentTransaction {
    pub fn is_due(&self, now: StorageCommitmentTime) -> bool {
        transaction_due(self, now)
    }
}
pub fn storage_commitment_state_terminal(state: &str) -> bool {
    state == STORAGE_COMMITMENT_STATE_REQUEST_REJECTED
        || state == STORAGE_COMMITMENT_STATE_DELIVERED
        || state == STORAGE_COMMITMENT_STATE_DELIVERY_EXHAUSTED
        || state == STORAGE_COMMITMENT_STATE_RESULT_RECEIVED
}
pub(crate) fn transaction_due(
    t: &StorageCommitmentTransaction,
    now: StorageCommitmentTime,
) -> bool {
    if t.state == STORAGE_COMMITMENT_STATE_PROCESSING
        || t.state == STORAGE_COMMITMENT_STATE_DELIVERING
    {
        return !t.lease_until.after(now);
    }
    t.next_attempt_at.is_zero() || !t.next_attempt_at.after(now)
}
#[derive(Debug, Clone, Default)]
pub struct StorageCommitmentTransactionQuery {
    pub due_at: StorageCommitmentTime,
    pub states: Vec<String>,
    pub limit: i64,
}
pub trait StorageCommitmentTransactionStore: Send + Sync {
    fn create(&self, ctx: &Ctx, transaction: StorageCommitmentTransaction) -> Result<bool>;
    fn get(&self, ctx: &Ctx, transaction_uid: &str) -> Result<StorageCommitmentTransaction>;
    fn compare_and_swap(
        &self,
        ctx: &Ctx,
        transaction_uid: &str,
        expected_version: u64,
        next: StorageCommitmentTransaction,
    ) -> Result<StorageCommitmentTransaction>;
    fn list(
        &self,
        ctx: &Ctx,
        query: &StorageCommitmentTransactionQuery,
    ) -> Result<Vec<StorageCommitmentTransaction>>;
    fn purge_completed(&self, ctx: &Ctx, before: StorageCommitmentTime, limit: i64) -> Result<i64>;
}
pub(crate) fn sentinel(kind: Kind) -> Error {
    Error::sentinel(kind)
}
pub(crate) fn conflict() -> Error {
    sentinel(Kind::StorageCommitmentTransactionConflict)
}
pub(crate) fn concurrent_update() -> Error {
    sentinel(Kind::StorageCommitmentConcurrentUpdate)
}
pub(crate) fn not_found() -> Error {
    sentinel(Kind::StorageCommitmentTransactionNotFound)
}
pub struct MemoryStorageCommitmentStore {
    max_transactions: usize,
    transactions: RwLock<HashMap<String, StorageCommitmentTransaction>>,
}
impl MemoryStorageCommitmentStore {
    pub fn new(max_transactions: i64) -> Result<MemoryStorageCommitmentStore> {
        if max_transactions < 0 {
            return Err(Error::wrap(
                Kind::StorageCommitmentResourceLimit,
                "MaxTransactions",
            ));
        }
        let max = if max_transactions == 0 {
            DEFAULT_MEMORY_COMMITMENT_TRANSACTIONS
        } else {
            max_transactions
        };
        Ok(MemoryStorageCommitmentStore {
            max_transactions: max as usize,
            transactions: RwLock::new(HashMap::new()),
        })
    }
}
impl StorageCommitmentTransactionStore for MemoryStorageCommitmentStore {
    fn create(&self, ctx: &Ctx, transaction: StorageCommitmentTransaction) -> Result<bool> {
        if let Some(e) = context_err(ctx) {
            return Err(e);
        }
        let mut transaction = transaction;
        if transaction.transaction_uid.is_empty() {
            return Err(invalid_request());
        }
        let mut m = self.transactions.write().unwrap();
        if m.contains_key(&transaction.transaction_uid) {
            return Ok(false);
        }
        if m.len() >= self.max_transactions {
            return Err(resource_limit());
        }
        transaction.version = 1;
        m.insert(transaction.transaction_uid.clone(), transaction);
        Ok(true)
    }
    fn get(&self, ctx: &Ctx, transaction_uid: &str) -> Result<StorageCommitmentTransaction> {
        if let Some(e) = context_err(ctx) {
            return Err(e);
        }
        let m = self.transactions.read().unwrap();
        m.get(transaction_uid).cloned().ok_or_else(not_found)
    }
    fn compare_and_swap(
        &self,
        ctx: &Ctx,
        transaction_uid: &str,
        expected_version: u64,
        next: StorageCommitmentTransaction,
    ) -> Result<StorageCommitmentTransaction> {
        if let Some(e) = context_err(ctx) {
            return Err(e);
        }
        let mut next = next;
        let mut m = self.transactions.write().unwrap();
        let Some(current) = m.get(transaction_uid) else {
            return Err(not_found());
        };
        if current.version != expected_version {
            return Err(concurrent_update());
        }
        if next.transaction_uid != transaction_uid
            || next.direction != current.direction
            || next.request_digest != current.request_digest
        {
            return Err(conflict());
        }
        next.version = expected_version.wrapping_add(1);
        m.insert(transaction_uid.to_string(), next.clone());
        Ok(next)
    }
    fn list(
        &self,
        ctx: &Ctx,
        query: &StorageCommitmentTransactionQuery,
    ) -> Result<Vec<StorageCommitmentTransaction>> {
        if let Some(e) = context_err(ctx) {
            return Err(e);
        }
        if query.limit <= 0 {
            return Err(Error::wrap(
                Kind::StorageCommitmentResourceLimit,
                "List limit",
            ));
        }
        let wanted: HashSet<&str> = query.states.iter().map(|s| s.as_str()).collect();
        let mut items = Vec::new();
        {
            let m = self.transactions.read().unwrap();
            for t in m.values() {
                if let Some(e) = context_err(ctx) {
                    return Err(e);
                }
                if !wanted.is_empty() && !wanted.contains(t.state.as_str()) {
                    continue;
                }
                if !query.due_at.is_zero() && !transaction_due(t, query.due_at) {
                    continue;
                }
                items.push(t.clone());
            }
        }
        items.sort_by(|a, b| {
            (a.updated_at, a.transaction_uid.as_str())
                .cmp(&(b.updated_at, b.transaction_uid.as_str()))
        });
        items.truncate(query.limit as usize);
        Ok(items)
    }
    fn purge_completed(&self, ctx: &Ctx, before: StorageCommitmentTime, limit: i64) -> Result<i64> {
        if let Some(e) = context_err(ctx) {
            return Err(e);
        }
        if limit <= 0 {
            return Err(resource_limit());
        }
        let mut m = self.transactions.write().unwrap();
        let mut keys: Vec<String> = m
            .iter()
            .filter(|(_, t)| {
                storage_commitment_state_terminal(&t.state) && t.updated_at.before(before)
            })
            .map(|(k, _)| k.clone())
            .collect();
        keys.sort();
        keys.truncate(limit as usize);
        for k in &keys {
            m.remove(k);
        }
        Ok(keys.len() as i64)
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StorageCommitmentLimits {
    pub max_references: i64,
    pub max_data_set_bytes: i64,
    pub max_list_batch: i64,
}
pub(crate) fn normalize_storage_commitment_limits(
    mut limits: StorageCommitmentLimits,
) -> Result<StorageCommitmentLimits> {
    if limits.max_references < 0 || limits.max_data_set_bytes < 0 || limits.max_list_batch < 0 {
        return Err(resource_limit());
    }
    if limits.max_references == 0 {
        limits.max_references = DEFAULT_STORAGE_COMMITMENT_MAX_REFERENCES as i64;
    }
    if limits.max_references > DEFAULT_STORAGE_COMMITMENT_MAX_REFERENCES as i64 {
        return Err(resource_limit());
    }
    if limits.max_data_set_bytes == 0 {
        limits.max_data_set_bytes = DEFAULT_STORAGE_COMMITMENT_MAX_DATA_SET_SIZE;
    }
    if limits.max_list_batch == 0 {
        limits.max_list_batch = 128;
    }
    Ok(limits)
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StorageCommitmentRetryPolicy {
    pub max_attempts: i64,
    pub initial_backoff: Nanos,
    pub max_backoff: Nanos,
}
pub(crate) fn normalize_storage_commitment_retry_policy(
    mut policy: StorageCommitmentRetryPolicy,
) -> Result<StorageCommitmentRetryPolicy> {
    if policy.max_attempts < 0 || policy.initial_backoff.0 < 0 || policy.max_backoff.0 < 0 {
        return Err(resource_limit());
    }
    if policy.max_attempts == 0 {
        policy.max_attempts = DEFAULT_STORAGE_COMMITMENT_MAX_ATTEMPTS;
    }
    if policy.initial_backoff.0 == 0 {
        policy.initial_backoff = DEFAULT_STORAGE_COMMITMENT_INITIAL_BACKOFF;
    }
    if policy.max_backoff.0 == 0 {
        policy.max_backoff = DEFAULT_STORAGE_COMMITMENT_MAX_BACKOFF;
    }
    if policy.max_backoff.0 < policy.initial_backoff.0 {
        return Err(resource_limit());
    }
    Ok(policy)
}
pub(crate) fn storage_commitment_backoff(
    policy: StorageCommitmentRetryPolicy,
    attempt: i64,
) -> Nanos {
    if attempt <= 1 {
        return policy.initial_backoff;
    }
    let mut backoff = policy.initial_backoff.0;
    for _ in 1..attempt {
        if backoff >= policy.max_backoff.0 / 2 {
            return policy.max_backoff;
        }
        backoff = backoff.wrapping_mul(2);
    }
    if backoff > policy.max_backoff.0 {
        return policy.max_backoff;
    }
    Nanos(backoff)
}
pub type StorageCommitmentProcessor = Arc<
    dyn Fn(&Ctx, StorageCommitmentTransaction) -> Result<StorageCommitmentResult> + Send + Sync,
>;
pub type StorageCommitmentResultConsumer =
    Arc<dyn Fn(&Ctx, StorageCommitmentTransaction) -> Result<()> + Send + Sync>;
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StorageCommitmentCallbackRequest {
    pub transaction_uid: String,
    pub requestor_ae_title: String,
    pub committer_ae_title: String,
}
#[derive(Clone, Default)]
pub struct StorageCommitmentCallbackTarget {
    pub address: String,
    pub dial_options: ul::DialOptions,
}
pub type StorageCommitmentCallbackResolver = Arc<
    dyn Fn(&Ctx, &StorageCommitmentCallbackRequest) -> Result<StorageCommitmentCallbackTarget>
        + Send
        + Sync,
>;
pub type StorageCommitmentAssociationDialer =
    Arc<dyn Fn(&Ctx, &str, ul::DialOptions) -> Result<Association> + Send + Sync>;
#[derive(Clone, Default)]
pub struct StorageCommitmentWorkflowOptions {
    pub store: Option<Arc<dyn StorageCommitmentTransactionStore>>,
    pub limits: StorageCommitmentLimits,
    pub retry_policy: StorageCommitmentRetryPolicy,
    pub lease_duration: Nanos,
    pub uid_generator: Option<Arc<dyn Fn() -> Result<String> + Send + Sync>>,
    pub processor: Option<StorageCommitmentProcessor>,
    pub consumer: Option<StorageCommitmentResultConsumer>,
    pub resolver: Option<StorageCommitmentCallbackResolver>,
    pub dialer: Option<StorageCommitmentAssociationDialer>,
    pub now: Option<StorageCommitmentClock>,
    pub default_delivery_mode: String,
}
pub(crate) struct WorkflowInner {
    pub(crate) store: Arc<dyn StorageCommitmentTransactionStore>,
    pub(crate) limits: StorageCommitmentLimits,
    pub(crate) retry: StorageCommitmentRetryPolicy,
    pub(crate) lease_duration: Nanos,
    pub(crate) uid_generator: Arc<dyn Fn() -> Result<String> + Send + Sync>,
    pub(crate) processor: Option<StorageCommitmentProcessor>,
    pub(crate) consumer: Option<StorageCommitmentResultConsumer>,
    pub(crate) resolver: Option<StorageCommitmentCallbackResolver>,
    pub(crate) dialer: StorageCommitmentAssociationDialer,
    pub(crate) now: StorageCommitmentClock,
    pub(crate) default_delivery_mode: String,
    message_id: Mutex<u16>,
}
#[derive(Clone)]
pub struct StorageCommitmentWorkflow {
    pub(crate) inner: Arc<WorkflowInner>,
}
impl fmt::Debug for StorageCommitmentWorkflow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("StorageCommitmentWorkflow")
    }
}
pub(crate) fn valid_storage_commitment_delivery_mode(mode: &str) -> bool {
    mode == STORAGE_COMMITMENT_DELIVERY_SAME_ASSOCIATION
        || mode == STORAGE_COMMITMENT_DELIVERY_CALLBACK
        || mode == STORAGE_COMMITMENT_DELIVERY_MANUAL
}
pub fn generate_storage_commitment_transaction_uid() -> Result<String> {
    Ok(format!("2.25.{}", uuid::Uuid::new_v4().as_u128()))
}
pub(crate) fn new_storage_commitment_lease_token() -> String {
    let a = uuid::Uuid::new_v4().as_u128();
    let b = uuid::Uuid::new_v4().as_u128();
    format!("{:032x}", a ^ b.rotate_left(64))
}
pub(crate) fn persistence_context() -> Ctx {
    Ctx::background().with_timeout(Duration::from_secs(1))
}
pub(crate) fn sanitized(class: &'static str, cause: Error) -> Error {
    delivery_error(class, 0, false, cause)
}
pub(crate) fn delivery_error(
    class: &'static str,
    status: u16,
    retryable: bool,
    cause: Error,
) -> Error {
    let inner = Box::new(cause.detail().clone());
    cause
        .relabel(format!(
            "dicom dimse: storage commitment delivery failed ({class})"
        ))
        .with_detail(Detail::StorageCommitmentDelivery(
            class, status, retryable, inner,
        ))
}
pub fn storage_commitment_delivery_error(err: &Error) -> Option<(&'static str, u16, bool)> {
    match err.detail() {
        Detail::StorageCommitmentDelivery(class, status, retryable, _) => {
            Some((*class, *status, *retryable))
        }
        _ => None,
    }
}
pub(crate) fn store_error(err: Error) -> Error {
    if storage_commitment_delivery_error(&err).is_some() {
        return err;
    }
    sanitized(STORAGE_COMMITMENT_FAILURE_PROCESSING, err)
}
#[derive(Clone, Default)]
pub struct StorageCommitmentRequestOptions {
    pub transaction_uid: String,
    pub delivery_mode: String,
    pub operation: NormalizedOperationOptions,
}
impl StorageCommitmentWorkflow {
    pub fn new(options: StorageCommitmentWorkflowOptions) -> Result<StorageCommitmentWorkflow> {
        let Some(store) = options.store else {
            return Err(Error::new(
                "dicom dimse: Storage Commitment store is required",
            ));
        };
        let limits = normalize_storage_commitment_limits(options.limits)?;
        let retry = normalize_storage_commitment_retry_policy(options.retry_policy)?;
        let lease = options.lease_duration;
        if lease.0 < 0
            || lease.0 > MAXIMUM_STORAGE_COMMITMENT_LEASE_DURATION.0
            || (lease.0 > 0 && lease.0 < MINIMUM_STORAGE_COMMITMENT_LEASE_DURATION.0)
        {
            return Err(resource_limit());
        }
        let lease = if lease.0 == 0 {
            DEFAULT_STORAGE_COMMITMENT_LEASE_DURATION
        } else {
            lease
        };
        let mode = if options.default_delivery_mode.is_empty() {
            STORAGE_COMMITMENT_DELIVERY_CALLBACK.to_string()
        } else {
            options.default_delivery_mode
        };
        if !valid_storage_commitment_delivery_mode(&mode) {
            return Err(invalid_request());
        }
        let dialer: StorageCommitmentAssociationDialer = options.dialer.unwrap_or_else(|| {
            Arc::new(|ctx: &Ctx, address: &str, opts: ul::DialOptions| {
                ul::dial_context(ctx, address, opts).map_err(Error::from)
            })
        });
        Ok(StorageCommitmentWorkflow {
            inner: Arc::new(WorkflowInner {
                store,
                limits,
                retry,
                lease_duration: lease,
                uid_generator: options
                    .uid_generator
                    .unwrap_or_else(|| Arc::new(generate_storage_commitment_transaction_uid)),
                processor: options.processor,
                consumer: options.consumer,
                resolver: options.resolver,
                dialer,
                now: options
                    .now
                    .unwrap_or_else(|| Arc::new(StorageCommitmentTime::now)),
                default_delivery_mode: mode,
                message_id: Mutex::new(0),
            }),
        })
    }
    pub(crate) fn now(&self) -> StorageCommitmentTime {
        (self.inner.now)()
    }
    pub(crate) fn lease(&self) -> Nanos {
        self.inner.lease_duration
    }
    pub fn limits(&self) -> StorageCommitmentLimits {
        self.inner.limits
    }
    pub(crate) fn store_create(&self, ctx: &Ctx, t: StorageCommitmentTransaction) -> Result<bool> {
        self.inner.store.create(ctx, t).map_err(store_error)
    }
    pub(crate) fn store_get(&self, ctx: &Ctx, uid: &str) -> Result<StorageCommitmentTransaction> {
        self.inner.store.get(ctx, uid).map_err(store_error)
    }
    pub(crate) fn store_cas(
        &self,
        ctx: &Ctx,
        uid: &str,
        version: u64,
        t: StorageCommitmentTransaction,
    ) -> Result<StorageCommitmentTransaction> {
        self.inner
            .store
            .compare_and_swap(ctx, uid, version, t)
            .map_err(store_error)
    }
    pub(crate) fn store_list(
        &self,
        ctx: &Ctx,
        q: &StorageCommitmentTransactionQuery,
    ) -> Result<Vec<StorageCommitmentTransaction>> {
        self.inner.store.list(ctx, q).map_err(store_error)
    }
    pub fn request(
        &self,
        ctx: Option<&Ctx>,
        assoc: &Association,
        references: &[StorageCommitmentSopReference],
        options: StorageCommitmentRequestOptions,
    ) -> (StorageCommitmentTransaction, Result<()>) {
        let ctx = ctx.cloned().unwrap_or_default();
        let fail = |e: Error| (StorageCommitmentTransaction::default(), Err(e));
        if !storage_commitment_requestor_can_act_as_scu(assoc) {
            return fail(invalid_request());
        }
        let mut uid = options.transaction_uid.trim().to_string();
        if uid.is_empty() {
            match (self.inner.uid_generator)() {
                Ok(u) => uid = u,
                Err(e) => return fail(e),
            }
        }
        let mode = if options.delivery_mode.is_empty() {
            self.inner.default_delivery_mode.clone()
        } else {
            options.delivery_mode.clone()
        };
        let references = normalize_storage_commitment_references(
            references,
            false,
            self.inner.limits.max_references as usize,
        );
        let references = match references {
            Ok(r) if valid_uid(&uid) && valid_storage_commitment_delivery_mode(&mode) => r,
            _ => return fail(invalid_request()),
        };
        let now = self.now();
        let mut transaction = StorageCommitmentTransaction {
            transaction_uid: uid.clone(),
            direction: STORAGE_COMMITMENT_DIRECTION_REQUESTOR.into(),
            state: STORAGE_COMMITMENT_STATE_REQUEST_PENDING.into(),
            delivery_mode: mode,
            requestor_ae_title: ul::normalize_ae_title(&assoc.calling_ae_title).to_string(),
            committer_ae_title: ul::normalize_ae_title(&assoc.called_ae_title).to_string(),
            referenced_sops: references,
            request_message_id: self.next_message_id(),
            created_at: now,
            updated_at: now,
            ..Default::default()
        };
        transaction.request_digest = storage_commitment_request_digest(&transaction);
        let created = match self.store_create(&ctx, transaction.clone()) {
            Ok(c) => c,
            Err(e) => return fail(e),
        };
        if !created {
            let existing = match self.store_get(&ctx, &uid) {
                Ok(t) => t,
                Err(e) => return fail(e),
            };
            if !same_storage_commitment_request(&existing, &transaction) {
                return fail(conflict());
            }
            return (existing, Ok(()));
        }
        let record = match self.store_get(&ctx, &uid) {
            Ok(t) => t,
            Err(e) => return fail(e),
        };
        self.submit_request(&ctx, assoc, record, options.operation)
    }
    pub fn retry_request(
        &self,
        ctx: Option<&Ctx>,
        assoc: &Association,
        transaction_uid: &str,
        operation: NormalizedOperationOptions,
    ) -> (StorageCommitmentTransaction, Result<()>) {
        let ctx = ctx.cloned().unwrap_or_default();
        let fail = |e: Error| (StorageCommitmentTransaction::default(), Err(e));
        if !storage_commitment_requestor_can_act_as_scu(assoc) {
            return fail(invalid_request());
        }
        let mut transaction = match self.store_get(&ctx, transaction_uid) {
            Ok(t) => t,
            Err(e) => return fail(e),
        };
        if transaction.direction != STORAGE_COMMITMENT_DIRECTION_REQUESTOR
            || transaction.state != STORAGE_COMMITMENT_STATE_REQUEST_PENDING
        {
            return fail(conflict());
        }
        transaction.request_message_id = self.next_message_id();
        transaction.updated_at = self.now();
        let version = transaction.version;
        let transaction = match self.store_cas(&ctx, transaction_uid, version, transaction) {
            Ok(t) => t,
            Err(e) => return fail(e),
        };
        self.submit_request(&ctx, assoc, transaction, operation)
    }
    fn submit_request(
        &self,
        ctx: &Ctx,
        assoc: &Association,
        transaction: StorageCommitmentTransaction,
        mut operation: NormalizedOperationOptions,
    ) -> (StorageCommitmentTransaction, Result<()>) {
        let data_set = match build_storage_commitment_action_information(
            &transaction.transaction_uid,
            &transaction.referenced_sops,
        ) {
            Ok(d) => d,
            Err(_) => return (transaction, Err(invalid_request())),
        };
        operation.operation.context = Some(ctx.clone());
        let (result, err) = NormalizedClient::new(assoc).action(
            &operation,
            NormalizedRequest {
                sop_class_uid: STORAGE_COMMITMENT_PUSH_MODEL_SOP_CLASS_UID.into(),
                sop_instance_uid: STORAGE_COMMITMENT_PUSH_MODEL_SOP_INSTANCE_UID.into(),
                message_id: transaction.request_message_id,
                command_data_set_type: DATA_SET_PRESENT,
                type_id: STORAGE_COMMITMENT_ACTION_TYPE_ID,
                ..Default::default()
            },
            Some(&data_set),
        );
        let reject = |status: u16, err: Option<Error>| {
            let persist = persistence_context();
            match self.update_state(
                &persist,
                &transaction.transaction_uid,
                &[STORAGE_COMMITMENT_STATE_REQUEST_PENDING],
                STORAGE_COMMITMENT_STATE_REQUEST_REJECTED,
                |next| {
                    next.last_dimse_status = status;
                    next.completed_at = self.now();
                },
            ) {
                Ok(updated) => (updated, err.map_or(Ok(()), Err)),
                Err(e) => (transaction.clone(), Err(e)),
            }
        };
        if let Err(e) = err {
            if let Some(response) = &result.response
                && e.is(Kind::NormalizedStatus)
            {
                let status = response.status;
                return reject(
                    status,
                    check_storage_commitment_status("N-ACTION-RSP", status),
                );
            }
            return (transaction, Err(e));
        }
        let Some(response) = &result.response else {
            return (transaction, Err(invalid_request()));
        };
        if let Some(e) = check_storage_commitment_status("N-ACTION-RSP", response.status) {
            return reject(response.status, Some(e));
        }
        match self.mark_requestor_accepted(ctx, &transaction.transaction_uid) {
            Ok(t) => (t, Ok(())),
            Err(e) => (StorageCommitmentTransaction::default(), Err(e)),
        }
    }
    fn mark_requestor_accepted(
        &self,
        _ctx: &Ctx,
        uid: &str,
    ) -> Result<StorageCommitmentTransaction> {
        let persist = persistence_context();
        for _ in 0..8 {
            let current = self
                .store_get(&persist, uid)
                .map_err(|e| sanitized(STORAGE_COMMITMENT_FAILURE_PROCESSING, e))?;
            if current.direction != STORAGE_COMMITMENT_DIRECTION_REQUESTOR {
                return Err(conflict());
            }
            match current.state.as_str() {
                STORAGE_COMMITMENT_STATE_ACCEPTED
                | STORAGE_COMMITMENT_STATE_RESULT_READY
                | STORAGE_COMMITMENT_STATE_RESULT_RECEIVED => return Ok(current),
                STORAGE_COMMITMENT_STATE_REQUEST_PENDING => {}
                _ => return Err(conflict()),
            }
            let mut next = current.clone();
            next.state = STORAGE_COMMITMENT_STATE_ACCEPTED.into();
            next.updated_at = self.now();
            match self.store_cas(&persist, &current.transaction_uid, current.version, next) {
                Err(e) if e.is(Kind::StorageCommitmentConcurrentUpdate) => continue,
                Err(e) => return Err(sanitized(STORAGE_COMMITMENT_FAILURE_PROCESSING, e)),
                Ok(t) => return Ok(t),
            }
        }
        Err(concurrent_update())
    }
    fn next_message_id(&self) -> u16 {
        let mut m = self.inner.message_id.lock().unwrap();
        *m = m.wrapping_add(1);
        if *m == 0 {
            *m = 1;
        }
        *m
    }
    pub fn get(
        &self,
        ctx: Option<&Ctx>,
        transaction_uid: &str,
    ) -> Result<StorageCommitmentTransaction> {
        let ctx = ctx.cloned().unwrap_or_default();
        self.store_get(&ctx, transaction_uid)
    }
    pub fn purge_completed(
        &self,
        ctx: Option<&Ctx>,
        before: StorageCommitmentTime,
        limit: i64,
    ) -> Result<i64> {
        if limit <= 0 || limit > self.inner.limits.max_list_batch {
            return Err(resource_limit());
        }
        let ctx = ctx.cloned().unwrap_or_default();
        self.inner
            .store
            .purge_completed(&ctx, before, limit)
            .map_err(store_error)
    }
    pub fn wait(
        &self,
        ctx: Option<&Ctx>,
        transaction_uid: &str,
        interval: Nanos,
    ) -> Result<StorageCommitmentTransaction> {
        let ctx = ctx.cloned().unwrap_or_default();
        if interval.0 < 0 {
            return Err(invalid_request());
        }
        let interval = if interval.0 == 0 {
            Duration::from_millis(100)
        } else {
            Duration::from_nanos(interval.0 as u64)
        };
        let mut next_tick = std::time::Instant::now() + interval;
        loop {
            let t = self.store_get(&ctx, transaction_uid)?;
            if storage_commitment_state_terminal(&t.state) {
                return Ok(t);
            }
            loop {
                if let Some(e) = context_err(&ctx) {
                    return Err(e);
                }
                let now = std::time::Instant::now();
                if now >= next_tick {
                    while next_tick <= now {
                        next_tick += interval;
                    }
                    break;
                }
                std::thread::sleep((next_tick - now).min(Duration::from_millis(2)));
            }
        }
    }
    pub(crate) fn update_state(
        &self,
        ctx: &Ctx,
        uid: &str,
        allowed: &[&str],
        state: &str,
        mutate: impl Fn(&mut StorageCommitmentTransaction),
    ) -> Result<StorageCommitmentTransaction> {
        for _ in 0..8 {
            let current = self.store_get(ctx, uid)?;
            if current.state == state {
                return Ok(current);
            }
            if !allowed.contains(&current.state.as_str()) {
                return Err(conflict());
            }
            let mut next = current.clone();
            next.state = state.to_string();
            next.updated_at = self.now();
            mutate(&mut next);
            match self.store_cas(ctx, uid, current.version, next) {
                Err(e) if e.is(Kind::StorageCommitmentConcurrentUpdate) => continue,
                r => return r,
            }
        }
        Err(concurrent_update())
    }
}
fn write_digest_string(hash: &mut Sha256, value: &str) {
    hash.update((value.len() as u64).to_be_bytes());
    hash.update(value.as_bytes());
}
#[doc(hidden)]
pub fn storage_commitment_request_digest(t: &StorageCommitmentTransaction) -> [u8; 32] {
    let mut hash = Sha256::new();
    write_digest_string(&mut hash, &t.direction);
    write_digest_string(&mut hash, &t.delivery_mode);
    write_digest_string(&mut hash, &t.transaction_uid);
    write_digest_string(&mut hash, &t.requestor_ae_title);
    write_digest_string(&mut hash, &t.committer_ae_title);
    for r in &t.referenced_sops {
        write_digest_string(&mut hash, &r.sop_class_uid);
        write_digest_string(&mut hash, &r.sop_instance_uid);
    }
    hash.finalize().into()
}
#[doc(hidden)]
pub fn storage_commitment_result_digest(result: &StorageCommitmentResult) -> [u8; 32] {
    let mut hash = Sha256::new();
    write_digest_string(&mut hash, &result.transaction_uid);
    for r in &result.referenced_sops {
        write_digest_string(&mut hash, &r.sop_class_uid);
        write_digest_string(&mut hash, &r.sop_instance_uid);
    }
    for r in &result.failed_sops {
        write_digest_string(&mut hash, &r.sop_class_uid);
        write_digest_string(&mut hash, &r.sop_instance_uid);
        hash.update(r.failure_reason.to_be_bytes());
    }
    hash.finalize().into()
}
pub(crate) fn same_storage_commitment_request(
    a: &StorageCommitmentTransaction,
    b: &StorageCommitmentTransaction,
) -> bool {
    a.request_digest == b.request_digest
        && a.direction == b.direction
        && a.requestor_ae_title == b.requestor_ae_title
        && a.committer_ae_title == b.committer_ae_title
}
pub(crate) fn storage_commitment_result_from_event(
    event_type_id: u16,
    information: &StorageCommitmentEventInformation,
) -> Result<StorageCommitmentResult> {
    if !valid_event_type(event_type_id) {
        return Err(invalid_result());
    }
    if event_type_id == STORAGE_COMMITMENT_EVENT_TYPE_SUCCESS && !information.failed_sops.is_empty()
    {
        return Err(invalid_result());
    }
    if event_type_id == STORAGE_COMMITMENT_EVENT_TYPE_FAILURES && information.failed_sops.is_empty()
    {
        return Err(invalid_result());
    }
    Ok(StorageCommitmentResult {
        transaction_uid: information.transaction_uid.clone(),
        referenced_sops: information.referenced_sops.clone(),
        failed_sops: information.failed_sops.clone(),
        produced_at: StorageCommitmentTime::ZERO,
    })
}
pub(crate) fn validate_storage_commitment_result(
    max_references: i64,
    now: impl Fn() -> StorageCommitmentTime,
    transaction: &StorageCommitmentTransaction,
    result: &StorageCommitmentResult,
) -> Result<StorageCommitmentResult> {
    let mut result = result.clone();
    if result.transaction_uid != transaction.transaction_uid {
        return Err(invalid_result());
    }
    let total = result.referenced_sops.len() + result.failed_sops.len();
    if total != transaction.referenced_sops.len() || total as i64 > max_references {
        return Err(invalid_result());
    }
    let limit = max_references.max(0) as usize;
    if !result.referenced_sops.is_empty() {
        result.referenced_sops =
            normalize_storage_commitment_references(&result.referenced_sops, false, limit)
                .map_err(|_| invalid_result())?;
    }
    if !result.failed_sops.is_empty() {
        result.failed_sops =
            normalize_storage_commitment_references(&result.failed_sops, true, limit)
                .map_err(|_| invalid_result())?;
    }
    if result.referenced_sops.is_empty() && result.failed_sops.is_empty() {
        return Err(invalid_result());
    }
    let key =
        |r: &StorageCommitmentSopReference| format!("{}\0{}", r.sop_class_uid, r.sop_instance_uid);
    let wanted: HashSet<String> = transaction.referenced_sops.iter().map(key).collect();
    let mut seen = HashSet::new();
    for r in result
        .referenced_sops
        .iter()
        .chain(result.failed_sops.iter())
    {
        let k = key(r);
        if !wanted.contains(&k) || !seen.insert(k) {
            return Err(invalid_result());
        }
    }
    if seen.len() != wanted.len() {
        return Err(invalid_result());
    }
    if result.produced_at.is_zero() {
        result.produced_at = now();
    }
    Ok(result)
}
