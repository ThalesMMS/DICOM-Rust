use super::lifecycle::read_lifecycle;
use super::reference::{
    ContentItemIdentifier, ReferenceIndex, ValidationMode, ValidationReport, append_path,
    default_reference_options, identifier_string, resolve_references,
};
use super::relationship::{RelationshipOptions, validate_relationships};
use super::tags as tg;
use super::template_metadata::{
    CodeContext, ContentItemCodeContexts, EquivalentCodeEntry, TemplateIdentification,
    TemplateValidationMetadata, code_contexts_empty, decode_code_context,
    decode_template_identification,
};
use super::{
    CodedEntry, CompositeReference, ContentItem, Document, ImageReference, Kind, Measurement,
    Point2D, SpatialCoordinates, SrError, TemporalCoordinates, WaveformChannel, WaveformReference,
    clean, derivedio, is_modeled_value_type, value,
};
use crate::object::{Element, Object, Tag, Vr};
use std::collections::HashMap;
pub(crate) const MAX_CONTENT_TREE_DEPTH: i64 = 64;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ReadOptions {
    pub max_depth: i64,
    pub max_items: i64,
    pub max_references: i64,
    pub max_path_components: i64,
    pub resolve_references: bool,
    pub reference_mode: ValidationMode,
    pub max_findings: i64,
    pub validate_relationships: bool,
    pub relationship_mode: ValidationMode,
}
pub fn default_read_options() -> ReadOptions {
    ReadOptions {
        max_depth: MAX_CONTENT_TREE_DEPTH,
        max_items: 100_000,
        max_references: 100_000,
        max_path_components: 65,
        resolve_references: false,
        reference_mode: ValidationMode::WARN,
        max_findings: 1_024,
        validate_relationships: false,
        relationship_mode: ValidationMode::WARN,
    }
}
#[derive(Clone, Debug, Default)]
pub struct ReadResult {
    pub document: Option<Document>,
    pub references: Option<ReferenceIndex>,
    pub report: ValidationReport,
    pub template_metadata: TemplateValidationMetadata,
}
pub(crate) fn normalize_read_options(mut o: ReadOptions) -> ReadOptions {
    let d = default_read_options();
    if o.max_depth == 0 {
        o.max_depth = d.max_depth;
    }
    if o.max_items == 0 {
        o.max_items = d.max_items;
    }
    if o.max_references == 0 {
        o.max_references = d.max_references;
    }
    if o.max_path_components == 0 {
        o.max_path_components = d.max_path_components;
    }
    if o.reference_mode.0 == 0 {
        o.reference_mode = d.reference_mode;
    }
    if o.max_findings == 0 {
        o.max_findings = d.max_findings;
    }
    o
}
#[derive(Default)]
pub(crate) struct Decoder {
    pub(crate) err: Option<SrError>,
    max_depth: i64,
    max_items: i64,
    max_references: i64,
    max_path_components: i64,
    item_count: i64,
    reference_count: i64,
    metadata_count: i64,
    pub(crate) template_metadata: TemplateValidationMetadata,
}
impl Decoder {
    pub(crate) fn bounded(
        max_depth: i64,
        max_items: i64,
        max_references: i64,
        max_path_components: i64,
    ) -> Decoder {
        Decoder {
            max_depth,
            max_items,
            max_references,
            max_path_components,
            ..Default::default()
        }
    }
    pub(crate) fn fail(&mut self, err: SrError) {
        if self.err.is_none() {
            self.err = Some(err);
        }
    }
    fn reserve_metadata_items(&mut self, count: i64) -> bool {
        if count <= 0 {
            return true;
        }
        if self.max_items > 0 && count > self.max_items - self.metadata_count {
            self.fail(SrError::new(
                Kind::ResourceLimitExceeded,
                &format!(": code metadata count exceeds {}", self.max_items),
            ));
            return false;
        }
        self.metadata_count += count;
        true
    }
    pub(crate) fn add_code_contexts(&mut self, c: ContentItemCodeContexts) {
        if !self.reserve_metadata_items(1) {
            return;
        }
        self.template_metadata.code_contexts.push(c);
    }
    pub(crate) fn add_template_identification(&mut self, t: TemplateIdentification) {
        if !self.reserve_metadata_items(1) {
            return;
        }
        self.template_metadata.template_identifications.push(t);
    }
    fn decode_failure(&mut self, tag: Tag, e: impl std::fmt::Display) {
        self.fail(SrError {
            msg: format!("decode {tag}: {e}"),
            kinds: vec![Kind::Object],
            reference: None,
            code: None,
        });
    }
    pub(crate) fn string(&mut self, obj: Option<&Object>, tag: Tag) -> String {
        let Some(obj) = obj else {
            return String::new();
        };
        if self.err.is_some() || !obj.has(tag) {
            return String::new();
        }
        match obj.lookup_string(tag) {
            Ok(v) => clean(&v).to_string(),
            Err(e) => {
                self.decode_failure(tag, e);
                String::new()
            }
        }
    }
    pub(crate) fn strings(&mut self, obj: Option<&Object>, tag: Tag) -> Vec<String> {
        let Some(obj) = obj else {
            return Vec::new();
        };
        if self.err.is_some() || !obj.has(tag) {
            return Vec::new();
        }
        match obj.lookup_strings(tag) {
            Ok(v) => v.iter().map(|s| clean(s).to_string()).collect(),
            Err(e) => {
                self.decode_failure(tag, e);
                Vec::new()
            }
        }
    }
    pub(crate) fn sequence(&mut self, obj: Option<&Object>, tag: Tag) -> (Vec<Object>, bool) {
        let Some(obj) = obj else {
            return (Vec::new(), false);
        };
        if self.err.is_some() {
            return (Vec::new(), false);
        }
        let Some(element) = obj.get(tag) else {
            return (Vec::new(), false);
        };
        if element.vr() != Vr::SQ {
            let msg = format!("VR is {}, want SQ", element.vr());
            self.decode_failure(tag, msg);
            return (Vec::new(), true);
        }
        match obj.get_sequence(tag) {
            Some(items) => (items, true),
            None => {
                self.decode_failure(tag, "value is not a sequence");
                (Vec::new(), true)
            }
        }
    }
    pub(crate) fn ints(&mut self, obj: Option<&Object>, tag: Tag) -> Vec<i64> {
        let Some(obj) = obj else {
            return Vec::new();
        };
        if self.err.is_some() || !obj.has(tag) {
            return Vec::new();
        }
        match derivedio::lookup_ints(Some(obj), tag) {
            Ok(v) => v,
            Err(e) => {
                self.decode_failure(tag, e);
                Vec::new()
            }
        }
    }
    fn content_item_identifier(&mut self, obj: &Object) -> (ContentItemIdentifier, bool) {
        if self.err.is_some() {
            return (Vec::new(), false);
        }
        let tag = tg::REFERENCED_CONTENT_ITEM_IDENTIFIER;
        let Some(element) = obj.get(tag) else {
            return (Vec::new(), false);
        };
        if element.vr() != Vr::UL {
            let msg = format!("VR is {}, want UL", element.vr());
            self.decode_failure(tag, msg);
            return (Vec::new(), true);
        }
        let values = match derivedio::lookup_ints(Some(obj), tag) {
            Ok(v) => v,
            Err(e) => {
                self.decode_failure(tag, e);
                return (Vec::new(), true);
            }
        };
        let mut out = Vec::with_capacity(values.len());
        for v in values {
            if v < 0 || v > i64::from(u32::MAX) {
                self.decode_failure(tag, "component is outside UL");
                return (Vec::new(), true);
            }
            out.push(v as u32);
        }
        (out, true)
    }
    pub(crate) fn floats(&mut self, obj: Option<&Object>, tag: Tag) -> Vec<f64> {
        let Some(obj) = obj else {
            return Vec::new();
        };
        if self.err.is_some() || !obj.has(tag) {
            return Vec::new();
        }
        match derivedio::lookup_floats(Some(obj), tag) {
            Ok(v) => v,
            Err(e) => {
                self.decode_failure(tag, e);
                Vec::new()
            }
        }
    }
}
#[derive(Clone, Debug)]
pub struct ReadDocumentError {
    pub document: Option<Box<Document>>,
    pub error: SrError,
}
impl std::fmt::Display for ReadDocumentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.error, f)
    }
}
impl std::error::Error for ReadDocumentError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        std::error::Error::source(&self.error)
    }
}
pub fn read_document(obj: Option<&Object>) -> Result<Document, ReadDocumentError> {
    let (result, err) = read_inner(obj, default_read_options());
    match err {
        None => Ok(result.document.unwrap_or_default()),
        Some(error) => Err(ReadDocumentError {
            document: result.document.map(Box::new),
            error,
        }),
    }
}
pub fn read_document_with_options(
    obj: Option<&Object>,
    options: ReadOptions,
) -> (ReadResult, Option<SrError>) {
    let options = normalize_read_options(options);
    if options.max_depth < 1
        || options.max_items < 1
        || options.max_references < 1
        || options.max_path_components < 1
        || options.max_findings < 1
    {
        return (
            ReadResult::default(),
            Some(SrError::new(
                Kind::ResourceLimitExceeded,
                ": invalid read limits",
            )),
        );
    }
    read_inner(obj, options)
}
fn read_inner(obj: Option<&Object>, options: ReadOptions) -> (ReadResult, Option<SrError>) {
    let Some(obj) = obj else {
        return (
            ReadResult::default(),
            Some(SrError::new(Kind::NoDataset, "")),
        );
    };
    let mut dec = Decoder {
        max_depth: options.max_depth,
        max_items: options.max_items,
        max_references: options.max_references,
        max_path_components: options.max_path_components,
        ..Default::default()
    };
    let o = Some(obj);
    let mut doc = Document {
        sop_class_uid: dec.string(o, tg::SOP_CLASS_UID),
        sop_instance_uid: dec.string(o, tg::SOP_INSTANCE_UID),
        study_instance_uid: dec.string(o, tg::STUDY_INSTANCE_UID),
        series_instance_uid: dec.string(o, tg::SERIES_INSTANCE_UID),
        modality: dec.string(o, tg::MODALITY),
        content_date: dec.string(o, tg::CONTENT_DATE),
        content_time: dec.string(o, tg::CONTENT_TIME),
        series_number: dec.string(o, tg::SERIES_NUMBER),
        instance_number: dec.string(o, tg::INSTANCE_NUMBER),
        completion_flag: dec.string(o, tg::COMPLETION_FLAG),
        verification_flag: dec.string(o, tg::VERIFICATION_FLAG),
        continuity_of_content: dec.string(o, tg::CONTINUITY_OF_CONTENT),
        ..Default::default()
    };
    (
        doc.completion_flag_description,
        doc.verifying_observers,
        doc.predecessor_documents,
    ) = read_lifecycle(&mut dec, obj);
    let root_path: ContentItemIdentifier = vec![1];
    let mut root_contexts = ContentItemCodeContexts {
        path: identifier_string(&root_path),
        ..Default::default()
    };
    let (title, ctx, eq, _) = decode_code_with_metadata(&mut dec, o, tg::CONCEPT_NAME_CODE_SEQ);
    doc.title = title;
    root_contexts.concept_name = ctx;
    root_contexts.concept_name_equivalent_codes = eq;
    if !code_contexts_empty(&root_contexts) {
        dec.add_code_contexts(root_contexts);
    }
    if let Some(t) = decode_template_identification(&mut dec, obj, &root_path) {
        dec.add_template_identification(t);
    }
    let evidence = read_current_requested_procedure_evidence(&mut dec, obj);
    let (items, present) = dec.sequence(o, tg::CONTENT_SEQUENCE);
    if present {
        match read_content_items(&mut dec, &items, 1, &root_path) {
            Err(e) => {
                let tm = dec.template_metadata;
                return (
                    ReadResult {
                        document: Some(doc),
                        template_metadata: tm,
                        ..Default::default()
                    },
                    Some(e),
                );
            }
            Ok(content) => {
                doc.content = content;
                apply_evidence_hierarchy(&mut doc.content, &evidence);
            }
        }
    }
    if let Some(e) = dec.err.take() {
        return (
            ReadResult {
                document: Some(doc),
                template_metadata: dec.template_metadata,
                ..Default::default()
            },
            Some(e),
        );
    }
    let mut result = ReadResult {
        document: None,
        template_metadata: dec.template_metadata,
        ..Default::default()
    };
    if options.resolve_references {
        let mut ro = default_reference_options();
        ro.mode = options.reference_mode;
        ro.max_depth = options.max_depth;
        ro.max_items = options.max_items;
        ro.max_references = options.max_references;
        ro.max_path_components = options.max_path_components;
        ro.max_findings = options.max_findings;
        let (index, report, err) = resolve_references(Some(&doc), ro);
        result.references = index;
        result.report = report;
        if let Some(e) = err {
            result.document = Some(doc);
            return (result, Some(e));
        }
    }
    if options.validate_relationships {
        let mut mode = options.relationship_mode;
        if mode.0 == 0 {
            mode = ValidationMode::WARN;
        }
        let (report, err) = validate_relationships(
            Some(&doc),
            RelationshipOptions {
                mode,
                max_depth: options.max_depth,
                max_items: options.max_items,
                max_findings: options.max_findings,
            },
        );
        result.report.findings.extend(report.findings);
        result.report.truncated = result.report.truncated || report.truncated;
        if let Some(e) = err
            && !e.is(Kind::UnconstrainedSopClass)
        {
            result.document = Some(doc);
            return (result, Some(e));
        }
    }
    result.document = Some(doc);
    (result, None)
}
fn resource(suffix: String) -> SrError {
    SrError::new(Kind::ResourceLimitExceeded, &suffix)
}
pub(crate) fn read_content_items_bounded(
    dec: &mut Decoder,
    items: &[Object],
) -> Result<Vec<ContentItem>, SrError> {
    read_content_items(dec, items, 1, &[1])
}
fn read_content_items(
    dec: &mut Decoder,
    items: &[Object],
    depth: i64,
    parent_path: &[u32],
) -> Result<Vec<ContentItem>, SrError> {
    if depth > dec.max_depth {
        return Err(resource(format!(
            ": content tree depth exceeds {}",
            dec.max_depth
        )));
    }
    let remaining = dec.max_items - dec.item_count;
    if dec.max_items > 0 && items.len() as i64 > remaining {
        return Err(resource(format!(
            ": content item count exceeds {}",
            dec.max_items
        )));
    }
    let mut out = Vec::with_capacity(items.len());
    for (index, it) in items.iter().enumerate() {
        let item_path = append_path(parent_path, index as u32 + 1);
        dec.item_count += 1;
        if dec.item_count > dec.max_items {
            return Err(resource(format!(
                ": content item count exceeds {}",
                dec.max_items
            )));
        }
        let o = Some(it);
        let mut ci = ContentItem {
            value_type: dec.string(o, tg::VALUE_TYPE),
            relationship_type: dec.string(o, tg::RELATIONSHIP_TYPE),
            continuity_of_content: dec.string(o, tg::CONTINUITY_OF_CONTENT),
            ..Default::default()
        };
        let (identifier, reference_present) = dec.content_item_identifier(it);
        ci.referenced_content_item_identifier = identifier;
        ci.encoding.empty_reference_macro_present =
            reference_present && ci.referenced_content_item_identifier.is_empty();
        ci.value_elements = unmodeled_value_elements(it, &ci.value_type);
        if reference_present {
            ci.encoding.by_value_macro_present = by_value_macro_present(it);
            dec.reference_count += 1;
            if dec.reference_count > dec.max_references {
                return Err(resource(format!(
                    ": reference count exceeds {}",
                    dec.max_references
                )));
            }
            if ci.referenced_content_item_identifier.len() as i64 > dec.max_path_components {
                return Err(resource(format!(
                    ": reference path exceeds {} components",
                    dec.max_path_components
                )));
            }
        }
        let mut contexts = ContentItemCodeContexts {
            path: identifier_string(&item_path),
            ..Default::default()
        };
        let (name, ctx, eq, _) = decode_code_with_metadata(dec, o, tg::CONCEPT_NAME_CODE_SEQ);
        ci.concept_name = name;
        contexts.concept_name = ctx;
        contexts.concept_name_equivalent_codes = eq;
        let (children, present) = dec.sequence(o, tg::CONTENT_SEQUENCE);
        if present {
            ci.children = read_content_items(dec, &children, depth + 1, &item_path)
                .map_err(|e| e.prefixed(&format!("content item {index} children: ")))?;
        }
        if reference_present {
            if !code_contexts_empty(&contexts) {
                dec.add_code_contexts(contexts);
            }
            out.push(ci);
            continue;
        }
        match ci.value_type.as_str() {
            value::TEXT => ci.text = dec.string(o, tg::TEXT_VALUE),
            value::CODE => {
                let (code, ctx, eq, _) = decode_code_with_metadata(dec, o, tg::CONCEPT_CODE_SEQ);
                ci.code = code;
                contexts.value = ctx;
                contexts.value_equivalent_codes = eq;
            }
            value::IMAGE => ci.image = read_image_reference(dec, it),
            value::NUM => {
                if let Some((m, units, eq)) = read_measurement(dec, it) {
                    ci.measurement = Some(m);
                    contexts.measurement_units = units;
                    contexts.measurement_units_equivalent_codes = eq;
                }
                let (q, ctx, eq, _) =
                    decode_code_with_metadata(dec, o, tg::NUMERIC_VALUE_QUALIFIER_CODE_SEQ);
                ci.numeric_value_qualifier = q;
                contexts.numeric_value_qualifier = ctx;
                contexts.numeric_value_qualifier_equivalents = eq;
            }
            value::DATETIME => ci.date_time = dec.string(o, tg::DATE_TIME_VALUE),
            value::DATE => ci.date = dec.string(o, tg::DATE_VALUE),
            value::TIME => ci.time = dec.string(o, tg::TIME_VALUE),
            value::UIDREF => ci.uid = dec.string(o, tg::UID),
            value::PNAME => ci.person_name = dec.string(o, tg::PERSON_NAME_VALUE),
            value::COMPOSITE => ci.composite = read_composite_reference(dec, it),
            value::WAVEFORM => ci.waveform = read_waveform_reference(dec, it),
            value::SCOORD => {
                ci.spatial = read_spatial_coordinates(dec, it)
                    .map_err(|e| e.prefixed(&format!("content item {index}: ")))?;
            }
            value::SCOORD3D => {
                ci.spatial3d = super::measurement_report::read_spatial_reference(dec, it)
                    .map_err(|e| e.prefixed(&format!("content item {index}: ")))?;
            }
            value::TCOORD => ci.temporal = read_temporal_coordinates(dec, it),
            _ => {}
        }
        if !code_contexts_empty(&contexts) {
            dec.add_code_contexts(contexts);
        }
        if let Some(t) = decode_template_identification(dec, it, &item_path) {
            dec.add_template_identification(t);
        }
        out.push(ci);
    }
    Ok(out)
}
#[derive(Clone)]
pub(crate) struct EvidenceHierarchy {
    pub(crate) study_uid: String,
    pub(crate) series_uid: String,
}
pub(crate) fn read_current_requested_procedure_evidence(
    dec: &mut Decoder,
    obj: &Object,
) -> HashMap<String, EvidenceHierarchy> {
    let mut out = HashMap::new();
    let (studies, _) = dec.sequence(Some(obj), tg::CURRENT_REQUESTED_PROCEDURE_EVIDENCE);
    for study in &studies {
        let study_uid = dec.string(Some(study), tg::STUDY_INSTANCE_UID);
        let (series_items, _) = dec.sequence(Some(study), tg::REFERENCED_SERIES_SEQUENCE);
        for series in &series_items {
            let series_uid = dec.string(Some(series), tg::SERIES_INSTANCE_UID);
            let (sops, _) = dec.sequence(Some(series), tg::REF_SOP_SEQUENCE);
            for sop in &sops {
                let instance = dec.string(Some(sop), tg::REF_SOP_INSTANCE_UID);
                if !instance.is_empty() {
                    out.insert(
                        instance,
                        EvidenceHierarchy {
                            study_uid: study_uid.clone(),
                            series_uid: series_uid.clone(),
                        },
                    );
                }
            }
        }
    }
    out
}
fn apply_evidence_hierarchy(
    items: &mut [ContentItem],
    evidence: &HashMap<String, EvidenceHierarchy>,
) {
    for item in items {
        if item.value_type == value::IMAGE
            && let Some(h) = evidence.get(clean(&item.image.sop_instance_uid))
        {
            item.image.study_instance_uid = h.study_uid.clone();
            item.image.series_instance_uid = h.series_uid.clone();
        }
        apply_evidence_hierarchy(&mut item.children, evidence);
    }
}
pub(crate) fn decode_code(dec: &mut Decoder, obj: Option<&Object>, tag: Tag) -> (CodedEntry, bool) {
    let (code, _, _, ok) = decode_code_with_metadata(dec, obj, tag);
    (code, ok)
}
pub(crate) fn decode_code_with_metadata(
    dec: &mut Decoder,
    obj: Option<&Object>,
    tag: Tag,
) -> (CodedEntry, CodeContext, Vec<EquivalentCodeEntry>, bool) {
    let (items, ok) = dec.sequence(obj, tag);
    if !ok || items.is_empty() {
        return (
            CodedEntry::default(),
            CodeContext::default(),
            Vec::new(),
            false,
        );
    }
    let item = &items[0];
    let code = CodedEntry {
        code_value: dec.string(Some(item), tg::CODE_VALUE),
        coding_scheme_designator: dec.string(Some(item), tg::CODING_SCHEME),
        code_meaning: dec.string(Some(item), tg::CODE_MEANING),
    };
    let context = decode_code_context(item);
    let mut equivalents = Vec::new();
    if let Some(eq_items) = item.get_sequence(tg::EQUIVALENT_CODE_SEQUENCE) {
        if !dec.reserve_metadata_items(eq_items.len() as i64) {
            return (code, context, Vec::new(), true);
        }
        for e in &eq_items {
            equivalents.push(EquivalentCodeEntry {
                code: CodedEntry {
                    code_value: clean_string(e, tg::CODE_VALUE),
                    coding_scheme_designator: clean_string(e, tg::CODING_SCHEME),
                    code_meaning: clean_string(e, tg::CODE_MEANING),
                },
                context: decode_code_context(e),
            });
        }
    }
    (code, context, equivalents, true)
}
#[doc(hidden)]
pub fn read_code(obj: &Object, tag: Tag) -> (CodedEntry, bool) {
    decode_code(&mut Decoder::default(), Some(obj), tag)
}
pub(crate) fn read_image_reference(dec: &mut Decoder, it: &Object) -> ImageReference {
    let (items, ok) = dec.sequence(Some(it), tg::REF_SOP_SEQUENCE);
    if !ok || items.is_empty() {
        return ImageReference::default();
    }
    let r = &items[0];
    ImageReference {
        sop_class_uid: dec.string(Some(r), tg::REF_SOP_CLASS_UID),
        sop_instance_uid: dec.string(Some(r), tg::REF_SOP_INSTANCE_UID),
        frames: dec.ints(Some(r), tg::REF_FRAME_NUMBER),
        ..Default::default()
    }
}
fn read_composite_reference(dec: &mut Decoder, it: &Object) -> CompositeReference {
    let (items, ok) = dec.sequence(Some(it), tg::REF_SOP_SEQUENCE);
    if !ok || items.is_empty() {
        return CompositeReference::default();
    }
    CompositeReference {
        sop_class_uid: dec.string(Some(&items[0]), tg::REF_SOP_CLASS_UID),
        sop_instance_uid: dec.string(Some(&items[0]), tg::REF_SOP_INSTANCE_UID),
    }
}
fn read_waveform_reference(dec: &mut Decoder, it: &Object) -> WaveformReference {
    let (items, ok) = dec.sequence(Some(it), tg::REF_SOP_SEQUENCE);
    if !ok || items.is_empty() {
        return WaveformReference::default();
    }
    let r = &items[0];
    let mut out = WaveformReference {
        composite: CompositeReference {
            sop_class_uid: dec.string(Some(r), tg::REF_SOP_CLASS_UID),
            sop_instance_uid: dec.string(Some(r), tg::REF_SOP_INSTANCE_UID),
        },
        channels: Vec::new(),
    };
    let channels = dec.ints(Some(r), tg::REFERENCED_WAVEFORM_CH);
    let tag = tg::REFERENCED_WAVEFORM_CH;
    if !channels.len().is_multiple_of(2) {
        dec.fail(SrError::msg(format!(
            "decode {tag}: waveform channel list has {} values, want pairs",
            channels.len()
        )));
        return out;
    }
    for &v in &channels {
        if !(0..=65535).contains(&v) {
            dec.fail(SrError::msg(format!(
                "decode {tag}: waveform channel value {v} is outside US"
            )));
            return out;
        }
    }
    for pair in channels.chunks(2) {
        out.channels.push(WaveformChannel {
            multiplex_group_number: pair[0] as u16,
            channel_number: pair[1] as u16,
        });
    }
    out
}
fn read_spatial_coordinates(dec: &mut Decoder, it: &Object) -> Result<SpatialCoordinates, SrError> {
    let o = Some(it);
    let mut out = SpatialCoordinates {
        graphic_type: dec.string(o, tg::GRAPHIC_TYPE),
        pixel_origin_interpretation: dec.string(o, tg::PIXEL_ORIGIN_INTERPRET),
        fiducial_uid: dec.string(o, tg::FIDUCIAL_UID),
        coordinates: Vec::new(),
    };
    let values = dec.floats(o, tg::GRAPHIC_DATA);
    if !values.len().is_multiple_of(2) {
        return Err(SrError::new(
            Kind::InvalidGraphicData,
            &format!(
                ": SCOORD has {} values, want complete x/y pairs",
                values.len()
            ),
        ));
    }
    for p in values.chunks(2) {
        out.coordinates.push(Point2D { x: p[0], y: p[1] });
    }
    Ok(out)
}
fn read_temporal_coordinates(dec: &mut Decoder, it: &Object) -> TemporalCoordinates {
    let o = Some(it);
    let mut out = TemporalCoordinates {
        range_type: dec.string(o, tg::TEMPORAL_RANGE_TYPE),
        ..Default::default()
    };
    for v in dec.ints(o, tg::REFERENCED_SAMPLES) {
        if v >= 0 && v <= i64::from(u32::MAX) {
            out.sample_positions.push(v as u32);
        }
    }
    out.time_offsets = dec.floats(o, tg::REFERENCED_TIME_OFFSET);
    out.date_times = dec
        .strings(o, tg::REFERENCED_DATE_TIME)
        .iter()
        .map(|v| clean(v).to_string())
        .collect();
    out
}
fn unmodeled_value_elements(it: &Object, value_type: &str) -> Vec<Element> {
    let modeled = is_modeled_value_type(value_type);
    it.elements()
        .iter()
        .filter(|e| {
            let tag = e.tag();
            if [
                tg::RELATIONSHIP_TYPE,
                tg::VALUE_TYPE,
                tg::CONCEPT_NAME_CODE_SEQ,
                tg::CONTENT_SEQUENCE,
                tg::REFERENCED_CONTENT_ITEM_IDENTIFIER,
            ]
            .contains(&tag)
            {
                return false;
            }
            if modeled {
                if MODELED_VALUE_TAGS.contains(&tag) {
                    return false;
                }
                if value_type == value::CONTAINER && tag == tg::CONTINUITY_OF_CONTENT {
                    return false;
                }
            }
            true
        })
        .cloned()
        .collect()
}
const MODELED_VALUE_TAGS: [Tag; 19] = [
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
];
fn by_value_macro_present(item: &Object) -> bool {
    [
        tg::VALUE_TYPE,
        tg::CONCEPT_NAME_CODE_SEQ,
        tg::CONTENT_SEQUENCE,
        tg::CONTINUITY_OF_CONTENT,
    ]
    .iter()
    .chain(MODELED_VALUE_TAGS.iter())
    .any(|&t| item.has(t))
}
pub(crate) fn read_measurement(
    dec: &mut Decoder,
    it: &Object,
) -> Option<(Measurement, CodeContext, Vec<EquivalentCodeEntry>)> {
    let (items, ok) = dec.sequence(Some(it), tg::MEASURED_VALUE_SEQ);
    if !ok || items.is_empty() {
        return None;
    }
    let mv = &items[0];
    let values = dec.floats(Some(mv), tg::NUMERIC_VALUE);
    if values.is_empty() {
        return None;
    }
    let (units, ctx, eq, _) = decode_code_with_metadata(dec, Some(mv), tg::MEASUREMENT_UNITS_CODE);
    Some((
        Measurement {
            value: values[0],
            units,
        },
        ctx,
        eq,
    ))
}
pub(crate) fn clean_string(obj: &Object, tag: Tag) -> String {
    match obj.get_string(tag) {
        Some(v) => clean(&v).to_string(),
        None => String::new(),
    }
}
