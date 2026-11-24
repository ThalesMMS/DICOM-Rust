#![forbid(unsafe_code)]
use super::command::*;
use super::commitment::*;
use super::commitment_workflow::*;
use super::controls::{ScpControls, begin_association_operation, context_err};
use super::error::{Detail, Error, Kind, NormalizedActionResponseHook, Result};
use super::message::receive_storage_command_or_release;
use super::normalized::*;
use super::scp::{AssociationScpOptions, serve_association};
use crate::net::ul::{self, Association, Ctx, Nanos, Pdu};
use crate::object::Object;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};
#[derive(Debug, Clone, Default)]
pub(crate) struct AssocIdentity {
    calling: String,
    called: String,
    roles: Vec<ul::RoleSelectionItem>,
}
impl AssocIdentity {
    fn of(assoc: &Association) -> AssocIdentity {
        AssocIdentity {
            calling: assoc.calling_ae_title.clone(),
            called: assoc.called_ae_title.clone(),
            roles: assoc.accepted_role_selections.clone(),
        }
    }
    fn can_act_as_scu(&self) -> bool {
        for role in &self.roles {
            if role.sop_class_uid == STORAGE_COMMITMENT_PUSH_MODEL_SOP_CLASS_UID {
                return role.scu_role;
            }
        }
        true
    }
    fn roles_valid(&self, t: &StorageCommitmentTransaction) -> bool {
        let original = ul::equal_ae_titles(&self.calling, &t.requestor_ae_title)
            && ul::equal_ae_titles(&self.called, &t.committer_ae_title);
        let callback = ul::equal_ae_titles(&self.calling, &t.committer_ae_title)
            && ul::equal_ae_titles(&self.called, &t.requestor_ae_title);
        for role in &self.roles {
            if role.sop_class_uid != STORAGE_COMMITMENT_PUSH_MODEL_SOP_CLASS_UID {
                continue;
            }
            if original {
                return role.scu_role;
            }
            if callback {
                return role.scp_role;
            }
        }
        original
    }
}
pub(crate) fn storage_commitment_association_roles_valid(
    assoc: &Association,
    transaction: &StorageCommitmentTransaction,
) -> bool {
    AssocIdentity::of(assoc).roles_valid(transaction)
}
pub(crate) fn new_delivery_error(err: Error, status: u16) -> Error {
    if let Some((class, st, retryable)) = storage_commitment_delivery_error(&err) {
        let status = if status != 0 { status } else { st };
        let inner = match err.detail() {
            Detail::StorageCommitmentDelivery(_, _, _, inner) => inner.clone(),
            _ => Box::new(Detail::None),
        };
        return err.with_detail(Detail::StorageCommitmentDelivery(
            class, status, retryable, inner,
        ));
    }
    let mut class = STORAGE_COMMITMENT_FAILURE_CALLBACK_OFFLINE;
    let mut retryable = true;
    let mut status = status;
    let status_error = match err.detail() {
        Detail::Status(s) if err.is(Kind::StorageCommitmentStatus) => Some(*s),
        _ => None,
    };
    if err.is(Kind::StorageCommitmentCallbackUnknown) {
        class = STORAGE_COMMITMENT_FAILURE_CALLBACK_UNKNOWN;
    } else if err.is_canceled() {
        class = STORAGE_COMMITMENT_FAILURE_CANCELED;
    } else if err.is_deadline() || err.is_ul(ul::Kind::AssociationTimeout) {
        class = STORAGE_COMMITMENT_FAILURE_TIMEOUT;
    } else if matches!(err.ul_detail(), ul::Detail::Tls(c) if c.is_certificate_or_record_failure())
    {
        class = super::commitment_workflow::STORAGE_COMMITMENT_FAILURE_TLS;
    } else if matches!(err.ul_detail(), ul::Detail::Rejection(_)) {
        class = STORAGE_COMMITMENT_FAILURE_ASSOCIATION_REJECTED;
    } else if let Some(s) = status_error {
        class = STORAGE_COMMITMENT_FAILURE_DIMSE_STATUS;
        status = s;
    } else if err.is_ul(ul::Kind::UnexpectedPdu) || err.is(Kind::PresentationContextMismatch) {
        class = STORAGE_COMMITMENT_FAILURE_PROTOCOL;
        retryable = false;
    }
    delivery_error(class, status, retryable, err)
}
fn validate_event_command_status(request: &NormalizedRequest) -> (u16, Option<Error>) {
    if request.sop_class_uid != STORAGE_COMMITMENT_PUSH_MODEL_SOP_CLASS_UID {
        return (STATUS_NO_SUCH_SOP_CLASS, Some(invalid_result()));
    }
    if request.sop_instance_uid != STORAGE_COMMITMENT_PUSH_MODEL_SOP_INSTANCE_UID {
        return (STATUS_NO_SUCH_SOP_INSTANCE, Some(invalid_result()));
    }
    if !valid_event_type(request.type_id) {
        return (STATUS_NO_SUCH_EVENT_TYPE, Some(invalid_result()));
    }
    if request.command_data_set_type == NO_DATA_SET {
        return (
            STATUS_STORAGE_COMMITMENT_PROCESSING_FAILURE,
            Some(invalid_result()),
        );
    }
    (STATUS_SUCCESS, None)
}
fn scp_result(status: u16) -> NormalizedScpResult {
    NormalizedScpResult {
        response: NormalizedResponse {
            status,
            ..Default::default()
        },
        data_set: None,
    }
}
pub(crate) struct Heartbeat(Option<mpsc::Sender<()>>);
impl Heartbeat {
    pub(crate) fn stop(&mut self) {
        self.0.take();
    }
}
pub(crate) type StorageCommitmentErrorCallback = Arc<dyn Fn(&Error) + Send + Sync>;
#[derive(Clone, Default)]
pub struct StorageCommitmentListenerOptions {
    pub accept: ul::AcceptOptions,
    pub controls: ScpControls,
    pub max_concurrent_associations: i64,
    pub on_error: Option<StorageCommitmentErrorCallback>,
}
#[derive(Debug, Clone, Default)]
pub struct StorageCommitmentAwaitResult {
    pub transaction: StorageCommitmentTransaction,
    pub peer_released: bool,
}
const STORAGE_COMMITMENT_AWAIT_POLL: Duration = Duration::from_millis(100);
impl StorageCommitmentWorkflow {
    pub fn normalized_options(&self, assoc: &Association) -> NormalizedScpOptions {
        let identity = AssocIdentity::of(assoc);
        let policy: NormalizedPresentationContextPolicy = Arc::new(|pc, command_class| {
            if pc.abstract_syntax_uid != STORAGE_COMMITMENT_PUSH_MODEL_SOP_CLASS_UID
                || command_class != STORAGE_COMMITMENT_PUSH_MODEL_SOP_CLASS_UID
            {
                return Err(Error::sentinel(Kind::PresentationContextMismatch));
            }
            Ok(())
        });
        let w = self.clone();
        let id = identity.clone();
        let action: NormalizedHandler = Arc::new(
            move |ctx: &Ctx, request: &NormalizedRequest, ds: Option<&Object>| {
                let command = validate_action_command_status(request);
                if command.is_err() || !id.can_act_as_scu() {
                    let status = match &command {
                        Err(e) => match e.detail() {
                            Detail::Status(s) => *s,
                            _ => STATUS_STORAGE_COMMITMENT_PROCESSING_FAILURE,
                        },
                        Ok(_) => STATUS_STORAGE_COMMITMENT_PROCESSING_FAILURE,
                    };
                    return (scp_result(status), Some(invalid_request()));
                }
                let information = match parse_storage_commitment_action_information(ds) {
                    Ok(i) => i,
                    Err(e) if e.is(Kind::StorageCommitmentResourceLimit) => {
                        return (
                            scp_result(STATUS_STORAGE_COMMITMENT_RESOURCE_LIMITATION),
                            Some(e),
                        );
                    }
                    Err(_) => {
                        return (
                            scp_result(STATUS_STORAGE_COMMITMENT_PROCESSING_FAILURE),
                            Some(invalid_request()),
                        );
                    }
                };
                let transaction =
                    match w.accept_request(ctx, Some(&id), request.message_id, &information) {
                        Ok(t) => t,
                        Err(e) => {
                            let status = if e.is(Kind::StorageCommitmentResourceLimit) {
                                STATUS_STORAGE_COMMITMENT_RESOURCE_LIMITATION
                            } else {
                                STATUS_STORAGE_COMMITMENT_PROCESSING_FAILURE
                            };
                            return (scp_result(status), Some(e));
                        }
                    };
                let heartbeat =
                    Mutex::new(w.start_committer_acceptance_heartbeat(ctx, &transaction));
                let after_w = w.clone();
                let uid = transaction.transaction_uid.clone();
                let hook = NormalizedActionResponseHook(Arc::new(
                    move |after_ctx: &Ctx, response: &NormalizedResponse, sent: bool| {
                        heartbeat.lock().unwrap().stop();
                        if !sent || response.status != STATUS_SUCCESS {
                            return None;
                        }
                        after_w.mark_committer_accepted(after_ctx, &uid).err()
                    },
                ));
                (scp_result(STATUS_SUCCESS), Some(hook.into_error()))
            },
        );
        let w = self.clone();
        let id = identity;
        let event: NormalizedHandler = Arc::new(
            move |ctx: &Ctx, request: &NormalizedRequest, ds: Option<&Object>| {
                let (status, err) = validate_event_command_status(request);
                if err.is_some() {
                    return (scp_result(status), err);
                }
                let information = match parse_storage_commitment_event_information(ds) {
                    Ok(i) => i,
                    Err(e) if e.is(Kind::StorageCommitmentResourceLimit) => {
                        return (
                            scp_result(STATUS_STORAGE_COMMITMENT_PROCESSING_FAILURE),
                            Some(e),
                        );
                    }
                    Err(_) => {
                        return (
                            scp_result(STATUS_STORAGE_COMMITMENT_PROCESSING_FAILURE),
                            Some(invalid_result()),
                        );
                    }
                };
                let r = storage_commitment_result_from_event(request.type_id, &information)
                    .and_then(|result| w.receive_result(ctx, Some(&id), result));
                match r {
                    Ok(()) => (scp_result(STATUS_SUCCESS), None),
                    Err(e) => (
                        scp_result(STATUS_STORAGE_COMMITMENT_PROCESSING_FAILURE),
                        Some(e),
                    ),
                }
            },
        );
        NormalizedScpOptions {
            event_report_handler: Some(event),
            action_handler: Some(action),
            max_data_set_bytes: self.inner.limits.max_data_set_bytes,
            presentation_context_policy: Some(policy),
            ..Default::default()
        }
    }
    pub fn serve_action(
        &self,
        ctx: Option<&Ctx>,
        assoc: &Association,
        pc_id: u8,
    ) -> (StorageCommitmentTransaction, Result<()>) {
        let ctx = ctx.cloned().unwrap_or_default();
        let identity = AssocIdentity::of(assoc);
        if !identity.can_act_as_scu() {
            return (
                StorageCommitmentTransaction::default(),
                Err(invalid_request()),
            );
        }
        let accepted = Arc::new(Mutex::new(StorageCommitmentTransaction::default()));
        let heartbeat = Arc::new(Mutex::new(Heartbeat(None)));
        let handler: StorageCommitmentActionHandler = {
            let w = self.clone();
            let accepted = accepted.clone();
            let heartbeat = heartbeat.clone();
            Arc::new(
                move |handler_ctx: &Ctx, request: &StorageCommitmentActionContext| {
                    let information = StorageCommitmentActionInformation {
                        transaction_uid: request.transaction_uid.clone(),
                        referenced_sops: request.referenced_sops.clone(),
                    };
                    match w.accept_request(
                        handler_ctx,
                        Some(&identity),
                        request.request.message_id,
                        &information,
                    ) {
                        Ok(t) => {
                            *heartbeat.lock().unwrap() =
                                w.start_committer_acceptance_heartbeat(handler_ctx, &t);
                            *accepted.lock().unwrap() = t;
                            (STATUS_SUCCESS, None)
                        }
                        Err(e) if e.is(Kind::StorageCommitmentResourceLimit) => {
                            (STATUS_STORAGE_COMMITMENT_RESOURCE_LIMITATION, Some(e))
                        }
                        Err(e) => (STATUS_STORAGE_COMMITMENT_PROCESSING_FAILURE, Some(e)),
                    }
                },
            )
        };
        let r = serve_storage_commitment_scp(
            Some(&ctx),
            assoc,
            pc_id,
            Some(&handler),
            StorageCommitmentScpOptions {
                max_data_set_bytes: self.inner.limits.max_data_set_bytes,
            },
        );
        let accepted = accepted.lock().unwrap().clone();
        let out = match r {
            Err(e) => (accepted, Err(e)),
            Ok(()) => match self.mark_committer_accepted(&ctx, &accepted.transaction_uid) {
                Ok(t) => (t, Ok(())),
                Err(e) => (StorageCommitmentTransaction::default(), Err(e)),
            },
        };
        heartbeat.lock().unwrap().stop();
        out
    }
    pub fn serve_association(
        &self,
        ctx: Option<&Ctx>,
        assoc: &Association,
        controls: ScpControls,
    ) -> Result<()> {
        let options = self.normalized_options(assoc);
        serve_association(
            ctx,
            assoc,
            &AssociationScpOptions {
                controls,
                normalized_scp: Some(options),
                ..Default::default()
            },
        )
    }
    pub fn serve_listener(
        &self,
        ctx: Option<&Ctx>,
        listener: &ul::Listener,
        options: StorageCommitmentListenerOptions,
    ) -> Result<()> {
        let ctx = ctx.cloned().unwrap_or_default();
        if options.max_concurrent_associations < 0 {
            return Err(invalid_request());
        }
        let max = if options.max_concurrent_associations == 0 {
            8
        } else {
            options.max_concurrent_associations as usize
        };
        let mut accept = options.accept.clone();
        accept.context = Some(ctx.clone());
        accept.supported_abstract_syntaxes =
            vec![STORAGE_COMMITMENT_PUSH_MODEL_SOP_CLASS_UID.into()];
        accept.supported_transfer_syntaxes = vec![
            ul::EXPLICIT_VR_LITTLE_ENDIAN.into(),
            ul::IMPLICIT_VR_LITTLE_ENDIAN.into(),
        ];
        accept.role_selections = vec![ul::RoleSelectionItem {
            sop_class_uid: STORAGE_COMMITMENT_PUSH_MODEL_SOP_CLASS_UID.into(),
            scu_role: false,
            scp_role: true,
        }];
        let slots = Arc::new((Mutex::new(0usize), Condvar::new()));
        let release_slot = |slots: &Arc<(Mutex<usize>, Condvar)>| {
            *slots.0.lock().unwrap() -= 1;
            slots.1.notify_all();
        };
        std::thread::scope(|scope| {
            loop {
                {
                    let mut active = slots.0.lock().unwrap();
                    loop {
                        if let Some(e) = context_err(&ctx) {
                            return Err(e);
                        }
                        if *active < max {
                            *active += 1;
                            break;
                        }
                        active = slots
                            .1
                            .wait_timeout(active, Duration::from_millis(2))
                            .unwrap()
                            .0;
                    }
                }
                let assoc = match listener.accept_association(accept.clone()) {
                    Ok(a) => a,
                    Err(e) => {
                        release_slot(&slots);
                        if let Some(c) = context_err(&ctx) {
                            return Err(c);
                        }
                        return Err(sanitized(STORAGE_COMMITMENT_FAILURE_PROTOCOL, e.into()));
                    }
                };
                let w = self.clone();
                let ctx = ctx.clone();
                let slots = slots.clone();
                let controls = options.controls;
                let on_error = options.on_error.clone();
                scope.spawn(move || {
                    let r = w.serve_association(Some(&ctx), &assoc, controls);
                    assoc.close();
                    if let Err(e) = r
                        && ctx.err().is_none()
                        && let Some(cb) = on_error
                    {
                        let e = sanitized(STORAGE_COMMITMENT_FAILURE_PROTOCOL, e);
                        let _ = catch_unwind(AssertUnwindSafe(|| cb(&e)));
                    }
                    release_slot(&slots);
                });
            }
        })
    }
    pub(crate) fn accept_request(
        &self,
        ctx: &Ctx,
        identity: Option<&AssocIdentity>,
        message_id: u16,
        information: &StorageCommitmentActionInformation,
    ) -> Result<StorageCommitmentTransaction> {
        let references = normalize_storage_commitment_references(
            &information.referenced_sops,
            false,
            self.inner.limits.max_references as usize,
        );
        let references = match references {
            Err(e) if e.is(Kind::StorageCommitmentResourceLimit) => return Err(e),
            Ok(r) if valid_uid(&information.transaction_uid) => r,
            _ => return Err(invalid_request()),
        };
        let now = self.now();
        let lease = self.lease();
        let mut transaction = StorageCommitmentTransaction {
            transaction_uid: information.transaction_uid.clone(),
            direction: STORAGE_COMMITMENT_DIRECTION_COMMITTER.into(),
            state: STORAGE_COMMITMENT_STATE_ACCEPTANCE_PREPARED.into(),
            delivery_mode: self.inner.default_delivery_mode.clone(),
            referenced_sops: references,
            request_message_id: message_id,
            created_at: now,
            updated_at: now,
            next_attempt_at: now.add(Nanos(lease.0.wrapping_mul(2))),
            lease_token: new_storage_commitment_lease_token(),
            lease_until: now.add(lease),
            ..Default::default()
        };
        if let Some(id) = identity {
            transaction.requestor_ae_title = ul::normalize_ae_title(&id.calling).to_string();
            transaction.committer_ae_title = ul::normalize_ae_title(&id.called).to_string();
        }
        transaction.request_digest = storage_commitment_request_digest(&transaction);
        let created = self
            .store_create(ctx, transaction.clone())
            .map_err(|e| sanitized(STORAGE_COMMITMENT_FAILURE_PROCESSING, e))?;
        if !created {
            let existing = self
                .store_get(ctx, &transaction.transaction_uid)
                .map_err(|e| sanitized(STORAGE_COMMITMENT_FAILURE_PROCESSING, e))?;
            if !same_storage_commitment_request(&existing, &transaction)
                || existing.state == STORAGE_COMMITMENT_STATE_REQUEST_REJECTED
            {
                return Err(conflict());
            }
            return Ok(existing);
        }
        Ok(transaction)
    }
    pub(crate) fn mark_committer_accepted(
        &self,
        _ctx: &Ctx,
        uid: &str,
    ) -> Result<StorageCommitmentTransaction> {
        let persist = persistence_context();
        for _ in 0..8 {
            let current = self
                .store_get(&persist, uid)
                .map_err(|e| sanitized(STORAGE_COMMITMENT_FAILURE_PROCESSING, e))?;
            if current.direction != STORAGE_COMMITMENT_DIRECTION_COMMITTER {
                return Err(conflict());
            }
            if current.state != STORAGE_COMMITMENT_STATE_ACCEPTANCE_PREPARED {
                if current.state == STORAGE_COMMITMENT_STATE_REQUEST_REJECTED {
                    return Err(conflict());
                }
                return Ok(current);
            }
            let mut next = current.clone();
            next.state = STORAGE_COMMITMENT_STATE_ACCEPTED.into();
            next.updated_at = self.now();
            next.next_attempt_at = next.updated_at;
            next.lease_token.clear();
            next.lease_until = StorageCommitmentTime::ZERO;
            match self.store_cas(&persist, &current.transaction_uid, current.version, next) {
                Err(e) if e.is(Kind::StorageCommitmentConcurrentUpdate) => continue,
                Err(e) => return Err(sanitized(STORAGE_COMMITMENT_FAILURE_PROCESSING, e)),
                Ok(t) => return Ok(t),
            }
        }
        Err(concurrent_update())
    }
    fn recover_committer_acceptance(
        &self,
        ctx: &Ctx,
        uid: &str,
    ) -> Result<StorageCommitmentTransaction> {
        for _ in 0..8 {
            let current = self.store_get(ctx, uid)?;
            if current.direction != STORAGE_COMMITMENT_DIRECTION_COMMITTER {
                return Err(conflict());
            }
            if current.state != STORAGE_COMMITMENT_STATE_ACCEPTANCE_PREPARED {
                return Ok(current);
            }
            if !current.lease_until.is_zero()
                && current.lease_until.add(self.lease()).after(self.now())
            {
                return Err(concurrent_update());
            }
            let mut next = current.clone();
            next.state = STORAGE_COMMITMENT_STATE_ACCEPTED.into();
            next.updated_at = self.now();
            next.next_attempt_at = next.updated_at;
            next.lease_token.clear();
            next.lease_until = StorageCommitmentTime::ZERO;
            match self.store_cas(ctx, &current.transaction_uid, current.version, next) {
                Err(e) if e.is(Kind::StorageCommitmentConcurrentUpdate) => continue,
                r => return r,
            }
        }
        Err(concurrent_update())
    }
    pub(crate) fn start_committer_acceptance_heartbeat(
        &self,
        ctx: &Ctx,
        transaction: &StorageCommitmentTransaction,
    ) -> Heartbeat {
        if transaction.state != STORAGE_COMMITMENT_STATE_ACCEPTANCE_PREPARED
            || transaction.lease_token.is_empty()
        {
            return Heartbeat(None);
        }
        let (tx, rx) = mpsc::channel::<()>();
        let interval = Duration::from_nanos((self.lease().0 / 3).max(1) as u64);
        let w = self.clone();
        let ctx = ctx.clone();
        let uid = transaction.transaction_uid.clone();
        let token = transaction.lease_token.clone();
        std::thread::spawn(move || {
            loop {
                match rx.recv_timeout(interval) {
                    Err(RecvTimeoutError::Timeout) => {
                        if ctx.err().is_some() || !w.renew_committer_acceptance(&ctx, &uid, &token)
                        {
                            return;
                        }
                    }
                    _ => return,
                }
            }
        });
        Heartbeat(Some(tx))
    }
    fn renew_committer_acceptance(&self, ctx: &Ctx, uid: &str, token: &str) -> bool {
        let timeout =
            Duration::from_nanos((self.lease().0 / 2).max(0) as u64).min(Duration::from_secs(1));
        let persist = ctx.with_timeout(timeout);
        for _ in 0..8 {
            let current = match self.store_get(&persist, uid) {
                Ok(c) => c,
                Err(_) => return false,
            };
            if current.state != STORAGE_COMMITMENT_STATE_ACCEPTANCE_PREPARED
                || current.lease_token != token
            {
                return false;
            }
            let now = self.now();
            let mut next = current.clone();
            next.updated_at = now;
            next.lease_until = now.add(self.lease());
            next.next_attempt_at = now.add(Nanos(self.lease().0.wrapping_mul(2)));
            match self.store_cas(&persist, &current.transaction_uid, current.version, next) {
                Err(e) if e.is(Kind::StorageCommitmentConcurrentUpdate) => continue,
                r => return r.is_ok(),
            }
        }
        false
    }
    pub fn set_result(
        &self,
        ctx: Option<&Ctx>,
        result: &StorageCommitmentResult,
    ) -> Result<StorageCommitmentTransaction> {
        let ctx = ctx.cloned().unwrap_or_default();
        self.set_result_inner(&ctx, result, "", false)
    }
    fn set_result_inner(
        &self,
        ctx: &Ctx,
        result: &StorageCommitmentResult,
        processing_lease_token: &str,
        allow_pending_request: bool,
    ) -> Result<StorageCommitmentTransaction> {
        let transaction = self.store_get(ctx, &result.transaction_uid)?;
        let result = validate_storage_commitment_result(
            self.inner.limits.max_references,
            || self.now(),
            &transaction,
            result,
        )?;
        let digest = storage_commitment_result_digest(&result);
        if transaction.result.is_some() {
            if transaction.result_digest != digest {
                return Err(conflict());
            }
            return Ok(transaction);
        }
        let allow_pending = allow_pending_request
            && transaction.direction == STORAGE_COMMITMENT_DIRECTION_REQUESTOR
            && transaction.state == STORAGE_COMMITMENT_STATE_REQUEST_PENDING;
        if processing_lease_token.is_empty()
            && transaction.state != STORAGE_COMMITMENT_STATE_ACCEPTED
            && !allow_pending
        {
            return Err(conflict());
        }
        if !processing_lease_token.is_empty()
            && (transaction.state != STORAGE_COMMITMENT_STATE_PROCESSING
                || transaction.lease_token != processing_lease_token)
        {
            return Err(conflict());
        }
        let mut next = transaction.clone();
        next.state = STORAGE_COMMITMENT_STATE_RESULT_READY.into();
        next.result = Some(result);
        next.result_digest = digest;
        next.updated_at = self.now();
        next.next_attempt_at = next.updated_at;
        next.lease_token.clear();
        next.lease_until = StorageCommitmentTime::ZERO;
        self.store_cas(ctx, &transaction.transaction_uid, transaction.version, next)
    }
    pub fn process(
        &self,
        ctx: Option<&Ctx>,
        transaction_uid: &str,
    ) -> (StorageCommitmentTransaction, Result<()>) {
        let ctx = ctx.cloned().unwrap_or_default();
        self.process_inner(&ctx, transaction_uid, false)
    }
    fn process_inner(
        &self,
        ctx: &Ctx,
        uid: &str,
        require_due: bool,
    ) -> (StorageCommitmentTransaction, Result<()>) {
        let fail = |e: Error| (StorageCommitmentTransaction::default(), Err(e));
        let Some(processor) = self.inner.processor.clone() else {
            return fail(sanitized(
                STORAGE_COMMITMENT_FAILURE_PROCESSING,
                Error::new("processor unavailable"),
            ));
        };
        let claimed = match self.claim(
            ctx,
            uid,
            &[
                STORAGE_COMMITMENT_STATE_ACCEPTED,
                STORAGE_COMMITMENT_STATE_PROCESSING,
            ],
            STORAGE_COMMITMENT_STATE_PROCESSING,
            require_due,
        ) {
            Ok(c) => c,
            Err(e) => return fail(e),
        };
        let outcome = catch_unwind(AssertUnwindSafe(|| processor(ctx, claimed.clone())))
            .unwrap_or_else(|_| Err(Error::new("storage commitment processor panic")));
        match outcome {
            Err(process_err) => {
                let persist = persistence_context();
                match self.finish_processing_failure(&persist, &claimed) {
                    Ok(failed) => (
                        failed,
                        Err(sanitized(
                            STORAGE_COMMITMENT_FAILURE_PROCESSING,
                            process_err,
                        )),
                    ),
                    Err(e) => fail(e),
                }
            }
            Ok(mut result) => {
                result.transaction_uid = claimed.transaction_uid.clone();
                match self.set_result_inner(ctx, &result, &claimed.lease_token, false) {
                    Ok(t) => (t, Ok(())),
                    Err(e) => fail(e),
                }
            }
        }
    }
    fn finish_processing_failure(
        &self,
        ctx: &Ctx,
        claimed: &StorageCommitmentTransaction,
    ) -> Result<StorageCommitmentTransaction> {
        let current = self.store_get(ctx, &claimed.transaction_uid)?;
        if current.state != STORAGE_COMMITMENT_STATE_PROCESSING
            || current.lease_token != claimed.lease_token
        {
            return Err(concurrent_update());
        }
        let mut next = current.clone();
        next.state = STORAGE_COMMITMENT_STATE_ACCEPTED.into();
        next.processing_tries += 1;
        next.last_failure = STORAGE_COMMITMENT_FAILURE_PROCESSING.into();
        next.lease_token.clear();
        next.lease_until = StorageCommitmentTime::ZERO;
        next.updated_at = self.now();
        next.next_attempt_at = next.updated_at.add(storage_commitment_backoff(
            self.inner.retry,
            next.processing_tries,
        ));
        self.store_cas(ctx, &current.transaction_uid, current.version, next)
    }
    pub fn process_due(&self, ctx: Option<&Ctx>, limit: i64) -> Result<()> {
        if limit <= 0 || limit > self.inner.limits.max_list_batch {
            return Err(resource_limit());
        }
        let ctx = ctx.cloned().unwrap_or_default();
        let items = self.store_list(
            &ctx,
            &StorageCommitmentTransactionQuery {
                due_at: self.now(),
                limit,
                states: vec![
                    STORAGE_COMMITMENT_STATE_ACCEPTANCE_PREPARED.into(),
                    STORAGE_COMMITMENT_STATE_ACCEPTED.into(),
                    STORAGE_COMMITMENT_STATE_PROCESSING.into(),
                ],
            },
        )?;
        let mut errs = Vec::new();
        for item in items {
            if item.state == STORAGE_COMMITMENT_STATE_ACCEPTANCE_PREPARED {
                match self.recover_committer_acceptance(&ctx, &item.transaction_uid) {
                    Err(e) if e.is(Kind::StorageCommitmentConcurrentUpdate) => continue,
                    Err(e) => {
                        errs.push(Some(e));
                        continue;
                    }
                    Ok(_) => {}
                }
                if self.inner.processor.is_none() {
                    continue;
                }
            }
            if let (_, Err(e)) = self.process_inner(&ctx, &item.transaction_uid, true)
                && !e.is(Kind::StorageCommitmentConcurrentUpdate)
            {
                if e.is(Kind::StorageCommitmentTransactionConflict)
                    && self.processing_already_advanced(&ctx, &item.transaction_uid)
                {
                    continue;
                }
                errs.push(Some(e));
            }
        }
        match Error::join(errs) {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }
    pub(crate) fn receive_result(
        &self,
        ctx: &Ctx,
        identity: Option<&AssocIdentity>,
        result: StorageCommitmentResult,
    ) -> Result<()> {
        let transaction = self
            .store_get(ctx, &result.transaction_uid)
            .map_err(|e| sanitized(STORAGE_COMMITMENT_FAILURE_PROCESSING, e))?;
        let (calling, called) = match identity {
            Some(id) => (
                ul::normalize_ae_title(&id.calling).to_string(),
                ul::normalize_ae_title(&id.called).to_string(),
            ),
            None => (String::new(), String::new()),
        };
        let callback = ul::equal_ae_titles(&calling, &transaction.committer_ae_title)
            && ul::equal_ae_titles(&called, &transaction.requestor_ae_title);
        let original = ul::equal_ae_titles(&calling, &transaction.requestor_ae_title)
            && ul::equal_ae_titles(&called, &transaction.committer_ae_title);
        let roles_ok = identity.is_some_and(|id| id.roles_valid(&transaction));
        if transaction.direction != STORAGE_COMMITMENT_DIRECTION_REQUESTOR
            || identity.is_none()
            || (!callback && !original)
            || !roles_ok
        {
            return Err(sanitized(
                STORAGE_COMMITMENT_FAILURE_PROTOCOL,
                invalid_result(),
            ));
        }
        let stored = self
            .set_result_inner(ctx, &result, "", true)
            .map_err(|e| sanitized(STORAGE_COMMITMENT_FAILURE_PROCESSING, e))?;
        if stored.state == STORAGE_COMMITMENT_STATE_RESULT_RECEIVED {
            return Ok(());
        }
        if let Some(consumer) = &self.inner.consumer {
            let r = catch_unwind(AssertUnwindSafe(|| consumer(ctx, stored.clone())))
                .unwrap_or_else(|_| Err(Error::new("storage commitment result consumer panic")));
            if let Err(e) = r {
                return Err(sanitized(STORAGE_COMMITMENT_FAILURE_PROCESSING, e));
            }
        }
        self.update_state(
            ctx,
            &stored.transaction_uid,
            &[STORAGE_COMMITMENT_STATE_RESULT_READY],
            STORAGE_COMMITMENT_STATE_RESULT_RECEIVED,
            |next| next.completed_at = self.now(),
        )
        .map(|_| ())
    }
    fn processing_already_advanced(&self, ctx: &Ctx, uid: &str) -> bool {
        match self.store_get(ctx, uid) {
            Ok(t) => matches!(
                t.state.as_str(),
                STORAGE_COMMITMENT_STATE_RESULT_READY
                    | STORAGE_COMMITMENT_STATE_DELIVERING
                    | STORAGE_COMMITMENT_STATE_DELIVERY_FAILED
                    | STORAGE_COMMITMENT_STATE_DELIVERY_EXHAUSTED
                    | STORAGE_COMMITMENT_STATE_DELIVERED
            ),
            Err(_) => false,
        }
    }
    fn claim(
        &self,
        ctx: &Ctx,
        uid: &str,
        allowed: &[&str],
        state: &str,
        require_due: bool,
    ) -> Result<StorageCommitmentTransaction> {
        let token = new_storage_commitment_lease_token();
        for _ in 0..8 {
            let current = self.store_get(ctx, uid)?;
            if !allowed.contains(&current.state.as_str()) {
                return Err(conflict());
            }
            if require_due && !transaction_due(&current, self.now()) {
                return Err(concurrent_update());
            }
            if (current.state == STORAGE_COMMITMENT_STATE_PROCESSING
                || current.state == STORAGE_COMMITMENT_STATE_DELIVERING)
                && current.lease_until.after(self.now())
            {
                return Err(concurrent_update());
            }
            let mut next = current.clone();
            next.state = state.to_string();
            next.lease_token = token.clone();
            next.lease_until = self.now().add(self.lease());
            next.updated_at = self.now();
            match self.store_cas(ctx, &current.transaction_uid, current.version, next) {
                Err(e) if e.is(Kind::StorageCommitmentConcurrentUpdate) => continue,
                r => return r,
            }
        }
        Err(concurrent_update())
    }
    pub fn deliver_callback(
        &self,
        ctx: Option<&Ctx>,
        transaction_uid: &str,
    ) -> (StorageCommitmentTransaction, Result<()>) {
        let ctx = ctx.cloned().unwrap_or_default();
        self.deliver_callback_inner(&ctx, transaction_uid, false)
    }
    fn deliver_callback_inner(
        &self,
        ctx: &Ctx,
        uid: &str,
        require_due: bool,
    ) -> (StorageCommitmentTransaction, Result<()>) {
        let fail = |e: Error| (StorageCommitmentTransaction::default(), Err(e));
        let ready = match self.store_get(ctx, uid) {
            Ok(t) => t,
            Err(e) => return fail(e),
        };
        if ready.delivery_attempts >= self.inner.retry.max_attempts {
            return fail(sentinel(Kind::StorageCommitmentDeliveryExhausted));
        }
        if ready.direction != STORAGE_COMMITMENT_DIRECTION_COMMITTER
            || ready.delivery_mode != STORAGE_COMMITMENT_DELIVERY_CALLBACK
            || ready.result.is_none()
        {
            return fail(conflict());
        }
        let claimed = match self.claim(
            ctx,
            uid,
            DELIVERY_CLAIM_STATES,
            STORAGE_COMMITMENT_STATE_DELIVERING,
            require_due,
        ) {
            Ok(c) => c,
            Err(e) => return fail(e),
        };
        let delivery_err = self.callback_attempt(ctx, &claimed);
        self.finish(&claimed, delivery_err)
    }
    fn finish(
        &self,
        claimed: &StorageCommitmentTransaction,
        delivery_err: Option<Error>,
    ) -> (StorageCommitmentTransaction, Result<()>) {
        let persist = persistence_context();
        match self.finish_delivery(&persist, claimed, delivery_err.as_ref()) {
            Ok(t) => (t, delivery_err.map_or(Ok(()), Err)),
            Err(e) => (StorageCommitmentTransaction::default(), Err(e)),
        }
    }
    fn callback_attempt(&self, ctx: &Ctx, t: &StorageCommitmentTransaction) -> Option<Error> {
        let unknown = || new_delivery_error(sentinel(Kind::StorageCommitmentCallbackUnknown), 0);
        let Some(resolver) = &self.inner.resolver else {
            return Some(unknown());
        };
        let target = match resolver(
            ctx,
            &StorageCommitmentCallbackRequest {
                transaction_uid: t.transaction_uid.clone(),
                requestor_ae_title: t.requestor_ae_title.clone(),
                committer_ae_title: t.committer_ae_title.clone(),
            },
        ) {
            Ok(target) => target,
            Err(e) => return Some(new_delivery_error(e, 0)),
        };
        let address = target.address.trim().to_string();
        let mut options = target.dial_options.clone();
        if address.is_empty() {
            return Some(unknown());
        }
        if options.calling_ae_title.is_empty() {
            options.calling_ae_title = t.committer_ae_title.clone();
        }
        if options.called_ae_title.is_empty() {
            options.called_ae_title = t.requestor_ae_title.clone();
        }
        if !ul::equal_ae_titles(&options.calling_ae_title, &t.committer_ae_title)
            || !ul::equal_ae_titles(&options.called_ae_title, &t.requestor_ae_title)
        {
            return Some(unknown());
        }
        options.contexts = vec![ul::PresentationContext::new(
            STORAGE_COMMITMENT_PUSH_MODEL_SOP_CLASS_UID,
            &[ul::EXPLICIT_VR_LITTLE_ENDIAN, ul::IMPLICIT_VR_LITTLE_ENDIAN],
        )];
        options.role_selections = vec![ul::RoleSelectionItem {
            sop_class_uid: STORAGE_COMMITMENT_PUSH_MODEL_SOP_CLASS_UID.into(),
            scu_role: false,
            scp_role: true,
        }];
        let assoc = match (self.inner.dialer)(ctx, &address, options) {
            Ok(a) => a,
            Err(e) => return Some(new_delivery_error(e, 0)),
        };
        let (acknowledged, send_err) = self.send_result_on_association(ctx, &assoc, t);
        let cleanup = Ctx::background().with_timeout(Duration::from_secs(1));
        if acknowledged {
            let _ = assoc.release(Some(&cleanup));
        } else {
            let _ = assoc.abort_with_context(Some(&cleanup), ul::ABORT_REASON_NOT_SPECIFIED);
        }
        assoc.close();
        send_err
    }
    pub fn deliver_on_association(
        &self,
        ctx: Option<&Ctx>,
        transaction_uid: &str,
        assoc: &Association,
    ) -> (StorageCommitmentTransaction, Result<()>) {
        let ctx = ctx.cloned().unwrap_or_default();
        let fail = |e: Error| (StorageCommitmentTransaction::default(), Err(e));
        let ready = match self.store_get(&ctx, transaction_uid) {
            Ok(t) => t,
            Err(e) => return fail(e),
        };
        if ready.direction != STORAGE_COMMITMENT_DIRECTION_COMMITTER
            || ready.delivery_mode != STORAGE_COMMITMENT_DELIVERY_SAME_ASSOCIATION
            || ready.result.is_none()
            || !ul::equal_ae_titles(&assoc.calling_ae_title, &ready.requestor_ae_title)
            || !ul::equal_ae_titles(&assoc.called_ae_title, &ready.committer_ae_title)
            || !storage_commitment_association_roles_valid(assoc, &ready)
        {
            return fail(conflict());
        }
        let claimed = match self.claim(
            &ctx,
            transaction_uid,
            DELIVERY_CLAIM_STATES,
            STORAGE_COMMITMENT_STATE_DELIVERING,
            false,
        ) {
            Ok(c) => c,
            Err(e) => return fail(e),
        };
        let (_, delivery_err) = self.send_result_on_association(&ctx, assoc, &claimed);
        self.finish(&claimed, delivery_err)
    }
    pub fn complete_manual_delivery(
        &self,
        ctx: Option<&Ctx>,
        transaction_uid: &str,
    ) -> Result<StorageCommitmentTransaction> {
        let ctx = ctx.cloned().unwrap_or_default();
        let t = self
            .store_get(&ctx, transaction_uid)
            .map_err(|e| sanitized(STORAGE_COMMITMENT_FAILURE_PROCESSING, e))?;
        if t.direction != STORAGE_COMMITMENT_DIRECTION_COMMITTER
            || t.delivery_mode != STORAGE_COMMITMENT_DELIVERY_MANUAL
            || t.result.is_none()
        {
            return Err(conflict());
        }
        self.update_state(
            &ctx,
            transaction_uid,
            &[STORAGE_COMMITMENT_STATE_RESULT_READY],
            STORAGE_COMMITMENT_STATE_DELIVERED,
            |next| {
                next.completed_at = self.now();
                next.next_attempt_at = StorageCommitmentTime::ZERO;
                next.last_failure = STORAGE_COMMITMENT_FAILURE_NONE.into();
            },
        )
    }
    fn send_result_on_association(
        &self,
        ctx: &Ctx,
        assoc: &Association,
        t: &StorageCommitmentTransaction,
    ) -> (bool, Option<Error>) {
        let Some(result) = &t.result else {
            return (false, Some(new_delivery_error(conflict(), 0)));
        };
        if !storage_commitment_association_roles_valid(assoc, t) {
            return (false, Some(new_delivery_error(conflict(), 0)));
        }
        let data_set = match build_storage_commitment_event_information(
            &t.transaction_uid,
            &result.referenced_sops,
            &result.failed_sops,
        ) {
            Ok(d) => d,
            Err(_) => return (false, Some(new_delivery_error(invalid_result(), 0))),
        };
        let event_type = if result.failed_sops.is_empty() {
            STORAGE_COMMITMENT_EVENT_TYPE_SUCCESS
        } else {
            STORAGE_COMMITMENT_EVENT_TYPE_FAILURES
        };
        let options = NormalizedOperationOptions {
            operation: super::controls::OperationOptions {
                context: Some(ctx.clone()),
                ..Default::default()
            },
            ..Default::default()
        };
        let (rsp, err) = NormalizedClient::new(assoc).event_report(
            &options,
            NormalizedRequest {
                sop_class_uid: STORAGE_COMMITMENT_PUSH_MODEL_SOP_CLASS_UID.into(),
                sop_instance_uid: STORAGE_COMMITMENT_PUSH_MODEL_SOP_INSTANCE_UID.into(),
                command_data_set_type: DATA_SET_PRESENT,
                type_id: event_type,
                ..Default::default()
            },
            Some(&data_set),
        );
        if let Err(e) = err {
            if let Some(response) = &rsp.response
                && e.is(Kind::NormalizedStatus)
            {
                let status = response.status;
                let status_err = Error::new(format!(
                    "dicom dimse: N-EVENT-REPORT-RSP: storage commitment status 0x{status:04X} ({})",
                    classify_storage_commitment_status(status)
                ))
                .also(Kind::StorageCommitmentStatus)
                .with_detail(Detail::Status(status));
                return (true, Some(new_delivery_error(status_err, status)));
            }
            return (false, Some(new_delivery_error(e, 0)));
        }
        let Some(response) = &rsp.response else {
            return (false, Some(new_delivery_error(invalid_result(), 0)));
        };
        if let Some(e) = check_storage_commitment_status("N-EVENT-REPORT-RSP", response.status) {
            return (true, Some(new_delivery_error(e, response.status)));
        }
        (true, None)
    }
    fn finish_delivery(
        &self,
        ctx: &Ctx,
        claimed: &StorageCommitmentTransaction,
        delivery_err: Option<&Error>,
    ) -> Result<StorageCommitmentTransaction> {
        let current = self.store_get(ctx, &claimed.transaction_uid)?;
        if current.state != STORAGE_COMMITMENT_STATE_DELIVERING
            || current.lease_token != claimed.lease_token
        {
            return Err(concurrent_update());
        }
        let mut next = current.clone();
        next.delivery_attempts += 1;
        next.lease_token.clear();
        next.lease_until = StorageCommitmentTime::ZERO;
        next.updated_at = self.now();
        match delivery_err {
            None => {
                next.state = STORAGE_COMMITMENT_STATE_DELIVERED.into();
                next.completed_at = next.updated_at;
                next.next_attempt_at = StorageCommitmentTime::ZERO;
                next.last_failure = STORAGE_COMMITMENT_FAILURE_NONE.into();
                next.last_dimse_status = 0;
            }
            Some(e) => {
                let typed = new_delivery_error(e.clone(), 0);
                let (class, status, retryable) =
                    storage_commitment_delivery_error(&typed).unwrap_or(("", 0, false));
                next.last_failure = class.to_string();
                next.last_dimse_status = status;
                if retryable && next.delivery_attempts < self.inner.retry.max_attempts {
                    next.state = STORAGE_COMMITMENT_STATE_DELIVERY_FAILED.into();
                    next.next_attempt_at = next.updated_at.add(storage_commitment_backoff(
                        self.inner.retry,
                        next.delivery_attempts,
                    ));
                } else {
                    next.state = STORAGE_COMMITMENT_STATE_DELIVERY_EXHAUSTED.into();
                    next.next_attempt_at = StorageCommitmentTime::ZERO;
                }
            }
        }
        self.store_cas(ctx, &current.transaction_uid, current.version, next)
    }
    pub fn deliver_due(&self, ctx: Option<&Ctx>, limit: i64) -> Result<()> {
        if limit <= 0 || limit > self.inner.limits.max_list_batch {
            return Err(resource_limit());
        }
        let ctx = ctx.cloned().unwrap_or_default();
        let items = self.store_list(
            &ctx,
            &StorageCommitmentTransactionQuery {
                due_at: self.now(),
                limit,
                states: DELIVERY_CLAIM_STATES
                    .iter()
                    .map(|s| s.to_string())
                    .collect(),
            },
        )?;
        let mut errs = Vec::new();
        for item in items {
            if item.delivery_mode != STORAGE_COMMITMENT_DELIVERY_CALLBACK
                || item.delivery_attempts >= self.inner.retry.max_attempts
                || (item.next_attempt_at.is_zero()
                    && item.state == STORAGE_COMMITMENT_STATE_DELIVERY_FAILED)
            {
                continue;
            }
            if let (_, Err(e)) = self.deliver_callback_inner(&ctx, &item.transaction_uid, true)
                && !e.is(Kind::StorageCommitmentConcurrentUpdate)
            {
                if e.is(Kind::StorageCommitmentTransactionConflict)
                    && self.delivery_already_advanced(&ctx, &item.transaction_uid)
                {
                    continue;
                }
                errs.push(Some(e));
            }
        }
        match Error::join(errs) {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }
    fn delivery_already_advanced(&self, ctx: &Ctx, uid: &str) -> bool {
        match self.store_get(ctx, uid) {
            Ok(t) => {
                t.state == STORAGE_COMMITMENT_STATE_DELIVERED
                    || t.state == STORAGE_COMMITMENT_STATE_DELIVERY_EXHAUSTED
            }
            Err(_) => false,
        }
    }
    pub fn await_result_on_association(
        &self,
        ctx: Option<&Ctx>,
        assoc: &Association,
        transaction_uid: &str,
        wait: Nanos,
    ) -> (StorageCommitmentAwaitResult, Result<()>) {
        let ctx = ctx.cloned().unwrap_or_default();
        let with = |t: StorageCommitmentTransaction, released: bool, r: Result<()>| {
            (
                StorageCommitmentAwaitResult {
                    transaction: t,
                    peer_released: released,
                },
                r,
            )
        };
        if wait.0 < 0 {
            return with(Default::default(), false, Err(invalid_request()));
        }
        let mut transaction = match self.store_get(&ctx, transaction_uid) {
            Ok(t) => t,
            Err(e) => return with(Default::default(), false, Err(e)),
        };
        if transaction.direction != STORAGE_COMMITMENT_DIRECTION_REQUESTOR {
            return with(transaction, false, Err(conflict()));
        }
        let _guard = match begin_association_operation(Some(assoc)) {
            Ok(g) => g,
            Err(e) => return with(transaction, false, Err(e)),
        };
        let start = Instant::now();
        let deadline = start
            .checked_add(Duration::from_nanos(wait.0 as u64))
            .unwrap_or_else(|| start + Duration::from_secs(100 * 365 * 24 * 3600));
        let options = self.normalized_options(assoc);
        loop {
            if storage_commitment_state_terminal(&transaction.state) || Instant::now() >= deadline {
                return with(transaction, false, Ok(()));
            }
            let polled = match await_storage_commitment_command(&ctx, assoc, deadline) {
                Ok(p) => p,
                Err(e) => return with(transaction, false, Err(e)),
            };
            match polled {
                Polled::Released => {
                    return match self.store_get(&ctx, transaction_uid) {
                        Ok(latest) => with(latest, true, Ok(())),
                        Err(e) => with(transaction, true, Err(e)),
                    };
                }
                Polled::Idle => {}
                Polled::Command(pc_id, command) => {
                    let field = match command_uint16(Some(&command), COMMAND_FIELD) {
                        Ok(f) => f,
                        Err(e) => return with(transaction, false, Err(e)),
                    };
                    if field != N_EVENT_REPORT_RQ {
                        return with(
                            transaction,
                            false,
                            Err(Error::new(format!(
                                "dicom dimse: unexpected command field 0x{field:04X} while awaiting Storage Commitment result"
                            ))),
                        );
                    }
                    if let Err(e) =
                        serve_normalized_command(Some(&ctx), assoc, pc_id, &command, &options)
                        && !is_handled_normalized_scp_error(&e)
                    {
                        return with(transaction, false, Err(e));
                    }
                }
            }
            match self.store_get(&ctx, transaction_uid) {
                Ok(latest) => transaction = latest,
                Err(e) => return with(transaction, false, Err(e)),
            }
        }
    }
}
const DELIVERY_CLAIM_STATES: &[&str] = &[
    STORAGE_COMMITMENT_STATE_RESULT_READY,
    STORAGE_COMMITMENT_STATE_DELIVERY_FAILED,
    STORAGE_COMMITMENT_STATE_DELIVERING,
];
enum Polled {
    Idle,
    Released,
    Command(u8, Box<Object>),
}
fn await_storage_commitment_command(
    ctx: &Ctx,
    assoc: &Association,
    deadline: Instant,
) -> Result<Polled> {
    let mut values = assoc.take_pdata_carryover(Some(ctx))?;
    if values.is_empty() {
        let poll_until = (Instant::now() + STORAGE_COMMITMENT_AWAIT_POLL).min(deadline);
        let poll_ctx = ctx.with_deadline(poll_until);
        let received = assoc.receive(Some(&poll_ctx));
        let poll_expired = poll_ctx.err().is_some() || Instant::now() >= poll_until;
        let pdu = match received {
            Ok(p) => p,
            Err(e) => {
                let e = Error::from(e);
                if poll_expired && ctx.err().is_none() && is_poll_timeout(&e) {
                    return Ok(Polled::Idle);
                }
                return Err(e);
            }
        };
        match pdu {
            Pdu::ReleaseRq => {
                assoc.write_pdu(&Pdu::ReleaseRp)?;
                return Ok(Polled::Released);
            }
            Pdu::PData(p) => values = p.values,
            other => {
                return Err(ul::unexpected_pdu_error(
                    &other,
                    "waiting for N-EVENT-REPORT or A-RELEASE-RQ",
                )
                .into());
            }
        }
    }
    assoc.store_pdata_carryover(Some(ctx), &values)?;
    match receive_storage_command_or_release(Some(&ctx.with_active_transfer()), assoc)? {
        None => Ok(Polled::Released),
        Some((pc_id, command)) => Ok(Polled::Command(pc_id, Box::new(command))),
    }
}
fn is_poll_timeout(err: &Error) -> bool {
    err.is_ul(ul::Kind::AssociationTimeout) || err.is_deadline() || err.is_ul(ul::Kind::NetTimeout)
}
