use super::ack::{
    ERROR_CODE_DATA_TYPE, ERROR_CODE_REQUIRED_FIELD_MISSING, ERROR_CODE_SEGMENT_SEQUENCE,
    ERROR_CODE_UNSUPPORTED_EVENT_CODE, ERROR_CODE_UNSUPPORTED_MESSAGE_TYPE,
};
use super::error::{ApplicationError, Error, ErrorLocation};
use super::{
    Component, ComponentExt, FieldExt, HierarchicDesignator, Message, MessageHeader, NULL,
    Repetition, RepetitionExt, Segment,
};
use crate::dcmtime;
use crate::object::{
    DataSet, Element, ElementHeader, IdWithIssuer, Issuer, Object, Tag, UNDEFINED_LENGTH, Value, Vr,
};
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PatientIdentifier {
    pub id_with_issuer: IdWithIssuer,
    pub identifier_type_code: String,
}
impl PatientIdentifier {
    pub(super) fn from_repetition(r: &[Component]) -> PatientIdentifier {
        let authority = HierarchicDesignator::from_component(r.component(4));
        PatientIdentifier {
            id_with_issuer: IdWithIssuer {
                id: r.string(1).trim().to_owned(),
                type_of_patient_id: String::new(),
                issuer: Issuer {
                    local_namespace_entity_id: authority.namespace_id.trim().to_owned(),
                    universal_entity_id: authority.universal_id.trim().to_owned(),
                    universal_entity_id_type: authority.universal_id_type.trim().to_owned(),
                },
            },
            identifier_type_code: r.string(5).trim().to_owned(),
        }
    }
    pub(super) fn repetition(&self) -> Repetition {
        let i = &self.id_with_issuer.issuer;
        vec![
            vec![self.id_with_issuer.id.clone()],
            Vec::new(),
            Vec::new(),
            vec![
                i.local_namespace_entity_id.clone(),
                i.universal_entity_id.clone(),
                i.universal_entity_id_type.clone(),
            ],
            vec![self.identifier_type_code.clone()],
        ]
    }
    pub fn id(&self) -> &str {
        &self.id_with_issuer.id
    }
}
fn identifiers_from(field: &[Repetition]) -> Vec<PatientIdentifier> {
    field
        .iter()
        .map(|r| PatientIdentifier::from_repetition(r))
        .filter(|id| !id.id().is_empty() && id.id() != NULL)
        .collect()
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Patient {
    pub identifiers: Vec<PatientIdentifier>,
    pub name: String,
    pub name_has_reserved_characters: bool,
    pub birth_date: String,
    pub sex: String,
    pub name_is_null: bool,
    pub birth_date_is_null: bool,
    pub sex_is_null: bool,
}
pub(super) fn patient_from(pid: &Segment) -> Patient {
    let (pn, reserved) = dicom_person_name(pid.field(5).first_rep(), false);
    let mut birth = pid.field(7).string();
    if birth.len() > 8 {
        birth = truncate_bytes(birth, 8);
    }
    Patient {
        identifiers: identifiers_from(pid.field(3)),
        name: pn.join("^"),
        name_has_reserved_characters: reserved,
        birth_date: birth.to_owned(),
        sex: pid.field(8).string().to_owned(),
        name_is_null: pid.field(5).is_null(),
        birth_date_is_null: pid.field(7).is_null(),
        sex_is_null: pid.field(8).is_null(),
    }
}
fn truncate_bytes(s: &str, n: usize) -> &str {
    let mut end = n.min(s.len());
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}
impl Patient {
    pub fn dicom_patient_ids(&self) -> Vec<IdWithIssuer> {
        self.identifiers
            .iter()
            .map(|i| i.id_with_issuer.clone())
            .collect()
    }
    pub fn dicom_sex(&self) -> &'static str {
        match crate::textfmt::to_upper(self.sex.trim()).as_str() {
            "M" => "M",
            "F" => "F",
            "O" | "A" | "N" | "X" => "O",
            _ => "",
        }
    }
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PatientMerge {
    pub patient: Patient,
    pub prior: Option<Vec<PatientIdentifier>>,
}
pub const EVENT_ADMIT_PATIENT: &str = "A01";
pub const EVENT_REGISTER_PATIENT: &str = "A04";
pub const EVENT_ADD_PERSON: &str = "A28";
pub const EVENT_UPDATE_PERSON: &str = "A31";
pub const EVENT_CHANGE_PATIENT_IDENTIFIER: &str = "A47";
pub const EVENT_UPDATE_PATIENT: &str = "A08";
pub const EVENT_MERGE_PATIENT: &str = "A40";
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AdtMessage {
    pub header: MessageHeader,
    pub event: String,
    pub recorded_date_time: String,
    pub patient: Patient,
    pub merges: Vec<PatientMerge>,
    pub visit: Option<Visit>,
    pub change: Option<PatientIdentifierChange>,
}
fn app(code: &str, reject: bool, location: ErrorLocation, reason: &str) -> Error {
    ApplicationError {
        code: code.to_owned(),
        location,
        reject,
        err: Error::text(reason),
    }
    .into()
}
fn loc(segment: &str, sequence: i64, field: i64, component: i64) -> ErrorLocation {
    ErrorLocation {
        segment: segment.to_owned(),
        sequence,
        field,
        component,
    }
}
pub(super) fn adt_error(segment: &str, field: i64, code: &str, reason: &str) -> Error {
    app(code, false, loc(segment, 1, field, 0), reason)
}
pub fn parse_adt(m: &Message) -> Result<AdtMessage, Error> {
    let header = m.header();
    if header.message_type.code != "ADT" {
        return Err(app(
            ERROR_CODE_UNSUPPORTED_MESSAGE_TYPE,
            true,
            loc("MSH", 1, 9, 0),
            "not an ADT message",
        ));
    }
    let mut event = header.message_type.trigger.clone();
    if let Some(evn) = m.segment("EVN")
        && event.is_empty()
    {
        event = evn.field(1).string().to_owned();
    }
    let mut adt = AdtMessage {
        header,
        event: event.clone(),
        ..AdtMessage::default()
    };
    if let Some(evn) = m.segment("EVN") {
        adt.recorded_date_time = evn.field(2).string().to_owned();
    }
    match event.as_str() {
        EVENT_ADMIT_PATIENT
        | EVENT_REGISTER_PATIENT
        | EVENT_UPDATE_PATIENT
        | EVENT_ADD_PERSON
        | EVENT_UPDATE_PERSON => {
            let Some(pid) = m.segment("PID") else {
                return Err(app(
                    ERROR_CODE_SEGMENT_SEQUENCE,
                    false,
                    loc("PID", 1, 0, 0),
                    "missing PID segment",
                ));
            };
            if event != EVENT_UPDATE_PATIENT {
                let (mut pids, mut pv1s) = (0i64, 0i64);
                for segment in &m.segments {
                    let count = match segment.name.as_str() {
                        "PID" => {
                            pids += 1;
                            pids
                        }
                        "PV1" => {
                            pv1s += 1;
                            if pids == 0 {
                                return Err(adt_error(
                                    "PV1",
                                    0,
                                    ERROR_CODE_SEGMENT_SEQUENCE,
                                    "PV1 before its PID",
                                ));
                            }
                            pv1s
                        }
                        _ => continue,
                    };
                    if count > 1 {
                        return Err(app(
                            ERROR_CODE_SEGMENT_SEQUENCE,
                            false,
                            loc(&segment.name, count, 0, 0),
                            "duplicate patient or visit segment",
                        ));
                    }
                }
            }
            adt.patient = patient_from(pid);
            if adt.patient.identifiers.is_empty() {
                return Err(app(
                    ERROR_CODE_REQUIRED_FIELD_MISSING,
                    false,
                    loc("PID", 1, 3, 0),
                    "missing patient identifier",
                ));
            }
            if event == EVENT_ADMIT_PATIENT
                || event == EVENT_REGISTER_PATIENT
                || event == EVENT_UPDATE_PATIENT
            {
                let pv1 = m.segment("PV1");
                if pv1.is_none() && event != EVENT_UPDATE_PATIENT {
                    return Err(adt_error(
                        "PV1",
                        0,
                        ERROR_CODE_SEGMENT_SEQUENCE,
                        "missing PV1 segment",
                    ));
                }
                if let Some(pv1) = pv1 {
                    let class = pv1.field(2);
                    if (class.string().trim().is_empty() || class.is_null())
                        && event != EVENT_UPDATE_PATIENT
                    {
                        return Err(adt_error(
                            "PV1",
                            2,
                            ERROR_CODE_REQUIRED_FIELD_MISSING,
                            "missing patient class",
                        ));
                    }
                    if !class.is_empty_value() && !class.is_null() && class.string().trim() != "N" {
                        adt.visit = Some(visit_from(pv1));
                    }
                }
            }
        }
        EVENT_CHANGE_PATIENT_IDENTIFIER => {
            let (mut pid, mut mrg): (Option<&Segment>, Option<&Segment>) = (None, None);
            let (mut pid_count, mut mrg_count) = (0i64, 0i64);
            for segment in &m.segments {
                match segment.name.as_str() {
                    "PID" => {
                        pid_count += 1;
                        if pid.is_some() || mrg.is_some() {
                            return Err(app(
                                ERROR_CODE_SEGMENT_SEQUENCE,
                                false,
                                loc("PID", pid_count, 0, 0),
                                "duplicate or misplaced PID segment",
                            ));
                        }
                        pid = Some(segment);
                    }
                    "MRG" => {
                        mrg_count += 1;
                        if pid.is_none() || mrg.is_some() {
                            return Err(app(
                                ERROR_CODE_SEGMENT_SEQUENCE,
                                false,
                                loc("MRG", mrg_count, 0, 0),
                                "MRG without its PID",
                            ));
                        }
                        mrg = Some(segment);
                    }
                    _ => {}
                }
            }
            let Some(pid) = pid else {
                return Err(adt_error(
                    "PID",
                    0,
                    ERROR_CODE_SEGMENT_SEQUENCE,
                    "missing PID segment",
                ));
            };
            let Some(mrg) = mrg else {
                return Err(adt_error(
                    "MRG",
                    0,
                    ERROR_CODE_SEGMENT_SEQUENCE,
                    "missing MRG segment",
                ));
            };
            let change = PatientIdentifierChange {
                current: identifiers_from(pid.field(3)),
                prior: identifiers_from(mrg.field(1)),
            };
            if change.current.is_empty() {
                return Err(adt_error(
                    "PID",
                    3,
                    ERROR_CODE_REQUIRED_FIELD_MISSING,
                    "missing patient identifier",
                ));
            }
            if change.prior.is_empty() {
                return Err(adt_error(
                    "MRG",
                    1,
                    ERROR_CODE_REQUIRED_FIELD_MISSING,
                    "missing prior patient identifier",
                ));
            }
            adt.change = Some(change);
        }
        EVENT_MERGE_PATIENT => {
            let (mut pid_count, mut mrg_count) = (0i64, 0i64);
            for segment in &m.segments {
                match segment.name.as_str() {
                    "PID" => {
                        pid_count += 1;
                        let patient = patient_from(segment);
                        if patient.identifiers.is_empty() {
                            return Err(app(
                                ERROR_CODE_REQUIRED_FIELD_MISSING,
                                false,
                                loc("PID", pid_count, 3, 0),
                                "missing patient identifier",
                            ));
                        }
                        adt.merges.push(PatientMerge {
                            patient,
                            prior: None,
                        });
                    }
                    "MRG" => {
                        mrg_count += 1;
                        let current = adt.merges.last_mut().filter(|c| c.prior.is_none());
                        let Some(current) = current else {
                            return Err(app(
                                ERROR_CODE_SEGMENT_SEQUENCE,
                                false,
                                loc("MRG", mrg_count, 0, 0),
                                "MRG without its PID",
                            ));
                        };
                        let prior = identifiers_from(segment.field(1));
                        if prior.is_empty() {
                            return Err(app(
                                ERROR_CODE_REQUIRED_FIELD_MISSING,
                                false,
                                loc("MRG", mrg_count, 1, 0),
                                "missing prior patient identifier",
                            ));
                        }
                        current.prior = Some(prior);
                    }
                    _ => {}
                }
            }
            if adt.merges.is_empty() {
                return Err(app(
                    ERROR_CODE_SEGMENT_SEQUENCE,
                    false,
                    loc("PID", 1, 0, 0),
                    "missing PID segment",
                ));
            }
            for (i, merge) in adt.merges.iter().enumerate() {
                if merge.prior.is_none() {
                    return Err(app(
                        ERROR_CODE_SEGMENT_SEQUENCE,
                        false,
                        loc("MRG", i as i64 + 1, 0, 0),
                        "missing MRG segment",
                    ));
                }
            }
        }
        _ => {
            return Err(app(
                ERROR_CODE_UNSUPPORTED_EVENT_CODE,
                true,
                loc("MSH", 1, 9, 2),
                "unsupported ADT event",
            ));
        }
    }
    Ok(adt)
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PatientIdentifierChange {
    pub current: Vec<PatientIdentifier>,
    pub prior: Vec<PatientIdentifier>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PatientLocation {
    pub point_of_care: String,
    pub room: String,
    pub bed: String,
    pub facility: HierarchicDesignator,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Visit {
    pub patient_class: String,
    pub location: PatientLocation,
    pub admission_type: String,
    pub hospital_service: String,
    pub number: PatientIdentifier,
    pub referring_physician: String,
    pub referring_name_has_reserved_characters: bool,
    pub admitted_date_time: String,
    pub discharged_date_time: String,
    pub location_is_null: bool,
    pub number_is_null: bool,
    pub referring_physician_is_null: bool,
    pub admission_type_is_null: bool,
    pub admitted_date_time_is_null: bool,
    pub discharged_date_time_is_null: bool,
}
fn visit_from(pv1: &Segment) -> Visit {
    let location = pv1.field(3).first_rep();
    let (name, reserved) = dicom_person_name(pv1.field(8).first_rep(), true);
    Visit {
        patient_class: pv1.field(2).string().to_owned(),
        location: PatientLocation {
            point_of_care: location.string(1).to_owned(),
            room: location.string(2).to_owned(),
            bed: location.string(3).to_owned(),
            facility: HierarchicDesignator::from_component(location.component(4)),
        },
        admission_type: pv1.field(4).string().to_owned(),
        hospital_service: pv1.field(10).string().to_owned(),
        number: PatientIdentifier::from_repetition(pv1.field(19).first_rep()),
        referring_physician: name.join("^"),
        referring_name_has_reserved_characters: reserved,
        admitted_date_time: pv1.field(44).string().to_owned(),
        discharged_date_time: pv1.field(45).string().to_owned(),
        location_is_null: pv1.field(3).is_null(),
        number_is_null: pv1.field(19).is_null(),
        referring_physician_is_null: pv1.field(8).is_null(),
        admission_type_is_null: pv1.field(4).is_null(),
        admitted_date_time_is_null: pv1.field(44).is_null(),
        discharged_date_time_is_null: pv1.field(45).is_null(),
    }
}
fn dicom_person_name(name: &[Component], xcn: bool) -> (Vec<String>, bool) {
    let mut parts: Vec<String> = if xcn {
        vec![
            name.component(2).sub(1),
            name.string(3),
            name.string(4),
            name.string(6),
            name.string(5),
        ]
    } else {
        vec![
            name.component(1).sub(1),
            name.string(2),
            name.string(3),
            name.string(5),
            name.string(4),
        ]
    }
    .into_iter()
    .map(str::to_owned)
    .collect();
    let reserved = parts.iter().any(|p| p.contains(['^', '=', '\\']));
    while parts.last().is_some_and(String::is_empty) {
        parts.pop();
    }
    (parts, reserved)
}
const ADT_CHARACTER_SET: Tag = Tag::new(0x0008, 0x0005);
const ADT_PATIENT_ID: Tag = Tag::new(0x0010, 0x0020);
const ADT_PATIENT_ISSUER: Tag = Tag::new(0x0010, 0x0021);
const ADT_PATIENT_QUALIFIERS: Tag = Tag::new(0x0010, 0x0024);
const ADT_OTHER_PATIENT_IDS: Tag = Tag::new(0x0010, 0x1002);
fn string_element(tag: Tag, vr: Vr, values: Vec<String>) -> Element {
    Element {
        header: ElementHeader {
            tag,
            vr,
            length: 0,
            length_set: false,
        },
        value: Value::Strings(values),
    }
}
fn sequence_element(tag: Tag, items: Vec<Object>) -> Element {
    let datasets = items
        .into_iter()
        .map(|item| {
            let mut ds: DataSet = item.to_data_set();
            ds.elements.sort_by_key(Element::tag);
            ds
        })
        .collect();
    Element {
        header: ElementHeader {
            tag,
            vr: Vr::SQ,
            length: UNDEFINED_LENGTH,
            length_set: true,
        },
        value: Value::Sequence(datasets),
    }
}
fn adt_utf8_object() -> Object {
    let mut out = Object::new();
    out.put(string_element(
        ADT_CHARACTER_SET,
        Vr::CS,
        vec!["ISO_IR 192".to_owned()],
    ));
    out
}
fn valid_string_value(vr: Vr, value: &str) -> bool {
    let max = match vr {
        Vr::CS | Vr::SH => 16,
        Vr::DA => 8,
        Vr::LO | Vr::PN => 64,
        Vr::TM => 16,
        _ => 0,
    };
    let length = if vr == Vr::PN {
        value
            .split('=')
            .map(|g| g.chars().count())
            .max()
            .unwrap_or(0)
    } else {
        value.chars().count()
    };
    if max > 0 && length > max {
        return false;
    }
    let uses_charset = matches!(
        vr,
        Vr::LO | Vr::LT | Vr::PN | Vr::SH | Vr::ST | Vr::UC | Vr::UT
    );
    let line_controls = matches!(vr, Vr::LT | Vr::ST | Vr::UT);
    for r in value.chars() {
        if line_controls && matches!(r, '\t' | '\n' | '\x0c' | '\r') {
            continue;
        }
        if uses_charset {
            if r < ' ' || r == '\x7f' {
                return false;
            }
        } else if !(' '..='~').contains(&r) || r == '\\' {
            return false;
        }
    }
    if value.is_empty() {
        return true;
    }
    match vr {
        Vr::CS => value
            .bytes()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == b'_' || c == b' '),
        Vr::DA => !value.starts_with(' ') && dcmtime::parse_da(value).is_ok(),
        Vr::TM => !value.starts_with(' ') && dcmtime::parse_tm(value).is_ok(),
        Vr::PN => {
            let groups: Vec<&str> = value.split('=').collect();
            groups.len() <= 3 && groups.iter().all(|g| g.split('^').count() <= 5)
        }
        _ => true,
    }
}
fn adt_put_text(
    out: &mut Object,
    tag: Tag,
    vr: Vr,
    value: &str,
    clear: bool,
    segment: &str,
    field: i64,
) -> Result<(), Error> {
    let value = if clear {
        ""
    } else if value.is_empty() {
        return Ok(());
    } else {
        value
    };
    if value.contains('\\') {
        return Err(adt_error(
            segment,
            field,
            ERROR_CODE_DATA_TYPE,
            "value contains a DICOM value separator",
        ));
    }
    if !valid_string_value(vr, value) {
        return Err(adt_error(
            segment,
            field,
            ERROR_CODE_DATA_TYPE,
            "value cannot be represented by the DICOM attribute VR",
        ));
    }
    out.put(string_element(tag, vr, vec![value.to_owned()]));
    Ok(())
}
fn identifier_attributes(
    id: &PatientIdentifier,
    segment: &str,
    field: i64,
) -> Result<Object, Error> {
    if id.id().is_empty() || id.id() == NULL {
        return Err(adt_error(
            segment,
            field,
            ERROR_CODE_REQUIRED_FIELD_MISSING,
            "missing identifier",
        ));
    }
    let mut out = adt_utf8_object();
    adt_put_text(
        &mut out,
        ADT_PATIENT_ID,
        Vr::LO,
        id.id(),
        false,
        segment,
        field,
    )?;
    let issuer = &id.id_with_issuer.issuer;
    if issuer.universal_entity_id.is_empty() != issuer.universal_entity_id_type.is_empty() {
        return Err(adt_error(
            segment,
            field,
            ERROR_CODE_DATA_TYPE,
            "universal assigning authority requires both ID and type",
        ));
    }
    adt_put_text(
        &mut out,
        ADT_PATIENT_ISSUER,
        Vr::LO,
        &issuer.local_namespace_entity_id,
        false,
        segment,
        field,
    )?;
    if !issuer.universal_entity_id.is_empty()
        || !issuer.universal_entity_id_type.is_empty()
        || !id.identifier_type_code.is_empty()
    {
        let mut item = adt_utf8_object();
        for (tag, vr, value) in [
            (
                Tag::new(0x0040, 0x0032),
                Vr::UT,
                &issuer.universal_entity_id,
            ),
            (
                Tag::new(0x0040, 0x0033),
                Vr::CS,
                &issuer.universal_entity_id_type,
            ),
            (Tag::new(0x0040, 0x0035), Vr::CS, &id.identifier_type_code),
        ] {
            adt_put_text(&mut item, tag, vr, value, false, segment, field)?;
        }
        item.remove(ADT_CHARACTER_SET);
        out.put(sequence_element(ADT_PATIENT_QUALIFIERS, vec![item]));
    }
    Ok(out)
}
impl Patient {
    pub fn dicom_attributes(&self, primary_index: i64) -> Result<Object, Error> {
        if primary_index < 0 || primary_index as usize >= self.identifiers.len() {
            return Err(adt_error(
                "PID",
                3,
                ERROR_CODE_DATA_TYPE,
                "patient identifier index out of range",
            ));
        }
        let primary = primary_index as usize;
        let mut out = identifier_attributes(&self.identifiers[primary], "PID", 3)?;
        let mut others = Vec::new();
        for (i, id) in self.identifiers.iter().enumerate() {
            if i == primary {
                continue;
            }
            let mut item = identifier_attributes(id, "PID", 3)?;
            item.remove(ADT_CHARACTER_SET);
            others.push(item);
        }
        if !others.is_empty() {
            out.put(sequence_element(ADT_OTHER_PATIENT_IDS, others));
        }
        if self.name_has_reserved_characters && !self.name_is_null {
            return Err(adt_error(
                "PID",
                5,
                ERROR_CODE_DATA_TYPE,
                "name contains an unrepresentable DICOM PN separator",
            ));
        }
        adt_put_text(
            &mut out,
            Tag::new(0x0010, 0x0010),
            Vr::PN,
            &self.name,
            self.name_is_null,
            "PID",
            5,
        )?;
        if !self.birth_date.is_empty() && !self.birth_date_is_null {
            if self.birth_date.len() != 8 {
                return Err(adt_error(
                    "PID",
                    7,
                    ERROR_CODE_DATA_TYPE,
                    "birth date cannot be represented as a full DICOM DA",
                ));
            }
            if dcmtime::parse_date(&self.birth_date).is_err() {
                return Err(adt_error(
                    "PID",
                    7,
                    ERROR_CODE_DATA_TYPE,
                    "invalid birth date",
                ));
            }
        }
        adt_put_text(
            &mut out,
            Tag::new(0x0010, 0x0030),
            Vr::DA,
            &self.birth_date,
            self.birth_date_is_null,
            "PID",
            7,
        )?;
        if !self.sex.is_empty() || self.sex_is_null {
            let value = self.dicom_sex();
            adt_put_text(
                &mut out,
                Tag::new(0x0010, 0x0040),
                Vr::CS,
                value,
                self.sex_is_null || value.is_empty(),
                "PID",
                8,
            )?;
        }
        Ok(out)
    }
}
impl PatientIdentifierChange {
    pub fn dicom_current_attributes(&self, primary_index: i64) -> Result<Object, Error> {
        Patient {
            identifiers: self.current.clone(),
            ..Patient::default()
        }
        .dicom_attributes(primary_index)
    }
}
impl Visit {
    pub fn dicom_attributes(&self) -> Result<Object, Error> {
        let mut out = adt_utf8_object();
        if self.number_is_null {
            adt_put_text(
                &mut out,
                Tag::new(0x0038, 0x0010),
                Vr::LO,
                "",
                true,
                "PV1",
                19,
            )?;
            out.put(sequence_element(Tag::new(0x0038, 0x0014), Vec::new()));
        } else if !self.number.id().is_empty() {
            adt_put_text(
                &mut out,
                Tag::new(0x0038, 0x0010),
                Vr::LO,
                self.number.id(),
                false,
                "PV1",
                19,
            )?;
            let issuer = &self.number.id_with_issuer.issuer;
            if issuer.universal_entity_id.is_empty() != issuer.universal_entity_id_type.is_empty() {
                return Err(adt_error(
                    "PV1",
                    19,
                    ERROR_CODE_DATA_TYPE,
                    "universal assigning authority requires both ID and type",
                ));
            }
            if !issuer.is_zero() {
                let mut item = adt_utf8_object();
                for (tag, vr, value) in [
                    (
                        Tag::new(0x0040, 0x0031),
                        Vr::UT,
                        &issuer.local_namespace_entity_id,
                    ),
                    (
                        Tag::new(0x0040, 0x0032),
                        Vr::UT,
                        &issuer.universal_entity_id,
                    ),
                    (
                        Tag::new(0x0040, 0x0033),
                        Vr::CS,
                        &issuer.universal_entity_id_type,
                    ),
                ] {
                    adt_put_text(&mut item, tag, vr, value, false, "PV1", 19)?;
                }
                item.remove(ADT_CHARACTER_SET);
                out.put(sequence_element(Tag::new(0x0038, 0x0014), vec![item]));
            }
        }
        let mut parts = vec![
            self.location.point_of_care.as_str(),
            self.location.room.as_str(),
            self.location.bed.as_str(),
        ];
        while parts.last().is_some_and(|p| p.is_empty()) {
            parts.pop();
        }
        adt_put_text(
            &mut out,
            Tag::new(0x0038, 0x0300),
            Vr::LO,
            &parts.join("/"),
            self.location_is_null,
            "PV1",
            3,
        )?;
        adt_put_text(
            &mut out,
            Tag::new(0x0038, 0x0016),
            Vr::LO,
            &self.admission_type,
            self.admission_type_is_null,
            "PV1",
            4,
        )?;
        if self.referring_name_has_reserved_characters && !self.referring_physician_is_null {
            return Err(adt_error(
                "PV1",
                8,
                ERROR_CODE_DATA_TYPE,
                "name contains an unrepresentable DICOM PN separator",
            ));
        }
        adt_put_text(
            &mut out,
            Tag::new(0x0008, 0x0090),
            Vr::PN,
            &self.referring_physician,
            self.referring_physician_is_null,
            "PV1",
            8,
        )?;
        if self.admitted_date_time_is_null {
            for (tag, vr) in [
                (Tag::new(0x0038, 0x0020), Vr::DA),
                (Tag::new(0x0038, 0x0021), Vr::TM),
            ] {
                adt_put_text(&mut out, tag, vr, "", true, "PV1", 44)?;
            }
        } else if !self.admitted_date_time.is_empty() {
            let mut value = self.admitted_date_time.as_str();
            if dcmtime::parse_datetime(value).is_err() {
                return Err(adt_error(
                    "PV1",
                    44,
                    ERROR_CODE_DATA_TYPE,
                    "invalid admission timestamp",
                ));
            }
            let b = value.as_bytes();
            if b.len() >= 5 && (b[b.len() - 5] == b'+' || b[b.len() - 5] == b'-') {
                let offset = &value[value.len() - 5..];
                value = &value[..value.len() - 5];
                adt_put_text(
                    &mut out,
                    Tag::new(0x0008, 0x0201),
                    Vr::SH,
                    offset,
                    false,
                    "PV1",
                    44,
                )?;
            }
            if value.len() < 8 {
                return Err(adt_error(
                    "PV1",
                    44,
                    ERROR_CODE_DATA_TYPE,
                    "admission date cannot be represented as a full DICOM DA",
                ));
            }
            adt_put_text(
                &mut out,
                Tag::new(0x0038, 0x0020),
                Vr::DA,
                &value[..8],
                false,
                "PV1",
                44,
            )?;
            adt_put_text(
                &mut out,
                Tag::new(0x0038, 0x0021),
                Vr::TM,
                &value[8..],
                false,
                "PV1",
                44,
            )?;
        }
        Ok(out)
    }
}
