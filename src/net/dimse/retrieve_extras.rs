#![forbid(unsafe_code)]
use super::error::{Error, Kind, Result};
use super::find_extras::{level_rank, model_levels, normalize_query_retrieve_level, unique_key};
use super::mwl::string_values;
use super::retrieve::{CMoveRequestContext, CMoveSubOperationResult};
use crate::dictionary::uid as uiddict;
use crate::net::ul::{self, Association, Ctx};
use crate::object::{self, Element, ElementHeader, Object, Tag, Value, Vr};
use crate::textfmt::quote;
use std::sync::Arc;
const TAG_QUERY_RETRIEVE_LEVEL: Tag = Tag::new(0x0008, 0x0052);
const TAG_QUERY_RETRIEVE_VIEW: Tag = Tag::new(0x0008, 0x0053);
const TAG_SPECIFIC_CHARACTER_SET: Tag = Tag::new(0x0008, 0x0005);
const MAX_ERROR_COMMENT_LENGTH: usize = 64;
const MAX_C_MOVE_SUB_OPERATIONS: usize = (1 << 16) - 1;
pub const DEFAULT_RETRIEVE_TRANSFER_SYNTAXES: [&str; 2] =
    [ul::IMPLICIT_VR_LITTLE_ENDIAN, ul::EXPLICIT_VR_LITTLE_ENDIAN];
