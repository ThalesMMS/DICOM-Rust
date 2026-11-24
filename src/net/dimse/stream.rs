#![forbid(unsafe_code)]
use super::command::{CStoreRequest, STATUS_SUCCESS};
use super::controls::{context_err, data_set_read_context};
use super::error::{Error, Kind, Result};
use super::message::should_drain_on_error;
use super::pdata::{PDataReader, drain_pdata_reader};
use super::store::{
    CStoreRequestContext, STATUS_C_STORE_CANNOT_UNDERSTAND, STATUS_C_STORE_DATA_SET_DOES_NOT_MATCH,
    STATUS_C_STORE_OUT_OF_RESOURCES, StorageScpOptions, new_scp_status_error,
    normalize_store_status, scp_status_of, validate_c_store_data_set,
};
use crate::net::ul::{AcceptedContext, Association, Ctx};
use crate::object::writer::{FileMetaAeTitles, WriteFileOptions, file_meta_mode};
use crate::object::{self, ElementHeader, File, Object, Syntax, Tag, Value, Vr};
use std::fs;
use std::io::{BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
#[derive(Debug, Clone)]
pub struct CStoreStreamRequest {
    pub request: CStoreRequest,
    pub presentation_context_id: u8,
    pub presentation_context: AcceptedContext,
    pub data_set_syntax: Syntax,
    pub calling_ae_title: String,
    pub called_ae_title: String,
}
impl CStoreStreamRequest {
    pub fn file_meta_header(&self, ae_titles: &FileMetaAeTitles) -> Result<Vec<u8>> {
        encode_file_meta_header(
            &self.request.affected_sop_class_uid,
            &self.request.affected_sop_instance_uid,
            self.data_set_syntax,
            ae_titles,
        )
    }
}
fn ui(tag: Tag, value: &str) -> object::Element {
    object::Element {
        header: ElementHeader {
            tag,
            vr: Vr::UI,
            length: 0,
            length_set: false,
        },
        value: Value::Strings(vec![value.to_string()]),
    }
}
#[doc(hidden)]
pub fn encode_file_meta_header(
    sop_class_uid: &str,
    sop_instance_uid: &str,
    syntax: Syntax,
    ae_titles: &FileMetaAeTitles,
) -> Result<Vec<u8>> {
    let class = object::transfer::normalize_uid(sop_class_uid);
    let instance = object::transfer::normalize_uid(sop_instance_uid);
    if !object::is_valid_uid(class.as_bytes()) {
        return Err(Error::from(object::Error::kind_suffix(
            object::Kind::MissingSopClassUid,
            &format!(": {}", crate::textfmt::quote(class)),
        )));
    }
    if !object::is_valid_uid(instance.as_bytes()) {
        return Err(Error::from(object::Error::kind_suffix(
            object::Kind::MissingSopInstanceUid,
            &format!(": {}", crate::textfmt::quote(instance)),
        )));
    }
    if !object::is_valid_uid(syntax.uid.as_bytes()) {
        return Err(Error::new(format!(
            "dicom: invalid transfer syntax UID {}",
            crate::textfmt::quote(syntax.uid)
        )));
    }
    let file = File {
        preamble: Vec::new(),
        meta: Object::from_elements(vec![
            ui(Tag::new(0x0002, 0x0002), class),
            ui(Tag::new(0x0002, 0x0003), instance),
        ]),
        dataset: Object::new(),
        transfer_syntax: syntax,
        partial_read: None,
        incomplete_pixel_data: None,
    };
    let bytes = object::writer::write_file_with_options(
        &file,
        &WriteFileOptions {
            meta_mode: file_meta_mode::CREATE,
            ae_titles: ae_titles.clone(),
            ..Default::default()
        },
    )?;
    if bytes.len() < 144 {
        return Err(Error::new("dicom: invalid File Meta Information"));
    }
    let group = u32::from_le_bytes([bytes[140], bytes[141], bytes[142], bytes[143]]) as usize;
    let end = (144 + group).min(bytes.len());
    Ok(bytes[..end].to_vec())
}
pub trait CStoreStream: Send {
    fn write(&mut self, p: &[u8]) -> Result<()>;
    fn commit(&mut self, ctx: &Ctx) -> (u16, Option<Error>);
    fn abort(&mut self);
}
pub trait CStoreStreamSink: Send + Sync {
    fn open_c_store_stream(
        &self,
        ctx: &Ctx,
        req: &CStoreStreamRequest,
    ) -> Result<Option<Box<dyn CStoreStream>>>;
}
fn stream_error_status(err: &Error, fallback: u16) -> (u16, String) {
    match scp_status_of(err, "C-STORE") {
        Some((status, comment)) if status != 0 => (status, comment),
        _ => (fallback, String::new()),
    }
}
pub(crate) fn serve_storage_c_store_stream(
    ctx: Option<&Ctx>,
    assoc: &Association,
    pc_id: u8,
    req: &CStoreRequest,
    req_ctx: &CStoreRequestContext,
    opts: &StorageScpOptions,
    sink: &dyn CStoreStreamSink,
) -> Result<Option<(u16, String)>> {
    let sreq = CStoreStreamRequest {
        request: req.clone(),
        presentation_context_id: pc_id,
        presentation_context: req_ctx.presentation_context.clone(),
        data_set_syntax: req_ctx
            .data_set_syntax
            .unwrap_or(object::transfer::IMPLICIT_VR_LITTLE_ENDIAN),
        calling_ae_title: assoc.calling_ae_title.clone(),
        called_ae_title: assoc.called_ae_title.clone(),
    };
    let rctx = data_set_read_context(ctx, Some(assoc));
    let mut reader = PDataReader::typed(Some(rctx.clone()), Some(assoc), pc_id, false);
    let refuse = |reader: &mut PDataReader<'_>, status: u16, comment: String| {
        drain_pdata_reader(reader)?;
        Ok(Some((status, comment)))
    };
    if !object::is_valid_uid(req.affected_sop_class_uid.as_bytes())
        || !object::is_valid_uid(req.affected_sop_instance_uid.as_bytes())
    {
        return refuse(
            &mut reader,
            STATUS_C_STORE_DATA_SET_DOES_NOT_MATCH,
            String::new(),
        );
    }
    let mut stream = match sink.open_c_store_stream(&rctx, &sreq) {
        Err(e) => {
            let (status, comment) = stream_error_status(&e, STATUS_C_STORE_OUT_OF_RESOURCES);
            return refuse(&mut reader, status, comment);
        }
        Ok(None) => return Ok(None),
        Ok(Some(s)) => s,
    };
    let max = opts.max_data_set_bytes;
    let limit = if max > 0 { max + 1 } else { i64::MAX };
    let mut written: i64 = 0;
    let mut buf = vec![0u8; 32 << 10];
    while written < limit {
        let want = buf.len().min((limit - written) as usize);
        let n = match reader.read_bytes(&mut buf[..want]) {
            Ok(n) => n,
            Err(e) => {
                stream.abort();
                if !should_drain_on_error(&e) {
                    return Err(e);
                }
                let (status, comment) = stream_error_status(&e, STATUS_C_STORE_OUT_OF_RESOURCES);
                return refuse(&mut reader, status, comment);
            }
        };
        if n == 0 {
            break;
        }
        if let Err(e) = stream.write(&buf[..n]) {
            stream.abort();
            let (status, comment) = stream_error_status(&e, STATUS_C_STORE_OUT_OF_RESOURCES);
            return refuse(&mut reader, status, comment);
        }
        written += n as i64;
    }
    if max > 0 && written > max {
        stream.abort();
        return refuse(&mut reader, STATUS_C_STORE_OUT_OF_RESOURCES, String::new());
    }
    let (status, err) = stream.commit(&rctx);
    Ok(Some(normalize_store_status(status, err.as_ref())))
}
fn netstore_failure(operation: &str, cause: Error, published: bool) -> Error {
    let text = if published {
        format!("netstore: {operation} failed after publication")
    } else {
        format!("netstore: {operation} failed")
    };
    cause.relabel(text)
}
fn io_error(e: &std::io::Error) -> Error {
    Error::new(e.to_string())
}
pub fn received_ae_titles(local_ae_title: &str, calling_ae_title: &str) -> FileMetaAeTitles {
    let local = local_ae_title.trim().to_string();
    FileMetaAeTitles {
        source: local.clone(),
        sending: calling_ae_title.trim().to_string(),
        receiving: local,
    }
}
pub(crate) struct StreamFile {
    ctx: Ctx,
    out_dir: PathBuf,
    instance: String,
    file: Option<fs::File>,
    temporary: PathBuf,
    finished: bool,
}
pub(crate) fn begin_stream(ctx: &Ctx, out_dir: &str, sop_instance_uid: &str) -> Result<StreamFile> {
    if !object::is_valid_uid(sop_instance_uid.as_bytes()) {
        return Err(Error::sentinel(Kind::NetstoreInvalidIdentity));
    }
    if let Some(e) = context_err(ctx) {
        return Err(netstore_failure("prepare", e, false));
    }
    let dir = PathBuf::from(out_dir);
    match fs::symlink_metadata(&dir) {
        Ok(m) if m.is_dir() => {}
        Ok(_) => {
            return Err(netstore_failure(
                "open output",
                Error::new("not a directory"),
                false,
            ));
        }
        Err(e) => return Err(netstore_failure("open output", io_error(&e), false)),
    }
    let mut last = None;
    for _ in 0..10 {
        let token = uuid::Uuid::new_v4().simple().to_string();
        let temporary = dir.join(format!(".netstore-{token}.partial"));
        match fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&temporary)
        {
            Ok(f) => {
                return Ok(StreamFile {
                    ctx: ctx.clone(),
                    out_dir: dir,
                    instance: sop_instance_uid.to_string(),
                    file: Some(f),
                    temporary,
                    finished: false,
                });
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => last = Some(e),
            Err(e) => return Err(netstore_failure("create temporary", io_error(&e), false)),
        }
    }
    Err(netstore_failure(
        "create temporary",
        io_error(&last.unwrap_or_else(|| std::io::Error::other("exists"))),
        false,
    ))
}
fn closed_error() -> Error {
    Error::new("file already closed")
}
impl StreamFile {
    pub fn write(&mut self, p: &[u8]) -> Result<usize> {
        if self.finished {
            return Err(closed_error());
        }
        let Some(f) = self.file.as_mut() else {
            return Err(closed_error());
        };
        let mut written = 0;
        for chunk in p.chunks(32 << 10) {
            if let Some(e) = context_err(&self.ctx) {
                return Err(e);
            }
            f.write_all(chunk).map_err(|e| io_error(&e))?;
            written += chunk.len();
        }
        Ok(written)
    }
    pub fn read_back(&mut self) -> Result<Vec<u8>> {
        if self.finished {
            return Err(closed_error());
        }
        let Some(f) = self.file.as_mut() else {
            return Err(closed_error());
        };
        f.seek(SeekFrom::Start(0)).map_err(|e| io_error(&e))?;
        let mut out = Vec::new();
        f.read_to_end(&mut out).map_err(|e| io_error(&e))?;
        f.seek(SeekFrom::End(0)).map_err(|e| io_error(&e))?;
        Ok(out)
    }
    pub fn abort(&mut self) -> Result<()> {
        if self.finished {
            return Ok(());
        }
        self.finished = true;
        self.file = None;
        match fs::remove_file(&self.temporary) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(netstore_failure("cleanup", io_error(&e), false)),
        }
    }
    pub fn commit(&mut self) -> Result<String> {
        if self.finished {
            return Err(closed_error());
        }
        self.finished = true;
        let result = self.publish();
        let _ = fs::remove_file(&self.temporary);
        result
    }
    fn publish(&mut self) -> Result<String> {
        if let Some(e) = context_err(&self.ctx) {
            return Err(netstore_failure("write instance", e, false));
        }
        let Some(f) = self.file.take() else {
            return Err(closed_error());
        };
        f.sync_all()
            .map_err(|e| netstore_failure("sync instance", io_error(&e), false))?;
        drop(f);
        match fs::symlink_metadata(&self.out_dir) {
            Ok(m) if m.is_dir() => {}
            _ => {
                return Err(netstore_failure(
                    "verify output",
                    Error::sentinel(Kind::NetstoreUnsafeDirectory),
                    false,
                ));
            }
        }
        for i in 0..1000 {
            if let Some(e) = context_err(&self.ctx) {
                return Err(netstore_failure("publish instance", e, false));
            }
            let name = if i == 0 {
                format!("{}.dcm", self.instance)
            } else {
                format!("{}.{i}.dcm", self.instance)
            };
            let target = self.out_dir.join(&name);
            match fs::hard_link(&self.temporary, &target) {
                Ok(()) => return Ok(path_string(&self.out_dir, &name)),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(netstore_failure("publish instance", io_error(&e), false)),
            }
        }
        Err(Error::sentinel(Kind::NetstoreCollisionLimit))
    }
}
impl Drop for StreamFile {
    fn drop(&mut self) {
        if !self.finished {
            let _ = self.abort();
        }
    }
}
fn path_string(dir: &Path, name: &str) -> String {
    dir.join(name).to_string_lossy().into_owned()
}
pub(crate) type OnStreamStored = Arc<dyn Fn(&Ctx, &str, &CStoreStreamRequest) + Send + Sync>;
#[derive(Clone, Default)]
pub struct FileStreamSink {
    pub dir: String,
    pub ae_titles: FileMetaAeTitles,
    pub verify: bool,
    pub on_stored: Option<OnStreamStored>,
}
impl CStoreStreamSink for FileStreamSink {
    fn open_c_store_stream(
        &self,
        ctx: &Ctx,
        req: &CStoreStreamRequest,
    ) -> Result<Option<Box<dyn CStoreStream>>> {
        let titles = if self.ae_titles == FileMetaAeTitles::default() {
            received_ae_titles(&req.called_ae_title, &req.calling_ae_title)
        } else {
            self.ae_titles.clone()
        };
        let header = req.file_meta_header(&titles).map_err(|e| {
            new_scp_status_error("C-STORE", STATUS_C_STORE_CANNOT_UNDERSTAND, "", Some(e))
        })?;
        let mut file = begin_stream(
            ctx,
            &self.dir,
            object::transfer::normalize_uid(&req.request.affected_sop_instance_uid),
        )
        .map_err(|e| {
            new_scp_status_error("C-STORE", STATUS_C_STORE_OUT_OF_RESOURCES, "", Some(e))
        })?;
        if let Err(e) = file.write(&header) {
            let joined = Error::join([Some(e), file.abort().err()]).unwrap_or_else(closed_error);
            return Err(new_scp_status_error(
                "C-STORE",
                STATUS_C_STORE_OUT_OF_RESOURCES,
                "",
                Some(joined),
            ));
        }
        Ok(Some(Box::new(FileStream {
            sink: self.clone(),
            buffer: Some(BufWriter::with_capacity(256 << 10, FileWriter(file))),
            req: req.clone(),
        })))
    }
}
struct FileWriter(StreamFile);
impl Write for FileWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0
            .write(buf)
            .map_err(|e| std::io::Error::other(e.text().to_string()))
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
struct FileStream {
    sink: FileStreamSink,
    buffer: Option<BufWriter<FileWriter>>,
    req: CStoreStreamRequest,
}
impl FileStream {
    fn file(&mut self) -> Option<&mut StreamFile> {
        self.buffer.as_mut().map(|b| &mut b.get_mut().0)
    }
    fn verify(&mut self) -> (u16, Result<()>) {
        let Some(file) = self.file() else {
            return (STATUS_C_STORE_OUT_OF_RESOURCES, Err(closed_error()));
        };
        let bytes = match file.read_back() {
            Ok(b) => b,
            Err(e) => return (STATUS_C_STORE_OUT_OF_RESOURCES, Err(e)),
        };
        let parsed = match object::read_file(&bytes) {
            Ok(f) => f,
            Err((_, e)) => {
                return (
                    STATUS_C_STORE_CANNOT_UNDERSTAND,
                    Err(Error::from(e).prefixed("dicom dimse: stored data set")),
                );
            }
        };
        if let Err(e) = validate_c_store_data_set(
            &self.req.request.affected_sop_class_uid,
            &self.req.request.affected_sop_instance_uid,
            &self.req.presentation_context,
            Some(&parsed.dataset),
        ) {
            return (STATUS_C_STORE_DATA_SET_DOES_NOT_MATCH, Err(e));
        }
        if let Err(e) = parsed
            .dataset
            .check_pixel_data_encoding(&self.req.data_set_syntax)
        {
            return (STATUS_C_STORE_CANNOT_UNDERSTAND, Err(Error::from(e)));
        }
        (STATUS_SUCCESS, Ok(()))
    }
}
impl CStoreStream for FileStream {
    fn write(&mut self, p: &[u8]) -> Result<()> {
        let Some(b) = self.buffer.as_mut() else {
            return Err(closed_error());
        };
        b.write_all(p).map_err(|e| io_error(&e))
    }
    fn abort(&mut self) {
        if let Some(b) = self.buffer.take() {
            let (mut w, _) = b.into_parts();
            let _ = w.0.abort();
        }
    }
    fn commit(&mut self, ctx: &Ctx) -> (u16, Option<Error>) {
        if let Some(b) = self.buffer.as_mut()
            && let Err(e) = b.flush()
        {
            self.abort();
            return (
                STATUS_C_STORE_OUT_OF_RESOURCES,
                Some(new_scp_status_error(
                    "C-STORE",
                    STATUS_C_STORE_OUT_OF_RESOURCES,
                    "",
                    Some(io_error(&e)),
                )),
            );
        }
        if self.sink.verify {
            let (status, r) = self.verify();
            if let Err(e) = r {
                self.abort();
                return (
                    status,
                    Some(new_scp_status_error("C-STORE", status, "", Some(e))),
                );
            }
        }
        let path = match self.file().map(StreamFile::commit) {
            Some(Ok(p)) => p,
            Some(Err(e)) => {
                return (
                    STATUS_C_STORE_OUT_OF_RESOURCES,
                    Some(new_scp_status_error(
                        "C-STORE",
                        STATUS_C_STORE_OUT_OF_RESOURCES,
                        "",
                        Some(e),
                    )),
                );
            }
            None => {
                return (
                    STATUS_C_STORE_OUT_OF_RESOURCES,
                    Some(new_scp_status_error(
                        "C-STORE",
                        STATUS_C_STORE_OUT_OF_RESOURCES,
                        "",
                        Some(closed_error()),
                    )),
                );
            }
        };
        if let Some(cb) = &self.sink.on_stored {
            cb(ctx, &path, &self.req);
        }
        (STATUS_SUCCESS, None)
    }
}
