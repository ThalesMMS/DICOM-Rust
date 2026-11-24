use super::measurement_context_evidence::admit_measurement_evidence;
use super::measurement_report::MeasurementReport;
use super::numeric_representation::{NumericRepresentation, resident_string};
use super::read::{ReadOptions, default_read_options, normalize_read_options};
use super::reference::{ValidationMode, ValidationReport, append_path, identifier_string};
use super::relationship::{RelationshipOptions, validate_relationships};
use super::template_catalog::{new_standard_template_registry, standard_template_key};
use super::template_metadata::{
    CodeContext, EquivalentCodeEntry, element_index, replace_or_append_element,
};
use super::template_registry::{
    TemplateRegistry, TemplateValidationOptions, default_template_validation_options,
    new_template_validator,
};
use super::template_snapshot::{
    METADATA_SNAPSHOT_STALE, MetadataSnapshotOptions, TemplateMetadataSnapshot,
    default_metadata_snapshot_options, metadata_snapshot_error, validate_with_metadata_snapshot,
};
use super::{
    CodedEntry, ContentItem, Document, Element, ImageReference, Kind, SrError, derivedio,
    hierarchical_reference_sequence, seq_element, str_elem, tags as tg, value,
};
use crate::charset::SpecificCharacterSet;
use crate::object::inherit::inherit_modules;
use crate::object::{ElementHeader, File, Object, Value, Vr};
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};
pub const MEASUREMENT_CONTEXT_SHAPE: &str = "context-shape";
pub const MEASUREMENT_CONTEXT_DUPLICATE: &str = "duplicate-singleton";
pub const MEASUREMENT_CONTEXT_CONFLICT: &str = "context-conflict";
pub const MEASUREMENT_CONTEXT_EVIDENCE: &str = "evidence-admission";
pub const MEASUREMENT_CONTEXT_LIMIT: &str = "context-resource-limit";
pub const MEASUREMENT_CONTEXT_UNSUPPORTED: &str = "unmodeled-content";
pub const MEASUREMENT_CONTEXT_RWVM_EXTENSION: &str = "rwvm-inferred-from-extension";
pub const MEASUREMENT_CONTEXT_OPAQUE_VALUES: &str = "opaque-value-elements";
pub fn measurement_context_error(code: &str, path: &str) -> SrError {
    SrError::coded_path(
        Kind::MeasurementContext,
        code,
        path,
        MEASUREMENT_CONTEXT_LIMIT,
    )
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MeasurementReferenceMetadataEntry {
    pub path: String,
    pub sop_class_uid: String,
    pub sop_instance_uid: String,
    pub segments: Vec<u16>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MeasurementReferenceMetadata {
    pub entries: Vec<MeasurementReferenceMetadataEntry>,
}
pub(crate) fn measurement_reference_metadata_count(
    m: &MeasurementReferenceMetadata,
    max: i64,
) -> bool {
    if max < 0 || m.entries.len() as i64 > max {
        return false;
    }
    let mut count = m.entries.len() as i64;
    for e in &m.entries {
        if e.segments.len() as i64 > max - count {
            return false;
        }
        count += e.segments.len() as i64;
    }
    true
}
fn valid_uid(uid: &str) -> bool {
    crate::object::is_valid_uid(uid.as_bytes())
}
pub(crate) fn index_measurement_reference_metadata(
    content: &[ContentItem],
    m: &MeasurementReferenceMetadata,
    max_items: i64,
    max_depth: i64,
) -> Result<HashMap<String, MeasurementReferenceMetadataEntry>, SrError> {
    let mut index = HashMap::new();
    for e in &m.entries {
        if !super::image_library::valid_numeric_path(&e.path)
            || !valid_uid(&e.sop_class_uid)
            || !valid_uid(&e.sop_instance_uid)
            || e.segments.is_empty()
        {
            return Err(measurement_context_error(
                MEASUREMENT_CONTEXT_SHAPE,
                &e.path,
            ));
        }
        if index.contains_key(&e.path) {
            return Err(measurement_context_error(
                MEASUREMENT_CONTEXT_DUPLICATE,
                &e.path,
            ));
        }
        if e.segments.contains(&0) {
            return Err(measurement_context_error(
                MEASUREMENT_CONTEXT_SHAPE,
                &e.path,
            ));
        }
        index.insert(e.path.clone(), e.clone());
    }
    if m.entries.is_empty() {
        return Ok(index);
    }
    let mut found = 0usize;
    let mut count = 0i64;
    let mut stack: Vec<(&[ContentItem], Vec<u32>, i64, usize)> = Vec::new();
    if !content.is_empty() {
        stack.push((content, vec![1], 1, 0));
    }
    while let Some(frame) = stack.last_mut() {
        if frame.3 >= frame.0.len() {
            stack.pop();
            continue;
        }
        let i = frame.3;
        frame.3 += 1;
        let item = &frame.0[i];
        let path = append_path(&frame.1, i as u32 + 1);
        let depth = frame.2;
        let key = identifier_string(&path);
        count += 1;
        if count > max_items || depth > max_depth {
            return Err(measurement_context_error(MEASUREMENT_CONTEXT_LIMIT, &key));
        }
        if let Some(r) = index.get(&key) {
            if item.value_type != value::IMAGE
                || !item.referenced_content_item_identifier.is_empty()
                || item.image.sop_class_uid != r.sop_class_uid
                || item.image.sop_instance_uid != r.sop_instance_uid
            {
                return Err(measurement_context_error(
                    MEASUREMENT_CONTEXT_CONFLICT,
                    &key,
                ));
            }
            found += 1;
        }
        if !item.children.is_empty() {
            stack.push((&item.children, path, depth + 1, 0));
        }
    }
    if found != m.entries.len() {
        return Err(measurement_context_error(MEASUREMENT_CONTEXT_CONFLICT, ""));
    }
    Ok(index)
}
pub(crate) fn measurement_resident_scalar(e: &Element, vr: Vr) -> Result<String, SrError> {
    let s = resident_string(e, vr)?;
    if vr.uses_text_value_delimiter() && s.contains('\\') {
        return Err(measurement_context_error(MEASUREMENT_CONTEXT_SHAPE, ""));
    }
    Ok(s)
}
pub(crate) fn read_measurement_reference_metadata(
    obj: &Object,
    read_max_items: i64,
    read_max_depth: i64,
    snapshot_max_items: i64,
) -> Result<MeasurementReferenceMetadata, SrError> {
    struct State {
        m: MeasurementReferenceMetadata,
        count: i64,
        segments: i64,
        read_max_items: i64,
        read_max_depth: i64,
        snapshot_max_items: i64,
    }
    fn visit(st: &mut State, parent: &Object, path: &[u32], depth: i64) -> Result<(), SrError> {
        let pkey = identifier_string(path);
        let Some(element) = parent.get(tg::CONTENT_SEQUENCE) else {
            return Ok(());
        };
        let Value::Sequence(sq) = &element.value else {
            return Err(measurement_context_error(MEASUREMENT_CONTEXT_SHAPE, &pkey));
        };
        if element.vr() != Vr::SQ {
            return Err(measurement_context_error(MEASUREMENT_CONTEXT_SHAPE, &pkey));
        }
        if sq.len() as i64 > st.read_max_items - st.count
            || (!sq.is_empty() && depth > st.read_max_depth)
        {
            return Err(measurement_context_error(MEASUREMENT_CONTEXT_LIMIT, &pkey));
        }
        st.count += sq.len() as i64;
        let items = parent
            .get_sequence(tg::CONTENT_SEQUENCE)
            .unwrap_or_default();
        for (idx, it) in items.iter().enumerate() {
            let ip = append_path(path, idx as u32 + 1);
            let key = identifier_string(&ip);
            if let Some(vt) = it.get(tg::VALUE_TYPE) {
                let v = resident_string(vt, Vr::CS)
                    .map_err(|_| measurement_context_error(MEASUREMENT_CONTEXT_SHAPE, &key))?;
                let vt = v.trim_matches(' ');
                if vt == value::IMAGE || vt == value::COMPOSITE || vt == value::WAVEFORM {
                    read_reference(st, it, vt, &key)?;
                }
            }
            visit(st, it, &ip, depth + 1)?;
        }
        Ok(())
    }
    fn read_reference(st: &mut State, it: &Object, vt: &str, key: &str) -> Result<(), SrError> {
        let shape = || measurement_context_error(MEASUREMENT_CONTEXT_SHAPE, key);
        match it.get(tg::REF_SOP_SEQUENCE) {
            Some(Element {
                value: Value::Sequence(sq),
                header,
            }) if header.vr == Vr::SQ && sq.len() == 1 => {}
            _ => return Err(shape()),
        }
        let refs = it.get_sequence(tg::REF_SOP_SEQUENCE).unwrap_or_default();
        let r = &refs[0];
        for field in r.elements() {
            let tag = field.tag();
            if tag == tg::REF_SOP_CLASS_UID || tag == tg::REF_SOP_INSTANCE_UID {
            } else if tag == tg::REF_FRAME_NUMBER || tag == tg::REFERENCED_SEGMENT_NUMBER {
                if vt != value::IMAGE {
                    return Err(shape());
                }
            } else if tag == tg::REFERENCED_WAVEFORM_CH {
                if vt != value::WAVEFORM {
                    return Err(shape());
                }
            } else {
                return Err(measurement_context_error(
                    MEASUREMENT_CONTEXT_UNSUPPORTED,
                    key,
                ));
            }
        }
        let uid = |tag| -> Result<String, SrError> {
            let e = r.get(tag).ok_or_else(shape)?;
            let v = measurement_resident_scalar(e, Vr::UI).map_err(|_| shape())?;
            let n = crate::object::text::normalize_uid(&v).to_string();
            if !valid_uid(&n) {
                return Err(shape());
            }
            Ok(n)
        };
        let c = uid(tg::REF_SOP_CLASS_UID)?;
        let u = uid(tg::REF_SOP_INSTANCE_UID)?;
        if let Some(seg) = r.get(tg::REFERENCED_SEGMENT_NUMBER) {
            if vt != value::IMAGE || seg.vr() != Vr::US {
                return Err(shape());
            }
            let Value::Raw(v) = &seg.value else {
                return Err(shape());
            };
            if v.len() % 2 != 0 {
                return Err(shape());
            }
            if (v.len() / 2) as i64 > st.snapshot_max_items - st.segments {
                return Err(measurement_context_error(MEASUREMENT_CONTEXT_LIMIT, key));
            }
            let big = r.value_big_endian();
            let values: Vec<u16> = v
                .as_chunks::<2>()
                .0
                .iter()
                .map(|b| {
                    if big {
                        u16::from_be_bytes([b[0], b[1]])
                    } else {
                        u16::from_le_bytes([b[0], b[1]])
                    }
                })
                .collect();
            if values.is_empty() || values.contains(&0) {
                return Err(shape());
            }
            st.segments += values.len() as i64;
            if st.m.entries.len() as i64 >= st.snapshot_max_items - st.segments {
                return Err(measurement_context_error(MEASUREMENT_CONTEXT_LIMIT, key));
            }
            st.m.entries.push(MeasurementReferenceMetadataEntry {
                path: key.to_string(),
                sop_class_uid: c,
                sop_instance_uid: u,
                segments: values,
            });
        }
        Ok(())
    }
    let mut st = State {
        m: MeasurementReferenceMetadata::default(),
        count: 0,
        segments: 0,
        read_max_items,
        read_max_depth,
        snapshot_max_items,
    };
    let r = visit(&mut st, obj, &[1], 1);
    r.map(|_| st.m)
}
pub(crate) fn restore_measurement_reference_metadata(
    elements: &mut [Element],
    items: &[ContentItem],
    parent: &[u32],
    refs: &HashMap<String, MeasurementReferenceMetadataEntry>,
) -> Result<(), SrError> {
    if refs.is_empty() || items.is_empty() {
        return Ok(());
    }
    let pkey = identifier_string(parent);
    let Some(index) = element_index(elements, tg::CONTENT_SEQUENCE) else {
        return Err(measurement_context_error(MEASUREMENT_CONTEXT_SHAPE, &pkey));
    };
    let Value::Sequence(sq) = &mut elements[index].value else {
        return Err(measurement_context_error(MEASUREMENT_CONTEXT_SHAPE, &pkey));
    };
    if sq.len() != items.len() {
        return Err(measurement_context_error(MEASUREMENT_CONTEXT_SHAPE, &pkey));
    }
    for (i, item) in items.iter().enumerate() {
        let path = append_path(parent, i as u32 + 1);
        let key = identifier_string(&path);
        let data = &mut sq[i];
        if let Some(r) = refs.get(&key) {
            let Some(ri) = element_index(&data.elements, tg::REF_SOP_SEQUENCE) else {
                return Err(measurement_context_error(MEASUREMENT_CONTEXT_SHAPE, &key));
            };
            let Value::Sequence(v) = &data.elements[ri].value else {
                return Err(measurement_context_error(MEASUREMENT_CONTEXT_SHAPE, &key));
            };
            if v.len() != 1 {
                return Err(measurement_context_error(MEASUREMENT_CONTEXT_SHAPE, &key));
            }
            let mut v = v.clone();
            let raw: Vec<u8> = r.segments.iter().flat_map(|s| s.to_le_bytes()).collect();
            replace_or_append_element(
                &mut v[0].elements,
                Element {
                    header: ElementHeader {
                        tag: tg::REFERENCED_SEGMENT_NUMBER,
                        vr: Vr::US,
                        length: 0,
                        length_set: false,
                    },
                    value: Value::Raw(raw),
                },
            );
            data.elements[ri] = seq_element(tg::REF_SOP_SEQUENCE, v);
        }
        restore_measurement_reference_metadata(&mut data.elements, &item.children, &path, refs)?;
    }
    Ok(())
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MeasurementContextCode {
    pub code: CodedEntry,
    pub context: CodeContext,
    pub equivalent_codes: Vec<EquivalentCodeEntry>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MeasurementFindingSite {
    pub site: MeasurementContextCode,
    pub laterality: Option<MeasurementContextCode>,
    pub topographical_modifier: Option<MeasurementContextCode>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MeasurementAlgorithm {
    pub name: String,
    pub name_code: Option<MeasurementContextCode>,
    pub version: String,
    pub manufacturer: String,
    pub parameters: Vec<String>,
    pub family: Option<MeasurementContextCode>,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MeasurementTimePoint {
    pub subject_identifier: String,
    pub protocol_identifier: String,
    pub time_point: String,
    pub types: Vec<MeasurementContextCode>,
    pub order: Option<NumericRepresentation>,
    pub temporal_offset: Option<NumericRepresentation>,
    pub event_type: Option<MeasurementContextCode>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MeasurementQualitativeModifier {
    pub concept_name: MeasurementContextCode,
    pub value: MeasurementContextCode,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MeasurementQualitativeEvaluation {
    pub concept_name: MeasurementContextCode,
    pub value_type: String,
    pub code: MeasurementContextCode,
    pub text: String,
    pub modifiers: Vec<MeasurementQualitativeModifier>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct MeasurementReferenceEvidence {
    pub study_instance_uid: String,
    pub series_instance_uid: String,
    pub sop_class_uid: String,
    pub sop_instance_uid: String,
}
pub const MEASUREMENT_RWVM_STORAGE: &str = "1.2.840.10008.5.1.4.1.1.67";
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MeasurementNumericContext {
    pub path: String,
    pub concept_name: MeasurementContextCode,
    pub units: MeasurementContextCode,
    pub numeric: Option<NumericRepresentation>,
    pub method: Option<MeasurementContextCode>,
    pub finding_sites: Vec<MeasurementFindingSite>,
    pub algorithm: Option<MeasurementAlgorithm>,
    pub real_world_value_map: Option<MeasurementReferenceEvidence>,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MeasurementGroupContext {
    pub path: String,
    pub activity_session: String,
    pub time_point: Option<MeasurementTimePoint>,
    pub method: Option<MeasurementContextCode>,
    pub finding_sites: Vec<MeasurementFindingSite>,
    pub algorithm: Option<MeasurementAlgorithm>,
    pub real_world_value_map: Option<MeasurementReferenceEvidence>,
    pub qualitative: Vec<MeasurementQualitativeEvaluation>,
    pub measurements: Vec<MeasurementNumericContext>,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MeasurementReportContext {
    pub imaging_algorithm: Option<MeasurementAlgorithm>,
    pub qualitative_algorithm: Option<MeasurementAlgorithm>,
    pub qualitative: Vec<MeasurementQualitativeEvaluation>,
    pub groups: Vec<MeasurementGroupContext>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MeasurementContextFinding {
    pub code: String,
    pub path: String,
}
#[derive(Clone, Debug, Default)]
pub struct MeasurementContextValidation {
    pub unsupported: Vec<MeasurementContextFinding>,
    pub catalog: ValidationReport,
    pub relationships: ValidationReport,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MeasurementContextOptions {
    pub read: ReadOptions,
    pub snapshot: MetadataSnapshotOptions,
    pub catalog: TemplateValidationOptions,
    pub strict_catalog: bool,
    pub character_set: Option<Vec<String>>,
    pub value_elements_big_endian: bool,
    pub max_evidence: i64,
    pub max_evidence_items: i64,
    pub max_string_bytes: i64,
    pub max_context_findings: i64,
}
pub fn default_measurement_context_options() -> MeasurementContextOptions {
    let mut r = default_read_options();
    r.resolve_references = true;
    r.validate_relationships = true;
    let mut c = default_template_validation_options();
    c.mode = ValidationMode::WARN;
    MeasurementContextOptions {
        read: r,
        snapshot: default_metadata_snapshot_options(),
        catalog: c,
        max_evidence: 100_000,
        max_evidence_items: 300_000,
        max_string_bytes: 1 << 20,
        max_context_findings: 1024,
        ..Default::default()
    }
}
fn normalize_catalog_options(mut o: TemplateValidationOptions) -> TemplateValidationOptions {
    let d = default_template_validation_options();
    if o.mode.0 == 0 {
        o.mode = d.mode;
    }
    if o.max_depth == 0 {
        o.max_depth = d.max_depth;
    }
    if o.max_items == 0 {
        o.max_items = d.max_items;
    }
    if o.max_steps == 0 {
        o.max_steps = d.max_steps;
    }
    if o.max_findings == 0 {
        o.max_findings = d.max_findings;
    }
    o
}
pub(crate) fn normalize_measurement_context_options(
    mut o: MeasurementContextOptions,
) -> Result<MeasurementContextOptions, SrError> {
    let d = default_measurement_context_options();
    o.read = normalize_read_options(o.read);
    if o.snapshot.max_depth == 0 {
        o.snapshot.max_depth = d.snapshot.max_depth;
    }
    if o.snapshot.max_items == 0 {
        o.snapshot.max_items = d.snapshot.max_items;
    }
    if o.snapshot.max_value_bytes == 0 {
        o.snapshot.max_value_bytes = d.snapshot.max_value_bytes;
    }
    o.catalog = normalize_catalog_options(o.catalog);
    o.catalog.mode = ValidationMode::WARN;
    if o.strict_catalog {
        o.catalog.mode = ValidationMode::STRICT;
    }
    if o.max_evidence == 0 {
        o.max_evidence = d.max_evidence;
    }
    if o.max_evidence_items == 0 {
        o.max_evidence_items = d.max_evidence_items;
    }
    if o.max_string_bytes == 0 {
        o.max_string_bytes = d.max_string_bytes;
    }
    if o.max_context_findings == 0 {
        o.max_context_findings = d.max_context_findings;
    }
    if o.read.max_depth < 1
        || o.read.max_depth > 64
        || o.read.max_items < 1
        || o.read.max_references < 1
        || o.read.max_path_components < 1
        || o.read.max_findings < 1
        || o.snapshot.max_depth < 1
        || o.snapshot.max_depth > 64
        || o.snapshot.max_items < 1
        || o.snapshot.max_value_bytes < 1
        || o.max_evidence < 1
        || o.max_evidence_items < 1
        || o.max_string_bytes < 1
        || o.max_context_findings < 1
    {
        return Err(measurement_context_error(MEASUREMENT_CONTEXT_LIMIT, ""));
    }
    Ok(o)
}
#[derive(Clone, Debug)]
pub struct DetailedMeasurementReport {
    pub(crate) document: Document,
    pub(crate) snapshot: TemplateMetadataSnapshot,
    pub(crate) evidence: Vec<MeasurementReferenceEvidence>,
    pub(crate) evidence_index: HashMap<String, MeasurementReferenceEvidence>,
    pub(crate) charset: Vec<String>,
    pub(crate) frame_of_reference_uid: String,
    pub(crate) options: MeasurementContextOptions,
    pub(crate) validation: MeasurementContextValidation,
}
impl DetailedMeasurementReport {
    pub fn document(&self) -> &Document {
        &self.document
    }
    #[doc(hidden)]
    pub fn document_mut(&mut self) -> &mut Document {
        &mut self.document
    }
    pub fn metadata_snapshot(&self) -> &TemplateMetadataSnapshot {
        &self.snapshot
    }
    fn check(&self) -> Result<(), SrError> {
        self.snapshot.check(Some(&self.document))
    }
    pub fn report(&self) -> Result<MeasurementReport, SrError> {
        self.check()?;
        self.project().map(|r| r.0)
    }
    pub fn context(&self) -> Result<MeasurementReportContext, SrError> {
        self.check()?;
        self.project().map(|r| r.1)
    }
    pub fn evidence(&self) -> Result<Vec<MeasurementReferenceEvidence>, SrError> {
        self.check()?;
        Ok(self.evidence.clone())
    }
    pub fn validation(&self) -> Result<MeasurementContextValidation, SrError> {
        self.check()?;
        Ok(self.validation.clone())
    }
    pub fn dataset(&self) -> Result<Object, SrError> {
        self.check()?;
        self.opaque_output_admission()?;
        let mut obj = self
            .document
            .dataset_with_metadata_snapshot(Some(&self.snapshot))?;
        if !self.evidence.is_empty() {
            let refs: Vec<ImageReference> = self
                .evidence
                .iter()
                .map(|e| ImageReference {
                    study_instance_uid: e.study_instance_uid.clone(),
                    series_instance_uid: e.series_instance_uid.clone(),
                    sop_class_uid: e.sop_class_uid.clone(),
                    sop_instance_uid: e.sop_instance_uid.clone(),
                    frames: Vec::new(),
                })
                .collect();
            obj.put(hierarchical_reference_sequence(
                tg::CURRENT_REQUESTED_PROCEDURE_EVIDENCE,
                &refs,
            ));
        }
        if !self.charset.is_empty() {
            obj.put(Element {
                header: ElementHeader {
                    tag: crate::object::Tag::new(0x0008, 0x0005),
                    vr: Vr::CS,
                    length: 0,
                    length_set: false,
                },
                value: Value::Strings(self.charset.clone()),
            });
        }
        if !self.frame_of_reference_uid.is_empty() {
            obj.put(str_elem(
                derivedio::TAG_FRAME_OF_REFERENCE_UID,
                Vr::UI,
                &self.frame_of_reference_uid,
            ));
        }
        self.document
            .common
            .apply(&mut obj, inherit_modules::WITHOUT_FRAME_OF_REFERENCE)
            .map_err(|e| SrError::msg(e.to_string()))?;
        Ok(obj)
    }
    pub fn file(&self) -> Result<File, SrError> {
        let obj = self.dataset()?;
        derivedio::file(
            &self.document.sop_class_uid,
            &self.document.sop_instance_uid,
            Some(obj),
        )
        .map_err(|e| {
            let mut err = SrError::msg(e.to_string());
            if e.is(crate::object::Kind::MissingSopClassUid) {
                err.kinds.push(Kind::MissingSopClassUid);
            }
            if e.is(crate::object::Kind::MissingSopInstanceUid) {
                err.kinds.push(Kind::MissingSopInstanceUid);
            }
            err
        })
    }
}
fn standard_registry() -> Result<Arc<TemplateRegistry>, SrError> {
    static REGISTRY: OnceLock<Result<Arc<TemplateRegistry>, SrError>> = OnceLock::new();
    REGISTRY
        .get_or_init(|| new_standard_template_registry(&[1500]).map(Arc::new))
        .clone()
}
pub fn bind_measurement_report_with_context(
    doc: Option<&Document>,
    snapshot: Option<&TemplateMetadataSnapshot>,
    evidence: &[MeasurementReferenceEvidence],
    options: MeasurementContextOptions,
) -> Result<DetailedMeasurementReport, SrError> {
    let o = normalize_measurement_context_options(options.clone())?;
    let (Some(snapshot), Some(doc)) = (snapshot, doc) else {
        return Err(metadata_snapshot_error(METADATA_SNAPSHOT_STALE));
    };
    snapshot.check(Some(doc))?;
    let admitted = admit_measurement_evidence(evidence, &o)?;
    let charset = options
        .character_set
        .unwrap_or_else(|| vec!["ISO_IR 192".to_string()]);
    if charset.len() > 64 {
        return Err(measurement_context_error(MEASUREMENT_CONTEXT_LIMIT, ""));
    }
    if SpecificCharacterSet::parse(&charset).is_err() {
        return Err(measurement_context_error(MEASUREMENT_CONTEXT_SHAPE, ""));
    }
    let mut evidence_index = HashMap::with_capacity(admitted.len());
    for e in &admitted {
        evidence_index.insert(e.sop_instance_uid.clone(), e.clone());
    }
    let mut r = DetailedMeasurementReport {
        document: doc.clone(),
        snapshot: snapshot.clone(),
        evidence: admitted,
        evidence_index,
        charset,
        frame_of_reference_uid: String::new(),
        options: o.clone(),
        validation: MeasurementContextValidation::default(),
    };
    r.validation.unsupported = r.project()?.2;
    r.check_evidence()?;
    let registry = standard_registry()?;
    let validator = new_template_validator(Some(registry), o.catalog)?;
    let key = standard_template_key(1500).unwrap_or_default();
    let (catalog, err) =
        validate_with_metadata_snapshot(&validator, Some(&r.document), &key, Some(&r.snapshot));
    if let Some(e) = err {
        return Err(e);
    }
    r.validation.catalog = catalog;
    let (relationships, err) = validate_relationships(
        Some(&r.document),
        RelationshipOptions {
            mode: ValidationMode::STRICT,
            max_depth: o.read.max_depth,
            max_items: o.read.max_items,
            max_findings: o.read.max_findings,
        },
    );
    if let Some(e) = err {
        return Err(e);
    }
    r.validation.relationships = relationships;
    Ok(r)
}
