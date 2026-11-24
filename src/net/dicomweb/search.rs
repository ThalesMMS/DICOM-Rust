use super::mimeheader::trim_space;
use super::urlrules::Values;
use crate::dicomjson::{Dataset, element_string, element_strings};
const CUSTOM_KEYWORD_REQUIRED: &str = "dicomweb: custom DICOM field keyword is required";
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StudySearchCriteria {
    pub patient_name: String,
    pub patient_id: String,
    pub patient_birth_date: String,
    pub study_date_from: String,
    pub study_date_to: String,
    pub study_time_from: String,
    pub study_time_to: String,
    pub study_description: String,
    pub accession_number: String,
    pub referring_physician_name: String,
    pub institution_name: String,
    pub patient_comments: String,
    pub study_status_id: String,
    pub body_part_examined: String,
    pub work_list_status: String,
    pub modality: String,
    pub study_instance_uid: String,
    pub custom_field_keyword: String,
    pub custom_field_value: String,
    pub limit: i64,
    pub offset: i64,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StudyMatch {
    pub patient_name: String,
    pub patient_id: String,
    pub issuer_of_patient_id: String,
    pub patient_birth_date: String,
    pub study_date: String,
    pub study_time: String,
    pub image_count: String,
    pub study_description: String,
    pub accession_number: String,
    pub referring_physician_name: String,
    pub institution_name: String,
    pub patient_comments: String,
    pub study_status_id: String,
    pub body_part_examined: String,
    pub work_list_status: String,
    pub study_instance_uid: String,
    pub modalities: String,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SeriesSearchCriteria {
    pub series_instance_uid: String,
    pub modality: String,
    pub series_number: String,
    pub series_description: String,
    pub series_date_from: String,
    pub series_date_to: String,
    pub series_time_from: String,
    pub series_time_to: String,
    pub custom_field_keyword: String,
    pub custom_field_value: String,
    pub limit: i64,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SeriesMatch {
    pub study_instance_uid: String,
    pub series_instance_uid: String,
    pub modality: String,
    pub series_number: String,
    pub series_description: String,
    pub series_date: String,
    pub series_time: String,
    pub instance_count: String,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InstanceSearchCriteria {
    pub sop_instance_uid: String,
    pub sop_class_uid: String,
    pub instance_number: String,
    pub modality: String,
    pub custom_field_keyword: String,
    pub custom_field_value: String,
    pub limit: i64,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InstanceMatch {
    pub sop_instance_uid: String,
    pub sop_class_uid: String,
    pub instance_number: String,
    pub modality: String,
    pub patient_id: String,
    pub patient_name: String,
    pub study_instance_uid: String,
    pub series_instance_uid: String,
}
const STUDY_RETURN_FIELDS: [&str; 17] = [
    "PatientName",
    "PatientID",
    "IssuerOfPatientID",
    "PatientBirthDate",
    "StudyDate",
    "StudyTime",
    "NumberOfStudyRelatedInstances",
    "StudyDescription",
    "AccessionNumber",
    "ReferringPhysicianName",
    "InstitutionName",
    "PatientComments",
    "StudyStatusID",
    "BodyPartExamined",
    "ScheduledProcedureStepStatus",
    "StudyInstanceUID",
    "ModalitiesInStudy",
];
const SERIES_RETURN_FIELDS: [&str; 8] = [
    "StudyInstanceUID",
    "SeriesInstanceUID",
    "Modality",
    "SeriesNumber",
    "SeriesDescription",
    "SeriesDate",
    "SeriesTime",
    "NumberOfSeriesRelatedInstances",
];
const INSTANCE_RETURN_FIELDS: [&str; 8] = [
    "SOPInstanceUID",
    "SOPClassUID",
    "InstanceNumber",
    "Modality",
    "PatientID",
    "PatientName",
    "StudyInstanceUID",
    "SeriesInstanceUID",
];
fn add_search_param(params: &mut Values, key: &str, value: &str) {
    let key = trim_space(key);
    let value = trim_space(value);
    if key.is_empty() || value.is_empty() {
        return;
    }
    params.set(key, value);
}
fn study_range_value(from: &str, to: &str) -> String {
    let from = trim_space(from);
    let to = trim_space(to);
    if from.is_empty() && to.is_empty() {
        String::new()
    } else if from == to {
        from.to_string()
    } else {
        format!("{from}-{to}")
    }
}
fn check_custom(keyword: &str, value: &str) -> Result<(), String> {
    if !trim_space(value).is_empty() && trim_space(keyword).is_empty() {
        return Err(CUSTOM_KEYWORD_REQUIRED.to_string());
    }
    Ok(())
}
fn finish(params: &mut Values, limit: i64, offset: i64, fields: &[&str]) {
    if limit > 0 {
        params.set("limit", &limit.to_string());
    }
    if offset > 0 {
        params.set("offset", &offset.to_string());
    }
    for field in fields {
        params.add("includefield", field);
    }
}
pub fn study_search_params(c: &StudySearchCriteria) -> Result<Values, crate::MessageError> {
    check_custom(&c.custom_field_keyword, &c.custom_field_value)?;
    let mut p = Values::new();
    add_search_param(&mut p, "PatientName", &c.patient_name);
    add_search_param(&mut p, "PatientID", &c.patient_id);
    add_search_param(&mut p, "PatientBirthDate", &c.patient_birth_date);
    let date = study_range_value(&c.study_date_from, &c.study_date_to);
    add_search_param(&mut p, "StudyDate", &date);
    let time = study_range_value(&c.study_time_from, &c.study_time_to);
    add_search_param(&mut p, "StudyTime", &time);
    add_search_param(&mut p, "StudyDescription", &c.study_description);
    add_search_param(&mut p, "AccessionNumber", &c.accession_number);
    add_search_param(
        &mut p,
        "ReferringPhysicianName",
        &c.referring_physician_name,
    );
    add_search_param(&mut p, "InstitutionName", &c.institution_name);
    add_search_param(&mut p, "PatientComments", &c.patient_comments);
    add_search_param(&mut p, "StudyStatusID", &c.study_status_id);
    add_search_param(&mut p, "BodyPartExamined", &c.body_part_examined);
    add_search_param(&mut p, "ScheduledProcedureStepStatus", &c.work_list_status);
    add_search_param(&mut p, "ModalitiesInStudy", &c.modality);
    add_search_param(&mut p, "StudyInstanceUID", &c.study_instance_uid);
    add_search_param(&mut p, &c.custom_field_keyword, &c.custom_field_value);
    finish(&mut p, c.limit, c.offset, &STUDY_RETURN_FIELDS);
    Ok(p)
}
pub fn series_search_params(c: &SeriesSearchCriteria) -> Result<Values, crate::MessageError> {
    check_custom(&c.custom_field_keyword, &c.custom_field_value)?;
    let mut p = Values::new();
    add_search_param(&mut p, "SeriesInstanceUID", &c.series_instance_uid);
    add_search_param(&mut p, "Modality", &c.modality);
    add_search_param(&mut p, "SeriesNumber", &c.series_number);
    add_search_param(&mut p, "SeriesDescription", &c.series_description);
    let date = study_range_value(&c.series_date_from, &c.series_date_to);
    add_search_param(&mut p, "SeriesDate", &date);
    let time = study_range_value(&c.series_time_from, &c.series_time_to);
    add_search_param(&mut p, "SeriesTime", &time);
    add_search_param(&mut p, &c.custom_field_keyword, &c.custom_field_value);
    finish(&mut p, c.limit, 0, &SERIES_RETURN_FIELDS);
    Ok(p)
}
pub fn instance_search_params(c: &InstanceSearchCriteria) -> Result<Values, crate::MessageError> {
    check_custom(&c.custom_field_keyword, &c.custom_field_value)?;
    let mut p = Values::new();
    add_search_param(&mut p, "SOPInstanceUID", &c.sop_instance_uid);
    add_search_param(&mut p, "SOPClassUID", &c.sop_class_uid);
    add_search_param(&mut p, "InstanceNumber", &c.instance_number);
    add_search_param(&mut p, "Modality", &c.modality);
    add_search_param(&mut p, &c.custom_field_keyword, &c.custom_field_value);
    finish(&mut p, c.limit, 0, &INSTANCE_RETURN_FIELDS);
    Ok(p)
}
pub fn study_search_return_fields() -> Vec<&'static str> {
    STUDY_RETURN_FIELDS.to_vec()
}
pub fn series_search_return_fields() -> Vec<&'static str> {
    SERIES_RETURN_FIELDS.to_vec()
}
pub fn instance_search_return_fields() -> Vec<&'static str> {
    INSTANCE_RETURN_FIELDS.to_vec()
}
pub fn study_matches_from_datasets(datasets: &[Dataset]) -> Vec<StudyMatch> {
    datasets
        .iter()
        .map(|d| StudyMatch {
            patient_name: element_string(d, "00100010"),
            patient_id: element_string(d, "00100020"),
            issuer_of_patient_id: element_string(d, "00100021"),
            patient_birth_date: element_string(d, "00100030"),
            study_date: element_string(d, "00080020"),
            study_time: element_string(d, "00080030"),
            image_count: element_string(d, "00201208"),
            study_description: element_string(d, "00081030"),
            accession_number: element_string(d, "00080050"),
            referring_physician_name: element_string(d, "00080090"),
            institution_name: element_string(d, "00080080"),
            patient_comments: element_string(d, "00104000"),
            study_status_id: element_string(d, "0032000A"),
            body_part_examined: element_string(d, "00180015"),
            work_list_status: element_string(d, "00400020"),
            study_instance_uid: element_string(d, "0020000D"),
            modalities: element_strings(d, "00080061").join("\\"),
        })
        .collect()
}
pub fn series_matches_from_datasets(datasets: &[Dataset]) -> Vec<SeriesMatch> {
    datasets
        .iter()
        .map(|d| SeriesMatch {
            study_instance_uid: element_string(d, "0020000D"),
            series_instance_uid: element_string(d, "0020000E"),
            modality: element_string(d, "00080060"),
            series_number: element_string(d, "00200011"),
            series_description: element_string(d, "0008103E"),
            series_date: element_string(d, "00080021"),
            series_time: element_string(d, "00080031"),
            instance_count: element_string(d, "00201209"),
        })
        .collect()
}
pub fn instance_matches_from_datasets(datasets: &[Dataset]) -> Vec<InstanceMatch> {
    datasets
        .iter()
        .map(|d| InstanceMatch {
            sop_instance_uid: element_string(d, "00080018"),
            sop_class_uid: element_string(d, "00080016"),
            instance_number: element_string(d, "00200013"),
            modality: element_string(d, "00080060"),
            patient_id: element_string(d, "00100020"),
            patient_name: element_string(d, "00100010"),
            study_instance_uid: element_string(d, "0020000D"),
            series_instance_uid: element_string(d, "0020000E"),
        })
        .collect()
}
