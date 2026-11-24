#![forbid(unsafe_code)]
use super::atna::{ID_SOP_CLASS_UID, ID_STUDY_INSTANCE_UID};
use super::clinical::{clinical_text, clinical_uid, validate_query_data};
use super::timetext;
use super::{
    ClinicalPolicy, Code, Error, Message, OUTCOME_MAJOR_FAILURE, OUTCOME_MINOR_FAILURE,
    OUTCOME_SERIOUS, OUTCOME_SUCCESS, Result,
};
use crate::dicomjson::sizedparse::base64_decode;
fn valid_code(c: &Code) -> bool {
    clinical_text(&c.value) && clinical_text(&c.code_system_name) && clinical_text(&c.original_text)
}
fn role_count(m: &Message, code: &str) -> usize {
    m.participants
        .iter()
        .flat_map(|a| &a.role_id_codes)
        .filter(|r| r.value == code && r.code_system_name == "DCM")
        .count()
}
fn encoded_len(n: i64) -> i64 {
    (n + 2) / 3 * 4
}
fn decode(value: &str) -> Option<Vec<u8>> {
    base64_decode(value.as_bytes(), false).ok()
}
fn clinical_enum(v: &str, max: i64) -> bool {
    if v.is_empty() {
        return true;
    }
    if !v.bytes().all(|c| c.is_ascii_digit()) || v.len() > 9 {
        return false;
    }
    match v.parse::<i64>() {
        Ok(n) => n >= 1 && n <= max && n.to_string() == v,
        Err(_) => false,
    }
}
const ROLE_SOURCE: &str = "110153";
const ROLE_DESTINATION: &str = "110152";
const ROLE_SOURCE_MEDIA: &str = "110155";
const ROLE_DESTINATION_MEDIA: &str = "110154";
impl Message {
    pub fn validate_clinical(&self, policy: &ClinicalPolicy) -> Result<()> {
        let m = self;
        let p = policy.normalize()?;
        let l = &p.limits;
        let err = |code: &str, field: &str| Err(Error::clinical(code, field));
        if m.event.type_codes.len() > 8 || m.sources.len() > 1 {
            return err("limit", "Codes");
        }
        if !valid_code(&m.event.id) || m.event.id.code_system_name != "DCM" {
            return err("code", "Event.ID");
        }
        let dt = &m.event.date_time;
        if timetext::parse_rfc3339(dt).is_none() {
            return err("time", "Event.DateTime");
        }
        let tail = &dt[10..];
        if !dt.ends_with('Z') && !tail.contains('+') && !tail.contains('-') {
            return err("time", "Event.DateTime");
        }
        if ![
            OUTCOME_SUCCESS,
            OUTCOME_MINOR_FAILURE,
            OUTCOME_SERIOUS,
            OUTCOME_MAJOR_FAILURE,
        ]
        .contains(&m.event.outcome_indicator.as_str())
        {
            return err("outcome", "Event.Outcome");
        }
        if m.sources.len() != 1 || !clinical_text(&m.sources[0].id) {
            return err("cardinality", "Sources");
        }
        let source = &m.sources[0];
        if m.participants.is_empty() || m.participants.len() > 8 {
            return err("cardinality", "Participants");
        }
        if m.objects.len() as i64 > l.max_objects {
            return err("limit", "Objects");
        }
        if source.type_codes.len() > 8 {
            return err("limit", "Sources.TypeCodes");
        }
        if !m.event.type_codes.iter().all(valid_code) {
            return err("code", "Event.Types");
        }
        for c in &source.type_codes {
            if c.value.is_empty() || (!clinical_enum(&c.value, 9) && !valid_code(c)) {
                return err("code", "Sources.TypeCodes");
            }
        }
        for c in std::iter::once(&m.event.id)
            .chain(&m.event.type_codes)
            .chain(&source.type_codes)
        {
            if c.text_len() as i64 > l.max_identity_bytes {
                return err("limit", "Codes");
            }
        }
        if !m.event.outcome_description.is_empty() && !clinical_text(&m.event.outcome_description) {
            return err("text", "Event.OutcomeDescription");
        }
        if !source.enterprise_site_id.is_empty() && !clinical_text(&source.enterprise_site_id) {
            return err("text", "Sources.Site");
        }
        let mut identity_bytes =
            m.event.outcome_description.len() + source.id.len() + source.enterprise_site_id.len();
        let mut requestors = 0;
        for (i, a) in m.participants.iter().enumerate() {
            let field = format!("Participants[{i}]");
            if a.role_id_codes.len() > 8 {
                return err("limit", &format!("{field}.Roles"));
            }
            if !clinical_text(&a.user_id) {
                return err("required", &format!("{field}.UserID"));
            }
            if a.user_is_requestor {
                requestors += 1;
            }
            let point = &a.network_access_point_type_code;
            if a.network_access_point_id.is_empty() != point.is_empty() {
                return err("network", &field);
            }
            if !point.is_empty() && !matches!(point.as_str(), "1" | "2" | "3" | "4" | "5") {
                return err("network", &field);
            }
            for v in &a.role_id_codes {
                if !valid_code(v) || v.text_len() as i64 > l.max_identity_bytes {
                    return err("code", &format!("{field}.Roles"));
                }
            }
            if let Some(media) = &a.media_identifier
                && (!valid_code(&media.media_type)
                    || media.media_type.text_len() as i64 > l.max_identity_bytes)
            {
                return err("code", &format!("{field}.MediaType"));
            }
            for v in [
                &a.user_id,
                &a.alternative_user_id,
                &a.user_name,
                &a.network_access_point_id,
            ] {
                if !v.is_empty() && !clinical_text(v) {
                    return err("text", &field);
                }
                identity_bytes += v.len();
            }
        }
        if requestors > 1 {
            return err("requestor", "Participants");
        }
        let (mut patients, mut studies, mut queries) = (0usize, 0usize, 0usize);
        for (i, o) in m.objects.iter().enumerate() {
            let field = format!("Objects[{i}]");
            let sub = |name: &str| format!("{field}.{name}");
            if !clinical_text(&o.id)
                || !valid_code(&o.id_type_code)
                || o.id_type_code.text_len() as i64 > l.max_identity_bytes
            {
                return err("required", &sub("ID"));
            }
            if o.name.is_empty() == o.query.is_empty() {
                return err("name_or_query", &field);
            }
            if !o.name.is_empty() && !clinical_text(&o.name) {
                return err("text", &sub("Name"));
            }
            identity_bytes += o.id.len() + o.name.len();
            if !o.query.is_empty() {
                if !p.include_query {
                    return err("opt_in", &sub("Query"));
                }
                if o.query.len() as i64 > encoded_len(l.max_query_bytes) {
                    return err("limit", &sub("Query"));
                }
                let raw = match decode(&o.query) {
                    Some(raw) if !raw.is_empty() => raw,
                    _ => return err("query", &sub("Query")),
                };
                if raw.len() as i64 > l.max_query_bytes {
                    return err("limit", &sub("Query"));
                }
            }
            if !clinical_enum(&o.type_code, 4)
                || !clinical_enum(&o.role, 26)
                || !clinical_enum(&o.life_cycle, 15)
            {
                return err("enum", &field);
            }
            let d = &o.description;
            if o.details.len() > 64
                || d.accessions.len() as i64 > l.max_objects
                || d.contains_study.studies.len() as i64 > l.max_groups
            {
                return err("limit", &sub("Description"));
            }
            for v in &d.contains_study.studies {
                if !clinical_uid(&v.uid) {
                    return err("uid", &sub("ContainsStudy"));
                }
                identity_bytes += v.uid.len();
            }
            for detail in &o.details {
                if detail.value.len() as i64 > encoded_len(l.max_identity_bytes) {
                    return err("limit", &sub("Details"));
                }
                let raw = decode(&detail.value);
                match raw {
                    Some(raw) if clinical_text(&detail.detail_type) && !raw.is_empty() => {
                        identity_bytes += detail.detail_type.len() + raw.len();
                    }
                    _ => return err("detail", &sub("Details")),
                }
            }
            for a in &d.accessions {
                if !clinical_text(&a.number) {
                    return err("required", &sub("Accession"));
                }
                identity_bytes += a.number.len();
            }
            if d.sop_classes.len() as i64 > l.max_classes {
                return err("limit", &sub("SOPClasses"));
            }
            for c in &d.sop_classes {
                if !clinical_uid(&c.uid)
                    || c.number_of_instances <= 0
                    || c.number_of_instances > l.max_instances
                    || c.instances.len() as i64 > c.number_of_instances
                {
                    return err("instances", &sub("SOPClasses"));
                }
                identity_bytes += c.uid.len();
                for v in &c.instances {
                    if !clinical_uid(&v.uid) {
                        return err("uid", &sub("Instances"));
                    }
                    identity_bytes += v.uid.len();
                }
            }
            if o.role == "1" {
                patients += 1;
                if !p.include_identities {
                    return err("opt_in", &field);
                }
                if o.type_code != "1"
                    || o.id_type_code.value != "2"
                    || o.id_type_code.code_system_name != "RFC-3881"
                    || o.name.is_empty()
                {
                    return err("patient", &field);
                }
            }
            if o.id_type_code.value == ID_STUDY_INSTANCE_UID
                && o.id_type_code.code_system_name == "DCM"
            {
                studies += 1;
                if !p.include_identities {
                    return err("opt_in", &field);
                }
                if !clinical_uid(&o.id)
                    || o.type_code != "2"
                    || o.role != "3"
                    || !d.contains_study.studies.is_empty()
                {
                    return err("study", &field);
                }
                if (!d.accessions.is_empty() || d.encrypted.is_some() || d.anonymized.is_some())
                    && d.sop_classes.is_empty()
                {
                    return err("sop_classes", &field);
                }
            }
            if !o.query.is_empty() && o.id_type_code.value == ID_SOP_CLASS_UID {
                queries += 1;
                if o.type_code != "2" || o.role != "3" || o.id_type_code.code_system_name != "DCM" {
                    return err("query", &field);
                }
                let mut syntax: Vec<u8> = Vec::new();
                for detail in &o.details {
                    if detail.detail_type == "TransferSyntax" {
                        let raw = decode(&detail.value).unwrap_or_default();
                        if !syntax.is_empty() {
                            return err("query", &field);
                        }
                        syntax = raw;
                    }
                }
                let raw = decode(&o.query).unwrap_or_default();
                if !clinical_uid(&o.id) {
                    return err("uid", &sub("ID"));
                }
                let syntax = String::from_utf8(syntax).unwrap_or_else(|_| "\u{fffd}".to_string());
                validate_query_data(&p, &raw, &syntax)?;
            }
        }
        if identity_bytes as i64 > l.max_identity_bytes {
            return err("limit", "Identity");
        }
        let id = m.event.id.value.as_str();
        let action = m.event.action_code.as_str();
        let action_in = |allowed: &str| action.len() == 1 && allowed.contains(action);
        let record = || -> Result<()> {
            if patients != 1 || m.participants.len() > 2 || m.objects.len() != patients + studies {
                return err("cardinality", "Record");
            }
            Ok(())
        };
        match id {
            "110102" | "110104" => {
                if patients != 1
                    || studies < 1
                    || m.objects.len() != patients + studies
                    || role_count(m, ROLE_SOURCE) != 1
                    || role_count(m, ROLE_DESTINATION) != 1
                    || m.participants.len() > 3
                {
                    return err("cardinality", "Transfer");
                }
                if (id == "110102" && !action_in("E")) || (id == "110104" && !action_in("CRU")) {
                    return err("action", "Event.Action");
                }
            }
            "110103" | "110105" => {
                record()?;
                if studies < 1 {
                    return err("cardinality", "Studies");
                }
                if (id == "110103" && !action_in("CRUD")) || (id == "110105" && !action_in("D")) {
                    return err("action", "Event.Action");
                }
            }
            "110109" | "110110" | "110111" => {
                record()?;
                if !action_in("CRUD") {
                    return err("action", "Event.Action");
                }
                if id != "110111" && studies != 0 {
                    return err("cardinality", "Studies");
                }
            }
            "110107" | "110106" => {
                let (role, party, allowed) = if id == "110106" {
                    (ROLE_DESTINATION_MEDIA, ROLE_SOURCE, "R")
                } else {
                    (ROLE_SOURCE_MEDIA, ROLE_DESTINATION, "C")
                };
                if patients < 1
                    || role_count(m, role) != 1
                    || role_count(m, party) < 1
                    || requestors != 1
                {
                    return err("cardinality", "Media");
                }
                if !action_in(allowed) {
                    return err("action", "Event.Action");
                }
                for a in &m.participants {
                    for r in &a.role_id_codes {
                        if r.value == role && (a.media_identifier.is_none() || a.user_is_requestor)
                        {
                            return err("media", "Participants");
                        }
                    }
                }
            }
            "110112" => {
                if !p.include_query {
                    return err("opt_in", "Query");
                }
                if queries != 1
                    || m.objects.len() != 1
                    || m.participants.len() != 2
                    || role_count(m, ROLE_SOURCE) != 1
                    || role_count(m, ROLE_DESTINATION) != 1
                    || requestors != 1
                {
                    return err("cardinality", "Query");
                }
                if !action_in("E") {
                    return err("action", "Event.Action");
                }
            }
            "110101" => {
                if m.objects.len() != 1 || m.participants.len() > 2 || !action_in("R") {
                    return err("cardinality", "AuditLog");
                }
                let o = &m.objects[0];
                let scheme = crate::net::dicomweb::urlrules::parse(&o.id)
                    .map(|u| u.scheme)
                    .unwrap_or_default();
                if scheme.is_empty()
                    || o.name != "Security Audit Log"
                    || o.id_type_code.value != "12"
                    || o.type_code != "2"
                    || o.role != "13"
                {
                    return err("audit_log", "Objects[0]");
                }
            }
            _ => return err("profile", "Event.ID"),
        }
        Ok(())
    }
}
