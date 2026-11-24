use super::{Error as ReadError, IndexResult, Options, Sentinel, read_path};
use std::fs::Metadata;
use std::path::{Path, PathBuf};
pub const MAX_SCAN_WORKERS: i64 = 64;
pub const MAX_SCAN_QUEUE_DEPTH: i64 = 4096;
const DEFAULT_SCAN_QUEUE_DEPTH: i64 = 32;
const DEFAULT_SCAN_MAX_FILES: i64 = 100_000;
const DEFAULT_SCAN_MAX_DIRECTORIES: i64 = 50_000;
const DEFAULT_SCAN_MAX_DEPTH: i64 = 64;
const DEFAULT_SCAN_MAX_PATH_BYTES: i64 = 4_096;
const DEFAULT_SCAN_MAX_ERRORS: i64 = 1_000;
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum SymlinkPolicy {
    #[default]
    Ignore,
    Reject,
    FollowFiles,
    Other(u8),
}
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScanErrorCode {
    InvalidOptions,
    Limit,
    Filesystem,
    Read,
    Symlink,
    Filter,
    Yield,
}
impl ScanErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            ScanErrorCode::InvalidOptions => "invalid_options",
            ScanErrorCode::Limit => "resource_limit",
            ScanErrorCode::Filesystem => "filesystem",
            ScanErrorCode::Read => "read",
            ScanErrorCode::Symlink => "symlink",
            ScanErrorCode::Filter => "filter_callback",
            ScanErrorCode::Yield => "yield_callback",
        }
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct ScanError {
    pub code: ScanErrorCode,
    pub limit: String,
    pub cause: Option<Box<ReadError>>,
}
impl std::fmt::Display for ScanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self.code {
            ScanErrorCode::InvalidOptions => "dicom index scan options are invalid",
            ScanErrorCode::Limit => "dicom index scan resource limit exceeded",
            ScanErrorCode::Filesystem => "dicom index scan filesystem operation failed",
            ScanErrorCode::Read => "dicom index scan metadata read failed",
            ScanErrorCode::Symlink => "dicom index scan rejected symbolic link",
            ScanErrorCode::Filter => "dicom index scan filter failed",
            ScanErrorCode::Yield => "dicom index scan result callback failed",
        })
    }
}
impl std::error::Error for ScanError {}
impl ScanError {
    fn new(code: ScanErrorCode) -> ScanError {
        ScanError {
            code,
            limit: String::new(),
            cause: None,
        }
    }
    fn limit(limit: &str) -> ScanError {
        ScanError {
            code: ScanErrorCode::Limit,
            limit: limit.to_string(),
            cause: None,
        }
    }
}
pub type ScanFilter<'a> = &'a dyn Fn(&str, &Metadata) -> bool;
#[derive(Clone, Debug, Default)]
pub struct ScanOptions {
    pub read_options: Options,
    pub workers: i64,
    pub queue_depth: i64,
    pub max_files: i64,
    pub max_directories: i64,
    pub max_depth: i64,
    pub max_path_bytes: i64,
    pub max_errors: i64,
    pub symlink_policy: SymlinkPolicy,
}
#[derive(Clone, Debug, Default)]
pub struct ScanResult {
    pub path: String,
    pub result: IndexResult,
    pub err: Option<ScanError>,
}
fn normalize_scan_options(mut o: ScanOptions) -> Result<ScanOptions, ScanError> {
    if o.workers < 0
        || o.queue_depth < 0
        || o.max_files < 0
        || o.max_directories < 0
        || o.max_depth < 0
        || o.max_path_bytes < 0
        || o.max_errors < 0
        || o.workers > MAX_SCAN_WORKERS
        || o.queue_depth > MAX_SCAN_QUEUE_DEPTH
        || matches!(o.symlink_policy, SymlinkPolicy::Other(_))
    {
        return Err(ScanError::new(ScanErrorCode::InvalidOptions));
    }
    let fill = |v: &mut i64, d: i64| {
        if *v == 0 {
            *v = d;
        }
    };
    fill(&mut o.workers, 1);
    fill(&mut o.queue_depth, DEFAULT_SCAN_QUEUE_DEPTH);
    fill(&mut o.max_files, DEFAULT_SCAN_MAX_FILES);
    fill(&mut o.max_directories, DEFAULT_SCAN_MAX_DIRECTORIES);
    fill(&mut o.max_depth, DEFAULT_SCAN_MAX_DEPTH);
    fill(&mut o.max_path_bytes, DEFAULT_SCAN_MAX_PATH_BYTES);
    fill(&mut o.max_errors, DEFAULT_SCAN_MAX_ERRORS);
    Ok(o)
}
struct Task {
    path: String,
    read_path: String,
    info: Metadata,
}
enum Item {
    Task(Task),
    Result(Box<ScanResult>),
}
fn exceeds(path: &str, max: i64) -> bool {
    path.len() as i64 > max
}
fn same_file(a: &Metadata, b: &Metadata) -> bool {
    same_file::is_same(a, b)
}
mod same_file {
    use std::fs::Metadata;
    #[cfg(windows)]
    pub fn is_same(a: &Metadata, b: &Metadata) -> bool {
        use std::os::windows::fs::MetadataExt;
        a.creation_time() == b.creation_time()
            && a.file_attributes() & 0x10 == b.file_attributes() & 0x10
            && a.len() == b.len()
            && a.last_write_time() == b.last_write_time()
    }
    #[cfg(unix)]
    pub fn is_same(a: &Metadata, b: &Metadata) -> bool {
        use std::os::unix::fs::MetadataExt;
        a.dev() == b.dev() && a.ino() == b.ino()
    }
    #[cfg(not(any(windows, unix)))]
    pub fn is_same(a: &Metadata, b: &Metadata) -> bool {
        a.len() == b.len() && a.modified().ok() == b.modified().ok()
    }
}
fn fs_err(path: &str) -> Item {
    Item::Result(Box::new(ScanResult {
        path: path.to_string(),
        result: IndexResult::default(),
        err: Some(ScanError::new(ScanErrorCode::Filesystem)),
    }))
}
fn resolve_root_file_link(path: &str) -> Result<(String, Metadata), ScanError> {
    let resolved =
        std::fs::canonicalize(path).map_err(|_| ScanError::new(ScanErrorCode::Symlink))?;
    let info = std::fs::metadata(&resolved).map_err(|_| ScanError::new(ScanErrorCode::Symlink))?;
    if !info.is_file() {
        return Err(ScanError::new(ScanErrorCode::Symlink));
    }
    Ok((resolved.to_string_lossy().into_owned(), info))
}
fn resolve_contained_file_link(path: &str, root: &Path) -> Result<(String, Metadata), ScanError> {
    let (resolved, info) = resolve_root_file_link(path)?;
    if !Path::new(&resolved).starts_with(root) {
        return Err(ScanError::new(ScanErrorCode::Symlink));
    }
    Ok((resolved, info))
}
fn produce(
    root: &str,
    o: &ScanOptions,
    filter: Option<ScanFilter<'_>>,
    emit: &mut dyn FnMut(Item) -> bool,
) -> Result<(), ScanError> {
    if exceeds(root, o.max_path_bytes) {
        return Err(ScanError::limit("MaxPathBytes"));
    }
    let Ok(info) = std::fs::symlink_metadata(root) else {
        emit(fs_err(root));
        return Ok(());
    };
    let mut file_count = 0i64;
    let mut count_file = || {
        file_count += 1;
        if file_count > o.max_files {
            return Err(ScanError::limit("MaxFiles"));
        }
        Ok(())
    };
    let queue =
        |path: &str, read_path: &str, info: Metadata, emit: &mut dyn FnMut(Item) -> bool| {
            if exceeds(path, o.max_path_bytes) || exceeds(read_path, o.max_path_bytes) {
                return Err(ScanError::limit("MaxPathBytes"));
            }
            if let Some(filter) = filter {
                let selected =
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| filter(path, &info)))
                        .map_err(|_| ScanError::new(ScanErrorCode::Filter))?;
                if !selected {
                    return Ok(());
                }
            }
            emit(Item::Task(Task {
                path: path.to_string(),
                read_path: read_path.to_string(),
                info,
            }));
            Ok(())
        };
    if info.file_type().is_symlink() {
        count_file()?;
        match o.symlink_policy {
            SymlinkPolicy::Reject => {
                emit(Item::Result(Box::new(ScanResult {
                    path: root.to_string(),
                    result: IndexResult::default(),
                    err: Some(ScanError::new(ScanErrorCode::Symlink)),
                })));
                return Ok(());
            }
            SymlinkPolicy::FollowFiles => {
                return match resolve_root_file_link(root) {
                    Ok((resolved, target)) => queue(root, &resolved, target, emit),
                    Err(e) => {
                        emit(Item::Result(Box::new(ScanResult {
                            path: root.to_string(),
                            result: IndexResult::default(),
                            err: Some(e),
                        })));
                        Ok(())
                    }
                };
            }
            _ => return Ok(()),
        }
    }
    if info.is_file() {
        count_file()?;
        return queue(root, root, info, emit);
    }
    if !info.is_dir() {
        count_file()?;
        return Ok(());
    }
    let Ok(canonical_root) = std::fs::canonicalize(root) else {
        emit(fs_err(root));
        return Ok(());
    };
    let mut directory_count = 1i64;
    let mut directories: Vec<(String, i64, Metadata)> = vec![(root.to_string(), 0, info)];
    while let Some((dir, depth, dir_info)) = directories.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => {
                emit(fs_err(&dir));
                continue;
            }
        };
        match std::fs::metadata(&dir) {
            Ok(opened) if opened.is_dir() && same_file(&opened, &dir_info) => {}
            _ => {
                emit(fs_err(&dir));
                continue;
            }
        }
        let mut read_failed = false;
        for entry in entries {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => {
                    read_failed = true;
                    break;
                }
            };
            let path = PathBuf::from(&dir)
                .join(entry.file_name())
                .to_string_lossy()
                .into_owned();
            if exceeds(&path, o.max_path_bytes) {
                return Err(ScanError::limit("MaxPathBytes"));
            }
            let Ok(entry_info) = std::fs::symlink_metadata(&path) else {
                emit(fs_err(&path));
                continue;
            };
            if !entry_info.is_dir() {
                count_file()?;
            }
            if entry_info.file_type().is_symlink() {
                match o.symlink_policy {
                    SymlinkPolicy::Reject => {
                        emit(Item::Result(Box::new(ScanResult {
                            path: path.clone(),
                            result: IndexResult::default(),
                            err: Some(ScanError::new(ScanErrorCode::Symlink)),
                        })));
                    }
                    SymlinkPolicy::FollowFiles => {
                        match resolve_contained_file_link(&path, &canonical_root) {
                            Ok((resolved, target)) => queue(&path, &resolved, target, emit)?,
                            Err(e) => {
                                emit(Item::Result(Box::new(ScanResult {
                                    path: path.clone(),
                                    result: IndexResult::default(),
                                    err: Some(e),
                                })));
                            }
                        }
                    }
                    _ => {}
                }
                continue;
            }
            if entry_info.is_dir() {
                let next = depth + 1;
                if next > o.max_depth {
                    return Err(ScanError::limit("MaxDepth"));
                }
                directory_count += 1;
                if directory_count > o.max_directories {
                    return Err(ScanError::limit("MaxDirectories"));
                }
                directories.push((path, next, entry_info));
                continue;
            }
            if entry_info.is_file() {
                queue(&path, &path, entry_info, emit)?;
            }
        }
        if read_failed {
            emit(fs_err(&dir));
        }
    }
    Ok(())
}
fn read_task(task: &Task, options: &Options) -> (IndexResult, Option<ScanError>) {
    match std::fs::symlink_metadata(&task.read_path) {
        Ok(info) if info.is_file() && same_file(&info, &task.info) => {}
        _ => {
            return (
                IndexResult::default(),
                Some(ScanError::new(ScanErrorCode::Filesystem)),
            );
        }
    }
    let (mut result, err) = read_path(&task.read_path, options.clone());
    result.record.source.origin = String::new();
    let err = err.map(|e| {
        let (code, limit) = if e.is(Sentinel::ResourceLimit) {
            (ScanErrorCode::Limit, "ReadOptions.Limits".to_string())
        } else {
            (ScanErrorCode::Read, String::new())
        };
        ScanError {
            code,
            limit,
            cause: Some(Box::new(e)),
        }
    });
    (result, err)
}
pub fn scan(
    root: &str,
    options: ScanOptions,
    filter: Option<ScanFilter<'_>>,
    yield_fn: &mut dyn FnMut(ScanResult) -> Result<(), ()>,
) -> Result<(), ScanError> {
    let o = normalize_scan_options(options)?;
    if root.trim().is_empty() {
        return Err(ScanError::new(ScanErrorCode::InvalidOptions));
    }
    let mut terminal: Option<ScanError> = None;
    let mut error_count = 0i64;
    let mut deliver = |result: ScanResult, terminal: &mut Option<ScanError>| -> bool {
        if terminal.is_some() {
            return false;
        }
        if result.err.is_some() {
            error_count += 1;
            if error_count > o.max_errors {
                *terminal = Some(ScanError::limit("MaxErrors"));
                return false;
            }
        }
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| yield_fn(result)));
        if !matches!(outcome, Ok(Ok(()))) {
            *terminal = Some(ScanError::new(ScanErrorCode::Yield));
            return false;
        }
        true
    };
    let producer = produce(root, &o, filter, &mut |item| {
        if terminal.is_some() {
            return false;
        }
        let result = match item {
            Item::Result(r) => *r,
            Item::Task(task) => {
                let (result, err) = read_task(&task, &o.read_options);
                ScanResult {
                    path: task.path,
                    result,
                    err,
                }
            }
        };
        deliver(result, &mut terminal)
    });
    if let Some(t) = terminal {
        return Err(t);
    }
    producer
}
