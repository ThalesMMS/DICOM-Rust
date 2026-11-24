#![forbid(unsafe_code)]
use super::error::{Error, Kind, Result};
use super::pdata::{PDataReader, read_pdata_data_set};
use crate::net::ul::Ctx;
use crate::object::parser::source::MemFile;
use crate::object::{self, Object, ReadFileOptions, Syntax};
use std::fs;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
pub const DATA_SET_SPOOL_PREFIX: &str = ".dimse-spool-";
pub const DEFAULT_SPOOL_INLINE_BYTES: i64 = 64 << 10;
const SPOOL_BUFFER_SIZE: usize = 256 << 10;
const SPOOL_REMOVE_ATTEMPTS: usize = 5;
const SPOOL_REMOVE_DELAY: Duration = Duration::from_millis(20);
pub type OnSpoolRemoveError = Arc<dyn Fn(&str, &Error) + Send + Sync>;
#[derive(Clone, Default)]
pub(crate) struct SpoolTarget {
    pub dir: String,
    #[allow(dead_code)]
    pub inline: i64,
    pub on_remove_error: Option<OnSpoolRemoveError>,
}
#[derive(Default)]
pub struct SpoolRelease {
    path: Option<PathBuf>,
    on_error: Option<OnSpoolRemoveError>,
}
impl SpoolRelease {
    pub fn release(&mut self) {
        if let Some(path) = self.path.take() {
            remove_spool_file(&path, self.on_error.as_ref());
        }
    }
}
impl Drop for SpoolRelease {
    fn drop(&mut self) {
        self.release();
    }
}
fn os_error(e: &std::io::Error) -> Error {
    Error::new(e.to_string())
}
pub(crate) fn remove_spool_file(path: &Path, on_error: Option<&OnSpoolRemoveError>) {
    let mut delay = SPOOL_REMOVE_DELAY;
    let mut last = None;
    for attempt in 0..SPOOL_REMOVE_ATTEMPTS {
        if attempt > 0 {
            std::thread::sleep(delay);
            delay *= 2;
        }
        match fs::remove_file(path) {
            Ok(()) => return,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
            Err(e) => last = Some(e),
        }
    }
    if let (Some(cb), Some(e)) = (on_error, last) {
        cb(&path.to_string_lossy(), &os_error(&e));
    }
}
fn create_temp(dir: &str) -> std::io::Result<(PathBuf, fs::File)> {
    let base = if dir.is_empty() {
        std::env::temp_dir()
    } else {
        PathBuf::from(dir)
    };
    let mut last = None;
    for _ in 0..10000 {
        let name = format!(
            "{DATA_SET_SPOOL_PREFIX}{}",
            uuid::Uuid::new_v4().as_u128() % 10_000_000_000
        );
        let path = base.join(name);
        match fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(f) => return Ok((path, f)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => last = Some(e),
            Err(e) => return Err(e),
        }
    }
    Err(last.unwrap_or_else(|| std::io::Error::other("spool file name collision")))
}
pub(crate) fn spool_data_set(
    reader: &mut PDataReader<'_>,
    syntax: Syntax,
    spool: &SpoolTarget,
    opts: ReadFileOptions,
) -> (Result<Object>, SpoolRelease) {
    if syntax.deflated {
        return (
            read_pdata_data_set(reader, syntax, opts),
            SpoolRelease::default(),
        );
    }
    let (path, file) = match create_temp(&spool.dir) {
        Ok(v) => v,
        Err(e) => {
            return (
                Err(Error::wrap_msg_err(
                    Kind::DataSetSpool,
                    "create",
                    os_error(&e),
                )),
                SpoolRelease::default(),
            );
        }
    };
    let mut release = SpoolRelease {
        path: Some(path.clone()),
        on_error: spool.on_remove_error.clone(),
    };
    let max_bytes = opts.max_total_bytes;
    let mut writer = BufWriter::with_capacity(SPOOL_BUFFER_SIZE, file);
    let mut written: i64 = 0;
    let mut buf = vec![0u8; 32 << 10];
    let limit = if max_bytes > 0 {
        max_bytes + 1
    } else {
        i64::MAX
    };
    let copy: Result<()> = (|| {
        while written < limit {
            let want = buf.len().min((limit - written) as usize);
            let n = reader.read_bytes(&mut buf[..want])?;
            if n == 0 {
                break;
            }
            writer
                .write_all(&buf[..n])
                .map_err(|e| Error::wrap_msg_err(Kind::DataSetSpool, "write", os_error(&e)))?;
            written += n as i64;
        }
        writer
            .flush()
            .map_err(|e| Error::wrap_msg_err(Kind::DataSetSpool, "write", os_error(&e)))
    })();
    let file = writer.into_inner().ok();
    drop(file);
    if let Err(e) = copy {
        release.release();
        return (Err(e), SpoolRelease::default());
    }
    if max_bytes > 0 && written > max_bytes {
        release.release();
        let mut e = Error::from(object::Error::sentinel(object::Kind::MaxTotalBytes));
        e = e.prefixed("dicom dimse: spooled data set");
        return (Err(e), SpoolRelease::default());
    }
    let data = match fs::read(&path) {
        Ok(d) => d,
        Err(e) => {
            release.release();
            return (
                Err(Error::wrap_msg_err(
                    Kind::DataSetSpool,
                    "rewind",
                    os_error(&e),
                )),
                SpoolRelease::default(),
            );
        }
    };
    match object::read_data_set_with_options(
        Box::new(MemFile::new(std::rc::Rc::from(data.as_slice()))),
        syntax,
        opts,
    ) {
        Ok(o) => (Ok(o), release),
        Err(e) => {
            release.release();
            (Err(Error::from(e)), SpoolRelease::default())
        }
    }
}
#[derive(Clone)]
pub struct CGetSpoolDir(pub String);
#[derive(Clone)]
pub struct CGetSpoolRemoveError(pub OnSpoolRemoveError);
pub fn with_c_get_spool_dir(ctx: &Ctx, dir: &str) -> Ctx {
    if dir.is_empty() {
        return ctx.clone();
    }
    ctx.with_value(CGetSpoolDir(dir.to_string()))
}
pub fn with_c_get_spool_remove_error(ctx: &Ctx, on_error: Option<OnSpoolRemoveError>) -> Ctx {
    match on_error {
        None => ctx.clone(),
        Some(cb) => ctx.with_value(CGetSpoolRemoveError(cb)),
    }
}
pub(crate) fn c_get_spool_dir(ctx: &Ctx) -> String {
    ctx.value::<CGetSpoolDir>()
        .map(|d| d.0.clone())
        .unwrap_or_default()
}
pub(crate) fn c_get_spool_remove_error(ctx: &Ctx) -> Option<OnSpoolRemoveError> {
    ctx.value::<CGetSpoolRemoveError>().map(|c| c.0.clone())
}
