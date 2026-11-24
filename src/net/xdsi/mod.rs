#![forbid(unsafe_code)]
mod client;
mod manifest;
mod metadata;
mod read;
mod wire;
use crate::dictionary::uid::{StorageCategory, lookup_storage_sop_class};
use crate::net::ul::{Ctx, Kind as CtxKind};
use crate::object::is_valid_uid;
pub use client::{
    AuthorizeFn, Client, ClientConfig, HoldFn, HttpOptions, InstanceResult, MappingFn,
    PublicationResult, RegistryError, Release, RetrievalResult,
};
pub use manifest::{Manifest, ManifestOptions, build_manifest, parse_manifest};
pub use metadata::{Code, PublicationMetadata};
use std::fmt;
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    Manifest,
    Identity,
    Limit,
    Protocol,
    Mapping,
    Authorization(Option<Box<Error>>),
    Availability,
    Context(CtxKind),
    PeerStatus(i64),
    Transport { timeout: bool, canceled: bool },
    Other(String),
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Manifest => f.write_str("xdsi: invalid or unsupported KOS manifest"),
            Error::Identity => f.write_str("xdsi: document identity differs from the manifest"),
            Error::Limit => f.write_str("xdsi: resource limit exceeded"),
            Error::Protocol => f.write_str("xdsi: invalid transaction response"),
            Error::Mapping => f.write_str("xdsi: patient or imaging source mapping missing"),
            Error::Authorization(None) => f.write_str("xdsi: transaction not authorized"),
            Error::Authorization(Some(inner)) => {
                write!(f, "xdsi: transaction not authorized: {inner}")
            }
            Error::Availability => {
                f.write_str("xdsi: imaging instance availability not established")
            }
            Error::Context(CtxKind::Canceled) => f.write_str("context canceled"),
            Error::Context(_) => f.write_str("context deadline exceeded"),
            Error::PeerStatus(code) => write!(f, "xdsi: peer HTTP status {code}"),
            Error::Transport { timeout: true, .. } => f.write_str("xdsi: HTTP exchange timed out"),
            Error::Transport { canceled: true, .. } => {
                f.write_str("xdsi: HTTP exchange: context canceled")
            }
            Error::Transport { .. } => f.write_str("xdsi: HTTP exchange failed"),
            Error::Other(text) => f.write_str(text),
        }
    }
}
impl std::error::Error for Error {}
impl Error {
    pub fn is(&self, target: &Error) -> bool {
        match (self, target) {
            (Error::Authorization(_), Error::Authorization(None)) => true,
            (Error::Authorization(Some(inner)), _) => inner.is(target),
            _ => self == target,
        }
    }
}
fn ctx_err(ctx: &Ctx) -> Result<(), Error> {
    match ctx.err() {
        Some(kind) => Err(Error::Context(kind)),
        None => Ok(()),
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Operation {
    Publish,
    Retrieve,
}
impl Operation {
    pub fn as_str(self) -> &'static str {
        match self {
            Operation::Publish => "RAD-68",
            Operation::Retrieve => "RAD-69",
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Limits {
    pub document_bytes: i64,
    pub total_bytes: i64,
    pub metadata_bytes: i64,
}
fn normalize_limits(mut l: Limits) -> Result<Limits, Error> {
    if l.document_bytes == 0 {
        l.document_bytes = 64 << 20;
    }
    if l.total_bytes == 0 {
        l.total_bytes = 128 << 20;
    }
    if l.metadata_bytes == 0 {
        l.metadata_bytes = 4 << 20;
    }
    if l.document_bytes < 1
        || l.document_bytes > 1 << 30
        || l.total_bytes < l.document_bytes
        || l.total_bytes > 1 << 30
        || l.metadata_bytes < 1
        || l.metadata_bytes > l.document_bytes
    {
        return Err(Error::Limit);
    }
    Ok(l)
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct PatientId {
    pub id: String,
    pub issuer_oid: String,
}
impl PatientId {
    fn valid(&self) -> bool {
        self.id.len() <= 64
            && !self.id.is_empty()
            && trim_unicode_space(&self.id) == self.id
            && !self.id.contains(['^', '~', '\\', '&', '|'])
            && is_printable_ascii(&self.id)
            && is_valid_uid(self.issuer_oid.as_bytes())
    }
    fn cx(&self) -> String {
        format!("{}^^^&{}&ISO", self.id, self.issuer_oid)
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct InstanceRef {
    pub study_uid: String,
    pub series_uid: String,
    pub sop_class_uid: String,
    pub sop_instance_uid: String,
}
impl InstanceRef {
    fn valid(&self) -> bool {
        lookup_storage_sop_class(&self.sop_class_uid)
            .is_some_and(|c| c.category == StorageCategory::Image && !c.retired)
            && is_valid_uid(self.study_uid.as_bytes())
            && is_valid_uid(self.series_uid.as_bytes())
            && is_valid_uid(self.sop_instance_uid.as_bytes())
    }
}
fn is_printable_ascii(text: &str) -> bool {
    text.bytes().all(|c| (32..=126).contains(&c))
}
fn trim_unicode_space(s: &str) -> &str {
    crate::net::dicomweb::mimeheader::trim_space(s)
}
