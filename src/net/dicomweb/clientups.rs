use super::client::{
    ACCEPT_DICOM_JSON, Client, ClientError, RequestOptions, context_request_error, decode_error,
    retry_after, sleep_context, status_error, web,
};
use super::mimeheader::trim_space;
use super::multipart::{ByteRead, ReadErr, SliceReader};
use super::response::{
    Response, datasets_from_dicom_json, parse_response_media_type, response_content_type,
};
use super::types::{
    Cause, Endpoint, Error, ErrorKind, ResponseDecodeError, ResponseDecodeErrorKind, valid_ae_title,
};
use super::urlrules::{self, Url, Values};
use crate::dicomjson::jsonwire::{self, JAny, RawElement};
use crate::dicomjson::{self, Dataset};
use crate::net::ul::{Ctx, Kind};
use crate::object::{Object, Tag, Vr, is_valid_uid};
const DICOM_JSON: &str = "application/dicom+json";
pub const GLOBAL_WORKITEM_SUBSCRIPTION_UID: &str = "1.2.840.10008.5.1.4.34.5";
fn invalid_endpoint(text: &str) -> Error {
    Error::new(ErrorKind::InvalidEndpoint, "", 0, Some(Cause::text(text)))
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WorkitemResponse {
    pub response: Response,
    pub warnings: Option<Vec<String>>,
    pub location: String,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WorkitemSubscribeOptions {
    pub deletion_lock: bool,
    pub filter: Vec<super::server::SearchFilter>,
}
fn append_path_segment(u: &mut Url, segment: &str) {
    let escaped = format!(
        "{}/{}",
        u.escaped_path()
            .strip_suffix('/')
            .unwrap_or(&u.escaped_path()),
        urlrules::path_escape(segment)
    );
    u.path = format!("{}/{segment}", u.path.strip_suffix('/').unwrap_or(&u.path));
    u.raw_path = escaped;
}
impl Endpoint {
    pub fn workitem_search_url(&self, params: Option<&Values>) -> Result<Url, Error> {
        self.service_url(&self.ups_path, &["workitems"], params)
    }
    pub fn workitem_url(
        &self,
        uid: &str,
        params: Option<&Values>,
        segments: &[&str],
    ) -> Result<Url, Error> {
        let uid = trim_space(uid);
        if uid.is_empty() {
            return Err(invalid_endpoint("workitem UID is required"));
        }
        let mut parts = vec!["workitems", uid];
        parts.extend_from_slice(segments);
        self.service_url(&self.ups_path, &parts, params)
    }
    pub fn event_channel_url(&self, ae_title: &str) -> Result<Url, Error> {
        if !valid_ae_title(ae_title) {
            return Err(invalid_endpoint("invalid AE Title"));
        }
        let mut u = self.service_url(&self.ups_path, &["subscribers"], None)?;
        append_path_segment(&mut u, ae_title);
        Ok(u)
    }
    pub fn subscribed_event_channel_urls(
        &self,
        ae_title: &str,
        response: &WorkitemResponse,
    ) -> Result<Vec<Url>, Error> {
        let standard = self.event_channel_url(ae_title)?;
        let mut out = vec![standard.clone()];
        if let Some(announced) = self.announced_event_channel(ae_title, &standard, response)
            && announced.string() != standard.string()
        {
            out.push(announced);
        }
        Ok(out)
    }
    fn announced_event_channel(
        &self,
        ae_title: &str,
        standard: &Url,
        response: &WorkitemResponse,
    ) -> Option<Url> {
        let location = trim_space(&response.location);
        if location.is_empty() {
            return None;
        }
        let reference = urlrules::parse(location).ok()?;
        if reference.user.is_some() || !reference.opaque.is_empty() {
            return None;
        }
        let web_socket = match reference.scheme.to_ascii_lowercase().as_str() {
            "ws" | "wss" => true,
            "" | "http" | "https" => false,
            _ => return None,
        };
        let mut base = standard.clone();
        if let Ok(request) = urlrules::parse(&response.response.url)
            && !request.scheme.is_empty()
            && super::http::same_origin(standard, &request)
        {
            base = request;
        }
        let target = self
            .event_channel_target(&base.resolve_reference(&reference))
            .ok()?;
        if !web_socket && !event_channel_path(&target, ae_title) {
            return None;
        }
        Some(target)
    }
    pub(super) fn event_channel_target(&self, target: &Url) -> Result<Url, Error> {
        let mut resolved = target.clone();
        resolved.scheme = match resolved.scheme.to_ascii_lowercase().as_str() {
            "ws" | "http" => "http".into(),
            "wss" | "https" => "https".into(),
            _ => {
                return Err(invalid_endpoint(
                    "event channel URL must use ws, wss, http or https",
                ));
            }
        };
        resolved.fragment.clear();
        resolved.raw_fragment.clear();
        let origin = self.service_url("", &[], None)?;
        if resolved.host.is_empty() || !super::http::same_origin(&origin, &resolved) {
            return Err(invalid_endpoint(
                "event channel URL must use the endpoint origin",
            ));
        }
        Ok(resolved)
    }
}
fn event_channel_path(u: &Url, ae_title: &str) -> bool {
    let escaped = u.escaped_path();
    let segments: Vec<&str> = escaped.split('/').collect();
    let n = segments.len();
    if n < 2 || segments[n - 2] != "subscribers" {
        return false;
    }
    match urlrules::path_unescape(segments[n - 1]) {
        Ok(last) if trim_space(&last) == trim_space(ae_title) => {}
        _ => return false,
    }
    !segments[..n - 2].contains(&"workitems")
}
fn warning_texts(values: &[String]) -> Option<Vec<String>> {
    let mut out: Option<Vec<String>> = None;
    for value in values {
        let mut value = trim_space(value).to_string();
        if let Some((code, rest)) = value.clone().split_once(' ')
            && code.parse::<i64>().is_ok()
        {
            value = rest.to_string();
            if let Some((agent, text)) = value.clone().split_once(": ")
                && !agent.contains([' ', '"'])
            {
                value = text.to_string();
            }
        }
        out.get_or_insert_with(Vec::new)
            .push(value.trim_matches('"').to_string());
    }
    out
}
fn encode_dataset(out: &mut String, dataset: &[(String, RawElement)]) {
    let mut entries: Vec<&(String, RawElement)> = dataset.iter().collect();
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    out.push('{');
    for (i, (tag, element)) in entries.into_iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        jsonwire::append_string(out, tag);
        out.push_str(":{\"vr\":");
        jsonwire::append_string(out, &element.vr);
        if !element.value.is_empty() {
            out.push_str(",\"Value\":[");
            for (j, v) in element.value.iter().enumerate() {
                if j > 0 {
                    out.push(',');
                }
                encode_any(out, v);
            }
            out.push(']');
        }
        if !element.inline_binary.is_empty() {
            out.push_str(",\"InlineBinary\":");
            jsonwire::append_string(out, &element.inline_binary);
        }
        if !element.bulk_data_uri.is_empty() {
            out.push_str(",\"BulkDataURI\":");
            jsonwire::append_string(out, &element.bulk_data_uri);
        }
        out.push('}');
    }
    out.push('}');
}
fn encode_any(out: &mut String, v: &JAny) {
    match v {
        JAny::Null => out.push_str("null"),
        JAny::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        JAny::Number(n) => out.push_str(n),
        JAny::Str(s) => jsonwire::append_string(out, s),
        JAny::Arr(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                encode_any(out, item);
            }
            out.push(']');
        }
        JAny::Obj(entries) => {
            out.push('{');
            for (i, (k, item)) in entries.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                jsonwire::append_string(out, k);
                out.push(':');
                encode_any(out, item);
            }
            out.push('}');
        }
    }
}
fn element(vr: &str, values: Vec<JAny>) -> RawElement {
    RawElement {
        vr: vr.into(),
        value: values,
        ..RawElement::default()
    }
}
fn ctx_read_err(ctx: &Ctx) -> Option<ReadErr> {
    ctx.err().map(|k| {
        if matches!(k, Kind::Canceled) {
            ReadErr::Canceled
        } else {
            ReadErr::Deadline
        }
    })
}
impl Client {
    #[allow(clippy::result_large_err, clippy::too_many_arguments)]
    fn do_request_bytes(
        &self,
        ctx: &Ctx,
        method: &str,
        url: &Url,
        body: Option<&[u8]>,
        content_type: &str,
        accept: &str,
        options: &RequestOptions,
    ) -> Result<Response, (Response, ClientError)> {
        let mut reader = body.map(SliceReader::new);
        let http = self
            .do_http_opts(
                ctx,
                method,
                url,
                reader.as_mut().map(|r| r as &mut dyn ByteRead),
                content_type,
                accept,
                options,
            )
            .map_err(|e| (Response::default(), e))?;
        match self.buffer_response(url, http) {
            (resp, None) => Ok(resp),
            (resp, Some(e)) => Err((resp, e)),
        }
    }
    fn requester_params(&self) -> Option<Values> {
        let requester = trim_space(&self.options.workitem_requester);
        if !valid_ae_title(requester) {
            return None;
        }
        let mut v = Values::new();
        v.set("requester", requester);
        Some(v)
    }
    fn workitem_request(
        &self,
        ctx: &Ctx,
        method: &str,
        url: &Url,
        body: Option<&[u8]>,
        expected: i64,
    ) -> (WorkitemResponse, Option<ClientError>) {
        let content_type = if body.is_some() { DICOM_JSON } else { "" };
        let (resp, err) = match self.do_request_bytes(
            ctx,
            method,
            url,
            body,
            content_type,
            ACCEPT_DICOM_JSON,
            &RequestOptions::default(),
        ) {
            Ok(r) => (r, None),
            Err((r, e)) => (r, Some(e)),
        };
        let mut result = WorkitemResponse {
            response: resp,
            ..WorkitemResponse::default()
        };
        if err.is_some() {
            return (result, err);
        }
        result.warnings = warning_texts(result.response.header.values("Warning"));
        result.location = result.response.header.get("Location").to_string();
        let status = result.response.status_code;
        if status != expected && !(200..300).contains(&status) {
            let e = status_error(&result.response);
            return (result, Some(e));
        }
        (result, None)
    }
    fn workitem_transaction(
        &self,
        ctx: &Ctx,
        method: &str,
        url: &Url,
        body: Option<&Object>,
        expected: i64,
    ) -> (WorkitemResponse, Option<ClientError>) {
        let Some(object) = body else {
            return self.workitem_request(ctx, method, url, None, expected);
        };
        let data = match dicomjson::marshal_text(Some(object), &mut dicomjson::Options::defaults())
        {
            Ok(d) => d,
            Err(e) => {
                return (
                    WorkitemResponse::default(),
                    Some(web(
                        ErrorKind::RequestFailure,
                        &url.string(),
                        0,
                        Cause::text(e),
                    )),
                );
            }
        };
        let mut wrapped = Vec::with_capacity(data.len() + 2);
        wrapped.push(b'[');
        wrapped.extend_from_slice(&data);
        wrapped.push(b']');
        self.workitem_request(ctx, method, url, Some(&wrapped), expected)
    }
    pub fn create_workitem(
        &self,
        ctx: &Ctx,
        uid: &str,
        workitem: Option<&Object>,
    ) -> (WorkitemResponse, Option<ClientError>) {
        let mut params = Values::new();
        if !trim_space(uid).is_empty() {
            params.set("workitem", trim_space(uid));
        }
        match self.endpoint.workitem_search_url(Some(&params)) {
            Ok(u) => self.workitem_transaction(ctx, "POST", &u, workitem, 201),
            Err(e) => (WorkitemResponse::default(), Some(ClientError::Web(e))),
        }
    }
    pub fn retrieve_workitem(&self, ctx: &Ctx, uid: &str) -> Result<Dataset, ClientError> {
        let u = self
            .endpoint
            .workitem_url(uid, None, &[])
            .map_err(ClientError::Web)?;
        self.limits()?;
        let resp = self
            .do_request_bytes(
                ctx,
                "GET",
                &u,
                None,
                "",
                ACCEPT_DICOM_JSON,
                &RequestOptions::default(),
            )
            .map_err(|(_, e)| e)?;
        if resp.status_code != 200 {
            return Err(status_error(&resp));
        }
        match datasets_from_dicom_json(&resp.body) {
            Ok(Some(mut d)) if d.len() == 1 => Ok(d.remove(0)),
            Ok(_) => Err(ClientError::Web(Error::new(
                ErrorKind::DecodeResponse,
                &resp.url,
                resp.status_code,
                None,
            ))),
            Err(e) => Err(decode_error(&resp, Cause::text(e))),
        }
    }
    pub fn update_workitem(
        &self,
        ctx: &Ctx,
        uid: &str,
        transaction: &str,
        modifications: Option<&Object>,
    ) -> (WorkitemResponse, Option<ClientError>) {
        let mut params = Values::new();
        if !trim_space(transaction).is_empty() {
            params.set("transaction", trim_space(transaction));
        }
        match self.endpoint.workitem_url(uid, Some(&params), &[]) {
            Ok(u) => self.workitem_transaction(ctx, "POST", &u, modifications, 200),
            Err(e) => (WorkitemResponse::default(), Some(ClientError::Web(e))),
        }
    }
    pub fn change_workitem_state(
        &self,
        ctx: &Ctx,
        uid: &str,
        state: &str,
        transaction: &str,
    ) -> (WorkitemResponse, Option<ClientError>) {
        let params = self.requester_params();
        let u = match self.endpoint.workitem_url(uid, params.as_ref(), &["state"]) {
            Ok(u) => u,
            Err(e) => return (WorkitemResponse::default(), Some(ClientError::Web(e))),
        };
        let body: Dataset = vec![
            (
                "00741000".into(),
                element("CS", vec![JAny::Str(state.into())]),
            ),
            (
                "00081195".into(),
                element("UI", vec![JAny::Str(transaction.into())]),
            ),
        ];
        let mut data = String::from("[");
        encode_dataset(&mut data, &body);
        data.push(']');
        self.workitem_request(ctx, "PUT", &u, Some(data.as_bytes()), 200)
    }
    pub fn request_workitem_cancellation(
        &self,
        ctx: &Ctx,
        uid: &str,
        information: Option<&Object>,
    ) -> (WorkitemResponse, Option<ClientError>) {
        let params = self.requester_params();
        match self
            .endpoint
            .workitem_url(uid, params.as_ref(), &["cancelrequest"])
        {
            Ok(u) => self.workitem_transaction(ctx, "POST", &u, information, 202),
            Err(e) => (WorkitemResponse::default(), Some(ClientError::Web(e))),
        }
    }
    pub fn search_workitems(
        &self,
        ctx: &Ctx,
        params: Option<&Values>,
    ) -> Result<Option<Vec<Dataset>>, ClientError> {
        let mut u = self
            .endpoint
            .workitem_search_url(params)
            .map_err(ClientError::Web)?;
        u.raw_query = u.raw_query.replace("%2C", ",");
        self.limits()?;
        let resp = self
            .do_request_bytes(
                ctx,
                "GET",
                &u,
                None,
                "",
                ACCEPT_DICOM_JSON,
                &RequestOptions::default(),
            )
            .map_err(|(_, e)| e)?;
        if resp.status_code == 204 {
            return Ok(None);
        }
        if resp.status_code != 200 {
            return Err(status_error(&resp));
        }
        datasets_from_dicom_json(&resp.body).map_err(|e| decode_error(&resp, Cause::text(e)))
    }
    pub fn subscribe_workitem(
        &self,
        ctx: &Ctx,
        uid: &str,
        ae_title: &str,
        options: &WorkitemSubscribeOptions,
    ) -> (WorkitemResponse, Option<ClientError>) {
        let mut params = Values::new();
        if options.deletion_lock {
            params.set("deletionlock", "true");
        }
        let filter = match subscription_filter(&options.filter) {
            Ok(f) => f,
            Err(e) => return (WorkitemResponse::default(), Some(ClientError::Web(e))),
        };
        let mut u = match self.subscriber_url(uid, ae_title, Some(&params)) {
            Ok(u) => u,
            Err(e) => return (WorkitemResponse::default(), Some(ClientError::Web(e))),
        };
        if !filter.is_empty() {
            if !u.raw_query.is_empty() {
                u.raw_query.push('&');
            }
            u.raw_query.push_str("filter=");
            u.raw_query.push_str(&filter);
        }
        self.workitem_request(ctx, "POST", &u, None, 201)
    }
    pub fn unsubscribe_workitem(
        &self,
        ctx: &Ctx,
        uid: &str,
        ae_title: &str,
    ) -> (WorkitemResponse, Option<ClientError>) {
        match self.subscriber_url(uid, ae_title, None) {
            Ok(u) => self.workitem_request(ctx, "DELETE", &u, None, 200),
            Err(e) => (WorkitemResponse::default(), Some(ClientError::Web(e))),
        }
    }
    pub fn suspend_workitem_subscription(
        &self,
        ctx: &Ctx,
        ae_title: &str,
    ) -> (WorkitemResponse, Option<ClientError>) {
        match self.subscriber_url(GLOBAL_WORKITEM_SUBSCRIPTION_UID, ae_title, None) {
            Ok(mut u) => {
                append_path_segment(&mut u, "suspend");
                self.workitem_request(ctx, "POST", &u, None, 200)
            }
            Err(e) => (WorkitemResponse::default(), Some(ClientError::Web(e))),
        }
    }
    fn subscriber_url(
        &self,
        uid: &str,
        ae_title: &str,
        params: Option<&Values>,
    ) -> Result<Url, Error> {
        if !valid_ae_title(ae_title) {
            return Err(invalid_endpoint("invalid AE Title"));
        }
        let mut u = self.endpoint.workitem_url(uid, params, &["subscribers"])?;
        append_path_segment(&mut u, ae_title);
        Ok(u)
    }
    fn mpps_client(&self) -> Client {
        let mut c = self.clone();
        c.options.retry = None;
        if c.options.max_body_bytes <= 0 || c.options.max_body_bytes > MPPS_MAX_BODY_BYTES {
            c.options.max_body_bytes = MPPS_MAX_BODY_BYTES;
        }
        c
    }
    pub fn create_mpps(
        &self,
        ctx: &Ctx,
        uid: &str,
        attributes: Option<&Object>,
    ) -> Result<(), ClientError> {
        self.write_mpps(ctx, uid, attributes, 201)
    }
    pub fn update_mpps(
        &self,
        ctx: &Ctx,
        uid: &str,
        modifications: Option<&Object>,
    ) -> Result<(), ClientError> {
        self.write_mpps(ctx, uid, modifications, 200)
    }
    fn write_mpps(
        &self,
        ctx: &Ctx,
        uid: &str,
        attributes: Option<&Object>,
        expected: i64,
    ) -> Result<(), ClientError> {
        let resource = self.endpoint.mpps_url(uid).map_err(ClientError::Web)?;
        let failure = |text: String| {
            web(
                ErrorKind::RequestFailure,
                &resource.string(),
                0,
                Cause::text(text),
            )
        };
        let Some(attributes) = attributes.filter(|a| !a.is_empty()) else {
            return Err(failure("MPPS requires a DICOM attribute list".into()));
        };
        verify_mpps_object_identity(attributes, uid, false).map_err(failure)?;
        let mut target = resource.clone();
        if expected == 200 {
            target.raw_query = "update".into();
        }
        let mut options = dicomjson::Options::default();
        options.limits.max_json_bytes = MPPS_MAX_BODY_BYTES;
        let data = dicomjson::marshal_text(Some(attributes), &mut options).map_err(failure)?;
        let no_follow = RequestOptions {
            no_follow: true,
            ..RequestOptions::default()
        };
        let resp = self
            .mpps_client()
            .do_request_bytes(
                ctx,
                "POST",
                &target,
                Some(&data),
                DICOM_JSON,
                DICOM_JSON,
                &no_follow,
            )
            .map_err(|(_, e)| e)?;
        if resp.status_code != expected {
            return Err(status_error(&resp));
        }
        verify_mpps_location(&resource, &resp)?;
        if !trim_space(&String::from_utf8_lossy(&resp.body)).is_empty() {
            return Err(decode_error(
                &resp,
                Cause::text("unexpected MPPS success response payload"),
            ));
        }
        match ctx_read_err(ctx) {
            Some(e) => Err(ClientError::Read(e)),
            None => Ok(()),
        }
    }
    pub fn retrieve_mpps(&self, ctx: &Ctx, uid: &str) -> Result<Object, ClientError> {
        let resource = self.endpoint.mpps_url(uid).map_err(ClientError::Web)?;
        let client = self.mpps_client();
        client.limits()?;
        let no_follow = RequestOptions {
            no_follow: true,
            ..RequestOptions::default()
        };
        let resp = client
            .do_request_bytes(ctx, "GET", &resource, None, "", DICOM_JSON, &no_follow)
            .map_err(|(_, e)| e)?;
        if resp.status_code != 200 {
            return Err(status_error(&resp));
        }
        verify_mpps_location(&resource, &resp)?;
        decode_mpps_dataset(&resp, uid).map_err(|cause| decode_error(&resp, cause))
    }
    pub fn commit_storage(
        &self,
        ctx: &Ctx,
        references: &[StorageCommitmentReference],
        opts: &StorageCommitmentOptions,
    ) -> (StorageCommitmentResult, Option<ClientError>) {
        let none = StorageCommitmentResult::default;
        let requested = match validate_commitment_references(references) {
            Ok(r) => r,
            Err(e) => return (none(), Some(ClientError::Other(e))),
        };
        if opts.timeout_nanos < 0 || opts.poll_interval_nanos < 0 {
            return (
                none(),
                Some(ClientError::Other(
                    "dicomweb: negative storage commitment timeout or poll interval".into(),
                )),
            );
        }
        let uid = if opts.transaction_uid.is_empty() {
            crate::object::inherit::new_uid()
        } else {
            opts.transaction_uid.clone()
        };
        let u = match self.storage_commitment_url(&uid) {
            Ok(u) => u,
            Err(e) => return (none(), Some(ClientError::Web(e))),
        };
        let body = encode_commitment_request(references);
        let timeout = if opts.timeout_nanos == 0 {
            120_000_000_000
        } else {
            opts.timeout_nanos
        };
        let interval = if opts.poll_interval_nanos == 0 {
            1_000_000_000
        } else {
            opts.poll_interval_nanos
        };
        let ctx = ctx.with_timeout(std::time::Duration::from_nanos(timeout as u64));
        let with_uid = || StorageCommitmentResult {
            transaction_uid: uid.clone(),
            ..StorageCommitmentResult::default()
        };
        if let Some(before) = &opts.before_request
            && let Err(e) = before(&ctx)
        {
            return (with_uid(), Some(e));
        }
        let (mut result, err) =
            self.storage_commitment_request(&ctx, &u, &uid, &requested, Some(body.as_bytes()));
        if err.is_some() {
            return (result, err);
        }
        while result.pending {
            if let Some(e) = sleep_context(&ctx, interval.max(result.retry_after_nanos)) {
                return (result, Some(context_request_error(&u.string(), &e)));
            }
            if let Some(before) = &opts.before_request
                && let Err(e) = before(&ctx)
            {
                return (result, Some(e));
            }
            let (next, err) = self.storage_commitment_request(&ctx, &u, &uid, &requested, None);
            if err.is_some() {
                return (result, err);
            }
            result = next;
        }
        (result, None)
    }
    pub fn check_storage_commitment(
        &self,
        ctx: &Ctx,
        uid: &str,
        references: &[StorageCommitmentReference],
    ) -> (StorageCommitmentResult, Option<ClientError>) {
        let requested = match validate_commitment_references(references) {
            Ok(r) => r,
            Err(e) => {
                return (
                    StorageCommitmentResult::default(),
                    Some(ClientError::Other(e)),
                );
            }
        };
        match self.storage_commitment_url(uid) {
            Ok(u) => self.storage_commitment_request(ctx, &u, uid, &requested, None),
            Err(e) => (
                StorageCommitmentResult::default(),
                Some(ClientError::Web(e)),
            ),
        }
    }
    fn storage_commitment_url(&self, uid: &str) -> Result<Url, Error> {
        if !is_valid_uid(uid.as_bytes()) {
            return Err(invalid_endpoint(
                "invalid storage commitment transaction UID",
            ));
        }
        self.endpoint
            .service_url("", &["commitment-requests", uid], None)
    }
    fn storage_commitment_request(
        &self,
        ctx: &Ctx,
        u: &Url,
        uid: &str,
        requested: &[(String, String)],
        body: Option<&[u8]>,
    ) -> (StorageCommitmentResult, Option<ClientError>) {
        let mut result = StorageCommitmentResult {
            transaction_uid: uid.to_string(),
            ..StorageCommitmentResult::default()
        };
        let mut client = self.clone();
        if client.options.max_body_bytes <= 0 || client.options.max_body_bytes > COMMITMENT_MAX_BODY
        {
            client.options.max_body_bytes = COMMITMENT_MAX_BODY;
        }
        let options = RequestOptions {
            same_origin_redirect: true,
            no_follow: true,
            ..RequestOptions::default()
        };
        let mut reader = body.map(SliceReader::new);
        let (method, content_type) = if body.is_some() {
            ("POST", DICOM_JSON)
        } else {
            ("GET", "")
        };
        let http = match client.do_http_attempt(
            ctx,
            method,
            u,
            reader.as_mut().map(|r| r as &mut dyn ByteRead),
            content_type,
            DICOM_JSON,
            &options,
        ) {
            Ok(h) => h,
            Err(e) => return (result, Some(e)),
        };
        let resp = match client.buffer_response(u, http) {
            (r, None) => r,
            (_, Some(e)) => return (result, Some(e)),
        };
        if let Err(text) = validate_commitment_location(u, resp.header.values("Location")) {
            return (
                result,
                Some(web(
                    ErrorKind::InvalidEndpoint,
                    &resp.url,
                    resp.status_code,
                    Cause::text(text),
                )),
            );
        }
        match resp.status_code {
            202 => {
                if !resp.body.is_empty() {
                    return (
                        result,
                        Some(decode_error(
                            &resp,
                            Cause::text("storage commitment HTTP 202 contains a payload"),
                        )),
                    );
                }
                result.pending = true;
                result.retry_after_nanos = retry_after(&resp.header).unwrap_or(0);
            }
            200 => {
                let content_type = match response_content_type(resp.header.values("Content-Type")) {
                    Ok(c) => c,
                    Err(e) => return (result, Some(decode_error(&resp, Cause::decode(e)))),
                };
                match parse_response_media_type(&content_type) {
                    Ok(m) if m.media_type == DICOM_JSON => {}
                    _ => {
                        return (
                            result,
                            Some(decode_error(
                                &resp,
                                Cause::text("expected a DICOM JSON storage commitment result"),
                            )),
                        );
                    }
                }
                match parse_commitment_result(&resp.body, uid, requested) {
                    Ok((committed, failed)) => {
                        result.committed = committed;
                        result.failed = failed;
                    }
                    Err(e) => return (result, Some(decode_error(&resp, Cause::text(e)))),
                }
            }
            _ => return (result, Some(status_error(&resp))),
        }
        if let Some(e) = ctx_read_err(ctx) {
            return (
                StorageCommitmentResult {
                    transaction_uid: uid.to_string(),
                    pending: result.pending,
                    ..StorageCommitmentResult::default()
                },
                Some(ClientError::Read(e)),
            );
        }
        (result, None)
    }
}
fn subscription_filter(filters: &[super::server::SearchFilter]) -> Result<String, Error> {
    let mut pairs = Vec::new();
    for filter in filters {
        let key = trim_space(&filter.key);
        if key.is_empty() || key.contains([',', '=']) {
            return Err(Error::new(
                ErrorKind::RequestFailure,
                "",
                0,
                Some(Cause::text("invalid subscription filter attribute")),
            ));
        }
        for value in &filter.values {
            if value.is_empty() || value.contains(',') {
                return Err(Error::new(
                    ErrorKind::RequestFailure,
                    "",
                    0,
                    Some(Cause::text(
                        "subscription filter value cannot be empty or contain a comma",
                    )),
                ));
            }
            pairs.push(urlrules::query_escape(&format!("{key}={value}")).replace('+', "%20"));
        }
    }
    Ok(pairs.join(","))
}
const MPPS_MAX_BODY_BYTES: i64 = 8 << 20;
const MPPS_SOP_CLASS_UID: &str = "1.2.840.10008.3.1.2.3.3";
const MPPS_RETRIEVE_SOP_CLASS_UID: &str = "1.2.840.10008.3.1.2.3.4";
impl Endpoint {
    pub fn mpps_url(&self, uid: &str) -> Result<Url, Error> {
        if !is_valid_uid(uid.as_bytes()) {
            return Err(invalid_endpoint("a valid MPPS UID is required"));
        }
        self.service_url("", &["modality-performed-procedure-steps", uid], None)
    }
}
fn verify_mpps_location(resource: &Url, resp: &Response) -> Result<(), ClientError> {
    for name in ["Location", "Content-Location"] {
        let locations = resp.header.values(name);
        if locations.is_empty() {
            continue;
        }
        if locations.len() == 1 && mpps_location_matches(resource, &locations[0]) {
            continue;
        }
        return Err(decode_error(
            resp,
            Cause::text(format!("MPPS {name} does not identify the requested step")),
        ));
    }
    Ok(())
}
fn mpps_location_matches(resource: &Url, value: &str) -> bool {
    let Ok(location) = urlrules::parse(trim_space(value)) else {
        return false;
    };
    if value.is_empty()
        || location.user.is_some()
        || !location.opaque.is_empty()
        || !location.raw_query.is_empty()
        || location.force_query
        || value.contains('#')
    {
        return false;
    }
    let target = resource.resolve_reference(&location);
    target.scheme.eq_ignore_ascii_case(&resource.scheme)
        && target.host.eq_ignore_ascii_case(&resource.host)
        && target.escaped_path() == resource.escaped_path()
}
fn verify_mpps_object_identity(dataset: &Object, uid: &str, retrieved: bool) -> Result<(), String> {
    for (tag, value) in [
        (
            Tag {
                group: 0x0008,
                element: 0x0018,
            },
            uid,
        ),
        (
            Tag {
                group: 0x0008,
                element: 0x0016,
            },
            MPPS_SOP_CLASS_UID,
        ),
    ] {
        if let Some(el) = dataset.get(tag) {
            let values = dataset.lookup_strings(tag);
            let Ok(values) = values.as_ref() else {
                return Err("MPPS attribute list contains a malformed SOP UID".into());
            };
            if el.vr() != Vr::UI || values.len() != 1 {
                return Err("MPPS attribute list contains a malformed SOP UID".into());
            }
            if values[0] != value
                && !(retrieved
                    && value == MPPS_SOP_CLASS_UID
                    && values[0] == MPPS_RETRIEVE_SOP_CLASS_UID)
            {
                return Err("MPPS attribute list identifies a different resource".into());
            }
        }
    }
    Ok(())
}
fn decode_mpps_dataset(resp: &Response, uid: &str) -> Result<Object, Cause> {
    let unexpected = || {
        Cause::decode(ResponseDecodeError::new(
            ResponseDecodeErrorKind::UnexpectedMedia,
        ))
    };
    let content_type =
        response_content_type(resp.header.values("Content-Type")).map_err(Cause::decode)?;
    let media = parse_response_media_type(&content_type).map_err(|_| unexpected())?;
    let params_ok = media
        .params
        .iter()
        .all(|(n, _)| n.eq_ignore_ascii_case("charset"));
    let charset = media
        .params
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case("charset"))
        .map(|(_, v)| trim_space(v).to_string())
        .unwrap_or_default();
    if media.media_type != DICOM_JSON
        || !params_ok
        || !(charset.is_empty() || charset.eq_ignore_ascii_case("utf-8"))
    {
        return Err(unexpected());
    }
    let text = String::from_utf8_lossy(&resp.body);
    let body = trim_space(&text).as_bytes();
    let opts = dicomjson::UnmarshalOptions {
        byte_order: Some(false),
        ..dicomjson::UnmarshalOptions::default()
    };
    let dataset = if body.first() == Some(&b'[') {
        let mut found: Option<Object> = None;
        dicomjson::decode_array_objects(body, &opts, &mut |candidate| {
            if found.is_some() {
                return Err("MPPS response contains more than one dataset".into());
            }
            found = Some(candidate);
            Ok(())
        })
        .map_err(Cause::text)?;
        found.ok_or_else(|| Cause::text("MPPS response contains no dataset"))?
    } else {
        dicomjson::decode_stream(body, &opts).map_err(Cause::text)?
    };
    verify_mpps_object_identity(&dataset, uid, true).map_err(Cause::text)?;
    let state_tag = Tag {
        group: 0x0040,
        element: 0x0252,
    };
    let vr = dataset.get(state_tag).map(|e| e.vr());
    let values = dataset.lookup_strings(state_tag);
    match (vr, values) {
        (Some(Vr::CS), Ok(values)) if values.len() == 1 => match values[0].as_str() {
            "IN PROGRESS" | "COMPLETED" | "DISCONTINUED" => Ok(dataset),
            _ => Err(Cause::text("invalid Performed Procedure Step Status")),
        },
        _ => Err(Cause::text(
            "MPPS response must provide one Performed Procedure Step Status",
        )),
    }
}
const MAX_COMMITMENT_REFERENCES: usize = 16_384;
const COMMITMENT_MAX_BODY: i64 = 8 << 20;
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StorageCommitmentReference {
    pub sop_class_uid: String,
    pub sop_instance_uid: String,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StorageCommitmentFailure {
    pub reference: StorageCommitmentReference,
    pub failure_reason: u16,
}
pub type BeforeRequest = Box<dyn Fn(&Ctx) -> Result<(), ClientError> + Send + Sync>;
#[derive(Default)]
pub struct StorageCommitmentOptions {
    pub transaction_uid: String,
    pub timeout_nanos: i64,
    pub poll_interval_nanos: i64,
    pub before_request: Option<BeforeRequest>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StorageCommitmentResult {
    pub transaction_uid: String,
    pub pending: bool,
    pub retry_after_nanos: i64,
    pub committed: Option<Vec<StorageCommitmentReference>>,
    pub failed: Option<Vec<StorageCommitmentFailure>>,
}
fn validate_commitment_references(
    references: &[StorageCommitmentReference],
) -> Result<Vec<(String, String)>, String> {
    if references.is_empty() || references.len() > MAX_COMMITMENT_REFERENCES {
        return Err(format!(
            "dicomweb: storage commitment requires between 1 and {MAX_COMMITMENT_REFERENCES} references"
        ));
    }
    let mut requested: Vec<(String, String)> = Vec::with_capacity(references.len());
    for r in references {
        if !is_valid_uid(r.sop_instance_uid.as_bytes()) || !is_valid_uid(r.sop_class_uid.as_bytes())
        {
            return Err("dicomweb: invalid SOP reference in storage commitment request".into());
        }
        if requested.iter().any(|(i, _)| *i == r.sop_instance_uid) {
            return Err("dicomweb: duplicate SOP instance in storage commitment request".into());
        }
        requested.push((r.sop_instance_uid.clone(), r.sop_class_uid.clone()));
    }
    Ok(requested)
}
fn encode_commitment_request(references: &[StorageCommitmentReference]) -> String {
    let mut out = String::from("{\"00081199\":{\"vr\":\"SQ\",\"Value\":[");
    for (i, r) in references.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        let item: Dataset = vec![
            (
                "00081150".into(),
                element("UI", vec![JAny::Str(r.sop_class_uid.clone())]),
            ),
            (
                "00081155".into(),
                element("UI", vec![JAny::Str(r.sop_instance_uid.clone())]),
            ),
        ];
        encode_dataset(&mut out, &item);
    }
    out.push_str("]}}");
    out
}
fn validate_commitment_location(u: &Url, locations: &[String]) -> Result<(), String> {
    if locations.is_empty() {
        return Ok(());
    }
    let invalid = || Err("invalid storage commitment Location".to_string());
    if locations.len() != 1 || trim_space(&locations[0]).is_empty() {
        return invalid();
    }
    let Ok(reference) = urlrules::parse(&locations[0]) else {
        return invalid();
    };
    let location = u.resolve_reference(&reference);
    if !super::http::same_origin(u, &location)
        || !location.opaque.is_empty()
        || location.escaped_path() != u.escaped_path()
        || !location.raw_query.is_empty()
        || location.force_query
        || !location.fragment.is_empty()
        || !location.raw_fragment.is_empty()
    {
        return Err(
            "storage commitment Location must identify the original transaction resource".into(),
        );
    }
    Ok(())
}
fn sequence_item_dataset(item: &JAny) -> Result<Dataset, String> {
    let mut text = String::new();
    encode_any(&mut text, item);
    super::response::dataset_from_json(text.as_bytes())
}
fn find<'a>(dataset: &'a Dataset, tag: &str) -> Option<&'a RawElement> {
    dataset.iter().find(|(k, _)| k == tag).map(|(_, e)| e)
}
fn commitment_uid(element: Option<&RawElement>) -> String {
    let Some(e) = element else {
        return String::new();
    };
    if e.vr != "UI"
        || !e.inline_binary.is_empty()
        || !e.bulk_data_uri.is_empty()
        || e.value.len() != 1
    {
        return String::new();
    }
    match &e.value[0] {
        JAny::Str(s) if is_valid_uid(s.as_bytes()) => s.clone(),
        _ => String::new(),
    }
}
type CommitmentLists = (
    Option<Vec<StorageCommitmentReference>>,
    Option<Vec<StorageCommitmentFailure>>,
);
fn parse_commitment_result(
    body: &[u8],
    uid: &str,
    requested: &[(String, String)],
) -> Result<CommitmentLists, String> {
    let one = "storage commitment response requires one DICOM dataset";
    let text = String::from_utf8_lossy(body);
    let trimmed = trim_space(&text).as_bytes();
    let dataset: Dataset = match trimmed.first() {
        None => return Err("storage commitment response is empty".into()),
        Some(b'[') => match datasets_from_dicom_json(trimmed) {
            Ok(Some(mut d)) if d.len() == 1 => d.remove(0),
            _ => return Err(one.into()),
        },
        Some(_) => super::response::dataset_from_json(trimmed).map_err(|_| one.to_string())?,
    };
    if let Some(e) = find(&dataset, "00081195")
        && commitment_uid(Some(e)) != uid
    {
        return Err("storage commitment response has an unexpected transaction UID".into());
    }
    for tag in ["00081110", "0008119B"] {
        if find(&dataset, tag).is_some() {
            return Err(
                "storage commitment response uses an unrequested grouped study layout".into(),
            );
        }
    }
    let mut seen: Vec<String> = Vec::new();
    let mut committed: Option<Vec<StorageCommitmentReference>> = None;
    let mut failed: Option<Vec<StorageCommitmentFailure>> = None;
    for tag in ["00081199", "00081198"] {
        let Some(sequence) = find(&dataset, tag) else {
            continue;
        };
        if sequence.vr != "SQ"
            || !sequence.inline_binary.is_empty()
            || !sequence.bulk_data_uri.is_empty()
            || sequence.value.is_empty()
            || sequence.value.len() > requested.len() - seen.len()
        {
            return Err(format!("invalid storage commitment {tag} sequence"));
        }
        for item in &sequence.value {
            let item = sequence_item_dataset(item)
                .map_err(|_| "invalid storage commitment reference item".to_string())?;
            let reference = StorageCommitmentReference {
                sop_class_uid: commitment_uid(find(&item, "00081150")),
                sop_instance_uid: commitment_uid(find(&item, "00081155")),
            };
            let expected = requested
                .iter()
                .find(|(i, _)| *i == reference.sop_instance_uid)
                .map(|(_, c)| c.as_str())
                .unwrap_or("");
            if reference.sop_instance_uid.is_empty()
                || reference.sop_class_uid.is_empty()
                || expected != reference.sop_class_uid
                || seen.contains(&reference.sop_instance_uid)
            {
                return Err(
                    "storage commitment response has an unexpected or repeated SOP reference"
                        .into(),
                );
            }
            seen.push(reference.sop_instance_uid.clone());
            if tag == "00081199" {
                if find(&item, "00081197").is_some() {
                    return Err("committed SOP reference has a failure reason".into());
                }
                committed.get_or_insert_with(Vec::new).push(reference);
                continue;
            }
            let reason = failure_reason(find(&item, "00081197"))?;
            failed
                .get_or_insert_with(Vec::new)
                .push(StorageCommitmentFailure {
                    reference,
                    failure_reason: reason,
                });
        }
    }
    if seen.len() != requested.len() {
        return Err("storage commitment response omitted a requested SOP reference".into());
    }
    Ok((committed, failed))
}
fn failure_reason(element: Option<&RawElement>) -> Result<u16, String> {
    if let Some(e) = element
        && e.vr == "US"
        && e.inline_binary.is_empty()
        && e.bulk_data_uri.is_empty()
        && e.value.len() == 1
        && let JAny::Number(n) = &e.value[0]
        && let Ok(reason) = crate::dicomjson::sizedparse::parse_uint(n, 16)
        && reason != 0
    {
        return Ok(reason as u16);
    }
    Err("failed storage commitment reference has no valid failure reason".into())
}
