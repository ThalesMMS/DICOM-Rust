use super::client::{
    ACCEPT_DICOM_JSON, BodyLimitReader, Client, ClientError, OpenPayload, Payload, RequestOptions,
    decode_error, random_boundary, response_from_http, status_error,
};
use super::clientx::{ACCEPT_BULK_DATA, BULK_DATA_POLICY, PartHandler};
use super::mimeheader::trim_space;
use super::multipart::{ByteRead, ReadErr};
use super::response::{self, Response, StoreResult};
use super::stowprofile::{
    STORE_DOCUMENT_TAG, compressed_store_content_type, store_metadata_frame_count,
    store_whole_profile, validate_store_whole_metadata,
};
use super::types::{
    Cause, CauseClass, METADATA_MEDIA_TYPE_DICOM_JSON, METADATA_MEDIA_TYPE_DICOM_XML,
    ResponseDecodeError, ResponseDecodeErrorKind as K,
};
use super::urlrules::Url;
use crate::net::ul::{Ctx, Kind};
use crate::object::{Element, Object, Tag, Value};
use crate::textfmt::quote;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
#[derive(Default)]
pub struct StoreBulkData {
    pub uri: String,
    pub content_type: String,
    pub frame_number: i64,
    pub data: Vec<u8>,
    pub reader: Option<Payload>,
    pub open: Option<OpenPayload>,
}
#[derive(Default)]
pub struct StoreMetadataRequest {
    pub media_type: String,
    pub instances: Vec<Option<Object>>,
    pub bulk_data: Vec<StoreBulkData>,
}
fn other(text: impl Into<String>) -> ClientError {
    ClientError::Other(text.into())
}
fn store_bulk_content_location(bulk: &StoreBulkData) -> String {
    if bulk.frame_number > 0 {
        format!("{}/frames/{}", bulk.uri, bulk.frame_number)
    } else {
        bulk.uri.clone()
    }
}
fn bulk_media(content_type: &str) -> &str {
    if content_type.is_empty() {
        "application/octet-stream"
    } else {
        content_type
    }
}
fn encode_store_metadata(
    media_type: &str,
    instances: &[Option<Object>],
) -> Result<Vec<Vec<u8>>, ClientError> {
    let mut encoded = Vec::with_capacity(instances.len());
    for (index, instance) in instances.iter().enumerate() {
        let Some(instance) = instance else {
            return Err(other(format!("STOW-RS metadata instance {index} is nil")));
        };
        if instance.elements().iter().any(|e| e.tag().group == 0x0002) {
            return Err(other(format!(
                "STOW-RS metadata instance {index} contains File Meta Information"
            )));
        }
        let data = if media_type == METADATA_MEDIA_TYPE_DICOM_XML {
            let mut opts = crate::dicomxml::Options::defaults();
            opts.pretty = false;
            crate::dicomxml::marshal_text(Some(instance), &mut opts)
        } else {
            let mut opts = crate::dicomjson::Options::defaults();
            opts.pretty = false;
            crate::dicomjson::marshal_text(Some(instance), &mut opts)
        }
        .map_err(|e| other(format!("encode STOW-RS metadata instance {index}: {e}")))?;
        encoded.push(data);
    }
    if media_type == METADATA_MEDIA_TYPE_DICOM_JSON {
        let mut out = vec![b'['];
        for (i, data) in encoded.iter().enumerate() {
            if i > 0 {
                out.push(b',');
            }
            out.extend_from_slice(data);
        }
        out.push(b']');
        return Ok(vec![out]);
    }
    Ok(encoded)
}
fn walk_bulk(
    instance: &Object,
    f: &mut dyn FnMut(usize, &Element, &str) -> Result<(), ClientError>,
) -> Result<(), ClientError> {
    let mut failure: Option<ClientError> = None;
    let walked = instance.walk(|path: &[Tag], element: &Element| {
        if let Value::BulkData(uri) = &element.value
            && let Err(e) = f(path.len(), element, uri)
        {
            failure = Some(e);
            return Err(crate::object::error::Error::msg("stop"));
        }
        Ok(())
    });
    if let Some(e) = failure {
        return Err(e);
    }
    walked.map_err(|e| other(e.to_string()))
}
fn validate_store_bulk_data(request: &StoreMetadataRequest) -> Result<(), ClientError> {
    let mut referenced: Vec<(String, i64)> = Vec::new();
    for instance in request.instances.iter().flatten() {
        walk_bulk(instance, &mut |_, _, uri| {
            if trim_space(uri).is_empty() || trim_space(uri) != uri {
                return Err(other("STOW-RS metadata has an empty or padded BulkDataURI"));
            }
            match referenced.iter_mut().find(|(u, _)| u == uri) {
                Some(slot) => slot.1 += 1,
                None => referenced.push((uri.to_string(), 1)),
            }
            Ok(())
        })?;
    }
    let mut supplied: Vec<String> = Vec::new();
    let mut parts_by_uri: Vec<(String, Vec<&StoreBulkData>)> = Vec::new();
    for bulk in &request.bulk_data {
        if bulk.uri.is_empty()
            || trim_space(&bulk.uri) != bulk.uri
            || bulk.uri.contains(['\r', '\n'])
        {
            return Err(other("STOW-RS bulk data URI is empty or invalid"));
        }
        let location = store_bulk_content_location(bulk);
        if supplied.contains(&location) {
            return Err(other("STOW-RS bulk data URI is duplicated"));
        }
        if bulk.open.is_none() && bulk.reader.is_none() && bulk.data.is_empty() {
            return Err(other("STOW-RS bulk data part is empty"));
        }
        let normalized = compressed_store_content_type(bulk_media(&bulk.content_type))
            .map_err(|e| {
                other(format!(
                    "STOW-RS bulk data content type is unsupported: {e}"
                ))
            })?
            .0;
        let whole = store_whole_profile(&normalized).is_some();
        if bulk.frame_number < 0
            || bulk.frame_number > 0 && (normalized == "application/octet-stream" || whole)
        {
            return Err(other("STOW-RS bulk frame number is invalid"));
        }
        supplied.push(location);
        match parts_by_uri.iter_mut().find(|(u, _)| *u == bulk.uri) {
            Some(slot) => slot.1.push(bulk),
            None => parts_by_uri.push((bulk.uri.clone(), vec![bulk])),
        }
        if !referenced.iter().any(|(u, _)| *u == bulk.uri) {
            return Err(other(
                "STOW-RS bulk data part is not referenced by the metadata",
            ));
        }
    }
    for (uri, _) in &referenced {
        if !parts_by_uri.iter().any(|(u, _)| u == uri) {
            return Err(other(
                "STOW-RS metadata references bulk data that is not supplied",
            ));
        }
    }
    for instance in request.instances.iter().flatten() {
        walk_bulk(instance, &mut |depth, element, uri| {
            let parts = parts_by_uri
                .iter()
                .find(|(u, _)| u == uri)
                .map(|(_, p)| p.as_slice())
                .unwrap_or_default();
            let Some(first) = parts.first() else {
                return Ok(());
            };
            let normalized = compressed_store_content_type(bulk_media(&first.content_type))
                .map(|(m, _)| m)
                .unwrap_or_default();
            if let Some(profile) = store_whole_profile(&normalized) {
                let count = referenced
                    .iter()
                    .find(|(u, _)| u == uri)
                    .map_or(0, |(_, n)| *n);
                if parts.len() != 1 || first.frame_number != 0 || count != 1 || depth != 1 {
                    return Err(other(
                        "STOW-RS whole content must have one transaction-local binding",
                    ));
                }
                return validate_store_whole_metadata(instance, element, &profile)
                    .map_err(|e| other(e.to_string()));
            }
            if normalized == "application/octet-stream" {
                if element.tag() == STORE_DOCUMENT_TAG {
                    return Err(other(
                        "STOW-RS document bulk requires its declared document MIME",
                    ));
                }
                if parts.len() != 1 || first.frame_number != 0 {
                    return Err(other("STOW-RS native bulk must be one part"));
                }
                return Ok(());
            }
            if depth != 1 || element.tag() != Tag::PIXEL_DATA {
                return Err(other(
                    "STOW-RS compressed bulk must reference top-level Pixel Data",
                ));
            }
            let count = store_metadata_frame_count(instance).map_err(|e| other(e.to_string()))?;
            if count != parts.len() as i64 {
                return Err(other("STOW-RS compressed frame count mismatch"));
            }
            for part in parts {
                let part_media = compressed_store_content_type(&part.content_type)
                    .map(|(m, _)| m)
                    .unwrap_or_default();
                if part_media != normalized
                    || count > 1 && (part.frame_number < 1 || part.frame_number > count)
                    || count == 1 && part.frame_number > 1
                {
                    return Err(other("STOW-RS compressed frame binding mismatch"));
                }
            }
            Ok(())
        })?;
    }
    Ok(())
}
enum Segment {
    Bytes(Vec<u8>),
    Bulk(usize),
}
struct MetadataBody<'a> {
    bulk: &'a mut [StoreBulkData],
    segments: Vec<Segment>,
    index: usize,
    offset: usize,
    payload: Option<Payload>,
    data_pos: Option<usize>,
    written: u64,
    failed: bool,
}
impl ByteRead for MetadataBody<'_> {
    fn read(&mut self, buf: &mut [u8]) -> (usize, Option<ReadErr>) {
        loop {
            if self.failed {
                return (0, Some(ReadErr::Other("STOW-RS payload failed".into())));
            }
            let Some(segment) = self.segments.get(self.index) else {
                return (0, Some(ReadErr::Eof));
            };
            match segment {
                Segment::Bytes(bytes) => {
                    if self.offset < bytes.len() {
                        let n = buf.len().min(bytes.len() - self.offset);
                        buf[..n].copy_from_slice(&bytes[self.offset..self.offset + n]);
                        self.offset += n;
                        return (n, None);
                    }
                    self.index += 1;
                    self.offset = 0;
                }
                Segment::Bulk(i) => {
                    let i = *i;
                    if self.payload.is_none() && self.data_pos.is_none() {
                        self.written = 0;
                        let bulk = &mut self.bulk[i];
                        if let Some(open) = &bulk.open {
                            match open() {
                                Ok(reader) => self.payload = Some(reader),
                                Err(_) => {
                                    self.failed = true;
                                    continue;
                                }
                            }
                        } else if let Some(reader) = bulk.reader.take() {
                            self.payload = Some(reader);
                        } else {
                            self.data_pos = Some(0);
                        }
                    }
                    let step = match (self.payload.as_mut(), self.data_pos.as_mut()) {
                        (Some(payload), _) => payload.read(buf),
                        (None, Some(pos)) => {
                            let data = &self.bulk[i].data;
                            let n = buf.len().min(data.len() - *pos);
                            buf[..n].copy_from_slice(&data[*pos..*pos + n]);
                            *pos += n;
                            if *pos == data.len() {
                                (n, Some(ReadErr::Eof))
                            } else {
                                (n, None)
                            }
                        }
                        (None, None) => (0, Some(ReadErr::Eof)),
                    };
                    match step {
                        (n, None) => {
                            self.written += n as u64;
                            if n > 0 {
                                return (n, None);
                            }
                        }
                        (n, Some(ReadErr::Eof)) => {
                            self.written += n as u64;
                            let empty = self.written == 0;
                            self.payload = None;
                            self.data_pos = None;
                            self.index += 1;
                            self.offset = 0;
                            if empty {
                                self.failed = true;
                            }
                            if n > 0 {
                                return (n, None);
                            }
                        }
                        (_, Some(_)) => {
                            self.payload = None;
                            self.failed = true;
                        }
                    }
                }
            }
        }
    }
}
impl Client {
    pub fn store_metadata(
        &self,
        ctx: &Ctx,
        request: &mut StoreMetadataRequest,
    ) -> (StoreResult, Option<ClientError>) {
        match self.endpoint.store_studies_url() {
            Ok(url) => self.store_metadata_at(ctx, &url, request),
            Err(e) => (StoreResult::default(), Some(e.into())),
        }
    }
    pub fn store_metadata_to_study(
        &self,
        ctx: &Ctx,
        study: &str,
        request: &mut StoreMetadataRequest,
    ) -> (StoreResult, Option<ClientError>) {
        match self.endpoint.store_study_url(study) {
            Ok(url) => self.store_metadata_at(ctx, &url, request),
            Err(e) => (StoreResult::default(), Some(e.into())),
        }
    }
    fn store_metadata_at(
        &self,
        ctx: &Ctx,
        url: &Url,
        request: &mut StoreMetadataRequest,
    ) -> (StoreResult, Option<ClientError>) {
        let fail = |e: ClientError| (StoreResult::default(), Some(e));
        if request.instances.is_empty() {
            return (StoreResult::default(), None);
        }
        let media_type = if request.media_type.is_empty() {
            METADATA_MEDIA_TYPE_DICOM_JSON.to_string()
        } else {
            request.media_type.clone()
        };
        if media_type != METADATA_MEDIA_TYPE_DICOM_JSON
            && media_type != METADATA_MEDIA_TYPE_DICOM_XML
        {
            return fail(other(format!(
                "STOW-RS metadata media type {} is not supported",
                quote(&media_type)
            )));
        }
        let metadata = match encode_store_metadata(&media_type, &request.instances) {
            Ok(metadata) => metadata,
            Err(e) => return fail(e),
        };
        if let Err(e) = validate_store_bulk_data(request) {
            return fail(e);
        }
        let boundary = random_boundary();
        let content_type = format!("multipart/related; boundary={boundary}; type=\"{media_type}\"");
        let mut segments = Vec::new();
        let mut first = true;
        let mut head = |headers: &str| {
            let lead = if first { "" } else { "\r\n" };
            first = false;
            Segment::Bytes(format!("{lead}--{boundary}\r\n{headers}\r\n").into_bytes())
        };
        for data in metadata {
            segments.push(head(&format!("Content-Type: {media_type}\r\n")));
            segments.push(Segment::Bytes(data));
        }
        for (i, bulk) in request.bulk_data.iter().enumerate() {
            segments.push(head(&format!(
                "Content-Location: {}\r\nContent-Type: {}\r\n",
                store_bulk_content_location(bulk),
                bulk_media(&bulk.content_type)
            )));
            segments.push(Segment::Bulk(i));
        }
        segments.push(Segment::Bytes(
            format!("\r\n--{boundary}--\r\n").into_bytes(),
        ));
        let mut body = MetadataBody {
            bulk: &mut request.bulk_data,
            segments,
            index: 0,
            offset: 0,
            payload: None,
            data_pos: None,
            written: 0,
            failed: false,
        };
        self.store_multipart_at(ctx, url, &mut body, &content_type)
    }
    fn store_multipart_at(
        &self,
        ctx: &Ctx,
        url: &Url,
        body: &mut dyn ByteRead,
        content_type: &str,
    ) -> (StoreResult, Option<ClientError>) {
        let sent = self.do_http(
            ctx,
            "POST",
            url,
            Some(body),
            content_type,
            ACCEPT_DICOM_JSON,
        );
        let (resp, failure) = match sent {
            Ok(http) => self.buffer_response(url, http),
            Err(e) => (Response::default(), Some(e)),
        };
        let with = |resp: Response| StoreResult {
            response: resp,
            ..StoreResult::default()
        };
        if let Some(e) = failure {
            return (with(resp), Some(e));
        }
        let success = (200..300).contains(&resp.status_code);
        match response::store_result_from_dicom_json(&resp.body) {
            Err(text) => {
                let failure = if success {
                    decode_error(&resp, Cause::text(text))
                } else {
                    status_error(&resp)
                };
                (with(resp), Some(failure))
            }
            Ok(mut parsed) => {
                let failure = (!success).then(|| status_error(&resp));
                parsed.response = resp;
                (parsed, failure)
            }
        }
    }
}
pub const ERR_STORE_REENCODE_UNSUPPORTED: &str =
    "dicomweb: STOW-RS payload cannot be re-encoded in Explicit VR Little Endian";
