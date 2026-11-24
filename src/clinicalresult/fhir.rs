use super::json::{self, Json, Kind};
use super::{
    ClinicalResult, Error, Identifier, Order, OrderReference, Patient, REF_ACCESSION, REF_FILLER,
    REF_PLACER, REF_SERVICE_REQUEST, STATUS_CORRECTED, STATUS_FINAL, STATUS_WITHDRAWN,
    identifier_valid, order_valid, patient_valid, result_valid,
};
use sha2::{Digest, Sha256};
const MAX_FHIR_PROJECTION_BYTES: usize = 1 << 20;
const MAX_FHIR_PROJECTION_DEPTH: usize = 32;
const MAX_FHIR_ORDER_IDENTIFIERS: usize = 32;
const FHIR_STRUCTURE_BASE: &str = "http://hl7.org/fhir/StructureDefinition/";
pub type OrderIdentifierKind<'a> = &'a dyn Fn(&Identifier) -> Option<String>;
pub struct FhirBinding<'a> {
    pub base_url: String,
    pub patient_logical_id: String,
    pub patient: Patient,
    pub order_identifier_kind: Option<OrderIdentifierKind<'a>>,
}
fn projection_error(field: &str) -> Error {
    Error::invalid(format!("FHIR {field}"))
}
#[derive(Default)]
struct FhirIdentifier {
    system: String,
    value: String,
    use_: String,
    period_len: usize,
}
impl FhirIdentifier {
    fn current(&self) -> Option<Identifier> {
        let id = Identifier {
            system: self.system.clone(),
            value: self.value.clone(),
        };
        match self.use_.as_str() {
            "" | "usual" | "official" | "secondary" => (self.period_len == 0
                && identifier_valid(&id)
                && !self.system.contains('\u{FFFD}')
                && !self.value.contains('\u{FFFD}'))
            .then_some(id),
            _ => None,
        }
    }
}
#[derive(Default)]
struct FhirReference {
    reference: String,
    type_: String,
    identifier: Option<FhirIdentifier>,
    modifier_extension: usize,
}
impl FhirReference {
    fn valid_type(&self, kind: &str) -> bool {
        self.modifier_extension == 0
            && (self.type_.is_empty()
                || self.type_ == kind
                || self.type_ == format!("{FHIR_STRUCTURE_BASE}{kind}"))
    }
}
#[derive(Default)]
struct FhirResource {
    resource_type: String,
    id: String,
    status: String,
    intent: String,
    do_not_perform: Option<Vec<u8>>,
    negation_extension: usize,
    subject: FhirReference,
    identifier: Vec<FhirIdentifier>,
    based_on: Vec<FhirReference>,
    modifier_extension: usize,
    version_id: String,
}
type Un<T> = Result<T, ()>;
fn un_string(v: &Json, out: &mut String) -> Un<()> {
    match &v.kind {
        Kind::Null => Ok(()),
        Kind::Str(s) => {
            *out = s.clone();
            Ok(())
        }
        _ => Err(()),
    }
}
fn members(v: &Json) -> Un<Option<&[(String, Json)]>> {
    match &v.kind {
        Kind::Null => Ok(None),
        Kind::Object(m) => Ok(Some(m)),
        _ => Err(()),
    }
}
fn items(v: &Json) -> Un<Option<&[Json]>> {
    match &v.kind {
        Kind::Null => Ok(None),
        Kind::Array(a) => Ok(Some(a)),
        _ => Err(()),
    }
}
fn un_identifier(v: &Json, out: &mut FhirIdentifier) -> Un<()> {
    let mut err = false;
    if let Some(m) = members(v)? {
        for (k, v) in m {
            let r = match k.as_str() {
                "system" => un_string(v, &mut out.system),
                "value" => un_string(v, &mut out.value),
                "use" => un_string(v, &mut out.use_),
                "period" => {
                    out.period_len = v.end - v.start;
                    Ok(())
                }
                _ => Ok(()),
            };
            err |= r.is_err();
        }
    }
    if err { Err(()) } else { Ok(()) }
}
fn un_raw_slice(v: &Json) -> Un<usize> {
    Ok(items(v)?.map_or(0, <[Json]>::len))
}
fn un_reference(v: &Json, out: &mut FhirReference) -> Un<()> {
    let mut err = false;
    if let Some(m) = members(v)? {
        for (k, v) in m {
            let r = match k.as_str() {
                "reference" => un_string(v, &mut out.reference),
                "type" => un_string(v, &mut out.type_),
                "identifier" => match &v.kind {
                    Kind::Null => {
                        out.identifier = None;
                        Ok(())
                    }
                    _ => {
                        let mut id = out.identifier.take().unwrap_or_default();
                        let r = un_identifier(v, &mut id);
                        out.identifier = Some(id);
                        r
                    }
                },
                "modifierExtension" => un_raw_slice(v).map(|n| out.modifier_extension = n),
                _ => Ok(()),
            };
            err |= r.is_err();
        }
    }
    if err { Err(()) } else { Ok(()) }
}
fn un_resource(wire: &[u8], v: &Json) -> Un<FhirResource> {
    let mut out = FhirResource::default();
    let mut err = false;
    let Some(m) = members(v)? else {
        return Ok(out);
    };
    for (k, v) in m {
        let raw = || wire[v.start..v.end].to_vec();
        let r = match k.as_str() {
            "resourceType" => un_string(v, &mut out.resource_type),
            "id" => un_string(v, &mut out.id),
            "status" => un_string(v, &mut out.status),
            "intent" => un_string(v, &mut out.intent),
            "doNotPerform" => {
                out.do_not_perform = Some(raw());
                Ok(())
            }
            "_doNotPerform" => {
                out.negation_extension = v.end - v.start;
                Ok(())
            }
            "subject" => un_reference(v, &mut out.subject),
            "identifier" => match items(v) {
                Err(()) => Err(()),
                Ok(None) => {
                    out.identifier.clear();
                    Ok(())
                }
                Ok(Some(list)) => {
                    out.identifier.clear();
                    let mut e = false;
                    for item in list {
                        let mut id = FhirIdentifier::default();
                        e |= un_identifier(item, &mut id).is_err();
                        out.identifier.push(id);
                    }
                    if e { Err(()) } else { Ok(()) }
                }
            },
            "basedOn" => match items(v) {
                Err(()) => Err(()),
                Ok(None) => {
                    out.based_on.clear();
                    Ok(())
                }
                Ok(Some(list)) => {
                    out.based_on.clear();
                    let mut e = false;
                    for item in list {
                        let mut r = FhirReference::default();
                        e |= un_reference(item, &mut r).is_err();
                        out.based_on.push(r);
                    }
                    if e { Err(()) } else { Ok(()) }
                }
            },
            "modifierExtension" => un_raw_slice(v).map(|n| out.modifier_extension = n),
            "meta" => match members(v) {
                Err(()) => Err(()),
                Ok(None) => Ok(()),
                Ok(Some(m)) => {
                    let mut e = false;
                    for (k, v) in m {
                        if k == "versionId" {
                            e |= un_string(v, &mut out.version_id).is_err();
                        }
                    }
                    if e { Err(()) } else { Ok(()) }
                }
            },
            _ => Ok(()),
        };
        err |= r.is_err();
    }
    if err { Err(()) } else { Ok(out) }
}
const KNOWN_PROPERTIES: [&str; 18] = [
    "resourceType",
    "id",
    "status",
    "intent",
    "doNotPerform",
    "_doNotPerform",
    "subject",
    "basedOn",
    "meta",
    "versionId",
    "reference",
    "type",
    "identifier",
    "system",
    "value",
    "use",
    "period",
    "modifierExtension",
];
fn container_valid(v: &Json, depth: usize) -> bool {
    if depth > MAX_FHIR_PROJECTION_DEPTH {
        return false;
    }
    match &v.kind {
        Kind::Object(m) => {
            let mut keys = std::collections::HashSet::new();
            for (key, value) in m {
                if !key.is_ascii() {
                    return false;
                }
                let folded = key.to_ascii_lowercase();
                if let Some(canonical) = KNOWN_PROPERTIES
                    .iter()
                    .find(|c| c.to_ascii_lowercase() == folded)
                    && *canonical != key
                {
                    return false;
                }
                if !keys.insert(folded) {
                    return false;
                }
                if matches!(value.kind, Kind::Object(_) | Kind::Array(_))
                    && !container_valid(value, depth + 1)
                {
                    return false;
                }
            }
            true
        }
        Kind::Array(items) => items.iter().all(|value| {
            !matches!(value.kind, Kind::Object(_) | Kind::Array(_))
                || container_valid(value, depth + 1)
        }),
        _ => true,
    }
}
fn read_association(
    wire: &[u8],
    binding: &FhirBinding<'_>,
    expected_type: &str,
) -> Result<(FhirResource, String), Error> {
    let base = association_base(&binding.base_url);
    let Some(base) = base
        .filter(|_| valid_fhir_id(&binding.patient_logical_id) && patient_valid(&binding.patient))
    else {
        return Err(projection_error("patient binding"));
    };
    let resource = (|| {
        if wire.len() > MAX_FHIR_PROJECTION_BYTES || std::str::from_utf8(wire).is_err() {
            return None;
        }
        let tree = json::parse(wire)?;
        if !matches!(tree.kind, Kind::Object(_)) || !container_valid(&tree, 1) {
            return None;
        }
        un_resource(wire, &tree).ok()
    })()
    .ok_or_else(|| projection_error("resource encoding"))?;
    if resource.resource_type != expected_type
        || !valid_fhir_id(&resource.id)
        || resource.modifier_extension != 0
    {
        return Err(projection_error("resource identity"));
    }
    let subject = &resource.subject;
    match literal_id(&base, "Patient", &subject.reference) {
        Some(id) if id == binding.patient_logical_id && subject.valid_type("Patient") => {}
        _ => return Err(projection_error("patient subject")),
    }
    if let Some(identifier) = &subject.identifier
        && identifier.current().as_ref() != Some(&binding.patient.id)
    {
        return Err(projection_error("patient identifier"));
    }
    Ok((resource, base))
}
impl FhirBinding<'_> {
    fn classified(&self, raw: &FhirIdentifier) -> Option<OrderReference> {
        let id = raw.current()?;
        let classify = self.order_identifier_kind?;
        let kind = classify(&id)?;
        if kind != REF_PLACER && kind != REF_FILLER && kind != REF_ACCESSION {
            return None;
        }
        Some(OrderReference { kind, id })
    }
    fn order_reference(&self, base: &str, r: &FhirReference) -> Result<Vec<OrderReference>, Error> {
        if !r.valid_type("ServiceRequest") || (r.reference.is_empty() && r.type_.is_empty()) {
            return Err(projection_error("DiagnosticReport order type"));
        }
        let mut references = Vec::new();
        if !r.reference.is_empty() {
            let Some(id) = literal_id(base, "ServiceRequest", &r.reference) else {
                return Err(projection_error("DiagnosticReport order scope"));
            };
            references.push(OrderReference {
                kind: REF_SERVICE_REQUEST.into(),
                id: Identifier {
                    system: format!("{base}/ServiceRequest"),
                    value: id,
                },
            });
        }
        if let Some(identifier) = &r.identifier {
            let Some(reference) = self.classified(identifier) else {
                return Err(projection_error("DiagnosticReport order identifier"));
            };
            references.push(reference);
        }
        if references.is_empty() {
            return Err(projection_error("DiagnosticReport order reference"));
        }
        Ok(references)
    }
}
pub fn order_from_fhir(wire: &[u8], binding: &FhirBinding<'_>) -> Result<Order, Error> {
    let (resource, base) = read_association(wire, binding, "ServiceRequest")?;
    if !matches!(resource.status.as_str(), "active" | "completed") {
        return Err(projection_error("ServiceRequest status"));
    }
    if !matches!(
        resource.intent.as_str(),
        "order" | "original-order" | "reflex-order" | "filler-order" | "instance-order"
    ) {
        return Err(projection_error("ServiceRequest intent"));
    }
    let dnp_ok = match &resource.do_not_perform {
        None => true,
        Some(raw) => raw.is_empty() || raw.trim_ascii() == b"false",
    };
    if resource.negation_extension != 0
        || !dnp_ok
        || resource.identifier.len() > MAX_FHIR_ORDER_IDENTIFIERS
    {
        return Err(projection_error("ServiceRequest identity or intent"));
    }
    let id = Identifier {
        system: format!("{base}/ServiceRequest"),
        value: resource.id.clone(),
    };
    let mut order = Order {
        id: id.clone(),
        patient: binding.patient.clone(),
        references: vec![OrderReference {
            kind: REF_SERVICE_REQUEST.into(),
            id,
        }],
    };
    for identifier in &resource.identifier {
        if let Some(reference) = binding.classified(identifier) {
            order.references.push(reference);
        }
    }
    if !order_valid(&order) {
        return Err(projection_error("ServiceRequest identity"));
    }
    Ok(order)
}
pub fn result_from_fhir(
    wire: &[u8],
    binding: &FhirBinding<'_>,
    sequence: u64,
) -> Result<ClinicalResult, Error> {
    let (resource, base) = read_association(wire, binding, "DiagnosticReport")?;
    if resource.based_on.len() != 1 || !valid_fhir_id(&resource.version_id) {
        return Err(projection_error("DiagnosticReport order or revision"));
    }
    let order = binding.order_reference(&base, &resource.based_on[0])?;
    let status = match resource.status.as_str() {
        "final" => STATUS_FINAL,
        "amended" | "corrected" | "appended" => STATUS_CORRECTED,
        "cancelled" | "entered-in-error" => STATUS_WITHDRAWN,
        _ => return Err(projection_error("DiagnosticReport status")),
    };
    let result = ClinicalResult {
        source: base.clone(),
        id: Identifier {
            system: format!("{base}/DiagnosticReport"),
            value: resource.id.clone(),
        },
        patient: binding.patient.clone(),
        order_references: order,
        sequence,
        revision_id: resource.version_id.clone(),
        status: status.to_owned(),
        digest: Sha256::digest(wire).into(),
    };
    if !result_valid(&result) {
        return Err(projection_error("DiagnosticReport identity or revision"));
    }
    Ok(result)
}
fn literal_id(base: &str, kind: &str, reference: &str) -> Option<String> {
    let prefix = format!("{base}/");
    let r = reference.strip_prefix(&prefix).unwrap_or(reference);
    let id = r.strip_prefix(&format!("{kind}/"))?;
    valid_fhir_id(id).then(|| id.to_owned())
}
fn valid_fhir_id(id: &str) -> bool {
    if id.is_empty() || id.len() > 64 || id == "." || id == ".." {
        return false;
    }
    id.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.')
}
fn host_byte_ok(c: u8) -> bool {
    c.is_ascii_alphanumeric() || b"-_.~!$&'()*+,;=:[]<>\"".contains(&c)
}
fn path_byte_plain(c: u8) -> bool {
    c.is_ascii_alphanumeric() || b"-_.~$&+,/:;=@".contains(&c)
}
fn unhex(c: u8) -> Option<u8> {
    (c as char).to_digit(16).map(|v| v as u8)
}
fn parse_host(host: &str) -> Option<String> {
    let b = host.as_bytes();
    let hostname: &str;
    if host.starts_with('[') {
        let i = host.rfind(']')?;
        let port = &host[i + 1..];
        if !valid_optional_port(port) {
            return None;
        }
        hostname = &host[1..i];
    } else if let Some(i) = host.rfind(':') {
        if !valid_optional_port(&host[i..]) {
            return None;
        }
        hostname = &host[..i];
    } else {
        hostname = host;
    }
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c == b'%' {
            let (h, l) = (b.get(i + 1)?, b.get(i + 2)?);
            let hi = unhex(*h)?;
            unhex(*l)?;
            if hi < 8 && &b[i..i + 3] != b"%25" {
                return None;
            }
            i += 3;
            continue;
        }
        if c < 0x80 && !host_byte_ok(c) {
            return None;
        }
        i += 1;
    }
    if hostname.contains('%') {
        return Some(hostname.to_owned());
    }
    Some(hostname.to_owned())
}
fn valid_optional_port(port: &str) -> bool {
    if port.is_empty() {
        return true;
    }
    let Some(digits) = port.strip_prefix(':') else {
        return false;
    };
    digits.bytes().all(|c| c.is_ascii_digit())
}
fn association_base(raw: &str) -> Option<String> {
    if raw.contains(['?', '#', '\\']) {
        return None;
    }
    if raw.bytes().any(|c| c < 0x20 || c == 0x7f) {
        return None;
    }
    let mut scheme = None;
    for (i, c) in raw.bytes().enumerate() {
        if c.is_ascii_alphabetic() {
            continue;
        }
        if c.is_ascii_digit() || c == b'+' || c == b'-' || c == b'.' {
            if i == 0 {
                break;
            }
            continue;
        }
        if c == b':' {
            if i == 0 {
                return None;
            }
            scheme = Some((raw[..i].to_ascii_lowercase(), &raw[i + 1..]));
        }
        break;
    }
    let (scheme, rest) = scheme?;
    if scheme != "https" && scheme != "http" {
        return None;
    }
    if !rest.starts_with('/') {
        return None;
    }
    let (host, path) = if let Some(after) = rest.strip_prefix("//") {
        let (authority, path) = match after.find('/') {
            Some(i) => (&after[..i], &after[i..]),
            None => (after, ""),
        };
        if authority.contains('@') {
            return None;
        }
        (parse_host(authority)?, path)
    } else {
        (String::new(), rest)
    };
    if host.is_empty() {
        return None;
    }
    if !path.bytes().all(path_byte_plain) {
        return None;
    }
    let trimmed = path.strip_suffix('/').unwrap_or(path);
    if trimmed.contains("//") || trimmed.ends_with('/') {
        return None;
    }
    if trimmed.split('/').any(|s| s == "." || s == "..") {
        return None;
    }
    let result = raw.strip_suffix('/').unwrap_or(raw).to_owned();
    identifier_valid(&Identifier {
        system: format!("{result}/DiagnosticReport"),
        value: "id".into(),
    })
    .then_some(result)
}
