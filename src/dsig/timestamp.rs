use super::asn1::{self, BIT_STRING, Fields, OCTET_STRING, Oid, SEQUENCE};
use super::cms::{self, Fail, OID_TST_INFO};
use super::sign::{Edit, parse_location, raw_element, string_element, update_location};
use super::stream::sorted_elements;
use super::{
    Certificate, Error, Kind, MacAlgorithm, PublicKey, Signature, TAG_CERTIFIED_TIMESTAMP,
    TAG_CERTIFIED_TIMESTAMP_TYPE, TAG_DIGITAL_SIGNATURE_UID, TAG_DIGITAL_SIGNATURES_SEQUENCE, der,
    find, signatures, string_value,
};
use crate::object::{DataSet, Element, Object, UNDEFINED_LENGTH, Value, Vr};
use crate::textfmt::quote;
use chrono::{DateTime, NaiveDate, SecondsFormat, Utc};
use num_bigint::BigInt;
pub const CERTIFIED_TIMESTAMP_CMS_TSP: &str = "CMS_TSP";
#[derive(Clone, Debug, Default)]
pub struct TimestampOptions {
    pub certificates: Vec<Certificate>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TimestampInfo {
    pub time: DateTime<Utc>,
    pub accuracy_nanos: i64,
    pub policy: Oid,
    pub serial_number: BigInt,
    pub nonce: Option<BigInt>,
    pub hash_algorithm: MacAlgorithm,
    pub certificate: Certificate,
    pub certificate_included: bool,
}
fn timestamped_value(sig: &Signature) -> &[u8] {
    let ecdsa = matches!(
        sig.certificate.as_ref().map(|c| &c.public_key),
        Some(PublicKey::Ecdsa { .. })
    );
    match der::read(&sig.signature) {
        Some((outer, _)) if ecdsa => outer.full,
        _ => &sig.signature,
    }
}
#[derive(Clone, Debug, Default)]
pub struct TimestampRequestOptions {
    pub hash: Option<MacAlgorithm>,
    pub policy: Oid,
    pub no_nonce: bool,
    pub no_certificate: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TimestampRequest {
    pub der: Vec<u8>,
    pub hash: MacAlgorithm,
    pub imprint: Vec<u8>,
    pub nonce: Option<BigInt>,
    pub policy: Oid,
    pub cert_req: bool,
}
pub fn new_timestamp_request(
    sig: &Signature,
    opts: &TimestampRequestOptions,
) -> Result<TimestampRequest, Error> {
    if sig.signature.is_empty() {
        return Err(Error::new(Kind::MalformedSignature, "empty Signature"));
    }
    let hash = opts.hash.unwrap_or(MacAlgorithm::Sha256);
    let Some(oid) = cms::oid_for_hash(hash) else {
        return Err(Error::new(
            Kind::Unsupported,
            format!("timestamp hash {hash:?}"),
        ));
    };
    let imprint = hash.digest(timestamped_value(sig));
    let nonce = (!opts.no_nonce).then(|| BigInt::from(uuid::Uuid::new_v4().as_u128() as u64));
    let mut algorithm = oid.encode();
    algorithm.extend_from_slice(&[0x05, 0x00]);
    let mut message_imprint = asn1::element(SEQUENCE, &algorithm);
    message_imprint.extend(asn1::element(OCTET_STRING, &imprint));
    let mut body = asn1::integer(&BigInt::from(1));
    body.extend(asn1::element(SEQUENCE, &message_imprint));
    if !opts.policy.is_empty() {
        body.extend(opts.policy.encode());
    }
    if let Some(nonce) = &nonce {
        body.extend(asn1::integer(nonce));
    }
    if !opts.no_certificate {
        body.extend(asn1::element(asn1::BOOLEAN, &[0xFF]));
    }
    Ok(TimestampRequest {
        der: asn1::element(SEQUENCE, &body),
        hash,
        imprint,
        nonce,
        policy: opts.policy.clone(),
        cert_req: !opts.no_certificate,
    })
}
fn message_imprint(f: &mut Fields<'_>) -> asn1::Result<(Oid, Vec<u8>)> {
    let imprint = f.required(SEQUENCE)?;
    let mut i = Fields::new(imprint.body);
    let algorithm = asn1::algorithm(&i.required(SEQUENCE)?)?;
    Ok((algorithm, i.octets()?.to_vec()))
}
pub fn parse_timestamp_request(der: &[u8]) -> Result<TimestampRequest, Error> {
    let parse = |outer| -> asn1::Result<_> {
        let mut f = asn1::sequence_fields(outer)?;
        let version = f.int()?;
        let (algorithm, imprint) = message_imprint(&mut f)?;
        let policy = f.optional_oid()?.unwrap_or_default();
        let nonce = f.optional_big_int()?;
        let cert_req = f.optional_bool()?;
        if let Some(extensions) = f.optional(0xA0)? {
            for extension in asn1::sequences(extensions.body)? {
                let mut e = Fields::new(extension.body);
                e.oid()?;
                e.optional_bool()?;
                e.octets()?;
            }
        }
        Ok((version, algorithm, imprint, policy, nonce, cert_req))
    };
    let (version, algorithm, imprint, policy, nonce, cert_req) = asn1::unmarshal(der, parse)
        .map_err(|err| Error::other(format!("dicom dsig: unreadable TimeStampReq: {err}")))?;
    if version != 1 {
        return Err(Error::other(format!(
            "dicom dsig: TimeStampReq version {version}"
        )));
    }
    let Some(hash) = cms::hash_for_oid(&algorithm) else {
        return Err(Error::new(
            Kind::Unsupported,
            format!("imprint hash {algorithm}"),
        ));
    };
    Ok(TimestampRequest {
        der: der.to_vec(),
        hash,
        imprint,
        nonce,
        policy,
        cert_req,
    })
}
impl TimestampRequest {
    pub fn for_signature(&self, sig: &Signature) -> bool {
        self.imprint == self.hash.digest(timestamped_value(sig))
    }
    pub fn check(
        &self,
        token: &[u8],
        certificates: &[Certificate],
    ) -> Result<TimestampInfo, (Option<Box<TimestampInfo>>, Error)> {
        let (imprint, info) =
            parse_timestamp_token(token, certificates).map_err(|fail| (None, invalid(fail)))?;
        let rejected = |detail: String| Error::new(Kind::TimestampResponse, detail);
        let err = if info.hash_algorithm != self.hash || imprint != self.imprint {
            rejected("the token certifies another imprint".to_string())
        } else if self.nonce != info.nonce {
            rejected("nonce does not match the request".to_string())
        } else if !self.policy.is_empty() && self.policy != info.policy {
            rejected(format!("policy {}, requested {}", info.policy, self.policy))
        } else if self.cert_req && !info.certificate_included {
            rejected("the TSA certificate was requested but not included".to_string())
        } else {
            return Ok(info);
        };
        Err((Some(Box::new(info)), err))
    }
}
fn pki_status_text(status: i64) -> String {
    match status {
        0 => "granted".to_string(),
        1 => "grantedWithMods".to_string(),
        2 => "rejection".to_string(),
        3 => "waiting".to_string(),
        4 => "revocationWarning".to_string(),
        5 => "revocationNotification".to_string(),
        _ => format!("status {status}"),
    }
}
fn to_valid_utf8(bytes: &[u8]) -> String {
    let mut out = String::new();
    let mut invalid_run = false;
    for chunk in bytes.utf8_chunks() {
        if !chunk.valid().is_empty() {
            out.push_str(chunk.valid());
            invalid_run = false;
        }
        if !chunk.invalid().is_empty() && !invalid_run {
            out.push('?');
            invalid_run = true;
        }
    }
    out
}
pub fn token_from_response(response: &[u8]) -> Result<Vec<u8>, Error> {
    let parse = |outer| -> asn1::Result<_> {
        let mut f = asn1::sequence_fields(outer)?;
        let status_info = f.required(SEQUENCE)?;
        let mut s = Fields::new(status_info.body);
        let status = s.int()?;
        let texts = match s.optional(SEQUENCE)? {
            Some(list) => asn1::elements(list.body)?,
            None => Vec::new(),
        };
        if let Some(fail_info) = s.optional(BIT_STRING)? {
            asn1::bit_string(fail_info.body)?;
        }
        Ok((status, texts, f.optional_any()?))
    };
    let (status, texts, token) = asn1::unmarshal(response, parse).map_err(|err| {
        Error::new(
            Kind::TimestampResponse,
            format!("unreadable TimeStampResp: {err}"),
        )
    })?;
    if status != 0 && status != 1 {
        let texts: Vec<String> = texts.iter().map(|t| to_valid_utf8(t.body)).collect();
        let mut text = texts.join("; ");
        if !text.is_empty() {
            text = format!(": {text}");
        }
        return Err(Error::new(
            Kind::TimestampResponse,
            format!("{}{text}", pki_status_text(status)),
        ));
    }
    match token {
        Some(token) => Ok(token.full.to_vec()),
        None => Err(Error::new(
            Kind::TimestampResponse,
            "granted without a token",
        )),
    }
}
fn invalid(fail: Fail) -> Error {
    let mut err = Error::new(Kind::TimestampInvalid, fail.text);
    err.also.extend(fail.kind);
    err
}
struct TstInfo {
    version: i64,
    policy: Oid,
    imprint_algorithm: Oid,
    imprint: Vec<u8>,
    serial_number: BigInt,
    gen_time: (u8, Vec<u8>),
    accuracy: (i64, i64, i64),
    nonce: Option<BigInt>,
}
fn tst_info(tlv: der::Tlv<'_>) -> asn1::Result<TstInfo> {
    let mut f = asn1::sequence_fields(tlv)?;
    let version = f.int()?;
    let policy = f.oid()?;
    let (imprint_algorithm, imprint) = message_imprint(&mut f)?;
    let serial_number = f.big_int()?;
    let gen_time = f.any()?;
    let mut accuracy = (0, 0, 0);
    if let Some(a) = f.optional(SEQUENCE)? {
        let mut a = Fields::new(a.body);
        let part = |tlv: Option<der::Tlv<'_>>| tlv.map_or(Ok(0), |t| asn1::int64(t.body));
        accuracy = (
            part(a.optional(asn1::INTEGER)?)?,
            part(a.optional(0x80)?)?,
            part(a.optional(0x81)?)?,
        );
    }
    f.optional_bool()?;
    let nonce = f.optional_big_int()?;
    f.optional_explicit(0)?;
    f.optional_context(1)?;
    Ok(TstInfo {
        version,
        policy,
        imprint_algorithm,
        imprint,
        serial_number,
        gen_time: (gen_time.tag, gen_time.body.to_vec()),
        accuracy,
        nonce,
    })
}
fn parse_generalized_time(tag: u8, body: &[u8]) -> Result<DateTime<Utc>, Fail> {
    if tag & 0xDF != 0x18 {
        return Err(Fail::new("genTime is not a GeneralizedTime"));
    }
    let text = String::from_utf8_lossy(body);
    let parse = || -> Option<DateTime<Utc>> {
        let bytes = text.as_bytes();
        let digits = bytes.get(..14)?;
        if !digits.iter().all(u8::is_ascii_digit) {
            return None;
        }
        let number = |from: usize, to: usize| text[from..to].parse::<u32>().ok();
        let mut rest = &bytes[14..];
        let mut nanos = 0u32;
        if let [b'.' | b',', digit, ..] = rest
            && digit.is_ascii_digit()
        {
            let count = rest[1..].iter().take_while(|b| b.is_ascii_digit()).count();
            for i in 0..9 {
                let digit = if i < count { rest[1 + i] - b'0' } else { 0 };
                nanos = nanos * 10 + u32::from(digit);
            }
            rest = &rest[1 + count..];
        }
        if rest != b"Z" {
            return None;
        }
        let date = NaiveDate::from_ymd_opt(number(0, 4)? as i32, number(4, 6)?, number(6, 8)?)?;
        let (hour, minute, second) = (number(8, 10)?, number(10, 12)?, number(12, 14)?);
        if hour > 23 || minute > 59 || second > 59 {
            return None;
        }
        Some(
            date.and_hms_nano_opt(hour, minute, second, nanos)?
                .and_utc(),
        )
    };
    parse().ok_or_else(|| Fail::new(format!("genTime {}: parsing time", quote(&text))))
}
fn parse_timestamp_token(
    token: &[u8],
    extra: &[Certificate],
) -> Result<(Vec<u8>, TimestampInfo), Fail> {
    let signed = cms::parse_signed_token(token, OID_TST_INFO, extra)?;
    let tst = cms::unmarshal("TSTInfo", &signed.content, tst_info)?;
    if tst.version != 1 {
        return Err(Fail::new(format!("TSTInfo version {}", tst.version)));
    }
    let Some(hash) = cms::hash_for_oid(&tst.imprint_algorithm) else {
        return Err(Fail::unsupported(format!(
            "imprint hash {}",
            tst.imprint_algorithm
        )));
    };
    let time = parse_generalized_time(tst.gen_time.0, &tst.gen_time.1)?;
    if !cms::has_critical_time_stamping_usage(&signed.signer) {
        return Err(Fail::new(
            "TSA certificate lacks a critical Extended Key Usage of timeStamping only (RFC 3161 2.3)",
        ));
    }
    let (seconds, millis, micros) = tst.accuracy;
    let info = TimestampInfo {
        time,
        accuracy_nanos: seconds
            .wrapping_mul(1_000_000_000)
            .wrapping_add(millis.wrapping_mul(1_000_000))
            .wrapping_add(micros.wrapping_mul(1_000)),
        policy: tst.policy,
        serial_number: tst.serial_number,
        nonce: tst.nonce,
        hash_algorithm: hash,
        certificate: signed.signer,
        certificate_included: signed.embedded,
    };
    Ok((tst.imprint, info))
}
pub fn verify_timestamp(
    sig: &Signature,
    opts: &TimestampOptions,
) -> Result<TimestampInfo, (Option<Box<TimestampInfo>>, Error)> {
    if sig.certified_timestamp.is_empty() {
        return Err((None, Error::bare(Kind::NoTimestamp)));
    }
    if sig.certified_timestamp_type != CERTIFIED_TIMESTAMP_CMS_TSP {
        return Err((
            None,
            Error::new(
                Kind::Unsupported,
                format!(
                    "Certified Timestamp Type {}",
                    quote(&sig.certified_timestamp_type)
                ),
            ),
        ));
    }
    let token = match der::read(&sig.certified_timestamp) {
        Some((outer, _)) => outer.full,
        None => &sig.certified_timestamp,
    };
    let (imprint, info) =
        parse_timestamp_token(token, &opts.certificates).map_err(|fail| (None, invalid(fail)))?;
    if imprint != info.hash_algorithm.digest(timestamped_value(sig)) {
        let err = Error::new(
            Kind::TimestampInvalid,
            "the token certifies another Signature value",
        );
        return Err((Some(Box::new(info)), err));
    }
    if let Some((not_before, not_after)) = sig.certificate.as_ref().and_then(|c| c.validity)
        && (info.time < not_before || info.time > not_after)
    {
        let err = Error::new(
            Kind::TimestampInvalid,
            format!(
                "certified time {} outside the signer certificate validity",
                info.time.to_rfc3339_opts(SecondsFormat::Secs, true)
            ),
        );
        return Err((Some(Box::new(info)), err));
    }
    Ok(info)
}
fn update_signature_item(
    elements: &[Element],
    uid: &str,
    update: &dyn Fn(&[Element]) -> Vec<Element>,
) -> Result<Edit, Error> {
    if let Some(element) = find(elements, TAG_DIGITAL_SIGNATURES_SEQUENCE)
        && let Value::Sequence(items) = &element.value
    {
        let mut items = items.clone();
        for item in &mut items {
            if string_value(&item.elements, TAG_DIGITAL_SIGNATURE_UID) != uid {
                continue;
            }
            let updated = update(&item.elements);
            *item = DataSet {
                elements: sorted_elements(&updated).into_iter().cloned().collect(),
                item_offset: None,
            };
            let mut header = element.header;
            header.length = UNDEFINED_LENGTH;
            header.length_set = true;
            return Ok(Edit {
                put: vec![Element {
                    header,
                    value: Value::Sequence(items),
                }],
                remove: Vec::new(),
            });
        }
    }
    Err(Error::new(
        Kind::MalformedSignature,
        format!("no signature with UID {uid} here"),
    ))
}
pub fn add_timestamp(
    obj: &mut Object,
    sig: &Signature,
    token: &[u8],
    certificates: &[Certificate],
) -> Result<Signature, Error> {
    let mut pending = sig.clone();
    pending.certified_timestamp_type = CERTIFIED_TIMESTAMP_CMS_TSP.to_string();
    pending.certified_timestamp = token.to_vec();
    let opts = TimestampOptions {
        certificates: certificates.to_vec(),
    };
    verify_timestamp(&pending, &opts).map_err(|(_, err)| err)?;
    let path = parse_location(&sig.path)?;
    let mut token = token.to_vec();
    if !token.len().is_multiple_of(2) {
        token.push(0);
    }
    update_location(obj, &path, &|elements| {
        update_signature_item(elements, &sig.uid, &|item| {
            let mut out: Vec<Element> = item
                .iter()
                .filter(|e| {
                    e.tag() != TAG_CERTIFIED_TIMESTAMP_TYPE && e.tag() != TAG_CERTIFIED_TIMESTAMP
                })
                .cloned()
                .collect();
            out.push(string_element(
                TAG_CERTIFIED_TIMESTAMP_TYPE,
                Vr::CS,
                CERTIFIED_TIMESTAMP_CMS_TSP,
            ));
            out.push(raw_element(TAG_CERTIFIED_TIMESTAMP, Vr::OB, token.clone()));
            out
        })
    })?;
    signatures(obj.elements())
        .into_iter()
        .find(|s| s.path == sig.path && s.uid == sig.uid)
        .ok_or_else(|| {
            Error::other(format!(
                "dicom dsig: signature {} not found at {}",
                sig.uid,
                quote(&sig.path)
            ))
        })
}
