#![cfg_attr(
    not(test),
    warn(clippy::indexing_slicing, clippy::arithmetic_side_effects)
)]
use std::fs::File;
use std::io;
use std::time::SystemTime;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub(crate) struct FileIdentity {
    pub volume: u64,
    pub file: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileStamp {
    pub identity: FileIdentity,
    pub size: i64,
    pub modified: Option<SystemTime>,
    pub regular: bool,
}
#[derive(Clone, Debug)]
pub struct DirEntry {
    pub name: String,
    pub is_dir: bool,
    pub is_symlink: bool,
    pub regular: bool,
}
pub fn stamp(file: &File) -> io::Result<FileStamp> {
    let meta = file.metadata()?;
    Ok(FileStamp {
        identity: file_identity(file)?,
        size: meta.len() as i64,
        modified: meta.modified().ok(),
        regular: meta.is_file(),
    })
}
pub(crate) fn list_directory(root: &str, relative: &str) -> io::Result<Vec<DirEntry>> {
    let directory = open_relative_directory(root, relative)?;
    let path = if relative.is_empty() {
        root.to_string()
    } else {
        super::pathrules::join(&[root, relative])
    };
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&path)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let (is_dir, is_symlink) = match entry.file_type() {
            Ok(t) => (t.is_dir(), t.is_symlink()),
            Err(_) => (false, false),
        };
        let regular = entry
            .metadata()
            .map(|m| m.file_type().is_file())
            .unwrap_or(false);
        out.push(DirEntry {
            name,
            is_dir,
            is_symlink,
            regular,
        });
    }
    drop(directory);
    Ok(out)
}
pub(crate) fn path_identity(path: &str) -> io::Result<FileIdentity> {
    imp::path_identity(path)
}
pub(crate) fn open_relative_file(root: &str, relative: &str) -> io::Result<File> {
    imp::open_relative(root, relative, false)
}
pub(crate) fn open_relative_directory(root: &str, relative: &str) -> io::Result<File> {
    imp::open_relative(root, relative, true)
}
pub(crate) fn file_identity(file: &File) -> io::Result<FileIdentity> {
    imp::file_identity(file)
}
pub(crate) fn replace_file_atomically(source: &str, destination: &str) -> io::Result<()> {
    std::fs::rename(source, destination)
}
pub fn sync_directory(path: &str) -> io::Result<()> {
    #[cfg(unix)]
    {
        File::open(path)?.sync_all()
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Ok(())
    }
}
#[cfg(windows)]
#[allow(unsafe_code)]
mod imp {
    use super::FileIdentity;
    use std::fs::{File, OpenOptions};
    use std::io;
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::{AsRawHandle, FromRawHandle};
    use windows_sys::Wdk::Foundation::OBJECT_ATTRIBUTES;
    use windows_sys::Wdk::Storage::FileSystem::{
        FILE_DIRECTORY_FILE, FILE_NON_DIRECTORY_FILE, FILE_OPEN, FILE_SYNCHRONOUS_IO_NONALERT,
        NtCreateFile,
    };
    use windows_sys::Win32::Foundation::{
        GENERIC_READ, HANDLE, OBJ_CASE_INSENSITIVE, OBJ_DONT_REPARSE, RtlNtStatusToDosError,
        UNICODE_STRING,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT,
        FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_GENERIC_READ,
        FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, GetFileInformationByHandle,
    };
    use windows_sys::Win32::System::IO::IO_STATUS_BLOCK;
    const SHARE: u32 = FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE;
    fn handle_info(file: &File) -> io::Result<BY_HANDLE_FILE_INFORMATION> {
        let mut info = BY_HANDLE_FILE_INFORMATION::default();
        if unsafe { GetFileInformationByHandle(file.as_raw_handle() as HANDLE, &mut info) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(info)
    }
    pub(crate) fn file_identity(file: &File) -> io::Result<FileIdentity> {
        let info = handle_info(file)?;
        Ok(FileIdentity {
            volume: u64::from(info.dwVolumeSerialNumber),
            file: (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
        })
    }
    fn open_reparse(path: &str, access: u32) -> io::Result<File> {
        OpenOptions::new()
            .access_mode(access)
            .share_mode(SHARE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)
    }
    pub(crate) fn path_identity(path: &str) -> io::Result<FileIdentity> {
        file_identity(&open_reparse(path, 0)?)
    }
    pub(crate) fn open_relative(root: &str, relative: &str, directory: bool) -> io::Result<File> {
        if root.contains('\0') || relative.contains('\0') {
            return Err(io::Error::from(io::ErrorKind::InvalidInput));
        }
        let root_file = open_reparse(root, GENERIC_READ)?;
        let info = handle_info(&root_file)?;
        if info.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY == 0
            || info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
        {
            return Err(io::Error::other("dicomdir: invalid media root"));
        }
        if relative.is_empty() {
            return Ok(root_file);
        }
        let wide: Vec<u16> = relative.encode_utf16().collect();
        let bytes = wide.len().saturating_mul(2);
        if bytes > usize::from(u16::MAX).saturating_sub(2) {
            return Err(io::Error::from(io::ErrorKind::InvalidInput));
        }
        let name = UNICODE_STRING {
            Length: bytes as u16,
            MaximumLength: bytes as u16,
            Buffer: wide.as_ptr() as *mut u16,
        };
        let attributes = OBJECT_ATTRIBUTES {
            Length: std::mem::size_of::<OBJECT_ATTRIBUTES>() as u32,
            RootDirectory: root_file.as_raw_handle() as HANDLE,
            ObjectName: &name,
            Attributes: OBJ_CASE_INSENSITIVE | OBJ_DONT_REPARSE,
            SecurityDescriptor: std::ptr::null(),
            SecurityQualityOfService: std::ptr::null(),
        };
        let options = FILE_SYNCHRONOUS_IO_NONALERT
            | if directory {
                FILE_DIRECTORY_FILE
            } else {
                FILE_NON_DIRECTORY_FILE
            };
        let mut handle: HANDLE = std::ptr::null_mut();
        let status = unsafe {
            let mut io_status: IO_STATUS_BLOCK = std::mem::zeroed();
            NtCreateFile(
                &mut handle,
                FILE_GENERIC_READ,
                &attributes,
                &mut io_status,
                std::ptr::null(),
                0,
                SHARE,
                FILE_OPEN,
                options,
                std::ptr::null(),
                0,
            )
        };
        if status < 0 {
            let code = unsafe { RtlNtStatusToDosError(status) };
            return Err(io::Error::from_raw_os_error(code as i32));
        }
        Ok(unsafe { File::from_raw_handle(handle as _) })
    }
}
#[cfg(unix)]
mod imp {
    #![forbid(unsafe_code)]
    use super::FileIdentity;
    use rustix::fs::{Mode, OFlags};
    use std::fs::File;
    use std::io;
    use std::os::unix::fs::MetadataExt;
    pub(crate) fn file_identity(file: &File) -> io::Result<FileIdentity> {
        let meta = file.metadata()?;
        Ok(FileIdentity {
            volume: meta.dev(),
            file: meta.ino(),
        })
    }
    pub(crate) fn path_identity(path: &str) -> io::Result<FileIdentity> {
        let meta = std::fs::symlink_metadata(path)?;
        Ok(FileIdentity {
            volume: meta.dev(),
            file: meta.ino(),
        })
    }
    pub(crate) fn open_relative(root: &str, relative: &str, directory: bool) -> io::Result<File> {
        let base = OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW;
        let mut current = rustix::fs::open(root, base | OFlags::DIRECTORY, Mode::empty())?;
        if relative.is_empty() {
            return Ok(File::from(current));
        }
        let parts: Vec<&str> = relative.split('/').collect();
        for (index, part) in parts.iter().enumerate() {
            let mut flags = base;
            if index < parts.len() - 1 || directory {
                flags |= OFlags::DIRECTORY;
            }
            current = rustix::fs::openat(&current, *part, flags, Mode::empty())?;
        }
        Ok(File::from(current))
    }
}
#[cfg(not(any(windows, unix)))]
mod imp {
    #![forbid(unsafe_code)]
    use super::FileIdentity;
    use std::fs::File;
    use std::io;
    pub(crate) fn file_identity(_: &File) -> io::Result<FileIdentity> {
        Err(io::Error::other(
            "dicomdir: file identity is unsupported on this platform",
        ))
    }
    pub(crate) fn path_identity(_: &str) -> io::Result<FileIdentity> {
        Err(io::Error::other(
            "dicomdir: file identity is unsupported on this platform",
        ))
    }
    pub(crate) fn open_relative(_: &str, _: &str, _: bool) -> io::Result<File> {
        Err(io::Error::other(
            "dicomdir: secure referenced-file opening is unsupported on this platform",
        ))
    }
}
