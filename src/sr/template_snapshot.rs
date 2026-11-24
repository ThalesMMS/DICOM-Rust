use super::measurement_context::{
    MeasurementReferenceMetadata, MeasurementReferenceMetadataEntry,
    index_measurement_reference_metadata, measurement_reference_metadata_count,
    restore_measurement_reference_metadata,
};
use super::numeric_representation::{
    NumericRepresentation, NumericRepresentationMetadata, index_numeric_representations,
    read_document_numeric_representations, restore_numeric_representations,
};
use super::read::{ReadOptions, ReadResult, read_document_with_options};
use super::reference::{ValidationReport, append_path, identifier_string};
use super::render::{
    PresentationDocument, RenderOptions, RenderedDocument, html_of, presentation_html_of,
    presentation_text_of, render_items_with_contexts, render_presentation_with_contexts, text_of,
};
use super::template_metadata::{
    ContentItemCodeContexts, MetadataIndex, TEMPLATE_CODE_CODE_CONTEXT,
    TEMPLATE_CODE_TEMPLATE_IDENTIFICATION, TemplateIdentification, TemplateValidationMetadata,
    apply_code_contexts, new_metadata_index, template_validation_error,
};
use super::template_registry::{TemplateKey, TemplateValidator};
use super::{ContentItem, Document, Element, Kind, SrError, value};
use crate::object::Object;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
pub const METADATA_SNAPSHOT_STALE: &str = "stale";
pub const METADATA_SNAPSHOT_AMBIGUOUS: &str = "ambiguous-association";
pub const METADATA_SNAPSHOT_LIMIT: &str = "resource-limit";
pub const METADATA_SNAPSHOT_UNSUPPORTED: &str = "unsupported-value";
pub fn metadata_snapshot_error(code: &str) -> SrError {
    SrError::coded_path(Kind::MetadataSnapshot, code, "", METADATA_SNAPSHOT_LIMIT)
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MetadataSnapshotOptions {
    pub max_depth: i64,
    pub max_items: i64,
    pub max_value_bytes: i64,
}
pub fn default_metadata_snapshot_options() -> MetadataSnapshotOptions {
    MetadataSnapshotOptions {
        max_depth: 64,
        max_items: 100_000,
        max_value_bytes: 64 << 20,
    }
}
pub(crate) type Fingerprint = [u8; 32];
#[derive(Clone, Debug)]
pub struct TemplateMetadataSnapshot {
    metadata: TemplateValidationMetadata,
    numeric: NumericRepresentationMetadata,
    measurement_references: MeasurementReferenceMetadata,
    code: HashMap<String, ContentItemCodeContexts>,
    identification: HashMap<String, TemplateIdentification>,
    numeric_index: HashMap<String, NumericRepresentation>,
    reference_index: HashMap<String, MeasurementReferenceMetadataEntry>,
    options: MetadataSnapshotOptions,
    fingerprint: Fingerprint,
    nodes: Vec<Fingerprint>,
}
struct Budget {
    remaining: i64,
}
impl Budget {
    fn write(&mut self, h: &mut Sha256, p: &[u8]) -> Result<(), SrError> {
        if p.len() as i64 > self.remaining {
            return Err(metadata_snapshot_error(METADATA_SNAPSHOT_LIMIT));
        }
        self.remaining -= p.len() as i64;
        h.update(p);
        Ok(())
    }
    fn value<T: std::fmt::Debug>(&mut self, h: &mut Sha256, v: &T) -> Result<(), SrError> {
        let s = format!("{v:?}");
        self.write(h, &(s.len() as u64).to_be_bytes())?;
        self.write(h, s.as_bytes())
    }
}
fn digest(h: Sha256) -> Fingerprint {
    h.finalize().into()
}
fn metadata_count(m: &TemplateValidationMetadata, max: i64) -> Option<i64> {
    let mut count = 0i64;
    let mut add = |n: usize| {
        if n as i64 > max - count {
            return false;
        }
        count += n as i64;
        true
    };
    if !add(m.code_contexts.len()) || !add(m.template_identifications.len()) {
        return None;
    }
    for c in &m.code_contexts {
        if !add(c.concept_name_equivalent_codes.len())
            || !add(c.value_equivalent_codes.len())
            || !add(c.measurement_units_equivalent_codes.len())
            || !add(c.numeric_value_qualifier_equivalents.len())
        {
            return None;
        }
    }
    Some(count)
}
pub fn bind_template_metadata(
    doc: Option<&Document>,
    metadata: &TemplateValidationMetadata,
    options: MetadataSnapshotOptions,
) -> Result<TemplateMetadataSnapshot, SrError> {
    bind_measurement_snapshot(
        doc,
        metadata,
        &NumericRepresentationMetadata::default(),
        &MeasurementReferenceMetadata::default(),
        options,
    )
}
pub fn bind_numeric_representations(
    doc: Option<&Document>,
    metadata: &TemplateValidationMetadata,
    numeric: &NumericRepresentationMetadata,
    options: MetadataSnapshotOptions,
) -> Result<TemplateMetadataSnapshot, SrError> {
    bind_measurement_snapshot(
        doc,
        metadata,
        numeric,
        &MeasurementReferenceMetadata::default(),
        options,
    )
}
pub(crate) fn bind_measurement_snapshot(
    doc: Option<&Document>,
    metadata: &TemplateValidationMetadata,
    numeric: &NumericRepresentationMetadata,
    references: &MeasurementReferenceMetadata,
    mut options: MetadataSnapshotOptions,
) -> Result<TemplateMetadataSnapshot, SrError> {
    let Some(doc) = doc else {
        return Err(SrError::new(Kind::NilDocument, ""));
    };
    let d = default_metadata_snapshot_options();
    if options.max_depth == 0 {
        options.max_depth = d.max_depth;
    }
    if options.max_items == 0 {
        options.max_items = d.max_items;
    }
    if options.max_value_bytes == 0 {
        options.max_value_bytes = d.max_value_bytes;
    }
    let limit = || metadata_snapshot_error(METADATA_SNAPSHOT_LIMIT);
    if options.max_depth < 1
        || options.max_depth > 64
        || options.max_items < 1
        || options.max_value_bytes < 1
    {
        return Err(limit());
    }
    let count = match metadata_count(metadata, options.max_items) {
        Some(c) if numeric.entries.len() as i64 <= options.max_items - c => c,
        _ => return Err(limit()),
    };
    if !measurement_reference_metadata_count(
        references,
        options.max_items - count - numeric.entries.len() as i64,
    ) {
        return Err(limit());
    }
    let mut index = new_metadata_index(metadata)?;
    let numeric_index =
        index_numeric_representations(&doc.content, numeric, options.max_items, options.max_depth)?;
    let reference_index = index_measurement_reference_metadata(
        &doc.content,
        references,
        options.max_items,
        options.max_depth,
    )?;
    validate_snapshot_metadata(doc, &mut index, options)?;
    let mut budget = Budget {
        remaining: options.max_value_bytes,
    };
    hash_metadata(metadata, numeric, references, &mut budget)?;
    let mut snapshot = TemplateMetadataSnapshot {
        metadata: metadata.clone(),
        numeric: numeric.clone(),
        measurement_references: references.clone(),
        code: index.code,
        identification: index.identification,
        numeric_index,
        reference_index,
        options,
        fingerprint: [0; 32],
        nodes: Vec::new(),
    };
    let (proof, nodes) = document_proof(doc, &snapshot, true, &mut budget)?;
    snapshot.fingerprint = proof;
    snapshot.nodes = nodes;
    Ok(snapshot)
}
pub(crate) fn plain_document_proof(
    doc: &Document,
    options: MetadataSnapshotOptions,
) -> Result<Fingerprint, SrError> {
    let s = TemplateMetadataSnapshot {
        metadata: Default::default(),
        numeric: Default::default(),
        measurement_references: Default::default(),
        code: HashMap::new(),
        identification: HashMap::new(),
        numeric_index: HashMap::new(),
        reference_index: HashMap::new(),
        options,
        fingerprint: [0; 32],
        nodes: Vec::new(),
    };
    let mut budget = Budget {
        remaining: options.max_value_bytes,
    };
    document_proof(doc, &s, false, &mut budget).map(|r| r.0)
}
pub(crate) fn value_digest<T: std::fmt::Debug>(v: &T, max_value_bytes: i64) -> Result<(), SrError> {
    let mut budget = Budget {
        remaining: max_value_bytes,
    };
    let mut h = Sha256::new();
    budget.value(&mut h, v)
}
fn hash_metadata(
    metadata: &TemplateValidationMetadata,
    numeric: &NumericRepresentationMetadata,
    references: &MeasurementReferenceMetadata,
    budget: &mut Budget,
) -> Result<(), SrError> {
    let mut h = Sha256::new();
    budget.value(&mut h, metadata)?;
    if !numeric.entries.is_empty() {
        budget.value(&mut h, numeric)?;
    }
    if !references.entries.is_empty() {
        budget.value(&mut h, references)?;
    }
    Ok(())
}
pub fn read_document_with_metadata_snapshot(
    obj: Option<&Object>,
    read_options: ReadOptions,
    snapshot_options: MetadataSnapshotOptions,
) -> (
    ReadResult,
    Option<TemplateMetadataSnapshot>,
    Option<SrError>,
) {
    let (result, err) = read_document_with_options(obj, read_options);
    if let Some(e) = err {
        return (result, None, Some(e));
    }
    match bind_template_metadata(
        result.document.as_ref(),
        &result.template_metadata,
        snapshot_options,
    ) {
        Ok(s) => (result, Some(s), None),
        Err(e) => (result, None, Some(e)),
    }
}
pub fn read_document_with_numeric_representations(
    obj: Option<&Object>,
    read_options: ReadOptions,
    snapshot_options: MetadataSnapshotOptions,
) -> (
    ReadResult,
    Option<TemplateMetadataSnapshot>,
    Option<SrError>,
) {
    let (result, numeric, err) = read_document_numeric_representations(obj, read_options);
    if let Some(e) = err {
        return (result, None, Some(e));
    }
    match bind_numeric_representations(
        result.document.as_ref(),
        &result.template_metadata,
        &numeric,
        snapshot_options,
    ) {
        Ok(s) => (result, Some(s), None),
        Err(e) => (result, None, Some(e)),
    }
}
impl TemplateMetadataSnapshot {
    pub fn metadata(&self) -> TemplateValidationMetadata {
        self.metadata.clone()
    }
    pub fn numeric_representations(&self) -> NumericRepresentationMetadata {
        self.numeric.clone()
    }
    pub fn measurement_references(&self) -> MeasurementReferenceMetadata {
        self.measurement_references.clone()
    }
    pub(crate) fn code_index(&self) -> &HashMap<String, ContentItemCodeContexts> {
        &self.code
    }
    pub(crate) fn identification_index(&self) -> &HashMap<String, TemplateIdentification> {
        &self.identification
    }
    pub(crate) fn numeric_index(&self) -> &HashMap<String, NumericRepresentation> {
        &self.numeric_index
    }
    pub(crate) fn reference_index(&self) -> &HashMap<String, MeasurementReferenceMetadataEntry> {
        &self.reference_index
    }
    pub fn check(&self, doc: Option<&Document>) -> Result<(), SrError> {
        let Some(doc) = doc else {
            return Err(metadata_snapshot_error(METADATA_SNAPSHOT_STALE));
        };
        let mut budget = Budget {
            remaining: self.options.max_value_bytes,
        };
        hash_metadata(
            &self.metadata,
            &self.numeric,
            &self.measurement_references,
            &mut budget,
        )?;
        let (proof, nodes) = document_proof(doc, self, false, &mut budget)?;
        if proof != self.fingerprint || nodes != self.nodes {
            return Err(metadata_snapshot_error(METADATA_SNAPSHOT_STALE));
        }
        Ok(())
    }
}
fn check(s: Option<&TemplateMetadataSnapshot>, doc: Option<&Document>) -> Result<(), SrError> {
    match s {
        Some(s) => s.check(doc),
        None => Err(metadata_snapshot_error(METADATA_SNAPSHOT_STALE)),
    }
}
impl Document {
    pub fn elements_with_metadata_snapshot(
        &self,
        s: Option<&TemplateMetadataSnapshot>,
    ) -> Result<Vec<Element>, SrError> {
        check(s, Some(self))?;
        let s = s.expect("checked");
        let mut elements = self.elements_with_template_metadata(&s.metadata)?;
        restore_numeric_representations(&mut elements, &self.content, &[1], &s.numeric_index)?;
        restore_measurement_reference_metadata(
            &mut elements,
            &self.content,
            &[1],
            &s.reference_index,
        )?;
        Ok(elements)
    }
    pub fn dataset_with_metadata_snapshot(
        &self,
        s: Option<&TemplateMetadataSnapshot>,
    ) -> Result<Object, SrError> {
        Ok(Object::from_elements(
            self.elements_with_metadata_snapshot(s)?,
        ))
    }
}
pub fn validate_with_metadata_snapshot(
    v: &TemplateValidator,
    d: Option<&Document>,
    key: &TemplateKey,
    s: Option<&TemplateMetadataSnapshot>,
) -> (ValidationReport, Option<SrError>) {
    if let Err(e) = check(s, d) {
        return (ValidationReport::default(), Some(e));
    }
    v.validate_with_metadata(d, key, &s.expect("checked").metadata)
}
pub fn render_items_with_metadata_snapshot(
    doc: Option<&Document>,
    snapshot: Option<&TemplateMetadataSnapshot>,
    opts: &RenderOptions,
) -> Result<RenderedDocument, SrError> {
    let Some(s) = snapshot else {
        return render_items_with_contexts(doc, opts, None);
    };
    s.check(doc)?;
    render_items_with_contexts(doc, opts, Some(&s.code))
}
pub fn render_text_with_metadata_snapshot(
    doc: Option<&Document>,
    snapshot: Option<&TemplateMetadataSnapshot>,
    opts: &RenderOptions,
) -> Result<String, SrError> {
    Ok(text_of(&render_items_with_metadata_snapshot(
        doc, snapshot, opts,
    )?))
}
pub fn render_html_with_metadata_snapshot(
    doc: Option<&Document>,
    snapshot: Option<&TemplateMetadataSnapshot>,
    opts: &RenderOptions,
) -> Result<String, SrError> {
    Ok(html_of(&render_items_with_metadata_snapshot(
        doc, snapshot, opts,
    )?))
}
pub fn render_presentation_with_metadata_snapshot(
    doc: Option<&Document>,
    snapshot: Option<&TemplateMetadataSnapshot>,
    opts: &RenderOptions,
) -> Result<PresentationDocument, SrError> {
    let Some(s) = snapshot else {
        return render_presentation_with_contexts(doc, opts, None);
    };
    s.check(doc)?;
    render_presentation_with_contexts(doc, opts, Some(&s.code))
}
pub fn render_presentation_text_with_metadata_snapshot(
    doc: Option<&Document>,
    snapshot: Option<&TemplateMetadataSnapshot>,
    opts: &RenderOptions,
) -> Result<String, SrError> {
    Ok(presentation_text_of(
        &render_presentation_with_metadata_snapshot(doc, snapshot, opts)?,
    ))
}
pub fn render_presentation_html_with_metadata_snapshot(
    doc: Option<&Document>,
    snapshot: Option<&TemplateMetadataSnapshot>,
    opts: &RenderOptions,
) -> Result<String, SrError> {
    Ok(presentation_html_of(
        &render_presentation_with_metadata_snapshot(doc, snapshot, opts)?,
    ))
}
fn validate_snapshot_metadata(
    doc: &Document,
    index: &mut MetadataIndex,
    options: MetadataSnapshotOptions,
) -> Result<(), SrError> {
    let root = "1".to_string();
    let contexts = index.code.get(&root).cloned();
    let mut elements = Vec::new();
    apply_code_contexts(
        &mut elements,
        &doc.title,
        None,
        &contexts.clone().unwrap_or_default(),
    )?;
    if contexts.is_some() {
        index.used_code.insert(root.clone());
    }
    if index.identification.contains_key(&root) {
        index.used_identification.insert(root.clone());
    }
    fn visit(
        items: &[ContentItem],
        parent: &[u32],
        depth: i64,
        count: &mut i64,
        index: &mut MetadataIndex,
        options: MetadataSnapshotOptions,
    ) -> Result<(), SrError> {
        if items.is_empty() {
            return Ok(());
        }
        let limit = || metadata_snapshot_error(METADATA_SNAPSHOT_LIMIT);
        if depth > options.max_depth || items.len() as i64 > options.max_items - *count {
            return Err(limit());
        }
        for (i, item) in items.iter().enumerate() {
            *count += 1;
            if *count > options.max_items {
                return Err(limit());
            }
            let path = append_path(parent, i as u32 + 1);
            let key = identifier_string(&path);
            let contexts = index.code.get(&key).cloned();
            let mut elements = Vec::new();
            apply_code_contexts(
                &mut elements,
                &item.concept_name,
                Some(item),
                &contexts.clone().unwrap_or_default(),
            )?;
            if contexts.is_some() {
                index.used_code.insert(key.clone());
            }
            if index.identification.contains_key(&key) {
                if item.value_type != value::CONTAINER {
                    return Err(template_validation_error(
                        TEMPLATE_CODE_TEMPLATE_IDENTIFICATION,
                    ));
                }
                index.used_identification.insert(key.clone());
            }
            visit(&item.children, &path, depth + 1, count, index, options)?;
        }
        Ok(())
    }
    let mut count = 0i64;
    visit(&doc.content, &[1], 1, &mut count, index, options)?;
    if index.used_code.len() != index.code.len()
        || index.used_identification.len() != index.identification.len()
    {
        return Err(template_validation_error(TEMPLATE_CODE_CODE_CONTEXT));
    }
    Ok(())
}
fn document_proof(
    doc: &Document,
    s: &TemplateMetadataSnapshot,
    detect_ambiguity: bool,
    budget: &mut Budget,
) -> Result<(Fingerprint, Vec<Fingerprint>), SrError> {
    struct State<'a> {
        s: &'a TemplateMetadataSnapshot,
        detect: bool,
        nodes: Vec<Fingerprint>,
        seen: HashMap<Fingerprint, Fingerprint>,
    }
    fn visit(
        st: &mut State,
        items: &[ContentItem],
        parent: &[u32],
        depth: i64,
        budget: &mut Budget,
    ) -> Result<(Fingerprint, Fingerprint), SrError> {
        let options = st.s.options;
        let limit = || metadata_snapshot_error(METADATA_SNAPSHOT_LIMIT);
        let mut h = Sha256::new();
        let mut mh = Sha256::new();
        if !items.is_empty()
            && (depth > options.max_depth
                || items.len() as i64 > options.max_items - st.nodes.len() as i64)
        {
            return Err(limit());
        }
        budget.write(&mut h, &(items.len() as u64).to_be_bytes())?;
        for (i, item) in items.iter().enumerate() {
            if st.nodes.len() as i64 >= options.max_items {
                return Err(limit());
            }
            let position = st.nodes.len();
            st.nodes.push([0; 32]);
            let path = append_path(parent, i as u32 + 1);
            let key = identifier_string(&path);
            let mut value = item.clone();
            value.children = Vec::new();
            let mut ih = Sha256::new();
            budget.value(&mut ih, &value)?;
            let (child_hash, child_metadata) = visit(st, &item.children, &path, depth + 1, budget)?;
            budget.write(&mut ih, &child_hash)?;
            let item_hash = digest(ih);
            st.nodes[position] = item_hash;
            budget.write(&mut h, &item_hash)?;
            if st.detect {
                let mut contexts = st.s.code.get(&key).cloned().unwrap_or_default();
                contexts.path.clear();
                let mut identification = st.s.identification.get(&key).cloned().unwrap_or_default();
                identification.path.clear();
                let mut imh = Sha256::new();
                budget.value(&mut imh, &contexts)?;
                budget.value(&mut imh, &identification)?;
                if !st.s.numeric_index.is_empty() {
                    let r = st.s.numeric_index.get(&key);
                    budget.value(&mut imh, &r.is_some())?;
                    budget.value(&mut imh, &r.cloned().unwrap_or_default())?;
                }
                if !st.s.reference_index.is_empty() {
                    let r = st.s.reference_index.get(&key);
                    let mut reference = r.cloned().unwrap_or_default();
                    reference.path.clear();
                    budget.value(&mut imh, &r.is_some())?;
                    budget.value(&mut imh, &reference)?;
                }
                budget.write(&mut imh, &child_metadata)?;
                let item_metadata = digest(imh);
                if let Some(previous) = st.seen.get(&item_hash)
                    && *previous != item_metadata
                {
                    return Err(metadata_snapshot_error(METADATA_SNAPSHOT_AMBIGUOUS));
                }
                st.seen.insert(item_hash, item_metadata);
                budget.write(&mut mh, &item_metadata)?;
            }
        }
        Ok((digest(h), digest(mh)))
    }
    let mut st = State {
        s,
        detect: detect_ambiguity,
        nodes: Vec::new(),
        seen: HashMap::new(),
    };
    let (content, _) = visit(&mut st, &doc.content, &[1], 1, budget)?;
    let mut h = Sha256::new();
    budget.value(&mut h, &doc.sop_class_uid)?;
    budget.value(&mut h, &doc.title)?;
    budget.value(&mut h, &doc.continuity_of_content)?;
    budget.write(&mut h, &content)?;
    Ok((digest(h), st.nodes))
}
