#![forbid(unsafe_code)]
use super::async_session::*;
use super::command::*;
use super::controls::{check_store_access, context_err};
use super::error::{Cause, Detail, Error, Kind, Result};
use super::message::{
    accepted_context_for_sop_class_transfer_syntaxes, explain_missing_presentation_context,
    transfer_syntax_for_accepted_context,
};
use super::storage_catalog::common_extended_negotiation_for;
use super::store::{CStoreOptions, StoreClient, check_c_store_status, is_c_store_warning_status};
use super::store_source::*;
use crate::index;
use crate::net::ul::{
    self, AcceptedContext, Association, AsynchronousOperationsWindow, Ctx, DialOptions, Nanos,
    PresentationContext,
};
use crate::object::{Syntax, is_valid_uid};
use crate::transfer::filter_transfer_syntax_uids;
use std::collections::{BTreeMap, HashMap};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, mpsc};
use std::time::Duration;
pub const DEFAULT_STORE_MAX_ITEMS: i64 = 100_000;
pub const DEFAULT_STORE_MAX_ITEM_BYTES: i64 = 64 << 30;
pub const DEFAULT_STORE_MAX_TOTAL_BYTES: i64 = 1 << 40;
pub const DEFAULT_STORE_MAX_ASSOCIATIONS: i64 = 1024;
pub const DEFAULT_STORE_MAX_TRANSFER_SYNTAXES: i64 = 16;
const MAX_DEAD_STORE_ASSOCIATIONS: i64 = 2;
const MAX_STORE_PROFILE_TRANSFER_SYNTAXES: usize = 256;
#[doc(hidden)]
pub const STORE_ERROR_NIL_TEXT: &str = "dicom dimse: C-STORE batch failed";
pub fn store_error_of(err: &Error) -> Option<(&str, i64, bool)> {
    match err.detail() {
        Detail::Store(stage, index, uncertain, _) => Some((stage.as_str(), *index, *uncertain)),
        _ => None,
    }
}
pub fn store_error_missing_presentation_context(
    err: &Error,
) -> Option<(&str, &[ul::PresentationContextOutcome])> {
    match err.detail() {
        Detail::Store(_, _, _, inner) => match inner.as_ref() {
            Detail::MissingPresentationContext(sop, outcomes) => {
                Some((sop.as_str(), outcomes.as_slice()))
            }
            _ => None,
        },
        Detail::MissingPresentationContext(sop, outcomes) => {
            Some((sop.as_str(), outcomes.as_slice()))
        }
        _ => None,
    }
}
#[derive(Default)]
struct Free {
    causes: Vec<Cause>,
    ul_detail: ul::Detail,
    os_error: Option<i32>,
}
impl Free {
    fn of(causes: Vec<Cause>) -> Free {
        Free {
            causes,
            ul_detail: ul::Detail::None,
            os_error: None,
        }
    }
    fn errno(err: &Error) -> Free {
        let mut causes = vec![Cause::Ul(ul::Kind::Syscall)];
        for kind in [
            ul::Kind::PeerReset,
            ul::Kind::AddressInUse,
            ul::Kind::AddressNotAvailable,
            ul::Kind::NoBufferSpace,
        ] {
            if err.is_ul(kind) {
                causes.push(Cause::Ul(kind));
            }
        }
        Free {
            causes,
            ul_detail: ul::Detail::None,
            os_error: err.os_error(),
        }
    }
    fn error(self) -> Error {
        Error::bare_with(String::new(), self.causes)
            .with_ul_detail(self.ul_detail)
            .with_os_error(self.os_error)
    }
    fn after(mut self, first: Vec<Cause>) -> Free {
        let mut causes = first;
        causes.append(&mut self.causes);
        self.causes = causes;
        self
    }
}
fn value_free_connection_loss(err: &Error) -> Option<Free> {
    if let ul::Detail::Abort(source, reason) = err.ul_detail() {
        return Some(Free {
            causes: vec![Cause::Ul(ul::Kind::AssociationAborted)],
            ul_detail: ul::Detail::Abort(*source, *reason),
            os_error: None,
        });
    }
    if err.is_ul(ul::Kind::AssociationAborted) {
        return Some(Free::of(vec![Cause::Ul(ul::Kind::AssociationAborted)]));
    }
    if err.is_ul(ul::Kind::Syscall) {
        return Some(Free::errno(err));
    }
    if err.is_ul(ul::Kind::Eof) || err.is_ul(ul::Kind::UnexpectedEof) {
        return Some(Free::of(vec![Cause::Ul(ul::Kind::UnexpectedEof)]));
    }
    None
}
fn value_free_dial_cause(err: &Error) -> Option<Free> {
    if err.is_ul(ul::Kind::AssociationTimeout) {
        return Some(Free::of(vec![Cause::Ul(ul::Kind::AssociationTimeout)]));
    }
    if let ul::Detail::Tls(class) = err.ul_detail() {
        return Some(Free {
            causes: vec![Cause::Ul(ul::Kind::Other)],
            ul_detail: ul::Detail::Tls(*class),
            os_error: None,
        });
    }
    if err.is_ul(ul::Kind::NameLookup) {
        return Some(Free::of(vec![
            Cause::Ul(ul::Kind::Other),
            Cause::Ul(ul::Kind::NameLookup),
        ]));
    }
    if err.is_ul(ul::Kind::Syscall) {
        return Some(Free::errno(err));
    }
    if err.is_ul(ul::Kind::Eof) || err.is_ul(ul::Kind::UnexpectedEof) {
        return Some(Free::of(vec![Cause::Ul(ul::Kind::UnexpectedEof)]));
    }
    None
}
fn timed_out(err: &Error) -> bool {
    err.is_deadline() || err.is_ul(ul::Kind::AssociationTimeout) || err.is_ul(ul::Kind::NetTimeout)
}
fn uncertain_store_cause(err: &Error) -> Error {
    let uncertain = vec![Cause::Dimse(Kind::StoreUncertain)];
    if timed_out(err) {
        return Free::of(vec![
            Cause::Dimse(Kind::StoreUncertain),
            Cause::Dimse(Kind::StoreTimeout),
        ])
        .error();
    }
    match value_free_connection_loss(err) {
        Some(lost) => lost.after(uncertain).error(),
        None => Free::of(uncertain).error(),
    }
}
fn lost_association_cause(result: &StoreBatchResult, attempted: &[StorePlannedItem]) -> Error {
    let association = vec![Cause::Dimse(Kind::StoreAssociation)];
    for planned in attempted.iter().rev() {
        let index = planned.source_index;
        if index < 0 || index as usize >= result.items.len() {
            continue;
        }
        let item = &result.items[index as usize];
        let Some(err) = &item.err else { continue };
        if item.outcome != StoreOutcome::Unknown {
            continue;
        }
        if err.is(Kind::StoreResourceLimit) {
            return Free::of(association).error();
        }
        if err.is(Kind::StoreTimeout) {
            return Free::of(vec![
                Cause::Dimse(Kind::StoreAssociation),
                Cause::Dimse(Kind::StoreTimeout),
            ])
            .error();
        }
        if let Some(lost) = value_free_connection_loss(err) {
            return lost
                .after(vec![
                    Cause::Dimse(Kind::StoreAssociation),
                    Cause::Dimse(Kind::StoreUncertain),
                ])
                .error();
        }
        return Free::of(association).error();
    }
    Free::of(association).error()
}
#[doc(hidden)]
pub fn new_store_error(
    stage: &str,
    source_index: i64,
    cause: Option<&Error>,
    uncertain: bool,
) -> Error {
    let invalid = Error::sentinel(Kind::StoreInvalidSource);
    let cause = cause.unwrap_or(&invalid);
    let one = |kind: Kind| Free::of(vec![Cause::Dimse(kind)]);
    let mut inner = Detail::None;
    let safe: Free = if cause.is_canceled() {
        Free::of(vec![Cause::Ul(ul::Kind::Canceled)])
    } else if cause.is_deadline() {
        Free::of(vec![Cause::Ul(ul::Kind::DeadlineExceeded)])
    } else if cause.is(Kind::StoreAccessDenied) {
        one(Kind::StoreAccessDenied)
    } else if cause.is(Kind::StoreInvalidOptions) {
        one(Kind::StoreInvalidOptions)
    } else if cause.is(Kind::StoreResourceLimit) {
        one(Kind::StoreResourceLimit)
    } else if cause.is(Kind::StoreSourceChanged) {
        one(Kind::StoreSourceChanged)
    } else if cause.is(Kind::StoreTransferSyntaxPolicy) {
        one(Kind::StoreTransferSyntaxPolicy)
    } else if cause.is(Kind::StoreTransferSyntax) {
        one(Kind::StoreTransferSyntax)
    } else if cause.is(Kind::StorePresentationContextRejected) {
        if let Detail::MissingPresentationContext(sop, outcomes) = cause.detail() {
            inner = Detail::MissingPresentationContext(sop.clone(), outcomes.clone());
            one(Kind::StorePresentationContextRejected)
        } else if let ul::Detail::NoAccepted(outcomes) = cause.ul_detail() {
            Free {
                causes: vec![
                    Cause::Dimse(Kind::StorePresentationContextRejected),
                    Cause::Ul(ul::Kind::NoAcceptedPresentationContexts),
                ],
                ul_detail: ul::Detail::NoAccepted(
                    outcomes.iter().filter(|o| !o.accepted()).cloned().collect(),
                ),
                os_error: None,
            }
        } else {
            one(Kind::StorePresentationContextRejected)
        }
    } else if cause.is(Kind::StoreAssociation) {
        let association = vec![Cause::Dimse(Kind::StoreAssociation)];
        if let ul::Detail::Rejection(rj) = cause.ul_detail() {
            Free {
                causes: vec![
                    Cause::Dimse(Kind::StoreAssociation),
                    Cause::Ul(ul::Kind::AssociationRejected),
                ],
                ul_detail: ul::Detail::Rejection(*rj),
                os_error: None,
            }
        } else if cause.is(Kind::StoreTimeout) {
            Free::of(vec![
                Cause::Dimse(Kind::StoreAssociation),
                Cause::Dimse(Kind::StoreTimeout),
            ])
        } else if cause.is(Kind::StoreUncertain)
            && let Some(lost) = value_free_connection_loss(cause)
        {
            lost.after(association)
        } else if let Some(transport) = value_free_dial_cause(cause) {
            transport.after(association)
        } else {
            Free::of(association)
        }
    } else if cause.is(Kind::StoreRemoteFailure) {
        one(Kind::StoreRemoteFailure)
    } else if cause.is(Kind::StoreUncertain) {
        let base = vec![Cause::Dimse(Kind::StoreUncertain)];
        if cause.is(Kind::StoreTimeout) {
            Free::of(vec![
                Cause::Dimse(Kind::StoreUncertain),
                Cause::Dimse(Kind::StoreTimeout),
            ])
        } else if let Some(lost) = value_free_connection_loss(cause) {
            lost.after(base)
        } else {
            Free::of(base)
        }
    } else if cause.is(Kind::StoreSessionClosed) {
        one(Kind::StoreSessionClosed)
    } else if cause.is(Kind::StoreCallback) {
        one(Kind::StoreCallback)
    } else {
        one(Kind::StoreInvalidSource)
    };
    let text = if safe
        .causes
        .contains(&Cause::Dimse(Kind::StoreTransferSyntaxPolicy))
    {
        Kind::StoreTransferSyntaxPolicy.text().to_string()
    } else if stage.is_empty() {
        STORE_ERROR_NIL_TEXT.to_string()
    } else {
        format!("dicom dimse: C-STORE batch {stage} failed")
    };
    Error::bare_with(text, safe.causes)
        .with_ul_detail(safe.ul_detail)
        .with_os_error(safe.os_error)
        .with_detail(Detail::Store(
            stage.to_string(),
            source_index,
            uncertain,
            Box::new(inner),
        ))
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StoreLimits {
    pub max_items: i64,
    pub max_item_bytes: i64,
    pub max_total_bytes: i64,
    pub max_associations: i64,
    pub max_transfer_syntaxes: i64,
}
#[derive(Debug, Clone, Default)]
pub struct StorePlanOptions {
    pub limits: StoreLimits,
    pub presentation_contexts: Option<Vec<PresentationContext>>,
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StorePlannedItem {
    pub source_index: i64,
    pub descriptor: StoreDescriptor,
    pub presentation_context_id: u8,
}
#[derive(Debug, Clone, Default)]
pub struct StoreAssociationPlan {
    pub contexts: Vec<PresentationContext>,
    pub items: Vec<StorePlannedItem>,
}
#[derive(Debug, Clone)]
pub struct StorePlanFailure {
    pub source_index: i64,
    pub err: Error,
}
#[derive(Debug, Clone, Default)]
pub struct StorePlan {
    pub associations: Vec<StoreAssociationPlan>,
    pub failures: Vec<StorePlanFailure>,
    pub source_count: i64,
    pub total_bytes: i64,
}
pub(crate) fn normalize_store_limits(mut limits: StoreLimits) -> Result<StoreLimits> {
    if limits.max_items < 0
        || limits.max_item_bytes < 0
        || limits.max_total_bytes < 0
        || limits.max_associations < 0
        || limits.max_transfer_syntaxes < 0
    {
        return Err(Error::sentinel(Kind::StoreInvalidOptions));
    }
    for (value, default) in [
        (&mut limits.max_items, DEFAULT_STORE_MAX_ITEMS),
        (&mut limits.max_item_bytes, DEFAULT_STORE_MAX_ITEM_BYTES),
        (&mut limits.max_total_bytes, DEFAULT_STORE_MAX_TOTAL_BYTES),
        (&mut limits.max_associations, DEFAULT_STORE_MAX_ASSOCIATIONS),
        (
            &mut limits.max_transfer_syntaxes,
            DEFAULT_STORE_MAX_TRANSFER_SYNTAXES,
        ),
    ] {
        if *value == 0 {
            *value = default;
        }
    }
    Ok(limits)
}
fn clone_store_profile_contexts(
    contexts: Option<&[PresentationContext]>,
) -> Result<Option<Vec<PresentationContext>>> {
    let Some(contexts) = contexts else {
        return Ok(None);
    };
    let invalid = || Error::sentinel(Kind::StoreInvalidOptions);
    if contexts.is_empty() || contexts.len() > ul::MAX_PRESENTATION_CONTEXTS {
        return Err(invalid());
    }
    for (index, pc) in contexts.iter().enumerate() {
        if pc.id as usize != 2 * index + 1
            || !is_valid_uid(pc.abstract_syntax_uid.as_bytes())
            || pc.transfer_syntax_uids.is_empty()
            || pc.transfer_syntax_uids.len() > MAX_STORE_PROFILE_TRANSFER_SYNTAXES
        {
            return Err(invalid());
        }
        let mut wire = 8 + pc.abstract_syntax_uid.len();
        for (i, ts) in pc.transfer_syntax_uids.iter().enumerate() {
            if !is_valid_uid(ts.as_bytes()) || pc.transfer_syntax_uids[..i].contains(ts) {
                return Err(invalid());
            }
            wire += 4 + ts.len();
        }
        if wire > u16::MAX as usize {
            return Err(invalid());
        }
    }
    Ok(Some(contexts.to_vec()))
}
fn store_profile_context_id(
    contexts: &[PresentationContext],
    descriptor: &StoreDescriptor,
) -> Option<u8> {
    contexts
        .iter()
        .filter(|pc| pc.abstract_syntax_uid == descriptor.sop_class_uid)
        .find(|pc| {
            pc.transfer_syntax_uids
                .iter()
                .any(|offered| descriptor.writable_transfer_syntax_uids.contains(offered))
        })
        .map(|pc| pc.id)
}
pub fn plan_store_batch(
    ctx: Option<&Ctx>,
    sources: &[Option<&dyn StoreSource>],
    options: &StorePlanOptions,
) -> (StorePlan, Option<Error>) {
    plan_batch(ctx, sources, options, &[])
}
fn plan_batch(
    ctx: Option<&Ctx>,
    sources: &[Option<&dyn StoreSource>],
    options: &StorePlanOptions,
    allowlist: &[String],
) -> (StorePlan, Option<Error>) {
    let ctx = ctx.cloned().unwrap_or_default();
    let limits = match normalize_store_limits(options.limits) {
        Ok(l) => l,
        Err(e) => return (StorePlan::default(), Some(e)),
    };
    let mut fixed = match clone_store_profile_contexts(options.presentation_contexts.as_deref()) {
        Ok(f) => f,
        Err(e) => return (StorePlan::default(), Some(e)),
    };
    if !allowlist.is_empty()
        && let Some(fixed) = &mut fixed
    {
        for pc in fixed.iter_mut() {
            pc.transfer_syntax_uids =
                filter_transfer_syntax_uids(&pc.transfer_syntax_uids, allowlist);
            if pc.transfer_syntax_uids.is_empty() {
                return (
                    StorePlan::default(),
                    Some(Error::sentinel(Kind::StoreInvalidOptions)),
                );
            }
        }
    }
    let mut plan = StorePlan {
        source_count: sources.len() as i64,
        ..Default::default()
    };
    let limit = |index: i64| {
        Some(new_store_error(
            "plan",
            index,
            Some(&Error::sentinel(Kind::StoreResourceLimit)),
            false,
        ))
    };
    if sources.len() as i64 > limits.max_items {
        return (plan, limit(-1));
    }
    let invalid = Error::sentinel(Kind::StoreInvalidSource);
    let mut have_current = false;
    let mut context_ids: HashMap<String, u8> = HashMap::new();
    for (source_index, source) in sources.iter().enumerate() {
        let source_index = source_index as i64;
        if let Some(e) = context_err(&ctx) {
            return (plan, Some(e));
        }
        let fail = |plan: &mut StorePlan, stage: &str, cause: &Error| {
            plan.failures.push(StorePlanFailure {
                source_index,
                err: new_store_error(stage, source_index, Some(cause), false),
            });
        };
        let Some(source) = source else {
            fail(&mut plan, "inspect", &invalid);
            continue;
        };
        let descriptor = match inspect_store_source(&ctx, *source) {
            Ok(d) => d,
            Err(inspect_err) => {
                if let Some(e) = context_err(&ctx) {
                    return (plan, Some(e));
                }
                if inspect_err.is(Kind::StoreAccessDenied) {
                    return (
                        plan,
                        Some(new_store_error(
                            "authorize",
                            source_index,
                            Some(&Error::sentinel(Kind::StoreAccessDenied)),
                            false,
                        )),
                    );
                }
                fail(&mut plan, "inspect", &invalid);
                continue;
            }
        };
        let offered =
            filter_transfer_syntax_uids(&descriptor.writable_transfer_syntax_uids, allowlist);
        if offered.is_empty() {
            fail(
                &mut plan,
                "transfer syntax policy",
                &Error::sentinel(Kind::StoreTransferSyntaxPolicy),
            );
            continue;
        }
        let size = store_minimum_representation_size(&descriptor, &offered);
        if size > limits.max_item_bytes
            || size > i64::MAX - plan.total_bytes
            || plan.total_bytes + size > limits.max_total_bytes
        {
            return (plan, limit(source_index));
        }
        if descriptor.writable_transfer_syntax_uids.len() as i64 > limits.max_transfer_syntaxes {
            return (plan, limit(source_index));
        }
        plan.total_bytes += size;
        if let Some(fixed) = &fixed {
            let Some(pc_id) = store_profile_context_id(fixed, &descriptor) else {
                fail(
                    &mut plan,
                    "profile",
                    &Error::sentinel(Kind::StorePresentationContextRejected),
                );
                continue;
            };
            if !have_current {
                if plan.associations.len() as i64 >= limits.max_associations {
                    return (plan, limit(source_index));
                }
                plan.associations.push(StoreAssociationPlan {
                    contexts: fixed.clone(),
                    items: Vec::new(),
                });
                have_current = true;
            }
            plan.associations
                .last_mut()
                .unwrap()
                .items
                .push(StorePlannedItem {
                    source_index,
                    descriptor,
                    presentation_context_id: pc_id,
                });
            continue;
        }
        let key = format!("{}\0{}", descriptor.sop_class_uid, offered.join("\0"));
        let mut existing = context_ids.get(&key).copied();
        let full = plan
            .associations
            .last()
            .is_some_and(|a| a.contexts.len() == ul::MAX_PRESENTATION_CONTEXTS);
        if !have_current || (existing.is_none() && full) {
            if plan.associations.len() as i64 >= limits.max_associations {
                return (plan, limit(source_index));
            }
            plan.associations.push(StoreAssociationPlan::default());
            have_current = true;
            context_ids.clear();
            existing = None;
        }
        let current = plan.associations.last_mut().unwrap();
        let pc_id = match existing {
            Some(id) => id,
            None => {
                let id = (2 * current.contexts.len() + 1) as u8;
                context_ids.insert(key, id);
                current.contexts.push(PresentationContext {
                    id,
                    abstract_syntax_uid: descriptor.sop_class_uid.clone(),
                    transfer_syntax_uids: offered,
                });
                id
            }
        };
        current.items.push(StorePlannedItem {
            source_index,
            descriptor,
            presentation_context_id: pc_id,
        });
    }
    (plan, None)
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum StoreOutcome {
    #[default]
    Unset,
    NotSent,
    Success,
    Warning,
    Failure,
    Unknown,
    Canceled,
}
impl StoreOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            StoreOutcome::Unset => "",
            StoreOutcome::NotSent => "not_sent",
            StoreOutcome::Success => "success",
            StoreOutcome::Warning => "warning",
            StoreOutcome::Failure => "failure",
            StoreOutcome::Unknown => "unknown",
            StoreOutcome::Canceled => "canceled",
        }
    }
}
#[derive(Debug, Clone, Default)]
pub struct StoreItemResult {
    pub association_id: String,
    pub source_index: i64,
    pub descriptor: StoreDescriptor,
    pub association_index: i64,
    pub attempt: i64,
    pub attempted: bool,
    pub status: u16,
    pub status_set: bool,
    pub negotiated_transfer_syntax_uid: String,
    pub outcome: StoreOutcome,
    pub err: Option<Error>,
}
#[derive(Debug, Clone, Default)]
pub struct StoreBatchResult {
    pub plan: StorePlan,
    pub items: Vec<StoreItemResult>,
    pub associations: i64,
    pub succeeded: i64,
    pub warnings: i64,
    pub failed: i64,
    pub unknown: i64,
    pub canceled: i64,
    pub complete: bool,
    pub release_failures: i64,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StoreProgress {
    pub association_id: String,
    pub source_index: i64,
    pub completed: i64,
    pub total: i64,
    pub association_index: i64,
    pub attempt: i64,
    pub outcome: StoreOutcome,
    pub status: u16,
    pub status_set: bool,
}
pub type StoreProgressHandler = Arc<dyn Fn(&Ctx, &StoreProgress) -> Result<()> + Send + Sync>;
pub struct StorePreparedObservation {
    pub association_id: String,
    pub source_index: i64,
    pub association_index: i64,
    pub attempt: i64,
    pub descriptor: StoreDescriptor,
    pub negotiated_transfer_syntax_uid: String,
    pub read_metadata: Option<StoreMetadataReader>,
}
pub type StorePreparedObserver = Arc<dyn Fn(&Ctx, &StorePreparedObservation) + Send + Sync>;
pub type StoreResponseObserver = Arc<dyn Fn(&Ctx, &StoreProgress) + Send + Sync>;
pub type StoreAssociationObserver = Arc<dyn Fn(&Ctx, &Association, i64) + Send + Sync>;
#[derive(Clone, Default)]
pub struct StoreSessionOptions {
    pub dial_options: DialOptions,
    pub plan_options: StorePlanOptions,
    pub continue_on_error: bool,
    pub disable_reconnect: bool,
    pub retry_uncertain: bool,
    pub max_association_attempts: i64,
    pub max_store_attempts: i64,
    pub max_in_flight_bytes: i64,
    pub max_invoked_operations: i64,
    pub priority: u16,
    pub move_originator_ae_title: String,
    pub move_originator_message_id: Option<u16>,
    pub release_timeout: Nanos,
    pub cleanup_timeout: Nanos,
    pub on_progress: Option<StoreProgressHandler>,
    pub on_store_prepared: Option<StorePreparedObserver>,
    pub on_store_response: Option<StoreResponseObserver>,
    pub on_association_started: Option<StoreAssociationObserver>,
    pub on_association_finished: Option<StoreAssociationObserver>,
}
#[derive(Default)]
struct Active {
    closed: bool,
    assoc: Option<Arc<Association>>,
    ctx: Option<Ctx>,
    index: i64,
}
pub struct StoreSession {
    address: String,
    options: StoreSessionOptions,
    run_busy: Mutex<bool>,
    run_free: Condvar,
    active: Mutex<Active>,
}
fn timeout_of(d: Nanos) -> Duration {
    d.positive().unwrap_or(Duration::ZERO)
}
pub fn new_store_session(address: &str, mut options: StoreSessionOptions) -> Result<StoreSession> {
    let invalid = || Error::sentinel(Kind::StoreInvalidOptions);
    if address.is_empty() {
        return Err(invalid());
    }
    options.plan_options.presentation_contexts =
        clone_store_profile_contexts(options.plan_options.presentation_contexts.as_deref())?;
    if options.priority > 2
        || options.max_association_attempts < 0
        || options.max_store_attempts < 0
        || options.max_in_flight_bytes < 0
        || options.max_invoked_operations < 0
        || options.release_timeout.is_negative()
        || options.cleanup_timeout.is_negative()
    {
        return Err(invalid());
    }
    if options.max_association_attempts == 0 {
        options.max_association_attempts = 1;
    }
    if options.max_store_attempts == 0 {
        options.max_store_attempts = 1;
    }
    if options.release_timeout.is_zero() {
        options.release_timeout = Nanos::from_secs(5);
    }
    if options.cleanup_timeout.is_zero() {
        options.cleanup_timeout = Nanos::from_secs(1);
    }
    options.plan_options.limits = normalize_store_limits(options.plan_options.limits)?;
    Ok(StoreSession {
        address: address.to_string(),
        options,
        run_busy: Mutex::new(false),
        run_free: Condvar::new(),
        active: Mutex::new(Active::default()),
    })
}
fn store_batch_complete(items: &[StoreItemResult]) -> bool {
    items.iter().all(|i| i.outcome != StoreOutcome::NotSent)
}
fn store_context_accepted(assoc: &Association, planned: &StorePlannedItem) -> bool {
    planned
        .descriptor
        .writable_transfer_syntax_uids
        .iter()
        .any(|ts| {
            assoc.accepted_contexts.iter().any(|pc| {
                pc.abstract_syntax_uid == planned.descriptor.sop_class_uid
                    && pc.transfer_syntax_uid == *ts
            })
        })
}
fn effective_store_invoked(
    opt_in: bool,
    configured: i64,
    negotiated: AsynchronousOperationsWindow,
) -> i64 {
    if !opt_in || configured <= 1 {
        return 1;
    }
    if negotiated.maximum_invoked == 0 {
        return configured;
    }
    let limit = negotiated.maximum_invoked as i64;
    if limit <= 1 {
        return 1;
    }
    configured.min(limit)
}
fn store_release_error() -> Error {
    new_store_error(
        "release",
        -1,
        Some(&Error::sentinel(Kind::StoreAssociation)),
        false,
    )
}
struct Selected {
    opened: Option<OpenedStoreSource>,
    pc: Option<AcceptedContext>,
    syntax: Option<Syntax>,
    err: Option<Error>,
}
struct OnceCloser {
    state: Mutex<(Option<StoreSourceCloser>, Option<Result<()>>)>,
}
impl OnceCloser {
    fn new(close: Option<StoreSourceCloser>) -> Arc<OnceCloser> {
        Arc::new(OnceCloser {
            state: Mutex::new((close, None)),
        })
    }
    fn close(&self) -> Result<()> {
        let mut st = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(done) = &st.1 {
            return done.clone();
        }
        let result = close_store_source(st.0.take());
        st.1 = Some(result.clone());
        result
    }
}
struct PipelineCompletion {
    source_index: i64,
    syntax_uid: String,
    response: Option<CStoreResponse>,
    err: Option<Error>,
    close_err: Option<Error>,
    uncertain: bool,
}
struct PipelineFlight {
    reserved: i64,
    close: Arc<OnceCloser>,
}
struct Piped {
    position: usize,
    completed: i64,
    usable: bool,
    err: Option<Error>,
}
impl StoreSession {
    fn active(&self) -> std::sync::MutexGuard<'_, Active> {
        self.active.lock().unwrap_or_else(|e| e.into_inner())
    }
    fn acquire_run(&self, ctx: &Ctx) -> Result<()> {
        let mut busy = self.run_busy.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            if !*busy {
                *busy = true;
                return Ok(());
            }
            if let Some(e) = context_err(ctx) {
                return Err(e);
            }
            busy = self
                .run_free
                .wait_timeout(busy, Duration::from_millis(5))
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
    }
    fn release_run(&self) {
        *self.run_busy.lock().unwrap_or_else(|e| e.into_inner()) = false;
        self.run_free.notify_one();
    }
    fn is_closed(&self) -> bool {
        self.active().closed
    }
    fn context_or_closed(&self, ctx: &Ctx) -> Option<Error> {
        if let Some(e) = context_err(ctx) {
            return Some(e);
        }
        if self.is_closed() {
            return Some(Error::sentinel(Kind::StoreSessionClosed));
        }
        None
    }
    fn set_active(&self, ctx: &Ctx, assoc: &Arc<Association>, index: i64) {
        {
            let mut a = self.active();
            a.assoc = Some(assoc.clone());
            a.ctx = Some(ctx.clone());
            a.index = index;
        }
        observe_store_association(&self.options.on_association_started, ctx, assoc, index);
    }
    fn clear_active(&self, assoc: &Arc<Association>) {
        let (ctx, index) = {
            let mut a = self.active();
            let active = a.assoc.as_ref().is_some_and(|x| Arc::ptr_eq(x, assoc));
            if !active {
                return;
            }
            a.assoc = None;
            (a.ctx.take().unwrap_or_default(), a.index)
        };
        observe_store_association(
            &self.options.on_association_finished,
            &ctx.without_cancel(),
            assoc,
            index,
        );
    }
    fn release_association(&self, assoc: &Association) -> Result<()> {
        let ctx = Ctx::background().with_timeout(timeout_of(self.options.release_timeout));
        assoc.release(Some(&ctx)).map_err(Error::from)
    }
    fn abort_association(&self, assoc: &Association, cause: Option<&Error>) {
        let ctx = Ctx::background().with_timeout(timeout_of(self.options.cleanup_timeout));
        let _ = match cause {
            Some(e) => assoc.abort_for_error(Some(&ctx), &e.to_ul()),
            None => assoc.abort_with_context(Some(&ctx), ul::ABORT_REASON_NOT_SPECIFIED),
        };
    }
    fn cancel_all_not_sent(&self, result: &mut StoreBatchResult, cause: &Error) {
        for association in &result.plan.associations {
            for planned in &association.items {
                result.items[planned.source_index as usize].descriptor = planned.descriptor.clone();
            }
        }
        for item in result.items.iter_mut() {
            if item.outcome != StoreOutcome::NotSent {
                continue;
            }
            item.outcome = StoreOutcome::Canceled;
            item.err = Some(new_store_error(
                "cancel",
                item.source_index,
                Some(cause),
                false,
            ));
            result.canceled += 1;
        }
    }
    fn report_progress(
        &self,
        ctx: &Ctx,
        item: &StoreItemResult,
        completed: i64,
        total: i64,
    ) -> Option<Error> {
        let handler = self.options.on_progress.as_ref()?;
        let progress = StoreProgress {
            association_id: item.association_id.clone(),
            source_index: item.source_index,
            completed,
            total,
            association_index: item.association_index,
            attempt: item.attempt,
            outcome: item.outcome,
            status: item.status,
            status_set: item.status_set,
        };
        let callback = || {
            new_store_error(
                "progress",
                item.source_index,
                Some(&Error::sentinel(Kind::StoreCallback)),
                false,
            )
        };
        let returned = catch_unwind(AssertUnwindSafe(|| handler(ctx, &progress)));
        if let Some(e) = context_err(ctx) {
            return Some(e);
        }
        match returned {
            Ok(Ok(())) => None,
            _ => Some(callback()),
        }
    }
    fn observe_store_prepared(
        &self,
        ctx: &Ctx,
        item: &StoreItemResult,
        opened: &OpenedStoreSource,
        syntax_uid: &str,
    ) {
        let Some(observer) = &self.options.on_store_prepared else {
            return;
        };
        let live = Arc::new(AtomicBool::new(true));
        let read_metadata = opened.read_metadata.clone().map(|read| {
            let live = live.clone();
            let wrapped: StoreMetadataReader =
                Arc::new(move |ctx: &Ctx, options: index::Options| {
                    if !live.load(Ordering::SeqCst) {
                        return Err(Error::sentinel(Kind::StoreInvalidSource));
                    }
                    read(ctx, options)
                });
            wrapped
        });
        let observation = StorePreparedObservation {
            association_id: item.association_id.clone(),
            source_index: item.source_index,
            association_index: item.association_index,
            attempt: item.attempt,
            descriptor: item.descriptor.clone(),
            negotiated_transfer_syntax_uid: syntax_uid.to_string(),
            read_metadata,
        };
        let _ = catch_unwind(AssertUnwindSafe(|| observer(ctx, &observation)));
        live.store(false, Ordering::SeqCst);
    }
    fn observe_store_response(&self, ctx: &Ctx, item: &StoreItemResult, status: u16) {
        let Some(observer) = &self.options.on_store_response else {
            return;
        };
        let outcome = if status == STATUS_SUCCESS {
            StoreOutcome::Success
        } else if is_c_store_warning_status(status) {
            StoreOutcome::Warning
        } else {
            StoreOutcome::Failure
        };
        let progress = StoreProgress {
            association_id: item.association_id.clone(),
            source_index: item.source_index,
            completed: 0,
            total: 0,
            association_index: item.association_index,
            attempt: item.attempt,
            outcome,
            status,
            status_set: true,
        };
        let ctx = ctx.without_cancel();
        let _ = catch_unwind(AssertUnwindSafe(|| observer(&ctx, &progress)));
    }
    fn dial_association(
        &self,
        ctx: &Ctx,
        contexts: &[PresentationContext],
    ) -> (Option<Arc<Association>>, i64, Option<Error>) {
        let mut last = None;
        for attempt in 1..=self.options.max_association_attempts {
            if let Some(e) = self.context_or_closed(ctx) {
                return (None, attempt, Some(e));
            }
            let mut opts = self.options.dial_options.clone();
            opts.context = Some(ctx.clone());
            opts.contexts = contexts.to_vec();
            opts.common_extended_negotiation =
                common_extended_negotiation_for(&opts.common_extended_negotiation, &opts.contexts);
            if self.options.max_invoked_operations > 1
                && opts.asynchronous_operations_window.is_none()
            {
                opts.asynchronous_operations_window = Some(AsynchronousOperationsWindow {
                    maximum_invoked: self.options.max_invoked_operations as u16,
                    maximum_performed: 1,
                });
            }
            match ul::dial_context(ctx, &self.address, opts) {
                Ok(assoc) => return (Some(Arc::new(assoc)), attempt, None),
                Err(e) => last = Some(Error::from(e)),
            }
        }
        (None, self.options.max_association_attempts, last)
    }
    fn selected_store_representation(
        &self,
        ctx: &Ctx,
        source: Option<&dyn StoreSource>,
        assoc: &Association,
        descriptor: &StoreDescriptor,
        remaining_total: i64,
    ) -> Selected {
        let mut selected = Selected {
            opened: None,
            pc: None,
            syntax: None,
            err: None,
        };
        let pc = match accepted_context_for_sop_class_transfer_syntaxes(
            assoc,
            &descriptor.sop_class_uid,
            &descriptor.writable_transfer_syntax_uids,
        ) {
            Ok(pc) => pc,
            Err(e) => {
                selected.err = Some(e);
                return selected;
            }
        };
        selected.pc = Some(pc.clone());
        let bound = store_representation_size(descriptor, &pc.transfer_syntax_uid);
        if bound > self.options.plan_options.limits.max_item_bytes || bound > remaining_total {
            selected.err = Some(Error::sentinel(Kind::StoreResourceLimit));
            return selected;
        }
        let syntax = match transfer_syntax_for_accepted_context(&pc) {
            Ok(s) => s,
            Err(e) => {
                selected.err = Some(e);
                return selected;
            }
        };
        selected.syntax = Some(syntax);
        match open_store_source_for_syntax(ctx, source, &syntax) {
            Ok(opened) => selected.opened = Some(opened),
            Err(e) => selected.err = Some(e),
        }
        selected
    }
    fn item_limit(&self, representation_bound: i64) -> i64 {
        let limit = self.options.plan_options.limits.max_item_bytes;
        if representation_bound > 0 {
            limit.min(representation_bound)
        } else {
            limit
        }
    }
    pub fn store(
        &self,
        ctx: Option<&Ctx>,
        source: Option<&dyn StoreSource>,
    ) -> (StoreItemResult, Option<Error>) {
        let (mut result, err) = self.store_batch(ctx, &[source]);
        if result.items.is_empty() {
            return (StoreItemResult::default(), err);
        }
        let item = result.items.remove(0);
        let err = err.or_else(|| item.err.clone());
        (item, err)
    }
    pub fn store_path(&self, ctx: Option<&Ctx>, path: &str) -> (StoreItemResult, Option<Error>) {
        let source = new_path_store_source(path, None);
        self.store(ctx, Some(source.as_ref()))
    }
    pub fn close(&self, ctx: Option<&Ctx>) -> Result<()> {
        let ctx = ctx.cloned().unwrap_or_default();
        self.acquire_run(&ctx)?;
        self.active().closed = true;
        self.release_run();
        Ok(())
    }
    pub fn abort(&self, ctx: Option<&Ctx>) -> Result<()> {
        let ctx = ctx.cloned().unwrap_or_default();
        let assoc = {
            let mut a = self.active();
            a.closed = true;
            a.assoc.clone()
        };
        match assoc {
            None => Ok(()),
            Some(assoc) => assoc
                .abort_with_context(Some(&ctx), ul::ABORT_REASON_NOT_SPECIFIED)
                .map_err(Error::from),
        }
    }
    pub fn store_batch(
        &self,
        ctx: Option<&Ctx>,
        sources: &[Option<&dyn StoreSource>],
    ) -> (StoreBatchResult, Option<Error>) {
        let ctx = ctx.cloned().unwrap_or_default();
        if let Err(e) = self.acquire_run(&ctx) {
            return (StoreBatchResult::default(), Some(e));
        }
        let (mut result, err) = if self.is_closed() {
            self.release_run();
            return (
                StoreBatchResult::default(),
                Some(Error::sentinel(Kind::StoreSessionClosed)),
            );
        } else {
            self.run_batch(&ctx, sources)
        };
        result.complete = store_batch_complete(&result.items);
        self.release_run();
        (result, err)
    }
    fn run_batch(
        &self,
        ctx: &Ctx,
        sources: &[Option<&dyn StoreSource>],
    ) -> (StoreBatchResult, Option<Error>) {
        let total = sources.len() as i64;
        let (plan, plan_err) = plan_batch(
            Some(ctx),
            sources,
            &self.options.plan_options,
            &self.options.dial_options.transfer_syntax_allowlist,
        );
        let items = (0..plan.source_count)
            .map(|i| StoreItemResult {
                source_index: i,
                association_index: -1,
                outcome: StoreOutcome::NotSent,
                ..Default::default()
            })
            .collect();
        let mut result = StoreBatchResult {
            plan,
            items,
            ..Default::default()
        };
        if let Some(e) = plan_err {
            if e.is_canceled() || e.is_deadline() {
                self.cancel_all_not_sent(&mut result, &e);
            }
            return (result, Some(e));
        }
        let mut completed: i64 = 0;
        for i in 0..result.plan.failures.len() {
            let failure = result.plan.failures[i].clone();
            let index = failure.source_index as usize;
            result.items[index].outcome = StoreOutcome::Failure;
            result.items[index].err = Some(failure.err);
            result.failed += 1;
            completed += 1;
            let item = result.items[index].clone();
            if let Some(progress_err) = self.report_progress(ctx, &item, completed, total) {
                if let Some(e) = context_err(ctx) {
                    self.cancel_all_not_sent(&mut result, &e);
                }
                return (result, Some(progress_err));
            }
        }
        if !result.plan.failures.is_empty() && !self.options.continue_on_error {
            let err = result.plan.failures[0].err.clone();
            return (result, Some(err));
        }
        let mut remaining_total = self.options.plan_options.limits.max_total_bytes;
        let mut dead_associations: i64 = 0;
        let plans = result.plan.associations.clone();
        for (association_index, association_plan) in plans.iter().enumerate() {
            let association_index = association_index as i64;
            let mut position = 0usize;
            while position < association_plan.items.len() {
                if let Some(e) = self.context_or_closed(ctx) {
                    self.cancel_all_not_sent(&mut result, &e);
                    return (result, Some(e));
                }
                let (assoc, dial_attempt, dial_err) =
                    self.dial_association(ctx, &association_plan.contexts);
                let Some(assoc) = assoc else {
                    if let Some(e) = self.context_or_closed(ctx) {
                        self.cancel_all_not_sent(&mut result, &e);
                        return (result, Some(e));
                    }
                    let dial_err =
                        dial_err.unwrap_or_else(|| Error::sentinel(Kind::StoreAssociation));
                    for planned in &association_plan.items[position..] {
                        let index = planned.source_index as usize;
                        let kind = if dial_err.is_ul(ul::Kind::NoAcceptedPresentationContexts) {
                            Kind::StorePresentationContextRejected
                        } else {
                            Kind::StoreAssociation
                        };
                        let cause = Error::wrap_err(kind, dial_err.clone());
                        let item = &mut result.items[index];
                        item.descriptor = planned.descriptor.clone();
                        item.association_index = association_index;
                        item.attempt = dial_attempt;
                        item.outcome = StoreOutcome::Failure;
                        item.err = Some(new_store_error(
                            "associate",
                            planned.source_index,
                            Some(&cause),
                            false,
                        ));
                        result.failed += 1;
                        completed += 1;
                        let item = result.items[index].clone();
                        if let Some(progress_err) =
                            self.report_progress(ctx, &item, completed, total)
                        {
                            if let Some(e) = context_err(ctx) {
                                self.cancel_all_not_sent(&mut result, &e);
                            }
                            return (result, Some(progress_err));
                        }
                    }
                    if !self.options.continue_on_error {
                        let index = association_plan.items[position].source_index as usize;
                        let err = result.items[index].err.clone();
                        return (result, err);
                    }
                    break;
                };
                result.associations += 1;
                self.set_active(ctx, &assoc, association_index);
                let mut association_usable = true;
                let succeeded_at_dial = result.succeeded;
                let mut stop_reconnecting = |succeeded: i64| -> bool {
                    if succeeded != succeeded_at_dial {
                        dead_associations = 0;
                        return false;
                    }
                    dead_associations += 1;
                    dead_associations >= MAX_DEAD_STORE_ASSOCIATIONS
                };
                let invoked = effective_store_invoked(
                    self.options.max_invoked_operations > 1,
                    self.options.max_invoked_operations,
                    assoc.effective_asynchronous_operations_window(),
                );
                if invoked > 1 {
                    let piped = self.store_association_pipelined(
                        ctx,
                        &assoc,
                        invoked,
                        association_plan,
                        &mut result,
                        sources,
                        association_index,
                        position,
                        completed,
                        &mut remaining_total,
                    );
                    position = piped.position;
                    completed = piped.completed;
                    if piped.err.is_some() {
                        return (result, piped.err);
                    }
                    if !piped.usable
                        && (stop_reconnecting(result.succeeded)
                            || self.options.disable_reconnect
                            || position >= association_plan.items.len())
                    {
                        for remaining in &association_plan.items[position..] {
                            let index = remaining.source_index as usize;
                            if result.items[index].outcome != StoreOutcome::NotSent {
                                continue;
                            }
                            let cause = lost_association_cause(
                                &result,
                                &association_plan.items[..position],
                            );
                            let item = &mut result.items[index];
                            item.descriptor = remaining.descriptor.clone();
                            item.outcome = StoreOutcome::Failure;
                            item.err = Some(new_store_error(
                                "reconnect",
                                remaining.source_index,
                                Some(&cause),
                                false,
                            ));
                            result.failed += 1;
                            completed += 1;
                        }
                        position = association_plan.items.len();
                    }
                    continue;
                }
                let mut client = StoreClient::new(&assoc);
                while position < association_plan.items.len() {
                    let planned = &association_plan.items[position];
                    let index = planned.source_index as usize;
                    {
                        let item = &mut result.items[index];
                        item.descriptor = planned.descriptor.clone();
                        item.association_index = association_index;
                        item.association_id = assoc.id().to_string();
                        item.attempt += 1;
                    }
                    if let Some(e) = self.context_or_closed(ctx) {
                        self.abort_association(&assoc, None);
                        self.clear_active(&assoc);
                        self.cancel_all_not_sent(&mut result, &e);
                        return (result, Some(e));
                    }
                    if !store_context_accepted(&assoc, planned) {
                        let cause = Error::wrap_err(
                            Kind::StorePresentationContextRejected,
                            explain_missing_presentation_context(
                                &assoc,
                                &planned.descriptor.sop_class_uid,
                            ),
                        );
                        let item = &mut result.items[index];
                        item.outcome = StoreOutcome::Failure;
                        item.err = Some(new_store_error(
                            "negotiate",
                            planned.source_index,
                            Some(&cause),
                            false,
                        ));
                        result.failed += 1;
                        completed += 1;
                        position += 1;
                        let item = result.items[index].clone();
                        if let Some(e) = self.report_progress(ctx, &item, completed, total) {
                            let _ = self.release_association(&assoc);
                            self.clear_active(&assoc);
                            if let Some(c) = context_err(ctx) {
                                self.cancel_all_not_sent(&mut result, &c);
                            }
                            return (result, Some(e));
                        }
                        if !self.options.continue_on_error {
                            let _ = self.release_association(&assoc);
                            self.clear_active(&assoc);
                            return (result, item.err);
                        }
                        continue;
                    }
                    let mut selected = self.selected_store_representation(
                        ctx,
                        sources[index],
                        &assoc,
                        &planned.descriptor,
                        remaining_total,
                    );
                    let selected_ts = selected
                        .pc
                        .as_ref()
                        .map(|pc| pc.transfer_syntax_uid.clone())
                        .unwrap_or_default();
                    let mut open_err = selected.err.take();
                    if open_err.is_none()
                        && let Some(opened) = &selected.opened
                    {
                        open_err = validate_opened_store_source(&planned.descriptor, opened).err();
                    }
                    if open_err.is_none()
                        && let Some(opened) = &selected.opened
                    {
                        let item = result.items[index].clone();
                        self.observe_store_prepared(ctx, &item, opened, &selected_ts);
                        open_err = check_store_access(ctx, opened.before_store.as_deref()).err();
                    }
                    if let Some(open_err) = open_err {
                        let had_before = selected
                            .opened
                            .as_ref()
                            .is_some_and(|o| o.before_store.is_some());
                        if let Some(opened) = &mut selected.opened
                            && opened.close.is_some()
                        {
                            let _ = close_store_source(opened.close.take());
                        }
                        if open_err.is(Kind::StoreAccessDenied) {
                            let err = new_store_error(
                                "authorize",
                                planned.source_index,
                                Some(&Error::sentinel(Kind::StoreAccessDenied)),
                                false,
                            );
                            result.items[index].err = Some(err.clone());
                            let _ = self.release_association(&assoc);
                            self.clear_active(&assoc);
                            return (result, Some(err));
                        }
                        if let Some(e) = context_err(ctx)
                            && had_before
                        {
                            self.abort_association(&assoc, None);
                            self.clear_active(&assoc);
                            self.cancel_all_not_sent(&mut result, &e);
                            return (result, Some(e));
                        }
                        let item = &mut result.items[index];
                        item.outcome = StoreOutcome::Failure;
                        item.err = Some(new_store_error(
                            "open",
                            planned.source_index,
                            Some(&open_err),
                            false,
                        ));
                        result.failed += 1;
                        completed += 1;
                        position += 1;
                        let item = result.items[index].clone();
                        if let Some(e) = self.report_progress(ctx, &item, completed, total) {
                            let _ = self.release_association(&assoc);
                            self.clear_active(&assoc);
                            if let Some(c) = context_err(ctx) {
                                self.cancel_all_not_sent(&mut result, &c);
                            }
                            return (result, Some(e));
                        }
                        if !self.options.continue_on_error {
                            let _ = self.release_association(&assoc);
                            self.clear_active(&assoc);
                            return (result, item.err);
                        }
                        continue;
                    }
                    let mut opened = selected.opened.take().unwrap();
                    let write = opened.write_data_set.clone().unwrap();
                    result.items[index].attempted = true;
                    let mut item_left = self.item_limit(store_writer_representation_bound(
                        &planned.descriptor,
                        &selected_ts,
                    ));
                    let total_left = std::cell::Cell::new(remaining_total);
                    let item_left_cell = std::cell::Cell::new(item_left);
                    let stored = client.store_streamed(
                        Some(ctx),
                        Some(&|write_ctx: &Ctx,
                               sink: &mut dyn FnMut(&[u8]) -> Result<()>,
                               syntax: &Syntax| {
                            write(
                                write_ctx,
                                &mut |chunk: &[u8]| {
                                    let n = chunk.len() as i64;
                                    if n > item_left_cell.get() || n > total_left.get() {
                                        return Err(Error::sentinel(Kind::StoreResourceLimit));
                                    }
                                    sink(chunk)?;
                                    item_left_cell.set(item_left_cell.get() - n);
                                    total_left.set(total_left.get() - n);
                                    Ok(())
                                },
                                syntax,
                            )
                        }),
                        CStoreOptions {
                            affected_sop_class_uid: planned.descriptor.sop_class_uid.clone(),
                            affected_sop_instance_uid: planned.descriptor.sop_instance_uid.clone(),
                            transfer_syntax_uids: vec![selected_ts.clone()],
                            priority: self.options.priority,
                            move_originator_ae_title: self.options.move_originator_ae_title.clone(),
                            move_originator_message_id: self.options.move_originator_message_id,
                            ..Default::default()
                        },
                        true,
                        None,
                    );
                    item_left = item_left_cell.get();
                    let _ = item_left;
                    remaining_total = total_left.get();
                    let (store_result, store_err) = match stored {
                        Ok(r) => (r, None),
                        Err((r, e)) => (r, Some(e)),
                    };
                    if let Some(response) = &store_result.response {
                        let item = result.items[index].clone();
                        self.observe_store_response(ctx, &item, response.status);
                    }
                    let close_err = close_store_source(opened.close.take()).err();
                    {
                        let item = &mut result.items[index];
                        item.negotiated_transfer_syntax_uid = store_result
                            .presentation_context
                            .transfer_syntax_uid
                            .clone();
                        if let Some(response) = &store_result.response {
                            item.status = response.status;
                            item.status_set = true;
                        }
                    }
                    let uncertain = store_err
                        .as_ref()
                        .is_some_and(|e| e.is(Kind::AssociationStateUncertain));
                    if uncertain
                        && !self.options.disable_reconnect
                        && self.options.retry_uncertain
                        && result.items[index].attempt < self.options.max_store_attempts
                    {
                        association_usable = false;
                        self.abort_association(&assoc, store_err.as_ref());
                        self.clear_active(&assoc);
                        let item = &mut result.items[index];
                        item.outcome = StoreOutcome::NotSent;
                        item.err = None;
                        break;
                    }
                    {
                        let status = result.items[index].status;
                        let (outcome, err) = match (&store_err, uncertain) {
                            (Some(e), true) => {
                                let cause = if e.is(Kind::StoreResourceLimit) {
                                    Error::sentinel(Kind::StoreResourceLimit)
                                } else {
                                    uncertain_store_cause(e)
                                };
                                result.unknown += 1;
                                association_usable = false;
                                (
                                    StoreOutcome::Unknown,
                                    Some(new_store_error(
                                        "transfer",
                                        planned.source_index,
                                        Some(&cause),
                                        true,
                                    )),
                                )
                            }
                            (Some(_), false) => {
                                result.failed += 1;
                                (
                                    StoreOutcome::Failure,
                                    Some(new_store_error(
                                        "response",
                                        planned.source_index,
                                        Some(&Error::sentinel(Kind::StoreRemoteFailure)),
                                        false,
                                    )),
                                )
                            }
                            (None, _) if close_err.is_some() => {
                                result.failed += 1;
                                (
                                    StoreOutcome::Failure,
                                    Some(new_store_error(
                                        "close",
                                        planned.source_index,
                                        Some(&Error::sentinel(Kind::StoreInvalidSource)),
                                        false,
                                    )),
                                )
                            }
                            (None, _) if is_c_store_warning_status(status) => {
                                result.succeeded += 1;
                                result.warnings += 1;
                                (StoreOutcome::Warning, None)
                            }
                            (None, _) => {
                                result.succeeded += 1;
                                (StoreOutcome::Success, None)
                            }
                        };
                        let item = &mut result.items[index];
                        item.outcome = outcome;
                        item.err = err;
                    }
                    completed += 1;
                    position += 1;
                    if let Some(e) = context_err(ctx)
                        && (store_result.response.is_none() || uncertain)
                    {
                        self.abort_association(&assoc, store_err.as_ref());
                        self.clear_active(&assoc);
                        self.cancel_all_not_sent(&mut result, &e);
                        return (result, Some(e));
                    }
                    let item = result.items[index].clone();
                    let progress_err = self
                        .report_progress(ctx, &item, completed, total)
                        .or_else(|| context_err(ctx));
                    if let Some(progress_err) = progress_err {
                        let _ = self.release_association(&assoc);
                        self.clear_active(&assoc);
                        if let Some(c) = context_err(ctx) {
                            self.cancel_all_not_sent(&mut result, &c);
                        }
                        return (result, Some(progress_err));
                    }
                    if item.err.is_some() && !self.options.continue_on_error {
                        if association_usable {
                            let _ = self.release_association(&assoc);
                        } else {
                            self.abort_association(&assoc, store_err.as_ref());
                        }
                        self.clear_active(&assoc);
                        return (result, item.err);
                    }
                    if !association_usable {
                        self.abort_association(&assoc, store_err.as_ref());
                        self.clear_active(&assoc);
                        if stop_reconnecting(result.succeeded)
                            || self.options.disable_reconnect
                            || position >= association_plan.items.len()
                        {
                            for remaining in &association_plan.items[position..] {
                                let cause = lost_association_cause(
                                    &result,
                                    &association_plan.items[..position],
                                );
                                let item = &mut result.items[remaining.source_index as usize];
                                item.descriptor = remaining.descriptor.clone();
                                item.outcome = StoreOutcome::Failure;
                                item.err = Some(new_store_error(
                                    "reconnect",
                                    remaining.source_index,
                                    Some(&cause),
                                    false,
                                ));
                                result.failed += 1;
                                completed += 1;
                            }
                            position = association_plan.items.len();
                        }
                        break;
                    }
                }
                if association_usable {
                    let release_err = self.release_association(&assoc).err();
                    self.clear_active(&assoc);
                    if release_err.is_some() {
                        result.release_failures += 1;
                        if !self.options.continue_on_error {
                            return (result, Some(store_release_error()));
                        }
                    }
                }
            }
        }
        if result.release_failures > 0 {
            return (result, Some(store_release_error()));
        }
        (result, None)
    }
    fn release_pipelined_session(&self, session: &AsyncSession) -> Result<()> {
        let ctx = Ctx::background().with_timeout(timeout_of(self.options.release_timeout));
        session.release(Some(&ctx), AsyncReleasePolicy::Wait)
    }
    fn abort_pipelined_session(&self, session: &AsyncSession) {
        let ctx = Ctx::background().with_timeout(timeout_of(self.options.cleanup_timeout));
        let _ = session.abort(Some(&ctx));
    }
    #[allow(clippy::too_many_arguments)]
    fn store_association_pipelined(
        &self,
        ctx: &Ctx,
        assoc: &Arc<Association>,
        effective_invoked: i64,
        association_plan: &StoreAssociationPlan,
        result: &mut StoreBatchResult,
        sources: &[Option<&dyn StoreSource>],
        association_index: i64,
        mut position: usize,
        mut completed: i64,
        remaining_total: &mut i64,
    ) -> Piped {
        let total = sources.len() as i64;
        let session = match new_async_session(
            Some(assoc.clone()),
            AsyncSessionOptions {
                max_invoked_operations: effective_invoked,
                ..Default::default()
            },
        ) {
            Ok(s) => s,
            Err(e) => {
                self.abort_association(assoc, Some(&e));
                self.clear_active(assoc);
                return Piped {
                    position,
                    completed,
                    usable: false,
                    err: Some(new_store_error("associate", -1, Some(&e), false)),
                };
            }
        };
        let (tx, rx) = mpsc::channel::<PipelineCompletion>();
        let mut workers: Vec<std::thread::JoinHandle<()>> = Vec::new();
        let mut in_flight: BTreeMap<i64, PipelineFlight> = BTreeMap::new();
        let mut bytes_in_flight: i64 = 0;
        let mut stop_admit = false;
        let mut usable = true;
        let mut access_err: Option<Error> = None;
        macro_rules! drain_responses {
            () => {{
                for worker in workers.drain(..) {
                    let _ = worker.join();
                }
                while let Ok(completion) = rx.try_recv() {
                    if let Some(response) = &completion.response {
                        let item = result.items[completion.source_index as usize].clone();
                        self.observe_store_response(ctx, &item, response.status);
                    }
                }
            }};
        }
        macro_rules! abort_in_flight {
            ($cause:expr) => {{
                let cause: Error = $cause;
                for (source_index, flight) in std::mem::take(&mut in_flight) {
                    let item = &mut result.items[source_index as usize];
                    if matches!(item.outcome, StoreOutcome::NotSent | StoreOutcome::Unset) {
                        item.outcome = StoreOutcome::Unknown;
                        item.err = Some(new_store_error(
                            "transfer",
                            source_index,
                            Some(&uncertain_store_cause(&cause)),
                            true,
                        ));
                        result.unknown += 1;
                        completed += 1;
                    }
                    let _ = flight.close.close();
                }
                self.abort_pipelined_session(&session);
                drain_responses!();
                self.clear_active(assoc);
                self.cancel_all_not_sent(result, &cause);
                result.complete = store_batch_complete(&result.items);
                return Piped {
                    position,
                    completed,
                    usable: false,
                    err: Some(cause),
                };
            }};
        }
        while position < association_plan.items.len() || !in_flight.is_empty() {
            if let Some(e) = self.context_or_closed(ctx) {
                abort_in_flight!(e);
            }
            while !stop_admit
                && position < association_plan.items.len()
                && (in_flight.len() as i64) < effective_invoked
            {
                if let Some(e) = self.context_or_closed(ctx) {
                    abort_in_flight!(e);
                }
                let planned = &association_plan.items[position];
                let index = planned.source_index as usize;
                {
                    let item = &mut result.items[index];
                    item.descriptor = planned.descriptor.clone();
                    item.association_index = association_index;
                    item.association_id = assoc.id().to_string();
                }
                let selection = accepted_context_for_sop_class_transfer_syntaxes(
                    assoc,
                    &planned.descriptor.sop_class_uid,
                    &planned.descriptor.writable_transfer_syntax_uids,
                );
                let selected_uid = selection
                    .as_ref()
                    .map(|pc| pc.transfer_syntax_uid.clone())
                    .unwrap_or_default();
                let reserve = store_representation_size(&planned.descriptor, &selected_uid).max(0);
                if self.options.max_in_flight_bytes > 0
                    && !in_flight.is_empty()
                    && reserve > self.options.max_in_flight_bytes - bytes_in_flight
                {
                    break;
                }
                result.items[index].attempt += 1;
                macro_rules! fail_before_send {
                    ($stage:expr, $cause:expr) => {{
                        let item = &mut result.items[index];
                        item.outcome = StoreOutcome::Failure;
                        item.err = Some(new_store_error(
                            $stage,
                            planned.source_index,
                            Some(&$cause),
                            false,
                        ));
                        result.failed += 1;
                        completed += 1;
                        position += 1;
                        let item = result.items[index].clone();
                        if let Some(progress_err) =
                            self.report_progress(ctx, &item, completed, total)
                        {
                            abort_in_flight!(progress_err);
                        }
                        if !self.options.continue_on_error {
                            stop_admit = true;
                            if in_flight.is_empty() {
                                let release_err = self.release_pipelined_session(&session).err();
                                self.clear_active(assoc);
                                if release_err.is_some() {
                                    result.release_failures += 1;
                                    return Piped {
                                        position,
                                        completed,
                                        usable: false,
                                        err: Some(store_release_error()),
                                    };
                                }
                                return Piped {
                                    position,
                                    completed,
                                    usable: true,
                                    err: item.err,
                                };
                            }
                            break;
                        }
                        continue;
                    }};
                }
                if selection.is_err() {
                    let cause = Error::wrap_err(
                        Kind::StorePresentationContextRejected,
                        explain_missing_presentation_context(
                            assoc,
                            &planned.descriptor.sop_class_uid,
                        ),
                    );
                    fail_before_send!("negotiate", cause);
                }
                let mut selected = self.selected_store_representation(
                    ctx,
                    sources[index],
                    assoc,
                    &planned.descriptor,
                    *remaining_total,
                );
                let pc = selected.pc.clone().unwrap_or_default();
                let mut open_err = selected.err.take();
                if open_err.is_none()
                    && let Some(opened) = &selected.opened
                {
                    open_err = validate_opened_store_source(&planned.descriptor, opened).err();
                }
                if open_err.is_none()
                    && let Some(opened) = &selected.opened
                {
                    let item = result.items[index].clone();
                    self.observe_store_prepared(ctx, &item, opened, &pc.transfer_syntax_uid);
                    open_err = check_store_access(ctx, opened.before_store.as_deref()).err();
                }
                if let Some(open_err) = open_err {
                    let had_before = selected
                        .opened
                        .as_ref()
                        .is_some_and(|o| o.before_store.is_some());
                    if let Some(opened) = &mut selected.opened
                        && opened.close.is_some()
                    {
                        let _ = close_store_source(opened.close.take());
                    }
                    if open_err.is(Kind::StoreAccessDenied) {
                        let err = new_store_error(
                            "authorize",
                            planned.source_index,
                            Some(&Error::sentinel(Kind::StoreAccessDenied)),
                            false,
                        );
                        result.items[index].err = Some(err.clone());
                        access_err = Some(err);
                        stop_admit = true;
                        break;
                    }
                    if let Some(e) = context_err(ctx)
                        && had_before
                    {
                        abort_in_flight!(e);
                    }
                    fail_before_send!("open", open_err);
                }
                let mut opened = selected.opened.take().unwrap();
                let write = opened.write_data_set.clone().unwrap();
                result.items[index].attempted = true;
                let item_left = std::cell::Cell::new(self.item_limit(
                    store_writer_representation_bound(&planned.descriptor, &pc.transfer_syntax_uid),
                ));
                let total_left = std::cell::Cell::new(*remaining_total);
                let source_index = planned.source_index;
                let closer = OnceCloser::new(opened.close.take());
                let started = session.start_c_store_streamed(
                    Some(ctx),
                    pc.id,
                    &CStoreRequest {
                        affected_sop_class_uid: planned.descriptor.sop_class_uid.clone(),
                        affected_sop_instance_uid: planned.descriptor.sop_instance_uid.clone(),
                        priority: self.options.priority,
                        move_originator_ae_title: self.options.move_originator_ae_title.clone(),
                        move_originator_message_id: self.options.move_originator_message_id,
                        ..Default::default()
                    },
                    Some(&|write_ctx: &Ctx,
                           sink: &mut dyn FnMut(&[u8]) -> Result<()>,
                           syntax: &Syntax| {
                        write(
                            write_ctx,
                            &mut |chunk: &[u8]| {
                                let n = chunk.len() as i64;
                                if n > item_left.get() || n > total_left.get() {
                                    return Err(Error::sentinel(Kind::StoreResourceLimit));
                                }
                                sink(chunk)?;
                                item_left.set(item_left.get() - n);
                                total_left.set(total_left.get() - n);
                                Ok(())
                            },
                            syntax,
                        )
                    }),
                );
                *remaining_total = total_left.get();
                let operation = match started {
                    Ok(op) => op,
                    Err(start_err) => {
                        let _ = closer.close();
                        if let Some(e) = self.context_or_closed(ctx) {
                            abort_in_flight!(e);
                        }
                        if start_err.is(Kind::AssociationStateUncertain) {
                            let item = &mut result.items[index];
                            item.outcome = StoreOutcome::Unknown;
                            item.err = Some(new_store_error(
                                "transfer",
                                source_index,
                                Some(&uncertain_store_cause(&start_err)),
                                true,
                            ));
                            result.unknown += 1;
                            completed += 1;
                            position += 1;
                            self.abort_pipelined_session(&session);
                            drain_responses!();
                            self.clear_active(assoc);
                            return Piped {
                                position,
                                completed,
                                usable: false,
                                err: None,
                            };
                        }
                        let item = &mut result.items[index];
                        item.outcome = StoreOutcome::Failure;
                        item.err = Some(new_store_error(
                            "response",
                            source_index,
                            Some(&Error::sentinel(Kind::StoreRemoteFailure)),
                            false,
                        ));
                        result.failed += 1;
                        completed += 1;
                        position += 1;
                        if !self.options.continue_on_error {
                            stop_admit = true;
                            break;
                        }
                        continue;
                    }
                };
                in_flight.insert(
                    source_index,
                    PipelineFlight {
                        reserved: reserve,
                        close: closer.clone(),
                    },
                );
                bytes_in_flight += reserve;
                position += 1;
                let worker_tx = tx.clone();
                let worker_ctx = ctx.clone();
                let syntax_uid = pc.transfer_syntax_uid.clone();
                workers.push(std::thread::spawn(move || {
                    let waited = operation.wait(Some(&worker_ctx));
                    let close_err = closer.close().err();
                    let mut completion = PipelineCompletion {
                        source_index,
                        syntax_uid,
                        response: None,
                        err: None,
                        close_err,
                        uncertain: waited.is_err(),
                    };
                    match waited {
                        Err(e) => completion.err = Some(e),
                        Ok(message) => {
                            let parsed = match &message.command {
                                Some(command) => parse_c_store_response(command),
                                None => Err(Error::new("dicom dimse: nil C-STORE response")),
                            };
                            match parsed {
                                Err(e) => {
                                    completion.err = Some(e);
                                    completion.uncertain = true;
                                }
                                Ok(response) => {
                                    completion.err = check_c_store_status(&response).err();
                                    completion.response = Some(response);
                                }
                            }
                        }
                    }
                    let _ = worker_tx.send(completion);
                }));
            }
            if in_flight.is_empty() {
                break;
            }
            let completion = loop {
                match rx.recv_timeout(Duration::from_millis(5)) {
                    Ok(c) => break c,
                    Err(_) => {
                        if let Some(e) = context_err(ctx) {
                            abort_in_flight!(e);
                        }
                    }
                }
            };
            let index = completion.source_index as usize;
            if let Some(response) = &completion.response {
                let item = result.items[index].clone();
                self.observe_store_response(ctx, &item, response.status);
            }
            if let Some(e) = self.context_or_closed(ctx) {
                abort_in_flight!(e);
            }
            if let Some(e) = &completion.err
                && (e.is_canceled() || e.is_deadline())
            {
                abort_in_flight!(e.clone());
            }
            if let Some(flight) = in_flight.remove(&completion.source_index) {
                bytes_in_flight = (bytes_in_flight - flight.reserved).max(0);
            }
            {
                let item = &mut result.items[index];
                item.negotiated_transfer_syntax_uid = completion.syntax_uid.clone();
                if let Some(response) = &completion.response {
                    item.status = response.status;
                    item.status_set = true;
                }
            }
            let uncertain = completion.uncertain
                || completion
                    .err
                    .as_ref()
                    .is_some_and(|e| e.is(Kind::AssociationStateUncertain));
            if uncertain
                && access_err.is_none()
                && !self.options.disable_reconnect
                && self.options.retry_uncertain
                && result.items[index].attempt < self.options.max_store_attempts
            {
                {
                    let item = &mut result.items[index];
                    item.outcome = StoreOutcome::NotSent;
                    item.err = None;
                    item.attempted = false;
                }
                position =
                    first_not_sent_position(association_plan, result, completion.source_index);
                self.abort_pipelined_session(&session);
                drain_responses!();
                self.clear_active(assoc);
                for flight in in_flight.values() {
                    let _ = flight.close.close();
                }
                return Piped {
                    position,
                    completed,
                    usable: false,
                    err: None,
                };
            }
            let status = result.items[index].status;
            let (outcome, err) = if uncertain {
                let cause_err = completion
                    .err
                    .clone()
                    .unwrap_or_else(|| Error::sentinel(Kind::StoreUncertain));
                let cause = if cause_err.is(Kind::StoreResourceLimit) {
                    Error::sentinel(Kind::StoreResourceLimit)
                } else {
                    uncertain_store_cause(&cause_err)
                };
                result.unknown += 1;
                usable = false;
                stop_admit = true;
                (
                    StoreOutcome::Unknown,
                    Some(new_store_error(
                        "transfer",
                        completion.source_index,
                        Some(&cause),
                        true,
                    )),
                )
            } else if completion.err.is_some() {
                result.failed += 1;
                if !self.options.continue_on_error {
                    stop_admit = true;
                }
                (
                    StoreOutcome::Failure,
                    Some(new_store_error(
                        "response",
                        completion.source_index,
                        Some(&Error::sentinel(Kind::StoreRemoteFailure)),
                        false,
                    )),
                )
            } else if completion.close_err.is_some() {
                result.failed += 1;
                if !self.options.continue_on_error {
                    stop_admit = true;
                }
                (
                    StoreOutcome::Failure,
                    Some(new_store_error(
                        "close",
                        completion.source_index,
                        Some(&Error::sentinel(Kind::StoreInvalidSource)),
                        false,
                    )),
                )
            } else if is_c_store_warning_status(status) {
                result.succeeded += 1;
                result.warnings += 1;
                (StoreOutcome::Warning, None)
            } else {
                result.succeeded += 1;
                (StoreOutcome::Success, None)
            };
            {
                let item = &mut result.items[index];
                item.outcome = outcome;
                item.err = err;
            }
            completed += 1;
            let item = result.items[index].clone();
            if let Some(progress_err) = self.report_progress(ctx, &item, completed, total) {
                abort_in_flight!(progress_err);
            }
            if usable
                && access_err.is_none()
                && item.err.is_some()
                && !self.options.continue_on_error
                && in_flight.is_empty()
            {
                let release_err = self.release_pipelined_session(&session).err();
                self.clear_active(assoc);
                if release_err.is_some() {
                    result.release_failures += 1;
                    return Piped {
                        position,
                        completed,
                        usable: false,
                        err: Some(store_release_error()),
                    };
                }
                return Piped {
                    position,
                    completed,
                    usable,
                    err: item.err,
                };
            }
            if !usable {
                self.abort_pipelined_session(&session);
                drain_responses!();
                self.clear_active(assoc);
                let cause = completion
                    .err
                    .clone()
                    .unwrap_or_else(|| Error::sentinel(Kind::StoreUncertain));
                for (source_index, flight) in &in_flight {
                    let remaining = &mut result.items[*source_index as usize];
                    if remaining.outcome == StoreOutcome::NotSent {
                        remaining.outcome = StoreOutcome::Unknown;
                        remaining.err = Some(new_store_error(
                            "transfer",
                            *source_index,
                            Some(&uncertain_store_cause(&cause)),
                            true,
                        ));
                        result.unknown += 1;
                        completed += 1;
                    }
                    let _ = flight.close.close();
                }
                let err = if access_err.is_some() {
                    access_err
                } else if !self.options.continue_on_error {
                    item.err
                } else {
                    None
                };
                return Piped {
                    position,
                    completed,
                    usable: false,
                    err,
                };
            }
        }
        if !usable {
            self.abort_pipelined_session(&session);
            drain_responses!();
            self.clear_active(assoc);
            return Piped {
                position,
                completed,
                usable: false,
                err: None,
            };
        }
        if self.release_pipelined_session(&session).is_err() {
            drain_responses!();
            self.clear_active(assoc);
            result.release_failures += 1;
            let release_err = store_release_error();
            if let Some(access) = access_err {
                return Piped {
                    position,
                    completed,
                    usable: false,
                    err: Error::join([Some(access), Some(release_err)]),
                };
            }
            if self.options.continue_on_error {
                return Piped {
                    position,
                    completed,
                    usable: true,
                    err: None,
                };
            }
            return Piped {
                position,
                completed,
                usable: false,
                err: Some(release_err),
            };
        }
        drain_responses!();
        self.clear_active(assoc);
        Piped {
            position,
            completed,
            usable: true,
            err: access_err,
        }
    }
}
fn first_not_sent_position(
    plan: &StoreAssociationPlan,
    result: &StoreBatchResult,
    source_index: i64,
) -> usize {
    for (i, planned) in plan.items.iter().enumerate() {
        if planned.source_index == source_index
            || result.items[planned.source_index as usize].outcome == StoreOutcome::NotSent
        {
            return i;
        }
    }
    plan.items.len()
}
fn observe_store_association(
    observer: &Option<StoreAssociationObserver>,
    ctx: &Ctx,
    assoc: &Association,
    index: i64,
) {
    if let Some(observer) = observer {
        let _ = catch_unwind(AssertUnwindSafe(|| observer(ctx, assoc, index)));
    }
}
