use super::*;
use crate::net::dimse::{NormalizedClient, NormalizedOperationOptions, NormalizedRequest};
use crate::net::ul::{self, Association, Ctx, DialOptions, Nanos};
use crate::object::Vr;
use std::collections::BTreeSet;
use std::time::Duration;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SubscriptionState {
    #[default]
    Unset,
    None,
    WithDeletionLock,
    WithoutLock,
}
impl SubscriptionState {
    pub fn as_str(self) -> &'static str {
        match self {
            SubscriptionState::Unset => "",
            SubscriptionState::None => "none",
            SubscriptionState::WithDeletionLock => "with_deletion_lock",
            SubscriptionState::WithoutLock => "without_deletion_lock",
        }
    }
}
#[derive(Debug, Clone, Default)]
pub struct Subscription {
    pub sop_instance_uid: String,
    pub receiving_ae_title: String,
    pub state: SubscriptionState,
    pub filter: Option<SubscriptionFilter>,
    pub version: u64,
    pub updated_at: Time,
}
#[derive(Debug, Clone, Default)]
pub struct SubscriptionQuery {
    pub sop_instance_uid: String,
    pub receiving_ae_title: String,
    pub active_only: bool,
    pub limit: i64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SubscriptionMutationKind {
    #[default]
    Subscribe,
    Unsubscribe,
    SuspendGlobal,
    Other,
}
#[derive(Debug, Clone, Default)]
pub struct SubscriptionMutation {
    pub kind: SubscriptionMutationKind,
    pub sop_instance_uid: String,
    pub receiving_ae_title: String,
    pub deletion_lock: bool,
    pub filter: Option<SubscriptionFilter>,
    pub filter_scan_limit: i64,
    pub updated_at: Time,
}
#[derive(Debug, Clone, Default)]
pub struct SubscribeRequest {
    pub sop_instance_uid: String,
    pub receiving_ae_title: String,
    pub deletion_lock: bool,
    pub matching_keys: MatchingKeys,
}
#[derive(Debug, Clone, Default)]
pub struct SubscribeResult {
    pub subscription: Subscription,
    pub status: u16,
}
#[derive(Debug, Clone, Default)]
pub struct UnsubscribeRequest {
    pub sop_instance_uid: String,
    pub receiving_ae_title: String,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DeliveryState {
    #[default]
    Unset,
    Pending,
    Delivering,
    Failed,
    Exhausted,
    Delivered,
}
impl DeliveryState {
    pub fn as_str(self) -> &'static str {
        match self {
            DeliveryState::Unset => "",
            DeliveryState::Pending => "pending",
            DeliveryState::Delivering => "delivering",
            DeliveryState::Failed => "failed",
            DeliveryState::Exhausted => "exhausted",
            DeliveryState::Delivered => "delivered",
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DeliveryFailureClass {
    #[default]
    None,
    CallbackUnknown,
    CallbackOffline,
    Tls,
    Timeout,
    Canceled,
    AssociationRejected,
    DimseStatus,
    Protocol,
}
impl DeliveryFailureClass {
    pub fn as_str(self) -> &'static str {
        match self {
            DeliveryFailureClass::None => "",
            DeliveryFailureClass::CallbackUnknown => "callback_unknown",
            DeliveryFailureClass::CallbackOffline => "callback_offline",
            DeliveryFailureClass::Tls => "tls",
            DeliveryFailureClass::Timeout => "timeout",
            DeliveryFailureClass::Canceled => "canceled",
            DeliveryFailureClass::AssociationRejected => "association_rejected",
            DeliveryFailureClass::DimseStatus => "dimse_status",
            DeliveryFailureClass::Protocol => "protocol",
        }
    }
}
#[derive(Debug, Clone, Default)]
pub struct Delivery {
    pub id: String,
    pub event_id: String,
    pub event_type: EventType,
    pub sop_instance_uid: String,
    pub receiving_ae_title: String,
    pub event: Event,
    pub state: DeliveryState,
    pub version: u64,
    pub attempts: i64,
    pub next_attempt_at: Time,
    pub lease_token: String,
    pub lease_until: Time,
    pub last_failure: DeliveryFailureClass,
    pub last_dimse_status: u16,
}
#[derive(Debug, Clone, Default)]
pub struct DeliveryQuery {
    pub sop_instance_uid: String,
    pub receiving_ae_title: String,
    pub states: Vec<DeliveryState>,
    pub limit: i64,
}
#[derive(Debug, Clone, Default)]
pub struct DeliveryOutcome {
    pub delivered: bool,
    pub retryable: bool,
    pub next_attempt: Time,
    pub failure: DeliveryFailureClass,
    pub dimse_status: u16,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DeliveryLimits {
    pub max_batch: i64,
    pub max_attempts: i64,
    pub initial_backoff: Nanos,
    pub max_backoff: Nanos,
    pub attempt_timeout: Nanos,
    pub lease_duration: Nanos,
    pub cleanup_timeout: Nanos,
}
#[derive(Debug, Clone, Default)]
pub struct CallbackRequest {
    pub receiving_ae_title: String,
}
#[derive(Clone, Default)]
pub struct CallbackTarget {
    pub address: String,
    pub dial_options: DialOptions,
}
pub trait CallbackResolver: Send + Sync {
    fn resolve_ups_callback(&self, ctx: &Ctx, request: &CallbackRequest) -> Result<CallbackTarget>;
}
pub struct CallbackResolverFunc<F>(pub F);
impl<F> CallbackResolver for CallbackResolverFunc<F>
where
    F: Fn(&Ctx, &CallbackRequest) -> Result<CallbackTarget> + Send + Sync,
{
    fn resolve_ups_callback(&self, ctx: &Ctx, request: &CallbackRequest) -> Result<CallbackTarget> {
        (self.0)(ctx, request)
    }
}
pub trait AssociationDialer: Send + Sync {
    fn dial_ups(
        &self,
        ctx: &Ctx,
        address: &str,
        options: DialOptions,
    ) -> (Option<Association>, Option<Error>);
}
pub struct AssociationDialerFunc<F>(pub F);
impl<F> AssociationDialer for AssociationDialerFunc<F>
where
    F: Fn(&Ctx, &str, DialOptions) -> (Option<Association>, Option<Error>) + Send + Sync,
{
    fn dial_ups(
        &self,
        ctx: &Ctx,
        address: &str,
        options: DialOptions,
    ) -> (Option<Association>, Option<Error>) {
        (self.0)(ctx, address, options)
    }
}
pub(crate) struct DefaultAssociationDialer;
impl AssociationDialer for DefaultAssociationDialer {
    fn dial_ups(
        &self,
        ctx: &Ctx,
        address: &str,
        options: DialOptions,
    ) -> (Option<Association>, Option<Error>) {
        match ul::dial_context(ctx, address, options) {
            Ok(a) => (Some(a), None),
            Err(e) => (None, Some(Error::Dimse(e.into()))),
        }
    }
}
fn delivery_error(
    class: DeliveryFailureClass,
    status: u16,
    retryable: bool,
    err: Option<Error>,
) -> Error {
    Error::Delivery {
        class,
        status,
        retryable,
        err: err.map(Box::new),
    }
}
pub(crate) fn normalize_delivery_limits(mut l: DeliveryLimits) -> Result<DeliveryLimits> {
    if l.max_batch < 0
        || l.max_attempts < 0
        || l.initial_backoff.0 < 0
        || l.max_backoff.0 < 0
        || l.attempt_timeout.0 < 0
        || l.lease_duration.0 < 0
        || l.cleanup_timeout.0 < 0
    {
        return Err(Kind::ResourceLimit.into());
    }
    if l.max_batch == 0 {
        l.max_batch = 256;
    }
    if l.max_attempts == 0 {
        l.max_attempts = 5;
    }
    if l.initial_backoff.0 == 0 {
        l.initial_backoff = Nanos::from_secs(1);
    }
    if l.max_backoff.0 == 0 {
        l.max_backoff = Nanos::from_secs(60);
    }
    if l.attempt_timeout.0 == 0 {
        l.attempt_timeout = Nanos::from_secs(20);
    }
    if l.lease_duration.0 == 0 {
        l.lease_duration = Nanos::from_secs(30);
    }
    if l.cleanup_timeout.0 == 0 {
        l.cleanup_timeout = Nanos::from_secs(5);
    }
    if l.max_backoff.0 < l.initial_backoff.0
        || l.attempt_timeout.0 <= 0
        || l.lease_duration.0 <= 0
        || l.cleanup_timeout.0 <= 0
        || l.attempt_timeout.0 >= l.lease_duration.0
        || l.cleanup_timeout.0 >= l.lease_duration.0 - l.attempt_timeout.0
    {
        return Err(Kind::ResourceLimit.into());
    }
    Ok(l)
}
fn timeout_ctx(parent: &Ctx, d: Nanos) -> Ctx {
    parent.with_timeout(Duration::from_nanos(d.0.max(0) as u64))
}
impl Service {
    pub fn subscribe(&self, ctx: &Ctx, request: &SubscribeRequest) -> Result<SubscribeResult> {
        const OP: &str = "N-ACTION Subscribe";
        let receiving = request.receiving_ae_title.trim().to_string();
        let mut filter = None;
        let store = &self.inner.store;
        if request.sop_instance_uid == FILTERED_GLOBAL_SUBSCRIPTION_SOP_INSTANCE_UID {
            match compile_subscription_filter(&request.matching_keys) {
                Err(e) => {
                    let status = subscription_filter_status(&e);
                    return Err(status_error(OP, status, e));
                }
                Ok(f) => filter = Some(f),
            }
            if store.filtered_global_committer().is_none() {
                return Err(status_error(
                    OP,
                    STATUS_UPS_NOT_FOUND,
                    Kind::InvalidDataSet.into(),
                ));
            }
        } else if !request.matching_keys.is_empty() {
            return Err(status_error(
                OP,
                STATUS_ACTION_NOT_APPROPRIATE,
                Kind::InvalidDataSet.into(),
            ));
        }
        self.validate_subscriber(ctx, &receiving)?;
        if !is_global_subscription_uid(&request.sop_instance_uid) {
            match store.get_step(ctx, &request.sop_instance_uid) {
                Err(e) if e.is(Kind::NotFound) => {
                    return Err(status_error(OP, STATUS_UPS_NOT_FOUND, e));
                }
                Err(e) => return Err(safe_repository_error(e)),
                Ok(_) => {}
            }
        }
        let mut status = STATUS_SUCCESS;
        let mut lock = request.deletion_lock;
        if lock && self.inner.refuse_deletion_locks {
            lock = false;
            status = STATUS_DELETION_LOCK_NOT_GRANTED;
        }
        let mutation = SubscriptionMutation {
            kind: SubscriptionMutationKind::Subscribe,
            sop_instance_uid: request.sop_instance_uid.clone(),
            receiving_ae_title: receiving,
            deletion_lock: lock,
            filter: filter.clone(),
            filter_scan_limit: self.inner.limits.max_subscription_filter_scanned,
            updated_at: self.now(),
        };
        let result = if filter.is_some() {
            store
                .filtered_global_committer()
                .unwrap()
                .commit_filtered_global_subscription(ctx, mutation)
        } else {
            store.commit_ups(
                ctx,
                CommitRequest {
                    subscription: Some(mutation),
                    ..Default::default()
                },
            )
        };
        let result = match result {
            Err(e) if e.is(Kind::ResourceLimit) => {
                return Err(status_error(OP, STATUS_RESOURCE_LIMITATION, e));
            }
            Err(e) => return Err(safe_repository_error(e)),
            Ok(r) => r,
        };
        let Some(subscription) = result.subscription else {
            return Err(safe_repository_error(Kind::Conflict.into()));
        };
        self.wake_deliveries();
        Ok(SubscribeResult {
            subscription,
            status,
        })
    }
    pub fn unsubscribe(&self, ctx: &Ctx, request: &UnsubscribeRequest) -> Result<()> {
        const OP: &str = "N-ACTION Unsubscribe";
        let receiving = request.receiving_ae_title.trim().to_string();
        if !valid_ae_title(&receiving) {
            return Err(status_error(
                OP,
                STATUS_RECEIVING_AE_UNKNOWN,
                Kind::InvalidDataSet.into(),
            ));
        }
        if request.sop_instance_uid.is_empty() {
            return Err(status_error(
                OP,
                STATUS_ACTION_NOT_APPROPRIATE,
                Kind::InvalidDataSet.into(),
            ));
        }
        if !is_global_subscription_uid(&request.sop_instance_uid) {
            match self.inner.store.get_step(ctx, &request.sop_instance_uid) {
                Err(e) if e.is(Kind::NotFound) => {
                    return Err(status_error(OP, STATUS_UPS_NOT_FOUND, e));
                }
                Err(e) => return Err(safe_repository_error(e)),
                Ok(_) => {}
            }
        }
        self.inner
            .store
            .commit_ups(
                ctx,
                CommitRequest {
                    subscription: Some(SubscriptionMutation {
                        kind: SubscriptionMutationKind::Unsubscribe,
                        sop_instance_uid: request.sop_instance_uid.clone(),
                        receiving_ae_title: receiving,
                        updated_at: self.now(),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
            )
            .map(|_| ())
            .map_err(safe_repository_error)
    }
    pub fn suspend_global(&self, ctx: &Ctx, receiving_ae_title: &str) -> Result<()> {
        let receiving = receiving_ae_title.trim().to_string();
        if !valid_ae_title(&receiving) {
            return Err(status_error(
                "N-ACTION Suspend Global",
                STATUS_RECEIVING_AE_UNKNOWN,
                Kind::InvalidDataSet.into(),
            ));
        }
        self.inner
            .store
            .commit_ups(
                ctx,
                CommitRequest {
                    subscription: Some(SubscriptionMutation {
                        kind: SubscriptionMutationKind::SuspendGlobal,
                        sop_instance_uid: GLOBAL_SUBSCRIPTION_SOP_INSTANCE_UID.into(),
                        receiving_ae_title: receiving,
                        updated_at: self.now(),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
            )
            .map(|_| ())
            .map_err(safe_repository_error)
    }
    pub fn subscription(
        &self,
        ctx: &Ctx,
        sop_instance_uid: &str,
        receiving_ae_title: &str,
    ) -> Result<Subscription> {
        self.inner
            .store
            .get_subscription(ctx, sop_instance_uid, receiving_ae_title)
            .map_err(safe_repository_error)
    }
    pub fn deliveries(&self, ctx: &Ctx, query: &DeliveryQuery) -> Result<Vec<Delivery>> {
        let mut query = query.clone();
        if query.limit == 0 {
            query.limit = 1_000;
        }
        if query.limit < 0 || query.limit > 1_000 {
            return Err(Kind::ResourceLimit.into());
        }
        self.inner
            .store
            .list_deliveries(ctx, &query)
            .map_err(safe_repository_error)
    }
    pub fn deliver_due(&self, ctx: &Ctx, limit: i64) -> Result<()> {
        match self.deliver_due_count(ctx, limit).1 {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }
    fn deliver_due_count(&self, ctx: &Ctx, limit: i64) -> (i64, Option<Error>) {
        let dl = self.inner.delivery_limits;
        if limit <= 0 || limit > dl.max_batch {
            return (0, Some(Kind::ResourceLimit.into()));
        }
        let store = &self.inner.store;
        let mut errors = Vec::new();
        let mut claimed = 0;
        for _ in 0..limit {
            let deliveries = match store.claim_due_deliveries(ctx, self.now(), 1, dl.lease_duration)
            {
                Err(e) => {
                    errors.push(safe_repository_error(e));
                    break;
                }
                Ok(d) => d,
            };
            let Some(delivery) = deliveries.into_iter().next() else {
                break;
            };
            claimed += 1;
            if delivery.attempts > dl.max_attempts {
                let persist = timeout_ctx(&Ctx::background(), dl.cleanup_timeout);
                if let Err(e) = store.complete_delivery(
                    &persist,
                    &delivery.id,
                    &delivery.lease_token,
                    &DeliveryOutcome {
                        failure: DeliveryFailureClass::Protocol,
                        retryable: false,
                        ..Default::default()
                    },
                ) {
                    errors.push(safe_repository_error(e));
                }
                continue;
            }
            let attempt = timeout_ctx(ctx, dl.attempt_timeout);
            let (outcome, attempt_err) = self.attempt_delivery(&attempt, &delivery);
            let persist = timeout_ctx(&Ctx::background(), dl.cleanup_timeout);
            if let Err(e) =
                store.complete_delivery(&persist, &delivery.id, &delivery.lease_token, &outcome)
            {
                errors.push(safe_repository_error(e));
                continue;
            }
            if let Some(e) = attempt_err {
                errors.push(e);
            }
        }
        (claimed, Error::join(errors))
    }
    pub fn wake_deliveries(&self) {
        let (lock, cvar) = &self.inner.wake;
        if let Ok(mut woken) = lock.lock() {
            *woken = true;
            cvar.notify_one();
        }
    }
    pub fn run_deliveries(&self, ctx: &Ctx, interval: Nanos) -> Result<()> {
        let interval = if interval.0 <= 0 {
            Duration::from_secs(1)
        } else {
            Duration::from_nanos(interval.0 as u64)
        };
        let max = self.inner.delivery_limits.max_batch;
        let mut next_tick = std::time::Instant::now() + interval;
        loop {
            while ctx.err().is_none() {
                let (claimed, _) = self.deliver_due_count(ctx, max);
                if claimed < max {
                    break;
                }
            }
            let (lock, cvar) = &self.inner.wake;
            loop {
                if let Some(e) = dimse::context_err(ctx) {
                    return Err(Error::Dimse(e));
                }
                let now = std::time::Instant::now();
                if now >= next_tick {
                    next_tick = now + interval;
                    break;
                }
                let Ok(guard) = lock.lock() else {
                    break;
                };
                let slice = (next_tick - now).min(Duration::from_millis(20));
                let Ok((mut woken, _)) = cvar.wait_timeout(guard, slice) else {
                    break;
                };
                if *woken {
                    *woken = false;
                    break;
                }
            }
        }
    }
    pub fn report_scp_status_change(&self, ctx: &Ctx, change: &ScpStatusChange) -> Result<()> {
        if !valid_scp_status_change(change) {
            return Err(Kind::InvalidDataSet.into());
        }
        let limits = self.inner.limits;
        let receiving = self
            .inner
            .store
            .list_active_receiving_ae_titles(ctx, limits.max_status_recipients + 1)
            .map_err(safe_repository_error)?;
        if receiving.len() as i64 > limits.max_status_recipients {
            return Err(Kind::ResourceLimit.into());
        }
        let mut recipients = BTreeSet::new();
        for ae in self
            .inner
            .fallback_receiving_aes
            .iter()
            .chain(receiving.iter())
        {
            recipients.insert(ae.clone());
        }
        if recipients.is_empty() {
            return Err(Kind::Conflict.into());
        }
        if recipients.len() as i64 > limits.max_status_recipients {
            return Err(Kind::ResourceLimit.into());
        }
        let information = item_of(vec![
            string_element(TAG_SCP_STATUS, Vr::CS, &[&change.status]),
            string_element(
                TAG_SUBSCRIPTION_LIST_STATUS,
                Vr::CS,
                &[&change.subscription_list_status],
            ),
            string_element(
                TAG_UNIFIED_PROCEDURE_STEP_LIST_STATUS,
                Vr::CS,
                &[&change.ups_list_status],
            ),
        ]);
        let events = recipients
            .into_iter()
            .map(|ae| Event {
                event_type: EVENT_SCP_STATUS_CHANGE,
                sop_instance_uid: GLOBAL_SUBSCRIPTION_SOP_INSTANCE_UID.into(),
                information: information.clone(),
                direct_receiving_ae: ae,
                created_at: self.now(),
                ..Default::default()
            })
            .collect();
        self.inner
            .store
            .commit_ups(
                ctx,
                CommitRequest {
                    events,
                    ..Default::default()
                },
            )
            .map_err(safe_repository_error)?;
        self.wake_deliveries();
        Ok(())
    }
    fn attempt_delivery(&self, ctx: &Ctx, delivery: &Delivery) -> (DeliveryOutcome, Option<Error>) {
        let inner = &self.inner;
        if let Some(sink) = &inner.event_sink {
            let (handled, r) = sink.deliver_ups_event(ctx, delivery);
            if handled && r.is_ok() {
                return (
                    DeliveryOutcome {
                        delivered: true,
                        ..Default::default()
                    },
                    None,
                );
            }
            if handled {
                let err = r.err();
                let class = classify_delivery_failure(err.as_ref());
                return (
                    self.failure_outcome(delivery, class, true, 0),
                    Some(delivery_error(class, 0, true, err)),
                );
            }
            if inner.callback_resolver.is_none() {
                let class = DeliveryFailureClass::CallbackOffline;
                return (
                    self.failure_outcome(delivery, class, true, 0),
                    Some(delivery_error(
                        class,
                        0,
                        true,
                        Some(Kind::DeliveryFailed.into()),
                    )),
                );
            }
        }
        let Some(resolver) = &inner.callback_resolver else {
            let class = DeliveryFailureClass::CallbackUnknown;
            return (
                self.failure_outcome(delivery, class, true, 0),
                Some(delivery_error(
                    class,
                    0,
                    true,
                    Some(Kind::DeliveryFailed.into()),
                )),
            );
        };
        let target = resolver.resolve_ups_callback(
            ctx,
            &CallbackRequest {
                receiving_ae_title: delivery.receiving_ae_title.clone(),
            },
        );
        let mut target = match target {
            Ok(t) if !t.address.trim().is_empty() => t,
            other => {
                let class = DeliveryFailureClass::CallbackUnknown;
                return (
                    self.failure_outcome(delivery, class, true, 0),
                    Some(delivery_error(class, 0, true, other.err())),
                );
            }
        };
        target.dial_options = match clone_ups_dial_options(&target.dial_options) {
            Ok(o) => o,
            Err(e) => {
                let class = DeliveryFailureClass::Protocol;
                return (
                    self.failure_outcome(delivery, class, false, 0),
                    Some(delivery_error(class, 0, false, Some(e))),
                );
            }
        };
        if target.dial_options.called_ae_title.trim().is_empty() {
            target.dial_options.called_ae_title = delivery.receiving_ae_title.clone();
        }
        if target.dial_options.calling_ae_title.trim().is_empty() {
            target.dial_options.calling_ae_title = inner.callback_calling_ae.clone();
        }
        let cleanup = || timeout_ctx(&Ctx::background(), inner.delivery_limits.cleanup_timeout);
        let (assoc, err) =
            inner
                .association_dialer
                .dial_ups(ctx, &target.address, target.dial_options.clone());
        if let Some(err) = err {
            if let Some(a) = &assoc {
                let _ = a.abort_with_context(Some(&cleanup()), ul::ABORT_REASON_NOT_SPECIFIED);
                a.close();
            }
            let class = classify_delivery_failure(Some(&err));
            return (
                self.failure_outcome(delivery, class, true, 0),
                Some(delivery_error(class, 0, true, Some(err))),
            );
        }
        let Some(assoc) = assoc else {
            let class = DeliveryFailureClass::CallbackOffline;
            return (
                self.failure_outcome(delivery, class, true, 0),
                Some(delivery_error(
                    class,
                    0,
                    true,
                    Some(Kind::DeliveryFailed.into()),
                )),
            );
        };
        let data_set = Object::from_data_set(delivery.event.information.clone());
        let client = NormalizedClient::new(&assoc);
        let mut options = NormalizedOperationOptions::default();
        options.operation.context = Some(ctx.clone());
        options.presentation_context_abstract_syntax_uid = EVENT_SOP_CLASS_UID.into();
        options.max_response_data_set_bytes = inner.limits.max_data_set_bytes;
        let (_, r) = client.event_report(
            &options,
            NormalizedRequest {
                sop_class_uid: PUSH_SOP_CLASS_UID.into(),
                sop_instance_uid: delivery.sop_instance_uid.clone(),
                command_data_set_type: dimse::DATA_SET_PRESENT,
                type_id: delivery.event_type,
                ..Default::default()
            },
            Some(&data_set),
        );
        if let Err(send) = r {
            let _ = assoc.abort_with_context(Some(&cleanup()), ul::ABORT_REASON_NOT_SPECIFIED);
            assoc.close();
            let send = Error::Dimse(send);
            let mut status = 0;
            let mut class = classify_delivery_failure(Some(&send));
            if let Some(d) = send.dimse_error()
                && d.is(dimse::Kind::NormalizedStatus)
                && let dimse::Detail::Status(s) = d.detail()
            {
                status = *s;
                class = DeliveryFailureClass::DimseStatus;
            }
            let retryable = class != DeliveryFailureClass::DimseStatus;
            return (
                self.failure_outcome(delivery, class, retryable, status),
                Some(delivery_error(class, status, retryable, Some(send))),
            );
        }
        let _ = assoc.release(Some(&cleanup()));
        assoc.close();
        (
            DeliveryOutcome {
                delivered: true,
                ..Default::default()
            },
            None,
        )
    }
    fn failure_outcome(
        &self,
        delivery: &Delivery,
        class: DeliveryFailureClass,
        mut retryable: bool,
        status: u16,
    ) -> DeliveryOutcome {
        let dl = &self.inner.delivery_limits;
        let next = delivery.attempts.max(1);
        if next >= dl.max_attempts {
            retryable = false;
        }
        let mut backoff = dl.initial_backoff.0;
        let mut attempt = 1;
        while attempt < next && backoff < dl.max_backoff.0 {
            if backoff > dl.max_backoff.0 / 2 {
                backoff = dl.max_backoff.0;
                break;
            }
            backoff *= 2;
            attempt += 1;
        }
        if backoff > dl.max_backoff.0 {
            backoff = dl.max_backoff.0;
        }
        DeliveryOutcome {
            retryable,
            next_attempt: self.now().add(Nanos(backoff)),
            failure: class,
            dimse_status: status,
            ..Default::default()
        }
    }
    fn validate_subscriber(&self, ctx: &Ctx, receiving: &str) -> Result<()> {
        const OP: &str = "N-ACTION Subscription";
        if receiving.trim().is_empty() || receiving.len() > 16 || receiving.contains('\\') {
            return Err(status_error(
                OP,
                STATUS_RECEIVING_AE_UNKNOWN,
                Kind::InvalidDataSet.into(),
            ));
        }
        let Some(resolver) = &self.inner.callback_resolver else {
            if self.inner.event_sink.is_some() {
                return Ok(());
            }
            return Err(status_error(
                OP,
                STATUS_EVENT_REPORTS_UNSUPPORTED,
                Kind::DeliveryFailed.into(),
            ));
        };
        match resolver.resolve_ups_callback(
            ctx,
            &CallbackRequest {
                receiving_ae_title: receiving.to_string(),
            },
        ) {
            Ok(t) if !t.address.trim().is_empty() => Ok(()),
            other => {
                if self.inner.event_sink.is_some() {
                    return Ok(());
                }
                Err(Error::Status {
                    operation: OP.into(),
                    status: STATUS_RECEIVING_AE_UNKNOWN,
                    err: other.err().map(Box::new),
                })
            }
        }
    }
}
pub(crate) fn valid_scp_status_change(change: &ScpStatusChange) -> bool {
    if change.status != SCP_STATUS_RESTARTED && change.status != SCP_STATUS_GOING_DOWN {
        return false;
    }
    if !valid_cs_value(&change.subscription_list_status) || !valid_cs_value(&change.ups_list_status)
    {
        return false;
    }
    if change.status != SCP_STATUS_RESTARTED {
        return true;
    }
    let ok = |s: &str| s == LIST_STATUS_WARM_START || s == LIST_STATUS_COLD_START;
    ok(&change.subscription_list_status) && ok(&change.ups_list_status)
}
fn valid_cs_value(value: &str) -> bool {
    let value = value.trim();
    if value.is_empty()
        || value.len() > 16
        || value != crate::textfmt::to_upper(value)
        || value.contains('\\')
    {
        return false;
    }
    value
        .chars()
        .all(|c| c == ' ' || c == '_' || c.is_ascii_uppercase() || c.is_ascii_digit())
}
pub(crate) fn clone_ups_dial_options(options: &DialOptions) -> Result<DialOptions> {
    let mut out = options.clone();
    let has_event = out
        .contexts
        .iter()
        .any(|c| c.abstract_syntax_uid == EVENT_SOP_CLASS_UID);
    if !has_event {
        if out.contexts.len() == ul::MAX_PRESENTATION_CONTEXTS {
            return Err(Kind::ResourceLimit.into());
        }
        out.contexts
            .push(presentation_context(EVENT_SOP_CLASS_UID)?);
    }
    Ok(out)
}
pub(crate) fn classify_delivery_failure(err: Option<&Error>) -> DeliveryFailureClass {
    let Some(err) = err else {
        return DeliveryFailureClass::Protocol;
    };
    if err.is_canceled() {
        return DeliveryFailureClass::Canceled;
    }
    if err.is_deadline() {
        return DeliveryFailureClass::Timeout;
    }
    if let Some(d) = err.dimse_error() {
        if d.is_ul(ul::Kind::NetTimeout) {
            return DeliveryFailureClass::Timeout;
        }
        let text = d.text();
        if text.starts_with("dial tcp")
            || text.contains("read tcp")
            || text.contains("write tcp")
            || d.is_ul(ul::Kind::Closed)
        {
            return DeliveryFailureClass::CallbackOffline;
        }
    }
    DeliveryFailureClass::Protocol
}
