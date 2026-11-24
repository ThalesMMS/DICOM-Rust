use super::*;
use crate::net::dimse::{
    AssociationScpOptions, NormalizedClient, NormalizedOperationOptions, NormalizedRequest,
    NormalizedResponse, NormalizedResult, NormalizedScpOptions, NormalizedScpResult,
};
use crate::net::ul::{AcceptedContext, Association, Ctx, PresentationContext};
use crate::object::text::normalize_uid;
use crate::object::{Element, Vr, transfer};
use std::collections::HashSet;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
pub const ACTION_CHANGE_STATE: u16 = 1;
pub const ACTION_REQUEST_CANCEL: u16 = 2;
pub const ACTION_SUBSCRIBE: u16 = 3;
pub const ACTION_UNSUBSCRIBE: u16 = 4;
pub const ACTION_SUSPEND_GLOBAL: u16 = 5;
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChangeStateInformation {
    pub state: State,
    pub transaction_uid: String,
}
pub fn build_change_state_information(state: &str, transaction_uid: &str) -> Result<Object> {
    if !valid_state(state) || !valid_uid(transaction_uid) {
        return Err(Kind::InvalidDataSet.into());
    }
    Ok(new_data_set(vec![
        string_element(TAG_PROCEDURE_STEP_STATE, Vr::CS, &[state]),
        string_element(
            TAG_TRANSACTION_UID,
            Vr::UI,
            &[normalize_uid(transaction_uid)],
        ),
    ]))
}
pub fn parse_change_state_information(ds: Option<&Object>) -> Result<ChangeStateInformation> {
    const OP: &str = "N-ACTION Change UPS State";
    let invalid = || Error::from(Kind::InvalidDataSet);
    let Some(ds) = ds else {
        return Err(status_error(OP, STATUS_MISSING_ATTRIBUTE, invalid()));
    };
    let data = ds.to_data_set();
    let Some(state) = data_set_element(&data, TAG_PROCEDURE_STEP_STATE) else {
        return Err(status_error(OP, STATUS_MISSING_ATTRIBUTE, invalid()));
    };
    let state = element_string_value(state);
    if state.trim().is_empty() {
        return Err(status_error(OP, STATUS_MISSING_ATTRIBUTE_VALUE, invalid()));
    }
    if !valid_state(&state) {
        return Err(status_error(
            OP,
            STATUS_ONLY_SCHEDULED_VIA_CREATE,
            invalid(),
        ));
    }
    let transaction = match ds.lookup_string(TAG_TRANSACTION_UID) {
        Ok(t) if valid_uid(&t) => t,
        _ => {
            return Err(status_error(
                OP,
                STATUS_INCORRECT_TRANSACTION_UID,
                invalid(),
            ));
        }
    };
    let elements = ds.elements();
    if elements.len() != 2 {
        return Err(invalid());
    }
    let mut seen = HashSet::new();
    for e in elements {
        if (e.tag() != TAG_PROCEDURE_STEP_STATE && e.tag() != TAG_TRANSACTION_UID)
            || !seen.insert(e.tag())
        {
            return Err(invalid());
        }
    }
    Ok(ChangeStateInformation {
        state,
        transaction_uid: normalize_uid(&transaction).to_string(),
    })
}
pub fn build_subscription_information(
    receiving_ae_title: &str,
    deletion_lock: bool,
    matching_keys: &MatchingKeys,
) -> Result<Object> {
    if !valid_ae_title(receiving_ae_title) {
        return Err(Kind::InvalidDataSet.into());
    }
    let lock = if deletion_lock { "TRUE" } else { "FALSE" };
    let mut elements = vec![
        string_element(TAG_RECEIVING_AE, Vr::AE, &[receiving_ae_title]),
        string_element(TAG_DELETION_LOCK, Vr::LO, &[lock]),
    ];
    if !matching_keys.is_empty() {
        let filter = compile_subscription_filter(matching_keys)
            .map_err(|_| Error::from(Kind::InvalidDataSet))?;
        for key in filter.keys {
            let vr = query_entry(key.tag).unwrap_or(Vr::UN);
            elements.push(string_element_owned(key.tag, vr, key.values));
        }
    }
    Ok(new_data_set(with_query_character_set(elements)))
}
pub fn build_unsubscription_information(receiving_ae_title: &str) -> Result<Object> {
    if !valid_ae_title(receiving_ae_title) {
        return Err(Kind::InvalidDataSet.into());
    }
    Ok(new_data_set(vec![string_element(
        TAG_RECEIVING_AE,
        Vr::AE,
        &[receiving_ae_title],
    )]))
}
const WATCH_OP: &str = "N-ACTION UPS Watch";
pub(crate) fn parse_subscription_information(
    ds: Option<&Object>,
    require_lock: bool,
) -> Result<(String, bool, MatchingKeys)> {
    let invalid = || Error::from(Kind::InvalidDataSet);
    let receiving = required_subscription_string(ds, TAG_RECEIVING_AE)?;
    if !valid_ae_title(&receiving) {
        return Err(invalid());
    }
    let mut allowed = vec![TAG_RECEIVING_AE];
    let mut lock = false;
    if require_lock {
        let value = required_subscription_string(ds, TAG_DELETION_LOCK)?;
        if value != "TRUE" && value != "FALSE" {
            return Err(invalid());
        }
        lock = value == "TRUE";
        allowed.push(TAG_DELETION_LOCK);
    }
    let ds = ds.unwrap();
    let mut keys = MatchingKeys::new();
    for e in ds.elements() {
        if allowed.contains(&e.tag()) {
            continue;
        }
        let Some(vr) = query_entry(e.tag()) else {
            return Err(status_error(WATCH_OP, STATUS_NO_SUCH_ARGUMENT, invalid()));
        };
        if e.vr() != vr {
            return Err(status_error(
                WATCH_OP,
                STATUS_INVALID_ARGUMENT_VALUE,
                invalid(),
            ));
        }
        let Ok(values) = ds.lookup_strings(e.tag()) else {
            return Err(status_error(
                WATCH_OP,
                STATUS_INVALID_ARGUMENT_VALUE,
                invalid(),
            ));
        };
        if e.tag() == TAG_SPECIFIC_CHARACTER_SET && ds.character_set().is_err() {
            return Err(status_error(
                WATCH_OP,
                STATUS_INVALID_ARGUMENT_VALUE,
                invalid(),
            ));
        }
        keys.insert(e.tag().to_string(), values);
    }
    Ok((receiving, lock, keys))
}
fn required_subscription_string(ds: Option<&Object>, tag: Tag) -> Result<String> {
    let invalid = || Error::from(Kind::InvalidDataSet);
    let Some(ds) = ds else {
        return Err(status_error(WATCH_OP, STATUS_MISSING_ATTRIBUTE, invalid()));
    };
    let data = ds.to_data_set();
    let Some(e) = data_set_element(&data, tag) else {
        return Err(status_error(WATCH_OP, STATUS_MISSING_ATTRIBUTE, invalid()));
    };
    let value = element_string_value(e);
    if value.trim().is_empty() {
        return Err(status_error(
            WATCH_OP,
            STATUS_MISSING_ATTRIBUTE_VALUE,
            invalid(),
        ));
    }
    Ok(value)
}
type HandlerOut = (NormalizedScpResult, Option<dimse::Error>);
fn reply(status: u16, err: Option<Error>) -> HandlerOut {
    (
        NormalizedScpResult {
            response: NormalizedResponse {
                status,
                ..Default::default()
            },
            data_set: None,
        },
        err.map(to_dimse),
    )
}
fn typed_reply(status: u16, type_id: u16, err: Option<Error>) -> HandlerOut {
    let (mut r, e) = reply(status, err);
    r.response.type_id = Some(type_id);
    (r, e)
}
impl Service {
    pub fn normalized_options(&self, assoc: Option<&Association>) -> NormalizedScpOptions {
        let calling = assoc.map(|a| a.calling_ae_title.clone());
        let create = self.clone();
        let get = self.clone();
        let set = self.clone();
        let action = self.clone();
        let event = self.clone();
        let mut options = NormalizedScpOptions {
            max_data_set_bytes: self.inner.limits.max_data_set_bytes,
            presentation_context_policy: Some(Arc::new(|pc: &AcceptedContext, class: &str| {
                let refuse = || {
                    Err(dimse::normalized_presentation_context_error(
                        pc.id,
                        &pc.abstract_syntax_uid,
                        class,
                    ))
                };
                if class != PUSH_SOP_CLASS_UID {
                    return refuse();
                }
                match pc.abstract_syntax_uid.as_str() {
                    PUSH_SOP_CLASS_UID | PULL_SOP_CLASS_UID | WATCH_SOP_CLASS_UID
                    | EVENT_SOP_CLASS_UID => Ok(()),
                    _ => refuse(),
                }
            })),
            create_handler: Some(Arc::new(move |ctx, req, ds| {
                create.normalized_create(ctx, req, ds)
            })),
            get_handler: Some(Arc::new(move |ctx, req, _| get.normalized_get(ctx, req))),
            set_handler: Some(Arc::new(move |ctx, req, ds| {
                set.normalized_set(ctx, req, ds)
            })),
            action_handler: Some(Arc::new(move |ctx, req, ds| {
                action.normalized_action(ctx, calling.as_deref(), req, ds)
            })),
            ..Default::default()
        };
        if self.inner.event_handler.is_some() {
            options.event_report_handler = Some(Arc::new(move |ctx, req, ds| {
                event.normalized_event_report(ctx, req, ds)
            }));
        }
        options
    }
    pub fn association_scp_options(&self, assoc: Option<&Association>) -> AssociationScpOptions {
        AssociationScpOptions {
            normalized_scp: Some(self.normalized_options(assoc)),
            ..Default::default()
        }
    }
    fn normalized_create(
        &self,
        ctx: &Ctx,
        req: &NormalizedRequest,
        ds: Option<&Object>,
    ) -> HandlerOut {
        if let Err(e) = require_normalized_abstract_syntax(ctx, "N-CREATE", &[PUSH_SOP_CLASS_UID]) {
            return reply(dimse::STATUS_UNRECOGNIZED_OPERATION, Some(e));
        }
        let r = self.create(
            ctx,
            &CreateRequest {
                sop_instance_uid: req.sop_instance_uid.clone(),
                attributes: ds,
            },
        );
        let uid = r
            .as_ref()
            .map(|s| s.sop_instance_uid.clone())
            .unwrap_or_default();
        let (status, err) = status_for_handler_error(r.err());
        let (mut out, err) = reply(status, err);
        out.response.affected_sop_instance_uid = uid;
        (out, err)
    }
    fn normalized_get(&self, ctx: &Ctx, req: &NormalizedRequest) -> HandlerOut {
        if let Err(e) = require_normalized_abstract_syntax(
            ctx,
            "N-GET",
            &[PUSH_SOP_CLASS_UID, PULL_SOP_CLASS_UID, WATCH_SOP_CLASS_UID],
        ) {
            return reply(dimse::STATUS_UNRECOGNIZED_OPERATION, Some(e));
        }
        for tag in &req.attribute_identifier_list {
            if *tag == TAG_TRANSACTION_UID
                || *tag == TAG_SOP_CLASS_UID
                || *tag == TAG_SOP_INSTANCE_UID
            {
                return reply(STATUS_NO_SUCH_ATTRIBUTE, Some(Kind::InvalidDataSet.into()));
            }
        }
        let step = match self.get(ctx, &req.sop_instance_uid) {
            Err(e) => {
                let (status, err) = status_for_handler_error(Some(e));
                return reply(status, err);
            }
            Ok(s) => s,
        };
        let mut projected: Vec<Element> = Vec::new();
        let mut missing = false;
        if req.attribute_identifier_list.is_empty() {
            for e in &step.attributes.elements {
                if e.tag() != TAG_TRANSACTION_UID
                    && e.tag() != TAG_SOP_CLASS_UID
                    && e.tag() != TAG_SOP_INSTANCE_UID
                {
                    projected.push(e.clone());
                }
            }
        } else {
            for tag in &req.attribute_identifier_list {
                match data_set_element(&step.attributes, *tag) {
                    None => missing = true,
                    Some(e) => projected.push(e.clone()),
                }
            }
        }
        let status = if missing {
            STATUS_REQUESTED_OPTIONAL_UNSUPPORTED
        } else {
            STATUS_SUCCESS
        };
        let (mut out, err) = reply(status, None);
        out.data_set = Some(Object::from_data_set(DataSet {
            elements: projected,
            item_offset: None,
        }));
        (out, err)
    }
    fn normalized_set(
        &self,
        ctx: &Ctx,
        req: &NormalizedRequest,
        ds: Option<&Object>,
    ) -> HandlerOut {
        if let Err(e) = require_normalized_abstract_syntax(ctx, "N-SET", &[PULL_SOP_CLASS_UID]) {
            return reply(dimse::STATUS_UNRECOGNIZED_OPERATION, Some(e));
        }
        let transaction = ds
            .and_then(|d| d.lookup_string(TAG_TRANSACTION_UID).ok())
            .unwrap_or_default();
        let r = self.set(
            ctx,
            &SetRequest {
                sop_instance_uid: req.sop_instance_uid.clone(),
                transaction_uid: transaction,
                modifications: ds,
            },
        );
        let (status, err) = status_for_handler_error(r.err());
        reply(status, err)
    }
    fn normalized_action(
        &self,
        ctx: &Ctx,
        calling: Option<&str>,
        req: &NormalizedRequest,
        information: Option<&Object>,
    ) -> HandlerOut {
        let action = req.type_id;
        if let Some(info) = information
            && let Err(e) = validate_data_set(ctx, &info.to_data_set(), &self.inner.limits)
        {
            let status = if e.is(Kind::ResourceLimit) {
                STATUS_RESOURCE_LIMITATION
            } else {
                dimse::STATUS_PROCESSING_FAILURE
            };
            return typed_reply(status, action, Some(e));
        }
        let uid = req.sop_instance_uid.clone();
        let (status, err): (u16, Option<Error>) = match action {
            ACTION_CHANGE_STATE => 'a: {
                if let Err(e) = require_normalized_abstract_syntax(
                    ctx,
                    "N-ACTION Change UPS State",
                    &[PULL_SOP_CLASS_UID],
                ) {
                    break 'a (dimse::STATUS_UNRECOGNIZED_OPERATION, Some(e));
                }
                if let Err(e) = self.get(ctx, &uid) {
                    break 'a status_for_handler_error(Some(e));
                }
                let parsed = match parse_change_state_information(information) {
                    Err(e) => break 'a status_for_handler_error(Some(e)),
                    Ok(p) => p,
                };
                let r = self.change_state(
                    ctx,
                    &ChangeStateRequest {
                        sop_instance_uid: uid,
                        state: parsed.state,
                        transaction_uid: parsed.transaction_uid,
                    },
                );
                status_for_handler_error(r.err())
            }
            ACTION_REQUEST_CANCEL => 'a: {
                if let Err(e) = require_normalized_abstract_syntax(
                    ctx,
                    "N-ACTION Request UPS Cancel",
                    &[PUSH_SOP_CLASS_UID, WATCH_SOP_CLASS_UID],
                ) {
                    break 'a (dimse::STATUS_UNRECOGNIZED_OPERATION, Some(e));
                }
                let r = self.request_cancel(
                    ctx,
                    &CancelRequest {
                        sop_instance_uid: uid,
                        requesting_ae_title: calling.unwrap_or_default().to_string(),
                        information,
                    },
                );
                status_for_handler_error(r.err())
            }
            ACTION_SUBSCRIBE => 'a: {
                if let Err(e) = require_normalized_abstract_syntax(
                    ctx,
                    "N-ACTION Subscribe",
                    &[WATCH_SOP_CLASS_UID],
                ) {
                    break 'a (dimse::STATUS_UNRECOGNIZED_OPERATION, Some(e));
                }
                let (receiving, lock, keys) =
                    match parse_subscription_information(information, true) {
                        Err(e) => break 'a status_for_handler_error(Some(e)),
                        Ok(p) => p,
                    };
                match self.subscribe(
                    ctx,
                    &SubscribeRequest {
                        sop_instance_uid: uid,
                        receiving_ae_title: receiving,
                        deletion_lock: lock,
                        matching_keys: keys,
                    },
                ) {
                    Err(e) => status_for_handler_error(Some(e)),
                    Ok(r) => (r.status, None),
                }
            }
            ACTION_UNSUBSCRIBE => 'a: {
                if let Err(e) = require_normalized_abstract_syntax(
                    ctx,
                    "N-ACTION Unsubscribe",
                    &[WATCH_SOP_CLASS_UID],
                ) {
                    break 'a (dimse::STATUS_UNRECOGNIZED_OPERATION, Some(e));
                }
                let (receiving, _, keys) = match parse_subscription_information(information, false)
                {
                    Err(e) => break 'a status_for_handler_error(Some(e)),
                    Ok(p) => p,
                };
                if !keys.is_empty() {
                    break 'a (
                        dimse::STATUS_PROCESSING_FAILURE,
                        Some(Kind::InvalidDataSet.into()),
                    );
                }
                let r = self.unsubscribe(
                    ctx,
                    &UnsubscribeRequest {
                        sop_instance_uid: uid,
                        receiving_ae_title: receiving,
                    },
                );
                status_for_handler_error(r.err())
            }
            ACTION_SUSPEND_GLOBAL => 'a: {
                if let Err(e) = require_normalized_abstract_syntax(
                    ctx,
                    "N-ACTION Suspend Global",
                    &[WATCH_SOP_CLASS_UID],
                ) {
                    break 'a (dimse::STATUS_UNRECOGNIZED_OPERATION, Some(e));
                }
                if uid != GLOBAL_SUBSCRIPTION_SOP_INSTANCE_UID {
                    break 'a (
                        STATUS_ACTION_NOT_APPROPRIATE,
                        Some(Kind::InvalidDataSet.into()),
                    );
                }
                let (receiving, _, keys) = match parse_subscription_information(information, false)
                {
                    Err(e) => break 'a status_for_handler_error(Some(e)),
                    Ok(p) => p,
                };
                if !keys.is_empty() {
                    break 'a (
                        dimse::STATUS_PROCESSING_FAILURE,
                        Some(Kind::InvalidDataSet.into()),
                    );
                }
                status_for_handler_error(self.suspend_global(ctx, &receiving).err())
            }
            _ => (
                dimse::STATUS_NO_SUCH_ACTION_TYPE,
                Some(Kind::InvalidDataSet.into()),
            ),
        };
        typed_reply(status, action, err)
    }
    fn normalized_event_report(
        &self,
        ctx: &Ctx,
        req: &NormalizedRequest,
        information: Option<&Object>,
    ) -> HandlerOut {
        if let Err(e) =
            require_normalized_abstract_syntax(ctx, "N-EVENT-REPORT", &[EVENT_SOP_CLASS_UID])
        {
            return reply(dimse::STATUS_UNRECOGNIZED_OPERATION, Some(e));
        }
        if req.type_id < EVENT_STATE_REPORT || req.type_id > EVENT_UPS_ASSIGNED {
            return reply(
                dimse::STATUS_NO_SUCH_EVENT_TYPE,
                Some(Kind::InvalidDataSet.into()),
            );
        }
        let Some(information) = information else {
            return reply(
                dimse::STATUS_PROCESSING_FAILURE,
                Some(Kind::InvalidDataSet.into()),
            );
        };
        if let Err(e) = validate_data_set(ctx, &information.to_data_set(), &self.inner.limits) {
            return reply(dimse::STATUS_PROCESSING_FAILURE, Some(e));
        }
        if let Err(e) = validate_event_report_information(req.type_id, Some(information)) {
            return reply(dimse::STATUS_PROCESSING_FAILURE, Some(e));
        }
        let handler = self.inner.event_handler.as_ref().unwrap();
        let err = call_event_handler(
            handler.as_ref(),
            ctx,
            &ReceivedEvent {
                sop_instance_uid: req.sop_instance_uid.clone(),
                event_type: req.type_id,
                information,
            },
        )
        .err();
        let status = if err.is_some() {
            dimse::STATUS_PROCESSING_FAILURE
        } else {
            STATUS_SUCCESS
        };
        typed_reply(status, req.type_id, err)
    }
}
fn require_normalized_abstract_syntax(ctx: &Ctx, operation: &str, allowed: &[&str]) -> Result<()> {
    let Some(info) = dimse::normalized_request_info_from_context(Some(ctx)) else {
        return Ok(());
    };
    if allowed.contains(&info.presentation_context.abstract_syntax_uid.as_str()) {
        return Ok(());
    }
    Err(status_error(
        operation,
        dimse::STATUS_UNRECOGNIZED_OPERATION,
        Kind::InvalidState.into(),
    ))
}
fn status_for_handler_error(err: Option<Error>) -> (u16, Option<Error>) {
    match err {
        None => (STATUS_SUCCESS, None),
        Some(e) => match e.status_error() {
            Some((_, status)) => (status, Some(e)),
            None => (dimse::STATUS_PROCESSING_FAILURE, Some(e)),
        },
    }
}
pub(crate) fn validate_event_report_information(
    event_type: EventType,
    information: Option<&Object>,
) -> Result<()> {
    let invalid = || Err(Error::from(Kind::InvalidDataSet));
    let Some(information) = information else {
        return invalid();
    };
    let ds = information.to_data_set();
    match event_type {
        EVENT_STATE_REPORT => {
            let state = data_set_string(&ds, TAG_PROCEDURE_STEP_STATE);
            let readiness = data_set_element(&ds, TAG_INPUT_READINESS_STATE);
            if !state.is_some_and(|s| valid_state(&s))
                || !readiness.is_some_and(|r| validate_set_element(r).is_ok())
            {
                return invalid();
            }
        }
        EVENT_CANCEL_REQUESTED => {
            let ok = data_set_string(&ds, TAG_REQUESTING_AE).is_some_and(|ae| valid_ae_title(&ae));
            if !ok || validate_cancel_information(&ds, true).is_err() {
                return invalid();
            }
        }
        EVENT_PROGRESS_REPORT => {
            let ok = data_set_element(&ds, TAG_PROCEDURE_STEP_PROGRESS_INFORMATION_SEQUENCE)
                .is_some_and(|p| {
                    sequence_items(p).is_some_and(|i| i.len() == 1)
                        && validate_progress_information_element(p).is_ok()
                });
            if !ok {
                return invalid();
            }
        }
        EVENT_SCP_STATUS_CHANGE => {
            let status = data_set_string(&ds, TAG_SCP_STATUS);
            let subscriptions = data_set_string(&ds, TAG_SUBSCRIPTION_LIST_STATUS);
            let steps = data_set_string(&ds, TAG_UNIFIED_PROCEDURE_STEP_LIST_STATUS);
            let (Some(status), Some(subscriptions), Some(steps)) = (status, subscriptions, steps)
            else {
                return invalid();
            };
            if !valid_scp_status_change(&ScpStatusChange {
                status,
                subscription_list_status: subscriptions,
                ups_list_status: steps,
            }) {
                return invalid();
            }
        }
        EVENT_UPS_ASSIGNED => {
            if data_set_element(&ds, TAG_SCHEDULED_HUMAN_PERFORMERS_SEQUENCE).is_some() {
                return invalid();
            }
            let station = has_non_empty_sequence(&ds, TAG_SCHEDULED_STATION_NAME_CODE_SEQUENCE);
            let station_present =
                data_set_element(&ds, TAG_SCHEDULED_STATION_NAME_CODE_SEQUENCE).is_some();
            let human = has_non_empty_sequence(&ds, TAG_HUMAN_PERFORMER_CODE_SEQUENCE);
            let human_present = data_set_element(&ds, TAG_HUMAN_PERFORMER_CODE_SEQUENCE).is_some();
            let organization = data_set_string(&ds, TAG_HUMAN_PERFORMER_ORGANIZATION)
                .is_some_and(|o| !o.trim().is_empty());
            if (station_present && !station) || (human_present && !human) || human != organization {
                return invalid();
            }
            if let Some(s) = data_set_element(&ds, TAG_SCHEDULED_STATION_NAME_CODE_SEQUENCE)
                && !required_single_code_sequence(s)
            {
                return invalid();
            }
            if let Some(h) = data_set_element(&ds, TAG_HUMAN_PERFORMER_CODE_SEQUENCE)
                && !required_single_code_sequence(h)
            {
                return invalid();
            }
        }
        _ => return invalid(),
    }
    Ok(())
}
fn call_event_handler(
    handler: &dyn EventHandler,
    ctx: &Ctx,
    event: &ReceivedEvent<'_>,
) -> Result<()> {
    match catch_unwind(AssertUnwindSafe(|| handler.handle_ups_event(ctx, event))) {
        Ok(Ok(())) => Ok(()),
        Ok(Err(e)) => Err(Error::Delivery {
            class: DeliveryFailureClass::CallbackUnknown,
            status: 0,
            retryable: false,
            err: Some(Box::new(e)),
        }),
        Err(_) => Err(Error::Delivery {
            class: DeliveryFailureClass::CallbackUnknown,
            status: 0,
            retryable: false,
            err: Some(Box::new(Kind::DeliveryFailed.into())),
        }),
    }
}
pub fn transfer_syntax_uids() -> Vec<String> {
    vec![
        transfer::EXPLICIT_VR_LITTLE_ENDIAN.uid.into(),
        transfer::IMPLICIT_VR_LITTLE_ENDIAN.uid.into(),
    ]
}
pub fn presentation_context(sop_class_uid: &str) -> Result<PresentationContext> {
    match sop_class_uid {
        PUSH_SOP_CLASS_UID | WATCH_SOP_CLASS_UID | PULL_SOP_CLASS_UID | EVENT_SOP_CLASS_UID
        | QUERY_SOP_CLASS_UID => Ok(PresentationContext {
            abstract_syntax_uid: sop_class_uid.into(),
            transfer_syntax_uids: transfer_syntax_uids(),
            ..Default::default()
        }),
        _ => Err(Kind::InvalidDataSet.into()),
    }
}
pub fn presentation_contexts(sop_class_uids: &[&str]) -> Result<Vec<PresentationContext>> {
    let all = [
        PUSH_SOP_CLASS_UID,
        WATCH_SOP_CLASS_UID,
        PULL_SOP_CLASS_UID,
        EVENT_SOP_CLASS_UID,
        QUERY_SOP_CLASS_UID,
    ];
    let uids: &[&str] = if sop_class_uids.is_empty() {
        &all
    } else {
        sop_class_uids
    };
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for uid in uids {
        if seen.contains(uid) {
            continue;
        }
        out.push(presentation_context(uid)?);
        seen.insert(*uid);
    }
    Ok(out)
}
#[derive(Debug, Clone, Default)]
pub struct OperationResult {
    pub status: u16,
    pub sop_instance_uid: String,
    pub data_set: Option<Object>,
    pub presentation_context: AcceptedContext,
}
fn operation_result(r: NormalizedResult) -> OperationResult {
    let mut out = OperationResult {
        data_set: r.data_set,
        presentation_context: r.presentation_context,
        ..Default::default()
    };
    if let Some(rsp) = r.response {
        out.status = rsp.status;
        out.sop_instance_uid = rsp.affected_sop_instance_uid;
    }
    out
}
fn data_set_type(ds: Option<&Object>) -> u16 {
    if ds.is_none() {
        dimse::NO_DATA_SET
    } else {
        dimse::DATA_SET_PRESENT
    }
}
pub struct Client<'a> {
    normalized: NormalizedClient<'a>,
}
impl<'a> Client<'a> {
    pub fn new(assoc: &'a Association) -> Client<'a> {
        Client {
            normalized: NormalizedClient::new(assoc),
        }
    }
    fn options(ctx: Option<&Ctx>, abstract_syntax: &str) -> NormalizedOperationOptions {
        let mut o = NormalizedOperationOptions::default();
        o.operation.context = ctx.cloned();
        o.presentation_context_abstract_syntax_uid = abstract_syntax.into();
        o
    }
    pub fn create(
        &self,
        ctx: Option<&Ctx>,
        sop_instance_uid: &str,
        attributes: Option<&Object>,
    ) -> (OperationResult, Result<()>) {
        let (r, e) = self.normalized.create(
            &Self::options(ctx, PUSH_SOP_CLASS_UID),
            NormalizedRequest {
                sop_class_uid: PUSH_SOP_CLASS_UID.into(),
                sop_instance_uid: sop_instance_uid.into(),
                command_data_set_type: data_set_type(attributes),
                ..Default::default()
            },
            attributes,
        );
        (operation_result(r), e.map_err(Error::Dimse))
    }
    pub fn get(
        &self,
        ctx: Option<&Ctx>,
        abstract_syntax_uid: &str,
        sop_instance_uid: &str,
        attributes: &[Tag],
    ) -> (OperationResult, Result<()>) {
        if ![PUSH_SOP_CLASS_UID, PULL_SOP_CLASS_UID, WATCH_SOP_CLASS_UID]
            .contains(&abstract_syntax_uid)
        {
            return (OperationResult::default(), Err(Kind::InvalidDataSet.into()));
        }
        let (r, e) = self.normalized.get(
            &Self::options(ctx, abstract_syntax_uid),
            NormalizedRequest {
                sop_class_uid: PUSH_SOP_CLASS_UID.into(),
                sop_instance_uid: sop_instance_uid.into(),
                attribute_identifier_list: attributes.to_vec(),
                ..Default::default()
            },
        );
        (operation_result(r), e.map_err(Error::Dimse))
    }
    pub fn set(
        &self,
        ctx: Option<&Ctx>,
        sop_instance_uid: &str,
        modifications: Option<&Object>,
    ) -> (OperationResult, Result<()>) {
        let (r, e) = self.normalized.set(
            &Self::options(ctx, PULL_SOP_CLASS_UID),
            NormalizedRequest {
                sop_class_uid: PUSH_SOP_CLASS_UID.into(),
                sop_instance_uid: sop_instance_uid.into(),
                ..Default::default()
            },
            modifications,
        );
        (operation_result(r), e.map_err(Error::Dimse))
    }
    pub fn change_state(
        &self,
        ctx: Option<&Ctx>,
        sop_instance_uid: &str,
        state: &str,
        transaction_uid: &str,
    ) -> (OperationResult, Result<()>) {
        match build_change_state_information(state, transaction_uid) {
            Err(e) => (OperationResult::default(), Err(e)),
            Ok(info) => self.action(
                ctx,
                PULL_SOP_CLASS_UID,
                sop_instance_uid,
                ACTION_CHANGE_STATE,
                Some(&info),
            ),
        }
    }
    pub fn request_cancel(
        &self,
        ctx: Option<&Ctx>,
        abstract_syntax_uid: &str,
        sop_instance_uid: &str,
        information: Option<&Object>,
    ) -> (OperationResult, Result<()>) {
        if abstract_syntax_uid != PUSH_SOP_CLASS_UID && abstract_syntax_uid != WATCH_SOP_CLASS_UID {
            return (OperationResult::default(), Err(Kind::InvalidDataSet.into()));
        }
        self.action(
            ctx,
            abstract_syntax_uid,
            sop_instance_uid,
            ACTION_REQUEST_CANCEL,
            information,
        )
    }
    pub fn subscribe(
        &self,
        ctx: Option<&Ctx>,
        sop_instance_uid: &str,
        receiving_ae_title: &str,
        deletion_lock: bool,
    ) -> (OperationResult, Result<()>) {
        match build_subscription_information(
            receiving_ae_title,
            deletion_lock,
            &MatchingKeys::new(),
        ) {
            Err(e) => (OperationResult::default(), Err(e)),
            Ok(info) => self.action(
                ctx,
                WATCH_SOP_CLASS_UID,
                sop_instance_uid,
                ACTION_SUBSCRIBE,
                Some(&info),
            ),
        }
    }
    pub fn subscribe_filtered(
        &self,
        ctx: Option<&Ctx>,
        receiving_ae_title: &str,
        deletion_lock: bool,
        matching_keys: &MatchingKeys,
    ) -> (OperationResult, Result<()>) {
        if matching_keys.is_empty() {
            return (OperationResult::default(), Err(Kind::InvalidDataSet.into()));
        }
        match build_subscription_information(receiving_ae_title, deletion_lock, matching_keys) {
            Err(e) => (OperationResult::default(), Err(e)),
            Ok(info) => self.action(
                ctx,
                WATCH_SOP_CLASS_UID,
                FILTERED_GLOBAL_SUBSCRIPTION_SOP_INSTANCE_UID,
                ACTION_SUBSCRIBE,
                Some(&info),
            ),
        }
    }
    pub fn unsubscribe(
        &self,
        ctx: Option<&Ctx>,
        sop_instance_uid: &str,
        receiving_ae_title: &str,
    ) -> (OperationResult, Result<()>) {
        match build_unsubscription_information(receiving_ae_title) {
            Err(e) => (OperationResult::default(), Err(e)),
            Ok(info) => self.action(
                ctx,
                WATCH_SOP_CLASS_UID,
                sop_instance_uid,
                ACTION_UNSUBSCRIBE,
                Some(&info),
            ),
        }
    }
    pub fn suspend_global(
        &self,
        ctx: Option<&Ctx>,
        receiving_ae_title: &str,
    ) -> (OperationResult, Result<()>) {
        match build_unsubscription_information(receiving_ae_title) {
            Err(e) => (OperationResult::default(), Err(e)),
            Ok(info) => self.action(
                ctx,
                WATCH_SOP_CLASS_UID,
                GLOBAL_SUBSCRIPTION_SOP_INSTANCE_UID,
                ACTION_SUSPEND_GLOBAL,
                Some(&info),
            ),
        }
    }
    pub fn action(
        &self,
        ctx: Option<&Ctx>,
        abstract_syntax_uid: &str,
        sop_instance_uid: &str,
        action_type: u16,
        information: Option<&Object>,
    ) -> (OperationResult, Result<()>) {
        let (r, e) = self.normalized.action(
            &Self::options(ctx, abstract_syntax_uid),
            NormalizedRequest {
                sop_class_uid: PUSH_SOP_CLASS_UID.into(),
                sop_instance_uid: sop_instance_uid.into(),
                type_id: action_type,
                command_data_set_type: data_set_type(information),
                ..Default::default()
            },
            information,
        );
        (operation_result(r), e.map_err(Error::Dimse))
    }
}
