use super::asn1::{self, Asn1Error, Fields, OCTET_STRING, Oid, SEQUENCE, SET};
use super::der::Tlv;
use super::hash::sha1;
use super::{Certificate, Kind, MacAlgorithm, PublicKey, parse_certificate, pubkey};
use num_bigint::BigInt;
use std::collections::HashMap;
pub(super) const OID_SIGNED_DATA: &[u32] = &[1, 2, 840, 113549, 1, 7, 2];
pub(super) const OID_TST_INFO: &[u32] = &[1, 2, 840, 113549, 1, 9, 16, 1, 4];
const OID_ATTRIBUTE_CONTENT_TYPE: &str = "1.2.840.113549.1.9.3";
const OID_ATTRIBUTE_MESSAGE_DIGEST: &str = "1.2.840.113549.1.9.4";
const OID_ATTRIBUTE_SIGNING_CERT: &str = "1.2.840.113549.1.9.16.2.12";
const OID_ATTRIBUTE_SIGNING_CERT_V2: &str = "1.2.840.113549.1.9.16.2.47";
const RSA_ALGORITHMS: &[&[u32]] = &[
    &[1, 2, 840, 113549, 1, 1, 1],
    &[1, 2, 840, 113549, 1, 1, 5],
    &[1, 2, 840, 113549, 1, 1, 11],
    &[1, 2, 840, 113549, 1, 1, 12],
    &[1, 2, 840, 113549, 1, 1, 13],
];
const ECDSA_ALGORITHMS: &[&[u32]] = &[
    &[1, 2, 840, 10045, 2, 1],
    &[1, 2, 840, 10045, 4, 1],
    &[1, 2, 840, 10045, 4, 3, 2],
    &[1, 2, 840, 10045, 4, 3, 3],
    &[1, 2, 840, 10045, 4, 3, 4],
];
const OID_EXTENDED_KEY_USAGE: &[u8] = &[0x55, 0x1D, 0x25];
const OID_KEY_USAGE_TIME_STAMPING: &[u32] = &[1, 3, 6, 1, 5, 5, 7, 3, 8];
const DIGEST_ALGORITHMS: &[(&[u32], MacAlgorithm)] = &[
    (&[1, 3, 14, 3, 2, 26], MacAlgorithm::Sha1),
    (&[2, 16, 840, 1, 101, 3, 4, 2, 1], MacAlgorithm::Sha256),
    (&[2, 16, 840, 1, 101, 3, 4, 2, 2], MacAlgorithm::Sha384),
    (&[2, 16, 840, 1, 101, 3, 4, 2, 3], MacAlgorithm::Sha512),
];
pub(super) fn hash_for_oid(oid: &Oid) -> Option<MacAlgorithm> {
    DIGEST_ALGORITHMS
        .iter()
        .find(|(arcs, _)| oid.is(arcs))
        .map(|(_, hash)| *hash)
}
pub(super) fn oid_for_hash(hash: MacAlgorithm) -> Option<Oid> {
    DIGEST_ALGORITHMS
        .iter()
        .find(|(_, h)| *h == hash)
        .map(|(arcs, _)| Oid(arcs.to_vec()))
}
#[derive(Debug)]
pub(super) struct Fail {
    pub text: String,
    pub kind: Option<Kind>,
}
impl Fail {
    pub fn new(text: impl Into<String>) -> Fail {
        Fail {
            text: text.into(),
            kind: None,
        }
    }
    pub fn unsupported(detail: impl std::fmt::Display) -> Fail {
        Fail {
            text: format!("{}: {detail}", Kind::Unsupported.text()),
            kind: Some(Kind::Unsupported),
        }
    }
    pub fn decoding(what: &str, err: Asn1Error) -> Fail {
        Fail::new(format!("{what}: {err}"))
    }
}
pub(super) fn unmarshal<'a, T>(
    what: &str,
    data: &'a [u8],
    decode: impl FnOnce(Tlv<'a>) -> asn1::Result<T>,
) -> Result<T, Fail> {
    asn1::unmarshal(data, decode).map_err(|err| Fail::new(format!("{what}: {err}")))
}
pub(super) struct SignedToken {
    pub content: Vec<u8>,
    pub signer: Certificate,
    pub embedded: bool,
}
struct SignerInfo<'a> {
    sid: Tlv<'a>,
    digest_algorithm: Oid,
    signed_attrs: Option<Tlv<'a>>,
    signature_algorithm: Oid,
    signature: &'a [u8],
}
struct SignedData<'a> {
    content_type: Oid,
    content: Option<Tlv<'a>>,
    certificates: Option<Tlv<'a>>,
    signer_infos: Vec<SignerInfo<'a>>,
}
fn signer_info<'a>(tlv: &Tlv<'a>) -> asn1::Result<SignerInfo<'a>> {
    let mut f = Fields::new(tlv.body);
    f.int()?;
    let sid = f.any()?;
    let digest_algorithm = asn1::algorithm(&f.required(SEQUENCE)?)?;
    let signed_attrs = f.optional_context(0)?;
    let signature_algorithm = asn1::algorithm(&f.required(SEQUENCE)?)?;
    let signature = f.octets()?;
    f.optional_context(1)?;
    Ok(SignerInfo {
        sid,
        digest_algorithm,
        signed_attrs,
        signature_algorithm,
        signature,
    })
}
fn signed_data<'a>(tlv: Tlv<'a>) -> asn1::Result<SignedData<'a>> {
    let mut f = asn1::sequence_fields(tlv)?;
    f.int()?;
    for algorithm in asn1::sequences(f.required(SET)?.body)? {
        asn1::algorithm(&algorithm)?;
    }
    let encapsulated = f.required(SEQUENCE)?;
    let mut e = Fields::new(encapsulated.body);
    let content_type = e.oid()?;
    let content = e.optional_explicit(0)?;
    let certificates = f.optional_context(0)?;
    f.optional_context(1)?;
    let signer_infos = asn1::sequences(f.required(SET)?.body)?
        .iter()
        .map(signer_info)
        .collect::<asn1::Result<Vec<_>>>()?;
    Ok(SignedData {
        content_type,
        content,
        certificates,
        signer_infos,
    })
}
pub(super) fn parse_signed_token(
    der: &[u8],
    content_type: &[u32],
    extra: &[Certificate],
) -> Result<SignedToken, Fail> {
    let (outer_type, outer_content) = unmarshal("ContentInfo", der, |info| {
        let mut f = asn1::sequence_fields(info)?;
        Ok((f.oid()?, f.optional_explicit(0)?))
    })?;
    if !outer_type.is(OID_SIGNED_DATA) {
        return Err(Fail::new(format!(
            "content type {outer_type} is not SignedData"
        )));
    }
    let sd = unmarshal(
        "SignedData",
        outer_content.map_or(&[][..], |c| c.body),
        signed_data,
    )?;
    if !sd.content_type.is(content_type) {
        return Err(Fail::new(format!(
            "encapsulated content type {}, want {}",
            sd.content_type,
            Oid(content_type.to_vec())
        )));
    }
    let content = unmarshal(
        "encapsulated content",
        sd.content.map_or(&[][..], |c| c.body),
        |content| {
            if content.tag != OCTET_STRING {
                return Err(Asn1Error);
            }
            Ok(content.body)
        },
    )?;
    if sd.signer_infos.len() != 1 {
        return Err(Fail::new(format!(
            "{} signer infos, want 1",
            sd.signer_infos.len()
        )));
    }
    let mut certificates = Vec::new();
    if let Some(set) = sd.certificates.filter(|set| !set.body.is_empty()) {
        let unreadable = || Fail::new("certificates: x509: malformed certificate");
        for item in asn1::elements(set.body).map_err(|_| unreadable())? {
            let certificate = parse_certificate(item.full).ok_or_else(unreadable)?;
            certificates.push(certificate);
        }
    }
    let si = &sd.signer_infos[0];
    let (signer, embedded) = match find_signer(&si.sid, &certificates) {
        Ok(signer) => (signer, true),
        Err(_) => (find_signer(&si.sid, extra)?, false),
    };
    let Some(hash) = hash_for_oid(&si.digest_algorithm) else {
        return Err(Fail::unsupported(format!(
            "digest algorithm {}",
            si.digest_algorithm
        )));
    };
    let Some(signed_attrs) = si.signed_attrs else {
        return Err(Fail::new("no signed attributes"));
    };
    let attributes = parse_attributes(signed_attrs.body)?;
    let signed_type = attributes
        .get(OID_ATTRIBUTE_CONTENT_TYPE)
        .and_then(|value| asn1::single(value).ok().flatten())
        .filter(|tlv| tlv.tag == asn1::OID)
        .and_then(|tlv| asn1::oid(tlv.body).ok());
    if !signed_type.is_some_and(|oid| oid.is(content_type)) {
        return Err(Fail::new(
            "content-type attribute missing or different from the content",
        ));
    }
    let message_digest = attributes
        .get(OID_ATTRIBUTE_MESSAGE_DIGEST)
        .and_then(|value| asn1::single(value).ok().flatten())
        .filter(|tlv| tlv.tag == OCTET_STRING);
    let Some(message_digest) = message_digest else {
        return Err(Fail::new("message-digest attribute missing"));
    };
    if message_digest.body != hash.digest(content) {
        return Err(Fail::new("message digest does not match the content"));
    }
    check_signing_certificate(&attributes, &signer)?;
    let mut signed_bytes = signed_attrs.full.to_vec();
    signed_bytes[0] = SET;
    check_cms_signature(
        &signer,
        &si.signature_algorithm,
        hash,
        &signed_bytes,
        si.signature,
    )?;
    Ok(SignedToken {
        content: content.to_vec(),
        signer,
        embedded,
    })
}
fn find_signer(sid: &Tlv<'_>, certificates: &[Certificate]) -> Result<Certificate, Fail> {
    let unreadable = || Fail::new("unreadable signer identifier");
    let found = if sid.tag == SEQUENCE {
        let mut f = Fields::new(sid.body);
        let issuer = f.any().map_err(|_| unreadable())?;
        let serial: BigInt = f.big_int().map_err(|_| unreadable())?;
        certificates
            .iter()
            .find(|c| c.raw_issuer == issuer.full && c.serial_number == serial)
    } else if sid.tag & 0xDF == 0x80 {
        certificates
            .iter()
            .find(|c| !c.subject_key_id.is_empty() && c.subject_key_id == sid.body)
    } else {
        return Err(unreadable());
    };
    found.cloned().ok_or_else(|| {
        Fail::new("signer certificate not in the token nor among the given certificates")
    })
}
fn parse_attributes(data: &[u8]) -> Result<HashMap<String, &[u8]>, Fail> {
    let decoding = |err| Fail::decoding("signed attributes", err);
    let mut out = HashMap::new();
    for attribute in asn1::elements(data).map_err(decoding)? {
        if attribute.tag != SEQUENCE {
            return Err(decoding(Asn1Error));
        }
        let mut f = Fields::new(attribute.body);
        let kind = f.oid().map_err(decoding)?;
        let values = f.any().map_err(decoding)?;
        let Ok(Some(value)) = asn1::single(values.body) else {
            return Err(Fail::new(format!(
                "signed attributes: attribute {kind} does not have exactly one value"
            )));
        };
        if out.insert(kind.to_string(), value.full).is_some() {
            return Err(Fail::new(format!(
                "signed attributes: attribute {kind} repeated"
            )));
        }
    }
    Ok(out)
}
fn first_cert_id(value: &[u8], v2: bool) -> asn1::Result<Option<(Option<Oid>, Vec<u8>)>> {
    let outer = asn1::single(value)?.ok_or(Asn1Error)?;
    if outer.tag != SEQUENCE {
        return Err(Asn1Error);
    }
    let mut f = Fields::new(outer.body);
    let certs = asn1::sequences(f.required(SEQUENCE)?.body)?;
    f.optional_any()?;
    let mut first = None;
    for (i, cert) in certs.iter().enumerate() {
        let mut c = Fields::new(cert.body);
        let algorithm = if v2 {
            c.optional(SEQUENCE)?
                .map(|a| asn1::algorithm(&a))
                .transpose()?
        } else {
            None
        };
        let hash = c.octets()?;
        c.optional_any()?;
        if i == 0 {
            first = Some((algorithm, hash.to_vec()));
        }
    }
    Ok(first)
}
fn check_signing_certificate(
    attributes: &HashMap<String, &[u8]>,
    signer: &Certificate,
) -> Result<(), Fail> {
    if let Some(value) = attributes.get(OID_ATTRIBUTE_SIGNING_CERT_V2) {
        let Ok(Some((algorithm, cert_hash))) = first_cert_id(value, true) else {
            return Err(Fail::new("unreadable signing-certificate-v2 attribute"));
        };
        let hash = match algorithm.filter(|oid| !oid.is_empty()) {
            None => MacAlgorithm::Sha256,
            Some(oid) => hash_for_oid(&oid)
                .ok_or_else(|| Fail::unsupported(format!("signing certificate hash {oid}")))?,
        };
        if cert_hash != hash.digest(&signer.raw) {
            return Err(Fail::new(
                "signing-certificate-v2 does not name the signer certificate",
            ));
        }
        return Ok(());
    }
    if let Some(value) = attributes.get(OID_ATTRIBUTE_SIGNING_CERT) {
        let Ok(Some((_, cert_hash))) = first_cert_id(value, false) else {
            return Err(Fail::new("unreadable signing-certificate attribute"));
        };
        if cert_hash != sha1(&signer.raw) {
            return Err(Fail::new(
                "signing-certificate does not name the signer certificate",
            ));
        }
        return Ok(());
    }
    Err(Fail::new("no ESS signing-certificate attribute"))
}
fn check_cms_signature(
    signer: &Certificate,
    algorithm: &Oid,
    hash: MacAlgorithm,
    message: &[u8],
    signature: &[u8],
) -> Result<(), Fail> {
    let invalid = || Fail::new("token signature does not verify with the signer certificate");
    let known = |list: &[&[u32]]| {
        if list.iter().any(|arcs| algorithm.is(arcs)) {
            Ok(())
        } else {
            Err(Fail::unsupported(format!(
                "token signature algorithm {algorithm}"
            )))
        }
    };
    let digest = hash.digest(message);
    match &signer.public_key {
        PublicKey::Rsa { n, e } => {
            known(RSA_ALGORITHMS)?;
            if !pubkey::rsa_verify(n, e, &hash.digest_info(&digest, true), signature) {
                return Err(invalid());
            }
        }
        PublicKey::Ecdsa { curve, x, y } => {
            known(ECDSA_ALGORITHMS)?;
            if !pubkey::ecdsa_verify(*curve, x, y, &digest, signature) {
                return Err(invalid());
            }
        }
        other => {
            return Err(Fail::unsupported(format!(
                "{} token signer key",
                other.type_label()
            )));
        }
    }
    Ok(())
}
pub(super) fn has_critical_time_stamping_usage(cert: &Certificate) -> bool {
    let Some(ext) = cert
        .extensions
        .iter()
        .find(|e| e.id == OID_EXTENDED_KEY_USAGE)
    else {
        return false;
    };
    if !ext.critical {
        return false;
    }
    let Ok(Some(list)) = asn1::single(&ext.value) else {
        return false;
    };
    if list.tag != SEQUENCE {
        return false;
    }
    match asn1::elements(list.body).as_deref() {
        Ok([usage]) if usage.tag == asn1::OID => {
            asn1::oid(usage.body).is_ok_and(|oid| oid.is(OID_KEY_USAGE_TIME_STAMPING))
        }
        _ => false,
    }
}
