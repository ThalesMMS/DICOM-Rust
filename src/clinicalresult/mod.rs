#![forbid(unsafe_code)]
mod fhir;
mod hl7;
mod json;
pub use fhir::{FhirBinding, OrderIdentifierKind, order_from_fhir, result_from_fhir};
pub use hl7::{
    HL7_FILLER_ORDER, HL7_PATIENT_IDENTIFIER, HL7_PLACER_ORDER, Hl7AuthorityResolver,
    order_from_hl7, result_from_hl7,
};
use std::collections::HashMap;
use std::fmt;
use std::sync::RwLock;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sentinel {
    InvalidInput,
    PatientScope,
    IdentityConflict,
    NoOrder,
    AmbiguousOrder,
    OrderChanged,
    RevisionConflict,
    Capacity,
}
impl Sentinel {
    pub fn text(self) -> &'static str {
        match self {
            Sentinel::InvalidInput => "clinical result: invalid or out-of-profile input",
            Sentinel::PatientScope => "clinical result: subject outside authorized patient scope",
            Sentinel::IdentityConflict => "clinical result: conflicting patient identity",
            Sentinel::NoOrder => "clinical result: no matching order in authorized scope",
            Sentinel::AmbiguousOrder => "clinical result: ambiguous order in authorized scope",
            Sentinel::OrderChanged => {
                "clinical result: revision cannot change the associated order"
            }
            Sentinel::RevisionConflict => {
                "clinical result: conflicting or invalid revision transition"
            }
            Sentinel::Capacity => "clinical result: ledger capacity exceeded",
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub kind: Sentinel,
    pub detail: Option<String>,
}
impl Error {
    fn bare(kind: Sentinel) -> Error {
        Error { kind, detail: None }
    }
    fn invalid(detail: impl Into<String>) -> Error {
        Error {
            kind: Sentinel::InvalidInput,
            detail: Some(detail.into()),
        }
    }
    pub fn is(&self, s: Sentinel) -> bool {
        self.kind == s
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.detail {
            None => f.write_str(self.kind.text()),
            Some(d) => write!(f, "{}: {d}", self.kind.text()),
        }
    }
}
impl std::error::Error for Error {}
const MAX_IDENTIFIER_BYTES: usize = 512;
const MAX_ORDER_REFERENCES: usize = 16;
const MAX_CANDIDATE_ORDERS: usize = 10_000;
const MAX_LEDGER_ENTRIES: i64 = 100_000;
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct Identifier {
    pub system: String,
    pub value: String,
}
impl Identifier {
    fn is_zero(&self) -> bool {
        self.system.is_empty() && self.value.is_empty()
    }
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Patient {
    pub id: Identifier,
    pub birth_date: String,
    pub sex: String,
}
pub const REF_PLACER: &str = "placer";
pub const REF_FILLER: &str = "filler";
pub const REF_ACCESSION: &str = "accession";
pub const REF_SERVICE_REQUEST: &str = "service-request";
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OrderReference {
    pub kind: String,
    pub id: Identifier,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Order {
    pub id: Identifier,
    pub patient: Patient,
    pub references: Vec<OrderReference>,
}
pub const STATUS_FINAL: &str = "final";
pub const STATUS_CORRECTED: &str = "corrected";
pub const STATUS_WITHDRAWN: &str = "withdrawn";
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClinicalResult {
    pub source: String,
    pub id: Identifier,
    pub patient: Patient,
    pub order_references: Vec<OrderReference>,
    pub sequence: u64,
    pub revision_id: String,
    pub status: String,
    pub digest: [u8; 32],
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Revision {
    pub sequence: u64,
    pub id: String,
    pub status: String,
    pub digest: [u8; 32],
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Current {
    pub order_id: Identifier,
    pub revision: Revision,
}
pub const CREATED: &str = "created";
pub const REPLACED: &str = "replaced";
pub const REPLAY: &str = "replay";
pub const STALE: &str = "stale";
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Decision {
    pub disposition: String,
    pub current: Current,
    pub previous: Revision,
}
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ResultKey {
    patient: Identifier,
    source: String,
    result: Identifier,
}
#[derive(Debug, Clone)]
struct Entry {
    patient: Patient,
    current: Current,
}
#[derive(Debug)]
pub struct Ledger {
    limit: usize,
    current: RwLock<HashMap<ResultKey, Entry>>,
}
impl Ledger {
    pub fn new(max_entries: i64) -> Result<Ledger, Error> {
        if !(1..=MAX_LEDGER_ENTRIES).contains(&max_entries) {
            return Err(Error::bare(Sentinel::InvalidInput));
        }
        Ok(Ledger {
            limit: max_entries as usize,
            current: RwLock::new(HashMap::new()),
        })
    }
    pub fn apply(
        &self,
        authorized: &Patient,
        result: &ClinicalResult,
        orders: &[Order],
    ) -> Result<Decision, Error> {
        if !patient_valid(authorized)
            || !result_valid(result)
            || orders.len() > MAX_CANDIDATE_ORDERS
        {
            return Err(Error::bare(Sentinel::InvalidInput));
        }
        if authorized.id != result.patient.id {
            return Err(Error::bare(Sentinel::PatientScope));
        }
        let patient = combine_patient(authorized, &result.patient)?;
        let (candidate, patient) = select_order(&patient, &result.order_references, orders)?;
        let current = Current {
            order_id: candidate,
            revision: Revision {
                sequence: result.sequence,
                id: result.revision_id.clone(),
                status: result.status.clone(),
                digest: result.digest,
            },
        };
        let key = ResultKey {
            patient: patient.id.clone(),
            source: result.source.clone(),
            result: result.id.clone(),
        };
        self.publish(key, patient, current)
    }
    fn publish(
        &self,
        key: ResultKey,
        patient: Patient,
        incoming: Current,
    ) -> Result<Decision, Error> {
        let mut map = self.current.write().unwrap_or_else(|e| e.into_inner());
        let Some(prior) = map.get(&key).cloned() else {
            if map.len() == self.limit {
                return Err(Error::bare(Sentinel::Capacity));
            }
            map.insert(
                key,
                Entry {
                    patient,
                    current: incoming.clone(),
                },
            );
            return Ok(Decision {
                disposition: CREATED.into(),
                current: incoming,
                previous: Revision::default(),
            });
        };
        let patient = combine_patient(&patient, &prior.patient)?;
        if prior.current.order_id != incoming.order_id {
            return Err(Error::bare(Sentinel::OrderChanged));
        }
        let (before, after) = (&prior.current.revision, &incoming.revision);
        if after.sequence < before.sequence {
            return Ok(Decision {
                disposition: STALE.into(),
                current: prior.current,
                previous: Revision::default(),
            });
        }
        if after == before {
            return Ok(Decision {
                disposition: REPLAY.into(),
                current: prior.current,
                previous: Revision::default(),
            });
        }
        if after.sequence == before.sequence
            || after.id == before.id
            || after.status == STATUS_FINAL
        {
            return Err(Error::bare(Sentinel::RevisionConflict));
        }
        if before.status == STATUS_WITHDRAWN && after.status != STATUS_WITHDRAWN {
            return Err(Error::bare(Sentinel::RevisionConflict));
        }
        let previous = before.clone();
        map.insert(
            key,
            Entry {
                patient,
                current: incoming.clone(),
            },
        );
        Ok(Decision {
            disposition: REPLACED.into(),
            current: incoming,
            previous,
        })
    }
    pub fn lookup(
        &self,
        authorized: &Patient,
        source: &str,
        result: &Identifier,
    ) -> Result<Option<Current>, Error> {
        if !patient_valid(authorized) || !identifier_part_valid(source) || !identifier_valid(result)
        {
            return Err(Error::bare(Sentinel::InvalidInput));
        }
        let map = self.current.read().unwrap_or_else(|e| e.into_inner());
        let key = ResultKey {
            patient: authorized.id.clone(),
            source: source.to_owned(),
            result: result.clone(),
        };
        let Some(prior) = map.get(&key) else {
            return Ok(None);
        };
        combine_patient(authorized, &prior.patient)?;
        Ok(Some(prior.current.clone()))
    }
}
fn select_order(
    patient: &Patient,
    required: &[OrderReference],
    candidates: &[Order],
) -> Result<(Identifier, Patient), Error> {
    let mut selected = Identifier::default();
    let mut ambiguous = false;
    let mut partial = patient.clone();
    for order in candidates {
        if order.patient.id != patient.id {
            continue;
        }
        if order.references.len() > MAX_ORDER_REFERENCES {
            return Err(Error::bare(Sentinel::InvalidInput));
        }
        let matches = required
            .iter()
            .filter(|r| order.references.contains(r))
            .count();
        if matches == 0 {
            continue;
        }
        if !order_valid(order) {
            return Err(Error::bare(Sentinel::InvalidInput));
        }
        partial = combine_patient(&partial, &order.patient)?;
        if matches == required.len() {
            ambiguous = ambiguous || (!selected.is_zero() && selected != order.id);
            selected = order.id.clone();
        }
    }
    if ambiguous {
        return Err(Error::bare(Sentinel::AmbiguousOrder));
    }
    if selected.is_zero() {
        return Err(Error::bare(Sentinel::NoOrder));
    }
    Ok((selected, partial))
}
fn combine_patient(a: &Patient, b: &Patient) -> Result<Patient, Error> {
    let shared = a.birth_date.len().min(b.birth_date.len());
    if a.id != b.id
        || a.birth_date.as_bytes()[..shared] != b.birth_date.as_bytes()[..shared]
        || (!a.sex.is_empty() && !b.sex.is_empty() && a.sex != b.sex)
    {
        return Err(Error::bare(Sentinel::IdentityConflict));
    }
    let mut out = a.clone();
    if b.birth_date.len() > a.birth_date.len() {
        out.birth_date = b.birth_date.clone();
    }
    if out.sex.is_empty() {
        out.sex = b.sex.clone();
    }
    Ok(out)
}
fn identifier_valid(id: &Identifier) -> bool {
    identifier_part_valid(&id.system) && identifier_part_valid(&id.value)
}
fn is_control(c: char) -> bool {
    let v = u32::from(c);
    v < 0x20 || (0x7f..=0x9f).contains(&v)
}
fn identifier_part_valid(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_IDENTIFIER_BYTES
        && value == value.trim()
        && !value.chars().any(is_control)
}
fn leap(y: i64) -> bool {
    y % 4 == 0 && (y % 100 != 0 || y % 400 == 0)
}
fn days_in(y: i64, m: i64) -> i64 {
    match m {
        2 if leap(y) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}
pub(crate) fn parse_date(value: &str, parts: &[usize], sep: &str) -> Option<(i64, i64, i64)> {
    let mut rest = value;
    let mut nums = Vec::new();
    for (i, &width) in parts.iter().enumerate() {
        if i > 0 {
            rest = rest.strip_prefix(sep)?;
        }
        if rest.len() < width || !rest.as_bytes()[..width].iter().all(u8::is_ascii_digit) {
            return None;
        }
        nums.push(rest[..width].parse::<i64>().ok()?);
        rest = &rest[width..];
    }
    if !rest.is_empty() {
        return None;
    }
    let year = nums[0];
    let month = nums.get(1).copied().unwrap_or(1);
    let day = nums.get(2).copied().unwrap_or(1);
    if !(1..=12).contains(&month) || day < 1 || day > days_in(year, month) {
        return None;
    }
    Some((year, month, day))
}
fn patient_valid(p: &Patient) -> bool {
    if !identifier_valid(&p.id) {
        return false;
    }
    if !matches!(p.sex.as_str(), "" | "male" | "female" | "other") {
        return false;
    }
    let date = &p.birth_date;
    if date.is_empty() {
        return true;
    }
    let parts: &[usize] = match date.len() {
        4 => &[4],
        7 => &[4, 2],
        10 => &[4, 2, 2],
        _ => return false,
    };
    parse_date(date, parts, "-").is_some_and(|(y, _, _)| y > 0)
}
fn references_valid(references: &[OrderReference]) -> bool {
    if references.is_empty() || references.len() > MAX_ORDER_REFERENCES {
        return false;
    }
    references.iter().all(|r| {
        identifier_valid(&r.id)
            && matches!(
                r.kind.as_str(),
                REF_PLACER | REF_FILLER | REF_ACCESSION | REF_SERVICE_REQUEST
            )
    })
}
fn order_valid(order: &Order) -> bool {
    identifier_valid(&order.id)
        && patient_valid(&order.patient)
        && references_valid(&order.references)
}
fn result_valid(r: &ClinicalResult) -> bool {
    if !matches!(
        r.status.as_str(),
        STATUS_FINAL | STATUS_CORRECTED | STATUS_WITHDRAWN
    ) {
        return false;
    }
    identifier_part_valid(&r.source)
        && identifier_valid(&r.id)
        && patient_valid(&r.patient)
        && references_valid(&r.order_references)
        && r.sequence > 0
        && identifier_part_valid(&r.revision_id)
        && r.digest != [0u8; 32]
}
