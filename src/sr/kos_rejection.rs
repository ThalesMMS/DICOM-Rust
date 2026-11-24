use super::code_value::{from_code, valid_code};
use super::read::{ReadOptions, read_document_with_options};
use super::reference::ValidationMode;
use super::template_catalog::{new_standard_template_registry, standard_template_key};
use super::template_registry::{
    TemplateRegistry, TemplateValidationOptions, new_template_validator,
};
use super::{
    CodeContext, CodedEntry, CompositeReference, ContentItem, DerivedCommon, Document,
    ImageReference, KEY_OBJECT_SELECTION_DOCUMENT_STORAGE, Kind, RELATIONSHIP_CONTAINS,
    RELATIONSHIP_HAS_CONCEPT_MOD, SrError, hierarchical_reference_sequence, str_elem, tags as tg,
    value,
};
use crate::codes::{self, dcm};
use crate::dcmtime::{self, ZonedTime};
use crate::dictionary::uid::{StorageCategory, lookup_storage_sop_class};
use crate::object::{self, Element, Object, Tag, Value, Vr, inherit};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, OnceLock};
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KosRejectionKind(pub u8);
impl KosRejectionKind {
    pub const QUALITY: KosRejectionKind = KosRejectionKind(1);
    pub const PATIENT_SAFETY: KosRejectionKind = KosRejectionKind(2);
    pub const INCORRECT_WORKLIST: KosRejectionKind = KosRejectionKind(3);
    pub const RETENTION_EXPIRED: KosRejectionKind = KosRejectionKind(4);
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KosRejectionReference {
    pub value_type: String,
    pub study_instance_uid: String,
    pub series_instance_uid: String,
    pub sop_class_uid: String,
    pub sop_instance_uid: String,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KosRejectionLimits {
    pub max_references: i64,
    pub max_items: i64,
    pub max_elements: i64,
    pub max_depth: i64,
    pub max_quality_reasons: i64,
    pub max_description_bytes: i64,
    pub max_metadata_bytes: i64,
}
pub fn default_kos_rejection_limits() -> KosRejectionLimits {
    KosRejectionLimits {
        max_references: 10000,
        max_items: 50000,
        max_elements: 200000,
        max_depth: 32,
        max_quality_reasons: 16,
        max_description_bytes: 65536,
        max_metadata_bytes: 1 << 20,
    }
}
#[derive(Clone, Debug, Default)]
pub struct KosRejectionOptions {
    pub sop_instance_uid: String,
    pub study_instance_uid: String,
    pub series_instance_uid: String,
    pub created_at: Option<ZonedTime>,
    pub series_number: String,
    pub instance_number: String,
    pub references: Vec<KosRejectionReference>,
    pub quality_reasons: Vec<CodedEntry>,
    pub description: String,
    pub common: DerivedCommon,
    pub limits: KosRejectionLimits,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KosRejectionNote {
    pub kind: KosRejectionKind,
    pub title: CodedEntry,
    pub sop_instance_uid: String,
    pub study_instance_uid: String,
    pub series_instance_uid: String,
    pub content_date: String,
    pub content_time: String,
    pub series_number: String,
    pub instance_number: String,
    pub references: Vec<KosRejectionReference>,
    pub quality_reasons: Vec<CodedEntry>,
    pub description: String,
}
const TAG_TIMEZONE_OFFSET: Tag = Tag::new(0x0008, 0x0201);
const TAG_SPECIFIC_CHARACTER_SET: Tag = Tag::new(0x0008, 0x0005);
fn kos_invalid(reason: &str) -> SrError {
    SrError::new(Kind::InvalidDocument, &format!(": KOS rejection {reason}"))
}
fn kos_limit(reason: &str) -> SrError {
    SrError::new(
        Kind::ResourceLimitExceeded,
        &format!(": KOS rejection {reason}"),
    )
}
fn kos_rejection_title(kind: KosRejectionKind) -> CodedEntry {
    match kind {
        KosRejectionKind::QUALITY => from_code(dcm::RejectedForQualityReasons),
        KosRejectionKind::PATIENT_SAFETY => from_code(dcm::RejectedForPatientSafetyReasons),
        KosRejectionKind::INCORRECT_WORKLIST => from_code(dcm::IncorrectModalityWorklistEntry),
        KosRejectionKind::RETENTION_EXPIRED => from_code(dcm::DataRetentionPolicyExpired),
        _ => CodedEntry::default(),
    }
}
fn normalize_kos_limits(mut l: KosRejectionLimits) -> Result<KosRejectionLimits, SrError> {
    let d = default_kos_rejection_limits();
    let fields = [
        (&mut l.max_references, d.max_references),
        (&mut l.max_items, d.max_items),
        (&mut l.max_elements, d.max_elements),
        (&mut l.max_depth, d.max_depth),
        (&mut l.max_quality_reasons, d.max_quality_reasons),
        (&mut l.max_description_bytes, d.max_description_bytes),
        (&mut l.max_metadata_bytes, d.max_metadata_bytes),
    ];
    for (f, cap) in fields {
        if *f < 0 || *f > cap {
            return Err(kos_limit("invalid limits"));
        }
        if *f == 0 {
            *f = cap;
        }
    }
    Ok(l)
}
struct KosBudget {
    limits: KosRejectionLimits,
    elements: i64,
    items: i64,
    bytes: i64,
}
impl KosBudget {
    fn new(limits: KosRejectionLimits) -> KosBudget {
        KosBudget {
            limits,
            elements: 0,
            items: 0,
            bytes: 0,
        }
    }
    fn add_bytes(&mut self, n: i64) -> Result<(), SrError> {
        if n < 0 || n > self.limits.max_metadata_bytes - self.bytes {
            return Err(kos_limit("metadata bytes"));
        }
        self.bytes += n;
        Ok(())
    }
    fn walk(&mut self, elements: &[Element], depth: i64, source: bool) -> Result<(), SrError> {
        if depth > self.limits.max_depth {
            return Err(kos_limit("depth"));
        }
        if elements.len() as i64 > self.limits.max_elements - self.elements {
            return Err(kos_limit("elements"));
        }
        self.elements += elements.len() as i64;
        let mut seen = HashSet::with_capacity(elements.len());
        for e in elements {
            if !seen.insert(e.tag()) {
                return Err(kos_invalid("duplicate item attribute"));
            }
            if source && depth == 0 && kos_bulk_tag(e.tag()) {
                continue;
            }
            if let Value::Sequence(items) = &e.value {
                if e.vr() != Vr::SQ {
                    return Err(kos_invalid("sequence VR"));
                }
                if items.len() as i64 > self.limits.max_items - self.items {
                    return Err(kos_limit("items"));
                }
                self.items += items.len() as i64;
                for item in items {
                    self.walk(&item.elements, depth + 1, source)?;
                }
                continue;
            }
            if e.vr() == Vr::SQ {
                return Err(kos_invalid("unavailable sequence"));
            }
            let n: i64 = match &e.value {
                Value::Nil => {
                    if !e.header.length_set || e.header.length != 0 {
                        return Err(kos_invalid("deferred metadata; materialize before use"));
                    }
                    continue;
                }
                Value::Raw(v) => {
                    let mut n = v.len() as i64;
                    if e.vr().uses_specific_character_set() {
                        if n > (self.limits.max_metadata_bytes - self.bytes) / 4 {
                            return Err(kos_limit("decoded metadata bytes"));
                        }
                        n *= 4;
                    }
                    n
                }
                Value::Strings(v) => {
                    if v.len() as i64 > self.limits.max_items {
                        return Err(kos_limit("value multiplicity"));
                    }
                    let mut n = if v.is_empty() { 0 } else { v.len() as i64 - 1 };
                    for s in v {
                        if s.len() as i64 > self.limits.max_metadata_bytes - n {
                            return Err(kos_limit("metadata bytes"));
                        }
                        n += s.len() as i64;
                    }
                    n
                }
                _ => return Err(kos_invalid("unavailable metadata payload")),
            };
            self.add_bytes(n)?;
        }
        Ok(())
    }
}
fn kos_bulk_tag(t: Tag) -> bool {
    t == Tag::new(0x7FE0, 0x0010)
        || t == Tag::new(0x7FE0, 0x0008)
        || t == Tag::new(0x7FE0, 0x0009)
        || t == Tag::new(0x5600, 0x0020)
}
fn kos_facade(src: Option<&Object>, b: &mut KosBudget, common: bool) -> Result<Object, SrError> {
    let Some(src) = src else {
        return Err(SrError::new(Kind::NoDataset, ""));
    };
    if src.len() as i64 > b.limits.max_elements - b.elements {
        return Err(kos_limit("elements"));
    }
    let elems = src.elements().to_vec();
    b.walk(&elems, 0, common)?;
    let mut view = Object::from_elements(elems);
    view.set_value_big_endian(src.value_big_endian());
    Ok(view)
}
fn kos_string(o: &Object, t: Tag, vr: Vr, required: bool) -> Result<String, SrError> {
    let Some(e) = o.get(t) else {
        if required {
            return Err(kos_invalid("missing attribute"));
        }
        return Ok(String::new());
    };
    if e.vr() != vr {
        return Err(kos_invalid("attribute VR"));
    }
    let values = match o.lookup_strings(t) {
        Ok(v) if v.len() <= 1 && (!required || v.len() == 1) => v,
        _ => return Err(kos_invalid("attribute VM or text")),
    };
    let Some(s) = values.into_iter().next() else {
        return Ok(String::new());
    };
    if !kos_text_clean(&s) || (required && s.is_empty()) {
        return Err(kos_invalid("attribute text"));
    }
    Ok(s)
}
fn kos_bound_sequence(o: &Object, t: Tag, min: i64, max: i64) -> Result<Vec<Object>, SrError> {
    if let Some(Element {
        value: Value::Sequence(v),
        ..
    }) = o.get(t)
        && v.len() as i64 > max
    {
        return Err(kos_limit("reference sequence"));
    }
    kos_sequence(o, t, min, max)
}
fn kos_sequence(o: &Object, t: Tag, min: i64, max: i64) -> Result<Vec<Object>, SrError> {
    let e = match o.get(t) {
        Some(e) if e.vr() == Vr::SQ => e,
        _ => return Err(kos_invalid("missing sequence or VR")),
    };
    match &e.value {
        Value::Sequence(v) if v.len() as i64 >= min && v.len() as i64 <= max => {}
        _ => return Err(kos_invalid("sequence cardinality")),
    }
    o.get_sequence(t)
        .ok_or_else(|| kos_invalid("unavailable sequence"))
}
fn kos_code(o: &Object, tag: Tag) -> Result<CodedEntry, SrError> {
    let items = kos_sequence(o, tag, 1, 1)?;
    let it = &items[0];
    let mut expected = 3;
    if it.has(tg::CODING_SCHEME_VERSION) {
        expected += 1;
    }
    if it.len() != expected
        || it.has(tg::LONG_CODE_VALUE)
        || it.has(tg::URN_CODE_VALUE)
        || it.has(tg::EQUIVALENT_CODE_SEQUENCE)
    {
        return Err(kos_invalid("unsupported code form or context"));
    }
    let value = kos_string(it, tg::CODE_VALUE, Vr::SH, true);
    let scheme = kos_string(it, tg::CODING_SCHEME, Vr::SH, true);
    let meaning = kos_string(it, tg::CODE_MEANING, Vr::LO, true);
    let ok = value.is_ok() && scheme.is_ok() && meaning.is_ok();
    let c = CodedEntry {
        code_value: value.unwrap_or_default(),
        coding_scheme_designator: scheme.unwrap_or_default(),
        code_meaning: meaning.unwrap_or_default(),
    };
    if !ok || !kos_valid_code(&c) {
        return Err(kos_invalid("invalid code"));
    }
    kos_string(it, tg::CODING_SCHEME_VERSION, Vr::SH, false)?;
    Ok(c)
}
fn kos_valid_code(c: &CodedEntry) -> bool {
    valid_code(c, &CodeContext::default())
        && c.code_value.chars().count() <= 16
        && c.coding_scheme_designator.chars().count() <= 16
        && c.code_meaning.chars().count() <= 64
}
fn kos_quality_code(c: &CodedEntry) -> bool {
    if !kos_valid_code(c) {
        return false;
    }
    let Some(g) = codes::context_group_by_id(7011) else {
        return false;
    };
    g.member(&codes::Code {
        value: &c.code_value,
        scheme: &c.coding_scheme_designator,
        meaning: "",
    })
    .is_some()
}
fn valid_uid(s: &str) -> bool {
    object::is_valid_uid(s.as_bytes())
}
fn kos_reference_valid(r: &KosRejectionReference) -> bool {
    if r.value_type != value::IMAGE
        && r.value_type != value::WAVEFORM
        && r.value_type != value::COMPOSITE
    {
        return false;
    }
    for s in [
        &r.study_instance_uid,
        &r.series_instance_uid,
        &r.sop_class_uid,
        &r.sop_instance_uid,
    ] {
        if !valid_uid(s) {
            return false;
        }
    }
    let Some(c) = lookup_storage_sop_class(&r.sop_class_uid) else {
        return false;
    };
    !matches!(
        c.category,
        StorageCategory::NonPatient
            | StorageCategory::Protocol
            | StorageCategory::Security
            | StorageCategory::Ndt
            | StorageCategory::Legacy
    )
}
fn kos_same_code(a: &CodedEntry, value: &str) -> bool {
    a.coding_scheme_designator == "DCM" && a.code_value == value
}
fn kos_identity_roles(
    roles: &mut HashMap<String, u8>,
    study: &str,
    series: &str,
    class: &str,
    instance: &str,
) -> bool {
    for (i, s) in [study, series, class, instance].into_iter().enumerate() {
        let role = i as u8 + 1;
        if let Some(old) = roles.get(s)
            && *old != role
        {
            return false;
        }
        roles.insert(s.to_string(), role);
    }
    true
}
fn kos_registry() -> Result<Arc<TemplateRegistry>, SrError> {
    static REGISTRY: OnceLock<Result<Arc<TemplateRegistry>, SrError>> = OnceLock::new();
    REGISTRY
        .get_or_init(|| new_standard_template_registry(&[2010]).map(Arc::new))
        .clone()
}
fn kos_validate_template(doc: Option<&Document>, l: &KosRejectionLimits) -> Result<(), SrError> {
    let registry = kos_registry()?;
    let v = new_template_validator(
        Some(registry),
        TemplateValidationOptions {
            mode: ValidationMode::STRICT,
            max_depth: l.max_depth,
            max_items: l.max_items,
            max_steps: 1_000_000,
            max_findings: 64,
        },
    )?;
    let key = standard_template_key(2010).unwrap_or_default();
    match v.validate(doc, &key).1 {
        Some(e) => Err(e),
        None => Ok(()),
    }
}
fn kos_positive_is(s: &str) -> bool {
    if s.is_empty() || s.len() > 12 {
        return false;
    }
    let mut positive = false;
    for c in s.chars() {
        if !c.is_ascii_digit() {
            return false;
        }
        positive = positive || c != '0';
    }
    positive
}
fn kos_no_selectors(o: &Object) -> bool {
    !o.has(tg::REF_FRAME_NUMBER)
        && !o.has(tg::REFERENCED_WAVEFORM_CH)
        && !o.has(Tag::new(0x0062, 0x000B))
        && !o.has(Tag::new(0x0040, 0xA170))
}
fn kos_text_clean(s: &str) -> bool {
    !s.contains('\0')
}
fn local_now() -> ZonedTime {
    use chrono::{Datelike, Timelike};
    let now = chrono::Local::now();
    ZonedTime::from_fields(
        i64::from(now.year()),
        i64::from(now.month()),
        i64::from(now.day()),
        i64::from(now.hour()),
        i64::from(now.minute()),
        i64::from(now.second()),
        i64::from(now.nanosecond()),
        i64::from(now.offset().local_minus_utc()),
    )
}
fn in_offset(t: ZonedTime, offset: i64) -> ZonedTime {
    ZonedTime::from_fields(
        t.year(),
        t.month(),
        t.day(),
        t.hour(),
        t.minute(),
        t.second() - t.offset() + offset,
        t.nanosecond(),
        offset,
    )
}
pub fn new_kos_rejection(
    kind: KosRejectionKind,
    opts: &KosRejectionOptions,
) -> Result<Object, SrError> {
    let l = normalize_kos_limits(opts.limits)?;
    if kos_rejection_title(kind).empty() {
        return Err(kos_invalid("unknown kind"));
    }
    for s in [
        &opts.sop_instance_uid,
        &opts.study_instance_uid,
        &opts.series_instance_uid,
    ] {
        if !valid_uid(s) {
            return Err(kos_invalid("note UID"));
        }
    }
    if opts.references.is_empty() {
        return Err(kos_invalid("no references"));
    }
    if opts.references.len() as i64 > l.max_references
        || opts.quality_reasons.len() as i64 > l.max_quality_reasons
        || opts.description.len() as i64 > l.max_description_bytes
    {
        return Err(kos_limit("options"));
    }
    if (kind == KosRejectionKind::QUALITY && opts.quality_reasons.is_empty())
        || (kind != KosRejectionKind::QUALITY && !opts.quality_reasons.is_empty())
    {
        return Err(kos_invalid("quality reasons"));
    }
    if !kos_text_clean(&opts.description)
        || (!opts.description.is_empty() && opts.description.trim().is_empty())
    {
        return Err(kos_invalid("description"));
    }
    let series_number = if opts.series_number.is_empty() {
        "1"
    } else {
        &opts.series_number
    };
    let instance_number = if opts.instance_number.is_empty() {
        "1"
    } else {
        &opts.instance_number
    };
    if !kos_positive_is(series_number) || !kos_positive_is(instance_number) {
        return Err(kos_invalid("instance or series number"));
    }
    let mut b = KosBudget::new(l);
    for s in [
        &opts.sop_instance_uid,
        &opts.study_instance_uid,
        &opts.series_instance_uid,
        &opts.description,
    ] {
        b.add_bytes(s.len() as i64)?;
    }
    b.add_bytes(series_number.len() as i64)?;
    b.add_bytes(instance_number.len() as i64)?;
    b.add_bytes(1024 + 128 * opts.quality_reasons.len() as i64)?;
    for r in &opts.references {
        if !kos_reference_valid(r)
            || r.study_instance_uid != opts.study_instance_uid
            || r.series_instance_uid == opts.series_instance_uid
            || r.sop_instance_uid == opts.sop_instance_uid
        {
            return Err(kos_invalid("reference identity or profile"));
        }
        for s in [
            &r.study_instance_uid,
            &r.series_instance_uid,
            &r.sop_class_uid,
            &r.sop_instance_uid,
        ] {
            b.add_bytes(s.len() as i64 * 3)?;
        }
    }
    for c in &opts.quality_reasons {
        if !kos_quality_code(c) || !kos_text_clean(&c.code_meaning) {
            return Err(kos_invalid("quality code outside vendored CID 7011"));
        }
        for s in [&c.code_value, &c.coding_scheme_designator, &c.code_meaning] {
            b.add_bytes(s.len() as i64)?;
        }
    }
    if let Some(e) = &opts.common.equipment {
        for s in [
            &e.manufacturer,
            &e.model_name,
            &e.device_serial_number,
            &e.software_versions,
        ] {
            if !kos_text_clean(s) || s.len() > 64 {
                return Err(kos_invalid("equipment text"));
            }
            b.add_bytes(s.len() as i64)?;
        }
    }
    let mut common = opts.common.clone();
    if let Some(source) = &common.source {
        let facade = kos_facade(Some(source), &mut b, true)?;
        let study = kos_string(&facade, tg::STUDY_INSTANCE_UID, Vr::UI, true);
        if study.as_deref().ok() != Some(opts.study_instance_uid.as_str()) {
            return Err(kos_invalid("source study"));
        }
        if facade.character_set().is_err() {
            return Err(kos_invalid("source character set"));
        }
        common.source = Some(facade);
    }
    let refs_n = opts.references.len() as i64;
    let reasons_n = opts.quality_reasons.len() as i64;
    if 64 + 12 * refs_n + 12 * reasons_n > l.max_elements - b.elements
        || 8 + 4 * refs_n + 4 * reasons_n > l.max_items - b.items
        || l.max_depth < 3
    {
        return Err(kos_limit("generated structure"));
    }
    let mut created = opts.created_at.or(common.now).unwrap_or_else(local_now);
    if let Some(source) = &common.source
        && source.has(TAG_TIMEZONE_OFFSET)
    {
        let offset = kos_string(source, TAG_TIMEZONE_OFFSET, Vr::SH, true);
        let Ok(offset) = offset
            .map_err(|_| ())
            .and_then(|o| if o.len() == 5 { Ok(o) } else { Err(()) })
        else {
            return Err(kos_invalid("source timezone offset"));
        };
        match dcmtime::parse_datetime(&format!("20000101000000{offset}")) {
            Ok(dt) if !dt.no_offset => created = in_offset(created, dt.time.offset()),
            _ => return Err(kos_invalid("source timezone offset")),
        }
    }
    if created.year() < 1 || created.year() > 9999 {
        return Err(kos_invalid("creation year"));
    }
    common.now = Some(created);
    let mut identities = HashMap::new();
    if !kos_identity_roles(
        &mut identities,
        &opts.study_instance_uid,
        &opts.series_instance_uid,
        KEY_OBJECT_SELECTION_DOCUMENT_STORAGE,
        &opts.sop_instance_uid,
    ) {
        return Err(kos_invalid("UID role collision"));
    }
    let mut refs: Vec<&KosRejectionReference> = Vec::with_capacity(opts.references.len());
    let mut seen: HashMap<&str, &KosRejectionReference> = HashMap::new();
    for r in &opts.references {
        if !kos_identity_roles(
            &mut identities,
            &r.study_instance_uid,
            &r.series_instance_uid,
            &r.sop_class_uid,
            &r.sop_instance_uid,
        ) {
            return Err(kos_invalid("UID role collision"));
        }
        if let Some(previous) = seen.get(r.sop_instance_uid.as_str()) {
            if *previous != r {
                return Err(kos_invalid("conflicting reference"));
            }
            continue;
        }
        seen.insert(&r.sop_instance_uid, r);
        refs.push(r);
    }
    let mut doc = Document {
        sop_class_uid: KEY_OBJECT_SELECTION_DOCUMENT_STORAGE.into(),
        sop_instance_uid: opts.sop_instance_uid.clone(),
        study_instance_uid: opts.study_instance_uid.clone(),
        series_instance_uid: opts.series_instance_uid.clone(),
        modality: "KO".into(),
        title: kos_rejection_title(kind),
        content_date: format!(
            "{:04}{:02}{:02}",
            created.year(),
            created.month(),
            created.day()
        ),
        content_time: format!(
            "{:02}{:02}{:02}",
            created.hour(),
            created.minute(),
            created.second()
        ),
        series_number: series_number.into(),
        instance_number: instance_number.into(),
        common: common.clone(),
        ..Default::default()
    };
    for c in &opts.quality_reasons {
        doc.content.push(ContentItem {
            value_type: value::CODE.into(),
            relationship_type: RELATIONSHIP_HAS_CONCEPT_MOD.into(),
            concept_name: from_code(dcm::DocumentTitleModifier),
            code: c.clone(),
            ..Default::default()
        });
    }
    if !opts.description.is_empty() {
        doc.content.push(ContentItem {
            value_type: value::TEXT.into(),
            relationship_type: RELATIONSHIP_CONTAINS.into(),
            concept_name: from_code(dcm::KeyObjectDescription),
            text: opts.description.clone(),
            ..Default::default()
        });
    }
    let mut evidence = Vec::with_capacity(refs.len());
    for r in refs {
        let mut item = ContentItem {
            value_type: r.value_type.clone(),
            relationship_type: RELATIONSHIP_CONTAINS.into(),
            ..Default::default()
        };
        let image = ImageReference {
            study_instance_uid: r.study_instance_uid.clone(),
            series_instance_uid: r.series_instance_uid.clone(),
            sop_class_uid: r.sop_class_uid.clone(),
            sop_instance_uid: r.sop_instance_uid.clone(),
            frames: Vec::new(),
        };
        let composite = CompositeReference {
            sop_class_uid: r.sop_class_uid.clone(),
            sop_instance_uid: r.sop_instance_uid.clone(),
        };
        match r.value_type.as_str() {
            value::IMAGE => item.image = image.clone(),
            value::COMPOSITE => item.composite = composite,
            value::WAVEFORM => item.waveform.composite = composite,
            _ => {}
        }
        doc.content.push(item);
        evidence.push(image);
    }
    kos_validate_template(Some(&doc), &l)?;
    let elems = doc.elements()?;
    let mut dataset = Object::from_elements(elems);
    dataset.put(str_elem(TAG_SPECIFIC_CHARACTER_SET, Vr::CS, "ISO_IR 192"));
    dataset.put(hierarchical_reference_sequence(
        tg::CURRENT_REQUESTED_PROCEDURE_EVIDENCE,
        &evidence,
    ));
    if let Some(source) = &common.source {
        dataset.set_value_big_endian(source.value_big_endian());
    }
    common
        .apply(
            &mut dataset,
            inherit::inherit_modules::WITHOUT_FRAME_OF_REFERENCE,
        )
        .map_err(|e| SrError::msg(e.to_string()))?;
    let mut final_budget = KosBudget::new(l);
    if dataset.len() as i64 > l.max_elements {
        return Err(kos_limit("output elements"));
    }
    final_budget.walk(dataset.elements(), 0, false)?;
    read_kos_rejection(Some(&dataset), l)?;
    Ok(dataset)
}
pub fn read_kos_rejection(
    source: Option<&Object>,
    limits: KosRejectionLimits,
) -> Result<KosRejectionNote, SrError> {
    let l = normalize_kos_limits(limits)?;
    let mut b = KosBudget::new(l);
    let o = kos_facade(source, &mut b, true)?;
    if o.character_set().is_err() {
        return Err(kos_invalid("character set"));
    }
    let sop = kos_string(&o, tg::SOP_CLASS_UID, Vr::UI, true)?;
    if sop != KEY_OBJECT_SELECTION_DOCUMENT_STORAGE {
        return Err(SrError::new(Kind::NotKosRejection, ""));
    }
    let mut note = KosRejectionNote {
        title: kos_code(&o, tg::CONCEPT_NAME_CODE_SEQ)?,
        ..Default::default()
    };
    for kind in [
        KosRejectionKind::QUALITY,
        KosRejectionKind::PATIENT_SAFETY,
        KosRejectionKind::INCORRECT_WORKLIST,
        KosRejectionKind::RETENTION_EXPIRED,
    ] {
        let t = kos_rejection_title(kind);
        if note.title.code_value == t.code_value
            && note.title.coding_scheme_designator == t.coding_scheme_designator
        {
            note.kind = kind;
            break;
        }
    }
    if note.kind.0 == 0 {
        return Err(SrError::new(Kind::NotKosRejection, ""));
    }
    for (tag, vr, want) in [
        (tg::MODALITY, Vr::CS, "KO"),
        (tg::VALUE_TYPE, Vr::CS, value::CONTAINER),
        (tg::CONTINUITY_OF_CONTENT, Vr::CS, "SEPARATE"),
    ] {
        match kos_string(&o, tag, vr, true) {
            Ok(got) if got == want => {}
            _ => return Err(kos_invalid("root content or modality")),
        }
    }
    for (tag, out) in [
        (tg::SOP_INSTANCE_UID, &mut note.sop_instance_uid),
        (tg::STUDY_INSTANCE_UID, &mut note.study_instance_uid),
        (tg::SERIES_INSTANCE_UID, &mut note.series_instance_uid),
    ] {
        match kos_string(&o, tag, Vr::UI, true) {
            Ok(v) if valid_uid(&v) => *out = v,
            _ => return Err(kos_invalid("note UID")),
        }
    }
    for (tag, out) in [
        (tg::SERIES_NUMBER, &mut note.series_number),
        (tg::INSTANCE_NUMBER, &mut note.instance_number),
    ] {
        match kos_string(&o, tag, Vr::IS, true) {
            Ok(v) if kos_positive_is(&v) => *out = v,
            _ => return Err(kos_invalid("series or instance number")),
        }
    }
    note.content_date = kos_string(&o, tg::CONTENT_DATE, Vr::DA, true)?;
    match dcmtime::parse_date(&note.content_date) {
        Ok(d) if !d.is_nema && note.content_date.len() == 8 => {}
        _ => return Err(kos_invalid("content date")),
    }
    note.content_time = kos_string(&o, tg::CONTENT_TIME, Vr::TM, true)?;
    if dcmtime::parse_time(&note.content_time).is_err() || note.content_time.len() < 6 {
        return Err(kos_invalid("content time precision"));
    }
    if o.has(tg::REFERENCED_CONTENT_ITEM_IDENTIFIER)
        || o.has(Tag::new(0x0040, 0xA525))
        || o.has(tg::COMPLETION_FLAG)
        || o.has(tg::VERIFICATION_FLAG)
    {
        return Err(kos_invalid("unsupported root extension"));
    }
    for tag in [
        tg::RELATIONSHIP_TYPE,
        tg::TEXT_VALUE,
        tg::CONCEPT_CODE_SEQ,
        tg::REF_SOP_SEQUENCE,
        tg::MEASURED_VALUE_SEQ,
        tg::NUMERIC_VALUE_QUALIFIER_CODE_SEQ,
        tg::DATE_TIME_VALUE,
        tg::DATE_VALUE,
        tg::TIME_VALUE,
        tg::UID,
        tg::PERSON_NAME_VALUE,
        tg::GRAPHIC_DATA,
        tg::GRAPHIC_TYPE,
        tg::FIDUCIAL_UID,
        tg::PIXEL_ORIGIN_INTERPRET,
        tg::REFERENCED_FRAME_OF_REF_UID,
        tg::TEMPORAL_RANGE_TYPE,
        tg::REFERENCED_SAMPLES,
        tg::REFERENCED_TIME_OFFSET,
        tg::REFERENCED_DATE_TIME,
    ] {
        if o.has(tag) {
            return Err(kos_invalid("root value payload"));
        }
    }
    kos_sequence(&o, tg::REFERENCED_PERFORMED_PROCEDURE_STEP, 0, 0)?;
    if o.has(tg::CONTENT_TEMPLATE_SEQUENCE) {
        let ids = kos_sequence(&o, tg::CONTENT_TEMPLATE_SEQUENCE, 1, 1)?;
        let mapping = kos_string(&ids[0], tg::MAPPING_RESOURCE, Vr::CS, true);
        let tid = kos_string(&ids[0], tg::TEMPLATE_IDENTIFIER, Vr::CS, true);
        if mapping.as_deref().ok() != Some("DCMR")
            || tid.as_deref().ok() != Some("2010")
            || ids[0].len() != 2
        {
            return Err(kos_invalid("template identification"));
        }
    }
    let evidence = kos_read_evidence(&o, &note, &l)?;
    let items = kos_sequence(&o, tg::CONTENT_SEQUENCE, 1, l.max_items)?;
    let mut seen: HashMap<String, KosRejectionReference> = HashMap::new();
    let mut identities = HashMap::new();
    if !kos_identity_roles(
        &mut identities,
        &note.study_instance_uid,
        &note.series_instance_uid,
        KEY_OBJECT_SELECTION_DOCUMENT_STORAGE,
        &note.sop_instance_uid,
    ) {
        return Err(kos_invalid("UID role collision"));
    }
    let mut reference_visits = 0i64;
    for it in &items {
        let vt = kos_string(it, tg::VALUE_TYPE, Vr::CS, true);
        let rel = kos_string(it, tg::RELATIONSHIP_TYPE, Vr::CS, true);
        let (Ok(vt), Ok(rel)) = (vt, rel) else {
            return Err(kos_invalid("content structure"));
        };
        if it.has(tg::CONTENT_SEQUENCE) || it.has(tg::REFERENCED_CONTENT_ITEM_IDENTIFIER) {
            return Err(kos_invalid("content structure"));
        }
        match vt.as_str() {
            value::IMAGE | value::WAVEFORM | value::COMPOSITE => {
                reference_visits += 1;
                if reference_visits > l.max_references {
                    return Err(kos_limit("content references"));
                }
                if rel != RELATIONSHIP_CONTAINS || it.len() != 3 {
                    return Err(kos_invalid("reference content"));
                }
                let pair = kos_read_sop_pair(it)?;
                let mut r = match evidence.get(&pair.sop_instance_uid) {
                    Some(r) if r.sop_class_uid == pair.sop_class_uid => r.clone(),
                    _ => return Err(kos_invalid("missing or conflicting evidence")),
                };
                r.value_type = vt.clone();
                if !kos_reference_valid(&r)
                    || r.sop_instance_uid == note.sop_instance_uid
                    || !kos_identity_roles(
                        &mut identities,
                        &r.study_instance_uid,
                        &r.series_instance_uid,
                        &r.sop_class_uid,
                        &r.sop_instance_uid,
                    )
                {
                    return Err(kos_invalid("reference identity or role"));
                }
                if let Some(old) = seen.get(&r.sop_instance_uid) {
                    if *old != r {
                        return Err(kos_invalid("conflicting content reference"));
                    }
                    continue;
                }
                seen.insert(r.sop_instance_uid.clone(), r.clone());
                note.references.push(r);
            }
            value::CODE => {
                if note.quality_reasons.len() as i64 >= l.max_quality_reasons {
                    return Err(kos_limit("quality reasons"));
                }
                if note.kind != KosRejectionKind::QUALITY
                    || rel != RELATIONSHIP_HAS_CONCEPT_MOD
                    || it.len() != 4
                {
                    return Err(kos_invalid("quality modifier"));
                }
                match kos_code(it, tg::CONCEPT_NAME_CODE_SEQ) {
                    Ok(name) if kos_same_code(&name, "113011") => {}
                    _ => return Err(kos_invalid("modifier concept")),
                }
                match kos_code(it, tg::CONCEPT_CODE_SEQ) {
                    Ok(code) if kos_quality_code(&code) => note.quality_reasons.push(code),
                    _ => return Err(kos_invalid("quality code outside vendored CID 7011")),
                }
            }
            value::TEXT => {
                if rel != RELATIONSHIP_CONTAINS || it.len() != 4 || !note.description.is_empty() {
                    return Err(kos_invalid("description content"));
                }
                match kos_code(it, tg::CONCEPT_NAME_CODE_SEQ) {
                    Ok(name) if kos_same_code(&name, "113012") => {}
                    _ => return Err(kos_invalid("description concept")),
                }
                match kos_string(it, tg::TEXT_VALUE, Vr::UT, true) {
                    Ok(d) if kos_text_clean(&d) && !d.trim().is_empty() => note.description = d,
                    _ => return Err(kos_invalid("description text")),
                }
                if note.description.len() as i64 > l.max_description_bytes {
                    return Err(kos_limit("description bytes"));
                }
            }
            _ => return Err(kos_invalid("unsupported content type")),
        }
    }
    if seen.is_empty() || seen.len() != evidence.len() {
        return Err(kos_invalid("evidence/content mismatch"));
    }
    if note.kind == KosRejectionKind::QUALITY && note.quality_reasons.is_empty() {
        return Err(kos_invalid("missing quality reason"));
    }
    let (decoded, err) = read_document_with_options(
        Some(&o),
        ReadOptions {
            max_depth: l.max_depth,
            max_items: l.max_items,
            max_references: l.max_references,
            max_path_components: l.max_depth + 1,
            resolve_references: true,
            reference_mode: ValidationMode::STRICT,
            max_findings: 64,
            validate_relationships: true,
            relationship_mode: ValidationMode::STRICT,
        },
    );
    if let Some(e) = err {
        return Err(e);
    }
    kos_validate_template(decoded.document.as_ref(), &l)?;
    Ok(note)
}
fn kos_read_sop_pair(o: &Object) -> Result<CompositeReference, SrError> {
    let items = kos_sequence(o, tg::REF_SOP_SEQUENCE, 1, 1)?;
    kos_read_sop_item(&items[0])
}
fn kos_read_sop_item(o: &Object) -> Result<CompositeReference, SrError> {
    if o.len() != 2 || !kos_no_selectors(o) {
        return Err(kos_invalid("partial reference or SOP item attributes"));
    }
    let class = kos_string(o, tg::REF_SOP_CLASS_UID, Vr::UI, true);
    let instance = kos_string(o, tg::REF_SOP_INSTANCE_UID, Vr::UI, true);
    match (class, instance) {
        (Ok(c), Ok(i)) if valid_uid(&c) && valid_uid(&i) => Ok(CompositeReference {
            sop_class_uid: c,
            sop_instance_uid: i,
        }),
        _ => Err(kos_invalid("referenced UID")),
    }
}
fn kos_read_evidence(
    o: &Object,
    note: &KosRejectionNote,
    l: &KosRejectionLimits,
) -> Result<HashMap<String, KosRejectionReference>, SrError> {
    let studies = kos_sequence(o, tg::CURRENT_REQUESTED_PROCEDURE_EVIDENCE, 1, 1)?;
    let study = match kos_string(&studies[0], tg::STUDY_INSTANCE_UID, Vr::UI, true) {
        Ok(s) if s == note.study_instance_uid && studies[0].len() == 2 => s,
        _ => return Err(kos_invalid("evidence study")),
    };
    let series = kos_bound_sequence(
        &studies[0],
        tg::REFERENCED_SERIES_SEQUENCE,
        1,
        l.max_references,
    )?;
    let mut out: HashMap<String, KosRejectionReference> = HashMap::new();
    let mut visits = 0i64;
    for it in &series {
        let series_uid = match kos_string(it, tg::SERIES_INSTANCE_UID, Vr::UI, true) {
            Ok(s) if valid_uid(&s) && s != note.series_instance_uid && it.len() == 2 => s,
            _ => return Err(kos_invalid("evidence series")),
        };
        let sops = kos_bound_sequence(it, tg::REF_SOP_SEQUENCE, 1, l.max_references - visits)?;
        visits += sops.len() as i64;
        for sop in &sops {
            let p = kos_read_sop_item(sop)?;
            let r = KosRejectionReference {
                value_type: String::new(),
                study_instance_uid: study.clone(),
                series_instance_uid: series_uid.clone(),
                sop_class_uid: p.sop_class_uid,
                sop_instance_uid: p.sop_instance_uid,
            };
            if let Some(old) = out.get(&r.sop_instance_uid)
                && *old != r
            {
                return Err(kos_invalid("conflicting evidence"));
            }
            out.insert(r.sop_instance_uid.clone(), r);
        }
    }
    Ok(out)
}
