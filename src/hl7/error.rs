use std::fmt;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sentinel {
    Syntax,
    Limit,
    Charset,
    Xml,
    StreamRead,
    BatchEnvelope,
    BatchCount,
    Canceled,
}
impl Sentinel {
    pub fn text(self) -> &'static str {
        match self {
            Sentinel::Syntax => "hl7: malformed message",
            Sentinel::Limit => "hl7: message exceeds a parse limit",
            Sentinel::Charset => "hl7: character set",
            Sentinel::Xml => "hl7: malformed structural XML",
            Sentinel::StreamRead => "hl7: could not read ER7 input",
            Sentinel::BatchEnvelope => "hl7: malformed message: invalid batch envelope",
            Sentinel::BatchCount => "hl7: malformed message: invalid batch trailer count",
            Sentinel::Canceled => "context canceled",
        }
    }
}
#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    Sentinel(Sentinel),
    Wrapped { text: String, wrapped: Vec<Error> },
    Text(String),
    Parse(Box<ParseError>),
    Application(Box<ApplicationError>),
    Batch(Box<BatchError>),
    Stream(Box<StreamError>),
}
impl Error {
    pub fn wrap(s: Sentinel, detail: &str) -> Error {
        Error::Wrapped {
            text: format!("{}: {detail}", s.text()),
            wrapped: vec![Error::Sentinel(s)],
        }
    }
    pub(crate) fn join2(a: Error, b: Error) -> Error {
        Error::Wrapped {
            text: format!("{a}: {b}"),
            wrapped: vec![a, b],
        }
    }
    pub fn text(s: impl Into<String>) -> Error {
        Error::Text(s.into())
    }
    pub fn is(&self, s: Sentinel) -> bool {
        match self {
            Error::Sentinel(x) => {
                *x == s
                    || (s == Sentinel::Syntax
                        && matches!(x, Sentinel::BatchEnvelope | Sentinel::BatchCount))
            }
            Error::Wrapped { wrapped, .. } => wrapped.iter().any(|e| e.is(s)),
            Error::Text(_) => false,
            Error::Parse(p) => p.err.is(s),
            Error::Application(a) => a.err.is(s),
            Error::Batch(b) => b.err.is(s),
            Error::Stream(st) => st.err.is(s),
        }
    }
    pub fn as_parse(&self) -> Option<&ParseError> {
        match self {
            Error::Parse(p) => Some(p),
            Error::Wrapped { wrapped, .. } => wrapped.iter().find_map(Error::as_parse),
            Error::Application(a) => a.err.as_parse(),
            Error::Batch(b) => b.err.as_parse(),
            Error::Stream(s) => s.err.as_parse(),
            _ => None,
        }
    }
    pub fn as_application(&self) -> Option<&ApplicationError> {
        match self {
            Error::Application(a) => Some(a),
            Error::Wrapped { wrapped, .. } => wrapped.iter().find_map(Error::as_application),
            Error::Parse(p) => p.err.as_application(),
            Error::Batch(b) => b.err.as_application(),
            Error::Stream(s) => s.err.as_application(),
            _ => None,
        }
    }
    pub fn as_batch(&self) -> Option<&BatchError> {
        match self {
            Error::Batch(b) => Some(b),
            Error::Wrapped { wrapped, .. } => wrapped.iter().find_map(Error::as_batch),
            _ => None,
        }
    }
    pub fn as_stream(&self) -> Option<&StreamError> {
        match self {
            Error::Stream(s) => Some(s),
            Error::Wrapped { wrapped, .. } => wrapped.iter().find_map(Error::as_stream),
            _ => None,
        }
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Sentinel(s) => f.write_str(s.text()),
            Error::Wrapped { text, .. } => f.write_str(text),
            Error::Text(t) => f.write_str(t),
            Error::Parse(p) => p.fmt(f),
            Error::Application(a) => a.fmt(f),
            Error::Batch(b) => b.fmt(f),
            Error::Stream(s) => s.fmt(f),
        }
    }
}
impl std::error::Error for Error {}
impl From<ParseError> for Error {
    fn from(p: ParseError) -> Error {
        Error::Parse(Box::new(p))
    }
}
impl From<ApplicationError> for Error {
    fn from(a: ApplicationError) -> Error {
        Error::Application(Box::new(a))
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct ParseError {
    pub segment: usize,
    pub name: String,
    pub field: usize,
    pub err: Error,
}
impl ParseError {
    pub(super) fn new(err: Error) -> ParseError {
        ParseError {
            segment: 0,
            name: String::new(),
            field: 0,
            err,
        }
    }
    pub(super) fn at(segment: usize, name: &str, field: usize, err: Error) -> ParseError {
        ParseError {
            segment,
            name: name.to_owned(),
            field,
            err,
        }
    }
}
impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.segment > 0 && self.field > 0 {
            write!(
                f,
                "{} at segment {} ({}) field {}",
                self.err, self.segment, self.name, self.field
            )
        } else if self.segment > 0 {
            write!(
                f,
                "{} at segment {} ({})",
                self.err, self.segment, self.name
            )
        } else {
            write!(f, "{}", self.err)
        }
    }
}
impl std::error::Error for ParseError {}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ErrorLocation {
    pub segment: String,
    pub sequence: i64,
    pub field: i64,
    pub component: i64,
}
impl fmt::Display for ErrorLocation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.segment.is_empty() {
            return Ok(());
        }
        if self.sequence > 1 {
            write!(f, "{}[{}]-{}", self.segment, self.sequence, self.field)?;
        } else {
            write!(f, "{}-{}", self.segment, self.field)?;
        }
        if self.component > 0 {
            write!(f, ".{}", self.component)?;
        }
        Ok(())
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct ApplicationError {
    pub code: String,
    pub location: ErrorLocation,
    pub reject: bool,
    pub err: Error,
}
impl fmt::Display for ApplicationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let location = self.location.to_string();
        if location.is_empty() {
            write!(f, "hl7: error {}: {}", self.code, self.err)
        } else {
            write!(f, "hl7: error {} at {location}: {}", self.code, self.err)
        }
    }
}
impl std::error::Error for ApplicationError {}
#[derive(Debug, Clone, PartialEq)]
pub struct BatchError {
    pub offset: i64,
    pub batch: usize,
    pub message: usize,
    pub err: Error,
}
impl fmt::Display for BatchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "hl7: batch file at byte {}, batch {}, message {}: {}",
            self.offset, self.batch, self.message, self.err
        )
    }
}
impl std::error::Error for BatchError {}
#[derive(Debug, Clone, PartialEq)]
pub struct StreamError {
    pub message: i64,
    pub offset: i64,
    pub err: Error,
}
impl fmt::Display for StreamError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "hl7: message {} at byte {}: {}",
            self.message, self.offset, self.err
        )
    }
}
impl std::error::Error for StreamError {}
