use crate::object::{self, File, ReadFileOptions};
use std::fmt;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SourceReadOptions {
    pub defer_pixel_data: bool,
    pub max_total_bytes: i64,
}
pub enum OpenedFile<'a> {
    Borrowed(&'a File),
    Owned(Box<File>),
}
impl OpenedFile<'_> {
    pub fn file(&self) -> &File {
        match self {
            OpenedFile::Borrowed(f) => f,
            OpenedFile::Owned(f) => f,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceError {
    pub text: String,
    pub limit: bool,
}
impl fmt::Display for SourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}
impl std::error::Error for SourceError {}
pub trait Source {
    fn len(&self) -> i64;
    fn is_empty(&self) -> bool {
        self.len() <= 0
    }
    fn open(&self, index: i64, options: &SourceReadOptions) -> Result<OpenedFile<'_>, SourceError>;
}
fn invalid(detail: String) -> SourceError {
    SourceError {
        text: format!("nifti: invalid source: {detail}"),
        limit: false,
    }
}
fn validate_open_request(count: i64, index: i64) -> Result<(), SourceError> {
    if index < 0 || index >= count {
        return Err(invalid(format!("index {index} outside [0,{count})")));
    }
    Ok(())
}
pub struct FilesSource<'a> {
    files: Vec<Option<&'a File>>,
}
impl<'a> FilesSource<'a> {
    pub fn new(files: Vec<Option<&'a File>>) -> FilesSource<'a> {
        FilesSource { files }
    }
}
impl Source for FilesSource<'_> {
    fn len(&self) -> i64 {
        self.files.len() as i64
    }
    fn open(&self, index: i64, _: &SourceReadOptions) -> Result<OpenedFile<'_>, SourceError> {
        validate_open_request(self.len(), index)?;
        match self.files[index as usize] {
            Some(file) => Ok(OpenedFile::Borrowed(file)),
            None => Err(invalid(format!("file {index} is nil"))),
        }
    }
}
pub struct PathSource {
    paths: Vec<String>,
}
impl PathSource {
    pub fn new(paths: Vec<String>) -> PathSource {
        PathSource { paths }
    }
}
impl Source for PathSource {
    fn len(&self) -> i64 {
        self.paths.len() as i64
    }
    fn open(&self, index: i64, options: &SourceReadOptions) -> Result<OpenedFile<'_>, SourceError> {
        validate_open_request(self.len(), index)?;
        let path = &self.paths[index as usize];
        if path.trim().is_empty() {
            return Err(invalid(format!("empty path at index {index}")));
        }
        let read_options = ReadFileOptions {
            max_total_bytes: options.max_total_bytes,
            ..ReadFileOptions::default()
        };
        match object::open_file_with_options(path, read_options) {
            Ok(file) => Ok(OpenedFile::Owned(Box::new(file))),
            Err((_, err)) => {
                let limit = err.is(object::Kind::MaxTotalBytes);
                let cause = if limit {
                    super::ExportSentinel::LimitExceeded.text().to_string()
                } else if std::fs::metadata(path).is_err() {
                    "file does not exist".to_string()
                } else {
                    "nifti: source open failed".to_string()
                };
                Err(SourceError {
                    text: format!("nifti: open source index {index}: {cause}"),
                    limit,
                })
            }
        }
    }
}
