use super::asn1;
use super::der::{self, TAG_BIT_STRING, TAG_INTEGER, TAG_OID, TAG_SEQUENCE, Tlv};
use super::hash::MacAlgorithm;
use super::pubkey;
use super::timestamp::{
    TimestampInfo, TimestampOptions, TimestampRequestOptions, add_timestamp, new_timestamp_request,
    token_from_response, verify_timestamp,
};
use super::{Error, Kind, PublicKey, Signature, signatures, verify_signature};
use crate::object::Object;
use chrono::{DateTime, FixedOffset, SecondsFormat, TimeZone, Utc};
use num_bigint::BigInt;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct X509Certificate {
    pub raw: Vec<u8>,
    raw_tbs: Vec<u8>,
    raw_subject: Vec<u8>,
    raw_issuer: Vec<u8>,
    raw_spki: Vec<u8>,
    pub serial_number: BigInt,
    pub not_before: DateTime<Utc>,
    pub not_after: DateTime<Utc>,
    pub subject: Name,
    public_key: PublicKey,
    signature_algorithm: Vec<u32>,
    signature: Vec<u8>,
    version: i64,
    subject_key_id: Vec<u8>,
    authority_key_id: Vec<u8>,
    basic_constraints_valid: bool,
    is_ca: bool,
    max_path_len: i64,
    pub key_usage: u32,
    ext_key_usage: Vec<i32>,
    unknown_ext_key_usage: usize,
    unhandled_critical: bool,
    subject_alt_name: Option<Vec<u8>>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Name {
    pub common_name: String,
    pub serial_number: String,
    pub country: Vec<String>,
    pub organization: Vec<String>,
    pub organizational_unit: Vec<String>,
    pub locality: Vec<String>,
    pub province: Vec<String>,
    pub street_address: Vec<String>,
    pub postal_code: Vec<String>,
    names: Vec<(Vec<u32>, AttributeValue)>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
enum AttributeValue {
    Text(String),
    Other(Vec<u8>),
}
const OID_CN: &[u32] = &[2, 5, 4, 3];
const OID_SERIAL: &[u32] = &[2, 5, 4, 5];
const OID_C: &[u32] = &[2, 5, 4, 6];
const OID_L: &[u32] = &[2, 5, 4, 7];
const OID_ST: &[u32] = &[2, 5, 4, 8];
const OID_STREET: &[u32] = &[2, 5, 4, 9];
const OID_O: &[u32] = &[2, 5, 4, 10];
const OID_OU: &[u32] = &[2, 5, 4, 11];
const OID_POSTAL: &[u32] = &[2, 5, 4, 17];
fn type_name(oid: &[u32]) -> Option<&'static str> {
    Some(match oid {
        OID_C => "C",
        OID_O => "O",
        OID_OU => "OU",
        OID_CN => "CN",
        OID_SERIAL => "SERIALNUMBER",
        OID_L => "L",
        OID_ST => "ST",
        OID_STREET => "STREET",
        OID_POSTAL => "POSTALCODE",
        _ => return None,
    })
}
fn oid_string(oid: &[u32]) -> String {
    oid.iter().map(u32::to_string).collect::<Vec<_>>().join(".")
}
impl Name {
    fn parse(tlv: &Tlv<'_>) -> Option<Name> {
        let mut name = Name::default();
        for set in der::sequence(tlv)? {
            if set.tag != 0x31 {
                return None;
            }
            for atv in der::children(set.body)? {
                let [kind, value] = der::sequence(&atv)?.try_into().ok()?;
                if kind.tag != TAG_OID {
                    return None;
                }
                let oid = asn1::oid(kind.body).ok()?.0;
                let text = match value.tag {
                    0x0C | 0x13 | 0x16 | 0x14 | 0x12 => {
                        Some(String::from_utf8_lossy(value.body).into_owned())
                    }
                    0x1E => Some(
                        value
                            .body
                            .as_chunks::<2>()
                            .0
                            .iter()
                            .map(|c| u16::from_be_bytes([c[0], c[1]]))
                            .map(|u| char::from_u32(u32::from(u)).unwrap_or('\u{FFFD}'))
                            .collect(),
                    ),
                    _ => None,
                };
                match &text {
                    Some(t) => {
                        match oid.as_slice() {
                            OID_C => name.country.push(t.clone()),
                            OID_O => name.organization.push(t.clone()),
                            OID_OU => name.organizational_unit.push(t.clone()),
                            OID_CN => name.common_name = t.clone(),
                            OID_SERIAL => name.serial_number = t.clone(),
                            OID_L => name.locality.push(t.clone()),
                            OID_ST => name.province.push(t.clone()),
                            OID_STREET => name.street_address.push(t.clone()),
                            OID_POSTAL => name.postal_code.push(t.clone()),
                            _ => {}
                        }
                        name.names.push((oid, AttributeValue::Text(t.clone())));
                    }
                    None => name
                        .names
                        .push((oid, AttributeValue::Other(value.full.to_vec()))),
                }
            }
        }
        Some(name)
    }
    pub fn string(&self) -> String {
        let mut rdns: Vec<Vec<(Vec<u32>, AttributeValue)>> = Vec::new();
        for (oid, value) in &self.names {
            let parsed = oid.len() == 4
                && oid[..3] == [2, 5, 4]
                && matches!(oid[3], 3 | 6 | 7 | 8 | 9 | 10 | 11 | 17);
            if !parsed {
                rdns.push(vec![(oid.clone(), value.clone())]);
            }
        }
        let mut append = |values: &[String], oid: &[u32]| {
            if !values.is_empty() {
                rdns.push(
                    values
                        .iter()
                        .map(|v| (oid.to_vec(), AttributeValue::Text(v.clone())))
                        .collect(),
                );
            }
        };
        append(&self.country, OID_C);
        append(&self.province, OID_ST);
        append(&self.locality, OID_L);
        append(&self.street_address, OID_STREET);
        append(&self.postal_code, OID_POSTAL);
        append(&self.organization, OID_O);
        append(&self.organizational_unit, OID_OU);
        if !self.common_name.is_empty() {
            append(std::slice::from_ref(&self.common_name), OID_CN);
        }
        if !self.serial_number.is_empty() {
            append(std::slice::from_ref(&self.serial_number), OID_SERIAL);
        }
        let mut s = String::new();
        for (i, rdn) in rdns.iter().rev().enumerate() {
            if i > 0 {
                s.push(',');
            }
            for (j, (oid, value)) in rdn.iter().enumerate() {
                if j > 0 {
                    s.push('+');
                }
                let (Some(type_name), AttributeValue::Text(text)) = (type_name(oid), value) else {
                    let der = match value {
                        AttributeValue::Text(t) => marshal_string(t),
                        AttributeValue::Other(der) => der.clone(),
                    };
                    s.push_str(&format!("{}=#{}", oid_string(oid), hex(&der)));
                    continue;
                };
                s.push_str(type_name);
                s.push('=');
                let chars: Vec<char> = text.chars().collect();
                let last = text.len().saturating_sub(1);
                let mut at = 0;
                for c in chars {
                    let escape = match c {
                        ',' | '+' | '"' | '\\' | '<' | '>' | ';' => true,
                        ' ' => at == 0 || at == last,
                        '#' => at == 0,
                        _ => false,
                    };
                    if escape {
                        s.push('\\');
                    }
                    s.push(c);
                    at += c.len_utf8();
                }
            }
        }
        s
    }
}
fn hex(data: &[u8]) -> String {
    data.iter().map(|b| format!("{b:02x}")).collect()
}
fn marshal_string(s: &str) -> Vec<u8> {
    let printable = s.bytes().all(|b| {
        b.is_ascii_alphanumeric() || b" '()+,-./:=?".contains(&b) || b == b'*' || b == b'&'
    });
    let mut out = vec![if printable { 0x13 } else { 0x0C }];
    let len = s.len();
    if len < 0x80 {
        out.push(len as u8);
    } else {
        let bytes: Vec<u8> = len
            .to_be_bytes()
            .iter()
            .copied()
            .skip_while(|&b| b == 0)
            .collect();
        out.push(0x80 | bytes.len() as u8);
        out.extend(bytes);
    }
    out.extend_from_slice(s.as_bytes());
    out
}
fn time(tlv: &Tlv<'_>) -> Option<DateTime<Utc>> {
    let text = std::str::from_utf8(tlv.body).ok()?.strip_suffix('Z')?;
    if !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let (year, rest) = match (tlv.tag, text.len()) {
        (0x17, 12) => {
            let year: i32 = text[..2].parse().ok()?;
            (
                if year >= 50 { 1900 + year } else { 2000 + year },
                &text[2..],
            )
        }
        (0x18, 14) => (text[..4].parse().ok()?, &text[4..]),
        _ => return None,
    };
    let n = |i: usize| rest[i..i + 2].parse::<u32>().ok();
    Utc.with_ymd_and_hms(year, n(0)?, n(2)?, n(4)?, n(6)?, n(8)?)
        .single()
}
const OID_EXT_SUBJECT_KEY_ID: &[u32] = &[2, 5, 29, 14];
const OID_EXT_KEY_USAGE: &[u32] = &[2, 5, 29, 15];
const OID_EXT_SAN: &[u32] = &[2, 5, 29, 17];
const OID_EXT_BASIC_CONSTRAINTS: &[u32] = &[2, 5, 29, 19];
const OID_EXT_AUTHORITY_KEY_ID: &[u32] = &[2, 5, 29, 35];
const OID_EXT_EXT_KEY_USAGE: &[u32] = &[2, 5, 29, 37];
const EXT_KEY_USAGES: &[(&[u32], i32)] = &[
    (&[2, 5, 29, 37, 0], 0),
    (&[1, 3, 6, 1, 5, 5, 7, 3, 1], 1),
    (&[1, 3, 6, 1, 5, 5, 7, 3, 2], 2),
    (&[1, 3, 6, 1, 5, 5, 7, 3, 3], 3),
    (&[1, 3, 6, 1, 5, 5, 7, 3, 4], 4),
    (&[1, 3, 6, 1, 5, 5, 7, 3, 5], 5),
    (&[1, 3, 6, 1, 5, 5, 7, 3, 6], 6),
    (&[1, 3, 6, 1, 5, 5, 7, 3, 7], 7),
    (&[1, 3, 6, 1, 5, 5, 7, 3, 8], 8),
    (&[1, 3, 6, 1, 5, 5, 7, 3, 9], 9),
    (&[1, 3, 6, 1, 4, 1, 311, 10, 3, 3], 10),
    (&[2, 16, 840, 1, 113730, 4, 1], 11),
    (&[1, 3, 6, 1, 4, 1, 311, 2, 1, 22], 12),
    (&[1, 3, 6, 1, 4, 1, 311, 61, 1, 1], 13),
];
fn handled(oid: &[u32]) -> bool {
    matches!(
        oid,
        [
            2,
            5,
            29,
            14 | 15 | 17 | 19 | 30 | 31 | 32 | 33 | 35 | 36 | 37 | 54
        ] | [1, 3, 6, 1, 5, 5, 7, 1, 1]
    )
}
impl X509Certificate {
    pub fn parse(der_bytes: &[u8]) -> Option<X509Certificate> {
        let (outer, rest) = der::read(der_bytes)?;
        if !rest.is_empty() {
            return None;
        }
        let [tbs, algorithm, signature] = der::sequence(&outer)?.try_into().ok()?;
        if algorithm.tag != TAG_SEQUENCE || signature.tag != TAG_BIT_STRING {
            return None;
        }
        let (&0, signature) = signature.body.split_first()? else {
            return None;
        };
        let algorithm_oid = der::sequence(&algorithm)?
            .first()
            .map(|o| asn1::oid(o.body).ok())??
            .0;
        let mut fields = der::sequence(&tbs)?.into_iter().peekable();
        let mut version = 0;
        if let Some(v) = fields.next_if(|f| f.tag == 0xA0) {
            let (int, _) = der::read(v.body)?;
            version = i64::from(*int.body.last()?);
        }
        let serial = fields.next()?;
        if serial.tag != TAG_INTEGER {
            return None;
        }
        let _signature_field = fields.next()?;
        let issuer = fields.next()?;
        let validity = fields.next()?;
        let subject = fields.next()?;
        let spki = fields.next()?;
        let [not_before, not_after] = der::sequence(&validity)?.try_into().ok()?;
        let leaf = super::cert::parse_certificate(der_bytes)?;
        let mut cert = X509Certificate {
            raw: outer.full.to_vec(),
            raw_tbs: tbs.full.to_vec(),
            raw_subject: subject.full.to_vec(),
            raw_issuer: issuer.full.to_vec(),
            raw_spki: spki.full.to_vec(),
            serial_number: BigInt::from_signed_bytes_be(serial.body),
            not_before: time(&not_before)?,
            not_after: time(&not_after)?,
            subject: Name::parse(&subject)?,
            public_key: leaf.public_key,
            signature_algorithm: algorithm_oid,
            signature: signature.to_vec(),
            version: version + 1,
            subject_key_id: Vec::new(),
            authority_key_id: Vec::new(),
            basic_constraints_valid: false,
            is_ca: false,
            max_path_len: -1,
            key_usage: 0,
            ext_key_usage: Vec::new(),
            unknown_ext_key_usage: 0,
            unhandled_critical: false,
            subject_alt_name: None,
        };
        for ext in leaf.extensions {
            let oid = asn1::oid(&ext.id).ok()?.0;
            if ext.critical && !handled(&oid) {
                cert.unhandled_critical = true;
            }
            match oid.as_slice() {
                OID_EXT_SUBJECT_KEY_ID => {
                    let (id, _) = der::read(&ext.value)?;
                    cert.subject_key_id = id.body.to_vec();
                }
                OID_EXT_AUTHORITY_KEY_ID => {
                    let (seq, _) = der::read(&ext.value)?;
                    for item in der::children(seq.body)? {
                        if item.tag == 0x80 {
                            cert.authority_key_id = item.body.to_vec();
                        }
                    }
                }
                OID_EXT_KEY_USAGE => {
                    let (bits, _) = der::read(&ext.value)?;
                    let (_, data) = bits.body.split_first()?;
                    for i in 0..9 {
                        let byte = data.get(i / 8).copied().unwrap_or(0);
                        if byte & (0x80 >> (i % 8)) != 0 {
                            cert.key_usage |= 1 << i;
                        }
                    }
                }
                OID_EXT_BASIC_CONSTRAINTS => {
                    let (seq, _) = der::read(&ext.value)?;
                    cert.basic_constraints_valid = true;
                    let mut items = der::children(seq.body)?.into_iter().peekable();
                    if let Some(b) = items.next_if(|t| t.tag == 0x01) {
                        cert.is_ca = b.body.first().is_some_and(|&v| v != 0);
                    }
                    if let Some(n) = items.next().filter(|t| t.tag == TAG_INTEGER) {
                        cert.max_path_len = BigInt::from_signed_bytes_be(n.body)
                            .try_into()
                            .unwrap_or(-1);
                    }
                }
                OID_EXT_EXT_KEY_USAGE => {
                    let (seq, _) = der::read(&ext.value)?;
                    for item in der::children(seq.body)? {
                        let usage = asn1::oid(item.body).ok()?.0;
                        match EXT_KEY_USAGES.iter().find(|(o, _)| *o == usage.as_slice()) {
                            Some((_, v)) => cert.ext_key_usage.push(*v),
                            None => cert.unknown_ext_key_usage += 1,
                        }
                    }
                }
                OID_EXT_SAN => cert.subject_alt_name = Some(ext.value.clone()),
                _ => {}
            }
        }
        Some(cert)
    }
    fn hint_name(&self) -> String {
        if !self.subject.common_name.is_empty() {
            return self.subject.common_name.clone();
        }
        if let Some(o) = self.subject.organization.first() {
            return o.clone();
        }
        format!("serial:{}", self.serial_number)
    }
    fn check_signature_from(&self, parent: &X509Certificate) -> Result<(), X509Error> {
        check_signed_by(
            parent,
            1 << 5,
            &self.signature_algorithm,
            &self.raw_tbs,
            &self.signature,
        )
    }
    fn is_valid(
        &self,
        cert_type: CertType,
        chain: &[&X509Certificate],
        at: &DateTime<FixedOffset>,
    ) -> Result<(), X509Error> {
        if self.unhandled_critical {
            return Err(X509Error::UnhandledCritical);
        }
        if let Some(child) = chain.last()
            && child.raw_issuer != self.raw_subject
        {
            return Err(X509Error::Invalid(
                Box::new(self.clone()),
                REASON_NAME_MISMATCH,
                String::new(),
            ));
        }
        let format = |t: &DateTime<FixedOffset>| t.to_rfc3339_opts(SecondsFormat::Secs, true);
        let utc = |t: &DateTime<Utc>| t.to_rfc3339_opts(SecondsFormat::Secs, true);
        if *at < self.not_before {
            return Err(X509Error::Invalid(
                Box::new(self.clone()),
                REASON_EXPIRED,
                format!(
                    "current time {} is before {}",
                    format(at),
                    utc(&self.not_before)
                ),
            ));
        }
        if *at > self.not_after {
            return Err(X509Error::Invalid(
                Box::new(self.clone()),
                REASON_EXPIRED,
                format!(
                    "current time {} is after {}",
                    format(at),
                    utc(&self.not_after)
                ),
            ));
        }
        if cert_type == CertType::Intermediate && (!self.basic_constraints_valid || !self.is_ca) {
            return Err(X509Error::Invalid(
                Box::new(self.clone()),
                REASON_NOT_AUTHORIZED_TO_SIGN,
                String::new(),
            ));
        }
        if self.basic_constraints_valid && self.max_path_len >= 0 {
            let intermediates = chain.len() as i64 - 1;
            if intermediates > self.max_path_len {
                return Err(X509Error::Invalid(
                    Box::new(self.clone()),
                    REASON_TOO_MANY_INTERMEDIATES,
                    String::new(),
                ));
            }
        }
        Ok(())
    }
}
fn check_signed_by(
    parent: &X509Certificate,
    usage_bit: u32,
    algorithm: &[u32],
    tbs: &[u8],
    signature: &[u8],
) -> Result<(), X509Error> {
    if parent.version == 3 && !parent.basic_constraints_valid
        || parent.basic_constraints_valid && !parent.is_ca
    {
        return Err(X509Error::ConstraintViolation);
    }
    if parent.key_usage != 0 && parent.key_usage & usage_bit == 0 {
        return Err(X509Error::ConstraintViolation);
    }
    let (rsa, hash) = match algorithm {
        [1, 2, 840, 113549, 1, 1, 11] => (true, MacAlgorithm::Sha256),
        [1, 2, 840, 113549, 1, 1, 12] => (true, MacAlgorithm::Sha384),
        [1, 2, 840, 113549, 1, 1, 13] => (true, MacAlgorithm::Sha512),
        [1, 2, 840, 10045, 4, 3, 2] => (false, MacAlgorithm::Sha256),
        [1, 2, 840, 10045, 4, 3, 3] => (false, MacAlgorithm::Sha384),
        [1, 2, 840, 10045, 4, 3, 4] => (false, MacAlgorithm::Sha512),
        _ => {
            return Err(X509Error::Other(
                "x509: cannot verify signature: algorithm unimplemented".into(),
            ));
        }
    };
    let digest = hash.digest(tbs);
    match (&parent.public_key, rsa) {
        (PublicKey::Rsa { n, e }, true) => {
            if pubkey::rsa_verify(n, e, &hash.digest_info(&digest, true), signature) {
                Ok(())
            } else {
                Err(X509Error::Other("crypto/rsa: verification error".into()))
            }
        }
        (PublicKey::Ecdsa { curve, x, y }, false) => {
            if pubkey::ecdsa_verify(*curve, x, y, &digest, signature) {
                Ok(())
            } else {
                Err(X509Error::Other("x509: ECDSA verification failure".into()))
            }
        }
        (key, _) => Err(X509Error::Other(format!(
            "x509: signature algorithm specifies an {} public key, but have public key of type {}",
            if rsa { "RSA" } else { "ECDSA" },
            key.type_label()
        ))),
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum CertType {
    Leaf,
    Intermediate,
    Root,
}
pub const REASON_NOT_AUTHORIZED_TO_SIGN: u8 = 0;
pub const REASON_EXPIRED: u8 = 1;
pub const REASON_NAME_MISMATCH: u8 = 5;
pub const REASON_TOO_MANY_INTERMEDIATES: u8 = 3;
pub const REASON_INCOMPATIBLE_USAGE: u8 = 4;
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum X509Error {
    UnknownAuthority(Option<(String, String)>),
    Invalid(Box<X509Certificate>, u8, String),
    UnhandledCritical,
    ConstraintViolation,
    Other(String),
}
impl std::fmt::Display for X509Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            X509Error::UnknownAuthority(None) => {
                f.write_str("x509: certificate signed by unknown authority")
            }
            X509Error::UnknownAuthority(Some((err, name))) => write!(
                f,
                "x509: certificate signed by unknown authority (possibly because of {} while trying to verify candidate authority certificate {})",
                crate::textfmt::quote(err),
                crate::textfmt::quote(name)
            ),
            X509Error::Invalid(_, reason, detail) => f.write_str(&match *reason {
                REASON_NOT_AUTHORIZED_TO_SIGN => {
                    "x509: certificate is not authorized to sign other certificates".into()
                }
                REASON_EXPIRED => {
                    format!("x509: certificate has expired or is not yet valid: {detail}")
                }
                REASON_TOO_MANY_INTERMEDIATES => {
                    "x509: too many intermediates for path length constraint".into()
                }
                REASON_INCOMPATIBLE_USAGE => {
                    "x509: certificate specifies an incompatible key usage".into()
                }
                REASON_NAME_MISMATCH => {
                    "x509: issuer name does not match subject from issuing certificate".into()
                }
                _ => "x509: unknown error".into(),
            }),
            X509Error::UnhandledCritical => f.write_str("x509: unhandled critical extension"),
            X509Error::ConstraintViolation => f.write_str(
                "x509: invalid signature: parent certificate cannot sign this kind of certificate",
            ),
            X509Error::Other(text) => f.write_str(text),
        }
    }
}
impl std::error::Error for X509Error {}
impl X509Error {
    fn kinds(&self) -> Vec<Kind> {
        match self {
            X509Error::UnknownAuthority(_) => vec![Kind::X509UnknownAuthority],
            X509Error::Invalid(_, reason, _) => vec![Kind::X509CertificateInvalid(*reason)],
            X509Error::UnhandledCritical => vec![Kind::X509UnhandledCriticalExtension],
            _ => Vec::new(),
        }
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CertPool {
    certs: Vec<X509Certificate>,
}
impl CertPool {
    pub fn new() -> CertPool {
        CertPool::default()
    }
    pub fn add(&mut self, cert: X509Certificate) {
        if !self.certs.iter().any(|c| c.raw == cert.raw) {
            self.certs.push(cert);
        }
    }
    fn contains(&self, cert: &X509Certificate) -> bool {
        self.certs.iter().any(|c| c.raw == cert.raw)
    }
    fn potential_parents(&self, cert: &X509Certificate) -> Vec<&X509Certificate> {
        let (mut matching, mut one, mut mismatch) = (Vec::new(), Vec::new(), Vec::new());
        for c in self
            .certs
            .iter()
            .filter(|c| c.raw_subject == cert.raw_issuer)
        {
            if c.subject_key_id == cert.authority_key_id {
                matching.push(c);
            } else if c.subject_key_id.is_empty() != cert.authority_key_id.is_empty() {
                one.push(c);
            } else {
                mismatch.push(c);
            }
        }
        matching.extend(one);
        matching.extend(mismatch);
        matching
    }
}
fn already_in_chain(candidate: &X509Certificate, chain: &[&X509Certificate]) -> bool {
    chain.iter().any(|c| {
        c.raw_subject == candidate.raw_subject
            && c.raw_spki == candidate.raw_spki
            && c.subject_alt_name == candidate.subject_alt_name
    })
}
type Chain = Vec<X509Certificate>;
struct Builder<'a> {
    roots: &'a CertPool,
    intermediates: Option<&'a CertPool>,
    at: DateTime<FixedOffset>,
}
impl Builder<'_> {
    fn build(
        &self,
        cert: &X509Certificate,
        chain: &[&X509Certificate],
    ) -> Result<Vec<Chain>, X509Error> {
        let mut hint: Option<(String, String)> = None;
        let mut chains = Vec::new();
        let mut last_err: Option<X509Error> = None;
        let groups = [
            (CertType::Root, self.roots.potential_parents(cert)),
            (
                CertType::Intermediate,
                self.intermediates
                    .map(|p| p.potential_parents(cert))
                    .unwrap_or_default(),
            ),
        ];
        for (cert_type, candidates) in groups {
            for candidate in candidates {
                if matches!(candidate.public_key, PublicKey::Unknown)
                    || already_in_chain(candidate, chain)
                {
                    continue;
                }
                if let Err(e) = cert.check_signature_from(candidate) {
                    hint.get_or_insert_with(|| (e.to_string(), candidate.hint_name()));
                    continue;
                }
                if let Err(e) = candidate.is_valid(cert_type, chain, &self.at) {
                    hint.get_or_insert_with(|| (e.to_string(), candidate.hint_name()));
                    last_err = Some(e);
                    continue;
                }
                let mut next: Vec<&X509Certificate> = chain.to_vec();
                next.push(candidate);
                match cert_type {
                    CertType::Root => chains.push(next.iter().map(|c| (*c).clone()).collect()),
                    _ => match self.build(candidate, &next) {
                        Ok(more) => {
                            chains.extend(more);
                            last_err = None;
                        }
                        Err(e) => last_err = Some(e),
                    },
                }
            }
        }
        if !chains.is_empty() {
            return Ok(chains);
        }
        match last_err {
            Some(e) => Err(e),
            None => Err(X509Error::UnknownAuthority(hint)),
        }
    }
}
#[doc(hidden)]
pub fn verify_chain(
    leaf: &X509Certificate,
    roots: &CertPool,
    intermediates: Option<&CertPool>,
    at: DateTime<FixedOffset>,
    key_usages: &[i32],
) -> Result<Vec<Chain>, X509Error> {
    leaf.is_valid(CertType::Leaf, &[], &at)?;
    let chains = if roots.contains(leaf) {
        vec![vec![leaf.clone()]]
    } else {
        Builder {
            roots,
            intermediates,
            at,
        }
        .build(leaf, &[leaf])?
    };
    if key_usages.contains(&0) {
        return Ok(chains);
    }
    let usages: Vec<i32> = if key_usages.is_empty() {
        vec![1]
    } else {
        key_usages.to_vec()
    };
    let accepted: Vec<Chain> = chains
        .into_iter()
        .filter(|chain| chain_allows(chain, &usages))
        .collect();
    if accepted.is_empty() {
        return Err(X509Error::Invalid(
            Box::new(leaf.clone()),
            REASON_INCOMPATIBLE_USAGE,
            String::new(),
        ));
    }
    Ok(accepted)
}
fn chain_allows(chain: &[X509Certificate], requested: &[i32]) -> bool {
    let mut usages: Vec<i32> = requested.to_vec();
    let mut remaining = usages.len();
    for cert in chain.iter().rev() {
        if cert.ext_key_usage.is_empty() && cert.unknown_ext_key_usage == 0 {
            continue;
        }
        if cert.ext_key_usage.contains(&0) {
            continue;
        }
        for usage in usages.iter_mut() {
            if *usage == -1 {
                continue;
            }
            if cert.ext_key_usage.contains(usage) {
                continue;
            }
            *usage = -1;
            remaining -= 1;
            if remaining == 0 {
                return false;
            }
        }
    }
    true
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RevokedEntry {
    pub serial_number: BigInt,
    pub revocation_time: DateTime<Utc>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RevocationList {
    raw_tbs: Vec<u8>,
    raw_issuer: Vec<u8>,
    signature_algorithm: Vec<u32>,
    signature: Vec<u8>,
    pub this_update: DateTime<Utc>,
    pub next_update: Option<DateTime<Utc>>,
    pub revoked: Vec<RevokedEntry>,
}
impl RevocationList {
    pub fn parse(der_bytes: &[u8]) -> Option<RevocationList> {
        let (outer, _) = der::read(der_bytes)?;
        let [tbs, algorithm, signature] = der::sequence(&outer)?.try_into().ok()?;
        let (&0, signature) = signature.body.split_first()? else {
            return None;
        };
        let signature_algorithm = asn1::oid(der::sequence(&algorithm)?.first()?.body).ok()?.0;
        let mut fields = der::sequence(&tbs)?.into_iter().peekable();
        fields.next_if(|f| f.tag == TAG_INTEGER);
        let _alg = fields.next()?;
        let issuer = fields.next()?;
        let this_update = time(&fields.next()?)?;
        let next_update = match fields.next_if(|f| f.tag == 0x17 || f.tag == 0x18) {
            Some(t) => Some(time(&t)?),
            None => None,
        };
        let mut revoked = Vec::new();
        if let Some(list) = fields.next_if(|f| f.tag == TAG_SEQUENCE) {
            for entry in der::sequence(&list)? {
                let parts = der::sequence(&entry)?;
                let serial = parts.first()?;
                revoked.push(RevokedEntry {
                    serial_number: BigInt::from_signed_bytes_be(serial.body),
                    revocation_time: time(parts.get(1)?)?,
                });
            }
        }
        Some(RevocationList {
            raw_tbs: tbs.full.to_vec(),
            raw_issuer: issuer.full.to_vec(),
            signature_algorithm,
            signature: signature.to_vec(),
            this_update,
            next_update,
            revoked,
        })
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KeyUsagePolicy {
    pub allowed: u32,
    pub require_extension: bool,
    pub ext_key_usages: Vec<i32>,
    pub ignore: bool,
}
#[derive(Clone, Debug, Default)]
pub struct TrustOptions {
    pub roots: Option<CertPool>,
    pub intermediates: Option<CertPool>,
    pub crls: Vec<Option<RevocationList>>,
    pub require_crl: bool,
    pub time: Option<DateTime<FixedOffset>>,
    pub at_signature_time: bool,
    pub key_usage: KeyUsagePolicy,
    pub timestamp_roots: Option<CertPool>,
    pub timestamp_intermediates: Option<CertPool>,
    pub at_timestamp_time: bool,
}
#[derive(Clone, Debug, Default)]
pub struct TimestampTrustOptions {
    pub roots: Option<CertPool>,
    pub intermediates: Option<CertPool>,
    pub time: Option<DateTime<FixedOffset>>,
    pub certificates: Vec<super::Certificate>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrustedTimestamp {
    pub info: TimestampInfo,
    pub trusted: bool,
    pub chains: Vec<Vec<X509Certificate>>,
}
fn now() -> DateTime<FixedOffset> {
    Utc::now().fixed_offset()
}
fn untrusted(detail: impl std::fmt::Display, kinds: Vec<Kind>) -> Error {
    let mut err = Error::new(Kind::CertificateUntrusted, detail);
    err.also.extend(kinds);
    err
}
#[doc(hidden)]
pub fn verify_timestamp_trusted(
    sig: &Signature,
    opts: &TimestampTrustOptions,
) -> Result<TrustedTimestamp, (Option<Box<TrustedTimestamp>>, Error)> {
    let plain = TimestampOptions {
        certificates: opts.certificates.clone(),
    };
    let wrap = |info: TimestampInfo| TrustedTimestamp {
        info,
        trusted: false,
        chains: Vec::new(),
    };
    let info =
        verify_timestamp(sig, &plain).map_err(|(info, e)| (info.map(|i| Box::new(wrap(*i))), e))?;
    let Some(roots) = &opts.roots else {
        return Ok(wrap(info));
    };
    let at = opts.time.unwrap_or_else(now);
    let verified = X509Certificate::parse(&info.certificate.raw)
        .ok_or_else(|| X509Error::Other("x509: malformed certificate".into()))
        .and_then(|leaf| verify_chain(&leaf, roots, opts.intermediates.as_ref(), at, &[8]));
    match verified {
        Ok(chains) => Ok(TrustedTimestamp {
            info,
            trusted: true,
            chains,
        }),
        Err(e) => {
            let mut err = Error::new(Kind::TimestampUntrusted, &e);
            err.also.extend(e.kinds());
            Err((Some(Box::new(wrap(info))), err))
        }
    }
}
fn verification_time(sig: &Signature, opts: &TrustOptions) -> Result<DateTime<FixedOffset>, Error> {
    if opts.at_timestamp_time {
        if opts.timestamp_roots.is_none() {
            return Err(Error::new(
                Kind::TimestampUntrusted,
                "AtTimestampTime without TimestampRoots",
            ));
        }
        let info = verify_timestamp_trusted(sig, &timestamp_options(opts)).map_err(|(_, e)| e)?;
        return Ok(info.info.time.fixed_offset());
    }
    if opts.at_signature_time {
        let value = sig.date_time.trim();
        if value.is_empty() {
            return Err(Error::new(
                Kind::MalformedSignature,
                "no Digital Signature DateTime",
            ));
        }
        let malformed = || {
            Error::new(
                Kind::MalformedSignature,
                format!(
                    "Digital Signature DateTime {}",
                    crate::textfmt::quote(value)
                ),
            )
        };
        let parsed = crate::dcmtime::parse_datetime(value).map_err(|_| malformed())?;
        let t = parsed.time;
        let offset = FixedOffset::east_opt(t.offset() as i32).ok_or_else(malformed)?;
        let local =
            chrono::NaiveDate::from_ymd_opt(t.year() as i32, t.month() as u32, t.day() as u32)
                .and_then(|d| {
                    d.and_hms_nano_opt(
                        t.hour() as u32,
                        t.minute() as u32,
                        t.second() as u32,
                        t.nanosecond() as u32,
                    )
                })
                .ok_or_else(malformed)?;
        return offset
            .from_local_datetime(&local)
            .single()
            .ok_or_else(malformed);
    }
    Ok(opts.time.unwrap_or_else(now))
}
fn timestamp_options(opts: &TrustOptions) -> TimestampTrustOptions {
    TimestampTrustOptions {
        roots: opts.timestamp_roots.clone(),
        intermediates: opts.timestamp_intermediates.clone(),
        time: opts.time,
        certificates: Vec::new(),
    }
}
fn check_key_usage(p: &KeyUsagePolicy, leaf: &X509Certificate) -> Result<(), Error> {
    if p.ignore {
        return Ok(());
    }
    let allowed = if p.allowed == 0 { 1 | 2 } else { p.allowed };
    if leaf.key_usage == 0 {
        if p.require_extension {
            return Err(Error::new(Kind::CertificateUsage, "no Key Usage extension"));
        }
        return Ok(());
    }
    if leaf.key_usage & allowed == 0 {
        return Err(Error::new(
            Kind::CertificateUsage,
            format!("Key Usage {:#x} has none of {:#x}", leaf.key_usage, allowed),
        ));
    }
    Ok(())
}
pub fn verify_certificate(
    sig: &Signature,
    opts: &TrustOptions,
) -> Result<Vec<Vec<X509Certificate>>, Error> {
    let Some(leaf) = sig
        .certificate
        .as_ref()
        .and_then(|c| X509Certificate::parse(&c.raw))
    else {
        return Err(untrusted(
            format!(
                "{}: unreadable Certificate of Signer",
                Kind::MalformedSignature.text()
            ),
            vec![Kind::MalformedSignature],
        ));
    };
    let Some(roots) = &opts.roots else {
        return Err(untrusted("no trust anchors", Vec::new()));
    };
    let wrap = |e: Error| {
        let mut kinds = vec![e.kind];
        kinds.extend(e.also.iter().copied());
        untrusted(&e, kinds)
    };
    let at = verification_time(sig, opts).map_err(wrap)?;
    check_key_usage(&opts.key_usage, &leaf).map_err(wrap)?;
    let usages = if opts.key_usage.ext_key_usages.is_empty() {
        vec![0]
    } else {
        opts.key_usage.ext_key_usages.clone()
    };
    let chains =
        verify_chain(&leaf, roots, opts.intermediates.as_ref(), at, &usages).map_err(|e| {
            let mut kinds = e.kinds();
            let reason = match &e {
                X509Error::Invalid(_, REASON_EXPIRED, _) => Some(Kind::CertificateExpired),
                X509Error::Invalid(_, REASON_INCOMPATIBLE_USAGE, _) => Some(Kind::CertificateUsage),
                _ => None,
            };
            match reason {
                Some(r) => {
                    kinds.insert(0, r);
                    untrusted(format!("{}: {e}", r.text()), kinds)
                }
                None => untrusted(&e, kinds),
            }
        })?;
    if opts.crls.is_empty() && !opts.require_crl {
        return Ok(chains);
    }
    let mut accepted = Vec::new();
    let mut first: Option<Error> = None;
    for chain in chains {
        match check_revocation(&chain, &opts.crls, &at, opts.require_crl) {
            Ok(()) => accepted.push(chain),
            Err(e) => {
                first.get_or_insert(e);
            }
        }
    }
    if accepted.is_empty() {
        let e = first.unwrap_or_else(|| Error::other(""));
        return Err(wrap(e));
    }
    Ok(accepted)
}
fn check_revocation(
    chain: &[X509Certificate],
    crls: &[Option<RevocationList>],
    at: &DateTime<FixedOffset>,
    require_crl: bool,
) -> Result<(), Error> {
    for pair in chain.windows(2) {
        let (certificate, issuer) = (&pair[0], &pair[1]);
        let mut current = false;
        for crl in crls.iter().flatten() {
            if crl.raw_issuer != issuer.raw_subject
                || check_signed_by(
                    issuer,
                    1 << 6,
                    &crl.signature_algorithm,
                    &crl.raw_tbs,
                    &crl.signature,
                )
                .is_err()
            {
                continue;
            }
            for entry in &crl.revoked {
                if entry.serial_number == certificate.serial_number {
                    return Err(Error::new(
                        Kind::CertificateRevoked,
                        format!(
                            "{} serial {} on {}",
                            crate::textfmt::quote(&certificate.subject.string()),
                            certificate.serial_number,
                            entry
                                .revocation_time
                                .to_rfc3339_opts(SecondsFormat::Secs, true)
                        ),
                    ));
                }
            }
            if crl.next_update.is_none_or(|next| *at <= next) {
                current = true;
            }
        }
        if require_crl && !current {
            return Err(Error::new(
                Kind::RevocationUnknown,
                format!(
                    "issuer {} of {}",
                    crate::textfmt::quote(&issuer.subject.string()),
                    crate::textfmt::quote(&certificate.subject.string())
                ),
            ));
        }
    }
    Ok(())
}
#[derive(Clone, Debug)]
pub struct VerifyResult {
    pub signature: Signature,
    pub err: Option<Error>,
    pub trust_checked: bool,
    pub trust_err: Option<Error>,
    pub chains: Option<Vec<Vec<X509Certificate>>>,
    pub timestamp: Option<TrustedTimestamp>,
    pub timestamp_err: Option<Error>,
}
impl VerifyResult {
    pub fn has_timestamp(&self) -> bool {
        !self.signature.certified_timestamp.is_empty()
    }
    pub fn timestamp_valid(&self) -> bool {
        self.has_timestamp() && self.timestamp.is_some() && self.timestamp_err.is_none()
    }
    pub fn valid(&self) -> bool {
        self.err.is_none()
    }
    pub fn trusted(&self) -> bool {
        self.valid() && self.trust_checked && self.trust_err.is_none()
    }
    fn check_timestamp(&mut self, opts: &TimestampTrustOptions) {
        self.timestamp = None;
        self.timestamp_err = None;
        if !self.has_timestamp() {
            return;
        }
        match verify_timestamp_trusted(&self.signature, opts) {
            Ok(info) => self.timestamp = Some(info),
            Err((info, e)) => {
                self.timestamp = info.map(|i| *i);
                self.timestamp_err = Some(e);
            }
        }
    }
}
pub fn verify(obj: &Object) -> Vec<VerifyResult> {
    signatures(obj.elements())
        .into_iter()
        .map(|sig| {
            let err = verify_signature(&sig).err();
            let mut result = VerifyResult {
                signature: sig,
                err,
                trust_checked: false,
                trust_err: None,
                chains: None,
                timestamp: None,
                timestamp_err: None,
            };
            result.check_timestamp(&TimestampTrustOptions::default());
            result
        })
        .collect()
}
pub fn verify_with_trust(obj: &Object, opts: &TrustOptions) -> Vec<VerifyResult> {
    let mut results = verify(obj);
    for r in &mut results {
        r.trust_checked = true;
        match verify_certificate(&r.signature, opts) {
            Ok(chains) => r.chains = Some(chains),
            Err(e) => r.trust_err = Some(e),
        }
        if opts.timestamp_roots.is_some() {
            r.check_timestamp(&timestamp_options(opts));
        }
    }
    results
}
pub trait Timestamper {
    fn timestamp(&self, ctx: &crate::net::ul::Ctx, request: &[u8]) -> Result<Vec<u8>, Error>;
}
pub struct TimestamperFn<F>(pub F);
impl<F> Timestamper for TimestamperFn<F>
where
    F: Fn(&crate::net::ul::Ctx, &[u8]) -> Result<Vec<u8>, Error>,
{
    fn timestamp(&self, ctx: &crate::net::ul::Ctx, request: &[u8]) -> Result<Vec<u8>, Error> {
        (self.0)(ctx, request)
    }
}
pub const MAX_TIMESTAMP_RESPONSE_SIZE: usize = 1 << 20;
#[cfg(feature = "dicomweb")]
#[derive(Clone, Debug, Default)]
pub struct HttpTimestamper {
    pub url: String,
}
#[cfg(feature = "dicomweb")]
impl Timestamper for HttpTimestamper {
    fn timestamp(&self, ctx: &crate::net::ul::Ctx, request: &[u8]) -> Result<Vec<u8>, Error> {
        use crate::net::dicomweb::http::{self, HttpRequest, RedirectCheck, TransportError};
        use crate::net::dicomweb::multipart::{ByteRead, ReadErr, SliceReader};
        use crate::net::dicomweb::urlrules;
        let fail = |detail: &str| Error::other(format!("dicom dsig: timestamp request: {detail}"));
        let url = urlrules::parse(&self.url).map_err(|e| fail(&e))?;
        let mut body = SliceReader::new(request);
        let sent = http::send_limited(
            HttpRequest {
                method: "POST",
                url: &url,
                headers: vec![
                    ("Content-Type".into(), "application/timestamp-query".into()),
                    ("Accept".into(), "application/timestamp-reply".into()),
                ],
                body: Some(&mut body),
                https: None,
                identity: None,
            },
            RedirectCheck::Default,
            10,
            Some(request),
            ctx,
        );
        let mut resp = match sent {
            Ok(r) => r,
            Err(TransportError::Timeout) if ctx.err().is_some() => {
                return Err(fail(&format!(
                    "Post {}: context deadline exceeded",
                    crate::textfmt::quote(&self.url)
                ))
                .with_kind(Kind::ContextDeadline));
            }
            Err(_) => {
                return Err(fail(&format!(
                    "Post {}: transport failure",
                    crate::textfmt::quote(&self.url)
                )));
            }
        };
        if resp.status_code != 200 {
            let text = crate::net::dicomweb::types::status_text(resp.status_code);
            let status = format!("{} {text}", resp.status_code);
            return Err(Error::new(
                Kind::TimestampResponse,
                format!("HTTP {status}"),
            ));
        }
        let content_type = resp.header.get("Content-Type");
        let media = content_type.split(';').next().unwrap_or("").trim();
        if !media.eq_ignore_ascii_case("application/timestamp-reply") {
            return Err(Error::new(
                Kind::TimestampResponse,
                format!("Content-Type {}", crate::textfmt::quote(media)),
            ));
        }
        let mut out = Vec::new();
        let mut buf = vec![0u8; 32 << 10];
        loop {
            let want = (MAX_TIMESTAMP_RESPONSE_SIZE + 1 - out.len()).min(buf.len());
            if want == 0 {
                break;
            }
            let (n, err) = resp.body.read(&mut buf[..want]);
            out.extend_from_slice(&buf[..n]);
            match err {
                None => {}
                Some(ReadErr::Eof) => break,
                Some(_) => {
                    return Err(Error::other(
                        "dicom dsig: timestamp response: unexpected EOF",
                    ));
                }
            }
        }
        if out.len() > MAX_TIMESTAMP_RESPONSE_SIZE {
            return Err(Error::new(
                Kind::TimestampResponse,
                format!("response larger than {MAX_TIMESTAMP_RESPONSE_SIZE} bytes"),
            ));
        }
        Ok(out)
    }
}
impl Error {
    #[cfg_attr(not(feature = "dicomweb"), allow(dead_code))]
    pub(crate) fn with_kind(mut self, kind: Kind) -> Error {
        self.also.push(kind);
        self
    }
}
pub fn timestamp_signature(
    ctx: &crate::net::ul::Ctx,
    obj: &mut Object,
    sig: &Signature,
    tsa: Option<&dyn Timestamper>,
    opts: &TimestampRequestOptions,
) -> Result<(Signature, TimestampInfo), Error> {
    let Some(tsa) = tsa else {
        return Err(Error::other("dicom dsig: no time stamping authority"));
    };
    let req = new_timestamp_request(sig, opts)?;
    let response = tsa.timestamp(ctx, &req.der)?;
    let token = token_from_response(&response)?;
    let info = req.check(&token, &[]).map_err(|(_, e)| e)?;
    let updated = add_timestamp(obj, sig, &token, &[])?;
    Ok((updated, info))
}
#[doc(hidden)]
pub fn x509_names(chain: &[X509Certificate]) -> Vec<String> {
    chain.iter().map(X509Certificate::hint_name).collect()
}
impl Error {
    pub fn from_text(text: impl Into<String>) -> Error {
        Error::other(text)
    }
}
