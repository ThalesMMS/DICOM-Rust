#![forbid(unsafe_code)]
use super::RenderError;
use std::fmt;
use std::io::Write;
mod comparison;
mod evidence;
mod fixtures;
mod frame_metrics;
mod sequence;
mod tolerance_compare;
mod transport;
pub use comparison::*;
pub use evidence::*;
pub use fixtures::*;
pub use frame_metrics::*;
pub use sequence::*;
pub use tolerance_compare::*;
pub use transport::*;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ErrorKind {
    InvalidComparison,
    InvalidEvidence,
    InvalidFrameMetrics,
    Render,
    Io,
    Other,
}
#[derive(Debug, Clone, PartialEq)]
pub struct Error {
    kind: ErrorKind,
    message: String,
    render: Option<RenderError>,
}
pub type Result<T> = std::result::Result<T, Error>;
pub const ERR_INVALID_COMPARISON: &str = "qualification: invalid comparison";
pub const ERR_INVALID_EVIDENCE: &str = "qualification: invalid evidence";
pub const ERR_INVALID_FRAME_METRICS: &str = "qualification: invalid RGBA frame metrics";
impl Error {
    pub(crate) fn comparison(detail: impl fmt::Display) -> Error {
        Error {
            kind: ErrorKind::InvalidComparison,
            message: format!("{ERR_INVALID_COMPARISON}: {detail}"),
            render: None,
        }
    }
    pub(crate) fn evidence(detail: impl fmt::Display) -> Error {
        Error {
            kind: ErrorKind::InvalidEvidence,
            message: format!("{ERR_INVALID_EVIDENCE}: {detail}"),
            render: None,
        }
    }
    pub(crate) fn metrics(detail: impl fmt::Display) -> Error {
        Error {
            kind: ErrorKind::InvalidFrameMetrics,
            message: format!("{ERR_INVALID_FRAME_METRICS}: {detail}"),
            render: None,
        }
    }
    pub(crate) fn other(message: impl Into<String>) -> Error {
        Error {
            kind: ErrorKind::Other,
            message: message.into(),
            render: None,
        }
    }
    pub(crate) fn io(err: &std::io::Error) -> Error {
        Error {
            kind: ErrorKind::Io,
            message: err.to_string(),
            render: None,
        }
    }
    pub(crate) fn prefixed(mut self, prefix: impl fmt::Display) -> Error {
        self.message = format!("{prefix}: {}", self.message);
        self
    }
    pub fn kind(&self) -> ErrorKind {
        self.kind
    }
    pub fn is(&self, kind: ErrorKind) -> bool {
        self.kind == kind
    }
    pub fn render(&self) -> Option<&RenderError> {
        self.render.as_ref()
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for Error {}
impl From<RenderError> for Error {
    fn from(err: RenderError) -> Error {
        Error {
            kind: ErrorKind::Render,
            message: err.to_string(),
            render: Some(err),
        }
    }
}
pub(crate) fn write_text(dst: &mut dyn Write, text: &str) -> Result<()> {
    dst.write_all(text.as_bytes()).map_err(|e| Error::io(&e))
}
pub(crate) fn json_string(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\u{2028}' | '\u{2029}' => out.push_str(&format!("\\u{:04x}", c as u32)),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}
pub(crate) fn json_float(out: &mut String, f: f64) {
    let abs = f.abs();
    let exponent = abs != 0.0 && !(1e-6..1e21).contains(&abs);
    let mut text = crate::numtext::format_float(f, if exponent { 'e' } else { 'f' }, -1);
    if exponent {
        let b = text.as_bytes();
        let n = b.len();
        if n >= 4 && b[n - 4] == b'e' && (b[n - 3] == b'-' || b[n - 3] == b'+') && b[n - 2] == b'0'
        {
            text.remove(n - 2);
        }
    }
    out.push_str(&text);
}
pub(crate) fn blank(value: &str) -> bool {
    trim_unicode_space(value).is_empty()
}
pub(crate) fn trim_unicode_space(value: &str) -> &str {
    fn space(c: char) -> bool {
        matches!(
            c,
            '\t' | '\n'
                | '\u{b}'
                | '\u{c}'
                | '\r'
                | ' '
                | '\u{85}'
                | '\u{a0}'
                | '\u{1680}'
                | '\u{2000}'
                ..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}'
        )
    }
    value.trim_matches(space)
}
pub(crate) fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push_str(&format!("{b:02x}"));
    }
    out
}
