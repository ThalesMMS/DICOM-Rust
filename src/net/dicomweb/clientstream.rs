use super::client::{
    ACCEPT_DICOM_JSON, BodyLimitReader, Client, ClientError, metadata_charset_supported,
    metadata_parameters_supported, metadata_transfer_syntax, read_cause, response_from_http,
    status_error, web,
};
use super::multipart::{ByteRead, ReadErr};
use super::response::{self, Response, scoped_metadata_ref};
use super::types::{
    Cause, CauseClass, DEFAULT_TIMEOUT_NANOS, ErrorKind, InstanceRef,
    METADATA_MEDIA_TYPE_DICOM_JSON, ResponseDecodeError, ResponseDecodeErrorKind as K,
};
use super::urlrules::{Url, Values};
use crate::dicomjson::dataset_stream::decode::{
    ReaderOptions, decode_datasets_reader, decode_raw_datasets_reader,
};
use crate::dicomjson::jsonwire::JAny;
use crate::dicomjson::{self, Dataset};
use crate::net::ul::{Ctx, Kind};
use std::io::Read;
use std::time::{Duration, Instant};
pub type DatasetHandler<'a> = dyn FnMut(Dataset) -> Result<(), Cause> + 'a;
pub type RefHandler<'a> = dyn FnMut(InstanceRef) -> Result<(), Cause> + 'a;
const METADATA_INSTANCE_MAX_ELEMENTS: i64 = 1_000_000;
const METADATA_INSTANCE_MAX_SEQUENCE_ITEMS: i64 = 1_000_000;
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Represented,
    Listing,
}
pub type StreamResult = (Response, Option<ClientError>);
const METADATA_JSON_RESPONSE_MEDIA_TYPES: [&str; 3] = [
    METADATA_MEDIA_TYPE_DICOM_JSON,
    "application/json",
    "text/plain",
];
fn identity(detail: &str) -> Cause {
    Cause {
        text: format!("{}: {detail}", response::ERR_INVALID_METADATA_IDENTITY),
        class: CauseClass::InvalidMetadataIdentity,
    }
}
#[derive(Default)]
struct InstanceBudget {
    elements: i64,
    items: i64,
}
impl InstanceBudget {
    fn dataset(&mut self, dataset: &Dataset) -> Result<(), Cause> {
        for (_, element) in dataset {
            self.element(&element.vr, &element.value)?;
        }
        Ok(())
    }
    fn element(&mut self, vr: &str, values: &[JAny]) -> Result<(), Cause> {
        self.elements += 1;
        if self.elements > METADATA_INSTANCE_MAX_ELEMENTS {
            return Err(Cause::text(format!(
                "{}: got more than {METADATA_INSTANCE_MAX_ELEMENTS} in one instance",
                dicomjson::ERR_MAX_ELEMENTS
            )));
        }
        if !super::mimeheader::trim_space(vr).eq_ignore_ascii_case("SQ") {
            return Ok(());
        }
        for value in values {
            self.items += 1;
            if self.items > METADATA_INSTANCE_MAX_SEQUENCE_ITEMS {
                return Err(Cause::text(format!(
                    "{}: got more than {METADATA_INSTANCE_MAX_SEQUENCE_ITEMS} in one instance",
                    dicomjson::ERR_MAX_SEQUENCE_ITEMS
                )));
            }
            let JAny::Obj(item) = value else {
                continue;
            };
            for (_, attribute) in item {
                let (mut nested_vr, mut nested_values): (&str, &[JAny]) = ("", &[]);
                if let JAny::Obj(fields) = attribute {
                    for (key, v) in fields {
                        match (key.as_str(), v) {
                            ("vr", JAny::Str(s)) => nested_vr = s,
                            ("Value", JAny::Arr(items)) => nested_values = items,
                            _ => {}
                        }
                    }
                }
                self.element(nested_vr, nested_values)?;
            }
        }
        Ok(())
    }
}
struct BodyReader<'a, 'b> {
    inner: &'a mut BodyLimitReader<'b>,
    pending: Vec<u8>,
    queued: Option<ReadErr>,
    read_err: Option<ReadErr>,
    ctx_err: Option<ReadErr>,
    ctx: &'a Ctx,
    deadline: Instant,
}
impl BodyReader<'_, '_> {
    fn context(&mut self) -> Option<ReadErr> {
        let err = match self.ctx.err() {
            Some(Kind::Canceled) => Some(ReadErr::Canceled),
            Some(_) => Some(ReadErr::Deadline),
            None if Instant::now() >= self.deadline => Some(ReadErr::Deadline),
            None => None,
        };
        if err.is_some() && self.ctx_err.is_none() {
            self.ctx_err = err.clone();
        }
        err
    }
    fn read_body(&mut self, buf: &mut [u8]) -> Result<usize, ReadErr> {
        if let Some(e) = self.queued.take() {
            return Err(e);
        }
        let (n, err) = self.inner.read(buf);
        match err {
            None | Some(ReadErr::Eof) => Ok(n),
            Some(e) => {
                if self.read_err.is_none() {
                    self.read_err = Some(e.clone());
                }
                if n > 0 {
                    self.queued = Some(e);
                    Ok(n)
                } else {
                    Err(e)
                }
            }
        }
    }
    fn empty_json_body(&mut self) -> Result<bool, ReadErr> {
        let mut chunk = [0u8; 4096];
        loop {
            let n = match self.read_body(&mut chunk) {
                Ok(0) => return Ok(true),
                Ok(n) => n,
                Err(e) => return Err(e),
            };
            if let Some(i) = chunk[..n]
                .iter()
                .position(|c| !matches!(c, b' ' | b'\t' | b'\r' | b'\n'))
            {
                self.pending.extend_from_slice(&chunk[i..n]);
                return Ok(false);
            }
        }
    }
}
impl Read for BodyReader<'_, '_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if let Some(e) = self.context() {
            return Err(std::io::Error::other(e.to_string()));
        }
        if !self.pending.is_empty() {
            let n = buf.len().min(self.pending.len());
            buf[..n].copy_from_slice(&self.pending[..n]);
            self.pending.drain(..n);
            return Ok(n);
        }
        self.read_body(buf)
            .map_err(|e| std::io::Error::other(e.to_string()))
    }
}
fn metadata_read_error(response: &Response, err: ReadErr) -> ClientError {
    match err {
        ReadErr::Web(e) => ClientError::Web(*e),
        other => web(
            ErrorKind::RequestFailure,
            &response.url,
            response.status_code,
            read_cause(&other),
        ),
    }
}
impl Client {
    pub fn search_studies_stream(
        &self,
        ctx: &Ctx,
        params: &Values,
        handle: Option<&mut DatasetHandler<'_>>,
    ) -> StreamResult {
        match self.endpoint.study_search_url(params) {
            Ok(url) => self.stream_metadata_at(ctx, &url, handle),
            Err(e) => (Response::default(), Some(ClientError::Web(e))),
        }
    }
    pub fn search_series_stream(
        &self,
        ctx: &Ctx,
        study: &str,
        params: &Values,
        handle: Option<&mut DatasetHandler<'_>>,
    ) -> StreamResult {
        match self.endpoint.series_search_url(study, params) {
            Ok(url) => self.stream_metadata_at(ctx, &url, handle),
            Err(e) => (Response::default(), Some(ClientError::Web(e))),
        }
    }
    pub fn search_instances_stream(
        &self,
        ctx: &Ctx,
        study: &str,
        series: &str,
        params: &Values,
        handle: Option<&mut DatasetHandler<'_>>,
    ) -> StreamResult {
        match self.endpoint.instance_search_url(study, series, params) {
            Ok(url) => self.stream_metadata_at(ctx, &url, handle),
            Err(e) => (Response::default(), Some(ClientError::Web(e))),
        }
    }
    pub fn search_all_series_stream(
        &self,
        ctx: &Ctx,
        params: &Values,
        handle: Option<&mut DatasetHandler<'_>>,
    ) -> StreamResult {
        match self.endpoint.all_series_search_url(params) {
            Ok(url) => self.stream_metadata_at(ctx, &url, handle),
            Err(e) => (Response::default(), Some(ClientError::Web(e))),
        }
    }
    pub fn search_all_instances_stream(
        &self,
        ctx: &Ctx,
        params: &Values,
        handle: Option<&mut DatasetHandler<'_>>,
    ) -> StreamResult {
        match self.endpoint.all_instance_search_url(params) {
            Ok(url) => self.stream_metadata_at(ctx, &url, handle),
            Err(e) => (Response::default(), Some(ClientError::Web(e))),
        }
    }
    pub fn search_study_instances_stream(
        &self,
        ctx: &Ctx,
        study: &str,
        params: &Values,
        handle: Option<&mut DatasetHandler<'_>>,
    ) -> StreamResult {
        match self.endpoint.study_instance_search_url(study, params) {
            Ok(url) => self.stream_metadata_at(ctx, &url, handle),
            Err(e) => (Response::default(), Some(ClientError::Web(e))),
        }
    }
    pub fn study_metadata_stream(
        &self,
        ctx: &Ctx,
        study: &str,
        handle: Option<&mut DatasetHandler<'_>>,
    ) -> StreamResult {
        let scope = InstanceRef {
            study_instance_uid: study.to_string(),
            ..InstanceRef::default()
        };
        match self.endpoint.study_metadata_url(study) {
            Ok(url) => self.stream_scoped_metadata_at(ctx, &url, &scope, handle),
            Err(e) => (Response::default(), Some(ClientError::Web(e))),
        }
    }
    pub fn series_metadata_stream(
        &self,
        ctx: &Ctx,
        study: &str,
        series: &str,
        handle: Option<&mut DatasetHandler<'_>>,
    ) -> StreamResult {
        let scope = InstanceRef {
            study_instance_uid: study.to_string(),
            series_instance_uid: series.to_string(),
            ..InstanceRef::default()
        };
        match self.endpoint.series_metadata_url(study, series) {
            Ok(url) => self.stream_scoped_metadata_at(ctx, &url, &scope, handle),
            Err(e) => (Response::default(), Some(ClientError::Web(e))),
        }
    }
    pub fn instance_metadata_stream(
        &self,
        ctx: &Ctx,
        instance: &InstanceRef,
        handle: Option<&mut DatasetHandler<'_>>,
    ) -> StreamResult {
        match self.endpoint.instance_metadata_url(instance) {
            Ok(url) => self.stream_scoped_metadata_at(ctx, &url, instance, handle),
            Err(e) => (Response::default(), Some(ClientError::Web(e))),
        }
    }
    pub fn study_metadata_refs_stream(
        &self,
        ctx: &Ctx,
        study: &str,
        handle: Option<&mut RefHandler<'_>>,
    ) -> StreamResult {
        let scope = InstanceRef {
            study_instance_uid: study.to_string(),
            ..InstanceRef::default()
        };
        match self.endpoint.study_metadata_url(study) {
            Ok(url) => self.refs_stream(ctx, &url, &scope, handle),
            Err(e) => (Response::default(), Some(ClientError::Web(e))),
        }
    }
    pub fn series_metadata_refs_stream(
        &self,
        ctx: &Ctx,
        study: &str,
        series: &str,
        handle: Option<&mut RefHandler<'_>>,
    ) -> StreamResult {
        let scope = InstanceRef {
            study_instance_uid: study.to_string(),
            series_instance_uid: series.to_string(),
            ..InstanceRef::default()
        };
        match self.endpoint.series_metadata_url(study, series) {
            Ok(url) => self.refs_stream(ctx, &url, &scope, handle),
            Err(e) => (Response::default(), Some(ClientError::Web(e))),
        }
    }
    fn refs_stream(
        &self,
        ctx: &Ctx,
        url: &Url,
        scope: &InstanceRef,
        handle: Option<&mut RefHandler<'_>>,
    ) -> StreamResult {
        match handle {
            None => self.stream_metadata_with(ctx, url, Mode::Represented, None),
            Some(handle) => {
                let mut wrapped = |r: InstanceRef, _: &Dataset| handle(r);
                self.stream_scoped_refs_at(ctx, url, scope, Mode::Listing, &mut wrapped)
            }
        }
    }
    fn stream_scoped_metadata_at(
        &self,
        ctx: &Ctx,
        url: &Url,
        scope: &InstanceRef,
        handle: Option<&mut DatasetHandler<'_>>,
    ) -> StreamResult {
        match handle {
            None => self.stream_metadata_with(ctx, url, Mode::Represented, None),
            Some(handle) => {
                let mut wrapped = |_: InstanceRef, d: &Dataset| handle(d.clone());
                self.stream_scoped_refs_at(ctx, url, scope, Mode::Represented, &mut wrapped)
            }
        }
    }
    fn stream_scoped_refs_at(
        &self,
        ctx: &Ctx,
        url: &Url,
        scope: &InstanceRef,
        mode: Mode,
        handle: &mut dyn FnMut(InstanceRef, &Dataset) -> Result<(), Cause>,
    ) -> StreamResult {
        let mut delivered = false;
        let track = scope.series_instance_uid.is_empty();
        let mut seen: Vec<InstanceRef> = Vec::new();
        let mut bind = |dataset: Dataset| -> Result<(), Cause> {
            if !scope.sop_instance_uid.is_empty() && delivered {
                return Err(identity("instance response contains more than one dataset"));
            }
            let found = scoped_metadata_ref(&dataset, scope).map_err(|text| Cause {
                text,
                class: CauseClass::InvalidMetadataIdentity,
            })?;
            if track {
                if let Some(previous) = seen
                    .iter_mut()
                    .find(|r| r.sop_instance_uid == found.sop_instance_uid)
                {
                    if *previous != found {
                        return Err(identity("tag 00080018 occurs with conflicting parent UIDs"));
                    }
                } else {
                    seen.push(found.clone());
                }
            }
            delivered = true;
            handle(found, &dataset)
        };
        self.stream_metadata_with(ctx, url, mode, Some(&mut bind))
    }
    fn stream_metadata_at(
        &self,
        ctx: &Ctx,
        url: &Url,
        handle: Option<&mut DatasetHandler<'_>>,
    ) -> StreamResult {
        self.stream_metadata_with(ctx, url, Mode::Represented, handle)
    }
    fn stream_metadata_with(
        &self,
        ctx: &Ctx,
        url: &Url,
        mode: Mode,
        handle: Option<&mut DatasetHandler<'_>>,
    ) -> StreamResult {
        let fail = |e: ClientError| (Response::default(), Some(e));
        let Some(handle) = handle else {
            return fail(web(
                ErrorKind::RequestFailure,
                "",
                0,
                Cause::text("nil metadata handler"),
            ));
        };
        if let Err(e) = self.validate_metadata_response_options() {
            return fail(e);
        }
        if !self.metadata_media_type_configured(METADATA_MEDIA_TYPE_DICOM_JSON) {
            return fail(web(
                ErrorKind::RequestFailure,
                "",
                0,
                Cause::text("JSON streaming requires the JSON metadata representation"),
            ));
        }
        let deadline = if self.options.timeout.0 > 0 {
            Instant::now() + Duration::from_nanos(self.options.timeout.0 as u64)
        } else {
            ctx.deadline().unwrap_or_else(|| {
                Instant::now() + Duration::from_nanos(DEFAULT_TIMEOUT_NANOS as u64)
            })
        };
        let mut http = match self.do_http(ctx, "GET", url, None, "", ACCEPT_DICOM_JSON) {
            Ok(http) => http,
            Err(e) => return fail(e),
        };
        let response = response_from_http(&url.string(), &http);
        if http.status_code == 204 {
            return (response, None);
        }
        if !(200..300).contains(&http.status_code) {
            let err = status_error(&response);
            return (response, Some(err));
        }
        let limits = match self.limits() {
            Ok(limits) => limits,
            Err(e) => return (response, Some(e)),
        };
        let maximum = self.max_body_bytes();
        let mut limited = BodyLimitReader {
            body: &mut http.body,
            remaining: maximum,
            limit: maximum,
            resp: &response,
            raw: true,
        };
        let mut body = BodyReader {
            inner: &mut limited,
            pending: Vec::new(),
            queued: None,
            read_err: None,
            ctx_err: None,
            ctx,
            deadline,
        };
        let decode = |cause: Cause| {
            web(
                ErrorKind::DecodeResponse,
                &response.url,
                response.status_code,
                cause,
            )
        };
        let empty = match body.empty_json_body() {
            Ok(empty) => empty,
            Err(e) => {
                let err = metadata_read_error(&response, e);
                return (response.clone(), Some(err));
            }
        };
        let content_types = response.header.values("Content-Type");
        if empty && content_types.is_empty() {
            return (response.clone(), None);
        }
        let content_type = match response::response_content_type(content_types) {
            Ok(value) => value,
            Err(e) => {
                let err = decode(Cause::decode(e));
                return (response.clone(), Some(err));
            }
        };
        let unsupported = || decode(Cause::text("unsupported JSON metadata Content-Type"));
        let parsed = match response::parse_response_media_type(&content_type) {
            Ok(parsed) => parsed,
            Err(_) => return (response.clone(), Some(unsupported())),
        };
        let charset = parsed
            .params
            .iter()
            .find(|(k, _)| k == "charset")
            .map_or("", |(_, v)| v.as_str());
        if !response::media_allowed(
            &super::mimeheader::to_lower(&parsed.media_type),
            &METADATA_JSON_RESPONSE_MEDIA_TYPES,
            &limits.allowed_media,
        ) || !metadata_charset_supported(charset)
            || !metadata_parameters_supported(&parsed.params, &["charset", "transfer-syntax"])
        {
            return (response.clone(), Some(unsupported()));
        }
        let syntax = match metadata_transfer_syntax(parsed.param("transfer-syntax")) {
            Ok(syntax) => syntax,
            Err(text) => {
                let err = decode(Cause::text(text));
                return (response.clone(), Some(err));
            }
        };
        if empty {
            return (response.clone(), None);
        }
        let maximum_datasets = self.metadata_dataset_limit(&limits);
        let mut count: i64 = 0;
        let mut failure: Option<Cause> = None;
        let mut next = |dataset: Dataset, failure: &mut Option<Cause>| -> Result<(), String> {
            let result = if count >= maximum_datasets {
                Err(Cause::decode(ResponseDecodeError::with(
                    K::PartLimit,
                    maximum_datasets,
                    count + 1,
                )))
            } else {
                count += 1;
                handle(dataset)
            };
            result.map_err(|cause| {
                let text = cause.text.clone();
                *failure = Some(cause);
                text
            })
        };
        let result = match mode {
            Mode::Listing => {
                let opts = ReaderOptions {
                    transfer_syntax: None,
                    limits: dicomjson::Limits {
                        max_json_bytes: maximum,
                        ..dicomjson::Limits::default()
                    },
                };
                decode_raw_datasets_reader(&mut body, &opts, &mut |dataset| {
                    let mut budget = InstanceBudget::default();
                    if let Err(cause) = budget.dataset(&dataset) {
                        let text = cause.text.clone();
                        failure = Some(cause);
                        return Err(text);
                    }
                    next(dataset, &mut failure)
                })
            }
            Mode::Represented => {
                let opts = ReaderOptions {
                    transfer_syntax: syntax,
                    limits: dicomjson::Limits {
                        max_json_bytes: maximum,
                        max_inline_binary_bytes: limits.max_part_bytes,
                        max_total_binary_bytes: maximum,
                        max_elements: 1_000_000,
                        max_sequence_items: 1_000_000,
                        ..dicomjson::Limits::default()
                    },
                };
                decode_datasets_reader(&mut body, &opts, &mut |dataset| next(dataset, &mut failure))
            }
        };
        let Err(text) = result else {
            return (response.clone(), None);
        };
        if let Some(e) = body.read_err.take() {
            let err = metadata_read_error(&response, e);
            return (response.clone(), Some(err));
        }
        let cause = match failure {
            Some(cause) => cause,
            None => match body.ctx_err.take() {
                Some(e) if text == e.to_string() => read_cause(&e),
                _ => Cause::text(text),
            },
        };
        let err = decode(cause);
        (response.clone(), Some(err))
    }
}
