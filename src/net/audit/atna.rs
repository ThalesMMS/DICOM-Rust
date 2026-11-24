#![forbid(unsafe_code)]
use super::timetext;
use super::{Error, ErrorKind, Result, Time};
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct Code {
    pub value: String,
    pub code_system_name: String,
    pub original_text: String,
}
impl Code {
    pub fn new(value: &str, code_system_name: &str, original_text: &str) -> Code {
        Code {
            value: value.to_string(),
            code_system_name: code_system_name.to_string(),
            original_text: original_text.to_string(),
        }
    }
    pub(crate) fn text_len(&self) -> usize {
        self.value.len() + self.code_system_name.len() + self.original_text.len()
    }
}
fn dcm(value: &str, meaning: &str) -> Code {
    Code::new(value, "DCM", meaning)
}
macro_rules! dcm_codes {
    ($($(#[$doc:meta])* $name:ident = ($value:literal, $meaning:literal);)*) => {
        $(
            $(#[$doc])*
            #[doc = concat!("The DCM code ", $value, " (", $meaning, ").")]
            pub fn $name() -> Code { dcm($value, $meaning) }
        )*
    };
}
dcm_codes! {
    event_application_activity = ("110100", "Application Activity");
    event_audit_log_used = ("110101", "Audit Log Used");
    event_begin_transferring = ("110102", "Begin Transferring DICOM Instances");
    event_instances_accessed = ("110103", "DICOM Instances Accessed");
    event_instances_transferred = ("110104", "DICOM Instances Transferred");
    event_study_deleted = ("110105", "DICOM Study Deleted");
    event_export = ("110106", "Export");
    event_import = ("110107", "Import");
    event_order_record = ("110109", "Order Record");
    event_patient_record = ("110110", "Patient Record");
    event_procedure_record = ("110111", "Procedure Record");
    event_query = ("110112", "Query");
    event_security_alert = ("110113", "Security Alert");
    event_user_authentication = ("110114", "User Authentication");
    type_application_start = ("110120", "Application Start");
    type_application_stop = ("110121", "Application Stop");
    type_login = ("110122", "Login");
    type_logout = ("110123", "Logout");
    type_node_authentication = ("110126", "Node Authentication");
    type_network_configuration = ("110128", "Network Configuration");
    type_security_configuration = ("110129", "Security Configuration");
    type_software_configuration = ("110131", "Software Configuration");
    type_use_of_restricted_feature = ("110132", "Use of Restricted Function");
    type_audit_recording_stopped = ("110133", "Audit Recording Stopped");
    type_audit_recording_started = ("110134", "Audit Recording Started");
    role_application = ("110150", "Application");
    role_application_launcher = ("110151", "Application Launcher");
    role_destination = ("110152", "Destination Role ID");
    role_source = ("110153", "Source Role ID");
    role_destination_media = ("110154", "Destination Media");
    role_source_media = ("110155", "Source Media");
}
pub(crate) const ID_STUDY_INSTANCE_UID: &str = "110180";
pub(crate) const ID_SOP_CLASS_UID: &str = "110181";
fn id_study_instance_uid() -> Code {
    dcm(ID_STUDY_INSTANCE_UID, "Study Instance UID")
}
fn id_sop_class_uid() -> Code {
    dcm(ID_SOP_CLASS_UID, "SOP Class UID")
}
fn id_node_id() -> Code {
    dcm("110182", "Node ID")
}
fn id_patient_number() -> Code {
    Code::new("2", "RFC-3881", "Patient Number")
}
pub const ACTION_CREATE: &str = "C";
pub const ACTION_READ: &str = "R";
pub const ACTION_UPDATE: &str = "U";
pub const ACTION_DELETE: &str = "D";
pub const ACTION_EXECUTE: &str = "E";
pub const OUTCOME_SUCCESS: &str = "0";
pub const OUTCOME_MINOR_FAILURE: &str = "4";
pub const OUTCOME_SERIOUS: &str = "8";
pub const OUTCOME_MAJOR_FAILURE: &str = "12";
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Message {
    pub event: EventIdentification,
    pub participants: Vec<ActiveParticipant>,
    pub sources: Vec<AuditSource>,
    pub objects: Vec<ParticipantObject>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EventIdentification {
    pub id: Code,
    pub type_codes: Vec<Code>,
    pub outcome_description: String,
    pub action_code: String,
    pub date_time: String,
    pub outcome_indicator: String,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ActiveParticipant {
    pub role_id_codes: Vec<Code>,
    pub media_identifier: Option<MediaIdentifier>,
    pub user_id: String,
    pub alternative_user_id: String,
    pub user_name: String,
    pub user_is_requestor: bool,
    pub network_access_point_id: String,
    pub network_access_point_type_code: String,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MediaIdentifier {
    pub media_type: Code,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AuditSource {
    pub enterprise_site_id: String,
    pub id: String,
    pub type_codes: Vec<Code>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParticipantObject {
    pub id_type_code: Code,
    pub name: String,
    pub query: String,
    pub details: Vec<ParticipantObjectDetail>,
    pub description: ParticipantObjectDescription,
    pub id: String,
    pub type_code: String,
    pub role: String,
    pub life_cycle: String,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParticipantObjectDetail {
    pub detail_type: String,
    pub value: String,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParticipantObjectDescription {
    pub accessions: Vec<Accession>,
    pub sop_classes: Vec<SopClass>,
    pub contains_study: ContainsStudy,
    pub encrypted: Option<bool>,
    pub anonymized: Option<bool>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Accession {
    pub number: String,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SopClass {
    pub instances: Vec<StudyId>,
    pub uid: String,
    pub number_of_instances: i64,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ContainsStudy {
    pub studies: Vec<StudyId>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StudyId {
    pub uid: String,
}
pub(crate) const XML_HEADER: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n";
fn escape(out: &mut String, s: &str) {
    for c in s.chars() {
        match c {
            '"' => out.push_str("&#34;"),
            '\'' => out.push_str("&#39;"),
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\t' => out.push_str("&#x9;"),
            '\n' => out.push_str("&#xA;"),
            '\r' => out.push_str("&#xD;"),
            c if in_character_range(c) => out.push(c),
            _ => out.push('\u{fffd}'),
        }
    }
}
fn in_character_range(c: char) -> bool {
    let r = c as u32;
    r == 0x09
        || r == 0x0a
        || r == 0x0d
        || (0x20..=0xd7ff).contains(&r)
        || (0xe000..=0xfffd).contains(&r)
        || (0x10000..=0x10ffff).contains(&r)
}
struct Xml(String);
impl Xml {
    fn open(&mut self, name: &str) {
        self.0.push('<');
        self.0.push_str(name);
    }
    fn attr(&mut self, name: &str, value: &str) {
        self.0.push(' ');
        self.0.push_str(name);
        self.0.push_str("=\"");
        escape(&mut self.0, value);
        self.0.push('"');
    }
    fn attr_omit(&mut self, name: &str, value: &str) {
        if !value.is_empty() {
            self.attr(name, value);
        }
    }
    fn close_tag(&mut self) {
        self.0.push('>');
    }
    fn end(&mut self, name: &str) {
        self.0.push_str("</");
        self.0.push_str(name);
        self.0.push('>');
    }
    fn text_element(&mut self, name: &str, value: &str) {
        self.open(name);
        self.close_tag();
        escape(&mut self.0, value);
        self.end(name);
    }
    fn code(&mut self, name: &str, c: &Code) {
        self.open(name);
        self.attr("csd-code", &c.value);
        self.attr_omit("codeSystemName", &c.code_system_name);
        self.attr_omit("originalText", &c.original_text);
        self.close_tag();
        self.end(name);
    }
    fn uid_element(&mut self, name: &str, uid: &str) {
        self.open(name);
        self.attr("UID", uid);
        self.close_tag();
        self.end(name);
    }
}
fn bool_text(v: bool) -> &'static str {
    if v { "true" } else { "false" }
}
pub(crate) fn encode_message(m: &Message) -> String {
    let mut x = Xml(String::new());
    x.open("AuditMessage");
    x.close_tag();
    let e = &m.event;
    x.open("EventIdentification");
    x.attr_omit("EventActionCode", &e.action_code);
    x.attr("EventDateTime", &e.date_time);
    x.attr("EventOutcomeIndicator", &e.outcome_indicator);
    x.close_tag();
    x.code("EventID", &e.id);
    for c in &e.type_codes {
        x.code("EventTypeCode", c);
    }
    if !e.outcome_description.is_empty() {
        x.text_element("EventOutcomeDescription", &e.outcome_description);
    }
    x.end("EventIdentification");
    for p in &m.participants {
        x.open("ActiveParticipant");
        x.attr("UserID", &p.user_id);
        x.attr_omit("AlternativeUserID", &p.alternative_user_id);
        x.attr_omit("UserName", &p.user_name);
        x.attr("UserIsRequestor", bool_text(p.user_is_requestor));
        x.attr_omit("NetworkAccessPointID", &p.network_access_point_id);
        x.attr_omit(
            "NetworkAccessPointTypeCode",
            &p.network_access_point_type_code,
        );
        x.close_tag();
        for c in &p.role_id_codes {
            x.code("RoleIDCode", c);
        }
        if let Some(media) = &p.media_identifier {
            x.open("MediaIdentifier");
            x.close_tag();
            x.code("MediaType", &media.media_type);
            x.end("MediaIdentifier");
        }
        x.end("ActiveParticipant");
    }
    for s in &m.sources {
        x.open("AuditSourceIdentification");
        x.attr_omit("AuditEnterpriseSiteID", &s.enterprise_site_id);
        x.attr("AuditSourceID", &s.id);
        x.close_tag();
        for c in &s.type_codes {
            x.code("AuditSourceTypeCode", c);
        }
        x.end("AuditSourceIdentification");
    }
    for o in &m.objects {
        x.open("ParticipantObjectIdentification");
        x.attr("ParticipantObjectID", &o.id);
        x.attr_omit("ParticipantObjectTypeCode", &o.type_code);
        x.attr_omit("ParticipantObjectTypeCodeRole", &o.role);
        x.attr_omit("ParticipantObjectDataLifeCycle", &o.life_cycle);
        x.close_tag();
        x.code("ParticipantObjectIDTypeCode", &o.id_type_code);
        if !o.name.is_empty() {
            x.text_element("ParticipantObjectName", &o.name);
        }
        if !o.query.is_empty() {
            x.text_element("ParticipantObjectQuery", &o.query);
        }
        for d in &o.details {
            x.open("ParticipantObjectDetail");
            x.attr("type", &d.detail_type);
            x.attr("value", &d.value);
            x.close_tag();
            x.end("ParticipantObjectDetail");
        }
        let d = &o.description;
        if !d.accessions.is_empty()
            || !d.sop_classes.is_empty()
            || !d.contains_study.studies.is_empty()
            || d.encrypted.is_some()
            || d.anonymized.is_some()
        {
            x.open("ParticipantObjectDescription");
            x.close_tag();
            for a in &d.accessions {
                x.open("Accession");
                x.attr("Number", &a.number);
                x.close_tag();
                x.end("Accession");
            }
            for c in &d.sop_classes {
                x.open("SOPClass");
                x.attr("UID", &c.uid);
                x.attr("NumberOfInstances", &c.number_of_instances.to_string());
                x.close_tag();
                for i in &c.instances {
                    x.uid_element("Instance", &i.uid);
                }
                x.end("SOPClass");
            }
            if !d.contains_study.studies.is_empty() {
                x.open("ParticipantObjectContainsStudy");
                x.close_tag();
                for s in &d.contains_study.studies {
                    x.uid_element("StudyIDs", &s.uid);
                }
                x.end("ParticipantObjectContainsStudy");
            }
            if let Some(v) = d.encrypted {
                x.text_element("Encrypted", bool_text(v));
            }
            if let Some(v) = d.anonymized {
                x.text_element("Anonymized", bool_text(v));
            }
            x.end("ParticipantObjectDescription");
        }
        x.end("ParticipantObjectIdentification");
    }
    x.end("AuditMessage");
    x.0
}
impl Message {
    pub fn marshal(&self) -> Result<Vec<u8>> {
        self.validate()?;
        let mut out = String::from(XML_HEADER);
        out.push_str(&encode_message(self));
        Ok(out.into_bytes())
    }
    pub(crate) fn validate(&self) -> Result<()> {
        let bad = |detail: &str| Err(Error::wrap(ErrorKind::AuditMessage, detail));
        if self.event.id.value.is_empty() {
            return bad("no EventID");
        }
        if self.event.date_time.is_empty() {
            return bad("no EventDateTime");
        }
        if self.event.outcome_indicator.is_empty() {
            return bad("no EventOutcomeIndicator");
        }
        if self.participants.is_empty() {
            return bad("no ActiveParticipant");
        }
        if self.sources.is_empty() {
            return bad("no AuditSourceIdentification");
        }
        if self.participants.iter().any(|p| p.user_id.is_empty()) {
            return bad("ActiveParticipant without UserID");
        }
        if self.sources.iter().any(|s| s.id.is_empty()) {
            return bad("AuditSourceIdentification without AuditSourceID");
        }
        for object in &self.objects {
            if object.id.is_empty() || object.id_type_code.value.is_empty() {
                return bad("ParticipantObjectIdentification without ID or ID type");
            }
            if !object.name.is_empty() && !object.query.is_empty() {
                return bad("ParticipantObjectName and ParticipantObjectQuery are exclusive");
            }
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Source {
    pub id: String,
    pub site_id: String,
    pub source_type: String,
}
impl Source {
    pub(crate) fn element(&self) -> AuditSource {
        let kind = if self.source_type.is_empty() {
            "4"
        } else {
            &self.source_type
        };
        AuditSource {
            enterprise_site_id: self.site_id.clone(),
            id: self.id.clone(),
            type_codes: vec![Code::new(kind, "", "")],
        }
    }
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Participant {
    pub user_id: String,
    pub alternative_user_id: String,
    pub user_name: String,
    pub requestor: bool,
    pub host: String,
}
impl Participant {
    pub(crate) fn element(&self, roles: Vec<Code>) -> ActiveParticipant {
        let mut out = ActiveParticipant {
            role_id_codes: roles,
            media_identifier: None,
            user_id: self.user_id.clone(),
            alternative_user_id: self.alternative_user_id.clone(),
            user_name: self.user_name.clone(),
            user_is_requestor: self.requestor,
            network_access_point_id: self.host.clone(),
            network_access_point_type_code: String::new(),
        };
        if !self.host.is_empty() {
            out.network_access_point_type_code =
                if is_ip_address(&self.host) { "2" } else { "1" }.to_string();
        }
        out
    }
}
fn is_ip_address(host: &str) -> bool {
    let host = host.trim_matches(|c| c == '[' || c == ']');
    if host.contains(':') {
        return true;
    }
    let parts: Vec<&str> = host.split('.').collect();
    parts.len() == 4
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.bytes().all(|c| c.is_ascii_digit()))
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Study {
    pub uid: String,
    pub accessions: Vec<String>,
    pub sop_classes: Vec<SopClass>,
}
impl Study {
    pub(crate) fn object(&self, life_cycle: &str) -> ParticipantObject {
        let mut description = ParticipantObjectDescription {
            sop_classes: self.sop_classes.clone(),
            ..Default::default()
        };
        for number in &self.accessions {
            let number = super::trim_unicode_space(number);
            if !number.is_empty() {
                description.accessions.push(Accession {
                    number: number.to_string(),
                });
            }
        }
        ParticipantObject {
            id_type_code: id_study_instance_uid(),
            id: self.uid.clone(),
            type_code: "2".to_string(),
            role: "3".to_string(),
            life_cycle: life_cycle.to_string(),
            description,
            ..Default::default()
        }
    }
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Patient {
    pub id: String,
    pub name: String,
}
impl Patient {
    pub(crate) fn object(&self) -> ParticipantObject {
        ParticipantObject {
            id_type_code: id_patient_number(),
            id: self.id.clone(),
            name: self.name.clone(),
            type_code: "1".to_string(),
            role: "1".to_string(),
            ..Default::default()
        }
    }
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Common {
    pub time: Time,
    pub outcome: String,
    pub outcome_description: String,
    pub source: Source,
}
impl Common {
    pub(crate) fn event(&self, id: Code, action: &str, types: Vec<Code>) -> EventIdentification {
        let at = if self.time.is_zero() {
            Time::now_local()
        } else {
            self.time
        };
        let outcome = if self.outcome.is_empty() {
            OUTCOME_SUCCESS
        } else {
            &self.outcome
        };
        EventIdentification {
            id,
            type_codes: types,
            outcome_description: self.outcome_description.clone(),
            action_code: action.to_string(),
            date_time: timetext::format_rfc3339_nano(&at),
            outcome_indicator: outcome.to_string(),
        }
    }
}
pub fn application_activity(
    c: &Common,
    kind: Code,
    application: &Participant,
    launcher: Option<&Participant>,
) -> Message {
    let mut application = application.clone();
    application.requestor = false;
    let mut m = Message {
        event: c.event(event_application_activity(), ACTION_EXECUTE, vec![kind]),
        participants: vec![application.element(vec![role_application()])],
        sources: vec![c.source.element()],
        objects: Vec::new(),
    };
    if let Some(launcher) = launcher {
        let mut l = launcher.clone();
        l.requestor = true;
        m.participants
            .push(l.element(vec![role_application_launcher()]));
    }
    m
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Transfer {
    pub common: Common,
    pub source: Participant,
    pub destination: Participant,
    pub requestor: Option<Participant>,
    pub studies: Vec<Study>,
    pub patient: Patient,
}
impl Transfer {
    fn message(&self, id: Code, action: &str, life_cycle: &str) -> Message {
        let mut m = Message {
            event: self.common.event(id, action, Vec::new()),
            participants: vec![
                self.source.element(vec![role_source()]),
                self.destination.element(vec![role_destination()]),
            ],
            sources: vec![self.common.source.element()],
            objects: Vec::new(),
        };
        if let Some(requestor) = &self.requestor {
            let mut r = requestor.clone();
            r.requestor = true;
            m.participants.push(r.element(Vec::new()));
        }
        for study in &self.studies {
            m.objects.push(study.object(life_cycle));
        }
        if !self.patient.id.is_empty() {
            m.objects.push(self.patient.object());
        }
        m
    }
}
pub fn begin_transferring(t: &Transfer) -> Message {
    t.message(event_begin_transferring(), ACTION_EXECUTE, "")
}
pub fn instances_transferred(t: &Transfer, action: &str) -> Message {
    t.message(event_instances_transferred(), action, "")
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Access {
    pub common: Common,
    pub user: Participant,
    pub studies: Vec<Study>,
    pub patient: Patient,
}
pub fn instances_accessed(a: &Access, action: &str) -> Message {
    let mut user = a.user.clone();
    user.requestor = true;
    let mut m = Message {
        event: a
            .common
            .event(event_instances_accessed(), action, Vec::new()),
        participants: vec![user.element(Vec::new())],
        sources: vec![a.common.source.element()],
        objects: Vec::new(),
    };
    for study in &a.studies {
        m.objects.push(study.object("6"));
    }
    if !a.patient.id.is_empty() {
        m.objects.push(a.patient.object());
    }
    m
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct QueryRequest {
    pub common: Common,
    pub source: Participant,
    pub destination: Participant,
    pub sop_class_uid: String,
    pub query: Vec<u8>,
    pub transfer_syntax_uid: String,
}
pub(crate) fn base64(data: &[u8]) -> String {
    crate::dicomjson::sizedparse::base64_encode(data)
}
pub fn query(q: &QueryRequest) -> Message {
    let mut source = q.source.clone();
    source.requestor = true;
    let mut object = ParticipantObject {
        id_type_code: id_sop_class_uid(),
        id: q.sop_class_uid.clone(),
        type_code: "2".to_string(),
        role: "3".to_string(),
        query: base64(&q.query),
        ..Default::default()
    };
    if !q.transfer_syntax_uid.is_empty() {
        object.details = vec![ParticipantObjectDetail {
            detail_type: "TransferSyntax".to_string(),
            value: base64(q.transfer_syntax_uid.as_bytes()),
        }];
    }
    Message {
        event: q.common.event(event_query(), ACTION_EXECUTE, Vec::new()),
        participants: vec![
            source.element(vec![role_source()]),
            q.destination.element(vec![role_destination()]),
        ],
        sources: vec![q.common.source.element()],
        objects: vec![object],
    }
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExportRequest {
    pub common: Common,
    pub user: Participant,
    pub media: String,
    pub studies: Vec<Study>,
    pub patient: Patient,
}
pub fn export(e: &ExportRequest) -> Message {
    let mut user = e.user.clone();
    user.requestor = true;
    let mut m = Message {
        event: e.common.event(event_export(), ACTION_READ, Vec::new()),
        participants: vec![
            user.element(vec![role_source()]),
            ActiveParticipant {
                role_id_codes: vec![role_destination_media()],
                user_id: e.media.clone(),
                user_is_requestor: false,
                ..Default::default()
            },
        ],
        sources: vec![e.common.source.element()],
        objects: Vec::new(),
    };
    for study in &e.studies {
        m.objects.push(study.object("10"));
    }
    if !e.patient.id.is_empty() {
        m.objects.push(e.patient.object());
    }
    m
}
pub fn user_authentication(
    c: &Common,
    kind: Code,
    user: &Participant,
    node: Option<&Participant>,
) -> Message {
    let mut user = user.clone();
    user.requestor = true;
    let mut m = Message {
        event: c.event(event_user_authentication(), ACTION_EXECUTE, vec![kind]),
        participants: vec![user.element(Vec::new())],
        sources: vec![c.source.element()],
        objects: Vec::new(),
    };
    if let Some(node) = node {
        let mut n = node.clone();
        n.requestor = false;
        m.participants.push(n.element(Vec::new()));
    }
    m
}
pub fn security_alert(c: &Common, kind: Code, reporter: &Participant, subject: &str) -> Message {
    let mut reporter = reporter.clone();
    reporter.requestor = false;
    let mut m = Message {
        event: c.event(event_security_alert(), ACTION_EXECUTE, vec![kind]),
        participants: vec![reporter.element(Vec::new())],
        sources: vec![c.source.element()],
        objects: Vec::new(),
    };
    if !subject.is_empty() {
        m.objects = vec![ParticipantObject {
            id_type_code: id_node_id(),
            id: subject.to_string(),
            type_code: "2".to_string(),
            role: "13".to_string(),
            ..Default::default()
        }];
    }
    m
}
