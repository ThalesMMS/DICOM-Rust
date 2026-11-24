#![forbid(unsafe_code)]
#![allow(clippy::result_large_err)]
mod cache;
mod client;
mod codestream;
mod databins;
mod decode;
mod index;
mod message;
mod plan;
mod policy;
mod server;
mod target;
mod tier2;
pub use cache::CacheStats;
pub use client::{
    ByteRange, Client, Config, Reference, Region, Registry, Request, Response, RetryDelay, Size,
    StreamType,
};
pub use databins::{DataBinSet, Reconstruction};
pub use decode::DecodedFrame;
pub use index::{CodestreamIndex, IndexLimits, index_codestream};
pub use message::{
    CLASS_EXTENDED_PRECINCT, CLASS_EXTENDED_TILE, CLASS_MAIN_HEADER, CLASS_METADATA,
    CLASS_PRECINCT, CLASS_TILE, CLASS_TILE_HEADER, DataBinClass, EOR_BYTE_LIMIT_REACHED,
    EOR_IMAGE_DONE, EOR_NON_SPECIFIED, EOR_QUALITY_LIMIT_REACHED, EOR_RESPONSE_LIMIT_REACHED,
    EOR_SESSION_LIMIT_REACHED, EOR_WINDOW_CHANGE, EOR_WINDOW_DONE, EorReason, Message, Stream,
    append_eor, append_message, parse_stream,
};
pub use policy::{BasicCredential, BearerCredential, Policy, Rule, rule_for_host, rule_from_url};
pub use server::{Clock, EventSink, Server, ServerConfig, ServerEvent, ServerEventKind};
use std::fmt;
pub use target::{StaticTargets, Target, TargetProvider, target_from_object};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sentinel {
    InvalidRequest,
    BearerTokenUnavailable,
    PolicyDenied,
    RedirectDenied,
    Offline,
    HttpStatus,
    UnsupportedContentType,
    ResponseTooLarge,
    PartialResponse,
    CorruptResponse,
    IncrementalStream,
    Decode,
    MalformedStream,
    MalformedCodestream,
    IndexLimit,
    DataBinLimit,
    IncompleteDataBins,
    TargetNotFound,
    UnsupportedTarget,
    Canceled,
    DeadlineExceeded,
}
impl Sentinel {
    pub fn text(self) -> &'static str {
        match self {
            Sentinel::InvalidRequest => "dicom: invalid JPIP request",
            Sentinel::BearerTokenUnavailable => "dicom: JPIP bearer token unavailable",
            Sentinel::PolicyDenied => "dicom: JPIP endpoint denied by policy",
            Sentinel::RedirectDenied => "dicom: JPIP redirect denied by policy",
            Sentinel::Offline => "dicom: JPIP endpoint unavailable",
            Sentinel::HttpStatus => "dicom: JPIP endpoint returned an unsuccessful status",
            Sentinel::UnsupportedContentType => "dicom: unsupported JPIP response content type",
            Sentinel::ResponseTooLarge => "dicom: JPIP response exceeds configured limit",
            Sentinel::PartialResponse => "dicom: incomplete JPIP response",
            Sentinel::CorruptResponse => "dicom: corrupt JPIP response",
            Sentinel::IncrementalStream => {
                "dicom: JPIP incremental data-bin stream is not directly decodable"
            }
            Sentinel::Decode => "dicom: JPIP response decode failed",
            Sentinel::MalformedStream => "dicom: malformed JPIP data-bin stream",
            Sentinel::MalformedCodestream => "dicom: malformed JPEG 2000 codestream for JPIP",
            Sentinel::IndexLimit => "dicom: JPEG 2000 codestream exceeds JPIP index limits",
            Sentinel::DataBinLimit => "dicom: JPIP data-bin cache limit exceeded",
            Sentinel::IncompleteDataBins => {
                "dicom: JPIP data-bins are insufficient for a codestream"
            }
            Sentinel::TargetNotFound => "dicom: JPIP target not found",
            Sentinel::UnsupportedTarget => "dicom: pixel data cannot be served as JPIP codestreams",
            Sentinel::Canceled => "context canceled",
            Sentinel::DeadlineExceeded => "context deadline exceeded",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChainError {
    text: String,
    chain: Vec<Sentinel>,
}
impl ChainError {
    pub fn plain(text: impl Into<String>) -> ChainError {
        ChainError {
            text: text.into(),
            chain: Vec::new(),
        }
    }
    pub fn sentinel(s: Sentinel) -> ChainError {
        ChainError {
            text: s.text().into(),
            chain: vec![s],
        }
    }
    pub fn wrap(s: Sentinel, detail: impl fmt::Display) -> ChainError {
        ChainError {
            text: format!("{}: {detail}", s.text()),
            chain: vec![s],
        }
    }
    pub fn join(a: ChainError, b: ChainError) -> ChainError {
        let mut chain = a.chain;
        chain.extend(b.chain);
        ChainError {
            text: format!("{}\n{}", a.text, b.text),
            chain,
        }
    }
    pub(crate) fn join_text(s: Sentinel, other: impl fmt::Display) -> ChainError {
        ChainError {
            text: format!("{}\n{other}", s.text()),
            chain: vec![s],
        }
    }
    pub fn with_sentinel(self, s: Sentinel) -> ChainError {
        self.with(s)
    }
    pub(crate) fn with(mut self, s: Sentinel) -> ChainError {
        self.chain.push(s);
        self
    }
    pub fn is(&self, s: Sentinel) -> bool {
        self.chain.contains(&s)
    }
    pub fn text(&self) -> &str {
        &self.text
    }
}
impl fmt::Display for ChainError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}
impl std::error::Error for ChainError {}
pub(crate) fn malformed(detail: impl fmt::Display) -> ChainError {
    ChainError::wrap(Sentinel::MalformedCodestream, detail)
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    InvalidRequest,
    Authentication,
    PolicyDenied,
    RedirectDenied,
    Offline,
    HttpStatus,
    UnsupportedContentType,
    ResponseTooLarge,
    PartialResponse,
    CorruptResponse,
    IncrementalStream,
    Decode,
}
impl ErrorKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorKind::InvalidRequest => "invalid-request",
            ErrorKind::Authentication => "authentication",
            ErrorKind::PolicyDenied => "policy-denied",
            ErrorKind::RedirectDenied => "redirect-denied",
            ErrorKind::Offline => "offline",
            ErrorKind::HttpStatus => "http-status",
            ErrorKind::UnsupportedContentType => "unsupported-content-type",
            ErrorKind::ResponseTooLarge => "response-too-large",
            ErrorKind::PartialResponse => "partial-response",
            ErrorKind::CorruptResponse => "corrupt-response",
            ErrorKind::IncrementalStream => "incremental-stream",
            ErrorKind::Decode => "decode",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub kind: ErrorKind,
    pub operation: String,
    pub status_code: i64,
    pub content_type: String,
    pub limit: i64,
    pub size: i64,
    pub err: Option<ChainError>,
}
impl Error {
    pub(crate) fn new(kind: ErrorKind, operation: &str, err: ChainError) -> Error {
        Error {
            kind,
            operation: operation.into(),
            status_code: 0,
            content_type: String::new(),
            limit: 0,
            size: 0,
            err: Some(err),
        }
    }
    pub(crate) fn sentinel(kind: ErrorKind, operation: &str, s: Sentinel) -> Error {
        Error::new(kind, operation, ChainError::sentinel(s))
    }
    pub(crate) fn status(mut self, status: i64) -> Error {
        self.status_code = status;
        self
    }
    pub(crate) fn content_type(mut self, ct: &str) -> Error {
        self.content_type = ct.into();
        self
    }
    pub(crate) fn sized(mut self, limit: i64, size: i64) -> Error {
        self.limit = limit;
        self.size = size;
        self
    }
    pub fn is(&self, s: Sentinel) -> bool {
        self.err.as_ref().is_some_and(|e| e.is(s))
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut message = match &self.err {
            Some(e) => e.text.clone(),
            None => "dicom: JPIP operation failed".to_string(),
        };
        if !self.operation.is_empty() {
            message.push_str(" during ");
            message.push_str(&self.operation);
        }
        if self.status_code != 0 {
            message.push_str(&format!(" (HTTP {})", self.status_code));
        }
        if !self.content_type.is_empty() {
            message.push_str(&format!(
                " (content type {})",
                crate::textfmt::quote(&self.content_type)
            ));
        }
        if self.limit > 0 {
            message.push_str(&format!(" (limit {} bytes", self.limit));
            if self.size > 0 {
                message.push_str(&format!(", received {}", self.size));
            }
            message.push(')');
        }
        f.write_str(&message)
    }
}
impl std::error::Error for Error {}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClientError {
    Jpip(Box<Error>),
    Context(ChainError),
}
impl ClientError {
    pub fn is(&self, s: Sentinel) -> bool {
        match self {
            ClientError::Jpip(e) => e.is(s),
            ClientError::Context(e) => e.is(s),
        }
    }
    pub fn as_jpip(&self) -> Option<&Error> {
        match self {
            ClientError::Jpip(e) => Some(e),
            ClientError::Context(_) => None,
        }
    }
}
impl From<Error> for ClientError {
    fn from(e: Error) -> ClientError {
        ClientError::Jpip(Box::new(e))
    }
}
impl fmt::Display for ClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ClientError::Jpip(e) => e.fmt(f),
            ClientError::Context(e) => e.fmt(f),
        }
    }
}
impl std::error::Error for ClientError {}
