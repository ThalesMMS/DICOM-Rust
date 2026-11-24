use std::fs::{self, File, OpenOptions};
use std::io;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
pub fn file_id(m: &fs::Metadata) -> (u64, u64) {
    (m.dev(), m.ino())
}
pub fn is_reparse(_: &fs::Metadata) -> bool {
    false
}
pub const OPENS_DIRECTORY_ENTRIES: bool = true;
pub fn system_symlink_target(parent: &Path, link: &Path, m: &fs::Metadata) -> Option<PathBuf> {
    if !m.file_type().is_symlink() || m.uid() != 0 {
        return None;
    }
    let pm = fs::metadata(if parent.as_os_str().is_empty() {
        Path::new("/")
    } else {
        parent
    })
    .ok()?;
    if pm.uid() != 0 || pm.mode() & 0o022 != 0 {
        return None;
    }
    let target = fs::read_link(link).ok()?;
    (!target.as_os_str().is_empty()).then_some(target)
}
pub fn create_directory(path: &Path) -> io::Result<()> {
    fs::DirBuilder::new().mode(0o700).create(path)
}
pub fn open_directory(path: &Path, _created: bool) -> io::Result<File> {
    File::open(path)
}
pub fn create_file(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
}
pub fn sync_directory(dir: &Path) -> io::Result<()> {
    File::open(dir)?.sync_all()
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
    temp: &File,
    name: &str,
    key: &str,
    replace: bool,
) -> (bool, io::Result<()>) {
    let source = dir.join(name);
    let destination = dir.join(key);
    if replace {
        let r = fs::rename(&source, &destination);
        return (r.is_ok(), r);
    }
    let expected = match temp.metadata() {
        Ok(m) => m,
        Err(e) => return (false, Err(e)),
    };
    match super::entry_metadata(&source) {
        Ok(m) if m.is_file() && file_id(&m) == file_id(&expected) => {}
        Ok(_) => return (false, Err(io::Error::from(io::ErrorKind::InvalidInput))),
        Err(e) => return (false, Err(e)),
    }
    if let Err(e) = fs::hard_link(&source, &destination) {
        return (false, Err(e));
    }
    (true, super::remove_at(dir, name))
}
