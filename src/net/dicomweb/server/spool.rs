use super::super::mimeheader::Header;
use super::{ServerError, ServerLimits, ServerOptions, lease_check_opt, lease_of};
use crate::net::ul::Ctx;
use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::SystemTime;
pub type Sink<'a> = dyn FnMut(&[u8]) -> Result<usize, ServerError> + 'a;
pub fn server_io_error(err: ServerError) -> io::Error {
    io::Error::other(err)
}
pub(crate) fn io_server_error(err: &io::Error) -> ServerError {
    if let Some(inner) = err.get_ref().and_then(|e| e.downcast_ref::<ServerError>()) {
        return inner.clone();
    }
    if err.kind() == io::ErrorKind::UnexpectedEof {
        return ServerError::Other("unexpected EOF".into());
    }
    ServerError::Other(err.to_string())
}
pub(crate) fn random_boundary() -> String {
    let mut out = String::with_capacity(64);
    while out.len() < 60 {
        out.push_str(&uuid::Uuid::new_v4().simple().to_string());
    }
    out.truncate(60);
    out
}
pub(crate) struct MultipartWriter {
    boundary: String,
    started: bool,
}
impl MultipartWriter {
    pub fn new() -> MultipartWriter {
        MultipartWriter {
            boundary: random_boundary(),
            started: false,
        }
    }
    pub fn boundary(&self) -> &str {
        &self.boundary
    }
    pub(crate) fn create_part(
        &mut self,
        header: &Header,
        w: &mut Sink<'_>,
    ) -> Result<(), ServerError> {
        let mut b = if self.started {
            format!("\r\n--{}\r\n", self.boundary)
        } else {
            format!("--{}\r\n", self.boundary)
        };
        self.started = true;
        let mut keys: Vec<&(String, Vec<String>)> = header.0.iter().collect();
        keys.sort_by(|a, b| a.0.cmp(&b.0));
        for (k, values) in keys {
            for v in values {
                b.push_str(&format!("{k}: {v}\r\n"));
            }
        }
        b.push_str("\r\n");
        w(b.as_bytes())?;
        Ok(())
    }
    pub fn close(&mut self, w: &mut Sink<'_>) -> Result<(), ServerError> {
        w(format!("\r\n--{}--\r\n", self.boundary).as_bytes())?;
        Ok(())
    }
}
impl Default for MultipartWriter {
    fn default() -> Self {
        MultipartWriter::new()
    }
}
pub fn create_temp(directory: &str, prefix: &str) -> io::Result<(File, PathBuf)> {
    let dir = if directory.is_empty() {
        std::env::temp_dir()
    } else {
        PathBuf::from(directory)
    };
    for _ in 0..10_000 {
        let n = uuid::Uuid::new_v4().as_u128() as u32;
        let path = dir.join(format!("{prefix}{n}"));
        let mut options = OpenOptions::new();
        options.read(true).write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(&path) {
            Ok(f) => return Ok((f, path)),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::other("createtemp: too many collisions"))
}
pub(crate) struct ResponseSpool {
    pub(crate) file: Option<File>,
    path: Option<PathBuf>,
    maximum: i64,
    pub(crate) size: i64,
}
impl ResponseSpool {
    pub fn new(directory: &str, maximum: i64) -> Result<ResponseSpool, ServerError> {
        Self::with_prefix(directory, ".dicomweb-render-", maximum)
    }
    pub(crate) fn with_prefix(
        directory: &str,
        prefix: &str,
        maximum: i64,
    ) -> Result<ResponseSpool, ServerError> {
        let (file, path) = create_temp(directory, prefix).map_err(|_| ServerError::Backend)?;
        Ok(ResponseSpool {
            file: Some(file),
            path: Some(path),
            maximum,
            size: 0,
        })
    }
    pub fn write(&mut self, data: &[u8]) -> Result<usize, ServerError> {
        let Some(file) = self.file.as_mut() else {
            return Err(ServerError::Backend);
        };
        if data.len() as i64 > self.maximum - self.size {
            return Err(ServerError::ResourceLimit);
        }
        file.write_all(data)
            .map_err(|e| ServerError::Other(e.to_string()))?;
        self.size += data.len() as i64;
        Ok(data.len())
    }
    pub fn rewind(&mut self) -> Result<(), ServerError> {
        let Some(file) = self.file.as_mut() else {
            return Err(ServerError::Backend);
        };
        file.seek(SeekFrom::Start(0))
            .map(|_| ())
            .map_err(|e| ServerError::Other(e.to_string()))
    }
    pub fn reader(&mut self) -> Option<&mut File> {
        self.file.as_mut()
    }
    pub fn close(&mut self) -> Result<(), ServerError> {
        self.file = None;
        if let Some(path) = self.path.take()
            && let Err(e) = fs::remove_file(&path)
            && e.kind() != io::ErrorKind::NotFound
        {
            return Err(ServerError::Backend);
        }
        Ok(())
    }
}
impl Drop for ResponseSpool {
    fn drop(&mut self) {
        let _ = self.close();
    }
}
pub struct JsonArraySpool {
    spool: ResponseSpool,
    pub(crate) count: usize,
    finalized: bool,
    failed: Option<ServerError>,
}
impl JsonArraySpool {
    pub fn new(directory: &str, maximum: i64) -> Result<JsonArraySpool, ServerError> {
        let mut spool = ResponseSpool::with_prefix(directory, ".dicomweb-json-", maximum)?;
        if let Some(file) = spool.file.as_mut()
            && file.write_all(b"[").is_err()
        {
            return Err(ServerError::Backend);
        }
        spool.size = 1;
        Ok(JsonArraySpool {
            spool,
            count: 0,
            finalized: false,
            failed: None,
        })
    }
    fn raw(&mut self, data: &[u8]) -> Result<(), ServerError> {
        match self.spool.file.as_mut() {
            Some(f) => f.write_all(data).map_err(|_| ServerError::Backend),
            None => Err(ServerError::Backend),
        }
    }
    pub fn append(&mut self, data: &[u8]) -> Result<(), ServerError> {
        if let Some(e) = &self.failed {
            return Err(e.clone());
        }
        let result = self.append_inner(data);
        if let Err(e) = &result {
            self.failed = Some(e.clone());
        }
        result
    }
    fn append_inner(&mut self, data: &[u8]) -> Result<(), ServerError> {
        if self.spool.file.is_none() || self.finalized {
            return Err(ServerError::Backend);
        }
        let max = self.spool.maximum;
        let mut extra = data.len() as i64;
        if self.count > 0 {
            extra += 1;
        }
        if extra > max - 1 || self.spool.size > max - 1 - extra {
            return Err(ServerError::ResourceLimit);
        }
        if self.count > 0 {
            self.raw(b",")?;
            self.spool.size += 1;
        }
        self.raw(data)?;
        self.spool.size += data.len() as i64;
        self.count += 1;
        Ok(())
    }
    pub fn append_encoded(
        &mut self,
        encoded: Result<Vec<u8>, ServerError>,
    ) -> Result<(), ServerError> {
        if let Some(e) = &self.failed {
            return Err(e.clone());
        }
        let result = self.append_encoded_inner(encoded);
        if let Err(e) = &result {
            self.failed = Some(e.clone());
        }
        result
    }
    fn append_encoded_inner(
        &mut self,
        encoded: Result<Vec<u8>, ServerError>,
    ) -> Result<(), ServerError> {
        if self.spool.file.is_none() || self.finalized {
            return Err(ServerError::Backend);
        }
        if self.count > 0 {
            self.write(b",")?;
        }
        let data = encoded?;
        for piece in data.chunks(32 << 10) {
            self.write(piece)?;
        }
        self.count += 1;
        Ok(())
    }
    fn write(&mut self, data: &[u8]) -> Result<usize, ServerError> {
        let max = self.spool.maximum;
        if self.spool.size >= max || data.len() as i64 > max - 1 - self.spool.size {
            return Err(ServerError::ResourceLimit);
        }
        self.raw(data)?;
        self.spool.size += data.len() as i64;
        Ok(data.len())
    }
    pub fn finalize(&mut self) -> Result<(), ServerError> {
        if let Some(e) = &self.failed {
            return Err(e.clone());
        }
        if self.spool.file.is_none() || self.finalized {
            return Err(ServerError::Backend);
        }
        if self.spool.size >= self.spool.maximum {
            return Err(ServerError::ResourceLimit);
        }
        self.raw(b"]")?;
        self.spool.size += 1;
        self.finalized = true;
        self.spool.rewind().map_err(|_| ServerError::Backend)
    }
    pub fn reader(&mut self) -> Option<&mut File> {
        self.spool.reader()
    }
    pub fn close(&mut self) -> Result<(), ServerError> {
        self.spool.close()
    }
}
pub(crate) fn copy_bounded(
    ctx: &Ctx,
    destination: &mut Sink<'_>,
    source: &mut dyn Read,
    maximum: i64,
) -> (i64, Result<(), ServerError>) {
    let lease = lease_of(ctx);
    let mut buffer = vec![0u8; 32 << 10];
    let mut total: i64 = 0;
    loop {
        if let Err(e) = lease_check_opt(lease.as_ref(), ctx) {
            return (total, Err(e));
        }
        let remaining = maximum - total;
        if remaining < 0 {
            return (total, Err(ServerError::ResourceLimit));
        }
        let mut read_size = buffer.len();
        if read_size as i64 > remaining + 1 {
            read_size = (remaining + 1) as usize;
        }
        let (n, read_err) = match source.read(&mut buffer[..read_size]) {
            Ok(n) => (n, None),
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => (0, Some(e)),
        };
        if n > 0 {
            if lease.is_some()
                && let Err(e) = lease_check_opt(lease.as_ref(), ctx)
            {
                return (total, Err(e));
            }
            if total + n as i64 > maximum {
                return (total, Err(ServerError::ResourceLimit));
            }
            match destination(&buffer[..n]) {
                Ok(written) => {
                    total += written as i64;
                    if written != n {
                        return (total, Err(ServerError::Other("short write".into())));
                    }
                }
                Err(e) => return (total, Err(e)),
            }
            continue;
        }
        if let Some(e) = read_err {
            return (total, Err(io_server_error(&e)));
        }
        return (total, Ok(()));
    }
}
fn active_retrieve_directories() -> &'static Mutex<HashSet<PathBuf>> {
    static ACTIVE: OnceLock<Mutex<HashSet<PathBuf>>> = OnceLock::new();
    ACTIVE.get_or_init(|| Mutex::new(HashSet::new()))
}
pub(crate) fn retrieve_directory_key(path: &Path) -> PathBuf {
    let mut p = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    if let Ok(physical) = fs::canonicalize(&p) {
        p = physical;
    }
    p
}
pub(crate) fn activate_retrieve_directory(path: &Path) {
    super::lock(active_retrieve_directories()).insert(retrieve_directory_key(path));
}
pub(crate) fn deactivate_retrieve_directory(path: &Path) {
    super::lock(active_retrieve_directories()).remove(&retrieve_directory_key(path));
}
fn is_active(path: &Path) -> bool {
    super::lock(active_retrieve_directories()).contains(&retrieve_directory_key(path))
}
struct SpoolEntry {
    name: String,
    size: i64,
    mod_time: SystemTime,
    removed: bool,
    job_directory: bool,
}
pub(crate) fn owns_spool_name(name: &str) -> bool {
    [
        ".dicomweb-json-",
        ".dicomweb-stow-",
        ".dicomweb-render-",
        ".dicomweb-retrieve-",
    ]
    .iter()
    .any(|p| name.starts_with(p) && name.len() > p.len())
}
fn retrieve_job_entries(path: &Path) -> Option<(Vec<String>, i64)> {
    let mut entries: Vec<fs::DirEntry> = fs::read_dir(path)
        .ok()?
        .filter_map(Result::ok)
        .take(17)
        .collect();
    if entries.len() > 16 {
        return None;
    }
    entries.sort_by_key(|e| e.file_name());
    let mut children = Vec::new();
    let mut total: i64 = 0;
    for entry in entries {
        let name = entry.file_name().to_string_lossy().into_owned();
        let mut known = name == "source.dcm" || name == "output.dcm";
        const PREFIX: &str = ".dicom-transcode-";
        if name.starts_with(PREFIX) && name.len() == PREFIX.len() + 32 {
            known = name[PREFIX.len()..]
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c));
        }
        let file_type = entry.file_type().ok()?;
        let meta = entry.metadata().ok()?;
        if !known || file_type.is_symlink() || !meta.is_file() {
            return None;
        }
        total = total.checked_add(meta.len() as i64)?;
        children.push(name);
    }
    Some((children, total))
}
pub(crate) fn scavenge(
    options: &ServerOptions,
    limits: &ServerLimits,
    now: SystemTime,
) -> Result<(), ServerError> {
    let directory = if options.spool_directory.is_empty() {
        std::env::temp_dir()
    } else {
        PathBuf::from(&options.spool_directory)
    };
    let mut entries: Vec<fs::DirEntry> = match fs::read_dir(&directory) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        #[cfg(windows)]
        Err(e) if e.kind() == io::ErrorKind::NotADirectory => return Ok(()),
        Err(_) => {
            return Err(ServerError::InvalidServerOptions(
                "spool directory is unavailable".into(),
            ));
        }
        Ok(rd) => rd.filter_map(Result::ok).collect(),
    };
    entries.sort_by_key(|e| e.file_name());
    let mut owned: Vec<SpoolEntry> = Vec::new();
    let mut aggregate: i64 = 0;
    for entry in &entries {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if name.starts_with(".dicomweb-retrieve-") && file_type.is_dir() && !file_type.is_symlink()
        {
            let path = directory.join(&name);
            if is_active(&path) {
                continue;
            }
            let info = entry.metadata();
            if let (Ok(info), Some((_, size))) = (info, retrieve_job_entries(&path))
                && size <= i64::MAX - aggregate
            {
                aggregate += size;
                owned.push(SpoolEntry {
                    name,
                    size,
                    mod_time: info.modified().unwrap_or(now),
                    removed: false,
                    job_directory: true,
                });
            }
            continue;
        }
        if !owns_spool_name(&name) || file_type.is_symlink() {
            continue;
        }
        let Ok(info) = entry.metadata() else {
            continue;
        };
        let size = info.len() as i64;
        if !info.is_file() || size < 0 || aggregate > i64::MAX - size {
            continue;
        }
        aggregate += size;
        owned.push(SpoolEntry {
            name,
            size,
            mod_time: info.modified().unwrap_or(now),
            removed: false,
            job_directory: false,
        });
    }
    let sub = |d: crate::net::ul::Nanos| {
        let nanos = d.0.max(0) as u64;
        now.checked_sub(std::time::Duration::from_nanos(nanos))
            .unwrap_or(SystemTime::UNIX_EPOCH)
    };
    let active_cutoff = sub(limits.max_duration);
    let retention_cutoff = sub(options.spool_retention_age);
    let remove_entry = |entry: &mut SpoolEntry, aggregate: &mut i64| {
        let path = directory.join(&entry.name);
        let info = fs::symlink_metadata(&path);
        if let Err(e) = &info
            && e.kind() == io::ErrorKind::NotFound
        {
            entry.removed = true;
            return;
        }
        if entry.job_directory {
            let Ok(info) = info else {
                return;
            };
            if !info.is_dir() || info.file_type().is_symlink() || is_active(&path) {
                return;
            }
            let Some((children, _)) = retrieve_job_entries(&path) else {
                return;
            };
            for child in children {
                if fs::remove_file(path.join(child)).is_err() {
                    return;
                }
            }
            if fs::remove_dir(&path).is_err() {
                return;
            }
            entry.removed = true;
            *aggregate -= entry.size;
            return;
        }
        match info {
            Ok(info) if info.is_file() => {}
            _ => return,
        }
        if fs::remove_file(&path).is_err() {
            return;
        }
        entry.removed = true;
        *aggregate -= entry.size;
    };
    for entry in owned.iter_mut() {
        if entry.mod_time < active_cutoff && entry.mod_time < retention_cutoff {
            remove_entry(entry, &mut aggregate);
        }
    }
    if aggregate <= options.spool_aggregate_quota_bytes {
        return Ok(());
    }
    owned.sort_by_key(|e| e.mod_time);
    for entry in owned.iter_mut() {
        if aggregate <= options.spool_aggregate_quota_bytes {
            break;
        }
        if entry.removed || entry.mod_time >= active_cutoff {
            continue;
        }
        remove_entry(entry, &mut aggregate);
    }
    Ok(())
}
