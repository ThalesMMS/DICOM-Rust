use super::image_library::{
    ImageLibraryEntry, ImageLibraryGroup, ImageLibraryOptions, default_image_library_options,
    image_library_error, normalize, preflight_library_object,
};
use super::measurement_report::{MeasurementReport, concept, container_item, matches_concept};
use super::numeric_representation::{
    NumericRepresentation, NumericRepresentationMetadata, read_numeric_metadata, resident_string,
};
use super::read::{Decoder, decode_code_with_metadata, read_content_items_bounded};
use super::reference::ValidationReport;
use super::template_catalog::{
    CatalogRow, new_standard_template_registry, standard_template_catalog, standard_template_key,
};
use super::template_metadata::{
    ContentItemCodeContexts, TemplateValidationMetadata, code_contexts_empty,
    decode_template_identification, element_index,
};
use super::template_registry::{TemplateValidationOptions, new_template_validator};
use super::template_snapshot::{
    Fingerprint, MetadataSnapshotOptions, TemplateMetadataSnapshot, bind_numeric_representations,
    plain_document_proof, value_digest,
};
use super::{
    ContentItem, DataSet, Document, ImageReference, Kind, RELATIONSHIP_CONTAINS,
    RELATIONSHIP_HAS_ACQ_CONTEXT, RELATIONSHIP_HAS_CONCEPT_MOD, SrError, content_item_data_set,
    tags as tg, value,
};
use crate::object::{Element, Object, Tag, Value, Vr};
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, LazyLock};
#[derive(Clone, Debug)]
pub struct ImageLibraryMetadataBinding {
    pub(crate) order: Vec<i64>,
    pub(crate) document: Document,
    pub(crate) snapshot: TemplateMetadataSnapshot,
    pub(crate) digest: Fingerprint,
    pub(crate) options: ImageLibraryOptions,
}
impl ImageLibraryMetadataBinding {
    pub fn metadata(&self) -> TemplateValidationMetadata {
        self.snapshot.metadata()
    }
    pub fn numeric_metadata(&self) -> NumericRepresentationMetadata {
        self.snapshot.numeric_representations()
    }
}
fn library_document(descriptors: &[ContentItem], groups: &[ImageLibraryGroup]) -> Document {
    let mut items = descriptors.to_vec();
    for g in groups {
        let mut children = g.descriptors.clone();
        for e in &g.entries {
            let mut item = e.header.clone();
            item.value_type = value::IMAGE.into();
            item.relationship_type = RELATIONSHIP_CONTAINS.into();
            item.image = e.image.clone();
            item.children = e.descriptors.clone();
            children.push(item);
        }
        let children = library_ordered_items(children, g.descriptors.len(), &g.order);
        let mut item = g.header.clone();
        item.value_type = value::CONTAINER.into();
        item.relationship_type = RELATIONSHIP_CONTAINS.into();
        item.concept_name = concept::image_library_group();
        if item.continuity_of_content.is_empty() {
            item.continuity_of_content = "SEPARATE".into();
        }
        item.children = children;
        items.push(item);
    }
    Document {
        sop_class_uid: super::measurement_report::COMPREHENSIVE_3D_SR_STORAGE.into(),
        title: concept::image_library(),
        content: items,
        ..Default::default()
    }
}
fn bounded_library_document(
    descriptors: &[ContentItem],
    groups: &[ImageLibraryGroup],
    o: &ImageLibraryOptions,
) -> Result<Document, SrError> {
    let mut remaining = o.max_items;
    let mut reserve = |n: usize| {
        if n as i64 > remaining {
            return false;
        }
        remaining -= n as i64;
        true
    };
    let limit = || image_library_error("resource-limit", "");
    if !reserve(descriptors.len()) || !reserve(groups.len()) {
        return Err(limit());
    }
    for g in groups {
        if !reserve(g.descriptors.len()) || !reserve(g.entries.len()) {
            return Err(limit());
        }
        for e in &g.entries {
            if !reserve(e.descriptors.len()) {
                return Err(limit());
            }
        }
    }
    for g in groups {
        valid_library_order(&g.order, g.descriptors.len(), g.entries.len())?;
    }
    Ok(library_document(descriptors, groups))
}
fn snapshot_options(o: &ImageLibraryOptions) -> MetadataSnapshotOptions {
    MetadataSnapshotOptions {
        max_depth: o.max_depth,
        max_items: o.max_items,
        max_value_bytes: o.max_value_bytes,
    }
}
fn library_digest(doc: &Document, o: &ImageLibraryOptions) -> Result<Fingerprint, SrError> {
    plain_document_proof(doc, snapshot_options(o))
}
pub fn bind_image_library_metadata(
    descriptors: &[ContentItem],
    groups: &[ImageLibraryGroup],
    metadata: &TemplateValidationMetadata,
    numeric: &NumericRepresentationMetadata,
    options: ImageLibraryOptions,
) -> Result<ImageLibraryMetadataBinding, SrError> {
    let o = normalize(options)?;
    let doc = bounded_library_document(descriptors, groups, &o)?;
    let digest = library_digest(&doc, &o)?;
    let snapshot =
        bind_numeric_representations(Some(&doc), metadata, numeric, snapshot_options(&o))?;
    Ok(ImageLibraryMetadataBinding {
        order: Vec::new(),
        document: doc,
        snapshot,
        digest,
        options: o,
    })
}
pub fn bind_image_library_metadata_with_order(
    descriptors: &[ContentItem],
    groups: &[ImageLibraryGroup],
    order: &[i64],
    metadata: &TemplateValidationMetadata,
    numeric: &NumericRepresentationMetadata,
    options: ImageLibraryOptions,
) -> Result<ImageLibraryMetadataBinding, SrError> {
    let o = normalize(options)?;
    valid_library_order(order, descriptors.len(), groups.len())?;
    for g in groups {
        valid_library_order(&g.order, g.descriptors.len(), g.entries.len())?;
    }
    let mut doc = bounded_library_document(descriptors, groups, &o)?;
    doc.content = library_ordered_items(std::mem::take(&mut doc.content), descriptors.len(), order);
    let digest = library_digest(&doc, &o)?;
    let snapshot =
        bind_numeric_representations(Some(&doc), metadata, numeric, snapshot_options(&o))?;
    Ok(ImageLibraryMetadataBinding {
        order: order.to_vec(),
        document: doc,
        snapshot,
        digest,
        options: o,
    })
}
fn library_parts(report: &MeasurementReport) -> (&[ContentItem], &[ImageLibraryGroup]) {
    (
        report
            .image_library_descriptors
            .as_deref()
            .unwrap_or_default(),
        report.image_library_groups.as_deref().unwrap_or_default(),
    )
}
pub(crate) fn image_library_data_sets(
    report: &MeasurementReport,
) -> Result<Option<Vec<DataSet>>, SrError> {
    if report.image_library_groups.is_none() && report.image_library_descriptors.is_none() {
        return Ok(None);
    }
    let o = match &report.image_library_metadata {
        Some(b) => b.options,
        None => default_image_library_options(),
    };
    let (descriptors, groups) = library_parts(report);
    validate_image_library(descriptors, groups, o)?;
    if report.image_library.is_some()
        && (!report.image_library_read_projection || !image_library_projection_matches(report))
    {
        return Err(image_library_error("ambiguous-representations", ""));
    }
    let mut doc = bounded_library_document(descriptors, groups, &o)?;
    if !report.image_library_order.is_empty() {
        valid_library_order(&report.image_library_order, descriptors.len(), groups.len())?;
        doc.content = library_ordered_items(
            std::mem::take(&mut doc.content),
            descriptors.len(),
            &report.image_library_order,
        );
    }
    if let Some(b) = &report.image_library_metadata {
        let d = library_digest(&doc, &o)?;
        if d != b.digest {
            return Err(image_library_error("stale-metadata", ""));
        }
        let obj = b
            .document
            .dataset_with_metadata_snapshot(Some(&b.snapshot))?;
        let Some(seq) = obj.get(tg::CONTENT_SEQUENCE) else {
            return Ok(Some(Vec::new()));
        };
        let Value::Sequence(items) = &seq.value else {
            return Err(image_library_error("shape", ""));
        };
        return Ok(Some(items.clone()));
    }
    library_digest(&doc, &o)?;
    let mut out = Vec::with_capacity(doc.content.len());
    for item in &doc.content {
        out.push(content_item_data_set(item)?);
    }
    Ok(Some(out))
}
pub(crate) fn structured_library_container(
    report: &MeasurementReport,
    children: Vec<DataSet>,
) -> Result<DataSet, SrError> {
    let mut ds = container_item(
        RELATIONSHIP_CONTAINS,
        &concept::image_library(),
        "",
        children,
    );
    if let Some(b) = &report.image_library_metadata {
        let obj = b
            .document
            .dataset_with_metadata_snapshot(Some(&b.snapshot))?;
        for tag in [tg::CONCEPT_NAME_CODE_SEQ, tg::CONTENT_TEMPLATE_SEQUENCE] {
            if let Some(e) = obj.get(tag) {
                match element_index(&ds.elements, tag) {
                    Some(i) => ds.elements[i] = e.clone(),
                    None => ds.elements.push(e.clone()),
                }
            }
        }
    }
    Ok(ds)
}
pub(crate) type GroupedLibrary = (
    Vec<ContentItem>,
    Vec<ImageLibraryGroup>,
    ImageLibraryMetadataBinding,
);
pub(crate) fn read_grouped_image_library(
    obj: &Object,
    options: ImageLibraryOptions,
) -> Result<GroupedLibrary, SrError> {
    let o = normalize(options)?;
    preflight_library_object(obj, &o)?;
    let mut dec = Decoder::bounded(o.max_depth, o.max_items, o.max_items, o.max_depth + 1);
    let (children, _) = dec.sequence(Some(obj), tg::CONTENT_SEQUENCE);
    let items = read_content_items_bounded(&mut dec, &children)?;
    if let Some(e) = dec.err.take() {
        return Err(e);
    }
    let mut descriptors = Vec::new();
    let mut groups = Vec::new();
    for (i, item) in items.iter().enumerate() {
        let path = format!("1.{}", i + 1);
        if item.relationship_type == RELATIONSHIP_HAS_ACQ_CONTEXT {
            descriptors.push(item.clone());
            continue;
        }
        if item.value_type != value::CONTAINER
            || !matches_concept(&item.concept_name, &concept::image_library_group())
            || item.relationship_type != RELATIONSHIP_CONTAINS
        {
            return Err(image_library_error("library-shape", &path));
        }
        let mut header = item.clone();
        header.children = Vec::new();
        let mut g = ImageLibraryGroup {
            header,
            ..Default::default()
        };
        for (j, child) in item.children.iter().enumerate() {
            let p = format!("{path}.{}", j + 1);
            if child.relationship_type == RELATIONSHIP_HAS_ACQ_CONTEXT {
                g.descriptors.push(child.clone());
                g.order.push(-(g.descriptors.len() as i64));
            } else if child.value_type == value::IMAGE
                && child.relationship_type == RELATIONSHIP_CONTAINS
            {
                let mut h = child.clone();
                h.children = Vec::new();
                g.entries.push(ImageLibraryEntry {
                    image: child.image.clone(),
                    descriptors: child.children.clone(),
                    header: h,
                });
                g.order.push(g.entries.len() as i64);
            } else {
                return Err(image_library_error("group-shape", &p));
            }
        }
        groups.push(g);
    }
    let mut order = Vec::with_capacity(items.len());
    let (mut di, mut gi) = (0i64, 0i64);
    for item in &items {
        if item.relationship_type == RELATIONSHIP_HAS_ACQ_CONTEXT {
            di += 1;
            order.push(-di);
        } else {
            gi += 1;
            order.push(gi);
        }
    }
    let mut root = ContentItemCodeContexts {
        path: "1".into(),
        ..Default::default()
    };
    let (_, ctx, eq, _) = decode_code_with_metadata(&mut dec, Some(obj), tg::CONCEPT_NAME_CODE_SEQ);
    root.concept_name = ctx;
    root.concept_name_equivalent_codes = eq;
    if !code_contexts_empty(&root) {
        dec.add_code_contexts(root);
    }
    if let Some(t) = decode_template_identification(&mut dec, obj, &[1]) {
        dec.add_template_identification(t);
    }
    let numeric = read_numeric_metadata(obj, o.max_items, o.max_depth)?;
    let mut canonical = library_document(&descriptors, &groups);
    canonical.content = library_ordered_items(
        std::mem::take(&mut canonical.content),
        descriptors.len(),
        &order,
    );
    let by_path: HashMap<&str, &NumericRepresentation> = numeric
        .entries
        .iter()
        .map(|e| (e.path.as_str(), &e.representation))
        .collect();
    apply_library_numeric(&mut canonical.content, &by_path)?;
    unpack_canonical(&canonical.content, &order, &mut descriptors, &mut groups);
    let b = bind_image_library_metadata_with_order(
        &descriptors,
        &groups,
        &order,
        &dec.template_metadata,
        &numeric,
        o,
    )?;
    Ok((descriptors, groups, b))
}
fn apply_library_numeric(
    items: &mut [ContentItem],
    by_path: &HashMap<&str, &NumericRepresentation>,
) -> Result<(), SrError> {
    let mut order: Vec<Vec<usize>> = Vec::new();
    {
        let mut stack: Vec<(&[ContentItem], Vec<usize>, usize)> = vec![(&*items, Vec::new(), 0)];
        while let Some(frame) = stack.last_mut() {
            if frame.2 >= frame.0.len() {
                stack.pop();
                continue;
            }
            let i = frame.2;
            frame.2 += 1;
            let mut p = frame.1.clone();
            p.push(i);
            order.push(p.clone());
            let item = &frame.0[i];
            if !item.children.is_empty() {
                stack.push((&item.children, p, 0));
            }
        }
    }
    for index_path in order {
        let mut item = &mut items[index_path[0]];
        for &i in &index_path[1..] {
            item = &mut item.children[i];
        }
        let key = format!(
            "1.{}",
            index_path
                .iter()
                .map(|i| (i + 1).to_string())
                .collect::<Vec<_>>()
                .join(".")
        );
        if let Some(n) = by_path.get(key.as_str()) {
            let Some(m) = &mut item.measurement else {
                return Err(image_library_error("numeric-association", &key));
            };
            m.value = n.resolve()?.0;
        }
    }
    Ok(())
}
fn unpack_canonical(
    content: &[ContentItem],
    order: &[i64],
    descriptors: &mut [ContentItem],
    groups: &mut [ImageLibraryGroup],
) {
    let root = position_order(order, descriptors.len(), groups.len());
    for (k, n) in root.iter().enumerate() {
        let item = &content[k];
        if *n < 0 {
            descriptors[(-n - 1) as usize] = item.clone();
            continue;
        }
        let g = &mut groups[(n - 1) as usize];
        let inner = position_order(&g.order, g.descriptors.len(), g.entries.len());
        for (j, m) in inner.iter().enumerate() {
            let child = &item.children[j];
            if *m < 0 {
                g.descriptors[(-m - 1) as usize] = child.clone();
            } else {
                g.entries[(m - 1) as usize].descriptors = child.children.clone();
            }
        }
    }
}
fn position_order(order: &[i64], descriptors: usize, entries: usize) -> Vec<i64> {
    if !order.is_empty() {
        return order.to_vec();
    }
    (1..=descriptors as i64)
        .map(|d| -d)
        .chain(1..=entries as i64)
        .collect()
}
static LIBRARY_UNIT_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"UNITS = (?:EV |DT )?\(([^,]+), UCUM, "([^"]+)"\)"#).expect("valid regex")
});
fn library_row(
    item: &ContentItem,
    modality: &str,
    root: bool,
) -> Option<(i64, &'static CatalogRow)> {
    let mut fallback = None;
    for t in standard_template_catalog() {
        if (root && t.id != 1600) || (!root && (t.id < 1602 || t.id > 1608)) {
            continue;
        }
        for row in t.rows {
            if (!root || (row.relationship == RELATIONSHIP_HAS_ACQ_CONTEXT && row.level == 1))
                && row.include == 0
                && item.value_type == row.value_type
                && item.concept_name.code_value == row.code.0
                && item.concept_name.coding_scheme_designator == row.code.1
            {
                if root || library_applies(t.id, modality) {
                    return Some((t.id, row));
                }
                fallback = Some((t.id, row));
            }
        }
    }
    fallback
}
fn library_modality(items: &[ContentItem]) -> String {
    for i in items {
        if i.value_type == value::CODE
            && i.concept_name.code_value == "121139"
            && i.concept_name.coding_scheme_designator == "DCM"
            && i.code.coding_scheme_designator == "DCM"
        {
            return i.code.code_value.clone();
        }
    }
    String::new()
}
pub(crate) fn library_applies(tid: i64, modality: &str) -> bool {
    match tid {
        1602 => true,
        1603 => {
            " CR DX IO MG PX RF RG XA ".contains(&format!(" {modality} ")) && !modality.is_empty()
        }
        1604 => modality == "CT" || modality == "MR" || modality == "PT",
        1605 => modality == "CT",
        1606 | 1608 => modality == "MR",
        1607 => modality == "PT",
        _ => false,
    }
}
fn library_da(s: &str) -> bool {
    s.len() == 8 && crate::dcmtime::parse_date(s).is_ok()
}
fn library_tm(s: &str) -> bool {
    !s.is_empty() && crate::dcmtime::parse_time(s).is_ok()
}
pub(crate) fn valid_uid(s: &str) -> bool {
    if s.is_empty() || s.len() > 64 {
        return false;
    }
    s.split('.').all(|p| {
        !p.is_empty()
            && p.bytes().all(|c| c.is_ascii_digit())
            && !(p.len() > 1 && p.starts_with('0'))
    })
}
struct LibraryValidator {
    max_depth: i64,
}
impl LibraryValidator {
    fn validate(
        &self,
        items: &[ContentItem],
        path: &str,
        modality: &str,
        parent: &str,
        depth: i64,
        conditions: bool,
    ) -> Result<(), SrError> {
        if depth > self.max_depth {
            return Err(image_library_error("resource-limit", path));
        }
        let mut seen: HashMap<String, i64> = HashMap::new();
        let (mut glucose, mut observed, mut date, mut time) = (false, false, false, false);
        for (n, item) in items.iter().enumerate() {
            let p = format!("{path}.{}", n + 1);
            if !item.referenced_content_item_identifier.is_empty() {
                return Err(image_library_error("by-reference-descriptor", &p));
            }
            let known = library_row(item, modality, parent == "library");
            if known.is_none() {
                for t in standard_template_catalog() {
                    if (parent == "library" && t.id != 1600)
                        || (parent != "library" && (t.id < 1602 || t.id > 1608))
                    {
                        continue;
                    }
                    for r in t.rows {
                        if r.include == 0
                            && item.concept_name.code_value == r.code.0
                            && item.concept_name.coding_scheme_designator == r.code.1
                        {
                            return Err(image_library_error("value-type", &p));
                        }
                    }
                }
            }
            if parent == "111031" {
                if item.relationship_type != RELATIONSHIP_HAS_CONCEPT_MOD {
                    return Err(image_library_error("relationship", &p));
                }
            } else if item.relationship_type != RELATIONSHIP_HAS_ACQ_CONTEXT {
                return Err(image_library_error("relationship", &p));
            }
            if let Some((tid, row)) = known {
                let c = seen.entry(format!("{tid}:{}", row.label)).or_default();
                *c += 1;
                if row.max > 0 && *c > row.max {
                    return Err(image_library_error("duplicate-singleton", &p));
                }
                if conditions && tid != 1600 && tid != 1602 && !library_applies(tid, modality) {
                    return Err(image_library_error("modality-applicability", &p));
                }
                if tid == 1603 && row.level > 0 && parent != "111031" {
                    return Err(image_library_error("modifier-parent", &p));
                }
                if row.level == 0 && parent == "111031" {
                    return Err(image_library_error("modifier-parent", &p));
                }
                if let Some(m) = LIBRARY_UNIT_PATTERN.captures(row.value_set) {
                    let unit = m.get(1).map_or("", |x| x.as_str());
                    match &item.measurement {
                        Some(me)
                            if me.units.code_value == unit
                                && me.units.coding_scheme_designator == "UCUM" => {}
                        _ => return Err(image_library_error("units", &p)),
                    }
                }
            }
            match item.value_type.as_str() {
                value::NUM => {
                    if let Some(m) = &item.measurement
                        && !m.value.is_finite()
                    {
                        return Err(image_library_error("numeric-value", &p));
                    }
                }
                value::CODE => {
                    if item.code.empty() {
                        return Err(image_library_error("code-value", &p));
                    }
                }
                value::DATE => {
                    if !library_da(&item.date) {
                        return Err(image_library_error("date-value", &p));
                    }
                }
                value::TIME => {
                    if !library_tm(&item.time) {
                        return Err(image_library_error("time-value", &p));
                    }
                }
                value::DATETIME => {
                    if crate::dcmtime::parse_datetime(&item.date_time).is_err() {
                        return Err(image_library_error("datetime-value", &p));
                    }
                }
                value::UIDREF if !valid_uid(&item.uid) => {
                    return Err(image_library_error("uid-value", &p));
                }
                _ => {}
            }
            if item.concept_name.coding_scheme_designator == "LN"
                && item.concept_name.code_value == "14749-6"
            {
                glucose = true;
                for e in &item.value_elements {
                    if e.tag() == Tag::new(0x0040, 0xA032) {
                        let ok = resident_string(e, Vr::DT)
                            .ok()
                            .is_some_and(|v| crate::dcmtime::parse_datetime(&v).is_ok());
                        if !ok {
                            return Err(image_library_error("observation-datetime", &p));
                        }
                        observed = true;
                    }
                }
            }
            if item.concept_name.coding_scheme_designator == "DCM"
                && item.concept_name.code_value == "127857"
                && item.value_type == value::DATE
            {
                date = true;
            }
            if item.concept_name.coding_scheme_designator == "DCM"
                && item.concept_name.code_value == "127858"
                && item.value_type == value::TIME
            {
                time = true;
            }
            if !item.children.is_empty() {
                self.validate(
                    &item.children,
                    &p,
                    modality,
                    &item.concept_name.code_value,
                    depth + 1,
                    conditions,
                )?;
            }
        }
        if conditions && glucose && !observed && (!date || !time) {
            return Err(image_library_error("pet-glucose-date-time", path));
        }
        Ok(())
    }
}
pub fn validate_image_library(
    descriptors: &[ContentItem],
    groups: &[ImageLibraryGroup],
    options: ImageLibraryOptions,
) -> Result<(), SrError> {
    let o = normalize(options)?;
    let doc = bounded_library_document(descriptors, groups, &o)?;
    if let Err(e) = library_digest(&doc, &o) {
        if e.is(Kind::ResourceLimitExceeded) {
            return Err(image_library_error("resource-limit", ""));
        }
        return Err(image_library_error("unpreservable-structure", ""));
    }
    let v = LibraryValidator {
        max_depth: o.max_depth,
    };
    v.validate(descriptors, "1", "", "library", 1, false)?;
    let mut instances: HashMap<&str, usize> = HashMap::new();
    for (i, g) in groups.iter().enumerate() {
        let p = format!("1.{}", descriptors.len() + i + 1);
        let inherited = library_modality(&g.descriptors);
        v.validate(&g.descriptors, &p, &inherited, "", 2, false)?;
        for (j, e) in g.entries.iter().enumerate() {
            let mut ordinal = g.descriptors.len() + j + 1;
            if let Some(k) = g.order.iter().position(|v| *v == j as i64 + 1) {
                ordinal = k + 1;
            }
            let ep = format!("{p}.{ordinal}");
            if !valid_uid(&e.image.sop_class_uid) || !valid_uid(&e.image.sop_instance_uid) {
                return Err(image_library_error("image-reference", &ep));
            }
            if e.image.frames.iter().any(|f| *f < 1) {
                return Err(image_library_error("frame-number", &ep));
            }
            let mut modality = library_modality(&e.descriptors);
            if modality.is_empty() {
                modality = inherited.clone();
            }
            if let Some(previous) = instances.get(e.image.sop_instance_uid.as_str())
                && *previous != i
            {
                return Err(image_library_error("instance-in-multiple-groups", &ep));
            }
            instances.insert(&e.image.sop_instance_uid, i);
            v.validate(&e.descriptors, &ep, &modality, "", 3, false)?;
            let effective = effective_library_descriptors(&g.descriptors, &e.descriptors);
            for d in &effective {
                if d.value_type == value::NUM
                    && d.concept_name.code_value == "121140"
                    && d.concept_name.coding_scheme_designator == "DCM"
                    && let Some(m) = &d.measurement
                {
                    let n = m.value;
                    if n < 1.0 || n.trunc() != n {
                        return Err(image_library_error("frame-number", &ep));
                    }
                    if e.image.frames.iter().any(|f| *f as f64 > n) {
                        return Err(image_library_error("frame-number", &ep));
                    }
                }
            }
            v.validate(&effective, &ep, &modality, "", 3, true)?;
        }
    }
    Ok(())
}
fn effective_library_descriptors(common: &[ContentItem], own: &[ContentItem]) -> Vec<ContentItem> {
    let key = |item: &ContentItem| {
        format!(
            "{}\0{}\0{}",
            item.concept_name.coding_scheme_designator,
            item.concept_name.code_value,
            item.value_type
        )
    };
    let overrides: HashSet<String> = own.iter().map(key).collect();
    let mut out: Vec<ContentItem> = common
        .iter()
        .filter(|i| !overrides.contains(&key(i)))
        .cloned()
        .collect();
    out.extend(own.iter().cloned());
    out
}
fn valid_library_order(order: &[i64], descriptors: usize, entries: usize) -> Result<(), SrError> {
    if order.is_empty() {
        return Ok(());
    }
    if order.len() != descriptors + entries {
        return Err(image_library_error("order-shape", ""));
    }
    let mut seen = HashSet::new();
    for &n in order {
        if n == 0 || n > entries as i64 || n < -(descriptors as i64) || !seen.insert(n) {
            return Err(image_library_error("order-shape", ""));
        }
    }
    Ok(())
}
fn library_ordered_items(
    items: Vec<ContentItem>,
    descriptors: usize,
    order: &[i64],
) -> Vec<ContentItem> {
    if order.is_empty() {
        return items;
    }
    order
        .iter()
        .map(|&n| {
            if n < 0 {
                items[(-n - 1) as usize].clone()
            } else {
                items[descriptors + n as usize - 1].clone()
            }
        })
        .collect()
}
pub fn validate_image_library_template(
    report: Option<&MeasurementReport>,
    options: ImageLibraryOptions,
    template_options: TemplateValidationOptions,
) -> (ValidationReport, Option<SrError>) {
    match validate_image_library_template_inner(report, options, template_options) {
        Ok(r) => r,
        Err(e) => (ValidationReport::default(), Some(e)),
    }
}
fn validate_image_library_template_inner(
    report: Option<&MeasurementReport>,
    options: ImageLibraryOptions,
    template_options: TemplateValidationOptions,
) -> Result<(ValidationReport, Option<SrError>), SrError> {
    let Some(report) = report else {
        return Err(image_library_error("report-required", ""));
    };
    let o = normalize(options)?;
    let (descriptors, groups) = library_parts(report);
    validate_image_library(descriptors, groups, o)?;
    if report.image_library.is_some()
        && (!report.image_library_read_projection || !image_library_projection_matches(report))
    {
        return Err(image_library_error("ambiguous-representations", ""));
    }
    let mut doc = bounded_library_document(descriptors, groups, &o)?;
    valid_library_order(&report.image_library_order, descriptors.len(), groups.len())?;
    doc.content = library_ordered_items(
        std::mem::take(&mut doc.content),
        descriptors.len(),
        &report.image_library_order,
    );
    let registry = new_standard_template_registry(&[1600])?;
    let validator = new_template_validator(Some(Arc::new(registry)), template_options)?;
    let key = standard_template_key(1600).unwrap_or_default();
    let mut metadata = TemplateValidationMetadata::default();
    if let Some(b) = &report.image_library_metadata {
        let digest = library_digest(&doc, &o)?;
        if digest != b.digest {
            return Err(image_library_error("stale-metadata", ""));
        }
        b.snapshot.check(Some(&b.document))?;
        metadata = b.metadata();
    }
    for c in &mut metadata.code_contexts {
        c.path = format!("1.{}", c.path);
    }
    for t in &mut metadata.template_identifications {
        t.path = format!("1.{}", t.path);
    }
    let library = ContentItem {
        value_type: value::CONTAINER.into(),
        relationship_type: RELATIONSHIP_CONTAINS.into(),
        concept_name: concept::image_library(),
        continuity_of_content: "SEPARATE".into(),
        children: std::mem::take(&mut doc.content),
        ..Default::default()
    };
    doc.content = vec![library];
    Ok(validator.validate_with_metadata(Some(&doc), &key, &metadata))
}
pub(crate) fn image_library_projection_matches(report: &MeasurementReport) -> bool {
    let (descriptors, groups) = library_parts(report);
    if !report.image_library_order.is_empty()
        && valid_library_order(&report.image_library_order, descriptors.len(), groups.len())
            .is_err()
    {
        return false;
    }
    let library = report.image_library.as_deref().unwrap_or_default();
    let mut index = 0usize;
    let mut reference = |want: &ImageReference| {
        if index >= library.len() {
            return false;
        }
        let got = &library[index];
        index += 1;
        got.study_instance_uid == want.study_instance_uid
            && got.series_instance_uid == want.series_instance_uid
            && got.sop_class_uid == want.sop_class_uid
            && got.sop_instance_uid == want.sop_instance_uid
            && got.frames == want.frames
    };
    let mut group = |g: &ImageLibraryGroup| {
        if g.order.is_empty() {
            return g.entries.iter().all(|e| reference(&e.image));
        }
        for &ordinal in &g.order {
            if ordinal > 0
                && (ordinal as usize > g.entries.len()
                    || !reference(&g.entries[ordinal as usize - 1].image))
            {
                return false;
            }
        }
        true
    };
    let ok = if report.image_library_order.is_empty() {
        groups.iter().all(&mut group)
    } else {
        report
            .image_library_order
            .iter()
            .all(|&ordinal| ordinal <= 0 || group(&groups[ordinal as usize - 1]))
    };
    ok && index == library.len()
}
struct ClonePreflight<'a> {
    o: &'a ImageLibraryOptions,
    nodes: i64,
    attributes: i64,
}
impl ClonePreflight<'_> {
    fn limit(&self, n: usize) -> Result<(), SrError> {
        if n as i64 > self.o.max_items {
            return Err(image_library_error("resource-limit", ""));
        }
        Ok(())
    }
    fn elements(&mut self, es: &[Element], depth: i64) -> Result<(), SrError> {
        let cap = self.o.max_items * 64;
        if depth > self.o.max_depth || es.len() as i64 > cap - self.attributes {
            return Err(image_library_error("resource-limit", ""));
        }
        self.attributes += es.len() as i64;
        for e in es {
            match &e.value {
                Value::Strings(v) => {
                    if v.len() as i64 > cap - self.attributes {
                        return Err(image_library_error("resource-limit", ""));
                    }
                    self.attributes += v.len() as i64;
                }
                Value::Sequence(items) => {
                    if items.len() as i64 > cap - self.attributes {
                        return Err(image_library_error("resource-limit", ""));
                    }
                    self.attributes += items.len() as i64;
                    for d in items {
                        self.elements(&d.elements, depth + 1)?;
                    }
                }
                Value::Fragments { fragments, .. } => self.limit(fragments.len())?,
                _ => {}
            }
        }
        Ok(())
    }
    fn item(&mut self, v: &ContentItem, depth: i64) -> Result<(), SrError> {
        if depth > self.o.max_depth || self.nodes >= self.o.max_items {
            return Err(image_library_error("resource-limit", ""));
        }
        self.nodes += 1;
        for n in [
            v.children.len(),
            v.image.frames.len(),
            v.waveform.channels.len(),
            v.spatial.coordinates.len(),
            v.spatial3d.coordinates.len(),
            v.temporal.sample_positions.len(),
            v.temporal.time_offsets.len(),
            v.temporal.date_times.len(),
            v.referenced_content_item_identifier.len(),
        ] {
            self.limit(n)?;
        }
        if v.children.len() as i64 > self.o.max_items - self.nodes {
            return Err(image_library_error("resource-limit", ""));
        }
        self.elements(&v.value_elements, depth)?;
        for c in &v.children {
            self.item(c, depth + 1)?;
        }
        Ok(())
    }
}
fn preflight_library_clone(
    groups: &[ImageLibraryGroup],
    o: &ImageLibraryOptions,
) -> Result<(), SrError> {
    let mut p = ClonePreflight {
        o,
        nodes: 0,
        attributes: 0,
    };
    p.limit(groups.len())?;
    for g in groups {
        p.item(&g.header, 2)?;
        p.limit(g.order.len())?;
        for d in &g.descriptors {
            p.item(d, 3)?;
        }
        p.limit(g.entries.len())?;
        for e in &g.entries {
            p.limit(e.image.frames.len())?;
            p.item(&e.header, 3)?;
            for d in &e.descriptors {
                p.item(d, 4)?;
            }
        }
    }
    Ok(())
}
pub fn promote_image_library_common_descriptors(
    report: Option<&MeasurementReport>,
    options: ImageLibraryOptions,
) -> Result<Vec<ImageLibraryGroup>, SrError> {
    let Some(report) = report else {
        return Err(image_library_error("report-required", ""));
    };
    if report.image_library_metadata.is_some() {
        return Err(image_library_error("explicit-metadata-remap-required", ""));
    }
    let (descriptors, groups) = library_parts(report);
    validate_image_library(descriptors, groups, options)?;
    let o = normalize(options)?;
    preflight_library_clone(groups, &o)?;
    if value_digest(&groups, o.max_value_bytes).is_err() {
        return Err(image_library_error("resource-limit", ""));
    }
    let mut out = groups.to_vec();
    for g in &mut out {
        if g.entries.len() < 2 {
            continue;
        }
        let mut common: Vec<ContentItem> = Vec::new();
        for candidate in &g.entries[0].descriptors {
            let all = g.entries[1..]
                .iter()
                .all(|entry| entry.descriptors.iter().any(|d| d == candidate));
            if all && !g.descriptors.iter().any(|d| d == candidate) {
                common.push(candidate.clone());
            }
        }
        for e in &mut g.entries {
            e.descriptors.retain(|d| !common.iter().any(|c| c == d));
        }
        g.descriptors.extend(common);
        g.order = Vec::new();
    }
    Ok(out)
}
