#![forbid(unsafe_code)]
use std::fmt;
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct MessageError {
    message: String,
}
impl MessageError {
    pub fn new(message: impl Into<String>) -> MessageError {
        MessageError {
            message: message.into(),
        }
    }
    pub fn message(&self) -> &str {
        &self.message
    }
    pub fn into_message(self) -> String {
        self.message
    }
}
impl fmt::Display for MessageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for MessageError {}
impl From<String> for MessageError {
    fn from(message: String) -> MessageError {
        MessageError { message }
    }
}
impl From<&str> for MessageError {
    fn from(message: &str) -> MessageError {
        MessageError::new(message)
    }
}
impl From<MessageError> for String {
    fn from(e: MessageError) -> String {
        e.message
    }
}
