use super::uri::absolute_code_uri;
use super::*;
use crate::net::ul::{Ctx, Nanos};
use crate::object::text::normalize_uid;
use crate::object::{Element, ElementHeader, Value, Vr, is_valid_uid};
use std::collections::HashSet;
use std::sync::{Arc, Condvar, Mutex};
pub trait Store: Send + Sync {
    fn get_step(&self, ctx: &Ctx, sop_instance_uid: &str) -> Result<Step>;
    fn list_steps(&self, ctx: &Ctx, query: &StepQuery) -> Result<Vec<Step>>;
    fn get_subscription(
        &self,
        ctx: &Ctx,
        sop_instance_uid: &str,
        receiving_ae_title: &str,
    ) -> Result<Subscription>;
    fn list_subscriptions(&self, ctx: &Ctx, query: &SubscriptionQuery)
    -> Result<Vec<Subscription>>;
    fn list_active_receiving_ae_titles(&self, ctx: &Ctx, limit: i64) -> Result<Vec<String>>;
    fn list_events(&self, ctx: &Ctx, query: &EventQuery) -> Result<Vec<Event>>;
    fn list_deliveries(&self, ctx: &Ctx, query: &DeliveryQuery) -> Result<Vec<Delivery>>;
    fn claim_due_deliveries(
        &self,
        ctx: &Ctx,
        now: Time,
        limit: i64,
        lease: Nanos,
    ) -> Result<Vec<Delivery>>;
    fn complete_delivery(
        &self,
        ctx: &Ctx,
        id: &str,
        lease_token: &str,
        outcome: &DeliveryOutcome,
    ) -> Result<Delivery>;
    fn commit_ups(&self, ctx: &Ctx, request: CommitRequest) -> Result<CommitResult>;
    fn filtered_global_committer(&self) -> Option<&dyn FilteredGlobalSubscriptionCommitter> {
        None
    }
}
pub trait FilteredGlobalSubscriptionCommitter {
    fn commit_filtered_global_subscription(
        &self,
        ctx: &Ctx,
        mutation: SubscriptionMutation,
    ) -> Result<CommitResult>;
}
pub type Clock = Arc<dyn Fn() -> Time + Send + Sync>;
#[derive(Clone, Default)]
pub struct ServiceOptions {
    pub limits: Limits,
    pub clock: Option<Clock>,
    pub default_worklist_label: String,
    pub callback_resolver: Option<Arc<dyn CallbackResolver>>,
    pub association_dialer: Option<Arc<dyn AssociationDialer>>,
    pub event_handler: Option<Arc<dyn EventHandler>>,
    pub callback_calling_ae: String,
    pub delivery_limits: DeliveryLimits,
    pub refuse_deletion_locks: bool,
    pub fallback_receiving_ae_titles: Vec<String>,
    pub event_sink: Option<Arc<dyn EventSink>>,
}
pub trait EventSink: Send + Sync {
    fn deliver_ups_event(&self, ctx: &Ctx, delivery: &Delivery) -> (bool, Result<()>);
}
pub(crate) struct Inner {
    pub(crate) store: Arc<dyn Store>,
    pub(crate) limits: Limits,
    pub(crate) clock: Clock,
    pub(crate) default_worklist_label: String,
    pub(crate) callback_resolver: Option<Arc<dyn CallbackResolver>>,
    pub(crate) association_dialer: Arc<dyn AssociationDialer>,
    pub(crate) event_handler: Option<Arc<dyn EventHandler>>,
    pub(crate) callback_calling_ae: String,
    pub(crate) delivery_limits: DeliveryLimits,
    pub(crate) refuse_deletion_locks: bool,
    pub(crate) fallback_receiving_aes: Vec<String>,
    pub(crate) event_sink: Option<Arc<dyn EventSink>>,
    pub(crate) wake: (Mutex<bool>, Condvar),
}
#[derive(Clone)]
pub struct Service {
    pub(crate) inner: Arc<Inner>,
}
impl Service {
    pub fn new(store: Option<Arc<dyn Store>>, options: ServiceOptions) -> Result<Service> {
        let Some(store) = store else {
            return Err(Kind::Conflict.into());
        };
        let limits = normalize_limits(options.limits)?;
        let clock = options
            .clock
            .unwrap_or_else(|| Arc::new(dimse::StorageCommitmentTime::now));
        let mut label = options.default_worklist_label;
        if label.is_empty() {
            label = "DEFAULT".into();
        }
        let label = label.trim().to_string();
        if !valid_default_worklist_label(&label) {
            return Err(Kind::InvalidDataSet.into());
        }
        let delivery_limits = normalize_delivery_limits(options.delivery_limits)?;
        let dialer = options
            .association_dialer
            .unwrap_or_else(|| Arc::new(DefaultAssociationDialer));
        let mut calling = options.callback_calling_ae;
        if calling.is_empty() {
            calling = "DCMRUST_UPS".into();
        }
        let calling = calling.trim().to_string();
        if !valid_ae_title(&calling) {
            return Err(Kind::InvalidDataSet.into());
        }
        let mut fallback = Vec::new();
        let mut seen = HashSet::new();
        for ae in &options.fallback_receiving_ae_titles {
            let ae = ae.trim().to_string();
            if !valid_ae_title(&ae) {
                return Err(Kind::InvalidDataSet.into());
            }
            if seen.insert(ae.clone()) {
                fallback.push(ae);
            }
        }
        Ok(Service {
            inner: Arc::new(Inner {
                store,
                limits,
                clock,
                default_worklist_label: label,
                callback_resolver: options.callback_resolver,
                association_dialer: dialer,
                event_handler: options.event_handler,
                callback_calling_ae: calling,
                delivery_limits,
                refuse_deletion_locks: options.refuse_deletion_locks,
                fallback_receiving_aes: fallback,
                event_sink: options.event_sink,
                wake: (Mutex::new(false), Condvar::new()),
            }),
        })
    }
    pub(crate) fn now(&self) -> Time {
        (self.inner.clock)()
    }
    pub fn create(&self, ctx: &Ctx, request: &CreateRequest<'_>) -> Result<Step> {
        const OP: &str = "N-CREATE";
        let limits = &self.inner.limits;
        if !valid_uid(&request.sop_instance_uid) {
            return Err(status_error(
                OP,
                STATUS_INVALID_OBJECT_INSTANCE,
                Kind::InvalidDataSet.into(),
            ));
        }
        let Some(attributes) = request.attributes else {
            return Err(status_error(
                OP,
                STATUS_MISSING_ATTRIBUTE,
                Kind::InvalidDataSet.into(),
            ));
        };
        let mut attributes = clone_data_set(ctx, &attributes.to_data_set(), limits)
            .map_err(|e| classify_data_set_error(OP, e))?;
        validate_data_set(ctx, &attributes, limits).map_err(|e| classify_data_set_error(OP, e))?;
        self.validate_create_attributes(&mut attributes)?;
        let now = self.now();
        put_element(
            &mut attributes,
            string_element(TAG_SOP_CLASS_UID, Vr::UI, &[PUSH_SOP_CLASS_UID]),
        );
        put_element(
            &mut attributes,
            string_element(TAG_SOP_INSTANCE_UID, Vr::UI, &[&request.sop_instance_uid]),
        );
        put_element(
            &mut attributes,
            string_element(
                TAG_SCHEDULED_PROCEDURE_STEP_MODIFICATION_DATE_TIME,
                Vr::DT,
                &[&dicom_date_time(now)],
            ),
        );
        put_element(
            &mut attributes,
            string_element(TAG_PROCEDURE_STEP_STATE, Vr::CS, &[STATE_SCHEDULED]),
        );
        put_element(
            &mut attributes,
            string_element(TAG_TRANSACTION_UID, Vr::UI, &[""]),
        );
        validate_data_set_encoding(ctx, &attributes).map_err(|e| classify_data_set_error(OP, e))?;
        let step = Step {
            sop_instance_uid: request.sop_instance_uid.clone(),
            state: STATE_SCHEDULED.into(),
            attributes,
            created_at: now,
            updated_at: now,
            ..Default::default()
        };
        let events = self.events_for_create(&step, now);
        let result = self.inner.store.commit_ups(
            ctx,
            CommitRequest {
                step: Some(StepMutation {
                    expected_version: 0,
                    next: step,
                }),
                events,
                ..Default::default()
            },
        );
        match result {
            Err(e) if e.is(Kind::Conflict) => {
                Err(status_error(OP, STATUS_DUPLICATE_SOP_INSTANCE, e))
            }
            Err(e) => Err(safe_repository_error(e)),
            Ok(r) => match r.step {
                None => Err(safe_repository_error(Kind::Conflict.into())),
                Some(step) => {
                    self.wake_deliveries();
                    Ok(step)
                }
            },
        }
    }
    pub fn get(&self, ctx: &Ctx, sop_instance_uid: &str) -> Result<Step> {
        match self.inner.store.get_step(ctx, sop_instance_uid) {
            Err(e) if e.is(Kind::NotFound) => Err(status_error("N-GET", STATUS_UPS_NOT_FOUND, e)),
            Err(e) => Err(safe_repository_error(e)),
            Ok(s) => Ok(s),
        }
    }
    pub fn set(&self, ctx: &Ctx, request: &SetRequest<'_>) -> Result<Step> {
        const OP: &str = "N-SET";
        let limits = self.inner.limits;
        let Some(modifications) = request.modifications else {
            return Err(classify_data_set_error(OP, Kind::InvalidDataSet.into()));
        };
        let mut modifications = clone_data_set(ctx, &modifications.to_data_set(), &limits)
            .map_err(|e| classify_data_set_error(OP, e))?;
        validate_data_set(ctx, &modifications, &limits)
            .map_err(|e| classify_data_set_error(OP, e))?;
        let mut transaction = request.transaction_uid.clone();
        if let Some(uid) = data_set_string(&modifications, TAG_TRANSACTION_UID) {
            if transaction.is_empty() {
                transaction = uid;
            } else if normalize_uid(&uid) != normalize_uid(&transaction) {
                return Err(status_error(
                    OP,
                    STATUS_INCORRECT_TRANSACTION_UID,
                    Kind::Conflict.into(),
                ));
            }
            remove_element(&mut modifications, TAG_TRANSACTION_UID);
        }
        self.update_step(ctx, OP, &request.sop_instance_uid, |mut step: Step| {
            if step.state == STATE_COMPLETED || step.state == STATE_CANCELED {
                return Err(status_error(
                    OP,
                    STATUS_MAY_NO_LONGER_BE_UPDATED,
                    Kind::InvalidState.into(),
                ));
            }
            if step.state == STATE_IN_PROGRESS
                && normalize_uid(&step.transaction_uid) != normalize_uid(&transaction)
            {
                return Err(status_error(
                    OP,
                    STATUS_INCORRECT_TRANSACTION_UID,
                    Kind::Conflict.into(),
                ));
            }
            if step.state == STATE_SCHEDULED && !transaction.is_empty() {
                return Err(status_error(
                    OP,
                    STATUS_INCORRECT_TRANSACTION_UID,
                    Kind::Conflict.into(),
                ));
            }
            let mut effective = DataSet::default();
            for element in &modifications.elements {
                if !SETTABLE_UPS_ATTRIBUTES.contains(&element.tag()) {
                    return Err(status_error(
                        OP,
                        STATUS_NO_SUCH_ATTRIBUTE,
                        Kind::InvalidDataSet.into(),
                    ));
                }
                if let Err(e) = validate_set_element(element) {
                    return Err(status_error(OP, STATUS_INVALID_ATTRIBUTE_VALUE, e));
                }
                let Some(current) = data_set_element(&step.attributes, element.tag()) else {
                    return Err(status_error(
                        OP,
                        STATUS_NO_SUCH_ATTRIBUTE,
                        Kind::InvalidDataSet.into(),
                    ));
                };
                if current == element {
                    continue;
                }
                put_element(&mut step.attributes, element.clone());
                effective.elements.push(element.clone());
            }
            if effective.elements.is_empty() {
                return Err(Kind::NoChange.into());
            }
            let now = self.now();
            put_element(
                &mut step.attributes,
                string_element(
                    TAG_SCHEDULED_PROCEDURE_STEP_MODIFICATION_DATE_TIME,
                    Vr::DT,
                    &[&dicom_date_time(now)],
                ),
            );
            validate_data_set(ctx, &step.attributes, &limits)
                .map_err(|e| classify_data_set_error(OP, e))?;
            validate_data_set_encoding(ctx, &step.attributes)
                .map_err(|e| classify_data_set_error(OP, e))?;
            step.updated_at = now;
            let events = self.events_for_modification(&step, &effective, now);
            Ok((step, events))
        })
    }
    pub fn change_state(&self, ctx: &Ctx, request: &ChangeStateRequest) -> Result<Step> {
        const OP: &str = "N-ACTION Change UPS State";
        self.update_step(ctx, OP, &request.sop_instance_uid, |mut step: Step| {
            let target = request.state.as_str();
            if target == STATE_SCHEDULED || !valid_state(target) {
                return Err(status_error(
                    OP,
                    STATUS_ONLY_SCHEDULED_VIA_CREATE,
                    Kind::InvalidState.into(),
                ));
            }
            if !valid_uid(&request.transaction_uid) {
                return Err(status_error(
                    OP,
                    STATUS_INCORRECT_TRANSACTION_UID,
                    Kind::InvalidDataSet.into(),
                ));
            }
            if step.state != STATE_SCHEDULED
                && normalize_uid(&step.transaction_uid) != normalize_uid(&request.transaction_uid)
            {
                return Err(status_error(
                    OP,
                    STATUS_INCORRECT_TRANSACTION_UID,
                    Kind::Conflict.into(),
                ));
            }
            let fail = |status: u16| Err(status_error(OP, status, Kind::InvalidState.into()));
            match step.state.as_str() {
                STATE_SCHEDULED => {
                    if target != STATE_IN_PROGRESS {
                        return fail(STATUS_NOT_IN_PROGRESS);
                    }
                    step.state = STATE_IN_PROGRESS.into();
                    step.transaction_uid = normalize_uid(&request.transaction_uid).to_string();
                }
                STATE_IN_PROGRESS => {
                    if target == STATE_IN_PROGRESS {
                        return fail(STATUS_ALREADY_IN_PROGRESS);
                    }
                    if target != STATE_COMPLETED && target != STATE_CANCELED {
                        return fail(STATUS_MAY_NO_LONGER_BE_UPDATED);
                    }
                    if target == STATE_CANCELED {
                        self.fill_cancellation_attributes(&mut step.attributes, false);
                    }
                    if let Err(e) = validate_final_state(&step.attributes, target) {
                        return Err(status_error(OP, STATUS_FINAL_STATE_REQUIREMENTS_NOT_MET, e));
                    }
                    step.state = target.to_string();
                }
                STATE_COMPLETED => {
                    if target == STATE_COMPLETED {
                        return fail(STATUS_ALREADY_COMPLETED);
                    }
                    return fail(STATUS_MAY_NO_LONGER_BE_UPDATED);
                }
                STATE_CANCELED => {
                    if target == STATE_CANCELED {
                        return fail(STATUS_ALREADY_CANCELED);
                    }
                    return fail(STATUS_MAY_NO_LONGER_BE_UPDATED);
                }
                _ => return fail(STATUS_MAY_NO_LONGER_BE_UPDATED),
            }
            let now = self.now();
            put_element(
                &mut step.attributes,
                string_element(TAG_PROCEDURE_STEP_STATE, Vr::CS, &[&step.state]),
            );
            let uid = step.transaction_uid.clone();
            put_element(
                &mut step.attributes,
                string_element(TAG_TRANSACTION_UID, Vr::UI, &[&uid]),
            );
            step.updated_at = now;
            let event = self.state_event(&step, now);
            Ok((step, vec![event]))
        })
    }
    pub fn request_cancel(&self, ctx: &Ctx, request: &CancelRequest<'_>) -> Result<Step> {
        const OP: &str = "N-ACTION Request UPS Cancel";
        let requesting = request.requesting_ae_title.trim().to_string();
        if !valid_ae_title(&requesting) {
            return Err(status_error(
                OP,
                STATUS_RECEIVING_AE_UNKNOWN,
                Kind::InvalidDataSet.into(),
            ));
        }
        let limits = self.inner.limits;
        self.update_step(ctx, OP, &request.sop_instance_uid, |mut step: Step| {
            match step.state.as_str() {
                STATE_COMPLETED => {
                    return Err(status_error(
                        OP,
                        STATUS_ALREADY_COMPLETED_CANCEL_REQUEST,
                        Kind::InvalidState.into(),
                    ));
                }
                STATE_CANCELED => {
                    return Err(status_error(
                        OP,
                        STATUS_ALREADY_CANCELED,
                        Kind::InvalidState.into(),
                    ));
                }
                _ => {}
            }
            let now = self.now();
            let mut information = item_of(vec![string_element(
                TAG_REQUESTING_AE,
                Vr::AE,
                &[&requesting],
            )]);
            if let Some(info) = request.information {
                let input = clone_data_set(ctx, &info.to_data_set(), &limits)
                    .map_err(|e| classify_data_set_error(OP, e))?;
                validate_data_set(ctx, &input, &limits)
                    .map_err(|e| classify_data_set_error(OP, e))?;
                if let Err(e) = validate_cancel_information(&input, false) {
                    return Err(status_error(OP, STATUS_INVALID_ATTRIBUTE_VALUE, e));
                }
                for tag in [
                    TAG_REASON_FOR_CANCELLATION,
                    TAG_PROCEDURE_STEP_DISCONTINUATION_REASON_CODE_SEQUENCE,
                    TAG_CONTACT_URI,
                    TAG_CONTACT_DISPLAY_NAME,
                ] {
                    if let Some(e) = data_set_element(&input, tag) {
                        put_element(&mut information, e.clone());
                    }
                }
            }
            let event = Event {
                event_type: EVENT_CANCEL_REQUESTED,
                sop_instance_uid: step.sop_instance_uid.clone(),
                information: information.clone(),
                created_at: now,
                ..Default::default()
            };
            if step.state == STATE_SCHEDULED {
                step.state = STATE_CANCELED.into();
                step.transaction_uid =
                    internal_transaction_uid(&step.sop_instance_uid, step.version + 1);
                if let Some(d) = data_set_element(
                    &information,
                    TAG_PROCEDURE_STEP_DISCONTINUATION_REASON_CODE_SEQUENCE,
                ) {
                    let progress = item_of(vec![d.clone()]);
                    put_element(
                        &mut step.attributes,
                        sequence_element(
                            TAG_PROCEDURE_STEP_PROGRESS_INFORMATION_SEQUENCE,
                            vec![progress],
                        ),
                    );
                }
                self.fill_cancellation_attributes(&mut step.attributes, true);
                put_element(
                    &mut step.attributes,
                    string_element(TAG_PROCEDURE_STEP_STATE, Vr::CS, &[STATE_CANCELED]),
                );
                let uid = step.transaction_uid.clone();
                put_element(
                    &mut step.attributes,
                    string_element(TAG_TRANSACTION_UID, Vr::UI, &[&uid]),
                );
                if let Err(e) = validate_final_state(&step.attributes, STATE_CANCELED) {
                    return Err(status_error(OP, STATUS_FINAL_STATE_REQUIREMENTS_NOT_MET, e));
                }
                step.updated_at = now;
                let events = vec![
                    self.state_event_with_state(&step, STATE_IN_PROGRESS, now),
                    self.state_event(&step, now),
                    event,
                ];
                return Ok((step, events));
            }
            Ok((step, vec![event]))
        })
    }
    pub fn events(&self, ctx: &Ctx, query: &EventQuery) -> Result<Vec<Event>> {
        let mut query = query.clone();
        if query.limit == 0 {
            query.limit = 1_000;
        }
        if query.limit < 0 || query.limit > 1_000 {
            return Err(Kind::ResourceLimit.into());
        }
        self.inner
            .store
            .list_events(ctx, &query)
            .map_err(safe_repository_error)
    }
    fn update_step(
        &self,
        ctx: &Ctx,
        operation: &str,
        sop_instance_uid: &str,
        mut mutate: impl FnMut(Step) -> Result<(Step, Vec<Event>)>,
    ) -> Result<Step> {
        for _ in 0..self.inner.limits.max_cas_attempts {
            let step = match self.inner.store.get_step(ctx, sop_instance_uid) {
                Err(e) if e.is(Kind::NotFound) => {
                    return Err(status_error(operation, STATUS_UPS_NOT_FOUND, e));
                }
                Err(e) => return Err(safe_repository_error(e)),
                Ok(s) => s,
            };
            let version = step.version;
            let (next, events) = match mutate(step.clone()) {
                Err(e) if e.is(Kind::NoChange) => return Ok(step),
                Err(e) => return Err(e),
                Ok(r) => r,
            };
            let has_events = !events.is_empty();
            let result = self.inner.store.commit_ups(
                ctx,
                CommitRequest {
                    step: Some(StepMutation {
                        expected_version: version,
                        next,
                    }),
                    events,
                    ..Default::default()
                },
            );
            match result {
                Err(e) if e.is(Kind::ConcurrentUpdate) => continue,
                Err(e) => return Err(safe_repository_error(e)),
                Ok(r) => {
                    let Some(step) = r.step else {
                        return Err(safe_repository_error(Kind::Conflict.into()));
                    };
                    if has_events {
                        self.wake_deliveries();
                    }
                    return Ok(step);
                }
            }
        }
        Err(Kind::ConcurrentUpdate.into())
    }
    fn validate_create_attributes(&self, ds: &mut DataSet) -> Result<()> {
        const OP: &str = "N-CREATE";
        let invalid = || Error::from(Kind::InvalidDataSet);
        let Some(state) = data_set_string(ds, TAG_PROCEDURE_STEP_STATE) else {
            return Err(status_error(OP, STATUS_MISSING_ATTRIBUTE, invalid()));
        };
        if state.trim().is_empty() {
            return Err(status_error(OP, STATUS_MISSING_ATTRIBUTE_VALUE, invalid()));
        }
        if state != STATE_SCHEDULED {
            return Err(status_error(
                OP,
                STATUS_CREATE_STATE_NOT_SCHEDULED,
                Kind::InvalidState.into(),
            ));
        }
        let Some(transaction) = data_set_string(ds, TAG_TRANSACTION_UID) else {
            return Err(status_error(OP, STATUS_MISSING_ATTRIBUTE, invalid()));
        };
        if !transaction.is_empty() {
            return Err(status_error(OP, STATUS_INVALID_ATTRIBUTE_VALUE, invalid()));
        }
        if let Some(class) = data_set_string(ds, TAG_SOP_CLASS_UID)
            && normalize_uid(&class) != PUSH_SOP_CLASS_UID
        {
            return Err(status_error(OP, STATUS_INVALID_ATTRIBUTE_VALUE, invalid()));
        }
        if data_set_element(ds, TAG_SOP_INSTANCE_UID).is_some() {
            return Err(status_error(OP, STATUS_NO_SUCH_ATTRIBUTE, invalid()));
        }
        for e in &ds.elements {
            if !CREATE_UPS_ATTRIBUTES.contains(&e.tag()) {
                return Err(status_error(OP, STATUS_NO_SUCH_ATTRIBUTE, invalid()));
            }
        }
        for tag in [
            TAG_SCHEDULED_PROCEDURE_STEP_PRIORITY,
            TAG_PROCEDURE_STEP_LABEL,
            TAG_SCHEDULED_PROCEDURE_STEP_START_DATE_TIME,
            TAG_INPUT_READINESS_STATE,
        ] {
            let Some(value) = data_set_string(ds, tag) else {
                return Err(status_error(OP, STATUS_MISSING_ATTRIBUTE, invalid()));
            };
            if value.trim().is_empty() {
                return Err(status_error(OP, STATUS_MISSING_ATTRIBUTE_VALUE, invalid()));
            }
            if tag == TAG_INPUT_READINESS_STATE
                && let Some(e) = data_set_element(ds, tag)
                && let Err(err) = validate_set_element(e)
            {
                return Err(status_error(OP, STATUS_INVALID_ATTRIBUTE_VALUE, err));
            }
            if tag == TAG_SCHEDULED_PROCEDURE_STEP_PRIORITY
                && value != "HIGH"
                && value != "MEDIUM"
                && value != "LOW"
            {
                return Err(status_error(OP, STATUS_INVALID_ATTRIBUTE_VALUE, invalid()));
            }
        }
        match data_set_string(ds, TAG_WORKLIST_LABEL) {
            None => return Err(status_error(OP, STATUS_MISSING_ATTRIBUTE, invalid())),
            Some(v) if v.is_empty() => put_element(
                ds,
                string_element(
                    TAG_WORKLIST_LABEL,
                    Vr::LO,
                    &[&self.inner.default_worklist_label],
                ),
            ),
            Some(_) => {}
        }
        if data_set_element(ds, TAG_STUDY_INSTANCE_UID).is_none() {
            put_element(ds, string_element(TAG_STUDY_INSTANCE_UID, Vr::UI, &[""]));
        }
        if data_set_element(ds, TAG_PATIENT_ID).is_none() {
            put_element(ds, string_element(TAG_PATIENT_ID, Vr::LO, &[""]));
        }
        for tag in CREATE_TYPE_TWO_TAGS {
            if data_set_element(ds, tag).is_none() {
                return Err(status_error(OP, STATUS_MISSING_ATTRIBUTE, invalid()));
            }
        }
        for tag in [
            TAG_PROCEDURE_STEP_PROGRESS_INFORMATION_SEQUENCE,
            TAG_UNIFIED_PROCEDURE_STEP_PERFORMED_PROCEDURE_SEQUENCE,
        ] {
            let ok = data_set_element(ds, tag).is_some_and(|e| {
                e.vr() == Vr::SQ && sequence_items(e).is_some_and(|items| items.is_empty())
            });
            if !ok {
                return Err(status_error(OP, STATUS_INVALID_ATTRIBUTE_VALUE, invalid()));
            }
        }
        if let Some(e) = data_set_element(ds, TAG_SCHEDULED_HUMAN_PERFORMERS_SEQUENCE)
            && let Err(err) = validate_set_element(e)
        {
            return Err(status_error(OP, STATUS_INVALID_ATTRIBUTE_VALUE, err));
        }
        for tag in [
            TAG_SCHEDULED_STATION_NAME_CODE_SEQUENCE,
            TAG_SCHEDULED_STATION_CLASS_CODE_SEQUENCE,
            TAG_SCHEDULED_STATION_GEOGRAPHIC_LOCATION_CODE_SEQUENCE,
            TAG_SCHEDULED_WORKITEM_CODE_SEQUENCE,
            TAG_ADMITTING_DIAGNOSES_CODE_SEQUENCE,
        ] {
            let e = element_or_zero(ds, tag);
            if let Err(err) =
                validate_optional_code_sequence(&e, tag == TAG_SCHEDULED_WORKITEM_CODE_SEQUENCE)
            {
                return Err(status_error(OP, STATUS_INVALID_ATTRIBUTE_VALUE, err));
            }
        }
        if let Err(err) = validate_referenced_request_sequence(ds) {
            return Err(status_error(OP, STATUS_INVALID_ATTRIBUTE_VALUE, err));
        }
        if let Some(e) = data_set_element(ds, TAG_REPLACED_PROCEDURE_STEP_SEQUENCE)
            && let Err(err) = validate_sop_instance_reference_sequence(e)
        {
            return Err(status_error(OP, STATUS_INVALID_ATTRIBUTE_VALUE, err));
        }
        if let Err(err) =
            validate_referenced_instances_and_access(ds, TAG_INPUT_INFORMATION_SEQUENCE)
        {
            return Err(status_error(OP, STATUS_INVALID_ATTRIBUTE_VALUE, err));
        }
        if validate_content_item_sequence(&element_or_zero(
            ds,
            TAG_SCHEDULED_PROCESSING_PARAMETERS_SEQUENCE,
        ))
        .is_err()
        {
            return Err(status_error(OP, STATUS_INVALID_ATTRIBUTE_VALUE, invalid()));
        }
        if let Err(err) = validate_patient_identification_sequences(ds) {
            return Err(status_error(OP, STATUS_INVALID_ATTRIBUTE_VALUE, err));
        }
        if validate_hierarchic_designator_sequence(&element_or_zero(
            ds,
            TAG_ISSUER_OF_ADMISSION_ID_SEQUENCE,
        ))
        .is_err()
        {
            return Err(status_error(OP, STATUS_INVALID_ATTRIBUTE_VALUE, invalid()));
        }
        Ok(())
    }
    fn fill_cancellation_attributes(&self, ds: &mut DataSet, synthesize_reason: bool) {
        let mut items = data_set_element(ds, TAG_PROCEDURE_STEP_PROGRESS_INFORMATION_SEQUENCE)
            .and_then(sequence_items)
            .filter(|items| !items.is_empty())
            .cloned()
            .unwrap_or_else(|| vec![DataSet::default()]);
        let mut item = items[0].clone();
        match data_set_string(&item, TAG_PROCEDURE_STEP_CANCELLATION_DATE_TIME) {
            Some(v) if !v.is_empty() => {}
            _ => put_element(
                &mut item,
                string_element(
                    TAG_PROCEDURE_STEP_CANCELLATION_DATE_TIME,
                    Vr::DT,
                    &[&dicom_date_time(self.now())],
                ),
            ),
        }
        if data_set_element(
            &item,
            TAG_PROCEDURE_STEP_DISCONTINUATION_REASON_CODE_SEQUENCE,
        )
        .is_none()
            && synthesize_reason
        {
            put_element(
                &mut item,
                code_sequence(
                    TAG_PROCEDURE_STEP_DISCONTINUATION_REASON_CODE_SEQUENCE,
                    &Code {
                        value: "CANCEL".into(),
                        scheme: "99DCMRUST".into(),
                        meaning: "Canceled by UPS request".into(),
                    },
                ),
            );
        }
        items[0] = item;
        put_element(
            ds,
            sequence_element(TAG_PROCEDURE_STEP_PROGRESS_INFORMATION_SEQUENCE, items),
        );
    }
    fn events_for_modification(
        &self,
        step: &Step,
        modifications: &DataSet,
        now: Time,
    ) -> Vec<Event> {
        let mut events = Vec::new();
        if data_set_element(modifications, TAG_INPUT_READINESS_STATE).is_some() {
            events.push(self.state_event(step, now));
        }
        if data_set_element(
            modifications,
            TAG_PROCEDURE_STEP_PROGRESS_INFORMATION_SEQUENCE,
        )
        .is_some()
        {
            let mut information = DataSet::default();
            if let Some(progress) = data_set_element(
                &step.attributes,
                TAG_PROCEDURE_STEP_PROGRESS_INFORMATION_SEQUENCE,
            ) && sequence_items(progress).is_some_and(|i| !i.is_empty())
            {
                information.elements.push(progress.clone());
            }
            if !information.elements.is_empty() {
                events.push(Event {
                    event_type: EVENT_PROGRESS_REPORT,
                    sop_instance_uid: step.sop_instance_uid.clone(),
                    information,
                    created_at: now,
                    ..Default::default()
                });
            }
        }
        for tag in [
            TAG_SCHEDULED_STATION_NAME_CODE_SEQUENCE,
            TAG_SCHEDULED_HUMAN_PERFORMERS_SEQUENCE,
        ] {
            if data_set_element(modifications, tag).is_some() {
                events.extend(self.assigned_events(step, now));
                break;
            }
        }
        events
    }
    fn events_for_create(&self, step: &Step, now: Time) -> Vec<Event> {
        let mut events = vec![self.state_event(step, now)];
        if has_non_empty_sequence(&step.attributes, TAG_SCHEDULED_STATION_NAME_CODE_SEQUENCE)
            || has_non_empty_sequence(&step.attributes, TAG_SCHEDULED_HUMAN_PERFORMERS_SEQUENCE)
        {
            events.extend(self.assigned_events(step, now));
        }
        events
    }
    fn assigned_events(&self, step: &Step, now: Time) -> Vec<Event> {
        let station = data_set_element(&step.attributes, TAG_SCHEDULED_STATION_NAME_CODE_SEQUENCE)
            .filter(|_| {
                has_non_empty_sequence(&step.attributes, TAG_SCHEDULED_STATION_NAME_CODE_SEQUENCE)
            });
        let humans = data_set_element(&step.attributes, TAG_SCHEDULED_HUMAN_PERFORMERS_SEQUENCE)
            .and_then(sequence_items)
            .filter(|items| !items.is_empty());
        let event = |information: DataSet| Event {
            event_type: EVENT_UPS_ASSIGNED,
            sop_instance_uid: step.sop_instance_uid.clone(),
            information,
            created_at: now,
            ..Default::default()
        };
        let Some(humans) = humans else {
            return match station {
                None => vec![event(DataSet::default())],
                Some(s) => vec![event(item_of(vec![s.clone()]))],
            };
        };
        humans
            .iter()
            .map(|human| {
                let mut information = DataSet::default();
                if let Some(s) = station {
                    information.elements.push(s.clone());
                }
                if let Some(c) = data_set_element(human, TAG_HUMAN_PERFORMER_CODE_SEQUENCE) {
                    information.elements.push(c.clone());
                }
                if let Some(o) = data_set_element(human, TAG_HUMAN_PERFORMER_ORGANIZATION) {
                    information.elements.push(o.clone());
                }
                event(information)
            })
            .collect()
    }
    pub(crate) fn state_event(&self, step: &Step, now: Time) -> Event {
        self.state_event_with_state(step, &step.state, now)
    }
    fn state_event_with_state(&self, step: &Step, state: &str, now: Time) -> Event {
        let mut information = item_of(vec![string_element(
            TAG_PROCEDURE_STEP_STATE,
            Vr::CS,
            &[state],
        )]);
        if let Some(r) = data_set_element(&step.attributes, TAG_INPUT_READINESS_STATE) {
            information.elements.push(r.clone());
        }
        Event {
            event_type: EVENT_STATE_REPORT,
            sop_instance_uid: step.sop_instance_uid.clone(),
            information,
            created_at: now,
            ..Default::default()
        }
    }
}
pub(crate) fn validate_cancel_information(ds: &DataSet, allow_requesting_ae: bool) -> Result<()> {
    for e in &ds.elements {
        let tag = e.tag();
        let allowed = matches!(
            tag,
            TAG_REASON_FOR_CANCELLATION
                | TAG_PROCEDURE_STEP_DISCONTINUATION_REASON_CODE_SEQUENCE
                | TAG_CONTACT_URI
                | TAG_CONTACT_DISPLAY_NAME
        ) || (allow_requesting_ae && tag == TAG_REQUESTING_AE);
        if !allowed {
            return Err(Kind::InvalidDataSet.into());
        }
        match tag {
            TAG_PROCEDURE_STEP_DISCONTINUATION_REASON_CODE_SEQUENCE => {
                if !required_single_code_sequence(e) {
                    return Err(Kind::InvalidDataSet.into());
                }
            }
            TAG_REASON_FOR_CANCELLATION | TAG_CONTACT_URI | TAG_CONTACT_DISPLAY_NAME => {
                let values = element_string_values(e);
                if values.len() != 1 || values[0].trim().is_empty() {
                    return Err(Kind::InvalidDataSet.into());
                }
            }
            TAG_REQUESTING_AE => {
                let values = element_string_values(e);
                if values.len() != 1 || !valid_ae_title(&values[0]) {
                    return Err(Kind::InvalidDataSet.into());
                }
            }
            _ => {}
        }
    }
    Ok(())
}
pub(crate) const CREATE_TYPE_TWO_TAGS: [Tag; 22] = [
    TAG_TRANSACTION_UID,
    TAG_WORKLIST_LABEL,
    TAG_SCHEDULED_PROCESSING_PARAMETERS_SEQUENCE,
    TAG_SCHEDULED_STATION_NAME_CODE_SEQUENCE,
    TAG_SCHEDULED_STATION_CLASS_CODE_SEQUENCE,
    TAG_SCHEDULED_STATION_GEOGRAPHIC_LOCATION_CODE_SEQUENCE,
    TAG_SCHEDULED_WORKITEM_CODE_SEQUENCE,
    TAG_COMMENTS_ON_SCHEDULED_PROCEDURE_STEP,
    TAG_INPUT_INFORMATION_SEQUENCE,
    TAG_PROCEDURE_STEP_PROGRESS_INFORMATION_SEQUENCE,
    TAG_UNIFIED_PROCEDURE_STEP_PERFORMED_PROCEDURE_SEQUENCE,
    TAG_PATIENT_NAME,
    TAG_ISSUER_OF_PATIENT_ID,
    TAG_ISSUER_OF_PATIENT_ID_QUALIFIERS_SEQUENCE,
    TAG_OTHER_PATIENT_IDS_SEQUENCE,
    TAG_PATIENT_BIRTH_DATE,
    TAG_PATIENT_SEX,
    TAG_ADMISSION_ID,
    TAG_ISSUER_OF_ADMISSION_ID_SEQUENCE,
    TAG_ADMITTING_DIAGNOSES_DESCRIPTION,
    TAG_ADMITTING_DIAGNOSES_CODE_SEQUENCE,
    TAG_REFERENCED_REQUEST_SEQUENCE,
];
pub(crate) const CREATE_UPS_ATTRIBUTES: [Tag; 35] = [
    TAG_SPECIFIC_CHARACTER_SET,
    TAG_TIMEZONE_OFFSET_FROM_UTC,
    TAG_SOP_CLASS_UID,
    TAG_TRANSACTION_UID,
    TAG_STUDY_INSTANCE_UID,
    TAG_SCHEDULED_PROCEDURE_STEP_PRIORITY,
    TAG_PROCEDURE_STEP_LABEL,
    TAG_WORKLIST_LABEL,
    TAG_SCHEDULED_PROCESSING_PARAMETERS_SEQUENCE,
    TAG_SCHEDULED_STATION_NAME_CODE_SEQUENCE,
    TAG_SCHEDULED_STATION_CLASS_CODE_SEQUENCE,
    TAG_SCHEDULED_STATION_GEOGRAPHIC_LOCATION_CODE_SEQUENCE,
    TAG_SCHEDULED_HUMAN_PERFORMERS_SEQUENCE,
    TAG_SCHEDULED_PROCEDURE_STEP_START_DATE_TIME,
    TAG_EXPECTED_COMPLETION_DATE_TIME,
    TAG_SCHEDULED_WORKITEM_CODE_SEQUENCE,
    TAG_COMMENTS_ON_SCHEDULED_PROCEDURE_STEP,
    TAG_INPUT_READINESS_STATE,
    TAG_INPUT_INFORMATION_SEQUENCE,
    TAG_PATIENT_NAME,
    TAG_PATIENT_ID,
    TAG_ISSUER_OF_PATIENT_ID,
    TAG_ISSUER_OF_PATIENT_ID_QUALIFIERS_SEQUENCE,
    TAG_OTHER_PATIENT_IDS_SEQUENCE,
    TAG_PATIENT_BIRTH_DATE,
    TAG_PATIENT_SEX,
    TAG_ADMISSION_ID,
    TAG_ISSUER_OF_ADMISSION_ID_SEQUENCE,
    TAG_ADMITTING_DIAGNOSES_DESCRIPTION,
    TAG_ADMITTING_DIAGNOSES_CODE_SEQUENCE,
    TAG_REFERENCED_REQUEST_SEQUENCE,
    TAG_REPLACED_PROCEDURE_STEP_SEQUENCE,
    TAG_PROCEDURE_STEP_STATE,
    TAG_PROCEDURE_STEP_PROGRESS_INFORMATION_SEQUENCE,
    TAG_UNIFIED_PROCEDURE_STEP_PERFORMED_PROCEDURE_SEQUENCE,
];
pub(crate) const SETTABLE_UPS_ATTRIBUTES: [Tag; 18] = [
    TAG_SPECIFIC_CHARACTER_SET,
    TAG_STUDY_INSTANCE_UID,
    TAG_SCHEDULED_PROCEDURE_STEP_PRIORITY,
    TAG_PROCEDURE_STEP_LABEL,
    TAG_WORKLIST_LABEL,
    TAG_SCHEDULED_PROCESSING_PARAMETERS_SEQUENCE,
    TAG_SCHEDULED_STATION_NAME_CODE_SEQUENCE,
    TAG_SCHEDULED_STATION_CLASS_CODE_SEQUENCE,
    TAG_SCHEDULED_STATION_GEOGRAPHIC_LOCATION_CODE_SEQUENCE,
    TAG_SCHEDULED_HUMAN_PERFORMERS_SEQUENCE,
    TAG_SCHEDULED_PROCEDURE_STEP_START_DATE_TIME,
    TAG_EXPECTED_COMPLETION_DATE_TIME,
    TAG_SCHEDULED_WORKITEM_CODE_SEQUENCE,
    TAG_COMMENTS_ON_SCHEDULED_PROCEDURE_STEP,
    TAG_INPUT_READINESS_STATE,
    TAG_INPUT_INFORMATION_SEQUENCE,
    TAG_PROCEDURE_STEP_PROGRESS_INFORMATION_SEQUENCE,
    TAG_UNIFIED_PROCEDURE_STEP_PERFORMED_PROCEDURE_SEQUENCE,
];
fn zero_element() -> Element {
    Element {
        header: ElementHeader {
            tag: Tag::default(),
            vr: Vr::UN,
            length: 0,
            length_set: false,
        },
        value: Value::Nil,
    }
}
fn element_or_zero(ds: &DataSet, tag: Tag) -> Element {
    data_set_element(ds, tag)
        .cloned()
        .unwrap_or_else(zero_element)
}
fn invalid() -> Result<()> {
    Err(Kind::InvalidDataSet.into())
}
pub(crate) fn validate_set_element(e: &Element) -> Result<()> {
    match e.tag() {
        TAG_SCHEDULED_PROCEDURE_STEP_PRIORITY => {
            let v = element_string_value(e);
            let v = v.trim();
            if v != "HIGH" && v != "MEDIUM" && v != "LOW" {
                return invalid();
            }
        }
        TAG_INPUT_READINESS_STATE => {
            let v = element_string_value(e);
            let v = v.trim();
            if v != "INCOMPLETE" && v != "UNAVAILABLE" && v != "READY" {
                return invalid();
            }
        }
        TAG_PROCEDURE_STEP_PROGRESS_INFORMATION_SEQUENCE => {
            return validate_progress_information_element(e);
        }
        TAG_UNIFIED_PROCEDURE_STEP_PERFORMED_PROCEDURE_SEQUENCE => {
            return validate_performed_procedure_element(e);
        }
        TAG_INPUT_INFORMATION_SEQUENCE => {
            return validate_referenced_instances_and_access(
                &item_of(vec![e.clone()]),
                TAG_INPUT_INFORMATION_SEQUENCE,
            );
        }
        TAG_SCHEDULED_PROCESSING_PARAMETERS_SEQUENCE => return validate_content_item_sequence(e),
        TAG_SCHEDULED_HUMAN_PERFORMERS_SEQUENCE => {
            let Some(items) = sequence_items(e).filter(|_| e.vr() == Vr::SQ) else {
                return invalid();
            };
            for item in items {
                let code = data_set_element(item, TAG_HUMAN_PERFORMER_CODE_SEQUENCE);
                let name = data_set_string(item, TAG_HUMAN_PERFORMER_NAME);
                let org = data_set_string(item, TAG_HUMAN_PERFORMER_ORGANIZATION);
                if !code.is_some_and(required_single_code_sequence)
                    || name.is_none_or(|n| n.trim().is_empty())
                    || org.is_none_or(|o| o.trim().is_empty())
                {
                    return invalid();
                }
            }
        }
        TAG_SCHEDULED_STATION_NAME_CODE_SEQUENCE
        | TAG_SCHEDULED_STATION_CLASS_CODE_SEQUENCE
        | TAG_SCHEDULED_STATION_GEOGRAPHIC_LOCATION_CODE_SEQUENCE => {
            return validate_optional_code_sequence(e, false);
        }
        TAG_SCHEDULED_WORKITEM_CODE_SEQUENCE if !required_single_code_sequence(e) => {
            return invalid();
        }
        _ => {}
    }
    Ok(())
}
fn validate_referenced_request_sequence(ds: &DataSet) -> Result<()> {
    let Some(e) = data_set_element(ds, TAG_REFERENCED_REQUEST_SEQUENCE) else {
        return invalid();
    };
    let Some(items) = sequence_items(e).filter(|_| e.vr() == Vr::SQ) else {
        return invalid();
    };
    for item in items {
        match data_set_string(item, TAG_STUDY_INSTANCE_UID) {
            Some(uid) if valid_uid(&uid) => {}
            _ => return invalid(),
        }
        for required in [
            TAG_ACCESSION_NUMBER,
            TAG_ISSUER_OF_ACCESSION_NUMBER_SEQUENCE,
            TAG_ORDER_PLACER_IDENTIFIER_SEQUENCE,
            TAG_ORDER_FILLER_IDENTIFIER_SEQUENCE,
            TAG_REQUESTED_PROCEDURE_ID,
            TAG_REQUESTED_PROCEDURE_DESCRIPTION,
            TAG_REQUESTED_PROCEDURE_CODE_SEQUENCE,
        ] {
            if data_set_element(item, required).is_none() {
                return invalid();
            }
        }
        if validate_optional_code_sequence(
            &element_or_zero(item, TAG_REQUESTED_PROCEDURE_CODE_SEQUENCE),
            true,
        )
        .is_err()
        {
            return invalid();
        }
        for tag in [
            TAG_ISSUER_OF_ACCESSION_NUMBER_SEQUENCE,
            TAG_ORDER_PLACER_IDENTIFIER_SEQUENCE,
            TAG_ORDER_FILLER_IDENTIFIER_SEQUENCE,
        ] {
            if validate_hierarchic_designator_sequence(&element_or_zero(item, tag)).is_err() {
                return invalid();
            }
        }
    }
    Ok(())
}
fn validate_sop_instance_reference_sequence(e: &Element) -> Result<()> {
    let Some(items) = sequence_items(e).filter(|_| e.vr() == Vr::SQ) else {
        return invalid();
    };
    for item in items {
        let class = data_set_string(item, TAG_REFERENCED_SOP_CLASS_UID);
        let instance = data_set_string(item, TAG_REFERENCED_SOP_INSTANCE_UID);
        match (class, instance) {
            (Some(c), Some(i)) if valid_uid(&c) && valid_uid(&i) => {}
            _ => return invalid(),
        }
    }
    Ok(())
}
pub(crate) fn validate_content_item_sequence(e: &Element) -> Result<()> {
    let Some(items) = sequence_items(e).filter(|_| e.vr() == Vr::SQ) else {
        return invalid();
    };
    for item in items {
        let value_type = data_set_string(item, TAG_VALUE_TYPE);
        let concept = data_set_element(item, TAG_CONCEPT_NAME_CODE_SEQUENCE);
        let Some(value_type) = value_type else {
            return invalid();
        };
        if !concept.is_some_and(required_single_code_sequence) {
            return invalid();
        }
        let value_tag = match value_type.as_str() {
            "DATETIME" => TAG_DATE_TIME,
            "DATE" => TAG_DATE,
            "TIME" => TAG_TIME,
            "PNAME" => TAG_PERSON_NAME,
            "UIDREF" => TAG_UID,
            "TEXT" => TAG_TEXT_VALUE,
            "CODE" => TAG_CONCEPT_CODE_SEQUENCE,
            "NUMERIC" => TAG_NUMERIC_VALUE,
            _ => return invalid(),
        };
        let Some(value) = data_set_element(item, value_tag) else {
            return invalid();
        };
        if value_tag == TAG_CONCEPT_CODE_SEQUENCE {
            if !required_single_code_sequence(value) {
                return invalid();
            }
        } else {
            let values = element_string_values(value);
            if values.is_empty() || values[0].trim().is_empty() {
                return invalid();
            }
        }
        if value_type == "NUMERIC"
            && !data_set_element(item, TAG_MEASUREMENT_UNITS_CODE_SEQUENCE)
                .is_some_and(required_single_code_sequence)
        {
            return invalid();
        }
        if let Some(modifiers) = data_set_element(item, TAG_CONTENT_ITEM_MODIFIER_SEQUENCE) {
            let ok = sequence_items(modifiers).is_some_and(|i| !i.is_empty())
                && validate_content_item_sequence(modifiers).is_ok();
            if !ok {
                return invalid();
            }
        }
    }
    Ok(())
}
fn validate_patient_identification_sequences(ds: &DataSet) -> Result<()> {
    match data_set_element(ds, TAG_ISSUER_OF_PATIENT_ID_QUALIFIERS_SEQUENCE) {
        Some(q) if validate_issuer_qualifiers_sequence(q).is_ok() => {}
        _ => return invalid(),
    }
    let Some(other) = data_set_element(ds, TAG_OTHER_PATIENT_IDS_SEQUENCE) else {
        return invalid();
    };
    let Some(items) = sequence_items(other).filter(|_| other.vr() == Vr::SQ) else {
        return invalid();
    };
    for item in items {
        let id = data_set_string(item, TAG_PATIENT_ID);
        let issuer = data_set_element(item, TAG_ISSUER_OF_PATIENT_ID);
        let qualifiers = data_set_element(item, TAG_ISSUER_OF_PATIENT_ID_QUALIFIERS_SEQUENCE);
        if id.is_none_or(|v| v.trim().is_empty())
            || issuer.is_none()
            || !qualifiers.is_some_and(|q| validate_issuer_qualifiers_sequence(q).is_ok())
        {
            return invalid();
        }
    }
    Ok(())
}
fn validate_issuer_qualifiers_sequence(e: &Element) -> Result<()> {
    let Some(items) = sequence_items(e).filter(|_| e.vr() == Vr::SQ) else {
        return invalid();
    };
    for item in items {
        let universal = data_set_string(item, TAG_UNIVERSAL_ENTITY_ID);
        let identifier = data_set_element(item, TAG_IDENTIFIER_TYPE_CODE);
        let facility = data_set_element(item, TAG_ASSIGNING_FACILITY_SEQUENCE);
        let jurisdiction = data_set_element(item, TAG_ASSIGNING_JURISDICTION_CODE_SEQUENCE);
        let agency = data_set_element(item, TAG_ASSIGNING_AGENCY_OR_DEPARTMENT_CODE_SEQUENCE);
        let (Some(universal), Some(_), Some(facility), Some(jurisdiction), Some(agency)) =
            (universal, identifier, facility, jurisdiction, agency)
        else {
            return invalid();
        };
        if validate_hierarchic_designator_sequence(facility).is_err()
            || validate_optional_code_sequence(jurisdiction, true).is_err()
            || validate_optional_code_sequence(agency, true).is_err()
        {
            return invalid();
        }
        if !universal.trim().is_empty()
            && data_set_string(item, TAG_UNIVERSAL_ENTITY_ID_TYPE)
                .is_none_or(|t| t.trim().is_empty())
        {
            return invalid();
        }
    }
    Ok(())
}
fn validate_hierarchic_designator_sequence(e: &Element) -> Result<()> {
    let Some(items) = sequence_items(e).filter(|i| e.vr() == Vr::SQ && i.len() <= 1) else {
        return invalid();
    };
    for item in items {
        let local = data_set_string(item, TAG_LOCAL_NAMESPACE_ENTITY_ID);
        let universal = data_set_string(item, TAG_UNIVERSAL_ENTITY_ID);
        let local_empty = local.as_ref().is_none_or(|v| v.trim().is_empty());
        let universal_empty = universal.as_ref().is_none_or(|v| v.trim().is_empty());
        if local_empty && universal_empty {
            return invalid();
        }
        if !universal_empty
            && data_set_string(item, TAG_UNIVERSAL_ENTITY_ID_TYPE)
                .is_none_or(|t| t.trim().is_empty())
        {
            return invalid();
        }
    }
    Ok(())
}
pub(crate) fn validate_referenced_instances_and_access(
    ds: &DataSet,
    sequence_tag: Tag,
) -> Result<()> {
    let Some(e) = data_set_element(ds, sequence_tag) else {
        return invalid();
    };
    let Some(items) = sequence_items(e).filter(|_| e.vr() == Vr::SQ) else {
        return invalid();
    };
    for item in items {
        let instance_type = data_set_string(item, TAG_TYPE_OF_INSTANCES);
        let references = data_set_element(item, TAG_REFERENCED_SOP_SEQUENCE);
        let reference_items = references
            .filter(|r| r.vr() == Vr::SQ)
            .and_then(sequence_items)
            .filter(|i| !i.is_empty());
        let Some(reference_items) = reference_items else {
            return invalid();
        };
        if instance_type.is_none_or(|t| t.trim().is_empty()) {
            return invalid();
        }
        for reference in reference_items {
            let class = data_set_string(reference, TAG_REFERENCED_SOP_CLASS_UID);
            let instance = data_set_string(reference, TAG_REFERENCED_SOP_INSTANCE_UID);
            match (class, instance) {
                (Some(c), Some(i)) if valid_uid(&c) && valid_uid(&i) => {}
                _ => return invalid(),
            }
        }
        if !valid_referenced_access_method(item) {
            return invalid();
        }
    }
    Ok(())
}
fn valid_referenced_access_method(item: &DataSet) -> bool {
    let requirements: [(Tag, &[Tag]); 5] = [
        (TAG_DICOM_RETRIEVAL_SEQUENCE, &[TAG_RETRIEVE_AE_TITLE]),
        (
            TAG_DICOM_MEDIA_RETRIEVAL_SEQUENCE,
            &[
                TAG_STORAGE_MEDIA_FILE_SET_ID,
                TAG_STORAGE_MEDIA_FILE_SET_UID,
            ],
        ),
        (TAG_WADO_RETRIEVAL_SEQUENCE, &[TAG_RETRIEVE_URI]),
        (
            TAG_XDS_RETRIEVAL_SEQUENCE,
            &[TAG_REPOSITORY_UNIQUE_ID, TAG_HOME_COMMUNITY_ID],
        ),
        (TAG_WADO_RS_RETRIEVAL_SEQUENCE, &[TAG_RETRIEVE_URL]),
    ];
    let mut found = false;
    for (sequence, required) in requirements {
        let Some(e) = data_set_element(item, sequence) else {
            continue;
        };
        let Some(items) = sequence_items(e).filter(|i| e.vr() == Vr::SQ && !i.is_empty()) else {
            return false;
        };
        found = true;
        for method in items {
            for &tag in required {
                match data_set_string(method, tag) {
                    None => return false,
                    Some(v) => {
                        if tag != TAG_STORAGE_MEDIA_FILE_SET_ID
                            && tag != TAG_HOME_COMMUNITY_ID
                            && v.trim().is_empty()
                        {
                            return false;
                        }
                    }
                }
            }
        }
    }
    found
}
pub(crate) fn validate_optional_code_sequence(e: &Element, single_item: bool) -> Result<()> {
    let Some(items) = sequence_items(e).filter(|_| e.vr() == Vr::SQ) else {
        return invalid();
    };
    if single_item && items.len() > 1 {
        return invalid();
    }
    if items.is_empty() {
        return Ok(());
    }
    if !required_code_sequence(e) {
        return invalid();
    }
    Ok(())
}
pub(crate) fn validate_progress_information_element(e: &Element) -> Result<()> {
    let Some(items) = sequence_items(e).filter(|i| e.vr() == Vr::SQ && i.len() <= 1) else {
        return invalid();
    };
    let Some(item) = items.first() else {
        return Ok(());
    };
    for tag in [
        TAG_PROCEDURE_STEP_PROGRESS,
        TAG_PROCEDURE_STEP_PROGRESS_DESCRIPTION,
        TAG_PROCEDURE_STEP_CANCELLATION_DATE_TIME,
        TAG_REASON_FOR_CANCELLATION,
    ] {
        if let Some(v) = data_set_string(item, tag)
            && v.trim().is_empty()
        {
            return invalid();
        }
    }
    if let Some(reason) = data_set_element(
        item,
        TAG_PROCEDURE_STEP_DISCONTINUATION_REASON_CODE_SEQUENCE,
    ) && !required_single_code_sequence(reason)
    {
        return invalid();
    }
    if let Some(parameters) =
        data_set_element(item, TAG_PROCEDURE_STEP_PROGRESS_PARAMETERS_SEQUENCE)
    {
        let ok = sequence_items(parameters).is_some_and(|i| !i.is_empty())
            && validate_content_item_sequence(parameters).is_ok();
        if !ok {
            return invalid();
        }
    }
    let Some(communications) =
        data_set_element(item, TAG_PROCEDURE_STEP_COMMUNICATIONS_URI_SEQUENCE)
    else {
        return Ok(());
    };
    let Some(items) =
        sequence_items(communications).filter(|i| communications.vr() == Vr::SQ && !i.is_empty())
    else {
        return invalid();
    };
    for communication in items {
        if data_set_string(communication, TAG_CONTACT_URI).is_none_or(|v| v.trim().is_empty()) {
            return invalid();
        }
        if let Some(d) = data_set_string(communication, TAG_CONTACT_DISPLAY_NAME)
            && d.trim().is_empty()
        {
            return invalid();
        }
    }
    Ok(())
}
fn validate_performed_procedure_element(e: &Element) -> Result<()> {
    let Some(items) = sequence_items(e).filter(|i| e.vr() == Vr::SQ && i.len() <= 1) else {
        return invalid();
    };
    let Some(item) = items.first() else {
        return Ok(());
    };
    match data_set_element(item, TAG_OUTPUT_INFORMATION_SEQUENCE) {
        Some(output)
            if validate_referenced_instances_and_access(
                &item_of(vec![output.clone()]),
                TAG_OUTPUT_INFORMATION_SEQUENCE,
            )
            .is_ok() => {}
        _ => return invalid(),
    }
    if let Some(station) = data_set_element(item, TAG_PERFORMED_STATION_NAME_CODE_SEQUENCE)
        && validate_optional_code_sequence(station, false).is_err()
    {
        return invalid();
    }
    if let Some(w) = data_set_element(item, TAG_PERFORMED_WORKITEM_CODE_SEQUENCE)
        && !required_code_sequence(w)
    {
        return invalid();
    }
    if let Some(p) = data_set_element(item, TAG_ACTUAL_HUMAN_PERFORMERS_SEQUENCE)
        && validate_actual_human_performers(p).is_err()
    {
        return invalid();
    }
    if let Some(parameters) = data_set_element(item, TAG_PERFORMED_PROCESSING_PARAMETERS_SEQUENCE) {
        let ok = sequence_items(parameters).is_some_and(|i| !i.is_empty())
            && validate_content_item_sequence(parameters).is_ok();
        if !ok {
            return invalid();
        }
    }
    Ok(())
}
fn validate_actual_human_performers(e: &Element) -> Result<()> {
    let Some(items) = sequence_items(e).filter(|i| e.vr() == Vr::SQ && !i.is_empty()) else {
        return invalid();
    };
    for item in items {
        let code = data_set_element(item, TAG_HUMAN_PERFORMER_CODE_SEQUENCE);
        let name = data_set_string(item, TAG_HUMAN_PERFORMER_NAME);
        let org = data_set_string(item, TAG_HUMAN_PERFORMER_ORGANIZATION);
        if !code.is_some_and(required_single_code_sequence)
            || name.is_none_or(|n| n.trim().is_empty())
            || org.is_none_or(|o| o.trim().is_empty())
        {
            return invalid();
        }
    }
    Ok(())
}
pub(crate) fn validate_final_state(ds: &DataSet, target: &str) -> Result<()> {
    if target == STATE_COMPLETED {
        let Some(performed) =
            data_set_element(ds, TAG_UNIFIED_PROCEDURE_STEP_PERFORMED_PROCEDURE_SEQUENCE)
        else {
            return invalid();
        };
        let Some(items) = sequence_items(performed).filter(|i| i.len() == 1) else {
            return invalid();
        };
        for tag in [
            TAG_PERFORMED_STATION_NAME_CODE_SEQUENCE,
            TAG_PERFORMED_PROCEDURE_STEP_START_DATE_TIME,
            TAG_PERFORMED_WORKITEM_CODE_SEQUENCE,
            TAG_PERFORMED_PROCEDURE_STEP_END_DATE_TIME,
            TAG_OUTPUT_INFORMATION_SEQUENCE,
        ] {
            let Some(e) = data_set_element(&items[0], tag) else {
                return invalid();
            };
            if (tag == TAG_PERFORMED_STATION_NAME_CODE_SEQUENCE
                || tag == TAG_PERFORMED_WORKITEM_CODE_SEQUENCE)
                && !required_code_sequence(e)
            {
                return invalid();
            }
            if (tag == TAG_PERFORMED_PROCEDURE_STEP_START_DATE_TIME
                || tag == TAG_PERFORMED_PROCEDURE_STEP_END_DATE_TIME)
                && element_string_value(e).trim().is_empty()
            {
                return invalid();
            }
        }
        if validate_referenced_instances_and_access(&items[0], TAG_OUTPUT_INFORMATION_SEQUENCE)
            .is_err()
        {
            return invalid();
        }
        return Ok(());
    }
    if target == STATE_CANCELED {
        let Some(progress) = data_set_element(ds, TAG_PROCEDURE_STEP_PROGRESS_INFORMATION_SEQUENCE)
        else {
            return invalid();
        };
        let Some(items) = sequence_items(progress).filter(|i| i.len() == 1) else {
            return invalid();
        };
        if data_set_string(&items[0], TAG_PROCEDURE_STEP_CANCELLATION_DATE_TIME)
            .is_none_or(|v| v.trim().is_empty())
        {
            return invalid();
        }
        if !data_set_element(
            &items[0],
            TAG_PROCEDURE_STEP_DISCONTINUATION_REASON_CODE_SEQUENCE,
        )
        .is_some_and(required_single_code_sequence)
        {
            return invalid();
        }
        return Ok(());
    }
    Err(Kind::InvalidState.into())
}
pub(crate) fn has_non_empty_sequence(ds: &DataSet, tag: Tag) -> bool {
    data_set_element(ds, tag)
        .and_then(sequence_items)
        .is_some_and(|i| !i.is_empty())
}
fn non_empty_data_set_string(ds: &DataSet, tag: Tag) -> Option<String> {
    let v = data_set_string(ds, tag)?;
    let v = v.trim().to_string();
    (!v.is_empty()).then_some(v)
}
pub(crate) fn required_code_sequence(e: &Element) -> bool {
    if e.vr() != Vr::SQ {
        return false;
    }
    let Some(items) = sequence_items(e).filter(|i| !i.is_empty()) else {
        return false;
    };
    for item in items {
        let value = non_empty_data_set_string(item, TAG_CODE_VALUE);
        let long = non_empty_data_set_string(item, TAG_LONG_CODE_VALUE);
        let urn = non_empty_data_set_string(item, TAG_URN_CODE_VALUE);
        let count = [value.is_some(), long.is_some(), urn.is_some()]
            .iter()
            .filter(|b| **b)
            .count();
        if count != 1 {
            return false;
        }
        if let Some(v) = &value
            && (v.len() > 16 || absolute_code_uri(v))
        {
            return false;
        }
        if let Some(v) = &long
            && (v.len() <= 16 || absolute_code_uri(v))
        {
            return false;
        }
        if let Some(v) = &urn
            && !absolute_code_uri(v)
        {
            return false;
        }
        let scheme_ok = non_empty_data_set_string(item, TAG_CODING_SCHEME_DESIGNATOR).is_some();
        let meaning = data_set_string(item, TAG_CODE_MEANING);
        let version_present = data_set_element(item, TAG_CODING_SCHEME_VERSION).is_some();
        if ((value.is_some() || long.is_some()) && !scheme_ok)
            || (version_present && !scheme_ok)
            || meaning.is_none_or(|m| m.trim().is_empty())
        {
            return false;
        }
    }
    true
}
pub(crate) fn required_single_code_sequence(e: &Element) -> bool {
    sequence_items(e).is_some_and(|i| i.len() == 1) && required_code_sequence(e)
}
pub(crate) fn valid_state(state: &str) -> bool {
    matches!(
        state,
        STATE_SCHEDULED | STATE_IN_PROGRESS | STATE_COMPLETED | STATE_CANCELED
    )
}
fn valid_default_worklist_label(value: &str) -> bool {
    if value.is_empty() || value.len() > 64 || value.contains('\\') {
        return false;
    }
    value.chars().all(|c| (' '..='~').contains(&c))
}
pub(crate) fn valid_uid(value: &str) -> bool {
    is_valid_uid(normalize_uid(value).as_bytes())
}
pub(crate) fn classify_data_set_error(operation: &str, err: Error) -> Error {
    if err.is(Kind::ResourceLimit) {
        return err;
    }
    status_error(operation, STATUS_INVALID_ATTRIBUTE_VALUE, err)
}
pub(crate) fn safe_repository_error(err: Error) -> Error {
    if err.is(Kind::Repository) {
        return err;
    }
    Error::Repository(Box::new(err))
}
pub(crate) fn dicom_date_time(t: Time) -> String {
    let nanos = t.unix_nanos();
    let secs = nanos.div_euclid(1_000_000_000) as i64;
    let frac = nanos.rem_euclid(1_000_000_000) as i64;
    let z = crate::dcmtime::ZonedTime::from_fields(1970, 1, 1, 0, 0, secs, frac, 0);
    let (y, m, d) = z.date();
    format!(
        "{y:04}{m:02}{d:02}{:02}{:02}{:02}.{:06}+0000",
        z.hour(),
        z.minute(),
        z.second(),
        z.nanosecond() / 1000
    )
}
fn internal_transaction_uid(sop_instance_uid: &str, version: u64) -> String {
    crate::deid::uid::name_uid(&[
        "dcm-rust/ups transaction",
        normalize_uid(sop_instance_uid),
        &version.to_string(),
    ])
}
pub(crate) fn valid_ae_title(value: &str) -> bool {
    value.len() <= crate::net::ul::MAX_AE_TITLE_LENGTH
        && !value.contains('\0')
        && crate::net::ul::validate_ae_title(value).is_ok()
}
