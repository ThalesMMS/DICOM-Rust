#![forbid(unsafe_code)]
use super::commitment::*;
use super::commitment_workflow::*;
use super::controls::context_err;
use super::error::{Error, Kind, Result};
use crate::net::ul::{self, Ctx};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;
const LOCK_NAME: &str = ".recordstore.lock";
const TEMP_PREFIX: &str = ".recordstore-";
#[derive(Debug)]
enum RsError {
    NotFound,
    Limit,
    Unavailable,
    Invalid,
    Context(Error),
}
type RsResult<T> = std::result::Result<T, RsError>;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Stamp {
    id: (u64, u64),
    len: u64,
    modified: Option<SystemTime>,
    regular: bool,
    dir: bool,
}
#[cfg(unix)]
fn file_id(m: &fs::Metadata) -> (u64, u64) {
    use std::os::unix::fs::MetadataExt;
    (m.dev(), m.ino())
}
#[cfg(windows)]
fn file_id(m: &fs::Metadata) -> (u64, u64) {
    use std::os::windows::fs::MetadataExt;
    (m.creation_time(), 0)
}
#[cfg(not(any(unix, windows)))]
fn file_id(_: &fs::Metadata) -> (u64, u64) {
    (0, 0)
}
fn stamp(m: &fs::Metadata) -> Stamp {
    Stamp {
        id: file_id(m),
        len: m.len(),
        modified: m.modified().ok(),
        regular: m.file_type().is_file(),
        dir: m.file_type().is_dir(),
    }
}
fn nofollow_metadata(path: &Path) -> std::io::Result<fs::Metadata> {
    let m = fs::symlink_metadata(path)?;
    if m.file_type().is_symlink() {
        return Err(std::io::Error::other("symbolic link refused"));
    }
    Ok(m)
}
fn nofollow_directory(path: &Path) -> std::io::Result<Stamp> {
    let m = nofollow_metadata(path)?;
    if !m.is_dir() {
        return Err(std::io::Error::other("not a directory"));
    }
    Ok(stamp(&m))
}
#[cfg(unix)]
fn sync_directory(dir: &Path) -> std::io::Result<()> {
    File::open(dir)?.sync_all()
}
#[cfg(not(unix))]
fn sync_directory(_: &Path) -> std::io::Result<()> {
    Ok(())
}
fn retry_sharing<T>(mut op: impl FnMut() -> std::io::Result<T>) -> std::io::Result<T> {
    let mut attempts = 0;
    loop {
        match op() {
            Err(e)
                if cfg!(windows)
                    && e.kind() == std::io::ErrorKind::PermissionDenied
                    && attempts < 50 =>
            {
                attempts += 1;
                std::thread::sleep(std::time::Duration::from_millis(2 * attempts));
            }
            r => return r,
        }
    }
}
fn valid_key(key: &str) -> bool {
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
#[derive(Debug, Clone, Copy)]
struct RsOptions {
    max_records: i64,
    max_record_bytes: i64,
    max_total_bytes: i64,
}
#[derive(Debug)]
struct RecordStore {
    dir: PathBuf,
    info: Stamp,
    lock: Option<File>,
    files: HashMap<String, Stamp>,
    bytes: i64,
    options: RsOptions,
    closed: bool,
    unhealthy: bool,
}
fn rs_context(ctx: &Ctx) -> RsResult<()> {
    match context_err(ctx) {
        Some(e) => Err(RsError::Context(e)),
        None => Ok(()),
    }
}
impl RecordStore {
    fn open(ctx: &Ctx, dir: &Path, opts: RsOptions) -> RsResult<RecordStore> {
        rs_context(ctx)?;
        if dir.as_os_str().to_string_lossy().trim().is_empty()
            || opts.max_records < 1
            || opts.max_records > 1_000_000
            || opts.max_record_bytes < 1
            || opts.max_record_bytes > 16 << 20
            || opts.max_total_bytes < opts.max_record_bytes
            || opts.max_total_bytes > 64 << 30
        {
            return Err(RsError::Invalid);
        }
        let abs = std::path::absolute(dir).map_err(|_| RsError::Invalid)?;
        let parent = abs
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| abs.clone());
        nofollow_directory(&parent).map_err(|_| RsError::Unavailable)?;
        let created = match nofollow_metadata(&abs) {
            Ok(_) => false,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir(&abs).map_err(|_| RsError::Unavailable)?;
                true
            }
            Err(_) => return Err(RsError::Unavailable),
        };
        if created {
            sync_directory(&parent).map_err(|_| RsError::Unavailable)?;
        }
        let info = nofollow_directory(&abs).map_err(|_| RsError::Unavailable)?;
        let lock_path = abs.join(LOCK_NAME);
        if let Ok(m) = fs::symlink_metadata(&lock_path)
            && m.file_type().is_symlink()
        {
            return Err(RsError::Unavailable);
        }
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .map_err(|_| RsError::Unavailable)?;
        let lm = lock.metadata().map_err(|_| RsError::Unavailable)?;
        if !lm.is_file()
            || lm.len() != 0
            || !fs4::fs_std::FileExt::try_lock_exclusive(&lock).unwrap_or(false)
        {
            return Err(RsError::Unavailable);
        }
        let mut s = RecordStore {
            dir: abs,
            info,
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
        Ok(s)
    }
    fn usable(&self, ctx: &Ctx) -> RsResult<()> {
        rs_context(ctx)?;
        if self.closed || self.unhealthy {
            return Err(RsError::Unavailable);
        }
        match nofollow_directory(&self.dir) {
            Ok(info) if info.id == self.info.id => Ok(()),
            _ => Err(RsError::Unavailable),
        }
    }
    fn replay(&mut self, ctx: &Ctx) -> RsResult<()> {
        let entries = fs::read_dir(&self.dir).map_err(|_| RsError::Unavailable)?;
        let limit = (self.options.max_records + 65) as usize;
        let mut names = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|_| RsError::Unavailable)?;
            names.push(entry.file_name().to_string_lossy().into_owned());
            if names.len() >= limit {
                break;
            }
        }
        if names.len() as i64 > self.options.max_records + 64 {
            return Err(RsError::Limit);
        }
        let mut temps = Vec::new();
        for name in names {
            rs_context(ctx)?;
            if name == LOCK_NAME {
                continue;
            }
            if !valid_key(&name) && !valid_temporary(&name) {
                return Err(RsError::Invalid);
            }
            let m = nofollow_metadata(&self.dir.join(&name)).map_err(|_| RsError::Unavailable)?;
            if !m.is_file() {
                return Err(RsError::Invalid);
            }
            if name.starts_with(TEMP_PREFIX) {
                temps.push(name);
                continue;
            }
            let size = m.len() as i64;
            if self.files.len() as i64 >= self.options.max_records
                || size < 1
                || size > self.options.max_record_bytes
                || size > self.options.max_total_bytes - self.bytes
            {
                return Err(RsError::Limit);
            }
            self.files.insert(name, stamp(&m));
            self.bytes += size;
        }
        for name in &temps {
            retry_sharing(|| fs::remove_file(self.dir.join(name)))
                .map_err(|_| RsError::Unavailable)?;
        }
        if !temps.is_empty() {
            sync_directory(&self.dir).map_err(|_| RsError::Unavailable)?;
        }
        self.usable(ctx)
    }
    fn keys(&self, ctx: &Ctx) -> RsResult<Vec<String>> {
        self.usable(ctx)?;
        let mut keys: Vec<String> = self.files.keys().cloned().collect();
        keys.sort();
        Ok(keys)
    }
    fn read_inner(&self, ctx: &Ctx, key: &str) -> RsResult<Vec<u8>> {
        if !valid_key(key) {
            return Err(RsError::Invalid);
        }
        let Some(expected) = self.files.get(key) else {
            return Err(RsError::NotFound);
        };
        let path = self.dir.join(key);
        let before = nofollow_metadata(&path).map_err(|_| RsError::Unavailable)?;
        let mut f = File::open(&path).map_err(|_| RsError::Unavailable)?;
        let i = f.metadata().map_err(|_| RsError::Unavailable)?;
        if !before.is_file() || !i.is_file() || stamp(&i) != *expected {
            return Err(RsError::Unavailable);
        }
        rs_context(ctx)?;
        let mut data = Vec::new();
        (&mut f)
            .take((self.options.max_record_bytes + 1) as u64)
            .read_to_end(&mut data)
            .map_err(|_| RsError::Unavailable)?;
        let after = f.metadata().map_err(|_| RsError::Unavailable)?;
        if data.len() as u64 != i.len() || stamp(&after) != stamp(&i) {
            return Err(RsError::Unavailable);
        }
        Ok(data)
    }
    fn read(&self, ctx: &Ctx, key: &str) -> RsResult<Vec<u8>> {
        self.usable(ctx)?;
        self.read_inner(ctx, key)
    }
    fn create(&mut self, ctx: &Ctx, key: &str, data: &[u8]) -> RsResult<bool> {
        self.usable(ctx)?;
        if !valid_key(key) {
            return Err(RsError::Invalid);
        }
        if self.files.contains_key(key) {
            self.read_inner(ctx, key)?;
            return Ok(false);
        }
        if self.files.len() as i64 >= self.options.max_records {
            return Err(RsError::Limit);
        }
        self.write(ctx, key, data, false)?;
        Ok(true)
    }
    fn replace(&mut self, ctx: &Ctx, key: &str, data: &[u8]) -> RsResult<()> {
        self.usable(ctx)?;
        self.read_inner(ctx, key)?;
        self.write(ctx, key, data, true)
    }
    fn write(&mut self, ctx: &Ctx, key: &str, data: &[u8], replace: bool) -> RsResult<()> {
        let old = self.files.get(key).map_or(0, |s| s.len as i64);
        let n = data.len() as i64;
        if n < 1
            || n > self.options.max_record_bytes
            || n > self.options.max_total_bytes - (self.bytes - old)
        {
            return Err(RsError::Limit);
        }
        let temp = format!(
            "{TEMP_PREFIX}{:032x}",
            uuid::Uuid::new_v4().as_u128() ^ uuid::Uuid::new_v4().as_u128().rotate_left(64)
        );
        let temp_path = self.dir.join(&temp);
        let key_path = self.dir.join(key);
        let mut published = false;
        let result = (|| -> RsResult<()> {
            let mut f = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp_path)
                .map_err(|_| RsError::Unavailable)?;
            rs_context(ctx)?;
            f.write_all(data).map_err(|_| RsError::Unavailable)?;
            f.sync_all().map_err(|_| RsError::Unavailable)?;
            drop(f);
            self.usable(ctx)?;
            let publish = if replace {
                retry_sharing(|| fs::rename(&temp_path, &key_path))
            } else {
                retry_sharing(|| fs::hard_link(&temp_path, &key_path)).and_then(|_| {
                    published = true;
                    retry_sharing(|| fs::remove_file(&temp_path))
                })
            };
            if replace && publish.is_ok() {
                published = true;
            }
            if publish.is_err() {
                if published {
                    self.unhealthy = true;
                }
                return Err(RsError::Unavailable);
            }
            if sync_directory(&self.dir).is_err() {
                self.unhealthy = true;
                return Err(RsError::Unavailable);
            }
            if let Err(e) = self.usable(ctx) {
                self.unhealthy = true;
                return Err(e);
            }
            let m = match nofollow_metadata(&key_path) {
                Ok(m) if m.is_file() && m.len() as i64 == n => m,
                _ => {
                    self.unhealthy = true;
                    return Err(RsError::Unavailable);
                }
            };
            self.files.insert(key.to_string(), stamp(&m));
            self.bytes = self.bytes - old + n;
            Ok(())
        })();
        if !published {
            let _ = fs::remove_file(&temp_path);
        }
        result
    }
    fn delete(&mut self, ctx: &Ctx, key: &str) -> RsResult<()> {
        self.usable(ctx)?;
        self.read_inner(ctx, key)?;
        retry_sharing(|| fs::remove_file(self.dir.join(key))).map_err(|_| RsError::Unavailable)?;
        if sync_directory(&self.dir).is_err() {
            self.unhealthy = true;
            return Err(RsError::Unavailable);
        }
        if let Some(s) = self.files.remove(key) {
            self.bytes -= s.len as i64;
        }
        Ok(())
    }
    fn close(&mut self) {
        if self.closed {
            return;
        }
        self.closed = true;
        if let Some(lock) = self.lock.take() {
            let _ = fs4::fs_std::FileExt::unlock(&lock);
        }
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FileStorageCommitmentStoreOptions {
    pub max_transactions: i64,
    pub max_record_bytes: i64,
    pub max_total_bytes: i64,
    pub max_references: i64,
    pub max_list_batch: i64,
}
fn normalize_file_options(
    mut o: FileStorageCommitmentStoreOptions,
) -> Result<FileStorageCommitmentStoreOptions> {
    if o.max_transactions < 0
        || o.max_record_bytes < 0
        || o.max_total_bytes < 0
        || o.max_references < 0
        || o.max_list_batch < 0
    {
        return Err(resource_limit());
    }
    if o.max_transactions == 0 {
        o.max_transactions = 10_000;
    }
    if o.max_record_bytes == 0 {
        o.max_record_bytes = 4 << 20;
    }
    if o.max_total_bytes == 0 {
        o.max_total_bytes = 64 << 20;
    }
    if o.max_references == 0 {
        o.max_references = DEFAULT_STORAGE_COMMITMENT_MAX_REFERENCES as i64;
    }
    if o.max_list_batch == 0 {
        o.max_list_batch = 128;
    }
    if o.max_transactions > 10_000
        || o.max_record_bytes > 16 << 20
        || o.max_total_bytes > 1 << 30
        || o.max_total_bytes < o.max_record_bytes
        || o.max_references > DEFAULT_STORAGE_COMMITMENT_MAX_REFERENCES as i64
        || o.max_list_batch > 1024
    {
        return Err(resource_limit());
    }
    Ok(o)
}
fn file_store_error(err: RsError) -> Error {
    match err {
        RsError::NotFound => not_found(),
        RsError::Limit => resource_limit(),
        RsError::Context(e) => e,
        RsError::Unavailable => sanitized(
            STORAGE_COMMITMENT_FAILURE_PROCESSING,
            Error::new("dicom record store: unavailable or inconsistent"),
        ),
        RsError::Invalid => sanitized(
            STORAGE_COMMITMENT_FAILURE_PROCESSING,
            Error::new("dicom record store: invalid argument or record"),
        ),
    }
}
fn file_invalid() -> Error {
    Error::sentinel(Kind::StorageCommitmentFileStoreInvalidRecord)
}
fn commitment_file_key(uid: &str) -> String {
    let digest: [u8; 32] = Sha256::digest(uid.as_bytes()).into();
    let mut s = String::with_capacity(69);
    for b in digest {
        s.push_str(&format!("{b:02x}"));
    }
    s.push_str(".json");
    s
}
struct FileState {
    records: RecordStore,
    closed: bool,
}
pub struct FileStorageCommitmentStore {
    state: Mutex<FileState>,
    options: FileStorageCommitmentStoreOptions,
}
impl std::fmt::Debug for FileStorageCommitmentStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("FileStorageCommitmentStore")
    }
}
impl FileStorageCommitmentStore {
    pub fn new(
        directory: impl AsRef<Path>,
        options: FileStorageCommitmentStoreOptions,
    ) -> Result<FileStorageCommitmentStore> {
        let options = normalize_file_options(options)?;
        let dir = directory.as_ref();
        if nofollow_directory(dir).is_err() {
            return Err(sanitized(
                STORAGE_COMMITMENT_FAILURE_PROCESSING,
                Error::new("nofollow: open directory"),
            ));
        }
        let ctx = Ctx::background();
        let records = RecordStore::open(
            &ctx,
            dir,
            RsOptions {
                max_records: options.max_transactions,
                max_record_bytes: options.max_record_bytes,
                max_total_bytes: options.max_total_bytes,
            },
        )
        .map_err(file_store_error)?;
        let store = FileStorageCommitmentStore {
            state: Mutex::new(FileState {
                records,
                closed: false,
            }),
            options,
        };
        {
            let mut st = store.state.lock().unwrap();
            if let Err(e) = store.scan(&ctx, &st, |_| Ok(())) {
                st.records.close();
                return Err(e);
            }
        }
        Ok(store)
    }
    pub fn close(&self) -> Result<()> {
        let mut st = self.state.lock().unwrap();
        if st.closed {
            return Ok(());
        }
        st.closed = true;
        st.records.close();
        Ok(())
    }
    fn check(&self, ctx: &Ctx, st: &FileState) -> Result<()> {
        if let Some(e) = context_err(ctx) {
            return Err(e);
        }
        if st.closed {
            return Err(Error::sentinel(Kind::StorageCommitmentFileStoreClosed));
        }
        Ok(())
    }
    fn read(&self, ctx: &Ctx, st: &FileState, key: &str) -> Result<StorageCommitmentTransaction> {
        let data = st.records.read(ctx, key).map_err(file_store_error)?;
        self.decode(key, &data)
    }
    fn scan(
        &self,
        ctx: &Ctx,
        st: &FileState,
        mut visit: impl FnMut(StorageCommitmentTransaction) -> Result<()>,
    ) -> Result<()> {
        let keys = st.records.keys(ctx).map_err(file_store_error)?;
        if keys.len() as i64 > self.options.max_transactions {
            return Err(resource_limit());
        }
        for key in keys {
            if let Some(e) = context_err(ctx) {
                return Err(e);
            }
            let t = self.read(ctx, st, &key)?;
            visit(t)?;
        }
        Ok(())
    }
    fn validate(&self, t: &StorageCommitmentTransaction) -> Result<()> {
        let bad = || Err(invalid_request());
        if !file_valid_uid(&t.transaction_uid)
            || t.version == 0
            || (t.direction != STORAGE_COMMITMENT_DIRECTION_REQUESTOR
                && t.direction != STORAGE_COMMITMENT_DIRECTION_COMMITTER)
            || !file_valid_state(&t.state)
            || !valid_storage_commitment_delivery_mode(&t.delivery_mode)
            || !file_valid_failure(&t.last_failure)
            || t.processing_tries < 0
            || t.delivery_attempts < 0
            || ul::validate_ae_title(&t.requestor_ae_title).is_err()
            || ul::validate_ae_title(&t.committer_ae_title).is_err()
            || t.requestor_ae_title != ul::normalize_ae_title(&t.requestor_ae_title)
            || t.committer_ae_title != ul::normalize_ae_title(&t.committer_ae_title)
            || t.created_at.is_zero()
            || t.updated_at.is_zero()
        {
            return bad();
        }
        for ts in [
            t.created_at,
            t.updated_at,
            t.completed_at,
            t.next_attempt_at,
            t.lease_until,
        ] {
            if !file_valid_time(ts) {
                return bad();
            }
        }
        if t.lease_token.len() > 32 {
            return bad();
        }
        if !t.lease_token.is_empty()
            && (t.lease_token.len() != 32
                || !t
                    .lease_token
                    .bytes()
                    .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)))
        {
            return bad();
        }
        self.validate_references(&t.referenced_sops, false)?;
        if t.request_digest != storage_commitment_request_digest(t) {
            return bad();
        }
        let Some(r) = &t.result else {
            if t.result_digest != [0; 32] {
                return Err(invalid_result());
            }
            if matches!(
                t.state.as_str(),
                STORAGE_COMMITMENT_STATE_RESULT_READY
                    | STORAGE_COMMITMENT_STATE_DELIVERING
                    | STORAGE_COMMITMENT_STATE_DELIVERY_FAILED
                    | STORAGE_COMMITMENT_STATE_DELIVERY_EXHAUSTED
                    | STORAGE_COMMITMENT_STATE_DELIVERED
                    | STORAGE_COMMITMENT_STATE_RESULT_RECEIVED
            ) {
                return Err(invalid_result());
            }
            return Ok(());
        };
        let max = self.options.max_references;
        if !file_valid_time(r.produced_at)
            || r.produced_at.is_zero()
            || r.transaction_uid != t.transaction_uid
            || r.referenced_sops.len() as i64 > max
            || r.failed_sops.len() as i64 > max
            || r.referenced_sops.len() + r.failed_sops.len() != t.referenced_sops.len()
        {
            return Err(invalid_result());
        }
        if !r.referenced_sops.is_empty() {
            self.validate_references(&r.referenced_sops, false)?;
        }
        if !r.failed_sops.is_empty() {
            self.validate_references(&r.failed_sops, true)?;
        }
        match validate_storage_commitment_result(max, || r.produced_at, t, r) {
            Ok(n) if t.result_digest == storage_commitment_result_digest(&n) => Ok(()),
            _ => Err(invalid_result()),
        }
    }
    fn validate_references(
        &self,
        refs: &[StorageCommitmentSopReference],
        failed: bool,
    ) -> Result<()> {
        if refs.is_empty() || refs.len() as i64 > self.options.max_references {
            return Err(resource_limit());
        }
        for r in refs {
            if !file_valid_uid(&r.sop_class_uid) || !file_valid_uid(&r.sop_instance_uid) {
                return Err(invalid_request());
            }
        }
        let normalized = normalize_storage_commitment_references(
            refs,
            failed,
            self.options.max_references as usize,
        )?;
        if normalized.as_slice() != refs {
            return Err(invalid_request());
        }
        Ok(())
    }
    fn encode(&self, t: &StorageCommitmentTransaction) -> Result<Vec<u8>> {
        self.validate(t)?;
        let data = encode_record(t);
        if data.len() as i64 > self.options.max_record_bytes {
            return Err(resource_limit());
        }
        Ok(data.into_bytes())
    }
    fn decode(&self, key: &str, data: &[u8]) -> Result<StorageCommitmentTransaction> {
        if data.len() as i64 > self.options.max_record_bytes {
            return Err(file_invalid());
        }
        let Ok(text) = std::str::from_utf8(data) else {
            return Err(file_invalid());
        };
        let Some(value) = json::parse(text) else {
            return Err(file_invalid());
        };
        let mut tokens = 0;
        if !strict_value(&value, "envelope", 0, &mut tokens) {
            return Err(file_invalid());
        }
        let Some(t) = decode_envelope(&value) else {
            return Err(file_invalid());
        };
        if self.validate(&t).is_err() || key != commitment_file_key(&t.transaction_uid) {
            return Err(file_invalid());
        }
        Ok(t)
    }
}
impl StorageCommitmentTransactionStore for FileStorageCommitmentStore {
    fn create(&self, ctx: &Ctx, transaction: StorageCommitmentTransaction) -> Result<bool> {
        let mut st = self.state.lock().unwrap();
        self.check(ctx, &st)?;
        let mut t = transaction;
        t.version = 1;
        let data = self.encode(&t)?;
        st.records
            .create(ctx, &commitment_file_key(&t.transaction_uid), &data)
            .map_err(file_store_error)
    }
    fn get(&self, ctx: &Ctx, uid: &str) -> Result<StorageCommitmentTransaction> {
        let st = self.state.lock().unwrap();
        self.check(ctx, &st)?;
        if !file_valid_uid(uid) {
            return Err(invalid_request());
        }
        self.read(ctx, &st, &commitment_file_key(uid))
    }
    fn compare_and_swap(
        &self,
        ctx: &Ctx,
        uid: &str,
        expected_version: u64,
        next: StorageCommitmentTransaction,
    ) -> Result<StorageCommitmentTransaction> {
        let mut st = self.state.lock().unwrap();
        self.check(ctx, &st)?;
        if !file_valid_uid(uid) {
            return Err(invalid_request());
        }
        let key = commitment_file_key(uid);
        let current = self.read(ctx, &st, &key)?;
        if current.version != expected_version {
            return Err(concurrent_update());
        }
        if expected_version == u64::MAX {
            return Err(resource_limit());
        }
        if next.transaction_uid != uid
            || next.direction != current.direction
            || next.request_digest != current.request_digest
            || next.created_at != current.created_at
            || (current.result.is_some()
                && (next.result.is_none() || next.result_digest != current.result_digest))
        {
            return Err(conflict());
        }
        let mut next = next;
        next.version = expected_version + 1;
        let data = self.encode(&next)?;
        st.records
            .replace(ctx, &key, &data)
            .map_err(file_store_error)?;
        Ok(next)
    }
    fn list(
        &self,
        ctx: &Ctx,
        query: &StorageCommitmentTransactionQuery,
    ) -> Result<Vec<StorageCommitmentTransaction>> {
        let st = self.state.lock().unwrap();
        self.check(ctx, &st)?;
        if query.limit <= 0 || query.limit > self.options.max_list_batch || query.states.len() > 11
        {
            return Err(resource_limit());
        }
        if !file_valid_time(query.due_at) {
            return Err(invalid_request());
        }
        for s in &query.states {
            if !file_valid_state(s) {
                return Err(invalid_request());
            }
        }
        let limit = query.limit as usize;
        let mut items: Vec<StorageCommitmentTransaction> = Vec::new();
        self.scan(ctx, &st, |t| {
            if (!query.states.is_empty() && !query.states.contains(&t.state))
                || (!query.due_at.is_zero() && !transaction_due(&t, query.due_at))
            {
                return Ok(());
            }
            let pos = items.partition_point(|i| {
                (i.updated_at, i.transaction_uid.as_str())
                    < (t.updated_at, t.transaction_uid.as_str())
            });
            if pos >= limit {
                return Ok(());
            }
            items.insert(pos, t);
            items.truncate(limit);
            Ok(())
        })?;
        Ok(items)
    }
    fn purge_completed(&self, ctx: &Ctx, before: StorageCommitmentTime, limit: i64) -> Result<i64> {
        let mut st = self.state.lock().unwrap();
        self.check(ctx, &st)?;
        if limit <= 0 || limit > self.options.max_list_batch {
            return Err(resource_limit());
        }
        if !file_valid_time(before) {
            return Err(invalid_request());
        }
        let limit = limit as usize;
        let mut uids: Vec<String> = Vec::new();
        self.scan(ctx, &st, |t| {
            if !storage_commitment_state_terminal(&t.state) || !t.updated_at.before(before) {
                return Ok(());
            }
            let pos = uids.partition_point(|u| u.as_str() < t.transaction_uid.as_str());
            if pos >= limit {
                return Ok(());
            }
            uids.insert(pos, t.transaction_uid);
            uids.truncate(limit);
            Ok(())
        })?;
        let mut deleted = 0;
        for uid in uids {
            st.records
                .delete(ctx, &commitment_file_key(&uid))
                .map_err(file_store_error)?;
            deleted += 1;
        }
        Ok(deleted)
    }
}
impl Drop for FileStorageCommitmentStore {
    fn drop(&mut self) {
        let _ = self.close();
    }
}
fn file_valid_uid(uid: &str) -> bool {
    uid.len() <= 64 && valid_uid(uid)
}
fn year_of(t: StorageCommitmentTime) -> Option<i64> {
    let days = t.nanos_since_zero().div_euclid(86_400 * 1_000_000_000);
    let date = chrono::NaiveDate::from_num_days_from_ce_opt(i32::try_from(days + 1).ok()?)?;
    Some(chrono::Datelike::year(&date) as i64)
}
fn file_valid_time(t: StorageCommitmentTime) -> bool {
    t.is_zero() || year_of(t).is_some_and(|y| (1..=9999).contains(&y))
}
const STATES: &[&str] = &[
    STORAGE_COMMITMENT_STATE_REQUEST_PENDING,
    STORAGE_COMMITMENT_STATE_ACCEPTANCE_PREPARED,
    STORAGE_COMMITMENT_STATE_ACCEPTED,
    STORAGE_COMMITMENT_STATE_PROCESSING,
    STORAGE_COMMITMENT_STATE_REQUEST_REJECTED,
    STORAGE_COMMITMENT_STATE_RESULT_READY,
    STORAGE_COMMITMENT_STATE_DELIVERING,
    STORAGE_COMMITMENT_STATE_DELIVERY_FAILED,
    STORAGE_COMMITMENT_STATE_DELIVERY_EXHAUSTED,
    STORAGE_COMMITMENT_STATE_DELIVERED,
    STORAGE_COMMITMENT_STATE_RESULT_RECEIVED,
];
fn file_valid_state(s: &str) -> bool {
    STATES.contains(&s)
}
fn file_valid_failure(f: &str) -> bool {
    matches!(
        f,
        STORAGE_COMMITMENT_FAILURE_NONE
            | STORAGE_COMMITMENT_FAILURE_PROCESSING
            | STORAGE_COMMITMENT_FAILURE_CALLBACK_UNKNOWN
            | STORAGE_COMMITMENT_FAILURE_CALLBACK_OFFLINE
            | STORAGE_COMMITMENT_FAILURE_TLS
            | STORAGE_COMMITMENT_FAILURE_TIMEOUT
            | STORAGE_COMMITMENT_FAILURE_CANCELED
            | STORAGE_COMMITMENT_FAILURE_ASSOCIATION_REJECTED
            | STORAGE_COMMITMENT_FAILURE_DIMSE_STATUS
            | STORAGE_COMMITMENT_FAILURE_PROTOCOL
    )
}
fn json_string(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '<' | '>' | '&' | '\u{2028}' | '\u{2029}' => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}
fn json_time(out: &mut String, t: StorageCommitmentTime) {
    out.push('"');
    out.push_str(&format_rfc3339_nano(t));
    out.push('"');
}
fn format_rfc3339_nano(t: StorageCommitmentTime) -> String {
    let total = t.nanos_since_zero();
    let day_nanos = 86_400i128 * 1_000_000_000;
    let days = total.div_euclid(day_nanos);
    let rem = total.rem_euclid(day_nanos);
    let date = i32::try_from(days + 1)
        .ok()
        .and_then(chrono::NaiveDate::from_num_days_from_ce_opt)
        .unwrap_or_default();
    let secs = (rem / 1_000_000_000) as i64;
    let nanos = (rem % 1_000_000_000) as u32;
    let mut s = format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
        chrono::Datelike::year(&date),
        chrono::Datelike::month(&date),
        chrono::Datelike::day(&date),
        secs / 3600,
        (secs / 60) % 60,
        secs % 60
    );
    if nanos != 0 {
        let frac = format!("{nanos:09}");
        s.push('.');
        s.push_str(frac.trim_end_matches('0'));
    }
    s.push('Z');
    s
}
fn json_digest(out: &mut String, d: &[u8; 32]) {
    out.push('[');
    for (i, b) in d.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&b.to_string());
    }
    out.push(']');
}
fn json_refs(out: &mut String, refs: &[StorageCommitmentSopReference]) {
    if refs.is_empty() {
        out.push_str("null");
        return;
    }
    out.push('[');
    for (i, r) in refs.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str("{\"SOPClassUID\":");
        json_string(out, &r.sop_class_uid);
        out.push_str(",\"SOPInstanceUID\":");
        json_string(out, &r.sop_instance_uid);
        out.push_str(&format!(",\"FailureReason\":{}}}", r.failure_reason));
    }
    out.push(']');
}
fn encode_record(t: &StorageCommitmentTransaction) -> String {
    let mut o = String::with_capacity(1024);
    o.push_str("{\"schema_version\":1,\"transaction\":{\"TransactionUID\":");
    json_string(&mut o, &t.transaction_uid);
    o.push_str(",\"Direction\":");
    json_string(&mut o, &t.direction);
    o.push_str(",\"State\":");
    json_string(&mut o, &t.state);
    o.push_str(",\"DeliveryMode\":");
    json_string(&mut o, &t.delivery_mode);
    o.push_str(",\"RequestorAETitle\":");
    json_string(&mut o, &t.requestor_ae_title);
    o.push_str(",\"CommitterAETitle\":");
    json_string(&mut o, &t.committer_ae_title);
    o.push_str(",\"ReferencedSOPs\":");
    json_refs(&mut o, &t.referenced_sops);
    o.push_str(&format!(",\"RequestMessageID\":{}", t.request_message_id));
    o.push_str(",\"RequestDigest\":");
    json_digest(&mut o, &t.request_digest);
    o.push_str(",\"ResultDigest\":");
    json_digest(&mut o, &t.result_digest);
    o.push_str(",\"Result\":");
    match &t.result {
        None => o.push_str("null"),
        Some(r) => {
            o.push_str("{\"TransactionUID\":");
            json_string(&mut o, &r.transaction_uid);
            o.push_str(",\"ReferencedSOPs\":");
            json_refs(&mut o, &r.referenced_sops);
            o.push_str(",\"FailedSOPs\":");
            json_refs(&mut o, &r.failed_sops);
            o.push_str(",\"ProducedAt\":");
            json_time(&mut o, r.produced_at);
            o.push('}');
        }
    }
    o.push_str(&format!(",\"Version\":{}", t.version));
    o.push_str(",\"CreatedAt\":");
    json_time(&mut o, t.created_at);
    o.push_str(",\"UpdatedAt\":");
    json_time(&mut o, t.updated_at);
    o.push_str(",\"CompletedAt\":");
    json_time(&mut o, t.completed_at);
    o.push_str(&format!(
        ",\"ProcessingTries\":{},\"DeliveryAttempts\":{}",
        t.processing_tries, t.delivery_attempts
    ));
    o.push_str(",\"NextAttemptAt\":");
    json_time(&mut o, t.next_attempt_at);
    o.push_str(",\"LeaseToken\":");
    json_string(&mut o, &t.lease_token);
    o.push_str(",\"LeaseUntil\":");
    json_time(&mut o, t.lease_until);
    o.push_str(",\"LastFailure\":");
    json_string(&mut o, &t.last_failure);
    o.push_str(&format!(",\"LastDIMSEStatus\":{}}}}}", t.last_dimse_status));
    o
}
mod json {
    #[derive(Debug, Clone, PartialEq)]
    pub enum Value {
        Null,
        Bool(bool),
        Number(String),
        String(String),
        Array(Vec<Value>),
        Object(Vec<(String, Value)>),
    }
    pub fn parse(text: &str) -> Option<Value> {
        let mut p = Parser {
            s: text.as_bytes(),
            i: 0,
            depth: 0,
        };
        p.ws();
        let v = p.value()?;
        p.ws();
        if p.i != p.s.len() {
            return None;
        }
        Some(v)
    }
    struct Parser<'a> {
        s: &'a [u8],
        i: usize,
        depth: usize,
    }
    impl Parser<'_> {
        fn ws(&mut self) {
            while self.i < self.s.len() && matches!(self.s[self.i], b' ' | b'\t' | b'\n' | b'\r') {
                self.i += 1;
            }
        }
        fn peek(&self) -> Option<u8> {
            self.s.get(self.i).copied()
        }
        fn lit(&mut self, word: &[u8], v: Value) -> Option<Value> {
            if self.s[self.i..].starts_with(word) {
                self.i += word.len();
                Some(v)
            } else {
                None
            }
        }
        fn value(&mut self) -> Option<Value> {
            self.depth += 1;
            if self.depth > 10_000 {
                return None;
            }
            let v = match self.peek()? {
                b'{' => self.object(),
                b'[' => self.array(),
                b'"' => self.string().map(Value::String),
                b'n' => self.lit(b"null", Value::Null),
                b't' => self.lit(b"true", Value::Bool(true)),
                b'f' => self.lit(b"false", Value::Bool(false)),
                b'-' | b'0'..=b'9' => self.number(),
                _ => None,
            };
            self.depth -= 1;
            v
        }
        fn object(&mut self) -> Option<Value> {
            self.i += 1;
            let mut fields = Vec::new();
            self.ws();
            if self.peek()? == b'}' {
                self.i += 1;
                return Some(Value::Object(fields));
            }
            loop {
                self.ws();
                if self.peek()? != b'"' {
                    return None;
                }
                let key = self.string()?;
                self.ws();
                if self.peek()? != b':' {
                    return None;
                }
                self.i += 1;
                self.ws();
                let v = self.value()?;
                fields.push((key, v));
                self.ws();
                match self.peek()? {
                    b',' => self.i += 1,
                    b'}' => {
                        self.i += 1;
                        return Some(Value::Object(fields));
                    }
                    _ => return None,
                }
            }
        }
        fn array(&mut self) -> Option<Value> {
            self.i += 1;
            let mut items = Vec::new();
            self.ws();
            if self.peek()? == b']' {
                self.i += 1;
                return Some(Value::Array(items));
            }
            loop {
                self.ws();
                items.push(self.value()?);
                self.ws();
                match self.peek()? {
                    b',' => self.i += 1,
                    b']' => {
                        self.i += 1;
                        return Some(Value::Array(items));
                    }
                    _ => return None,
                }
            }
        }
        fn number(&mut self) -> Option<Value> {
            let start = self.i;
            if self.peek() == Some(b'-') {
                self.i += 1;
            }
            match self.peek()? {
                b'0' => self.i += 1,
                b'1'..=b'9' => {
                    while matches!(self.peek(), Some(b'0'..=b'9')) {
                        self.i += 1;
                    }
                }
                _ => return None,
            }
            if self.peek() == Some(b'.') {
                self.i += 1;
                if !matches!(self.peek(), Some(b'0'..=b'9')) {
                    return None;
                }
                while matches!(self.peek(), Some(b'0'..=b'9')) {
                    self.i += 1;
                }
            }
            if matches!(self.peek(), Some(b'e' | b'E')) {
                self.i += 1;
                if matches!(self.peek(), Some(b'+' | b'-')) {
                    self.i += 1;
                }
                if !matches!(self.peek(), Some(b'0'..=b'9')) {
                    return None;
                }
                while matches!(self.peek(), Some(b'0'..=b'9')) {
                    self.i += 1;
                }
            }
            Some(Value::Number(
                String::from_utf8_lossy(&self.s[start..self.i]).into_owned(),
            ))
        }
        fn hex4(&mut self) -> Option<u32> {
            let h = self.s.get(self.i..self.i + 4)?;
            let v = u32::from_str_radix(std::str::from_utf8(h).ok()?, 16).ok()?;
            self.i += 4;
            Some(v)
        }
        fn string(&mut self) -> Option<String> {
            self.i += 1;
            let mut out: Vec<u8> = Vec::new();
            loop {
                let c = self.peek()?;
                self.i += 1;
                match c {
                    b'"' => return String::from_utf8(out).ok(),
                    b'\\' => {
                        let e = self.peek()?;
                        self.i += 1;
                        let ch = match e {
                            b'"' => '"',
                            b'\\' => '\\',
                            b'/' => '/',
                            b'b' => '\u{8}',
                            b'f' => '\u{c}',
                            b'n' => '\n',
                            b'r' => '\r',
                            b't' => '\t',
                            b'u' => {
                                let hi = self.hex4()?;
                                if (0xD800..0xDC00).contains(&hi)
                                    && self.s.get(self.i..self.i + 2) == Some(b"\\u")
                                {
                                    let save = self.i;
                                    self.i += 2;
                                    let lo = self.hex4()?;
                                    if (0xDC00..0xE000).contains(&lo) {
                                        char::from_u32(
                                            0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00),
                                        )
                                        .unwrap_or('\u{FFFD}')
                                    } else {
                                        self.i = save;
                                        '\u{FFFD}'
                                    }
                                } else {
                                    char::from_u32(hi).unwrap_or('\u{FFFD}')
                                }
                            }
                            _ => return None,
                        };
                        let mut buf = [0u8; 4];
                        out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                    }
                    c if c < 0x20 => return None,
                    c => out.push(c),
                }
            }
        }
    }
}
use json::Value;
fn fields_of(kind: &str) -> Option<&'static [(&'static str, &'static str)]> {
    const ENVELOPE: &[(&str, &str)] =
        &[("schema_version", "scalar"), ("transaction", "transaction")];
    const TRANSACTION: &[(&str, &str)] = &[
        ("TransactionUID", "scalar"),
        ("Direction", "scalar"),
        ("State", "scalar"),
        ("DeliveryMode", "scalar"),
        ("RequestorAETitle", "scalar"),
        ("CommitterAETitle", "scalar"),
        ("ReferencedSOPs", "references"),
        ("RequestMessageID", "scalar"),
        ("RequestDigest", "digest"),
        ("ResultDigest", "digest"),
        ("Result", "result"),
        ("Version", "scalar"),
        ("CreatedAt", "scalar"),
        ("UpdatedAt", "scalar"),
        ("CompletedAt", "scalar"),
        ("ProcessingTries", "scalar"),
        ("DeliveryAttempts", "scalar"),
        ("NextAttemptAt", "scalar"),
        ("LeaseToken", "scalar"),
        ("LeaseUntil", "scalar"),
        ("LastFailure", "scalar"),
        ("LastDIMSEStatus", "scalar"),
    ];
    const RESULT: &[(&str, &str)] = &[
        ("TransactionUID", "scalar"),
        ("ReferencedSOPs", "references"),
        ("FailedSOPs", "references"),
        ("ProducedAt", "scalar"),
    ];
    const REFERENCE: &[(&str, &str)] = &[
        ("SOPClassUID", "scalar"),
        ("SOPInstanceUID", "scalar"),
        ("FailureReason", "scalar"),
    ];
    match kind {
        "envelope" => Some(ENVELOPE),
        "transaction" => Some(TRANSACTION),
        "result" => Some(RESULT),
        "reference" => Some(REFERENCE),
        _ => None,
    }
}
fn strict_value(v: &Value, kind: &str, depth: usize, tokens: &mut usize) -> bool {
    if depth > 12 || *tokens >= 400_000 {
        return false;
    }
    *tokens += 1;
    match v {
        Value::Array(items) => {
            if kind != "references" && kind != "digest" {
                return false;
            }
            let child = if kind == "digest" {
                "scalar"
            } else {
                "reference"
            };
            let mut count = 0;
            for item in items {
                count += 1;
                if count > 10_000
                    || (kind == "digest" && count > 32)
                    || !strict_value(item, child, depth + 1, tokens)
                {
                    return false;
                }
            }
            kind != "digest" || count == 32
        }
        Value::Object(fields) => {
            let Some(known) = fields_of(kind) else {
                return false;
            };
            let mut seen: Vec<&str> = Vec::new();
            for (key, value) in fields {
                let Some((_, child)) = known.iter().find(|(k, _)| k == key) else {
                    return false;
                };
                if seen.contains(&key.as_str()) {
                    return false;
                }
                seen.push(key);
                if !strict_value(value, child, depth + 1, tokens) {
                    return false;
                }
            }
            kind != "envelope"
                || (seen.contains(&"schema_version") && seen.contains(&"transaction"))
        }
        Value::Null => kind == "result" || kind == "references",
        _ => kind == "scalar",
    }
}
fn object_map(v: &Value) -> Option<BTreeMap<&str, &Value>> {
    match v {
        Value::Object(fields) => Some(fields.iter().map(|(k, v)| (k.as_str(), v)).collect()),
        _ => None,
    }
}
fn json_unsigned(v: &Value, max: u64) -> Option<u64> {
    let Value::Number(n) = v else { return None };
    if n.starts_with('-') || n.contains(['.', 'e', 'E']) {
        return None;
    }
    let x: u64 = n.parse().ok()?;
    (x <= max).then_some(x)
}
fn json_int(v: &Value) -> Option<i64> {
    let Value::Number(n) = v else { return None };
    if n.contains(['.', 'e', 'E']) {
        return None;
    }
    n.parse().ok()
}
fn json_str(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        _ => None,
    }
}
fn json_digest_value(v: &Value) -> Option<[u8; 32]> {
    let Value::Array(items) = v else { return None };
    let mut d = [0u8; 32];
    for (i, item) in items.iter().enumerate() {
        *d.get_mut(i)? = json_unsigned(item, 255)? as u8;
    }
    Some(d)
}
fn parse_rfc3339(s: &str) -> Option<StorageCommitmentTime> {
    let b = s.as_bytes();
    let num = |r: std::ops::Range<usize>| -> Option<i64> {
        let part = b.get(r)?;
        if !part.iter().all(u8::is_ascii_digit) {
            return None;
        }
        std::str::from_utf8(part).ok()?.parse().ok()
    };
    if b.len() < 20
        || b[4] != b'-'
        || b[7] != b'-'
        || b[10] != b'T'
        || b[13] != b':'
        || b[16] != b':'
    {
        return None;
    }
    let (year, month, day) = (num(0..4)?, num(5..7)?, num(8..10)?);
    let (hour, minute, second) = (num(11..13)?, num(14..16)?, num(17..19)?);
    if hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    let mut i = 19;
    let mut nanos: i64 = 0;
    if b.get(i) == Some(&b'.') {
        i += 1;
        let start = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        let digits = &s[start..i];
        if digits.is_empty() || digits.len() > 9 {
            return None;
        }
        nanos = format!("{digits:0<9}").parse().ok()?;
    }
    let offset_secs = match b.get(i)? {
        b'Z' if i + 1 == b.len() => 0,
        c @ (b'+' | b'-') if i + 6 == b.len() && b[i + 3] == b':' => {
            let (h, m) = (num(i + 1..i + 3)?, num(i + 4..i + 6)?);
            if h > 23 || m > 59 {
                return None;
            }
            let o = h * 3600 + m * 60;
            if *c == b'-' { -o } else { o }
        }
        _ => return None,
    };
    let date = chrono::NaiveDate::from_ymd_opt(year as i32, month as u32, day as u32)?;
    let days = chrono::Datelike::num_days_from_ce(&date) as i128 - 1;
    let secs = days * 86_400 + (hour * 3600 + minute * 60 + second - offset_secs) as i128;
    Some(StorageCommitmentTime::from_nanos_since_zero(
        secs * 1_000_000_000 + nanos as i128,
    ))
}
fn json_time_value(v: &Value) -> Option<StorageCommitmentTime> {
    match v {
        Value::String(s) => parse_rfc3339(s),
        _ => None,
    }
}
fn decode_refs(v: Option<&&Value>) -> Option<Vec<StorageCommitmentSopReference>> {
    let Some(v) = v else { return Some(Vec::new()) };
    match v {
        Value::Null => Some(Vec::new()),
        Value::Array(items) => items
            .iter()
            .map(|item| {
                let m = object_map(item)?;
                let mut r = StorageCommitmentSopReference::default();
                if let Some(x) = m.get("SOPClassUID") {
                    r.sop_class_uid = json_str(x)?;
                }
                if let Some(x) = m.get("SOPInstanceUID") {
                    r.sop_instance_uid = json_str(x)?;
                }
                if let Some(x) = m.get("FailureReason") {
                    r.failure_reason = json_unsigned(x, u16::MAX as u64)? as u16;
                }
                Some(r)
            })
            .collect(),
        _ => None,
    }
}
fn decode_envelope(v: &Value) -> Option<StorageCommitmentTransaction> {
    let env = object_map(v)?;
    if json_int(env.get("schema_version")?)? != 1 {
        return None;
    }
    let m = object_map(env.get("transaction")?)?;
    let mut t = StorageCommitmentTransaction::default();
    for (key, value) in &m {
        match *key {
            "TransactionUID" => t.transaction_uid = json_str(value)?,
            "Direction" => t.direction = json_str(value)?,
            "State" => t.state = json_str(value)?,
            "DeliveryMode" => t.delivery_mode = json_str(value)?,
            "RequestorAETitle" => t.requestor_ae_title = json_str(value)?,
            "CommitterAETitle" => t.committer_ae_title = json_str(value)?,
            "ReferencedSOPs" => t.referenced_sops = decode_refs(Some(value))?,
            "RequestMessageID" => {
                t.request_message_id = json_unsigned(value, u16::MAX as u64)? as u16
            }
            "RequestDigest" => t.request_digest = json_digest_value(value)?,
            "ResultDigest" => t.result_digest = json_digest_value(value)?,
            "Result" => {
                t.result = match value {
                    Value::Null => None,
                    r => {
                        let rm = object_map(r)?;
                        let mut res = StorageCommitmentResult::default();
                        if let Some(x) = rm.get("TransactionUID") {
                            res.transaction_uid = json_str(x)?;
                        }
                        res.referenced_sops = decode_refs(rm.get("ReferencedSOPs"))?;
                        res.failed_sops = decode_refs(rm.get("FailedSOPs"))?;
                        if let Some(x) = rm.get("ProducedAt") {
                            res.produced_at = json_time_value(x)?;
                        }
                        Some(res)
                    }
                }
            }
            "Version" => t.version = json_unsigned(value, u64::MAX)?,
            "CreatedAt" => t.created_at = json_time_value(value)?,
            "UpdatedAt" => t.updated_at = json_time_value(value)?,
            "CompletedAt" => t.completed_at = json_time_value(value)?,
            "ProcessingTries" => t.processing_tries = json_int(value)?,
            "DeliveryAttempts" => t.delivery_attempts = json_int(value)?,
            "NextAttemptAt" => t.next_attempt_at = json_time_value(value)?,
            "LeaseToken" => t.lease_token = json_str(value)?,
            "LeaseUntil" => t.lease_until = json_time_value(value)?,
            "LastFailure" => t.last_failure = json_str(value)?,
            "LastDIMSEStatus" => {
                t.last_dimse_status = json_unsigned(value, u16::MAX as u64)? as u16
            }
            _ => return None,
        }
    }
    Some(t)
}
