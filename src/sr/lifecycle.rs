use super::read::{Decoder, decode_code};
use super::tags as tg;
use super::{
    COMPLETION_COMPLETE, COMPLETION_PARTIAL, CodedEntry, DataSet, Document, ImageReference,
    KEY_OBJECT_SELECTION_DOCUMENT_STORAGE, Kind, SrError, VERIFICATION_UNVERIFIED,
    VERIFICATION_VERIFIED, clean, code_sequence, hierarchical_reference_sequence, seq_element,
    str_elem,
};
use crate::object::{Element, Object, Vr};
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VerifyingObserver {
    pub name: String,
    pub organization: String,
    pub date_time: String,
    pub code: CodedEntry,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DocumentReference {
    pub study_instance_uid: String,
    pub series_instance_uid: String,
    pub sop_class_uid: String,
    pub sop_instance_uid: String,
}
fn lifecycle_error(suffix: &str) -> SrError {
    SrError::new(Kind::DocumentLifecycle, suffix)
}
impl Document {
    pub fn complete(&mut self, description: &str) -> Result<(), SrError> {
        self.check_lifecycle()?;
        if self.completion_flag.trim() == COMPLETION_COMPLETE {
            return Err(lifecycle_error(": the document is already complete"));
        }
        if description.len() > 64 || description.contains(['\\', '\r', '\n']) {
            return Err(lifecycle_error(
                ": Completion Flag Description is not an LO value",
            ));
        }
        self.completion_flag = COMPLETION_COMPLETE.into();
        self.completion_flag_description = description.into();
        Ok(())
    }
    pub fn verify(&mut self, mut observer: VerifyingObserver) -> Result<(), SrError> {
        self.check_lifecycle()?;
        if self.completion_flag.trim() != COMPLETION_COMPLETE {
            return Err(lifecycle_error(
                ": only a complete document can be verified",
            ));
        }
        if observer.date_time.is_empty() {
            observer.date_time = chrono::Local::now().format("%Y%m%d%H%M%S").to_string();
        }
        validate_verifying_observer(&observer).map_err(|e| e.wrapped(Kind::DocumentLifecycle))?;
        self.verifying_observers.push(observer);
        self.verification_flag = VERIFICATION_VERIFIED.into();
        Ok(())
    }
    pub fn revise(&mut self, sop_instance_uid: &str) -> Result<(), SrError> {
        self.check_lifecycle()?;
        if self.completion_flag.trim() != COMPLETION_COMPLETE {
            return Err(lifecycle_error(": only a complete document can be revised"));
        }
        let current = DocumentReference {
            study_instance_uid: self.study_instance_uid.clone(),
            series_instance_uid: self.series_instance_uid.clone(),
            sop_class_uid: self.sop_class_uid.clone(),
            sop_instance_uid: self.sop_instance_uid.clone(),
        };
        if !complete_document_reference(&current) {
            return Err(lifecycle_error(
                ": a revision needs the study, series, SOP Class and SOP Instance UIDs of the document",
            ));
        }
        let uid = if sop_instance_uid.is_empty() {
            crate::deid::uid::new_uid()
        } else {
            sop_instance_uid.to_string()
        };
        if !crate::object::is_valid_uid(uid.as_bytes()) || uid == self.sop_instance_uid {
            return Err(lifecycle_error(
                ": the revision needs a new valid SOP Instance UID",
            ));
        }
        self.predecessor_documents.push(current);
        self.sop_instance_uid = uid;
        self.completion_flag = COMPLETION_PARTIAL.into();
        self.completion_flag_description.clear();
        self.verification_flag = VERIFICATION_UNVERIFIED.into();
        self.verifying_observers.clear();
        self.content_date.clear();
        self.content_time.clear();
        Ok(())
    }
    fn check_lifecycle(&self) -> Result<(), SrError> {
        if self.sop_class_uid == KEY_OBJECT_SELECTION_DOCUMENT_STORAGE {
            return Err(lifecycle_error(
                ": a Key Object Selection document has no completion or verification",
            ));
        }
        Ok(())
    }
}
fn validate_verifying_observer(o: &VerifyingObserver) -> Result<(), SrError> {
    if o.name.trim().is_empty() || o.organization.trim().is_empty() || o.date_time.trim().is_empty()
    {
        return Err(SrError::msg(
            "a verifying observer needs a name, an organization and a verification date and time",
        ));
    }
    Ok(())
}
fn complete_document_reference(r: &DocumentReference) -> bool {
    !clean(&r.study_instance_uid).is_empty()
        && !clean(&r.series_instance_uid).is_empty()
        && !clean(&r.sop_class_uid).is_empty()
        && !clean(&r.sop_instance_uid).is_empty()
}
pub(crate) fn lifecycle_elements(
    description: &str,
    observers: &[VerifyingObserver],
    predecessors: &[DocumentReference],
) -> Result<Vec<Element>, SrError> {
    let mut elements = Vec::new();
    if !description.is_empty() {
        elements.push(str_elem(
            tg::COMPLETION_FLAG_DESCRIPTION,
            Vr::LO,
            description,
        ));
    }
    if !observers.is_empty() {
        let mut items = Vec::with_capacity(observers.len());
        for (i, o) in observers.iter().enumerate() {
            validate_verifying_observer(o).map_err(|e| {
                e.prefixed(&format!("verifying observer {i}: "))
                    .wrapped(Kind::InvalidDocument)
            })?;
            let code = if o.code.empty() {
                seq_element(tg::VERIFYING_OBSERVER_CODE, Vec::new())
            } else {
                code_sequence(tg::VERIFYING_OBSERVER_CODE, &o.code)
            };
            items.push(DataSet {
                elements: vec![
                    str_elem(tg::VERIFYING_ORGANIZATION, Vr::LO, &o.organization),
                    str_elem(tg::VERIFICATION_DATE_TIME, Vr::DT, &o.date_time),
                    str_elem(tg::VERIFYING_OBSERVER_NAME, Vr::PN, &o.name),
                    code,
                ],
                item_offset: None,
            });
        }
        elements.push(seq_element(tg::VERIFYING_OBSERVER_SEQUENCE, items));
    }
    if !predecessors.is_empty() {
        let mut refs = Vec::with_capacity(predecessors.len());
        for (i, r) in predecessors.iter().enumerate() {
            if !complete_document_reference(r) {
                return Err(SrError::invalid_document(&format!(
                    ": predecessor document {i} needs its study, series, SOP Class and SOP Instance UIDs"
                )));
            }
            refs.push(ImageReference {
                study_instance_uid: r.study_instance_uid.clone(),
                series_instance_uid: r.series_instance_uid.clone(),
                sop_class_uid: r.sop_class_uid.clone(),
                sop_instance_uid: r.sop_instance_uid.clone(),
                frames: Vec::new(),
            });
        }
        elements.push(hierarchical_reference_sequence(
            tg::PREDECESSOR_DOCUMENTS,
            &refs,
        ));
    }
    Ok(elements)
}
pub(crate) fn read_lifecycle(
    dec: &mut Decoder,
    obj: &Object,
) -> (String, Vec<VerifyingObserver>, Vec<DocumentReference>) {
    let description = dec.string(Some(obj), tg::COMPLETION_FLAG_DESCRIPTION);
    let mut observers = Vec::new();
    let (items, _) = dec.sequence(Some(obj), tg::VERIFYING_OBSERVER_SEQUENCE);
    for item in &items {
        let mut o = VerifyingObserver {
            name: dec.string(Some(item), tg::VERIFYING_OBSERVER_NAME),
            organization: dec.string(Some(item), tg::VERIFYING_ORGANIZATION),
            date_time: dec.string(Some(item), tg::VERIFICATION_DATE_TIME),
            code: CodedEntry::default(),
        };
        o.code = decode_code(dec, Some(item), tg::VERIFYING_OBSERVER_CODE).0;
        observers.push(o);
    }
    let mut predecessors = Vec::new();
    let (studies, _) = dec.sequence(Some(obj), tg::PREDECESSOR_DOCUMENTS);
    for study in &studies {
        let study_uid = dec.string(Some(study), tg::STUDY_INSTANCE_UID);
        let (series_items, _) = dec.sequence(Some(study), tg::REFERENCED_SERIES_SEQUENCE);
        for series in &series_items {
            let series_uid = dec.string(Some(series), tg::SERIES_INSTANCE_UID);
            let (sops, _) = dec.sequence(Some(series), tg::REF_SOP_SEQUENCE);
            for sop in &sops {
                predecessors.push(DocumentReference {
                    study_instance_uid: study_uid.clone(),
                    series_instance_uid: series_uid.clone(),
                    sop_class_uid: dec.string(Some(sop), tg::REF_SOP_CLASS_UID),
                    sop_instance_uid: dec.string(Some(sop), tg::REF_SOP_INSTANCE_UID),
                });
            }
        }
    }
    (description, observers, predecessors)
}
