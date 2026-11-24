#![forbid(unsafe_code)]
pub mod code_value;
pub mod error;
pub mod image_library;
pub mod image_library_extract;
pub mod image_library_structured;
pub mod kos;
pub mod kos_rejection;
pub mod lifecycle;
pub mod measurement_context;
pub mod measurement_context_build;
pub mod measurement_context_evidence;
pub mod measurement_context_read;
pub mod measurement_export;
pub mod measurement_report;
pub mod numeric_representation;
pub mod read;
pub mod reference;
pub mod relationship;
pub mod render;
#[rustfmt::skip]
mod relationship_tables;
pub mod template_catalog;
#[rustfmt::skip]
mod template_catalog_tables;
pub mod template_metadata;
pub mod template_registry;
pub mod template_snapshot;
use crate::codes::dcm;
use crate::derivedio;
use crate::object::inherit::inherit_modules;
use crate::object::{DataSet, Element, ElementHeader, Object, Tag, UNDEFINED_LENGTH, Value, Vr};
pub use code_value::{from_code, new_code, valid_code};
pub use error::{Kind, ReferenceErrorInfo, SrError};
pub use lifecycle::{DocumentReference, VerifyingObserver};
pub use read::{
    ReadOptions, ReadResult, default_read_options, read_document, read_document_with_options,
};
pub use reference::{
    ContentItemIdentifier, DiagnosticFinding, DiagnosticSeverity, ReferenceEdge, ReferenceIndex,
    ReferenceOptions, ValidationMode, ValidationReport, default_reference_options,
    identifier_string, resolve_references,
};
pub use relationship::{
    RelationshipConstraint, RelationshipOptions, default_relationship_options,
    relationship_allowed, relationship_constraint_for, validate_relationships,
};
pub use relationship_tables::RELATIONSHIP_CONSTRAINTS_EDITION;
pub use template_metadata::{
    CodeContext, CodeKey, ContentItemCodeContexts, EquivalentCodeEntry, TemplateIdentification,
    TemplateValidationMetadata,
};
pub const KEY_OBJECT_SELECTION_DOCUMENT_STORAGE: &str = "1.2.840.10008.5.1.4.1.1.88.59";
pub const BASIC_TEXT_SR_STORAGE: &str = "1.2.840.10008.5.1.4.1.1.88.11";
pub const EXTENSIBLE_SR_STORAGE: &str = "1.2.840.10008.5.1.4.1.1.88.35";
const COMPLETION_PARTIAL: &str = "PARTIAL";
const COMPLETION_COMPLETE: &str = "COMPLETE";
const VERIFICATION_UNVERIFIED: &str = "UNVERIFIED";
const VERIFICATION_VERIFIED: &str = "VERIFIED";
pub mod value {
    pub const CONTAINER: &str = "CONTAINER";
    pub const TEXT: &str = "TEXT";
    pub const CODE: &str = "CODE";
    pub const IMAGE: &str = "IMAGE";
    #[doc(hidden)]
    pub const NUM: &str = "NUM";
    pub const DATETIME: &str = "DATETIME";
    pub const DATE: &str = "DATE";
    pub const TIME: &str = "TIME";
    pub const UIDREF: &str = "UIDREF";
    pub const PNAME: &str = "PNAME";
    pub const COMPOSITE: &str = "COMPOSITE";
    pub const WAVEFORM: &str = "WAVEFORM";
    pub const SCOORD: &str = "SCOORD";
    pub const SCOORD3D: &str = "SCOORD3D";
    pub const TCOORD: &str = "TCOORD";
    pub const TABLE: &str = "TABLE";
}
pub const RELATIONSHIP_CONTAINS: &str = "CONTAINS";
pub const RELATIONSHIP_HAS_OBS_CONTEXT: &str = "HAS OBS CONTEXT";
pub const RELATIONSHIP_HAS_ACQ_CONTEXT: &str = "HAS ACQ CONTEXT";
pub const RELATIONSHIP_HAS_CONCEPT_MOD: &str = "HAS CONCEPT MOD";
pub const RELATIONSHIP_HAS_PROPERTIES: &str = "HAS PROPERTIES";
pub const RELATIONSHIP_INFERRED_FROM: &str = "INFERRED FROM";
pub const RELATIONSHIP_SELECTED_FROM: &str = "SELECTED FROM";
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct CodedEntry {
    pub code_value: String,
    pub coding_scheme_designator: String,
    pub code_meaning: String,
}
impl CodedEntry {
    pub fn new(value: &str, scheme: &str, meaning: &str) -> CodedEntry {
        CodedEntry {
            code_value: value.into(),
            coding_scheme_designator: scheme.into(),
            code_meaning: meaning.into(),
        }
    }
    pub fn empty(&self) -> bool {
        self.code_value.is_empty()
            && self.coding_scheme_designator.is_empty()
            && self.code_meaning.is_empty()
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ImageReference {
    pub study_instance_uid: String,
    pub series_instance_uid: String,
    pub sop_class_uid: String,
    pub sop_instance_uid: String,
    pub frames: Vec<i64>,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Measurement {
    pub value: f64,
    pub units: CodedEntry,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CompositeReference {
    pub sop_class_uid: String,
    pub sop_instance_uid: String,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WaveformChannel {
    pub multiplex_group_number: u16,
    pub channel_number: u16,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WaveformReference {
    pub composite: CompositeReference,
    pub channels: Vec<WaveformChannel>,
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point2D {
    pub x: f64,
    pub y: f64,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SpatialCoordinates {
    pub graphic_type: String,
    pub coordinates: Vec<Point2D>,
    pub pixel_origin_interpretation: String,
    pub fiducial_uid: String,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TemporalCoordinates {
    pub range_type: String,
    pub sample_positions: Vec<u32>,
    pub time_offsets: Vec<f64>,
    pub date_times: Vec<String>,
}
pub use measurement_report::{Point3D, SpatialReference};
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ContentItemEncoding {
    pub by_value_macro_present: bool,
    pub empty_reference_macro_present: bool,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ContentItem {
    pub value_type: String,
    pub relationship_type: String,
    pub concept_name: CodedEntry,
    pub text: String,
    pub code: CodedEntry,
    pub image: ImageReference,
    pub measurement: Option<Measurement>,
    pub numeric_value_qualifier: CodedEntry,
    pub date_time: String,
    pub date: String,
    pub time: String,
    pub uid: String,
    pub person_name: String,
    pub composite: CompositeReference,
    pub waveform: WaveformReference,
    pub spatial: SpatialCoordinates,
    pub spatial3d: SpatialReference,
    pub temporal: TemporalCoordinates,
    pub value_elements: Vec<Element>,
    pub continuity_of_content: String,
    pub referenced_content_item_identifier: ContentItemIdentifier,
    pub encoding: ContentItemEncoding,
    pub children: Vec<ContentItem>,
}
pub use crate::object::inherit::DerivedCommon;
#[derive(Clone, Debug, Default)]
pub struct Document {
    pub sop_class_uid: String,
    pub sop_instance_uid: String,
    pub study_instance_uid: String,
    pub series_instance_uid: String,
    pub modality: String,
    pub title: CodedEntry,
    pub content_date: String,
    pub content_time: String,
    pub series_number: String,
    pub instance_number: String,
    pub completion_flag: String,
    pub verification_flag: String,
    pub completion_flag_description: String,
    pub verifying_observers: Vec<VerifyingObserver>,
    pub predecessor_documents: Vec<DocumentReference>,
    pub continuity_of_content: String,
    pub content: Vec<ContentItem>,
    pub common: DerivedCommon,
}
const fn t(group: u16, element: u16) -> Tag {
    Tag::new(group, element)
}
pub(crate) mod tags {
    use super::t;
    use crate::object::Tag;
    pub const SOP_CLASS_UID: Tag = t(0x0008, 0x0016);
    pub const SOP_INSTANCE_UID: Tag = t(0x0008, 0x0018);
    pub const MODALITY: Tag = t(0x0008, 0x0060);
    pub const CONTENT_DATE: Tag = t(0x0008, 0x0023);
    pub const CONTENT_TIME: Tag = t(0x0008, 0x0033);
    pub const STUDY_INSTANCE_UID: Tag = t(0x0020, 0x000D);
    pub const SERIES_INSTANCE_UID: Tag = t(0x0020, 0x000E);
    pub const SERIES_NUMBER: Tag = t(0x0020, 0x0011);
    pub const INSTANCE_NUMBER: Tag = t(0x0020, 0x0013);
    pub(crate) const REFERENCED_SERIES_SEQUENCE: Tag = t(0x0008, 0x1115);
    pub const REF_SOP_SEQUENCE: Tag = t(0x0008, 0x1199);
    pub const REF_SOP_CLASS_UID: Tag = t(0x0008, 0x1150);
    pub const REF_SOP_INSTANCE_UID: Tag = t(0x0008, 0x1155);
    pub const REF_FRAME_NUMBER: Tag = t(0x0008, 0x1160);
    pub const CODE_VALUE: Tag = t(0x0008, 0x0100);
    pub const CODING_SCHEME: Tag = t(0x0008, 0x0102);
    pub const CODING_SCHEME_VERSION: Tag = t(0x0008, 0x0103);
    pub const CODE_MEANING: Tag = t(0x0008, 0x0104);
    pub const MAPPING_RESOURCE: Tag = t(0x0008, 0x0105);
    pub const CONTEXT_GROUP_VERSION: Tag = t(0x0008, 0x0106);
    pub const CONTEXT_GROUP_LOCAL_VERSION: Tag = t(0x0008, 0x0107);
    pub const CONTEXT_GROUP_EXTENSION_FLAG: Tag = t(0x0008, 0x010B);
    pub const CONTEXT_GROUP_EXTENSION_CREATOR_UID: Tag = t(0x0008, 0x010D);
    pub const CONTEXT_IDENTIFIER: Tag = t(0x0008, 0x010F);
    pub const CONTEXT_UID: Tag = t(0x0008, 0x0117);
    pub const MAPPING_RESOURCE_UID: Tag = t(0x0008, 0x0118);
    pub const LONG_CODE_VALUE: Tag = t(0x0008, 0x0119);
    pub const URN_CODE_VALUE: Tag = t(0x0008, 0x0120);
    pub const EQUIVALENT_CODE_SEQUENCE: Tag = t(0x0008, 0x0121);
    pub const MAPPING_RESOURCE_NAME: Tag = t(0x0008, 0x0122);
    pub const MEASUREMENT_UNITS_CODE: Tag = t(0x0040, 0x08EA);
    pub const RELATIONSHIP_TYPE: Tag = t(0x0040, 0xA010);
    pub const VALUE_TYPE: Tag = t(0x0040, 0xA040);
    pub const CONCEPT_NAME_CODE_SEQ: Tag = t(0x0040, 0xA043);
    pub const NUMERIC_VALUE: Tag = t(0x0040, 0xA30A);
    pub const MEASURED_VALUE_SEQ: Tag = t(0x0040, 0xA300);
    pub const NUMERIC_VALUE_QUALIFIER_CODE_SEQ: Tag = t(0x0040, 0xA301);
    pub const CONCEPT_CODE_SEQ: Tag = t(0x0040, 0xA168);
    pub const TEXT_VALUE: Tag = t(0x0040, 0xA160);
    pub(crate) const CONTENT_SEQUENCE: Tag = t(0x0040, 0xA730);
    pub const REFERENCED_CONTENT_ITEM_IDENTIFIER: Tag = t(0x0040, 0xDB73);
    pub const CONTENT_TEMPLATE_SEQUENCE: Tag = t(0x0040, 0xA504);
    pub const TEMPLATE_IDENTIFIER: Tag = t(0x0040, 0xDB00);
    pub const TEMPLATE_VERSION: Tag = t(0x0040, 0xDB06);
    pub const CONTINUITY_OF_CONTENT: Tag = t(0x0040, 0xA050);
    pub const CURRENT_REQUESTED_PROCEDURE_EVIDENCE: Tag = t(0x0040, 0xA375);
    pub const COMPLETION_FLAG: Tag = t(0x0040, 0xA491);
    pub const REFERENCED_PERFORMED_PROCEDURE_STEP: Tag = t(0x0008, 0x1111);
    pub const PERFORMED_PROCEDURE_CODE_SEQUENCE: Tag = t(0x0040, 0xA372);
    pub const VERIFICATION_FLAG: Tag = t(0x0040, 0xA493);
    pub const DATE_TIME_VALUE: Tag = t(0x0040, 0xA120);
    pub const DATE_VALUE: Tag = t(0x0040, 0xA121);
    pub const TIME_VALUE: Tag = t(0x0040, 0xA122);
    pub const PERSON_NAME_VALUE: Tag = t(0x0040, 0xA123);
    pub const UID: Tag = t(0x0040, 0xA124);
    pub const REFERENCED_WAVEFORM_CH: Tag = t(0x0040, 0xA0B0);
    pub const TEMPORAL_RANGE_TYPE: Tag = t(0x0040, 0xA130);
    pub const REFERENCED_SAMPLES: Tag = t(0x0040, 0xA132);
    pub const REFERENCED_TIME_OFFSET: Tag = t(0x0040, 0xA138);
    pub const REFERENCED_DATE_TIME: Tag = t(0x0040, 0xA13A);
    pub const GRAPHIC_DATA: Tag = t(0x0070, 0x0022);
    pub const GRAPHIC_TYPE: Tag = t(0x0070, 0x0023);
    pub const FIDUCIAL_UID: Tag = t(0x0070, 0x031A);
    pub const PIXEL_ORIGIN_INTERPRET: Tag = t(0x0048, 0x0301);
    pub const REFERENCED_FRAME_OF_REF_UID: Tag = t(0x3006, 0x0024);
    pub const REFERENCED_SEGMENT_NUMBER: Tag = t(0x0062, 0x000B);
    pub const COMPLETION_FLAG_DESCRIPTION: Tag = t(0x0040, 0xA492);
    pub const VERIFYING_OBSERVER_SEQUENCE: Tag = t(0x0040, 0xA073);
    pub const VERIFYING_OBSERVER_NAME: Tag = t(0x0040, 0xA075);
    pub const VERIFYING_OBSERVER_CODE: Tag = t(0x0040, 0xA088);
    pub const VERIFYING_ORGANIZATION: Tag = t(0x0040, 0xA027);
    pub const VERIFICATION_DATE_TIME: Tag = t(0x0040, 0xA030);
    pub const PREDECESSOR_DOCUMENTS: Tag = t(0x0040, 0xA360);
}
use tags as tg;
impl Document {
    pub fn elements(&self) -> Result<Vec<Element>, SrError> {
        if let (_, _, Some(err)) = resolve_references(Some(self), default_reference_options()) {
            return Err(err.wrapped(Kind::InvalidDocument));
        }
        if let (_, Some(err)) = validate_relationships(Some(self), default_relationship_options())
            && !err.is(Kind::UnconstrainedSopClass)
        {
            return Err(err.wrapped(Kind::InvalidDocument));
        }
        self.encode_elements()
    }
    pub(crate) fn encode_elements(&self) -> Result<Vec<Element>, SrError> {
        let (mut completion, mut verification) = (String::new(), String::new());
        if self.sop_class_uid != KEY_OBJECT_SELECTION_DOCUMENT_STORAGE {
            (completion, verification) = validate_document_state(
                &self.completion_flag,
                &self.verification_flag,
                self.verifying_observers.len(),
            )?;
        }
        let refs = content_image_references(&self.content);
        let evidence = evidence_references(&refs, self.common.source.as_ref())
            .map_err(|e| e.wrapped(Kind::InvalidDocument))?;
        let mut elems = vec![
            str_elem(tg::SOP_CLASS_UID, Vr::UI, &self.sop_class_uid),
            str_elem(tg::SOP_INSTANCE_UID, Vr::UI, &self.sop_instance_uid),
            str_elem(tg::MODALITY, Vr::CS, self.modality()),
            str_elem(
                tg::SERIES_NUMBER,
                Vr::IS,
                &default_string(&self.series_number, "1"),
            ),
            str_elem(
                tg::INSTANCE_NUMBER,
                Vr::IS,
                &default_string(&self.instance_number, "1"),
            ),
            str_elem(tg::VALUE_TYPE, Vr::CS, value::CONTAINER),
            str_elem(
                tg::CONTINUITY_OF_CONTENT,
                Vr::CS,
                &default_string(&self.continuity_of_content, "SEPARATE"),
            ),
            derivedio::seq(tg::REFERENCED_PERFORMED_PROCEDURE_STEP, Vec::new()),
        ];
        if self.sop_class_uid != KEY_OBJECT_SELECTION_DOCUMENT_STORAGE {
            elems.push(str_elem(tg::COMPLETION_FLAG, Vr::CS, &completion));
            elems.push(str_elem(tg::VERIFICATION_FLAG, Vr::CS, &verification));
            elems.push(derivedio::seq(
                tg::PERFORMED_PROCEDURE_CODE_SEQUENCE,
                Vec::new(),
            ));
            elems.extend(lifecycle::lifecycle_elements(
                &self.completion_flag_description,
                &self.verifying_observers,
                &self.predecessor_documents,
            )?);
        }
        if !self.study_instance_uid.is_empty() {
            elems.push(str_elem(
                tg::STUDY_INSTANCE_UID,
                Vr::UI,
                &self.study_instance_uid,
            ));
        }
        elems.push(str_elem(
            tg::SERIES_INSTANCE_UID,
            Vr::UI,
            &document_series_uid(
                &self.sop_class_uid,
                &self.sop_instance_uid,
                &self.series_instance_uid,
            ),
        ));
        let (mut content_date, mut content_time) =
            (self.content_date.clone(), self.content_time.clone());
        if content_date.is_empty() || content_time.is_empty() {
            let (d, t) = current_content_date_time();
            if content_date.is_empty() {
                content_date = d;
            }
            if content_time.is_empty() {
                content_time = t;
            }
        }
        elems.push(str_elem(tg::CONTENT_DATE, Vr::DA, &content_date));
        elems.push(str_elem(tg::CONTENT_TIME, Vr::TM, &content_time));
        if !self.title.empty() {
            elems.push(code_sequence(tg::CONCEPT_NAME_CODE_SEQ, &self.title));
        }
        if !self.content.is_empty() {
            elems.push(content_sequence(&self.content)?);
        }
        if self.sop_class_uid == KEY_OBJECT_SELECTION_DOCUMENT_STORAGE || !evidence.is_empty() {
            elems.push(hierarchical_reference_sequence(
                tg::CURRENT_REQUESTED_PROCEDURE_EVIDENCE,
                &evidence,
            ));
        }
        Ok(elems)
    }
    pub fn dataset(&self) -> Result<Object, SrError> {
        let elements = self.elements()?;
        let mut dataset = Object::from_elements(elements);
        self.common
            .apply(&mut dataset, inherit_modules::WITHOUT_FRAME_OF_REFERENCE)
            .map_err(|e| SrError::msg(e.to_string()))?;
        Ok(dataset)
    }
    fn modality(&self) -> &str {
        if !self.modality.is_empty() {
            return &self.modality;
        }
        if self.sop_class_uid == KEY_OBJECT_SELECTION_DOCUMENT_STORAGE {
            return "KO";
        }
        "SR"
    }
}
pub(crate) fn validate_document_state(
    completion: &str,
    verification: &str,
    verifying_observers: usize,
) -> Result<(String, String), SrError> {
    let completion = default_string(completion.trim(), COMPLETION_PARTIAL);
    let verification = default_string(verification.trim(), VERIFICATION_UNVERIFIED);
    if completion != COMPLETION_PARTIAL && completion != COMPLETION_COMPLETE {
        return Err(SrError::invalid_document(&format!(
            ": unsupported Completion Flag {}",
            crate::textfmt::quote(&completion)
        )));
    }
    if verification != VERIFICATION_UNVERIFIED && verification != VERIFICATION_VERIFIED {
        return Err(SrError::invalid_document(&format!(
            ": unsupported Verification Flag {}",
            crate::textfmt::quote(&verification)
        )));
    }
    if verification == VERIFICATION_VERIFIED && completion != COMPLETION_COMPLETE {
        return Err(SrError::invalid_document(": VERIFIED requires COMPLETE"));
    }
    if verification == VERIFICATION_VERIFIED && verifying_observers == 0 {
        return Err(SrError::invalid_document(
            ": VERIFIED requires a verifying observer",
        ));
    }
    Ok((completion, verification))
}
pub(crate) fn content_sequence(items: &[ContentItem]) -> Result<Element, SrError> {
    let mut datasets = Vec::with_capacity(items.len());
    for (i, item) in items.iter().enumerate() {
        datasets.push(
            content_item_data_set(item)
                .map_err(|e| e.suffixed(&format!(": content item {}", i + 1)))?,
        );
    }
    Ok(seq_element(tg::CONTENT_SEQUENCE, datasets))
}
fn ds(elements: Vec<Element>) -> DataSet {
    DataSet {
        elements,
        item_offset: None,
    }
}
pub(crate) fn content_item_data_set(item: &ContentItem) -> Result<DataSet, SrError> {
    if !item.referenced_content_item_identifier.is_empty() {
        reference::validate_encoded_reference_item(
            item,
            default_reference_options().max_path_components,
        )?;
        return Ok(ds(vec![
            str_elem(tg::RELATIONSHIP_TYPE, Vr::CS, &item.relationship_type),
            content_item_identifier_element(&item.referenced_content_item_identifier),
        ]));
    }
    let mut elems = Vec::new();
    if !item.relationship_type.is_empty() {
        elems.push(str_elem(
            tg::RELATIONSHIP_TYPE,
            Vr::CS,
            &item.relationship_type,
        ));
    }
    elems.push(str_elem(tg::VALUE_TYPE, Vr::CS, &item.value_type));
    if !item.concept_name.empty() {
        elems.push(code_sequence(tg::CONCEPT_NAME_CODE_SEQ, &item.concept_name));
    }
    match item.value_type.as_str() {
        value::TEXT => elems.push(str_elem(tg::TEXT_VALUE, Vr::UT, &item.text)),
        value::CODE => elems.push(code_sequence(tg::CONCEPT_CODE_SEQ, &item.code)),
        value::IMAGE => elems.push(referenced_sop_sequence(&item.image)),
        value::NUM => {
            match &item.measurement {
                Some(m) => elems.push(measured_value_sequence(m)),
                None => elems.push(seq_element(tg::MEASURED_VALUE_SEQ, Vec::new())),
            }
            let mut qualifier = item.numeric_value_qualifier.clone();
            if item.measurement.is_none() && qualifier.empty() {
                qualifier = from_code(dcm::ValueUnknown);
            }
            if !qualifier.empty() {
                elems.push(code_sequence(
                    tg::NUMERIC_VALUE_QUALIFIER_CODE_SEQ,
                    &qualifier,
                ));
            }
        }
        value::CONTAINER => elems.push(str_elem(
            tg::CONTINUITY_OF_CONTENT,
            Vr::CS,
            &default_string(&item.continuity_of_content, "SEPARATE"),
        )),
        value::DATETIME => elems.push(str_elem(tg::DATE_TIME_VALUE, Vr::DT, &item.date_time)),
        value::DATE => elems.push(str_elem(tg::DATE_VALUE, Vr::DA, &item.date)),
        value::TIME => elems.push(str_elem(tg::TIME_VALUE, Vr::TM, &item.time)),
        value::UIDREF => elems.push(str_elem(tg::UID, Vr::UI, &item.uid)),
        value::PNAME => elems.push(str_elem(tg::PERSON_NAME_VALUE, Vr::PN, &item.person_name)),
        value::COMPOSITE => elems.push(seq_element(
            tg::REF_SOP_SEQUENCE,
            vec![ds(referenced_sop_elements(
                &item.composite.sop_class_uid,
                &item.composite.sop_instance_uid,
            ))],
        )),
        value::WAVEFORM => elems.push(waveform_reference_sequence(&item.waveform)),
        value::SCOORD => elems.extend(spatial_coordinate_elements(&item.spatial)),
        value::SCOORD3D => elems.extend(spatial_coordinate3d_elements(&item.spatial3d)),
        value::TCOORD => elems.extend(temporal_coordinate_elements(&item.temporal)),
        _ => elems.extend(item.value_elements.iter().cloned()),
    }
    if is_modeled_value_type(&item.value_type) {
        elems.extend(item.value_elements.iter().cloned());
    }
    if !item.children.is_empty() {
        elems.push(content_sequence(&item.children)?);
    }
    Ok(ds(elems))
}
pub(crate) fn is_modeled_value_type(v: &str) -> bool {
    matches!(
        v,
        value::TEXT
            | value::CODE
            | value::IMAGE
            | value::NUM
            | value::CONTAINER
            | value::DATETIME
            | value::DATE
            | value::TIME
            | value::UIDREF
            | value::PNAME
            | value::COMPOSITE
            | value::WAVEFORM
            | value::SCOORD
            | value::SCOORD3D
            | value::TCOORD
    )
}
fn untyped(tag: Tag, vr: Vr, value: Value) -> Element {
    Element {
        header: ElementHeader {
            tag,
            vr,
            length: 0,
            length_set: false,
        },
        value,
    }
}
pub(crate) fn content_item_identifier_element(identifier: &[u32]) -> Element {
    let raw = identifier.iter().flat_map(|v| v.to_le_bytes()).collect();
    untyped(
        tg::REFERENCED_CONTENT_ITEM_IDENTIFIER,
        Vr::UL,
        Value::Raw(raw),
    )
}
pub(crate) fn code_sequence(tag: Tag, code: &CodedEntry) -> Element {
    seq_element(
        tag,
        vec![ds(vec![
            str_elem(tg::CODE_VALUE, Vr::SH, &code.code_value),
            str_elem(tg::CODING_SCHEME, Vr::SH, &code.coding_scheme_designator),
            str_elem(tg::CODE_MEANING, Vr::LO, &code.code_meaning),
        ])],
    )
}
pub(crate) fn referenced_sop_sequence(r: &ImageReference) -> Element {
    let mut elems = referenced_sop_elements(&r.sop_class_uid, &r.sop_instance_uid);
    if !r.frames.is_empty() {
        elems.push(untyped(
            tg::REF_FRAME_NUMBER,
            Vr::IS,
            Value::Strings(r.frames.iter().map(i64::to_string).collect()),
        ));
    }
    seq_element(tg::REF_SOP_SEQUENCE, vec![ds(elems)])
}
fn waveform_reference_sequence(r: &WaveformReference) -> Element {
    let mut elems =
        referenced_sop_elements(&r.composite.sop_class_uid, &r.composite.sop_instance_uid);
    if !r.channels.is_empty() {
        let channels: Vec<u16> = r
            .channels
            .iter()
            .flat_map(|c| [c.multiplex_group_number, c.channel_number])
            .collect();
        elems.push(derivedio::us(tg::REFERENCED_WAVEFORM_CH, &channels));
    }
    seq_element(tg::REF_SOP_SEQUENCE, vec![ds(elems)])
}
pub(crate) fn referenced_sop_elements(sop_class_uid: &str, sop_instance_uid: &str) -> Vec<Element> {
    vec![
        str_elem(tg::REF_SOP_CLASS_UID, Vr::UI, sop_class_uid),
        str_elem(tg::REF_SOP_INSTANCE_UID, Vr::UI, sop_instance_uid),
    ]
}
fn spatial_coordinate_elements(s: &SpatialCoordinates) -> Vec<Element> {
    let flat: Vec<f64> = s.coordinates.iter().flat_map(|p| [p.x, p.y]).collect();
    let mut out = vec![
        str_elem(tg::GRAPHIC_TYPE, Vr::CS, &s.graphic_type),
        derivedio::fl(tg::GRAPHIC_DATA, &flat),
    ];
    if !s.pixel_origin_interpretation.is_empty() {
        out.push(str_elem(
            tg::PIXEL_ORIGIN_INTERPRET,
            Vr::CS,
            &s.pixel_origin_interpretation,
        ));
    }
    if !s.fiducial_uid.is_empty() {
        out.push(str_elem(tg::FIDUCIAL_UID, Vr::UI, &s.fiducial_uid));
    }
    out
}
pub(crate) fn spatial_coordinate3d_elements(s: &SpatialReference) -> Vec<Element> {
    let flat: Vec<f64> = s.coordinates.iter().flat_map(|p| [p.x, p.y, p.z]).collect();
    let mut out = vec![
        str_elem(
            tg::REFERENCED_FRAME_OF_REF_UID,
            Vr::UI,
            &s.frame_of_reference_uid,
        ),
        str_elem(tg::GRAPHIC_TYPE, Vr::CS, &s.graphic_type),
        derivedio::fl(tg::GRAPHIC_DATA, &flat),
    ];
    if !s.fiducial_uid.is_empty() {
        out.push(str_elem(tg::FIDUCIAL_UID, Vr::UI, &s.fiducial_uid));
    }
    out
}
fn temporal_coordinate_elements(t: &TemporalCoordinates) -> Vec<Element> {
    let mut out = vec![str_elem(tg::TEMPORAL_RANGE_TYPE, Vr::CS, &t.range_type)];
    if !t.sample_positions.is_empty() {
        out.push(derivedio::ul(tg::REFERENCED_SAMPLES, &t.sample_positions));
    }
    if !t.time_offsets.is_empty() {
        out.push(derivedio::ds(tg::REFERENCED_TIME_OFFSET, &t.time_offsets));
    }
    if !t.date_times.is_empty() {
        out.push(untyped(
            tg::REFERENCED_DATE_TIME,
            Vr::DT,
            Value::Strings(t.date_times.clone()),
        ));
    }
    out
}
pub(crate) fn measured_value_sequence(m: &Measurement) -> Element {
    seq_element(
        tg::MEASURED_VALUE_SEQ,
        vec![ds(vec![
            derivedio::ds(tg::NUMERIC_VALUE, &[m.value]),
            code_sequence(tg::MEASUREMENT_UNITS_CODE, &m.units),
        ])],
    )
}
fn content_image_references(items: &[ContentItem]) -> Vec<ImageReference> {
    let mut refs = Vec::new();
    for item in items {
        if item.value_type == value::IMAGE {
            refs.push(item.image.clone());
        }
        refs.extend(content_image_references(&item.children));
    }
    refs
}
pub(crate) fn hierarchical_reference_sequence(tag: Tag, refs: &[ImageReference]) -> Element {
    struct Series {
        uid: String,
        refs: Vec<(String, String)>,
        seen: std::collections::HashSet<String>,
    }
    struct Study {
        uid: String,
        series: Vec<Series>,
    }
    let mut studies: Vec<Study> = Vec::new();
    for r in refs {
        let study_uid = clean(&r.study_instance_uid).to_string();
        let series_uid = clean(&r.series_instance_uid).to_string();
        let class = clean(&r.sop_class_uid).to_string();
        let instance = clean(&r.sop_instance_uid).to_string();
        let si = match studies.iter().position(|s| s.uid == study_uid) {
            Some(i) => i,
            None => {
                studies.push(Study {
                    uid: study_uid,
                    series: Vec::new(),
                });
                studies.len() - 1
            }
        };
        let study = &mut studies[si];
        let ri = match study.series.iter().position(|s| s.uid == series_uid) {
            Some(i) => i,
            None => {
                study.series.push(Series {
                    uid: series_uid,
                    refs: Vec::new(),
                    seen: Default::default(),
                });
                study.series.len() - 1
            }
        };
        let series = &mut study.series[ri];
        if !series.seen.insert(instance.clone()) {
            continue;
        }
        series.refs.push((class, instance));
    }
    let study_items = studies
        .iter()
        .map(|study| {
            let series_items = study
                .series
                .iter()
                .map(|series| {
                    let sops = series
                        .refs
                        .iter()
                        .map(|(c, i)| ds(referenced_sop_elements(c, i)))
                        .collect();
                    ds(vec![
                        str_elem(tg::SERIES_INSTANCE_UID, Vr::UI, &series.uid),
                        seq_element(tg::REF_SOP_SEQUENCE, sops),
                    ])
                })
                .collect();
            ds(vec![
                str_elem(tg::STUDY_INSTANCE_UID, Vr::UI, &study.uid),
                seq_element(tg::REFERENCED_SERIES_SEQUENCE, series_items),
            ])
        })
        .collect();
    seq_element(tag, study_items)
}
pub(crate) fn evidence_references(
    refs: &[ImageReference],
    source: Option<&Object>,
) -> Result<Vec<ImageReference>, SrError> {
    let (mut source_instance, mut source_study, mut source_series) =
        (String::new(), String::new(), String::new());
    if let Some(source) = source {
        source_instance = source.get_uid(tg::SOP_INSTANCE_UID).unwrap_or_default();
        source_study = source.get_uid(tg::STUDY_INSTANCE_UID).unwrap_or_default();
        source_series = source.get_uid(tg::SERIES_INSTANCE_UID).unwrap_or_default();
    }
    let mut out = Vec::with_capacity(refs.len());
    for (i, r) in refs.iter().enumerate() {
        let mut r = r.clone();
        let instance = clean(&r.sop_instance_uid);
        if !instance.is_empty() && instance == clean(&source_instance) {
            if clean(&r.study_instance_uid).is_empty() {
                r.study_instance_uid = clean(&source_study).into();
            }
            if clean(&r.series_instance_uid).is_empty() {
                r.series_instance_uid = clean(&source_series).into();
            }
        }
        if clean(&r.study_instance_uid).is_empty()
            || clean(&r.series_instance_uid).is_empty()
            || clean(&r.sop_class_uid).is_empty()
            || clean(&r.sop_instance_uid).is_empty()
        {
            return Err(SrError::msg(format!(
                "evidence reference {i} ({}) requires study, series, SOP class, and SOP instance UIDs",
                clean(&r.sop_instance_uid)
            )));
        }
        out.push(r);
    }
    Ok(out)
}
pub(crate) fn document_series_uid(
    sop_class_uid: &str,
    sop_instance_uid: &str,
    uid: &str,
) -> String {
    let uid = clean(uid);
    if !uid.is_empty() {
        return uid.into();
    }
    if sop_class_uid == KEY_OBJECT_SELECTION_DOCUMENT_STORAGE {
        return derivedio::derived_uid(&[sop_instance_uid, "key-object-selection-series"]);
    }
    derivedio::derived_uid(&[sop_instance_uid, "sr-document-series"])
}
pub(crate) fn str_elem(tag: Tag, vr: Vr, value: &str) -> Element {
    untyped(tag, vr, Value::Strings(vec![value.to_string()]))
}
pub(crate) fn seq_element(tag: Tag, mut items: Vec<DataSet>) -> Element {
    for item in &mut items {
        item.elements.sort_by_key(|e| e.tag().key());
    }
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
pub(crate) fn clean(value: &str) -> &str {
    value.trim().trim_matches('\0')
}
pub(crate) fn default_string(value: &str, fallback: &str) -> String {
    let v = clean(value);
    if v.is_empty() {
        fallback.into()
    } else {
        v.into()
    }
}
pub(crate) fn current_content_date_time() -> (String, String) {
    let now = chrono::Local::now();
    (
        now.format("%Y%m%d").to_string(),
        now.format("%H%M%S%.6f").to_string(),
    )
}
