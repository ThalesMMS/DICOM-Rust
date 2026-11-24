use super::image_library::{
    ImageLibraryEntry, ImageLibraryGroup, ImageLibraryOptions, default_image_library_options,
    image_library_error, normalize, valid_numeric_path,
};
use super::image_library_structured::{library_applies, valid_uid, validate_image_library};
use super::numeric_representation::{
    NumericRepresentation, NumericRepresentationEntry, NumericRepresentationMetadata,
};
use super::read::{Decoder, decode_code_with_metadata};
use super::template_catalog::CatalogRow;
use super::template_catalog_tables::STANDARD_TEMPLATE_CATALOG;
use super::template_metadata::{
    ContentItemCodeContexts, TemplateValidationMetadata, code_contexts_empty,
};
use super::{
    CodedEntry, ContentItem, ImageReference, Measurement, RELATIONSHIP_HAS_ACQ_CONTEXT, SrError,
    value,
};
use crate::object::{Element, Object, Tag, Value, Vr};
use std::collections::HashSet;
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ImageLibraryExtractionFinding {
    pub template: i64,
    pub row: String,
    pub reason: String,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ImageLibraryExtraction {
    pub entry: ImageLibraryEntry,
    pub metadata: TemplateValidationMetadata,
    pub numeric: NumericRepresentationMetadata,
    pub findings: Vec<ImageLibraryExtractionFinding>,
}
const fn tag(g: u16, e: u16) -> Tag {
    Tag::new(g, e)
}
const NO_TAG: Tag = Tag::new(0, 0);
type Rule = (i64, &'static str, Tag, usize, usize, &'static str, Tag);
const RULES: [Rule; 54] = [
    (1602, "1", tag(0x0008, 0x0060), 0, 1, "", NO_TAG),
    (1602, "2", tag(0x0008, 0x2218), 0, 1, "", NO_TAG),
    (1602, "2b", tag(0x0018, 0x0015), 0, 1, "", NO_TAG),
    (1602, "3", tag(0x0020, 0x0062), 0, 1, "", NO_TAG),
    (1602, "4", tag(0x0008, 0x0020), 0, 1, "", NO_TAG),
    (1602, "5", tag(0x0008, 0x0030), 0, 1, "", NO_TAG),
    (1602, "5a", tag(0x0020, 0x000e), 0, 1, "", NO_TAG),
    (1602, "5b", tag(0x0020, 0x0011), 0, 1, "", NO_TAG),
    (1602, "5c", tag(0x0008, 0x103e), 0, 1, "", NO_TAG),
    (1602, "5e", tag(0x0008, 0x0021), 0, 1, "", NO_TAG),
    (1602, "5f", tag(0x0008, 0x0031), 0, 1, "", NO_TAG),
    (1602, "5g", tag(0x0020, 0x1209), 0, 1, "{instances}", NO_TAG),
    (1602, "6", tag(0x0008, 0x0023), 0, 1, "", NO_TAG),
    (1602, "7", tag(0x0008, 0x0033), 0, 1, "", NO_TAG),
    (1602, "8", tag(0x0008, 0x0022), 0, 1, "", NO_TAG),
    (1602, "9", tag(0x0008, 0x0032), 0, 1, "", NO_TAG),
    (1602, "10", tag(0x0020, 0x0052), 0, 1, "", NO_TAG),
    (1602, "11", tag(0x0028, 0x0010), 0, 1, "{pixels}", NO_TAG),
    (1602, "12", tag(0x0028, 0x0011), 0, 1, "{pixels}", NO_TAG),
    (1602, "12a", tag(0x0020, 0x0013), 0, 1, "", NO_TAG),
    (1602, "12b", tag(0x0028, 0x0008), 0, 1, "{frames}", NO_TAG),
    (1603, "1", tag(0x0054, 0x0220), 0, 1, "", NO_TAG),
    (1603, "3", tag(0x0020, 0x0020), 0, 2, "", NO_TAG),
    (1603, "4", tag(0x0020, 0x0020), 1, 2, "", NO_TAG),
    (1603, "5", tag(0x0018, 0x1164), 1, 2, "mm", NO_TAG),
    (1603, "6", tag(0x0018, 0x1164), 0, 2, "mm", NO_TAG),
    (1603, "7", tag(0x0018, 0x1510), 0, 1, "deg", NO_TAG),
    (1603, "8", tag(0x0018, 0x1511), 0, 1, "deg", NO_TAG),
    (1604, "1", tag(0x0028, 0x0030), 1, 2, "mm", NO_TAG),
    (1604, "2", tag(0x0028, 0x0030), 0, 2, "mm", NO_TAG),
    (1604, "3", tag(0x0018, 0x0088), 0, 1, "mm", NO_TAG),
    (1604, "4", tag(0x0018, 0x0050), 0, 1, "mm", NO_TAG),
    (1604, "5", tag(0x0020, 0x0032), 0, 3, "mm", NO_TAG),
    (1604, "6", tag(0x0020, 0x0032), 1, 3, "mm", NO_TAG),
    (1604, "7", tag(0x0020, 0x0032), 2, 3, "mm", NO_TAG),
    (1604, "8", tag(0x0020, 0x0037), 0, 6, "{-1:1}", NO_TAG),
    (1604, "9", tag(0x0020, 0x0037), 1, 6, "{-1:1}", NO_TAG),
    (1604, "10", tag(0x0020, 0x0037), 2, 6, "{-1:1}", NO_TAG),
    (1604, "11", tag(0x0020, 0x0037), 3, 6, "{-1:1}", NO_TAG),
    (1604, "12", tag(0x0020, 0x0037), 4, 6, "{-1:1}", NO_TAG),
    (1604, "13", tag(0x0020, 0x0037), 5, 6, "{-1:1}", NO_TAG),
    (1605, "1", tag(0x0018, 0x9302), 0, 1, "", NO_TAG),
    (1605, "2", tag(0x0018, 0x9315), 0, 1, "", NO_TAG),
    (1606, "1", tag(0x0018, 0x9005), 0, 1, "", NO_TAG),
    (1606, "2", tag(0x0018, 0x0087), 0, 1, "T", NO_TAG),
    (1606, "6", tag(0x0018, 0x9087), 0, 1, "s/mm2", NO_TAG),
    (
        1607,
        "1",
        tag(0x0054, 0x0300),
        0,
        1,
        "",
        tag(0x0054, 0x0016),
    ),
    (
        1607,
        "2",
        tag(0x0054, 0x0304),
        0,
        1,
        "",
        tag(0x0054, 0x0016),
    ),
    (
        1607,
        "3",
        tag(0x0018, 0x1075),
        0,
        1,
        "s",
        tag(0x0054, 0x0016),
    ),
    (1607, "3b", tag(0x0008, 0x0050), 0, 1, "", NO_TAG),
    (
        1607,
        "4",
        tag(0x0018, 0x1078),
        0,
        1,
        "",
        tag(0x0054, 0x0016),
    ),
    (
        1607,
        "5",
        tag(0x0018, 0x1079),
        0,
        1,
        "",
        tag(0x0054, 0x0016),
    ),
    (
        1607,
        "6",
        tag(0x0018, 0x1071),
        0,
        1,
        "cm3",
        tag(0x0054, 0x0016),
    ),
    (
        1607,
        "7",
        tag(0x0018, 0x1074),
        0,
        1,
        "Bq",
        tag(0x0054, 0x0016),
    ),
];
fn catalog_row(tid: i64, label: &str) -> Option<&'static CatalogRow> {
    STANDARD_TEMPLATE_CATALOG
        .iter()
        .filter(|t| t.id == tid)
        .flat_map(|t| t.rows.iter())
        .find(|r| r.label == label)
}
fn err(code: &str) -> SrError {
    image_library_error(code, "")
}
struct Budget {
    items: i64,
    bytes: i64,
    o: ImageLibraryOptions,
}
impl Budget {
    fn resident(&mut self, e: &Element) -> Result<(), SrError> {
        self.items += 1;
        if self.items > self.o.max_items {
            return Err(err("resource-limit"));
        }
        let mut n: i64 = 0;
        match &e.value {
            Value::Raw(v) => n = v.len() as i64,
            Value::Strings(v) => {
                for s in v {
                    if s.len() as i64 > self.o.max_value_bytes - self.bytes - n {
                        return Err(err("resource-limit"));
                    }
                    n += s.len() as i64;
                }
            }
            Value::Sequence(items) => {
                if items.len() as i64 > self.o.max_items - self.items {
                    return Err(err("resource-limit"));
                }
            }
            _ => return Err(err("nonresident-or-custom-value")),
        }
        if n > self.o.max_value_bytes - self.bytes {
            return Err(err("resource-limit"));
        }
        self.bytes += n;
        Ok(())
    }
    fn strings(&mut self, obj: &Object, t: Tag) -> Result<Option<Vec<String>>, SrError> {
        let Some(e) = obj.get(t) else {
            return Ok(None);
        };
        self.resident(e)?;
        obj.lookup_strings(t)
            .map(Some)
            .map_err(|_| err("source-value"))
    }
    fn sequence_elements(
        &mut self,
        items: &[crate::object::DataSet],
        depth: i64,
    ) -> Result<(), SrError> {
        if depth > self.o.max_depth * 4 + 4 {
            return Err(err("resource-limit"));
        }
        for item in items {
            if item.elements.len() as i64 > self.o.max_items - self.items {
                return Err(err("resource-limit"));
            }
            for e in &item.elements {
                self.resident(e)?;
                if let Value::Sequence(nested) = &e.value {
                    self.sequence_elements(nested, depth + 1)?;
                }
            }
        }
        Ok(())
    }
    fn sequence(&mut self, obj: &Object, t: Tag) -> Result<Option<Vec<Object>>, SrError> {
        let Some(e) = obj.get(t) else {
            return Ok(None);
        };
        self.resident(e)?;
        if e.vr() != Vr::SQ {
            return Err(err("source-sequence"));
        }
        let Value::Sequence(items) = &e.value else {
            return Err(err("source-sequence"));
        };
        self.sequence_elements(items, 1)?;
        obj.get_sequence(t)
            .map(Some)
            .ok_or_else(|| err("source-sequence"))
    }
}
fn library_defined_term(tid: i64, row: &str, term: &str) -> Option<CodedEntry> {
    let c = |v: &str, s: &str, m: &str| Some(CodedEntry::new(v, s, m));
    match (tid, row, term) {
        (1602, "1", _) => c(term, "DCM", term),
        (1602, "3", "R") => c("24028007", "SCT", "Right"),
        (1602, "3", "L") => c("7771000", "SCT", "Left"),
        (1602, "3", "B") => c("51440002", "SCT", "Bilateral"),
        (1602, "3", "U") => c("66459002", "SCT", "Unilateral"),
        (1605, "1", "SEQUENCED") => c("113804", "DCM", "Sequenced Acquisition"),
        (1605, "1", "CONSTANT_ANGLE") => c("113805", "DCM", "Constant Angle Acquisition"),
        (1605, "1", "STATIONARY") => c("113806", "DCM", "Stationary Acquisition"),
        (1605, "1", "FREE") => c("113807", "DCM", "Free Acquisition"),
        (1605, "1", "SPIRAL") => c("116152004", "SCT", "Spiral Acquisition"),
        (1605, "2", "FILTER_BACK_PROJ") => c("113962", "DCM", "Filtered Back Projection"),
        (1605, "2", "ITERATIVE") => c("113963", "DCM", "Iterative Reconstruction"),
        _ => None,
    }
}
fn parse_decimal_int(s: &str) -> Option<i64> {
    let digits = s.strip_prefix(['+', '-']).unwrap_or(s);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}
