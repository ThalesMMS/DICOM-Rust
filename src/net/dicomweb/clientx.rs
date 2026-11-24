use super::client::{
    BodyLimitReader, Client, ClientError, PartStream, RequestOptions, StoreInstance, decode_error,
    decode_kind, read_cause, request_failure, response_from_http, status_error, web,
};
use super::endpoint::join_url_path;
use super::http::same_origin as origin_same;
use super::mimeheader::{parse_media_type, to_lower, trim_space};
use super::multipart::{ReadErr, read_all};
use super::response::{self, MediaPolicy, ObjectPart, Response, StoreResult};
use super::types::{
    Cause, CauseClass, Endpoint, Error, ErrorKind, InstanceRef, ResponseDecodeError,
    ResponseDecodeErrorKind as K,
};
use super::urlrules::{self, Url, Values};
use crate::dicomjson::Dataset;
use crate::dicomjson::jsonwire::JAny;
use crate::net::ul::{Ctx, Nanos};
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
pub type PartHandler<'h> = &'h mut dyn FnMut(PartStream) -> Result<(), ClientError>;
fn part_read_error(err: ReadErr) -> ClientError {
    match err {
        ReadErr::Web(e) => ClientError::Web(*e),
        other => ClientError::Read(other),
    }
}
pub const DEFAULT_SEARCH_PAGE_SIZE: i64 = 100;
pub const DEFAULT_MAX_SEARCH_PAGES: i64 = 1000;
pub const ERR_SEARCH_PAGE_LIMIT: &str = "dicomweb: QIDO-RS search exceeded the page limit";
pub const ERR_SEARCH_PAGINATION: &str = "dicomweb: QIDO-RS pagination did not advance";
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SearchPagerOptions {
    pub page_size: i64,
    pub max_pages: i64,
    pub max_results: i64,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SearchPage {
    pub datasets: Vec<Dataset>,
    pub offset: i64,
    pub warnings: Vec<String>,
    pub more_results: bool,
}
#[derive(Clone, Debug)]
enum PagerScope {
    Studies,
    Series(String),
    Instances(String, String),
    AllSeries,
    AllInstances,
    StudyInstances(String),
}
#[derive(Clone)]
pub struct SearchPager {
    client: Client,
    scope: PagerScope,
    params: Values,
    opts: SearchPagerOptions,
    identity_tag: &'static str,
    seen: HashSet<String>,
    previous: Option<Vec<Dataset>>,
    offset: i64,
    pages: i64,
    results: i64,
    done: bool,
    err: Option<&'static str>,
}
pub(crate) fn search_warns_more_results(warnings: &[String]) -> bool {
    warnings.iter().any(|warning| {
        let warning = trim_space(warning);
        if !warning.starts_with("299") {
            return false;
        }
        let text = to_lower(warning);
        text.contains("additional result") || text.contains("exceed") || text.contains("truncat")
    })
}
fn sorted_dataset(dataset: &Dataset) -> Dataset {
    let mut sorted = dataset.clone();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    sorted
}
impl SearchPager {
    fn new(
        client: &Client,
        scope: PagerScope,
        identity_tag: &'static str,
        params: &Values,
        opts: SearchPagerOptions,
    ) -> SearchPager {
        let mut pager = SearchPager {
            client: client.clone(),
            scope,
            params: params.clone(),
            opts,
            identity_tag,
            seen: HashSet::new(),
            previous: None,
            offset: 0,
            pages: 0,
            results: 0,
            done: false,
            err: None,
        };
        if opts.page_size < 0 || opts.max_pages < 0 || opts.max_results < 0 {
            pager.err = Some("dicomweb: search pager options cannot be negative");
        }
        if pager.opts.page_size == 0 {
            pager.opts.page_size = DEFAULT_SEARCH_PAGE_SIZE;
        }
        if pager.opts.max_pages == 0 {
            pager.opts.max_pages = DEFAULT_MAX_SEARCH_PAGES;
        }
        let raw = trim_space(pager.params.get("offset")).to_string();
        if !raw.is_empty() {
            match raw.parse::<i64>() {
                Ok(offset) if offset >= 0 => pager.offset = offset,
                _ => {
                    pager.err = Some("dicomweb: search offset must be a non-negative integer");
                }
            }
        }
        pager.params.del("offset");
        pager.params.del("limit");
        pager
    }
    pub fn done(&self) -> bool {
        self.done
    }
    fn build(&self, params: &Values) -> Result<Url, Error> {
        let endpoint = &self.client.endpoint;
        match &self.scope {
            PagerScope::Studies => endpoint.study_search_url(params),
            PagerScope::Series(study) => endpoint.series_search_url(study, params),
            PagerScope::Instances(study, series) => {
                endpoint.instance_search_url(study, series, params)
            }
            PagerScope::AllSeries => endpoint.all_series_search_url(params),
            PagerScope::AllInstances => endpoint.all_instance_search_url(params),
            PagerScope::StudyInstances(study) => endpoint.study_instance_search_url(study, params),
        }
    }
    pub fn next(&mut self, ctx: &Ctx) -> Result<Option<SearchPage>, ClientError> {
        if let Some(text) = self.err {
            return Err(ClientError::Other(text.to_string()));
        }
        if self.done {
            return Ok(None);
        }
        if self.pages >= self.opts.max_pages {
            self.done = true;
            return Err(ClientError::SearchPageLimit);
        }
        self.client.validate_metadata_response_options()?;
        let mut limit = self.opts.page_size;
        if self.opts.max_results > 0 && self.opts.max_results - self.results < limit {
            limit = self.opts.max_results - self.results;
        }
        let mut params = self.params.clone();
        params.set("limit", &limit.to_string());
        if self.offset > 0 {
            params.set("offset", &self.offset.to_string());
        }
        let url = self.build(&params)?;
        let (datasets, resp) = self.client.search_at_response(ctx, &url)?;
        self.pages += 1;
        let warnings = resp.header.values("Warning").to_vec();
        let more_results = search_warns_more_results(&warnings);
        let datasets = datasets.unwrap_or_default();
        let count = datasets.len() as i64;
        if count == 0 {
            self.done = true;
            if more_results {
                return Err(ClientError::SearchPagination);
            }
            return Ok(None);
        }
        if count > limit || self.offset > i64::MAX - count {
            self.done = true;
            return Err(ClientError::SearchPagination);
        }
        let offset = self.offset;
        let Some(fresh) = self.new_results(datasets) else {
            self.done = true;
            return Err(ClientError::SearchPagination);
        };
        self.offset += count;
        self.results += fresh.len() as i64;
        if (self.opts.max_results > 0 && self.results >= self.opts.max_results)
            || (count < limit && !more_results)
        {
            self.done = true;
        }
        Ok(Some(SearchPage {
            datasets: fresh,
            offset,
            warnings,
            more_results,
        }))
    }
    fn result_uid<'d>(&self, dataset: &'d Dataset) -> Option<&'d str> {
        let (_, element) = dataset.iter().find(|(tag, _)| tag == self.identity_tag)?;
        if element.vr != "UI" || element.value.len() != 1 {
            return None;
        }
        match &element.value[0] {
            JAny::Str(uid) if crate::object::is_valid_uid(uid.as_bytes()) => Some(uid),
            _ => None,
        }
    }
    fn new_results(&mut self, datasets: Vec<Dataset>) -> Option<Vec<Dataset>> {
        let has_unidentified = datasets.iter().any(|d| self.result_uid(d).is_none());
        if has_unidentified {
            let fingerprint: Vec<Dataset> = datasets.iter().map(sorted_dataset).collect();
            if self.previous.as_ref() == Some(&fingerprint) {
                return None;
            }
            self.previous = Some(fingerprint);
        } else {
            self.previous = None;
        }
        let mut results = Vec::with_capacity(datasets.len());
        for dataset in datasets {
            if let Some(uid) = self.result_uid(&dataset) {
                if self.seen.contains(uid) {
                    continue;
                }
                self.seen.insert(uid.to_string());
            }
            results.push(dataset);
        }
        if results.is_empty() {
            return None;
        }
        Some(results)
    }
    pub fn all(&mut self, ctx: &Ctx) -> (Vec<Dataset>, Option<ClientError>) {
        let mut all = Vec::new();
        loop {
            match self.next(ctx) {
                Err(e) => return (all, Some(e)),
                Ok(None) => return (all, None),
                Ok(Some(page)) => all.extend(page.datasets),
            }
        }
    }
}
impl Endpoint {
    pub fn all_series_search_url(&self, params: &Values) -> Result<Url, Error> {
        self.service_root_url(&self.qido_path, &["series"], Some(params))
    }
    pub fn all_instance_search_url(&self, params: &Values) -> Result<Url, Error> {
        self.service_root_url(&self.qido_path, &["instances"], Some(params))
    }
    pub fn study_instance_search_url(&self, study: &str, params: &Values) -> Result<Url, Error> {
        let study = trim_space(study);
        if study.is_empty() {
            return Err(Error::invalid_endpoint("study instance UID is required"));
        }
        self.service_url(
            &self.qido_path,
            &["studies", study, "instances"],
            Some(params),
        )
    }
}
impl Client {
    pub fn study_pager(&self, params: &Values, opts: SearchPagerOptions) -> SearchPager {
        SearchPager::new(self, PagerScope::Studies, "0020000D", params, opts)
    }
    pub fn series_pager(
        &self,
        study: &str,
        params: &Values,
        opts: SearchPagerOptions,
    ) -> SearchPager {
        let scope = PagerScope::Series(study.to_string());
        SearchPager::new(self, scope, "0020000E", params, opts)
    }
    pub fn instance_pager(
        &self,
        study: &str,
        series: &str,
        params: &Values,
        opts: SearchPagerOptions,
    ) -> SearchPager {
        let scope = PagerScope::Instances(study.to_string(), series.to_string());
        SearchPager::new(self, scope, "00080018", params, opts)
    }
    pub fn all_series_pager(&self, params: &Values, opts: SearchPagerOptions) -> SearchPager {
        SearchPager::new(self, PagerScope::AllSeries, "0020000E", params, opts)
    }
    pub fn all_instance_pager(&self, params: &Values, opts: SearchPagerOptions) -> SearchPager {
        SearchPager::new(self, PagerScope::AllInstances, "00080018", params, opts)
    }
    pub fn study_instance_pager(
        &self,
        study: &str,
        params: &Values,
        opts: SearchPagerOptions,
    ) -> SearchPager {
        let scope = PagerScope::StudyInstances(study.to_string());
        SearchPager::new(self, scope, "00080018", params, opts)
    }
    fn search_built(
        &self,
        ctx: &Ctx,
        build: impl FnOnce() -> Result<Url, Error>,
    ) -> Result<Option<Vec<Dataset>>, ClientError> {
        self.validate_metadata_response_options()?;
        let url = build()?;
        self.search_at_response(ctx, &url)
            .map(|(datasets, _)| datasets)
    }
    pub fn search_all_series(
        &self,
        ctx: &Ctx,
        params: &Values,
    ) -> Result<Option<Vec<Dataset>>, ClientError> {
        self.search_built(ctx, || self.endpoint.all_series_search_url(params))
    }
    pub fn search_all_instances(
        &self,
        ctx: &Ctx,
        params: &Values,
    ) -> Result<Option<Vec<Dataset>>, ClientError> {
        self.search_built(ctx, || self.endpoint.all_instance_search_url(params))
    }
    pub fn search_study_instances(
        &self,
        ctx: &Ctx,
        study: &str,
        params: &Values,
    ) -> Result<Option<Vec<Dataset>>, ClientError> {
        self.search_built(ctx, || {
            self.endpoint.study_instance_search_url(study, params)
        })
    }
}
const WADO_URI_POLICY: MediaPolicy = MediaPolicy {
    require_multipart: false,
    plain_single_media: true,
    frames: false,
    single: &["application/dicom"],
    parts: &[],
};
pub fn normalize_wado_uri_path(value: &str) -> Result<String, crate::MessageError> {
    if value.contains(['\\', '?', '#', ':', '%']) || value.chars().any(char::is_control) {
        return Err("WADO-URI path must be a relative HTTP service path".into());
    }
    let value = trim_space(value);
    if value.is_empty() {
        return Ok(String::new());
    }
    let path = value.strip_prefix('/').unwrap_or(value);
    let path = path.strip_suffix('/').unwrap_or(path);
    if path
        .split('/')
        .any(|segment| segment.is_empty() || segment == "." || segment == "..")
    {
        return Err("WADO-URI path must contain only nonempty service segments".into());
    }
    Ok(path.to_string())
}
impl Endpoint {
    pub fn wado_uri_instance_url(&self, instance: &InstanceRef) -> Result<Url, Error> {
        let path = normalize_wado_uri_path(&self.wado_uri_path)
            .map_err(|e| Error::invalid_endpoint(e.message()))?;
        if path.is_empty() {
            return Err(Error::invalid_endpoint("WADO-URI path is required"));
        }
        let valid = |uid: &str| crate::object::is_valid_uid(uid.as_bytes());
        if !valid(&instance.study_instance_uid)
            || !valid(&instance.series_instance_uid)
            || !valid(&instance.sop_instance_uid)
        {
            return Err(Error::invalid_endpoint(
                "WADO-URI requires valid study, series and SOP instance UIDs",
            ));
        }
        let mut params = Values::new();
        params.set("requestType", "WADO");
        params.set("studyUID", &instance.study_instance_uid);
        params.set("seriesUID", &instance.series_instance_uid);
        params.set("objectUID", &instance.sop_instance_uid);
        params.set("contentType", "application/dicom");
        self.service_url(&path, &[], Some(&params))
    }
}
impl Client {
    pub fn retrieve_wado_uri_instance_stream(
        &self,
        ctx: &Ctx,
        instance: &InstanceRef,
        handle: Option<PartHandler>,
    ) -> Result<(), ClientError> {
        let Some(handle) = handle else {
            return Err(ClientError::Other(
                "WADO-URI stream handler is required".into(),
            ));
        };
        let url = self.endpoint.wado_uri_instance_url(instance)?;
        self.limits()?;
        let options = RequestOptions {
            headers: Vec::new(),
            same_origin_redirect: true,
            ..RequestOptions::default()
        };
        let mut http =
            self.do_http_opts(ctx, "GET", &url, None, "", "application/dicom", &options)?;
        let resp = response_from_http(&url.string(), &http);
        if resp.status_code != 200 {
            return Err(status_error(&resp));
        }
        if !resp.header.values("Content-Range").is_empty() {
            return Err(decode_error(
                &resp,
                Cause::text("WADO-URI response must contain the complete instance"),
            ));
        }
        self.stream_object_parts(&resp, &mut http.body, &WADO_URI_POLICY, &mut |mut part| {
            part.content_location = String::new();
            handle(part)
        })
    }
}
pub(super) const ACCEPT_BULK_DATA: &str = "multipart/related; type=\"application/octet-stream\"";
const BULK_DATA_RANGE_MEDIA_TYPE: &str = "application/octet-stream";
const BULK_DATA_MEDIA: &[&str] = &[
    "application/octet-stream",
    "image/jpeg",
    "image/jls",
    "image/jp2",
    "image/jpx",
    "image/jphc",
    "image/jxl",
    "image/dicom-rle",
    "video/mpeg",
    "video/mp4",
    "video/h265",
];
pub(super) const BULK_DATA_POLICY: MediaPolicy = MediaPolicy {
    require_multipart: false,
    plain_single_media: false,
    frames: false,
    single: BULK_DATA_MEDIA,
    parts: BULK_DATA_MEDIA,
};
pub const ERR_BULK_DATA_RANGE_RESPONSE: &str =
    "DICOMweb BulkData response does not match the requested byte range";
