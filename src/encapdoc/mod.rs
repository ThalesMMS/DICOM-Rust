#![forbid(unsafe_code)]
mod cda;
mod payload;
mod urlrules;
use crate::object::element_setters::{sequence_element, ul_element};
use crate::object::inherit::{apply_derived_common, inherit_modules};
use crate::object::text::trim_text_value;
use crate::object::types::{Element, ElementHeader, Tag, Value, Vr};
use crate::object::{File, Object, element_length, is_valid_uid, transfer};
pub use cda::{
    CDA_NARRATIVE_CAPTION, CDA_NARRATIVE_LIST, CDA_NARRATIVE_LIST_ITEM, CDA_NARRATIVE_PARAGRAPH,
    CDA_NARRATIVE_SECTION, CDA_NARRATIVE_TABLE, CDA_NARRATIVE_TABLE_ROW, CDA_PROFILE_BASE_HEADER,
    CDA_PROFILE_CCD_HEADER, CdaNarrative, CdaNarrativeBlock, CdaNarrativeDiagnostic,
    CdaNarrativeLimits, CdaValidationFinding, CdaValidationOptions, CdaValidationReport,
    default_cda_narrative_limits, parse_cda_narrative, parse_encapsulated_cda_narrative,
    validate_cda,
};
pub use payload::{PayloadInfo, PayloadLimits, inspect_payload};
use std::fmt;
pub use urlrules::model_relative_uri;
pub const ENCAPSULATED_PDF_STORAGE: &str = "1.2.840.10008.5.1.4.1.1.104.1";
pub const ENCAPSULATED_CDA_STORAGE: &str = "1.2.840.10008.5.1.4.1.1.104.2";
pub const ENCAPSULATED_STL_STORAGE: &str = "1.2.840.10008.5.1.4.1.1.104.3";
pub const ENCAPSULATED_OBJ_STORAGE: &str = "1.2.840.10008.5.1.4.1.1.104.4";
pub const ENCAPSULATED_MTL_STORAGE: &str = "1.2.840.10008.5.1.4.1.1.104.5";
pub const MIME_TYPE_PDF: &str = "application/pdf";
pub const MIME_TYPE_CDA: &str = "text/XML";
pub const MIME_TYPE_STL: &str = "model/stl";
pub const MIME_TYPE_OBJ: &str = "model/obj";
pub const MIME_TYPE_MTL: &str = "model/mtl";
pub const BURNED_IN_ANNOTATION_YES: &str = "YES";
pub const BURNED_IN_ANNOTATION_NO: &str = "NO";
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    InvalidDocument,
    MissingHl7InstanceIdentifier,
    MissingFrameOfReference,
    UnsupportedSopClass,
    TooLarge,
    InvalidBurnedInAnnotation,
    CdaIdentityMismatch,
    DocumentLength,
    CdaNarrativeAbsent,
    CdaNarrativeUnsupported,
    UnsupportedCdaValidationProfile,
    Object,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub kind: Kind,
    msg: String,
    pub stored_bytes: i64,
    pub declared_bytes: i64,
}
impl Error {
    fn new(kind: Kind, msg: impl Into<String>) -> Error {
        Error {
            kind,
            msg: msg.into(),
            stored_bytes: 0,
            declared_bytes: 0,
        }
    }
    pub fn sentinel(kind: Kind) -> Error {
        let text = match kind {
            Kind::InvalidDocument => "encapdoc: invalid encapsulated document",
            Kind::MissingHl7InstanceIdentifier => "encapdoc: CDA needs an HL7 Instance Identifier",
            Kind::MissingFrameOfReference => "encapdoc: 3D model needs a Frame of Reference UID",
            Kind::UnsupportedSopClass => "encapdoc: not an encapsulated document",
            Kind::TooLarge => "encapdoc: encapsulated document is too large",
            Kind::InvalidBurnedInAnnotation => "encapdoc: Burned In Annotation must be YES or NO",
            Kind::CdaIdentityMismatch => {
                "encapdoc: invalid encapsulated document: HL7 Instance Identifier disagrees with the CDA document id"
            }
            Kind::DocumentLength => "encapdoc: invalid encapsulated document",
            Kind::CdaNarrativeAbsent => "encapdoc: no readable CDA narrative",
            Kind::CdaNarrativeUnsupported => "encapdoc: unsupported CDA narrative",
            Kind::UnsupportedCdaValidationProfile => "encapdoc: unsupported CDA validation profile",
            Kind::Object => "encapdoc: object error",
        };
        Error::new(kind, text)
    }
    fn wrap(kind: Kind, suffix: &str) -> Error {
        let base = Error::sentinel(kind);
        Error::new(kind, format!("{}: {suffix}", base.msg))
    }
    fn object(err: crate::object::Error) -> Error {
        Error::new(Kind::Object, err.to_string())
    }
    pub fn is(&self, kind: Kind) -> bool {
        self.kind == kind
            || kind == Kind::InvalidDocument
                && matches!(self.kind, Kind::CdaIdentityMismatch | Kind::DocumentLength)
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.msg)
    }
}
impl std::error::Error for Error {}
pub type Result<T> = std::result::Result<T, Error>;
const fn t(group: u16, element: u16) -> Tag {
    Tag::new(group, element)
}
const TAG_SPECIFIC_CHARACTER_SET: Tag = t(0x0008, 0x0005);
const TAG_SOP_CLASS_UID: Tag = t(0x0008, 0x0016);
const TAG_SOP_INSTANCE_UID: Tag = t(0x0008, 0x0018);
const TAG_CONTENT_DATE: Tag = t(0x0008, 0x0023);
const TAG_ACQUISITION_DATE_TIME: Tag = t(0x0008, 0x002A);
const TAG_CONTENT_TIME: Tag = t(0x0008, 0x0033);
const TAG_MODALITY: Tag = t(0x0008, 0x0060);
const TAG_CONVERSION_TYPE: Tag = t(0x0008, 0x0064);
const TAG_CODE_VALUE: Tag = t(0x0008, 0x0100);
const TAG_CODING_SCHEME_DESIGNATOR: Tag = t(0x0008, 0x0102);
const TAG_CODE_MEANING: Tag = t(0x0008, 0x0104);
const TAG_SERIES_DESCRIPTION: Tag = t(0x0008, 0x103E);
const TAG_PATIENT_NAME: Tag = t(0x0010, 0x0010);
const TAG_PATIENT_ID: Tag = t(0x0010, 0x0020);
const TAG_STUDY_INSTANCE_UID: Tag = t(0x0020, 0x000D);
const TAG_SERIES_INSTANCE_UID: Tag = t(0x0020, 0x000E);
const TAG_SERIES_NUMBER: Tag = t(0x0020, 0x0011);
const TAG_INSTANCE_NUMBER: Tag = t(0x0020, 0x0013);
const TAG_FRAME_OF_REFERENCE_UID: Tag = t(0x0020, 0x0052);
const TAG_POSITION_REFERENCE_INDICATOR: Tag = t(0x0020, 0x1040);
const TAG_ICC_PROFILE: Tag = t(0x0028, 0x2000);
const TAG_COLOR_SPACE: Tag = t(0x0028, 0x2002);
const TAG_BURNED_IN_ANNOTATION: Tag = t(0x0028, 0x0301);
const TAG_MEASUREMENT_UNITS_CODE_SEQ: Tag = t(0x0040, 0x08EA);
const TAG_CONCEPT_NAME_CODE_SEQUENCE: Tag = t(0x0040, 0xA043);
const TAG_HL7_INSTANCE_IDENTIFIER: Tag = t(0x0040, 0xE001);
const TAG_DOCUMENT_TITLE: Tag = t(0x0042, 0x0010);
const TAG_ENCAPSULATED_DOCUMENT: Tag = t(0x0042, 0x0011);
const TAG_MIME_TYPE: Tag = t(0x0042, 0x0012);
const TAG_ENCAPSULATED_DOCUMENT_LENGTH: Tag = t(0x0042, 0x0015);
pub use crate::object::inherit::DerivedCommon;
#[derive(Clone, Debug, Default)]
pub struct DocumentOptions {
    pub sop_instance_uid: String,
    pub study_instance_uid: String,
    pub series_instance_uid: String,
    pub patient_name: String,
    pub patient_id: String,
    pub series_description: String,
    pub series_number: String,
    pub instance_number: String,
    pub content_date: String,
    pub content_time: String,
    pub acquisition_date_time: String,
    pub burned_in_annotation: String,
    pub document_title: String,
    pub data: Vec<u8>,
    pub common: DerivedCommon,
}
#[derive(Clone, Debug, Default)]
pub struct CdaOptions {
    pub document: DocumentOptions,
    pub hl7_instance_identifier: String,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Code {
    pub value: String,
    pub scheme: String,
    pub meaning: String,
}
pub fn millimetre() -> Code {
    Code {
        value: "mm".into(),
        scheme: "UCUM".into(),
        meaning: "millimeter".into(),
    }
}
#[derive(Clone, Debug, Default)]
pub struct ModelOptions {
    pub document: DocumentOptions,
    pub frame_of_reference_uid: String,
    pub measurement_units: Code,
    pub icc_profile: Option<Vec<u8>>,
}
type Validator = fn(&[u8]) -> Result<()>;
#[derive(Clone, Copy)]
pub(crate) struct DocKind {
    pub(crate) sop_class_uid: &'static str,
    pub(crate) mime_type: &'static str,
    modality: &'static str,
    burned_in_default: &'static str,
    secondary_cap: bool,
    frame_of_ref: bool,
    icc_profile: bool,
    validate: Option<Validator>,
}
pub(crate) const KIND_PDF: DocKind = DocKind {
    sop_class_uid: ENCAPSULATED_PDF_STORAGE,
    mime_type: MIME_TYPE_PDF,
    modality: "DOC",
    burned_in_default: BURNED_IN_ANNOTATION_YES,
    secondary_cap: true,
    frame_of_ref: false,
    icc_profile: false,
    validate: None,
};
pub(crate) const KIND_CDA: DocKind = DocKind {
    sop_class_uid: ENCAPSULATED_CDA_STORAGE,
    mime_type: MIME_TYPE_CDA,
    ..KIND_PDF
};
pub(crate) const KIND_STL: DocKind = DocKind {
    sop_class_uid: ENCAPSULATED_STL_STORAGE,
    mime_type: MIME_TYPE_STL,
    modality: "M3D",
    burned_in_default: BURNED_IN_ANNOTATION_NO,
    secondary_cap: false,
    frame_of_ref: true,
    icc_profile: false,
    validate: Some(validate_stl),
};
pub(crate) const KIND_OBJ: DocKind = DocKind {
    sop_class_uid: ENCAPSULATED_OBJ_STORAGE,
    mime_type: MIME_TYPE_OBJ,
    icc_profile: true,
    validate: Some(validate_text),
    ..KIND_STL
};
pub(crate) const KIND_MTL: DocKind = DocKind {
    sop_class_uid: ENCAPSULATED_MTL_STORAGE,
    mime_type: MIME_TYPE_MTL,
    frame_of_ref: false,
    ..KIND_OBJ
};
fn string_element(tag: Tag, vr: Vr, value: &str) -> Element {
    Element {
        header: ElementHeader {
            tag,
            vr,
            length: 0,
            length_set: false,
        },
        value: Value::Strings(vec![value.trim().to_string()]),
    }
}
fn raw_element(tag: Tag, vr: Vr, raw: Vec<u8>) -> Element {
    Element {
        header: ElementHeader::new(tag, vr, raw.len() as u32),
        value: Value::Raw(raw),
    }
}
fn default_string(value: &str, fallback: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        fallback.to_string()
    } else {
        value.to_string()
    }
}
pub fn new_pdf(opts: &DocumentOptions) -> Result<File> {
    build(KIND_PDF, opts, |_| Ok(()))
}
pub fn new_cda(opts: &CdaOptions) -> Result<File> {
    let source_identifier = cda::cda_instance_identifier(&opts.document.data)?;
    let mut identifier = opts.hl7_instance_identifier.trim().to_string();
    if identifier.is_empty() {
        identifier = source_identifier.clone();
    }
    if !source_identifier.is_empty() {
        compare_cda_identifiers(&identifier, &source_identifier)?;
    }
    if identifier.is_empty() {
        return Err(Error::sentinel(Kind::MissingHl7InstanceIdentifier));
    }
    build(KIND_CDA, &opts.document, |dataset| {
        dataset.put(string_element(
            TAG_HL7_INSTANCE_IDENTIFIER,
            Vr::ST,
            &identifier,
        ));
        Ok(())
    })
}
pub fn new_stl(opts: &ModelOptions) -> Result<File> {
    build_model(KIND_STL, opts)
}
pub fn new_obj(opts: &ModelOptions) -> Result<File> {
    build_model(KIND_OBJ, opts)
}
pub fn new_mtl(opts: &ModelOptions) -> Result<File> {
    build_model(KIND_MTL, opts)
}
fn build_model(k: DocKind, opts: &ModelOptions) -> Result<File> {
    let mut units = opts.measurement_units.clone();
    if units == Code::default() {
        units = millimetre();
    }
    if units.value.trim().is_empty()
        || units.scheme.trim().is_empty()
        || units.meaning.trim().is_empty()
    {
        return Err(Error::wrap(
            Kind::InvalidDocument,
            &format!(
                "incomplete measurement units {{Value:{} Scheme:{} Meaning:{}}}",
                units.value, units.scheme, units.meaning
            ),
        ));
    }
    build(k, &opts.document, |dataset| {
        let item = Object::from_elements(vec![
            string_element(TAG_CODE_VALUE, Vr::SH, &units.value),
            string_element(TAG_CODING_SCHEME_DESIGNATOR, Vr::SH, &units.scheme),
            string_element(TAG_CODE_MEANING, Vr::LO, &units.meaning),
        ]);
        dataset.put(sequence_element(
            TAG_MEASUREMENT_UNITS_CODE_SEQ,
            &[Some(&item)],
        ));
        if k.icc_profile {
            let profile = match &opts.icc_profile {
                Some(p) => p.clone(),
                None => {
                    dataset.put(string_element(TAG_COLOR_SPACE, Vr::CS, "SRGB"));
                    crate::icc::iccprofile::srgb()
                }
            };
            dataset.put(raw_element(TAG_ICC_PROFILE, Vr::OB, profile));
        }
        if !k.frame_of_ref {
            return Ok(());
        }
        let uid = opts.frame_of_reference_uid.trim();
        if !uid.is_empty() {
            dataset.put(string_element(TAG_FRAME_OF_REFERENCE_UID, Vr::UI, uid));
            if !dataset.has(TAG_POSITION_REFERENCE_INDICATOR) {
                dataset.put(string_element(TAG_POSITION_REFERENCE_INDICATOR, Vr::LO, ""));
            }
        }
        match dataset.get_uid(TAG_FRAME_OF_REFERENCE_UID) {
            Some(uid) if !uid.trim().is_empty() => Ok(()),
            _ => Err(Error::sentinel(Kind::MissingFrameOfReference)),
        }
    })
}
fn build(
    k: DocKind,
    opts: &DocumentOptions,
    extra: impl FnOnce(&mut Object) -> Result<()>,
) -> Result<File> {
    if opts.sop_instance_uid.trim().is_empty() {
        return Err(Error::object(crate::object::Error::sentinel(
            crate::object::Kind::MissingSopInstanceUid,
        )));
    }
    let mut burned = crate::textfmt::to_upper(opts.burned_in_annotation.trim());
    if burned.is_empty() {
        burned = k.burned_in_default.to_string();
    }
    if burned != BURNED_IN_ANNOTATION_YES && burned != BURNED_IN_ANNOTATION_NO {
        return Err(Error::wrap(
            Kind::InvalidBurnedInAnnotation,
            &crate::textfmt::quote(&opts.burned_in_annotation),
        ));
    }
    if let Some(validate) = k.validate {
        validate(&opts.data)?;
    }
    if opts.data.len() as u64 > u32::MAX as u64 {
        return Err(Error::wrap(
            Kind::TooLarge,
            &format!("{} bytes", opts.data.len()),
        ));
    }
    let mut elements = vec![
        string_element(TAG_SPECIFIC_CHARACTER_SET, Vr::CS, "ISO_IR 192"),
        string_element(TAG_SOP_CLASS_UID, Vr::UI, k.sop_class_uid),
        string_element(TAG_SOP_INSTANCE_UID, Vr::UI, &opts.sop_instance_uid),
        string_element(TAG_CONTENT_DATE, Vr::DA, &opts.content_date),
        string_element(
            TAG_ACQUISITION_DATE_TIME,
            Vr::DT,
            &opts.acquisition_date_time,
        ),
        string_element(TAG_CONTENT_TIME, Vr::TM, &opts.content_time),
        string_element(TAG_MODALITY, Vr::CS, k.modality),
        string_element(TAG_SERIES_DESCRIPTION, Vr::LO, &opts.series_description),
        string_element(TAG_STUDY_INSTANCE_UID, Vr::UI, &opts.study_instance_uid),
        string_element(TAG_SERIES_INSTANCE_UID, Vr::UI, &opts.series_instance_uid),
        string_element(
            TAG_SERIES_NUMBER,
            Vr::IS,
            &default_string(&opts.series_number, "1"),
        ),
        string_element(
            TAG_INSTANCE_NUMBER,
            Vr::IS,
            &default_string(&opts.instance_number, "1"),
        ),
        string_element(TAG_PATIENT_NAME, Vr::PN, &opts.patient_name),
        string_element(TAG_PATIENT_ID, Vr::LO, &opts.patient_id),
        string_element(TAG_BURNED_IN_ANNOTATION, Vr::CS, &burned),
        sequence_element(TAG_CONCEPT_NAME_CODE_SEQUENCE, &[]),
        string_element(TAG_DOCUMENT_TITLE, Vr::ST, &opts.document_title),
        string_element(TAG_MIME_TYPE, Vr::LO, k.mime_type),
        raw_element(TAG_ENCAPSULATED_DOCUMENT, Vr::OB, opts.data.clone()),
        ul_element(
            TAG_ENCAPSULATED_DOCUMENT_LENGTH,
            false,
            &[opts.data.len() as u32],
        ),
    ];
    if k.secondary_cap {
        elements.push(string_element(TAG_CONVERSION_TYPE, Vr::CS, "WSD"));
    }
    let mut dataset = Object::from_elements(elements);
    for (tag, value) in [
        (TAG_PATIENT_NAME, &opts.patient_name),
        (TAG_PATIENT_ID, &opts.patient_id),
        (TAG_STUDY_INSTANCE_UID, &opts.study_instance_uid),
    ] {
        if value.trim().is_empty() {
            dataset.remove(tag);
        }
    }
    let modules = if k.frame_of_ref {
        inherit_modules::ALL
    } else {
        inherit_modules::WITHOUT_FRAME_OF_REFERENCE
    };
    apply_derived_common(
        &mut dataset,
        opts.common.source.as_ref(),
        opts.common.equipment.clone(),
        opts.common.now,
        modules,
    )
    .map_err(Error::object)?;
    extra(&mut dataset)?;
    Ok(File {
        preamble: Vec::new(),
        meta: Object::new(),
        dataset,
        transfer_syntax: transfer::EXPLICIT_VR_LITTLE_ENDIAN,
        partial_read: None,
        incomplete_pixel_data: None,
    })
}
pub(crate) fn binary_stl_length(data: &[u8]) -> Option<i64> {
    if data.len() < 84 {
        return None;
    }
    let count = u32::from_le_bytes([data[80], data[81], data[82], data[83]]) as i64;
    Some(84 + 50 * count)
}
fn trim_right_nul(data: &[u8]) -> &[u8] {
    let end = data.iter().rposition(|&b| b != 0).map_or(0, |i| i + 1);
    &data[..end]
}
fn validate_stl(data: &[u8]) -> Result<()> {
    if let Some(want) = binary_stl_length(data) {
        let len = data.len() as i64;
        if len == want || len == want + 1 && data[want as usize] == 0 {
            return Ok(());
        }
    }
    if let Ok(text) = std::str::from_utf8(trim_right_nul(data)) {
        let text = text.trim();
        if text.starts_with("solid") && text.contains("endsolid") {
            return Ok(());
        }
    }
    Err(Error::wrap(
        Kind::InvalidDocument,
        "neither a binary STL nor an ASCII STL",
    ))
}
fn validate_text(data: &[u8]) -> Result<()> {
    let text = trim_right_nul(data);
    if std::str::from_utf8(text).is_err() || text.contains(&0) {
        return Err(Error::wrap(Kind::InvalidDocument, "not text"));
    }
    Ok(())
}
pub(crate) fn compare_cda_identifiers(declared: &str, source: &str) -> Result<()> {
    let declared = trim_text_value(Vr::ST, declared);
    if declared.is_empty() {
        return Ok(());
    }
    let cut = |s: &str| match s.split_once('^') {
        Some((a, b)) => (a.to_string(), b.to_string(), true),
        None => (s.to_string(), String::new(), false),
    };
    let (declared_root, declared_ext, declared_has) = cut(declared);
    let (source_root, source_ext, source_has) = cut(trim_text_value(Vr::ST, source));
    let roots_match = declared_root == source_root
        || source_root.len() == 36
            && payload::payload_oid_or_uuid(&source_root)
            && simple_fold_eq(&declared_root, &source_root);
    if !roots_match || declared_has != source_has || declared_ext != source_ext {
        return Err(Error::sentinel(Kind::CdaIdentityMismatch));
    }
    Ok(())
}
fn simple_fold_eq(a: &str, b: &str) -> bool {
    let fold = |c: char| -> char {
        match c {
            '\u{212A}' => 'k',
            '\u{17F}' => 's',
            '\u{2126}' => '\u{3C9}',
            _ => {
                let mut l = c.to_lowercase();
                match (l.next(), l.next()) {
                    (Some(x), None) => x,
                    _ => c,
                }
            }
        }
    };
    let mut ai = a.chars();
    let mut bi = b.chars();
    loop {
        match (ai.next(), bi.next()) {
            (None, None) => return true,
            (Some(x), Some(y)) if fold(x) == fold(y) => {}
            _ => return false,
        }
    }
}
pub fn cda_envelope_identifier(dataset: &Object) -> Result<String> {
    let Some(element) = dataset.get(TAG_HL7_INSTANCE_IDENTIFIER) else {
        return Ok(String::new());
    };
    if element.vr() == Vr::ST
        && let Ok(values) = dataset.lookup_strings(TAG_HL7_INSTANCE_IDENTIFIER)
    {
        match values.len() {
            0 => return Ok(String::new()),
            1 => return Ok(values[0].clone()),
            _ => {}
        }
    }
    Err(Error::wrap(
        Kind::InvalidDocument,
        "unreadable CDA envelope HL7 Instance Identifier",
    ))
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Document {
    pub sop_class_uid: String,
    pub mime_type: String,
    pub data: Vec<u8>,
    pub hl7_instance_identifier: String,
    pub declared_length: bool,
}
fn document_kind(obj: &Object) -> Result<DocKind> {
    let sop = obj.get_uid(TAG_SOP_CLASS_UID).unwrap_or_default();
    let sop = sop.trim();
    match sop {
        ENCAPSULATED_PDF_STORAGE => Ok(KIND_PDF),
        ENCAPSULATED_CDA_STORAGE => Ok(KIND_CDA),
        ENCAPSULATED_STL_STORAGE => Ok(KIND_STL),
        ENCAPSULATED_OBJ_STORAGE => Ok(KIND_OBJ),
        ENCAPSULATED_MTL_STORAGE => Ok(KIND_MTL),
        _ => Err(Error::wrap(
            Kind::UnsupportedSopClass,
            &format!("SOP Class {}", crate::textfmt::quote(sop)),
        )),
    }
}
fn last_index(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.len() > haystack.len() {
        return None;
    }
    (0..=haystack.len() - needle.len())
        .rev()
        .find(|&i| &haystack[i..i + needle.len()] == needle)
}
const PDF_EOF: &[u8] = b"%%EOF";
fn unpad(k: DocKind, raw: &[u8]) -> &[u8] {
    match k.sop_class_uid {
        ENCAPSULATED_PDF_STORAGE => {
            if let Some(mut end) = last_index(raw, PDF_EOF) {
                end += PDF_EOF.len();
                while end < raw.len() && (raw[end] == b'\r' || raw[end] == b'\n') {
                    end += 1;
                }
                return &raw[..end];
            }
            trim_right_nul(raw)
        }
        ENCAPSULATED_STL_STORAGE => {
            if let Some(want) = binary_stl_length(raw)
                && want <= raw.len() as i64
                && raw.len() as i64 - want <= 1
            {
                return &raw[..want as usize];
            }
            trim_right_nul(raw)
        }
        _ => trim_right_nul(raw),
    }
}
pub fn extract(obj: &Object) -> Result<Document> {
    let k = document_kind(obj)?;
    let Some(element) = obj.get(TAG_ENCAPSULATED_DOCUMENT) else {
        return Err(Error::wrap(
            Kind::InvalidDocument,
            "Encapsulated Document is missing",
        ));
    };
    let Some(raw) = element.raw() else {
        return Err(Error::wrap(
            Kind::InvalidDocument,
            "Encapsulated Document is not a byte value",
        ));
    };
    let mut doc = Document {
        sop_class_uid: k.sop_class_uid.to_string(),
        mime_type: k.mime_type.to_string(),
        ..Document::default()
    };
    if let Some(mime) = obj.get_string(TAG_MIME_TYPE)
        && !mime.trim().is_empty()
    {
        doc.mime_type = mime.trim().to_string();
    }
    if let Some(identifier) = obj.get_string(TAG_HL7_INSTANCE_IDENTIFIER) {
        doc.hl7_instance_identifier = identifier.trim().to_string();
    }
    let length = inspect_document_length(obj)?;
    if length.has_declaration {
        doc.data = raw[..length.declared_bytes as usize].to_vec();
        doc.declared_length = true;
        return Ok(doc);
    }
    doc.data = unpad(k, raw).to_vec();
    Ok(doc)
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DocumentLength {
    pub stored_bytes: i64,
    pub declared_bytes: i64,
    pub has_declaration: bool,
}
fn document_length_error(stored: i64, declared: i64) -> Error {
    let msg = if declared < 0 {
        "encapdoc: Encapsulated Document Length must be a single UL value".to_string()
    } else {
        format!(
            "encapdoc: Encapsulated Document Length {declared} is inconsistent with the {stored}-byte value or its padding"
        )
    };
    let mut e = Error::new(Kind::DocumentLength, msg);
    e.stored_bytes = stored;
    e.declared_bytes = declared;
    e
}
pub fn inspect_document_length(obj: &Object) -> Result<DocumentLength> {
    inspect_document_length_info(obj).map_err(|(_, e)| e)
}
#[doc(hidden)]
pub fn inspect_document_length_info(
    obj: &Object,
) -> std::result::Result<DocumentLength, (DocumentLength, Error)> {
    let Some(document) = obj.get(TAG_ENCAPSULATED_DOCUMENT) else {
        return Err((
            DocumentLength::default(),
            Error::wrap(Kind::InvalidDocument, "Encapsulated Document is missing"),
        ));
    };
    let mut info = DocumentLength::default();
    let raw = document.raw();
    if let Some(raw) = raw {
        info.stored_bytes = raw.len() as i64;
    } else if document.value == Value::Nil
        && document.header.length_set
        && document.header.length != crate::object::UNDEFINED_LENGTH
    {
        info.stored_bytes = document.header.length as i64;
    } else {
        return Err((
            DocumentLength::default(),
            Error::wrap(
                Kind::InvalidDocument,
                "Encapsulated Document has no defined byte value",
            ),
        ));
    }
    let Some(length) = obj.get(TAG_ENCAPSULATED_DOCUMENT_LENGTH) else {
        return Ok(info);
    };
    info.has_declaration = true;
    info.declared_bytes = -1;
    if length.vr() == Vr::UL
        && element_length(length) == 4
        && let Some(value) = length.raw()
        && value.len() == 4
    {
        let b = [value[0], value[1], value[2], value[3]];
        info.declared_bytes = if obj.value_big_endian() {
            u32::from_be_bytes(b)
        } else {
            u32::from_le_bytes(b)
        } as i64;
    }
    if info.declared_bytes == info.stored_bytes {
        return Ok(info);
    }
    if info.declared_bytes > 0
        && info.declared_bytes % 2 == 1
        && info.stored_bytes - info.declared_bytes == 1
        && let Some(raw) = raw
        && raw[raw.len() - 1] == 0
    {
        return Ok(info);
    }
    Err((
        info,
        document_length_error(info.stored_bytes, info.declared_bytes),
    ))
}
pub fn copy_document_to(obj: &Object, dst: &mut Vec<u8>, max_bytes: i64) -> Result<i64> {
    if max_bytes < 0 {
        return Err(Error::new(
            Kind::Object,
            "encapdoc: destination writer and nonnegative byte limit required",
        ));
    }
    let k = document_kind(obj)?;
    let info = inspect_document_length(obj)?;
    let element = obj.get(TAG_ENCAPSULATED_DOCUMENT);
    let Some(raw) = element.and_then(Element::raw) else {
        return Err(Error::new(
            Kind::Object,
            "encapdoc: replay document payload: deferred values are not supported",
        ));
    };
    let mut length = info.declared_bytes;
    if !info.has_declaration {
        length = unpad(k, raw).len() as i64;
    }
    if max_bytes > 0 && length > max_bytes {
        return Err(Error::wrap(Kind::TooLarge, &format!("{length} bytes")));
    }
    dst.extend_from_slice(&raw[..length as usize]);
    Ok(length)
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ModelMetadata {
    pub sop_class_uid: String,
    pub frame_of_reference_uid: String,
    pub units: Code,
    pub millimetres_per_unit: f64,
    pub icc_profile: Option<Vec<u8>>,
    pub color_space: String,
    pub references: Vec<ModelReference>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ModelReference {
    pub sop_class_uid: String,
    pub sop_instance_uid: String,
    pub relative_uri: String,
}
fn valid_uid(uid: &str) -> bool {
    is_valid_uid(uid.as_bytes())
}
pub fn read_model_metadata(dataset: &Object) -> Result<ModelMetadata> {
    let mut out = ModelMetadata {
        sop_class_uid: dataset.get_uid(TAG_SOP_CLASS_UID).unwrap_or_default(),
        ..ModelMetadata::default()
    };
    match out.sop_class_uid.as_str() {
        ENCAPSULATED_STL_STORAGE | ENCAPSULATED_OBJ_STORAGE | ENCAPSULATED_MTL_STORAGE => {}
        _ => return Err(Error::sentinel(Kind::UnsupportedSopClass)),
    }
    let invalid = |s: &str| Error::wrap(Kind::InvalidDocument, s);
    if out.sop_class_uid != ENCAPSULATED_MTL_STORAGE {
        out.frame_of_reference_uid = dataset
            .get_uid(TAG_FRAME_OF_REFERENCE_UID)
            .unwrap_or_default()
            .trim()
            .to_string();
        if !valid_uid(&out.frame_of_reference_uid) {
            return Err(Error::sentinel(Kind::MissingFrameOfReference));
        }
        let items = dataset.get_sequence(TAG_MEASUREMENT_UNITS_CODE_SEQ);
        let Some(items) = items.filter(|i| i.len() == 1) else {
            return Err(invalid("one explicit Measurement Units item required"));
        };
        let get = |tag| {
            items[0]
                .get_string(tag)
                .unwrap_or_default()
                .trim()
                .to_string()
        };
        out.units.value = get(TAG_CODE_VALUE);
        out.units.scheme = get(TAG_CODING_SCHEME_DESIGNATOR);
        out.units.meaning = get(TAG_CODE_MEANING);
        if out.units.scheme != "UCUM"
            || out.units.meaning.is_empty()
            || out.units.meaning.len() > 64
            || out.units.meaning.chars().any(char::is_control)
        {
            return Err(invalid("unsupported/incomplete model units"));
        }
        out.millimetres_per_unit = match out.units.value.as_str() {
            "mm" => 1.0,
            "cm" => 10.0,
            "m" => 1000.0,
            "um" => 0.001,
            _ => return Err(invalid("unsupported model scale unit")),
        };
    }
    out.color_space = dataset
        .get_string(TAG_COLOR_SPACE)
        .unwrap_or_default()
        .trim()
        .to_string();
    if out.color_space.len() > 16 || out.color_space.chars().any(char::is_control) {
        return Err(invalid("invalid model color-space declaration"));
    }
    if let Some(elem) = dataset.get(TAG_ICC_PROFILE) {
        match elem.raw() {
            Some(raw) if raw.len() <= 2 << 20 => {
                out.icc_profile = (!raw.is_empty()).then(|| raw.to_vec())
            }
            _ => return Err(invalid("ICC profile unavailable/exceeds 2 MiB")),
        }
    }
    let items = dataset.get_sequence(t(0x0008, 0x114A)).unwrap_or_default();
    if items.len() > 1024 {
        return Err(invalid("model reference count exceeds 1024"));
    }
    let mut names: Vec<(String, String)> = Vec::new();
    for item in &items {
        let get_uid = |tag| item.get_uid(tag).unwrap_or_default().trim().to_string();
        let mut r = ModelReference {
            sop_class_uid: get_uid(t(0x0008, 0x1150)),
            sop_instance_uid: get_uid(t(0x0008, 0x1155)),
            relative_uri: item
                .get_string(t(0x0068, 0x7005))
                .unwrap_or_default()
                .trim()
                .to_string(),
        };
        if !valid_uid(&r.sop_class_uid) || !valid_uid(&r.sop_instance_uid) {
            return Err(invalid("invalid model reference UID"));
        }
        if !r.relative_uri.is_empty() {
            let name = model_relative_uri(&r.relative_uri)?;
            r.relative_uri = name.clone();
            match names.iter().find(|(n, _)| *n == name) {
                Some((_, prior)) if *prior != r.sop_instance_uid => {
                    return Err(invalid("ambiguous model reference name"));
                }
                Some(_) => {}
                None => names.push((name, r.sop_instance_uid.clone())),
            }
        }
        out.references.push(r);
    }
    Ok(out)
}
