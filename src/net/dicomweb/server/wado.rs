use super::super::mimeheader::{Header, parse_media_type};
use super::super::types::InstanceRef;
use super::super::urlrules;
use super::http::{Request, ResponseWriter};
use super::media::{
    MediaPreference, best_matching_media_preference, format_media_type, frame_media_matches_syntax,
    is_frame_media_type, matching_media_preference, media_type_with_transfer_syntax,
    normalized_bulk_content_type, normalized_dicom_part_content_type,
    normalized_frame_content_type, parse_accept_header, parse_media_preferences,
    split_accept_values, web_service_syntax_allowed,
};
use super::qido::bulk_data_token;
use super::spool::{MultipartWriter, ResponseSpool, copy_bounded, io_server_error};
use super::{
    Inner, Operation, ServerError, ServerWriter, check_read_lease, classify_backend_error,
    parse_frames, request_accept_header, resource_parts,
};
use crate::net::ul::Ctx;
use std::io::{self, Read};
type Served = Result<usize, (usize, ServerError)>;
fn fail(err: ServerError) -> Served {
    Err((0, err))
}
pub trait PartReader: Read + Send {
    fn close(&mut self) -> Result<(), ServerError> {
        Ok(())
    }
}
impl PartReader for io::Cursor<Vec<u8>> {}
impl PartReader for io::Empty {}
pub trait ReadAt: Send + Sync {
    fn read_at(&self, buf: &mut [u8], offset: u64) -> io::Result<usize>;
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RetrieveLevel {
    Study,
    Series,
    Instance,
}
impl RetrieveLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            RetrieveLevel::Study => "study",
            RetrieveLevel::Series => "series",
            RetrieveLevel::Instance => "instance",
        }
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct RetrieveRequest {
    pub level: RetrieveLevel,
    pub study_instance_uid: String,
    pub series_instance_uid: String,
    pub sop_instance_uid: String,
    pub accept: Vec<MediaPreference>,
    pub prepared: super::transcode::PreparedSource,
}
impl RetrieveRequest {
    pub fn accepts_original(&self, transfer_syntax_uid: &str) -> bool {
        if !web_service_syntax_allowed(transfer_syntax_uid) {
            return false;
        }
        matching_media_preference(
            &self.accept,
            "application/dicom",
            transfer_syntax_uid,
            self.level != RetrieveLevel::Instance,
        )
        .is_some()
    }
}
pub struct RetrievePart {
    pub reference: InstanceRef,
    pub content_type: String,
    pub transfer_syntax_uid: String,
    pub reader: Option<Box<dyn PartReader>>,
    pub size: i64,
}
pub type RetrieveYield<'a> = dyn FnMut(RetrievePart) -> Result<(), ServerError> + 'a;
pub trait RetrieveBackend: Send + Sync {
    fn retrieve(
        &self,
        ctx: &Ctx,
        request: &RetrieveRequest,
        yield_part: &mut RetrieveYield<'_>,
    ) -> Result<(), ServerError>;
}
#[derive(Clone, Debug, PartialEq)]
pub struct FrameRequest {
    pub reference: InstanceRef,
    pub frames: Vec<i64>,
    pub accept: Vec<MediaPreference>,
}
pub struct FramePartStream {
    pub frame_number: i64,
    pub content_type: String,
    pub transfer_syntax_uid: String,
    pub reader: Option<Box<dyn PartReader>>,
    pub size: i64,
}
pub type FrameYield<'a> = dyn FnMut(FramePartStream) -> Result<(), ServerError> + 'a;
pub trait FrameBackend: Send + Sync {
    fn retrieve_frames(
        &self,
        ctx: &Ctx,
        request: &FrameRequest,
        yield_frame: &mut FrameYield<'_>,
    ) -> Result<(), ServerError>;
}
#[derive(Clone, Debug, PartialEq)]
pub struct BulkDataRequest {
    pub token: String,
    pub accept: Vec<MediaPreference>,
}
#[derive(Default)]
pub struct BulkDataPart {
    pub content_type: String,
    pub reader: Option<Box<dyn PartReader>>,
    pub size: i64,
    pub reader_at: Option<Box<dyn ReadAt>>,
}
pub trait BulkDataBackend: Send + Sync {
    fn retrieve_bulk_data(
        &self,
        ctx: &Ctx,
        request: &BulkDataRequest,
    ) -> Result<BulkDataPart, ServerError>;
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderLevel {
    Study,
    Series,
    Instance,
    Frames,
}
impl RenderLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            RenderLevel::Study => "study",
            RenderLevel::Series => "series",
            RenderLevel::Instance => "instance",
            RenderLevel::Frames => "frames",
        }
    }
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ServerRenderViewport {
    pub width: i64,
    pub height: i64,
    pub source_x: Option<f64>,
    pub source_y: Option<f64>,
    pub source_width: Option<f64>,
    pub source_height: Option<f64>,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ServerRenderWindow {
    pub center: f64,
    pub width: f64,
    pub function: String,
}
#[derive(Clone, Debug, PartialEq)]
pub struct RenderRequest {
    pub level: RenderLevel,
    pub reference: InstanceRef,
    pub frames: Vec<i64>,
    pub thumbnail: bool,
    pub accept: Vec<MediaPreference>,
    pub viewport: Option<ServerRenderViewport>,
    pub window: Option<ServerRenderWindow>,
    pub quality: i64,
    pub quality_set: bool,
    pub annotations: Vec<String>,
    pub icc_profile: String,
    pub max_pixels: i64,
    pub max_output_bytes: i64,
}
pub struct RenderedPart {
    pub reference: InstanceRef,
    pub frame_number: i64,
    pub content_type: String,
    pub width: i64,
    pub height: i64,
    pub reader: Option<Box<dyn PartReader>>,
    pub size: i64,
}
pub type RenderYield<'a> = dyn FnMut(RenderedPart) -> Result<(), ServerError> + 'a;
pub trait Renderer: Send + Sync {
    fn render(
        &self,
        ctx: &Ctx,
        request: &RenderRequest,
        yield_part: &mut RenderYield<'_>,
    ) -> Result<(), ServerError>;
}
pub(super) fn recover<T>(f: impl FnOnce() -> Result<T, ServerError>) -> Result<T, ServerError> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).unwrap_or(Err(ServerError::Backend))
}
pub(super) fn with_reader_closed(
    reader: &mut dyn PartReader,
    consume: impl FnOnce(&mut dyn PartReader) -> Result<(), ServerError>,
) -> Result<(), ServerError> {
    let result = consume(reader);
    let closed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| reader.close()))
        .unwrap_or(Err(ServerError::Backend));
    match (result, closed) {
        (Ok(()), Err(_)) => Err(ServerError::Backend),
        (r, _) => r,
    }
}
pub(super) fn header_with(content_type: &str, location: &str) -> Header {
    let mut h = Header::new();
    h.set("Content-Type", content_type);
    h.set("Content-Location", location);
    h
}
pub(super) fn retrieve_ref_matches(request: &RetrieveRequest, r: &InstanceRef) -> bool {
    if r.study_instance_uid != request.study_instance_uid {
        return false;
    }
    if request.level == RetrieveLevel::Study {
        return !r.series_instance_uid.is_empty() && !r.sop_instance_uid.is_empty();
    }
    if r.series_instance_uid != request.series_instance_uid {
        return false;
    }
    request.level != RetrieveLevel::Instance || r.sop_instance_uid == request.sop_instance_uid
}
fn has_multipart_dicom_preference(prefs: &[MediaPreference]) -> bool {
    prefs.iter().any(|p| {
        p.quality > 0.0
            && ((p.multipart && p.media_type == "application/dicom") || p.media_type == "*/*")
    })
}
pub(super) fn has_multipart_frame_preference(prefs: &[MediaPreference]) -> bool {
    prefs.iter().any(|p| {
        p.quality > 0.0
            && (p.media_type == "*/*"
                || (p.multipart
                    && (is_frame_media_type(&p.media_type)
                        || p.media_type == "image/*"
                        || p.media_type == "video/*")))
    })
}
impl Inner {
    pub(super) fn frame_location(&self, r: &InstanceRef, frame: i64) -> String {
        let location = self.instance_location(r).unwrap_or_default();
        format!("{location}/frames/{frame}")
    }
    fn retrieve_request(
        &self,
        r: &Request,
        operation: Operation,
    ) -> Result<RetrieveRequest, ServerError> {
        let parts = resource_parts(&r.url.path);
        let accept_header = request_accept_header(r);
        let mut accept = parse_media_preferences(&accept_header, "application/dicom");
        if accept_header.trim().is_empty() {
            accept[0].multipart = true;
        }
        let level = match operation {
            Operation::RetrieveStudy => RetrieveLevel::Study,
            Operation::RetrieveSeries => RetrieveLevel::Series,
            _ => RetrieveLevel::Instance,
        };
        if accept.is_empty()
            || (level != RetrieveLevel::Instance && !has_multipart_dicom_preference(&accept))
        {
            return Err(ServerError::status(
                406,
                "not_acceptable",
                ServerError::Unsupported,
            ));
        }
        let get = |i: usize| parts.get(i).cloned().unwrap_or_default();
        let request = RetrieveRequest {
            level,
            study_instance_uid: if parts.len() >= 2 {
                get(1)
            } else {
                String::new()
            },
            series_instance_uid: if parts.len() >= 4 {
                get(3)
            } else {
                String::new()
            },
            sop_instance_uid: if parts.len() >= 6 {
                get(5)
            } else {
                String::new()
            },
            accept,
            prepared: Default::default(),
        };
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
    pub(crate) fn serve_retrieve(
        &self,
        w: &mut ServerWriter<'_>,
        r: &mut Request,
        operation: Operation,
    ) -> Served {
        let Some(backend) = self
            .options
            .backend
            .as_ref()
            .and_then(|b| b.retrieve_backend())
        else {
            return fail(ServerError::status(
                501,
                "not_implemented",
                ServerError::Unsupported,
            ));
        };
        let request = self.retrieve_request(r, operation).map_err(|e| (0, e))?;
        if self.options.retrieve_transcode.is_some() {
            return self.serve_prepared_retrieve(w, r, request, false);
        }
        let ctx = r.context().clone();
        let mut writer: Option<MultipartWriter> = None;
        let mut response_multipart = request.level != RetrieveLevel::Instance;
        let mut count = 0usize;
        let result = {
            let mut yield_part = |mut part: RetrievePart| -> Result<(), ServerError> {
                let Some(mut reader) = part.reader.take() else {
                    return Err(ServerError::Backend);
                };
                with_reader_closed(reader.as_mut(), |reader| {
                    check_read_lease(&ctx)?;
                    if count as i64 >= self.limits.max_results
                        || part.size < -1
                        || part.size > self.limits.max_part_bytes
                    {
                        return Err(ServerError::ResourceLimit);
                    }
                    if request.level == RetrieveLevel::Instance && count > 0 {
                        return Err(ServerError::Backend);
                    }
                    if !part.transfer_syntax_uid.is_empty() {
                        self.validate_uid(&part.transfer_syntax_uid)
                            .map_err(|_| ServerError::Backend)?;
                        if !web_service_syntax_allowed(&part.transfer_syntax_uid) {
                            return Err(ServerError::Backend);
                        }
                    }
                    let content_type = normalized_dicom_part_content_type(
                        &part.content_type,
                        &part.transfer_syntax_uid,
                    )
                    .map_err(|_| ServerError::Unsupported)?;
                    let preference = matching_media_preference(
                        &request.accept,
                        &content_type,
                        &part.transfer_syntax_uid,
                        response_multipart,
                    )
                    .ok_or(ServerError::Unsupported)?;
                    if request.level == RetrieveLevel::Instance && count == 0 {
                        response_multipart = preference.multipart;
                    }
                    let mut reference = part.reference.clone();
                    if request.level == RetrieveLevel::Instance
                        && reference == InstanceRef::default()
                    {
                        reference = InstanceRef {
                            study_instance_uid: request.study_instance_uid.clone(),
                            series_instance_uid: request.series_instance_uid.clone(),
                            sop_instance_uid: request.sop_instance_uid.clone(),
                        };
                    }
                    if !retrieve_ref_matches(&request, &reference) {
                        return Err(ServerError::Backend);
                    }
                    let location = self
                        .instance_location(&reference)
                        .map_err(|_| ServerError::Backend)?;
                    if response_multipart {
                        if writer.is_none() {
                            let mw = MultipartWriter::new();
                            w.header().set(
                                "Content-Type",
                                &format!(
                                    "multipart/related; type=\"application/dicom\"; boundary={}",
                                    mw.boundary()
                                ),
                            );
                            writer = Some(mw);
                        }
                        if let Some(mw) = writer.as_mut() {
                            mw.create_part(
                                &header_with(&content_type, &location),
                                &mut |d: &[u8]| w.write(d),
                            )?;
                        }
                    } else {
                        if count != 0 {
                            return Err(ServerError::Backend);
                        }
                        w.header().set("Content-Type", &content_type);
                        w.header().set("Content-Location", &location);
                    }
                    copy_bounded(
                        &ctx,
                        &mut |d: &[u8]| w.write(d),
                        reader,
                        self.limits.max_part_bytes,
                    )
                    .1?;
                    count += 1;
                    Ok(())
                })
            };
            check_read_lease(&ctx)
                .and_then(|()| recover(|| backend.retrieve(&ctx, &request, &mut yield_part)))
        };
        if let Err(e) = result {
            if count == 0 {
                return fail(classify_backend_error(e));
            }
            return Err((count, e));
        }
        if count == 0 {
            return fail(ServerError::NotFound);
        }
        if let Some(mw) = writer.as_mut() {
            mw.close(&mut |d: &[u8]| w.write(d))
                .map_err(|e| (count, e))?;
        }
        Ok(count)
    }
    pub(crate) fn serve_frames(&self, w: &mut ServerWriter<'_>, r: &mut Request) -> Served {
        if self.options.retrieve_transcode.is_some() {
            return self.serve_prepared_frames(w, r);
        }
        let Some(backend) = self
            .options
            .backend
            .as_ref()
            .and_then(|b| b.frame_backend())
        else {
            return fail(ServerError::status(
                501,
                "not_implemented",
                ServerError::Unsupported,
            ));
        };
        let parts = resource_parts(&r.url.path);
        let reference = InstanceRef {
            study_instance_uid: parts[1].clone(),
            series_instance_uid: parts[3].clone(),
            sop_instance_uid: parts[5].clone(),
        };
        for uid in [
            &reference.study_instance_uid,
            &reference.series_instance_uid,
            &reference.sop_instance_uid,
        ] {
            self.validate_uid(uid).map_err(|e| (0, e))?;
        }
        let frames = parse_frames(&parts[7], self.limits.max_frames).map_err(|e| (0, e))?;
        let accept_header = request_accept_header(r);
        let mut preferences = parse_media_preferences(&accept_header, "application/octet-stream");
        if accept_header.trim().is_empty() {
            preferences[0].multipart = true;
        }
        if preferences.is_empty() || !has_multipart_frame_preference(&preferences) {
            return fail(ServerError::status(
                406,
                "not_acceptable",
                ServerError::Unsupported,
            ));
        }
        let request = FrameRequest {
            reference: reference.clone(),
            frames: frames.clone(),
            accept: preferences.clone(),
        };
        let mut writer = MultipartWriter::new();
        w.header().set(
            "Content-Type",
            &format!(
                "multipart/related; type=\"application/octet-stream\"; boundary={}",
                writer.boundary()
            ),
        );
        let ctx = r.context().clone();
        let mut count = 0usize;
        let result = {
            let mut yield_frame = |mut part: FramePartStream| -> Result<(), ServerError> {
                let Some(mut reader) = part.reader.take() else {
                    return Err(ServerError::Backend);
                };
                with_reader_closed(reader.as_mut(), |reader| {
                    check_read_lease(&ctx)?;
                    if part.size < -1 || part.size > self.limits.max_part_bytes {
                        return Err(ServerError::ResourceLimit);
                    }
                    if count >= frames.len() || part.frame_number != frames[count] {
                        return Err(ServerError::Backend);
                    }
                    if !part.transfer_syntax_uid.is_empty() {
                        self.validate_uid(&part.transfer_syntax_uid)
                            .map_err(|_| ServerError::Backend)?;
                    }
                    let content_type = normalized_frame_content_type(&part.content_type)
                        .map_err(|_| ServerError::Backend)?;
                    if !frame_media_matches_syntax(&content_type, &part.transfer_syntax_uid) {
                        return Err(ServerError::Backend);
                    }
                    if matching_media_preference(
                        &preferences,
                        &content_type,
                        &part.transfer_syntax_uid,
                        true,
                    )
                    .is_none()
                    {
                        return Err(ServerError::Unsupported);
                    }
                    let part_media_type =
                        media_type_with_transfer_syntax(&content_type, &part.transfer_syntax_uid);
                    if count == 0 {
                        w.header().set(
                            "Content-Type",
                            &format!(
                                "multipart/related; type=\"{content_type}\"; boundary={}",
                                writer.boundary()
                            ),
                        );
                    }
                    let location = self.frame_location(&reference, part.frame_number);
                    writer.create_part(
                        &header_with(&part_media_type, &location),
                        &mut |d: &[u8]| w.write(d),
                    )?;
                    copy_bounded(
                        &ctx,
                        &mut |d: &[u8]| w.write(d),
                        reader,
                        self.limits.max_part_bytes,
                    )
                    .1?;
                    count += 1;
                    Ok(())
                })
            };
            check_read_lease(&ctx).and_then(|()| {
                recover(|| backend.retrieve_frames(&ctx, &request, &mut yield_frame))
            })
        };
        if let Err(e) = result {
            if count == 0 {
                return fail(classify_backend_error(e));
            }
            return Err((count, e));
        }
        if count == 0 {
            return fail(ServerError::NotFound);
        }
        if count != frames.len() {
            return Err((count, ServerError::Backend));
        }
        writer
            .close(&mut |d: &[u8]| w.write(d))
            .map_err(|e| (count, e))?;
        Ok(count)
    }
    pub(crate) fn serve_bulk_data(&self, w: &mut ServerWriter<'_>, r: &mut Request) -> Served {
        let Some(backend) = self
            .options
            .backend
            .as_ref()
            .and_then(|b| b.bulk_data_backend())
        else {
            return fail(ServerError::status(
                501,
                "not_implemented",
                ServerError::Unsupported,
            ));
        };
        let parts = resource_parts(&r.url.path);
        if parts.len() < 2 {
            return fail(ServerError::InvalidRequest);
        }
        let token =
            bulk_data_token(&r.url.path, &r.url.raw_path, "/bulkdata/").map_err(|e| (0, e))?;
        let accept_header = request_accept_header(r);
        let mut accept = parse_media_preferences(&accept_header, "application/octet-stream");
        if accept_header.trim().is_empty() {
            accept[0].multipart = true;
        }
        let request = BulkDataRequest { token, accept };
        let ctx = r.context().clone();
        let mut part = check_read_lease(&ctx)
            .and_then(|()| recover(|| backend.retrieve_bulk_data(&ctx, &request)))
            .map_err(|e| (0, classify_backend_error(e)))?;
        let Some(mut reader) = part.reader.take() else {
            return fail(ServerError::Backend);
        };
        let escaped = r.url.escaped_path();
        let result = with_reader_closed(reader.as_mut(), |reader| {
            check_read_lease(&ctx)?;
            if part.size < -1 || part.size > self.limits.max_part_bytes {
                return Err(ServerError::ResourceLimit);
            }
            let content_type = normalized_bulk_content_type(&part.content_type)
                .map_err(|_| ServerError::Backend)?;
            let preference = matching_media_preference(&request.accept, &content_type, "", false)
                .ok_or_else(|| {
                ServerError::status(406, "not_acceptable", ServerError::Unsupported)
            })?;
            let multipart_response = preference.multipart || preference.media_type == "*/*";
            if !multipart_response {
                if self.options.enable_bulk_data_ranges {
                    return self.serve_bulk_range(w, r, &part, reader, &content_type);
                }
                w.header().set("Content-Type", &content_type);
                return copy_bounded(
                    &ctx,
                    &mut |d: &[u8]| w.write(d),
                    reader,
                    self.limits.max_part_bytes,
                )
                .1;
            }
            let mut writer = MultipartWriter::new();
            w.header().set(
                "Content-Type",
                &format!(
                    "multipart/related; type=\"application/octet-stream\"; boundary={}",
                    writer.boundary()
                ),
            );
            let location = self.service_path(&escaped);
            writer.create_part(&header_with(&content_type, &location), &mut |d: &[u8]| {
                w.write(d)
            })?;
            copy_bounded(
                &ctx,
                &mut |d: &[u8]| w.write(d),
                reader,
                self.limits.max_part_bytes,
            )
            .1?;
            writer.close(&mut |d: &[u8]| w.write(d))
        });
        result.map_err(|e| (0, e))?;
        Ok(1)
    }
    fn serve_bulk_range(
        &self,
        w: &mut ServerWriter<'_>,
        r: &Request,
        part: &BulkDataPart,
        reader: &mut dyn PartReader,
        content_type: &str,
    ) -> Result<(), ServerError> {
        let ctx = r.context();
        check_read_lease(ctx)?;
        w.header().set("Content-Type", content_type);
        let Some(reader_at) = part.reader_at.as_ref().filter(|_| part.size >= 0) else {
            w.header().set("Accept-Ranges", "none");
            let max = self.limits.max_part_bytes.min(w.remaining);
            return copy_bounded(ctx, &mut |d: &[u8]| w.write(d), reader, max).1;
        };
        let mut selected = BulkByteRange {
            start: 0,
            length: part.size,
            satisfiable: true,
        };
        let mut partial = false;
        if r.header.values("If-Range").is_empty()
            && r.header.values("Range").len() <= 1
            && let Some(parsed) = parse_bulk_range(r.header.get("Range"), part.size)
        {
            selected = parsed;
            partial = true;
        }
        w.header().set("Accept-Ranges", "bytes");
        if partial && !selected.satisfiable {
            check_read_lease(ctx)?;
            w.header()
                .set("Content-Range", &format!("bytes */{}", part.size));
            w.header().set("Content-Length", "0");
            w.write_header(416);
            return Ok(());
        }
        if selected.length > self.limits.max_part_bytes || selected.length > w.remaining {
            return Err(ServerError::ResourceLimit);
        }
        w.header()
            .set("Content-Length", &selected.length.to_string());
        if partial {
            check_read_lease(ctx)?;
            w.header().set(
                "Content-Range",
                &format!(
                    "bytes {}-{}/{}",
                    selected.start,
                    selected.start + selected.length - 1,
                    part.size
                ),
            );
            w.write_header(206);
        }
        let mut offset = selected.start;
        let limit = selected.start + selected.length;
        let mut buffer = vec![0u8; 32 << 10];
        let mut written: i64 = 0;
        loop {
            check_read_lease(ctx)?;
            let (n, eof) = if offset >= limit {
                (0, true)
            } else {
                let max = ((limit - offset) as usize).min(buffer.len());
                match reader_at.read_at(&mut buffer[..max], offset as u64) {
                    Ok(n) => (n, n < max),
                    Err(e) => {
                        check_read_lease(ctx)?;
                        return Err(io_server_error(&e));
                    }
                }
            };
            offset += n as i64;
            check_read_lease(ctx)?;
            if n > 0 {
                written += w.write(&buffer[..n])? as i64;
            }
            if eof || n == 0 {
                break;
            }
        }
        if written != selected.length {
            return Err(ServerError::Other("unexpected EOF".into()));
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct BulkByteRange {
    pub start: i64,
    pub length: i64,
    pub satisfiable: bool,
}
pub(crate) fn parse_bulk_range(header: &str, size: i64) -> Option<BulkByteRange> {
    let (unit, value) = header.trim().split_once('=')?;
    if !unit.trim().eq_ignore_ascii_case("bytes") || value.contains(',') || size <= 0 {
        return None;
    }
    let (first, last) = value.trim().split_once('-')?;
    if last.contains('-') {
        return None;
    }
    if first.is_empty() {
        let suffix = bulk_range_number(last)?;
        if suffix == 0 {
            return Some(BulkByteRange::default());
        }
        let length = suffix.min(size);
        return Some(BulkByteRange {
            start: size - length,
            length,
            satisfiable: true,
        });
    }
    let start = bulk_range_number(first)?;
    let mut end = size - 1;
    if !last.is_empty() {
        end = bulk_range_number(last)?;
        if end < start {
            return None;
        }
    }
    if start >= size {
        return Some(BulkByteRange::default());
    }
    end = end.min(size - 1);
    Some(BulkByteRange {
        start,
        length: end - start + 1,
        satisfiable: true,
    })
}
fn bulk_range_number(raw: &str) -> Option<i64> {
    if raw.is_empty() {
        return None;
    }
    let mut n: i64 = 0;
    for c in raw.bytes() {
        if !c.is_ascii_digit() {
            return None;
        }
        let digit = (c - b'0') as i64;
        if n > (i64::MAX - digit) / 10 {
            n = i64::MAX;
        } else {
            n = n * 10 + digit;
        }
    }
    Some(n)
}
pub(crate) fn is_rendered_media_type(media_type: &str, thumbnail: bool) -> bool {
    let m = super::super::mimeheader::to_lower(media_type);
    if m.starts_with("image/")
        && matches!(
            m.as_str(),
            "image/jpeg"
                | "image/png"
                | "image/gif"
                | "image/jp2"
                | "image/jph"
                | "image/jphc"
                | "image/jxl"
        )
    {
        return true;
    }
    if thumbnail {
        return false;
    }
    matches!(
        m.as_str(),
        "video/mpeg" | "video/mp4" | "video/h265" | "text/html" | "text/plain" | "application/pdf"
    )
}
fn normalized_rendered_content_type(
    content_type: &str,
    thumbnail: bool,
) -> Result<String, ServerError> {
    match parse_media_type(content_type.trim()) {
        Ok(mt) if mt.params.is_empty() && is_rendered_media_type(&mt.media_type, thumbnail) => {
            Ok(super::super::mimeheader::to_lower(&mt.media_type))
        }
        _ => Err(ServerError::Unsupported),
    }
}
fn rendered_media_supports_quality(media_type: &str) -> bool {
    let m = super::super::mimeheader::to_lower(media_type);
    matches!(
        m.as_str(),
        "image/jpeg" | "image/jp2" | "image/jph" | "image/jphc" | "image/jxl"
    ) || m.starts_with("video/")
}
fn concrete_rendered_preferences(
    prefs: &[MediaPreference],
    thumbnail: bool,
) -> Vec<MediaPreference> {
    let mut result = Vec::new();
    for p in prefs {
        if p.quality <= 0.0 || !p.transfer_syntax_uid.is_empty() {
            continue;
        }
        let mut p = p.clone();
        if p.media_type == "*/*" || p.media_type == "image/*" {
            p.media_type = "image/jpeg".into();
        }
        if is_rendered_media_type(&p.media_type, thumbnail) {
            result.push(p);
        }
    }
    result
}
fn not_acceptable() -> ServerError {
    ServerError::status(406, "not_acceptable", ServerError::Unsupported)
}
fn rendered_preferences(
    header: &str,
    query_accept: &[String],
    thumbnail: bool,
) -> Result<Vec<MediaPreference>, ServerError> {
    let parsed_header = parse_accept_header(header);
    if parsed_header.len() != split_accept_values(header).len() {
        return Err(not_acceptable());
    }
    let (mut header_dicom, mut header_rendered) = (false, false);
    for p in &parsed_header {
        if p.quality <= 0.0 {
            continue;
        }
        if p.media_type == "application/dicom" {
            header_dicom = true;
        } else if is_rendered_media_type(&p.media_type, thumbnail)
            || p.media_type == "*/*"
            || p.media_type == "image/*"
        {
            header_rendered = true;
        }
    }
    if header_dicom && header_rendered {
        return Err(ServerError::InvalidRequest);
    }
    let header_preferences = concrete_rendered_preferences(&parsed_header, thumbnail);
    let compatible: Vec<MediaPreference> = parsed_header
        .iter()
        .filter(|p| p.quality > 0.0 && p.transfer_syntax_uid.is_empty())
        .cloned()
        .collect();
    if query_accept.is_empty() {
        if header_preferences.is_empty() {
            return Err(not_acceptable());
        }
        return Ok(header_preferences);
    }
    if query_accept.len() != 1 || query_accept[0].trim().is_empty() {
        return Err(ServerError::InvalidRequest);
    }
    let raw_values = split_accept_values(&query_accept[0]);
    let query_preferences = parse_accept_header(&query_accept[0]);
    if query_preferences.len() != raw_values.len() || query_preferences.is_empty() {
        return Err(ServerError::InvalidRequest);
    }
    let (mut has_dicom, mut has_rendered) = (false, false);
    for p in &query_preferences {
        if p.media_type == "application/dicom" {
            has_dicom = true;
            continue;
        }
        if p.media_type.contains('*')
            || !p.transfer_syntax_uid.is_empty()
            || !is_rendered_media_type(&p.media_type, thumbnail)
        {
            return Err(ServerError::InvalidRequest);
        }
        has_rendered = true;
        if matching_media_preference(&compatible, &p.media_type, "", p.multipart).is_none()
            && !header.trim().is_empty()
        {
            return Err(not_acceptable());
        }
    }
    if has_dicom && has_rendered {
        return Err(ServerError::InvalidRequest);
    }
    if has_dicom {
        return Err(not_acceptable());
    }
    Ok(query_preferences)
}
fn single_query_value(query: &urlrules::Values, name: &str) -> Option<String> {
    let values = query.0.iter().find(|(k, _)| k == name).map(|(_, v)| v)?;
    if values.len() != 1 || values[0].is_empty() {
        return Some(String::new());
    }
    Some(values[0].clone())
}
fn parse_go_float(s: &str) -> Option<f64> {
    crate::numtext::parse_float(s).ok()
}
fn parse_render_window(raw: &str) -> Result<ServerRenderWindow, ServerError> {
    let parts: Vec<&str> = raw.split(',').collect();
    if parts.len() != 3 {
        return Err(ServerError::InvalidRequest);
    }
    let center = parse_go_float(parts[0])
        .filter(|v| v.is_finite())
        .ok_or(ServerError::InvalidRequest)?;
    let width = parse_go_float(parts[1])
        .filter(|v| v.is_finite() && *v > 0.0)
        .ok_or(ServerError::InvalidRequest)?;
    if !matches!(parts[2], "linear" | "linear-exact" | "sigmoid") {
        return Err(ServerError::InvalidRequest);
    }
    Ok(ServerRenderWindow {
        center,
        width,
        function: parts[2].to_string(),
    })
}
fn is_render_keyword(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
}
fn parse_render_annotations(values: &[String]) -> Result<(Vec<String>, bool), ServerError> {
    if values.is_empty() {
        return Ok((Vec::new(), false));
    }
    if values.len() != 1 || values[0].is_empty() {
        return Err(ServerError::InvalidRequest);
    }
    let mut result = Vec::new();
    let mut warning = false;
    for value in values[0].split(',') {
        if !is_render_keyword(value) {
            return Err(ServerError::InvalidRequest);
        }
        if value != "patient" && value != "technique" {
            warning = true;
            continue;
        }
        if !result.iter().any(|v: &String| v == value) {
            result.push(value.to_string());
        }
    }
    Ok((result, warning))
}
impl Inner {
    fn parse_render_viewport(
        &self,
        raw: &str,
        thumbnail: bool,
    ) -> Result<ServerRenderViewport, ServerError> {
        let parts: Vec<&str> = raw.split(',').collect();
        if parts.len() < 2
            || parts.len() > 6
            || (thumbnail && parts.len() != 2)
            || (parts.len() > 2 && parts[parts.len() - 1].is_empty())
        {
            return Err(ServerError::InvalidRequest);
        }
        let int31 = |s: &str| crate::dicomjson::sizedparse::parse_int(s, 31).ok();
        let width = int31(parts[0])
            .filter(|w| *w > 0)
            .ok_or(ServerError::InvalidRequest)?;
        let invalid_viewport =
            || ServerError::status(400, "invalid_viewport", ServerError::ResourceLimit);
        let height = int31(parts[1])
            .filter(|h| *h > 0)
            .ok_or_else(invalid_viewport)?;
        if (width as u64) > (self.limits.max_rendered_pixels as u64) / (height as u64) {
            return Err(invalid_viewport());
        }
        let mut viewport = ServerRenderViewport {
            width,
            height,
            ..ServerRenderViewport::default()
        };
        for (index, part) in parts.iter().enumerate().skip(2) {
            if part.is_empty() {
                continue;
            }
            let value = parse_go_float(part)
                .filter(|v| {
                    v.is_finite()
                        && v.abs() <= self.limits.max_rendered_pixels as f64
                        && !(index >= 4 && *v == 0.0)
                })
                .ok_or(ServerError::InvalidRequest)?;
            match index {
                2 => viewport.source_x = Some(value),
                3 => viewport.source_y = Some(value),
                4 => viewport.source_width = Some(value),
                _ => viewport.source_height = Some(value),
            }
        }
        Ok(viewport)
    }
    fn render_request(
        &self,
        r: &Request,
        thumbnail: bool,
    ) -> Result<(RenderRequest, bool), ServerError> {
        let parts = resource_parts(&r.url.path);
        let mut request = RenderRequest {
            level: RenderLevel::Study,
            reference: InstanceRef::default(),
            frames: Vec::new(),
            thumbnail,
            accept: Vec::new(),
            viewport: None,
            window: None,
            quality: 0,
            quality_set: false,
            annotations: Vec::new(),
            icc_profile: String::new(),
            max_pixels: self.limits.max_rendered_pixels,
            max_output_bytes: self.limits.max_response_bytes,
        };
        let full = |p: &[String]| InstanceRef {
            study_instance_uid: p[1].clone(),
            series_instance_uid: p[3].clone(),
            sop_instance_uid: p[5].clone(),
        };
        match parts.len() {
            3 => request.reference.study_instance_uid = parts[1].clone(),
            5 => {
                request.level = RenderLevel::Series;
                request.reference.study_instance_uid = parts[1].clone();
                request.reference.series_instance_uid = parts[3].clone();
            }
            7 => {
                request.level = RenderLevel::Instance;
                request.reference = full(&parts);
            }
            9 => {
                request.level = RenderLevel::Frames;
                request.reference = full(&parts);
                request.frames = parse_frames(&parts[7], self.limits.max_frames)?;
            }
            _ => return Err(ServerError::InvalidRequest),
        }
        for uid in [
            &request.reference.study_instance_uid,
            &request.reference.series_instance_uid,
            &request.reference.sop_instance_uid,
        ] {
            if !uid.is_empty() {
                self.validate_uid(uid)?;
            }
        }
        let (query, err) = urlrules::parse_query(&r.url.raw_query);
        if err.is_some() {
            return Err(ServerError::InvalidRequest);
        }
        if query.0.len() as i64 > self.limits.max_query_fields {
            return Err(ServerError::ResourceLimit);
        }
        for (_, values) in &query.0 {
            if values
                .iter()
                .any(|v| v.len() as i64 > self.limits.max_query_value_bytes)
            {
                return Err(ServerError::ResourceLimit);
            }
        }
        for name in [
            "accept",
            "annotation",
            "quality",
            "viewport",
            "window",
            "iccprofile",
        ] {
            if query.values(name).len() > 1 {
                return Err(ServerError::InvalidRequest);
            }
        }
        let preferences =
            rendered_preferences(&request_accept_header(r), query.values("accept"), thumbnail)?;
        request.accept = preferences.clone();
        if let Some(raw) = single_query_value(&query, "viewport") {
            let viewport = self.parse_render_viewport(&raw, thumbnail)?;
            let items = if request.level == RenderLevel::Frames && !thumbnail {
                request.frames.len() as i64
            } else {
                1
            };
            let pixels = viewport.width * viewport.height;
            if items > 0 && pixels > request.max_pixels / items {
                return Err(ServerError::status(
                    400,
                    "invalid_viewport",
                    ServerError::ResourceLimit,
                ));
            }
            request.viewport = Some(viewport);
        }
        if thumbnail {
            return Ok((request, false));
        }
        if let Some(raw) = single_query_value(&query, "quality") {
            let quality = match crate::dicomjson::sizedparse::parse_int(&raw, 64) {
                Ok(q) if (1..=100).contains(&q) => q,
                _ => return Err(ServerError::InvalidRequest),
            };
            request.quality = quality;
            request.quality_set = true;
            if !preferences
                .iter()
                .any(|p| rendered_media_supports_quality(&p.media_type))
            {
                return Err(ServerError::InvalidRequest);
            }
        }
        if let Some(raw) = single_query_value(&query, "window") {
            request.window = Some(parse_render_window(&raw)?);
        }
        if let Some(raw) = single_query_value(&query, "iccprofile") {
            match raw.as_str() {
                "no" | "yes" | "srgb" | "adobergb" | "rommrgb" | "displayp3" => {
                    request.icc_profile = raw
                }
                _ => return Err(ServerError::InvalidRequest),
            }
        }
        let (annotations, warning) = parse_render_annotations(query.values("annotation"))?;
        request.annotations = annotations;
        Ok((request, warning))
    }
    fn rendered_part_location(
        &self,
        r: &Request,
        request: &RenderRequest,
        part: &RenderedPart,
    ) -> Result<String, ServerError> {
        if request.thumbnail {
            return Ok(self.service_path(&r.url.escaped_path()));
        }
        let mut reference = part.reference.clone();
        if matches!(request.level, RenderLevel::Instance | RenderLevel::Frames)
            && reference == InstanceRef::default()
        {
            reference = request.reference.clone();
        }
        let rq = &request.reference;
        let ok = match request.level {
            RenderLevel::Study => {
                reference.study_instance_uid == rq.study_instance_uid
                    && !reference.series_instance_uid.is_empty()
                    && !reference.sop_instance_uid.is_empty()
            }
            RenderLevel::Series => {
                reference.study_instance_uid == rq.study_instance_uid
                    && reference.series_instance_uid == rq.series_instance_uid
                    && !reference.sop_instance_uid.is_empty()
            }
            _ => reference == *rq,
        };
        if !ok {
            return Err(ServerError::Backend);
        }
        if request.level == RenderLevel::Frames
            && (part.frame_number <= 0 || !request.frames.contains(&part.frame_number))
        {
            return Err(ServerError::Backend);
        }
        if part.frame_number > 0 {
            return Ok(format!(
                "{}/rendered",
                self.frame_location(&reference, part.frame_number)
            ));
        }
        Ok(format!("{}/rendered", self.instance_location(&reference)?))
    }
    pub(crate) fn serve_rendered(
        &self,
        w: &mut ServerWriter<'_>,
        r: &Request,
        thumbnail: bool,
    ) -> Served {
        let Some(renderer) = self.options.renderer.as_ref() else {
            return fail(ServerError::status(
                501,
                "not_implemented",
                ServerError::Unsupported,
            ));
        };
        let (request, warning) = self.render_request(r, thumbnail).map_err(|e| (0, e))?;
        if warning {
            w.header().set(
                "Warning",
                "299 dicomweb \"Some annotation values are not supported\"",
            );
        }
        let mut require_multipart =
            matches!(request.level, RenderLevel::Study | RenderLevel::Series)
                || (request.level == RenderLevel::Frames && request.frames.len() > 1);
        if thumbnail {
            require_multipart = false;
            if !request
                .accept
                .iter()
                .any(|p| p.quality > 0.0 && !p.multipart)
            {
                return fail(not_acceptable());
            }
        } else if require_multipart
            && !request
                .accept
                .iter()
                .any(|p| p.quality > 0.0 && p.multipart)
        {
            return fail(not_acceptable());
        }
        let mut spool = ResponseSpool::new(
            &self.options.spool_directory,
            self.limits.max_response_bytes,
        )
        .map_err(|e| (0, e))?;
        let ctx = r.context().clone();
        let mut count = 0usize;
        let mut rendered_pixels: i64 = 0;
        let mut response_multipart = require_multipart;
        let mut writer: Option<MultipartWriter> = None;
        let mut part_media_type = String::new();
        let mut response_content_type = String::new();
        let mut response_content_location = String::new();
        let result = {
            let mut yield_part = |mut part: RenderedPart| -> Result<(), ServerError> {
                let Some(mut reader) = part.reader.take() else {
                    return Err(ServerError::Backend);
                };
                with_reader_closed(reader.as_mut(), |reader| {
                    check_read_lease(&ctx)?;
                    if part.size < -1
                        || part.size > self.limits.max_part_bytes
                        || count as i64 >= self.limits.max_frames
                    {
                        return Err(ServerError::ResourceLimit);
                    }
                    if thumbnail && count > 0 {
                        return Err(ServerError::Backend);
                    }
                    let content_type =
                        normalized_rendered_content_type(&part.content_type, thumbnail)
                            .map_err(|_| ServerError::Backend)?;
                    if request.quality_set && !rendered_media_supports_quality(&content_type) {
                        return Err(ServerError::InvalidRequest);
                    }
                    if content_type.starts_with("image/") {
                        if part.width <= 0
                            || part.height <= 0
                            || part.width > request.max_pixels / part.height
                        {
                            return Err(ServerError::ResourceLimit);
                        }
                        let pixels = part.width * part.height;
                        if pixels > request.max_pixels - rendered_pixels {
                            return Err(ServerError::ResourceLimit);
                        }
                        if let Some(v) = &request.viewport
                            && (part.width > v.width || part.height > v.height)
                        {
                            return Err(ServerError::Backend);
                        }
                        rendered_pixels += pixels;
                    }
                    if request.level == RenderLevel::Frames
                        && !thumbnail
                        && (count >= request.frames.len()
                            || part.frame_number != request.frames[count])
                    {
                        return Err(ServerError::Backend);
                    }
                    let preference = matching_media_preference(
                        &request.accept,
                        &content_type,
                        "",
                        require_multipart,
                    )
                    .ok_or(ServerError::Unsupported)?;
                    if count == 0 && !thumbnail && !require_multipart {
                        response_multipart = preference.multipart;
                    }
                    if thumbnail && preference.multipart {
                        return Err(ServerError::Unsupported);
                    }
                    let location = self
                        .rendered_part_location(r, &request, &part)
                        .map_err(|_| ServerError::Backend)?;
                    if response_multipart {
                        if !part_media_type.is_empty() && part_media_type != content_type {
                            return Err(ServerError::Backend);
                        }
                        if writer.is_none() {
                            let mw = MultipartWriter::new();
                            part_media_type = content_type.clone();
                            response_content_type = format!(
                                "multipart/related; type=\"{content_type}\"; boundary={}",
                                mw.boundary()
                            );
                            writer = Some(mw);
                        }
                        if let Some(mw) = writer.as_mut() {
                            mw.create_part(
                                &header_with(&content_type, &location),
                                &mut |d: &[u8]| spool.write(d),
                            )?;
                        }
                        copy_bounded(
                            &ctx,
                            &mut |d: &[u8]| spool.write(d),
                            reader,
                            self.limits.max_part_bytes,
                        )
                        .1?;
                    } else {
                        if count != 0 {
                            return Err(ServerError::Backend);
                        }
                        response_content_type = content_type.clone();
                        response_content_location = location;
                        let max = self.limits.max_part_bytes.min(request.max_output_bytes);
                        copy_bounded(&ctx, &mut |d: &[u8]| spool.write(d), reader, max).1?;
                    }
                    count += 1;
                    Ok(())
                })
            };
            check_read_lease(&ctx)
                .and_then(|()| recover(|| renderer.render(&ctx, &request, &mut yield_part)))
        };
        if let Err(e) = result {
            if count == 0 {
                return fail(classify_backend_error(e));
            }
            return Err((count, e));
        }
        if count == 0 {
            return fail(ServerError::NotFound);
        }
        if request.level == RenderLevel::Frames && !thumbnail && count != request.frames.len() {
            return Err((count, ServerError::Backend));
        }
        if let Some(mw) = writer.as_mut() {
            mw.close(&mut |d: &[u8]| spool.write(d))
                .map_err(|e| (count, e))?;
        }
        spool.rewind().map_err(|e| (count, e))?;
        w.header().set("Content-Type", &response_content_type);
        if !response_content_location.is_empty() {
            w.header()
                .set("Content-Location", &response_content_location);
        }
        let max = self.limits.max_response_bytes;
        let copied = match spool.reader() {
            Some(file) => copy_bounded(&ctx, &mut |d: &[u8]| w.write(d), file, max).1,
            None => Err(ServerError::Backend),
        };
        copied.map_err(|e| (count, e))?;
        spool.close().map_err(|e| (count, e))?;
        Ok(count)
    }
    pub(crate) fn serve_wado_uri(&self, w: &mut ServerWriter<'_>, r: &mut Request) -> Served {
        if !self.options.enable_wado_uri {
            return fail(ServerError::status(
                501,
                "not_implemented",
                ServerError::Unsupported,
            ));
        }
        if r.content_length != 0 {
            return fail(ServerError::InvalidRequest);
        }
        let (query, err) = urlrules::parse_query(&r.url.raw_query);
        if err.is_some() {
            return fail(ServerError::InvalidRequest);
        }
        if query.0.len() as i64 > self.limits.max_query_fields {
            return fail(ServerError::ResourceLimit);
        }
        const ALLOWED: [&str; 19] = [
            "requestType",
            "studyUID",
            "seriesUID",
            "objectUID",
            "contentType",
            "transferSyntax",
            "frameNumber",
            "rows",
            "columns",
            "windowCenter",
            "windowWidth",
            "imageQuality",
            "annotation",
            "imageAnnotation",
            "anonymize",
            "charset",
            "region",
            "presentationSeriesUID",
            "presentationUID",
        ];
        let mut keys: Vec<&(String, Vec<String>)> = query.0.iter().collect();
        keys.sort_by(|a, b| a.0.cmp(&b.0));
        for (k, v) in keys {
            if !ALLOWED.contains(&k.as_str()) || v.len() != 1 || v[0].is_empty() {
                return fail(ServerError::InvalidRequest);
            }
            if v[0].len() as i64 > self.limits.max_query_value_bytes {
                return fail(ServerError::ResourceLimit);
            }
        }
        let has = |k: &str| query.has(k);
        let get = |k: &str| query.get(k).to_string();
        if get("requestType") != "WADO" {
            return fail(ServerError::InvalidRequest);
        }
        let reference = InstanceRef {
            study_instance_uid: get("studyUID"),
            series_instance_uid: get("seriesUID"),
            sop_instance_uid: get("objectUID"),
        };
        for uid in [
            &reference.study_instance_uid,
            &reference.series_instance_uid,
            &reference.sop_instance_uid,
        ] {
            self.validate_uid(uid).map_err(|e| (0, e))?;
        }
        let preferences = uri_media_preferences(&request_accept_header(r), &get("contentType"))
            .map_err(|e| (0, e))?;
        let dicom = preferences
            .iter()
            .any(|p| p.media_type == "application/dicom");
        let has_window = has("windowCenter") || has("windowWidth");
        let has_ps = has("presentationSeriesUID") || has("presentationUID");
        if has("rows") != has("columns")
            || has("windowCenter") != has("windowWidth")
            || has("presentationSeriesUID") != has("presentationUID")
            || (has_window && has_ps)
            || (dicom && has_window)
            || (has("annotation") && has("imageAnnotation"))
        {
            return fail(ServerError::InvalidRequest);
        }
        if has_ps
            && (self.validate_uid(&get("presentationSeriesUID")).is_err()
                || self.validate_uid(&get("presentationUID")).is_err())
        {
            return fail(ServerError::InvalidRequest);
        }
        if has("anonymize") && get("anonymize") != "yes" {
            return fail(ServerError::InvalidRequest);
        }
        if has("anonymize")
            || has("charset")
            || has("region")
            || has_ps
            || !r.header.get("Accept-Charset").trim().is_empty()
        {
            return fail(ServerError::status(
                501,
                "uri_option_not_implemented",
                ServerError::Unsupported,
            ));
        }
        if dicom {
            for k in [
                "frameNumber",
                "rows",
                "columns",
                "imageQuality",
                "annotation",
                "imageAnnotation",
            ] {
                if has(k) {
                    return fail(ServerError::InvalidRequest);
                }
            }
            let mut syntax = "1.2.840.10008.1.2.1".to_string();
            if has("transferSyntax") {
                syntax = get("transferSyntax");
                if self.validate_uid(&syntax).is_err() {
                    return fail(ServerError::InvalidRequest);
                }
                if !web_service_syntax_allowed(&syntax) {
                    return fail(ServerError::Unsupported);
                }
            }
            let mut preferences = preferences;
            for p in &mut preferences {
                p.transfer_syntax_uid = syntax.clone();
            }
            let request = RetrieveRequest {
                level: RetrieveLevel::Instance,
                study_instance_uid: reference.study_instance_uid.clone(),
                series_instance_uid: reference.series_instance_uid.clone(),
                sop_instance_uid: reference.sop_instance_uid.clone(),
                accept: preferences,
                prepared: Default::default(),
            };
            return self.serve_prepared_retrieve(w, r, request, true);
        }
        if has("transferSyntax") {
            return fail(ServerError::InvalidRequest);
        }
        let mut mapped = urlrules::Values::new();
        if has("rows") {
            for k in ["rows", "columns"] {
                match uri_positive_integer(&get(k)) {
                    Some(n) if n > 0 => {}
                    _ => return fail(ServerError::InvalidRequest),
                }
            }
            mapped.set("viewport", &format!("{},{}", get("columns"), get("rows")));
        }
        if has_window {
            mapped.set(
                "window",
                &format!("{},{},linear", get("windowCenter"), get("windowWidth")),
            );
        }
        if has("imageQuality") {
            match uri_positive_integer(&get("imageQuality")) {
                Some(n) if (1..=100).contains(&n) => {}
                _ => return fail(ServerError::InvalidRequest),
            }
            mapped.set("quality", &get("imageQuality"));
        }
        for k in ["annotation", "imageAnnotation"] {
            if has(k) {
                mapped.set("annotation", &get(k));
            }
        }
        let mut path = format!(
            "/studies/{}/series/{}/instances/{}",
            reference.study_instance_uid, reference.series_instance_uid, reference.sop_instance_uid
        );
        if has("frameNumber") {
            match uri_positive_integer(&get("frameNumber")) {
                Some(n) if n > 0 && n as i64 <= self.limits.max_frames => {
                    path.push_str(&format!("/frames/{n}"))
                }
                _ => return fail(ServerError::InvalidRequest),
            }
        }
        path.push_str("/rendered");
        let accept: Vec<String> = preferences
            .iter()
            .map(|p| format_media_type(&p.media_type, &[("q", &format_quality(p.quality))]))
            .collect();
        let saved_url = r.url.clone();
        let saved_accept = r.header.values("Accept").to_vec();
        r.url.path = path;
        r.url.raw_path = String::new();
        r.url.raw_query = mapped.encode();
        r.header.set("Accept", &accept.join(","));
        let served = self.serve_rendered(w, r, false);
        r.url = saved_url;
        r.header.del("Accept");
        for v in saved_accept {
            r.header.add("Accept", &v);
        }
        served
    }
}
fn format_quality(q: f64) -> String {
    format!("{q}")
}
fn uri_positive_integer(raw: &str) -> Option<u64> {
    if !raw.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    crate::dicomjson::sizedparse::parse_uint(raw, 31).ok()
}
fn uri_media_preferences(
    header: &str,
    content_type: &str,
) -> Result<Vec<MediaPreference>, ServerError> {
    let parse = |raw: &str, query: bool| -> Result<Vec<MediaPreference>, ServerError> {
        let result = parse_accept_header(raw);
        if result.is_empty() || result.len() != split_accept_values(raw).len() {
            return Err(if query {
                ServerError::InvalidRequest
            } else {
                ServerError::Unsupported
            });
        }
        for part in split_accept_values(raw) {
            let mt = parse_media_type(part.trim()).map_err(|_| ServerError::InvalidRequest)?;
            if mt.params.iter().any(|(k, _)| k != "q") {
                return Err(ServerError::InvalidRequest);
            }
            if query && mt.media_type.contains('*') {
                return Err(ServerError::InvalidRequest);
            }
        }
        let (mut dicom, mut rendered) = (false, false);
        for p in &result {
            if p.multipart {
                return Err(ServerError::InvalidRequest);
            }
            if p.quality > 0.0 {
                if p.media_type == "application/dicom" {
                    dicom = true;
                } else if !p.media_type.contains('*') {
                    rendered = true;
                }
            }
        }
        if dicom && rendered {
            return Err(ServerError::InvalidRequest);
        }
        Ok(result)
    };
    let headers = parse(header, false)?;
    let mut candidates: Vec<String> = [
        "application/dicom",
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
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let mut query_preferences = headers.clone();
    if !content_type.is_empty() {
        query_preferences = parse(content_type, true)?;
        candidates = query_preferences
            .iter()
            .map(|p| p.media_type.clone())
            .collect();
    }
    let mut result = Vec::new();
    let (mut dicom, mut rendered) = (false, false);
    for media in candidates {
        if media != "application/dicom" && !is_rendered_media_type(&media, false) {
            continue;
        }
        let hp = match best_matching_media_preference(&headers, &media, "", false) {
            Some(hp) if hp.quality > 0.0 => hp,
            _ => {
                if !content_type.is_empty() {
                    return Err(ServerError::Unsupported);
                }
                continue;
            }
        };
        let qp = match best_matching_media_preference(&query_preferences, &media, "", false) {
            Some(qp) if qp.quality > 0.0 => qp,
            _ => continue,
        };
        if content_type.is_empty()
            && hp.media_type != "application/dicom"
            && media == "application/dicom"
        {
            continue;
        }
        if media == "application/dicom" {
            dicom = true;
        } else {
            rendered = true;
        }
        result.push(MediaPreference {
            media_type: media,
            quality: qp.quality,
            ..MediaPreference::default()
        });
    }
    if dicom && rendered {
        return Err(ServerError::InvalidRequest);
    }
    if result.is_empty() {
        return Err(ServerError::Unsupported);
    }
    Ok(result)
}
