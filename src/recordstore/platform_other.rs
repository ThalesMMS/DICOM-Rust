use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
fn unsupported() -> io::Error {
    io::Error::from(io::ErrorKind::Unsupported)
}
pub fn file_id(_: &fs::Metadata) -> (u64, u64) {
    (0, 0)
}
pub fn is_reparse(_: &fs::Metadata) -> bool {
    false
}
pub const OPENS_DIRECTORY_ENTRIES: bool = true;
pub fn system_symlink_target(_: &Path, _: &Path, _: &fs::Metadata) -> Option<PathBuf> {
    None
}
pub fn create_directory(path: &Path) -> io::Result<()> {
    fs::create_dir(path)
}
pub fn open_directory(path: &Path, _created: bool) -> io::Result<File> {
    File::open(path)
}
pub fn create_file(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(path)
}
pub fn sync_directory(_: &Path) -> io::Result<()> {
    Err(unsupported())
}
pub fn lock_file(_: &File) -> io::Result<()> {
    Err(unsupported())
}
pub fn unlock_file(_: &File) -> io::Result<()> {
    Ok(())
}
pub fn publish(_: &Path, _: &File, _: &str, _: &str, _: bool) -> (bool, io::Result<()>) {
    (false, Err(unsupported()))
}
