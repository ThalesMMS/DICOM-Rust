use super::mimeheader::{Header, MediaType, parse_media_type, to_lower, trim_space};
use super::multipart::{ByteRead, MultipartReader, PartReader, ReadErr, SliceReader, read_all};
use super::types::{
    DEFAULT_MAX_BODY_BYTES, DEFAULT_MAX_MULTIPART_DEPTH, DEFAULT_MAX_PART_HEADER_BYTES,
    DEFAULT_MAX_RESPONSE_PARTS, InstanceRef, ResponseDecodeError, ResponseDecodeErrorKind as K,
    ResponseLimits,
};
use crate::dicomjson::jsonwire::{self, JAny, JsonError};
use crate::dicomjson::{Dataset, element_string};
use crate::textfmt::quote;
use crate::transfer;
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Response {
    pub url: String,
    pub status_code: i64,
    pub status: String,
    pub header: Header,
    pub body: Vec<u8>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ObjectPart {
    pub content_type: String,
    pub transfer_syntax_uid: String,
    pub data: Vec<u8>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StoreItem {
    pub sop_class_uid: String,
    pub sop_instance_uid: String,
    pub retrieve_url: String,
    pub warning_reason: u16,
    pub failure_reason: u16,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StoreResult {
    pub response: Response,
    pub stored: Option<Vec<StoreItem>>,
    pub failed: Option<Vec<StoreItem>>,
}
fn type_error(raw: &[u8], typ: &str) -> String {
    format!(
        "json: cannot unmarshal {} into value of type {typ}",
        jsonwire::kind(raw)
    )
}
fn dataset_from_raw(raw: &[u8], first: &mut Option<String>) -> Dataset {
    let mut dataset: Dataset = Vec::new();
    match raw.first() {
        Some(b'{') => {}
        Some(b'n') => return dataset,
        _ => {
            first.get_or_insert_with(|| type_error(raw, "Dataset"));
            return dataset;
        }
    }
    let members = jsonwire::as_raw_map(raw).unwrap_or_default();
    for (key, value) in members {
        match jsonwire::as_element(value) {
            Ok(element) => dataset.push((key, element)),
            Err(e) => {
                first.get_or_insert_with(|| e.to_string());
            }
        }
    }
    dataset
}
fn decode_one(data: &[u8]) -> Result<(&[u8], jsonwire::Decoder<'_>), String> {
    let mut decoder = jsonwire::Decoder::with_tail(data, None, true);
    let raw = decoder.decode_raw().map_err(|e| e.to_string())?;
    Ok((raw, decoder))
}
fn ensure_single(decoder: &mut jsonwire::Decoder) -> Result<(), String> {
    match decoder.decode_raw() {
        Err(JsonError::Eof) => Ok(()),
        Ok(_) => Err("DICOM JSON response contains multiple JSON values".into()),
        Err(e) => Err(e.to_string()),
    }
}
#[doc(hidden)]
pub fn dataset_from_json(data: &[u8]) -> Result<Dataset, String> {
    let (raw, mut decoder) = decode_one(data)?;
    let mut first = None;
    let dataset = dataset_from_raw(raw, &mut first);
    if let Some(e) = first {
        return Err(e);
    }
    ensure_single(&mut decoder)?;
    Ok(dataset)
}
#[doc(hidden)]
pub fn datasets_from_dicom_json(data: &[u8]) -> Result<Option<Vec<Dataset>>, String> {
    if trim_space(&String::from_utf8_lossy(data)).is_empty() {
        return Ok(None);
    }
    let (raw, mut decoder) = decode_one(data)?;
    let mut first = None;
    let datasets = match raw.first() {
        Some(b'[') => Some(
            jsonwire::as_raw_array(raw)
                .unwrap_or_default()
                .into_iter()
                .map(|item| dataset_from_raw(item, &mut first))
                .collect::<Vec<_>>(),
        ),
        Some(b'n') => None,
        _ => return Err(type_error(raw, "array of Dataset")),
    };
    if let Some(e) = first {
        return Err(e);
    }
    ensure_single(&mut decoder)?;
    Ok(datasets)
}
fn any_json(value: &JAny, out: &mut String) {
    match value {
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
                any_json(item, out);
            }
            out.push(']');
        }
        JAny::Obj(members) => {
            out.push('{');
            for (i, (key, item)) in members.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                jsonwire::append_string(out, key);
                out.push(':');
                any_json(item, out);
            }
            out.push('}');
        }
    }
}
fn parse_uint16(s: &str, base: u32) -> Result<u16, String> {
    let fail = |what: &str| format!("parse_uint: parsing {}: {what}", quote(s));
    let (digits, radix, underscores) = if base == 0 {
        (&s[2.min(s.len())..], 16, true)
    } else {
        (s, base, false)
    };
    if digits.is_empty() {
        return Err(fail("invalid syntax"));
    }
    if underscores {
        let bytes = digits.as_bytes();
        for (i, &c) in bytes.iter().enumerate() {
            if c == b'_' && (i + 1 == bytes.len() || bytes[i + 1] == b'_') {
                return Err(fail("invalid syntax"));
            }
        }
    }
    let mut n: u64 = 0;
    let mut overflow = false;
    for c in digits.chars() {
        if underscores && c == '_' {
            continue;
        }
        let Some(d) = c.to_digit(radix) else {
            return Err(fail("invalid syntax"));
        };
        if !overflow {
            n = n * u64::from(radix) + u64::from(d);
            overflow = n > 0xFFFF;
        }
    }
    if overflow {
        return Err(fail("value out of range"));
    }
    Ok(n as u16)
}
fn element_uint16(dataset: &Dataset, tag: &str) -> Result<u16, String> {
    let value = element_string(dataset, tag);
    if value.is_empty() {
        return Ok(0);
    }
    let trimmed = trim_space(&value);
    let base = if to_lower(trimmed).starts_with("0x") {
        0
    } else if crate::textfmt::to_upper(trimmed)
        .chars()
        .any(|c| "ABCDEF".contains(c))
    {
        16
    } else {
        10
    };
    parse_uint16(trimmed, base)
}
fn store_items_from_sequence(
    dataset: &Dataset,
    tag: &str,
) -> Result<Option<Vec<StoreItem>>, String> {
    let Some((_, element)) = dataset.iter().find(|(k, _)| k == tag) else {
        return Ok(None);
    };
    let mut items = Vec::with_capacity(element.value.len());
    for value in &element.value {
        let mut json = String::new();
        any_json(value, &mut json);
        let item = dataset_from_json(json.as_bytes())?;
        let warning_reason = element_uint16(&item, "00081196")?;
        let failure_reason = element_uint16(&item, "00081197")?;
        items.push(StoreItem {
            sop_class_uid: element_string(&item, "00081150"),
            sop_instance_uid: element_string(&item, "00081155"),
            retrieve_url: element_string(&item, "00081190"),
            warning_reason,
            failure_reason,
        });
    }
    Ok(Some(items))
}
fn first_dicom_json_dataset(data: &[u8]) -> Result<Dataset, String> {
    if let Ok(dataset) = dataset_from_json(data) {
        return Ok(dataset);
    }
    let datasets = datasets_from_dicom_json(data)?;
    Ok(datasets
        .and_then(|d| d.into_iter().next())
        .unwrap_or_default())
}
pub fn store_result_from_dicom_json(data: &[u8]) -> Result<StoreResult, crate::MessageError> {
    if trim_space(&String::from_utf8_lossy(data)).is_empty() {
        return Ok(StoreResult::default());
    }
    let dataset = first_dicom_json_dataset(data)?;
    let stored = store_items_from_sequence(&dataset, "00081199")?;
    let mut failed = store_items_from_sequence(&dataset, "00081198")?;
    if let Some(other) = store_items_from_sequence(&dataset, "0008119A")?
        && (!other.is_empty() || failed.is_none())
    {
        match &mut failed {
            Some(list) => list.extend(other),
            None if other.is_empty() => {}
            None => failed = Some(other),
        }
    }
    Ok(StoreResult {
        response: Response::default(),
        stored,
        failed,
    })
}
pub const ERR_INVALID_METADATA_IDENTITY: &str = "invalid DICOMweb metadata identity";
fn identity(detail: String) -> String {
    format!("{ERR_INVALID_METADATA_IDENTITY}: {detail}")
}
fn scoped_metadata_uid(
    dataset: &Dataset,
    tag: &str,
    requested: &str,
    inherit_parent: bool,
) -> Result<String, String> {
    let mut uid = String::new();
    if let Some((_, element)) = dataset.iter().find(|(k, _)| k == tag) {
        if element.vr != "UI"
            || element.value.len() > 1
            || !element.inline_binary.is_empty()
            || !element.bulk_data_uri.is_empty()
        {
            return Err(identity(format!("tag {tag} must contain one UID value")));
        }
        if let Some(value) = element.value.first() {
            let JAny::Str(value) = value else {
                return Err(identity(format!("tag {tag} must contain a UID string")));
            };
            uid = trim_space(value).to_string();
        }
    }
    let requested = trim_space(requested);
    if uid.is_empty() && inherit_parent {
        uid = requested.to_string();
    }
    if !crate::object::is_valid_uid(uid.as_bytes()) {
        return Err(identity(format!("tag {tag} is missing or invalid")));
    }
    if !requested.is_empty() && uid != requested {
        return Err(identity(format!(
            "tag {tag} conflicts with the requested resource"
        )));
    }
    Ok(uid)
}
pub(crate) fn scoped_metadata_ref(
    dataset: &Dataset,
    scope: &InstanceRef,
) -> Result<InstanceRef, String> {
    let mut noncanonical: Vec<String> = dataset
        .iter()
        .filter_map(|(tag, _)| {
            let canonical = crate::textfmt::to_upper(trim_space(tag));
            (matches!(canonical.as_str(), "0020000D" | "0020000E" | "00080018")
                && *tag != canonical)
                .then_some(canonical)
        })
        .collect();
    noncanonical.sort();
    if let Some(canonical) = noncanonical.first() {
        return Err(identity(format!("noncanonical tag {canonical}")));
    }
    Ok(InstanceRef {
        study_instance_uid: scoped_metadata_uid(
            dataset,
            "0020000D",
            &scope.study_instance_uid,
            true,
        )?,
        series_instance_uid: scoped_metadata_uid(
            dataset,
            "0020000E",
            &scope.series_instance_uid,
            true,
        )?,
        sop_instance_uid: scoped_metadata_uid(dataset, "00080018", &scope.sop_instance_uid, false)?,
    })
}
#[doc(hidden)]
pub fn instance_refs_from_datasets(
    datasets: &[Dataset],
    scope: &InstanceRef,
) -> Result<Vec<InstanceRef>, String> {
    if !scope.sop_instance_uid.is_empty() && datasets.len() > 1 {
        return Err(identity(
            "instance response contains more than one dataset".into(),
        ));
    }
    let track = scope.series_instance_uid.is_empty();
    let mut refs: Vec<InstanceRef> = Vec::with_capacity(datasets.len());
    for dataset in datasets {
        let found = scoped_metadata_ref(dataset, scope)?;
        if track
            && refs
                .iter()
                .any(|r| r.sop_instance_uid == found.sop_instance_uid && *r != found)
        {
            return Err(identity(
                "tag 00080018 occurs with conflicting parent UIDs".into(),
            ));
        }
        refs.push(found);
    }
    Ok(refs)
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ParseLimits {
    pub max_parts: i64,
    pub max_part_bytes: i64,
    pub max_header_bytes: i64,
    pub max_depth: i64,
    pub allowed_media: Option<Vec<String>>,
}
pub(crate) fn maximum_response_body_bytes(max_body_bytes: i64) -> i64 {
    if max_body_bytes > 0 {
        max_body_bytes
    } else {
        DEFAULT_MAX_BODY_BYTES
    }
}
pub(crate) fn response_limits(
    configured: &ResponseLimits,
    max_body_bytes: i64,
) -> Result<ParseLimits, String> {
    if configured.max_parts < 0
        || configured.max_part_bytes < 0
        || configured.max_part_header_bytes < 0
        || configured.max_multipart_depth < 0
    {
        return Err("dicomweb: response limits cannot be negative".into());
    }
    if configured.max_multipart_depth > DEFAULT_MAX_MULTIPART_DEPTH {
        return Err("dicomweb: WADO-RS multipart depth must be zero or one".into());
    }
    let maximum_body = maximum_response_body_bytes(max_body_bytes);
    let mut limits = ParseLimits {
        max_parts: configured.max_parts,
        max_part_bytes: configured.max_part_bytes,
        max_header_bytes: configured.max_part_header_bytes,
        max_depth: configured.max_multipart_depth,
        allowed_media: None,
    };
    if limits.max_parts <= 0 {
        limits.max_parts = DEFAULT_MAX_RESPONSE_PARTS;
    }
    if limits.max_part_bytes <= 0 || limits.max_part_bytes > maximum_body {
        limits.max_part_bytes = maximum_body;
    }
    if limits.max_header_bytes <= 0 {
        limits.max_header_bytes = DEFAULT_MAX_PART_HEADER_BYTES;
    }
    if limits.max_depth <= 0 {
        limits.max_depth = DEFAULT_MAX_MULTIPART_DEPTH;
    }
    if !configured.allowed_media_types.is_empty() {
        let mut allowed = Vec::new();
        for value in &configured.allowed_media_types {
            match parse_media_type(trim_space(value)) {
                Ok(parsed) if !parsed.media_type.is_empty() => {
                    allowed.push(to_lower(&parsed.media_type));
                }
                _ => return Err("dicomweb: allowed WADO media type is invalid".into()),
            }
        }
        limits.allowed_media = Some(allowed);
    }
    Ok(limits)
}
#[derive(Clone, Copy, Debug)]
pub(crate) struct MediaPolicy {
    pub require_multipart: bool,
    pub plain_single_media: bool,
    pub frames: bool,
    pub single: &'static [&'static str],
    pub parts: &'static [&'static str],
}
pub(crate) const DICOM_OBJECT_POLICY: MediaPolicy = MediaPolicy {
    require_multipart: false,
    plain_single_media: false,
    frames: false,
    single: &["application/dicom"],
    parts: &["application/dicom"],
};
pub(crate) const FRAME_POLICY: MediaPolicy = MediaPolicy {
    require_multipart: true,
    plain_single_media: false,
    frames: true,
    single: &[],
    parts: &[
        "application/octet-stream",
        "application/x-deflate",
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
    ],
};
type Decode<T> = Result<T, ResponseDecodeError>;
fn fail<T>(kind: K) -> Decode<T> {
    Err(ResponseDecodeError::new(kind))
}
pub(crate) fn parse_response_media_type(value: &str) -> Decode<MediaType> {
    match parse_media_type(trim_space(value)) {
        Ok(mut parsed) if !parsed.media_type.is_empty() => {
            parsed.media_type = to_lower(&parsed.media_type);
            Ok(parsed)
        }
        _ => fail(K::MalformedMediaType),
    }
}
pub(crate) fn response_content_type(values: &[String]) -> Decode<String> {
    if values.len() != 1 {
        return fail(K::MalformedMediaType);
    }
    Ok(trim_space(&values[0]).to_string())
}
pub(crate) fn media_allowed(
    media_type: &str,
    operation: &[&str],
    configured: &Option<Vec<String>>,
) -> bool {
    if !operation.contains(&media_type) {
        return false;
    }
    match configured {
        None => true,
        Some(list) => list.iter().any(|m| m == media_type),
    }
}
pub(crate) fn valid_multipart_boundary(boundary: &str) -> bool {
    if boundary.is_empty() || boundary.len() > 70 {
        return false;
    }
    boundary.char_indices().all(|(index, c)| {
        c.is_ascii_alphanumeric()
            || "'()+_,-./:=?".contains(c)
            || (c == ' ' && index != boundary.len() - 1)
    })
}
pub(crate) fn validate_outer_response_media_type(
    content_type: &str,
    policy: &MediaPolicy,
    limits: &ParseLimits,
) -> Decode<MediaType> {
    let parsed = parse_response_media_type(content_type)?;
    if parsed.media_type == "multipart/related" {
        if limits.max_depth < 1 {
            return Err(ResponseDecodeError::with(
                K::MultipartDepth,
                limits.max_depth,
                1,
            ));
        }
        if trim_space(parsed.param("boundary")).is_empty() {
            return fail(K::MissingBoundary);
        }
        if !valid_multipart_boundary(parsed.param("boundary")) {
            return fail(K::MalformedMultipart);
        }
        return match parse_response_media_type(parsed.param("type")) {
            Ok(declared)
                if media_allowed(&declared.media_type, policy.parts, &limits.allowed_media) =>
            {
                Ok(parsed)
            }
            _ => fail(K::UnexpectedMedia),
        };
    }
    if policy.require_multipart
        || (policy.plain_single_media && !parsed.params.is_empty())
        || !media_allowed(&parsed.media_type, policy.single, &limits.allowed_media)
    {
        return fail(K::UnexpectedMedia);
    }
    Ok(parsed)
}
pub(crate) fn frame_media_type_matches_transfer_syntax(media_type: &str, uid: &str) -> bool {
    let uid = crate::object::transfer::normalize_uid(uid);
    if uid.is_empty() {
        return true;
    }
    let any = |list: &[&transfer::Syntax]| list.iter().any(|s| s.uid == uid);
    match to_lower(media_type).as_str() {
        "application/octet-stream" => any(&[
            &transfer::EXPLICIT_VR_LITTLE_ENDIAN,
            &transfer::ENCAPSULATED_UNCOMPRESSED_EXPLICIT_VR_LITTLE_ENDIAN,
        ]),
        "application/x-deflate" => any(&[&transfer::DEFLATED_IMAGE_FRAME_COMPRESSION]),
        "image/jpeg" => any(&[
            &transfer::JPEG_BASELINE,
            &transfer::JPEG_EXTENDED,
            &transfer::JPEG_LOSSLESS_NON_HIERARCHICAL,
            &transfer::JPEG_LOSSLESS_SV1,
        ]),
        "image/dicom-rle" => any(&[&transfer::RLE_LOSSLESS]),
        "image/jls" => transfer::is_jpeg_ls_transfer_syntax(uid),
        "image/jp2" => any(&[&transfer::JPEG_2000_LOSSLESS_ONLY, &transfer::JPEG_2000]),
        "image/jpx" => any(&[
            &transfer::JPEG_2000_PART2_LOSSLESS,
            &transfer::JPEG_2000_PART2,
        ]),
        "image/jphc" => any(&[
            &transfer::HTJ2K_LOSSLESS,
            &transfer::HTJ2K_LOSSLESS_RPCL,
            &transfer::HTJ2K,
        ]),
        "image/jxl" => transfer::is_jpeg_xl_transfer_syntax(uid),
        "video/mpeg" => any(&[
            &transfer::MPEG2_MPML,
            &transfer::MPEG2_MPML_F,
            &transfer::MPEG2_MPHL,
            &transfer::MPEG2_MPHL_F,
        ]),
        "video/mp4" => any(&[
            &transfer::MPEG4_HP41,
            &transfer::MPEG4_HP41_F,
            &transfer::MPEG4_HP41_BD,
            &transfer::MPEG4_HP41_BD_F,
            &transfer::MPEG4_HP42_2D,
            &transfer::MPEG4_HP42_2D_F,
            &transfer::MPEG4_HP42_3D,
            &transfer::MPEG4_HP42_3D_F,
            &transfer::MPEG4_HP42_STEREO,
            &transfer::MPEG4_HP42_STEREO_F,
        ]),
        "video/h265" => any(&[&transfer::HEVC_MP51, &transfer::HEVC_M10P51]),
        _ => false,
    }
}
pub(crate) fn web_service_transfer_syntax_allowed(uid: &str) -> bool {
    let uid = crate::object::transfer::normalize_uid(uid);
    if uid.is_empty()
        || uid == transfer::IMPLICIT_VR_LITTLE_ENDIAN.uid
        || uid == transfer::EXPLICIT_VR_BIG_ENDIAN.uid
    {
        return false;
    }
    transfer::default_registry().get(uid).is_some()
}
pub(crate) fn validate_part_media_type(
    content_type: &str,
    policy: &MediaPolicy,
    limits: &ParseLimits,
) -> Decode<MediaType> {
    let parsed = parse_response_media_type(content_type)?;
    if parsed.media_type.starts_with("multipart/") {
        return Err(ResponseDecodeError::with(
            K::MultipartDepth,
            limits.max_depth,
            2,
        ));
    }
    if !media_allowed(&parsed.media_type, policy.parts, &limits.allowed_media) {
        return fail(K::UnexpectedMedia);
    }
    if policy.frames
        && !frame_media_type_matches_transfer_syntax(
            &parsed.media_type,
            parsed.param("transfer-syntax"),
        )
    {
        return fail(K::UnexpectedMedia);
    }
    Ok(parsed)
}
pub(crate) fn validate_part_header(header: &Header, limits: &ParseLimits) -> Decode<()> {
    let mut size: i64 = 2;
    for (name, values) in &header.0 {
        for value in values {
            size += (name.len() + 2 + value.len() + 2) as i64;
        }
    }
    if size > limits.max_header_bytes {
        return Err(ResponseDecodeError::with(
            K::HeaderBytes,
            limits.max_header_bytes,
            size,
        ));
    }
    let encoding = trim_space(header.get("Content-Transfer-Encoding"));
    if !encoding.is_empty() && !encoding.eq_ignore_ascii_case("binary") {
        return fail(K::TransferEncoding);
    }
    if header.values("Content-Type").len() != 1 {
        return fail(K::MalformedMediaType);
    }
    if header.values("Content-Transfer-Encoding").len() > 1 {
        return fail(K::TransferEncoding);
    }
    Ok(())
}
pub(crate) fn response_part_read_error(err: ReadErr) -> ReadErr {
    match err {
        ReadErr::UnexpectedEof => ReadErr::Decode(ResponseDecodeError::new(K::MalformedMultipart)),
        other => other,
    }
}
pub(crate) fn multipart_decode_error(err: &ReadErr) -> ResponseDecodeError {
    match err {
        ReadErr::Decode(e) => *e,
        _ => ResponseDecodeError::new(K::MalformedMultipart),
    }
}
pub(crate) fn read_response_part(
    part: &mut dyn ByteRead,
    limits: &ParseLimits,
) -> Result<Vec<u8>, ReadErr> {
    let read_limit = limits.max_part_bytes.saturating_add(1);
    let mut limited = LimitReader {
        reader: part,
        remaining: read_limit,
    };
    let (data, err) = read_all(&mut limited);
    if let Some(e) = err {
        return Err(e);
    }
    if data.len() as i64 > limits.max_part_bytes {
        return Err(ReadErr::Decode(ResponseDecodeError::with(
            K::PartBytes,
            limits.max_part_bytes,
            data.len() as i64,
        )));
    }
    Ok(data)
}
pub(crate) struct LimitReader<'a> {
    pub reader: &'a mut dyn ByteRead,
    pub remaining: i64,
}
impl ByteRead for LimitReader<'_> {
    fn read(&mut self, buf: &mut [u8]) -> (usize, Option<ReadErr>) {
        if self.remaining <= 0 {
            return (0, Some(ReadErr::Eof));
        }
        let n = (buf.len() as i64).min(self.remaining) as usize;
        let (n, err) = self.reader.read(&mut buf[..n]);
        self.remaining -= n as i64;
        (n, err)
    }
}
pub(crate) fn transfer_syntax_from_content_type(content_type: &str) -> String {
    parse_media_type(trim_space(content_type))
        .map(|parsed| trim_space(parsed.param("transfer-syntax")).to_string())
        .unwrap_or_default()
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum PartsError {
    Decode(ResponseDecodeError),
    Read(ReadErr),
}
impl From<ResponseDecodeError> for PartsError {
    fn from(e: ResponseDecodeError) -> Self {
        PartsError::Decode(e)
    }
}
pub(crate) fn object_parts_from_response(
    resp: &Response,
    limits: &ParseLimits,
    policy: &MediaPolicy,
) -> Result<Vec<ObjectPart>, PartsError> {
    let content_type = response_content_type(resp.header.values("Content-Type"))?;
    let outer = validate_outer_response_media_type(&content_type, policy, limits)?;
    if outer.media_type == "multipart/related" {
        let mut body = SliceReader::new(&resp.body);
        let mut reader =
            MultipartReader::new(&mut body, outer.param("boundary"), limits.max_header_bytes);
        let mut parts: Vec<ObjectPart> = Vec::new();
        loop {
            let header = match reader.next_part() {
                Ok(header) => header,
                Err(ReadErr::Eof) => break,
                Err(e) => return Err(multipart_decode_error(&e).into()),
            };
            if parts.len() as i64 >= limits.max_parts {
                return Err(ResponseDecodeError::with(
                    K::PartLimit,
                    limits.max_parts,
                    parts.len() as i64 + 1,
                )
                .into());
            }
            validate_part_header(&header, limits)?;
            let part_content_type = header.get("Content-Type").to_string();
            validate_part_media_type(&part_content_type, policy, limits)?;
            let data = read_response_part(
                &mut PartReader {
                    multipart: &mut reader,
                },
                limits,
            )
            .map_err(|e| match response_part_read_error(e) {
                ReadErr::Decode(d) => PartsError::Decode(d),
                other => PartsError::Read(other),
            })?;
            parts.push(ObjectPart {
                transfer_syntax_uid: transfer_syntax_from_content_type(&part_content_type),
                content_type: part_content_type,
                data,
            });
        }
        if parts.is_empty() {
            return Err(ResponseDecodeError::new(K::MalformedMultipart).into());
        }
        return Ok(parts);
    }
    if resp.body.len() as i64 > limits.max_part_bytes {
        return Err(ResponseDecodeError::with(
            K::PartBytes,
            limits.max_part_bytes,
            resp.body.len() as i64,
        )
        .into());
    }
    Ok(vec![ObjectPart {
        transfer_syntax_uid: transfer_syntax_from_content_type(&content_type),
        content_type,
        data: resp.body.clone(),
    }])
}
