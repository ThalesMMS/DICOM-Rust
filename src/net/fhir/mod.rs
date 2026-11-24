#![forbid(unsafe_code)]
mod client;
mod imaging_study;
mod search_page;
mod smart;
use crate::net::ul::Kind as CtxKind;
pub use client::{AuthorizeFn, Client, ClientOptions, ImagingSearch, SmartGrant};
pub use imaging_study::{
    Coding, Identifier, ImagingInstance, ImagingSeries, ImagingStudy, ImagingStudyOptions,
    Reference, map_imaging_study,
};
pub use smart::{SMART_PATIENT, SMART_SYSTEM, SMART_USER, check_smart_scopes};
use std::fmt;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Endpoint,
    Options,
    Limit,
    PageLimit,
    PageLoop,
    Request,
    Auth,
    Response,
    Transport,
    ImagingIdentity,
    SmartScopeDenied,
    SmartTarget,
    HttpStatus(i64),
    Context(CtxKind),
}
impl Error {
    pub fn is_limit(&self) -> bool {
        matches!(self, Error::Limit | Error::PageLimit)
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Error::Endpoint => "fhir: invalid or out-of-scope endpoint",
            Error::Options => "fhir: invalid client options",
            Error::Limit => "fhir: resource limit exceeded",
            Error::PageLimit => "fhir: resource limit exceeded: page count exceeded",
            Error::PageLoop => "fhir: repeated search page",
            Error::Request => "fhir: invalid search request",
            Error::Auth => "fhir: authorization unavailable or out of scope",
            Error::Response => "fhir: invalid search response",
            Error::Transport => "fhir: HTTP exchange failed",
            Error::ImagingIdentity => "fhir: invalid DICOM imaging identity",
            Error::SmartScopeDenied => "fhir: SMART scope does not cover interaction",
            Error::SmartTarget => "fhir: unsupported SMART scope target",
            Error::HttpStatus(code) => return write!(f, "fhir: search returned HTTP {code}"),
            Error::Context(CtxKind::Canceled) => "context canceled",
            Error::Context(_) => "context deadline exceeded",
        })
    }
}
impl std::error::Error for Error {}
fn valid_resource_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id != "."
        && id != ".."
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.')
}
