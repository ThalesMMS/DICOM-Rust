#![forbid(unsafe_code)]
use super::admission::{AdmissionDecider, AdmissionHandler, RejectedAssociation};
use super::aetitle::{equal_ae_titles, normalize_ae_title, validate_ae_title};
use super::called_ae::CalledAeRegistry;
use super::common_ext::{
    CommonExtendedNegotiationHandler, common_extended_negotiation_from_user_info,
    related_general_sop_class_for,
};
use super::conn::{Ctx, Nanos};
use super::error::{Error, Kind, Result};
use super::outcome::{PresentationContextOutcome, presentation_context_outcomes_from_results};
use super::pdu::*;
use super::rules::{
    PresentationContextRule, presentation_context_rule, requested_role_for, rule_role_accepts,
};
use crate::textfmt::quote;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AcceptedContext {
    pub id: u8,
    pub abstract_syntax_uid: String,
    pub transfer_syntax_uid: String,
    pub related_general_sop_class_uid: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UserIdentityRequest {
    pub identity_type: u8,
    pub positive_response_requested: bool,
    pub primary_field: Vec<u8>,
    pub secondary_field: Vec<u8>,
}
impl UserIdentityRequest {
    pub fn username(username: &str, positive_response_requested: bool) -> Self {
        UserIdentityRequest {
            identity_type: USER_IDENTITY_USERNAME,
            positive_response_requested,
            primary_field: username.as_bytes().to_vec(),
            secondary_field: Vec::new(),
        }
    }
    pub fn username_password(
        username: &str,
        password: &str,
        positive_response_requested: bool,
    ) -> Self {
        UserIdentityRequest {
            identity_type: USER_IDENTITY_USERNAME_PASSWORD,
            positive_response_requested,
            primary_field: username.as_bytes().to_vec(),
            secondary_field: password.as_bytes().to_vec(),
        }
    }
    pub fn token(identity_type: u8, token: &[u8], positive_response_requested: bool) -> Self {
        UserIdentityRequest {
            identity_type,
            positive_response_requested,
            primary_field: token.to_vec(),
            secondary_field: Vec::new(),
        }
    }
}
pub type UserIdentityAction = u8;
pub const USER_IDENTITY_ACTION_REJECT: UserIdentityAction = 0;
pub const USER_IDENTITY_ACTION_ACCEPT: UserIdentityAction = 1;
pub const USER_IDENTITY_ACTION_IGNORE: UserIdentityAction = 2;
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UserIdentityResult {
    pub action: UserIdentityAction,
    pub server_response: Vec<u8>,
}
pub type UserIdentityHandler = Arc<
    dyn Fn(&UserIdentityRequest) -> std::result::Result<UserIdentityResult, crate::BoxError>
        + Send
        + Sync,
>;
pub type ExtendedNegotiationAction = u8;
pub const EXTENDED_NEGOTIATION_ACTION_REJECT: ExtendedNegotiationAction = 0;
pub const EXTENDED_NEGOTIATION_ACTION_ACCEPT: ExtendedNegotiationAction = 1;
pub const EXTENDED_NEGOTIATION_ACTION_IGNORE: ExtendedNegotiationAction = 2;
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ExtendedNegotiationResult {
    pub action: ExtendedNegotiationAction,
    pub data: Vec<u8>,
}
pub type ExtendedNegotiationHandler = Arc<
    dyn Fn(
            &SopClassExtendedNegotiationItem,
        ) -> std::result::Result<ExtendedNegotiationResult, crate::BoxError>
        + Send
        + Sync,
>;
pub type RejectionCallback = Arc<dyn Fn(&RejectedAssociation) + Send + Sync>;
#[derive(Clone, Default)]
pub struct DialOptions {
    pub called_ae_title: String,
    pub calling_ae_title: String,
    pub max_pdu: u32,
    pub context: Option<Ctx>,
    pub implementation_class_uid: String,
    pub implementation_version_name: String,
    pub protocol_version: u16,
    pub contexts: Vec<PresentationContext>,
    pub transfer_syntax_allowlist: Vec<String>,
    pub application_context_name: String,
    pub user_identity: Option<UserIdentityRequest>,
    pub role_selections: Vec<RoleSelectionItem>,
    pub extended_negotiation: Vec<SopClassExtendedNegotiationItem>,
    pub common_extended_negotiation: Vec<SopClassCommonExtendedNegotiationItem>,
    pub asynchronous_operations_window: Option<AsynchronousOperationsWindow>,
    pub negotiation_timeout: Nanos,
    pub idle_timeout: Nanos,
    pub read_progress_timeout: Nanos,
    pub write_progress_timeout: Nanos,
    pub release_timeout: Nanos,
    pub artim_timeout: Nanos,
    pub max_send_pdu: u32,
    pub max_association_pdu: u32,
    #[cfg(feature = "tls")]
    pub tls_config: Option<super::tls_profile::TlsClientConfig>,
    pub observability: Option<super::observability::ObservabilityOptions>,
    pub transport: super::transport::TransportOptions,
    pub socket_buffers: super::socket_buffers::SocketBuffers,
}
#[cfg(feature = "tls")]
pub(crate) type TlsHandshakeFailureCallback =
    Arc<dyn Fn(&super::tls_conn::TlsHandshakeFailure) + Send + Sync>;
#[derive(Clone, Default)]
pub struct AcceptOptions {
    pub called_ae_registry: Option<Arc<CalledAeRegistry>>,
    pub ae_title: String,
    pub max_pdu: u32,
    pub supported_abstract_syntaxes: Vec<String>,
    pub supported_transfer_syntaxes: Vec<String>,
    pub prefer_proposed_transfer_syntax_order: bool,
    pub accept_any_abstract_syntax: bool,
    pub presentation_context_rules: Vec<PresentationContextRule>,
    pub called_ae_presentation_context_rules: Vec<(String, Vec<PresentationContextRule>)>,
    pub always_accept_default_role: bool,
    pub require_matching_called_ae: bool,
    pub admission: Option<AdmissionHandler>,
    pub admission_decider: Option<AdmissionDecider>,
    pub context: Option<Ctx>,
    pub implementation_class_uid: String,
    pub implementation_version_name: String,
    pub user_identity_handler: Option<UserIdentityHandler>,
    pub role_selections: Vec<RoleSelectionItem>,
    pub extended_negotiation_handler: Option<ExtendedNegotiationHandler>,
    pub common_extended_negotiation_handler: Option<CommonExtendedNegotiationHandler>,
    pub asynchronous_operations_window: Option<AsynchronousOperationsWindow>,
    pub on_association_rejected: Option<Arc<dyn Fn() + Send + Sync>>,
    pub on_association_rejection: Option<RejectionCallback>,
    pub negotiation_timeout: Nanos,
    pub idle_timeout: Nanos,
    pub read_progress_timeout: Nanos,
    pub write_progress_timeout: Nanos,
    pub release_timeout: Nanos,
    pub artim_timeout: Nanos,
    pub max_send_pdu: u32,
    pub max_association_pdu: u32,
    #[doc(hidden)]
    pub rejected_request: Option<AssociationRq>,
    pub observability: Option<super::observability::ObservabilityOptions>,
    #[cfg(feature = "tls")]
    pub tls_config: Option<super::tls_profile::TlsServerConfig>,
    #[cfg(feature = "tls")]
    pub on_tls_handshake_failure: Option<TlsHandshakeFailureCallback>,
}
pub(crate) fn normalize_dial_options(mut opts: DialOptions) -> DialOptions {
    if opts.called_ae_title.is_empty() {
        opts.called_ae_title = "ANY-SCP".into();
    }
    if opts.calling_ae_title.is_empty() {
        opts.calling_ae_title = "THIS-SCU".into();
    }
    if opts.application_context_name.is_empty() {
        opts.application_context_name = APPLICATION_CONTEXT_NAME.into();
    }
    if opts.implementation_class_uid.is_empty() {
        opts.implementation_class_uid = IMPLEMENTATION_CLASS_UID.into();
    }
    if opts.implementation_version_name.is_empty() {
        opts.implementation_version_name = IMPLEMENTATION_VERSION_NAME.into();
    }
    if opts.protocol_version == 0 {
        opts.protocol_version = DEFAULT_PROTOCOL_VERSION;
    }
    if opts.max_pdu == 0 {
        opts.max_pdu = DEFAULT_MAX_PDU;
    }
    if opts.context.is_none() {
        opts.context = Some(Ctx::background());
    }
    opts
}
pub(crate) fn normalize_accept_options(mut opts: AcceptOptions) -> AcceptOptions {
    if opts.ae_title.is_empty() {
        opts.ae_title = "THIS-SCP".into();
    }
    if opts.max_pdu == 0 {
        opts.max_pdu = DEFAULT_MAX_PDU;
    }
    if opts.context.is_none() {
        opts.context = Some(Ctx::background());
    }
    if opts.implementation_class_uid.is_empty() {
        opts.implementation_class_uid = IMPLEMENTATION_CLASS_UID.into();
    }
    if opts.implementation_version_name.is_empty() {
        opts.implementation_version_name = IMPLEMENTATION_VERSION_NAME.into();
    }
    opts
}
pub(crate) fn validate_operational_timeouts(
    negotiation: Nanos,
    idle: Nanos,
    read_progress: Nanos,
    write_progress: Nanos,
    release: Nanos,
) -> Result<()> {
    let checks = [
        (negotiation, "negotiation"),
        (idle, "idle"),
        (read_progress, "read progress"),
        (write_progress, "write progress"),
        (release, "release"),
    ];
    for (value, name) in checks {
        if value.is_negative() {
            return Err(Error::other(format!(
                "dicom ul: {name} timeout must not be negative"
            )));
        }
    }
    Ok(())
}
pub(crate) fn filter_transfer_syntax_uids(
    proposals: &[String],
    allowlist: &[String],
) -> Vec<String> {
    if allowlist.is_empty() {
        return proposals.to_vec();
    }
    let norm = |s: &str| s.trim_end_matches([' ', '\0']).to_string();
    proposals
        .iter()
        .filter(|p| allowlist.iter().any(|a| norm(p) == norm(a)))
        .cloned()
        .collect()
}
pub(crate) fn apply_allowlist(opts: &mut DialOptions) -> Result<()> {
    if opts.transfer_syntax_allowlist.is_empty() {
        return Ok(());
    }
    for context in opts.contexts.iter_mut() {
        let allowed = filter_transfer_syntax_uids(
            &context.transfer_syntax_uids,
            &opts.transfer_syntax_allowlist,
        );
        if allowed.is_empty() {
            return Err(Error::wrap(
                Kind::UnsupportedTransferSyntax,
                format!(
                    "transfer syntax allowlist excludes all proposals for abstract syntax {}",
                    quote(&context.abstract_syntax_uid)
                ),
            ));
        }
        context.transfer_syntax_uids = allowed;
    }
    Ok(())
}
pub(crate) fn proposed_contexts(
    contexts: &[PresentationContext],
) -> Result<Vec<PresentationContextProposed>> {
    if contexts.is_empty() {
        return Err(Error::wrap(Kind::MissingPduField, "presentation context"));
    }
    if contexts.len() > MAX_PRESENTATION_CONTEXTS {
        return Err(Error::wrap(
            Kind::InvalidPcid,
            format!(
                "presentation context count {} exceeds {MAX_PRESENTATION_CONTEXTS}",
                contexts.len()
            ),
        ));
    }
    let mut proposed = Vec::with_capacity(contexts.len());
    for (i, pc) in contexts.iter().enumerate() {
        let mut pc = pc.clone();
        pc.id = (2 * i + 1) as u8;
        if pc.abstract_syntax_uid.is_empty() {
            return Err(Error::wrap(Kind::MissingPduField, "abstract syntax"));
        }
        if pc.transfer_syntax_uids.is_empty() {
            return Err(Error::wrap(Kind::MissingPduField, "transfer syntax"));
        }
        proposed.push(pc);
    }
    Ok(proposed)
}
pub(crate) fn validate_dial_options(opts: &DialOptions) -> Result<()> {
    let opts = normalize_dial_options(opts.clone());
    validate_operational_timeouts(
        opts.negotiation_timeout,
        opts.idle_timeout,
        opts.read_progress_timeout,
        opts.write_progress_timeout,
        opts.release_timeout,
    )?;
    opts.transport.validate_with(&opts.socket_buffers)?;
    opts.socket_buffers.validate()?;
    if opts.called_ae_title.len() > MAX_AE_TITLE_LENGTH {
        return Err(Error::wrap(
            Kind::InvalidAeTitle,
            format!(
                "called AE title length {} exceeds {MAX_AE_TITLE_LENGTH}",
                opts.called_ae_title.len()
            ),
        ));
    }
    if opts.calling_ae_title.len() > MAX_AE_TITLE_LENGTH {
        return Err(Error::wrap(
            Kind::InvalidAeTitle,
            format!(
                "calling AE title length {} exceeds {MAX_AE_TITLE_LENGTH}",
                opts.calling_ae_title.len()
            ),
        ));
    }
    validate_ae_title(&opts.called_ae_title).map_err(|e| e.prefixed("called AE title"))?;
    validate_ae_title(&opts.calling_ae_title).map_err(|e| e.prefixed("calling AE title"))?;
    if let Some(identity) = &opts.user_identity {
        validate_user_identity_request(identity)?;
    }
    let proposed = proposed_contexts(&opts.contexts)?;
    let classes: HashSet<&str> = proposed
        .iter()
        .map(|pc| pc.abstract_syntax_uid.as_str())
        .collect();
    for role in &opts.role_selections {
        validate_role_selection_item(role)?;
        if !classes.contains(role.sop_class_uid.as_str()) {
            return Err(Error::wrap(
                Kind::InvalidUserItem,
                format!(
                    "role selection SOP class {} was not proposed",
                    quote(&role.sop_class_uid)
                ),
            ));
        }
    }
    for item in &opts.extended_negotiation {
        validate_sop_class_extended_negotiation_item(item)?;
        if !classes.contains(item.sop_class_uid.as_str()) {
            return Err(Error::wrap(
                Kind::InvalidUserItem,
                format!(
                    "extended negotiation SOP class {} was not proposed",
                    quote(&item.sop_class_uid)
                ),
            ));
        }
    }
    let common_items: Vec<UserItem> = opts
        .common_extended_negotiation
        .iter()
        .cloned()
        .map(UserItem::CommonExtendedNegotiation)
        .collect();
    common_extended_negotiation_from_user_info(&common_items)?;
    for item in &opts.common_extended_negotiation {
        if !classes.contains(item.sop_class_uid.as_str()) {
            return Err(Error::wrap(
                Kind::InvalidUserItem,
                format!(
                    "common extended negotiation SOP class {} was not proposed",
                    quote(&item.sop_class_uid)
                ),
            ));
        }
    }
    Ok(())
}
fn implementation_user_info(max_pdu: u32, class_uid: &str, version_name: &str) -> Vec<UserItem> {
    let class_uid = if class_uid.is_empty() {
        IMPLEMENTATION_CLASS_UID
    } else {
        class_uid
    };
    let version_name = if version_name.is_empty() {
        IMPLEMENTATION_VERSION_NAME
    } else {
        version_name
    };
    vec![
        UserItem::MaxLength(max_pdu),
        UserItem::ImplementationClassUid(class_uid.to_string()),
        UserItem::ImplementationVersionName(version_name.to_string()),
    ]
}
pub(crate) fn build_associate_rq(opts: &DialOptions) -> AssociationRq {
    let opts = normalize_dial_options(opts.clone());
    let proposed = proposed_contexts(&opts.contexts).unwrap_or_default();
    let mut items = implementation_user_info(
        opts.max_pdu,
        &opts.implementation_class_uid,
        &opts.implementation_version_name,
    );
    if let Some(w) = opts.asynchronous_operations_window {
        items.push(UserItem::AsynchronousOperations(w));
    }
    for item in &opts.extended_negotiation {
        items.push(UserItem::ExtendedNegotiation(item.clone()));
    }
    for item in &opts.common_extended_negotiation {
        items.push(UserItem::CommonExtendedNegotiation(item.clone()));
    }
    for role in &opts.role_selections {
        items.push(UserItem::RoleSelection(role.clone()));
    }
    if let Some(identity) = &opts.user_identity {
        items.push(UserItem::UserIdentity(user_identity_item(identity)));
    }
    AssociationRq {
        protocol_version: opts.protocol_version,
        called_ae_title: normalize_ae_title(&opts.called_ae_title).to_string(),
        calling_ae_title: normalize_ae_title(&opts.calling_ae_title).to_string(),
        application_context_name: opts.application_context_name.clone(),
        presentation_contexts: proposed,
        user_info: Some(items),
    }
}
#[allow(clippy::too_many_arguments)]
pub(crate) fn accept_user_info(
    max_pdu: u32,
    class_uid: &str,
    version_name: &str,
    identity_req: Option<&UserIdentityRequest>,
    identity_result: &UserIdentityResult,
    roles: &[RoleSelectionItem],
    extended: &[SopClassExtendedNegotiationItem],
    asynchronous: Option<AsynchronousOperationsWindow>,
) -> Vec<UserItem> {
    let mut items = implementation_user_info(max_pdu, class_uid, version_name);
    if let Some(w) = asynchronous {
        items.push(UserItem::AsynchronousOperations(w));
    }
    for item in extended {
        items.push(UserItem::ExtendedNegotiation(item.clone()));
    }
    for role in roles {
        items.push(UserItem::RoleSelection(role.clone()));
    }
    if let Some(req) = identity_req
        && req.positive_response_requested
        && identity_result.action == USER_IDENTITY_ACTION_ACCEPT
    {
        items.push(UserItem::UserIdentityResponse(
            identity_result.server_response.clone(),
        ));
    }
    items
}
pub(crate) fn reject_association_request(
    rq: &AssociationRq,
    opts: &AcceptOptions,
) -> Option<AssociationRj> {
    if rq.protocol_version != DEFAULT_PROTOCOL_VERSION {
        return Some(AssociationRj {
            result: ASSOCIATE_RJ_RESULT_PERMANENT,
            source: ASSOCIATE_RJ_SOURCE_SERVICE_PROVIDER_ACSE,
            reason: ASSOCIATE_RJ_REASON_PROTOCOL_VERSION_NOT_SUPPORTED,
        });
    }
    if rq.application_context_name != APPLICATION_CONTEXT_NAME {
        return Some(AssociationRj {
            result: ASSOCIATE_RJ_RESULT_PERMANENT,
            source: ASSOCIATE_RJ_SOURCE_SERVICE_USER,
            reason: ASSOCIATE_RJ_REASON_APPLICATION_CONTEXT_NAME_NOT_SUPPORTED,
        });
    }
    if validate_ae_title(&rq.calling_ae_title).is_err() {
        return Some(super::admission::reject_calling_ae_title());
    }
    if validate_ae_title(&rq.called_ae_title).is_err() {
        return Some(super::admission::reject_called_ae_title());
    }
    if opts.require_matching_called_ae && !equal_ae_titles(&rq.called_ae_title, &opts.ae_title) {
        return Some(AssociationRj {
            result: ASSOCIATE_RJ_RESULT_PERMANENT,
            source: ASSOCIATE_RJ_SOURCE_SERVICE_USER,
            reason: ASSOCIATE_RJ_REASON_CALLED_AE_TITLE_NOT_RECOGNIZED,
        });
    }
    None
}
pub(crate) fn invalid_request_ae_title(rq: &AssociationRq) -> Option<Error> {
    if let Err(e) = validate_ae_title(&rq.calling_ae_title) {
        return Some(e.prefixed("calling AE title"));
    }
    if let Err(e) = validate_ae_title(&rq.called_ae_title) {
        return Some(e.prefixed("called AE title"));
    }
    None
}
fn contains(values: &[String], value: &str) -> bool {
    values.iter().any(|v| v == value)
}
pub(crate) fn negotiate_presentation_contexts_with_roles(
    requested: &[PresentationContextProposed],
    opts: &AcceptOptions,
    requested_roles: &[RoleSelectionItem],
    common: &[SopClassCommonExtendedNegotiationItem],
) -> (Vec<PresentationContextResult>, Vec<AcceptedContext>) {
    let mut results = Vec::with_capacity(requested.len());
    let mut accepted = Vec::with_capacity(requested.len());
    for pc in requested {
        let mut transfer_syntax = IMPLICIT_VR_LITTLE_ENDIAN.to_string();
        let mut result = PRESENTATION_CONTEXT_ACCEPTANCE;
        let mut settings_sop_class = pc.abstract_syntax_uid.clone();
        let mut related_general = String::new();
        let mut abstract_supported = opts.accept_any_abstract_syntax
            || contains(&opts.supported_abstract_syntaxes, &pc.abstract_syntax_uid);
        if presentation_context_rule(opts, &pc.abstract_syntax_uid).is_some() {
            abstract_supported = true;
        }
        if !abstract_supported
            && let Some(general) =
                related_general_sop_class_for(&pc.abstract_syntax_uid, common, opts)
        {
            settings_sop_class = general.clone();
            related_general = general;
            abstract_supported = true;
        }
        let rule = presentation_context_rule(opts, &settings_sop_class);
        let supported_ts = match rule {
            Some(r) if !r.transfer_syntaxes.is_empty() => &r.transfer_syntaxes,
            _ => &opts.supported_transfer_syntaxes,
        };
        if !abstract_supported {
            result = PRESENTATION_CONTEXT_ABSTRACT_SYNTAX_NOT_SUPPORTED;
        } else if let Some(ts) = choose_server_transfer_syntax(
            &pc.transfer_syntax_uids,
            supported_ts,
            opts.prefer_proposed_transfer_syntax_order,
        ) {
            transfer_syntax = ts;
            if let Some(r) = rule
                && let Some(roles) = r.roles
                && !rule_role_accepts(
                    roles,
                    requested_role_for(requested_roles, &pc.abstract_syntax_uid),
                    opts.always_accept_default_role,
                )
            {
                result = PRESENTATION_CONTEXT_USER_REJECTION;
            }
        } else {
            result = PRESENTATION_CONTEXT_TRANSFER_SYNTAXES_NOT_SUPPORTED;
        }
        results.push(PresentationContextResult {
            id: pc.id,
            result,
            transfer_syntax_uid: transfer_syntax.clone(),
        });
        if result == PRESENTATION_CONTEXT_ACCEPTANCE {
            accepted.push(AcceptedContext {
                id: pc.id,
                abstract_syntax_uid: pc.abstract_syntax_uid.clone(),
                transfer_syntax_uid: transfer_syntax,
                related_general_sop_class_uid: related_general,
            });
        }
    }
    (results, accepted)
}
pub(crate) fn choose_server_transfer_syntax(
    requested: &[String],
    supported: &[String],
    proposer_order: bool,
) -> Option<String> {
    if requested.is_empty() {
        return None;
    }
    if supported.is_empty() {
        return Some(requested[0].clone());
    }
    if proposer_order {
        return requested.iter().find(|ts| contains(supported, ts)).cloned();
    }
    supported.iter().find(|ts| contains(requested, ts)).cloned()
}
pub(crate) fn asynchronous_operations_window_from_user_info(
    items: &[UserItem],
) -> Result<Option<AsynchronousOperationsWindow>> {
    let mut found = None;
    for item in items {
        if let UserItem::AsynchronousOperations(w) = item {
            if found.is_some() {
                return Err(Error::wrap(
                    Kind::InvalidUserItem,
                    "duplicate asynchronous operations window",
                ));
            }
            found = Some(*w);
        }
    }
    Ok(found)
}
pub(crate) fn negotiate_asynchronous_operations_window(
    requestor: AsynchronousOperationsWindow,
    acceptor_local: AsynchronousOperationsWindow,
) -> AsynchronousOperationsWindow {
    AsynchronousOperationsWindow {
        maximum_invoked: negotiate_limit(
            requestor.maximum_invoked,
            acceptor_local.maximum_performed,
        ),
        maximum_performed: negotiate_limit(
            requestor.maximum_performed,
            acceptor_local.maximum_invoked,
        ),
    }
}
fn negotiate_limit(offered: u16, capacity: u16) -> u16 {
    if offered == 0 {
        capacity
    } else if capacity == 0 || offered < capacity {
        offered
    } else {
        capacity
    }
}
pub(crate) fn validate_accepted_asynchronous_operations_window(
    accepted: AsynchronousOperationsWindow,
    offered: AsynchronousOperationsWindow,
) -> Result<()> {
    if !limit_accepted(accepted.maximum_invoked, offered.maximum_invoked) {
        return Err(Error::wrap(
            Kind::InvalidUserItem,
            format!(
                "accepted maximum invoked {} exceeds offered {}",
                accepted.maximum_invoked, offered.maximum_invoked
            ),
        ));
    }
    if !limit_accepted(accepted.maximum_performed, offered.maximum_performed) {
        return Err(Error::wrap(
            Kind::InvalidUserItem,
            format!(
                "accepted maximum performed {} exceeds offered {}",
                accepted.maximum_performed, offered.maximum_performed
            ),
        ));
    }
    Ok(())
}
fn limit_accepted(accepted: u16, offered: u16) -> bool {
    offered == 0 || (accepted != 0 && accepted <= offered)
}
pub(crate) fn handle_user_identity(
    items: &[UserItem],
    handler: Option<&UserIdentityHandler>,
) -> (
    Option<UserIdentityRequest>,
    UserIdentityResult,
    Option<Error>,
) {
    let req = match user_identity_request_from_user_info(items) {
        Err(e) => return (None, UserIdentityResult::default(), Some(e)),
        Ok(None) => return (None, UserIdentityResult::default(), None),
        Ok(Some(req)) => req,
    };
    if let Err(e) = validate_user_identity_request(&req) {
        return (Some(req), UserIdentityResult::default(), Some(e));
    }
    let Some(handler) = handler else {
        return (
            Some(req),
            UserIdentityResult::default(),
            Some(Error::wrap(
                Kind::UserIdentityRejected,
                "no accept policy configured",
            )),
        );
    };
    match handler(&req) {
        Err(text) => (
            Some(req),
            UserIdentityResult::default(),
            Some(Error::wrap(Kind::UserIdentityRejected, text)),
        ),
        Ok(result) => {
            let err = match result.action {
                USER_IDENTITY_ACTION_ACCEPT | USER_IDENTITY_ACTION_IGNORE => None,
                USER_IDENTITY_ACTION_REJECT => Some(Error::wrap(
                    Kind::UserIdentityRejected,
                    "rejected by policy",
                )),
                other => Some(Error::wrap(
                    Kind::InvalidUserIdentity,
                    format!("decision {other}"),
                )),
            };
            (Some(req), result, err)
        }
    }
}
pub(crate) fn negotiate_role_selections(
    requested: &[RoleSelectionItem],
    opts: &AcceptOptions,
    accepted_contexts: &[AcceptedContext],
) -> Result<Vec<RoleSelectionItem>> {
    let mut supported: HashMap<&str, &RoleSelectionItem> = HashMap::new();
    for role in &opts.role_selections {
        validate_role_selection_item(role)?;
        supported.insert(role.sop_class_uid.as_str(), role);
    }
    if requested.is_empty() || accepted_contexts.is_empty() {
        return Ok(Vec::new());
    }
    let accepted_classes: HashSet<&str> = accepted_contexts
        .iter()
        .map(|c| c.abstract_syntax_uid.as_str())
        .collect();
    let mut accepted = Vec::new();
    for req in requested {
        if !accepted_classes.contains(req.sop_class_uid.as_str()) {
            continue;
        }
        let settings = related_general_sop_class_of(accepted_contexts, &req.sop_class_uid);
        if let Some(rule) = presentation_context_rule(opts, &settings)
            && let Some(roles) = rule.roles
        {
            accepted.push(RoleSelectionItem {
                sop_class_uid: req.sop_class_uid.clone(),
                scu_role: req.scu_role && roles.scu,
                scp_role: req.scp_role && roles.scp,
            });
            continue;
        }
        let Some(support) = supported.get(settings.as_str()) else {
            continue;
        };
        let role = RoleSelectionItem {
            sop_class_uid: req.sop_class_uid.clone(),
            scu_role: req.scu_role && support.scu_role,
            scp_role: req.scp_role && support.scp_role,
        };
        if role.scu_role || role.scp_role {
            accepted.push(role);
        }
    }
    Ok(accepted)
}
pub(crate) fn negotiate_extended_negotiation(
    items: &[UserItem],
    handler: Option<&ExtendedNegotiationHandler>,
    accepted_contexts: &[AcceptedContext],
) -> (
    Vec<SopClassExtendedNegotiationItem>,
    Vec<SopClassExtendedNegotiationItem>,
    Option<Error>,
) {
    let requested = match extended_negotiation_from_user_info(items) {
        Ok(r) => r,
        Err(e) => return (Vec::new(), Vec::new(), Some(e)),
    };
    let Some(handler) = handler else {
        return (requested, Vec::new(), None);
    };
    if requested.is_empty() || accepted_contexts.is_empty() {
        return (requested, Vec::new(), None);
    }
    let accepted_classes: HashSet<&str> = accepted_contexts
        .iter()
        .map(|c| c.abstract_syntax_uid.as_str())
        .collect();
    let mut accepted = Vec::new();
    for req in &requested {
        if !accepted_classes.contains(req.sop_class_uid.as_str()) {
            continue;
        }
        match handler(req) {
            Err(text) => {
                return (
                    requested.clone(),
                    Vec::new(),
                    Some(Error::wrap(Kind::ExtendedNegotiationRejected, text)),
                );
            }
            Ok(result) => match result.action {
                EXTENDED_NEGOTIATION_ACTION_ACCEPT => {
                    accepted.push(SopClassExtendedNegotiationItem {
                        sop_class_uid: req.sop_class_uid.clone(),
                        data: result.data,
                    })
                }
                EXTENDED_NEGOTIATION_ACTION_IGNORE => {}
                EXTENDED_NEGOTIATION_ACTION_REJECT => {
                    return (
                        requested.clone(),
                        Vec::new(),
                        Some(Error::wrap(
                            Kind::ExtendedNegotiationRejected,
                            "rejected by policy",
                        )),
                    );
                }
                other => {
                    return (
                        requested.clone(),
                        Vec::new(),
                        Some(Error::wrap(
                            Kind::InvalidUserItem,
                            format!("extended negotiation decision {other}"),
                        )),
                    );
                }
            },
        }
    }
    (requested, accepted, None)
}
pub(crate) fn related_general_sop_class_of(
    contexts: &[AcceptedContext],
    sop_class_uid: &str,
) -> String {
    contexts
        .iter()
        .find(|c| {
            c.abstract_syntax_uid == sop_class_uid && !c.related_general_sop_class_uid.is_empty()
        })
        .map(|c| c.related_general_sop_class_uid.clone())
        .unwrap_or_else(|| sop_class_uid.to_string())
}
pub(crate) fn accepted_role_selections_from_user_info(
    items: &[UserItem],
    proposed: &[RoleSelectionItem],
) -> Result<Vec<RoleSelectionItem>> {
    let accepted = role_selections_from_user_info(items)?;
    if accepted.is_empty() {
        return Ok(Vec::new());
    }
    let mut by_sop: HashMap<&str, &RoleSelectionItem> = HashMap::new();
    for role in proposed {
        validate_role_selection_item(role)?;
        by_sop.insert(role.sop_class_uid.as_str(), role);
    }
    for role in &accepted {
        let Some(proposed_role) = by_sop.get(role.sop_class_uid.as_str()) else {
            return Err(Error::wrap(
                Kind::InvalidUserItem,
                format!(
                    "accepted role selection for unrequested SOP class {}",
                    quote(&role.sop_class_uid)
                ),
            ));
        };
        if role.scu_role && !proposed_role.scu_role {
            return Err(Error::wrap(
                Kind::InvalidUserItem,
                format!(
                    "accepted SCU role for unrequested SOP class {}",
                    quote(&role.sop_class_uid)
                ),
            ));
        }
        if role.scp_role && !proposed_role.scp_role {
            return Err(Error::wrap(
                Kind::InvalidUserItem,
                format!(
                    "accepted SCP role for unrequested SOP class {}",
                    quote(&role.sop_class_uid)
                ),
            ));
        }
    }
    Ok(accepted)
}
pub(crate) fn accepted_extended_negotiation_from_user_info(
    items: &[UserItem],
    proposed: &[SopClassExtendedNegotiationItem],
) -> Result<Vec<SopClassExtendedNegotiationItem>> {
    let accepted = extended_negotiation_from_user_info(items)?;
    if accepted.is_empty() {
        return Ok(Vec::new());
    }
    let mut by_sop = HashSet::new();
    for item in proposed {
        validate_sop_class_extended_negotiation_item(item)?;
        by_sop.insert(item.sop_class_uid.as_str());
    }
    for item in &accepted {
        if !by_sop.contains(item.sop_class_uid.as_str()) {
            return Err(Error::wrap(
                Kind::InvalidUserItem,
                format!(
                    "accepted extended negotiation for unrequested SOP class {}",
                    quote(&item.sop_class_uid)
                ),
            ));
        }
    }
    Ok(accepted)
}
pub(crate) fn role_selections_from_user_info(items: &[UserItem]) -> Result<Vec<RoleSelectionItem>> {
    let mut roles: Vec<RoleSelectionItem> = Vec::new();
    for item in items {
        let UserItem::RoleSelection(role) = item else {
            continue;
        };
        validate_role_selection_item(role)?;
        if let Some(previous) = roles.iter().find(|r| r.sop_class_uid == role.sop_class_uid) {
            if previous.scu_role != role.scu_role || previous.scp_role != role.scp_role {
                return Err(Error::wrap(
                    Kind::InvalidUserItem,
                    format!(
                        "conflicting role selections for SOP class {}",
                        quote(&role.sop_class_uid)
                    ),
                ));
            }
            continue;
        }
        roles.push(role.clone());
    }
    Ok(roles)
}
pub(crate) fn extended_negotiation_from_user_info(
    items: &[UserItem],
) -> Result<Vec<SopClassExtendedNegotiationItem>> {
    let mut extended: Vec<SopClassExtendedNegotiationItem> = Vec::new();
    for item in items {
        let UserItem::ExtendedNegotiation(e) = item else {
            continue;
        };
        validate_sop_class_extended_negotiation_item(e)?;
        if extended.iter().any(|x| x.sop_class_uid == e.sop_class_uid) {
            return Err(Error::wrap(
                Kind::InvalidUserItem,
                format!(
                    "duplicate extended negotiation for SOP class {}",
                    quote(&e.sop_class_uid)
                ),
            ));
        }
        extended.push(e.clone());
    }
    Ok(extended)
}
pub(crate) fn validate_role_selection_item(role: &RoleSelectionItem) -> Result<()> {
    if role.sop_class_uid.is_empty() {
        return Err(Error::wrap(
            Kind::MissingPduField,
            "role selection SOP class UID",
        ));
    }
    if role.sop_class_uid.len() > u16::MAX as usize {
        return Err(Error::wrap(
            Kind::LengthOverflow,
            format!(
                "role selection SOP class UID length {} exceeds uint16",
                role.sop_class_uid.len()
            ),
        ));
    }
    Ok(())
}
pub(crate) fn validate_sop_class_extended_negotiation_item(
    item: &SopClassExtendedNegotiationItem,
) -> Result<()> {
    if item.sop_class_uid.is_empty() {
        return Err(Error::wrap(
            Kind::MissingPduField,
            "extended negotiation SOP class UID",
        ));
    }
    if item.sop_class_uid.len() > u16::MAX as usize {
        return Err(Error::wrap(
            Kind::LengthOverflow,
            format!(
                "extended negotiation SOP class UID length {} exceeds uint16",
                item.sop_class_uid.len()
            ),
        ));
    }
    Ok(())
}
pub(crate) fn user_identity_request_from_user_info(
    items: &[UserItem],
) -> Result<Option<UserIdentityRequest>> {
    let mut found = None;
    for item in items {
        let UserItem::UserIdentity(identity) = item else {
            continue;
        };
        if found.is_some() {
            return Err(Error::wrap(
                Kind::InvalidUserIdentity,
                "multiple user identity items",
            ));
        }
        found = Some(UserIdentityRequest {
            identity_type: identity.identity_type,
            positive_response_requested: identity.positive_response_requested,
            primary_field: identity.primary_field.clone(),
            secondary_field: identity.secondary_field.clone(),
        });
    }
    Ok(found)
}
pub(crate) fn user_identity_response_from_user_info(items: &[UserItem]) -> Result<Option<Vec<u8>>> {
    let mut found = None;
    for item in items {
        let UserItem::UserIdentityResponse(r) = item else {
            continue;
        };
        if found.is_some() {
            return Err(Error::wrap(
                Kind::InvalidUserItem,
                "multiple user identity response items",
            ));
        }
        found = Some(r.clone());
    }
    Ok(found)
}
pub(crate) fn user_identity_item(req: &UserIdentityRequest) -> UserIdentityItem {
    UserIdentityItem {
        identity_type: req.identity_type,
        positive_response_requested: req.positive_response_requested,
        primary_field: req.primary_field.clone(),
        secondary_field: req.secondary_field.clone(),
    }
}
pub(crate) fn validate_user_identity_request(req: &UserIdentityRequest) -> Result<()> {
    if req.primary_field.is_empty() {
        return Err(Error::wrap(
            Kind::InvalidUserIdentity,
            "primary field is empty",
        ));
    }
    if req.primary_field.len() > u16::MAX as usize {
        return Err(Error::wrap(
            Kind::InvalidUserIdentity,
            format!(
                "primary field length {} exceeds uint16",
                req.primary_field.len()
            ),
        ));
    }
    if req.secondary_field.len() > u16::MAX as usize {
        return Err(Error::wrap(
            Kind::InvalidUserIdentity,
            format!(
                "secondary field length {} exceeds uint16",
                req.secondary_field.len()
            ),
        ));
    }
    match req.identity_type {
        USER_IDENTITY_USERNAME => {
            if !req.secondary_field.is_empty() {
                return Err(Error::wrap(
                    Kind::InvalidUserIdentity,
                    "username identity must not include a secondary field",
                ));
            }
        }
        USER_IDENTITY_USERNAME_PASSWORD => {}
        USER_IDENTITY_KERBEROS | USER_IDENTITY_SAML | USER_IDENTITY_JWT => {
            if !req.secondary_field.is_empty() {
                return Err(Error::wrap(
                    Kind::InvalidUserIdentity,
                    format!(
                        "token identity type {} must not include a secondary field",
                        req.identity_type
                    ),
                ));
            }
        }
        other => return Err(Error::wrap(Kind::UnsupportedUserIdentityType, other)),
    }
    Ok(())
}
pub(crate) fn max_pdu_from_user_info(items: &[UserItem]) -> u32 {
    for item in items {
        if let UserItem::MaxLength(v) = item {
            return if *v == 0 { u32::MAX } else { *v };
        }
    }
    DEFAULT_MAX_PDU
}
pub(crate) fn process_association_ac(
    ac: &AssociationAc,
    proposed: &[PresentationContextProposed],
    opts: &DialOptions,
) -> Result<(Vec<AcceptedContext>, Vec<PresentationContextOutcome>)> {
    if ac.protocol_version != opts.protocol_version {
        return Err(Error::wrap(
            Kind::InvalidPduField,
            format!(
                "protocol version {}, want {}",
                ac.protocol_version, opts.protocol_version
            ),
        ));
    }
    if ac.application_context_name != opts.application_context_name {
        return Err(Error::wrap(
            Kind::InvalidPduField,
            format!(
                "application context {}, want {}",
                quote(&ac.application_context_name),
                quote(&opts.application_context_name)
            ),
        ));
    }
    if ac.presentation_contexts.len() != proposed.len() {
        return Err(Error::wrap(
            Kind::InvalidPduField,
            format!(
                "presentation context result count {}, want {}",
                ac.presentation_contexts.len(),
                proposed.len()
            ),
        ));
    }
    let by_id: HashMap<u8, &PresentationContextProposed> =
        proposed.iter().map(|p| (p.id, p)).collect();
    let mut accepted = Vec::new();
    let mut seen = HashSet::new();
    for result in &ac.presentation_contexts {
        let Some(pc) = by_id.get(&result.id) else {
            return Err(Error::wrap(
                Kind::InvalidPduField,
                format!(
                    "unexpected or duplicate presentation context result {}",
                    result.id
                ),
            ));
        };
        if !seen.insert(result.id) {
            return Err(Error::wrap(
                Kind::InvalidPduField,
                format!(
                    "unexpected or duplicate presentation context result {}",
                    result.id
                ),
            ));
        }
        if result.result != PRESENTATION_CONTEXT_ACCEPTANCE {
            continue;
        }
        if !contains(&pc.transfer_syntax_uids, &result.transfer_syntax_uid) {
            return Err(Error::wrap(
                Kind::InvalidPduField,
                format!(
                    "accepted transfer syntax {} was not proposed for context {}",
                    quote(&result.transfer_syntax_uid),
                    result.id
                ),
            ));
        }
        accepted.push(AcceptedContext {
            id: result.id,
            abstract_syntax_uid: pc.abstract_syntax_uid.clone(),
            transfer_syntax_uid: result.transfer_syntax_uid.clone(),
            related_general_sop_class_uid: String::new(),
        });
    }
    let outcomes = presentation_context_outcomes_from_results(proposed, &ac.presentation_contexts);
    if accepted.is_empty() {
        return Err(Error::no_accepted(outcomes));
    }
    Ok((accepted, outcomes))
}
