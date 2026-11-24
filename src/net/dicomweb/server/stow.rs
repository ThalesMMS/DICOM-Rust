use super::super::mimeheader::{Header, parse_media_type, read_mime_header};
use super::super::types::InstanceRef;
use super::super::urlrules;
use super::http::{Body, Request, ResponseWriter};
use super::media::{accepts_dicom_json, web_service_syntax_allowed};
use super::spool::{copy_bounded, create_temp, io_server_error, server_io_error};
use super::{
    Inner, ServerError, ServerWriter, StorePolicy, classify_backend_error,
    classify_multipart_error, ctx_err, resource_parts,
};
use crate::dicomjson::dataset_stream::marshal_any;
use crate::dicomjson::jsonwire::JAny;
use crate::net::ul::Ctx;
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::PathBuf;
mod meta;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StoreStatus {
    Stored,
    Warning,
    Duplicate,
    Conflict,
    Failed,
    Other,
}
impl StoreStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            StoreStatus::Stored => "stored",
            StoreStatus::Warning => "warning",
            StoreStatus::Duplicate => "duplicate",
            StoreStatus::Conflict => "conflict",
            StoreStatus::Failed => "failed",
            StoreStatus::Other => "",
        }
    }
}
pub struct StoreRequest<'a> {
    pub index: usize,
    pub route_study_uid: String,
    pub reference: InstanceRef,
    pub sop_class_uid: String,
    pub content_type: String,
    pub transfer_syntax_uid: String,
    pub policy: StorePolicy,
    pub reader: &'a mut dyn Read,
    pub size: i64,
    pub sha256: [u8; 32],
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoreOutcome {
    pub status: StoreStatus,
    pub reference: InstanceRef,
    pub sop_class_uid: String,
    pub warning_reason: u16,
    pub failure_reason: u16,
    pub err: Option<ServerError>,
}
impl Default for StoreOutcome {
    fn default() -> Self {
        StoreOutcome {
            status: StoreStatus::Other,
            reference: InstanceRef::default(),
            sop_class_uid: String::new(),
            warning_reason: 0,
            failure_reason: 0,
            err: None,
        }
    }
}
pub trait StoreBackend: Send + Sync {
    fn store(&self, ctx: &Ctx, request: StoreRequest<'_>) -> StoreOutcome;
}
type Served = Result<usize, (usize, ServerError)>;
pub(crate) struct MaxBytesReader {
    inner: Body,
    remaining: i64,
    limit: i64,
    err: Option<ServerError>,
}
impl MaxBytesReader {
    pub fn new(inner: Body, limit: i64) -> MaxBytesReader {
        MaxBytesReader {
            inner,
            remaining: limit,
            limit,
            err: None,
        }
    }
}
impl Read for MaxBytesReader {
    fn read(&mut self, p: &mut [u8]) -> io::Result<usize> {
        if let Some(e) = &self.err {
            return Err(server_io_error(e.clone()));
        }
        if p.is_empty() {
            return Ok(0);
        }
        let want = (p.len() as i64)
            .min(self.remaining.saturating_add(1))
            .max(0) as usize;
        let n = self.inner.read(&mut p[..want])?;
        if n as i64 <= self.remaining {
            self.remaining -= n as i64;
            return Ok(n);
        }
        let keep = self.remaining.max(0) as usize;
        self.remaining = 0;
        let e = ServerError::MaxBytes(self.limit);
        self.err = Some(e.clone());
        if keep > 0 {
            return Ok(keep);
        }
        Err(server_io_error(e))
    }
}
const PEEK_BUFFER_SIZE: usize = 4096;
struct PeekBuf<'a> {
    inner: &'a mut dyn Read,
    buf: Vec<u8>,
    err: Option<ServerError>,
}
enum SliceErr {
    Eof,
    BufferFull,
    Err(ServerError),
}
impl PeekBuf<'_> {
    fn fill(&mut self) {
        if self.err.is_some() || self.buf.len() >= PEEK_BUFFER_SIZE {
            return;
        }
        let mut chunk = vec![0u8; PEEK_BUFFER_SIZE - self.buf.len()];
        for _ in 0..100 {
            match self.inner.read(&mut chunk) {
                Ok(0) => {
                    self.err = Some(ServerError::Other("EOF".into()));
                    return;
                }
                Ok(n) => {
                    self.buf.extend_from_slice(&chunk[..n]);
                    return;
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => {
                    self.err = Some(io_server_error(&e));
                    return;
                }
            }
        }
    }
    fn is_eof(&self) -> bool {
        matches!(&self.err, Some(ServerError::Other(t)) if t == "EOF")
    }
    fn take_err(&self) -> SliceErr {
        match &self.err {
            Some(ServerError::Other(t)) if t == "EOF" => SliceErr::Eof,
            Some(e) => SliceErr::Err(e.clone()),
            None => SliceErr::Eof,
        }
    }
    fn read_slice(&mut self) -> Result<Vec<u8>, (Vec<u8>, SliceErr)> {
        let mut searched = 0;
        loop {
            if let Some(i) = self.buf[searched..].iter().position(|&c| c == b'\n') {
                let line: Vec<u8> = self.buf.drain(..searched + i + 1).collect();
                return Ok(line);
            }
            if self.err.is_some() {
                let line = std::mem::take(&mut self.buf);
                return Err((line, self.take_err()));
            }
            if self.buf.len() >= PEEK_BUFFER_SIZE {
                let line = std::mem::take(&mut self.buf);
                return Err((line, SliceErr::BufferFull));
            }
            searched = self.buf.len();
            self.fill();
        }
    }
}
fn read_part_header(b: &mut PeekBuf<'_>) -> Result<Header, ServerError> {
    let mut block = Vec::new();
    loop {
        let mut line = Vec::new();
        loop {
            match b.read_slice() {
                Ok(l) => {
                    line.extend_from_slice(&l);
                    break;
                }
                Err((l, SliceErr::BufferFull)) => line.extend_from_slice(&l),
                Err((_, SliceErr::Eof)) => return Err(ServerError::Other("unexpected EOF".into())),
                Err((_, SliceErr::Err(e))) => return Err(e),
            }
            if line.len() > 10 << 20 {
                return Err(ServerError::Other("message too large".into()));
            }
        }
        let blank = line == b"\r\n" || line == b"\n";
        block.extend_from_slice(&line);
        if block.len() > 10 << 20 {
            return Err(ServerError::Other("message too large".into()));
        }
        if blank {
            break;
        }
    }
    read_mime_header(&block).map_err(ServerError::Other)
}
pub(crate) struct MultipartReader<'a> {
    buf: PeekBuf<'a>,
    nl: Vec<u8>,
    nl_dash_boundary: Vec<u8>,
    dash_boundary_dash: Vec<u8>,
    dash_boundary: Vec<u8>,
    parts_read: usize,
    in_part: bool,
    part_n: usize,
    part_total: i64,
    part_err: Option<SliceErr>,
    part_read_err: Option<SliceErr>,
}
fn skip_lwsp(b: &[u8]) -> &[u8] {
    let i = b
        .iter()
        .position(|c| *c != b' ' && *c != b'\t')
        .unwrap_or(b.len());
    &b[i..]
}
fn match_after_prefix(buf: &[u8], prefix: &[u8], read_err: bool) -> i32 {
    if buf.len() == prefix.len() {
        return if read_err { 1 } else { 0 };
    }
    let c = buf[prefix.len()];
    if matches!(c, b' ' | b'\t' | b'\r' | b'\n') {
        return 1;
    }
    if c == b'-' {
        if buf.len() == prefix.len() + 1 {
            return if read_err { -1 } else { 0 };
        }
        if buf[prefix.len() + 1] == b'-' {
            return 1;
        }
    }
    -1
}
fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    hay.windows(needle.len()).position(|w| w == needle)
}
impl<'a> MultipartReader<'a> {
    pub fn new(input: &'a mut dyn Read, boundary: &str) -> MultipartReader<'a> {
        let b = format!("\r\n--{boundary}--").into_bytes();
        MultipartReader {
            buf: PeekBuf {
                inner: input,
                buf: Vec::new(),
                err: None,
            },
            nl: b[..2].to_vec(),
            nl_dash_boundary: b[..b.len() - 2].to_vec(),
            dash_boundary_dash: b[2..].to_vec(),
            dash_boundary: b[2..b.len() - 2].to_vec(),
            parts_read: 0,
            in_part: false,
            part_n: 0,
            part_total: 0,
            part_err: None,
            part_read_err: None,
        }
    }
    fn is_final_boundary(&self, line: &[u8]) -> bool {
        if !line.starts_with(&self.dash_boundary_dash) {
            return false;
        }
        let rest = skip_lwsp(&line[self.dash_boundary_dash.len()..]);
        rest.is_empty() || rest == self.nl.as_slice()
    }
    fn is_boundary_delimiter_line(&mut self, line: &[u8]) -> bool {
        if !line.starts_with(&self.dash_boundary) {
            return false;
        }
        let rest = skip_lwsp(&line[self.dash_boundary.len()..]);
        if self.parts_read == 0 && rest == b"\n" {
            self.nl = self.nl[1..].to_vec();
            self.nl_dash_boundary = self.nl_dash_boundary[1..].to_vec();
        }
        rest == self.nl.as_slice()
    }
    pub(crate) fn next_part(&mut self) -> Result<Option<Header>, ServerError> {
        if self.in_part {
            let mut sink = [0u8; 4096];
            while let Ok(n) = self.read_part(&mut sink) {
                if n == 0 {
                    break;
                }
            }
            self.in_part = false;
        }
        if self.dash_boundary == b"--" {
            return Err(ServerError::Other("multipart: boundary is empty".into()));
        }
        let mut expect_new_part = false;
        loop {
            let line = match self.buf.read_slice() {
                Ok(line) => line,
                Err((line, SliceErr::Eof)) if self.is_final_boundary(&line) => return Ok(None),
                Err((_, SliceErr::Err(e))) => return Err(e),
                Err(_) => return Err(ServerError::Other("multipart: NextPart: EOF".into())),
            };
            if self.is_boundary_delimiter_line(&line) {
                self.parts_read += 1;
                let header = read_part_header(&mut self.buf)?;
                self.in_part = true;
                self.part_n = 0;
                self.part_total = 0;
                self.part_err = None;
                self.part_read_err = None;
                return Ok(Some(header));
            }
            if self.is_final_boundary(&line) {
                return Ok(None);
            }
            if expect_new_part {
                return Err(ServerError::Other("multipart: expecting a new Part".into()));
            }
            if self.parts_read == 0 {
                continue;
            }
            if line == self.nl {
                expect_new_part = true;
                continue;
            }
            return Err(ServerError::Other(
                "multipart: unexpected line in Next()".into(),
            ));
        }
    }
    fn read_err_is_some(&self) -> bool {
        self.part_read_err.is_some()
    }
    fn scan_until_boundary(&self, buf: &[u8]) -> (usize, Option<SliceErr>) {
        let read_err = self.read_err_is_some();
        let pending = || match &self.part_read_err {
            Some(SliceErr::Err(e)) => Some(SliceErr::Err(e.clone())),
            Some(_) => Some(SliceErr::Err(ServerError::Other("unexpected EOF".into()))),
            None => None,
        };
        if self.part_total == 0 {
            if buf.starts_with(&self.dash_boundary) {
                match match_after_prefix(buf, &self.dash_boundary, read_err) {
                    -1 => return (self.dash_boundary.len(), None),
                    0 => return (0, None),
                    _ => return (0, Some(SliceErr::Eof)),
                }
            }
            if self.dash_boundary.starts_with(buf) {
                return (0, pending());
            }
        }
        if let Some(i) = find(buf, &self.nl_dash_boundary) {
            match match_after_prefix(&buf[i..], &self.nl_dash_boundary, read_err) {
                -1 => return (i + self.nl_dash_boundary.len(), None),
                0 => return (i, None),
                _ => return (i, Some(SliceErr::Eof)),
            }
        }
        if self.nl_dash_boundary.starts_with(buf) {
            return (0, pending());
        }
        if let Some(i) = buf.iter().rposition(|c| *c == self.nl_dash_boundary[0])
            && self.nl_dash_boundary.starts_with(&buf[i..])
        {
            return (i, None);
        }
        (buf.len(), pending())
    }
    pub(crate) fn read_part(&mut self, d: &mut [u8]) -> Result<usize, ServerError> {
        while self.part_n == 0 && self.part_err.is_none() {
            let (n, err) = self.scan_until_boundary(&self.buf.buf.clone());
            self.part_n = n;
            self.part_err = err;
            if self.part_n == 0 && self.part_err.is_none() {
                let before = self.buf.buf.len();
                if self.buf.err.is_none() && before < PEEK_BUFFER_SIZE {
                    self.buf.fill();
                }
                if self.buf.buf.len() == before {
                    self.part_read_err = Some(if self.buf.is_eof() || self.buf.err.is_none() {
                        SliceErr::Eof
                    } else {
                        self.buf.take_err()
                    });
                }
            }
        }
        if self.part_n == 0 {
            return match &self.part_err {
                Some(SliceErr::Eof) | None => Ok(0),
                Some(SliceErr::Err(e)) => Err(e.clone()),
                Some(SliceErr::BufferFull) => Err(ServerError::Other("bufio: buffer full".into())),
            };
        }
        let n = d.len().min(self.part_n);
        d[..n].copy_from_slice(&self.buf.buf[..n]);
        self.buf.buf.drain(..n);
        self.part_total += n as i64;
        self.part_n -= n;
        if self.part_n == 0
            && let Some(SliceErr::Err(e)) = &self.part_err
        {
            let e = e.clone();
            if n > 0 {
                return Ok(n);
            }
            return Err(e);
        }
        Ok(n)
    }
}
struct PartRead<'r, 'a>(&'r mut MultipartReader<'a>);
impl Read for PartRead<'_, '_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.0.read_part(buf).map_err(server_io_error)
    }
}
struct StagedPart {
    path: PathBuf,
    content_type: String,
    transfer_syntax_uid: String,
    size: i64,
    digest: [u8; 32],
    reference: InstanceRef,
    sop_class_uid: String,
    preflight_failure: u16,
}
fn spool_failure() -> ServerError {
    ServerError::status(503, "spool_failure", ServerError::Backend)
}
fn unsupported_media() -> ServerError {
    ServerError::status(415, "unsupported_media_type", ServerError::Unsupported)
}
fn cleanup_staged(parts: &[StagedPart]) -> Result<(), ServerError> {
    let mut failed = false;
    for p in parts {
        if p.path.as_os_str().is_empty() {
            continue;
        }
        if let Err(e) = fs::remove_file(&p.path)
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
fn cleanup_staging(
    parts: &[StagedPart],
    temporary: Option<PathBuf>,
    preferred: ServerError,
) -> ServerError {
    let mut failed = false;
    if let Some(path) = temporary
        && let Err(e) = fs::remove_file(&path)
        && e.kind() != io::ErrorKind::NotFound
    {
        failed = true;
    }
    if cleanup_staged(parts).is_err() {
        failed = true;
    }
    if failed {
        ServerError::Backend
    } else {
        preferred
    }
}
impl Inner {
    pub(crate) fn serve_store(&self, w: &mut ServerWriter<'_>, r: &mut Request) -> Served {
        let Some(backend) = self
            .options
            .backend
            .as_ref()
            .and_then(|b| b.store_backend())
        else {
            return Err((
                0,
                ServerError::status(501, "not_implemented", ServerError::Unsupported),
            ));
        };
        if !accepts_dicom_json(&super::request_accept_header(r)) {
            return Err((
                0,
                ServerError::status(406, "not_acceptable", ServerError::Unsupported),
            ));
        }
        let parts = resource_parts(&r.url.path);
        let mut route_study_uid = String::new();
        if parts.len() == 2 {
            route_study_uid = parts[1].clone();
            self.validate_uid(&route_study_uid).map_err(|e| (0, e))?;
        }
        let staged = self
            .stage_store_request(r, &route_study_uid)
            .map_err(|e| (0, e))?;
        let result = self.store_staged(w, r, backend, &staged, &route_study_uid);
        if result.is_err() {
            let _ = cleanup_staged(&staged);
        }
        result
    }
    fn store_staged(
        &self,
        w: &mut ServerWriter<'_>,
        r: &Request,
        backend: &dyn StoreBackend,
        staged: &[StagedPart],
        route_study_uid: &str,
    ) -> Served {
        let ctx = r.context().clone();
        let mut outcomes: Vec<StoreOutcome> = Vec::new();
        let (mut stored, mut failed, mut warnings) = (0, 0, 0);
        let mut created: Vec<String> = Vec::new();
        for (index, part) in staged.iter().enumerate() {
            ctx_err(&ctx).map_err(|e| (index, e))?;
            if part.preflight_failure != 0 {
                failed += 1;
                outcomes.push(StoreOutcome {
                    status: StoreStatus::Failed,
                    failure_reason: part.preflight_failure,
                    reference: part.reference.clone(),
                    sop_class_uid: part.sop_class_uid.clone(),
                    ..StoreOutcome::default()
                });
                continue;
            }
            let mut file =
                open_staged_part(&ctx, part).map_err(|_| (index, ServerError::Backend))?;
            let request = StoreRequest {
                index,
                route_study_uid: route_study_uid.to_string(),
                reference: part.reference.clone(),
                sop_class_uid: part.sop_class_uid.clone(),
                content_type: part.content_type.clone(),
                transfer_syntax_uid: part.transfer_syntax_uid.clone(),
                policy: self.options.store_policy.clone(),
                reader: &mut file,
                size: part.size,
                sha256: part.digest,
            };
            let mut outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                backend.store(&ctx, request)
            }))
            .unwrap_or_else(|_| StoreOutcome {
                status: StoreStatus::Failed,
                err: Some(ServerError::Backend),
                ..StoreOutcome::default()
            });
            drop(file);
            ctx_err(&ctx).map_err(|e| (index, e))?;
            if outcome.reference == InstanceRef::default() {
                outcome.reference = part.reference.clone();
            }
            if outcome.sop_class_uid.trim().is_empty() {
                outcome.sop_class_uid = part.sop_class_uid.clone();
            }
            if outcome.reference != part.reference || outcome.sop_class_uid != part.sop_class_uid {
                outcome.status = StoreStatus::Failed;
                outcome.err = Some(ServerError::Backend);
                if outcome.failure_reason == 0 {
                    outcome.failure_reason = 0x0110;
                }
            }
            if let Some(e) = outcome.err.clone() {
                return Err((index, classify_backend_error(e)));
            }
            if matches!(
                outcome.status,
                StoreStatus::Stored | StoreStatus::Warning | StoreStatus::Duplicate
            ) && (self.validate_uid(&outcome.sop_class_uid).is_err()
                || self
                    .validate_uid(&outcome.reference.study_instance_uid)
                    .is_err()
                || self
                    .validate_uid(&outcome.reference.series_instance_uid)
                    .is_err()
                || self
                    .validate_uid(&outcome.reference.sop_instance_uid)
                    .is_err())
            {
                outcome.status = StoreStatus::Failed;
                outcome.failure_reason = 0x0110;
            }
            let mut create = |uid: &str| {
                if !created.iter().any(|c| c == uid) {
                    created.push(uid.to_string());
                }
            };
            match outcome.status {
                StoreStatus::Stored => {
                    stored += 1;
                    create(&outcome.reference.study_instance_uid);
                }
                StoreStatus::Warning => {
                    if outcome.warning_reason == 0 {
                        outcome.status = StoreStatus::Failed;
                        outcome.failure_reason = 0x0110;
                        failed += 1;
                    } else {
                        stored += 1;
                        warnings += 1;
                        create(&outcome.reference.study_instance_uid);
                    }
                }
                StoreStatus::Duplicate => {
                    if self.options.store_policy.0 == "idempotent" {
                        stored += 1;
                    } else {
                        failed += 1;
                        outcome.status = StoreStatus::Conflict;
                        if outcome.failure_reason == 0 {
                            outcome.failure_reason = 0x0111;
                        }
                    }
                }
                StoreStatus::Conflict => {
                    failed += 1;
                    if outcome.failure_reason == 0 {
                        outcome.failure_reason = 0x0110;
                    }
                }
                _ => {
                    failed += 1;
                    outcome.status = StoreStatus::Failed;
                    if outcome.failure_reason == 0 {
                        outcome.failure_reason = 0x0110;
                    }
                }
            }
            outcomes.push(outcome);
        }
        cleanup_staged(staged).map_err(|e| (0, e))?;
        let mut status = 200;
        if stored > 0 && (failed > 0 || warnings > 0) {
            status = 202;
        }
        if stored == 0 {
            status = 409;
        }
        let data = self.store_response(&outcomes, route_study_uid);
        if data.len() as i64 > self.limits.max_response_bytes {
            return Err((0, ServerError::ResourceLimit));
        }
        w.header().set("Content-Type", "application/dicom+json");
        if created.len() == 1 {
            let location =
                self.service_path(&format!("/studies/{}", urlrules::path_escape(&created[0])));
            w.header().set("Location", &location);
            w.header().set("Content-Location", &location);
        }
        w.write_header(status);
        let n = outcomes.len();
        w.write(data.as_bytes()).map_err(|e| (n, e))?;
        Ok(n)
    }
    fn stage_store_request(
        &self,
        r: &mut Request,
        route_study_uid: &str,
    ) -> Result<Vec<StagedPart>, ServerError> {
        if r.header.values("Content-Type").len() != 1 {
            return Err(unsupported_media());
        }
        let mt = parse_media_type(r.header.get("Content-Type")).map_err(|_| unsupported_media())?;
        let boundary = mt.param("boundary").to_string();
        if !mt.media_type.eq_ignore_ascii_case("multipart/related") || boundary.is_empty() {
            return Err(unsupported_media());
        }
        let root = super::super::mimeheader::to_lower(mt.param("type").trim_matches('"').trim());
        match root.as_str() {
            "application/dicom+json" | "application/dicom+xml" => {
                return self.stage_store_metadata_request(r, route_study_uid, &root, &mt.params);
            }
            "application/dicom" => {}
            _ => return Err(unsupported_media()),
        }
        let body = std::mem::replace(&mut r.body, Box::new(io::empty()));
        let mut limited = MaxBytesReader::new(body, self.limits.max_request_bytes);
        let ctx = r.context().clone();
        let mut reader = MultipartReader::new(&mut limited, &boundary);
        let mut staged: Vec<StagedPart> = Vec::new();
        loop {
            if let Err(e) = ctx_err(&ctx) {
                return Err(cleanup_staging(&staged, None, e));
            }
            let header = match reader.next_part() {
                Ok(Some(h)) => h,
                Ok(None) => break,
                Err(e) => return Err(cleanup_staging(&staged, None, classify_multipart_error(e))),
            };
            if staged.len() as i64 >= self.limits.max_parts {
                return Err(cleanup_staging(&staged, None, ServerError::ResourceLimit));
            }
            let part_type = parse_media_type(header.get("Content-Type"));
            let part_params = match part_type {
                Ok(mt) if mt.media_type.eq_ignore_ascii_case("application/dicom") => mt,
                _ => return Err(cleanup_staging(&staged, None, unsupported_media())),
            };
            if super::request_header_bytes(&header) > self.limits.max_part_header_bytes {
                return Err(cleanup_staging(&staged, None, ServerError::ResourceLimit));
            }
            let Ok((mut file, path)) =
                create_temp(&self.options.spool_directory, ".dicomweb-stow-")
            else {
                return Err(cleanup_staging(&staged, None, spool_failure()));
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
                    self.limits.max_part_bytes,
                )
            };
            if let Err(e) = copied {
                if spool_err {
                    return Err(cleanup_staging(&staged, Some(path), spool_failure()));
                }
                return Err(cleanup_staging(
                    &staged,
                    Some(path),
                    classify_multipart_error(e),
                ));
            }
            if file.seek(SeekFrom::Start(0)).is_err() || file.flush().is_err() {
                return Err(cleanup_staging(&staged, Some(path), spool_failure()));
            }
            match file.metadata() {
                Ok(info) if info.is_file() && info.len() as i64 == size => {}
                Ok(_) => return Err(cleanup_staging(&staged, Some(path), ServerError::Backend)),
                Err(_) => return Err(cleanup_staging(&staged, Some(path), spool_failure())),
            }
            drop(file);
            let mut candidate = StagedPart {
                path: path.clone(),
                content_type: "application/dicom".into(),
                transfer_syntax_uid: part_params.param("transfer-syntax").trim().to_string(),
                size,
                digest: hash.finalize().into(),
                reference: InstanceRef::default(),
                sop_class_uid: String::new(),
                preflight_failure: 0,
            };
            if let Err(e) = self.inspect_staged_part(&ctx, &mut candidate, route_study_uid) {
                return Err(cleanup_staging(&staged, Some(path), e));
            }
            staged.push(candidate);
        }
        if staged.is_empty() {
            return Err(ServerError::InvalidRequest);
        }
        Ok(staged)
    }
    fn inspect_staged_part(
        &self,
        ctx: &Ctx,
        staged: &mut StagedPart,
        route_study_uid: &str,
    ) -> Result<(), ServerError> {
        use crate::index;
        use crate::object::error::Kind as ObjectKind;
        ctx_err(ctx)?;
        let mut options = index::default_options();
        options.limits.max_total_bytes = self.limits.max_decoded_part_bytes;
        let (result, err) = index::read_path(&staged.path, options);
        if let Some(e) = err {
            if e.is(index::Sentinel::ResourceLimit) {
                return Err(ServerError::ResourceLimit);
            }
            if e.is_kind(ObjectKind::UnknownTransferSyntax)
                || e.is_kind(ObjectKind::UnsupportedTransferSyntax)
            {
                staged.preflight_failure = 0xC122;
                return Ok(());
            }
            return Err(ServerError::InvalidRequest);
        }
        let record = result.record;
        let reference = InstanceRef {
            study_instance_uid: record.study.instance_uid.clone(),
            series_instance_uid: record.series.instance_uid.clone(),
            sop_instance_uid: record.instance.sop_instance_uid.clone(),
        };
        for uid in [
            &reference.study_instance_uid,
            &reference.series_instance_uid,
            &reference.sop_instance_uid,
            &record.instance.sop_class_uid,
            &record.file_meta.transfer_syntax_uid,
        ] {
            self.validate_uid(uid)
                .map_err(|_| ServerError::InvalidRequest)?;
        }
        if record.file_meta.media_storage_sop_class_uid != record.instance.sop_class_uid
            || record.file_meta.media_storage_sop_instance_uid != reference.sop_instance_uid
        {
            return Err(ServerError::InvalidRequest);
        }
        if !staged.transfer_syntax_uid.is_empty()
            && staged.transfer_syntax_uid != record.file_meta.transfer_syntax_uid
        {
            return Err(ServerError::status(
                415,
                "transfer_syntax_mismatch",
                ServerError::Unsupported,
            ));
        }
        if !route_study_uid.is_empty() && reference.study_instance_uid != route_study_uid {
            return Err(ServerError::Conflict);
        }
        staged.reference = reference;
        staged.sop_class_uid = record.instance.sop_class_uid.clone();
        staged.transfer_syntax_uid = record.file_meta.transfer_syntax_uid.clone();
        if !web_service_syntax_allowed(&staged.transfer_syntax_uid) {
            staged.preflight_failure = 0xC122;
        }
        Ok(())
    }
    fn store_response(&self, outcomes: &[StoreOutcome], route_study_uid: &str) -> String {
        let string_values = |v: &str| -> Option<JAny> {
            let t = v.trim();
            (!t.is_empty()).then(|| JAny::Arr(vec![JAny::Str(t.to_string())]))
        };
        let element = |vr: &str, value: Option<JAny>| -> JAny {
            let mut fields = vec![("vr".to_string(), JAny::Str(vr.to_string()))];
            if let Some(v) = value {
                fields.push(("Value".to_string(), v));
            }
            JAny::Obj(fields)
        };
        let number = |n: u16| JAny::Arr(vec![JAny::Number(n.to_string())]);
        let (mut stored_items, mut failed_items, mut other_items) =
            (Vec::new(), Vec::new(), Vec::new());
        for outcome in outcomes {
            let mut item: Vec<(String, JAny)> = vec![
                (
                    "00081150".into(),
                    element("UI", string_values(&outcome.sop_class_uid)),
                ),
                (
                    "00081155".into(),
                    element("UI", string_values(&outcome.reference.sop_instance_uid)),
                ),
            ];
            if let Ok(location) = self.instance_location(&outcome.reference) {
                item.push((
                    "00081190".into(),
                    element("UR", Some(JAny::Arr(vec![JAny::Str(location)]))),
                ));
            }
            match outcome.status {
                StoreStatus::Stored | StoreStatus::Duplicate => stored_items.push(item),
                StoreStatus::Warning => {
                    item.push((
                        "00081196".into(),
                        element("US", Some(number(outcome.warning_reason))),
                    ));
                    stored_items.push(item);
                }
                _ => {
                    let reason = element("US", Some(number(outcome.failure_reason)));
                    if outcome.sop_class_uid.trim().is_empty()
                        || outcome.reference.sop_instance_uid.trim().is_empty()
                    {
                        other_items.push(vec![("00081197".to_string(), reason)]);
                    } else {
                        item.push(("00081197".into(), reason));
                        failed_items.push(item);
                    }
                }
            }
        }
        let mut response: Vec<(String, JAny)> = vec![("00081190".into(), element("UR", None))];
        if !route_study_uid.is_empty() {
            let location = self.service_path(&format!(
                "/studies/{}",
                urlrules::path_escape(route_study_uid)
            ));
            response[0].1 = element("UR", Some(JAny::Arr(vec![JAny::Str(location)])));
        }
        let seq = |items: Vec<Vec<(String, JAny)>>| {
            element(
                "SQ",
                Some(JAny::Arr(
                    items
                        .into_iter()
                        .map(|mut i| {
                            i.sort_by(|a, b| a.0.cmp(&b.0));
                            JAny::Obj(i)
                        })
                        .collect(),
                )),
            )
        };
        if !stored_items.is_empty() {
            response.push(("00081199".into(), seq(stored_items)));
        }
        if !failed_items.is_empty() {
            response.push(("00081198".into(), seq(failed_items)));
        }
        if !other_items.is_empty() {
            response.push(("0008119A".into(), seq(other_items)));
        }
        response.sort_by(|a, b| a.0.cmp(&b.0));
        let mut out = String::new();
        marshal_any(&JAny::Obj(response), &mut out);
        out
    }
}
fn open_staged_part(ctx: &Ctx, staged: &StagedPart) -> Result<File, ServerError> {
    let mut file = File::open(&staged.path).map_err(|_| ServerError::Backend)?;
    match file.metadata() {
        Ok(info) if info.is_file() && info.len() as i64 == staged.size => {}
        _ => return Err(ServerError::Backend),
    }
    let mut hash = Sha256::new();
    let (written, result) = copy_bounded(
        ctx,
        &mut |d: &[u8]| {
            hash.update(d);
            Ok(d.len())
        },
        &mut file,
        staged.size,
    );
    if result.is_err() || written != staged.size {
        return Err(ServerError::Backend);
    }
    let digest: [u8; 32] = hash.finalize().into();
    if digest != staged.digest {
        return Err(ServerError::Backend);
    }
    file.seek(SeekFrom::Start(0))
        .map_err(|_| ServerError::Backend)?;
    Ok(file)
}
