use super::measurement_context::{
    DetailedMeasurementReport, MEASUREMENT_CONTEXT_CONFLICT, MEASUREMENT_CONTEXT_EVIDENCE,
    MEASUREMENT_CONTEXT_LIMIT, MeasurementContextOptions, MeasurementReferenceEvidence,
    measurement_context_error,
};
use super::numeric_representation::resident_string;
use super::reference::{append_path, identifier_string};
use super::{ContentItem, SrError, tags as tg, value};
use crate::object::{Object, Tag, Value, Vr};
use std::collections::HashMap;
pub(crate) fn valid_uid(uid: &str) -> bool {
    crate::object::is_valid_uid(uid.as_bytes())
}
pub(crate) fn admit_measurement_evidence(
    input: &[MeasurementReferenceEvidence],
    o: &MeasurementContextOptions,
) -> Result<Vec<MeasurementReferenceEvidence>, SrError> {
    if input.len() as i64 > o.max_evidence {
        return Err(measurement_context_error(MEASUREMENT_CONTEXT_LIMIT, ""));
    }
    let mut out = Vec::with_capacity(input.len());
    let mut seen: HashMap<&str, &MeasurementReferenceEvidence> = HashMap::new();
    for e in input {
        for u in [
            &e.study_instance_uid,
            &e.series_instance_uid,
            &e.sop_class_uid,
            &e.sop_instance_uid,
        ] {
            if !valid_uid(u) {
                return Err(measurement_context_error(MEASUREMENT_CONTEXT_EVIDENCE, ""));
            }
        }
        if let Some(previous) = seen.get(e.sop_instance_uid.as_str()) {
            if *previous != e {
                return Err(measurement_context_error(MEASUREMENT_CONTEXT_CONFLICT, ""));
            }
            continue;
        }
        seen.insert(&e.sop_instance_uid, e);
        out.push(e.clone());
    }
    Ok(out)
}
pub(crate) fn read_measurement_evidence(
    obj: &Object,
    o: &MeasurementContextOptions,
) -> Result<Vec<MeasurementReferenceEvidence>, SrError> {
    let evidence = || measurement_context_error(MEASUREMENT_CONTEXT_EVIDENCE, "");
    let mut used = 0i64;
    let mut sequence =
        |parent: &Object, tag: Tag, optional: bool| -> Result<Vec<Object>, SrError> {
            let element = parent.get(tag);
            if element.is_none() && optional {
                return Ok(Vec::new());
            }
            let Some(element) = element else {
                return Err(evidence());
            };
            let Value::Sequence(v) = &element.value else {
                return Err(evidence());
            };
            if element.vr() != Vr::SQ {
                return Err(evidence());
            }
            if v.len() as i64 > o.max_evidence_items - used {
                return Err(measurement_context_error(MEASUREMENT_CONTEXT_LIMIT, ""));
            }
            used += v.len() as i64;
            parent.get_sequence(tag).ok_or_else(evidence)
        };
    let uid = |parent: &Object, tag: Tag| -> Result<String, SrError> {
        let Some(element) = parent.get(tag) else {
            return Err(evidence());
        };
        if element.vr() != Vr::UI {
            return Err(evidence());
        }
        let raw = resident_string(element, Vr::UI).map_err(|_| evidence())?;
        if raw.len() > 65 {
            return Err(evidence());
        }
        let u = crate::object::text::normalize_uid(&raw).to_string();
        if !valid_uid(&u) {
            return Err(evidence());
        }
        Ok(u)
    };
    let studies = sequence(obj, tg::CURRENT_REQUESTED_PROCEDURE_EVIDENCE, true)?;
    let mut refs: Vec<MeasurementReferenceEvidence> = Vec::new();
    for study in &studies {
        let su = uid(study, tg::STUDY_INSTANCE_UID)?;
        let series = sequence(study, tg::REFERENCED_SERIES_SEQUENCE, false)?;
        for s in &series {
            let se = uid(s, tg::SERIES_INSTANCE_UID)?;
            let instances = sequence(s, tg::REF_SOP_SEQUENCE, false)?;
            if instances.len() as i64 > o.max_evidence - refs.len() as i64 {
                return Err(measurement_context_error(MEASUREMENT_CONTEXT_LIMIT, ""));
            }
            for instance in &instances {
                let class = uid(instance, tg::REF_SOP_CLASS_UID)?;
                let instance_uid = uid(instance, tg::REF_SOP_INSTANCE_UID)?;
                refs.push(MeasurementReferenceEvidence {
                    study_instance_uid: su.clone(),
                    series_instance_uid: se.clone(),
                    sop_class_uid: class,
                    sop_instance_uid: instance_uid,
                });
            }
        }
    }
    admit_measurement_evidence(&refs, o)
}
pub(crate) type ContentVisitor<'a, E> = dyn FnMut(&ContentItem, &[u32], i64) -> Result<(), E> + 'a;
pub(crate) fn walk_content<E>(
    items: &[ContentItem],
    parent: &[u32],
    depth: i64,
    f: &mut ContentVisitor<'_, E>,
) -> Result<(), E> {
    for (i, item) in items.iter().enumerate() {
        let path = append_path(parent, i as u32 + 1);
        f(item, &path, depth)?;
        walk_content(&item.children, &path, depth + 1, f)?;
    }
    Ok(())
}
impl DetailedMeasurementReport {
    pub(crate) fn evidence_for(
        &self,
        class: &str,
        instance: &str,
        path: &str,
    ) -> Result<MeasurementReferenceEvidence, SrError> {
        if !valid_uid(class) || !valid_uid(instance) {
            return Err(measurement_context_error(
                MEASUREMENT_CONTEXT_EVIDENCE,
                path,
            ));
        }
        if let Some(e) = self.evidence_index.get(instance) {
            if e.sop_class_uid != class {
                return Err(measurement_context_error(
                    MEASUREMENT_CONTEXT_CONFLICT,
                    path,
                ));
            }
            return Ok(e.clone());
        }
        Err(measurement_context_error(
            MEASUREMENT_CONTEXT_EVIDENCE,
            path,
        ))
    }
    pub(crate) fn check_evidence(&self) -> Result<(), SrError> {
        walk_content(&self.document.content, &[1], 1, &mut |i, path, _| {
            if !i.referenced_content_item_identifier.is_empty() {
                return Ok(());
            }
            let key = identifier_string(path);
            match i.value_type.as_str() {
                value::IMAGE => {
                    let e =
                        self.evidence_for(&i.image.sop_class_uid, &i.image.sop_instance_uid, &key)?;
                    if e.study_instance_uid != i.image.study_instance_uid
                        || e.series_instance_uid != i.image.series_instance_uid
                    {
                        return Err(measurement_context_error(
                            MEASUREMENT_CONTEXT_CONFLICT,
                            &key,
                        ));
                    }
                }
                value::COMPOSITE => {
                    self.evidence_for(
                        &i.composite.sop_class_uid,
                        &i.composite.sop_instance_uid,
                        &key,
                    )?;
                }
                value::WAVEFORM => {
                    self.evidence_for(
                        &i.waveform.composite.sop_class_uid,
                        &i.waveform.composite.sop_instance_uid,
                        &key,
                    )?;
                }
                _ => {}
            }
            Ok(())
        })
    }
}
