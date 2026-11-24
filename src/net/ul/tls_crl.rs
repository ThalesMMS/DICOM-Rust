#![forbid(unsafe_code)]
use super::error::{Error, Result, TlsRevocationError, TlsRevocationKind as Kind};
use rustls::pki_types::SignatureVerificationAlgorithm;
use std::collections::HashSet;
use std::time::SystemTime;
use x509_parser::prelude::{CertificateRevocationList, FromDer, X509Certificate};
pub const MAX_TLS_CRL_LISTS: usize = 64;
pub const MAX_TLS_CRL_BYTES: usize = 16 << 20;
pub const MAX_TLS_CRL_ENTRIES: usize = 100_000;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TlsCrlMode {
    #[default]
    None,
    Leaf,
    Chain,
    Other(u8),
}
#[derive(Debug, Clone, Default)]
pub struct TlsRevocationOptions {
    pub mode: TlsCrlMode,
    pub crls: Vec<Vec<u8>>,
}
fn failure(kind: Kind, list: i32) -> Error {
    TlsRevocationError::new(kind, list).into()
}
#[derive(Debug, Clone, Copy)]
pub(crate) struct Raw<'a> {
    pub class: u8,
    pub tag: u32,
    pub compound: bool,
    pub bytes: &'a [u8],
}
const TAG_INTEGER: u32 = 2;
const TAG_BIT_STRING: u32 = 3;
const TAG_SEQUENCE: u32 = 16;
const TAG_UTC_TIME: u32 = 23;
const TAG_GENERALIZED_TIME: u32 = 24;
pub(crate) fn raw_der_field(data: &[u8]) -> Option<(Raw<'_>, &[u8])> {
    let mut offset = 0usize;
    let b = *data.first()?;
    offset += 1;
    let class = b >> 6;
    let compound = b & 0x20 == 0x20;
    let mut tag = u32::from(b & 0x1f);
    if tag == 0x1f {
        let mut value: u64 = 0;
        let mut shifted = 0;
        loop {
            if shifted == 5 {
                return None;
            }
            let c = *data.get(offset)?;
            if shifted == 0 && c == 0x80 {
                return None;
            }
            value = (value << 7) | u64::from(c & 0x7f);
            offset += 1;
            shifted += 1;
            if c & 0x80 == 0 {
                break;
            }
        }
        if value > i32::MAX as u64 {
            return None;
        }
        tag = value as u32;
        if tag < 0x1f {
            return None;
        }
    }
    let b = *data.get(offset)?;
    offset += 1;
    let length: usize = if b & 0x80 == 0 {
        usize::from(b & 0x7f)
    } else {
        let count = usize::from(b & 0x7f);
        if count == 0 {
            return None;
        }
        let mut value: usize = 0;
        for _ in 0..count {
            let c = *data.get(offset)?;
            offset += 1;
            if value >= 1 << 23 {
                return None;
            }
            value = (value << 8) | usize::from(c);
            if value == 0 {
                return None;
            }
        }
        if value < 0x80 {
            return None;
        }
        value
    };
    let end = offset.checked_add(length)?;
    if end > data.len() {
        return None;
    }
    Some((
        Raw {
            class,
            tag,
            compound,
            bytes: &data[offset..end],
        },
        &data[end..],
    ))
}
fn universal(value: &Raw<'_>, tag: u32, compound: bool) -> bool {
    value.class == 0 && value.tag == tag && value.compound == compound
}
fn time_der(value: &Raw<'_>) -> bool {
    value.class == 0
        && !value.compound
        && (value.tag == TAG_UTC_TIME || value.tag == TAG_GENERALIZED_TIME)
}
fn count_der_entries(raw: &[u8], remaining: usize) -> std::result::Result<usize, Kind> {
    let invalid = Err(Kind::InvalidMaterial);
    let Some((envelope, tail)) = raw_der_field(raw) else {
        return invalid;
    };
    if !tail.is_empty() || !universal(&envelope, TAG_SEQUENCE, true) {
        return invalid;
    }
    let Some((tbs, outer)) = raw_der_field(envelope.bytes) else {
        return invalid;
    };
    if !universal(&tbs, TAG_SEQUENCE, true) {
        return invalid;
    }
    let Some((algorithm, outer)) = raw_der_field(outer) else {
        return invalid;
    };
    if !universal(&algorithm, TAG_SEQUENCE, true) {
        return invalid;
    }
    let Some((signature, outer)) = raw_der_field(outer) else {
        return invalid;
    };
    if !outer.is_empty() || !universal(&signature, TAG_BIT_STRING, false) {
        return invalid;
    }
    let mut data = tbs.bytes;
    let Some((field, rest)) = raw_der_field(data) else {
        return invalid;
    };
    if universal(&field, TAG_INTEGER, false) {
        data = rest;
    }
    for _ in 0..2 {
        let Some((field, rest)) = raw_der_field(data) else {
            return invalid;
        };
        if !universal(&field, TAG_SEQUENCE, true) {
            return invalid;
        }
        data = rest;
    }
    let Some((field, rest)) = raw_der_field(data) else {
        return invalid;
    };
    if !time_der(&field) {
        return invalid;
    }
    data = rest;
    if !data.is_empty() {
        let Some((field, rest)) = raw_der_field(data) else {
            return invalid;
        };
        if time_der(&field) {
            data = rest;
        }
    }
    let mut count = 0usize;
    if !data.is_empty() {
        let Some((field, rest)) = raw_der_field(data) else {
            return invalid;
        };
        if universal(&field, TAG_SEQUENCE, true) {
            let mut entries = field.bytes;
            while !entries.is_empty() {
                let Some((entry, next)) = raw_der_field(entries) else {
                    return invalid;
                };
                if !universal(&entry, TAG_SEQUENCE, true) {
                    return invalid;
                }
                count += 1;
                if count > remaining {
                    return Err(Kind::Limit);
                }
                entries = next;
            }
            data = rest;
        }
    }
    if !data.is_empty() {
        let Some((field, rest)) = raw_der_field(data) else {
            return invalid;
        };
        if field.class != 2 || field.tag != 0 || !field.compound || !rest.is_empty() {
            return invalid;
        }
    }
    Ok(count)
}
pub fn preflight_tls_crl_der<T: AsRef<[u8]>>(lists: &[T]) -> Result<()> {
    if lists.len() > MAX_TLS_CRL_LISTS {
        return Err(failure(Kind::Limit, -1));
    }
    let mut bytes_total = 0usize;
    for (i, raw) in lists.iter().enumerate() {
        let raw = raw.as_ref();
        if raw.is_empty() {
            return Err(failure(Kind::InvalidMaterial, i as i32));
        }
        if raw.len() > MAX_TLS_CRL_BYTES - bytes_total {
            return Err(failure(Kind::Limit, i as i32));
        }
        bytes_total += raw.len();
    }
    let mut entries_total = 0usize;
    for (i, raw) in lists.iter().enumerate() {
        match count_der_entries(raw.as_ref(), MAX_TLS_CRL_ENTRIES - entries_total) {
            Ok(count) => entries_total += count,
            Err(kind) => return Err(failure(kind, i as i32)),
        }
    }
    Ok(())
}
const OID_AKI: &str = "2.5.29.35";
const OID_CRL_NUMBER: &str = "2.5.29.20";
const OID_DELTA: &str = "2.5.29.27";
const OID_IDP: &str = "2.5.29.28";
const OID_CERTIFICATE_ISSUER: &str = "2.5.29.29";
const OID_REASON: &str = "2.5.29.21";
fn parse_authority_key_id(der: &[u8]) -> std::result::Result<Vec<u8>, Kind> {
    let Some((sequence, rest)) = raw_der_field(der) else {
        return Err(Kind::InvalidMaterial);
    };
    if !rest.is_empty() || !universal(&sequence, TAG_SEQUENCE, true) {
        return Err(Kind::InvalidMaterial);
    }
    let mut identifier: Option<Vec<u8>> = None;
    let mut data = sequence.bytes;
    while !data.is_empty() {
        let Some((field, remaining)) = raw_der_field(data) else {
            return Err(Kind::InvalidMaterial);
        };
        if field.class != 2 {
            return Err(Kind::InvalidMaterial);
        }
        if field.tag == 1 || field.tag == 2 {
            return Err(Kind::Unsupported);
        }
        if field.tag != 0 || field.compound || field.bytes.is_empty() || identifier.is_some() {
            return Err(Kind::InvalidMaterial);
        }
        identifier = Some(field.bytes.to_vec());
        data = remaining;
    }
    identifier.ok_or(Kind::InvalidMaterial)
}
fn serial_magnitude(raw: &[u8]) -> Option<Vec<u8>> {
    if raw.first().is_some_and(|b| b & 0x80 != 0) {
        return None;
    }
    let start = raw.iter().position(|b| *b != 0).unwrap_or(raw.len());
    Some(raw[start..].to_vec())
}
#[derive(Debug, Clone)]
struct List {
    raw_issuer: Vec<u8>,
    authority_key_id: Vec<u8>,
    this_update: i64,
    next_update: i64,
    tbs: Vec<u8>,
    signature_algorithm: Vec<u8>,
    signature: Vec<u8>,
    serials: HashSet<Vec<u8>>,
}
#[derive(Debug, Clone)]
pub(crate) struct CertInfo {
    pub raw_issuer: Vec<u8>,
    pub raw_subject: Vec<u8>,
    pub serial: Option<Vec<u8>>,
    pub subject_key_id: Vec<u8>,
    pub key_algorithm: Vec<u8>,
    pub key: Vec<u8>,
    pub may_sign_crl: bool,
}
impl CertInfo {
    pub(crate) fn parse(der: &[u8]) -> Option<CertInfo> {
        let (rest, cert) = X509Certificate::from_der(der).ok()?;
        if !rest.is_empty() {
            return None;
        }
        let spki = cert.public_key().raw;
        let (spki_seq, _) = raw_der_field(spki)?;
        let (algorithm, after) = raw_der_field(spki_seq.bytes)?;
        let (key_bits, _) = raw_der_field(after)?;
        let key = key_bits.bytes.get(1..)?.to_vec();
        let mut subject_key_id = Vec::new();
        let mut basic: Option<bool> = None;
        let mut key_usage: Option<bool> = None;
        for ext in cert.extensions() {
            use x509_parser::extensions::ParsedExtension;
            match ext.parsed_extension() {
                ParsedExtension::SubjectKeyIdentifier(id) => subject_key_id = id.0.to_vec(),
                ParsedExtension::BasicConstraints(bc) => basic = Some(bc.ca),
                ParsedExtension::KeyUsage(ku) => key_usage = Some(ku.crl_sign()),
                _ => {}
            }
        }
        let version3 = cert.version().0 == 2;
        let ca_ok = match basic {
            Some(ca) => ca,
            None => !version3,
        };
        Some(CertInfo {
            raw_issuer: cert.issuer().as_raw().to_vec(),
            raw_subject: cert.subject().as_raw().to_vec(),
            serial: serial_magnitude(cert.raw_serial()),
            subject_key_id,
            key_algorithm: algorithm.bytes.to_vec(),
            key,
            may_sign_crl: ca_ok && key_usage.unwrap_or(true),
        })
    }
}
#[derive(Debug, Clone, Default)]
pub struct TlsRevocationSet {
    lists: Vec<List>,
}
impl TlsRevocationSet {
    pub fn new<T: AsRef<[u8]>>(crls: &[T]) -> Result<TlsRevocationSet> {
        if crls.len() > MAX_TLS_CRL_LISTS {
            return Err(failure(Kind::Limit, -1));
        }
        let mut bytes_total = 0usize;
        for (i, supplied) in crls.iter().enumerate() {
            let raw = supplied.as_ref();
            if raw.is_empty() {
                return Err(failure(Kind::InvalidMaterial, i as i32));
            }
            if raw.len() > MAX_TLS_CRL_BYTES - bytes_total {
                return Err(failure(Kind::Limit, i as i32));
            }
            bytes_total += raw.len();
        }
        preflight_tls_crl_der(crls)?;
        let mut set = TlsRevocationSet::default();
        for (i, raw) in crls.iter().enumerate() {
            let raw = raw.as_ref();
            let list = compile_list(raw).map_err(|kind| failure(kind, i as i32))?;
            set.lists.push(list);
        }
        Ok(set)
    }
    pub fn preflight(&self, at: SystemTime) -> Result<()> {
        if self.lists.is_empty() {
            return Err(failure(Kind::Missing, -1));
        }
        let at = unix_nanos(at);
        for l in &self.lists {
            if at >= nanos(l.this_update) && at < nanos(l.next_update) {
                return Ok(());
            }
        }
        if at < nanos(self.lists[0].this_update) {
            return Err(failure(Kind::NotYetValid, 0));
        }
        Err(failure(Kind::Expired, 0))
    }
    pub(crate) fn check(
        &self,
        certificate: &CertInfo,
        issuer: &CertInfo,
        at: SystemTime,
        algorithms: &[&dyn SignatureVerificationAlgorithm],
    ) -> std::result::Result<(), TlsRevocationError> {
        let fail = |kind, list| Err(TlsRevocationError::new(kind, list));
        let Some(serial) = &certificate.serial else {
            return fail(Kind::IssuerMismatch, -1);
        };
        if certificate.raw_issuer != issuer.raw_subject {
            return fail(Kind::IssuerMismatch, -1);
        }
        if self.lists.is_empty() {
            return fail(Kind::Missing, -1);
        }
        let at = unix_nanos(at);
        let mut current = false;
        let mut reason = TlsRevocationError::new(Kind::IssuerMismatch, -1);
        for (i, l) in self.lists.iter().enumerate() {
            let i = i as i32;
            if l.raw_issuer != issuer.raw_subject {
                continue;
            }
            if !l.authority_key_id.is_empty()
                && !issuer.subject_key_id.is_empty()
                && l.authority_key_id != issuer.subject_key_id
            {
                continue;
            }
            if !signature_valid(l, issuer, algorithms) {
                reason = TlsRevocationError::new(Kind::BadSignature, i);
                continue;
            }
            if at < nanos(l.this_update) {
                reason = TlsRevocationError::new(Kind::NotYetValid, i);
                continue;
            }
            if at >= nanos(l.next_update) {
                reason = TlsRevocationError::new(Kind::Expired, i);
                continue;
            }
            current = true;
            if l.serials.contains(serial) {
                return fail(Kind::Revoked, i);
            }
        }
        if current { Ok(()) } else { Err(reason) }
    }
    pub(crate) fn check_chains(
        &self,
        chains: &[Vec<CertInfo>],
        all: bool,
        at: SystemTime,
        algorithms: &[&dyn SignatureVerificationAlgorithm],
    ) -> std::result::Result<(), TlsRevocationError> {
        let mut first: Option<TlsRevocationError> = None;
        for (ci, chain) in chains.iter().enumerate() {
            let mut end = 1usize;
            if all && chain.len().saturating_sub(1) > end {
                end = chain.len() - 1;
            }
            if chain.is_empty() {
                first.get_or_insert(TlsRevocationError::new(Kind::Missing, -1));
                continue;
            }
            let mut failed = None;
            for i in 0..end {
                let issuer = chain.get(i + 1).unwrap_or(&chain[i]);
                if let Err(mut e) = self.check(&chain[i], issuer, at, algorithms) {
                    e.chain_index = ci as i32;
                    e.certificate_index = i as i32;
                    failed = Some(e);
                    break;
                }
            }
            match failed {
                None => return Ok(()),
                Some(e) => {
                    first.get_or_insert(e);
                }
            }
        }
        Err(first.unwrap_or(TlsRevocationError::new(Kind::Missing, -1)))
    }
}
fn nanos(seconds: i64) -> i128 {
    i128::from(seconds) * 1_000_000_000
}
fn unix_nanos(at: SystemTime) -> i128 {
    match at.duration_since(SystemTime::UNIX_EPOCH) {
        Ok(d) => d.as_nanos() as i128,
        Err(e) => -(e.duration().as_nanos() as i128),
    }
}
fn signature_valid(
    list: &List,
    issuer: &CertInfo,
    algorithms: &[&dyn SignatureVerificationAlgorithm],
) -> bool {
    if !issuer.may_sign_crl {
        return false;
    }
    algorithms.iter().any(|alg| {
        alg.signature_alg_id().as_ref() == list.signature_algorithm.as_slice()
            && alg.public_key_alg_id().as_ref() == issuer.key_algorithm.as_slice()
            && alg
                .verify_signature(&issuer.key, &list.tbs, &list.signature)
                .is_ok()
    })
}
fn compile_list(raw: &[u8]) -> std::result::Result<List, Kind> {
    let Ok((rest, crl)) = CertificateRevocationList::from_der(raw) else {
        return Err(Kind::InvalidMaterial);
    };
    if !rest.is_empty() {
        return Err(Kind::InvalidMaterial);
    }
    let tbs = &crl.tbs_cert_list;
    let this_update = tbs.this_update.timestamp();
    let Some(next_update) = tbs.next_update.map(|t| t.timestamp()) else {
        return Err(Kind::InvalidMaterial);
    };
    if this_update >= next_update {
        return Err(Kind::InvalidMaterial);
    }
    let mut authority_key_id = Vec::new();
    let mut seen = HashSet::new();
    for ext in tbs.extensions() {
        let id = ext.oid.to_id_string();
        if !seen.insert(id.clone()) {
            return Err(Kind::InvalidMaterial);
        }
        if id == OID_AKI {
            authority_key_id = parse_authority_key_id(ext.value)?;
        }
        if id == OID_DELTA || id == OID_IDP {
            return Err(Kind::Unsupported);
        }
        if ext.critical && id != OID_AKI && id != OID_CRL_NUMBER {
            return Err(Kind::Unsupported);
        }
    }
    let mut serials = HashSet::with_capacity(tbs.revoked_certificates.len());
    for entry in &tbs.revoked_certificates {
        let Some(serial) = serial_magnitude(entry.raw_serial()) else {
            return Err(Kind::InvalidMaterial);
        };
        let reason = entry.reason_code().map(|(_, code)| code.0).unwrap_or(0);
        if reason == 8 {
            return Err(Kind::Unsupported);
        }
        if reason == 7 || reason > 10 {
            return Err(Kind::InvalidMaterial);
        }
        let mut seen = HashSet::new();
        for ext in entry.extensions() {
            let id = ext.oid.to_id_string();
            if !seen.insert(id.clone()) {
                return Err(Kind::InvalidMaterial);
            }
            if id == OID_CERTIFICATE_ISSUER || (ext.critical && id != OID_REASON) {
                return Err(Kind::Unsupported);
            }
        }
        serials.insert(serial);
    }
    let invalid = Kind::InvalidMaterial;
    let (envelope, _) = raw_der_field(raw).ok_or(invalid)?;
    let (tbs_field, outer) = raw_der_field(envelope.bytes).ok_or(invalid)?;
    let tbs_len = envelope.bytes.len() - outer.len();
    let _ = tbs_field;
    let (algorithm, outer) = raw_der_field(outer).ok_or(invalid)?;
    let (signature, _) = raw_der_field(outer).ok_or(invalid)?;
    Ok(List {
        raw_issuer: tbs.issuer.as_raw().to_vec(),
        authority_key_id,
        this_update,
        next_update,
        tbs: envelope.bytes[..tbs_len].to_vec(),
        signature_algorithm: algorithm.bytes.to_vec(),
        signature: signature.bytes.get(1..).ok_or(invalid)?.to_vec(),
        serials,
    })
}
pub fn validate_tls_crls<T: AsRef<[u8]>>(crls: &[T], at: SystemTime) -> Result<()> {
    TlsRevocationSet::new(crls)?.preflight(at)
}
