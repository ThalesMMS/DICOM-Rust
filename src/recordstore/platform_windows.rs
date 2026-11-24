use std::fs::{self, File, OpenOptions};
use std::io;
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
const FILE_GENERIC_READ: u32 = 0x0012_0089;
const FILE_GENERIC_WRITE: u32 = 0x0012_0116;
const DELETE: u32 = 0x0001_0000;
const FILE_SHARE_ALL: u32 = 0x7;
const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
pub fn file_id(m: &fs::Metadata) -> (u64, u64) {
    (m.creation_time(), 0)
}
pub fn is_reparse(m: &fs::Metadata) -> bool {
    m.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}
pub const OPENS_DIRECTORY_ENTRIES: bool = false;
pub fn system_symlink_target(_: &Path, _: &Path, _: &fs::Metadata) -> Option<PathBuf> {
    None
}
pub fn create_directory(path: &Path) -> io::Result<()> {
    fs::create_dir(path)
}
pub fn open_directory(path: &Path, created: bool) -> io::Result<File> {
    let mut access = FILE_GENERIC_READ;
    if created {
        access |= DELETE;
    }
    OpenOptions::new()
        .read(true)
        .access_mode(access)
        .share_mode(FILE_SHARE_ALL)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
}
pub fn create_file(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .access_mode(FILE_GENERIC_READ | FILE_GENERIC_WRITE | DELETE)
        .share_mode(FILE_SHARE_ALL)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
}
pub fn sync_directory(_: &Path) -> io::Result<()> {
    Ok(())
}
pub fn lock_file(f: &File) -> io::Result<()> {
    if fs4::fs_std::FileExt::try_lock_exclusive(f)? {
        Ok(())
    } else {
        Err(io::Error::from(io::ErrorKind::WouldBlock))
    }
}
pub fn unlock_file(f: &File) -> io::Result<()> {
    fs4::fs_std::FileExt::unlock(f)
}
pub fn publish(
    dir: &Path,
    _temp: &File,
    name: &str,
    key: &str,
    replace: bool,
) -> (bool, io::Result<()>) {
    let source = dir.join(name);
    let destination = dir.join(key);
    if replace {
        let r = super::retry_sharing(|| fs::rename(&source, &destination));
        return (r.is_ok(), r);
    }
    if let Err(e) = super::retry_sharing(|| fs::hard_link(&source, &destination)) {
        return (false, Err(e));
    }
    (true, super::remove_at(dir, name))
}
