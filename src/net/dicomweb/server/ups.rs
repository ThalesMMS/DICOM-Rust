use super::super::mimeheader::parse_media_type;
use super::super::types::valid_ae_title;
use super::super::urlrules;
use super::http::{Request, ResponseWriter};
use super::media::{
    DICOM_JSON, MetadataRepresentation, best_matching_media_preference,
    parse_metadata_accept_header, select_metadata_representation,
};
use super::qido::{
    DatasetYield, SearchBackend, SearchFilter, SearchLevel, SearchRequest, SearchResult,
    valid_attribute_identifier,
};
use super::stow::MaxBytesReader;
use super::{
    Inner, Operation, ServerError, ServerWriter, classify_backend_error, classify_multipart_error,
    request_accept_header,
};
use crate::dicomjson::{self, Dataset};
use crate::net::ul::Ctx;
use crate::object::{Object, Tag, Value};
use std::io::Read;
pub const GLOBAL_WORKITEM_SUBSCRIPTION_UID: &str = "1.2.840.10008.5.1.4.34.5";
pub const FILTERED_GLOBAL_WORKITEM_SUBSCRIPTION_UID: &str = "1.2.840.10008.5.1.4.34.5.1";
pub const UPS_EVENT_SOP_CLASS_UID: &str = "1.2.840.10008.5.1.4.34.6.4";
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WorkitemResult {
    pub warnings: Vec<String>,
}
pub struct WorkitemCreateRequest {
    pub sop_instance_uid: String,
    pub workitem: Object,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WorkitemCreated {
    pub result: WorkitemResult,
    pub sop_instance_uid: String,
}
pub struct WorkitemUpdateRequest {
    pub sop_instance_uid: String,
    pub transaction_uid: String,
    pub modifications: Object,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkitemStateRequest {
    pub sop_instance_uid: String,
    pub state: String,
    pub transaction_uid: String,
}
pub struct WorkitemCancellationRequest {
    pub sop_instance_uid: String,
    pub information: Option<Object>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkitemSubscriptionRequest {
    pub sop_instance_uid: String,
    pub ae_title: String,
    pub deletion_lock: bool,
    pub filter: Vec<SearchFilter>,
}
type WResult<T> = Result<T, ServerError>;
pub trait WorkitemBackend: Send + Sync {
    fn create_workitem(
        &self,
        ctx: &Ctx,
        request: WorkitemCreateRequest,
    ) -> WResult<WorkitemCreated>;
    fn retrieve_workitem(&self, ctx: &Ctx, uid: &str) -> WResult<Dataset>;
    fn update_workitem(&self, ctx: &Ctx, request: WorkitemUpdateRequest)
    -> WResult<WorkitemResult>;
    fn change_workitem_state(
        &self,
        ctx: &Ctx,
        request: WorkitemStateRequest,
    ) -> WResult<WorkitemResult>;
    fn request_workitem_cancellation(
        &self,
        ctx: &Ctx,
        request: WorkitemCancellationRequest,
    ) -> WResult<WorkitemResult>;
    fn search_workitems(
        &self,
        ctx: &Ctx,
        request: &SearchRequest,
        yield_dataset: &mut DatasetYield<'_>,
    ) -> (SearchResult, Result<(), ServerError>);
    fn subscriptions(&self) -> Option<&dyn WorkitemSubscriptionBackend> {
        None
    }
}
pub trait WorkitemSubscriptionBackend: Send + Sync {
    fn subscribe_workitem(
        &self,
        ctx: &Ctx,
        request: WorkitemSubscriptionRequest,
    ) -> WResult<WorkitemResult>;
    fn unsubscribe_workitem(
        &self,
        ctx: &Ctx,
        request: WorkitemSubscriptionRequest,
    ) -> WResult<WorkitemResult>;
    fn suspend_workitem_subscription(
        &self,
        ctx: &Ctx,
        request: WorkitemSubscriptionRequest,
    ) -> WResult<WorkitemResult>;
}
type Served = Result<usize, (usize, ServerError)>;
fn fail(err: ServerError) -> Served {
    Err((0, err))
}
fn safe_call<T>(call: impl FnOnce() -> WResult<T>) -> WResult<T> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(call))
        .unwrap_or(Err(ServerError::Backend))
}
fn classify_workitem_error(err: ServerError) -> ServerError {
    if err.is(&ServerError::Forbidden) {
        return ServerError::status(403, "forbidden", ServerError::Forbidden);
    }
    if err.is(&ServerError::ResourceLimit) {
        if matches!(err, ServerError::Status { .. }) {
            return err;
        }
        return ServerError::status(503, "resource_limit", ServerError::ResourceLimit);
    }
    classify_backend_error(err)
}
fn safe_warning_text(text: &str) -> String {
    let mut b = String::new();
    for c in text.trim_matches(char::is_whitespace).chars() {
        if (0x20..0x7f).contains(&(c as u32)) && c != '"' {
            b.push(c);
        }
        if b.len() >= 200 {
            break;
        }
    }
    b
}
fn unsupported_media() -> ServerError {
    ServerError::status(415, "unsupported_media_type", ServerError::Unsupported)
}
fn require_dicom_json_response(r: &Request) -> Result<(), ServerError> {
    let accept = request_accept_header(r);
    let accept = accept.trim();
    if accept.is_empty() {
        return Ok(());
    }
    if select_metadata_representation(accept) == Some(MetadataRepresentation::Json) {
        return Ok(());
    }
    let prefs = parse_metadata_accept_header(accept);
    if matches!(best_matching_media_preference(&prefs, DICOM_JSON, "1.2.840.10008.1.2.1", false), Some(p) if p.quality > 0.0)
    {
        return Ok(());
    }
    Err(ServerError::status(
        406,
        "not_acceptable",
        ServerError::Unsupported,
    ))
}
fn workitem_subscription_filters(raw_query: &str) -> Result<Vec<SearchFilter>, ServerError> {
    let mut filters = Vec::new();
    for field in raw_query.split('&') {
        let (raw_key, list) = field.split_once('=').unwrap_or((field, ""));
        let key = urlrules::query_unescape(raw_key).map_err(|_| ServerError::InvalidRequest)?;
        if key != "filter" {
            continue;
        }
        for raw_pair in list.split(',') {
            let pair =
                urlrules::query_unescape(raw_pair).map_err(|_| ServerError::InvalidRequest)?;
            match pair.split_once('=') {
                Some((k, v)) if valid_attribute_identifier(k) && !v.is_empty() => {
                    filters.push(SearchFilter {
                        key: k.to_string(),
                        values: vec![v.to_string()],
                    })
                }
                _ => return Err(ServerError::InvalidRequest),
            }
        }
    }
    Ok(filters)
}
fn single_string_value(obj: &Object, tag: Tag) -> Option<String> {
    obj.get(tag)?;
    let values = obj.get_strings(tag).unwrap_or_default();
    if values.len() != 1 {
        return None;
    }
    let value = values[0].trim_end_matches(['\0', ' ']).to_string();
    (!value.trim().is_empty()).then_some(value)
}
struct WorkitemSearch<'a>(&'a dyn WorkitemBackend);
impl SearchBackend for WorkitemSearch<'_> {
    fn search(
        &self,
        ctx: &Ctx,
        request: &SearchRequest,
        yield_dataset: &mut DatasetYield<'_>,
    ) -> (SearchResult, Result<(), ServerError>) {
        self.0.search_workitems(ctx, request, yield_dataset)
    }
}
impl Inner {
    fn workitem_backend(&self) -> Option<&dyn WorkitemBackend> {
        if let Some(w) = &self.options.workitems {
            return Some(w.as_ref());
        }
        self.options
            .backend
            .as_ref()
            .and_then(|b| b.workitem_backend())
    }
    fn workitem_query(
        &self,
        r: &Request,
        allowed: &[&str],
    ) -> Result<urlrules::Values, ServerError> {
        let (query, err) = urlrules::parse_query(&r.url.raw_query);
        if err.is_some() {
            return Err(ServerError::InvalidRequest);
        }
        let mut fields = 0;
        let mut keys: Vec<&(String, Vec<String>)> = query.0.iter().collect();
        keys.sort_by(|a, b| a.0.cmp(&b.0));
        for (key, values) in keys {
            fields += values.len();
            if !allowed.contains(&key.as_str()) {
                return Err(ServerError::InvalidRequest);
            }
            if values
                .iter()
                .any(|v| v.len() as i64 > self.limits.max_query_value_bytes)
            {
                return Err(ServerError::ResourceLimit);
            }
        }
        if fields as i64 > self.limits.max_query_fields {
            return Err(ServerError::ResourceLimit);
        }
        Ok(query)
    }
    fn workitem_requester(&self, r: &Request) -> Result<(), ServerError> {
        let query = self.workitem_query(r, &["requester"])?;
        if query.has("requester") {
            let values = query.values("requester");
            if values.len() != 1 || !valid_ae_title(&values[0]) {
                return Err(ServerError::InvalidRequest);
            }
        }
        Ok(())
    }
    fn create_workitem_uid(
        &self,
        r: &Request,
        workitem: &mut Object,
    ) -> Result<String, ServerError> {
        let (query, err) = urlrules::parse_query(&r.url.raw_query);
        if err.is_some() {
            return Err(ServerError::InvalidRequest);
        }
        let mut uid = String::new();
        let mut keys: Vec<&(String, Vec<String>)> = query.0.iter().collect();
        keys.sort_by(|a, b| a.0.cmp(&b.0));
        for (key, values) in keys {
            if key == "workitem" || key == "AffectedSOPInstanceUID" {
                if values.len() != 1 || (!uid.is_empty() && uid != values[0]) {
                    return Err(ServerError::InvalidRequest);
                }
                uid = values[0].clone();
            } else if values.len() == 1 && values[0].is_empty() && self.validate_uid(key).is_ok() {
                if !uid.is_empty() && uid != *key {
                    return Err(ServerError::InvalidRequest);
                }
                uid = key.clone();
            } else {
                return Err(ServerError::InvalidRequest);
            }
        }
        let sop = Tag::new(0x0008, 0x0018);
        if workitem.get(sop).is_some() {
            let values = workitem.get_strings(sop).unwrap_or_default();
            if values.len() != 1 {
                return Err(ServerError::InvalidRequest);
            }
            let dataset_uid = values[0].trim_end_matches(['\0', ' ']).to_string();
            if !uid.is_empty() && dataset_uid != uid {
                return Err(ServerError::Conflict);
            }
            uid = dataset_uid;
            workitem.remove(sop);
        }
        if !uid.is_empty() {
            self.validate_uid(&uid)?;
        }
        Ok(uid)
    }
    fn decode_workitem_body(
        &self,
        r: &mut Request,
        required: bool,
    ) -> Result<Option<Object>, ServerError> {
        if !r.header.values("Content-Encoding").is_empty()
            || !r.header.values("Content-Transfer-Encoding").is_empty()
        {
            return Err(unsupported_media());
        }
        let body = std::mem::replace(&mut r.body, Box::new(std::io::empty()));
        let mut limited = MaxBytesReader::new(body, self.limits.max_metadata_bytes);
        let mut data = Vec::new();
        if let Err(e) = limited.read_to_end(&mut data) {
            return Err(classify_multipart_error(super::spool::io_server_error(&e)));
        }
        let trimmed = trim_space_bytes(&data);
        if trimmed.is_empty() {
            if required {
                return Err(ServerError::InvalidRequest);
            }
            return Ok(None);
        }
        let mt = parse_media_type(r.header.get("Content-Type")).map_err(|_| unsupported_media())?;
        if !mt.media_type.eq_ignore_ascii_case(DICOM_JSON) {
            return Err(unsupported_media());
        }
        for (key, value) in &mt.params {
            if key != "charset" || !value.eq_ignore_ascii_case("utf-8") {
                return Err(unsupported_media());
            }
        }
        let mut raw = trimmed;
        if raw[0] == b'[' {
            let items = decode_raw_array(raw).ok_or(ServerError::InvalidRequest)?;
            if items.len() != 1 {
                return Err(ServerError::InvalidRequest);
            }
            raw = items[0];
        }
        let l = &self.limits;
        let opts = dicomjson::UnmarshalOptions {
            transfer_syntax: Some(crate::object::transfer::EXPLICIT_VR_LITTLE_ENDIAN),
            limits: dicomjson::Limits {
                max_json_bytes: raw.len() as i64,
                max_inline_binary_bytes: l.max_metadata_bytes,
                max_total_binary_bytes: l.max_metadata_bytes,
                max_sequence_depth: l.max_json_depth,
                max_elements: l.max_json_values,
                max_sequence_items: l.max_json_values,
            },
            ..dicomjson::UnmarshalOptions::default()
        };
        let workitem = dicomjson::unmarshal_text(raw, &opts)
            .map_err(|e| classify_store_metadata_decode_error(&e))?;
        let mut bad = false;
        let walked = workitem.walk_with(
            crate::object::WalkOptions {
                max_depth: l.max_json_depth,
                max_elements: l.max_json_values,
            },
            |_, element| {
                if element.tag().group == 0x0002 || matches!(element.value, Value::BulkData(_)) {
                    bad = true;
                    return Err(crate::object::error::Error::new_kind(
                        crate::object::error::Kind::Other,
                        "workitem element refused",
                    ));
                }
                Ok(())
            },
        );
        if let Err(e) = walked {
            if !bad && e.is(crate::object::error::Kind::WalkResourceLimit) {
                return Err(ServerError::ResourceLimit);
            }
            return Err(ServerError::InvalidRequest);
        }
        Ok(Some(workitem))
    }
    fn write_workitem_result(
        &self,
        w: &mut ServerWriter<'_>,
        r: &Request,
        status: u16,
        result: &WorkitemResult,
    ) {
        for warning in &result.warnings {
            let text = safe_warning_text(warning);
            if !text.is_empty() {
                let uri = self.service_uri(r);
                w.header().add("Warning", &format!("299 {uri}: {text}"));
            }
        }
        w.write_header(status);
    }
    fn finish_transaction(
        &self,
        w: &mut ServerWriter<'_>,
        r: &Request,
        status: u16,
        result: WResult<WorkitemResult>,
    ) -> Served {
        match result {
            Err(e) => fail(classify_workitem_error(e)),
            Ok(result) => {
                self.write_workitem_result(w, r, status, &result);
                Ok(1)
            }
        }
    }
    fn write_workitem_datasets(
        &self,
        w: &mut ServerWriter<'_>,
        r: &Request,
        status: u16,
        datasets: &[Dataset],
    ) -> Result<(), ServerError> {
        let mut body = vec![b'['];
        for (index, dataset) in datasets.iter().enumerate() {
            let data = self.write_response_dataset(r.context(), dataset)?;
            if index > 0 {
                body.push(b',');
            }
            body.extend_from_slice(&data);
        }
        body.push(b']');
        if body.len() as i64 > self.limits.max_response_bytes {
            return Err(ServerError::ResourceLimit);
        }
        w.header().set("Content-Type", DICOM_JSON);
        w.header().set("X-Content-Type-Options", "nosniff");
        w.write_header(status);
        w.write(&body)?;
        Ok(())
    }
    fn serve_event_channel(&self, w: &mut ServerWriter<'_>, r: &Request, ae_title: &str) -> Served {
        let Some(hub) = &self.options.workitem_events else {
            return fail(ServerError::status(
                501,
                "not_implemented",
                ServerError::Unsupported,
            ));
        };
        if !valid_ae_title(ae_title) {
            return fail(ServerError::InvalidRequest);
        }
        match hub.accept(w, r, ae_title) {
            Ok(()) => {
                w.status = 101;
                w.written = true;
                Ok(1)
            }
            Err(None) => Ok(0),
            Err(Some(e)) => fail(e),
        }
    }
    pub(crate) fn serve_workitems(
        &self,
        w: &mut ServerWriter<'_>,
        r: &mut Request,
        operation: Operation,
    ) -> Served {
        let parts = super::request_resource_parts(&r.url);
        if operation == Operation::OpenEventChannel {
            return self.serve_event_channel(w, r, parts.get(1).map_or("", String::as_str));
        }
        let Some(backend) = self.workitem_backend() else {
            return fail(ServerError::status(
                501,
                "not_implemented",
                ServerError::Unsupported,
            ));
        };
        let mut uid = String::new();
        if parts.len() >= 2 {
            uid = parts[1].clone();
            self.validate_uid(&uid).map_err(|e| (0, e))?;
        }
        let ctx = r.context().clone();
        match operation {
            Operation::SearchWorkitems => {
                require_dicom_json_response(r).map_err(|e| (0, e))?;
                let mut request = self
                    .parse_search_request(r, operation)
                    .map_err(|e| (0, e))?;
                request.level = SearchLevel::Workitem;
                self.run_search(
                    w,
                    r,
                    &WorkitemSearch(backend),
                    request,
                    MetadataRepresentation::Json,
                )
            }
            Operation::RetrieveWorkitem => {
                require_dicom_json_response(r).map_err(|e| (0, e))?;
                let dataset = safe_call(|| backend.retrieve_workitem(&ctx, &uid))
                    .map_err(|e| (0, classify_workitem_error(e)))?;
                self.write_workitem_datasets(w, r, 200, &[dataset])
                    .map_err(|e| (1, e))?;
                Ok(1)
            }
            Operation::CreateWorkitem => {
                let Some(mut workitem) = self.decode_workitem_body(r, true).map_err(|e| (0, e))?
                else {
                    return fail(ServerError::InvalidRequest);
                };
                let sop_instance_uid = self
                    .create_workitem_uid(r, &mut workitem)
                    .map_err(|e| (0, e))?;
                let created = safe_call(|| {
                    backend.create_workitem(
                        &ctx,
                        WorkitemCreateRequest {
                            sop_instance_uid,
                            workitem,
                        },
                    )
                })
                .map_err(|e| (0, classify_workitem_error(e)))?;
                if self.validate_uid(&created.sop_instance_uid).is_err() {
                    return fail(ServerError::Backend);
                }
                let location = self.service_path(&format!(
                    "/workitems/{}",
                    urlrules::path_escape(&created.sop_instance_uid)
                ));
                w.header().set("Location", &location);
                w.header().set("Content-Location", &location);
                self.write_workitem_result(w, r, 201, &created.result);
                Ok(1)
            }
            Operation::UpdateWorkitem => {
                let query = self
                    .workitem_query(r, &["transaction"])
                    .map_err(|e| (0, e))?;
                let transaction = query.get("transaction").to_string();
                if !transaction.is_empty() {
                    self.validate_uid(&transaction).map_err(|e| (0, e))?;
                }
                let Some(modifications) = self.decode_workitem_body(r, true).map_err(|e| (0, e))?
                else {
                    return fail(ServerError::InvalidRequest);
                };
                let result = safe_call(|| {
                    backend.update_workitem(
                        &ctx,
                        WorkitemUpdateRequest {
                            sop_instance_uid: uid.clone(),
                            transaction_uid: transaction,
                            modifications,
                        },
                    )
                });
                self.finish_transaction(w, r, 200, result)
            }
            Operation::ChangeWorkitemState => {
                self.workitem_requester(r).map_err(|e| (0, e))?;
                let Some(body) = self.decode_workitem_body(r, true).map_err(|e| (0, e))? else {
                    return fail(ServerError::InvalidRequest);
                };
                let state = single_string_value(&body, Tag::new(0x0074, 0x1000));
                let transaction = single_string_value(&body, Tag::new(0x0008, 0x1195));
                let (Some(state), Some(transaction)) = (state, transaction) else {
                    return fail(ServerError::InvalidRequest);
                };
                if body.len() != 2 {
                    return fail(ServerError::InvalidRequest);
                }
                let result = safe_call(|| {
                    backend.change_workitem_state(
                        &ctx,
                        WorkitemStateRequest {
                            sop_instance_uid: uid.clone(),
                            state,
                            transaction_uid: transaction,
                        },
                    )
                });
                self.finish_transaction(w, r, 200, result)
            }
            Operation::RequestWorkitemCancellation => {
                self.workitem_requester(r).map_err(|e| (0, e))?;
                let information = self.decode_workitem_body(r, false).map_err(|e| (0, e))?;
                let result = safe_call(|| {
                    backend.request_workitem_cancellation(
                        &ctx,
                        WorkitemCancellationRequest {
                            sop_instance_uid: uid.clone(),
                            information,
                        },
                    )
                });
                self.finish_transaction(w, r, 202, result)
            }
            _ => self.serve_workitem_subscription(w, r, backend, operation, &uid, &parts),
        }
    }
    fn serve_workitem_subscription(
        &self,
        w: &mut ServerWriter<'_>,
        r: &mut Request,
        backend: &dyn WorkitemBackend,
        operation: Operation,
        uid: &str,
        parts: &[String],
    ) -> Served {
        let Some(subscriptions) = backend.subscriptions() else {
            return fail(ServerError::status(
                501,
                "not_implemented",
                ServerError::Unsupported,
            ));
        };
        let ae_title = parts[3].clone();
        if !valid_ae_title(&ae_title) {
            return fail(ServerError::InvalidRequest);
        }
        let mut request = WorkitemSubscriptionRequest {
            sop_instance_uid: uid.to_string(),
            ae_title,
            deletion_lock: false,
            filter: Vec::new(),
        };
        let ctx = r.context().clone();
        let (result, status) = match operation {
            Operation::SubscribeWorkitem => {
                let query = self
                    .workitem_query(r, &["deletionlock", "filter"])
                    .map_err(|e| (0, e))?;
                if query.has("deletionlock") {
                    let values = query.values("deletionlock");
                    if values.len() != 1 {
                        return fail(ServerError::InvalidRequest);
                    }
                    request.deletion_lock = match values[0].as_str() {
                        "true" => true,
                        "false" => false,
                        _ => return fail(ServerError::InvalidRequest),
                    };
                }
                request.filter =
                    workitem_subscription_filters(&r.url.raw_query).map_err(|e| (0, e))?;
                if !request.filter.is_empty() && uid != FILTERED_GLOBAL_WORKITEM_SUBSCRIPTION_UID {
                    return fail(ServerError::InvalidRequest);
                }
                (
                    safe_call(|| subscriptions.subscribe_workitem(&ctx, request)),
                    201,
                )
            }
            Operation::UnsubscribeWorkitem => {
                self.workitem_query(r, &[]).map_err(|e| (0, e))?;
                (
                    safe_call(|| subscriptions.unsubscribe_workitem(&ctx, request)),
                    200,
                )
            }
            Operation::SuspendWorkitemSubscription => {
                self.workitem_query(r, &[]).map_err(|e| (0, e))?;
                if uid != GLOBAL_WORKITEM_SUBSCRIPTION_UID
                    && uid != FILTERED_GLOBAL_WORKITEM_SUBSCRIPTION_UID
                {
                    return fail(ServerError::InvalidRequest);
                }
                (
                    safe_call(|| subscriptions.suspend_workitem_subscription(&ctx, request)),
                    200,
                )
            }
            _ => return fail(ServerError::NotFound),
        };
        self.finish_transaction(w, r, status, result)
    }
}
fn classify_store_metadata_decode_error(text: &str) -> ServerError {
    const LIMITS: [&str; 13] = [
        dicomjson::ERR_MAX_JSON_BYTES,
        dicomjson::ERR_MAX_INLINE_BINARY_BYTES,
        dicomjson::ERR_MAX_TOTAL_BINARY_BYTES,
        dicomjson::ERR_MAX_SEQUENCE_DEPTH,
        dicomjson::ERR_MAX_ELEMENTS,
        dicomjson::ERR_MAX_SEQUENCE_ITEMS,
        crate::dicomxml::ERR_MAX_XML_BYTES,
        crate::dicomxml::ERR_MAX_VALUE_BYTES,
        crate::dicomxml::ERR_MAX_INLINE_BYTES,
        crate::dicomxml::ERR_MAX_BINARY_BYTES,
        crate::dicomxml::ERR_MAX_DEPTH,
        crate::dicomxml::ERR_MAX_ELEMENTS,
        crate::dicomxml::ERR_MAX_ITEMS,
    ];
    if LIMITS.iter().any(|l| text.contains(l)) {
        ServerError::ResourceLimit
    } else {
        ServerError::InvalidRequest
    }
}
fn trim_space_bytes(data: &[u8]) -> &[u8] {
    let start = data
        .iter()
        .position(|c| !c.is_ascii_whitespace() && *c != 0x0b)
        .unwrap_or(data.len());
    let end = data
        .iter()
        .rposition(|c| !c.is_ascii_whitespace() && *c != 0x0b)
        .map_or(start, |i| i + 1);
    &data[start..end]
}
fn decode_raw_array(raw: &[u8]) -> Option<Vec<&[u8]>> {
    use crate::dicomjson::jsonwire;
    let mut decoder = jsonwire::Decoder::new(raw);
    let value = decoder.decode_raw().ok()?;
    jsonwire::ensure_eof(&mut decoder).ok()?;
    jsonwire::as_raw_array(value).ok()
}
