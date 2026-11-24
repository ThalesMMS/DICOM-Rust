use super::manifest::{DICOM_MEDIA_TYPE, Manifest};
use super::wire::{NS_LCM, NS_RIM, NS_XDS, NS_XOP, Node, element};
use super::{Error, PatientId, is_printable_ascii, trim_unicode_space};
use crate::dcmtime::ZonedTime;
use crate::object::is_valid_uid;
use crate::sr::KEY_OBJECT_SELECTION_DOCUMENT_STORAGE;
use sha1::{Digest, Sha1};
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Code {
    pub value: String,
    pub scheme: String,
    pub meaning: String,
}
impl Code {
    fn valid(&self) -> bool {
        clean_text(&self.value, 256)
            && clean_text(&self.scheme, 256)
            && clean_text(&self.meaning, 256)
    }
}
fn clean_text(s: &str, limit: usize) -> bool {
    !s.is_empty() && s.len() <= limit && trim_unicode_space(s) == s && is_printable_ascii(s)
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PublicationMetadata {
    pub patient: PatientId,
    pub source_patient: PatientId,
    pub submission_uid: String,
    pub source_uid: String,
    pub title: String,
    pub language_code: String,
    pub created_at: Option<ZonedTime>,
    pub submitted_at: Option<ZonedTime>,
    pub class_code: Code,
    pub type_code: Code,
    pub confidentiality_code: Code,
    pub facility_code: Code,
    pub practice_code: Code,
    pub content_type_code: Code,
    pub event_codes: Vec<Code>,
    pub author_person: String,
    pub author_institution: String,
    pub author_role: String,
    pub author_specialty: String,
    pub service_start: Option<ZonedTime>,
    pub service_stop: Option<ZonedTime>,
    pub replaces_entry_uuid: String,
}
fn non_zero(t: Option<ZonedTime>) -> Option<ZonedTime> {
    t.filter(|t| !t.is_zero())
}
fn timestamp(t: ZonedTime) -> String {
    let utc = ZonedTime::from_unix_utc(t.unix(), t.nanosecond());
    let (year, month, day) = utc.date();
    format!(
        "{year:04}{month:02}{day:02}{:02}{:02}{:02}",
        utc.hour(),
        utc.minute(),
        utc.second()
    )
}
impl PublicationMetadata {
    pub(super) fn validate(&self, manifest: Option<&Manifest>) -> Result<(), Error> {
        let Some(manifest) = manifest else {
            return Err(Error::Mapping);
        };
        if !self.patient.valid()
            || !self.source_patient.valid()
            || self.source_patient != manifest.patient
        {
            return Err(Error::Mapping);
        }
        if !is_valid_uid(self.submission_uid.as_bytes())
            || !is_valid_uid(self.source_uid.as_bytes())
            || self.submission_uid == manifest.uid
            || !clean_text(&self.title, 256)
            || !clean_text(&self.language_code, 35)
            || non_zero(self.created_at).is_none()
            || non_zero(self.submitted_at).is_none()
            || self.event_codes.len() > 64
            || !valid_entry_uuid(&self.replaces_entry_uuid)
        {
            return Err(Error::Manifest);
        }
        let codes = [
            &self.class_code,
            &self.type_code,
            &self.confidentiality_code,
            &self.facility_code,
            &self.practice_code,
            &self.content_type_code,
        ];
        if codes
            .into_iter()
            .chain(&self.event_codes)
            .any(|c| !c.valid())
        {
            return Err(Error::Manifest);
        }
        let authors = [
            &self.author_person,
            &self.author_institution,
            &self.author_role,
            &self.author_specialty,
        ];
        if authors.iter().any(|a| !a.is_empty() && !clean_text(a, 256)) {
            return Err(Error::Manifest);
        }
        if let (Some(start), Some(stop)) =
            (non_zero(self.service_start), non_zero(self.service_stop))
            && stop.nanos_since_zero() < start.nanos_since_zero()
        {
            return Err(Error::Manifest);
        }
        Ok(())
    }
}
fn valid_entry_uuid(value: &str) -> bool {
    if value.is_empty() {
        return true;
    }
    let Some(rest) = value
        .strip_prefix("urn:uuid:")
        .filter(|_| value.len() == 45)
    else {
        return false;
    };
    rest.bytes().enumerate().all(|(i, c)| {
        if matches!(i, 8 | 13 | 18 | 23) {
            c == b'-'
        } else {
            c.is_ascii_hexdigit()
        }
    })
}
fn slot(name: &str, values: &[&str]) -> Node {
    let list = element(NS_RIM, "ValueList", "", &[]).with(
        values
            .iter()
            .map(|v| element(NS_RIM, "Value", v, &[]))
            .collect(),
    );
    element(NS_RIM, "Slot", "", &[("name", name)]).with(vec![list])
}
fn name(value: &str) -> Node {
    element(NS_RIM, "Name", "", &[]).with(vec![element(
        NS_RIM,
        "LocalizedString",
        "",
        &[("value", value)],
    )])
}
fn classification(target: &str, id: &str, scheme: &str, code: &Code) -> Node {
    element(
        NS_RIM,
        "Classification",
        "",
        &[
            ("id", id),
            ("classifiedObject", target),
            ("classificationScheme", scheme),
            ("nodeRepresentation", &code.value),
        ],
    )
    .with(vec![
        slot("codingScheme", &[&code.scheme]),
        name(&code.meaning),
    ])
}
fn external_id(target: &str, id: &str, scheme: &str, label: &str, value: &str) -> Node {
    element(
        NS_RIM,
        "ExternalIdentifier",
        "",
        &[
            ("id", id),
            ("registryObject", target),
            ("identificationScheme", scheme),
            ("value", value),
        ],
    )
    .with(vec![name(label)])
}
fn author_metadata(target: &str, id: &str, scheme: &str, m: &PublicationMetadata) -> Option<Node> {
    let slots: Vec<Node> = [
        ("authorPerson", &m.author_person),
        ("authorInstitution", &m.author_institution),
        ("authorRole", &m.author_role),
        ("authorSpecialty", &m.author_specialty),
    ]
    .into_iter()
    .filter(|(_, v)| !v.is_empty())
    .map(|(k, v)| slot(k, &[v]))
    .collect();
    if slots.is_empty() {
        return None;
    }
    Some(
        element(
            NS_RIM,
            "Classification",
            "",
            &[
                ("id", id),
                ("classifiedObject", target),
                ("classificationScheme", scheme),
                ("nodeRepresentation", ""),
            ],
        )
        .with(slots),
    )
}
pub(super) fn publication_body(
    manifest: &Manifest,
    m: &PublicationMetadata,
    entry_id: &str,
    set_id: &str,
) -> Node {
    const DOC_TYPE: &str = "urn:uuid:7edca82f-054d-47f2-a032-9b2a5b5186c1";
    let digest: String = Sha1::digest(&manifest.data)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let size = manifest.data.len().to_string();
    let reference = format!(
        "{}^^^^urn:ihe:iti:xds:2016:studyInstanceUID",
        manifest.study_uid
    );
    let created = m.created_at.map(timestamp).unwrap_or_default();
    let mut doc_children = vec![
        slot("creationTime", &[&created]),
        slot("languageCode", &[&m.language_code]),
        slot("sourcePatientId", &[&m.source_patient.cx()]),
        slot("hash", &[&digest]),
        slot("size", &[&size]),
        slot("urn:ihe:iti:xds:2013:referenceIdList", &[&reference]),
        name(&m.title),
        external_id(
            entry_id,
            "document-patient",
            "urn:uuid:58a6f841-87b3-4a3e-92fd-a8ffeff98427",
            "XDSDocumentEntry.patientId",
            &m.patient.cx(),
        ),
        external_id(
            entry_id,
            "document-unique",
            "urn:uuid:2e82c1f6-a085-4c72-9da3-8640a32e42ab",
            "XDSDocumentEntry.uniqueId",
            &manifest.uid,
        ),
    ];
    if let Some(start) = non_zero(m.service_start) {
        doc_children.push(slot("serviceStartTime", &[&timestamp(start)]));
    }
    if let Some(stop) = non_zero(m.service_stop) {
        doc_children.push(slot("serviceStopTime", &[&timestamp(stop)]));
    }
    let format = Code {
        value: KEY_OBJECT_SELECTION_DOCUMENT_STORAGE.into(),
        scheme: "1.2.840.10008.2.6.1".into(),
        meaning: "DICOM Manifest".into(),
    };
    for (id, scheme, code) in [
        (
            "class",
            "urn:uuid:41a5887f-8865-4c09-adf7-e362475b143a",
            &m.class_code,
        ),
        (
            "type",
            "urn:uuid:f0306f51-975f-434e-a61c-c59651d33983",
            &m.type_code,
        ),
        (
            "confidentiality",
            "urn:uuid:f4f85eac-e6cb-4883-b524-f2705394840f",
            &m.confidentiality_code,
        ),
        (
            "facility",
            "urn:uuid:f33fb8ac-18af-42cc-ae0e-ed0b0bdb91e1",
            &m.facility_code,
        ),
        (
            "practice",
            "urn:uuid:cccf5598-8b07-4b77-a05e-ae952c785ead",
            &m.practice_code,
        ),
        (
            "format",
            "urn:uuid:a09d5840-386c-46f2-b5ad-9c3699a4309d",
            &format,
        ),
    ] {
        doc_children.push(classification(
            entry_id,
            &format!("document-{id}"),
            scheme,
            code,
        ));
    }
    for (i, code) in m.event_codes.iter().enumerate() {
        doc_children.push(classification(
            entry_id,
            &format!("event-{i}"),
            "urn:uuid:2c6b8cb7-8b2a-4051-b291-b1ae6a575ef4",
            code,
        ));
    }
    if let Some(author) = author_metadata(
        entry_id,
        "document-author",
        "urn:uuid:93606bcf-9494-43ec-9b4e-a7748d1a838d",
        m,
    ) {
        doc_children.push(author);
    }
    let doc = element(
        NS_RIM,
        "ExtrinsicObject",
        "",
        &[
            ("id", entry_id),
            ("mimeType", DICOM_MEDIA_TYPE),
            ("objectType", DOC_TYPE),
        ],
    )
    .with(doc_children);
    let submitted = m.submitted_at.map(timestamp).unwrap_or_default();
    let mut set_children = vec![
        slot("submissionTime", &[&submitted]),
        name(&m.title),
        external_id(
            set_id,
            "submission-unique",
            "urn:uuid:96fdda7c-d067-4183-912e-bf5ee74998a8",
            "XDSSubmissionSet.uniqueId",
            &m.submission_uid,
        ),
        external_id(
            set_id,
            "submission-source",
            "urn:uuid:554ac39e-e3fe-47fe-b233-965d2a147832",
            "XDSSubmissionSet.sourceId",
            &m.source_uid,
        ),
        external_id(
            set_id,
            "submission-patient",
            "urn:uuid:6b5aea1a-874d-4603-a4bc-96a0a7b38446",
            "XDSSubmissionSet.patientId",
            &m.patient.cx(),
        ),
        classification(
            set_id,
            "submission-content",
            "urn:uuid:aa543740-bdda-424e-8c96-df4873be8500",
            &m.content_type_code,
        ),
    ];
    if let Some(author) = author_metadata(
        set_id,
        "submission-author",
        "urn:uuid:a7058bb9-b4e4-4307-ba5b-e3f0ab85e12d",
        m,
    ) {
        set_children.push(author);
    }
    let set = element(NS_RIM, "RegistryPackage", "", &[("id", set_id)]).with(set_children);
    let set_type = element(
        NS_RIM,
        "Classification",
        "",
        &[
            ("id", "submission-set-type"),
            ("classifiedObject", set_id),
            (
                "classificationNode",
                "urn:uuid:a54d6aa5-d40d-43f9-88c5-b4633d873bdd",
            ),
        ],
    );
    let association = element(
        NS_RIM,
        "Association",
        "",
        &[
            ("id", "submission-document"),
            (
                "associationType",
                "urn:oasis:names:tc:ebxml-regrep:AssociationType:HasMember",
            ),
            ("sourceObject", set_id),
            ("targetObject", entry_id),
        ],
    )
    .with(vec![slot("SubmissionSetStatus", &["Original"])]);
    let mut list = vec![doc, set, set_type, association];
    if !m.replaces_entry_uuid.is_empty() {
        list.push(element(
            NS_RIM,
            "Association",
            "",
            &[
                ("id", "replacement"),
                ("associationType", "urn:ihe:iti:2007:AssociationType:RPLC"),
                ("sourceObject", entry_id),
                ("targetObject", &m.replaces_entry_uuid),
            ],
        ));
    }
    let submit = element(NS_LCM, "SubmitObjectsRequest", "", &[]).with(vec![
        element(NS_RIM, "RegistryObjectList", "", &[]).with(list),
    ]);
    let attachment = element(NS_XDS, "Document", "", &[("id", entry_id)]).with(vec![element(
        NS_XOP,
        "Include",
        "",
        &[("href", "cid:manifest@xdsi")],
    )]);
    element(NS_XDS, "ProvideAndRegisterDocumentSetRequest", "", &[]).with(vec![submit, attachment])
}