fn keys_error(message: impl std::fmt::Display) -> Error {
    Error::wrap(Kind::CGetIdentifierKeys, message)
}
pub fn c_get_identifier_error_comment(err: Option<&Error>) -> String {
    let Some(err) = err else {
        return String::new();
    };
    let mut comment = err.text().to_string();
    let prefix = format!("{}: ", Kind::CGetIdentifierKeys.text());
    if let Some(reason) = comment.strip_prefix(&prefix) {
        comment = format!("C-GET Identifier: {reason}");
    }
    let runes: Vec<char> = comment.chars().collect();
    if runes.len() > MAX_ERROR_COMMENT_LENGTH {
        comment = runes[..MAX_ERROR_COMMENT_LENGTH].iter().collect();
    }
    comment
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CGetIdentifierKeys {
    pub level: String,
    pub patient_id: String,
    pub study_instance_uids: Vec<String>,
    pub series_instance_uids: Vec<String>,
    pub sop_instance_uids: Vec<String>,
}
pub fn parse_c_get_identifier_keys(
    model: &str,
    identifier: Option<&Object>,
    negotiated: &super::retrieve::CRetrieveExtendedNegotiation,
) -> Result<CGetIdentifierKeys> {
    let levels = model_levels(model)?;
    let Some(identifier) = identifier else {
        return Err(keys_error("missing Identifier"));
    };
    let (level, ok) = read_single_value(identifier, TAG_QUERY_RETRIEVE_LEVEL, Vr::CS);
    let level = normalize_query_retrieve_level(&level);
    if !ok || !levels.contains(&level.as_str()) {
        return Err(keys_error("missing or unsupported Query/Retrieve Level"));
    }
    if identifier.has(TAG_QUERY_RETRIEVE_VIEW) {
        let (view, valid) = read_single_value(identifier, TAG_QUERY_RETRIEVE_VIEW, Vr::CS);
        if !negotiated.enhanced_multi_frame_image_conversion
            || !valid
            || (view != "CLASSIC" && view != "ENHANCED")
        {
            return Err(keys_error("unsupported Query/Retrieve View"));
        }
    }
    let mut keys = CGetIdentifierKeys {
        level: level.clone(),
        ..Default::default()
    };
    let mut allowed = vec![
        TAG_QUERY_RETRIEVE_LEVEL,
        TAG_QUERY_RETRIEVE_VIEW,
        TAG_SPECIFIC_CHARACTER_SET,
    ];
    for key_level in levels {
        let tag = unique_key(key_level);
        if level_rank(key_level) > level_rank(&level) {
            break;
        }
        allowed.push(tag);
        if !identifier.has(tag) {
            if *key_level == level || !negotiated.relational_retrieval {
                return Err(keys_error(format!("missing {key_level} Unique Key {tag}")));
            }
            continue;
        }
        if *key_level == "PATIENT" {
            let (patient_id, valid) = read_single_value(identifier, tag, Vr::LO);
            if !valid || patient_id.contains(['*', '?']) {
                return Err(keys_error("invalid Patient ID"));
            }
            keys.patient_id = patient_id;
            continue;
        }
        let uids = read_uid_list(identifier, tag, *key_level == level)?;
        match *key_level {
            "STUDY" => keys.study_instance_uids = uids,
            "SERIES" => keys.series_instance_uids = uids,
            _ => keys.sop_instance_uids = uids,
        }
    }
    for e in identifier.elements() {
        let tag = e.tag();
        if !allowed.contains(&tag) && tag.element != 0 {
            return Err(keys_error(format!("unexpected attribute {tag}")));
        }
    }
    Ok(keys)
}
pub(crate) fn read_single_value(identifier: &Object, tag: Tag, vr: Vr) -> (String, bool) {
    let Some(e) = identifier.get(tag) else {
        return (String::new(), false);
    };
    if e.vr() != vr {
        return (String::new(), false);
    }
    let limit = if vr == Vr::LO { 4 * 64 } else { 16 };
    if object::element_length(e) > limit {
        return (String::new(), false);
    }
    let Ok(values) = identifier.lookup_strings(tag) else {
        return (String::new(), false);
    };
    if values.len() != 1 {
        return (String::new(), false);
    }
    let value = values[0].trim().to_string();
    let ok = !value.is_empty() && !value.contains('\\');
    (value, ok)
}
pub(crate) fn read_uid_list(
    identifier: &Object,
    tag: Tag,
    allow_list: bool,
) -> Result<Vec<String>> {
    let Some(e) = identifier.get(tag).filter(|e| e.vr() == Vr::UI) else {
        return Err(keys_error(format!("invalid UID element {tag}")));
    };
    let limit = if allow_list {
        MAX_C_MOVE_SUB_OPERATIONS
    } else {
        1
    };
    let multiplicity = || keys_error(format!("invalid UID multiplicity for {tag}"));
    match &e.value {
        Value::Strings(s) if s.len() > limit => return Err(multiplicity()),
        Value::Raw(raw)
            if raw.len() > 65 * limit || raw.iter().filter(|b| **b == b'\\').count() >= limit =>
        {
            return Err(multiplicity());
        }
        _ => {}
    }
    let values = string_values(e);
    if values.is_empty() || values.len() > limit {
        return Err(multiplicity());
    }
    let mut uids: Vec<String> = Vec::with_capacity(values.len());
    for v in values {
        let uid = object::transfer::normalize_uid(&v).to_string();
        if !object::is_valid_uid(uid.as_bytes()) {
            return Err(keys_error(format!("invalid UID in {tag}")));
        }
        if !uids.contains(&uid) {
            uids.push(uid);
        }
    }
    Ok(uids)
}
#[derive(Clone, Copy)]
pub struct CheckCGetIdentifier;
pub(crate) fn with_c_get_identifier_check(ctx: &Ctx) -> Ctx {
    ctx.with_value(CheckCGetIdentifier)
}
pub(crate) fn c_get_identifier_check_enabled(ctx: Option<&Ctx>) -> bool {
    ctx.is_some_and(|c| c.value::<CheckCGetIdentifier>().is_some())
}
#[derive(Debug, Clone, Default)]
pub struct RetrieveIdentifierKeys {
    pub level: String,
    pub patient_id: String,
    pub study_instance_uid: String,
    pub series_instance_uid: String,
    pub sop_instance_uid: String,
}
pub fn build_c_get_identifier(model: &str, keys: &RetrieveIdentifierKeys) -> Result<Object> {
    build_retrieve_identifier(model, keys)
}
pub fn build_c_move_identifier(model: &str, keys: &RetrieveIdentifierKeys) -> Result<Object> {
    build_retrieve_identifier(model, keys)
}
fn key_element(tag: Tag, vr: Vr, value: &str) -> Element {
    Element {
        header: ElementHeader {
            tag,
            vr,
            length: 0,
            length_set: false,
        },
        value: Value::Strings(vec![value.to_string()]),
    }
}
fn build_retrieve_identifier(model: &str, keys: &RetrieveIdentifierKeys) -> Result<Object> {
    let levels = model_levels(model)?;
    let level = normalize_query_retrieve_level(&keys.level);
    if !levels.contains(&level.as_str()) {
        return Err(Error::new(format!(
            "dicom dimse: model {model} does not support QueryRetrieveLevel {}",
            quote(&level)
        )));
    }
    let mut elements = vec![key_element(TAG_QUERY_RETRIEVE_LEVEL, Vr::CS, &level)];
    let norm = |s: &str| object::transfer::normalize_uid(s).to_string();
    for (key_level, keyword, value) in [
        ("PATIENT", "PatientID", keys.patient_id.trim().to_string()),
        ("STUDY", "StudyInstanceUID", norm(&keys.study_instance_uid)),
        (
            "SERIES",
            "SeriesInstanceUID",
            norm(&keys.series_instance_uid),
        ),
        ("IMAGE", "SOPInstanceUID", norm(&keys.sop_instance_uid)),
    ] {
        if !levels.contains(&key_level) || level_rank(key_level) > level_rank(&level) {
            if !value.is_empty() {
                return Err(Error::new(format!(
                    "dicom dimse: unexpected {keyword} for {model} {level} retrieval"
                )));
            }
            continue;
        }
        if value.is_empty() {
            return Err(Error::new(format!(
                "dicom dimse: missing {keyword} for {model} {level} retrieval"
            )));
        }
        let tag = unique_key(key_level);
        if key_level == "PATIENT" {
            if value.chars().count() > 64
                || value.contains(['*', '?', '\\'])
                || value.chars().any(char::is_control)
            {
                return Err(Error::new("dicom dimse: invalid PatientID for retrieval"));
            }
            if !value.is_ascii() {
                elements.push(key_element(
                    TAG_SPECIFIC_CHARACTER_SET,
                    Vr::CS,
                    "ISO_IR 192",
                ));
            }
            elements.push(key_element(tag, Vr::LO, &value));
            continue;
        }
        if !object::is_valid_uid(value.as_bytes()) {
            return Err(Error::new(format!(
                "dicom dimse: invalid {keyword} for retrieval"
            )));
        }
        elements.push(key_element(tag, Vr::UI, &value));
    }
    Ok(Object::from_elements(elements))
}
fn presentation_context_for(abstract_uid: &str, syntaxes: &[String]) -> ul::PresentationContext {
    let s: Vec<&str> = syntaxes.iter().map(String::as_str).collect();
    ul::PresentationContext::new(abstract_uid, &s)
}
fn defaults() -> Vec<String> {
    DEFAULT_RETRIEVE_TRANSFER_SYNTAXES
        .iter()
        .map(|s| s.to_string())
        .collect()
}
pub fn study_root_move_presentation_context() -> ul::PresentationContext {
    presentation_context_for(super::retrieve::STUDY_ROOT_MOVE_SOP_CLASS_UID, &defaults())
}
pub fn patient_root_move_presentation_context() -> ul::PresentationContext {
    presentation_context_for(
        super::retrieve::PATIENT_ROOT_MOVE_SOP_CLASS_UID,
        &defaults(),
    )
}
pub fn study_root_get_presentation_context() -> ul::PresentationContext {
    presentation_context_for(super::retrieve::STUDY_ROOT_GET_SOP_CLASS_UID, &defaults())
}
pub fn patient_root_get_presentation_context() -> ul::PresentationContext {
    presentation_context_for(super::retrieve::PATIENT_ROOT_GET_SOP_CLASS_UID, &defaults())
}
pub type CGetProposal = (Vec<ul::PresentationContext>, Vec<ul::RoleSelectionItem>);
pub fn build_c_get_presentation_contexts(
    model: &str,
    storage_sop_class_uids: &[String],
    storage_transfer_syntax_uids: &[String],
) -> Result<CGetProposal> {
    match model {
        "STUDY_ROOT" => build_study_root_c_get_presentation_contexts(
            storage_sop_class_uids,
            storage_transfer_syntax_uids,
        ),
        "PATIENT_ROOT" => build_c_get_contexts(
            patient_root_get_presentation_context(),
            storage_sop_class_uids,
            storage_transfer_syntax_uids,
        ),
        _ => Err(Error::new(format!(
            "dicom dimse: unsupported C-GET model {}",
            quote(model)
        ))),
    }
}
pub fn build_study_root_c_get_presentation_contexts(
    storage_sop_class_uids: &[String],
    storage_transfer_syntax_uids: &[String],
) -> Result<CGetProposal> {
    build_c_get_contexts(
        study_root_get_presentation_context(),
        storage_sop_class_uids,
        storage_transfer_syntax_uids,
    )
}
fn build_c_get_contexts(
    get: ul::PresentationContext,
    classes: &[String],
    syntaxes: &[String],
) -> Result<CGetProposal> {
    let max = ul::MAX_PRESENTATION_CONTEXTS;
    if classes.is_empty() || classes.len() >= max {
        return Err(Error::new(format!(
            "dicom dimse: C-GET needs 1 to {} Storage SOP Classes",
            max - 1
        )));
    }
    let mut longest = 0;
    let mut seen: Vec<&str> = Vec::new();
    for (i, class) in classes.iter().enumerate() {
        let known = uiddict::by_uid(class).is_some();
        if !object::is_valid_uid(class.as_bytes()) || (known && !uiddict::is_storage(class)) {
            return Err(Error::new(format!(
                "dicom dimse: invalid C-GET Storage SOP Class UID at index {i}"
            )));
        }
        if seen.contains(&class.as_str()) {
            return Err(Error::new(format!(
                "dicom dimse: duplicate C-GET Storage SOP Class UID at index {i}"
            )));
        }
        seen.push(class);
        longest = longest.max(class.len());
    }
    validate_c_get_transfer_syntax_uids(&get.transfer_syntax_uids, get.abstract_syntax_uid.len())?;
    validate_c_get_transfer_syntax_uids(syntaxes, longest)?;
    let mut contexts = vec![get];
    let mut roles = Vec::new();
    for class in classes {
        contexts.push(presentation_context_for(class, syntaxes));
        roles.push(ul::RoleSelectionItem {
            sop_class_uid: class.clone(),
            scu_role: false,
            scp_role: true,
        });
    }
    Ok((contexts, roles))
}
pub fn build_c_get_presentation_contexts_with_alternatives(
    model: &str,
    classes: &[String],
    syntaxes: &[String],
    alternatives: &[String],
) -> Result<CGetProposal> {
    let (contexts, roles) = build_c_get_presentation_contexts(model, classes, syntaxes)?;
    append_c_get_alternative_contexts(contexts, roles, syntaxes, alternatives)
}
pub fn build_study_root_c_get_alternative_presentation_contexts(
    classes: &[String],
    syntaxes: &[String],
    alternatives: &[String],
) -> Result<CGetProposal> {
    let (contexts, roles) = build_study_root_c_get_presentation_contexts(classes, syntaxes)?;
    append_c_get_alternative_contexts(contexts, roles, syntaxes, alternatives)
}
fn append_c_get_alternative_contexts(
    contexts: Vec<ul::PresentationContext>,
    roles: Vec<ul::RoleSelectionItem>,
    syntaxes: &[String],
    alternatives: &[String],
) -> Result<CGetProposal> {
    if alternatives.is_empty() {
        return Ok((contexts, roles));
    }
    if 1 + alternatives.len() > (ul::MAX_PRESENTATION_CONTEXTS - 2) / roles.len() {
        return Err(Error::new(
            "dicom dimse: C-GET Storage SOP Classes and transfer syntax alternatives exceed presentation context capacity",
        ));
    }
    let all: Vec<String> = syntaxes.iter().chain(alternatives).cloned().collect();
    validate_c_get_transfer_syntax_uids(&all, 64)?;
    let mut proposals = vec![contexts[0].clone()];
    for storage in &contexts[1..] {
        for alt in alternatives {
            proposals.push(presentation_context_for(
                &storage.abstract_syntax_uid,
                std::slice::from_ref(alt),
            ));
        }
        proposals.push(storage.clone());
    }
    Ok((proposals, roles))
}
fn validate_c_get_transfer_syntax_uids(syntaxes: &[String], class_uid_length: usize) -> Result<()> {
    if syntaxes.is_empty() {
        return Err(Error::new("dicom dimse: C-GET requires transfer syntaxes"));
    }
    let mut seen: Vec<&str> = Vec::new();
    let mut wire = 8 + class_uid_length;
    for (i, s) in syntaxes.iter().enumerate() {
        let entry = uiddict::by_uid(s);
        let wrong_type = entry.is_some_and(|e| e.typ != uiddict::Type::TRANSFER_SYNTAX);
        if !object::is_valid_uid(s.as_bytes()) || wrong_type {
            return Err(Error::new(format!(
                "dicom dimse: invalid C-GET transfer syntax UID at index {i}"
            )));
        }
        if seen.contains(&s.as_str()) {
            return Err(Error::new(format!(
                "dicom dimse: duplicate C-GET transfer syntax UID at index {i}"
            )));
        }
        seen.push(s);
        wire += 4 + s.len();
        if wire > u16::MAX as usize {
            return Err(Error::new(
                "dicom dimse: C-GET transfer syntax offers exceed presentation context item capacity",
            ));
        }
    }
    Ok(())
}
#[derive(Clone, Default)]
pub struct RetrieveDialOptions {
    pub transport: ul::TransportOptions,
    #[cfg(feature = "tls")]
    pub tls_config: Option<ul::TlsClientConfig>,
    pub address: String,
    pub called_ae_title: String,
    pub calling_ae_title: String,
    pub max_pdu: u32,
    pub contexts: Vec<ul::PresentationContext>,
    pub role_selections: Vec<ul::RoleSelectionItem>,
    pub implementation_class_uid: String,
    pub implementation_version_name: String,
    pub user_identity: Option<ul::UserIdentityRequest>,
}
pub fn dial_retrieve_scu(ctx: &Ctx, opts: &RetrieveDialOptions) -> Result<Association> {
    let address = opts.address.trim();
    if address.is_empty() {
        return Err(Error::new("dicom dimse: missing address"));
    }
    if opts.contexts.is_empty() {
        return Err(Error::new("dicom dimse: no presentation contexts provided"));
    }
    let dial = ul::DialOptions {
        called_ae_title: opts.called_ae_title.clone(),
        calling_ae_title: opts.calling_ae_title.clone(),
        max_pdu: opts.max_pdu,
        contexts: opts.contexts.clone(),
        role_selections: opts.role_selections.clone(),
        implementation_class_uid: opts.implementation_class_uid.clone(),
        implementation_version_name: opts.implementation_version_name.clone(),
        user_identity: opts.user_identity.clone(),
        transport: opts.transport.clone(),
        #[cfg(feature = "tls")]
        tls_config: opts.tls_config.clone(),
        ..Default::default()
    };
    ul::dial_context(ctx, address, dial).map_err(|e| {
        Error::from(e).prefixed(format!(
            "dicom dimse: dial retrieve SCU address={} calledAET={} callingAET={} maxPDU={} contexts={}",
            quote(address),
            quote(&opts.called_ae_title),
            quote(&opts.calling_ae_title),
            opts.max_pdu,
            opts.contexts.len()
        ))
    })
}
#[derive(Debug, Clone, Default)]
pub struct CMoveSubOperationDescriptor {
    pub affected_sop_class_uid: String,
    pub affected_sop_instance_uid: String,
}
pub type CMoveBatchYield<'a> = dyn FnMut(CMoveSubOperationResult) -> Result<()> + 'a;
pub type CMoveBatchRun = Arc<dyn Fn(&Ctx, &mut CMoveBatchYield<'_>) -> Result<()> + Send + Sync>;
#[derive(Clone, Default)]
pub struct CMoveBatch {
    pub sub_operations: Vec<CMoveSubOperationDescriptor>,
    pub run: Option<CMoveBatchRun>,
}
pub type CMoveBatchHandler =
    Arc<dyn Fn(&Ctx, &CMoveRequestContext) -> Result<CMoveBatch> + Send + Sync>;
pub(crate) fn batch_contract(message: impl std::fmt::Display) -> Error {
    Error::wrap(Kind::CMoveBatchContract, message)
}
pub(crate) fn validate_c_move_batch(batch: &CMoveBatch) -> Result<()> {
    let n = batch.sub_operations.len();
    if n > MAX_C_MOVE_SUB_OPERATIONS {
        return Err(batch_contract(format!(
            "too many sub-operations: {n} exceeds {MAX_C_MOVE_SUB_OPERATIONS}"
        )));
    }
    if batch.run.is_none() {
        return Err(batch_contract("missing Run callback"));
    }
    for (i, d) in batch.sub_operations.iter().enumerate() {
        if !d.affected_sop_class_uid.is_empty()
            && !object::is_valid_uid(d.affected_sop_class_uid.as_bytes())
        {
            return Err(batch_contract(format!(
                "sub-operation {i} has invalid Affected SOP Class UID"
            )));
        }
        if !object::is_valid_uid(d.affected_sop_instance_uid.as_bytes()) {
            return Err(batch_contract(format!(
                "sub-operation {i} has invalid Affected SOP Instance UID"
            )));
        }
    }
    Ok(())
}
