use std::fmt;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    message: String,
}
impl Error {
    pub(crate) fn new(message: String) -> Error {
        Error { message }
    }
    pub fn message(&self) -> &str {
        &self.message
    }
    pub fn into_message(self) -> String {
        self.message
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for Error {}
