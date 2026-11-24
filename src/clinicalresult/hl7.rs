use super::{
    ClinicalResult, Error, Identifier, Order, OrderReference, Patient, REF_FILLER, REF_PLACER,
    STATUS_CORRECTED, STATUS_FINAL, STATUS_WITHDRAWN, identifier_valid, order_valid, parse_date,
    result_valid,
};
use crate::hl7::{
    ComponentExt, Field, FieldExt, HierarchicDesignator, NULL, ParseOptions, RepetitionExt,
    Segment, parse_with_options,
};
use sha2::{Digest, Sha256};
pub const HL7_PATIENT_IDENTIFIER: &str = "patient";
pub const HL7_PLACER_ORDER: &str = "placer";
pub const HL7_FILLER_ORDER: &str = "filler";
pub type Hl7AuthorityResolver<'a> = &'a dyn Fn(&HierarchicDesignator, &str, &str) -> Option<String>;
const MAX_HL7_WIRE_BYTES: i64 = 1 << 20;
fn identity_error(segment: &str, field: usize, reason: &str) -> Error {
    Error::invalid(format!("HL7 {segment}-{field}: {reason}"))
}
pub fn order_from_hl7(
    wire: &[u8],
    order_id: &Identifier,
    map_authority: Option<Hl7AuthorityResolver<'_>>,
) -> Result<Order, Error> {
    let Some(map_authority) = map_authority.filter(|_| identifier_valid(order_id)) else {
        return Err(Error::invalid("HL7 order identity or authority crosswalk"));
    };
    let segments = parse_group_message(wire)?;
    let group = identity_group(&segments, "ORM", "O01", "ORM_O01")?;
    if group
        .orc
        .is_none_or(|orc| !simple_field(orc.field(1), "NW"))
    {
        return Err(identity_error("ORC", 1, "expected a new order"));
    }
    let (patient, references) = group.identities(map_authority, false)?;
    let order = Order {
        id: order_id.clone(),
        patient,
        references,
    };
    if !order_valid(&order) {
        return Err(Error::invalid("HL7 order projection"));
    }
    Ok(order)
}
pub fn result_from_hl7(
    wire: &[u8],
    source: &str,
    result_id: &Identifier,
    sequence: u64,
    map_authority: Option<Hl7AuthorityResolver<'_>>,
) -> Result<ClinicalResult, Error> {
    let map_authority = match map_authority {
        Some(m) if identifier_valid(result_id) && !source.is_empty() && sequence != 0 => m,
        _ => {
            return Err(Error::invalid(
                "HL7 result identity, revision sequence or authority crosswalk",
            ));
        }
    };
    let segments = parse_group_message(wire)?;
    let group = identity_group(&segments, "ORU", "R01", "ORU_R01")?;
    if let Some(orc) = group.orc
        && !simple_field(orc.field(1), "RE")
    {
        return Err(identity_error("ORC", 1, "unsupported result order control"));
    }
    let field = group.obr.field(25);
    let status = if simple_field(field, "F") {
        STATUS_FINAL
    } else if simple_field(field, "C") {
        STATUS_CORRECTED
    } else if simple_field(field, "X") {
        STATUS_WITHDRAWN
    } else {
        return Err(identity_error("OBR", 25, "unsupported result status"));
    };
    let (patient, references) = group.identities(map_authority, true)?;
    let result = ClinicalResult {
        source: source.to_owned(),
        id: result_id.clone(),
        patient,
        order_references: references,
        sequence,
        revision_id: group.control_id.clone(),
        status: status.to_owned(),
        digest: Sha256::digest(wire).into(),
    };
    if !result_valid(&result) {
        return Err(Error::invalid("HL7 result projection"));
    }
    Ok(result)
}
struct IdentityGroup<'a> {
    pid: &'a Segment,
    orc: Option<&'a Segment>,
    obr: &'a Segment,
    control_id: String,
}
fn parse_group_message(wire: &[u8]) -> Result<Vec<Segment>, Error> {
    let options = ParseOptions {
        max_bytes: MAX_HL7_WIRE_BYTES,
        max_segments: 4096,
        max_fields: 256,
        max_repetitions: 128,
        default_character_set: String::new(),
    };
    parse_with_options(wire, &options)
        .map(|m| m.segments)
        .map_err(|_| Error::invalid("invalid or oversized HL7 message"))
}
fn identity_group<'a>(
    segments: &'a [Segment],
    code: &str,
    trigger: &str,
    structure: &str,
) -> Result<IdentityGroup<'a>, Error> {
    let msh = &segments[0];
    let kind = msh.field(9);
    if kind.len() != 1
        || !components_bounded(&kind[0], 3)
        || kind[0].string(1) != code
        || kind[0].string(2) != trigger
        || (!kind[0].string(3).is_empty() && kind[0].string(3) != structure)
    {
        return Err(identity_error(
            "MSH",
            9,
            "unsupported message type or structure",
        ));
    }
    let version = msh.field(12);
    if version.len() != 1
        || version[0].len() > 3
        || version[0].component(1).len() != 1
        || version[0].string(1) != "2.5.1"
    {
        return Err(identity_error("MSH", 12, "expected v2.5.1"));
    }
    let control = msh.field(10);
    if !scalar(control) || control.string().is_empty() || control.is_null() {
        return Err(identity_error("MSH", 10, "expected one control identifier"));
    }
    if !msh.field(14).is_empty_value() {
        return Err(identity_error("MSH", 14, "fragmented message"));
    }
    let (mut pid, mut orc, mut obr): (Option<&Segment>, Option<&Segment>, Option<&Segment>) =
        (None, None, None);
    for (i, segment) in segments.iter().enumerate().skip(1) {
        let ok = match segment.name.as_str() {
            "SFT" => pid.is_none(),
            "PID" if pid.is_none() => {
                pid = Some(segment);
                true
            }
            "PD1" | "NK1" | "PV1" | "PV2" => pid.is_some() && orc.is_none() && obr.is_none(),
            "ORC" if pid.is_some() && orc.is_none() && obr.is_none() => {
                orc = Some(segment);
                true
            }
            "OBR" if pid.is_some() && obr.is_none() && (code != "ORM" || orc.is_some()) => {
                obr = Some(segment);
                true
            }
            "OBX" | "TQ1" | "TQ2" | "CTD" | "CTI" => obr.is_some(),
            "NTE" => true,
            _ => false,
        };
        if !ok {
            return Err(Error::invalid(format!(
                "unsupported, repeated or misplaced HL7 segment at position {}",
                i + 1
            )));
        }
    }
    match (pid, obr) {
        (Some(pid), Some(obr)) => Ok(IdentityGroup {
            pid,
            orc,
            obr,
            control_id: control.string().to_owned(),
        }),
        _ => Err(Error::invalid(
            "expected one HL7 patient and observation request group",
        )),
    }
}
#[derive(Default, PartialEq)]
struct OrderId {
    value: String,
    authority: HierarchicDesignator,
}
impl IdentityGroup<'_> {
    fn identities(
        &self,
        map_authority: Hl7AuthorityResolver<'_>,
        result: bool,
    ) -> Result<(Patient, Vec<OrderReference>), Error> {
        let patient = hl7_patient(self.pid, map_authority)?;
        let mut references = Vec::new();
        for (field, kind, role) in [
            (2usize, REF_PLACER, HL7_PLACER_ORDER),
            (3, REF_FILLER, HL7_FILLER_ORDER),
        ] {
            let common_field: &[_] = match self.orc {
                Some(orc) => orc.field(field),
                None => &[],
            };
            let common = read_order_id(common_field, "ORC", field)?;
            let mut detail = read_order_id(self.obr.field(field), "OBR", field)?;
            if !common.value.is_empty() && !detail.value.is_empty() && common != detail {
                return Err(identity_error(
                    "OBR",
                    field,
                    "order identifier disagrees with ORC",
                ));
            }
            if detail.value.is_empty() {
                if result && !common.value.is_empty() {
                    return Err(identity_error(
                        "OBR",
                        field,
                        "result is missing its order identifier",
                    ));
                }
                detail = common;
            }
            if detail.value.is_empty() {
                continue;
            }
            let system = map_authority(&detail.authority, role, "");
            let id = Identifier {
                system: system.clone().unwrap_or_default(),
                value: detail.value,
            };
            if system.is_none() || !identifier_valid(&id) {
                return Err(identity_error(
                    "OBR",
                    field,
                    "order authority is not in the crosswalk",
                ));
            }
            references.push(OrderReference {
                kind: kind.to_owned(),
                id,
            });
        }
        if references.is_empty() {
            return Err(identity_error("OBR", 2, "no assigned order identifier"));
        }
        Ok((patient, references))
    }
}
fn hl7_patient(pid: &Segment, map_authority: Hl7AuthorityResolver<'_>) -> Result<Patient, Error> {
    let mut patient = Patient::default();
    for repetition in pid.field(3) {
        if repetition.len() > 10
            || repetition.component(1).len() != 1
            || repetition.component(4).len() > 3
            || repetition.component(5).len() > 1
        {
            return Err(identity_error("PID", 3, "invalid patient identifier shape"));
        }
        let hd = repetition.component(4);
        let authority = HierarchicDesignator {
            namespace_id: hd.sub(1).to_owned(),
            universal_id: hd.sub(2).to_owned(),
            universal_id_type: hd.sub(3).to_owned(),
        };
        let value = repetition.string(1);
        if !authority_present(&authority) || value.is_empty() || value == NULL {
            return Err(identity_error(
                "PID",
                3,
                "missing patient identity or assigning authority",
            ));
        }
        let Some(system) = map_authority(&authority, HL7_PATIENT_IDENTIFIER, repetition.string(5))
        else {
            continue;
        };
        let mut qualifiers: Field = vec![vec![
            repetition.component(2).to_vec(),
            repetition.component(3).to_vec(),
        ]];
        if repetition.len() > 5 {
            qualifiers.push(repetition[5..].to_vec());
        }
        if !qualifiers.is_empty_value() {
            return Err(identity_error(
                "PID",
                3,
                "unsupported patient identifier qualifiers",
            ));
        }
        let id = Identifier {
            system,
            value: value.to_owned(),
        };
        if !identifier_valid(&id) || (!patient.id.is_zero() && patient.id != id) {
            return Err(identity_error(
                "PID",
                3,
                "ambiguous or invalid mapped patient identity",
            ));
        }
        patient.id = id;
    }
    if patient.id.is_zero() {
        return Err(identity_error("PID", 3, "no accepted patient authority"));
    }
    patient.birth_date = birth_date(pid.field(7))?;
    let field = pid.field(8);
    if field.is_empty_value()
        || simple_field(field, "A")
        || simple_field(field, "N")
        || simple_field(field, "U")
    {
    } else if simple_field(field, "M") {
        patient.sex = "male".into();
    } else if simple_field(field, "F") {
        patient.sex = "female".into();
    } else if simple_field(field, "O") {
        patient.sex = "other".into();
    } else {
        return Err(identity_error("PID", 8, "unsupported administrative sex"));
    }
    Ok(patient)
}
fn birth_date(field: &[crate::hl7::Repetition]) -> Result<String, Error> {
    if field.is_empty_value() {
        return Ok(String::new());
    }
    if !scalar(field) {
        return Err(identity_error("PID", 7, "expected one date"));
    }
    let date = field.string();
    let parts: &[usize] = match date.len() {
        4 => &[4],
        6 => &[4, 2],
        8 => &[4, 2, 2],
        _ => {
            return Err(identity_error("PID", 7, "unsupported birth date precision"));
        }
    };
    match parse_date(date, parts, "") {
        Some((y, m, d)) if y != 0 => Ok(match parts.len() {
            1 => format!("{y:04}"),
            2 => format!("{y:04}-{m:02}"),
            _ => format!("{y:04}-{m:02}-{d:02}"),
        }),
        _ => Err(identity_error("PID", 7, "invalid birth date")),
    }
}
fn read_order_id(
    field: &[crate::hl7::Repetition],
    segment: &str,
    number: usize,
) -> Result<OrderId, Error> {
    if field.is_empty_value() {
        return Ok(OrderId::default());
    }
    if field.len() != 1 || !components_bounded(&field[0], 4) {
        return Err(identity_error(
            segment,
            number,
            "invalid order identifier shape",
        ));
    }
    let id = &field[0];
    let authority = HierarchicDesignator {
        namespace_id: id.string(2).to_owned(),
        universal_id: id.string(3).to_owned(),
        universal_id_type: id.string(4).to_owned(),
    };
    if id.string(1).is_empty() || id.string(1) == NULL || !authority_present(&authority) {
        return Err(identity_error(
            segment,
            number,
            "missing order identifier or assigning authority",
        ));
    }
    Ok(OrderId {
        value: id.string(1).to_owned(),
        authority,
    })
}
fn authority_present(a: &HierarchicDesignator) -> bool {
    (!a.namespace_id.is_empty() || !a.universal_id.is_empty())
        && a.universal_id.is_empty() == a.universal_id_type.is_empty()
        && a.namespace_id != NULL
        && a.universal_id != NULL
        && a.universal_id_type != NULL
}
fn scalar(field: &[crate::hl7::Repetition]) -> bool {
    field.len() == 1 && field[0].len() == 1 && field[0][0].len() == 1
}
fn simple_field(field: &[crate::hl7::Repetition], want: &str) -> bool {
    scalar(field) && field.string() == want
}
fn components_bounded(repetition: &[crate::hl7::Component], limit: usize) -> bool {
    repetition.len() <= limit && repetition.iter().all(|c| c.len() <= 1)
}
