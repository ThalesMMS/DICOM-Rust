use super::measurement_context::{
    DetailedMeasurementReport, MEASUREMENT_CONTEXT_CONFLICT, MEASUREMENT_CONTEXT_LIMIT,
    MEASUREMENT_CONTEXT_SHAPE, MEASUREMENT_RWVM_STORAGE, MeasurementAlgorithm,
    MeasurementContextCode, MeasurementContextOptions, MeasurementFindingSite,
    MeasurementGroupContext, MeasurementNumericContext, MeasurementQualitativeEvaluation,
    MeasurementReferenceEvidence, MeasurementReferenceMetadata, MeasurementReferenceMetadataEntry,
    MeasurementReportContext, MeasurementTimePoint, bind_measurement_report_with_context,
    measurement_context_error, normalize_measurement_context_options,
    read_measurement_reference_metadata,
};
use super::measurement_context_evidence::{admit_measurement_evidence, read_measurement_evidence};
use super::measurement_context_read::{context, measurement_context_concept};
use super::measurement_report::{
    MeasurementGroup, MeasurementReport, concept, write_measurement_report,
};
use super::numeric_representation::{
    NumericRepresentation, NumericRepresentationEntry, NumericRepresentationMetadata,
    read_document_numeric_representations,
};
use super::reference::{append_path, identifier_string};
use super::template_metadata::{
    CodeContext, ContentItemCodeContexts, TemplateIdentification, TemplateValidationMetadata,
    code_contexts_empty, code_key_for_entry, new_metadata_index, valid_code_context_shape,
    valid_code_key, valid_equivalents,
};
use super::template_snapshot::bind_measurement_snapshot;
use super::{
    CodedEntry, CompositeReference, ContentItem, Measurement, RELATIONSHIP_CONTAINS,
    RELATIONSHIP_HAS_CONCEPT_MOD, RELATIONSHIP_HAS_OBS_CONTEXT, RELATIONSHIP_INFERRED_FROM,
    SrError, str_elem, value,
};
use crate::object::{Tag, Vr};
use std::collections::HashMap;
#[derive(Clone, Debug, Default)]
struct AuthoredNode {
    item: ContentItem,
    code: ContentItemCodeContexts,
    identification: Option<TemplateIdentification>,
    numeric: Option<NumericRepresentation>,
    reference: Option<MeasurementReferenceMetadataEntry>,
    children: Vec<AuthoredNode>,
}
struct Author {
    options: MeasurementContextOptions,
    evidence: Vec<MeasurementReferenceEvidence>,
    count: i64,
}
fn limit() -> SrError {
    measurement_context_error(MEASUREMENT_CONTEXT_LIMIT, "")
}
fn shape() -> SrError {
    measurement_context_error(MEASUREMENT_CONTEXT_SHAPE, "")
}
fn conflict() -> SrError {
    measurement_context_error(MEASUREMENT_CONTEXT_CONFLICT, "")
}
fn code_context_empty(c: &CodeContext) -> bool {
    *c == CodeContext::default()
}
fn valid_context_code(c: &MeasurementContextCode) -> bool {
    valid_code_key(&code_key_for_entry(&c.code, &c.context))
        && valid_code_context_shape(&c.context)
        && valid_equivalents(&c.equivalent_codes)
}
impl Author {
    fn reserve(&mut self, n: i64) -> Result<(), SrError> {
        if n < 0 || n > self.options.read.max_items - self.count {
            return Err(limit());
        }
        self.count += n;
        Ok(())
    }
    fn scalar(
        &mut self,
        vt: &str,
        rel: &str,
        concept: &CodedEntry,
        text: &str,
    ) -> Result<AuthoredNode, SrError> {
        self.reserve(1)?;
        if text.len() as i64 > self.options.max_string_bytes {
            return Err(limit());
        }
        Ok(AuthoredNode {
            item: ContentItem {
                value_type: vt.to_string(),
                relationship_type: rel.to_string(),
                concept_name: concept.clone(),
                text: text.to_string(),
                ..Default::default()
            },
            ..Default::default()
        })
    }
    fn code(
        &mut self,
        rel: &str,
        concept: &CodedEntry,
        v: &MeasurementContextCode,
    ) -> Result<AuthoredNode, SrError> {
        self.reserve(1)?;
        if !valid_context_code(v) {
            return Err(shape());
        }
        Ok(AuthoredNode {
            item: ContentItem {
                value_type: value::CODE.to_string(),
                relationship_type: rel.to_string(),
                concept_name: concept.clone(),
                code: v.code.clone(),
                ..Default::default()
            },
            code: ContentItemCodeContexts {
                value: v.context.clone(),
                value_equivalent_codes: v.equivalent_codes.clone(),
                ..Default::default()
            },
            ..Default::default()
        })
    }
    fn algorithm(
        &mut self,
        v: Option<&MeasurementAlgorithm>,
    ) -> Result<Vec<AuthoredNode>, SrError> {
        let Some(v) = v else {
            return Ok(Vec::new());
        };
        if v.name.is_empty() || v.version.is_empty() {
            return Err(shape());
        }
        let mut out = Vec::new();
        out.push(self.scalar(
            value::TEXT,
            RELATIONSHIP_HAS_CONCEPT_MOD,
            &context::algorithm_name(),
            &v.name,
        )?);
        if let Some(code) = &v.name_code {
            out.push(self.code(
                RELATIONSHIP_HAS_CONCEPT_MOD,
                &context::algorithm_name(),
                code,
            )?);
        }
        out.push(self.scalar(
            value::TEXT,
            RELATIONSHIP_HAS_CONCEPT_MOD,
            &context::algorithm_version(),
            &v.version,
        )?);
        if !v.manufacturer.is_empty() {
            out.push(self.scalar(
                value::TEXT,
                RELATIONSHIP_HAS_CONCEPT_MOD,
                &context::algorithm_manufacturer(),
                &v.manufacturer,
            )?);
        }
        for parameter in &v.parameters {
            if parameter.is_empty() {
                return Err(shape());
            }
            out.push(self.scalar(
                value::TEXT,
                RELATIONSHIP_HAS_CONCEPT_MOD,
                &context::algorithm_parameters(),
                parameter,
            )?);
        }
        if let Some(family) = &v.family {
            out.push(self.code(
                RELATIONSHIP_HAS_CONCEPT_MOD,
                &context::algorithm_family(),
                family,
            )?);
        }
        Ok(out)
    }
    fn sites(&mut self, sites: &[MeasurementFindingSite]) -> Result<Vec<AuthoredNode>, SrError> {
        if sites.len() as i64 > self.options.read.max_items - self.count {
            return Err(limit());
        }
        let mut out = Vec::with_capacity(sites.len());
        for site in sites {
            let mut n = self.code(
                RELATIONSHIP_HAS_CONCEPT_MOD,
                &concept::finding_site(),
                &site.site,
            )?;
            for (concept, value) in [
                (context::laterality(), &site.laterality),
                (context::topographical(), &site.topographical_modifier),
            ] {
                if let Some(value) = value {
                    let child = self.code(RELATIONSHIP_HAS_CONCEPT_MOD, &concept, value)?;
                    n.children.push(child);
                }
            }
            out.push(n);
        }
        Ok(out)
    }
    fn rwvm(
        &mut self,
        e: Option<&MeasurementReferenceEvidence>,
        rel: &str,
    ) -> Result<Vec<AuthoredNode>, SrError> {
        let Some(e) = e else {
            return Ok(Vec::new());
        };
        if e.sop_class_uid != MEASUREMENT_RWVM_STORAGE {
            return Err(shape());
        }
        let admitted = admit_measurement_evidence(std::slice::from_ref(e), &self.options)?;
        if self.evidence.len() as i64 >= self.options.max_evidence {
            return Err(limit());
        }
        self.evidence.extend(admitted);
        self.reserve(1)?;
        Ok(vec![AuthoredNode {
            item: ContentItem {
                value_type: value::COMPOSITE.to_string(),
                relationship_type: rel.to_string(),
                concept_name: context::rwvm(),
                composite: CompositeReference {
                    sop_class_uid: e.sop_class_uid.clone(),
                    sop_instance_uid: e.sop_instance_uid.clone(),
                },
                ..Default::default()
            },
            ..Default::default()
        }])
    }
    fn time_point(
        &mut self,
        t: Option<&MeasurementTimePoint>,
    ) -> Result<Vec<AuthoredNode>, SrError> {
        let Some(t) = t else {
            return Ok(Vec::new());
        };
        if t.time_point.is_empty() || t.temporal_offset.is_some() != t.event_type.is_some() {
            return Err(shape());
        }
        let mut out = Vec::new();
        for (concept, v) in [
            (context::subject_time_point(), &t.subject_identifier),
            (context::protocol_time_point(), &t.protocol_identifier),
            (context::time_point(), &t.time_point),
        ] {
            if !v.is_empty() {
                out.push(self.scalar(value::TEXT, RELATIONSHIP_HAS_OBS_CONTEXT, &concept, v)?);
            }
        }
        for v in &t.types {
            out.push(self.code(RELATIONSHIP_HAS_OBS_CONTEXT, &context::time_point_type(), v)?);
        }
        for (concept, unit, numeric) in [
            (context::time_point_order(), "1", &t.order),
            (context::temporal_offset(), "d", &t.temporal_offset),
        ] {
            let Some(numeric) = numeric else {
                continue;
            };
            let (value, _) = numeric.resolve()?;
            self.reserve(1)?;
            let mut n = AuthoredNode {
                item: ContentItem {
                    value_type: value::NUM.to_string(),
                    relationship_type: RELATIONSHIP_HAS_OBS_CONTEXT.to_string(),
                    concept_name: concept,
                    measurement: Some(Measurement {
                        value,
                        units: measurement_context_concept(unit, "UCUM", unit),
                    }),
                    ..Default::default()
                },
                numeric: Some(numeric.clone()),
                ..Default::default()
            };
            if unit == "d" {
                let event = t.event_type.as_ref().expect("checked with the offset");
                let event =
                    self.code(RELATIONSHIP_HAS_CONCEPT_MOD, &context::event_type(), event)?;
                n.children.push(event);
            }
            out.push(n);
        }
        Ok(out)
    }
    fn qualitative(
        &mut self,
        input: &[MeasurementQualitativeEvaluation],
    ) -> Result<Vec<AuthoredNode>, SrError> {
        if input.len() as i64 > self.options.read.max_items - self.count {
            return Err(limit());
        }
        let mut out = Vec::with_capacity(input.len());
        for q in input {
            if !valid_context_code(&q.concept_name) {
                return Err(shape());
            }
            let mut n = match q.value_type.as_str() {
                value::CODE => {
                    if !q.text.is_empty() {
                        return Err(conflict());
                    }
                    self.code(RELATIONSHIP_CONTAINS, &q.concept_name.code, &q.code)?
                }
                value::TEXT => {
                    if !q.code.code.empty()
                        || !code_context_empty(&q.code.context)
                        || !q.code.equivalent_codes.is_empty()
                        || !q.modifiers.is_empty()
                        || q.text.is_empty()
                    {
                        return Err(conflict());
                    }
                    self.scalar(
                        value::TEXT,
                        RELATIONSHIP_CONTAINS,
                        &q.concept_name.code,
                        &q.text,
                    )?
                }
                _ => return Err(shape()),
            };
            n.code.concept_name = q.concept_name.context.clone();
            n.code.concept_name_equivalent_codes = q.concept_name.equivalent_codes.clone();
            for modifier in &q.modifiers {
                let mut child = self.code(
                    RELATIONSHIP_HAS_CONCEPT_MOD,
                    &modifier.concept_name.code,
                    &modifier.value,
                )?;
                child.code.concept_name = modifier.concept_name.context.clone();
                child.code.concept_name_equivalent_codes =
                    modifier.concept_name.equivalent_codes.clone();
                n.children.push(child);
            }
            out.push(n);
        }
        Ok(out)
    }
    fn numeric_context(
        &mut self,
        n: &mut AuthoredNode,
        c: &MeasurementNumericContext,
    ) -> Result<(), SrError> {
        if !c.path.is_empty() {
            return Err(conflict());
        }
        let measurement = n.item.measurement.clone().unwrap_or_default();
        if let Some(numeric) = &c.numeric {
            let (value, _) = numeric.resolve()?;
            if value.to_bits() != measurement.value.to_bits() {
                return Err(conflict());
            }
            n.numeric = Some(numeric.clone());
        }
        for (got, want, units) in [
            (&c.concept_name, &n.item.concept_name, false),
            (&c.units, &measurement.units, true),
        ] {
            if !got.code.empty()
                || !code_context_empty(&got.context)
                || !got.equivalent_codes.is_empty()
            {
                if code_key_for_entry(&got.code, &CodeContext::default())
                    != code_key_for_entry(want, &CodeContext::default())
                {
                    return Err(conflict());
                }
                if units {
                    n.code.measurement_units = got.context.clone();
                    n.code.measurement_units_equivalent_codes = got.equivalent_codes.clone();
                } else {
                    n.code.concept_name = got.context.clone();
                    n.code.concept_name_equivalent_codes = got.equivalent_codes.clone();
                }
            }
        }
        let mut prefix = Vec::new();
        if let Some(method) = &c.method {
            prefix.push(self.code(RELATIONSHIP_HAS_CONCEPT_MOD, &context::method(), method)?);
        }
        let sites = self.sites(&c.finding_sites)?;
        let derivation_concept = concept::derivation();
        let (derivation, rest): (Vec<AuthoredNode>, Vec<AuthoredNode>) =
            std::mem::take(&mut n.children)
                .into_iter()
                .partition(|child| child.item.concept_name == derivation_concept);
        prefix.extend(derivation);
        prefix.extend(sites);
        prefix.extend(rest);
        n.children = prefix;
        let rwvm = self.rwvm(c.real_world_value_map.as_ref(), RELATIONSHIP_INFERRED_FROM)?;
        n.children.extend(rwvm);
        let algorithm = self.algorithm(c.algorithm.as_ref())?;
        n.children.extend(algorithm);
        Ok(())
    }
    fn group(
        &mut self,
        n: &mut AuthoredNode,
        c: &MeasurementGroupContext,
        legacy: &MeasurementGroup,
    ) -> Result<(), SrError> {
        if !c.path.is_empty()
            || (!c.measurements.is_empty() && c.measurements.len() != legacy.measurements.len())
        {
            return Err(conflict());
        }
        let template = n
            .identification
            .as_ref()
            .map(|i| i.identifier.clone())
            .unwrap_or_default();
        let sites = &c.finding_sites;
        if !sites.is_empty() && !legacy.finding_sites.is_empty() {
            if sites.len() != legacy.finding_sites.len() {
                return Err(conflict());
            }
            for (site, old) in sites.iter().zip(&legacy.finding_sites) {
                if code_key_for_entry(&site.site.code, &site.site.context)
                    != code_key_for_entry(old, &CodeContext::default())
                {
                    return Err(conflict());
                }
            }
        }
        let (mut tracking, mut finding, mut references, mut old_sites, mut numeric) =
            (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
        let tracking_id = concept::tracking_identifier();
        let tracking_uid = concept::tracking_unique_identifier();
        let finding_concept = concept::finding();
        let site_concept = concept::finding_site();
        for ch in std::mem::take(&mut n.children) {
            if ch.item.concept_name == tracking_id || ch.item.concept_name == tracking_uid {
                tracking.push(ch);
            } else if ch.item.concept_name == finding_concept {
                finding.push(ch);
            } else if ch.item.concept_name == site_concept {
                old_sites.push(ch);
            } else if ch.item.value_type == value::NUM {
                numeric.push(ch);
            } else {
                references.push(ch);
            }
        }
        if numeric.len() != legacy.measurements.len() {
            return Err(conflict());
        }
        if !c.measurements.is_empty() {
            for (node, mc) in numeric.iter_mut().zip(&c.measurements) {
                self.numeric_context(node, mc)?;
            }
        }
        let mut children = Vec::new();
        if !c.activity_session.is_empty() {
            children.push(self.scalar(
                value::TEXT,
                RELATIONSHIP_HAS_OBS_CONTEXT,
                &context::activity_session(),
                &c.activity_session,
            )?);
        }
        children.append(&mut tracking);
        children.append(&mut finding);
        let time_point = self.time_point(c.time_point.as_ref())?;
        children.extend(time_point);
        if template == "1411" {
            children.append(&mut references);
        }
        if let Some(method) = &c.method {
            children.push(self.code(RELATIONSHIP_HAS_CONCEPT_MOD, &context::method(), method)?);
        }
        if !sites.is_empty() {
            let v = self.sites(sites)?;
            children.extend(v);
        } else {
            children.append(&mut old_sites);
        }
        let rwvm = self.rwvm(c.real_world_value_map.as_ref(), RELATIONSHIP_CONTAINS)?;
        children.extend(rwvm);
        let algorithm = self.algorithm(c.algorithm.as_ref())?;
        children.extend(algorithm);
        if template == "1501" {
            children.append(&mut references);
        }
        children.append(&mut numeric);
        let qualitative = self.qualitative(&c.qualitative)?;
        children.extend(qualitative);
        n.children = children;
        Ok(())
    }
}
pub fn build_measurement_report_with_context(
    report: Option<&MeasurementReport>,
    c: &MeasurementReportContext,
    options: MeasurementContextOptions,
) -> Result<DetailedMeasurementReport, SrError> {
    let mut o = normalize_measurement_context_options(options)?;
    let Some(report) = report else {
        return Err(shape());
    };
    if !c.groups.is_empty() && c.groups.len() != report.groups.len() {
        return Err(conflict());
    }
    let library = report.image_library.as_ref().map_or(0, |l| l.len()) as i64;
    let mut count = report.groups.len() as i64
        + report.observers.len() as i64
        + report.procedures_reported.len() as i64
        + library;
    if count > o.read.max_items || library > o.max_evidence {
        return Err(limit());
    }
    for g in &report.groups {
        let measurements = g.measurements.len() as i64;
        let sites = g.finding_sites.len() as i64;
        if measurements > o.read.max_items - count
            || sites > o.read.max_items - count - measurements
        {
            return Err(limit());
        }
        count += measurements + sites;
    }
    let mut file = write_measurement_report(Some(report)).map_err(|_| shape())?;
    file.dataset
        .put(str_elem(Tag::new(0x0008, 0x0005), Vr::CS, "ISO_IR 192"));
    let dataset = &file.dataset;
    let evidence = read_measurement_evidence(dataset, &o)?;
    let refs = read_measurement_reference_metadata(
        dataset,
        o.read.max_items,
        o.read.max_depth,
        o.snapshot.max_items,
    )?;
    let (result, numeric, err) = read_document_numeric_representations(Some(dataset), o.read);
    if let Some(e) = err {
        return Err(e);
    }
    let index = new_metadata_index(&result.template_metadata)?;
    let numbers: HashMap<&str, &NumericRepresentation> = numeric
        .entries
        .iter()
        .map(|e| (e.path.as_str(), &e.representation))
        .collect();
    let details: HashMap<&str, &MeasurementReferenceMetadataEntry> =
        refs.entries.iter().map(|e| (e.path.as_str(), e)).collect();
    let mut a = Author {
        options: o.clone(),
        evidence,
        count: 0,
    };
    fn annotate(
        a: &mut Author,
        items: &[ContentItem],
        parent: &[u32],
        index: &super::template_metadata::MetadataIndex,
        numbers: &HashMap<&str, &NumericRepresentation>,
        details: &HashMap<&str, &MeasurementReferenceMetadataEntry>,
    ) -> Result<Vec<AuthoredNode>, SrError> {
        a.reserve(items.len() as i64)?;
        let mut out = Vec::with_capacity(items.len());
        for (i, item) in items.iter().enumerate() {
            let path = append_path(parent, i as u32 + 1);
            let key = identifier_string(&path);
            let mut v = item.clone();
            v.children = Vec::new();
            let mut n = AuthoredNode {
                item: v,
                code: index.code.get(&key).cloned().unwrap_or_default(),
                identification: index.identification.get(&key).cloned(),
                numeric: numbers.get(key.as_str()).map(|n| (*n).clone()),
                reference: details.get(key.as_str()).map(|r| (*r).clone()),
                children: Vec::new(),
            };
            n.children = annotate(a, &item.children, &path, index, numbers, details)?;
            out.push(n);
        }
        Ok(out)
    }
    let mut document = result.document.unwrap_or_default();
    let mut tree = annotate(&mut a, &document.content, &[1], &index, &numbers, &details)?;
    let imaging = concept::imaging_measurements();
    let empty_group = MeasurementGroupContext::default();
    for node in tree.iter_mut() {
        if node.item.concept_name != imaging {
            continue;
        }
        if node.children.len() != report.groups.len() {
            return Err(conflict());
        }
        for (j, child) in node.children.iter_mut().enumerate() {
            let gc = c.groups.get(j).unwrap_or(&empty_group);
            a.group(child, gc, &report.groups[j])?;
        }
        let mut algorithm = a.algorithm(c.imaging_algorithm.as_ref())?;
        algorithm.append(&mut node.children);
        node.children = algorithm;
    }
    if !c.qualitative.is_empty() || c.qualitative_algorithm.is_some() {
        a.reserve(1)?;
        let mut q = AuthoredNode {
            item: ContentItem {
                value_type: value::CONTAINER.to_string(),
                relationship_type: RELATIONSHIP_CONTAINS.to_string(),
                concept_name: concept::qualitative_evaluations(),
                continuity_of_content: "SEPARATE".to_string(),
                ..Default::default()
            },
            ..Default::default()
        };
        let mut children = a.algorithm(c.qualitative_algorithm.as_ref())?;
        let rows = a.qualitative(&c.qualitative)?;
        children.extend(rows);
        q.children = children;
        tree.push(q);
    }
    let mut metadata = TemplateValidationMetadata::default();
    if let Some(id) = index.identification.get("1") {
        metadata.template_identifications.push(id.clone());
    }
    if let Some(cc) = index.code.get("1") {
        metadata.code_contexts.push(cc.clone());
    }
    let mut final_numeric = NumericRepresentationMetadata::default();
    let mut final_refs = MeasurementReferenceMetadata::default();
    fn flatten(
        nodes: Vec<AuthoredNode>,
        parent: &[u32],
        metadata: &mut TemplateValidationMetadata,
        numeric: &mut NumericRepresentationMetadata,
        refs: &mut MeasurementReferenceMetadata,
    ) -> Vec<ContentItem> {
        let mut out = Vec::with_capacity(nodes.len());
        for (i, n) in nodes.into_iter().enumerate() {
            let path = append_path(parent, i as u32 + 1);
            let key = identifier_string(&path);
            let mut item = n.item;
            item.children = flatten(n.children, &path, metadata, numeric, refs);
            let mut cc = n.code;
            cc.path = key.clone();
            if !code_contexts_empty(&cc) {
                metadata.code_contexts.push(cc);
            }
            if let Some(mut id) = n.identification {
                id.path = key.clone();
                metadata.template_identifications.push(id);
            }
            if let Some(r) = n.numeric {
                numeric.entries.push(NumericRepresentationEntry {
                    path: key.clone(),
                    representation: r,
                });
            }
            if let Some(mut r) = n.reference {
                r.path = key;
                refs.entries.push(r);
            }
            out.push(item);
        }
        out
    }
    document.content = flatten(
        tree,
        &[1],
        &mut metadata,
        &mut final_numeric,
        &mut final_refs,
    );
    document.common = report.common.clone();
    let snapshot = bind_measurement_snapshot(
        Some(&document),
        &metadata,
        &final_numeric,
        &final_refs,
        o.snapshot,
    )?;
    o.character_set = Some(vec!["ISO_IR 192".to_string()]);
    o.value_elements_big_endian = false;
    let mut r =
        bind_measurement_report_with_context(Some(&document), Some(&snapshot), &a.evidence, o)?;
    r.frame_of_reference_uid = report.frame_of_reference_uid.clone();
    Ok(r)
}
