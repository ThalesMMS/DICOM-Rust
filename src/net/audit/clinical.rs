#![forbid(unsafe_code)]
use super::atna::{XML_HEADER, base64, encode_message};
use super::{
    ACTION_CREATE, ACTION_DELETE, ACTION_EXECUTE, ACTION_READ, ActiveParticipant, Code, Common,
    Error, MediaIdentifier, Message, Participant, ParticipantObject, ParticipantObjectDetail,
    Patient, QueryRequest, Result, SopClass, Study, Time, event_audit_log_used,
    event_begin_transferring, event_export, event_import, event_instances_accessed,
    event_instances_transferred, event_order_record, event_patient_record, event_procedure_record,
    event_study_deleted, query, role_destination, role_destination_media, role_source,
    role_source_media,
};
use crate::object::parser::source::MemFile;
use crate::object::transfer::Syntax;
use crate::object::{self, Object, ReadFileOptions, Tag};
use std::rc::Rc;
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ClinicalPolicy {
    pub include_identities: bool,
    pub include_query: bool,
    pub limits: ClinicalLimits,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ClinicalLimits {
    pub max_groups: i64,
    pub max_instances: i64,
    pub max_classes: i64,
    pub max_objects: i64,
    pub max_identity_bytes: i64,
    pub max_query_bytes: i64,
    pub max_message_bytes: i64,
}
impl ClinicalPolicy {
    pub fn normalize(&self) -> Result<ClinicalPolicy> {
        let mut p = *self;
        let l = &mut p.limits;
        let fields: [(&mut i64, i64); 7] = [
            (&mut l.max_groups, 64),
            (&mut l.max_instances, 10000),
            (&mut l.max_classes, 64),
            (&mut l.max_objects, 1024),
            (&mut l.max_identity_bytes, 1 << 20),
            (&mut l.max_query_bytes, 1 << 20),
            (&mut l.max_message_bytes, 4 << 20),
        ];
        for (v, default) in fields {
            if *v < 0 {
                return Err(Error::clinical("limit", "Limits"));
            }
            if *v == 0 {
                *v = default;
            }
        }
        Ok(p)
    }
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct PatientIdentity {
    pub id: String,
    pub name: String,
    pub issuer: String,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StudyIdentity {
    pub uid: String,
    pub name: String,
    pub query: Vec<u8>,
    pub accessions: Vec<String>,
    pub sop_classes: Vec<SopClass>,
    pub encrypted: Option<bool>,
    pub anonymized: Option<bool>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClinicalTransfer {
    pub common: Common,
    pub source: Participant,
    pub destination: Participant,
    pub requestor: Option<Participant>,
    pub patient: PatientIdentity,
    pub studies: Vec<StudyIdentity>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClinicalRecord {
    pub common: Common,
    pub actors: Vec<Participant>,
    pub patient: PatientIdentity,
    pub studies: Vec<StudyIdentity>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClinicalMedia {
    pub id: String,
    pub media_type: Code,
    pub host: String,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClinicalImport {
    pub common: Common,
    pub destinations: Vec<Participant>,
    pub media: ClinicalMedia,
    pub sources: Vec<Participant>,
    pub patients: Vec<PatientIdentity>,
    pub studies: Vec<StudyIdentity>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClinicalExport {
    pub common: Common,
    pub sources: Vec<Participant>,
    pub media: ClinicalMedia,
    pub patients: Vec<PatientIdentity>,
    pub studies: Vec<StudyIdentity>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClinicalAuditLog {
    pub common: Common,
    pub actors: Vec<Participant>,
    pub uri: String,
}
fn clinical_common(c: &Common) -> Common {
    let mut c = c.clone();
    if c.time.is_zero() {
        c.time = Time::now_local();
    }
    c.time = Time::from_unix_utc(c.time.unix(), c.time.nanosecond());
    c
}
fn patient_object(p: &PatientIdentity) -> ParticipantObject {
    let mut o = Patient {
        id: p.id.clone(),
        name: p.name.clone(),
    }
    .object();
    if !p.issuer.is_empty() {
        o.details = vec![ParticipantObjectDetail {
            detail_type: "IssuerOfPatientID".to_string(),
            value: base64(p.issuer.as_bytes()),
        }];
    }
    o
}
fn study_object(s: &StudyIdentity) -> ParticipantObject {
    let mut o = Study {
        uid: s.uid.clone(),
        accessions: s.accessions.clone(),
        sop_classes: s.sop_classes.clone(),
    }
    .object("");
    o.name = s.name.clone();
    if !s.query.is_empty() {
        o.query = base64(&s.query);
    }
    o.description.encrypted = s.encrypted;
    o.description.anonymized = s.anonymized;
    o
}
fn check_clinical_inputs(
    p: &ClinicalPolicy,
    studies: &[StudyIdentity],
    patients: &[PatientIdentity],
) -> Result<()> {
    let p = p.normalize()?;
    let l = &p.limits;
    if (studies.len() + patients.len()) as i64 > l.max_objects {
        return Err(Error::clinical("limit", "Objects"));
    }
    let mut n = 0i64;
    let mut add = |v: usize| -> bool {
        if v as i64 > l.max_identity_bytes - n {
            return false;
        }
        n += v as i64;
        true
    };
    let identity = || Error::clinical("limit", "Identity");
    for v in patients {
        if !v.issuer.is_empty() && !clinical_text(&v.issuer) {
            return Err(Error::clinical("text", "Patient.Issuer"));
        }
        if !add(v.id.len()) || !add(v.name.len()) || !add(v.issuer.len()) {
            return Err(identity());
        }
    }
    for v in studies {
        if !add(v.uid.len()) || !add(v.name.len()) {
            return Err(identity());
        }
        if v.query.len() as i64 > l.max_query_bytes {
            return Err(Error::clinical("limit", "Study.Query"));
        }
        if v.sop_classes.len() as i64 > l.max_classes {
            return Err(Error::clinical("limit", "Study.SOPClasses"));
        }
        if v.accessions.len() as i64 > l.max_objects {
            return Err(Error::clinical("limit", "Study.Accessions"));
        }
        for a in &v.accessions {
            if !add(a.len()) {
                return Err(identity());
            }
        }
        for c in &v.sop_classes {
            if c.instances.len() as i64 > l.max_instances || c.number_of_instances > l.max_instances
            {
                return Err(Error::clinical("limit", "Study.Instances"));
            }
            if !add(c.uid.len()) {
                return Err(identity());
            }
            for i in &c.instances {
                if !add(i.uid.len()) {
                    return Err(identity());
                }
            }
        }
    }
    Ok(())
}
pub(crate) fn encode_clinical(p: &ClinicalPolicy, m: &Message) -> Result<Vec<u8>> {
    let p = p.normalize()?;
    let mut out = String::from(XML_HEADER);
    out.push_str(&encode_message(m));
    if out.len() as i64 > p.limits.max_message_bytes {
        return Err(Error::clinical("limit", "Message"));
    }
    Ok(out.into_bytes())
}
fn qualified(p: &ClinicalPolicy, m: Message) -> Result<Message> {
    m.validate_clinical(p)?;
    encode_clinical(p, &m)?;
    Ok(m)
}
impl Message {
    pub fn marshal_clinical(&self, p: &ClinicalPolicy) -> Result<Vec<u8>> {
        self.validate_clinical(p)?;
        encode_clinical(p, self)
    }
}
fn transfer_clinical(
    p: &ClinicalPolicy,
    t: &ClinicalTransfer,
    id: Code,
    action: &str,
) -> Result<Message> {
    check_clinical_inputs(p, &t.studies, std::slice::from_ref(&t.patient))?;
    let common = clinical_common(&t.common);
    let mut m = Message {
        event: common.event(id, action, Vec::new()),
        sources: vec![common.source.element()],
        participants: vec![
            t.source.element(vec![role_source()]),
            t.destination.element(vec![role_destination()]),
        ],
        objects: Vec::new(),
    };
    if let Some(requestor) = &t.requestor {
        let mut v = requestor.clone();
        v.requestor = true;
        m.participants.push(v.element(Vec::new()));
    }
    for s in &t.studies {
        m.objects.push(study_object(s));
    }
    m.objects.push(patient_object(&t.patient));
    qualified(p, m)
}
pub fn build_instances_transferred(
    p: &ClinicalPolicy,
    t: &ClinicalTransfer,
    action: &str,
) -> Result<Message> {
    transfer_clinical(p, t, event_instances_transferred(), action)
}
pub fn build_begin_transferring(p: &ClinicalPolicy, t: &ClinicalTransfer) -> Result<Message> {
    transfer_clinical(p, t, event_begin_transferring(), ACTION_EXECUTE)
}
fn record_clinical(
    p: &ClinicalPolicy,
    r: &ClinicalRecord,
    id: Code,
    action: &str,
) -> Result<Message> {
    check_clinical_inputs(p, &r.studies, std::slice::from_ref(&r.patient))?;
    let common = clinical_common(&r.common);
    let mut m = Message {
        event: common.event(id, action, Vec::new()),
        sources: vec![common.source.element()],
        ..Default::default()
    };
    for a in &r.actors {
        m.participants.push(a.element(Vec::new()));
    }
    for s in &r.studies {
        m.objects.push(study_object(s));
    }
    m.objects.push(patient_object(&r.patient));
    qualified(p, m)
}
pub fn build_instances_accessed(
    p: &ClinicalPolicy,
    r: &ClinicalRecord,
    action: &str,
) -> Result<Message> {
    record_clinical(p, r, event_instances_accessed(), action)
}
pub fn build_study_deleted(p: &ClinicalPolicy, r: &ClinicalRecord) -> Result<Message> {
    record_clinical(p, r, event_study_deleted(), ACTION_DELETE)
}
pub fn build_order_record(p: &ClinicalPolicy, r: &ClinicalRecord, action: &str) -> Result<Message> {
    record_clinical(p, r, event_order_record(), action)
}
pub fn build_patient_record(
    p: &ClinicalPolicy,
    r: &ClinicalRecord,
    action: &str,
) -> Result<Message> {
    record_clinical(p, r, event_patient_record(), action)
}
pub fn build_procedure_record(
    p: &ClinicalPolicy,
    r: &ClinicalRecord,
    action: &str,
) -> Result<Message> {
    record_clinical(p, r, event_procedure_record(), action)
}
fn media_participant(m: &ClinicalMedia, role: Code) -> ActiveParticipant {
    let mut o = Participant {
        user_id: m.id.clone(),
        host: m.host.clone(),
        ..Default::default()
    }
    .element(vec![role]);
    o.media_identifier = Some(MediaIdentifier {
        media_type: m.media_type.clone(),
    });
    o
}
pub fn build_import(p: &ClinicalPolicy, r: &ClinicalImport) -> Result<Message> {
    check_clinical_inputs(p, &r.studies, &r.patients)?;
    let common = clinical_common(&r.common);
    let mut m = Message {
        event: common.event(event_import(), ACTION_CREATE, Vec::new()),
        sources: vec![common.source.element()],
        ..Default::default()
    };
    for a in &r.destinations {
        m.participants.push(a.element(vec![role_destination()]));
    }
    m.participants
        .push(media_participant(&r.media, role_source_media()));
    for a in &r.sources {
        m.participants.push(a.element(vec![role_source()]));
    }
    for s in &r.studies {
        m.objects.push(study_object(s));
    }
    for v in &r.patients {
        m.objects.push(patient_object(v));
    }
    qualified(p, m)
}
pub fn build_export(p: &ClinicalPolicy, r: &ClinicalExport) -> Result<Message> {
    check_clinical_inputs(p, &r.studies, &r.patients)?;
    let common = clinical_common(&r.common);
    let mut m = Message {
        event: common.event(event_export(), ACTION_READ, Vec::new()),
        sources: vec![common.source.element()],
        ..Default::default()
    };
    for a in &r.sources {
        m.participants.push(a.element(vec![role_source()]));
    }
    m.participants
        .push(media_participant(&r.media, role_destination_media()));
    for s in &r.studies {
        m.objects.push(study_object(s));
    }
    for v in &r.patients {
        m.objects.push(patient_object(v));
    }
    qualified(p, m)
}
pub fn build_audit_log_used(p: &ClinicalPolicy, r: &ClinicalAuditLog) -> Result<Message> {
    let common = clinical_common(&r.common);
    let mut m = Message {
        event: common.event(event_audit_log_used(), ACTION_READ, Vec::new()),
        sources: vec![common.source.element()],
        objects: vec![ParticipantObject {
            id: r.uri.clone(),
            id_type_code: Code::new("12", "RFC-3881", "URI"),
            name: "Security Audit Log".to_string(),
            type_code: "2".to_string(),
            role: "13".to_string(),
            ..Default::default()
        }],
        ..Default::default()
    };
    for a in &r.actors {
        m.participants.push(a.element(Vec::new()));
    }
    qualified(p, m)
}
pub fn build_query(p: &ClinicalPolicy, q: &QueryRequest) -> Result<Message> {
    if !p.include_query {
        return Err(Error::clinical("opt_in", "Query"));
    }
    let mut q = q.clone();
    q.common = clinical_common(&q.common);
    validate_query_data(p, &q.query, &q.transfer_syntax_uid)?;
    qualified(p, query(&q))
}
pub(crate) fn clinical_uid(v: &str) -> bool {
    if v.is_empty() || v.len() > 64 || super::trim_unicode_space(v) != v {
        return false;
    }
    let parts: Vec<&str> = v.split('.').collect();
    if parts.len() < 2 {
        return false;
    }
    parts.iter().all(|part| {
        !part.is_empty()
            && !(part.len() > 1 && part.starts_with('0'))
            && part.bytes().all(|c| c.is_ascii_digit())
    })
}
pub(crate) fn clinical_text(v: &str) -> bool {
    if super::trim_unicode_space(v).is_empty() {
        return false;
    }
    v.chars().all(|c| {
        let r = c as u32;
        r == 9
            || r == 10
            || r == 13
            || (0x20..=0xd7ff).contains(&r)
            || (0xe000..=0xfffd).contains(&r)
            || (0x10000..=0x10ffff).contains(&r)
    })
}
pub(crate) fn validate_query_data(policy: &ClinicalPolicy, data: &[u8], uid: &str) -> Result<()> {
    let p = policy.normalize()?;
    if data.is_empty() {
        return Err(Error::clinical("required", "Query.DataSet"));
    }
    if data.len() as i64 > p.limits.max_query_bytes {
        return Err(Error::clinical("limit", "Query.DataSet"));
    }
    let syntax = match object::transfer::lookup(uid) {
        Some(s) if !s.encapsulated => s,
        _ => return Err(Error::clinical("syntax", "Query.TransferSyntax")),
    };
    let options = ReadFileOptions {
        max_total_bytes: p.limits.max_query_bytes,
        max_element_bytes: p.limits.max_query_bytes,
        max_elements: 4096,
        max_sequence_depth: 16,
        ..ReadFileOptions::default()
    };
    let shared: Rc<[u8]> = Rc::from(data);
    match object::read_data_set_with_options(Box::new(MemFile::new(shared)), syntax, options) {
        Ok(ds) if !ds.is_empty() => Ok(()),
        _ => Err(Error::clinical("dataset", "Query.DataSet")),
    }
}
pub fn encode_clinical_query(
    p: &ClinicalPolicy,
    ds: Option<&Object>,
    syntax: &Syntax,
) -> Result<Vec<u8>> {
    if !p.include_query {
        return Err(Error::clinical("opt_in", "Query"));
    }
    let p = p.normalize()?;
    let Some(ds) = ds else {
        return Err(Error::clinical("required", "Query.DataSet"));
    };
    let data = match object::writer::write_data_set(ds, syntax.uid) {
        Ok(data) => data,
        Err(_) => return Err(Error::clinical("dataset", "Query.DataSet")),
    };
    if data.len() as i64 > p.limits.max_query_bytes {
        return Err(Error::clinical("dataset", "Query.DataSet"));
    }
    validate_query_data(&p, &data, syntax.uid)?;
    Ok(data)
}
pub fn clinical_identity_from_object(
    p: &ClinicalPolicy,
    ds: Option<&Object>,
) -> Result<(PatientIdentity, StudyIdentity)> {
    if !p.include_identities {
        return Err(Error::clinical("opt_in", "Identity"));
    }
    let p = p.normalize()?;
    let Some(ds) = ds else {
        return Err(Error::clinical("required", "DataSet"));
    };
    let get = |g: u16, e: u16| {
        ds.get_string(Tag {
            group: g,
            element: e,
        })
        .unwrap_or_default()
    };
    let patient = PatientIdentity {
        id: get(0x0010, 0x0020),
        name: get(0x0010, 0x0010),
        issuer: get(0x0010, 0x0021),
    };
    let mut study = StudyIdentity {
        uid: get(0x0020, 0x000d),
        name: get(0x0008, 0x1030),
        ..Default::default()
    };
    if study.name.is_empty() {
        study.name = get(0x0020, 0x0010);
    }
    if !clinical_text(&patient.id) || !clinical_text(&patient.name) {
        return Err(Error::clinical("required", "Patient"));
    }
    if !clinical_uid(&study.uid) || !clinical_text(&study.name) {
        return Err(Error::clinical("required", "Study"));
    }
    if (patient.id.len()
        + patient.name.len()
        + patient.issuer.len()
        + study.uid.len()
        + study.name.len()) as i64
        > p.limits.max_identity_bytes
    {
        return Err(Error::clinical("limit", "Identity"));
    }
    Ok((patient, study))
}
