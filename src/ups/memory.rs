use super::*;
use crate::net::ul::{Ctx, Nanos};
use crate::object::Vr;
use std::collections::{HashMap, HashSet};
use std::sync::RwLock;
const DEFAULT_MEMORY_MAX_STEPS: i64 = 10_000;
const DEFAULT_MEMORY_MAX_SUBSCRIPTIONS: i64 = 100_000;
const DEFAULT_MEMORY_MAX_EVENTS: i64 = 100_000;
const DEFAULT_MEMORY_MAX_DELIVERIES: i64 = 500_000;
const DEFAULT_MEMORY_MAX_FAN_OUT: i64 = 10_000;
#[derive(Debug, Clone, Copy, Default)]
pub struct MemoryStoreOptions {
    pub max_steps: i64,
    pub max_subscriptions: i64,
    pub max_events: i64,
    pub max_deliveries: i64,
    pub max_fan_out: i64,
}
#[derive(Default)]
struct State {
    sequence: u64,
    event_sequence: u64,
    lease_sequence: u64,
    steps: HashMap<String, Step>,
    subscriptions: HashMap<String, Subscription>,
    events: HashMap<String, Event>,
    event_order: Vec<String>,
    deliveries: HashMap<String, Delivery>,
    delivery_order: Vec<String>,
}
pub struct MemoryStore {
    max_steps: i64,
    max_subscriptions: i64,
    max_events: i64,
    max_deliveries: i64,
    max_fan_out: i64,
    state: RwLock<State>,
}
fn ctx_error(ctx: &Ctx) -> Result<()> {
    ctx_err(ctx)
}
impl MemoryStore {
    pub fn new(mut o: MemoryStoreOptions) -> Result<MemoryStore> {
        if o.max_steps < 0
            || o.max_subscriptions < 0
            || o.max_events < 0
            || o.max_deliveries < 0
            || o.max_fan_out < 0
        {
            return Err(Kind::ResourceLimit.into());
        }
        if o.max_steps == 0 {
            o.max_steps = DEFAULT_MEMORY_MAX_STEPS;
        }
        if o.max_events == 0 {
            o.max_events = DEFAULT_MEMORY_MAX_EVENTS;
        }
        if o.max_subscriptions == 0 {
            o.max_subscriptions = DEFAULT_MEMORY_MAX_SUBSCRIPTIONS;
        }
        if o.max_deliveries == 0 {
            o.max_deliveries = DEFAULT_MEMORY_MAX_DELIVERIES;
        }
        if o.max_fan_out == 0 {
            o.max_fan_out = DEFAULT_MEMORY_MAX_FAN_OUT;
        }
        Ok(MemoryStore {
            max_steps: o.max_steps,
            max_subscriptions: o.max_subscriptions,
            max_events: o.max_events,
            max_deliveries: o.max_deliveries,
            max_fan_out: o.max_fan_out,
            state: RwLock::new(State::default()),
        })
    }
    fn read(&self) -> std::sync::RwLockReadGuard<'_, State> {
        self.state.read().unwrap()
    }
    fn write(&self) -> std::sync::RwLockWriteGuard<'_, State> {
        self.state.write().unwrap()
    }
}
pub(crate) fn clone_step(ctx: &Ctx, step: &Step) -> Result<Step> {
    let attributes = clone_data_set(ctx, &step.attributes, &default_limits())?;
    Ok(Step {
        attributes,
        ..step.clone()
    })
}
pub(crate) fn clone_event(ctx: &Ctx, event: &Event) -> Result<Event> {
    let information = clone_data_set(ctx, &event.information, &default_limits())?;
    Ok(Event {
        information,
        ..event.clone()
    })
}
fn clone_delivery(ctx: &Ctx, delivery: &Delivery) -> Result<Delivery> {
    let event = clone_event(ctx, &delivery.event)?;
    Ok(Delivery {
        event,
        ..delivery.clone()
    })
}
fn subscription_key(sop_instance_uid: &str, receiving: &str) -> String {
    format!("{receiving}\x00{sop_instance_uid}")
}
pub(crate) fn is_global_subscription_uid(uid: &str) -> bool {
    uid == GLOBAL_SUBSCRIPTION_SOP_INSTANCE_UID
        || uid == FILTERED_GLOBAL_SUBSCRIPTION_SOP_INSTANCE_UID
}
fn delivery_key(event_id: &str, receiving: &str) -> String {
    format!("{event_id}\x00{receiving}")
}
fn state_event_for_step(step: &Step, receiving: &str, now: Time) -> Event {
    let mut information = item_of(vec![string_element(
        TAG_PROCEDURE_STEP_STATE,
        Vr::CS,
        &[&step.state],
    )]);
    if let Some(r) = data_set_element(&step.attributes, TAG_INPUT_READINESS_STATE) {
        information.elements.push(r.clone());
    }
    Event {
        event_type: EVENT_STATE_REPORT,
        sop_instance_uid: step.sop_instance_uid.clone(),
        step_version: step.version,
        information,
        direct_receiving_ae: receiving.to_string(),
        created_at: now,
        ..Default::default()
    }
}
fn reclaim_subscription_tombstones(
    subscriptions: &mut HashMap<String, Subscription>,
    steps: &HashMap<String, Step>,
    maximum: i64,
) {
    let active_global: HashSet<String> = subscriptions
        .values()
        .filter(|s| {
            is_global_subscription_uid(&s.sop_instance_uid) && s.state != SubscriptionState::None
        })
        .map(|s| s.receiving_ae_title.clone())
        .collect();
    let mut tombstones: Vec<(String, Time)> = Vec::new();
    let keys: Vec<String> = subscriptions.keys().cloned().collect();
    for key in keys {
        let s = &subscriptions[&key];
        if s.state != SubscriptionState::None {
            continue;
        }
        if !steps.contains_key(&s.sop_instance_uid)
            || !active_global.contains(&s.receiving_ae_title)
        {
            subscriptions.remove(&key);
            continue;
        }
        tombstones.push((key, s.updated_at));
    }
    if tombstones.len() as i64 <= maximum {
        return;
    }
    tombstones.sort_by_key(|a| a.1);
    let excess = tombstones.len() - maximum as usize;
    for (key, _) in &tombstones[..excess] {
        subscriptions.remove(key);
    }
}
impl Store for MemoryStore {
    fn get_step(&self, ctx: &Ctx, uid: &str) -> Result<Step> {
        ctx_error(ctx)?;
        let step = self.read().steps.get(uid).cloned();
        match step {
            None => Err(Kind::NotFound.into()),
            Some(s) => clone_step(ctx, &s),
        }
    }
    fn list_steps(&self, ctx: &Ctx, query: &StepQuery) -> Result<Vec<Step>> {
        ctx_error(ctx)?;
        if query.limit <= 0 {
            return Err(Kind::ResourceLimit.into());
        }
        let mut steps: Vec<Step> = self
            .read()
            .steps
            .values()
            .filter(|s| {
                s.sequence > query.after_sequence
                    && (query.states.is_empty() || query.states.contains(&s.state))
            })
            .cloned()
            .collect();
        steps.sort_by_key(|s| s.sequence);
        steps.truncate(query.limit as usize);
        steps.iter().map(|s| clone_step(ctx, s)).collect()
    }
    fn get_subscription(&self, ctx: &Ctx, uid: &str, receiving: &str) -> Result<Subscription> {
        ctx_error(ctx)?;
        match self
            .read()
            .subscriptions
            .get(&subscription_key(uid, receiving))
        {
            Some(s) if s.state != SubscriptionState::None => Ok(s.clone()),
            _ => Err(Kind::NotFound.into()),
        }
    }
    fn list_subscriptions(&self, ctx: &Ctx, q: &SubscriptionQuery) -> Result<Vec<Subscription>> {
        ctx_error(ctx)?;
        if q.limit <= 0 {
            return Err(Kind::ResourceLimit.into());
        }
        let mut out: Vec<Subscription> = self
            .read()
            .subscriptions
            .values()
            .filter(|s| {
                (q.sop_instance_uid.is_empty() || s.sop_instance_uid == q.sop_instance_uid)
                    && (q.receiving_ae_title.is_empty()
                        || s.receiving_ae_title == q.receiving_ae_title)
                    && !(q.active_only && s.state == SubscriptionState::None)
            })
            .cloned()
            .collect();
        out.sort_by(|a, b| {
            (&a.receiving_ae_title, &a.sop_instance_uid)
                .cmp(&(&b.receiving_ae_title, &b.sop_instance_uid))
        });
        out.truncate(q.limit as usize);
        Ok(out)
    }
    fn list_active_receiving_ae_titles(&self, ctx: &Ctx, limit: i64) -> Result<Vec<String>> {
        ctx_error(ctx)?;
        if limit <= 0 {
            return Err(Kind::ResourceLimit.into());
        }
        let unique: HashSet<String> = self
            .read()
            .subscriptions
            .values()
            .filter(|s| s.state != SubscriptionState::None)
            .map(|s| s.receiving_ae_title.clone())
            .collect();
        let mut out: Vec<String> = unique.into_iter().collect();
        out.sort();
        out.truncate(limit as usize);
        Ok(out)
    }
    fn list_events(&self, ctx: &Ctx, q: &EventQuery) -> Result<Vec<Event>> {
        ctx_error(ctx)?;
        if q.limit <= 0 {
            return Err(Kind::ResourceLimit.into());
        }
        let mut result = Vec::new();
        {
            let st = self.read();
            let mut started = q.after_id.is_empty();
            for id in &st.event_order {
                if !started {
                    started = *id == q.after_id;
                    continue;
                }
                let event = &st.events[id];
                if !q.sop_instance_uid.is_empty() && event.sop_instance_uid != q.sop_instance_uid {
                    continue;
                }
                result.push(event.clone());
                if result.len() as i64 == q.limit {
                    break;
                }
            }
        }
        result.iter().map(|e| clone_event(ctx, e)).collect()
    }
    fn list_deliveries(&self, ctx: &Ctx, q: &DeliveryQuery) -> Result<Vec<Delivery>> {
        ctx_error(ctx)?;
        if q.limit <= 0 {
            return Err(Kind::ResourceLimit.into());
        }
        let mut result = Vec::new();
        {
            let st = self.read();
            for id in &st.delivery_order {
                let d = &st.deliveries[id];
                if !q.sop_instance_uid.is_empty() && d.sop_instance_uid != q.sop_instance_uid {
                    continue;
                }
                if !q.receiving_ae_title.is_empty() && d.receiving_ae_title != q.receiving_ae_title
                {
                    continue;
                }
                if !q.states.is_empty() && !q.states.contains(&d.state) {
                    continue;
                }
                result.push(d.clone());
                if result.len() as i64 == q.limit {
                    break;
                }
            }
        }
        result.iter().map(|d| clone_delivery(ctx, d)).collect()
    }
    fn claim_due_deliveries(
        &self,
        ctx: &Ctx,
        now: Time,
        limit: i64,
        lease: Nanos,
    ) -> Result<Vec<Delivery>> {
        ctx_error(ctx)?;
        if limit <= 0 || lease.0 <= 0 {
            return Err(Kind::ResourceLimit.into());
        }
        let mut st = self.write();
        let mut claimed = Vec::new();
        let order = st.delivery_order.clone();
        for id in order {
            if claimed.len() as i64 == limit {
                break;
            }
            let mut d = st.deliveries[&id].clone();
            let due = d.state == DeliveryState::Pending
                || d.state == DeliveryState::Failed
                || (d.state == DeliveryState::Delivering && !d.lease_until.after(now));
            if !due || d.next_attempt_at.after(now) {
                continue;
            }
            st.lease_sequence += 1;
            d.state = DeliveryState::Delivering;
            d.version += 1;
            d.attempts += 1;
            d.lease_token = format!("lease-{:020}", st.lease_sequence);
            d.lease_until = now.add(lease);
            st.deliveries.insert(id, d.clone());
            claimed.push(d);
        }
        claimed
            .iter()
            .map(|d| clone_delivery(&Ctx::background(), d))
            .collect()
    }
    fn complete_delivery(
        &self,
        ctx: &Ctx,
        id: &str,
        lease_token: &str,
        outcome: &DeliveryOutcome,
    ) -> Result<Delivery> {
        ctx_error(ctx)?;
        let mut st = self.write();
        let Some(mut d) = st.deliveries.get(id).cloned() else {
            return Err(Kind::NotFound.into());
        };
        if d.state != DeliveryState::Delivering
            || d.lease_token.is_empty()
            || d.lease_token != lease_token
        {
            return Err(Kind::ConcurrentUpdate.into());
        }
        d.version += 1;
        d.lease_token.clear();
        d.lease_until = Time::ZERO;
        d.last_failure = outcome.failure;
        d.last_dimse_status = outcome.dimse_status;
        if outcome.delivered {
            d.state = DeliveryState::Delivered;
            d.next_attempt_at = Time::ZERO;
        } else if outcome.retryable {
            d.state = DeliveryState::Failed;
            d.next_attempt_at = outcome.next_attempt;
        } else {
            d.state = DeliveryState::Exhausted;
            d.next_attempt_at = Time::ZERO;
        }
        st.deliveries.insert(id.to_string(), d.clone());
        clone_delivery(&Ctx::background(), &d)
    }
    fn commit_ups(&self, ctx: &Ctx, request: CommitRequest) -> Result<CommitResult> {
        ctx_error(ctx)?;
        if request.step.is_none() && request.subscription.is_none() && request.events.is_empty() {
            return Err(Kind::Conflict.into());
        }
        let mut next_step = None;
        if let Some(m) = &request.step {
            if m.next.sop_instance_uid.is_empty() {
                return Err(Kind::Conflict.into());
            }
            next_step = Some(clone_step(ctx, &m.next)?);
        }
        let mut events = Vec::with_capacity(request.events.len());
        for e in &request.events {
            events.push(clone_event(ctx, e)?);
        }
        let mut st = self.write();
        let mut staged = st.subscriptions.clone();
        let mut step_exists = false;
        if let (Some(m), Some(next)) = (&request.step, next_step.as_mut()) {
            let current = st.steps.get(&next.sop_instance_uid);
            step_exists = current.is_some();
            if m.expected_version == 0 {
                if step_exists {
                    return Err(Kind::Conflict.into());
                }
                if st.steps.len() as i64 >= self.max_steps {
                    return Err(Kind::ResourceLimit.into());
                }
                next.version = 1;
                next.sequence = st.sequence + 1;
            } else {
                let Some(current) = current else {
                    return Err(Kind::NotFound.into());
                };
                if current.version != m.expected_version {
                    return Err(Kind::ConcurrentUpdate.into());
                }
                if current.sequence != next.sequence {
                    return Err(Kind::Conflict.into());
                }
                next.version = current.version + 1;
                next.sequence = current.sequence;
            }
        }
        if let Some(next) = &next_step
            && !step_exists
        {
            let snapshot: Vec<Subscription> = staged.values().cloned().collect();
            for s in snapshot {
                ctx_error(ctx)?;
                if !is_global_subscription_uid(&s.sop_instance_uid)
                    || s.state == SubscriptionState::None
                {
                    continue;
                }
                if s.sop_instance_uid == FILTERED_GLOBAL_SUBSCRIPTION_SOP_INSTANCE_UID
                    && !subscription_filter_matches(s.filter.as_ref(), next)
                {
                    continue;
                }
                let key = subscription_key(&next.sop_instance_uid, &s.receiving_ae_title);
                if staged.contains_key(&key) {
                    continue;
                }
                let mut inherited = s.clone();
                inherited.sop_instance_uid = next.sop_instance_uid.clone();
                inherited.filter = None;
                inherited.version = 1;
                staged.insert(key, inherited);
            }
        }
        let mut committed = None;
        if let Some(mutation) = &request.subscription {
            if mutation.receiving_ae_title.is_empty() {
                return Err(Kind::Conflict.into());
            }
            let filtered =
                mutation.sop_instance_uid == FILTERED_GLOBAL_SUBSCRIPTION_SOP_INSTANCE_UID;
            match mutation.kind {
                SubscriptionMutationKind::Subscribe => {
                    if filtered && mutation.filter.is_none() {
                        return Err(Kind::Conflict.into());
                    }
                    if filtered && mutation.filter_scan_limit <= 0 {
                        return Err(Kind::ResourceLimit.into());
                    }
                    if !filtered && mutation.filter.is_some() {
                        return Err(Kind::Conflict.into());
                    }
                    let state = if mutation.deletion_lock {
                        SubscriptionState::WithDeletionLock
                    } else {
                        SubscriptionState::WithoutLock
                    };
                    let key =
                        subscription_key(&mutation.sop_instance_uid, &mutation.receiving_ae_title);
                    let current_version = staged.get(&key).map_or(0, |s| s.version);
                    let mut next = Subscription {
                        sop_instance_uid: mutation.sop_instance_uid.clone(),
                        receiving_ae_title: mutation.receiving_ae_title.clone(),
                        state,
                        filter: mutation.filter.clone(),
                        version: current_version.wrapping_add(1),
                        updated_at: mutation.updated_at,
                    };
                    if next.version == 0 {
                        next.version = 1;
                    }
                    staged.insert(key, next.clone());
                    if is_global_subscription_uid(&mutation.sop_instance_uid) {
                        let mut fan_out = 0i64;
                        let mut steps = Vec::new();
                        let mut scanned = 0i64;
                        for step in st.steps.values() {
                            ctx_error(ctx)?;
                            scanned += 1;
                            if filtered && scanned > mutation.filter_scan_limit {
                                return Err(Kind::ResourceLimit.into());
                            }
                            if filtered && !subscription_filter_matches(next.filter.as_ref(), step)
                            {
                                continue;
                            }
                            steps.push(step.clone());
                        }
                        steps.sort_by(|a, b| {
                            (a.sequence, &a.sop_instance_uid)
                                .cmp(&(b.sequence, &b.sop_instance_uid))
                        });
                        ctx_error(ctx)?;
                        for step in &steps {
                            ctx_error(ctx)?;
                            let specific_key = subscription_key(
                                &step.sop_instance_uid,
                                &mutation.receiving_ae_title,
                            );
                            let existing = staged.get(&specific_key);
                            if existing.is_some_and(|s| s.state != SubscriptionState::None) {
                                continue;
                            }
                            let version = existing.map_or(0, |s| s.version) + 1;
                            fan_out += 1;
                            if fan_out > self.max_fan_out {
                                return Err(Kind::ResourceLimit.into());
                            }
                            staged.insert(
                                specific_key,
                                Subscription {
                                    sop_instance_uid: step.sop_instance_uid.clone(),
                                    receiving_ae_title: mutation.receiving_ae_title.clone(),
                                    state,
                                    filter: None,
                                    version,
                                    updated_at: mutation.updated_at,
                                },
                            );
                            if mutation.deletion_lock {
                                events.push(state_event_for_step(
                                    step,
                                    &mutation.receiving_ae_title,
                                    mutation.updated_at,
                                ));
                            }
                        }
                    } else {
                        let step = match &next_step {
                            Some(n) if n.sop_instance_uid == mutation.sop_instance_uid => {
                                Some(n.clone())
                            }
                            _ => st.steps.get(&mutation.sop_instance_uid).cloned(),
                        };
                        let Some(step) = step else {
                            return Err(Kind::NotFound.into());
                        };
                        events.push(state_event_for_step(
                            &step,
                            &mutation.receiving_ae_title,
                            mutation.updated_at,
                        ));
                    }
                    committed = Some(next);
                }
                SubscriptionMutationKind::Unsubscribe => {
                    if mutation.sop_instance_uid == GLOBAL_SUBSCRIPTION_SOP_INSTANCE_UID {
                        staged.retain(|_, s| s.receiving_ae_title != mutation.receiving_ae_title);
                    } else {
                        let key = subscription_key(
                            &mutation.sop_instance_uid,
                            &mutation.receiving_ae_title,
                        );
                        let mut current = staged.get(&key).cloned().unwrap_or_default();
                        current.sop_instance_uid = mutation.sop_instance_uid.clone();
                        current.receiving_ae_title = mutation.receiving_ae_title.clone();
                        current.state = SubscriptionState::None;
                        current.version += 1;
                        current.updated_at = mutation.updated_at;
                        staged.insert(key, current);
                    }
                }
                SubscriptionMutationKind::SuspendGlobal => {
                    staged.remove(&subscription_key(
                        GLOBAL_SUBSCRIPTION_SOP_INSTANCE_UID,
                        &mutation.receiving_ae_title,
                    ));
                    staged.remove(&subscription_key(
                        FILTERED_GLOBAL_SUBSCRIPTION_SOP_INSTANCE_UID,
                        &mutation.receiving_ae_title,
                    ));
                }
                SubscriptionMutationKind::Other => return Err(Kind::Conflict.into()),
            }
        }
        reclaim_subscription_tombstones(&mut staged, &st.steps, self.max_subscriptions);
        let active = staged
            .values()
            .filter(|s| s.state != SubscriptionState::None)
            .count() as i64;
        if active > self.max_subscriptions {
            return Err(Kind::ResourceLimit.into());
        }
        let mut staged_deliveries: Vec<Delivery> = Vec::new();
        let mut next_event_sequence = st.event_sequence;
        let mut staged_ids = HashSet::new();
        for event in events.iter_mut() {
            ctx_error(ctx)?;
            if event.sop_instance_uid.is_empty()
                && let Some(n) = &next_step
            {
                event.sop_instance_uid = n.sop_instance_uid.clone();
            }
            if event.sop_instance_uid.is_empty() {
                return Err(Kind::Conflict.into());
            }
            if let Some(n) = &next_step
                && event.sop_instance_uid == n.sop_instance_uid
            {
                event.step_version = n.version;
            }
            if event.id.is_empty() {
                next_event_sequence += 1;
                event.id = format!("event-{next_event_sequence:020}");
            }
            *event = clone_event(ctx, event)?;
            if st.events.contains_key(&event.id) || !staged_ids.insert(event.id.clone()) {
                return Err(Kind::Conflict.into());
            }
            let mut recipients: Vec<String> = Vec::new();
            if !event.direct_receiving_ae.is_empty() {
                recipients.push(event.direct_receiving_ae.clone());
            } else {
                recipients = staged
                    .values()
                    .filter(|s| {
                        s.sop_instance_uid == event.sop_instance_uid
                            && s.state != SubscriptionState::None
                    })
                    .map(|s| s.receiving_ae_title.clone())
                    .collect();
                recipients.sort();
            }
            if recipients.len() as i64 > self.max_fan_out {
                return Err(Kind::ResourceLimit.into());
            }
            for receiving in recipients {
                ctx_error(ctx)?;
                let id = delivery_key(&event.id, &receiving);
                if st.deliveries.contains_key(&id) {
                    continue;
                }
                staged_deliveries.push(Delivery {
                    id,
                    event_id: event.id.clone(),
                    event_type: event.event_type,
                    sop_instance_uid: event.sop_instance_uid.clone(),
                    receiving_ae_title: receiving,
                    event: event.clone(),
                    state: DeliveryState::Pending,
                    version: 1,
                    ..Default::default()
                });
            }
        }
        if events.len() as i64 > self.max_events - st.events.len() as i64
            || staged_deliveries.len() as i64 > self.max_deliveries - st.deliveries.len() as i64
        {
            return Err(Kind::ResourceLimit.into());
        }
        ctx_error(ctx)?;
        if let Some(next) = &next_step {
            st.steps.insert(next.sop_instance_uid.clone(), next.clone());
            if !step_exists {
                st.sequence = next.sequence;
            }
        }
        st.subscriptions = staged;
        st.event_sequence = next_event_sequence;
        for event in events {
            st.event_order.push(event.id.clone());
            st.events.insert(event.id.clone(), event);
        }
        for d in staged_deliveries {
            st.delivery_order.push(d.id.clone());
            st.deliveries.insert(d.id.clone(), d);
        }
        let mut result = CommitResult::default();
        if let Some(next) = &next_step {
            result.step = Some(clone_step(&Ctx::background(), next)?);
        }
        result.subscription = committed;
        Ok(result)
    }
    fn filtered_global_committer(&self) -> Option<&dyn FilteredGlobalSubscriptionCommitter> {
        Some(self)
    }
}
impl FilteredGlobalSubscriptionCommitter for MemoryStore {
    fn commit_filtered_global_subscription(
        &self,
        ctx: &Ctx,
        mutation: SubscriptionMutation,
    ) -> Result<CommitResult> {
        if mutation.kind != SubscriptionMutationKind::Subscribe
            || mutation.sop_instance_uid != FILTERED_GLOBAL_SUBSCRIPTION_SOP_INSTANCE_UID
            || mutation.filter.is_none()
        {
            return Err(Kind::Conflict.into());
        }
        self.commit_ups(
            ctx,
            CommitRequest {
                subscription: Some(mutation),
                ..Default::default()
            },
        )
    }
}
