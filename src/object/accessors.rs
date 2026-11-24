#![forbid(unsafe_code)]
use super::error::{Error, Kind};
use super::file::File;
use super::obj::Object;
use super::text::{PersonName, TextOptions};
use super::types::{Element, Tag, Value, Vr};
use crate::charset::SpecificCharacterSet;
use crate::dcmtime;
impl File {
    pub(crate) fn object_for_tag(&self, tag: Tag) -> &Object {
        if tag.group == 0x0002 {
            &self.meta
        } else {
            &self.dataset
        }
    }
    pub fn get(&self, tag: Tag) -> Option<&Element> {
        self.object_for_tag(tag).get(tag)
    }
    pub fn has(&self, tag: Tag) -> bool {
        self.object_for_tag(tag).has(tag)
    }
    pub fn get_raw(&self, tag: Tag) -> Option<Vec<u8>> {
        self.object_for_tag(tag).get_raw(tag)
    }
    pub fn get_string(&self, tag: Tag) -> Option<String> {
        self.object_for_tag(tag).get_string(tag)
    }
    pub fn get_strings(&self, tag: Tag) -> Option<Vec<String>> {
        self.object_for_tag(tag).get_strings(tag)
    }
    pub fn lookup_string(&self, tag: Tag) -> Result<String, Error> {
        self.object_for_tag(tag).lookup_string(tag)
    }
    pub fn lookup_strings(&self, tag: Tag) -> Result<Vec<String>, Error> {
        self.object_for_tag(tag).lookup_strings(tag)
    }
    pub fn character_set(&self) -> Result<SpecificCharacterSet, Error> {
        self.dataset.character_set()
    }
    pub fn get_person_name(&self, tag: Tag) -> Option<PersonName> {
        self.object_for_tag(tag).get_person_name(tag)
    }
    pub fn get_person_names(&self, tag: Tag) -> Option<Vec<PersonName>> {
        self.object_for_tag(tag).get_person_names(tag)
    }
    pub fn get_uid(&self, tag: Tag) -> Option<String> {
        self.object_for_tag(tag).get_uid(tag)
    }
    pub fn get_uids(&self, tag: Tag) -> Option<Vec<String>> {
        self.object_for_tag(tag).get_uids(tag)
    }
    pub fn get_int(&self, tag: Tag) -> Result<i64, Error> {
        self.object_for_tag(tag).get_int(tag)
    }
    pub fn get_ints(&self, tag: Tag) -> Result<Vec<i64>, Error> {
        self.object_for_tag(tag).get_ints(tag)
    }
    pub fn get_float(&self, tag: Tag) -> Result<f64, Error> {
        self.object_for_tag(tag).get_float(tag)
    }
    pub fn get_floats(&self, tag: Tag) -> Result<Vec<f64>, Error> {
        self.object_for_tag(tag).get_floats(tag)
    }
    pub fn lookup_date(&self, tag: Tag) -> Result<dcmtime::Date, Error> {
        self.object_for_tag(tag).lookup_date(tag)
    }
    pub fn lookup_time(&self, tag: Tag) -> Result<dcmtime::Time, Error> {
        self.object_for_tag(tag).lookup_time(tag)
    }
    pub fn lookup_date_time(&self, tag: Tag) -> Result<dcmtime::Datetime, Error> {
        self.object_for_tag(tag).lookup_date_time(tag)
    }
    pub fn get_sequence(&self, tag: Tag) -> Option<Vec<Object>> {
        self.object_for_tag(tag).get_sequence(tag)
    }
    pub fn set_text_options(&mut self, opts: TextOptions) {
        self.meta.set_text_options(opts.clone());
        self.dataset.set_text_options(opts);
    }
    pub fn metadata(&self) -> FileMetadata {
        let s = |group, element| {
            self.get_string(Tag::new(group, element))
                .unwrap_or_default()
        };
        let transfer_syntax_uid = if !self.transfer_syntax.uid.is_empty() {
            self.transfer_syntax.uid.to_string()
        } else {
            self.get_uid(Tag::new(0x0002, 0x0010)).unwrap_or_default()
        };
        let transfer_syntax_name = if !self.transfer_syntax.name.is_empty() {
            self.transfer_syntax.name.to_string()
        } else {
            super::transfer::lookup(&transfer_syntax_uid)
                .map(|s| s.name.to_string())
                .unwrap_or_default()
        };
        FileMetadata {
            transfer_syntax_uid,
            transfer_syntax_name,
            patient_name: s(0x0010, 0x0010),
            patient_id: s(0x0010, 0x0020),
            issuer_of_patient_id: patient_id_with_issuer(&self.dataset)
                .map(|p| p.issuer)
                .unwrap_or_default(),
            patient_birth_date: s(0x0010, 0x0030),
            institution_name: s(0x0008, 0x0080),
            study_date: s(0x0008, 0x0020),
            study_time: s(0x0008, 0x0030),
            series_date: s(0x0008, 0x0021),
            series_time: s(0x0008, 0x0031),
            study_description: s(0x0008, 0x1030),
            modality: s(0x0008, 0x0060),
            accession_number: s(0x0008, 0x0050),
            series_description: s(0x0008, 0x103E),
            protocol_name: s(0x0018, 0x1030),
            study_instance_uid: s(0x0020, 0x000D),
            series_instance_uid: s(0x0020, 0x000E),
            series_number: s(0x0020, 0x0011),
            sop_class_uid: s(0x0008, 0x0016),
            sop_instance_uid: s(0x0008, 0x0018),
            instance_number: s(0x0020, 0x0013),
            study_id: s(0x0020, 0x0010),
            body_part_examined: s(0x0018, 0x0015),
            referring_physician_name: s(0x0008, 0x0090),
            performing_physician_name: s(0x0008, 0x1050),
        }
    }
    pub fn resolve_sop_class(&self, policy: SopClassPolicy) -> (SopClassResolution, Option<Error>) {
        let mut result = SopClassResolution::default();
        if let SopClassPolicy::Invalid(_) = policy {
            return (result, Some(Error::sentinel(Kind::InvalidSopClassPolicy)));
        }
        let data = declared_sop_class_uid(&self.dataset, Tag::new(0x0008, 0x0016));
        let meta = declared_sop_class_uid(&self.meta, Tag::new(0x0002, 0x0002));
        let (data_err, meta_err) = (data.as_ref().err().cloned(), meta.as_ref().err().cloned());
        result.dataset_uid = data.unwrap_or_default();
        result.file_meta_uid = meta.unwrap_or_default();
        if let Some(err) = Error::join(data_err, meta_err) {
            return (result, Some(err));
        }
        match (
            result.dataset_uid.is_empty(),
            result.file_meta_uid.is_empty(),
        ) {
            (true, true) => {
                return (result, Some(Error::sentinel(Kind::MissingSopClassUid)));
            }
            (true, false) => {
                result.uid = result.file_meta_uid.clone();
                result.source = SopClassSource::FileMeta;
            }
            (false, true) => {
                result.uid = result.dataset_uid.clone();
                result.source = SopClassSource::Dataset;
            }
            _ if result.dataset_uid == result.file_meta_uid => {
                result.uid = result.dataset_uid.clone();
                result.source = SopClassSource::Agreed;
            }
            _ => {
                result.mismatch = true;
                match policy {
                    SopClassPolicy::RejectMismatch => {
                        return (result, Some(Error::sentinel(Kind::SopClassMismatch)));
                    }
                    SopClassPolicy::PreferDataset => {
                        result.uid = result.dataset_uid.clone();
                        result.source = SopClassSource::Dataset;
                    }
                    SopClassPolicy::PreferFileMeta => {
                        result.uid = result.file_meta_uid.clone();
                        result.source = SopClassSource::FileMeta;
                    }
                    SopClassPolicy::Invalid(_) => {
                        return (result, Some(Error::sentinel(Kind::InvalidSopClassPolicy)));
                    }
                }
            }
        }
        (result, None)
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FileMetadata {
    pub transfer_syntax_uid: String,
    pub transfer_syntax_name: String,
    pub patient_name: String,
    pub patient_id: String,
    pub issuer_of_patient_id: Issuer,
    pub patient_birth_date: String,
    pub institution_name: String,
    pub study_date: String,
    pub study_time: String,
    pub series_date: String,
    pub series_time: String,
    pub study_description: String,
    pub modality: String,
    pub accession_number: String,
    pub series_description: String,
    pub protocol_name: String,
    pub study_instance_uid: String,
    pub series_instance_uid: String,
    pub series_number: String,
    pub sop_class_uid: String,
    pub sop_instance_uid: String,
    pub instance_number: String,
    pub study_id: String,
    pub body_part_examined: String,
    pub referring_physician_name: String,
    pub performing_physician_name: String,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SopClassPolicy {
    RejectMismatch,
    PreferDataset,
    PreferFileMeta,
    Invalid(u8),
}
impl SopClassPolicy {
    pub fn from_u8(v: u8) -> SopClassPolicy {
        match v {
            0 => SopClassPolicy::RejectMismatch,
            1 => SopClassPolicy::PreferDataset,
            2 => SopClassPolicy::PreferFileMeta,
            v => SopClassPolicy::Invalid(v),
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SopClassSource {
    #[default]
    None,
    Dataset,
    FileMeta,
    Agreed,
}
impl SopClassSource {
    pub fn as_str(self) -> &'static str {
        match self {
            SopClassSource::None => "",
            SopClassSource::Dataset => "dataset",
            SopClassSource::FileMeta => "file-meta",
            SopClassSource::Agreed => "agreed",
        }
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SopClassResolution {
    pub uid: String,
    pub source: SopClassSource,
    pub dataset_uid: String,
    pub file_meta_uid: String,
    pub mismatch: bool,
}
pub fn is_valid_uid(uid: &[u8]) -> bool {
    if uid.len() < 3 || uid.len() > 64 {
        return false;
    }
    let mut component_index = 0;
    let mut component_start = 0;
    let mut root = 0u8;
    for index in 0..=uid.len() {
        if index < uid.len() && uid[index] != b'.' {
            if !uid[index].is_ascii_digit() {
                return false;
            }
            continue;
        }
        let len = index - component_start;
        if len == 0 || len > 1 && uid[component_start] == b'0' {
            return false;
        }
        match component_index {
            0 => {
                if len != 1 || uid[component_start] > b'2' {
                    return false;
                }
                root = uid[component_start];
            }
            1 if root != b'2' => {
                let mut arc = u32::from(uid[component_start] - b'0');
                if len == 2 {
                    arc = arc * 10 + u32::from(uid[component_start + 1] - b'0');
                }
                if len > 2 || arc > 39 {
                    return false;
                }
            }
            _ => {}
        }
        component_index += 1;
        component_start = index + 1;
    }
    component_index >= 2
}
fn declared_sop_class_uid(o: &Object, tag: Tag) -> Result<String, Error> {
    let Some(element) = o.get(tag) else {
        return Ok(String::new());
    };
    let invalid = || {
        Err(Error::kind_suffix(
            Kind::InvalidSopClassUid,
            &format!(": {tag}"),
        ))
    };
    if element.vr() != Vr::UI {
        return invalid();
    }
    let Value::Raw(raw) = &element.value else {
        return invalid();
    };
    if raw.len() > 64 {
        return invalid();
    }
    let mut end = raw.len();
    while end > 0 && (raw[end - 1] == b' ' || raw[end - 1] == 0) {
        end -= 1;
    }
    let value = &raw[..end];
    if !value.is_empty() && !is_valid_uid(value) {
        return invalid();
    }
    Ok(String::from_utf8_lossy(value).into_owned())
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Issuer {
    pub local_namespace_entity_id: String,
    pub universal_entity_id: String,
    pub universal_entity_id_type: String,
}
impl Issuer {
    fn normalized(&self) -> Issuer {
        Issuer {
            local_namespace_entity_id: self.local_namespace_entity_id.trim().to_string(),
            universal_entity_id: self.universal_entity_id.trim().to_string(),
            universal_entity_id_type: self.universal_entity_id_type.trim().to_string(),
        }
    }
    pub fn is_zero(&self) -> bool {
        let i = self.normalized();
        i.local_namespace_entity_id.is_empty() && i.universal_entity_id.is_empty()
    }
    pub fn matches(&self, other: &Issuer) -> bool {
        let (left, right) = (self.normalized(), other.normalized());
        if left.is_zero() || right.is_zero() {
            return true;
        }
        if !left.universal_entity_id.is_empty() && !right.universal_entity_id.is_empty() {
            if left.universal_entity_id != right.universal_entity_id {
                return false;
            }
            return left.universal_entity_id_type.is_empty()
                || right.universal_entity_id_type.is_empty()
                || left.universal_entity_id_type == right.universal_entity_id_type;
        }
        if !left.local_namespace_entity_id.is_empty() && !right.local_namespace_entity_id.is_empty()
        {
            return left.local_namespace_entity_id == right.local_namespace_entity_id;
        }
        false
    }
    pub fn parse(value: &str) -> Issuer {
        let value = value.trim();
        if value.is_empty() {
            return Issuer::default();
        }
        let parts: Vec<&str> = value.splitn(4, '&').collect();
        let component = |i: usize| {
            if i >= parts.len() || i >= 3 {
                String::new()
            } else {
                unescape_issuer_component(parts[i])
            }
        };
        Issuer {
            local_namespace_entity_id: component(0),
            universal_entity_id: component(1),
            universal_entity_id_type: component(2),
        }
        .normalized()
    }
}
impl std::fmt::Display for Issuer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let i = self.normalized();
        if i.is_zero() {
            return Ok(());
        }
        if i.universal_entity_id.is_empty() {
            return f.write_str(&escape_issuer_component(&i.local_namespace_entity_id));
        }
        write!(
            f,
            "{}&{}&{}",
            escape_issuer_component(&i.local_namespace_entity_id),
            escape_issuer_component(&i.universal_entity_id),
            escape_issuer_component(&i.universal_entity_id_type)
        )
    }
}
fn escape_issuer_component(value: &str) -> String {
    value.replace('\\', "\\E\\").replace('&', "\\T\\")
}
fn unescape_issuer_component(value: &str) -> String {
    if !value.contains('\\') {
        return value.to_string();
    }
    let mut out = String::new();
    let mut rest = value;
    while !rest.is_empty() {
        if let Some(r) = rest.strip_prefix("\\T\\") {
            out.push('&');
            rest = r;
        } else if let Some(r) = rest.strip_prefix("\\E\\") {
            out.push('\\');
            rest = r;
        } else {
            let Some(c) = rest.chars().next() else { break };
            out.push(c);
            rest = &rest[c.len_utf8()..];
        }
    }
    out
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IdWithIssuer {
    pub id: String,
    pub type_of_patient_id: String,
    pub issuer: Issuer,
}
impl IdWithIssuer {
    pub fn matches(&self, other: &IdWithIssuer) -> bool {
        let id = self.id.trim();
        !id.is_empty() && id == other.id.trim() && self.issuer.matches(&other.issuer)
    }
}
impl std::fmt::Display for IdWithIssuer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let id = self.id.trim();
        let issuer = self.issuer.to_string();
        if issuer.is_empty() {
            f.write_str(id)
        } else {
            write!(f, "{id} ({issuer})")
        }
    }
}
pub(super) fn trimmed_string(o: &Object, tag: Tag) -> String {
    o.get_string(tag).unwrap_or_default().trim().to_string()
}
fn id_with_issuer_from(o: &Object) -> Option<IdWithIssuer> {
    let id = trimmed_string(o, Tag::new(0x0010, 0x0020));
    if id.is_empty() {
        return None;
    }
    let mut issuer = Issuer {
        local_namespace_entity_id: trimmed_string(o, Tag::new(0x0010, 0x0021)),
        ..Issuer::default()
    };
    if let Some(qualifiers) = o.get_sequence(Tag::new(0x0010, 0x0024))
        && let Some(first) = qualifiers.first()
    {
        issuer.universal_entity_id = trimmed_string(first, Tag::new(0x0040, 0x0032));
        issuer.universal_entity_id_type = trimmed_string(first, Tag::new(0x0040, 0x0033));
    }
    Some(IdWithIssuer {
        id,
        type_of_patient_id: trimmed_string(o, Tag::new(0x0010, 0x0022)),
        issuer: issuer.normalized(),
    })
}
pub fn patient_id_with_issuer(o: &Object) -> Option<IdWithIssuer> {
    id_with_issuer_from(o)
}
pub fn all_patient_ids_with_issuer(o: &Object) -> Vec<IdWithIssuer> {
    let mut ids: Vec<IdWithIssuer> = Vec::new();
    let mut add = |c: IdWithIssuer| {
        if !ids.contains(&c) {
            ids.push(c);
        }
    };
    if let Some(primary) = id_with_issuer_from(o) {
        add(primary);
    }
    for item in o.get_sequence(Tag::new(0x0010, 0x1002)).unwrap_or_default() {
        if let Some(other) = id_with_issuer_from(&item) {
            add(other);
        }
    }
    ids
}
pub fn any_patient_id_matches(left: &[IdWithIssuer], right: &[IdWithIssuer]) -> bool {
    left.iter().any(|a| right.iter().any(|b| a.matches(b)))
}