pub fn extract_image_library_entry(
    source: &Object,
    frames: &[i64],
    options: ImageLibraryOptions,
) -> Result<ImageLibraryExtraction, SrError> {
    let mut out = ImageLibraryExtraction::default();
    let o = normalize(options)?;
    let mut b = Budget {
        items: 0,
        bytes: 0,
        o,
    };
    if let Some(e) = source.get(tag(0x0008, 0x0005)) {
        b.resident(e)?;
    }
    if frames.len() as i64 > o.max_items {
        return Err(err("resource-limit"));
    }
    let mut r = ImageReference {
        frames: frames.to_vec(),
        ..Default::default()
    };
    for (t, which) in [
        (tag(0x0008, 0x0016), 0),
        (tag(0x0008, 0x0018), 1),
        (tag(0x0020, 0x000d), 2),
        (tag(0x0020, 0x000e), 3),
    ] {
        if let Some(v) = b.strings(source, t)? {
            if v.len() != 1 || !valid_uid(&v[0]) {
                return Err(err("source-uid"));
            }
            let dst = match which {
                0 => &mut r.sop_class_uid,
                1 => &mut r.sop_instance_uid,
                2 => &mut r.study_instance_uid,
                _ => &mut r.series_instance_uid,
            };
            *dst = v[0].clone();
        }
    }
    if !valid_uid(&r.sop_class_uid) || !valid_uid(&r.sop_instance_uid) {
        return Err(err("image-reference"));
    }
    let mut frame_limit = 0;
    if let Some(values) = b.strings(source, tag(0x0028, 0x0008))? {
        if values.len() != 1 {
            return Err(err("source-multiplicity"));
        }
        frame_limit = match parse_decimal_int(&values[0]) {
            Some(n) if n >= 1 => n,
            _ => return Err(err("frame-number")),
        };
    }
    if frames
        .iter()
        .any(|&f| f < 1 || (frame_limit > 0 && f > frame_limit))
    {
        return Err(err("frame-number"));
    }
    let mut modality = String::new();
    if let Some(v) = b.strings(source, tag(0x0008, 0x0060))? {
        if v.len() != 1 || v[0].is_empty() {
            return Err(err("source-modality"));
        }
        modality = v[0].clone();
    }
    let mut scopes: Vec<Object> = vec![source.clone()];
    if let Some(shared) = b.sequence(source, tag(0x5200, 0x9229))? {
        if shared.len() != 1 {
            return Err(err("shared-cardinality"));
        }
        scopes.extend(shared);
    }
    if let Some(per) = b.sequence(source, tag(0x5200, 0x9230))? {
        if frames.len() != 1 {
            return Err(err("explicit-single-frame-required"));
        }
        if frames[0] > per.len() as i64 {
            return Err(err("frame-number"));
        }
        scopes.push(per[frames[0] as usize - 1].clone());
    }
    let macros = [
        tag(0x0028, 0x9110),
        tag(0x0020, 0x9113),
        tag(0x0020, 0x9116),
        tag(0x0018, 0x9301),
        tag(0x0018, 0x9314),
        tag(0x0018, 0x9226),
        tag(0x0018, 0x9117),
    ];
    for scope in scopes.clone() {
        for t in macros {
            if let Some(items) = b.sequence(&scope, t)? {
                if items.len() != 1 {
                    return Err(err("macro-cardinality"));
                }
                scopes.extend(items);
            }
        }
    }
    let mut seen: HashSet<String> = HashSet::new();
    for &(tid, label, rule_tag, component, multiplicity, unit, sequence) in &RULES {
        if !library_applies(tid, &modality) {
            continue;
        }
        let row = catalog_row(tid, label);
        let mut found: Option<(ContentItem, ContentItemCodeContexts)> = None;
        for scope in &scopes {
            let holder;
            let target: &Object = if sequence != NO_TAG {
                let Some(items) = b.sequence(scope, sequence)? else {
                    continue;
                };
                if items.len() != 1 {
                    return Err(err("radiopharmaceutical-cardinality"));
                }
                holder = items.into_iter().next().unwrap_or_default();
                &holder
            } else {
                scope
            };
            let Some(e) = target.get(rule_tag) else {
                continue;
            };
            b.resident(e)?;
            let value_type = row.map_or("", |r| r.value_type);
            let mut code_context = ContentItemCodeContexts::default();
            let mut item = ContentItem {
                value_type: value_type.to_string(),
                relationship_type: RELATIONSHIP_HAS_ACQ_CONTEXT.to_string(),
                concept_name: row.map_or_else(CodedEntry::default, |r| {
                    CodedEntry::new(r.code.0, r.code.1, r.code.2)
                }),
                ..Default::default()
            };
            if value_type == value::NUM {
                let nums: Vec<f64> = if matches!(e.vr(), Vr::US | Vr::IS) {
                    let ints = target
                        .get_ints(rule_tag)
                        .map_err(|_| err("source-number"))?;
                    ints.into_iter().map(|n| n as f64).collect()
                } else {
                    target
                        .get_floats(rule_tag)
                        .map_err(|_| err("source-number"))?
                };
                if nums.len() != multiplicity {
                    return Err(err("source-multiplicity"));
                }
                let n = nums[component];
                if !n.is_finite() {
                    return Err(err("source-number"));
                }
                item.measurement = Some(Measurement {
                    value: n,
                    units: CodedEntry::new(unit, "UCUM", unit),
                });
            } else if value_type == value::CODE && e.vr() == Vr::SQ {
                match b.sequence(target, rule_tag)? {
                    Some(items) if items.len() == 1 => {}
                    _ => return Err(err("code-cardinality")),
                }
                let mut dec = Decoder::bounded(0, o.max_items, 0, 0);
                let (code, context, equivalents, _) =
                    decode_code_with_metadata(&mut dec, Some(target), rule_tag);
                item.code = code;
                code_context.value = context;
                code_context.value_equivalent_codes = equivalents;
                if dec.err.is_some() || item.code.empty() {
                    return Err(err("code-value"));
                }
            } else {
                let strs = match target.lookup_strings(rule_tag) {
                    Ok(s) if s.len() == multiplicity => s,
                    _ => return Err(err("source-multiplicity")),
                };
                let s = strs[component].clone();
                if s.is_empty() {
                    continue;
                }
                match value_type {
                    value::TEXT => item.text = s,
                    value::DATE => item.date = s,
                    value::TIME => item.time = s,
                    value::DATETIME => item.date_time = s,
                    value::UIDREF => item.uid = s,
                    value::CODE => match library_defined_term(tid, label, &s) {
                        Some(code) => item.code = code,
                        None => {
                            out.findings.push(ImageLibraryExtractionFinding {
                                template: tid,
                                row: label.to_string(),
                                reason: "unmapped-defined-term".into(),
                            });
                            continue;
                        }
                    },
                    _ => {}
                }
            }
            if let Some((f, fc)) = &found
                && (*f != item || *fc != code_context)
            {
                return Err(err("source-conflict"));
            }
            found = Some((item, code_context));
        }
        if let Some((item, mut context)) = found {
            let numeric = item.measurement.as_ref().map(|m| m.value);
            out.entry.descriptors.push(item);
            let path = format!("1.{}", out.entry.descriptors.len());
            if !code_contexts_empty(&context) {
                context.path = path.clone();
                out.metadata.code_contexts.push(context);
            }
            if let Some(v) = numeric {
                let decimal = crate::object::element_setters::format_ds(v)
                    .map_err(|_| err("source-number"))?;
                out.numeric.entries.push(NumericRepresentationEntry {
                    path,
                    representation: NumericRepresentation {
                        decimal_string: decimal,
                        has_decimal_string: true,
                        floating_point: v,
                        has_floating_point: true,
                        ..Default::default()
                    },
                });
            }
            seen.insert(format!("{tid}:{label}"));
        }
    }
    out.entry.image = r;
    for t in STANDARD_TEMPLATE_CATALOG.iter() {
        if t.id < 1602 || t.id > 1607 || !library_applies(t.id, &modality) {
            continue;
        }
        for row in t.rows {
            if row.include != 0 || row.level > 0 {
                continue;
            }
            if !seen.contains(&format!("{}:{}", t.id, row.label)) {
                out.findings.push(ImageLibraryExtractionFinding {
                    template: t.id,
                    row: row.label.to_string(),
                    reason: "absent-or-not-directly-extractable".into(),
                });
            }
        }
    }
    validate_image_library(
        &[],
        &[ImageLibraryGroup {
            entries: vec![out.entry.clone()],
            ..Default::default()
        }],
        o,
    )?;
    Ok(out)
}
fn rooted(path: &str) -> bool {
    valid_numeric_path(path) && (path == "1" || path.starts_with("1."))
}
impl ImageLibraryExtraction {
    pub fn metadata_at(
        &self,
        image_path: &str,
    ) -> Result<(TemplateValidationMetadata, NumericRepresentationMetadata), SrError> {
        if !valid_numeric_path(image_path) {
            return Err(err("metadata-path"));
        }
        let count = self.metadata.code_contexts.len()
            + self.metadata.template_identifications.len()
            + self.numeric.entries.len();
        if count as i64 > default_image_library_options().max_items {
            return Err(err("resource-limit"));
        }
        if self.metadata.code_contexts.iter().any(|c| !rooted(&c.path))
            || self.numeric.entries.iter().any(|n| !rooted(&n.path))
            || self
                .metadata
                .template_identifications
                .iter()
                .any(|i| !rooted(&i.path))
        {
            return Err(err("metadata-path"));
        }
        let mut m = self.metadata.clone();
        let mut n = self.numeric.clone();
        for c in &mut m.code_contexts {
            c.path = format!("{image_path}{}", &c.path[1..]);
        }
        for i in &mut m.template_identifications {
            i.path = format!("{image_path}{}", &i.path[1..]);
        }
        for e in &mut n.entries {
            e.path = format!("{image_path}{}", &e.path[1..]);
        }
        Ok((m, n))
    }
}
