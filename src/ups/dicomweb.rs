use super::query::{
    MAX_QUERY_PAGE_SIZE, is_query_tag, project_query_result, query_entry, query_matches_step,
    valid_ups_timezone_offset,
};
use super::*;
use crate::dicomjson::{self, Dataset, jsonwire};
use crate::net::dicomweb::server::ServerError;
use crate::net::dicomweb::server::WorkitemEventHub;
use crate::net::dicomweb::server::events::WorkitemEvent;
use crate::net::dicomweb::server::qido::{DatasetYield, SearchRequest, SearchResult};
use crate::net::dicomweb::server::ups::{
    WorkitemBackend, WorkitemCancellationRequest, WorkitemCreateRequest, WorkitemCreated,
    WorkitemResult, WorkitemStateRequest, WorkitemSubscriptionBackend, WorkitemSubscriptionRequest,
    WorkitemUpdateRequest,
};
use crate::net::ul::Ctx;
use crate::object::{DataSet, Object, Tag, Vr};
use std::collections::HashMap;
use std::sync::Arc;
pub(crate) type DicomwebNewUid =
    Arc<dyn Fn() -> std::result::Result<String, crate::BoxError> + Send + Sync>;
#[derive(Clone, Default)]
pub struct DicomwebOptions {
    pub requesting_ae_title: String,
    pub new_uid: Option<DicomwebNewUid>,
    pub max_search_scanned: i64,
}
pub struct DicomwebBackend {
    service: Service,
    requesting_ae_title: String,
    new_uid: DicomwebNewUid,
    max_search_scanned: i64,
}
pub fn new_dicomweb_backend(
    service: Option<&Service>,
    options: DicomwebOptions,
) -> Result<DicomwebBackend> {
    let Some(service) = service else {
        return Err(Kind::Conflict.into());
    };
    let mut requesting = options.requesting_ae_title.trim().to_string();
    if requesting.is_empty() {
        requesting = "DICOMWEB".into();
    }
    if !valid_ae_title(&requesting) {
        return Err(Kind::InvalidDataSet.into());
    }
    let new_uid = options
        .new_uid
        .unwrap_or_else(|| Arc::new(|| Ok(crate::object::inherit::new_uid())));
    let mut max_search_scanned = options.max_search_scanned;
    if max_search_scanned <= 0 {
        max_search_scanned = 100_000;
    }
    if max_search_scanned > MAX_QUERY_SCANNED_STEPS {
        return Err(Kind::ResourceLimit.into());
    }
    Ok(DicomwebBackend {
        service: service.clone(),
        requesting_ae_title: requesting,
        new_uid,
        max_search_scanned,
    })
}
const DEFAULT_DICOMWEB_RETURN_KEYS: [Tag; 16] = [
    TAG_SOP_CLASS_UID,
    TAG_SOP_INSTANCE_UID,
    TAG_PATIENT_NAME,
    TAG_PATIENT_ID,
    TAG_PATIENT_BIRTH_DATE,
    TAG_PATIENT_SEX,
    TAG_STUDY_INSTANCE_UID,
    TAG_PROCEDURE_STEP_STATE,
    TAG_SCHEDULED_PROCEDURE_STEP_PRIORITY,
    TAG_PROCEDURE_STEP_LABEL,
    TAG_WORKLIST_LABEL,
    TAG_SCHEDULED_PROCEDURE_STEP_START_DATE_TIME,
    TAG_EXPECTED_COMPLETION_DATE_TIME,
    TAG_INPUT_READINESS_STATE,
    TAG_SCHEDULED_PROCEDURE_STEP_MODIFICATION_DATE_TIME,
    TAG_SCHEDULED_WORKITEM_CODE_SEQUENCE,
];
fn resolve_dicomweb_attribute(key: &str) -> Option<Tag> {
    if key.len() == 8
        && let Some(tag) = parse_tag(key)
    {
        return Some(tag);
    }
    crate::object::dict::by_keyword(key).map(|(tag, _)| tag)
}
fn dataset_to_dicomweb(
    ctx: &Ctx,
    attributes: DataSet,
) -> std::result::Result<Dataset, ServerError> {
    if ctx.err().is_some() {
        return Err(ServerError::Backend);
    }
    let object = Object::from_data_set(attributes);
    let data = dicomjson::marshal(Some(&object), &mut dicomjson::Options::defaults())
        .map_err(|_| ServerError::Backend)?;
    let mut decoder = jsonwire::Decoder::with_tail(&data, None, true);
    let value = decoder.decode_raw().map_err(|_| ServerError::Backend)?;
    let mut dataset = Dataset::new();
    for (key, member) in jsonwire::as_raw_map(value).map_err(|_| ServerError::Backend)? {
        let element = jsonwire::as_element(member).map_err(|_| ServerError::Backend)?;
        dataset.push((key, element));
    }
    Ok(dataset)
}
fn dicomweb_result<T>(outcome: Result<T>) -> std::result::Result<WorkitemResult, ServerError> {
    let err = match outcome {
        Ok(_) => return Ok(WorkitemResult::default()),
        Err(err) => err,
    };
    if err.is_canceled() {
        return Err(ServerError::Canceled);
    }
    if err.is_deadline() {
        return Err(ServerError::DeadlineExceeded);
    }
    let Some((_, status)) = err.status_error() else {
        if err.is(Kind::ResourceLimit) {
            return Err(ServerError::ResourceLimit);
        }
        if err.is(Kind::ConcurrentUpdate) || err.is(Kind::Conflict) {
            return Err(ServerError::Conflict);
        }
        return Err(ServerError::Backend);
    };
    let warning = |text: &str| {
        Ok(WorkitemResult {
            warnings: vec![text.to_string()],
        })
    };
    match status {
        STATUS_ALREADY_CANCELED => warning("The workitem is already in the CANCELED state."),
        STATUS_ALREADY_COMPLETED => warning("The workitem is already in the COMPLETED state."),
        STATUS_NO_SUCH_SOP_INSTANCE | STATUS_UPS_NOT_FOUND => Err(ServerError::NotFound),
        STATUS_DUPLICATE_SOP_INSTANCE
        | STATUS_MAY_NO_LONGER_BE_UPDATED
        | STATUS_INCORRECT_TRANSACTION_UID
        | STATUS_ALREADY_IN_PROGRESS
        | STATUS_FINAL_STATE_REQUIREMENTS_NOT_MET
        | STATUS_NOT_IN_PROGRESS
        | STATUS_ALREADY_COMPLETED_CANCEL_REQUEST
        | STATUS_PERFORMER_CANNOT_BE_CONTACTED
        | STATUS_PERFORMER_CHOOSES_NOT_TO_CANCEL => Err(ServerError::Conflict),
        STATUS_NOT_AUTHORIZED => Err(ServerError::Forbidden),
        STATUS_RESOURCE_LIMITATION => Err(ServerError::ResourceLimit),
        _ => Err(ServerError::InvalidRequest),
    }
}
struct SearchKeys {
    query: ParsedQuery,
}
impl SearchKeys {
    fn add(&mut self, tag: Tag, values: Vec<String>) -> std::result::Result<(), ServerError> {
        if !is_query_tag(tag) {
            return Err(ServerError::InvalidRequest);
        }
        match self.query.keys.get(&tag) {
            Some(existing) => {
                if !values.is_empty() && !existing.values.is_empty() {
                    return Err(ServerError::InvalidRequest);
                }
                if values.is_empty() {
                    return Ok(());
                }
            }
            None => self.query.order.push(tag),
        }
        self.query.keys.insert(
            tag,
            QueryKey {
                present: true,
                values,
            },
        );
        Ok(())
    }
}
impl DicomwebBackend {
    fn search(
        &self,
        ctx: &Ctx,
        request: &SearchRequest,
        yield_dataset: &mut DatasetYield<'_>,
    ) -> std::result::Result<SearchResult, ServerError> {
        let mut keys = SearchKeys {
            query: ParsedQuery {
                keys: HashMap::new(),
                order: Vec::new(),
            },
        };
        for filter in &request.filters {
            let Some(tag) = resolve_dicomweb_attribute(&filter.key) else {
                return Err(ServerError::InvalidRequest);
            };
            let sequence = query_entry(tag) == Some(Vr::SQ);
            let mut values: Vec<String> = Vec::new();
            for value in &filter.values {
                if sequence || value.is_empty() {
                    return Err(ServerError::InvalidRequest);
                }
                values.extend(value.split(',').map(str::to_string));
            }
            if tag == TAG_TIMEZONE_OFFSET_FROM_UTC
                && (values.len() != 1 || !valid_ups_timezone_offset(&values[0]))
            {
                return Err(ServerError::InvalidRequest);
            }
            keys.add(tag, values)?;
        }
        let all = request.include_fields.len() == 1 && request.include_fields[0] == "all";
        if !all {
            let mut return_keys: Vec<Tag> = DEFAULT_DICOMWEB_RETURN_KEYS.to_vec();
            for field in &request.include_fields {
                let Some(tag) = resolve_dicomweb_attribute(field) else {
                    return Err(ServerError::InvalidRequest);
                };
                return_keys.push(tag);
            }
            for tag in return_keys {
                keys.add(tag, Vec::new())?;
            }
        }
        let query = keys.query;
        let mut limit = request.maximum_results;
        if limit <= 0 {
            limit = 1;
        }
        let (mut matched, mut emitted) = (0i64, 0i64);
        let mut after = 0u64;
        let mut scanned = 0i64;
        loop {
            let page = StepQuery {
                after_sequence: after,
                limit: MAX_QUERY_PAGE_SIZE,
                states: Vec::new(),
            };
            let steps = match self.service.inner.store.list_steps(ctx, &page) {
                Ok(steps) => steps,
                Err(err) => {
                    return match dicomweb_result::<()>(Err(safe_repository_error(err))) {
                        Err(mapped) => Err(mapped),
                        Ok(_) => Err(ServerError::Backend),
                    };
                }
            };
            let count = steps.len() as i64;
            for step in steps {
                crate::net::dicomweb::server::ctx_err(ctx)?;
                scanned += 1;
                if scanned > self.max_search_scanned {
                    return Err(ServerError::ResourceLimit);
                }
                after = step.sequence;
                if !query_matches_step(&query, &step) {
                    continue;
                }
                matched += 1;
                if matched <= request.offset || emitted >= limit {
                    continue;
                }
                let dataset = if all {
                    let mut attributes = step.attributes;
                    remove_element(&mut attributes, TAG_TRANSACTION_UID);
                    dataset_to_dicomweb(ctx, attributes)?
                } else {
                    let (projected, _) = project_query_result(&query, &step);
                    dataset_to_dicomweb(ctx, projected.to_data_set())?
                };
                yield_dataset(dataset)?;
                emitted += 1;
            }
            if count < MAX_QUERY_PAGE_SIZE {
                break;
            }
        }
        let mut result = SearchResult::default();
        if emitted >= limit {
            result.remaining = Some((matched - request.offset - emitted).max(0));
        }
        Ok(result)
    }
}
impl WorkitemBackend for DicomwebBackend {
    fn create_workitem(
        &self,
        ctx: &Ctx,
        request: WorkitemCreateRequest,
    ) -> std::result::Result<WorkitemCreated, ServerError> {
        let mut uid = request.sop_instance_uid;
        if uid.is_empty() {
            uid = (self.new_uid)().map_err(|_| ServerError::Backend)?;
        }
        let mut attributes = request.workitem;
        if attributes.get(TAG_TRANSACTION_UID).is_none() {
            attributes.put(string_element(TAG_TRANSACTION_UID, Vr::UI, &[""]));
        }
        let created = self.service.create(
            ctx,
            &CreateRequest {
                sop_instance_uid: uid.clone(),
                attributes: Some(&attributes),
            },
        );
        let result = dicomweb_result(created)?;
        Ok(WorkitemCreated {
            result,
            sop_instance_uid: uid,
        })
    }
    fn retrieve_workitem(&self, ctx: &Ctx, uid: &str) -> std::result::Result<Dataset, ServerError> {
        let step = match self.service.get(ctx, uid) {
            Ok(step) => step,
            Err(err) => {
                return match dicomweb_result::<()>(Err(err)) {
                    Err(mapped) => Err(mapped),
                    Ok(_) => Ok(Dataset::new()),
                };
            }
        };
        let mut attributes = step.attributes;
        remove_element(&mut attributes, TAG_TRANSACTION_UID);
        dataset_to_dicomweb(ctx, attributes)
    }
    fn update_workitem(
        &self,
        ctx: &Ctx,
        request: WorkitemUpdateRequest,
    ) -> std::result::Result<WorkitemResult, ServerError> {
        dicomweb_result(self.service.set(
            ctx,
            &SetRequest {
                sop_instance_uid: request.sop_instance_uid,
                transaction_uid: request.transaction_uid,
                modifications: Some(&request.modifications),
            },
        ))
    }
    fn change_workitem_state(
        &self,
        ctx: &Ctx,
        request: WorkitemStateRequest,
    ) -> std::result::Result<WorkitemResult, ServerError> {
        dicomweb_result(self.service.change_state(
            ctx,
            &ChangeStateRequest {
                sop_instance_uid: request.sop_instance_uid,
                state: request.state.trim().to_string(),
                transaction_uid: request.transaction_uid,
            },
        ))
    }
    fn request_workitem_cancellation(
        &self,
        ctx: &Ctx,
        request: WorkitemCancellationRequest,
    ) -> std::result::Result<WorkitemResult, ServerError> {
        let mut requester = self.requesting_ae_title.clone();
        let mut information = request.information;
        if let Some(info) = information.as_mut()
            && let Some(element) = info.get(TAG_REQUESTING_AE)
        {
            let values = element_string_values(element);
            if values.len() != 1 || !valid_ae_title(values[0].trim()) {
                return Err(ServerError::InvalidRequest);
            }
            requester = values[0].trim().to_string();
            info.remove(TAG_REQUESTING_AE);
        }
        dicomweb_result(self.service.request_cancel(
            ctx,
            &CancelRequest {
                sop_instance_uid: request.sop_instance_uid,
                requesting_ae_title: requester,
                information: information.as_ref(),
            },
        ))
    }
    fn search_workitems(
        &self,
        ctx: &Ctx,
        request: &SearchRequest,
        yield_dataset: &mut DatasetYield<'_>,
    ) -> (SearchResult, std::result::Result<(), ServerError>) {
        match self.search(ctx, request, yield_dataset) {
            Ok(result) => (result, Ok(())),
            Err(err) => (SearchResult::default(), Err(err)),
        }
    }
    fn subscriptions(&self) -> Option<&dyn WorkitemSubscriptionBackend> {
        Some(self)
    }
}
impl WorkitemSubscriptionBackend for DicomwebBackend {
    fn subscribe_workitem(
        &self,
        ctx: &Ctx,
        request: WorkitemSubscriptionRequest,
    ) -> std::result::Result<WorkitemResult, ServerError> {
        let mut matching_keys = MatchingKeys::new();
        for filter in &request.filter {
            let Some(tag) = resolve_dicomweb_attribute(&filter.key) else {
                return Err(ServerError::InvalidRequest);
            };
            let key = format!("{:04X}{:04X}", tag.group, tag.element);
            if matching_keys.contains_key(&key) {
                return Err(ServerError::InvalidRequest);
            }
            matching_keys.insert(key, filter.values.clone());
        }
        let subscribed = self.service.subscribe(
            ctx,
            &SubscribeRequest {
                sop_instance_uid: request.sop_instance_uid,
                receiving_ae_title: request.ae_title,
                deletion_lock: request.deletion_lock,
                matching_keys,
            },
        );
        match subscribed {
            Err(err) => dicomweb_result::<()>(Err(err)),
            Ok(result) if result.status == STATUS_DELETION_LOCK_NOT_GRANTED => Ok(WorkitemResult {
                warnings: vec!["Deletion lock not granted.".to_string()],
            }),
            Ok(_) => Ok(WorkitemResult::default()),
        }
    }
    fn unsubscribe_workitem(
        &self,
        ctx: &Ctx,
        request: WorkitemSubscriptionRequest,
    ) -> std::result::Result<WorkitemResult, ServerError> {
        dicomweb_result(self.service.unsubscribe(
            ctx,
            &UnsubscribeRequest {
                sop_instance_uid: request.sop_instance_uid,
                receiving_ae_title: request.ae_title,
            },
        ))
    }
    fn suspend_workitem_subscription(
        &self,
        ctx: &Ctx,
        request: WorkitemSubscriptionRequest,
    ) -> std::result::Result<WorkitemResult, ServerError> {
        dicomweb_result(self.service.suspend_global(ctx, &request.ae_title))
    }
}
struct DicomwebEventSink {
    hub: Option<Arc<WorkitemEventHub>>,
}
pub fn new_dicomweb_event_sink(hub: Option<Arc<WorkitemEventHub>>) -> Arc<dyn EventSink> {
    Arc::new(DicomwebEventSink { hub })
}
impl EventSink for DicomwebEventSink {
    fn deliver_ups_event(&self, ctx: &Ctx, delivery: &Delivery) -> (bool, Result<()>) {
        let Some(hub) = &self.hub else {
            return (false, Ok(()));
        };
        if hub.connections(&delivery.receiving_ae_title) == 0 {
            return (false, Ok(()));
        }
        let information = match dataset_to_dicomweb(ctx, delivery.event.information.clone()) {
            Ok(information) => information,
            Err(err) => return (true, Err(Error::Other(err.to_string()))),
        };
        let event = WorkitemEvent {
            sop_instance_uid: delivery.sop_instance_uid.clone(),
            event_type_id: delivery.event_type,
            information,
        };
        match hub.publish(ctx, &delivery.receiving_ae_title, &event) {
            Ok(sent) if sent > 0 => (true, Ok(())),
            Ok(_) => (false, Ok(())),
            Err(err) => (true, Err(Error::Other(err.to_string()))),
        }
    }
}
