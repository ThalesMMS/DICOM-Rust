use super::cms::{
    self, CONTENT_ENCRYPTION_AES256_CBC, KEY_TRANSPORT_RSA_OAEP_SHA256, KEY_TRANSPORT_RSA_PKCS1V15,
    MAX_CMS_RECIPIENTS,
};
use super::{Error, Kind, ProfileLimits, TAG_PIXEL_DATA, pad_uid};
use crate::dsig::Certificate;
use crate::object::{
    self, DataSet, Element, ElementHeader, File, Object, ReadFileOptions, Tag, UNDEFINED_LENGTH,
    Value, Vr, transfer,
};
use crate::textfmt::quote;
use std::collections::{HashMap, HashSet};
pub(crate) const TAG_ENCRYPTED_ATTRIBUTES_SEQUENCE: Tag = Tag::new(0x0400, 0x0500);
const TAG_ENCRYPTED_CONTENT_TRANSFER_SYNTAX_UID: Tag = Tag::new(0x0400, 0x0510);
const TAG_ENCRYPTED_CONTENT: Tag = Tag::new(0x0400, 0x0520);
const TAG_MODIFIED_ATTRIBUTES_SEQUENCE: Tag = Tag::new(0x0400, 0x0550);
const TAG_SPECIFIC_CHARACTER_SET: Tag = Tag::new(0x0008, 0x0005);
const TAG_PATIENT_IDENTITY_REMOVED: Tag = Tag::new(0x0012, 0x0062);
const TAG_DEIDENTIFICATION_METHOD: Tag = Tag::new(0x0012, 0x0063);
const TAG_DEIDENTIFICATION_METHOD_CODE_SEQUENCE: Tag = Tag::new(0x0012, 0x0064);
const TAG_LONGITUDINAL_TEMPORAL_INFORMATION: Tag = Tag::new(0x0028, 0x0303);
pub(crate) const ENCRYPTED_ATTRIBUTES_METHOD: &str =
    "Encrypted Attributes Sequence for authorized re-identification";
