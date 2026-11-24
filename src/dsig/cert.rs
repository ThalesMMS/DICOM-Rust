use super::der::{self, TAG_INTEGER, TAG_SEQUENCE};
use super::pubkey::Curve;
use chrono::{DateTime, NaiveDate, Utc};
use num_bigint::{BigInt, BigUint};
const OID_RSA: &[u8] = &[0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x01, 0x01];
const OID_EC: &[u8] = &[0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x02, 0x01];
const OID_ED25519: &[u8] = &[0x2B, 0x65, 0x70];
const OID_P224: &[u8] = &[0x2B, 0x81, 0x04, 0x00, 0x21];
const OID_P256: &[u8] = &[0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x03, 0x01, 0x07];
const OID_P384: &[u8] = &[0x2B, 0x81, 0x04, 0x00, 0x22];
const OID_P521: &[u8] = &[0x2B, 0x81, 0x04, 0x00, 0x23];
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PublicKey {
    Rsa {
        n: BigUint,
        e: BigUint,
    },
    Ecdsa {
        curve: Curve,
        x: BigUint,
        y: BigUint,
    },
    Ed25519(Vec<u8>),
    Unknown,
}
impl PublicKey {
    #[doc(hidden)]
    pub fn type_label(&self) -> &'static str {
        match self {
            PublicKey::Rsa { .. } => "RSA",
            PublicKey::Ecdsa { .. } => "ECDSA",
            PublicKey::Ed25519(_) => "Ed25519",
            PublicKey::Unknown => "unknown",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Certificate {
    pub raw: Vec<u8>,
    pub public_key: PublicKey,
    pub raw_issuer: Vec<u8>,
    pub serial_number: BigInt,
    pub validity: Option<(DateTime<Utc>, DateTime<Utc>)>,
    pub subject_key_id: Vec<u8>,
    pub extensions: Vec<Extension>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Extension {
    pub id: Vec<u8>,
    pub critical: bool,
    pub value: Vec<u8>,
}
const OID_SUBJECT_KEY_ID: &[u8] = &[0x55, 0x1D, 0x0E];
fn time((tag, body): (u8, &[u8])) -> Option<DateTime<Utc>> {
    let t = match tag {
        0x17 => super::x509check::parse_utc_time(body)?,
        0x18 => super::x509check::parse_generalized_time(body)?,
        _ => return None,
    };
    let date = NaiveDate::from_ymd_opt(i32::try_from(t.year).ok()?, t.month, t.day)?;
    let local = date.and_hms_opt(t.hour, t.minute, t.second)?;
    Some((local - chrono::Duration::seconds(i64::from(t.offset_seconds))).and_utc())
}
pub fn parse_certificate(value: &[u8]) -> Option<Certificate> {
    let (outer, _padding) = der::read(value)?;
    let go = super::x509check::parse_cert_fields(outer.full).ok()?;
    let public_key = public_key(go.key_algorithm, &go.key)?;
    let extensions: Vec<Extension> = go
        .extensions
        .iter()
        .map(|&(id, critical, value)| Extension {
            id: id.to_vec(),
            critical,
            value: value.to_vec(),
        })
        .collect();
    let subject_key_id = extensions
        .iter()
        .find(|e| e.id == OID_SUBJECT_KEY_ID)
        .and_then(|e| der::read(&e.value))
        .filter(|(id, _)| id.tag == 0x04)
        .map(|(id, _)| id.body.to_vec())
        .unwrap_or_default();
    let [not_before, not_after] = go.validity;
    Some(Certificate {
        raw: go.raw.to_vec(),
        public_key,
        raw_issuer: go.issuer.to_vec(),
        serial_number: BigInt::from_signed_bytes_be(go.serial),
        validity: time(not_before).zip(time(not_after)),
        subject_key_id,
        extensions,
    })
}
#[doc(hidden)]
pub fn parse_certificates(mut der: &[u8]) -> Option<Vec<Certificate>> {
    let mut out = Vec::new();
    while !der.is_empty() {
        let (item, rest) = der::read(der)?;
        out.push(parse_certificate(item.full)?);
        der = rest;
    }
    Some(out)
}
fn public_key((oid, parameters): (&[u8], &[u8]), key: &[u8]) -> Option<PublicKey> {
    match oid {
        OID_RSA => {
            let (pair, _) = der::read(key)?;
            let (n, rest) = der::read(pair.body)?;
            let (e, _) = der::read(rest)?;
            if pair.tag != TAG_SEQUENCE || n.tag != TAG_INTEGER || e.tag != TAG_INTEGER {
                return None;
            }
            let (n, e) = (der::unsigned(&n)?, der::unsigned(&e)?);
            if n.bits() == 0 || e.bits() == 0 || e.bits() > 63 {
                return None;
            }
            Some(PublicKey::Rsa { n, e })
        }
        OID_EC => {
            let (named, _) = der::read(parameters)?;
            let curve = match named.body {
                OID_P224 => Curve::P224,
                OID_P256 => Curve::P256,
                OID_P384 => Curve::P384,
                OID_P521 => Curve::P521,
                _ => return None,
            };
            let size = curve.byte_len();
            let (&4, coordinates) = key.split_first()? else {
                return None;
            };
            if coordinates.len() != 2 * size {
                return None;
            }
            let x = BigUint::from_bytes_be(&coordinates[..size]);
            let y = BigUint::from_bytes_be(&coordinates[size..]);
            curve
                .on_curve(&x, &y)
                .then_some(PublicKey::Ecdsa { curve, x, y })
        }
        OID_ED25519 => {
            (parameters.is_empty() && key.len() == 32).then(|| PublicKey::Ed25519(key.to_vec()))
        }
        _ => Some(PublicKey::Unknown),
    }
}
