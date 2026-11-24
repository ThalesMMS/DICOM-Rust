#![forbid(unsafe_code)]
use crate::object::text::{trim_text_value, trim_text_value_bytes};
use crate::object::{Element, Tag, Value};
use crate::textfmt::quote;
use std::fmt;
mod asn1;
mod cert;
mod cms;
mod der;
mod hash;
mod profile;
mod profile_catalog;
mod pubkey;
mod sign;
mod stream;
mod timestamp;
mod trust;
mod x509check;
pub use asn1::Oid;
pub use cert::{Certificate, Extension, PublicKey, parse_certificate, parse_certificates};
pub use hash::MacAlgorithm;
pub use profile::{
    PROFILE_COVERAGE_COMPLETE, PROFILE_COVERAGE_NEEDS_CONTEXT, PROFILE_COVERAGE_PARTIAL,
    PROFILE_MODULE_SOURCE_SHA256, PROFILE_SOURCE_SHA256, PROFILE_STANDARD_VERSION, Profile,
    ProfileError, ProfileIssue, ProfileOptions, ProfileReport, check_profile,
    check_profile_with_options,
};
pub use pubkey::Curve;
pub use sign::{SignOptions, SigningKey, remove_all_signatures, remove_signature, sign};
pub use timestamp::{
    CERTIFIED_TIMESTAMP_CMS_TSP, TimestampInfo, TimestampOptions, TimestampRequest,
    TimestampRequestOptions, add_timestamp, new_timestamp_request, parse_timestamp_request,
    token_from_response, verify_timestamp,
};
#[cfg(feature = "dicomweb")]
pub use trust::HttpTimestamper;
pub use trust::{
    CertPool, KeyUsagePolicy, MAX_TIMESTAMP_RESPONSE_SIZE, Name, RevocationList, RevokedEntry,
    TimestampTrustOptions, Timestamper, TimestamperFn, TrustOptions, TrustedTimestamp,
    VerifyResult, X509Certificate, X509Error, timestamp_signature, verify, verify_certificate,
    verify_chain, verify_timestamp_trusted, verify_with_trust, x509_names,
};
pub const CERTIFICATE_TYPE_X509: &str = "X509_1993_SIG";
const fn tag(group: u16, element: u16) -> Tag {
    Tag { group, element }
}
const TAG_MAC_PARAMETERS_SEQUENCE: Tag = tag(0x4FFE, 0x0001);
const TAG_DIGITAL_SIGNATURES_SEQUENCE: Tag = tag(0xFFFA, 0xFFFA);
const TAG_MAC_ID_NUMBER: Tag = tag(0x0400, 0x0005);
const TAG_MAC_CALCULATION_TRANSFER_SYNTAX: Tag = tag(0x0400, 0x0010);
const TAG_MAC_ALGORITHM: Tag = tag(0x0400, 0x0015);
const TAG_DATA_ELEMENTS_SIGNED: Tag = tag(0x0400, 0x0020);
const TAG_DIGITAL_SIGNATURE_UID: Tag = tag(0x0400, 0x0100);
const TAG_DIGITAL_SIGNATURE_DATE_TIME: Tag = tag(0x0400, 0x0105);
const TAG_CERTIFICATE_TYPE: Tag = tag(0x0400, 0x0110);
const TAG_CERTIFICATE_OF_SIGNER: Tag = tag(0x0400, 0x0115);
const TAG_SIGNATURE: Tag = tag(0x0400, 0x0120);
const TAG_CERTIFIED_TIMESTAMP_TYPE: Tag = tag(0x0400, 0x0305);
const TAG_CERTIFIED_TIMESTAMP: Tag = tag(0x0400, 0x0310);
const TAG_DIGITAL_SIGNATURE_PURPOSE_CODE_SEQ: Tag = tag(0x0400, 0x0401);
const TAG_CODE_VALUE: Tag = tag(0x0008, 0x0100);
const TAG_CODING_SCHEME_DESIGNATOR: Tag = tag(0x0008, 0x0102);
const TAG_CODE_MEANING: Tag = tag(0x0008, 0x0104);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    SignatureInvalid,
    MalformedSignature,
    Unsupported,
    EncapsulatedMacSyntax,
    ProfileViolation,
    ProfileIncomplete,
    ProfileLimit,
    ItemNotFound,
    NoTimestamp,
    TimestampInvalid,
    TimestampUntrusted,
    TimestampResponse,
    Other,
    CertificateUntrusted,
    CertificateExpired,
    CertificateRevoked,
    RevocationUnknown,
    CertificateUsage,
    X509UnknownAuthority,
    X509CertificateInvalid(u8),
    X509UnhandledCriticalExtension,
    ContextDeadline,
}
impl Kind {
    pub fn text(self) -> &'static str {
        match self {
            Kind::SignatureInvalid => "dicom dsig: signature invalid",
            Kind::MalformedSignature => "dicom dsig: malformed signature attributes",
            Kind::Unsupported => "dicom dsig: unsupported signature parameters",
            Kind::EncapsulatedMacSyntax => {
                "dicom dsig: encapsulated Pixel Data needs an encapsulated MAC Calculation Transfer Syntax"
            }
            Kind::ProfileViolation => "dicom dsig: signature profile violated",
            Kind::ProfileIncomplete => "dicom dsig: attribute-profile coverage incomplete",
            Kind::ProfileLimit => "dicom dsig: attribute-profile evaluation limit exceeded",
            Kind::ItemNotFound => "dicom dsig: sequence item not found",
            Kind::NoTimestamp => "dicom dsig: no certified timestamp",
            Kind::TimestampInvalid => "dicom dsig: certified timestamp invalid",
            Kind::TimestampUntrusted => "dicom dsig: time stamping authority not trusted",
            Kind::TimestampResponse => "dicom dsig: time stamp response rejected",
            Kind::Other => "dicom dsig: error",
            Kind::CertificateUntrusted => "dicom dsig: signer certificate not trusted",
            Kind::CertificateExpired => {
                "dicom dsig: certificate not valid at the verification time"
            }
            Kind::CertificateRevoked => "dicom dsig: certificate revoked",
            Kind::RevocationUnknown => "dicom dsig: no current certificate revocation list",
            Kind::CertificateUsage => "dicom dsig: certificate usage does not allow signing",
            Kind::X509UnknownAuthority => "x509: certificate signed by unknown authority",
            Kind::X509CertificateInvalid(_) => "x509: certificate invalid",
            Kind::X509UnhandledCriticalExtension => "x509: unhandled critical extension",
            Kind::ContextDeadline => "context deadline exceeded",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub kind: Kind,
    text: String,
    also: Vec<Kind>,
}
impl Error {
    pub(crate) fn bare(kind: Kind) -> Error {
        Error {
            kind,
            text: kind.text().to_string(),
            also: Vec::new(),
        }
    }
    pub(crate) fn new(kind: Kind, detail: impl fmt::Display) -> Error {
        Error {
            kind,
            text: format!("{}: {detail}", kind.text()),
            also: Vec::new(),
        }
    }
    pub(crate) fn other(text: impl Into<String>) -> Error {
        Error {
            kind: Kind::Other,
            text: text.into(),
            also: Vec::new(),
        }
    }
    pub(crate) fn prefixed(mut self, prefix: impl fmt::Display) -> Error {
        self.text = format!("{prefix}: {}", self.text);
        self
    }
    pub fn is(&self, kind: Kind) -> bool {
        self.kind == kind || self.also.contains(&kind)
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}
impl std::error::Error for Error {}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Code {
    pub value: String,
    pub scheme: String,
    pub meaning: String,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Signature {
    pub path: String,
    pub mac_id_number: u16,
    pub uid: String,
    pub date_time: String,
    pub certificate_type: String,
    pub certificate_of_signer: Vec<u8>,
    pub certificate: Option<Certificate>,
    pub signature: Vec<u8>,
    pub certified_timestamp_type: String,
    pub certified_timestamp: Vec<u8>,
    pub purposes: Vec<Code>,
    pub mac_algorithm: String,
    pub mac_transfer_syntax_uid: String,
    pub data_elements_signed: Vec<Tag>,
    mac_parameters_item_present: bool,
    data_set: Vec<Element>,
    item: Vec<Element>,
}
impl Signature {
    pub fn has_mac_parameters(&self) -> bool {
        self.mac_parameters_item_present
    }
}
fn find(elements: &[Element], tag: Tag) -> Option<&Element> {
    elements.iter().find(|e| e.tag() == tag)
}
fn sequence_items(elements: &[Element], tag: Tag) -> &[crate::object::DataSet] {
    match find(elements, tag).map(|e| &e.value) {
        Some(Value::Sequence(items)) => items,
        _ => &[],
    }
}
fn string_value(elements: &[Element], tag: Tag) -> String {
    let Some(element) = find(elements, tag) else {
        return String::new();
    };
    let first = match &element.value {
        Value::Raw(raw) => {
            let first = raw.split(|&b| b == b'\\').next().unwrap_or_default();
            String::from_utf8_lossy(trim_text_value_bytes(element.vr(), first)).into_owned()
        }
        Value::Strings(values) => values
            .first()
            .map(|v| trim_text_value(element.vr(), v).to_string())
            .unwrap_or_default(),
        _ => String::new(),
    };
    first.trim().to_string()
}
fn bytes_value(elements: &[Element], tag: Tag) -> Vec<u8> {
    match find(elements, tag).map(|e| &e.value) {
        Some(Value::Raw(raw)) => raw.clone(),
        _ => Vec::new(),
    }
}
fn uint16_value(elements: &[Element], tag: Tag) -> Option<u16> {
    match find(elements, tag).map(|e| &e.value) {
        Some(Value::Raw(raw)) if raw.len() == 2 => Some(u16::from_le_bytes([raw[0], raw[1]])),
        _ => None,
    }
}
fn tag_values(elements: &[Element], tag: Tag) -> Vec<Tag> {
    match find(elements, tag).map(|e| &e.value) {
        Some(Value::Raw(raw)) if raw.len() % 4 == 0 => raw
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| Tag {
                group: u16::from_le_bytes([b[0], b[1]]),
                element: u16::from_le_bytes([b[2], b[3]]),
            })
            .collect(),
        _ => Vec::new(),
    }
}
fn locations(elements: &[Element]) -> Vec<(String, &[Element])> {
    fn walk<'a>(prefix: &str, elements: &'a [Element], out: &mut Vec<(String, &'a [Element])>) {
        for element in elements {
            let tag = element.tag();
            if tag == TAG_DIGITAL_SIGNATURES_SEQUENCE || tag == TAG_MAC_PARAMETERS_SEQUENCE {
                continue;
            }
            let Value::Sequence(items) = &element.value else {
                continue;
            };
            for (i, item) in items.iter().enumerate() {
                let path = format!("{prefix}{tag}[{i}]");
                out.push((path.clone(), &item.elements));
                walk(&format!("{path}."), &item.elements, out);
            }
        }
    }
    let mut out = vec![(String::new(), elements)];
    walk("", elements, &mut out);
    out
}
pub fn signatures(elements: &[Element]) -> Vec<Signature> {
    let mut out = Vec::new();
    for (path, data_set) in locations(elements) {
        let mac_items = sequence_items(data_set, TAG_MAC_PARAMETERS_SEQUENCE);
        for item in sequence_items(data_set, TAG_DIGITAL_SIGNATURES_SEQUENCE) {
            let fields = &item.elements;
            let certificate_of_signer = bytes_value(fields, TAG_CERTIFICATE_OF_SIGNER);
            let mut sig = Signature {
                path: path.clone(),
                mac_id_number: uint16_value(fields, TAG_MAC_ID_NUMBER).unwrap_or(0),
                uid: string_value(fields, TAG_DIGITAL_SIGNATURE_UID),
                date_time: string_value(fields, TAG_DIGITAL_SIGNATURE_DATE_TIME),
                certificate_type: string_value(fields, TAG_CERTIFICATE_TYPE),
                certificate: parse_certificate(&certificate_of_signer),
                certificate_of_signer,
                signature: bytes_value(fields, TAG_SIGNATURE),
                certified_timestamp_type: string_value(fields, TAG_CERTIFIED_TIMESTAMP_TYPE),
                certified_timestamp: bytes_value(fields, TAG_CERTIFIED_TIMESTAMP),
                data_set: data_set.to_vec(),
                item: fields.clone(),
                ..Signature::default()
            };
            for code in sequence_items(fields, TAG_DIGITAL_SIGNATURE_PURPOSE_CODE_SEQ) {
                sig.purposes.push(Code {
                    value: string_value(&code.elements, TAG_CODE_VALUE),
                    scheme: string_value(&code.elements, TAG_CODING_SCHEME_DESIGNATOR),
                    meaning: string_value(&code.elements, TAG_CODE_MEANING),
                });
            }
            let mac = mac_items.iter().find(|mac| {
                uint16_value(&mac.elements, TAG_MAC_ID_NUMBER) == Some(sig.mac_id_number)
            });
            if let Some(mac) = mac {
                sig.mac_parameters_item_present = true;
                sig.mac_algorithm = string_value(&mac.elements, TAG_MAC_ALGORITHM);
                sig.mac_transfer_syntax_uid =
                    string_value(&mac.elements, TAG_MAC_CALCULATION_TRANSFER_SYNTAX);
                sig.data_elements_signed = tag_values(&mac.elements, TAG_DATA_ELEMENTS_SIGNED);
            }
            out.push(sig);
        }
    }
    out
}
fn compute_mac(sig: &Signature) -> Result<(Vec<u8>, MacAlgorithm), Error> {
    let Some(algorithm) = MacAlgorithm::parse(&sig.mac_algorithm) else {
        return Err(Error::new(
            Kind::Unsupported,
            format!("MAC Algorithm {}", quote(&sig.mac_algorithm)),
        ));
    };
    let syntax = stream::mac_transfer_syntax(&sig.mac_transfer_syntax_uid)?;
    let bytes = stream::signed_stream(&sig.data_set, &sig.data_elements_signed, &sig.item, syntax)?;
    Ok((algorithm.digest(&bytes), algorithm))
}
pub fn verify_signature(sig: &Signature) -> Result<(), Error> {
    if !sig.mac_parameters_item_present {
        return Err(Error::new(
            Kind::MalformedSignature,
            format!(
                "no MAC Parameters item with MAC ID Number {}",
                sig.mac_id_number
            ),
        ));
    }
    if sig.certificate_type != CERTIFICATE_TYPE_X509 {
        return Err(Error::new(
            Kind::Unsupported,
            format!("Certificate Type {}", quote(&sig.certificate_type)),
        ));
    }
    let Some(certificate) = &sig.certificate else {
        return Err(Error::new(
            Kind::MalformedSignature,
            "unreadable Certificate of Signer",
        ));
    };
    if sig.signature.is_empty() {
        return Err(Error::new(Kind::MalformedSignature, "empty Signature"));
    }
    let (mac, algorithm) = compute_mac(sig)?;
    let valid = match &certificate.public_key {
        PublicKey::Rsa { n, e } => [true, false].into_iter().any(|null_parameters| {
            let info = algorithm.digest_info(&mac, null_parameters);
            pubkey::rsa_verify(n, e, &info, &sig.signature)
        }),
        PublicKey::Ecdsa { curve, x, y } => match der::read(&sig.signature) {
            Some((outer, _padding)) => pubkey::ecdsa_verify(*curve, x, y, &mac, outer.full),
            None => false,
        },
        other => {
            return Err(Error::new(
                Kind::Unsupported,
                format!("{} signer key", other.type_label()),
            ));
        }
    };
    if valid {
        Ok(())
    } else {
        Err(Error::bare(Kind::SignatureInvalid))
    }
}
