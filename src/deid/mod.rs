#![forbid(unsafe_code)]
#[cfg(feature = "deid-encrypted")]
mod cms;
#[cfg(feature = "deid-encrypted")]
pub mod encrypted;
pub mod path;
pub mod pixel_redaction;
pub mod profile;
mod pseudonym;
mod safe_private;
#[rustfmt::skip]
pub mod tables;
pub mod text_candidates;
pub mod uid;
use crate::attrpath;
use crate::object::text::{raw_string_values, trim_text_value};
use crate::object::{self, DataSet, Element, ElementHeader, File, Object, Tag, Value, Vr};
use crate::siemens;
pub use profile::*;
pub use pseudonym::{
    DEFAULT_PSEUDONYM_DOMAIN, MIN_PSEUDONYM_SECRET_BYTES, Pseudonym, PseudonymTemplate,
    Pseudonymizer, patient_pseudonym_identity,
};
pub use safe_private::{SafePrivateProvenance, SafePrivateRegistry, SafePrivateRule};
use std::collections::HashMap;
use std::fmt;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    NilObject,
    UnsupportedPixelRedaction,
    InvalidActionCode,
    InvalidProfileOptions,
    ProfileResourceLimit,
    StaleProfilePlan,
    CleanerRequired,
    DummyValueProviderRequired,
    DateShiftPolicyRequired,
    SafePrivateRegistryRequired,
    PixelCleanerRequired,
    VisualCleanerRequired,
    ProfileCallback,
    DicomdirPolicyRequired,
    UnrepresentableDateShift,
    DeferredValueUnavailable,
    StructuredContentValue,
    UnclassifiedStructuredContent,
    InvalidSafePrivateRegistry,
    InvalidPseudonymTemplate,
    PseudonymSecretRequired,
    PseudonymCollision,
    InvalidPixelRedaction,
    StalePixelRedactionAttestation,
    PixelRedactionRequiresFile,
    PixelRedactionFailed,
    SamePath,
    PixelTextReviewInconclusive,
    InvalidEncryptedAttributesOptions,
    EncryptedAttributesRecipient,
    NoEncryptedAttributes,
    RecipientNotFound,
    EncryptedAttributesDecrypt,
    MalformedEncryptedAttributes,
    UnsupportedEncryptedAttributes,
    RecipientKey,
    Edit(attrpath::Kind),
    Object,
}
impl Kind {
    pub fn text(self) -> &'static str {
        match self {
            Kind::NilObject => "deid: nil object",
            Kind::UnsupportedPixelRedaction => "deid: unsupported pixel redaction",
            Kind::InvalidActionCode => "deid: invalid PS3.15 action code",
            Kind::InvalidProfileOptions => "deid: invalid basic profile options",
            Kind::ProfileResourceLimit => "deid: basic profile resource limit exceeded",
            Kind::StaleProfilePlan => "deid: basic profile plan no longer matches object",
            Kind::CleanerRequired => "deid: cleaner required for selected profile option",
            Kind::DummyValueProviderRequired => {
                "deid: dummy value provider required for sequence attribute"
            }
            Kind::DateShiftPolicyRequired => "deid: date shift policy required for modified dates",
            Kind::SafePrivateRegistryRequired => "deid: verified safe-private registry required",
            Kind::PixelCleanerRequired => "deid: pixel cleaner required for selected option",
            Kind::VisualCleanerRequired => {
                "deid: visual-feature cleaner required for selected option"
            }
            Kind::ProfileCallback => "deid: profile callback failed",
            Kind::DicomdirPolicyRequired => {
                "deid: DICOMDIR requires a media-set transaction policy"
            }
            Kind::UnrepresentableDateShift => {
                "deid: date shift cannot be represented at source precision"
            }
            Kind::DeferredValueUnavailable => {
                "deid: deferred value unavailable to profile callback"
            }
            Kind::StructuredContentValue => {
                "deid: structured content item value is missing or malformed"
            }
            Kind::UnclassifiedStructuredContent => {
                "deid: structured content concept is not covered by the selected normative table"
            }
            Kind::InvalidSafePrivateRegistry => "deid: invalid safe-private registry",
            Kind::InvalidPseudonymTemplate => "deid: invalid pseudonym template",
            Kind::PseudonymSecretRequired => "deid: keyed pseudonym template requires a secret",
            Kind::PseudonymCollision => {
                "deid: pseudonym collision between distinct original identities"
            }
            Kind::InvalidPixelRedaction => "deid: invalid pixel redaction request",
            Kind::StalePixelRedactionAttestation => {
                "deid: pixel redaction attestation does not match the requested regions"
            }
            Kind::PixelRedactionRequiresFile => {
                "deid: this pixel redaction requires the Part 10 file and its transfer syntax"
            }
            Kind::PixelRedactionFailed => "deid: pixel redaction failed",
            Kind::SamePath => "deid: destination is the source file",
            Kind::PixelTextReviewInconclusive => "deid: pixel text candidate scan is inconclusive",
            Kind::InvalidEncryptedAttributesOptions => "deid: invalid encrypted attributes options",
            Kind::EncryptedAttributesRecipient => {
                "deid: unusable encrypted attributes recipient certificate"
            }
            Kind::NoEncryptedAttributes => "deid: no Encrypted Attributes Sequence",
            Kind::RecipientNotFound => {
                "deid: no Encrypted Attributes item is addressed to this recipient"
            }
            Kind::EncryptedAttributesDecrypt => {
                "deid: encrypted attributes could not be decrypted with this key"
            }
            Kind::MalformedEncryptedAttributes => "deid: malformed encrypted attributes",
            Kind::UnsupportedEncryptedAttributes => {
                "deid: unsupported encrypted attributes encoding or algorithm"
            }
            Kind::RecipientKey => "deid: recipient key does not match its certificate",
            Kind::Edit(_) | Kind::Object => "",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub kind: Kind,
    text: String,
}
impl Error {
    pub(crate) fn bare(kind: Kind) -> Error {
        Error {
            kind,
            text: kind.text().to_string(),
        }
    }
    pub(crate) fn new(kind: Kind, detail: impl fmt::Display) -> Error {
        Error {
            kind,
            text: format!("{}: {detail}", kind.text()),
        }
    }
    pub(crate) fn with_text(kind: Kind, text: impl Into<String>) -> Error {
        Error {
            kind,
            text: text.into(),
        }
    }
    pub fn is(&self, kind: Kind) -> bool {
        self.kind == kind
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}
impl std::error::Error for Error {}
impl From<attrpath::Error> for Error {
    fn from(e: attrpath::Error) -> Error {
        Error {
            kind: Kind::Edit(e.kind),
            text: e.to_string(),
        }
    }
}
impl From<object::Error> for Error {
    fn from(e: object::Error) -> Error {
        Error {
            kind: Kind::Object,
            text: e.to_string(),
        }
    }
}
pub(crate) const TAG_PATIENT_NAME: Tag = Tag::new(0x0010, 0x0010);
pub(crate) const TAG_PATIENT_ID: Tag = Tag::new(0x0010, 0x0020);
pub(crate) const TAG_PATIENT_BIRTH_DATE: Tag = Tag::new(0x0010, 0x0030);
pub(crate) const TAG_ISSUER_OF_PATIENT_ID: Tag = Tag::new(0x0010, 0x0021);
const TAG_STUDY_INSTANCE_UID: Tag = Tag::new(0x0020, 0x000D);
const TAG_SERIES_INSTANCE_UID: Tag = Tag::new(0x0020, 0x000E);
const TAG_SOP_INSTANCE_UID: Tag = Tag::new(0x0008, 0x0018);
pub(crate) const TAG_BURNED_IN_ANNOTATION: Tag = Tag::new(0x0028, 0x0301);
pub(crate) const TAG_RECOGNIZABLE_VISUAL_FEATURES: Tag = Tag::new(0x0028, 0x0302);
pub(crate) const TAG_PIXEL_DATA: Tag = Tag::new(0x7FE0, 0x0010);
const BASIC_PROFILE_TEXT_ATTRIBUTES: [(u16, u16, Vr); 19] = [
    (0x0010, 0x0032, Vr::TM),
    (0x0010, 0x0021, Vr::LO),
    (0x0010, 0x1001, Vr::PN),
    (0x0010, 0x1005, Vr::PN),
    (0x0010, 0x1090, Vr::LO),
    (0x0010, 0x1060, Vr::PN),
    (0x0010, 0x2154, Vr::SH),
    (0x0010, 0x4000, Vr::LT),
    (0x0008, 0x0081, Vr::ST),
    (0x0008, 0x1010, Vr::SH),
    (0x0008, 0x1030, Vr::LO),
    (0x0008, 0x103E, Vr::LO),
    (0x0008, 0x0092, Vr::ST),
    (0x0008, 0x0094, Vr::SH),
    (0x0032, 0x1032, Vr::PN),
    (0x0018, 0x1000, Vr::LO),
    (0x0018, 0x1030, Vr::LO),
    (0x0020, 0x0010, Vr::SH),
    (0x0020, 0x4000, Vr::LT),
];
const LONGITUDINAL_DATE_TIME_ATTRIBUTES: [(u16, u16, Vr); 15] = [
    (0x0008, 0x0012, Vr::DA),
    (0x0008, 0x0013, Vr::TM),
    (0x0008, 0x0020, Vr::DA),
    (0x0008, 0x0021, Vr::DA),
    (0x0008, 0x0022, Vr::DA),
    (0x0008, 0x0023, Vr::DA),
    (0x0008, 0x0024, Vr::DA),
    (0x0008, 0x0025, Vr::DA),
    (0x0008, 0x002A, Vr::DT),
    (0x0008, 0x0030, Vr::TM),
    (0x0008, 0x0031, Vr::TM),
    (0x0008, 0x0032, Vr::TM),
    (0x0008, 0x0033, Vr::TM),
    (0x0008, 0x0034, Vr::TM),
    (0x0008, 0x0035, Vr::TM),
];
const IDENTIFYING_SEQUENCE_ATTRIBUTES: [(u16, u16); 8] = [
    (0x0008, 0x0082),
    (0x0008, 0x0096),
    (0x0008, 0x1049),
    (0x0008, 0x1052),
    (0x0008, 0x1072),
    (0x0010, 0x0024),
    (0x0010, 0x1002),
    (0x0032, 0x1031),
];
const INSTANCE_UID_ATTRIBUTES: [(u16, u16); 54] = [
    (0x0008, 0x0017),
    (0x0020, 0x9161),
    (0x3010, 0x0006),
    (0x3010, 0x0013),
    (0x0018, 0x1002),
    (0x0400, 0x0100),
    (0x0020, 0x9164),
    (0x300A, 0x0013),
    (0x3010, 0x006E),
    (0x0008, 0x0058),
    (0x0070, 0x031A),
    (0x0020, 0x0052),
    (0x0008, 0x0014),
    (0x0008, 0x3010),
    (0x0028, 0x1214),
    (0x0018, 0x100B),
    (0x003A, 0x0310),
    (0x0040, 0xA402),
    (0x0040, 0xA171),
    (0x0028, 0x1199),
    (0x300A, 0x0650),
    (0x0070, 0x1101),
    (0x0070, 0x1102),
    (0x0008, 0x0019),
    (0x3010, 0x000B),
    (0x300A, 0x0083),
    (0x3010, 0x006F),
    (0x3010, 0x0031),
    (0x3006, 0x0024),
    (0x0040, 0x4023),
    (0x0040, 0xA172),
    (0x0008, 0x1155),
    (0x0004, 0x1511),
    (0x300A, 0x0785),
    (0x3006, 0x00C2),
    (0x0000, 0x1001),
    (0x3010, 0x003B),
    (0x0020, 0x000E),
    (0x0008, 0x0018),
    (0x3010, 0x0015),
    (0x0064, 0x0003),
    (0x0040, 0x0554),
    (0x0088, 0x0140),
    (0x0020, 0x000D),
    (0x0020, 0x0200),
    (0x300A, 0x0054),
    (0x0018, 0x2042),
    (0x0040, 0xDB0D),
    (0x0040, 0xDB0C),
    (0x0062, 0x0021),
    (0x0008, 0x1195),
    (0x300A, 0x0609),
    (0x300A, 0x0700),
    (0x0040, 0xA124),
];
pub(crate) fn string_value(e: &Element) -> String {
    match &e.value {
        Value::Raw(raw) => trim_text_value(e.vr(), &String::from_utf8_lossy(raw)).to_string(),
        Value::Strings(values) => values
            .first()
            .map(|v| trim_text_value(e.vr(), v).to_string())
            .unwrap_or_default(),
        _ => String::new(),
    }
}
pub(crate) fn string_values(e: &Element) -> Vec<String> {
    match &e.value {
        Value::Raw(raw) => raw_string_values(e.vr(), raw)
            .into_iter()
            .map(|v| String::from_utf8_lossy(&v).into_owned())
            .collect(),
        Value::Strings(values) => values
            .iter()
            .map(|v| trim_text_value(e.vr(), v).to_string())
            .collect(),
        _ => Vec::new(),
    }
}
pub(crate) fn trim_space(s: &str) -> &str {
    s.trim_matches(|c: char| {
        matches!(
            c,
            '\t' | '\n' | '\u{b}' | '\u{c}' | '\r' | ' ' | '\u{85}' | '\u{a0}'
        ) || (c as u32 > 0xff && c.is_whitespace())
    })
}
pub(crate) fn raw_element(tag: Tag, vr: Vr, data: Vec<u8>) -> Element {
    Element {
        header: ElementHeader::new(tag, vr, data.len() as u32),
        value: Value::Raw(data),
    }
}
pub(crate) fn pad_text(s: &str) -> Vec<u8> {
    let mut b = s.as_bytes().to_vec();
    if !b.len().is_multiple_of(2) {
        b.push(b' ');
    }
    b
}
pub(crate) fn pad_uid(s: &str) -> Vec<u8> {
    let mut b = s.as_bytes().to_vec();
    if !b.len().is_multiple_of(2) {
        b.push(0);
    }
    b
}
pub(crate) fn private_creator_tag_for_data_tag(tag: Tag) -> Tag {
    if !tag.is_private() || tag.element < 0x1000 {
        return Tag::default();
    }
    Tag::new(tag.group, tag.element >> 8)
}
pub(crate) fn is_private_creator_tag(tag: Tag) -> bool {
    tag.is_private() && (0x0010..=0x00ff).contains(&tag.element)
}
#[derive(Clone, Debug, Default)]
pub struct UidRemapper {
    m: HashMap<String, String>,
    reverse: HashMap<String, String>,
    missing: u64,
    key: Vec<u8>,
    keyed: bool,
    fallback_counter: u64,
}
impl UidRemapper {
    pub fn new() -> UidRemapper {
        UidRemapper::default()
    }
    pub fn new_keyed(key: &[u8]) -> UidRemapper {
        UidRemapper {
            key: key.to_vec(),
            keyed: true,
            ..Default::default()
        }
    }
    pub fn map(&mut self, old: &str) -> String {
        self.map_with_collisions(old).0
    }
    pub(crate) fn map_with_collisions(&mut self, old: &str) -> (String, usize) {
        let keyed = self.keyed;
        self.map_minting(old, keyed)
    }
    fn map_minting(&mut self, old: &str, keyed: bool) -> (String, usize) {
        if let Some(uid) = self.m.get(old) {
            return (uid.clone(), 0);
        }
        let mut collisions = 0;
        if keyed {
            let uid = uid::keyed_name_uid(&self.key, &[old]);
            if !self.reverse.contains_key(&uid) {
                self.m.insert(old.to_string(), uid.clone());
                self.reverse.insert(uid.clone(), old.to_string());
                return (uid, 0);
            }
            collisions += 1;
        }
        loop {
            let uid = uid::new_uid();
            let free = self
                .reverse
                .get(&uid)
                .is_none_or(|existing| existing == old);
            if free {
                self.m.insert(old.to_string(), uid.clone());
                self.reverse.insert(uid.clone(), old.to_string());
                return (uid, collisions);
            }
            collisions += 1;
            let uid = self.fallback_uid();
            if !self.reverse.contains_key(&uid) {
                self.m.insert(old.to_string(), uid.clone());
                self.reverse.insert(uid.clone(), old.to_string());
                return (uid, collisions);
            }
        }
    }
    fn fallback_uid(&mut self) -> String {
        self.fallback_counter += 1;
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos() as i64);
        uid::name_uid(&[
            "dcm-rust/deid fallback",
            &nanos.to_string(),
            &self.fallback_counter.to_string(),
        ])
    }
    pub(crate) fn map_missing(&mut self, scope: &str) -> String {
        self.map_missing_with_collisions(scope).0
    }
    pub(crate) fn map_missing_with_collisions(&mut self, scope: &str) -> (String, usize) {
        self.missing += 1;
        let key = format!("{scope}:{}", self.missing);
        self.map_minting(&key, false)
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BurnedInPixelRisk {
    #[default]
    Unknown,
    Absent,
    Present,
}
impl BurnedInPixelRisk {
    pub fn as_str(self) -> &'static str {
        match self {
            BurnedInPixelRisk::Absent => "absent",
            BurnedInPixelRisk::Present => "present",
            BurnedInPixelRisk::Unknown => "unknown",
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PixelRegion {
    pub frame: i64,
    pub x: i64,
    pub y: i64,
    pub width: i64,
    pub height: i64,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BurnedInPixelReport {
    pub risk: BurnedInPixelRisk,
    pub metadata_value: String,
    pub redacted_regions: Vec<PixelRegion>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RecognizableVisualFeaturesReport {
    pub risk: BurnedInPixelRisk,
    pub metadata_value: String,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CsaScrubReport {
    pub scrubbed: usize,
    pub bytes_cleared: usize,
    pub unscrubbable: usize,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub burned_in_pixel: BurnedInPixelReport,
    pub recognizable_visual_features: RecognizableVisualFeaturesReport,
    pub edited_attributes: usize,
    pub csa: CsaScrubReport,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrivateTagAction {
    Remove,
    Retain,
}
#[derive(Clone, Debug)]
pub struct PrivateTagContext<'a> {
    pub tag: Tag,
    pub element: &'a Element,
    pub creator_tag: Tag,
    pub creator: String,
}
pub type PrivateTagPolicy = Box<dyn Fn(&PrivateTagContext) -> PrivateTagAction>;
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PrivateTagAllow {
    pub tag: Tag,
    pub creator: String,
}
pub fn retain_private_tags() -> PrivateTagPolicy {
    Box::new(|_| PrivateTagAction::Retain)
}
pub fn remove_private_tags() -> PrivateTagPolicy {
    Box::new(|_| PrivateTagAction::Remove)
}
pub fn allow_private_tags(allowlist: &[PrivateTagAllow]) -> PrivateTagPolicy {
    let mut creators = std::collections::HashSet::new();
    let mut data = std::collections::HashSet::new();
    for allowed in allowlist {
        if is_private_creator_tag(allowed.tag) {
            creators.insert(allowed.tag);
            continue;
        }
        let creator = private_creator_tag_for_data_tag(allowed.tag);
        if creator == Tag::default() {
            creators.insert(allowed.tag);
            continue;
        }
        data.insert((allowed.tag, trim_space(&allowed.creator).to_string()));
        creators.insert(creator);
    }
    Box::new(move |ctx| {
        if creators.contains(&ctx.tag) {
            return PrivateTagAction::Retain;
        }
        if !ctx.creator.is_empty()
            && data.contains(&(ctx.tag, trim_space(&ctx.creator).to_string()))
        {
            return PrivateTagAction::Retain;
        }
        PrivateTagAction::Remove
    })
}
#[derive(Default)]
pub struct Options {
    pub patient_name: String,
    pub patient_id: String,
    pub keep_dates: bool,
    pub private_tag_policy: Option<PrivateTagPolicy>,
    pub edits: attrpath::Script,
    pub edit_options: attrpath::Options,
    pub burned_in_pixel_policy: Option<BurnedInPixelPolicy>,
}
#[derive(Clone, Debug, Default)]
pub struct BurnedInPixelPolicyContext {
    pub report: BurnedInPixelReport,
    pub metadata: Option<crate::pixel::Metadata>,
    pub rows: i64,
    pub columns: i64,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BurnedInPixelPolicyResult {
    pub regions: Vec<PixelRegion>,
    pub fill: Vec<u8>,
}
pub type BurnedInPixelPolicy =
    Box<dyn Fn(&BurnedInPixelPolicyContext) -> BurnedInPixelPolicyResult>;
fn pixel_policy_metadata(obj: &Object) -> (Option<crate::pixel::Metadata>, i64, i64) {
    if let Ok(m) = crate::pixel::pixeldata::extract_metadata(obj) {
        let (rows, columns) = (i64::from(m.rows), i64::from(m.columns));
        return (Some(m), rows, columns);
    }
    let dimension = |tag: Tag| {
        obj.get_raw(tag)
            .filter(|raw| raw.len() >= 2)
            .map_or(0, |raw| {
                let b = [raw[0], raw[1]];
                i64::from(if obj.value_big_endian() {
                    u16::from_be_bytes(b)
                } else {
                    u16::from_le_bytes(b)
                })
            })
    };
    (
        None,
        dimension(Tag::new(0x0028, 0x0010)),
        dimension(Tag::new(0x0028, 0x0011)),
    )
}
pub fn assess_visual_phi_risk(obj: &Object) -> Report {
    Report {
        burned_in_pixel: assess_burned_in_pixel(obj),
        recognizable_visual_features: assess_recognizable_visual_features(obj),
        ..Default::default()
    }
}
pub fn anonymize_object(
    obj: &mut Object,
    opts: &Options,
    uids: &mut UidRemapper,
) -> Result<(), Error> {
    anonymize_object_with_report(obj, opts, uids)
        .1
        .map_or(Ok(()), Err)
}
pub fn anonymize_object_with_report(
    obj: &mut Object,
    opts: &Options,
    uids: &mut UidRemapper,
) -> (Report, Option<Error>) {
    let mut report = Report {
        burned_in_pixel: assess_burned_in_pixel(obj),
        recognizable_visual_features: assess_recognizable_visual_features(obj),
        ..Default::default()
    };
    if let Some(policy) = &opts.burned_in_pixel_policy {
        let (metadata, rows, columns) = pixel_policy_metadata(obj);
        let result = policy(&BurnedInPixelPolicyContext {
            report: report.burned_in_pixel.clone(),
            metadata,
            rows,
            columns,
        });
        if !result.regions.is_empty() {
            match pixel_redaction::mask_pixel_regions(obj, &result.regions, &result.fill) {
                Ok(regions) => report.burned_in_pixel.redacted_regions = regions,
                Err(e) => return (report, Some(e)),
            }
        }
    }
    anonymize(obj, opts, uids, true, &mut report);
    if !opts.edits.is_empty() {
        let (edited, err) = opts.edits.apply(obj, &opts.edit_options);
        report.edited_attributes = edited;
        if let Some(err) = err {
            return (report, Some(err.into()));
        }
    }
    (report, None)
}
pub(crate) fn clone_file(src: &File) -> Result<File, Error> {
    let bytes = object::writer::write_file(src)?;
    object::read_file(&bytes).map_err(|(_, e)| e.into())
}
pub(crate) fn rederive_file_meta(mut file: File) -> Result<File, Error> {
    file.preamble = Vec::new();
    file.meta = Object::new();
    file.rebuild_file_meta()?;
    Ok(file)
}
#[allow(clippy::result_large_err)]
pub fn clone_anonymized_file(
    src: &File,
    opts: &Options,
    uids: &mut UidRemapper,
) -> Result<(File, Report), (Report, Error)> {
    let mut clone = clone_file(src).map_err(|e| (Report::default(), e))?;
    let (report, err) = anonymize_object_with_report(&mut clone.dataset, opts, uids);
    if let Some(err) = err {
        return Err((report, err));
    }
    let clone = rederive_file_meta(clone).map_err(|e| (report.clone(), e))?;
    Ok((clone, report))
}
pub fn remap_hierarchy_uids(obj: &mut Object, uids: &mut UidRemapper) {
    remap_uid(obj, uids, TAG_STUDY_INSTANCE_UID, "\0study", true);
    remap_uid(obj, uids, TAG_SERIES_INSTANCE_UID, "\0series", true);
    remap_uid(obj, uids, TAG_SOP_INSTANCE_UID, "\0sop", true);
}
pub fn clone_with_remapped_hierarchy_uids(
    src: &File,
    uids: &mut UidRemapper,
) -> Result<File, Error> {
    let mut clone = clone_file(src)?;
    remap_hierarchy_uids(&mut clone.dataset, uids);
    rederive_file_meta(clone)
}
fn anonymize(
    obj: &mut Object,
    opts: &Options,
    uids: &mut UidRemapper,
    root: bool,
    report: &mut Report,
) {
    rewrite_sequences_and_private_tags(obj, opts, uids, report);
    apply_direct_identifiers(obj, opts, uids, root);
}
fn private_creators(elements: &[Element]) -> HashMap<Tag, String> {
    let mut creators = HashMap::new();
    for e in elements {
        if !is_private_creator_tag(e.tag()) {
            continue;
        }
        let value = trim_space(&string_value(e)).to_string();
        if !value.is_empty() {
            creators.insert(e.tag(), value);
        }
    }
    creators
}
fn is_overlay_tag(tag: Tag) -> bool {
    (0x6000..=0x60ff).contains(&tag.group) && tag.group.is_multiple_of(2)
}
fn rewrite_sequences_and_private_tags(
    obj: &mut Object,
    opts: &Options,
    uids: &mut UidRemapper,
    report: &mut Report,
) {
    let creators = private_creators(obj.elements());
    let elements = obj.elements().to_vec();
    for mut elem in elements {
        let tag = elem.tag();
        if is_overlay_tag(tag) {
            obj.remove(tag);
            continue;
        }
        if tag.is_private() {
            if private_tag_action(opts, &elem, &creators) != PrivateTagAction::Retain {
                obj.remove(tag);
                continue;
            }
            let creator = creators
                .get(&private_creator_tag_for_data_tag(tag))
                .cloned()
                .unwrap_or_default();
            if is_csa_header(tag, &creator) {
                let raw = obj.get_raw(tag).unwrap_or_default();
                let (scrubbed, outcome) = scrub_csa_element(&elem, &creator, &raw, &mut report.csa);
                match outcome {
                    CsaOutcome::Scrubbed => obj.put(scrubbed),
                    CsaOutcome::Unscrubbable => {
                        obj.remove(tag);
                    }
                    _ => {}
                }
                continue;
            }
            if let Value::Sequence(items) = &elem.value {
                elem.value = Value::Sequence(anonymized_sequence(items, opts, uids, report));
                obj.put(elem);
            }
            continue;
        }
        let Value::Sequence(items) = &elem.value else {
            continue;
        };
        elem.value = Value::Sequence(anonymized_sequence(items, opts, uids, report));
        obj.put(elem);
    }
}
fn anonymized_sequence(
    items: &[DataSet],
    opts: &Options,
    uids: &mut UidRemapper,
    report: &mut Report,
) -> Vec<DataSet> {
    items
        .iter()
        .map(|item| {
            let mut obj = Object::from_data_set(item.clone());
            anonymize(&mut obj, opts, uids, false, report);
            obj.to_data_set()
        })
        .collect()
}
fn private_tag_action(
    opts: &Options,
    elem: &Element,
    creators: &HashMap<Tag, String>,
) -> PrivateTagAction {
    let Some(policy) = &opts.private_tag_policy else {
        return PrivateTagAction::Remove;
    };
    let creator_tag = private_creator_tag_for_data_tag(elem.tag());
    policy(&PrivateTagContext {
        tag: elem.tag(),
        element: elem,
        creator_tag,
        creator: creators.get(&creator_tag).cloned().unwrap_or_default(),
    })
}
fn apply_direct_identifiers(obj: &mut Object, opts: &Options, uids: &mut UidRemapper, root: bool) {
    let name = if opts.patient_name.is_empty() {
        "Anonymous"
    } else {
        &opts.patient_name
    };
    let id = if opts.patient_id.is_empty() {
        "ANON"
    } else {
        &opts.patient_id
    };
    if root || obj.has(TAG_PATIENT_NAME) {
        obj.put(raw_element(TAG_PATIENT_NAME, Vr::PN, pad_text(name)));
    }
    if root || obj.has(TAG_PATIENT_ID) {
        obj.put(raw_element(TAG_PATIENT_ID, Vr::LO, pad_text(id)));
    }
    let blank = |obj: &mut Object, tag: Tag, vr: Vr| {
        if !root && !obj.has(tag) {
            return;
        }
        obj.put(raw_element(tag, vr, Vec::new()));
    };
    blank(obj, TAG_PATIENT_BIRTH_DATE, Vr::DA);
    blank(obj, Tag::new(0x0010, 0x1000), Vr::LO);
    blank(obj, Tag::new(0x0010, 0x1040), Vr::LO);
    blank(obj, Tag::new(0x0008, 0x0080), Vr::LO);
    blank(obj, Tag::new(0x0008, 0x0090), Vr::PN);
    blank(obj, Tag::new(0x0008, 0x1048), Vr::PN);
    blank(obj, Tag::new(0x0008, 0x1050), Vr::PN);
    blank(obj, Tag::new(0x0008, 0x1070), Vr::PN);
    blank(obj, Tag::new(0x0008, 0x0050), Vr::SH);
    let blank_if_present = |obj: &mut Object, (g, e, vr): (u16, u16, Vr)| {
        let tag = Tag::new(g, e);
        if obj.has(tag) {
            obj.put(raw_element(tag, vr, Vec::new()));
        }
    };
    for attribute in BASIC_PROFILE_TEXT_ATTRIBUTES {
        blank_if_present(obj, attribute);
    }
    for (g, e) in IDENTIFYING_SEQUENCE_ATTRIBUTES {
        obj.remove(Tag::new(g, e));
    }
    if !opts.keep_dates {
        for attribute in LONGITUDINAL_DATE_TIME_ATTRIBUTES {
            blank_if_present(obj, attribute);
        }
    }
    for (g, e) in INSTANCE_UID_ATTRIBUTES {
        let tag = Tag::new(g, e);
        let force = root && is_hierarchy_uid(tag);
        remap_uid(obj, uids, tag, &format!("\0uid:{tag}"), force);
    }
}
fn is_hierarchy_uid(tag: Tag) -> bool {
    tag == TAG_STUDY_INSTANCE_UID || tag == TAG_SERIES_INSTANCE_UID || tag == TAG_SOP_INSTANCE_UID
}
fn remap_uid(obj: &mut Object, uids: &mut UidRemapper, tag: Tag, empty_key: &str, force: bool) {
    if !force && !obj.has(tag) {
        return;
    }
    let old_values = obj.get_strings(tag).unwrap_or_default();
    if old_values.is_empty() {
        let uid = uids.map_missing(empty_key);
        obj.put(raw_element(tag, Vr::UI, pad_uid(&uid)));
        return;
    }
    let mut mapped = Vec::with_capacity(old_values.len());
    for (i, old) in old_values.iter().enumerate() {
        let old = object::text::normalize_uid(old);
        if old.is_empty() {
            mapped.push(uids.map_missing(&format!("{empty_key}:{i}")));
            continue;
        }
        mapped.push(uids.map(old));
    }
    obj.put(raw_element(tag, Vr::UI, pad_uid(&mapped.join("\\"))));
}
fn single_cs_value(obj: &Object, tag: Tag) -> Option<String> {
    let element = obj.get(tag)?;
    if element.vr() != Vr::CS {
        return None;
    }
    let values = string_values(element);
    if values.len() != 1 {
        return None;
    }
    let value = crate::textfmt::to_upper(trim_space(&values[0]));
    (!value.is_empty()).then_some(value)
}
pub(crate) fn single_cs(obj: &Object, tag: Tag) -> Option<String> {
    single_cs_value(obj, tag)
}
fn assess(obj: &Object, tag: Tag) -> (BurnedInPixelRisk, String) {
    match single_cs_value(obj, tag) {
        None if obj.has(tag) => (BurnedInPixelRisk::Unknown, "OTHER".into()),
        None => (BurnedInPixelRisk::Unknown, String::new()),
        Some(v) if v == "YES" => (BurnedInPixelRisk::Present, v),
        Some(v) if v == "NO" => (BurnedInPixelRisk::Absent, v),
        Some(_) => (BurnedInPixelRisk::Unknown, "OTHER".into()),
    }
}
fn assess_burned_in_pixel(obj: &Object) -> BurnedInPixelReport {
    let (risk, metadata_value) = assess(obj, TAG_BURNED_IN_ANNOTATION);
    BurnedInPixelReport {
        risk,
        metadata_value,
        redacted_regions: Vec::new(),
    }
}
fn assess_recognizable_visual_features(obj: &Object) -> RecognizableVisualFeaturesReport {
    let (risk, metadata_value) = assess(obj, TAG_RECOGNIZABLE_VISUAL_FEATURES);
    RecognizableVisualFeaturesReport {
        risk,
        metadata_value,
    }
}
const CSA_CREATOR: &str = "SIEMENS CSA HEADER";
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CsaOutcome {
    NotHeader,
    Scrubbed,
    Kept,
    Unscrubbable,
}
pub(crate) fn is_csa_header(tag: Tag, creator: &str) -> bool {
    let offset = tag.element & 0xFF;
    tag.group == 0x0029
        && tag.element >= 0x1000
        && creator == CSA_CREATOR
        && (offset == 0x10 || offset == 0x20)
}
pub(crate) fn scrub_csa_element(
    element: &Element,
    creator: &str,
    raw: &[u8],
    report: &mut CsaScrubReport,
) -> (Element, CsaOutcome) {
    if !is_csa_header(element.tag(), creator) {
        return (element.clone(), CsaOutcome::NotHeader);
    }
    if matches!(element.value, Value::Sequence(_)) {
        report.unscrubbable += 1;
        return (element.clone(), CsaOutcome::Unscrubbable);
    }
    let (scrubbed, result, err) = siemens::scrub_csa(raw, siemens::CsaLimits::default());
    if err.is_none() {
        report.scrubbed += 1;
        report.bytes_cleared += result.cleared;
        return (
            raw_element(element.tag(), element.vr(), scrubbed.unwrap_or_default()),
            CsaOutcome::Scrubbed,
        );
    }
    if is_unscrubbed_csa_format(raw) {
        return (element.clone(), CsaOutcome::Kept);
    }
    report.unscrubbable += 1;
    (element.clone(), CsaOutcome::Unscrubbable)
}
fn is_unscrubbed_csa_format(raw: &[u8]) -> bool {
    matches!(
        siemens::parse_csa(raw).map(|h| h.format()),
        Ok(siemens::CsaFormat::Dataset | siemens::CsaFormat::Interfile | siemens::CsaFormat::Xml)
    )
}
