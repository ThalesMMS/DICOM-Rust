use super::uid::hmac_sha256;
use super::{
    Error, Kind, TAG_ISSUER_OF_PATIENT_ID, TAG_PATIENT_BIRTH_DATE, TAG_PATIENT_ID,
    TAG_PATIENT_NAME, trim_space,
};
use crate::object::Object;
use std::collections::HashMap;
pub const MIN_PSEUDONYM_SECRET_BYTES: usize = 16;
pub const DEFAULT_PSEUDONYM_DOMAIN: &[u8] = b"dcm-rust/deid pseudonym v1";
const MAX_PSEUDONYM_LENGTH: usize = 64;
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PseudonymTemplate {
    pub patient_name: String,
    pub patient_id: String,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Pseudonym {
    pub patient_name: String,
    pub patient_id: String,
}
#[derive(Clone, Debug)]
enum Part {
    Literal(String),
    Counter(usize),
    Hash(usize),
}
#[derive(Clone, Debug, Default)]
struct Field {
    parts: Vec<Part>,
}
#[derive(Clone, Debug)]
pub struct Pseudonymizer {
    name: Field,
    id: Field,
    secret: Vec<u8>,
    domain: Vec<u8>,
    by_key: HashMap<String, Pseudonym>,
    owners: HashMap<String, String>,
    next: usize,
}
fn invalid(detail: &str) -> Error {
    Error::new(Kind::InvalidPseudonymTemplate, detail)
}
impl Pseudonymizer {
    pub fn new(template: &PseudonymTemplate, secret: &[u8]) -> Result<Pseudonymizer, Error> {
        let name = compile_field(&template.patient_name, true)?;
        let id = compile_field(&template.patient_id, false)?;
        if name.parts.is_empty() && id.parts.is_empty() {
            return Err(invalid("both fields are empty"));
        }
        if (name.uses_hash() || id.uses_hash()) && secret.len() < MIN_PSEUDONYM_SECRET_BYTES {
            return Err(Error::bare(Kind::PseudonymSecretRequired));
        }
        Ok(Pseudonymizer {
            name,
            id,
            secret: secret.to_vec(),
            domain: DEFAULT_PSEUDONYM_DOMAIN.to_vec(),
            by_key: HashMap::new(),
            owners: HashMap::new(),
            next: 0,
        })
    }
    #[must_use]
    pub fn with_domain(mut self, domain: &[u8]) -> Pseudonymizer {
        self.domain = domain.to_vec();
        self
    }
    pub fn pseudonym(&mut self, identity: &str) -> Result<Pseudonym, Error> {
        let key = identity.to_string();
        if let Some(existing) = self.by_key.get(&key) {
            return Ok(existing.clone());
        }
        let mut hash = String::new();
        if !self.secret.is_empty() {
            let mut message = self.domain.clone();
            message.push(0);
            message.extend_from_slice(identity.as_bytes());
            hash = hmac_sha256(&self.secret, &message)
                .iter()
                .map(|b| format!("{b:02X}"))
                .collect();
        }
        let counter = self.next + 1;
        let name = self.name.render(counter, &hash)?;
        let id = self.id.render(counter, &hash)?;
        for owned in [format!("id\0{id}"), format!("name\0{name}")] {
            if owned.ends_with('\0') {
                continue;
            }
            if self.owners.get(&owned).is_some_and(|owner| *owner != key) {
                return Err(Error::bare(Kind::PseudonymCollision));
            }
        }
        if !id.is_empty() {
            self.owners.insert(format!("id\0{id}"), key.clone());
        }
        if !name.is_empty() {
            self.owners.insert(format!("name\0{name}"), key.clone());
        }
        self.next = counter;
        let pseudonym = Pseudonym {
            patient_name: name,
            patient_id: id,
        };
        self.by_key.insert(key, pseudonym.clone());
        Ok(pseudonym)
    }
}
fn compile_field(template: &str, person_name: bool) -> Result<Field, Error> {
    let mut field = Field::default();
    if template.is_empty() {
        return Ok(field);
    }
    let mut has_token = false;
    let mut rest = template;
    while !rest.is_empty() {
        let open = rest.find('{');
        let literal = match open {
            Some(o) => &rest[..o],
            None => rest,
        };
        validate_literal(literal, person_name)?;
        if !literal.is_empty() {
            field.parts.push(Part::Literal(literal.to_string()));
        }
        let Some(open) = open else {
            break;
        };
        let Some(closing) = rest[open..].find('}') else {
            return Err(invalid("unterminated token"));
        };
        field
            .parts
            .push(parse_token(&rest[open + 1..open + closing])?);
        has_token = true;
        rest = &rest[open + closing + 1..];
    }
    if !has_token {
        return Err(invalid("a template needs a {counter} or {hash} token"));
    }
    if field.max_length() > MAX_PSEUDONYM_LENGTH {
        return Err(invalid(&format!(
            "rendered value may exceed {MAX_PSEUDONYM_LENGTH} characters"
        )));
    }
    Ok(field)
}
fn atoi(s: &str) -> Option<i64> {
    crate::numtext::parse_int(s).ok()
}
fn parse_token(token: &str) -> Result<Part, Error> {
    let (name, width) = match token.split_once(':') {
        Some((n, w)) => (n, Some(w)),
        None => (token, None),
    };
    match name {
        "counter" => {
            let mut w = 1;
            if let Some(text) = width {
                match atoi(text) {
                    Some(n) if (1..=12).contains(&n) => w = n as usize,
                    _ => return Err(invalid("counter width must be 1..12")),
                }
            }
            Ok(Part::Counter(w))
        }
        "hash" => {
            let mut w = 12;
            if let Some(text) = width {
                match atoi(text) {
                    Some(n) if (1..=64).contains(&n) => w = n as usize,
                    _ => return Err(invalid("hash width must be 1..64")),
                }
            }
            Ok(Part::Hash(w))
        }
        _ => Err(invalid("unknown token")),
    }
}
fn validate_literal(literal: &str, person_name: bool) -> Result<(), Error> {
    for r in literal.chars() {
        if (r as u32) < 0x20 || (r as u32) > 0x7e || matches!(r, '\\' | '{' | '}' | '=') {
            return Err(invalid(
                "literal characters must be printable ASCII without \\ { } =",
            ));
        }
        if r == '^' && !person_name {
            return Err(invalid("Patient ID literals cannot contain ^"));
        }
    }
    Ok(())
}
impl Field {
    fn uses_hash(&self) -> bool {
        self.parts.iter().any(|p| matches!(p, Part::Hash(_)))
    }
    fn max_length(&self) -> usize {
        self.parts
            .iter()
            .map(|p| match p {
                Part::Literal(l) => l.len(),
                Part::Counter(w) | Part::Hash(w) => *w,
            })
            .sum()
    }
    fn render(&self, counter: usize, hash: &str) -> Result<String, Error> {
        if self.parts.is_empty() {
            return Ok(String::new());
        }
        let mut out = String::new();
        for part in &self.parts {
            match part {
                Part::Counter(w) => out.push_str(&format!("{counter:0w$}")),
                Part::Hash(w) => out.push_str(&hash[..*w]),
                Part::Literal(l) => out.push_str(l),
            }
        }
        if out.len() > MAX_PSEUDONYM_LENGTH {
            return Err(invalid(&format!(
                "rendered value exceeds {MAX_PSEUDONYM_LENGTH} characters"
            )));
        }
        Ok(out)
    }
}
pub fn patient_pseudonym_identity(obj: &Object) -> String {
    let value = |tag| trim_space(&obj.get_string(tag).unwrap_or_default()).to_string();
    let id = value(TAG_PATIENT_ID);
    if !id.is_empty() {
        return format!("id\0{}\0{id}", value(TAG_ISSUER_OF_PATIENT_ID));
    }
    let name = value(TAG_PATIENT_NAME);
    if !name.is_empty() {
        return format!("name\0{name}\0{}", value(TAG_PATIENT_BIRTH_DATE));
    }
    String::new()
}
#[cfg(test)]
mod domain_tests {
    use super::*;
    fn keyed() -> Pseudonymizer {
        let template = PseudonymTemplate {
            patient_name: "ANON^{hash:10}".into(),
            patient_id: "PSN-{hash:16}".into(),
        };
        Pseudonymizer::new(&template, b"0123456789abcdef").unwrap()
    }
    #[test]
    fn the_domain_prefixes_the_keyed_hash() {
        let plain = keyed().pseudonym("Doe^Jane").unwrap();
        let default = keyed()
            .with_domain(DEFAULT_PSEUDONYM_DOMAIN)
            .pseudonym("Doe^Jane")
            .unwrap();
        let other = keyed()
            .with_domain(b"another domain")
            .pseudonym("Doe^Jane")
            .unwrap();
        let again = keyed()
            .with_domain(b"another domain")
            .pseudonym("Doe^Jane")
            .unwrap();
        assert_eq!(plain, default);
        assert_eq!(other, again);
        assert_ne!(plain.patient_name, other.patient_name);
        assert_ne!(plain.patient_id, other.patient_id);
    }
}
