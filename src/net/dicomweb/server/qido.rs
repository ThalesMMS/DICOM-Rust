use super::super::mimeheader::Header;
use super::super::types::InstanceRef;
use super::super::urlrules;
use super::http::{Request, ResponseWriter};
use super::media::{
    DICOM_JSON, DICOM_XML, MetadataRepresentation, format_media_type,
    select_metadata_representation,
};
use super::spool::{JsonArraySpool, MultipartWriter, ResponseSpool, copy_bounded};
use super::{
    Inner, Operation, ServerError, ServerWriter, check_read_lease, classify_backend_error, ctx_err,
    resource_parts,
};
use crate::dicomjson::dataset_stream::{
    DatasetEncodeError, DatasetOptions, encode_dataset, marshal_dataset,
};
use crate::dicomjson::jsonwire::{self, JAny, RawElement};
use crate::dicomjson::{self, Dataset};
use crate::dicomxml;
use crate::net::ul::Ctx;
use crate::object::Vr;
use std::io::Read;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchLevel {
    Study,
    Series,
    Instance,
    Workitem,
}
impl SearchLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            SearchLevel::Study => "study",
            SearchLevel::Series => "series",
            SearchLevel::Instance => "instance",
            SearchLevel::Workitem => "workitem",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchFilter {
    pub key: String,
    pub values: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchRequest {
    pub level: SearchLevel,
    pub study_instance_uid: String,
    pub series_instance_uid: String,
    pub filters: Vec<SearchFilter>,
    pub include_fields: Vec<String>,
    pub limit: i64,
    pub limit_set: bool,
    pub maximum_results: i64,
    pub offset: i64,
    pub fuzzy_matching: bool,
    pub empty_value_matching: bool,
    pub multiple_value_matching: bool,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SearchResult {
    pub remaining: Option<i64>,
    pub fuzzy_matching_applied: bool,
    pub empty_value_matching_applied: bool,
    pub multiple_value_matching_applied: bool,
}
pub type DatasetYield<'a> = dyn FnMut(Dataset) -> Result<(), ServerError> + 'a;
pub trait SearchBackend: Send + Sync {
    fn search(
        &self,
        ctx: &Ctx,
        request: &SearchRequest,
        yield_dataset: &mut DatasetYield<'_>,
    ) -> (SearchResult, Result<(), ServerError>);
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MetadataLevel {
    Study,
    Series,
    Instance,
}
impl MetadataLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            MetadataLevel::Study => "study",
            MetadataLevel::Series => "series",
            MetadataLevel::Instance => "instance",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MetadataRequest {
    pub level: MetadataLevel,
    pub study_instance_uid: String,
    pub series_instance_uid: String,
    pub sop_instance_uid: String,
}
pub trait MetadataBackend: Send + Sync {
    fn metadata(
        &self,
        ctx: &Ctx,
        request: &MetadataRequest,
        yield_dataset: &mut DatasetYield<'_>,
    ) -> Result<(), ServerError>;
}
fn recover<T>(f: impl FnOnce() -> Result<T, ServerError>) -> Result<T, ServerError> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).unwrap_or(Err(ServerError::Backend))
}
type Served = Result<usize, (usize, ServerError)>;
fn fail(err: ServerError) -> Served {
    Err((0, err))
}
pub(crate) struct DatasetSpool {
    representation: MetadataRepresentation,
    json: Option<JsonArraySpool>,
    xml: Option<ResponseSpool>,
    xml_writer: MultipartWriter,
    pub(crate) content_type: String,
    pub(crate) count: usize,
}
impl DatasetSpool {
    pub(crate) fn new(
        directory: &str,
        maximum: i64,
        representation: MetadataRepresentation,
    ) -> Result<DatasetSpool, ServerError> {
        let xml_writer = MultipartWriter::new();
        let (json, xml, content_type) = match representation {
            MetadataRepresentation::Json => (
                Some(JsonArraySpool::new(directory, maximum)?),
                None,
                DICOM_JSON.to_string(),
            ),
            MetadataRepresentation::Xml => {
                let ct = format_media_type(
                    "multipart/related",
                    &[("type", DICOM_XML), ("boundary", xml_writer.boundary())],
                );
                (None, Some(ResponseSpool::new(directory, maximum)?), ct)
            }
        };
        Ok(DatasetSpool {
            representation,
            json,
            xml,
            xml_writer,
            content_type,
            count: 0,
        })
    }
    pub(crate) fn append(
        &mut self,
        ctx: &Ctx,
        server: &Inner,
        dataset: &Dataset,
        content_location: &str,
    ) -> Result<(), ServerError> {
        match self.representation {
            MetadataRepresentation::Json => {
                let encoded = server.write_response_dataset(ctx, dataset);
                self.json
                    .as_mut()
                    .ok_or(ServerError::Backend)?
                    .append_encoded(encoded)?;
            }
            MetadataRepresentation::Xml => {
                let obj = server.response_dataset_object(ctx, dataset)?;
                let mut header = Header::new();
                header.set("Content-Type", DICOM_XML);
                if !content_location.is_empty() {
                    header.set("Content-Location", content_location);
                }
                let spool = self.xml.as_mut().ok_or(ServerError::Backend)?;
                self.xml_writer
                    .create_part(&header, &mut |d: &[u8]| spool.write(d))?;
                let l = &server.limits;
                let mut opts = dicomxml::Options {
                    omit_group_length: true,
                    limits: dicomxml::Limits {
                        max_xml_bytes: l.max_response_bytes,
                        max_value_bytes: l.max_json_value_bytes,
                        max_inline_binary_bytes: l.max_json_value_bytes,
                        max_total_binary_bytes: l.max_response_bytes,
                        max_sequence_depth: l.max_json_depth,
                        max_elements: l.max_json_values,
                        max_sequence_items: l.max_json_values,
                    },
                    ..dicomxml::Options::default()
                };
                let xml = dicomxml::marshal_text(Some(&obj), &mut opts)
                    .map_err(|e| classify_metadata_conversion_error(&e))?;
                spool.write(&xml)?;
            }
        }
        self.count += 1;
        Ok(())
    }
    pub(crate) fn finalize(
        &mut self,
        server: &Inner,
        empty_xml_root: bool,
    ) -> Result<(), ServerError> {
        match self.representation {
            MetadataRepresentation::Json => {
                self.json.as_mut().ok_or(ServerError::Backend)?.finalize()
            }
            MetadataRepresentation::Xml => {
                let spool = self.xml.as_mut().ok_or(ServerError::Backend)?;
                if self.count == 0 && empty_xml_root {
                    let mut header = Header::new();
                    header.set("Content-Type", DICOM_XML);
                    self.xml_writer
                        .create_part(&header, &mut |d: &[u8]| spool.write(d))?;
                    let mut opts = dicomxml::Options {
                        omit_group_length: true,
                        limits: dicomxml::Limits {
                            max_xml_bytes: server.limits.max_response_bytes,
                            ..dicomxml::Limits::default()
                        },
                        ..dicomxml::Options::default()
                    };
                    let xml = dicomxml::marshal_text(None, &mut opts)
                        .map_err(|e| classify_metadata_conversion_error(&e))?;
                    spool.write(&xml)?;
                }
                self.xml_writer.close(&mut |d: &[u8]| spool.write(d))?;
                spool.rewind()
            }
        }
    }
    pub(crate) fn reader(&mut self) -> Option<&mut std::fs::File> {
        if let Some(j) = self.json.as_mut() {
            return j.reader();
        }
        self.xml.as_mut().and_then(ResponseSpool::reader)
    }
    pub(crate) fn close(&mut self) -> Result<(), ServerError> {
        if let Some(j) = self.json.as_mut() {
            return j.close();
        }
        if let Some(x) = self.xml.as_mut() {
            return x.close();
        }
        Ok(())
    }
}
pub(crate) fn classify_metadata_conversion_error(text: &str) -> ServerError {
    const LIMITS: [&str; 15] = [
        dicomjson::ERR_MAX_JSON_BYTES,
        dicomjson::ERR_MAX_INLINE_BINARY_BYTES,
        dicomjson::ERR_MAX_TOTAL_BINARY_BYTES,
        dicomjson::ERR_MAX_SEQUENCE_DEPTH,
        dicomjson::ERR_MAX_ELEMENTS,
        dicomjson::ERR_MAX_SEQUENCE_ITEMS,
        dicomjson::dataset_stream::ERR_MAX_VALUES,
        dicomxml::ERR_MAX_XML_BYTES,
        dicomxml::ERR_MAX_VALUE_BYTES,
        dicomxml::ERR_MAX_INLINE_BYTES,
        dicomxml::ERR_MAX_BINARY_BYTES,
        dicomxml::ERR_MAX_DEPTH,
        dicomxml::ERR_MAX_ELEMENTS,
        dicomxml::ERR_MAX_ITEMS,
        "dicomweb: resource limit exceeded",
    ];
    if LIMITS.iter().any(|l| text.contains(l)) {
        ServerError::ResourceLimit
    } else {
        ServerError::Backend
    }
}
fn classify_encode_error(err: DatasetEncodeError<ServerError>) -> ServerError {
    match err {
        DatasetEncodeError::Validate(e) => {
            if e.is(&ServerError::Canceled) || e.is(&ServerError::DeadlineExceeded) {
                e
            } else if e.is(&ServerError::ResourceLimit) {
                ServerError::ResourceLimit
            } else {
                ServerError::Backend
            }
        }
        DatasetEncodeError::Encode(text) => classify_metadata_conversion_error(&text),
    }
}
#[derive(Default)]
struct Validation {
    values: i64,
    estimated: i64,
}
fn response_json_scalar_size(value: &JAny, floats: bool) -> Option<i64> {
    match value {
        JAny::Null => Some(4),
        JAny::Str(s) => Some(s.len() as i64),
        JAny::Number(n) => Some(if floats { 32 } else { n.len() as i64 }),
        JAny::Obj(entries) => {
            let mut total = 0i64;
            for (key, nested) in entries {
                if !matches!(key.as_str(), "Alphabetic" | "Ideographic" | "Phonetic") {
                    return None;
                }
                let JAny::Str(text) = nested else {
                    return None;
                };
                total += (key.len() + text.len()) as i64;
            }
            Some(total)
        }
        _ => None,
    }
}
pub(crate) fn bulk_data_token(
    path: &str,
    raw_path: &str,
    prefix: &str,
) -> Result<String, ServerError> {
    if !raw_path.is_empty() || !path.starts_with(prefix) {
        return Err(ServerError::InvalidRequest);
    }
    let token = &path[prefix.len()..];
    if token.is_empty() {
        return Err(ServerError::InvalidRequest);
    }
    for segment in token.split('/') {
        if segment.is_empty() || segment == "." || segment == ".." {
            return Err(ServerError::InvalidRequest);
        }
    }
    Ok(token.to_string())
}
fn strict_base64_ok(s: &str) -> bool {
    dicomjson::sizedparse::base64_decode(s.as_bytes(), true).is_ok()
}
impl Inner {
    fn check_element_common(
        &self,
        tag: &str,
        element: &RawElement,
        state: &mut Validation,
        values_first: bool,
    ) -> Result<Vr, ServerError> {
        let l = &self.limits;
        if values_first {
            state.values += 1;
            state.estimated += (tag.len()
                + element.vr.len()
                + element.inline_binary.len()
                + element.bulk_data_uri.len()
                + 32) as i64;
            if state.values > l.max_json_values
                || element.inline_binary.len() as i64 > l.max_json_value_bytes
                || element.bulk_data_uri.len() as i64 > l.max_json_value_bytes
                || state.estimated > l.max_response_bytes
            {
                return Err(ServerError::ResourceLimit);
            }
        }
        if tag.len() != 8 || tag != crate::textfmt::to_upper(tag) {
            return Err(ServerError::Backend);
        }
        match dicomjson::parse_core_tag(tag) {
            Ok(t) if t.group != 0x0002 => {}
            _ => return Err(ServerError::Backend),
        }
        Vr::parse(element.vr.as_bytes()).ok_or(ServerError::Backend)
    }
    fn check_bulk_uri(&self, uri: &str) -> Result<(), ServerError> {
        let parsed = urlrules::parse(uri).map_err(|_| ServerError::Backend)?;
        if !parsed.scheme.is_empty()
            || !parsed.host.is_empty()
            || !parsed.raw_query.is_empty()
            || !parsed.fragment.is_empty()
        {
            return Err(ServerError::Backend);
        }
        bulk_data_token(
            &parsed.path,
            &parsed.raw_path,
            &self.service_path("/bulkdata/"),
        )
        .map(|_| ())
        .map_err(|_| ServerError::Backend)
    }
    fn validate_response_element(
        &self,
        ctx: &Ctx,
        tag: &str,
        element: &RawElement,
        depth: i64,
        state: &mut Validation,
    ) -> Result<(), ServerError> {
        ctx_err(ctx)?;
        if depth > self.limits.max_json_depth {
            return Err(ServerError::ResourceLimit);
        }
        let vr = self.check_element_common(tag, element, state, true)?;
        if !element.inline_binary.is_empty() && !strict_base64_ok(&element.inline_binary) {
            return Err(ServerError::Backend);
        }
        if !element.bulk_data_uri.is_empty() {
            self.check_bulk_uri(&element.bulk_data_uri)?;
        }
        for value in &element.value {
            state.values += 1;
            if state.values > self.limits.max_json_values {
                return Err(ServerError::ResourceLimit);
            }
            if vr == Vr::SQ {
                if !matches!(value, JAny::Obj(_)) {
                    return Err(ServerError::Backend);
                }
                continue;
            }
            let size = response_json_scalar_size(value, false).ok_or(ServerError::Backend)?;
            if size > self.limits.max_json_value_bytes {
                return Err(ServerError::ResourceLimit);
            }
            state.estimated += size * 6 + 8;
            if state.estimated > self.limits.max_response_bytes {
                return Err(ServerError::ResourceLimit);
            }
        }
        Ok(())
    }
    pub(crate) fn write_response_dataset(
        &self,
        ctx: &Ctx,
        dataset: &Dataset,
    ) -> Result<Vec<u8>, ServerError> {
        let l = self.limits;
        let mut state = Validation::default();
        let mut opts: DatasetOptions<'_, ServerError> = DatasetOptions {
            max_values: l.max_json_values,
            limits: dicomjson::Limits {
                max_json_bytes: l.max_response_bytes,
                max_inline_binary_bytes: l.max_json_value_bytes,
                max_total_binary_bytes: l.max_response_bytes,
                max_sequence_depth: l.max_json_depth,
                ..dicomjson::Limits::default()
            },
            validate_element: Some(Box::new(|tag: &str, element: &RawElement, depth: i64| {
                self.validate_response_element(ctx, tag, element, depth + 1, &mut state)
            })),
        };
        encode_dataset(Some(dataset), &mut opts).map_err(classify_encode_error)
    }
    fn validate_response_dataset_at_depth(
        &self,
        ctx: &Ctx,
        dataset: &Dataset,
        depth: i64,
        state: &mut Validation,
    ) -> Result<(), ServerError> {
        if depth > self.limits.max_json_depth {
            return Err(ServerError::ResourceLimit);
        }
        let floats = depth > 1;
        for (tag, element) in dataset {
            ctx_err(ctx)?;
            let vr = self.check_element_common(tag, element, state, true)?;
            let representations = [
                !element.value.is_empty(),
                !element.inline_binary.is_empty(),
                !element.bulk_data_uri.is_empty(),
            ]
            .iter()
            .filter(|b| **b)
            .count();
            if !element.inline_binary.is_empty() && !strict_base64_ok(&element.inline_binary) {
                return Err(ServerError::Backend);
            }
            if representations > 1 {
                return Err(ServerError::Backend);
            }
            if !element.bulk_data_uri.is_empty() {
                self.check_bulk_uri(&element.bulk_data_uri)?;
            }
            if vr == Vr::SQ {
                if !element.inline_binary.is_empty() || !element.bulk_data_uri.is_empty() {
                    return Err(ServerError::Backend);
                }
                for value in &element.value {
                    state.values += 1;
                    if state.values > self.limits.max_json_values {
                        return Err(ServerError::ResourceLimit);
                    }
                    let JAny::Obj(item) = value else {
                        return Err(ServerError::Backend);
                    };
                    let nested = nested_dataset(item)?;
                    self.validate_response_dataset_at_depth(ctx, &nested, depth + 1, state)?;
                }
                continue;
            }
            for value in &element.value {
                state.values += 1;
                if state.values > self.limits.max_json_values {
                    return Err(ServerError::ResourceLimit);
                }
                let size = response_json_scalar_size(value, floats).ok_or(ServerError::Backend)?;
                if size > self.limits.max_json_value_bytes {
                    return Err(ServerError::ResourceLimit);
                }
                state.estimated += size * 6 + 8;
                if state.estimated > self.limits.max_response_bytes {
                    return Err(ServerError::ResourceLimit);
                }
            }
        }
        Ok(())
    }
    fn response_dataset_object(
        &self,
        ctx: &Ctx,
        dataset: &Dataset,
    ) -> Result<crate::object::Object, ServerError> {
        let mut state = Validation::default();
        self.validate_response_dataset_at_depth(ctx, dataset, 1, &mut state)?;
        let data = marshal_dataset(dataset);
        if data.len() as i64 > self.limits.max_response_bytes {
            return Err(ServerError::ResourceLimit);
        }
        let l = self.limits;
        let opts = dicomjson::UnmarshalOptions {
            limits: dicomjson::Limits {
                max_json_bytes: l.max_response_bytes,
                max_inline_binary_bytes: l.max_json_value_bytes,
                max_total_binary_bytes: l.max_response_bytes,
                max_sequence_depth: l.max_json_depth,
                max_elements: l.max_json_values,
                max_sequence_items: l.max_json_values,
            },
            ..dicomjson::UnmarshalOptions::default()
        };
        dicomjson::unmarshal_text(data.as_bytes(), &opts)
            .map_err(|e| classify_metadata_conversion_error(&e))
    }
    fn metadata_content_location(
        &self,
        request: &MetadataRequest,
        dataset: &Dataset,
    ) -> Result<String, ServerError> {
        let r = InstanceRef {
            study_instance_uid: dicomjson::element_string(dataset, "0020000D"),
            series_instance_uid: dicomjson::element_string(dataset, "0020000E"),
            sop_instance_uid: dicomjson::element_string(dataset, "00080018"),
        };
        if r.study_instance_uid != request.study_instance_uid
            || (!request.series_instance_uid.is_empty()
                && r.series_instance_uid != request.series_instance_uid)
            || (!request.sop_instance_uid.is_empty()
                && r.sop_instance_uid != request.sop_instance_uid)
        {
            return Err(ServerError::Backend);
        }
        let location = self
            .instance_location(&r)
            .map_err(|_| ServerError::Backend)?;
        Ok(format!("{location}/metadata"))
    }
    fn send_spool(
        &self,
        w: &mut ServerWriter<'_>,
        r: &Request,
        spool: &mut DatasetSpool,
    ) -> Served {
        w.header().set("Content-Type", &spool.content_type);
        let count = spool.count;
        let ctx = r.context().clone();
        let max = self.limits.max_response_bytes;
        let copied = match spool.reader() {
            Some(reader) => {
                let reader: &mut dyn Read = reader;
                copy_bounded(&ctx, &mut |d: &[u8]| w.write(d), reader, max).1
            }
            None => Err(ServerError::Backend),
        };
        let close = spool.close();
        match (copied, close) {
            (Err(e), _) | (Ok(()), Err(e)) => Err((count, e)),
            _ => Ok(count),
        }
    }
    pub(crate) fn serve_search(
        &self,
        w: &mut ServerWriter<'_>,
        r: &mut Request,
        operation: Operation,
    ) -> Served {
        let Some(backend) = self
            .options
            .backend
            .as_ref()
            .and_then(|b| b.search_backend())
        else {
            return fail(ServerError::status(
                501,
                "not_implemented",
                ServerError::Unsupported,
            ));
        };
        let Some(representation) = select_metadata_representation(&super::request_accept_header(r))
        else {
            return fail(ServerError::status(
                406,
                "not_acceptable",
                ServerError::Unsupported,
            ));
        };
        w.header().set("Vary", "Accept");
        w.header().set("X-Content-Type-Options", "nosniff");
        let request = self
            .parse_search_request(r, operation)
            .map_err(|e| (0, e))?;
        self.run_search(w, r, backend, request, representation)
    }
    pub(crate) fn run_search(
        &self,
        w: &mut ServerWriter<'_>,
        r: &Request,
        backend: &dyn SearchBackend,
        mut request: SearchRequest,
        representation: MetadataRepresentation,
    ) -> Served {
        w.header().set("Vary", "Accept");
        w.header().set("X-Content-Type-Options", "nosniff");
        request.maximum_results = self.limits.max_results;
        if request.limit_set && request.limit < request.maximum_results {
            request.maximum_results = request.limit;
        }
        let mut spool = DatasetSpool::new(
            &self.options.spool_directory,
            self.limits.max_response_bytes,
            representation,
        )
        .map_err(|_| (0, ServerError::Backend))?;
        let ctx = r.context().clone();
        let mut truncated = false;
        let maximum = request.maximum_results;
        let result = {
            let spool_ref = &mut spool;
            let truncated_ref = &mut truncated;
            let mut yield_dataset = |dataset: Dataset| -> Result<(), ServerError> {
                check_read_lease(&ctx)?;
                if spool_ref.count as i64 >= maximum {
                    *truncated_ref = true;
                    return Err(ServerError::Other(LIMIT_REACHED.into()));
                }
                spool_ref.append(&ctx, self, &dataset, "")
            };
            match check_read_lease(&ctx) {
                Err(e) => (SearchResult::default(), Err(e)),
                Ok(()) => std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    backend.search(&ctx, &request, &mut yield_dataset)
                }))
                .unwrap_or((SearchResult::default(), Err(ServerError::Backend))),
            }
        };
        let result = match result {
            (res, Ok(())) => res,
            (res, Err(ServerError::Other(t))) if t == LIMIT_REACHED => res,
            (_, Err(e)) => return fail(classify_backend_error(e)),
        };
        if result.remaining.is_some_and(|n| n < 0) {
            return fail(ServerError::Backend);
        }
        if spool.count as i64 == request.maximum_results && result.remaining.is_none() {
            return fail(ServerError::Backend);
        }
        if truncated && result.remaining.is_none_or(|n| n <= 0) {
            return fail(ServerError::Backend);
        }
        let uri = self.service_uri(r);
        if request.fuzzy_matching && !result.fuzzy_matching_applied {
            w.header().add("Warning", &format!("299 {uri}: The fuzzymatching parameter is not supported. Only literal matching has been performed."));
        }
        if request.empty_value_matching && !result.empty_value_matching_applied {
            w.header().add("Warning", &format!("299 {uri}: The emptyvaluematching parameter is not supported. Empty Value Matching has not been performed."));
        }
        if request.multiple_value_matching && !result.multiple_value_matching_applied {
            w.header().add("Warning", &format!("299 {uri}: The multiplevaluematching parameter is not supported. Multiple Value Matching has not been performed."));
        }
        if let Some(n) = result.remaining
            && n > 0
        {
            w.header().add(
                "Warning",
                &format!("299 {uri}: There are {n} additional results that can be requested"),
            );
        }
        spool.finalize(self, true).map_err(|e| (0, e))?;
        self.send_spool(w, r, &mut spool)
    }
    pub(crate) fn parse_search_request(
        &self,
        r: &Request,
        operation: Operation,
    ) -> Result<SearchRequest, ServerError> {
        let parts = resource_parts(&r.url.path);
        let mut request = SearchRequest {
            level: match operation {
                Operation::SearchSeries => SearchLevel::Series,
                Operation::SearchInstances => SearchLevel::Instance,
                _ => SearchLevel::Study,
            },
            study_instance_uid: String::new(),
            series_instance_uid: String::new(),
            filters: Vec::new(),
            include_fields: Vec::new(),
            limit: 0,
            limit_set: false,
            maximum_results: 0,
            offset: 0,
            fuzzy_matching: false,
            empty_value_matching: false,
            multiple_value_matching: false,
        };
        if parts.len() >= 2 && parts[0] == "studies" {
            request.study_instance_uid = parts[1].clone();
            self.validate_uid(&request.study_instance_uid)?;
        }
        if parts.len() >= 4 && parts[2] == "series" {
            request.series_instance_uid = parts[3].clone();
            self.validate_uid(&request.series_instance_uid)?;
        }
        let (query, err) = urlrules::parse_query(&r.url.raw_query);
        if err.is_some() {
            return Err(ServerError::InvalidRequest);
        }
        let fields: usize = query.0.iter().map(|(_, v)| v.len()).sum();
        if fields as i64 > self.limits.max_query_fields {
            return Err(ServerError::ResourceLimit);
        }
        let mut keys: Vec<&(String, Vec<String>)> = query.0.iter().collect();
        keys.sort_by(|a, b| a.0.cmp(&b.0));
        for (key, values) in keys {
            if values
                .iter()
                .any(|v| v.len() as i64 > self.limits.max_query_value_bytes)
            {
                return Err(ServerError::ResourceLimit);
            }
            match key.as_str() {
                "limit" => {
                    request.limit = single_non_negative_integer(values)?;
                    request.limit_set = true;
                }
                "offset" => {
                    let value = single_non_negative_integer(values)?;
                    if value > self.limits.max_offset {
                        return Err(ServerError::ResourceLimit);
                    }
                    request.offset = value;
                }
                "fuzzymatching" => request.fuzzy_matching = single_boolean(values)?,
                "emptyvaluematching" => request.empty_value_matching = single_boolean(values)?,
                "multiplevaluematching" => {
                    request.multiple_value_matching = single_boolean(values)?
                }
                "includefield" => {
                    for value in values {
                        for field in value.split(',') {
                            if field.is_empty()
                                || (field != "all" && !valid_attribute_identifier(field))
                            {
                                return Err(ServerError::InvalidRequest);
                            }
                            request.include_fields.push(field.to_string());
                        }
                    }
                }
                _ => {
                    if !valid_attribute_identifier(key) || values.len() != 1 {
                        return Err(ServerError::InvalidRequest);
                    }
                    request.filters.push(SearchFilter {
                        key: key.clone(),
                        values: values.clone(),
                    });
                }
            }
        }
        let mut all = false;
        for field in &request.include_fields {
            if field == "all" {
                if all || request.include_fields.len() != 1 {
                    return Err(ServerError::InvalidRequest);
                }
                all = true;
            }
        }
        Ok(request)
    }
    pub(crate) fn serve_metadata(&self, w: &mut ServerWriter<'_>, r: &mut Request) -> Served {
        let Some(backend) = self
            .options
            .backend
            .as_ref()
            .and_then(|b| b.metadata_backend())
        else {
            return fail(ServerError::status(
                501,
                "not_implemented",
                ServerError::Unsupported,
            ));
        };
        let Some(representation) = select_metadata_representation(&super::request_accept_header(r))
        else {
            return fail(ServerError::status(
                406,
                "not_acceptable",
                ServerError::Unsupported,
            ));
        };
        w.header().set("Vary", "Accept");
        w.header().set("X-Content-Type-Options", "nosniff");
        let request = self.metadata_request(&r.url.path).map_err(|e| (0, e))?;
        let mut spool = DatasetSpool::new(
            &self.options.spool_directory,
            self.limits.max_response_bytes,
            representation,
        )
        .map_err(|_| (0, ServerError::Backend))?;
        let ctx = r.context().clone();
        let result = {
            let spool_ref = &mut spool;
            let request_ref = &request;
            let mut yield_dataset = |dataset: Dataset| -> Result<(), ServerError> {
                check_read_lease(&ctx)?;
                if spool_ref.count as i64 >= self.limits.max_results {
                    return Err(ServerError::ResourceLimit);
                }
                let mut location = String::new();
                if representation == MetadataRepresentation::Xml {
                    location = self.metadata_content_location(request_ref, &dataset)?;
                }
                spool_ref.append(&ctx, self, &dataset, &location)
            };
            check_read_lease(&ctx)
                .and_then(|()| recover(|| backend.metadata(&ctx, &request, &mut yield_dataset)))
        };
        if let Err(e) = result {
            return fail(classify_backend_error(e));
        }
        spool.finalize(self, false).map_err(|e| (0, e))?;
        self.send_spool(w, r, &mut spool)
    }
    fn metadata_request(&self, path: &str) -> Result<MetadataRequest, ServerError> {
        let parts = resource_parts(path);
        let mut request = MetadataRequest {
            level: MetadataLevel::Study,
            study_instance_uid: String::new(),
            series_instance_uid: String::new(),
            sop_instance_uid: String::new(),
        };
        if parts.len() >= 2 {
            request.study_instance_uid = parts[1].clone();
        }
        if parts.len() >= 4 {
            request.level = MetadataLevel::Series;
            request.series_instance_uid = parts[3].clone();
        }
        if parts.len() >= 6 {
            request.level = MetadataLevel::Instance;
            request.sop_instance_uid = parts[5].clone();
        }
        for uid in [
            &request.study_instance_uid,
            &request.series_instance_uid,
            &request.sop_instance_uid,
        ] {
            if !uid.is_empty() {
                self.validate_uid(uid)?;
            }
        }
        Ok(request)
    }
}
const LIMIT_REACHED: &str = "dicomweb: search response limit reached";
fn nested_dataset(item: &[(String, JAny)]) -> Result<Dataset, ServerError> {
    let mut out = Dataset::new();
    for (key, value) in item {
        let mut raw = String::new();
        dicomjson::dataset_stream::marshal_any(value, &mut raw);
        let element = jsonwire::as_element(raw.as_bytes()).map_err(|_| ServerError::Backend)?;
        out.push((key.clone(), element));
    }
    Ok(out)
}
pub(crate) fn valid_attribute_identifier(value: &str) -> bool {
    if value.len() == 8
        && let Ok(tag) = dicomjson::parse_core_tag(value)
        && tag.group != 0x0002
        && tag.group % 2 == 0
    {
        return true;
    }
    matches!(crate::object::dict::by_keyword(value), Some((_, keyword)) if keyword == value)
}
fn single_non_negative_integer(values: &[String]) -> Result<i64, ServerError> {
    if values.len() != 1 {
        return Err(ServerError::InvalidRequest);
    }
    match dicomjson::sizedparse::parse_int(&values[0], 63) {
        Ok(v) if v >= 0 => Ok(v),
        _ => Err(ServerError::InvalidRequest),
    }
}
fn single_boolean(values: &[String]) -> Result<bool, ServerError> {
    if values.len() != 1 {
        return Err(ServerError::InvalidRequest);
    }
    match values[0].as_str() {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(ServerError::InvalidRequest),
    }
}
