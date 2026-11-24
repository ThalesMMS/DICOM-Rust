use super::{Error, Kind};
use crate::dsig::{Certificate, PublicKey};
use num_bigint::BigInt;
use rand_core::{OsRng, RngCore};
pub(crate) const MAX_CMS_RECIPIENTS: usize = 64;
const MAX_BER_DEPTH: usize = 32;
const OID_DATA: &[u64] = &[1, 2, 840, 113549, 1, 7, 1];
const OID_ENVELOPED: &[u64] = &[1, 2, 840, 113549, 1, 7, 3];
const OID_RSA_ENCRYPTION: &[u64] = &[1, 2, 840, 113549, 1, 1, 1];
const OID_RSAES_OAEP: &[u64] = &[1, 2, 840, 113549, 1, 1, 7];
const OID_MGF1: &[u64] = &[1, 2, 840, 113549, 1, 1, 8];
const OID_PSPECIFIED: &[u64] = &[1, 2, 840, 113549, 1, 1, 9];
const OID_SHA1: &[u64] = &[1, 3, 14, 3, 2, 26];
const OID_SHA256: &[u64] = &[2, 16, 840, 1, 101, 3, 4, 2, 1];
const OID_SHA384: &[u64] = &[2, 16, 840, 1, 101, 3, 4, 2, 2];
const OID_SHA512: &[u64] = &[2, 16, 840, 1, 101, 3, 4, 2, 3];
const OID_AES128_CBC: &[u64] = &[2, 16, 840, 1, 101, 3, 4, 1, 2];
const OID_AES192_CBC: &[u64] = &[2, 16, 840, 1, 101, 3, 4, 1, 22];
const OID_AES256_CBC: &[u64] = &[2, 16, 840, 1, 101, 3, 4, 1, 42];
const OID_DES_EDE3_CBC: &[u64] = &[1, 2, 840, 113549, 3, 7];
pub const KEY_TRANSPORT_RSA_OAEP_SHA256: &str = "rsaes-oaep-sha256";
pub const KEY_TRANSPORT_RSA_PKCS1V15: &str = "rsaes-pkcs1-v1_5";
const KEY_TRANSPORT_RSA_OAEP_SHA1: &str = "rsaes-oaep-sha1";
const KEY_TRANSPORT_RSA_OAEP_SHA384: &str = "rsaes-oaep-sha384";
const KEY_TRANSPORT_RSA_OAEP_SHA512: &str = "rsaes-oaep-sha512";
pub const CONTENT_ENCRYPTION_AES128_CBC: &str = "aes128-cbc";
pub const CONTENT_ENCRYPTION_AES192_CBC: &str = "aes192-cbc";
pub const CONTENT_ENCRYPTION_AES256_CBC: &str = "aes256-cbc";
const CONTENT_ENCRYPTION_DES_EDE3_CBC: &str = "des-ede3-cbc";
fn err(kind: Kind, detail: impl std::fmt::Display) -> Error {
    Error::new(kind, detail)
}
fn malformed(detail: &str) -> Error {
    err(Kind::MalformedEncryptedAttributes, detail)
}
fn unsupported(detail: impl std::fmt::Display) -> Error {
    err(Kind::UnsupportedEncryptedAttributes, detail)
}
fn ber_error() -> Error {
    malformed("BER encoding")
}
fn der_length(out: &mut Vec<u8>, n: usize) {
    if n < 0x80 {
        out.push(n as u8);
        return;
    }
    let bytes = (n as u64).to_be_bytes();
    let first = bytes.iter().position(|&b| b != 0).unwrap_or(7);
    out.push(0x80 | (8 - first) as u8);
    out.extend_from_slice(&bytes[first..]);
}
pub(crate) fn der_tlv(identifier: &[u8], content: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(identifier.len() + 6 + content.len());
    out.extend_from_slice(identifier);
    der_length(&mut out, content.len());
    out.extend_from_slice(content);
    out
}
fn seq(parts: &[&[u8]]) -> Vec<u8> {
    der_tlv(&[0x30], &parts.concat())
}
fn oid_der(arcs: &[u64]) -> Vec<u8> {
    let mut body = Vec::new();
    let mut push = |mut v: u64| {
        let mut tmp = vec![(v & 0x7f) as u8];
        v >>= 7;
        while v > 0 {
            tmp.push(0x80 | (v & 0x7f) as u8);
            v >>= 7;
        }
        tmp.reverse();
        body.extend_from_slice(&tmp);
    };
    push(arcs[0] * 40 + arcs[1]);
    for &a in &arcs[2..] {
        push(a);
    }
    der_tlv(&[0x06], &body)
}
fn int_der(v: &BigInt) -> Vec<u8> {
    der_tlv(&[0x02], &v.to_signed_bytes_be())
}
fn octets(v: &[u8]) -> Vec<u8> {
    der_tlv(&[0x04], v)
}
fn algorithm(oid: &[u64], parameters: &[u8]) -> Vec<u8> {
    seq(&[&oid_der(oid), parameters])
}
#[derive(Clone, Copy, Debug)]
pub(crate) struct Raw<'a> {
    pub class: u8,
    pub compound: bool,
    pub tag: u64,
    pub body: &'a [u8],
    pub full: &'a [u8],
}
type AsnResult<T> = Result<T, ()>;
fn base128(bytes: &[u8], mut offset: usize) -> AsnResult<(u64, usize)> {
    let mut ret: u64 = 0;
    let mut shifted = 0;
    while offset < bytes.len() {
        if shifted == 5 {
            return Err(());
        }
        ret <<= 7;
        let b = bytes[offset];
        if shifted == 0 && b == 0x80 {
            return Err(());
        }
        ret |= u64::from(b & 0x7f);
        offset += 1;
        if b & 0x80 == 0 {
            if ret > i32::MAX as u64 {
                return Err(());
            }
            return Ok((ret, offset));
        }
        shifted += 1;
    }
    Err(())
}
fn tag_and_length(bytes: &[u8], mut offset: usize) -> AsnResult<(u8, bool, u64, usize, usize)> {
    let b = *bytes.get(offset).ok_or(())?;
    offset += 1;
    let class = b >> 6;
    let compound = b & 0x20 != 0;
    let mut tag = u64::from(b & 0x1f);
    if tag == 0x1f {
        let (t, o) = base128(bytes, offset)?;
        if t < 0x1f {
            return Err(());
        }
        tag = t;
        offset = o;
    }
    let b = *bytes.get(offset).ok_or(())?;
    offset += 1;
    let length = if b & 0x80 == 0 {
        usize::from(b & 0x7f)
    } else {
        let n = usize::from(b & 0x7f);
        if n == 0 {
            return Err(());
        }
        let mut length: usize = 0;
        for _ in 0..n {
            let b = *bytes.get(offset).ok_or(())?;
            offset += 1;
            if length >= 1 << 23 {
                return Err(());
            }
            length = length << 8 | usize::from(b);
            if length == 0 {
                return Err(());
            }
        }
        if length < 0x80 {
            return Err(());
        }
        length
    };
    Ok((class, compound, tag, length, offset))
}
pub(crate) fn raw_at(bytes: &[u8], offset: usize) -> AsnResult<(Raw<'_>, usize)> {
    let (class, compound, tag, length, start) = tag_and_length(bytes, offset)?;
    let end = start.checked_add(length).ok_or(())?;
    if end > bytes.len() {
        return Err(());
    }
    Ok((
        Raw {
            class,
            compound,
            tag,
            body: &bytes[start..end],
            full: &bytes[offset..end],
        },
        end,
    ))
}
pub(crate) struct Fields<'a> {
    body: &'a [u8],
    offset: usize,
}
impl<'a> Fields<'a> {
    pub(crate) fn new(body: &'a [u8]) -> Fields<'a> {
        Fields { body, offset: 0 }
    }
    fn at_end(&self) -> bool {
        self.offset == self.body.len()
    }
    pub(crate) fn any(&mut self) -> AsnResult<Raw<'a>> {
        if self.at_end() {
            return Err(());
        }
        let (raw, next) = raw_at(self.body, self.offset)?;
        self.offset = next;
        Ok(raw)
    }
    pub(crate) fn universal(&mut self, tag: u64, compound: bool) -> AsnResult<Raw<'a>> {
        let raw = self.any()?;
        if raw.class != 0 || raw.tag != tag || raw.compound != compound {
            return Err(());
        }
        Ok(raw)
    }
    pub(crate) fn optional_implicit(&mut self, number: u64) -> AsnResult<Option<Raw<'a>>> {
        if self.at_end() {
            return Ok(None);
        }
        let (class, _, tag, _, _) = tag_and_length(self.body, self.offset)?;
        if class != 2 || tag != number {
            return Ok(None);
        }
        self.any().map(Some)
    }
    pub(crate) fn optional_explicit(&mut self, number: u64) -> AsnResult<Option<Raw<'a>>> {
        if self.at_end() {
            return Ok(None);
        }
        let (class, compound, tag, length, start) = tag_and_length(self.body, self.offset)?;
        if start == self.body.len() {
            return Err(());
        }
        if !(class == 2 && tag == number && (length == 0 || compound)) {
            return Ok(None);
        }
        if length == 0 {
            return Err(());
        }
        let (iclass, icompound, itag, ilength, istart) = tag_and_length(self.body, start)?;
        if !(iclass == 0 && itag == 16 && icompound) {
            return Ok(None);
        }
        let end = istart.checked_add(ilength).ok_or(())?;
        if end > self.body.len() {
            return Err(());
        }
        self.offset = end;
        Ok(Some(Raw {
            class: iclass,
            compound: icompound,
            tag: itag,
            body: &self.body[istart..end],
            full: &self.body[start..end],
        }))
    }
    pub(crate) fn explicit_raw(&mut self, number: u64) -> AsnResult<&'a [u8]> {
        if self.at_end() {
            return Err(());
        }
        let (class, compound, tag, length, start) = tag_and_length(self.body, self.offset)?;
        if start == self.body.len() {
            return Err(());
        }
        if !(class == 2 && tag == number && (length == 0 || compound)) {
            return Err(());
        }
        let end = start.checked_add(length).ok_or(())?;
        if end > self.body.len() {
            return Err(());
        }
        self.offset = end;
        Ok(&self.body[start..end])
    }
    pub(crate) fn int(&mut self) -> AsnResult<i64> {
        parse_int64(self.universal(2, false)?.body)
    }
    pub(crate) fn big_int(&mut self) -> AsnResult<BigInt> {
        let raw = self.universal(2, false)?;
        check_integer(raw.body)?;
        Ok(BigInt::from_signed_bytes_be(raw.body))
    }
    pub(crate) fn oid(&mut self) -> AsnResult<Vec<u64>> {
        parse_oid(self.universal(6, false)?.body)
    }
    pub(crate) fn octet_string(&mut self) -> AsnResult<&'a [u8]> {
        Ok(self.universal(4, false)?.body)
    }
    pub(crate) fn algorithm(&mut self) -> AsnResult<(Vec<u64>, &'a [u8])> {
        let raw = self.universal(16, true)?;
        algorithm_of(raw.body)
    }
}
fn algorithm_of(body: &[u8]) -> AsnResult<(Vec<u64>, &[u8])> {
    let mut f = Fields::new(body);
    let oid = f.oid()?;
    let parameters = if f.at_end() { &[][..] } else { f.any()?.full };
    Ok((oid, parameters))
}
fn check_integer(b: &[u8]) -> AsnResult<()> {
    if b.is_empty() {
        return Err(());
    }
    if b.len() > 1 && (b[0] == 0 && b[1] & 0x80 == 0 || b[0] == 0xff && b[1] & 0x80 == 0x80) {
        return Err(());
    }
    Ok(())
}
fn parse_int64(b: &[u8]) -> AsnResult<i64> {
    check_integer(b)?;
    if b.len() > 8 {
        return Err(());
    }
    let mut v: i64 = 0;
    for &x in b {
        v = v << 8 | i64::from(x);
    }
    let shift = 64 - b.len() * 8;
    Ok(if shift == 64 {
        0
    } else {
        (v << shift) >> shift
    })
}
pub(crate) fn parse_oid(bytes: &[u8]) -> AsnResult<Vec<u64>> {
    if bytes.is_empty() {
        return Err(());
    }
    let (v, mut offset) = base128(bytes, 0)?;
    let mut s = if v < 80 {
        vec![v / 40, v % 40]
    } else {
        vec![2, v - 80]
    };
    while offset < bytes.len() {
        let (v, o) = base128(bytes, offset)?;
        s.push(v);
        offset = o;
    }
    Ok(s)
}
pub(crate) fn oid_string(arcs: &[u64]) -> String {
    let parts: Vec<String> = arcs.iter().map(u64::to_string).collect();
    parts.join(".")
}
fn unmarshal_octets(bytes: &[u8]) -> AsnResult<&[u8]> {
    let (raw, next) = raw_at(bytes, 0)?;
    if next != bytes.len() || raw.class != 0 || raw.tag != 4 || raw.compound {
        return Err(());
    }
    Ok(raw.body)
}
fn unmarshal_sequence(bytes: &[u8]) -> AsnResult<&[u8]> {
    let (raw, next) = raw_at(bytes, 0)?;
    if next != bytes.len() || raw.class != 0 || raw.tag != 16 || !raw.compound {
        return Err(());
    }
    Ok(raw.body)
}
pub(crate) fn ber_to_der(data: &[u8]) -> Result<Vec<u8>, Error> {
    let (out, rest) = ber_element(data, 0)?;
    if rest.len() > 1 || (rest.len() == 1 && rest[0] != 0) {
        return Err(ber_error());
    }
    Ok(out)
}
fn ber_element(data: &[u8], depth: usize) -> Result<(Vec<u8>, &[u8]), Error> {
    if depth > MAX_BER_DEPTH {
        return Err(ber_error());
    }
    let identifier_end = ber_identifier_length(data)?;
    let identifier = &data[..identifier_end];
    let constructed = identifier[0] & 0x20 != 0;
    let (length, length_end, indefinite) = ber_length(&data[identifier_end..])?;
    let body = &data[identifier_end + length_end..];
    if indefinite && !constructed {
        return Err(ber_error());
    }
    if !constructed {
        if length > body.len() {
            return Err(ber_error());
        }
        return Ok((der_tlv(identifier, &body[..length]), &body[length..]));
    }
    let mut children = Vec::new();
    let rest;
    if indefinite {
        let mut r = body;
        loop {
            if r.len() >= 2 && r[0] == 0 && r[1] == 0 {
                r = &r[2..];
                break;
            }
            if r.is_empty() {
                return Err(ber_error());
            }
            let (child, next) = ber_element(r, depth + 1)?;
            children.push(child);
            r = next;
        }
        rest = r;
    } else {
        if length > body.len() {
            return Err(ber_error());
        }
        let mut inner = &body[..length];
        rest = &body[length..];
        while !inner.is_empty() {
            let (child, next) = ber_element(inner, depth + 1)?;
            children.push(child);
            inner = next;
        }
    }
    if identifier == [0x24] {
        let mut joined = Vec::new();
        for child in &children {
            let segment = unmarshal_octets(child).map_err(|_| ber_error())?;
            joined.extend_from_slice(segment);
        }
        return Ok((der_tlv(&[0x04], &joined), rest));
    }
    Ok((der_tlv(identifier, &children.concat()), rest))
}
fn ber_identifier_length(data: &[u8]) -> Result<usize, Error> {
    let first = *data.first().ok_or_else(ber_error)?;
    if first & 0x1f != 0x1f {
        return Ok(1);
    }
    for (index, b) in data.iter().enumerate().take(6).skip(1) {
        if b & 0x80 == 0 {
            return Ok(index + 1);
        }
    }
    Err(ber_error())
}
fn ber_length(data: &[u8]) -> Result<(usize, usize, bool), Error> {
    let first = *data.first().ok_or_else(ber_error)?;
    match first {
        f if f < 0x80 => return Ok((usize::from(f), 1, false)),
        0x80 => return Ok((0, 1, true)),
        0xff => return Err(ber_error()),
        _ => {}
    }
    let count = usize::from(first & 0x7f);
    if count > 4 || data.len() < 1 + count {
        return Err(ber_error());
    }
    let mut length: u64 = 0;
    for &octet in &data[1..1 + count] {
        length = length << 8 | u64::from(octet);
    }
    Ok((length as usize, 1 + count, false))
}
pub(crate) fn rsa_public_key(cert: &Certificate) -> Option<rsa::PublicKey> {
    let PublicKey::Rsa { n, e } = &cert.public_key else {
        return None;
    };
    Some(rsa::PublicKey::new_unchecked(
        rsa::BoxedUint::from_be_slice_vartime(&n.to_bytes_be()),
        rsa::BoxedUint::from_be_slice_vartime(&e.to_bytes_be()),
    ))
}
pub(crate) fn rsa_bits(cert: &Certificate) -> Option<u64> {
    match &cert.public_key {
        PublicKey::Rsa { n, .. } => Some(n.bits()),
        _ => None,
    }
}
pub(crate) fn key_usage(cert: &Certificate) -> u32 {
    const OID_KEY_USAGE: &[u8] = &[0x55, 0x1D, 0x0F];
    let Some(ext) = cert.extensions.iter().find(|e| e.id == OID_KEY_USAGE) else {
        return 0;
    };
    let Ok((raw, _)) = raw_at(&ext.value, 0) else {
        return 0;
    };
    if raw.tag != 3 || raw.body.is_empty() {
        return 0;
    }
    let bits = &raw.body[1..];
    let mut usage = 0u32;
    for i in 0..(bits.len() * 8).min(9) {
        if bits[i / 8] & (0x80 >> (i % 8)) != 0 {
            usage |= 1 << i;
        }
    }
    usage
}
pub(crate) const KEY_USAGE_KEY_ENCIPHERMENT: u32 = 1 << 2;
fn key_trans_recipient_info(
    cert: &Certificate,
    public_key: &rsa::PublicKey,
    key: &[u8],
    transport: &str,
    use_ski: bool,
) -> Result<(Vec<u8>, bool), Error> {
    let (version, rid) = if use_ski {
        if cert.subject_key_id.is_empty() {
            return Err(err(
                Kind::EncryptedAttributesRecipient,
                "certificate has no subject key identifier",
            ));
        }
        (2, der_tlv(&[0x80], &cert.subject_key_id))
    } else {
        (0, seq(&[&cert.raw_issuer, &int_der(&cert.serial_number)]))
    };
    let (algorithm_id, encrypted) = match transport {
        KEY_TRANSPORT_RSA_OAEP_SHA256 => {
            let hash = algorithm(OID_SHA256, &[]);
            let mgf = algorithm(OID_MGF1, &hash);
            let params = seq(&[&der_tlv(&[0xA0], &hash), &der_tlv(&[0xA1], &mgf)]);
            let encrypted = public_key
                .oaep_encrypt::<rsa::sha2::Sha256>(key)
                .map_err(|e| err(Kind::EncryptedAttributesRecipient, e))?;
            (algorithm(OID_RSAES_OAEP, &params), encrypted)
        }
        KEY_TRANSPORT_RSA_PKCS1V15 => {
            let encrypted = public_key
                .pkcs1v15_encrypt(key)
                .map_err(|e| err(Kind::EncryptedAttributesRecipient, e))?;
            (algorithm(OID_RSA_ENCRYPTION, &[0x05, 0x00]), encrypted)
        }
        other => {
            return Err(err(
                Kind::InvalidEncryptedAttributesOptions,
                format!("unsupported key transport {}", crate::textfmt::quote(other)),
            ));
        }
    };
    let info = seq(&[
        &int_der(&BigInt::from(version)),
        &rid,
        &algorithm_id,
        &octets(&encrypted),
    ]);
    Ok((info, version == 2))
}
pub(crate) fn content_encryption_parameters(
    content: &str,
) -> Result<(&'static [u64], usize), Error> {
    match content {
        CONTENT_ENCRYPTION_AES128_CBC => Ok((OID_AES128_CBC, 16)),
        CONTENT_ENCRYPTION_AES192_CBC => Ok((OID_AES192_CBC, 24)),
        CONTENT_ENCRYPTION_AES256_CBC => Ok((OID_AES256_CBC, 32)),
        other => Err(err(
            Kind::InvalidEncryptedAttributesOptions,
            format!(
                "unsupported content encryption {}",
                crate::textfmt::quote(other)
            ),
        )),
    }
}
fn pkcs7_pad(data: &[u8], block: usize) -> Vec<u8> {
    let padding = block - data.len() % block;
    let mut out = data.to_vec();
    out.resize(data.len() + padding, padding as u8);
    out
}
fn cbc_encrypt(key: &[u8], iv: &[u8], data: &mut [u8]) {
    use cbc::cipher::{BlockEncryptMut, KeyIvInit, block_padding::NoPadding};
    let len = data.len();
    let _ = match key.len() {
        16 => cbc::Encryptor::<aes::Aes128>::new_from_slices(key, iv)
            .map(|c| c.encrypt_padded_mut::<NoPadding>(data, len).map(|_| ())),
        24 => cbc::Encryptor::<aes::Aes192>::new_from_slices(key, iv)
            .map(|c| c.encrypt_padded_mut::<NoPadding>(data, len).map(|_| ())),
        _ => cbc::Encryptor::<aes::Aes256>::new_from_slices(key, iv)
            .map(|c| c.encrypt_padded_mut::<NoPadding>(data, len).map(|_| ())),
    };
}
pub(crate) fn encrypt_enveloped_data(
    plaintext: &[u8],
    recipients: &[(&Certificate, rsa::PublicKey)],
    transport: &str,
    content: &str,
    use_ski: bool,
) -> Result<Vec<u8>, Error> {
    if recipients.is_empty() || recipients.len() > MAX_CMS_RECIPIENTS {
        return Err(err(
            Kind::InvalidEncryptedAttributesOptions,
            format!("between 1 and {MAX_CMS_RECIPIENTS} recipients are required"),
        ));
    }
    let (content_oid, key_size) = content_encryption_parameters(content)?;
    let mut key = vec![0u8; key_size];
    OsRng.fill_bytes(&mut key);
    let mut iv = vec![0u8; 16];
    OsRng.fill_bytes(&mut iv);
    let mut ciphertext = pkcs7_pad(plaintext, 16);
    cbc_encrypt(&key, &iv, &mut ciphertext);
    let mut version = 0;
    let mut infos = Vec::with_capacity(recipients.len());
    for (cert, public_key) in recipients {
        let (info, v2) = key_trans_recipient_info(cert, public_key, &key, transport, use_ski)?;
        if v2 {
            version = 2;
        }
        infos.push(info);
    }
    key.iter_mut().for_each(|b| *b = 0);
    infos.sort();
    let enveloped = seq(&[
        &int_der(&BigInt::from(version)),
        &der_tlv(&[0x31], &infos.concat()),
        &seq(&[
            &oid_der(OID_DATA),
            &algorithm(content_oid, &octets(&iv)),
            &der_tlv(&[0x80], &ciphertext),
        ]),
    ]);
    Ok(seq(&[
        &oid_der(OID_ENVELOPED),
        &der_tlv(&[0xA0], &enveloped),
    ]))
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct EnvelopedAlgorithms {
    pub key_transport: String,
    pub content_encryption: String,
}
struct KeyTransRecipient<'a> {
    rid: Raw<'a>,
    algorithm: Vec<u64>,
    parameters: &'a [u8],
    encrypted_key: &'a [u8],
}
fn parse_ktri(full: &[u8]) -> AsnResult<KeyTransRecipient<'_>> {
    let body = unmarshal_sequence(full)?;
    let mut f = Fields::new(body);
    f.int()?;
    let rid = f.any()?;
    let (algorithm, parameters) = f.algorithm()?;
    let encrypted_key = f.octet_string()?;
    Ok(KeyTransRecipient {
        rid,
        algorithm,
        parameters,
        encrypted_key,
    })
}
fn recipient_identifies(rid: &Raw<'_>, cert: &Certificate) -> bool {
    if rid.class == 2 && rid.tag == 0 && !rid.compound {
        return !cert.subject_key_id.is_empty() && rid.body == cert.subject_key_id.as_slice();
    }
    if rid.class != 0 || rid.tag != 16 {
        return false;
    }
    let parsed = (|| -> AsnResult<(Raw<'_>, BigInt)> {
        let body = unmarshal_sequence(rid.full)?;
        let mut f = Fields::new(body);
        let issuer = f.any()?;
        let serial = f.big_int()?;
        Ok((issuer, serial))
    })();
    match parsed {
        Ok((issuer, serial)) => {
            issuer.full == cert.raw_issuer.as_slice() && serial == cert.serial_number
        }
        Err(()) => false,
    }
}
fn find_key_trans_recipient<'a>(
    infos: &[Raw<'a>],
    cert: &Certificate,
) -> Result<Option<KeyTransRecipient<'a>>, Error> {
    for raw in infos {
        if raw.class != 0 || raw.tag != 16 {
            continue;
        }
        let info = parse_ktri(raw.full).map_err(|_| malformed("key transport recipient"))?;
        if recipient_identifies(&info.rid, cert) {
            return Ok(Some(info));
        }
    }
    Ok(None)
}
#[derive(Clone, Copy)]
enum Hash {
    Sha1,
    Sha256,
    Sha384,
    Sha512,
}
fn oaep_hash(oid: &[u64]) -> Option<(Hash, &'static str)> {
    match oid {
        o if o == OID_SHA1 => Some((Hash::Sha1, KEY_TRANSPORT_RSA_OAEP_SHA1)),
        o if o == OID_SHA256 => Some((Hash::Sha256, KEY_TRANSPORT_RSA_OAEP_SHA256)),
        o if o == OID_SHA384 => Some((Hash::Sha384, KEY_TRANSPORT_RSA_OAEP_SHA384)),
        o if o == OID_SHA512 => Some((Hash::Sha512, KEY_TRANSPORT_RSA_OAEP_SHA512)),
        _ => None,
    }
}
fn oaep_decrypt(
    key: &rsa::PrivateKey,
    hash: Hash,
    mgf: Hash,
    label: Vec<u8>,
    ciphertext: &[u8],
) -> rsa::Result<Vec<u8>> {
    use rsa::sha2::{Sha256, Sha384, Sha512};
    use sha1::Sha1;
    let label = (!label.is_empty()).then(|| label.into_boxed_slice());
    macro_rules! decrypt {
        ($d:ty, $m:ty) => {{
            let mut padding = rsa::Oaep::<$d, $m>::new_with_mgf_hash();
            padding.label = label;
            key.decrypt(padding, ciphertext)
        }};
    }
    macro_rules! with_mgf {
        ($d:ty) => {
            match mgf {
                Hash::Sha1 => decrypt!($d, Sha1),
                Hash::Sha256 => decrypt!($d, Sha256),
                Hash::Sha384 => decrypt!($d, Sha384),
                Hash::Sha512 => decrypt!($d, Sha512),
            }
        };
    }
    match hash {
        Hash::Sha1 => with_mgf!(Sha1),
        Hash::Sha256 => with_mgf!(Sha256),
        Hash::Sha384 => with_mgf!(Sha384),
        Hash::Sha512 => with_mgf!(Sha512),
    }
}
const NULL_BYTES: &[u8] = &[0x05, 0x00];
#[allow(clippy::type_complexity)]
fn parse_oaep_parameters(raw: &[u8]) -> Result<(Hash, Hash, Vec<u8>, &'static str), Error> {
    let mut out = (
        Hash::Sha1,
        Hash::Sha1,
        Vec::new(),
        KEY_TRANSPORT_RSA_OAEP_SHA1,
    );
    if raw.is_empty() || raw == NULL_BYTES {
        return Ok(out);
    }
    let body = unmarshal_sequence(raw).map_err(|_| malformed("RSAES-OAEP parameters"))?;
    let parsed = (|| -> AsnResult<[Option<(Vec<u64>, &[u8])>; 3]> {
        let mut f = Fields::new(body);
        let mut fields = [None, None, None];
        for (i, slot) in fields.iter_mut().enumerate() {
            if let Some(inner) = f.optional_explicit(i as u64)? {
                if inner.class != 0 || inner.tag != 16 || !inner.compound {
                    return Err(());
                }
                *slot = Some(algorithm_of(inner.body)?);
            }
        }
        Ok(fields)
    })()
    .map_err(|_| malformed("RSAES-OAEP parameters"))?;
    let [hash_func, mask_gen, p_source] = parsed;
    if let Some((oid, _)) = hash_func {
        let Some((hash, name)) = oaep_hash(&oid) else {
            return Err(unsupported(format!("RSAES-OAEP hash {}", oid_string(&oid))));
        };
        out.0 = hash;
        out.1 = hash;
        out.3 = name;
    }
    if let Some((oid, parameters)) = mask_gen {
        if oid != OID_MGF1 {
            return Err(unsupported(format!(
                "RSAES-OAEP mask generation {}",
                oid_string(&oid)
            )));
        }
        let mgf = unmarshal_sequence(parameters)
            .and_then(algorithm_of)
            .map_err(|_| malformed("MGF1 parameters"))?;
        let Some((hash, _)) = oaep_hash(&mgf.0) else {
            return Err(unsupported(format!("MGF1 hash {}", oid_string(&mgf.0))));
        };
        out.1 = hash;
    }
    if let Some((oid, parameters)) = p_source {
        if oid != OID_PSPECIFIED {
            return Err(unsupported(format!(
                "RSAES-OAEP label source {}",
                oid_string(&oid)
            )));
        }
        let label = unmarshal_octets(parameters).map_err(|_| malformed("RSAES-OAEP label"))?;
        out.2 = label.to_vec();
    }
    Ok(out)
}
fn decrypt_error() -> Error {
    Error::bare(Kind::EncryptedAttributesDecrypt)
}
fn decrypt_content_key(
    info: &KeyTransRecipient<'_>,
    key: &rsa::PrivateKey,
    key_size: usize,
) -> Result<(Vec<u8>, &'static str), Error> {
    if info.algorithm == OID_RSA_ENCRYPTION {
        let content_key = key
            .decrypt(rsa::Pkcs1v15Encrypt, info.encrypted_key)
            .map_err(|_| decrypt_error())?;
        if content_key.len() != key_size {
            return Err(decrypt_error());
        }
        return Ok((content_key, KEY_TRANSPORT_RSA_PKCS1V15));
    }
    if info.algorithm == OID_RSAES_OAEP {
        let (hash, mgf, label, transport) = parse_oaep_parameters(info.parameters)?;
        let content_key =
            oaep_decrypt(key, hash, mgf, label, info.encrypted_key).map_err(|_| decrypt_error())?;
        if content_key.len() != key_size {
            return Err(decrypt_error());
        }
        return Ok((content_key, transport));
    }
    Err(unsupported(format!(
        "key-encryption algorithm {}",
        oid_string(&info.algorithm)
    )))
}
fn pkcs7_unpad(data: &[u8], block: usize) -> Option<&[u8]> {
    if data.is_empty() || !data.len().is_multiple_of(block) {
        return None;
    }
    let padding = usize::from(data[data.len() - 1]);
    let mut good = u8::from(padding >= 1) & u8::from(padding <= block);
    for index in 1..=block {
        let in_padding = index <= padding;
        let matches = data[data.len() - index] == padding as u8;
        good &= if in_padding { u8::from(matches) } else { 1 };
    }
    if good != 1 {
        return None;
    }
    Some(&data[..data.len() - padding])
}
fn cbc_decrypt(content: &str, key: &[u8], iv: &[u8], data: &mut [u8]) -> Result<(), Error> {
    use cbc::cipher::{BlockDecryptMut, KeyIvInit, block_padding::NoPadding};
    let r = match content {
        CONTENT_ENCRYPTION_AES128_CBC => cbc::Decryptor::<aes::Aes128>::new_from_slices(key, iv)
            .map(|c| c.decrypt_padded_mut::<NoPadding>(data).map(|_| ())),
        CONTENT_ENCRYPTION_AES192_CBC => cbc::Decryptor::<aes::Aes192>::new_from_slices(key, iv)
            .map(|c| c.decrypt_padded_mut::<NoPadding>(data).map(|_| ())),
        CONTENT_ENCRYPTION_AES256_CBC => cbc::Decryptor::<aes::Aes256>::new_from_slices(key, iv)
            .map(|c| c.decrypt_padded_mut::<NoPadding>(data).map(|_| ())),
        _ => cbc::Decryptor::<des::TdesEde3>::new_from_slices(key, iv)
            .map(|c| c.decrypt_padded_mut::<NoPadding>(data).map(|_| ())),
    };
    match r {
        Ok(Ok(())) => Ok(()),
        _ => Err(decrypt_error()),
    }
}
fn content_decryption_algorithm(oid: &[u64]) -> Result<(usize, usize, &'static str), Error> {
    match oid {
        o if o == OID_AES128_CBC => Ok((16, 16, CONTENT_ENCRYPTION_AES128_CBC)),
        o if o == OID_AES192_CBC => Ok((24, 16, CONTENT_ENCRYPTION_AES192_CBC)),
        o if o == OID_AES256_CBC => Ok((32, 16, CONTENT_ENCRYPTION_AES256_CBC)),
        o if o == OID_DES_EDE3_CBC => Ok((24, 8, CONTENT_ENCRYPTION_DES_EDE3_CBC)),
        _ => Err(unsupported(format!(
            "content-encryption algorithm {}",
            oid_string(oid)
        ))),
    }
}
fn encrypted_content_bytes(value: Option<Raw<'_>>) -> Result<Vec<u8>, Error> {
    let Some(value) = value.filter(|v| v.class == 2 && v.tag == 0) else {
        return Err(malformed("encrypted content is absent"));
    };
    if !value.compound {
        return Ok(value.body.to_vec());
    }
    let mut out = Vec::new();
    let mut offset = 0;
    while offset < value.body.len() {
        let (raw, next) =
            raw_at(value.body, offset).map_err(|_| malformed("encrypted content segment"))?;
        if raw.class != 0 || raw.tag != 4 || raw.compound {
            return Err(malformed("encrypted content segment"));
        }
        out.extend_from_slice(raw.body);
        offset = next;
    }
    Ok(out)
}
pub(crate) fn decrypt_enveloped_data(
    data: &[u8],
    cert: &Certificate,
    key: &rsa::PrivateKey,
) -> Result<(Vec<u8>, EnvelopedAlgorithms), Error> {
    let mut algorithms = EnvelopedAlgorithms::default();
    let der = ber_to_der(data)?;
    let (content_type, content) = (|| -> AsnResult<(Vec<u64>, &[u8])> {
        let body = unmarshal_sequence(&der)?;
        let mut f = Fields::new(body);
        let oid = f.oid()?;
        let content = f.explicit_raw(0)?;
        Ok((oid, content))
    })()
    .map_err(|_| malformed("content info"))?;
    if content_type != OID_ENVELOPED {
        return Err(unsupported("content type is not enveloped-data"));
    }
    let enveloped = (|| -> AsnResult<_> {
        let body = unmarshal_sequence(content)?;
        let mut f = Fields::new(body);
        f.int()?;
        f.optional_implicit(0)?;
        let set = f.universal(17, true)?;
        let mut infos = Vec::new();
        let mut offset = 0;
        while offset < set.body.len() {
            let (raw, next) = raw_at(set.body, offset)?;
            infos.push(raw);
            offset = next;
        }
        let eci = f.universal(16, true)?;
        let mut e = Fields::new(eci.body);
        let content_type = e.oid()?;
        let (algorithm, parameters) = e.algorithm()?;
        let encrypted = e.optional_implicit(0)?;
        f.optional_implicit(1)?;
        Ok((infos, content_type, algorithm, parameters, encrypted))
    })()
    .map_err(|_| malformed("enveloped-data"))?;
    let (infos, content_type, content_algorithm, parameters, encrypted) = enveloped;
    if infos.is_empty() || infos.len() > MAX_CMS_RECIPIENTS {
        return Err(malformed("recipient count"));
    }
    if content_type != OID_DATA {
        return Err(unsupported("encrypted content type is not data"));
    }
    let (key_size, block_size, content_name) = content_decryption_algorithm(&content_algorithm)?;
    algorithms.content_encryption = content_name.to_string();
    let iv = unmarshal_octets(parameters)
        .ok()
        .filter(|iv| iv.len() == block_size)
        .ok_or_else(|| malformed("content-encryption IV"))?;
    let ciphertext = encrypted_content_bytes(encrypted)?;
    if ciphertext.is_empty() || ciphertext.len() % block_size != 0 {
        return Err(malformed("encrypted content length"));
    }
    let Some(recipient) = find_key_trans_recipient(&infos, cert)? else {
        return Err(Error::bare(Kind::RecipientNotFound));
    };
    let (mut content_key, transport) = decrypt_content_key(&recipient, key, key_size)?;
    algorithms.key_transport = transport.to_string();
    let mut plaintext = ciphertext;
    let decrypted = cbc_decrypt(content_name, &content_key, iv, &mut plaintext);
    content_key.iter_mut().for_each(|b| *b = 0);
    decrypted?;
    let Some(unpadded) = pkcs7_unpad(&plaintext, block_size) else {
        return Err(decrypt_error());
    };
    let n = unpadded.len();
    plaintext.truncate(n);
    Ok((plaintext, algorithms))
}
