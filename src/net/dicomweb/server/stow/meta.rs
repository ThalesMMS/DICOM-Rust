use super::super::super::mimeheader::{Header, parse_media_type, to_lower};
use super::super::super::stowprofile::{
    EXPLICIT_VR_LITTLE_ENDIAN_UID, STORE_DOCUMENT_MIME_TAG, STORE_DOCUMENT_TAG, VIDEO_SOP_CLASSES,
    WholeProfile, store_bulk_part_parameters, store_metadata_frame_count, store_required_string,
    store_single_string, store_whole_profile,
};
use super::super::spool::{copy_bounded, create_temp, io_server_error};
use super::super::{Inner, ServerError, classify_multipart_error, ctx_err};
use super::{
    MaxBytesReader, MultipartReader, PartRead, StagedPart, spool_failure, unsupported_media,
};
use crate::net::dicomweb::server::http::Request;
use crate::net::dicomweb::types::InstanceRef;
use crate::net::ul::Ctx;
use crate::object::bulk::{BulkDataSource, BulkDataWriteOptions};
use crate::object::transfer::Syntax;
use crate::object::writer::{WriteFileOptions, write_file_with_bulk_data};
use crate::object::{
    Element, ElementHeader, File as DicomFile, Object, Tag, UNDEFINED_LENGTH, Value, Vr,
};
use sha2::{Digest, Sha256};
use std::cell::RefCell;
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::rc::Rc;
const fn tag(group: u16, element: u16) -> Tag {
    Tag { group, element }
}
#[derive(Clone)]
struct MimePart {
    path: PathBuf,
    content_type: String,
    transfer_syntax_uid: String,
    semantic_failure: bool,
    content_location: String,
    content_id: String,
    size: i64,
    digest: [u8; 32],
}
struct MetaInstance {
    object: Object,
    refs: Vec<String>,
}
struct RawParts(Vec<MimePart>);
impl Drop for RawParts {
    fn drop(&mut self) {
        let _ = cleanup_mime_parts(&self.0);
    }
}
fn cleanup_mime_parts(parts: &[MimePart]) -> Result<(), ServerError> {
    let mut failed = false;
    for part in parts {
        if let Err(e) = fs::remove_file(&part.path)
            && e.kind() != io::ErrorKind::NotFound
        {
            failed = true;
        }
    }
    if failed {
        Err(ServerError::Backend)
    } else {
        Ok(())
    }
}
fn has_content_coding(header: &Header) -> bool {
    !header.values("Content-Encoding").is_empty()
        || !header.values("Content-Transfer-Encoding").is_empty()
}
fn param<'p>(params: &'p [(String, String)], name: &str) -> Option<&'p str> {
    params
        .iter()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.as_str())
}
fn ele_uid(value: &str) -> bool {
    crate::object::transfer::normalize_uid(value.trim()) == EXPLICIT_VR_LITTLE_ENDIAN_UID
}
fn valid_metadata_outer_parameters(params: &[(String, String)]) -> bool {
    params
        .iter()
        .all(|(name, value)| match to_lower(name).as_str() {
            "boundary" | "type" | "start" | "start-info" => !value.trim().is_empty(),
            "charset" => value.trim().eq_ignore_ascii_case("utf-8"),
            "transfer-syntax" => ele_uid(value),
            _ => false,
        })
}
fn valid_metadata_part_parameters(params: &[(String, String)]) -> bool {
    params
        .iter()
        .all(|(name, value)| match to_lower(name).as_str() {
            "charset" => value.trim().eq_ignore_ascii_case("utf-8"),
            "transfer-syntax" => ele_uid(value),
            _ => false,
        })
}
struct Observed<'a> {
    inner: &'a mut dyn Read,
    err: Option<ServerError>,
}
impl Read for Observed<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let result = self.inner.read(buf);
        if let Err(e) = &result
            && self.err.is_none()
        {
            self.err = Some(io_server_error(e));
        }
        result
    }
}
fn read_mime_part(ctx: &Ctx, part: &MimePart, maximum: i64) -> Result<Vec<u8>, ServerError> {
    ctx_err(ctx)?;
    let mut file = File::open(&part.path).map_err(|_| ServerError::Backend)?;
    match file.metadata() {
        Ok(info) if info.is_file() && info.len() as i64 == part.size => {}
        _ => return Err(ServerError::Backend),
    }
    let mut data = Vec::with_capacity(part.size.max(0) as usize);
    file.read_to_end(&mut data)
        .map_err(|_| ServerError::Backend)?;
    if data.len() as i64 != part.size {
        return Err(ServerError::Backend);
    }
    let digest: [u8; 32] = Sha256::digest(&data).into();
    if digest != part.digest {
        return Err(ServerError::Backend);
    }
    ctx_err(ctx)?;
    if data.len() as i64 > maximum {
        return Err(ServerError::ResourceLimit);
    }
    Ok(data)
}
fn classify_decode_error(text: &str) -> ServerError {
    use crate::dicomjson as j;
    use crate::dicomxml as x;
    let limits = [
        j::ERR_MAX_JSON_BYTES,
        j::ERR_MAX_INLINE_BINARY_BYTES,
        j::ERR_MAX_TOTAL_BINARY_BYTES,
        j::ERR_MAX_SEQUENCE_DEPTH,
        j::ERR_MAX_ELEMENTS,
        j::ERR_MAX_SEQUENCE_ITEMS,
        x::ERR_MAX_XML_BYTES,
        x::ERR_MAX_VALUE_BYTES,
        x::ERR_MAX_INLINE_BYTES,
        x::ERR_MAX_BINARY_BYTES,
        x::ERR_MAX_DEPTH,
        x::ERR_MAX_ELEMENTS,
        x::ERR_MAX_ITEMS,
    ];
    if limits.iter().any(|l| text.contains(l)) {
        ServerError::ResourceLimit
    } else {
        ServerError::InvalidRequest
    }
}
fn raw_element(tag: Tag, vr: Vr, data: Vec<u8>) -> Element {
    Element {
        header: ElementHeader::new(tag, vr, data.len() as u32),
        value: Value::Raw(data),
    }
}
fn compressed_store_us(
    obj: &mut Object,
    t: Tag,
    want: i64,
    derive: bool,
) -> Result<(), ServerError> {
    if obj.get(t).is_some() {
        return match obj.get_int(t) {
            Ok(actual) if actual == want => Ok(()),
            _ => Err(ServerError::InvalidRequest),
        };
    }
    if !derive {
        return Err(ServerError::InvalidRequest);
    }
    obj.put(raw_element(t, Vr::US, (want as u16).to_le_bytes().to_vec()));
    Ok(())
}
fn store_whole_string(
    obj: &mut Object,
    t: Tag,
    vr: Vr,
    want: &str,
    derive: bool,
) -> Result<(), ServerError> {
    if let Some(element) = obj.get(t) {
        if element.vr() != vr {
            return Err(ServerError::InvalidRequest);
        }
        return match store_single_string(obj, t) {
            Some(v) if v == want => Ok(()),
            _ => Err(ServerError::InvalidRequest),
        };
    }
    if !derive {
        return Err(ServerError::InvalidRequest);
    }
    obj.put(raw_element(t, vr, want.as_bytes().to_vec()));
    Ok(())
}
fn store_whole_sequence(
    obj: &Object,
    t: Tag,
    minimum: i64,
    maximum: i64,
) -> Result<(), ServerError> {
    let Some(e) = obj.get(t) else {
        return Err(ServerError::InvalidRequest);
    };
    if e.vr() != Vr::SQ {
        return Err(ServerError::InvalidRequest);
    }
    match &e.value {
        Value::Sequence(items)
            if (items.len() as i64) >= minimum && (items.len() as i64) <= maximum =>
        {
            Ok(())
        }
        _ => Err(ServerError::InvalidRequest),
    }
}
fn store_whole_code(obj: &Object, t: Tag) -> Result<(), ServerError> {
    store_whole_sequence(obj, t, 1, 1)?;
    let items = obj.get_sequence(t).unwrap_or_default();
    if items.len() != 1 {
        return Err(ServerError::InvalidRequest);
    }
    for code in [
        tag(0x0008, 0x0100),
        tag(0x0008, 0x0102),
        tag(0x0008, 0x0104),
    ] {
        store_required_string(&items[0], code)?;
    }
    Ok(())
}
fn clone_object(obj: &Object) -> Object {
    let mut out = Object::from_elements(obj.elements().to_vec());
    out.set_value_big_endian(obj.value_big_endian());
    out
}
struct FrameInfo {
    rows: i64,
    columns: i64,
    components: i64,
    precision: i64,
    uid: &'static str,
    unsigned_derivable: bool,
}
fn inspect_jpeg_frame(data: &[u8], max_bytes: i64) -> Result<FrameInfo, ServerError> {
    use crate::pixel::encapsulated::{Format, jpeg_scan_selection_values, validate_frame};
    if max_bytes <= 0 || data.len() as i64 > max_bytes {
        return Err(ServerError::InvalidRequest);
    }
    validate_frame(data, Format::JPEG).map_err(|_| ServerError::InvalidRequest)?;
    let header = parse_jpeg_frame_header(data).ok_or(ServerError::InvalidRequest)?;
    let (marker, precision, rows, columns, components) = header;
    if rows <= 0
        || columns <= 0
        || (components != 1 && components != 3)
        || !(2..=16).contains(&precision)
    {
        return Err(ServerError::InvalidRequest);
    }
    let uid = match marker {
        0xc0 if precision == 8 => "1.2.840.10008.1.2.4.50",
        0xc1 if precision <= 12 => "1.2.840.10008.1.2.4.51",
        0xc3 => {
            let predictors =
                jpeg_scan_selection_values(data).map_err(|_| ServerError::InvalidRequest)?;
            let mut uid = "1.2.840.10008.1.2.4.70";
            for p in predictors {
                if !(1..=7).contains(&p) {
                    return Err(ServerError::InvalidRequest);
                }
                if p != 1 {
                    uid = "1.2.840.10008.1.2.4.57";
                }
            }
            uid
        }
        _ => return Err(ServerError::InvalidRequest),
    };
    Ok(FrameInfo {
        rows,
        columns,
        components,
        precision,
        uid,
        unsigned_derivable: marker == 0xc0 || marker == 0xc1,
    })
}
fn parse_jpeg_frame_header(data: &[u8]) -> Option<(u8, i64, i64, i64, i64)> {
    if data.len() < 2 || data[0] != 0xff || data[1] != 0xd8 {
        return None;
    }
    let mut i = 2;
    while i + 4 <= data.len() {
        if data[i] != 0xff {
            return None;
        }
        let marker = data[i + 1];
        if marker == 0xff {
            i += 1;
            continue;
        }
        if marker == 0x01 || (0xd0..=0xd7).contains(&marker) {
            i += 2;
            continue;
        }
        if marker == 0xda || marker == 0xd9 {
            return None;
        }
        let length = u16::from_be_bytes([data[i + 2], data[i + 3]]) as usize;
        if length < 2 || i + 2 + length > data.len() {
            return None;
        }
        if (0xc0..=0xcf).contains(&marker) && marker != 0xc4 && marker != 0xc8 && marker != 0xcc {
            if length < 8 {
                return None;
            }
            let s = &data[i + 4..];
            return Some((
                marker,
                i64::from(s[0]),
                i64::from(u16::from_be_bytes([s[1], s[2]])),
                i64::from(u16::from_be_bytes([s[3], s[4]])),
                i64::from(s[5]),
            ));
        }
        i += 2 + length;
    }
    None
}
type Resolved = Rc<RefCell<(HashMap<String, i64>, Option<ServerError>)>>;
impl Inner {
    pub(super) fn stage_store_metadata_request(
        &self,
        r: &mut Request,
        route_study_uid: &str,
        root_type: &str,
        outer_params: &[(String, String)],
    ) -> Result<Vec<StagedPart>, ServerError> {
        if has_content_coding(&r.header) || !valid_metadata_outer_parameters(outer_params) {
            return Err(unsupported_media());
        }
        let ctx = r.context().clone();
        let body = std::mem::replace(&mut r.body, Box::new(io::empty()));
        let mut limited = MaxBytesReader::new(body, self.limits.max_request_bytes);
        let mut observed = Observed {
            inner: &mut limited,
            err: None,
        };
        let boundary = param(outer_params, "boundary").unwrap_or("").to_string();
        let mut raw = RawParts(Vec::new());
        let mut bulk: HashMap<String, MimePart> = HashMap::new();
        let mut content_ids: Vec<String> = Vec::new();
        let mut metadata_parts: Vec<MimePart> = Vec::new();
        let mut metadata_bytes: i64 = 0;
        let mut saw_bulk = false;
        {
            let mut reader = MultipartReader::new(&mut observed, &boundary);
            loop {
                ctx_err(&ctx)?;
                let header = match reader.next_part() {
                    Ok(Some(h)) => h,
                    Ok(None) => break,
                    Err(e) => return Err(classify_multipart_error(e)),
                };
                if raw.0.len() as i64 >= self.limits.max_parts {
                    return Err(ServerError::ResourceLimit);
                }
                if super::super::request_header_bytes(&header) > self.limits.max_part_header_bytes {
                    return Err(ServerError::ResourceLimit);
                }
                if has_content_coding(&header) {
                    return Err(unsupported_media());
                }
                if header.values("Content-Type").len() != 1
                    || header.values("Content-Location").len() > 1
                    || header.values("Content-ID").len() > 1
                {
                    return Err(ServerError::InvalidRequest);
                }
                let parsed = parse_media_type(header.get("Content-Type"))
                    .map_err(|_| unsupported_media())?;
                let part_type = to_lower(parsed.media_type.trim());
                let content_id = header.get("Content-ID").trim().to_string();
                if !content_id.is_empty() {
                    if content_ids.contains(&content_id) {
                        return Err(ServerError::InvalidRequest);
                    }
                    content_ids.push(content_id.clone());
                }
                let is_metadata =
                    part_type == "application/dicom+json" || part_type == "application/dicom+xml";
                if is_metadata {
                    if saw_bulk
                        || part_type != root_type
                        || !valid_metadata_part_parameters(&parsed.params)
                        || (root_type == "application/dicom+json" && !metadata_parts.is_empty())
                    {
                        return Err(ServerError::InvalidRequest);
                    }
                } else {
                    let whole = store_whole_profile(&part_type).is_some();
                    if !store_bulk_part_parameters(&part_type, &parsed.params)
                        && !whole
                        && !part_type.starts_with("video/")
                    {
                        return Err(unsupported_media());
                    }
                    saw_bulk = true;
                }
                let maximum = if is_metadata {
                    self.limits.max_metadata_bytes
                } else {
                    self.limits.max_part_bytes
                };
                let Ok((mut file, path)) =
                    create_temp(&self.options.spool_directory, ".dicomweb-stow-metadata-")
                else {
                    return Err(spool_failure());
                };
                let mut hash = Sha256::new();
                let mut spool_err = false;
                let (size, copied) = {
                    let mut part = PartRead(&mut reader);
                    copy_bounded(
                        &ctx,
                        &mut |d: &[u8]| {
                            if file.write_all(d).is_err() {
                                spool_err = true;
                                return Err(ServerError::Backend);
                            }
                            hash.update(d);
                            Ok(d.len())
                        },
                        &mut part,
                        maximum,
                    )
                };
                drop(file);
                let fail = |e: ServerError| {
                    let _ = fs::remove_file(&path);
                    e
                };
                if spool_err {
                    return Err(fail(spool_failure()));
                }
                if let Err(e) = copied {
                    return Err(fail(classify_multipart_error(e)));
                }
                if size == 0 {
                    return Err(fail(ServerError::InvalidRequest));
                }
                let staged = MimePart {
                    path: path.clone(),
                    content_type: part_type.clone(),
                    transfer_syntax_uid: crate::object::transfer::normalize_uid(
                        param(&parsed.params, "transfer-syntax").unwrap_or(""),
                    )
                    .to_string(),
                    semantic_failure: !is_metadata
                        && !store_bulk_part_parameters(&part_type, &parsed.params),
                    content_location: header.get("Content-Location").trim().to_string(),
                    content_id,
                    size,
                    digest: hash.finalize().into(),
                };
                raw.0.push(staged.clone());
                if is_metadata {
                    metadata_bytes += staged.size;
                    if metadata_bytes > self.limits.max_metadata_bytes {
                        return Err(ServerError::ResourceLimit);
                    }
                    metadata_parts.push(staged);
                    continue;
                }
                if staged.content_location.is_empty() || bulk.contains_key(&staged.content_location)
                {
                    return Err(ServerError::InvalidRequest);
                }
                bulk.insert(staged.content_location.clone(), staged);
            }
        }
        if let Some(e) = observed.err.take() {
            return Err(classify_multipart_error(e));
        }
        let (_, drained) = copy_bounded(
            &ctx,
            &mut |d: &[u8]| Ok(d.len()),
            &mut observed,
            self.limits.max_request_bytes,
        );
        if let Err(e) = drained {
            return Err(classify_multipart_error(e));
        }
        if let Some(e) = observed.err.take() {
            return Err(classify_multipart_error(e));
        }
        ctx_err(&ctx)?;
        if metadata_parts.is_empty() || raw.0.is_empty() {
            return Err(ServerError::InvalidRequest);
        }
        if let Some(start) = param(outer_params, "start").map(str::trim)
            && !start.is_empty()
            && !metadata_parts.iter().any(|p| p.content_id == start)
        {
            return Err(ServerError::InvalidRequest);
        }
        let instances = self.decode_store_metadata(&ctx, root_type, &metadata_parts)?;
        if instances.is_empty() || instances.len() as i64 > self.limits.max_parts {
            return Err(ServerError::ResourceLimit);
        }
        let mut reference_counts: HashMap<String, i64> = HashMap::new();
        for instance in &instances {
            for r in &instance.refs {
                for part in store_referenced_parts(instance, r, &bulk, self.limits.max_parts)? {
                    *reference_counts
                        .entry(part.content_location.clone())
                        .or_default() += 1;
                }
            }
        }
        for (location, part) in &bulk {
            let count = reference_counts.get(location).copied().unwrap_or(0);
            let whole = store_whole_profile(&part.content_type).is_some();
            if (whole || part.semantic_failure) && count != 1 || count < 1 {
                return Err(ServerError::InvalidRequest);
            }
        }
        let mut staged_instances: Vec<StagedPart> = Vec::with_capacity(instances.len());
        let mut staged_bytes: i64 = 0;
        for instance in instances {
            let remaining = self.limits.max_request_bytes - staged_bytes;
            let maximum = self.limits.max_decoded_part_bytes.min(remaining);
            if maximum <= 0 {
                let _ = super::cleanup_staged(&staged_instances);
                return Err(ServerError::ResourceLimit);
            }
            let mut candidate = self.write_staged_metadata_instance(
                &ctx,
                &instance,
                &bulk,
                route_study_uid,
                maximum,
            );
            if let Err(e) = &candidate
                && store_instance_uses_whole_bulk(&instance, &bulk)
                && (e.is(&ServerError::InvalidRequest) || e.is(&ServerError::Unsupported))
            {
                candidate = self.failed_whole_store_instance(&instance, route_study_uid);
            }
            match candidate {
                Ok(c) => {
                    staged_bytes += c.size;
                    staged_instances.push(c);
                }
                Err(e) => {
                    let _ = super::cleanup_staged(&staged_instances);
                    return Err(e);
                }
            }
        }
        let parts = std::mem::take(&mut raw.0);
        if let Err(e) = cleanup_mime_parts(&parts) {
            let _ = super::cleanup_staged(&staged_instances);
            return Err(e);
        }
        Ok(staged_instances)
    }
    fn decode_store_metadata(
        &self,
        ctx: &Ctx,
        root_type: &str,
        parts: &[MimePart],
    ) -> Result<Vec<MetaInstance>, ServerError> {
        let l = &self.limits;
        let mut objects: Vec<Object> = Vec::new();
        match root_type {
            "application/dicom+json" => {
                if parts.len() != 1 {
                    return Err(ServerError::InvalidRequest);
                }
                let data = read_mime_part(ctx, &parts[0], l.max_metadata_bytes)?;
                let datasets = raw_json_array(&data).ok_or(ServerError::InvalidRequest)?;
                if datasets.is_empty() {
                    return Err(ServerError::InvalidRequest);
                }
                if datasets.len() as i64 > l.max_parts {
                    return Err(ServerError::ResourceLimit);
                }
                for dataset in datasets {
                    let opts = crate::dicomjson::UnmarshalOptions {
                        transfer_syntax: Some(crate::object::transfer::EXPLICIT_VR_LITTLE_ENDIAN),
                        limits: crate::dicomjson::Limits {
                            max_json_bytes: dataset.len() as i64,
                            max_inline_binary_bytes: l.max_metadata_bytes,
                            max_total_binary_bytes: l.max_metadata_bytes,
                            max_sequence_depth: l.max_json_depth,
                            max_elements: l.max_json_values,
                            max_sequence_items: l.max_json_values,
                        },
                        ..Default::default()
                    };
                    ctx_err(ctx)?;
                    let obj = crate::dicomjson::unmarshal_text(&dataset, &opts)
                        .map_err(|e| classify_decode_error(&e))?;
                    objects.push(obj);
                }
            }
            "application/dicom+xml" => {
                for part in parts {
                    let data = read_mime_part(ctx, part, i64::MAX)?;
                    let mut opts = crate::dicomxml::UnmarshalOptions {
                        transfer_syntax: Some(crate::object::transfer::EXPLICIT_VR_LITTLE_ENDIAN),
                        limits: crate::dicomxml::Limits {
                            max_xml_bytes: part.size,
                            max_value_bytes: l.max_metadata_bytes.min(l.max_json_value_bytes),
                            max_inline_binary_bytes: l.max_metadata_bytes,
                            max_total_binary_bytes: l.max_metadata_bytes,
                            max_sequence_depth: l.max_json_depth,
                            max_elements: l.max_json_values,
                            max_sequence_items: l.max_json_values,
                        },
                        ..Default::default()
                    };
                    let obj = crate::dicomxml::unmarshal_text(&data, &mut opts)
                        .map_err(|e| classify_decode_error(&e))?;
                    objects.push(obj);
                }
            }
            _ => return Err(ServerError::InvalidRequest),
        }
        let mut instances = Vec::with_capacity(objects.len());
        let mut total: i64 = 0;
        for obj in objects {
            let mut refs: Vec<String> = Vec::new();
            let mut failure: Option<ServerError> = None;
            let walked = obj.walk_with(
                crate::object::WalkOptions {
                    max_depth: l.max_json_depth,
                    max_elements: l.max_json_values,
                },
                |_, elem: &Element| {
                    let stop = |failure: &mut Option<ServerError>, e: ServerError| {
                        *failure = Some(e);
                        Err(crate::object::error::Error::msg("stop"))
                    };
                    total += 1;
                    if total > l.max_json_values {
                        return stop(&mut failure, ServerError::ResourceLimit);
                    }
                    if elem.tag().group == 0x0002 {
                        return stop(&mut failure, ServerError::InvalidRequest);
                    }
                    if let Value::BulkData(uri) = &elem.value {
                        let trimmed = uri.trim();
                        if trimmed.is_empty() || trimmed != uri {
                            return stop(&mut failure, ServerError::InvalidRequest);
                        }
                        refs.push(trimmed.to_string());
                    }
                    Ok(())
                },
            );
            if let Some(e) = failure {
                return Err(e);
            }
            if let Err(e) = walked {
                if e.is(crate::object::error::Kind::WalkResourceLimit) {
                    return Err(ServerError::ResourceLimit);
                }
                return Err(ServerError::InvalidRequest);
            }
            instances.push(MetaInstance { object: obj, refs });
        }
        Ok(instances)
    }
    #[allow(clippy::type_complexity)]
    fn prepare_whole_store(
        &self,
        ctx: &Ctx,
        instance: &MetaInstance,
        bulk: &HashMap<String, MimePart>,
    ) -> Result<(Option<Object>, Syntax, String, String), ServerError> {
        let syntax = crate::object::transfer::EXPLICIT_VR_LITTLE_ENDIAN;
        let l = &self.limits;
        let mut found: Option<(Tag, MimePart, WholeProfile, String)> = None;
        let mut failure: Option<ServerError> = None;
        let walked = instance.object.walk_with(
            crate::object::WalkOptions {
                max_depth: l.max_json_depth,
                max_elements: l.max_json_values,
            },
            |path: &[Tag], e: &Element| {
                let Value::BulkData(uri) = &e.value else {
                    return Ok(());
                };
                let Some(p) = bulk.get(uri) else {
                    return Ok(());
                };
                let invalid = |failure: &mut Option<ServerError>| {
                    *failure = Some(ServerError::InvalidRequest);
                    Err(crate::object::error::Error::msg("stop"))
                };
                if p.semantic_failure {
                    return invalid(&mut failure);
                }
                let Some(w) = store_whole_profile(&p.content_type) else {
                    return Ok(());
                };
                if found.is_some()
                    || path.len() != 1
                    || w.video && e.tag() != Tag::PIXEL_DATA
                    || !w.video && e.tag() != STORE_DOCUMENT_TAG
                    || e.vr() != Vr::OB
                {
                    return invalid(&mut failure);
                }
                found = Some((e.tag(), p.clone(), w, uri.clone()));
                Ok(())
            },
        );
        if let Some(e) = failure {
            return Err(e);
        }
        walked.map_err(|_| ServerError::InvalidRequest)?;
        let Some((target, part, profile, reference)) = found else {
            if let Some(e) = instance.object.get(STORE_DOCUMENT_TAG)
                && matches!(e.value, Value::BulkData(_))
            {
                return Err(ServerError::InvalidRequest);
            }
            return Ok((None, syntax, String::new(), String::new()));
        };
        for other in &instance.refs {
            if *other != reference && !bulk.contains_key(other) {
                return Err(ServerError::InvalidRequest);
            }
        }
        let mut obj = clone_object(&instance.object);
        let sop = store_required_string(&obj, tag(0x0008, 0x0016))?;
        for t in [tag(0x0020, 0x0011), tag(0x0020, 0x0013)] {
            let Some(element) = obj.get(t) else {
                return Err(ServerError::InvalidRequest);
            };
            let Some(values) = obj.get_strings(t) else {
                return Err(ServerError::InvalidRequest);
            };
            if element.vr() != Vr::IS || values.len() > 1 {
                return Err(ServerError::InvalidRequest);
            }
            let value = values.first().map_or("", |v| v.trim());
            if value.is_empty() {
                if t == tag(0x0020, 0x0013) && !profile.video {
                    return Err(ServerError::InvalidRequest);
                }
            } else if value.parse::<i32>().is_err() {
                return Err(ServerError::InvalidRequest);
            }
        }
        let max = l.max_part_bytes.min(l.max_decoded_part_bytes).min(1 << 30);
        let data = read_mime_part(ctx, &part, max)?;
        if profile.video {
            if !VIDEO_SOP_CLASSES.contains(&sop.as_str()) {
                return Err(ServerError::InvalidRequest);
            }
            if obj.get(STORE_DOCUMENT_TAG).is_some() {
                return Err(ServerError::InvalidRequest);
            }
            let modality = match sop.as_str() {
                "1.2.840.10008.5.1.4.1.1.77.1.1.1" => "ES",
                "1.2.840.10008.5.1.4.1.1.77.1.2.1" => "GM",
                _ => "XC",
            };
            store_whole_string(&mut obj, tag(0x0008, 0x0060), Vr::CS, modality, false)?;
            let region = tag(0x0008, 0x2218);
            if obj.get(region).is_some() {
                store_whole_code(&obj, region)?;
            } else {
                store_whole_sequence(&obj, tag(0x0040, 0x0560), 1, l.max_json_values)?;
            }
            let info = crate::video::inspect_source(
                &data,
                crate::video::ImportLimits {
                    max_bytes: max,
                    max_frames: l.max_frames.min(100_000),
                    max_pixels: l.max_decoded_part_bytes / 3,
                    max_decoded_bytes: l.max_decoded_part_bytes,
                },
            )
            .map_err(|e| {
                if e.kind == crate::video::Kind::MediaTooLarge {
                    ServerError::ResourceLimit
                } else {
                    ServerError::InvalidRequest
                }
            })?;
            if profile.media == "video/mp4" && info.container != crate::video::CONTAINER_MP4
                || profile.media == "video/mpeg"
                    && info.container != crate::video::CONTAINER_MPEG_PROGRAM
                || !part.transfer_syntax_uid.is_empty()
                    && part.transfer_syntax_uid != info.transfer_syntax.uid
            {
                return Err(ServerError::InvalidRequest);
            }
            for (t, want) in [
                (tag(0x0028, 0x0010), info.rows),
                (tag(0x0028, 0x0011), info.columns),
                (tag(0x0028, 0x0002), 3),
                (tag(0x0028, 0x0100), 8),
                (tag(0x0028, 0x0101), 8),
                (tag(0x0028, 0x0102), 7),
                (tag(0x0028, 0x0103), 0),
                (tag(0x0028, 0x0006), 0),
            ] {
                if let Some(old) = obj.get(t)
                    && old.vr() != Vr::US
                {
                    return Err(ServerError::InvalidRequest);
                }
                compressed_store_us(&mut obj, t, want, true)?;
            }
            for (t, value) in [
                (tag(0x0028, 0x0004), "YBR_PARTIAL_420"),
                (tag(0x0028, 0x2110), "01"),
                (tag(0x0028, 0x2114), info.compression_method),
            ] {
                store_whole_string(&mut obj, t, Vr::CS, value, true)?;
            }
            store_whole_string(
                &mut obj,
                tag(0x0028, 0x0008),
                Vr::IS,
                &info.frames.to_string(),
                true,
            )?;
            let frame_time = 1000.0 / info.frame_rate;
            let ft = tag(0x0018, 0x1063);
            if let Some(element) = obj.get(ft) {
                let vr = element.vr();
                let v = store_single_string(&obj, ft);
                let f = v.as_deref().map(crate::numtext::parse_float);
                match (vr, f) {
                    (Vr::DS, Some(Ok(f)))
                        if f.is_finite() && (f - frame_time).abs() <= (frame_time.abs() * 1e-8) => {
                    }
                    _ => return Err(ServerError::InvalidRequest),
                }
            } else {
                obj.put(raw_element(
                    ft,
                    Vr::DS,
                    crate::numtext::format_float(frame_time, 'g', 12).into_bytes(),
                ));
            }
            let pointer = tag(0x0028, 0x0009);
            if let Some(existing) = obj.get(pointer) {
                if existing.vr() != Vr::AT || existing.raw() != Some(&[0x18, 0x00, 0x63, 0x10][..])
                {
                    return Err(ServerError::InvalidRequest);
                }
            } else {
                obj.put(raw_element(pointer, Vr::AT, vec![0x18, 0x00, 0x63, 0x10]));
            }
            obj.put(Element {
                header: ElementHeader::new(target, Vr::OB, UNDEFINED_LENGTH),
                value: Value::Fragments {
                    offset_table: Vec::new(),
                    fragments: vec![data],
                },
            });
            return Ok((Some(obj), info.transfer_syntax, reference, String::new()));
        }
        if sop != profile.sop || obj.get(Tag::PIXEL_DATA).is_some() {
            return Err(ServerError::InvalidRequest);
        }
        store_whole_string(
            &mut obj,
            tag(0x0008, 0x0060),
            Vr::CS,
            profile.modality,
            false,
        )?;
        let declared = store_required_string(&obj, STORE_DOCUMENT_MIME_TAG)?;
        match parse_media_type(&declared) {
            Ok(m) if to_lower(&m.media_type) == profile.media && m.params.is_empty() => {}
            _ => return Err(ServerError::InvalidRequest),
        }
        let burned = store_required_string(&obj, tag(0x0028, 0x0301))?;
        if burned != "YES" && burned != "NO" {
            return Err(ServerError::InvalidRequest);
        }
        for (t, vr) in [
            (tag(0x0008, 0x0023), Vr::DA),
            (tag(0x0008, 0x0033), Vr::TM),
            (tag(0x0008, 0x002a), Vr::DT),
            (tag(0x0042, 0x0010), Vr::ST),
        ] {
            match obj.get(t) {
                Some(e) if e.vr() == vr => {}
                _ => return Err(ServerError::InvalidRequest),
            }
        }
        store_whole_sequence(&obj, tag(0x0040, 0xa043), 0, 1)?;
        let info = crate::encapdoc::inspect_payload(
            profile.media,
            &data,
            crate::encapdoc::PayloadLimits {
                max_bytes: max,
                max_depth: l.max_json_depth.min(256),
                max_records: l.max_json_values.min(1_000_000),
            },
        )
        .map_err(|e| {
            if e.is(crate::encapdoc::Kind::TooLarge) {
                ServerError::ResourceLimit
            } else {
                ServerError::InvalidRequest
            }
        })?;
        if !info.hl7_instance_identifier.is_empty() {
            store_whole_string(
                &mut obj,
                tag(0x0040, 0xe001),
                Vr::ST,
                &info.hl7_instance_identifier,
                true,
            )?;
        }
        if profile.modality == "M3D" {
            if profile.sop != "1.2.840.10008.5.1.4.1.1.104.5" {
                let frame_of_reference = store_required_string(&obj, tag(0x0020, 0x0052))?;
                if !crate::object::is_valid_uid(frame_of_reference.as_bytes()) {
                    return Err(ServerError::InvalidRequest);
                }
                if obj.get(tag(0x0020, 0x1040)).is_none() {
                    return Err(ServerError::InvalidRequest);
                }
            }
            store_whole_sequence(&obj, tag(0x0040, 0x08ea), 1, 1)?;
            let items = obj.get_sequence(tag(0x0040, 0x08ea)).unwrap_or_default();
            for t in [
                tag(0x0008, 0x0100),
                tag(0x0008, 0x0102),
                tag(0x0008, 0x0104),
            ] {
                store_required_string(items.first().ok_or(ServerError::InvalidRequest)?, t)?;
            }
        }
        let length_tag = tag(0x0042, 0x0015);
        if let Some(element) = obj.get(length_tag) {
            let n = obj.get_int(length_tag);
            let known = matches!(&element.value, Value::Raw(b) if b.len() == 4);
            if element.vr() != Vr::UL || !known || n != Ok(part.size) {
                return Err(ServerError::InvalidRequest);
            }
        } else {
            obj.put(raw_element(
                length_tag,
                Vr::UL,
                (part.size as u32).to_le_bytes().to_vec(),
            ));
        }
        Ok((Some(obj), syntax, String::new(), reference))
    }
    #[allow(clippy::type_complexity)]
    fn prepare_compressed_store(
        &self,
        ctx: &Ctx,
        object: &Object,
        bulk: &HashMap<String, MimePart>,
    ) -> Result<(Option<Object>, Syntax, Option<Vec<Vec<u8>>>, String), ServerError> {
        let ele = crate::object::transfer::EXPLICIT_VR_LITTLE_ENDIAN;
        let Some(Value::BulkData(uri)) = object.get(Tag::PIXEL_DATA).map(|e| &e.value) else {
            return Ok((None, ele, None, String::new()));
        };
        let instance = MetaInstance {
            object: clone_object(object),
            refs: Vec::new(),
        };
        let parts = store_referenced_parts(&instance, uri, bulk, self.limits.max_parts)?;
        if parts[0].content_type == "application/octet-stream" {
            return Ok((None, ele, None, String::new()));
        }
        let count = store_metadata_frame_count(object)?;
        if count != parts.len() as i64 {
            return Err(ServerError::InvalidRequest);
        }
        let mut occurrences = 0;
        let mut bad = false;
        let _ = object.walk(|path: &[Tag], e: &Element| {
            if let Value::BulkData(b) = &e.value
                && b == uri
            {
                occurrences += 1;
                if path.len() != 1 || e.tag() != Tag::PIXEL_DATA {
                    bad = true;
                    return Err(crate::object::error::Error::msg("stop"));
                }
            }
            Ok(())
        });
        if bad || occurrences != 1 {
            return Err(ServerError::InvalidRequest);
        }
        let l = &self.limits;
        let mut obj = clone_object(object);
        let mut syntax_uid = String::new();
        let mut frames: Vec<Vec<u8>> = Vec::with_capacity(parts.len());
        for part in &parts {
            ctx_err(ctx)?;
            let data = read_mime_part(ctx, part, l.max_part_bytes)?;
            let info = match part.content_type.as_str() {
                "image/jpeg" => inspect_jpeg_frame(&data, l.max_part_bytes)?,
                #[cfg(feature = "jpeg-ls")]
                "image/jls" => {
                    let i = crate::codec::jpegls::inspect_codestream(
                        &data,
                        l.max_part_bytes,
                        Default::default(),
                    )
                    .map_err(|_| ServerError::InvalidRequest)?;
                    FrameInfo {
                        rows: i.rows,
                        columns: i.columns,
                        components: i.components,
                        precision: i.precision,
                        uid: if i.near != 0 {
                            "1.2.840.10008.1.2.4.81"
                        } else {
                            "1.2.840.10008.1.2.4.80"
                        },
                        unsigned_derivable: false,
                    }
                }
                _ => return Err(ServerError::Unsupported),
            };
            let mut uid = info.uid.to_string();
            let declared = &part.transfer_syntax_uid;
            if !declared.is_empty() {
                if *declared != uid
                    && !(declared == "1.2.840.10008.1.2.4.57" && uid == "1.2.840.10008.1.2.4.70")
                {
                    return Err(ServerError::InvalidRequest);
                }
                uid = declared.clone();
            }
            if !syntax_uid.is_empty() && syntax_uid != uid {
                return Err(ServerError::InvalidRequest);
            }
            syntax_uid = uid.clone();
            let (rows, columns, components, precision) =
                (info.rows, info.columns, info.components, info.precision);
            if rows < 1
                || columns < 1
                || rows > 65535
                || columns > 65535
                || (components != 1 && components != 3)
                || !(2..=16).contains(&precision)
            {
                return Err(ServerError::InvalidRequest);
            }
            let allocated = if precision > 8 { 16 } else { 8 };
            let decoded =
                (rows as u64) * (columns as u64) * (components as u64) * (allocated as u64 / 8);
            if decoded > l.max_decoded_part_bytes as u64 {
                return Err(ServerError::ResourceLimit);
            }
            for (t, want) in [
                (tag(0x0028, 0x0010), rows),
                (tag(0x0028, 0x0011), columns),
                (tag(0x0028, 0x0002), components),
                (tag(0x0028, 0x0100), allocated),
                (tag(0x0028, 0x0101), precision),
                (tag(0x0028, 0x0102), precision - 1),
            ] {
                compressed_store_us(&mut obj, t, want, true)?;
            }
            let sign = tag(0x0028, 0x0103);
            if info.unsigned_derivable {
                compressed_store_us(&mut obj, sign, 0, true)?;
            } else {
                match obj.get_int(sign) {
                    Ok(0 | 1) => {}
                    _ => return Err(ServerError::InvalidRequest),
                }
            }
            let photo_tag = tag(0x0028, 0x0004);
            match obj.get_string(photo_tag) {
                None => {
                    if components != 1 {
                        return Err(ServerError::InvalidRequest);
                    }
                    obj.put(raw_element(photo_tag, Vr::CS, b"MONOCHROME2".to_vec()));
                }
                Some(photo) => {
                    let photo = photo.trim();
                    if components == 1 && photo != "MONOCHROME1" && photo != "MONOCHROME2"
                        || components == 3
                            && photo != "RGB"
                            && photo != "YBR_FULL"
                            && photo != "YBR_FULL_422"
                    {
                        return Err(ServerError::InvalidRequest);
                    }
                }
            }
            if components == 3 {
                compressed_store_us(&mut obj, tag(0x0028, 0x0006), 0, true)?;
            }
            let mut frame_obj = clone_object(&obj);
            frame_obj.put(raw_element(tag(0x0028, 0x0008), Vr::IS, b"1".to_vec()));
            let fragments = vec![data];
            crate::pixel::pixeldata::decode_frames(
                &uid,
                &crate::pixel::pixeldata::PixelData::Encapsulated {
                    offset_table: &[],
                    fragments: &fragments,
                },
                &frame_obj,
            )
            .map_err(|_| ServerError::InvalidRequest)?;
            frames.extend(fragments);
        }
        let syntax =
            crate::object::transfer::lookup(&syntax_uid).ok_or(ServerError::Unsupported)?;
        Ok((Some(obj), syntax, Some(frames), uri.clone()))
    }
    fn write_staged_metadata_instance(
        &self,
        ctx: &Ctx,
        instance: &MetaInstance,
        bulk: &HashMap<String, MimePart>,
        route_study_uid: &str,
        maximum: i64,
    ) -> Result<StagedPart, ServerError> {
        let (whole_obj, whole_syntax, whole_pixel_ref, document_ref) =
            self.prepare_whole_store(ctx, instance, bulk)?;
        let object = whole_obj.unwrap_or_else(|| clone_object(&instance.object));
        let (compressed_obj, mut syntax, frames, mut pixel_ref) =
            self.prepare_compressed_store(ctx, &object, bulk)?;
        if !whole_pixel_ref.is_empty() {
            syntax = whole_syntax;
            pixel_ref = whole_pixel_ref;
        }
        let mut object = compressed_obj.unwrap_or(object);
        let resolved: Resolved = Rc::new(RefCell::new((HashMap::new(), None)));
        let resolve = {
            let resolved = resolved.clone();
            let ctx = ctx.clone();
            let bulk = bulk.clone();
            let document_ref = document_ref.clone();
            move |uri: &str| -> Result<Vec<u8>, ServerError> {
                ctx_err(&ctx)?;
                let Some(part) = bulk.get(uri) else {
                    return Err(ServerError::InvalidRequest);
                };
                if part.content_type != "application/octet-stream" && uri != document_ref {
                    return Err(ServerError::InvalidRequest);
                }
                let data = read_mime_part(&ctx, part, i64::MAX)?;
                *resolved.borrow_mut().0.entry(uri.to_string()).or_default() += 1;
                Ok(data)
            }
        };
        let written = match &frames {
            Some(frames) => {
                let mut failure = None;
                substitute_bulk(&mut object, &resolve, &mut failure);
                if let Some(e) = failure {
                    return Err(classify_write_error(e));
                }
                let file = dicom_file(object, syntax);
                crate::object::frame_writer::write_file_frames(
                    &file,
                    crate::object::frame_writer::FrameSource {
                        frames,
                        packed_bits: None,
                    },
                    &crate::object::frame_writer::FrameWriteOptions {
                        max_fragments: self.limits.max_parts,
                        ..Default::default()
                    },
                )
                .map_err(|_| ServerError::InvalidRequest)?
            }
            None => {
                let slot = resolved.clone();
                let resolve = Rc::new(resolve);
                let resolver: crate::object::bulk::BulkDataResolver = {
                    let resolve = resolve.clone();
                    Rc::new(move |uri: &str| match resolve(uri) {
                        Ok(data) => Ok(BulkDataSource {
                            size: data.len() as i64,
                            reader: Some(data),
                        }),
                        Err(e) => {
                            slot.borrow_mut().1.get_or_insert(e);
                            Err(crate::object::error::Error::msg("bulk data"))
                        }
                    })
                };
                let file = dicom_file(object, syntax);
                match write_file_with_bulk_data(
                    &file,
                    &WriteFileOptions::default(),
                    &BulkDataWriteOptions {
                        resolver: Some(resolver),
                        size: None,
                    },
                ) {
                    Ok(out) => out,
                    Err(_) => {
                        let e = resolved.borrow_mut().1.take();
                        return Err(e.map_or(ServerError::InvalidRequest, classify_write_error));
                    }
                }
            }
        };
        if written.len() as i64 > maximum {
            return Err(ServerError::ResourceLimit);
        }
        let mut expected: HashMap<String, i64> = HashMap::new();
        for r in &instance.refs {
            if *r != pixel_ref {
                *expected.entry(r.clone()).or_default() += 1;
            }
        }
        let counts = resolved.borrow().0.clone();
        if expected != counts {
            return Err(ServerError::InvalidRequest);
        }
        let Ok((mut file, path)) = create_temp(&self.options.spool_directory, ".dicomweb-stow-")
        else {
            return Err(spool_failure());
        };
        if file.write_all(&written).and_then(|_| file.flush()).is_err() {
            drop(file);
            let _ = fs::remove_file(&path);
            return Err(spool_failure());
        }
        drop(file);
        let mut candidate = StagedPart {
            path: path.clone(),
            content_type: "application/dicom".into(),
            transfer_syntax_uid: syntax.uid.to_string(),
            size: written.len() as i64,
            digest: Sha256::digest(&written).into(),
            reference: InstanceRef::default(),
            sop_class_uid: String::new(),
            preflight_failure: 0,
        };
        if let Err(e) = self.inspect_staged_part(ctx, &mut candidate, route_study_uid) {
            let _ = fs::remove_file(&path);
            return Err(e);
        }
        Ok(candidate)
    }
    fn failed_whole_store_instance(
        &self,
        instance: &MetaInstance,
        route_study_uid: &str,
    ) -> Result<StagedPart, ServerError> {
        let mut values = Vec::with_capacity(4);
        for t in [
            tag(0x0008, 0x0016),
            tag(0x0008, 0x0018),
            tag(0x0020, 0x000d),
            tag(0x0020, 0x000e),
        ] {
            let v = store_required_string(&instance.object, t)?;
            self.validate_uid(&v)
                .map_err(|_| ServerError::InvalidRequest)?;
            values.push(v);
        }
        if !route_study_uid.is_empty() && values[2] != route_study_uid {
            return Err(ServerError::Conflict);
        }
        Ok(StagedPart {
            path: PathBuf::new(),
            content_type: String::new(),
            transfer_syntax_uid: String::new(),
            size: 0,
            digest: [0; 32],
            reference: InstanceRef {
                study_instance_uid: values[2].clone(),
                series_instance_uid: values[3].clone(),
                sop_instance_uid: values[1].clone(),
            },
            sop_class_uid: values[0].clone(),
            preflight_failure: 0xc000,
        })
    }
}
fn classify_write_error(e: ServerError) -> ServerError {
    if e.is(&ServerError::Canceled)
        || e.is(&ServerError::DeadlineExceeded)
        || e.is(&ServerError::ResourceLimit)
    {
        return e;
    }
    if e.is(&ServerError::Backend) {
        return ServerError::Backend;
    }
    ServerError::InvalidRequest
}
fn dicom_file(dataset: Object, syntax: Syntax) -> DicomFile {
    DicomFile {
        preamble: Vec::new(),
        meta: Object::new(),
        dataset,
        transfer_syntax: syntax,
        partial_read: None,
        incomplete_pixel_data: None,
    }
}
fn substitute_bulk(
    obj: &mut Object,
    resolve: &dyn Fn(&str) -> Result<Vec<u8>, ServerError>,
    failure: &mut Option<ServerError>,
) {
    let mut elements = obj.elements().to_vec();
    substitute_elements(&mut elements, resolve, failure, true);
    let big = obj.value_big_endian();
    *obj = Object::from_elements(elements);
    obj.set_value_big_endian(big);
}
fn substitute_elements(
    elements: &mut [Element],
    resolve: &dyn Fn(&str) -> Result<Vec<u8>, ServerError>,
    failure: &mut Option<ServerError>,
    top: bool,
) {
    for element in elements {
        if failure.is_some() {
            return;
        }
        match &mut element.value {
            Value::BulkData(uri) if !(top && element.header.tag == Tag::PIXEL_DATA) => {
                match resolve(uri) {
                    Ok(data) => {
                        element.header.length = data.len() as u32;
                        element.value = Value::Raw(data);
                    }
                    Err(e) => *failure = Some(e),
                }
            }
            Value::Sequence(items) => {
                for item in items {
                    substitute_elements(&mut item.elements, resolve, failure, false);
                }
            }
            _ => {}
        }
    }
}
fn store_instance_uses_whole_bulk(
    instance: &MetaInstance,
    bulk: &HashMap<String, MimePart>,
) -> bool {
    if let Some(element) = instance.object.get(STORE_DOCUMENT_TAG)
        && matches!(element.value, Value::BulkData(_))
    {
        return true;
    }
    instance.refs.iter().any(|r| {
        bulk.get(r).is_some_and(|part| {
            store_whole_profile(&part.content_type).is_some() || part.semantic_failure
        })
    })
}
fn store_referenced_parts(
    instance: &MetaInstance,
    reference: &str,
    bulk: &HashMap<String, MimePart>,
    max_parts: i64,
) -> Result<Vec<MimePart>, ServerError> {
    if let Some(direct) = bulk.get(reference) {
        if direct.content_type != "application/octet-stream" {
            let prefix = format!("{reference}/frames/");
            if bulk.keys().any(|k| k.starts_with(&prefix)) {
                return Err(ServerError::InvalidRequest);
            }
        }
        return Ok(vec![direct.clone()]);
    }
    let n = store_metadata_frame_count(&instance.object)?;
    if n > max_parts {
        return Err(ServerError::ResourceLimit);
    }
    let mut parts = Vec::with_capacity(n as usize);
    for i in 1..=n {
        match bulk.get(&format!("{reference}/frames/{i}")) {
            Some(part) if part.content_type == "image/jpeg" || part.content_type == "image/jls" => {
                parts.push(part.clone())
            }
            _ => return Err(ServerError::InvalidRequest),
        }
    }
    Ok(parts)
}
fn raw_json_array(data: &[u8]) -> Option<Vec<Vec<u8>>> {
    use crate::dicomjson::jsonwire::{Decoder, JsonError, as_raw_array};
    let mut dec = Decoder::new(data);
    let value = dec.decode_raw().ok()?;
    if value.first() != Some(&b'[') {
        return None;
    }
    let items: Vec<Vec<u8>> = as_raw_array(value)
        .ok()?
        .into_iter()
        .map(<[u8]>::to_vec)
        .collect();
    match dec.decode_raw() {
        Err(JsonError::Eof) => Some(items),
        _ => None,
    }
}
