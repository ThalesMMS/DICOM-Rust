use super::read::read_part10;
use super::{Error, InstanceRef, Limits, PatientId, ctx_err, trim_unicode_space};
use crate::codes::dcm;
use crate::dcmtime::ZonedTime;
use crate::derivedio;
use crate::net::ul::{Ctx, validate_ae_title};
use crate::object::writer::write_file;
use crate::object::{Element, Object, Tag, Value, Vr, is_valid_uid, patient_id_with_issuer};
use crate::sr::{
    self, DerivedCommon, Document, ImageReference, KEY_OBJECT_SELECTION_DOCUMENT_STORAGE,
    RELATIONSHIP_CONTAINS, ReadOptions, from_code,
};
use std::collections::{HashMap, HashSet};
pub(super) const MAX_REFERENCES: usize = 512;
pub(super) const MAX_MANIFEST_BYTES: i64 = 4 << 20;
pub(super) const DICOM_MEDIA_TYPE: &str = "application/dicom";
const TAG_SOP_CLASS_UID: Tag = Tag::new(0x0008, 0x0016);
const TAG_SOP_INSTANCE_UID: Tag = Tag::new(0x0008, 0x0018);
const TAG_MODALITY: Tag = Tag::new(0x0008, 0x0060);
const TAG_RETRIEVE_AE_TITLE: Tag = Tag::new(0x0008, 0x0054);
const TAG_PATIENT_ID: Tag = Tag::new(0x0010, 0x0020);
const TAG_STUDY_INSTANCE_UID: Tag = Tag::new(0x0020, 0x000D);
const TAG_SERIES_INSTANCE_UID: Tag = Tag::new(0x0020, 0x000E);
const TAG_MEDIA_STORAGE_SOP_CLASS_UID: Tag = Tag::new(0x0002, 0x0002);
const TAG_MEDIA_STORAGE_SOP_INSTANCE_UID: Tag = Tag::new(0x0002, 0x0003);
const TAG_PATIENT_ISSUER_QUALIFIERS: Tag = Tag::new(0x0010, 0x0024);
const TAG_UNIVERSAL_ENTITY_ID: Tag = Tag::new(0x0040, 0x0032);
const TAG_UNIVERSAL_ENTITY_ID_TYPE: Tag = Tag::new(0x0040, 0x0033);
const TAG_RETRIEVE_LOCATION_UID: Tag = Tag::new(0x0040, 0xE011);
const TAG_RETRIEVE_URL: Tag = Tag::new(0x0008, 0x1190);
const TAG_REQUESTED_EVIDENCE: Tag = Tag::new(0x0040, 0xA375);
const TAG_REFERENCED_SERIES: Tag = Tag::new(0x0008, 0x1115);
const TAG_REFERENCED_SOP: Tag = Tag::new(0x0008, 0x1199);
const TAG_REFERENCED_SOP_CLASS: Tag = Tag::new(0x0008, 0x1150);
const TAG_REFERENCED_SOP_INSTANCE: Tag = Tag::new(0x0008, 0x1155);
const TAG_CONTENT_TEMPLATE: Tag = Tag::new(0x0040, 0xA504);
const TAG_MAPPING_RESOURCE: Tag = Tag::new(0x0008, 0x0105);
const TAG_TEMPLATE_ID: Tag = Tag::new(0x0040, 0xDB00);
pub(super) fn patient_id_from(ds: &Object) -> Option<PatientId> {
    let id = patient_id_with_issuer(ds)?;
    let ids = ds.get_strings(TAG_PATIENT_ID)?;
    let qualifiers = ds.get_sequence(TAG_PATIENT_ISSUER_QUALIFIERS)?;
    if ids.len() != 1 || qualifiers.len() != 1 || id.issuer.universal_entity_id_type != "ISO" {
        return None;
    }
    let p = PatientId {
        id: id.id,
        issuer_oid: id.issuer.universal_entity_id,
    };
    (p.valid()
        && single_string(&qualifiers[0], TAG_UNIVERSAL_ENTITY_ID, &p.issuer_oid)
        && single_string(&qualifiers[0], TAG_UNIVERSAL_ENTITY_ID_TYPE, "ISO"))
    .then_some(p)
}
pub(super) fn single_string(ds: &Object, tag: Tag, want: &str) -> bool {
    ds.get_strings(tag)
        .is_some_and(|values| values.len() == 1 && values[0] == want)
}
fn single_uid(ds: &Object, tag: Tag) -> Option<String> {
    let mut values = ds.get_uids(tag)?;
    if values.len() != 1 || !is_valid_uid(values[0].as_bytes()) {
        return None;
    }
    values.pop()
}
#[derive(Clone, Debug, Default)]
pub struct ManifestOptions {
    pub source: Option<Object>,
    pub sop_instance_uid: String,
    pub series_uid: String,
    pub retrieve_location_uid: String,
    pub retrieve_ae_title: String,
    pub created_at: Option<ZonedTime>,
    pub references: Vec<InstanceRef>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Manifest {
    pub(super) data: Vec<u8>,
    pub(super) patient: PatientId,
    pub(super) uid: String,
    pub(super) study_uid: String,
    pub(super) source_uid: String,
    pub(super) refs: Vec<InstanceRef>,
}
impl Manifest {
    pub fn bytes(&self) -> &[u8] {
        &self.data
    }
    pub fn references(&self) -> &[InstanceRef] {
        &self.refs
    }
    pub fn sop_instance_uid(&self) -> &str {
        &self.uid
    }
}
fn manifest_references(
    ctx: &Ctx,
    study_uid: &str,
    manifest_uid: &str,
    manifest_series: &str,
    refs: &[InstanceRef],
) -> Result<(), Error> {
    if refs.is_empty() {
        return Err(Error::Manifest);
    }
    if refs.len() > MAX_REFERENCES {
        return Err(Error::Limit);
    }
    let mut seen = HashSet::with_capacity(refs.len());
    for r in refs {
        ctx_err(ctx)?;
        if !r.valid()
            || r.study_uid != study_uid
            || r.series_uid == manifest_series
            || r.sop_instance_uid == manifest_uid
            || !seen.insert(r.sop_instance_uid.as_str())
        {
            return Err(Error::Manifest);
        }
    }
    Ok(())
}
fn utc(t: ZonedTime) -> ZonedTime {
    ZonedTime::from_unix_utc(t.unix(), t.nanosecond())
}
pub fn build_manifest(ctx: &Ctx, opts: &ManifestOptions) -> Result<Manifest, Error> {
    let created = opts.created_at.filter(|t| !t.is_zero());
    let (Some(source), Some(created)) = (opts.source.as_ref(), created) else {
        return Err(Error::Manifest);
    };
    if !is_valid_uid(opts.sop_instance_uid.as_bytes())
        || !is_valid_uid(opts.series_uid.as_bytes())
        || opts.series_uid == opts.sop_instance_uid
        || !is_valid_uid(opts.retrieve_location_uid.as_bytes())
        || validate_ae_title(&opts.retrieve_ae_title).is_err()
        || opts.retrieve_ae_title != trim_unicode_space(&opts.retrieve_ae_title)
    {
        return Err(Error::Manifest);
    }
    if patient_id_from(source).is_none() {
        return Err(Error::Mapping);
    }
    bound_source_metadata(ctx, source)?;
    let study_uid = match single_uid(source, TAG_STUDY_INSTANCE_UID) {
        Some(uid) if uid != opts.sop_instance_uid && uid != opts.series_uid => uid,
        _ => return Err(Error::Manifest),
    };
    manifest_references(
        ctx,
        &study_uid,
        &opts.sop_instance_uid,
        &opts.series_uid,
        &opts.references,
    )?;
    let image_refs: Vec<ImageReference> = opts
        .references
        .iter()
        .map(|r| ImageReference {
            study_instance_uid: r.study_uid.clone(),
            series_instance_uid: r.series_uid.clone(),
            sop_class_uid: r.sop_class_uid.clone(),
            sop_instance_uid: r.sop_instance_uid.clone(),
            frames: Vec::new(),
        })
        .collect();
    let created = utc(created);
    let mut doc: Document =
        sr::kos::new_key_object_selection(&opts.sop_instance_uid, &image_refs, None);
    doc.title = from_code(dcm::Manifest);
    doc.series_instance_uid = opts.series_uid.clone();
    let (year, month, day) = created.date();
    doc.content_date = format!("{year:04}{month:02}{day:02}");
    doc.content_time = format!(
        "{:02}{:02}{:02}",
        created.hour(),
        created.minute(),
        created.second()
    );
    doc.common = DerivedCommon {
        source: Some(source.clone()),
        equipment: None,
        now: Some(created),
    };
    let mut ds = doc.dataset().map_err(|_| Error::Manifest)?;
    let mut studies = ds.get_sequence(TAG_REQUESTED_EVIDENCE).unwrap_or_default();
    if studies.len() != 1 {
        return Err(Error::Manifest);
    }
    let mut series = studies[0]
        .get_sequence(TAG_REFERENCED_SERIES)
        .unwrap_or_default();
    for item in &mut series {
        item.put(derivedio::str_elem(
            TAG_RETRIEVE_AE_TITLE,
            Vr::AE,
            &opts.retrieve_ae_title,
        ));
        item.put(derivedio::ui(
            TAG_RETRIEVE_LOCATION_UID,
            &opts.retrieve_location_uid,
        ));
    }
    studies[0].set_sequence(TAG_REFERENCED_SERIES, &items(&series));
    ds.set_sequence(TAG_REQUESTED_EVIDENCE, &items(&studies));
    let template = derivedio::object(vec![
        derivedio::str_elem(TAG_MAPPING_RESOURCE, Vr::CS, "DCMR"),
        derivedio::str_elem(TAG_TEMPLATE_ID, Vr::CS, "2010"),
    ]);
    ds.set_sequence(TAG_CONTENT_TEMPLATE, &[Some(&template)]);
    let file = derivedio::file(
        KEY_OBJECT_SELECTION_DOCUMENT_STORAGE,
        &opts.sop_instance_uid,
        Some(ds),
    )
    .map_err(|_| Error::Manifest)?;
    ctx_err(ctx)?;
    let out = write_file(&file).map_err(|e| Error::Other(e.to_string()))?;
    if out.len() as i64 > MAX_MANIFEST_BYTES {
        return Err(Error::Limit);
    }
    parse_owned(ctx, out)
}
fn items(objects: &[Object]) -> Vec<Option<&Object>> {
    objects.iter().map(Some).collect()
}
fn is_root_pixel_data(tag: Tag) -> bool {
    tag == Tag::new(0x7FE0, 0x0010)
        || tag == Tag::new(0x7FE0, 0x0008)
        || tag == Tag::new(0x7FE0, 0x0009)
}
fn bound_source_metadata(ctx: &Ctx, source: &Object) -> Result<(), Error> {
    if source.len() > 4096 {
        return Err(Error::Limit);
    }
    struct Budget {
        remaining: i64,
        items: i64,
    }
    fn visit(ctx: &Ctx, elements: &[Element], depth: usize, b: &mut Budget) -> Result<(), Error> {
        if depth > 12 {
            return Err(Error::Limit);
        }
        for item in elements {
            ctx_err(ctx)?;
            b.items -= 1;
            if b.items < 0 {
                return Err(Error::Limit);
            }
            if depth == 0 && is_root_pixel_data(item.tag()) {
                continue;
            }
            b.remaining -= 12;
            match &item.value {
                Value::Sequence(seq) => {
                    if seq.len() as i64 > b.items {
                        return Err(Error::Limit);
                    }
                    for child in seq {
                        visit(ctx, &child.elements, depth + 1, b)?;
                    }
                }
                Value::Raw(raw) => b.remaining -= raw.len() as i64,
                Value::Strings(values) => {
                    let total = values.iter().map(|v| v.len() as i64).sum::<i64>()
                        + values.len().saturating_sub(1) as i64;
                    b.remaining -= total + total % 2;
                }
                Value::Nil | Value::Fragments { .. } | Value::BulkData(_) => {
                    return Err(Error::Manifest);
                }
            }
            if b.remaining < 0 {
                return Err(Error::Limit);
            }
        }
        Ok(())
    }
    let mut budget = Budget {
        remaining: MAX_MANIFEST_BYTES,
        items: 4096,
    };
    visit(ctx, source.elements(), 0, &mut budget)
}
pub fn parse_manifest(ctx: &Ctx, data: &[u8]) -> Result<Manifest, Error> {
    if data.len() as i64 > MAX_MANIFEST_BYTES {
        return Err(Error::Limit);
    }
    parse_owned(ctx, data.to_vec())
}
fn parse_owned(ctx: &Ctx, data: Vec<u8>) -> Result<Manifest, Error> {
    ctx_err(ctx)?;
    let limits = Limits {
        document_bytes: MAX_MANIFEST_BYTES,
        total_bytes: MAX_MANIFEST_BYTES,
        metadata_bytes: MAX_MANIFEST_BYTES,
    };
    let file = match read_part10(ctx, &data, limits) {
        Ok(file) => file,
        Err(_) => {
            ctx_err(ctx)?;
            return Err(Error::Manifest);
        }
    };
    let ds = &file.dataset;
    let uid = single_uid(ds, TAG_SOP_INSTANCE_UID);
    let meta_uid = single_uid(&file.meta, TAG_MEDIA_STORAGE_SOP_INSTANCE_UID);
    let class = single_uid(ds, TAG_SOP_CLASS_UID);
    let meta_class = single_uid(&file.meta, TAG_MEDIA_STORAGE_SOP_CLASS_UID);
    let (Some(uid), Some(meta_uid), Some(class), Some(meta_class)) =
        (uid, meta_uid, class, meta_class)
    else {
        return Err(Error::Manifest);
    };
    if meta_uid != uid
        || class != KEY_OBJECT_SELECTION_DOCUMENT_STORAGE
        || meta_class != class
        || !single_string(ds, TAG_MODALITY, "KO")
    {
        return Err(Error::Manifest);
    }
    let Some(patient) = patient_id_from(ds) else {
        return Err(Error::Mapping);
    };
    let study_uid = single_uid(ds, TAG_STUDY_INSTANCE_UID);
    let manifest_series = single_uid(ds, TAG_SERIES_INSTANCE_UID);
    let (Some(study_uid), Some(manifest_series)) = (study_uid, manifest_series) else {
        return Err(Error::Manifest);
    };
    if uid == study_uid || uid == manifest_series || study_uid == manifest_series {
        return Err(Error::Manifest);
    }
    if let Some(template) = ds.get_sequence(TAG_CONTENT_TEMPLATE)
        && (template.len() != 1
            || !single_string(&template[0], TAG_MAPPING_RESOURCE, "DCMR")
            || !single_string(&template[0], TAG_TEMPLATE_ID, "2010"))
    {
        return Err(Error::Manifest);
    }
    let (refs, source_uid) = manifest_evidence(ds, &study_uid)?;
    manifest_references(ctx, &study_uid, &uid, &manifest_series, &refs)?;
    let (result, err) = sr::read_document_with_options(
        Some(ds),
        ReadOptions {
            max_depth: 8,
            max_items: MAX_REFERENCES as i64 + 1,
            max_references: MAX_REFERENCES as i64,
            ..ReadOptions::default()
        },
    );
    let manifest_code = dcm::Manifest;
    match result.document {
        Some(doc)
            if err.is_none()
                && doc.title.code_value == manifest_code.value
                && doc.title.coding_scheme_designator == manifest_code.scheme =>
        {
            match_manifest_content(&doc, &refs)?
        }
        _ => return Err(Error::Manifest),
    }
    Ok(Manifest {
        data,
        patient,
        uid,
        study_uid,
        source_uid,
        refs,
    })
}
fn manifest_evidence(ds: &Object, study_uid: &str) -> Result<(Vec<InstanceRef>, String), Error> {
    let studies = match ds.get_sequence(TAG_REQUESTED_EVIDENCE) {
        Some(s) if s.len() == 1 && single_string(&s[0], TAG_STUDY_INSTANCE_UID, study_uid) => s,
        _ => return Err(Error::Manifest),
    };
    let series = match studies[0].get_sequence(TAG_REFERENCED_SERIES) {
        Some(s) if !s.is_empty() => s,
        _ => return Err(Error::Manifest),
    };
    if series.len() > MAX_REFERENCES {
        return Err(Error::Limit);
    }
    let mut refs = Vec::new();
    let mut source_uid = String::new();
    let mut ae_title = String::new();
    let mut seen = HashSet::with_capacity(series.len());
    for item in &series {
        let series_uid = single_uid(item, TAG_SERIES_INSTANCE_UID);
        let location = single_uid(item, TAG_RETRIEVE_LOCATION_UID);
        let ae = item.get_string(TAG_RETRIEVE_AE_TITLE);
        let (Some(series_uid), Some(location), Some(ae)) = (series_uid, location, ae) else {
            return Err(Error::Manifest);
        };
        if seen.contains(&series_uid)
            || !single_string(item, TAG_RETRIEVE_AE_TITLE, &ae)
            || validate_ae_title(&ae).is_err()
            || (!source_uid.is_empty() && (location != source_uid || ae != ae_title))
        {
            return Err(Error::Manifest);
        }
        if item
            .get_string(TAG_RETRIEVE_URL)
            .is_some_and(|url| !url.is_empty())
        {
            return Err(Error::Manifest);
        }
        let sops = match item.get_sequence(TAG_REFERENCED_SOP) {
            Some(s) if !s.is_empty() => s,
            _ => return Err(Error::Manifest),
        };
        if sops.len() > MAX_REFERENCES - refs.len() {
            return Err(Error::Limit);
        }
        for sop in &sops {
            let class = single_uid(sop, TAG_REFERENCED_SOP_CLASS);
            let instance = single_uid(sop, TAG_REFERENCED_SOP_INSTANCE);
            let (Some(class), Some(instance)) = (class, instance) else {
                return Err(Error::Manifest);
            };
            refs.push(InstanceRef {
                study_uid: study_uid.to_string(),
                series_uid: series_uid.clone(),
                sop_class_uid: class,
                sop_instance_uid: instance,
            });
        }
        seen.insert(series_uid);
        source_uid = location;
        ae_title = ae;
    }
    Ok((refs, source_uid))
}
fn match_manifest_content(doc: &Document, refs: &[InstanceRef]) -> Result<(), Error> {
    if doc.content.len() != refs.len() {
        return Err(Error::Manifest);
    }
    let mut expected: HashMap<&str, &InstanceRef> = refs
        .iter()
        .map(|r| (r.sop_instance_uid.as_str(), r))
        .collect();
    for item in &doc.content {
        let image = &item.image;
        let Some(r) = expected.remove(image.sop_instance_uid.as_str()) else {
            return Err(Error::Manifest);
        };
        if item.value_type != sr::value::IMAGE
            || item.relationship_type != RELATIONSHIP_CONTAINS
            || image.sop_class_uid != r.sop_class_uid
            || image.study_instance_uid != r.study_uid
            || image.series_instance_uid != r.series_uid
            || !image.frames.is_empty()
            || !item.children.is_empty()
        {
            return Err(Error::Manifest);
        }
    }
    Ok(())
}
