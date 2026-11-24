use super::{
    CodedEntry, ContentItem, Document, ImageReference, KEY_OBJECT_SELECTION_DOCUMENT_STORAGE,
    RELATIONSHIP_CONTAINS, RELATIONSHIP_HAS_ACQ_CONTEXT, current_content_date_time,
    document_series_uid, from_code, value,
};
use crate::codes::dcm;
use std::collections::HashMap;
pub fn kos_title_of_interest() -> CodedEntry {
    from_code(dcm::OfInterest)
}
pub fn kos_finding_concept() -> CodedEntry {
    from_code(dcm::Finding)
}
pub fn new_key_object_selection(
    sop_instance_uid: &str,
    refs: &[ImageReference],
    findings: Option<&HashMap<String, String>>,
) -> Document {
    let (content_date, content_time) = current_content_date_time();
    let mut doc = Document {
        sop_class_uid: KEY_OBJECT_SELECTION_DOCUMENT_STORAGE.into(),
        sop_instance_uid: sop_instance_uid.into(),
        series_instance_uid: document_series_uid(
            KEY_OBJECT_SELECTION_DOCUMENT_STORAGE,
            sop_instance_uid,
            "",
        ),
        modality: "KO".into(),
        title: kos_title_of_interest(),
        content_date,
        content_time,
        ..Default::default()
    };
    for (i, r) in refs.iter().enumerate() {
        if i == 0 {
            doc.study_instance_uid = r.study_instance_uid.clone();
        } else if r.study_instance_uid != doc.study_instance_uid {
            doc.study_instance_uid.clear();
            break;
        }
    }
    for r in refs {
        let mut item = ContentItem {
            value_type: value::IMAGE.into(),
            relationship_type: RELATIONSHIP_CONTAINS.into(),
            image: ImageReference {
                study_instance_uid: r.study_instance_uid.clone(),
                series_instance_uid: r.series_instance_uid.clone(),
                sop_class_uid: r.sop_class_uid.clone(),
                sop_instance_uid: r.sop_instance_uid.clone(),
                frames: r.frames.clone(),
            },
            ..Default::default()
        };
        if let Some(finding) = findings.and_then(|f| f.get(&r.sop_instance_uid))
            && !finding.is_empty()
        {
            item.children = vec![ContentItem {
                value_type: value::TEXT.into(),
                relationship_type: RELATIONSHIP_HAS_ACQ_CONTEXT.into(),
                concept_name: kos_finding_concept(),
                text: finding.clone(),
                ..Default::default()
            }];
        }
        doc.content.push(item);
    }
    doc
}
pub fn key_object_selection_images(doc: Option<&Document>) -> Vec<ImageReference> {
    let Some(doc) = doc else {
        return Vec::new();
    };
    doc.content
        .iter()
        .filter(|i| i.value_type == value::IMAGE)
        .map(|i| i.image.clone())
        .collect()
}
pub fn key_object_selection_findings(doc: Option<&Document>) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let Some(doc) = doc else {
        return out;
    };
    for item in &doc.content {
        if item.value_type != value::IMAGE || item.image.sop_instance_uid.is_empty() {
            continue;
        }
        if let Some(child) = item
            .children
            .iter()
            .find(|c| c.value_type == value::TEXT && !c.text.is_empty())
        {
            out.insert(item.image.sop_instance_uid.clone(), child.text.clone());
        }
    }
    out
}