fn same_origin(left: &Url, right: &Url) -> bool {
    right.user.is_none() && origin_same(left, right)
}
fn resolve_bulk_data_reference(base: &Url, uri: &str) -> Result<Url, Error> {
    let uri = trim_space(uri);
    let reference = match urlrules::parse(uri) {
        Ok(reference) if !uri.is_empty() => reference,
        _ => return Err(Error::invalid_endpoint("BulkDataURI is malformed")),
    };
    if reference.user.is_some() {
        return Err(Error::invalid_endpoint(
            "BulkDataURI must not include user information",
        ));
    }
    let mut resolved = base.resolve_reference(&reference);
    resolved.fragment.clear();
    resolved.raw_fragment.clear();
    if !same_origin(base, &resolved) {
        return Err(Error::invalid_endpoint(
            "BulkDataURI must use the endpoint origin",
        ));
    }
    Ok(resolved)
}
impl Endpoint {
    pub fn bulk_data_url(&self, uri: &str) -> Result<Url, Error> {
        let mut base = self.service_url("", &[], None)?;
        if !base.path.ends_with('/') {
            base.path.push('/');
        }
        resolve_bulk_data_reference(&base, uri)
    }
    pub fn bulk_data_url_from_metadata(&self, metadata_url: &str, uri: &str) -> Result<Url, Error> {
        let origin = self.service_url("", &[], None)?;
        match urlrules::parse(trim_space(metadata_url)) {
            Ok(base)
                if same_origin(&origin, &base)
                    && base.opaque.is_empty()
                    && base.fragment.is_empty()
                    && base.raw_fragment.is_empty() =>
            {
                resolve_bulk_data_reference(&base, uri)
            }
            _ => Err(Error::invalid_endpoint(
                "BulkData metadata request URL must be absolute and use the endpoint origin",
            )),
        }
    }
}
fn equal_fold_ascii(value: &str, lower: &str) -> bool {
    let folded: String = value
        .chars()
        .map(|c| match c {
            '\u{017F}' => 's',
            '\u{212A}' => 'k',
            c => c.to_ascii_lowercase(),
        })
        .collect();
    folded == lower
}
fn parse_uint63(text: &str) -> Option<i64> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse::<i64>().ok()
}
fn matches_bulk_data_content_range(values: &[String], offset: i64, end: i64) -> bool {
    if values.len() != 1 {
        return false;
    }
    let Some((unit, remainder)) = trim_space(&values[0]).split_once(' ') else {
        return false;
    };
    if !equal_fold_ascii(unit, "bytes") {
        return false;
    }
    let Some((interval, total_length)) = remainder.split_once('/') else {
        return false;
    };
    let Some((first, last)) = interval.split_once('-') else {
        return false;
    };
    let (Some(got_offset), Some(got_end)) = (parse_uint63(first), parse_uint63(last)) else {
        return false;
    };
    if got_offset != offset || got_end != end {
        return false;
    }
    if total_length == "*" {
        return true;
    }
    matches!(parse_uint63(total_length), Some(total) if total > got_end)
}
fn range_mismatch(resp: &Response, text: String, inner: CauseClass) -> ClientError {
    decode_error(
        resp,
        Cause {
            text,
            class: CauseClass::BulkDataRange(Box::new(inner)),
        },
    )
}
impl Client {
    fn stream_resource(
        &self,
        ctx: &Ctx,
        url: &Url,
        accept: &str,
        policy: &MediaPolicy,
        same_origin_redirect: bool,
        handle: PartHandler,
    ) -> Result<(), ClientError> {
        self.limits()?;
        let options = RequestOptions {
            headers: Vec::new(),
            same_origin_redirect,
            ..RequestOptions::default()
        };
        let mut http = self.do_http_opts(ctx, "GET", url, None, "", accept, &options)?;
        let resp = response_from_http(&url.string(), &http);
        if !(200..300).contains(&resp.status_code) {
            return Err(status_error(&resp));
        }
        self.stream_object_parts(&resp, &mut http.body, policy, handle)
    }
    pub fn retrieve_bulk_data_stream(
        &self,
        ctx: &Ctx,
        uri: &str,
        handle: Option<PartHandler>,
    ) -> Result<(), ClientError> {
        let Some(handle) = handle else {
            return Err(ClientError::Other(
                "WADO-RS bulk data stream handler is required".into(),
            ));
        };
        let url = self.endpoint.bulk_data_url(uri)?;
        self.stream_resource(ctx, &url, ACCEPT_BULK_DATA, &BULK_DATA_POLICY, true, handle)
    }
    pub fn retrieve_bulk_data(&self, ctx: &Ctx, uri: &str) -> Result<Vec<ObjectPart>, ClientError> {
        let mut parts = Vec::new();
        self.retrieve_bulk_data_stream(
            ctx,
            uri,
            Some(&mut |part| {
                let (data, err) = read_all(part.reader);
                if let Some(e) = err {
                    return Err(part_read_error(e));
                }
                parts.push(ObjectPart {
                    content_type: part.content_type,
                    transfer_syntax_uid: part.transfer_syntax_uid,
                    data,
                });
                Ok(())
            }),
        )?;
        Ok(parts)
    }
    pub fn resolve_bulk_data_uri(
        &self,
        ctx: &Ctx,
        uri: &str,
        max_bytes: i64,
    ) -> Result<Vec<u8>, ClientError> {
        let limits = self.limits()?;
        let mut limit = limits.max_part_bytes;
        if max_bytes > 0 && max_bytes < limit {
            limit = max_bytes;
        }
        let mut data = Vec::new();
        let mut count: i64 = 0;
        let mut too_large = false;
        let result = self.retrieve_bulk_data_stream(
            ctx,
            uri,
            Some(&mut |part| {
                count += 1;
                if count > 1 {
                    return Err(ClientError::Read(ReadErr::Decode(
                        ResponseDecodeError::with(K::PartLimit, 1, count),
                    )));
                }
                let mut limited = response::LimitReader {
                    reader: part.reader,
                    remaining: limit.saturating_add(1),
                };
                let (read, err) = read_all(&mut limited);
                if let Some(e) = err {
                    return Err(part_read_error(e));
                }
                if read.len() as i64 > limit {
                    too_large = true;
                    return Err(ClientError::Other(
                        "bulk data exceeds the caller limit".into(),
                    ));
                }
                data = read;
                Ok(())
            }),
        );
        if too_large {
            return Err(ClientError::BulkDataTooLarge(None));
        }
        match result {
            Ok(()) => Ok(data),
            Err(e) => {
                let part_bytes = match &e {
                    ClientError::Web(web) => matches!(
                        web.err.as_ref().map(|c| &c.class),
                        Some(CauseClass::Decode(d)) if d.kind == K::PartBytes
                    ),
                    ClientError::Read(ReadErr::Decode(d)) => d.kind == K::PartBytes,
                    _ => false,
                };
                if part_bytes {
                    Err(ClientError::BulkDataTooLarge(Some(Box::new(e))))
                } else {
                    Err(e)
                }
            }
        }
    }
    pub fn retrieve_bulk_data_range(
        &self,
        ctx: &Ctx,
        uri: &str,
        offset: i64,
        length: i64,
    ) -> Result<ObjectPart, ClientError> {
        let limits = self.limits()?;
        if offset < 0 || length <= 0 || offset > i64::MAX - (length - 1) {
            return Err(request_failure("invalid BulkData byte range"));
        }
        let maximum = limits.max_part_bytes;
        if length > maximum {
            return Err(web(
                ErrorKind::RequestFailure,
                "",
                0,
                Cause::decode(ResponseDecodeError::with(K::PartBytes, maximum, length)),
            ));
        }
        let url = self.endpoint.bulk_data_url(uri)?;
        let end = offset + (length - 1);
        let options = RequestOptions {
            headers: vec![
                ("Range".to_string(), format!("bytes={offset}-{end}")),
                ("Accept-Encoding".to_string(), "identity".to_string()),
            ],
            same_origin_redirect: true,
            ..RequestOptions::default()
        };
        let mut http = self.do_http_opts(
            ctx,
            "GET",
            &url,
            None,
            "",
            BULK_DATA_RANGE_MEDIA_TYPE,
            &options,
        )?;
        let resp = response_from_http(&url.string(), &http);
        if !(200..300).contains(&resp.status_code) {
            return Err(status_error(&resp));
        }
        let encodings = resp.header.values("Content-Encoding");
        let identity = encodings.is_empty()
            || (encodings.len() == 1 && equal_fold_ascii(trim_space(&encodings[0]), "identity"));
        if resp.status_code != 206
            || !matches_bulk_data_content_range(resp.header.values("Content-Range"), offset, end)
            || (http.content_length >= 0 && http.content_length != length)
            || !identity
        {
            return Err(range_mismatch(
                &resp,
                ERR_BULK_DATA_RANGE_RESPONSE.to_string(),
                CauseClass::Other,
            ));
        }
        let content_type = response::response_content_type(resp.header.values("Content-Type"))
            .map_err(|e| decode_kind(&resp, e))?;
        let parsed = response::parse_response_media_type(&content_type)
            .map_err(|e| decode_kind(&resp, e))?;
        if parsed.media_type != BULK_DATA_RANGE_MEDIA_TYPE
            || !response::media_allowed(
                &parsed.media_type,
                BULK_DATA_POLICY.single,
                &limits.allowed_media,
            )
        {
            return Err(decode_kind(
                &resp,
                ResponseDecodeError::new(K::UnexpectedMedia),
            ));
        }
        let mut reader = BodyLimitReader {
            body: &mut http.body,
            remaining: length,
            limit: length,
            resp: &resp,
            raw: true,
        };
        let (data, err) = read_all(&mut reader);
        match err {
            Some(e) => {
                let class = match &e {
                    ReadErr::Web(inner) => inner
                        .err
                        .as_ref()
                        .map(|c| c.class.clone())
                        .unwrap_or_default(),
                    other => read_cause(other).class,
                };
                Err(range_mismatch(
                    &resp,
                    format!("{ERR_BULK_DATA_RANGE_RESPONSE}\n{e}"),
                    class,
                ))
            }
            None if data.len() as i64 != length => Err(range_mismatch(
                &resp,
                ERR_BULK_DATA_RANGE_RESPONSE.to_string(),
                CauseClass::Other,
            )),
            None => Ok(ObjectPart {
                transfer_syntax_uid: trim_space(parsed.param("transfer-syntax")).to_string(),
                content_type,
                data,
            }),
        }
    }
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RenderViewport {
    pub width: i64,
    pub height: i64,
    pub source_x: Option<f64>,
    pub source_y: Option<f64>,
    pub source_width: Option<f64>,
    pub source_height: Option<f64>,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RenderWindow {
    pub center: f64,
    pub width: f64,
    pub function: String,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RenderTarget {
    pub study_instance_uid: String,
    pub series_instance_uid: String,
    pub sop_instance_uid: String,
    pub frames: Vec<i64>,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RenderedOptions {
    pub media_types: Vec<String>,
    pub viewport: Option<RenderViewport>,
    pub window: Option<RenderWindow>,
    pub quality: i64,
    pub annotations: Vec<String>,
    pub icc_profile: String,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ThumbnailOptions {
    pub media_types: Vec<String>,
    pub viewport: Option<RenderViewport>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RenderedObject {
    pub content_type: String,
    pub content_location: String,
    pub data: Vec<u8>,
}
fn invalid_render_option(message: &str) -> Error {
    Error::invalid_endpoint(&format!("dicomweb: {message}"))
}
fn format_render_float(value: f64) -> String {
    format!("{value}")
}
fn render_viewport_value(viewport: &RenderViewport) -> Result<String, Error> {
    if viewport.width <= 0 || viewport.height <= 0 {
        return Err(invalid_render_option(
            "viewport width and height must be positive",
        ));
    }
    let mut values = vec![viewport.width.to_string(), viewport.height.to_string()];
    let sources = [
        viewport.source_x,
        viewport.source_y,
        viewport.source_width,
        viewport.source_height,
    ];
    let mut last = None;
    for (index, source) in sources.iter().enumerate() {
        if let Some(source) = source {
            if !source.is_finite() || (index >= 2 && *source == 0.0) {
                return Err(invalid_render_option(
                    "viewport source values must be finite and source sizes non-zero",
                ));
            }
            last = Some(index);
        }
    }
    if let Some(last) = last {
        for source in &sources[..=last] {
            values.push(source.map(format_render_float).unwrap_or_default());
        }
    }
    Ok(values.join(","))
}
fn rendered_query(opts: &RenderedOptions) -> Result<Values, Error> {
    let mut query = Values::new();
    if let Some(viewport) = &opts.viewport {
        query.set("viewport", &render_viewport_value(viewport)?);
    }
    if let Some(window) = &opts.window {
        if !window.center.is_finite() || !window.width.is_finite() || window.width <= 0.0 {
            return Err(invalid_render_option(
                "window center must be finite and width positive",
            ));
        }
        let mut function = trim_space(&window.function);
        if function.is_empty() {
            function = "linear";
        }
        if !matches!(function, "linear" | "linear-exact" | "sigmoid") {
            return Err(invalid_render_option(
                "window function must be linear, linear-exact or sigmoid",
            ));
        }
        query.set(
            "window",
            &format!(
                "{},{},{function}",
                format_render_float(window.center),
                format_render_float(window.width)
            ),
        );
    }
    if opts.quality != 0 {
        if !(1..=100).contains(&opts.quality) {
            return Err(invalid_render_option("quality must be between 1 and 100"));
        }
        query.set("quality", &opts.quality.to_string());
    }
    if !opts.annotations.is_empty() {
        let mut values = Vec::with_capacity(opts.annotations.len());
        for annotation in &opts.annotations {
            let annotation = trim_space(annotation);
            if annotation.is_empty() || annotation.contains([',', ' ']) {
                return Err(invalid_render_option(
                    "annotation values must be single keywords",
                ));
            }
            values.push(annotation);
        }
        query.set("annotation", &values.join(","));
    }
    let profile = trim_space(&opts.icc_profile);
    if !profile.is_empty() {
        if !matches!(
            profile,
            "no" | "yes" | "srgb" | "adobergb" | "rommrgb" | "displayp3"
        ) {
            return Err(invalid_render_option("unsupported iccprofile value"));
        }
        query.set("iccprofile", profile);
    }
    Ok(query)
}
const THUMBNAIL_MEDIA: &[&str] = &[
    "image/jpeg",
    "image/png",
    "image/gif",
    "image/jp2",
    "image/jph",
    "image/jphc",
    "image/jxl",
];
const RENDERED_MEDIA: &[&str] = &[
    "image/jpeg",
    "image/png",
    "image/gif",
    "image/jp2",
    "image/jph",
    "image/jphc",
    "image/jxl",
    "video/mpeg",
    "video/mp4",
    "video/h265",
    "text/html",
    "text/plain",
    "application/pdf",
];
pub(crate) fn is_rendered_media_type(media_type: &str, thumbnail: bool) -> bool {
    let media_type = to_lower(media_type);
    if thumbnail {
        THUMBNAIL_MEDIA.contains(&media_type.as_str())
    } else {
        RENDERED_MEDIA.contains(&media_type.as_str())
    }
}
fn rendered_response_policy(thumbnail: bool) -> MediaPolicy {
    let set = if thumbnail {
        THUMBNAIL_MEDIA
    } else {
        RENDERED_MEDIA
    };
    MediaPolicy {
        require_multipart: false,
        plain_single_media: false,
        frames: false,
        single: set,
        parts: set,
    }
}
fn rendered_accept_header(
    media_types: &[String],
    multipart: bool,
    thumbnail: bool,
) -> Result<String, Error> {
    let default = ["image/jpeg".to_string()];
    let media_types = if media_types.is_empty() {
        &default[..]
    } else {
        media_types
    };
    let mut values: Vec<String> = Vec::with_capacity(media_types.len());
    let mut seen: Vec<String> = Vec::new();
    for (index, raw) in media_types.iter().enumerate() {
        let media_type = match parse_media_type(trim_space(raw)) {
            Ok(parsed)
                if parsed.params.is_empty()
                    && is_rendered_media_type(&parsed.media_type, thumbnail) =>
            {
                to_lower(&parsed.media_type)
            }
            _ => return Err(invalid_render_option("unsupported rendered media type")),
        };
        if seen.contains(&media_type) {
            continue;
        }
        seen.push(media_type.clone());
        let mut value = if multipart {
            format!("multipart/related; type=\"{media_type}\"")
        } else {
            media_type
        };
        if index > 0 {
            let quality = (1.0 - 0.1 * index as f64).max(0.1);
            value.push_str(&format!("; q={quality:.1}"));
        }
        values.push(value);
    }
    Ok(values.join(", "))
}
fn rendered_list_refused(err: &ClientError) -> bool {
    matches!(err, ClientError::Web(e) if e.status_code == 400 || e.status_code == 415)
}
impl RenderTarget {
    fn multiple_representations(&self) -> bool {
        if !self.frames.is_empty() {
            return self.frames.len() > 1;
        }
        trim_space(&self.sop_instance_uid).is_empty()
    }
}
impl Endpoint {
    fn render_resource_url(&self, target: &RenderTarget, resource: &str) -> Result<Url, Error> {
        let study = trim_space(&target.study_instance_uid);
        let series = trim_space(&target.series_instance_uid);
        let instance = trim_space(&target.sop_instance_uid);
        let reference = InstanceRef {
            study_instance_uid: study.to_string(),
            series_instance_uid: series.to_string(),
            sop_instance_uid: instance.to_string(),
        };
        let mut url = if !target.frames.is_empty() {
            self.frames_url(&reference, &target.frames)?
        } else if !instance.is_empty() {
            self.instance_url(&reference)?
        } else if !series.is_empty() {
            self.series_url(study, series)?
        } else {
            self.study_url(study)?
        };
        url.path = join_url_path(&url.path, "", resource);
        Ok(url)
    }
    pub fn rendered_url(
        &self,
        target: &RenderTarget,
        opts: &RenderedOptions,
    ) -> Result<Url, Error> {
        let mut url = self.render_resource_url(target, "rendered")?;
        url.raw_query = rendered_query(opts)?.encode();
        Ok(url)
    }
    pub fn thumbnail_url(
        &self,
        target: &RenderTarget,
        opts: &ThumbnailOptions,
    ) -> Result<Url, Error> {
        let mut url = self.render_resource_url(target, "thumbnail")?;
        let mut query = Values::new();
        if let Some(viewport) = &opts.viewport {
            if viewport.source_x.is_some()
                || viewport.source_y.is_some()
                || viewport.source_width.is_some()
                || viewport.source_height.is_some()
            {
                return Err(invalid_render_option(
                    "thumbnail viewport accepts only width and height",
                ));
            }
            query.set("viewport", &render_viewport_value(viewport)?);
        }
        url.raw_query = query.encode();
        Ok(url)
    }
}
impl Client {
    #[allow(clippy::too_many_arguments)]
    fn stream_rendered(
        &self,
        ctx: &Ctx,
        url: &Url,
        media_types: &[String],
        multipart: bool,
        thumbnail: bool,
        policy: &MediaPolicy,
        handle: PartHandler,
    ) -> Result<(), ClientError> {
        let accept = rendered_accept_header(media_types, multipart, thumbnail)?;
        let mut err = match self.stream_resource(ctx, url, &accept, policy, false, &mut *handle) {
            Err(e) if accept.contains(',') && rendered_list_refused(&e) => e,
            other => return other,
        };
        for media_type in media_types {
            let single =
                rendered_accept_header(std::slice::from_ref(media_type), multipart, thumbnail)?;
            match self.stream_resource(ctx, url, &single, policy, false, &mut *handle) {
                Err(e) if rendered_list_refused(&e) => err = e,
                other => return other,
            }
        }
        Err(err)
    }
    pub fn retrieve_rendered_stream(
        &self,
        ctx: &Ctx,
        target: &RenderTarget,
        opts: &RenderedOptions,
        handle: Option<PartHandler>,
    ) -> Result<(), ClientError> {
        let Some(handle) = handle else {
            return Err(ClientError::Other(
                "WADO-RS rendered stream handler is required".into(),
            ));
        };
        let url = self.endpoint.rendered_url(target, opts)?;
        let multipart = target.multiple_representations();
        let mut policy = rendered_response_policy(false);
        policy.require_multipart = multipart;
        self.stream_rendered(
            ctx,
            &url,
            &opts.media_types,
            multipart,
            false,
            &policy,
            handle,
        )
    }
    pub fn retrieve_rendered(
        &self,
        ctx: &Ctx,
        target: &RenderTarget,
        opts: &RenderedOptions,
    ) -> Result<Vec<RenderedObject>, ClientError> {
        let mut objects = Vec::new();
        self.retrieve_rendered_stream(
            ctx,
            target,
            opts,
            Some(&mut |part| {
                let (data, err) = read_all(part.reader);
                if let Some(e) = err {
                    return Err(part_read_error(e));
                }
                objects.push(RenderedObject {
                    content_type: part.content_type,
                    content_location: part.content_location,
                    data,
                });
                Ok(())
            }),
        )?;
        Ok(objects)
    }
    pub fn retrieve_thumbnail(
        &self,
        ctx: &Ctx,
        target: &RenderTarget,
        opts: &ThumbnailOptions,
    ) -> Result<RenderedObject, ClientError> {
        let url = self.endpoint.thumbnail_url(target, opts)?;
        let mut thumbnail = RenderedObject::default();
        let mut count: i64 = 0;
        let policy = rendered_response_policy(true);
        self.stream_rendered(
            ctx,
            &url,
            &opts.media_types,
            false,
            true,
            &policy,
            &mut |part| {
                count += 1;
                if count > 1 {
                    return Err(ClientError::Read(ReadErr::Decode(
                        ResponseDecodeError::with(K::PartLimit, 1, count),
                    )));
                }
                let (data, err) = read_all(part.reader);
                if let Some(e) = err {
                    return Err(part_read_error(e));
                }
                thumbnail = RenderedObject {
                    content_type: part.content_type,
                    content_location: part.content_location,
                    data,
                };
                Ok(())
            },
        )?;
        Ok(thumbnail)
    }
}
pub const DEFAULT_STORE_BATCH_INSTANCES: i64 = 100;
pub const DEFAULT_STORE_BATCH_BYTES: i64 = 64 << 20;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StoreBatchLimits {
    pub max_instances: i64,
    pub max_bytes: i64,
}
impl StoreBatchLimits {
    fn normalized(mut self) -> StoreBatchLimits {
        if self.max_instances <= 0 {
            self.max_instances = DEFAULT_STORE_BATCH_INSTANCES;
        }
        if self.max_bytes <= 0 {
            self.max_bytes = DEFAULT_STORE_BATCH_BYTES;
        }
        self
    }
}
#[derive(Clone, Debug, Default)]
pub struct StoreBatch {
    pub start: usize,
    pub end: usize,
    pub result: StoreResult,
    pub err: Option<ClientError>,
    pub reduced: bool,
}
pub(crate) type BatchHandler<'h> = &'h mut dyn FnMut(StoreBatch) -> Result<(), ClientError>;
fn store_instance_size(instance: &StoreInstance) -> i64 {
    if instance.size > 0 {
        return instance.size;
    }
    if instance.open.is_none() && instance.reader.is_none() {
        return instance.data.len() as i64;
    }
    0
}
fn next_store_batch_end(
    instances: &[StoreInstance],
    start: usize,
    limits: StoreBatchLimits,
) -> usize {
    let mut end = start;
    let mut bytes: i64 = 0;
    while end < instances.len() && ((end - start) as i64) < limits.max_instances {
        let size = store_instance_size(&instances[end]);
        if end > start && size > limits.max_bytes - bytes {
            break;
        }
        bytes += size;
        end += 1;
    }
    end
}
pub fn plan_store_batches(instances: &[StoreInstance], limits: StoreBatchLimits) -> Vec<usize> {
    let limits = limits.normalized();
    let mut ends = Vec::new();
    let mut start = 0;
    while start < instances.len() {
        start = next_store_batch_end(instances, start, limits);
        ends.push(start);
    }
    ends
}
fn store_batch_bytes(instances: &[StoreInstance]) -> i64 {
    instances.iter().map(store_instance_size).sum()
}
fn store_instances_replayable(instances: &[StoreInstance]) -> bool {
    instances.iter().all(|instance| {
        instance.open.is_some() || (instance.reader.is_none() && !instance.data.is_empty())
    })
}
fn store_payload_index(err: &ClientError) -> Option<usize> {
    match err {
        ClientError::StorePayload(payload) => Some(payload.index),
        ClientError::Web(web) => match web.err.as_ref().map(|c| &c.class) {
            Some(CauseClass::StorePayload(payload)) => Some(payload.index),
            _ => None,
        },
        _ => None,
    }
}
struct StoreBatchUpload<'c, 'h> {
    client: &'c Client,
    limits: StoreBatchLimits,
    on_batch: Option<BatchHandler<'h>>,
}
impl StoreBatchUpload<'_, '_> {
    fn report(&mut self, batch: StoreBatch) -> Result<(), ClientError> {
        match self.on_batch.as_mut() {
            Some(on_batch) => on_batch(batch),
            None => Ok(()),
        }
    }
    fn send(
        &mut self,
        ctx: &Ctx,
        instances: &mut [StoreInstance],
        mut start: usize,
        end: usize,
        reduced: bool,
    ) -> Result<(), ClientError> {
        let context_error = |ctx: &Ctx| {
            ctx.err().map(|kind| {
                ClientError::Read(if matches!(kind, crate::net::ul::Kind::Canceled) {
                    ReadErr::Canceled
                } else {
                    ReadErr::Deadline
                })
            })
        };
        while start < end {
            if let Some(e) = context_error(ctx) {
                return Err(e);
            }
            let (result, err) = self.client.store_instances(ctx, &mut instances[start..end]);
            let requested = &instances[start..end];
            if ctx.err().is_none()
                && let Some(index) = err.as_ref().and_then(store_payload_index)
                && index < requested.len()
                && store_instances_replayable(&requested[..index])
                && store_instances_replayable(&requested[index + 1..])
            {
                let failed = start + index;
                if failed > start {
                    self.send(ctx, instances, start, failed, reduced)?;
                }
                self.report(StoreBatch {
                    start: failed,
                    end: failed + 1,
                    result: StoreResult::default(),
                    err,
                    reduced,
                })?;
                start = failed + 1;
                continue;
            }
            let too_large = matches!(
                &err,
                Some(ClientError::Web(web))
                    if web.kind == ErrorKind::HttpStatus && web.status_code == 413
            );
            if reduced || !too_large || end - start < 2 || !store_instances_replayable(requested) {
                return self.report(StoreBatch {
                    start,
                    end,
                    result,
                    err,
                    reduced,
                });
            }
            let middle = start + (end - start).div_ceil(2);
            let (first, second) = (&instances[start..middle], &instances[middle..end]);
            self.limits.max_instances = self
                .limits
                .max_instances
                .min(first.len().max(second.len()) as i64)
                .max(1);
            let bytes = store_batch_bytes(first).max(store_batch_bytes(second));
            if bytes > 0 {
                self.limits.max_bytes = self.limits.max_bytes.min(bytes);
            }
            self.send(ctx, instances, start, middle, true)?;
            return self.send(ctx, instances, middle, end, true);
        }
        Ok(())
    }
}
impl Client {
    pub fn store_instances_batched(
        &self,
        ctx: &Ctx,
        instances: &mut [StoreInstance],
        limits: StoreBatchLimits,
        on_batch: Option<BatchHandler>,
    ) -> Result<(), ClientError> {
        let mut upload = StoreBatchUpload {
            client: self,
            limits: limits.normalized(),
            on_batch,
        };
        let mut start = 0;
        while start < instances.len() {
            let end = next_store_batch_end(instances, start, upload.limits);
            upload.send(ctx, instances, start, end, false)?;
            start = end;
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ResponseCacheKey {
    pub url: String,
    pub accept: String,
    pub origin_bound: bool,
}
struct CacheEntry {
    key: ResponseCacheKey,
    response: Response,
    redirected_url: String,
    size: i64,
    expires: Option<Instant>,
}
#[derive(Default)]
struct CacheState {
    size: i64,
    order: Vec<CacheEntry>,
    hits: u64,
    misses: u64,
}
pub struct ResponseCache {
    max_bytes: i64,
    ttl: Duration,
    state: Mutex<CacheState>,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ResponseCacheStats {
    pub entries: usize,
    pub bytes: i64,
    pub hits: u64,
    pub misses: u64,
}
impl ResponseCache {
    pub fn new(max_bytes: i64, ttl: Nanos) -> Option<Arc<ResponseCache>> {
        if max_bytes <= 0 {
            return None;
        }
        Some(Arc::new(ResponseCache {
            max_bytes,
            ttl: Duration::from_nanos(ttl.0.max(0) as u64),
            state: Mutex::new(CacheState::default()),
        }))
    }
    fn lock(&self) -> std::sync::MutexGuard<'_, CacheState> {
        match self.state.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }
    pub fn stats(&self) -> ResponseCacheStats {
        let state = self.lock();
        ResponseCacheStats {
            entries: state.order.len(),
            bytes: state.size,
            hits: state.hits,
            misses: state.misses,
        }
    }
    pub fn purge(&self) {
        let mut state = self.lock();
        state.order.clear();
        state.size = 0;
    }
    pub(super) fn get(&self, key: &ResponseCacheKey) -> Option<(Response, String)> {
        let mut state = self.lock();
        let Some(index) = state.order.iter().position(|entry| entry.key == *key) else {
            state.misses += 1;
            return None;
        };
        let entry = state.order.remove(index);
        if matches!(entry.expires, Some(expires) if Instant::now() >= expires) {
            state.size -= entry.size;
            state.misses += 1;
            return None;
        }
        let found = (entry.response.clone(), entry.redirected_url.clone());
        state.order.insert(0, entry);
        state.hits += 1;
        Some(found)
    }
    pub(super) fn put(&self, key: ResponseCacheKey, response: &Response, redirected_url: &str) {
        if !(200..300).contains(&response.status_code) || response.status_code == 206 {
            return;
        }
        if response
            .header
            .values("Cache-Control")
            .iter()
            .any(|value| to_lower(value).contains("no-store"))
        {
            return;
        }
        let mut size = response.body.len()
            + key.url.len()
            + key.accept.len()
            + response.url.len()
            + redirected_url.len();
        for (name, values) in &response.header.0 {
            size += name.len() + values.iter().map(String::len).sum::<usize>();
        }
        let size = size as i64;
        if size > self.max_bytes {
            return;
        }
        let mut state = self.lock();
        let expires = (!self.ttl.is_zero()).then(|| Instant::now() + self.ttl);
        if let Some(index) = state.order.iter().position(|entry| entry.key == key) {
            let existing = state.order.remove(index);
            state.size -= existing.size;
        }
        state.order.insert(
            0,
            CacheEntry {
                key,
                response: response.clone(),
                redirected_url: redirected_url.to_string(),
                size,
                expires,
            },
        );
        state.size += size;
        while state.size > self.max_bytes {
            match state.order.pop() {
                Some(evicted) => state.size -= evicted.size,
                None => break,
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn content_range_matching() {
        let one = |v: &str| vec![v.to_string()];
        assert!(matches_bulk_data_content_range(&one("bytes 2-5/10"), 2, 5));
        assert!(matches_bulk_data_content_range(&one("BYTES 2-5/*"), 2, 5));
        assert!(!matches_bulk_data_content_range(&one("bytes 2-5/5"), 2, 5));
        assert!(!matches_bulk_data_content_range(&one("bytes +2-5/9"), 2, 5));
        assert!(!matches_bulk_data_content_range(&[], 2, 5));
    }
    #[test]
    fn wado_uri_path() {
        assert_eq!(normalize_wado_uri_path("/wado/").as_deref(), Ok("wado"));
        assert!(normalize_wado_uri_path("a//b").is_err());
        assert!(normalize_wado_uri_path("a?b").is_err());
    }
    #[test]
    fn viewport_values() {
        let viewport = RenderViewport {
            width: 4,
            height: 2,
            source_y: Some(1.5),
            ..RenderViewport::default()
        };
        assert_eq!(render_viewport_value(&viewport).as_deref(), Ok("4,2,,1.5"));
    }
}
