use super::profile::{Profile, ProfileOptions, check_profile_with_options, sign_precheck};
use super::stream::{excluded, mac_transfer_syntax, sorted_elements};
use super::{
    CERTIFICATE_TYPE_X509, Certificate, Code, Curve, Error, Kind, MacAlgorithm, PublicKey,
    Signature, TAG_CERTIFICATE_OF_SIGNER, TAG_CERTIFICATE_TYPE, TAG_CODE_MEANING, TAG_CODE_VALUE,
    TAG_CODING_SCHEME_DESIGNATOR, TAG_DATA_ELEMENTS_SIGNED, TAG_DIGITAL_SIGNATURE_DATE_TIME,
    TAG_DIGITAL_SIGNATURE_PURPOSE_CODE_SEQ, TAG_DIGITAL_SIGNATURE_UID,
    TAG_DIGITAL_SIGNATURES_SEQUENCE, TAG_MAC_ALGORITHM, TAG_MAC_CALCULATION_TRANSFER_SYNTAX,
    TAG_MAC_ID_NUMBER, TAG_MAC_PARAMETERS_SEQUENCE, TAG_SIGNATURE, compute_mac, find,
    sequence_items, signatures, string_value, uint16_value,
};
use crate::attrpath::{self, Path, Segment};
use crate::dictionary::reservations::private_tag;
use crate::object::transfer::EXPLICIT_VR_LITTLE_ENDIAN;
use crate::object::{DataSet, Element, ElementHeader, Object, Tag, UNDEFINED_LENGTH, Value, Vr};
use crate::textfmt::quote;
use chrono::{DateTime, Utc};
use num_bigint::BigUint;
use rsa::pkcs8::DecodePrivateKey;
use rsa::traits::PublicKeyParts;
pub enum SigningKey {
    Rsa(Box<rsa::PrivateKey>),
    P224(p224::ecdsa::SigningKey),
    P256(p256::ecdsa::SigningKey),
    P384(p384::ecdsa::SigningKey),
    P521(Box<p521::ecdsa::SigningKey>),
}
impl SigningKey {
    pub fn from_pkcs8_der(der: &[u8]) -> Option<SigningKey> {
        if let Ok(key) = rsa::PrivateKey::from_pkcs8_der(der) {
            return Some(SigningKey::Rsa(Box::new(key)));
        }
        if let Ok(key) = p256::SecretKey::from_pkcs8_der(der) {
            return Some(SigningKey::P256(p256::ecdsa::SigningKey::from(&key)));
        }
        if let Ok(key) = p384::SecretKey::from_pkcs8_der(der) {
            return Some(SigningKey::P384(p384::ecdsa::SigningKey::from(&key)));
        }
        if let Ok(key) = p224::SecretKey::from_pkcs8_der(der) {
            return Some(SigningKey::P224(p224::ecdsa::SigningKey::from(&key)));
        }
        let key = p521::SecretKey::from_pkcs8_der(der).ok()?;
        let key = p521::ecdsa::SigningKey::from_bytes(&key.to_bytes()).ok()?;
        Some(SigningKey::P521(Box::new(key)))
    }
    fn public_key(&self) -> PublicKey {
        fn point(curve: Curve, sec1: &[u8]) -> PublicKey {
            let size = curve.byte_len();
            let coordinates = sec1.get(1..).unwrap_or_default();
            PublicKey::Ecdsa {
                curve,
                x: BigUint::from_bytes_be(coordinates.get(..size).unwrap_or_default()),
                y: BigUint::from_bytes_be(coordinates.get(size..).unwrap_or_default()),
            }
        }
        match self {
            SigningKey::Rsa(key) => PublicKey::Rsa {
                n: BigUint::from_bytes_be(&key.n_bytes()),
                e: BigUint::from_bytes_be(&key.e_bytes()),
            },
            SigningKey::P224(key) => point(
                Curve::P224,
                key.verifying_key().to_sec1_point(false).as_bytes(),
            ),
            SigningKey::P256(key) => point(
                Curve::P256,
                key.verifying_key().to_sec1_point(false).as_bytes(),
            ),
            SigningKey::P384(key) => point(
                Curve::P384,
                key.verifying_key().to_sec1_point(false).as_bytes(),
            ),
            SigningKey::P521(key) => point(
                Curve::P521,
                p521::ecdsa::VerifyingKey::from(key.as_ref())
                    .to_sec1_point(false)
                    .as_bytes(),
            ),
        }
    }
    fn sign(&self, mac: &[u8], algorithm: MacAlgorithm) -> Result<Vec<u8>, String> {
        use p256::ecdsa::signature::hazmat::PrehashSigner;
        let padded = |size: usize| {
            let mut out = vec![0u8; size.saturating_sub(mac.len())];
            out.extend_from_slice(mac);
            out
        };
        match self {
            SigningKey::Rsa(key) => {
                let bits = key.n().bit_width_vartime();
                if bits < 1024 {
                    return Err(format!(
                        "rsa: {bits}-bit keys are insecure (the minimum is 1024 bits)"
                    ));
                }
                let info = algorithm.digest_info(mac, true);
                key.sign(rsa::Pkcs1v15Sign::new_unprefixed(), &info)
                    .map_err(|e| e.to_string())
            }
            SigningKey::P224(key) => {
                let sig: p224::ecdsa::Signature =
                    key.sign_prehash(&padded(28)).map_err(|e| e.to_string())?;
                Ok(sig.to_der().as_bytes().to_vec())
            }
            SigningKey::P256(key) => {
                let sig: p256::ecdsa::Signature =
                    key.sign_prehash(&padded(32)).map_err(|e| e.to_string())?;
                Ok(sig.to_der().as_bytes().to_vec())
            }
            SigningKey::P384(key) => {
                let sig: p384::ecdsa::Signature =
                    key.sign_prehash(&padded(48)).map_err(|e| e.to_string())?;
                Ok(sig.to_der().as_bytes().to_vec())
            }
            SigningKey::P521(key) => {
                let sig: p521::ecdsa::Signature =
                    key.sign_prehash(&padded(66)).map_err(|e| e.to_string())?;
                Ok(sig.to_der().as_bytes().to_vec())
            }
        }
    }
}
pub struct SignOptions {
    pub certificate: Certificate,
    pub key: SigningKey,
    pub mac_algorithm: String,
    pub mac_transfer_syntax_uid: String,
    pub tags: Option<Vec<Tag>>,
    pub purpose: Option<Code>,
    pub profile: Profile,
    pub profile_options: ProfileOptions,
    pub time: DateTime<Utc>,
    pub uid: String,
    pub item: String,
}
pub(super) fn string_element(tag: Tag, vr: Vr, value: &str) -> Element {
    Element {
        header: ElementHeader {
            tag,
            vr,
            length: 0,
            length_set: false,
        },
        value: Value::Strings(vec![value.to_string()]),
    }
}
pub(super) fn raw_element(tag: Tag, vr: Vr, raw: Vec<u8>) -> Element {
    Element {
        header: ElementHeader::new(tag, vr, raw.len() as u32),
        value: Value::Raw(raw),
    }
}
fn sequence_element(tag: Tag, items: Vec<DataSet>) -> Element {
    Element {
        header: ElementHeader::new(tag, Vr::SQ, UNDEFINED_LENGTH),
        value: Value::Sequence(items),
    }
}
fn item_not_found(detail: impl std::fmt::Display) -> Error {
    Error::new(Kind::ItemNotFound, detail)
}
pub(super) fn parse_location(text: &str) -> Result<Path, Error> {
    if text.is_empty() {
        return Ok(Path::default());
    }
    let path = attrpath::parse(text).map_err(item_not_found)?;
    if !path.selects_item() || path.wildcard() {
        return Err(item_not_found(format!(
            "{} does not select one item",
            quote(text)
        )));
    }
    Ok(path)
}
fn segment_tag(elements: &[Element], segment: &Segment) -> Result<Tag, Error> {
    if !segment.private() {
        return Ok(segment.tag);
    }
    let object = Object::from_elements(elements.to_vec());
    private_tag(
        &object,
        segment.tag.group,
        &segment.creator,
        segment.tag.element as u8,
    )
    .map_err(item_not_found)
}
fn segment_item<'a>(
    elements: &'a [Element],
    path: &Path,
    depth: usize,
) -> Result<(&'a Element, &'a [DataSet]), Error> {
    let segment = &path.0[depth];
    let tag = segment_tag(elements, segment)?;
    match find(elements, tag) {
        Some(element) => match &element.value {
            Value::Sequence(items) if segment.item < items.len() => Ok((element, items)),
            _ => Err(item_not_found(Path(path.0[..=depth].to_vec()))),
        },
        None => Err(item_not_found(Path(path.0[..=depth].to_vec()))),
    }
}
fn location_elements(obj: &Object, path: &Path) -> Result<Vec<Element>, Error> {
    let mut elements: &[Element] = obj.elements();
    for depth in 0..path.0.len() {
        let (_, items) = segment_item(elements, path, depth)?;
        elements = &items[path.0[depth].item].elements;
    }
    Ok(elements.to_vec())
}
#[derive(Default)]
pub(super) struct Edit {
    pub put: Vec<Element>,
    pub remove: Vec<Tag>,
}
impl Edit {
    fn apply(self, elements: &[Element]) -> Vec<Element> {
        let dropped =
            |tag: Tag| self.remove.contains(&tag) || self.put.iter().any(|e| e.tag() == tag);
        let mut out: Vec<Element> = elements
            .iter()
            .filter(|e| !dropped(e.tag()))
            .cloned()
            .collect();
        out.extend(self.put);
        out.sort_by_key(|e| (e.tag().group, e.tag().element));
        out
    }
}
pub(super) fn update_location(
    obj: &mut Object,
    path: &Path,
    change: &dyn Fn(&[Element]) -> Result<Edit, Error>,
) -> Result<(), Error> {
    if path.is_empty() {
        let edit = change(obj.elements())?;
        for tag in edit.remove {
            obj.remove(tag);
        }
        for element in edit.put {
            obj.put(element);
        }
        return Ok(());
    }
    fn rebuild(
        elements: &[Element],
        path: &Path,
        depth: usize,
        change: &dyn Fn(&[Element]) -> Result<Edit, Error>,
    ) -> Result<Element, Error> {
        let (element, items) = segment_item(elements, path, depth)?;
        let index = path.0[depth].item;
        let item = &items[index].elements;
        let edit = if depth == path.0.len() - 1 {
            change(item)?
        } else {
            Edit {
                put: vec![rebuild(item, path, depth + 1, change)?],
                remove: Vec::new(),
            }
        };
        let mut items = items.to_vec();
        items[index] = DataSet {
            elements: edit.apply(item),
            item_offset: None,
        };
        let mut header = element.header;
        header.length = UNDEFINED_LENGTH;
        header.length_set = true;
        Ok(Element {
            header,
            value: Value::Sequence(items),
        })
    }
    let top = rebuild(obj.elements(), path, 0, change)?;
    obj.put(top);
    Ok(())
}
fn signed_tags(elements: &[Element], requested: Option<&[Tag]>) -> Result<Vec<Tag>, Error> {
    let mut wanted: Option<Vec<Tag>> = requested.map(<[Tag]>::to_vec);
    let mut tags = Vec::new();
    for element in sorted_elements(elements) {
        let tag = element.tag();
        if wanted.as_ref().is_some_and(|w| !w.contains(&tag)) {
            continue;
        }
        if excluded(element) {
            if wanted.is_some() {
                return Err(Error::other(format!("dicom dsig: {tag} may not be signed")));
            }
            continue;
        }
        tags.push(tag);
        if let Some(wanted) = &mut wanted {
            wanted.retain(|t| *t != tag);
        }
    }
    if let Some(missing) = wanted.as_ref().and_then(|w| w.first()) {
        return Err(Error::other(format!(
            "dicom dsig: {missing} is not in the data set"
        )));
    }
    if tags.is_empty() {
        return Err(Error::other("dicom dsig: no data element to sign"));
    }
    Ok(tags)
}
fn append_item(elements: &[Element], tag: Tag, item: Vec<Element>) -> Element {
    let mut items = sequence_items(elements, tag).to_vec();
    items.push(DataSet {
        elements: item,
        item_offset: None,
    });
    sequence_element(tag, items)
}
pub fn sign(obj: &mut Object, opts: &SignOptions) -> Result<Signature, Error> {
    if opts.key.public_key() != opts.certificate.public_key {
        return Err(Error::other(
            "dicom dsig: the signer key does not match the certificate",
        ));
    }
    let mac_algorithm = if opts.mac_algorithm.is_empty() {
        "SHA256"
    } else {
        &opts.mac_algorithm
    };
    let syntax_uid = if opts.mac_transfer_syntax_uid.is_empty() {
        EXPLICIT_VR_LITTLE_ENDIAN.uid
    } else {
        &opts.mac_transfer_syntax_uid
    };
    if MacAlgorithm::parse(mac_algorithm).is_none() {
        return Err(Error::new(
            Kind::Unsupported,
            format!("MAC Algorithm {}", quote(mac_algorithm)),
        ));
    }
    mac_transfer_syntax(syntax_uid)?;
    if obj.value_big_endian() {
        return Err(Error::new(Kind::Unsupported, "big endian values"));
    }
    let location = parse_location(&opts.item)?;
    let elements = location_elements(obj, &location)?;
    if opts.profile != Profile::None {
        let listed = opts.tags.as_ref().map_or(0, Vec::len);
        sign_precheck(&elements, listed, &opts.profile_options).map_err(Error::from)?;
    }
    let tags = signed_tags(&elements, opts.tags.as_deref())?;
    let mut mac_id: u16 = 1;
    for item in sequence_items(&elements, TAG_MAC_PARAMETERS_SEQUENCE) {
        if let Some(id) = uint16_value(&item.elements, TAG_MAC_ID_NUMBER)
            && id >= mac_id
        {
            mac_id = id.wrapping_add(1);
        }
    }
    let us = |value: u16| raw_element(TAG_MAC_ID_NUMBER, Vr::US, value.to_le_bytes().to_vec());
    let mut signed_raw = Vec::with_capacity(tags.len() * 4);
    for tag in &tags {
        signed_raw.extend_from_slice(&tag.group.to_le_bytes());
        signed_raw.extend_from_slice(&tag.element.to_le_bytes());
    }
    let mac_item = vec![
        us(mac_id),
        string_element(TAG_MAC_CALCULATION_TRANSFER_SYNTAX, Vr::UI, syntax_uid),
        string_element(TAG_MAC_ALGORITHM, Vr::CS, mac_algorithm),
        raw_element(TAG_DATA_ELEMENTS_SIGNED, Vr::AT, signed_raw),
    ];
    let date_time = opts.time.format("%Y%m%d%H%M%S%.6f+0000").to_string();
    let mut signature_item = vec![
        us(mac_id),
        string_element(TAG_DIGITAL_SIGNATURE_UID, Vr::UI, &opts.uid),
        string_element(TAG_DIGITAL_SIGNATURE_DATE_TIME, Vr::DT, &date_time),
        string_element(TAG_CERTIFICATE_TYPE, Vr::CS, CERTIFICATE_TYPE_X509),
    ];
    if let Some(purpose) = &opts.purpose {
        signature_item.push(sequence_element(
            TAG_DIGITAL_SIGNATURE_PURPOSE_CODE_SEQ,
            vec![DataSet {
                elements: vec![
                    string_element(TAG_CODE_VALUE, Vr::SH, &purpose.value),
                    string_element(TAG_CODING_SCHEME_DESIGNATOR, Vr::SH, &purpose.scheme),
                    string_element(TAG_CODE_MEANING, Vr::LO, &purpose.meaning),
                ],
                item_offset: None,
            }],
        ));
    }
    let pending = Signature {
        path: location.to_string(),
        mac_id_number: mac_id,
        uid: opts.uid.clone(),
        certificate_type: CERTIFICATE_TYPE_X509.to_string(),
        certificate_of_signer: opts.certificate.raw.clone(),
        certificate: Some(opts.certificate.clone()),
        purposes: opts.purpose.iter().cloned().collect(),
        mac_algorithm: mac_algorithm.to_string(),
        mac_transfer_syntax_uid: syntax_uid.to_string(),
        data_elements_signed: tags,
        mac_parameters_item_present: true,
        data_set: elements,
        item: signature_item.clone(),
        ..Signature::default()
    };
    if opts.profile != Profile::None
        && let (_, Some(err)) =
            check_profile_with_options(&pending, opts.profile, &opts.profile_options)
    {
        return Err(Error::from(err));
    }
    let (mac, algorithm) = compute_mac(&pending)?;
    let signature = opts
        .key
        .sign(&mac, algorithm)
        .map_err(|e| Error::other(format!("dicom dsig: sign: {e}")))?;
    signature_item.push(raw_element(
        TAG_CERTIFICATE_OF_SIGNER,
        Vr::OB,
        opts.certificate.raw.clone(),
    ));
    signature_item.push(raw_element(TAG_SIGNATURE, Vr::OB, signature));
    signature_item.sort_by_key(|e| (e.tag().group, e.tag().element));
    update_location(obj, &location, &|elements| {
        Ok(Edit {
            put: vec![
                append_item(elements, TAG_MAC_PARAMETERS_SEQUENCE, mac_item.clone()),
                append_item(
                    elements,
                    TAG_DIGITAL_SIGNATURES_SEQUENCE,
                    signature_item.clone(),
                ),
            ],
            remove: Vec::new(),
        })
    })?;
    signatures(obj.elements())
        .into_iter()
        .find(|sig| sig.uid == opts.uid)
        .ok_or_else(|| Error::other("dicom dsig: signature not found after signing"))
}
pub fn remove_signature(obj: &mut Object, sig: &Signature) -> Result<(), Error> {
    let known = signatures(obj.elements())
        .iter()
        .any(|s| s.path == sig.path && s.uid == sig.uid);
    if !known {
        return Err(Error::other(format!(
            "dicom dsig: signature {} not found at {}",
            sig.uid,
            quote(&sig.path)
        )));
    }
    let path = parse_location(&sig.path)?;
    update_location(obj, &path, &|elements| {
        let mut removed_mac_id: Option<u16> = None;
        let mut remaining_ids: Vec<u16> = Vec::new();
        let mut kept_signatures = Vec::new();
        for item in sequence_items(elements, TAG_DIGITAL_SIGNATURES_SEQUENCE) {
            let id = uint16_value(&item.elements, TAG_MAC_ID_NUMBER).unwrap_or(0);
            if removed_mac_id.is_none()
                && string_value(&item.elements, TAG_DIGITAL_SIGNATURE_UID) == sig.uid
            {
                removed_mac_id = Some(id);
                continue;
            }
            remaining_ids.push(id);
            kept_signatures.push(item.clone());
        }
        let kept_macs: Vec<DataSet> = sequence_items(elements, TAG_MAC_PARAMETERS_SEQUENCE)
            .iter()
            .filter(
                |item| match uint16_value(&item.elements, TAG_MAC_ID_NUMBER) {
                    Some(id) => removed_mac_id != Some(id) || remaining_ids.contains(&id),
                    None => true,
                },
            )
            .cloned()
            .collect();
        let mut edit = Edit::default();
        for (tag, items) in [
            (TAG_MAC_PARAMETERS_SEQUENCE, kept_macs),
            (TAG_DIGITAL_SIGNATURES_SEQUENCE, kept_signatures),
        ] {
            if items.is_empty() {
                edit.remove.push(tag);
            } else {
                edit.put.push(sequence_element(tag, items));
            }
        }
        Ok(edit)
    })
}
pub fn remove_all_signatures(obj: &mut Object) -> Result<usize, Error> {
    let list = signatures(obj.elements());
    for sig in &list {
        remove_signature(obj, sig)?;
    }
    Ok(list.len())
}