const MINIMUM_RECIPIENT_KEY_BITS: u64 = 2048;
#[derive(Clone, Debug, Default)]
pub struct EncryptedAttributesOptions {
    pub recipients: Vec<Option<Certificate>>,
    pub key_transport: String,
    pub content_encryption: String,
    pub subject_key_identifier: bool,
}
impl EncryptedAttributesOptions {
    pub fn validate(&self) -> Result<(), Error> {
        normalize_encrypted_attributes_options(self).map(|_| ())
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EncryptedAttributesReport {
    pub recipients: i64,
    pub attributes: i64,
    pub key_transport: String,
    pub content_encryption: String,
    pub transfer_syntax_uid: String,
}
impl EncryptedAttributesReport {
    pub fn json(&self) -> String {
        format!(
            "{{\"recipients\":{},\"attributes\":{},\"key_transport\":{},\"content_encryption\":{},\"transfer_syntax_uid\":{}}}",
            self.recipients,
            self.attributes,
            super::json_string(&self.key_transport),
            super::json_string(&self.content_encryption),
            super::json_string(&self.transfer_syntax_uid)
        )
    }
}
fn options_error(detail: String) -> Error {
    Error::new(Kind::InvalidEncryptedAttributesOptions, detail)
}
fn recipient_error(detail: &str) -> Error {
    Error::new(Kind::EncryptedAttributesRecipient, detail)
}
pub(crate) fn normalize_encrypted_attributes_options(
    options: &EncryptedAttributesOptions,
) -> Result<EncryptedAttributesOptions, Error> {
    let mut out = options.clone();
    if out.key_transport.is_empty() {
        out.key_transport = KEY_TRANSPORT_RSA_OAEP_SHA256.into();
    }
    if out.content_encryption.is_empty() {
        out.content_encryption = CONTENT_ENCRYPTION_AES256_CBC.into();
    }
    if out.key_transport != KEY_TRANSPORT_RSA_OAEP_SHA256
        && out.key_transport != KEY_TRANSPORT_RSA_PKCS1V15
    {
        return Err(options_error(format!(
            "unsupported key transport {}",
            quote(&out.key_transport)
        )));
    }
    cms::content_encryption_parameters(&out.content_encryption)?;
    if out.recipients.is_empty() || out.recipients.len() > MAX_CMS_RECIPIENTS {
        return Err(options_error(format!(
            "between 1 and {MAX_CMS_RECIPIENTS} recipients are required"
        )));
    }
    for cert in &out.recipients {
        recipient_public_key(cert.as_ref(), out.subject_key_identifier)?;
    }
    Ok(out)
}
fn recipient_public_key(
    cert: Option<&Certificate>,
    use_ski: bool,
) -> Result<rsa::PublicKey, Error> {
    let Some(cert) = cert else {
        return Err(recipient_error("nil certificate"));
    };
    let (Some(key), Some(bits)) = (cms::rsa_public_key(cert), cms::rsa_bits(cert)) else {
        return Err(recipient_error("only RSA key transport is supported"));
    };
    if bits < MINIMUM_RECIPIENT_KEY_BITS {
        return Err(recipient_error(&format!(
            "RSA key shorter than {MINIMUM_RECIPIENT_KEY_BITS} bits"
        )));
    }
    let usage = cms::key_usage(cert);
    if usage != 0 && usage & cms::KEY_USAGE_KEY_ENCIPHERMENT == 0 {
        return Err(recipient_error("key usage does not allow key encipherment"));
    }
    if use_ski && cert.subject_key_id.is_empty() {
        return Err(recipient_error("certificate has no subject key identifier"));
    }
    if cert.raw_issuer.is_empty() {
        return Err(recipient_error(
            "certificate has no issuer or serial number",
        ));
    }
    Ok(key)
}
fn is_deidentification_marker(tag: Tag) -> bool {
    matches!(
        tag,
        TAG_PATIENT_IDENTITY_REMOVED
            | TAG_DEIDENTIFICATION_METHOD
            | TAG_DEIDENTIFICATION_METHOD_CODE_SEQUENCE
            | TAG_LONGITUDINAL_TEMPORAL_INFORMATION
    )
}
fn is_unencrypted_pixel_tag(tag: Tag) -> bool {
    tag == TAG_PIXEL_DATA || tag == Tag::new(0x7FE0, 0x0008) || tag == Tag::new(0x7FE0, 0x0009)
}
fn sequence_element(tag: Tag, items: Vec<DataSet>) -> Element {
    Element {
        header: ElementHeader {
            tag,
            vr: Vr::SQ,
            length: UNDEFINED_LENGTH,
            length_set: true,
        },
        value: Value::Sequence(items),
    }
}
fn raw_element(tag: Tag, vr: Vr, data: Vec<u8>) -> Element {
    Element {
        header: ElementHeader::new(tag, vr, data.len() as u32),
        value: Value::Raw(data),
    }
}
fn materialize_original(
    source: &Object,
    element: &Element,
    limits: &ProfileLimits,
) -> Result<Element, Error> {
    let mut out = element.clone();
    if matches!(out.value, Value::Nil) {
        if element.header.length != UNDEFINED_LENGTH
            && i64::from(element.header.length) > limits.max_value_bytes
        {
            return Err(Error::bare(Kind::ProfileResourceLimit));
        }
        let mut buffer = Vec::new();
        if let (_, Some(e)) = source.copy_value_to(element.tag(), &mut buffer) {
            return Err(Error::new(Kind::DeferredValueUnavailable, e));
        }
        out.value = Value::Raw(buffer);
    }
    if has_nested_deferred_value(&out) {
        return Err(Error::bare(Kind::DeferredValueUnavailable));
    }
    Ok(out)
}
fn has_nested_deferred_value(element: &Element) -> bool {
    let Value::Sequence(items) = &element.value else {
        return false;
    };
    items.iter().any(|item| {
        item.elements
            .iter()
            .any(|n| matches!(n.value, Value::Nil) || has_nested_deferred_value(n))
    })
}
pub(crate) fn apply_encrypted_attributes(
    options: &EncryptedAttributesOptions,
    limits: &ProfileLimits,
    source: &Object,
    planned: &mut Object,
) -> Result<EncryptedAttributesReport, Error> {
    let after: HashMap<Tag, Element> = planned
        .to_data_set()
        .elements
        .into_iter()
        .map(|e| (e.tag(), e))
        .collect();
    let mut originals: Vec<Element> = Vec::new();
    let mut character_set: Option<Element> = None;
    for element in source.to_data_set().elements {
        let tag = element.tag();
        if tag == TAG_SPECIFIC_CHARACTER_SET {
            character_set = Some(element.clone());
        }
        if is_unencrypted_pixel_tag(tag) {
            continue;
        }
        if let Some(replacement) = after.get(&tag)
            && !is_deidentification_marker(tag)
            && *replacement == element
        {
            continue;
        }
        originals.push(materialize_original(source, &element, limits)?);
    }
    if let Some(cs) = character_set
        && !originals.is_empty()
    {
        let original = materialize_original(source, &cs, limits)?;
        if !originals
            .iter()
            .any(|e| e.tag() == TAG_SPECIFIC_CHARACTER_SET)
        {
            originals.push(original);
        }
    }
    originals.sort_by_key(|e| e.tag());
    let attributes = originals.len() as i64;
    let item = encrypt_originals(originals, source.value_big_endian(), options)?;
    append_encrypted_attributes_item(planned, item);
    Ok(EncryptedAttributesReport {
        recipients: options.recipients.len() as i64,
        attributes,
        key_transport: options.key_transport.clone(),
        content_encryption: options.content_encryption.clone(),
        transfer_syntax_uid: transfer::EXPLICIT_VR_LITTLE_ENDIAN.uid.into(),
    })
}
fn encrypt_originals(
    originals: Vec<Element>,
    big_endian: bool,
    options: &EncryptedAttributesOptions,
) -> Result<DataSet, Error> {
    let mut plaintext = encode_modified_attributes(originals, big_endian)?;
    let mut recipients = Vec::with_capacity(options.recipients.len());
    for cert in &options.recipients {
        let key = recipient_public_key(cert.as_ref(), options.subject_key_identifier)?;
        recipients.push((cert.as_ref().expect("checked recipient"), key));
    }
    let content = cms::encrypt_enveloped_data(
        &plaintext,
        &recipients,
        &options.key_transport,
        &options.content_encryption,
        options.subject_key_identifier,
    );
    plaintext.iter_mut().for_each(|b| *b = 0);
    let mut content = content?;
    if content.len() % 2 == 1 {
        content.push(0);
    }
    Ok(DataSet {
        elements: vec![
            raw_element(
                TAG_ENCRYPTED_CONTENT_TRANSFER_SYNTAX_UID,
                Vr::UI,
                pad_uid(transfer::EXPLICIT_VR_LITTLE_ENDIAN.uid),
            ),
            raw_element(TAG_ENCRYPTED_CONTENT, Vr::OB, content),
        ],
        item_offset: None,
    })
}
fn encode_modified_attributes(originals: Vec<Element>, big_endian: bool) -> Result<Vec<u8>, Error> {
    let mut payload = Object::from_elements(vec![sequence_element(
        TAG_MODIFIED_ATTRIBUTES_SEQUENCE,
        vec![DataSet {
            elements: originals,
            item_offset: None,
        }],
    )]);
    payload.set_value_big_endian(big_endian);
    object::writer::write_data_set(&payload, transfer::EXPLICIT_VR_LITTLE_ENDIAN.uid).map_err(|e| {
        Error::with_text(
            Kind::Object,
            format!("deid: encode modified attributes: {e}"),
        )
    })
}
fn append_encrypted_attributes_item(obj: &mut Object, item: DataSet) {
    let mut items = match obj.get(TAG_ENCRYPTED_ATTRIBUTES_SEQUENCE).map(|e| &e.value) {
        Some(Value::Sequence(existing)) => existing.clone(),
        _ => Vec::new(),
    };
    items.push(item);
    obj.put(sequence_element(TAG_ENCRYPTED_ATTRIBUTES_SEQUENCE, items));
}
#[derive(Clone, Debug)]
pub struct RecipientKey {
    pub certificate: Option<Certificate>,
    pub private_key: Option<rsa::PrivateKey>,
}
impl RecipientKey {
    fn validate(&self) -> Result<(&Certificate, &rsa::PrivateKey), Error> {
        let (Some(cert), Some(key)) = (&self.certificate, &self.private_key) else {
            return Err(Error::new(
                Kind::RecipientKey,
                "certificate and private key are required",
            ));
        };
        let Some(public) = cms::rsa_public_key(cert) else {
            return Err(Error::new(
                Kind::RecipientKey,
                "only RSA key transport is supported",
            ));
        };
        use rsa::traits::PublicKeyParts;
        if public.n_bytes() != key.n_bytes() || public.e_bytes() != key.e_bytes() {
            return Err(Error::bare(Kind::RecipientKey));
        }
        Ok((cert, key))
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReidentificationReport {
    pub restored: i64,
    pub markers_removed: i64,
    pub remaining_items: i64,
    pub key_transport: String,
    pub content_encryption: String,
}
impl ReidentificationReport {
    pub fn json(&self) -> String {
        format!(
            "{{\"restored\":{},\"markers_removed\":{},\"remaining_items\":{},\"key_transport\":{},\"content_encryption\":{}}}",
            self.restored,
            self.markers_removed,
            self.remaining_items,
            super::json_string(&self.key_transport),
            super::json_string(&self.content_encryption)
        )
    }
}
struct DecryptedItem {
    index: usize,
    originals: DataSet,
    algorithms: cms::EnvelopedAlgorithms,
}
fn decrypt_latest_item(
    obj: &Object,
    key: &RecipientKey,
) -> Result<(DecryptedItem, Vec<DataSet>), Error> {
    let (cert, private_key) = key.validate()?;
    let Some(element) = obj.get(TAG_ENCRYPTED_ATTRIBUTES_SEQUENCE) else {
        return Err(Error::bare(Kind::NoEncryptedAttributes));
    };
    let items = match &element.value {
        Value::Sequence(items) if !items.is_empty() => items.clone(),
        _ => return Err(Error::bare(Kind::NoEncryptedAttributes)),
    };
    let mut unsupported_err = None;
    for index in (0..items.len()).rev() {
        let (syntax, content) = match encrypted_attributes_item(&items[index]) {
            Ok(v) => v,
            Err(e) => {
                unsupported_err.get_or_insert(e);
                continue;
            }
        };
        let (mut plaintext, algorithms) =
            match cms::decrypt_enveloped_data(&content, cert, private_key) {
                Ok(v) => v,
                Err(e) if e.is(Kind::RecipientNotFound) => continue,
                Err(e) => return Err(e),
            };
        let originals = decode_modified_attributes(&plaintext, syntax);
        plaintext.iter_mut().for_each(|b| *b = 0);
        return Ok((
            DecryptedItem {
                index,
                originals: originals?,
                algorithms,
            },
            items,
        ));
    }
    Err(unsupported_err.unwrap_or_else(|| Error::bare(Kind::RecipientNotFound)))
}
fn element_strings(element: &Element) -> Vec<String> {
    match &element.value {
        Value::Raw(raw) => object::text::raw_string_values(element.vr(), raw)
            .into_iter()
            .map(|v| String::from_utf8_lossy(&v).into_owned())
            .collect(),
        Value::Strings(values) => values
            .iter()
            .map(|v| object::text::trim_text_value(element.vr(), v).to_string())
            .collect(),
        _ => Vec::new(),
    }
}
fn encrypted_attributes_item(item: &DataSet) -> Result<(transfer::Syntax, Vec<u8>), Error> {
    let find = |tag: Tag| item.elements.iter().find(|e| e.tag() == tag);
    let (Some(syntax_element), Some(content_element)) = (
        find(TAG_ENCRYPTED_CONTENT_TRANSFER_SYNTAX_UID),
        find(TAG_ENCRYPTED_CONTENT),
    ) else {
        return Err(Error::new(
            Kind::MalformedEncryptedAttributes,
            "item lacks the transfer syntax or the content",
        ));
    };
    let values = element_strings(syntax_element);
    let uid = if values.len() == 1 {
        transfer::normalize_uid(&values[0]).to_string()
    } else {
        String::new()
    };
    let syntax = transfer::lookup(&uid)
        .filter(|s| s.explicit_vr && !s.big_endian && !is_media_payload(s.uid))
        .ok_or_else(|| {
            Error::new(
                Kind::UnsupportedEncryptedAttributes,
                "encrypted content transfer syntax",
            )
        })?;
    let content = match &content_element.value {
        Value::Raw(raw) if !raw.is_empty() => raw.clone(),
        _ => {
            return Err(Error::new(
                Kind::MalformedEncryptedAttributes,
                "encrypted content",
            ));
        }
    };
    Ok((syntax, content))
}
fn is_media_payload(uid: &str) -> bool {
    crate::transfer::default_registry()
        .get(uid)
        .is_some_and(|s| s.media_payload)
}
fn decode_modified_attributes(
    plaintext: &[u8],
    syntax: transfer::Syntax,
) -> Result<DataSet, Error> {
    let limits = ProfileLimits::defaults();
    let opts = ReadFileOptions {
        max_total_bytes: plaintext.len() as i64 + 1,
        max_element_bytes: limits.max_value_bytes,
        ..ReadFileOptions::default()
    };
    let source = crate::object::parser::source::MemFile::new(std::rc::Rc::from(plaintext));
    let dataset = object::read_data_set_with_options(Box::new(source), syntax, opts)
        .map_err(|e| Error::new(Kind::EncryptedAttributesDecrypt, e))?;
    let Some(element) = dataset.get(TAG_MODIFIED_ATTRIBUTES_SEQUENCE) else {
        return Err(Error::new(
            Kind::MalformedEncryptedAttributes,
            "no Modified Attributes Sequence",
        ));
    };
    let item = match &element.value {
        Value::Sequence(items) if items.len() == 1 => items[0].clone(),
        _ => {
            return Err(Error::new(
                Kind::MalformedEncryptedAttributes,
                "Modified Attributes Sequence must have one item",
            ));
        }
    };
    super::profile::validate_profile_budget(&item, &limits)?;
    for original in &item.elements {
        let tag = original.tag();
        if tag.group == 0x0002 || tag.group == 0xFFFE || matches!(original.value, Value::Nil) {
            return Err(Error::new(
                Kind::MalformedEncryptedAttributes,
                format!("{tag} cannot be restored"),
            ));
        }
    }
    Ok(item)
}
pub fn decrypt_encrypted_attributes(obj: &Object, key: &RecipientKey) -> Result<Object, Error> {
    let (decrypted, _) = decrypt_latest_item(obj, key)?;
    let mut originals = Object::from_data_set(decrypted.originals);
    originals.set_value_big_endian(false);
    Ok(originals)
}
pub fn reidentify(obj: &mut Object, key: &RecipientKey) -> Result<ReidentificationReport, Error> {
    let (decrypted, items) = decrypt_latest_item(obj, key)?;
    if obj.value_big_endian() {
        return Err(Error::new(
            Kind::UnsupportedEncryptedAttributes,
            "object is not little endian",
        ));
    }
    let mut report = ReidentificationReport {
        key_transport: decrypted.algorithms.key_transport.clone(),
        content_encryption: decrypted.algorithms.content_encryption.clone(),
        ..Default::default()
    };
    let remaining: Vec<DataSet> = items
        .into_iter()
        .enumerate()
        .filter(|(i, _)| *i != decrypted.index)
        .map(|(_, item)| item)
        .collect();
    if remaining.is_empty() {
        obj.remove(TAG_ENCRYPTED_ATTRIBUTES_SEQUENCE);
    } else {
        obj.put(sequence_element(
            TAG_ENCRYPTED_ATTRIBUTES_SEQUENCE,
            remaining,
        ));
    }
    let mut restored = HashSet::new();
    for original in decrypted.originals.elements {
        restored.insert(original.tag());
        obj.put(original);
        report.restored += 1;
    }
    for marker in [
        TAG_PATIENT_IDENTITY_REMOVED,
        TAG_DEIDENTIFICATION_METHOD,
        TAG_DEIDENTIFICATION_METHOD_CODE_SEQUENCE,
        TAG_LONGITUDINAL_TEMPORAL_INFORMATION,
    ] {
        if !restored.contains(&marker) && obj.remove(marker) {
            report.markers_removed += 1;
        }
    }
    if let Some(items) = obj.get_sequence(TAG_ENCRYPTED_ATTRIBUTES_SEQUENCE) {
        report.remaining_items = items.len() as i64;
    }
    Ok(report)
}
pub fn clone_file_reidentified(
    src: &File,
    key: &RecipientKey,
) -> Result<(File, ReidentificationReport), Error> {
    let mut clone = super::clone_file(src)?;
    let report = reidentify(&mut clone.dataset, key)?;
    let clone = super::rederive_file_meta(clone)?;
    Ok((clone, report))
}
pub fn reidentify_path(
    source: impl AsRef<std::path::Path>,
    destination: impl AsRef<std::path::Path>,
    key: &RecipientKey,
) -> (ReidentificationReport, Option<Error>) {
    let source = source.as_ref().to_string_lossy();
    let source: &str = &source;
    let destination = destination.as_ref().to_string_lossy();
    let destination: &str = &destination;
    let mut report = ReidentificationReport::default();
    let result = super::path::deidentify_path(source, destination, true, &mut |file| {
        report = reidentify(&mut file.dataset, key)?;
        Ok(())
    });
    (report, result.err())
}
struct PemBlock {
    kind: String,
    encrypted: bool,
    bytes: Vec<u8>,
}
fn pem_decode(data: &[u8]) -> Option<(PemBlock, &[u8])> {
    const BEGIN: &[u8] = b"-----BEGIN ";
    const DASHES: &[u8] = b"-----";
    let mut rest = data;
    loop {
        let start = if rest.starts_with(BEGIN) {
            0
        } else {
            let i = find(rest, b"\n-----BEGIN ")?;
            i + 1
        };
        let after = &rest[start + BEGIN.len()..];
        let line_end = after
            .iter()
            .position(|&b| b == b'\n')
            .unwrap_or(after.len());
        let line = trim_cr(&after[..line_end]);
        let Some(kind) = line.strip_suffix(DASHES) else {
            rest = &after[line_end.min(after.len())..];
            continue;
        };
        let kind = String::from_utf8_lossy(kind).into_owned();
        let mut body = &after[(line_end + 1).min(after.len())..];
        let mut encrypted = false;
        let mut cursor = body;
        let mut saw_header = false;
        loop {
            let end = cursor
                .iter()
                .position(|&b| b == b'\n')
                .unwrap_or(cursor.len());
            let header_line = trim_cr(&cursor[..end]);
            let Some(colon) = header_line.iter().position(|&b| b == b':') else {
                break;
            };
            saw_header = true;
            if header_line[..colon].trim_ascii() == b"DEK-Info" {
                encrypted = true;
            }
            cursor = &cursor[(end + 1).min(cursor.len())..];
        }
        if saw_header {
            let end = cursor
                .iter()
                .position(|&b| b == b'\n')
                .unwrap_or(cursor.len());
            if !trim_cr(&cursor[..end]).trim_ascii().is_empty() {
                rest = body;
                continue;
            }
            body = &cursor[(end + 1).min(cursor.len())..];
        }
        let end_marker = [b"-----END ".as_slice(), kind.as_bytes(), DASHES].concat();
        let Some(end_at) = find(body, &end_marker) else {
            rest = body;
            continue;
        };
        if end_at != 0 && body[end_at - 1] != b'\n' {
            rest = body;
            continue;
        }
        let base64: Vec<u8> = body[..end_at]
            .iter()
            .copied()
            .filter(|b| !matches!(b, b' ' | b'\t' | b'\r' | b'\n'))
            .collect();
        let Ok(bytes) = crate::dicomjson::sizedparse::base64_decode(&base64, false) else {
            rest = &body[end_at + end_marker.len()..];
            continue;
        };
        let mut tail = &body[end_at + end_marker.len()..];
        if let Some(stripped) = tail.strip_prefix(b"\r") {
            tail = stripped;
        }
        if let Some(stripped) = tail.strip_prefix(b"\n") {
            tail = stripped;
        }
        return Some((
            PemBlock {
                kind,
                encrypted,
                bytes,
            },
            tail,
        ));
    }
}
fn trim_cr(line: &[u8]) -> &[u8] {
    line.strip_suffix(b"\r").unwrap_or(line)
}
fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}
pub fn parse_recipient_certificates(data: &[u8]) -> Result<Vec<Certificate>, Error> {
    let mut certificates = Vec::new();
    let mut rest = data;
    while let Some((block, next)) = pem_decode(rest) {
        rest = next;
        if block.kind != "CERTIFICATE" {
            continue;
        }
        let Some(cert) = crate::dsig::parse_certificate(&block.bytes) else {
            return Err(recipient_error("x509: malformed certificate"));
        };
        certificates.push(cert);
    }
    if !certificates.is_empty() {
        return Ok(certificates);
    }
    match crate::dsig::parse_certificate(data) {
        Some(cert) if cert.raw.len() == data.len() => Ok(vec![cert]),
        _ => Err(recipient_error("no PEM or DER certificate")),
    }
}
const OID_RSA_ENCRYPTION_DER: &[u8] = &[0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x01, 0x01];
const OID_EC_PUBLIC_KEY_DER: &[u8] = &[0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x02, 0x01];
const OID_ED25519_DER: &[u8] = &[0x2B, 0x65, 0x70];
const OID_X25519_DER: &[u8] = &[0x2B, 0x65, 0x6E];
fn pkcs8_algorithm(der: &[u8]) -> Option<Vec<u8>> {
    let (outer, _) = cms::raw_at(der, 0).ok()?;
    let mut f = cms::Fields::new(outer.body);
    f.int().ok()?;
    let (oid, _) = f.algorithm().ok()?;
    let _ = f.octet_string().ok()?;
    let encoded = {
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
        push(oid[0] * 40 + oid[1]);
        for &a in &oid[2..] {
            push(a);
        }
        body
    };
    Some(encoded)
}
pub fn parse_recipient_key(
    certificate_data: &[u8],
    key_data: &[u8],
) -> Result<RecipientKey, Error> {
    let certificates = parse_recipient_certificates(certificate_data)?;
    if certificates.len() != 1 {
        return Err(Error::new(
            Kind::RecipientKey,
            "exactly one certificate is required",
        ));
    }
    let mut der = key_data.to_vec();
    if let Some((block, _)) = pem_decode(key_data) {
        if block.encrypted {
            return Err(Error::new(
                Kind::RecipientKey,
                "encrypted PEM keys are not supported",
            ));
        }
        der = block.bytes;
    }
    use rsa::pkcs1::DecodeRsaPrivateKey;
    use rsa::pkcs8::DecodePrivateKey;
    let private_key = match pkcs8_algorithm(&der) {
        Some(oid) if oid == OID_RSA_ENCRYPTION_DER => rsa::PrivateKey::from_pkcs8_der(&der)
            .or_else(|_| rsa::PrivateKey::from_pkcs1_der(&der))
            .ok(),
        Some(oid)
            if oid == OID_EC_PUBLIC_KEY_DER || oid == OID_ED25519_DER || oid == OID_X25519_DER =>
        {
            return Err(Error::new(
                Kind::RecipientKey,
                "only RSA keys are supported",
            ));
        }
        _ => rsa::PrivateKey::from_pkcs1_der(&der).ok(),
    };
    let Some(private_key) = private_key else {
        return Err(Error::new(Kind::RecipientKey, "unreadable private key"));
    };
    let key = RecipientKey {
        certificate: certificates.into_iter().next(),
        private_key: Some(private_key),
    };
    key.validate()?;
    Ok(key)
}
