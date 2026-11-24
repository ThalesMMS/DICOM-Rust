use super::read::{Decoder, clean_string};
use super::reference::{ContentItemIdentifier, append_path, identifier_string};
use super::tags as tg;
use super::{
    CodedEntry, ContentItem, DataSet, Document, Kind, SrError, derivedio, seq_element, str_elem,
    value,
};
use crate::object::{Element, Object, Tag, Value, Vr};
use std::collections::{HashMap, HashSet};
pub(crate) const MAX_REGISTRY_STRING: usize = 2048;
pub(crate) const DEFAULT_TEMPLATE_MAX_ITEMS: usize = 100_000;
pub const TEMPLATE_CODE_TEMPLATE_NOT_FOUND: &str = "sr.template.not_found";
pub const TEMPLATE_CODE_CARDINALITY: &str = "sr.template.cardinality";
pub const TEMPLATE_CODE_UNKNOWN_CONTENT: &str = "sr.template.unknown_content";
pub const TEMPLATE_CODE_ORDER: &str = "sr.template.order";
pub const TEMPLATE_CODE_CONDITION_UNSUPPORTED: &str = "sr.template.condition_unsupported";
pub const TEMPLATE_CODE_CONDITION_FAILED: &str = "sr.template.condition_failed";
pub const TEMPLATE_CODE_REFERENCE_MODE: &str = "sr.template.reference_mode";
pub const TEMPLATE_CODE_CONTEXT_GROUP: &str = "sr.template.context_group";
pub const TEMPLATE_CODE_CODE_CONTEXT: &str = "sr.template.code_context";
pub const TEMPLATE_CODE_TEMPLATE_IDENTIFICATION: &str = "sr.template.identification";
pub const TEMPLATE_CODE_RESOURCE_LIMIT: &str = "sr.template.resource_limit";
pub fn template_validation_error(code: &str) -> SrError {
    SrError::coded(Kind::TemplateValidation, code)
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct CodeKey {
    pub code_value: String,
    pub long_code_value: String,
    pub urn_code_value: String,
    pub coding_scheme_designator: String,
    pub coding_scheme_version: String,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct CodeContext {
    pub coding_scheme_version: String,
    pub context_identifier: String,
    pub mapping_resource: String,
    pub context_group_version: String,
    pub context_group_local_version: String,
    pub extension_flag: String,
    pub extension_creator_uid: String,
    pub context_uid: String,
    pub mapping_resource_uid: String,
    pub mapping_resource_name: String,
    pub long_code_value: String,
    pub urn_code_value: String,
}
impl CodeContext {
    fn is_empty(&self) -> bool {
        *self == CodeContext::default()
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EquivalentCodeEntry {
    pub code: CodedEntry,
    pub context: CodeContext,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ContentItemCodeContexts {
    pub path: String,
    pub concept_name: CodeContext,
    pub concept_name_equivalent_codes: Vec<EquivalentCodeEntry>,
    pub value: CodeContext,
    pub value_equivalent_codes: Vec<EquivalentCodeEntry>,
    pub measurement_units: CodeContext,
    pub measurement_units_equivalent_codes: Vec<EquivalentCodeEntry>,
    pub numeric_value_qualifier: CodeContext,
    pub numeric_value_qualifier_equivalents: Vec<EquivalentCodeEntry>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TemplateIdentification {
    pub path: String,
    pub mapping_resource: String,
    pub mapping_resource_uid: String,
    pub identifier: String,
    pub version: String,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TemplateValidationMetadata {
    pub code_contexts: Vec<ContentItemCodeContexts>,
    pub template_identifications: Vec<TemplateIdentification>,
}
pub(crate) fn code_key_for_entry(entry: &CodedEntry, context: &CodeContext) -> CodeKey {
    CodeKey {
        code_value: entry.code_value.clone(),
        long_code_value: context.long_code_value.clone(),
        urn_code_value: context.urn_code_value.clone(),
        coding_scheme_designator: entry.coding_scheme_designator.clone(),
        coding_scheme_version: context.coding_scheme_version.clone(),
    }
}
fn valid_registry_part(v: &str) -> bool {
    !v.is_empty() && v.len() <= MAX_REGISTRY_STRING
}
pub(crate) fn valid_code_key(code: &CodeKey) -> bool {
    let mut count = 0;
    for v in [
        &code.code_value,
        &code.long_code_value,
        &code.urn_code_value,
    ] {
        if !v.is_empty() {
            count += 1;
            if v.len() > MAX_REGISTRY_STRING {
                return false;
            }
        }
    }
    if count != 1 || code.coding_scheme_version.len() > MAX_REGISTRY_STRING {
        return false;
    }
    if !code.code_value.is_empty() && code.code_value.chars().count() > 16 {
        return false;
    }
    if !code.long_code_value.is_empty() && code.long_code_value.chars().count() <= 16 {
        return false;
    }
    if !code.urn_code_value.is_empty() {
        if code.coding_scheme_designator.is_empty() {
            return code.coding_scheme_version.is_empty();
        }
        return valid_registry_part(&code.coding_scheme_designator);
    }
    valid_registry_part(&code.coding_scheme_designator)
}
pub(crate) fn valid_code_context_shape(c: &CodeContext) -> bool {
    for v in [
        &c.coding_scheme_version,
        &c.context_identifier,
        &c.mapping_resource,
        &c.context_group_version,
        &c.context_group_local_version,
        &c.extension_flag,
        &c.extension_creator_uid,
        &c.context_uid,
        &c.mapping_resource_uid,
        &c.mapping_resource_name,
        &c.long_code_value,
        &c.urn_code_value,
    ] {
        if v.len() > MAX_REGISTRY_STRING {
            return false;
        }
    }
    if !c.long_code_value.is_empty() && !c.urn_code_value.is_empty() {
        return false;
    }
    let group = !c.context_identifier.is_empty()
        || !c.mapping_resource.is_empty()
        || !c.context_group_version.is_empty()
        || !c.context_group_local_version.is_empty()
        || !c.extension_flag.is_empty()
        || !c.extension_creator_uid.is_empty();
    if group
        && (c.context_identifier.is_empty()
            || c.mapping_resource.is_empty()
            || c.context_group_version.is_empty())
    {
        return false;
    }
    if !c.extension_flag.is_empty() && c.extension_flag != "Y" && c.extension_flag != "N" {
        return false;
    }
    if c.extension_flag == "Y" {
        return !c.context_group_local_version.is_empty() && !c.extension_creator_uid.is_empty();
    }
    c.context_group_local_version.is_empty() && c.extension_creator_uid.is_empty()
}
pub(crate) fn valid_template_identification(t: &TemplateIdentification) -> bool {
    if t.path.is_empty() || t.path.len() > MAX_REGISTRY_STRING {
        return false;
    }
    [
        &t.mapping_resource,
        &t.mapping_resource_uid,
        &t.identifier,
        &t.version,
    ]
    .iter()
    .all(|v| v.len() <= MAX_REGISTRY_STRING)
}
pub(crate) fn code_contexts_empty(c: &ContentItemCodeContexts) -> bool {
    c.concept_name.is_empty()
        && c.concept_name_equivalent_codes.is_empty()
        && c.value.is_empty()
        && c.value_equivalent_codes.is_empty()
        && c.measurement_units.is_empty()
        && c.measurement_units_equivalent_codes.is_empty()
        && c.numeric_value_qualifier.is_empty()
        && c.numeric_value_qualifier_equivalents.is_empty()
}
pub(crate) fn decode_code_context(item: &Object) -> CodeContext {
    CodeContext {
        coding_scheme_version: clean_string(item, tg::CODING_SCHEME_VERSION),
        context_identifier: clean_string(item, tg::CONTEXT_IDENTIFIER),
        mapping_resource: clean_string(item, tg::MAPPING_RESOURCE),
        context_group_version: clean_string(item, tg::CONTEXT_GROUP_VERSION),
        context_group_local_version: clean_string(item, tg::CONTEXT_GROUP_LOCAL_VERSION),
        extension_flag: clean_string(item, tg::CONTEXT_GROUP_EXTENSION_FLAG),
        extension_creator_uid: clean_string(item, tg::CONTEXT_GROUP_EXTENSION_CREATOR_UID),
        context_uid: clean_string(item, tg::CONTEXT_UID),
        mapping_resource_uid: clean_string(item, tg::MAPPING_RESOURCE_UID),
        mapping_resource_name: clean_string(item, tg::MAPPING_RESOURCE_NAME),
        long_code_value: clean_string(item, tg::LONG_CODE_VALUE),
        urn_code_value: clean_string(item, tg::URN_CODE_VALUE),
    }
}
pub(crate) fn decode_template_identification(
    _dec: &mut Decoder,
    obj: &Object,
    path: &[u32],
) -> Option<TemplateIdentification> {
    let items = obj.get_sequence(tg::CONTENT_TEMPLATE_SEQUENCE)?;
    let item = items.first()?;
    Some(TemplateIdentification {
        path: identifier_string(path),
        mapping_resource: clean_string(item, tg::MAPPING_RESOURCE),
        mapping_resource_uid: clean_string(item, tg::MAPPING_RESOURCE_UID),
        identifier: clean_string(item, tg::TEMPLATE_IDENTIFIER),
        version: clean_string(item, tg::TEMPLATE_VERSION),
    })
}
pub(crate) struct MetadataIndex {
    pub(crate) code: HashMap<String, ContentItemCodeContexts>,
    pub(crate) identification: HashMap<String, TemplateIdentification>,
    pub(crate) used_code: HashSet<String>,
    pub(crate) used_identification: HashSet<String>,
}
pub(crate) fn metadata_item_count(m: &TemplateValidationMetadata, limit: usize) -> bool {
    let mut count = 0usize;
    let mut add = |n: usize| {
        if n > limit - count {
            return false;
        }
        count += n;
        true
    };
    if !add(m.code_contexts.len()) || !add(m.template_identifications.len()) {
        return false;
    }
    for c in &m.code_contexts {
        if !add(c.concept_name_equivalent_codes.len())
            || !add(c.value_equivalent_codes.len())
            || !add(c.measurement_units_equivalent_codes.len())
            || !add(c.numeric_value_qualifier_equivalents.len())
        {
            return false;
        }
    }
    true
}
pub(crate) fn valid_equivalents(entries: &[EquivalentCodeEntry]) -> bool {
    entries.iter().all(|e| {
        valid_code_context_shape(&e.context)
            && valid_code_key(&code_key_for_entry(&e.code, &e.context))
    })
}
pub(crate) fn new_metadata_index(m: &TemplateValidationMetadata) -> Result<MetadataIndex, SrError> {
    if !metadata_item_count(m, DEFAULT_TEMPLATE_MAX_ITEMS) {
        return Err(template_validation_error(TEMPLATE_CODE_RESOURCE_LIMIT));
    }
    let mut index = MetadataIndex {
        code: HashMap::new(),
        identification: HashMap::new(),
        used_code: HashSet::new(),
        used_identification: HashSet::new(),
    };
    for c in &m.code_contexts {
        if c.path.is_empty()
            || c.path.len() > MAX_REGISTRY_STRING
            || code_contexts_empty(c)
            || !valid_code_context_shape(&c.concept_name)
            || !valid_code_context_shape(&c.value)
            || !valid_code_context_shape(&c.measurement_units)
            || !valid_code_context_shape(&c.numeric_value_qualifier)
            || !valid_equivalents(&c.concept_name_equivalent_codes)
            || !valid_equivalents(&c.value_equivalent_codes)
            || !valid_equivalents(&c.measurement_units_equivalent_codes)
            || !valid_equivalents(&c.numeric_value_qualifier_equivalents)
            || index.code.contains_key(&c.path)
        {
            return Err(template_validation_error(TEMPLATE_CODE_CODE_CONTEXT));
        }
        index.code.insert(c.path.clone(), c.clone());
    }
    for t in &m.template_identifications {
        if !valid_template_identification(t)
            || t.mapping_resource.is_empty()
            || t.identifier.is_empty()
            || index.identification.contains_key(&t.path)
        {
            return Err(template_validation_error(
                TEMPLATE_CODE_TEMPLATE_IDENTIFICATION,
            ));
        }
        index.identification.insert(t.path.clone(), t.clone());
    }
    Ok(index)
}
impl Document {
    pub fn elements_with_template_metadata(
        &self,
        metadata: &TemplateValidationMetadata,
    ) -> Result<Vec<Element>, SrError> {
        let mut elements = self.elements()?;
        let mut index = new_metadata_index(metadata)?;
        let root = "1".to_string();
        let contexts = index.code.get(&root).cloned().unwrap_or_default();
        apply_code_contexts(&mut elements, &self.title, None, &contexts)?;
        if index.code.contains_key(&root) {
            index.used_code.insert(root.clone());
        }
        if let Some(t) = index.identification.get(&root).cloned() {
            apply_template_identification(&mut elements, &t)?;
            index.used_identification.insert(root.clone());
        }
        apply_content_metadata(&mut elements, &self.content, &[1], &mut index)?;
        if index.used_code.len() != index.code.len()
            || index.used_identification.len() != index.identification.len()
        {
            return Err(template_validation_error(TEMPLATE_CODE_CODE_CONTEXT));
        }
        Ok(elements)
    }
    pub fn dataset_with_template_metadata(
        &self,
        metadata: &TemplateValidationMetadata,
    ) -> Result<Object, SrError> {
        Ok(Object::from_elements(
            self.elements_with_template_metadata(metadata)?,
        ))
    }
}
fn apply_content_metadata(
    elements: &mut [Element],
    items: &[ContentItem],
    parent: &[u32],
    index: &mut MetadataIndex,
) -> Result<(), SrError> {
    let seq_index = element_index(elements, tg::CONTENT_SEQUENCE);
    if items.is_empty() {
        return Ok(());
    }
    let Some(seq_index) = seq_index else {
        return Err(template_validation_error(TEMPLATE_CODE_CODE_CONTEXT));
    };
    let Value::Sequence(seq_items) = &mut elements[seq_index].value else {
        return Err(template_validation_error(TEMPLATE_CODE_CODE_CONTEXT));
    };
    if seq_items.len() != items.len() {
        return Err(template_validation_error(TEMPLATE_CODE_CODE_CONTEXT));
    }
    for (i, item) in items.iter().enumerate() {
        let path: ContentItemIdentifier = append_path(parent, i as u32 + 1);
        let key = identifier_string(&path);
        let data_set: &mut DataSet = &mut seq_items[i];
        let contexts = index.code.get(&key).cloned();
        apply_code_contexts(
            &mut data_set.elements,
            &item.concept_name,
            Some(item),
            &contexts.clone().unwrap_or_default(),
        )?;
        if contexts.is_some() {
            index.used_code.insert(key.clone());
        }
        if let Some(t) = index.identification.get(&key).cloned() {
            if item.value_type != value::CONTAINER {
                return Err(template_validation_error(
                    TEMPLATE_CODE_TEMPLATE_IDENTIFICATION,
                ));
            }
            apply_template_identification(&mut data_set.elements, &t)?;
            index.used_identification.insert(key.clone());
        }
        apply_content_metadata(&mut data_set.elements, &item.children, &path, index)?;
    }
    Ok(())
}
pub(crate) fn apply_code_contexts(
    elements: &mut Vec<Element>,
    concept_name: &CodedEntry,
    item: Option<&ContentItem>,
    c: &ContentItemCodeContexts,
) -> Result<(), SrError> {
    let err = || template_validation_error(TEMPLATE_CODE_CODE_CONTEXT);
    if !c.concept_name.is_empty() || !c.concept_name_equivalent_codes.is_empty() {
        if concept_name.empty() {
            return Err(err());
        }
        let s = code_sequence_with_context(
            tg::CONCEPT_NAME_CODE_SEQ,
            concept_name,
            &c.concept_name,
            &c.concept_name_equivalent_codes,
        )?;
        replace_or_append_element(elements, s);
    }
    let Some(item) = item else {
        if !c.value.is_empty()
            || !c.value_equivalent_codes.is_empty()
            || !c.measurement_units.is_empty()
            || !c.measurement_units_equivalent_codes.is_empty()
            || !c.numeric_value_qualifier.is_empty()
            || !c.numeric_value_qualifier_equivalents.is_empty()
        {
            return Err(err());
        }
        return Ok(());
    };
    if !c.value.is_empty() || !c.value_equivalent_codes.is_empty() {
        if item.value_type != value::CODE {
            return Err(err());
        }
        let s = code_sequence_with_context(
            tg::CONCEPT_CODE_SEQ,
            &item.code,
            &c.value,
            &c.value_equivalent_codes,
        )?;
        replace_or_append_element(elements, s);
    }
    if !c.measurement_units.is_empty() || !c.measurement_units_equivalent_codes.is_empty() {
        let Some(m) = item
            .measurement
            .as_ref()
            .filter(|_| item.value_type == value::NUM)
        else {
            return Err(err());
        };
        let units = code_sequence_with_context(
            tg::MEASUREMENT_UNITS_CODE,
            &m.units,
            &c.measurement_units,
            &c.measurement_units_equivalent_codes,
        )?;
        let measurement = seq_element(
            tg::MEASURED_VALUE_SEQ,
            vec![DataSet {
                elements: vec![derivedio::ds(tg::NUMERIC_VALUE, &[m.value]), units],
                item_offset: None,
            }],
        );
        replace_or_append_element(elements, measurement);
    }
    if !c.numeric_value_qualifier.is_empty() || !c.numeric_value_qualifier_equivalents.is_empty() {
        if item.value_type != value::NUM || item.numeric_value_qualifier.empty() {
            return Err(err());
        }
        let q = code_sequence_with_context(
            tg::NUMERIC_VALUE_QUALIFIER_CODE_SEQ,
            &item.numeric_value_qualifier,
            &c.numeric_value_qualifier,
            &c.numeric_value_qualifier_equivalents,
        )?;
        replace_or_append_element(elements, q);
    }
    Ok(())
}
pub(crate) fn code_sequence_with_context(
    tag: Tag,
    code: &CodedEntry,
    context: &CodeContext,
    equivalents: &[EquivalentCodeEntry],
) -> Result<Element, SrError> {
    if !valid_code_context_shape(context) {
        return Err(template_validation_error(TEMPLATE_CODE_CODE_CONTEXT));
    }
    let key = code_key_for_entry(code, context);
    if !valid_code_key(&key) {
        return Err(template_validation_error(TEMPLATE_CODE_CODE_CONTEXT));
    }
    let mut elements = Vec::with_capacity(12);
    if !key.long_code_value.is_empty() {
        elements.push(str_elem(tg::LONG_CODE_VALUE, Vr::UC, &key.long_code_value));
    } else if !key.urn_code_value.is_empty() {
        elements.push(str_elem(tg::URN_CODE_VALUE, Vr::UR, &key.urn_code_value));
    } else {
        elements.push(str_elem(tg::CODE_VALUE, Vr::SH, &key.code_value));
    }
    if !key.coding_scheme_designator.is_empty() {
        elements.push(str_elem(
            tg::CODING_SCHEME,
            Vr::SH,
            &key.coding_scheme_designator,
        ));
    }
    elements.push(str_elem(tg::CODE_MEANING, Vr::LO, &code.code_meaning));
    for (tag, vr, v) in [
        (
            tg::CODING_SCHEME_VERSION,
            Vr::SH,
            &context.coding_scheme_version,
        ),
        (tg::CONTEXT_IDENTIFIER, Vr::CS, &context.context_identifier),
        (tg::MAPPING_RESOURCE, Vr::CS, &context.mapping_resource),
        (
            tg::CONTEXT_GROUP_VERSION,
            Vr::DT,
            &context.context_group_version,
        ),
        (
            tg::CONTEXT_GROUP_LOCAL_VERSION,
            Vr::DT,
            &context.context_group_local_version,
        ),
        (
            tg::CONTEXT_GROUP_EXTENSION_FLAG,
            Vr::CS,
            &context.extension_flag,
        ),
        (
            tg::CONTEXT_GROUP_EXTENSION_CREATOR_UID,
            Vr::UI,
            &context.extension_creator_uid,
        ),
        (tg::CONTEXT_UID, Vr::UI, &context.context_uid),
        (
            tg::MAPPING_RESOURCE_UID,
            Vr::UI,
            &context.mapping_resource_uid,
        ),
        (
            tg::MAPPING_RESOURCE_NAME,
            Vr::LO,
            &context.mapping_resource_name,
        ),
    ] {
        if !v.is_empty() {
            elements.push(str_elem(tag, vr, v));
        }
    }
    if !equivalents.is_empty() {
        let mut items = Vec::with_capacity(equivalents.len());
        for e in equivalents {
            let encoded =
                code_sequence_with_context(tg::EQUIVALENT_CODE_SEQUENCE, &e.code, &e.context, &[])?;
            if let Value::Sequence(mut s) = encoded.value {
                items.push(s.remove(0));
            }
        }
        elements.push(seq_element(tg::EQUIVALENT_CODE_SEQUENCE, items));
    }
    Ok(seq_element(
        tag,
        vec![DataSet {
            elements,
            item_offset: None,
        }],
    ))
}
fn apply_template_identification(
    elements: &mut Vec<Element>,
    t: &TemplateIdentification,
) -> Result<(), SrError> {
    if t.mapping_resource.is_empty() || t.identifier.is_empty() {
        return Err(template_validation_error(
            TEMPLATE_CODE_TEMPLATE_IDENTIFICATION,
        ));
    }
    let mut item = vec![
        str_elem(tg::MAPPING_RESOURCE, Vr::CS, &t.mapping_resource),
        str_elem(tg::TEMPLATE_IDENTIFIER, Vr::CS, &t.identifier),
    ];
    if !t.mapping_resource_uid.is_empty() {
        item.push(str_elem(
            tg::MAPPING_RESOURCE_UID,
            Vr::UI,
            &t.mapping_resource_uid,
        ));
    }
    if !t.version.is_empty() {
        item.push(str_elem(tg::TEMPLATE_VERSION, Vr::DT, &t.version));
    }
    replace_or_append_element(
        elements,
        seq_element(
            tg::CONTENT_TEMPLATE_SEQUENCE,
            vec![DataSet {
                elements: item,
                item_offset: None,
            }],
        ),
    );
    Ok(())
}
pub(crate) fn replace_or_append_element(elements: &mut Vec<Element>, replacement: Element) {
    match element_index(elements, replacement.tag()) {
        Some(i) => elements[i] = replacement,
        None => elements.push(replacement),
    }
}
pub(crate) fn element_index(elements: &[Element], tag: Tag) -> Option<usize> {
    elements.iter().position(|e| e.tag() == tag)
}
