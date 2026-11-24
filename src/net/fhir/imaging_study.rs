use super::{Error, valid_resource_id};
use crate::dcmtime;
use crate::dicomjson::jsonwire::{self, JVal};
use crate::net::dicomweb::urlrules;
use crate::object::text::normalize_uid;
use crate::object::{IdWithIssuer, Object, Tag, Vr, is_valid_uid, patient_id_with_issuer};
use std::collections::{BTreeSet, HashMap, HashSet};
const IMAGING_STUDY_TYPE: &str = "ImagingStudy";
const DICOM_UID_SYSTEM: &str = "urn:dicom:uid";
const DICOM_ONTOLOGY: &str = "http://dicom.nema.org/resources/ontology/DCM";
const OID_PREFIX: &str = "urn:oid:";
const MAX_MAPPED_OBJECTS: usize = 100_000;
const STUDY_INSTANCE_UID: Tag = Tag::new(0x0020, 0x000D);
const SERIES_INSTANCE_UID: Tag = Tag::new(0x0020, 0x000E);
const SOP_INSTANCE_UID: Tag = Tag::new(0x0008, 0x0018);
const SOP_CLASS_UID: Tag = Tag::new(0x0008, 0x0016);
const MODALITY: Tag = Tag::new(0x0008, 0x0060);
const STUDY_DATE: Tag = Tag::new(0x0008, 0x0020);
const STUDY_TIME: Tag = Tag::new(0x0008, 0x0030);
const SERIES_DATE: Tag = Tag::new(0x0008, 0x0021);
const SERIES_TIME: Tag = Tag::new(0x0008, 0x0031);
const TIMEZONE_OFFSET: Tag = Tag::new(0x0008, 0x0201);
const STUDY_DESCRIPTION: Tag = Tag::new(0x0008, 0x1030);
const SERIES_DESCRIPTION: Tag = Tag::new(0x0008, 0x103E);
const ACCESSION_NUMBER: Tag = Tag::new(0x0008, 0x0050);
const SERIES_NUMBER: Tag = Tag::new(0x0020, 0x0011);
const INSTANCE_NUMBER: Tag = Tag::new(0x0020, 0x0013);
const PATIENT_ID: Tag = Tag::new(0x0010, 0x0020);
const ISSUER_OF_PATIENT_ID: Tag = Tag::new(0x0010, 0x0021);
const TYPE_OF_PATIENT_ID: Tag = Tag::new(0x0010, 0x0022);
const ISSUER_QUALIFIERS: Tag = Tag::new(0x0010, 0x0024);
const UNIVERSAL_ENTITY_ID: Tag = Tag::new(0x0040, 0x0032);
const UNIVERSAL_ENTITY_ID_TYPE: Tag = Tag::new(0x0040, 0x0033);
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Identifier {
    pub system: String,
    pub value: String,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Reference {
    pub reference: String,
    pub identifier: Option<Identifier>,
    pub display: String,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Coding {
    pub system: String,
    pub code: String,
    pub display: String,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ImagingStudy {
    pub resource_type: String,
    pub id: String,
    pub identifiers: Vec<Identifier>,
    pub status: String,
    pub subject: Reference,
    pub started: String,
    pub description: String,
    pub modality: Vec<Coding>,
    pub endpoints: Vec<Reference>,
    pub number_of_series: Option<i64>,
    pub number_of_instances: Option<i64>,
    pub series: Vec<ImagingSeries>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ImagingSeries {
    pub uid: String,
    pub number: Option<i64>,
    pub modality: Coding,
    pub description: String,
    pub started: String,
    pub number_of_instances: Option<i64>,
    pub instances: Vec<ImagingInstance>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ImagingInstance {
    pub uid: String,
    pub sop_class: Coding,
    pub number: Option<i64>,
}
impl ImagingStudy {
    pub fn dicom_study_instance_uid(&self) -> Option<String> {
        let mut result: Option<&str> = None;
        for identifier in &self.identifiers {
            if identifier.system != DICOM_UID_SYSTEM {
                continue;
            }
            let uid = identifier.value.strip_prefix(OID_PREFIX)?;
            if !is_valid_uid(uid.as_bytes()) || result.is_some_and(|r| r != uid) {
                return None;
            }
            result = Some(uid);
        }
        result.map(str::to_string)
    }
    pub fn to_json(&self) -> String {
        let mut out = String::new();
        jsonwire::write_compact(&self.to_jval(), &mut out);
        out
    }
    fn to_jval(&self) -> JVal {
        let mut f = Fields::default();
        f.always("resourceType", JVal::Str(self.resource_type.clone()));
        f.text("id", &self.id);
        f.list("identifier", &self.identifiers, Identifier::to_jval);
        f.always("status", JVal::Str(self.status.clone()));
        f.always("subject", self.subject.to_jval());
        f.text("started", &self.started);
        f.text("description", &self.description);
        f.list("modality", &self.modality, Coding::to_jval);
        f.list("endpoint", &self.endpoints, Reference::to_jval);
        f.number("numberOfSeries", self.number_of_series);
        f.number("numberOfInstances", self.number_of_instances);
        f.list("series", &self.series, ImagingSeries::to_jval);
        JVal::Obj(f.0)
    }
}
impl Identifier {
    fn to_jval(&self) -> JVal {
        let mut f = Fields::default();
        f.text("system", &self.system);
        f.text("value", &self.value);
        JVal::Obj(f.0)
    }
}
impl Reference {
    fn to_jval(&self) -> JVal {
        let mut f = Fields::default();
        f.text("reference", &self.reference);
        if let Some(identifier) = &self.identifier {
            f.always("identifier", identifier.to_jval());
        }
        f.text("display", &self.display);
        JVal::Obj(f.0)
    }
}
impl Coding {
    fn to_jval(&self) -> JVal {
        let mut f = Fields::default();
        f.text("system", &self.system);
        f.text("code", &self.code);
        f.text("display", &self.display);
        JVal::Obj(f.0)
    }
}
impl ImagingSeries {
    fn to_jval(&self) -> JVal {
        let mut f = Fields::default();
        f.always("uid", JVal::Str(self.uid.clone()));
        f.number("number", self.number);
        f.always("modality", self.modality.to_jval());
        f.text("description", &self.description);
        f.text("started", &self.started);
        f.number("numberOfInstances", self.number_of_instances);
        f.list("instance", &self.instances, ImagingInstance::to_jval);
        JVal::Obj(f.0)
    }
}
impl ImagingInstance {
    fn to_jval(&self) -> JVal {
        let mut f = Fields::default();
        f.always("uid", JVal::Str(self.uid.clone()));
        f.always("sopClass", self.sop_class.to_jval());
        f.number("number", self.number);
        JVal::Obj(f.0)
    }
}
#[derive(Default)]
struct Fields(Vec<(String, JVal)>);
impl Fields {
    fn always(&mut self, name: &str, value: JVal) {
        self.0.push((name.to_string(), value));
    }
    fn text(&mut self, name: &str, value: &str) {
        if !value.is_empty() {
            self.always(name, JVal::Str(value.to_string()));
        }
    }
    fn number(&mut self, name: &str, value: Option<i64>) {
        if let Some(n) = value {
            self.always(name, JVal::I64(n));
        }
    }
    fn list<T>(&mut self, name: &str, items: &[T], f: fn(&T) -> JVal) {
        if !items.is_empty() {
            self.always(name, JVal::Arr(items.iter().map(f).collect()));
        }
    }
}
pub(super) type Members<'a> = Vec<(String, &'a [u8])>;
pub(super) fn object_members(raw: &[u8]) -> Result<Option<Members<'_>>, ()> {
    let start = raw
        .iter()
        .position(|c| !matches!(c, b' ' | b'\t' | b'\r' | b'\n'))
        .unwrap_or(raw.len());
    let raw = &raw[start..];
    match raw.first() {
        Some(b'{') => jsonwire::as_raw_map(raw).map(Some).map_err(|_| ()),
        Some(b'n') => Ok(None),
        _ => Err(()),
    }
}
pub(super) fn field<'a>(members: &Members<'a>, name: &str) -> Option<&'a [u8]> {
    members
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| *value)
}
pub(super) fn decode_string(members: &Members, name: &str) -> Result<String, ()> {
    field(members, name).map_or(Ok(String::new()), |raw| {
        jsonwire::as_string(raw).map_err(|_| ())
    })
}
fn decode_int(members: &Members, name: &str) -> Result<Option<i64>, ()> {
    let Some(raw) = field(members, name) else {
        return Ok(None);
    };
    match raw.first() {
        Some(b'n') => Ok(None),
        Some(b'-' | b'0'..=b'9') => std::str::from_utf8(raw)
            .ok()
            .and_then(|s| s.parse::<i64>().ok())
            .map(Some)
            .ok_or(()),
        _ => Err(()),
    }
}
fn decode_struct<T: Default>(
    raw: Option<&[u8]>,
    decode: fn(&Members) -> Result<T, ()>,
) -> Result<T, ()> {
    match raw.map(object_members).transpose()?.flatten() {
        Some(members) => decode(&members),
        None => Ok(T::default()),
    }
}
fn decode_pointer<T>(
    raw: Option<&[u8]>,
    decode: fn(&Members) -> Result<T, ()>,
) -> Result<Option<T>, ()> {
    match raw.map(object_members).transpose()?.flatten() {
        Some(members) => decode(&members).map(Some),
        None => Ok(None),
    }
}
pub(super) fn decode_list<T: Default>(
    raw: Option<&[u8]>,
    decode: fn(&Members) -> Result<T, ()>,
) -> Result<Vec<T>, ()> {
    let Some(raw) = raw else {
        return Ok(Vec::new());
    };
    match raw.first() {
        Some(b'[') => jsonwire::as_raw_array(raw)
            .map_err(|_| ())?
            .into_iter()
            .map(|item| decode_struct(Some(item), decode))
            .collect(),
        Some(b'n') => Ok(Vec::new()),
        _ => Err(()),
    }
}
fn decode_identifier(m: &Members) -> Result<Identifier, ()> {
    Ok(Identifier {
        system: decode_string(m, "system")?,
        value: decode_string(m, "value")?,
    })
}
fn decode_reference(m: &Members) -> Result<Reference, ()> {
    Ok(Reference {
        reference: decode_string(m, "reference")?,
        identifier: decode_pointer(field(m, "identifier"), decode_identifier)?,
        display: decode_string(m, "display")?,
    })
}
fn decode_coding(m: &Members) -> Result<Coding, ()> {
    Ok(Coding {
        system: decode_string(m, "system")?,
        code: decode_string(m, "code")?,
        display: decode_string(m, "display")?,
    })
}
fn decode_instance(m: &Members) -> Result<ImagingInstance, ()> {
    Ok(ImagingInstance {
        uid: decode_string(m, "uid")?,
        sop_class: decode_struct(field(m, "sopClass"), decode_coding)?,
        number: decode_int(m, "number")?,
    })
}
fn decode_series(m: &Members) -> Result<ImagingSeries, ()> {
    Ok(ImagingSeries {
        uid: decode_string(m, "uid")?,
        number: decode_int(m, "number")?,
        modality: decode_struct(field(m, "modality"), decode_coding)?,
        description: decode_string(m, "description")?,
        started: decode_string(m, "started")?,
        number_of_instances: decode_int(m, "numberOfInstances")?,
        instances: decode_list(field(m, "instance"), decode_instance)?,
    })
}
fn decode_study(m: &Members) -> Result<ImagingStudy, ()> {
    Ok(ImagingStudy {
        resource_type: decode_string(m, "resourceType")?,
        id: decode_string(m, "id")?,
        identifiers: decode_list(field(m, "identifier"), decode_identifier)?,
        status: decode_string(m, "status")?,
        subject: decode_struct(field(m, "subject"), decode_reference)?,
        started: decode_string(m, "started")?,
        description: decode_string(m, "description")?,
        modality: decode_list(field(m, "modality"), decode_coding)?,
        endpoints: decode_list(field(m, "endpoint"), decode_reference)?,
        number_of_series: decode_int(m, "numberOfSeries")?,
        number_of_instances: decode_int(m, "numberOfInstances")?,
        series: decode_list(field(m, "series"), decode_series)?,
    })
}
pub(super) fn decode_imaging_study(raw: Option<&[u8]>) -> Result<ImagingStudy, ()> {
    decode_struct(Some(raw.ok_or(())?), decode_study)
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ImagingStudyOptions {
    pub id: String,
    pub subject: String,
    pub endpoint: String,
    pub accession_system: String,
}
pub fn map_imaging_study(
    datasets: &[&Object],
    options: &ImagingStudyOptions,
) -> Result<ImagingStudy, Error> {
    let Some(first) = datasets.first() else {
        return Err(Error::ImagingIdentity);
    };
    if datasets.len() > MAX_MAPPED_OBJECTS {
        return Err(Error::Limit);
    }
    if (!options.id.is_empty() && !valid_resource_id(&options.id))
        || !valid_reference(&options.subject, "Patient")
        || (!options.endpoint.is_empty() && !valid_reference(&options.endpoint, "Endpoint"))
    {
        return Err(Error::ImagingIdentity);
    }
    let system = &options.accession_system;
    if !system.is_empty() {
        let absolute = urlrules::parse(system).is_ok_and(|u| !u.scheme.is_empty());
        if !absolute
            || system == DICOM_UID_SYSTEM
            || system.len() > 4096
            || system.contains(['\r', '\n', '\t', ' '])
        {
            return Err(Error::ImagingIdentity);
        }
    }
    let study_uid = dicom_uid(first, STUDY_INSTANCE_UID);
    if !is_valid_uid(study_uid.as_bytes()) {
        return Err(Error::ImagingIdentity);
    }
    let mut study = ImagingStudy {
        resource_type: IMAGING_STUDY_TYPE.into(),
        id: options.id.clone(),
        identifiers: vec![Identifier {
            system: DICOM_UID_SYSTEM.into(),
            value: format!("{OID_PREFIX}{study_uid}"),
        }],
        status: "available".into(),
        subject: Reference {
            reference: options.subject.clone(),
            ..Reference::default()
        },
        started: dicom_started(first, STUDY_DATE, STUDY_TIME),
        description: dicom_value(first, STUDY_DESCRIPTION),
        ..ImagingStudy::default()
    };
    if !options.endpoint.is_empty() {
        study.endpoints = vec![Reference {
            reference: options.endpoint.clone(),
            ..Reference::default()
        }];
    }
    let accession = dicom_value(first, ACCESSION_NUMBER);
    if !accession.is_empty() && !system.is_empty() {
        study.identifiers.push(Identifier {
            system: system.clone(),
            value: accession,
        });
    }
    let mut series_indexes: HashMap<String, usize> = HashMap::new();
    let mut instances: HashSet<String> = HashSet::with_capacity(datasets.len());
    let mut modalities: BTreeSet<String> = BTreeSet::new();
    let mut patient: Option<IdWithIssuer> = None;
    for dataset in datasets {
        if dicom_uid(dataset, STUDY_INSTANCE_UID) != study_uid || !unambiguous_patient_id(dataset) {
            return Err(Error::ImagingIdentity);
        }
        if let Some(latest) = patient_id_with_issuer(dataset) {
            if patient.as_ref().is_some_and(|p| *p != latest) {
                return Err(Error::ImagingIdentity);
            }
            patient = Some(latest);
        }
        let series_uid = dicom_uid(dataset, SERIES_INSTANCE_UID);
        let instance_uid = dicom_uid(dataset, SOP_INSTANCE_UID);
        let sop_class = dicom_uid(dataset, SOP_CLASS_UID);
        let modality = dicom_value(dataset, MODALITY);
        if !is_valid_uid(series_uid.as_bytes())
            || !is_valid_uid(instance_uid.as_bytes())
            || !is_valid_uid(sop_class.as_bytes())
            || !valid_modality(&modality)
        {
            return Err(Error::ImagingIdentity);
        }
        if !instances.insert(instance_uid.clone()) {
            return Err(Error::ImagingIdentity);
        }
        let index = match series_indexes.get(&series_uid) {
            Some(&index) => index,
            None => {
                let index = study.series.len();
                series_indexes.insert(series_uid.clone(), index);
                study.series.push(ImagingSeries {
                    uid: series_uid,
                    number: dicom_number(dataset, SERIES_NUMBER),
                    modality: Coding {
                        system: DICOM_ONTOLOGY.into(),
                        code: modality.clone(),
                        ..Coding::default()
                    },
                    started: dicom_started(dataset, SERIES_DATE, SERIES_TIME),
                    description: dicom_value(dataset, SERIES_DESCRIPTION),
                    ..ImagingSeries::default()
                });
                index
            }
        };
        let series = &mut study.series[index];
        if series.modality.code != modality {
            return Err(Error::ImagingIdentity);
        }
        series.instances.push(ImagingInstance {
            uid: instance_uid,
            sop_class: Coding {
                system: "urn:ietf:rfc:3986".into(),
                code: format!("{OID_PREFIX}{sop_class}"),
                ..Coding::default()
            },
            number: dicom_number(dataset, INSTANCE_NUMBER),
        });
        modalities.insert(modality);
    }
    study.modality = modalities
        .into_iter()
        .map(|code| Coding {
            system: DICOM_ONTOLOGY.into(),
            code,
            ..Coding::default()
        })
        .collect();
    for series in &mut study.series {
        series.number_of_instances = Some(series.instances.len() as i64);
        series
            .instances
            .sort_by(|a, b| compare_numbered(a.number, &a.uid, b.number, &b.uid));
    }
    study
        .series
        .sort_by(|a, b| compare_numbered(a.number, &a.uid, b.number, &b.uid));
    study.number_of_series = Some(study.series.len() as i64);
    study.number_of_instances = Some(instances.len() as i64);
    Ok(study)
}
fn single_dicom_text(dataset: &Object, tag: Tag) -> Option<String> {
    if dataset.has_duplicate_parsed_tag(tag) {
        return None;
    }
    if !dataset.has(tag) {
        return Some(String::new());
    }
    match dataset.lookup_strings(tag) {
        Ok(values) if values.len() <= 1 => Some(values.into_iter().next().unwrap_or_default()),
        _ => None,
    }
}
fn dicom_value(dataset: &Object, tag: Tag) -> String {
    match single_dicom_text(dataset, tag) {
        Some(value) if !value.contains('\0') => value.trim().to_string(),
        _ => String::new(),
    }
}
fn dicom_uid(dataset: &Object, tag: Tag) -> String {
    match dataset.get(tag) {
        Some(element) if element.header.vr == Vr::UI => {}
        _ => return String::new(),
    }
    single_dicom_text(dataset, tag)
        .map(|value| normalize_uid(&value).to_string())
        .unwrap_or_default()
}
fn single_text_without_nul(dataset: &Object, tag: Tag) -> bool {
    single_dicom_text(dataset, tag).is_some_and(|value| !value.contains('\0'))
}
fn unambiguous_patient_id(dataset: &Object) -> bool {
    if ![PATIENT_ID, ISSUER_OF_PATIENT_ID, TYPE_OF_PATIENT_ID]
        .into_iter()
        .all(|tag| single_text_without_nul(dataset, tag))
    {
        return false;
    }
    if !dataset.has(ISSUER_QUALIFIERS) {
        return true;
    }
    let Some(items) = dataset.get_sequence(ISSUER_QUALIFIERS) else {
        return false;
    };
    if items.len() > 1 || dataset.has_duplicate_parsed_tag(ISSUER_QUALIFIERS) {
        return false;
    }
    items.first().is_none_or(|item| {
        [UNIVERSAL_ENTITY_ID, UNIVERSAL_ENTITY_ID_TYPE]
            .into_iter()
            .all(|tag| single_text_without_nul(item, tag))
    })
}
fn valid_modality(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 16
        && value
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
}
fn dicom_number(dataset: &Object, tag: Tag) -> Option<i64> {
    let value = dicom_value(dataset, tag);
    if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    value
        .parse::<u64>()
        .ok()
        .filter(|&n| n <= i32::MAX as u64)
        .map(|n| n as i64)
}
fn compare_numbered(
    left: Option<i64>,
    left_uid: &str,
    right: Option<i64>,
    right_uid: &str,
) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    match (left, right) {
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (Some(l), Some(r)) if l != r => l.cmp(&r),
        _ => left_uid.as_bytes().cmp(right_uid.as_bytes()),
    }
}
fn dicom_started(dataset: &Object, date_tag: Tag, time_tag: Tag) -> String {
    let Ok(date) = dcmtime::parse_date(&dicom_value(dataset, date_tag)) else {
        return String::new();
    };
    let dcm = date.dcm();
    if dcm.len() != 8 || date.time.year() < 1 {
        return String::new();
    }
    let (year, month, day) = date.time.date();
    let started = format!("{year:04}-{month:02}-{day:02}");
    let clock = dicom_value(dataset, time_tag);
    let offset = dicom_value(dataset, TIMEZONE_OFFSET);
    if clock.len() < 4 || offset.len() != 5 {
        return started;
    }
    if dcmtime::parse_datetime(&format!("{dcm}{clock}{offset}")).is_err() {
        return started;
    }
    let (Some(hour), Some(minute), Some(rest), Some(sign_hour), Some(offset_minute)) = (
        clock.get(..2),
        clock.get(2..4),
        clock.get(4..),
        offset.get(..3),
        offset.get(3..),
    ) else {
        return started;
    };
    let second = if clock.len() >= 6 { rest } else { "00" };
    format!("{started}T{hour}:{minute}:{second}{sign_hour}:{offset_minute}")
}
fn valid_reference(value: &str, resource: &str) -> bool {
    value
        .split_once('/')
        .is_some_and(|(prefix, id)| prefix == resource && valid_resource_id(id))
}