const IMPLICIT_VR_LITTLE_ENDIAN: &str = "1.2.840.10008.1.2";
const EXPLICIT_VR_LITTLE_ENDIAN: &str = "1.2.840.10008.1.2.1";
const EXPLICIT_VR_BIG_ENDIAN: &str = "1.2.840.10008.1.2.2";
const DEFLATED_EXPLICIT_VR_LITTLE_ENDIAN: &str = "1.2.840.10008.1.2.1.99";
const STORE_REENCODE_DEFLATE_RATIO: i64 = 1024;
const STORE_REENCODE_UNKNOWN_SIZE_ELEMENT_BYTES: i64 = 64 << 20;
pub fn store_requires_explicit_little_endian(uid: &str) -> bool {
    matches!(
        crate::object::transfer::normalize_uid(uid),
        IMPLICIT_VR_LITTLE_ENDIAN | EXPLICIT_VR_BIG_ENDIAN
    )
}
pub fn store_can_reencode_explicit_little_endian(uid: &str) -> bool {
    matches!(
        crate::object::transfer::normalize_uid(uid),
        IMPLICIT_VR_LITTLE_ENDIAN
            | EXPLICIT_VR_LITTLE_ENDIAN
            | EXPLICIT_VR_BIG_ENDIAN
            | DEFLATED_EXPLICIT_VR_LITTLE_ENDIAN
    )
}
fn store_reencode_read_options(
    size: i64,
    deflated: bool,
    big_endian: bool,
) -> crate::object::ReadFileOptions {
    let materialize = deflated || big_endian;
    let limits = crate::index::default_options().limits;
    let mut options = crate::object::ReadFileOptions {
        max_sequence_depth: limits.max_sequence_depth,
        max_elements: limits.max_elements,
        max_fragments: limits.max_fragments,
        ..Default::default()
    };
    if deflated && size > 0 && size <= i64::MAX / STORE_REENCODE_DEFLATE_RATIO {
        options.max_total_bytes = size * STORE_REENCODE_DEFLATE_RATIO;
    } else if size > 0 && size < i64::MAX {
        options.max_total_bytes = size + 1;
    }
    options.max_element_bytes = options.max_total_bytes;
    if options.max_total_bytes == 0 && materialize {
        options.max_element_bytes = STORE_REENCODE_UNKNOWN_SIZE_ELEMENT_BYTES;
    }
    options
}
struct ReencodedPayload {
    data: Result<Vec<u8>, String>,
    pos: usize,
}
impl ByteRead for ReencodedPayload {
    fn read(&mut self, buf: &mut [u8]) -> (usize, Option<ReadErr>) {
        match &self.data {
            Err(text) => (0, Some(ReadErr::Other(text.clone()))),
            Ok(data) => {
                if self.pos >= data.len() {
                    return (0, Some(ReadErr::Eof));
                }
                let n = buf.len().min(data.len() - self.pos);
                buf[..n].copy_from_slice(&data[self.pos..self.pos + n]);
                self.pos += n;
                (n, None)
            }
        }
    }
}
pub fn explicit_little_endian_store_payload<R: Read + Seek>(
    mut source: R,
    size: i64,
) -> Result<Payload, crate::MessageError> {
    let mut data = Vec::new();
    if let Err(e) = source
        .seek(SeekFrom::Start(0))
        .and_then(|_| source.read_to_end(&mut data))
    {
        return Err(format!("read STOW-RS payload header: {e}").into());
    }
    let shared: std::rc::Rc<[u8]> = std::rc::Rc::from(data.clone().into_boxed_slice());
    let file = crate::object::parser::source::MemFile::new(shared);
    let header =
        crate::object::read_part10_header_mem(&file, &crate::object::ReadFileOptions::default())
            .map_err(|e| format!("read STOW-RS payload header: {e}"))?;
    let source_uid = crate::object::transfer::normalize_uid(header.transfer_syntax.uid).to_string();
    if !store_can_reencode_explicit_little_endian(&source_uid) {
        return Err(format!(
            "{ERR_STORE_REENCODE_UNSUPPORTED}: source transfer syntax {source_uid}"
        )
        .into());
    }
    let deflated = source_uid == DEFLATED_EXPLICIT_VR_LITTLE_ENDIAN;
    let big_endian = source_uid == EXPLICIT_VR_BIG_ENDIAN;
    let mut parsed = crate::object::read_file_with_options(
        &data,
        store_reencode_read_options(size, deflated, big_endian),
    )
    .map_err(|(_, e)| format!("read STOW-RS payload: {e}"))?;
    parsed.transfer_syntax = crate::object::transfer::EXPLICIT_VR_LITTLE_ENDIAN;
    let encoded = crate::object::writer::write_file_with_options(
        &parsed,
        &crate::object::writer::WriteFileOptions {
            keep_partial_byte_order_units: true,
            ..Default::default()
        },
    )
    .map_err(|e| e.to_string());
    Ok(Box::new(ReencodedPayload {
        data: encoded,
        pos: 0,
    }))
}
pub fn explicit_little_endian_store_payload_size<R: Read + Seek>(
    source: R,
    size: i64,
) -> Result<i64, crate::MessageError> {
    let mut payload = explicit_little_endian_store_payload(source, size)?;
    let (data, err) = super::multipart::read_all(&mut *payload);
    if let Some(e) = err {
        return Err(format!("re-encode STOW-RS payload: {e}").into());
    }
    Ok(data.len() as i64)
}
pub trait StagedReader: Read + Send {
    fn close(&mut self) -> Result<(), ClientError>;
}
#[derive(Default)]
pub struct StagedSource {
    pub reader: Option<Box<dyn StagedReader>>,
    pub size: i64,
}
pub trait BulkDataStage {
    fn write(&mut self, data: &[u8]) -> Result<usize, ClientError>;
    fn publish(&mut self, ctx: &Ctx) -> Result<StagedSource, ClientError>;
    fn close(&mut self) -> Result<(), ClientError>;
}
pub trait BulkDataStaging {
    fn begin_bulk_data(
        &self,
        ctx: &Ctx,
        max_bytes: i64,
    ) -> Result<Option<Box<dyn BulkDataStage>>, ClientError>;
}
const ERR_CLOSED: &str = "file already closed";
fn ctx_error(ctx: &Ctx) -> Option<ClientError> {
    ctx.err().map(|kind| {
        ClientError::Read(if matches!(kind, Kind::Canceled) {
            ReadErr::Canceled
        } else {
            ReadErr::Deadline
        })
    })
}
#[derive(Clone, Debug, Default)]
pub struct FileBulkDataStaging {
    pub directory: String,
    pub max_bytes: i64,
}
fn unique_dir(parent: &Path, prefix: &str) -> std::io::Result<PathBuf> {
    use std::hash::{BuildHasher, Hasher};
    for _ in 0..10_000 {
        let bits = std::collections::hash_map::RandomState::new()
            .build_hasher()
            .finish();
        let path = parent.join(format!("{prefix}{}", bits % 10_000_000_000));
        match std::fs::create_dir(&path) {
            Ok(()) => {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700));
                }
                return Ok(path);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(std::io::Error::other("too many temporary directories"))
}
impl BulkDataStaging for FileBulkDataStaging {
    fn begin_bulk_data(
        &self,
        ctx: &Ctx,
        max_bytes: i64,
    ) -> Result<Option<Box<dyn BulkDataStage>>, ClientError> {
        if max_bytes <= 0 || self.max_bytes < 0 {
            return Err(other(
                "bulk staging requires a context and positive byte limit",
            ));
        }
        if let Some(e) = ctx_error(ctx) {
            return Err(e);
        }
        let mut limit = max_bytes;
        if self.max_bytes > 0 && self.max_bytes < limit {
            limit = self.max_bytes;
        }
        let parent = if self.directory.is_empty() {
            std::env::temp_dir()
        } else {
            PathBuf::from(&self.directory)
        };
        let dir = unique_dir(&parent, "dicom-bulk-").map_err(|e| other(e.to_string()))?;
        let path = dir.join("value-0");
        let file = match std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(file) => file,
            Err(e) => {
                let _ = std::fs::remove_dir_all(&dir);
                return Err(other(e.to_string()));
            }
        };
        Ok(Some(Box::new(FileStage {
            ctx: ctx.clone(),
            reader: Some(FileStageReader {
                file: Some(file),
                dir,
                removed: false,
            }),
            limit,
            size: 0,
            published: false,
            closed: false,
        })))
    }
}
struct FileStageReader {
    file: Option<std::fs::File>,
    dir: PathBuf,
    removed: bool,
}
impl Read for FileStageReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self.file.as_mut() {
            Some(file) => file.read(buf),
            None => Err(std::io::Error::other(ERR_CLOSED)),
        }
    }
}
impl StagedReader for FileStageReader {
    fn close(&mut self) -> Result<(), ClientError> {
        self.file = None;
        if self.removed {
            return Ok(());
        }
        std::fs::remove_dir_all(&self.dir).map_err(|e| other(e.to_string()))?;
        self.removed = true;
        Ok(())
    }
}
impl Drop for FileStageReader {
    fn drop(&mut self) {
        let _ = StagedReader::close(self);
    }
}
struct FileStage {
    ctx: Ctx,
    reader: Option<FileStageReader>,
    limit: i64,
    size: i64,
    published: bool,
    closed: bool,
}
impl BulkDataStage for FileStage {
    fn write(&mut self, data: &[u8]) -> Result<usize, ClientError> {
        if self.closed || self.published {
            return Err(other(ERR_CLOSED));
        }
        if let Some(e) = ctx_error(&self.ctx) {
            return Err(e);
        }
        if data.len() as i64 > self.limit - self.size {
            return Err(ClientError::BulkDataTooLarge(None));
        }
        let Some(file) = self.reader.as_mut().and_then(|r| r.file.as_mut()) else {
            return Err(other(ERR_CLOSED));
        };
        file.write_all(data).map_err(|e| other(e.to_string()))?;
        self.size += data.len() as i64;
        Ok(data.len())
    }
    fn publish(&mut self, ctx: &Ctx) -> Result<StagedSource, ClientError> {
        if self.closed || self.published {
            return Err(other(ERR_CLOSED));
        }
        if let Some(e) = ctx_error(&self.ctx).or_else(|| ctx_error(ctx)) {
            return Err(e);
        }
        let Some(mut reader) = self.reader.take() else {
            return Err(other(ERR_CLOSED));
        };
        if let Some(file) = reader.file.as_mut()
            && let Err(e) = file.seek(SeekFrom::Start(0))
        {
            self.reader = Some(reader);
            return Err(other(e.to_string()));
        }
        self.published = true;
        Ok(StagedSource {
            reader: Some(Box::new(reader)),
            size: self.size,
        })
    }
    fn close(&mut self) -> Result<(), ClientError> {
        if self.published {
            return Ok(());
        }
        self.closed = true;
        match self.reader.as_mut() {
            Some(reader) => reader.close(),
            None => Ok(()),
        }
    }
}
pub struct StagedBulkDataResolver<'a> {
    pub client: Client,
    pub staging: Option<&'a dyn BulkDataStaging>,
    pub chunk_bytes: i64,
}
struct Observer<'a> {
    reader: &'a mut dyn ByteRead,
    err: Option<ReadErr>,
}
impl ByteRead for Observer<'_> {
    fn read(&mut self, buf: &mut [u8]) -> (usize, Option<ReadErr>) {
        let (n, err) = self.reader.read(buf);
        if let Some(e) = &err
            && !matches!(e, ReadErr::Eof)
            && self.err.is_none()
        {
            self.err = Some(e.clone());
        }
        (n, err)
    }
}
fn join(err: ClientError, observed: Option<ReadErr>) -> ClientError {
    match observed {
        None => err,
        Some(e) => ClientError::Joined(vec![err, read_error(e)]),
    }
}
fn read_error(e: ReadErr) -> ClientError {
    match e {
        ReadErr::Web(w) => ClientError::Web(*w),
        other => ClientError::Read(other),
    }
}
fn is_too_large(err: &ClientError) -> bool {
    match err {
        ClientError::BulkDataTooLarge(_) => true,
        ClientError::Other(text) => text == "bulk data exceeds the caller limit",
        ClientError::Joined(errors) => errors.iter().any(is_too_large),
        _ => false,
    }
}
fn is_part_bytes(err: &ClientError) -> bool {
    match err {
        ClientError::Web(web) => matches!(
            web.err.as_ref().map(|c| &c.class),
            Some(CauseClass::Decode(d)) if d.kind == K::PartBytes
        ),
        ClientError::Read(ReadErr::Decode(d)) => d.kind == K::PartBytes,
        ClientError::Joined(errors) => errors.iter().any(is_part_bytes),
        _ => false,
    }
}
impl Client {
    fn stream_resource_eof(
        &self,
        ctx: &Ctx,
        url: &Url,
        handle: PartHandler,
    ) -> Result<(), ClientError> {
        self.limits()?;
        let options = RequestOptions {
            same_origin_redirect: true,
            ..RequestOptions::default()
        };
        let mut http = self.do_http_opts(ctx, "GET", url, None, "", ACCEPT_BULK_DATA, &options)?;
        let resp = response_from_http(&url.string(), &http);
        if !(200..300).contains(&resp.status_code) {
            return Err(status_error(&resp));
        }
        let maximum = self.max_body_bytes();
        let mut limited = BodyLimitReader {
            body: &mut http.body,
            remaining: maximum,
            limit: maximum,
            resp: &resp,
            raw: true,
        };
        let mut observed = Observer {
            reader: &mut limited,
            err: None,
        };
        if let Err(e) = self.stream_object_parts(&resp, &mut observed, &BULK_DATA_POLICY, handle) {
            return Err(join(e, observed.err.take()));
        }
        if let Some(e) = observed.err.take() {
            return Err(read_error(e));
        }
        let (_, err) = super::multipart::read_all(&mut observed);
        if let Some(e) = err {
            let observed = observed.err.take();
            return Err(join(read_error(e), observed));
        }
        if let Some(e) = observed.err.take() {
            return Err(read_error(e));
        }
        if let Some(e) = ctx_error(ctx) {
            return Err(e);
        }
        Ok(())
    }
}
impl StagedBulkDataResolver<'_> {
    pub fn resolve_bulk_data_uri(
        &self,
        ctx: &Ctx,
        uri: &str,
        max_bytes: i64,
    ) -> Result<StagedSource, ClientError> {
        let Some(staging) = self.staging else {
            return Err(other("invalid bulk staging configuration"));
        };
        if max_bytes < 0 || self.chunk_bytes < 0 || self.chunk_bytes > 1 << 20 {
            return Err(other("invalid bulk staging configuration"));
        }
        if let Some(e) = ctx_error(ctx) {
            return Err(e);
        }
        let limits = self.client.limits()?;
        let url = self.client.endpoint.bulk_data_url(uri)?;
        let mut limit = limits.max_part_bytes;
        if max_bytes > 0 && max_bytes < limit {
            limit = max_bytes;
        }
        let chunk = if self.chunk_bytes == 0 {
            32 << 10
        } else {
            self.chunk_bytes as usize
        };
        let mut stage: Option<Box<dyn BulkDataStage>> = None;
        let mut count: i64 = 0;
        let mut total: i64 = 0;
        let mut nil_stage = false;
        let result = self.client.stream_resource_eof(ctx, &url, &mut |part| {
            count += 1;
            if count != 1 {
                return Err(ClientError::Read(ReadErr::Decode(
                    ResponseDecodeError::with(K::PartLimit, 1, count),
                )));
            }
            let Some(mut opened) = staging.begin_bulk_data(ctx, limit)? else {
                nil_stage = true;
                return Err(other("bulk staging returned no stage"));
            };
            let mut buffer = vec![0u8; chunk];
            let mut read: i64 = 0;
            let mut empty_reads = 0;
            let outcome = loop {
                if let Some(e) = ctx_error(ctx) {
                    break Err(e);
                }
                let (n, read_err) = part.reader.read(&mut buffer);
                if n > 0 {
                    if n as i64 > limit - read {
                        break Err(other("bulk data exceeds the caller limit"));
                    }
                    match opened.write(&buffer[..n]) {
                        Ok(written) if written == n => {}
                        Ok(_) => break Err(other("short write")),
                        Err(e) => break Err(e),
                    }
                    read += n as i64;
                    total = read;
                    empty_reads = 0;
                }
                match read_err {
                    Some(ReadErr::Eof) => break Ok(()),
                    Some(e) => break Err(read_error(e)),
                    None => {}
                }
                if n == 0 {
                    empty_reads += 1;
                    if empty_reads == 100 {
                        break Err(other("multiple Read calls return no data or error"));
                    }
                }
            };
            stage = Some(opened);
            outcome
        });
        let close_stage = |stage: &mut Option<Box<dyn BulkDataStage>>, err: ClientError| match stage
            .as_mut()
            .map(|s| s.close())
        {
            Some(Err(close)) => ClientError::Joined(vec![err, close]),
            _ => err,
        };
        if let Err(err) = result {
            let err = if is_too_large(&err) {
                ClientError::Joined(vec![ClientError::BulkDataTooLarge(None), err])
            } else if is_part_bytes(&err) {
                ClientError::BulkDataTooLarge(Some(Box::new(err)))
            } else {
                err
            };
            let _ = nil_stage;
            return Err(close_stage(&mut stage, err));
        }
        if count != 1 || stage.is_none() {
            let err = ClientError::Read(ReadErr::Decode(ResponseDecodeError::with(
                K::PartLimit,
                1,
                count,
            )));
            return Err(close_stage(&mut stage, err));
        }
        if let Some(e) = ctx_error(ctx) {
            return Err(close_stage(&mut stage, e));
        }
        let Some(opened) = stage.as_mut() else {
            return Err(other("bulk staging returned no stage"));
        };
        let published = opened.publish(ctx);
        let outcome = match published {
            Ok(source)
                if source.reader.is_some()
                    && source.size == total
                    && source.size >= 0
                    && source.size <= limit =>
            {
                Ok(source)
            }
            Ok(mut source) => {
                let mut err = other("bulk staging returned an invalid source");
                if let Some(reader) = source.reader.as_mut()
                    && let Err(close) = reader.close()
                {
                    err = ClientError::Joined(vec![err, close]);
                }
                Err(err)
            }
            Err(e) => Err(e),
        };
        match outcome {
            Ok(source) => {
                let _ = opened.close();
                Ok(source)
            }
            Err(err) => Err(close_stage(&mut stage, err)),
        }
    }
}
