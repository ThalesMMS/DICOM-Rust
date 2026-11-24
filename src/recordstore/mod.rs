#![forbid(unsafe_code)]
use crate::net::ul::Ctx;
use crate::net::ul::Kind as CtxKind;
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;
#[cfg(unix)]
#[path = "platform_unix.rs"]
mod platform;
#[cfg(windows)]
#[path = "platform_windows.rs"]
mod platform;
#[cfg(not(any(unix, windows)))]
#[path = "platform_other.rs"]
mod platform;
pub const LOCK_NAME: &str = ".recordstore.lock";
pub const TEMP_PREFIX: &str = ".recordstore-";
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    NotFound,
    Limit,
    Unavailable,
    Invalid,
    Canceled,
    DeadlineExceeded,
    Io(String),
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Error::NotFound => "dicom record store: record not found",
            Error::Limit => "dicom record store: logical disk budget exceeded",
            Error::Unavailable => "dicom record store: unavailable or inconsistent",
            Error::Invalid => "dicom record store: invalid argument or record",
            Error::Canceled => "context canceled",
            Error::DeadlineExceeded => "context deadline exceeded",
            Error::Io(text) => text,
        })
    }
}
impl std::error::Error for Error {}
pub type Result<T> = std::result::Result<T, Error>;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Options {
    pub max_records: i64,
    pub max_record_bytes: i64,
    pub max_total_bytes: i64,
}
pub(crate) fn context_error(ctx: Option<&Ctx>) -> Result<()> {
    match ctx.and_then(Ctx::err) {
        Some(CtxKind::DeadlineExceeded) => Err(Error::DeadlineExceeded),
        Some(_) => Err(Error::Canceled),
        None => Ok(()),
    }
}
pub fn valid_key(key: &str) -> bool {
    key.len() == 69
        && key.ends_with(".json")
        && key.as_bytes()[..64]
            .iter()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(c))
}
fn valid_temporary(name: &str) -> bool {
    name.len() == TEMP_PREFIX.len() + 32
        && name.starts_with(TEMP_PREFIX)
        && name.as_bytes()[TEMP_PREFIX.len()..]
            .iter()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(c))
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Stamp {
    id: (u64, u64),
    len: u64,
    modified: Option<std::time::SystemTime>,
    regular: bool,
    dir: bool,
}
fn stamp(m: &fs::Metadata) -> Stamp {
    Stamp {
        id: platform::file_id(m),
        len: m.len(),
        modified: m.modified().ok(),
        regular: m.file_type().is_file(),
        dir: m.file_type().is_dir(),
    }
}
fn same_file(a: &Stamp, b: &Stamp) -> bool {
    a.regular && b.regular && a.id == b.id && a.len == b.len && a.modified == b.modified
}
fn refused() -> io::Error {
    io::Error::other("dicom nofollow: link or reparse point refused")
}
fn absolute(dir: &Path) -> io::Result<PathBuf> {
    let abs = std::path::absolute(dir)?;
    Ok(clean(&abs))
}
fn clean(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                if out.file_name().is_some() {
                    out.pop();
                }
            }
            c => out.push(c.as_os_str()),
        }
    }
    out
}
fn entry_metadata(path: &Path) -> io::Result<fs::Metadata> {
    let m = fs::symlink_metadata(path)?;
    if m.file_type().is_symlink() || platform::is_reparse(&m) {
        return Err(refused());
    }
    Ok(m)
}
fn nofollow_directory(path: &Path) -> io::Result<fs::Metadata> {
    nofollow_directory_hops(path, 0)
}
fn nofollow_directory_hops(path: &Path, hops: usize) -> io::Result<fs::Metadata> {
    let components: Vec<Component> = path.components().collect();
    let mut current = PathBuf::new();
    for (index, c) in components.iter().enumerate() {
        match c {
            Component::Normal(name) => {
                let next = current.join(name);
                let m = fs::symlink_metadata(&next)?;
                if m.file_type().is_symlink() || platform::is_reparse(&m) {
                    if let Some(target) = platform::system_symlink_target(&current, &next, &m)
                        && hops < 8
                    {
                        let mut rest = if target.is_absolute() {
                            target
                        } else {
                            current.join(target)
                        };
                        for c in &components[index + 1..] {
                            rest.push(c.as_os_str());
                        }
                        return nofollow_directory_hops(&clean(&rest), hops + 1);
                    }
                    return Err(refused());
                }
                if !m.is_dir() {
                    return Err(io::Error::other("dicom nofollow: not a directory"));
                }
                current = next;
            }
            c => current.push(c.as_os_str()),
        }
    }
    let m = fs::symlink_metadata(&current)?;
    if m.file_type().is_symlink() || platform::is_reparse(&m) || !m.is_dir() {
        return Err(refused());
    }
    Ok(m)
}
fn retry_sharing<T>(mut op: impl FnMut() -> io::Result<T>) -> io::Result<T> {
    let mut attempts = 0;
    loop {
        match op() {
            Err(e)
                if cfg!(windows)
                    && e.kind() == io::ErrorKind::PermissionDenied
                    && attempts < 50 =>
            {
                attempts += 1;
                std::thread::sleep(std::time::Duration::from_millis(2 * attempts));
            }
            r => return r,
        }
    }
}
fn remove_at(dir: &Path, name: &str) -> io::Result<()> {
    match retry_sharing(|| fs::remove_file(dir.join(name))) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        r => r,
    }
}
fn open_at(dir: &Path, name: &str) -> io::Result<(File, fs::Metadata)> {
    let path = dir.join(name);
    let m = entry_metadata(&path)?;
    if m.is_dir() && !platform::OPENS_DIRECTORY_ENTRIES {
        return Err(io::Error::other("dicom nofollow: directory entry"));
    }
    let f = File::open(&path)?;
    let i = f.metadata()?;
    Ok((f, i))
}
fn temporary_name() -> String {
    let a = uuid::Uuid::new_v4().as_u128();
    let b = uuid::Uuid::new_v4().as_u128();
    format!("{TEMP_PREFIX}{:032x}", a ^ b.rotate_left(64))
}
#[derive(Debug)]
struct Inner {
    dir: PathBuf,
    info: Stamp,
    pinned: Option<File>,
    lock: Option<File>,
    files: HashMap<String, Stamp>,
    bytes: i64,
    options: Options,
    closed: bool,
    unhealthy: bool,
}
#[derive(Debug)]
pub struct Store {
    inner: Mutex<Inner>,
}
impl Store {
    pub fn open(ctx: Option<&Ctx>, dir: impl AsRef<Path>, opts: Options) -> Result<Store> {
        context_error(ctx)?;
        let dir = dir.as_ref();
        if dir.as_os_str().to_string_lossy().trim().is_empty()
            || opts.max_records < 1
            || opts.max_records > 1_000_000
            || opts.max_record_bytes < 1
            || opts.max_record_bytes > 16 << 20
            || opts.max_total_bytes < opts.max_record_bytes
            || opts.max_total_bytes > 64 << 30
        {
            return Err(Error::Invalid);
        }
        let abs = absolute(dir).map_err(|_| Error::Invalid)?;
        let (parent, leaf) = match (abs.parent(), abs.file_name()) {
            (Some(p), Some(_)) => (p.to_path_buf(), abs.clone()),
            _ => return Err(Error::Unavailable),
        };
        nofollow_directory(&parent).map_err(|_| Error::Unavailable)?;
        let created = match platform::create_directory(&leaf) {
            Ok(()) => true,
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => false,
            Err(_) => return Err(Error::Unavailable),
        };
        let m = entry_metadata(&leaf).map_err(|_| Error::Unavailable)?;
        if !m.is_dir() {
            return Err(Error::Unavailable);
        }
        let pinned = platform::open_directory(&leaf, created).map_err(|_| Error::Unavailable)?;
        if created {
            platform::sync_directory(&parent).map_err(|_| Error::Unavailable)?;
        }
        let m = pinned.metadata().map_err(|_| Error::Unavailable)?;
        if !m.is_dir() {
            return Err(Error::Unavailable);
        }
        let info = stamp(&m);
        let lock = match platform::create_file(&leaf.join(LOCK_NAME)) {
            Ok(f) => f,
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                open_at(&leaf, LOCK_NAME).map_err(|_| Error::Unavailable)?.0
            }
            Err(_) => return Err(Error::Unavailable),
        };
        let li = lock.metadata().map_err(|_| Error::Unavailable)?;
        if !li.is_file() || li.len() != 0 || platform::lock_file(&lock).is_err() {
            return Err(Error::Unavailable);
        }
        let mut s = Inner {
            dir: leaf,
            info,
            pinned: Some(pinned),
            lock: Some(lock),
            files: HashMap::new(),
            bytes: 0,
            options: opts,
            closed: false,
            unhealthy: false,
        };
        if let Err(e) = s.replay(ctx) {
            s.close();
            return Err(e);
        }
        Ok(Store {
            inner: Mutex::new(s),
        })
    }
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }
    pub fn keys(&self, ctx: Option<&Ctx>) -> Result<Vec<String>> {
        let s = self.lock();
        s.usable(ctx)?;
        let mut keys: Vec<String> = s.files.keys().cloned().collect();
        keys.sort();
        Ok(keys)
    }
    pub fn read(&self, ctx: Option<&Ctx>, key: &str) -> Result<Vec<u8>> {
        let s = self.lock();
        s.usable(ctx)?;
        s.read(ctx, key)
    }
    pub fn create(&self, ctx: Option<&Ctx>, key: &str, data: &[u8]) -> Result<bool> {
        let mut s = self.lock();
        s.usable(ctx)?;
        if !valid_key(key) {
            return Err(Error::Invalid);
        }
        if s.files.contains_key(key) {
            s.read(ctx, key)?;
            return Ok(false);
        }
        if s.files.len() as i64 >= s.options.max_records {
            return Err(Error::Limit);
        }
        s.write(ctx, key, data, false)?;
        Ok(true)
    }
    pub fn replace(&self, ctx: Option<&Ctx>, key: &str, data: &[u8]) -> Result<()> {
        let mut s = self.lock();
        s.usable(ctx)?;
        s.read(ctx, key)?;
        s.write(ctx, key, data, true)
    }
    pub fn delete(&self, ctx: Option<&Ctx>, key: &str) -> Result<()> {
        let mut s = self.lock();
        s.usable(ctx)?;
        s.read(ctx, key)?;
        remove_at(&s.dir, key).map_err(|_| Error::Unavailable)?;
        let dir = s.dir.clone();
        if platform::sync_directory(&dir).is_err() {
            s.unhealthy = true;
            return Err(Error::Unavailable);
        }
        if let Some(i) = s.files.remove(key) {
            s.bytes -= i.len as i64;
        }
        Ok(())
    }
    pub fn close(&self) -> Result<()> {
        self.lock().close();
        Ok(())
    }
}
impl Inner {
    fn close(&mut self) {
        if self.closed {
            return;
        }
        self.closed = true;
        if let Some(lock) = self.lock.take() {
            let _ = platform::unlock_file(&lock);
        }
        self.pinned = None;
    }
    fn usable(&self, ctx: Option<&Ctx>) -> Result<()> {
        context_error(ctx)?;
        if self.closed || self.unhealthy {
            return Err(Error::Unavailable);
        }
        match nofollow_directory(&self.dir) {
            Ok(m) if stamp(&m).id == self.info.id => Ok(()),
            _ => Err(Error::Unavailable),
        }
    }
    fn replay(&mut self, ctx: Option<&Ctx>) -> Result<()> {
        let entries = fs::read_dir(&self.dir).map_err(|_| Error::Unavailable)?;
        let limit = (self.options.max_records + 65) as usize;
        let mut names = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|_| Error::Unavailable)?;
            names.push(entry.file_name());
            if names.len() >= limit {
                break;
            }
        }
        if names.len() as i64 > self.options.max_records + 64 {
            return Err(Error::Limit);
        }
        let mut temps = Vec::new();
        for name in names {
            context_error(ctx)?;
            let Some(name) = name.to_str().map(str::to_string) else {
                return Err(Error::Invalid);
            };
            if name == LOCK_NAME {
                continue;
            }
            if !valid_key(&name) && !valid_temporary(&name) {
                return Err(Error::Invalid);
            }
            let (f, i) = open_at(&self.dir, &name).map_err(|_| Error::Unavailable)?;
            drop(f);
            if !i.is_file() {
                return Err(Error::Invalid);
            }
            if name.starts_with(TEMP_PREFIX) {
                temps.push(name);
                continue;
            }
            let size = i.len() as i64;
            if self.files.len() as i64 >= self.options.max_records
                || size < 1
                || size > self.options.max_record_bytes
                || size > self.options.max_total_bytes - self.bytes
            {
                return Err(Error::Limit);
            }
            self.files.insert(name, stamp(&i));
            self.bytes += size;
        }
        for name in &temps {
            remove_at(&self.dir, name).map_err(|_| Error::Unavailable)?;
        }
        if !temps.is_empty() {
            platform::sync_directory(&self.dir).map_err(|_| Error::Unavailable)?;
        }
        self.usable(ctx)
    }
    fn read(&self, ctx: Option<&Ctx>, key: &str) -> Result<Vec<u8>> {
        if !valid_key(key) {
            return Err(Error::Invalid);
        }
        let Some(expected) = self.files.get(key) else {
            return Err(Error::NotFound);
        };
        let (mut f, i) = open_at(&self.dir, key).map_err(|_| Error::Unavailable)?;
        let i = stamp(&i);
        if !same_file(&i, expected) {
            return Err(Error::Unavailable);
        }
        context_error(ctx)?;
        let mut data = Vec::new();
        (&mut f)
            .take((self.options.max_record_bytes + 1) as u64)
            .read_to_end(&mut data)
            .map_err(|e| Error::Io(e.to_string()))?;
        let after = f.metadata().map_err(|_| Error::Unavailable)?;
        if data.len() as u64 != i.len || !same_file(&i, &stamp(&after)) {
            return Err(Error::Unavailable);
        }
        Ok(data)
    }
    fn write(&mut self, ctx: Option<&Ctx>, key: &str, data: &[u8], replace: bool) -> Result<()> {
        let old = self.files.get(key).map_or(0, |i| i.len as i64);
        let n = data.len() as i64;
        if n < 1
            || n > self.options.max_record_bytes
            || n > self.options.max_total_bytes - (self.bytes - old)
        {
            return Err(Error::Limit);
        }
        let temp = temporary_name();
        let mut f = platform::create_file(&self.dir.join(&temp)).map_err(|_| Error::Unavailable)?;
        let mut published = false;
        let result = self.publish_temporary(ctx, &mut f, &temp, key, data, replace, &mut published);
        drop(f);
        if !published {
            let _ = remove_at(&self.dir, &temp);
        }
        result?;
        if platform::sync_directory(&self.dir).is_err() {
            self.unhealthy = true;
            return Err(Error::Unavailable);
        }
        if let Err(e) = self.usable(ctx) {
            self.unhealthy = true;
            return Err(e);
        }
        let i = match open_at(&self.dir, key) {
            Ok((_, i)) if i.is_file() && i.len() as i64 == n => i,
            _ => {
                self.unhealthy = true;
                return Err(Error::Unavailable);
            }
        };
        self.files.insert(key.to_string(), stamp(&i));
        self.bytes = self.bytes - old + n;
        Ok(())
    }
    #[allow(clippy::too_many_arguments)]
    fn publish_temporary(
        &mut self,
        ctx: Option<&Ctx>,
        f: &mut File,
        temp: &str,
        key: &str,
        data: &[u8],
        replace: bool,
        published: &mut bool,
    ) -> Result<()> {
        context_error(ctx)?;
        f.write_all(data).map_err(|e| Error::Io(e.to_string()))?;
        f.sync_all().map_err(|_| Error::Unavailable)?;
        self.usable(ctx)?;
        let (was_published, r) = platform::publish(&self.dir, f, temp, key, replace);
        *published = was_published;
        if r.is_err() {
            if was_published {
                self.unhealthy = true;
            }
            return Err(Error::Unavailable);
        }
        Ok(())
    }
}
impl Drop for Inner {
    fn drop(&mut self) {
        self.close();
    }
}
