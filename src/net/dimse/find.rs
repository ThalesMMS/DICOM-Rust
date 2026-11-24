#![forbid(unsafe_code)]
use super::command::*;
use super::controls::*;
use super::error::{Error, Kind, Result};
use super::message::*;
use super::monitor::ScpCancelMonitor;
use super::store::{new_scp_status_error, scp_status_of};
use crate::net::ul::{self, Association, Ctx};
use crate::object::{self, Element, Object, Syntax, Tag};
use std::sync::Arc;
pub const STUDY_ROOT_FIND_SOP_CLASS_UID: &str = "1.2.840.10008.5.1.4.1.2.2.1";
pub const PATIENT_ROOT_FIND_SOP_CLASS_UID: &str = "1.2.840.10008.5.1.4.1.2.1.1";
pub(crate) const PATIENT_STUDY_ONLY_FIND_SOP_CLASS_UID: &str = "1.2.840.10008.5.1.4.1.2.3.1";
pub const MODALITY_WORKLIST_FIND_SOP_CLASS_UID: &str = "1.2.840.10008.5.1.4.31";
pub const C_FIND_STATUS_IDENTIFIER_DOES_NOT_MATCH_SOP_CLASS: u16 = 0xA900;
pub const QUERY_RETRIEVE_LEVEL_PATIENT: &str = "PATIENT";
pub const QUERY_RETRIEVE_LEVEL_STUDY: &str = "STUDY";
pub const QUERY_RETRIEVE_LEVEL_SERIES: &str = "SERIES";
pub const QUERY_RETRIEVE_LEVEL_IMAGE: &str = "IMAGE";
pub(crate) const TAG_QUERY_RETRIEVE_LEVEL: Tag = Tag::new(0x0008, 0x0052);
pub const DEFAULT_FIND_TRANSFER_SYNTAXES: [&str; 2] =
    [ul::IMPLICIT_VR_LITTLE_ENDIAN, ul::EXPLICIT_VR_LITTLE_ENDIAN];
pub fn study_root_find_presentation_context() -> ul::PresentationContext {
    ul::PresentationContext::new(
        STUDY_ROOT_FIND_SOP_CLASS_UID,
        &DEFAULT_FIND_TRANSFER_SYNTAXES,
    )
}
pub fn patient_root_find_presentation_context() -> ul::PresentationContext {
    ul::PresentationContext::new(
        PATIENT_ROOT_FIND_SOP_CLASS_UID,
        &DEFAULT_FIND_TRANSFER_SYNTAXES,
    )
}
pub const QUERY_RETRIEVE_MODEL_STUDY_ROOT: &str = "STUDY_ROOT";
pub const QUERY_RETRIEVE_MODEL_PATIENT_ROOT: &str = "PATIENT_ROOT";
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryRetrieveModel {
    StudyRoot,
    PatientRoot,
}
impl QueryRetrieveModel {
    pub fn as_str(self) -> &'static str {
        match self {
            QueryRetrieveModel::StudyRoot => QUERY_RETRIEVE_MODEL_STUDY_ROOT,
            QueryRetrieveModel::PatientRoot => QUERY_RETRIEVE_MODEL_PATIENT_ROOT,
        }
    }
    fn levels(self) -> &'static [&'static str] {
        match self {
            QueryRetrieveModel::StudyRoot => &["STUDY", "SERIES", "IMAGE"],
            QueryRetrieveModel::PatientRoot => &["PATIENT", "STUDY", "SERIES", "IMAGE"],
        }
    }
}
pub fn parse_query_retrieve_model(value: &str) -> Result<QueryRetrieveModel> {
    let normalized = value.trim().to_uppercase().replace(['-', ' '], "_");
    match normalized.trim() {
        "STUDY_ROOT" if normalized == "STUDY_ROOT" => Ok(QueryRetrieveModel::StudyRoot),
        "PATIENT_ROOT" if normalized == "PATIENT_ROOT" => Ok(QueryRetrieveModel::PatientRoot),
        _ => Err(Error::new(format!(
            "dicom dimse: unsupported Query/Retrieve model {}",
            crate::textfmt::quote(normalized.trim())
        ))),
    }
}
pub fn query_retrieve_levels(model: QueryRetrieveModel) -> Vec<String> {
    model.levels().iter().map(|s| s.to_string()).collect()
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CFindExtendedNegotiation {
    pub relational_queries: bool,
    pub combined_date_time_matching: bool,
    pub fuzzy_semantic_matching: bool,
    pub timezone_query_adjustment: bool,
    pub enhanced_multi_frame_image_conversion: bool,
    pub empty_value_matching: bool,
    pub multiple_value_matching: bool,
}
impl CFindExtendedNegotiation {
    fn flags(&self) -> [bool; 7] {
        [
            self.relational_queries,
            self.combined_date_time_matching,
            self.fuzzy_semantic_matching,
            self.timezone_query_adjustment,
            self.enhanced_multi_frame_image_conversion,
            self.empty_value_matching,
            self.multiple_value_matching,
        ]
    }
    pub fn bytes(&self) -> Vec<u8> {
        encode_extended_negotiation_flags(&self.flags())
    }
    pub fn item(&self, sop_class_uid: &str) -> ul::SopClassExtendedNegotiationItem {
        ul::SopClassExtendedNegotiationItem {
            sop_class_uid: sop_class_uid.to_string(),
            data: self.bytes(),
        }
    }
    fn intersect(&self, s: &CFindExtendedNegotiation) -> CFindExtendedNegotiation {
        CFindExtendedNegotiation {
            relational_queries: self.relational_queries && s.relational_queries,
            combined_date_time_matching: self.combined_date_time_matching
                && s.combined_date_time_matching,
            fuzzy_semantic_matching: self.fuzzy_semantic_matching && s.fuzzy_semantic_matching,
            timezone_query_adjustment: self.timezone_query_adjustment
                && s.timezone_query_adjustment,
            enhanced_multi_frame_image_conversion: self.enhanced_multi_frame_image_conversion
                && s.enhanced_multi_frame_image_conversion,
            empty_value_matching: self.empty_value_matching && s.empty_value_matching,
            multiple_value_matching: self.multiple_value_matching && s.multiple_value_matching,
        }
    }
}
pub(crate) fn encode_extended_negotiation_flags(flags: &[bool]) -> Vec<u8> {
    let mut length = 1;
    for (i, set) in flags.iter().enumerate() {
        if *set {
            length = i + 1;
        }
    }
    (0..length)
        .map(|i| u8::from(flags.get(i) == Some(&true)))
        .collect()
}
pub fn parse_c_find_extended_negotiation(data: &[u8]) -> Result<CFindExtendedNegotiation> {
    if data.is_empty() {
        return Err(Error::new("dicom dimse: empty C-FIND extended negotiation"));
    }
    let flag = |i: usize| data.get(i) == Some(&1);
    Ok(CFindExtendedNegotiation {
        relational_queries: flag(0),
        combined_date_time_matching: flag(1),
        fuzzy_semantic_matching: flag(2),
        timezone_query_adjustment: flag(3),
        enhanced_multi_frame_image_conversion: flag(4),
        empty_value_matching: flag(5),
        multiple_value_matching: flag(6),
    })
}
pub(super) fn is_query_retrieve_find_sop_class(uid: &str) -> bool {
    matches!(
        uid,
        PATIENT_ROOT_FIND_SOP_CLASS_UID
            | STUDY_ROOT_FIND_SOP_CLASS_UID
            | PATIENT_STUDY_ONLY_FIND_SOP_CLASS_UID
    )
}
pub fn c_find_extended_negotiation_handler(
    supported: CFindExtendedNegotiation,
) -> ul::ExtendedNegotiationHandler {
    Arc::new(move |item: &ul::SopClassExtendedNegotiationItem| {
        let ignore = ul::ExtendedNegotiationResult {
            action: ul::EXTENDED_NEGOTIATION_ACTION_IGNORE,
            data: Vec::new(),
        };
        if !is_query_retrieve_find_sop_class(&item.sop_class_uid) {
            return Ok(ignore);
        }
        let Ok(requested) = parse_c_find_extended_negotiation(&item.data) else {
            return Ok(ignore);
        };
        let accepted = requested.intersect(&supported).bytes();
        let mut data = vec![0u8; item.data.len()];
        let n = accepted.len().min(data.len());
        data[..n].copy_from_slice(&accepted[..n]);
        Ok(ul::ExtendedNegotiationResult {
            action: ul::EXTENDED_NEGOTIATION_ACTION_ACCEPT,
            data,
        })
    })
}
pub fn accepted_c_find_extended_negotiation(
    assoc: &Association,
    sop_class_uid: &str,
) -> CFindExtendedNegotiation {
    if !is_query_retrieve_find_sop_class(sop_class_uid) {
        return CFindExtendedNegotiation::default();
    }
    for item in &assoc.accepted_extended_negotiation {
        if item.sop_class_uid != sop_class_uid {
            continue;
        }
        return parse_c_find_extended_negotiation(&item.data).unwrap_or_default();
    }
    CFindExtendedNegotiation::default()
}
pub fn send_c_find_request(
    assoc: &Association,
    pc_id: u8,
    req: &CFindRequest,
    identifier: Option<&[Element]>,
) -> Result<()> {
    if req.affected_sop_class_uid.is_empty() {
        return Err(Error::new(
            "dicom dimse: missing C-FIND Affected SOP Class UID",
        ));
    }
    let Some(identifier) = identifier else {
        return Err(Error::new("dicom dimse: missing C-FIND identifier dataset"));
    };
    send_command_set(None, assoc, pc_id, &req.command_set())?;
    let Some(accepted) = assoc.accepted_contexts.iter().find(|pc| pc.id == pc_id) else {
        return Err(Error::new(format!(
            "dicom dimse: no accepted presentation context with ID {pc_id}"
        )));
    };
    let Some(syntax) = object::transfer::lookup(&accepted.transfer_syntax_uid) else {
        return Err(Error::new(format!(
            "dicom dimse: unsupported transfer syntax {}",
            crate::textfmt::quote(&accepted.transfer_syntax_uid)
        )));
    };
    send_data_set(
        None,
        assoc,
        pc_id,
        &Object::from_elements(identifier.to_vec()),
        &syntax,
    )
}
pub fn receive_c_find_response(
    ctx: Option<&Ctx>,
    assoc: &Association,
    pc_id: u8,
    syntax: Syntax,
) -> Result<(CFindResponse, Option<Object>)> {
    let cmd = receive_command_set(ctx, assoc, pc_id)?;
    let resp = parse_c_find_response(&cmd)?;
    if resp.command_data_set_type == NO_DATA_SET {
        return Ok((resp, None));
    }
    let id = receive_identifier_data_set(ctx, assoc, pc_id, syntax)?;
    Ok((resp, Some(id)))
}
#[derive(Debug, Clone)]
pub struct FindResult {
    pub response: CFindResponse,
    pub identifier: Option<Object>,
}
impl FindResult {
    pub fn status(&self) -> u16 {
        self.response.status
    }
}
pub fn find_with_options(
    options: OperationOptions,
    assoc: &Association,
    pc_id: u8,
    req: &CFindRequest,
    identifier: Option<&Object>,
    syntax: &Syntax,
    on_result: &mut dyn FnMut(FindResult),
) -> Result<()> {
    let options = operation_options_with_default_policy(options, OperationErrorPolicy::LeaveOpen);
    let ctx = operation_context(&options);
    if let Some(e) = context_err(&ctx) {
        return Err(new_operation_error("C-FIND", e, false));
    }
    let Some(identifier) = identifier else {
        return Err(Error::new("dicom dimse: nil C-FIND identifier"));
    };
    let _guard = begin_association_operation(Some(assoc))
        .map_err(|e| new_operation_error("C-FIND", e, false))?;
    let policy = options.error_policy;
    let fail = |e: Error| {
        apply_operation_error_policy(Some(assoc), policy, new_operation_error("C-FIND", e, true))
    };
    send_command_set(None, assoc, pc_id, &req.command_set()).map_err(fail)?;
    send_data_set(None, assoc, pc_id, identifier, syntax).map_err(fail)?;
    loop {
        if let Some(e) = context_err(&ctx) {
            return Err(fail(e));
        }
        let rctx = operation_response_context(&ctx, options.response_timeout);
        let (rsp, id) =
            receive_c_find_response(Some(&rctx), assoc, pc_id, *syntax).map_err(fail)?;
        if rsp.message_id_being_responded_to != req.message_id {
            return Err(fail(Error::classified(
                Kind::ProtocolViolation,
                format!(
                    "dicom dimse: C-FIND response message ID {}, want {}",
                    rsp.message_id_being_responded_to, req.message_id
                ),
            )));
        }
        let category = categorize_c_find_status(rsp.status);
        if id.is_none() && category == CFindStatusCategory::Pending {
            return Err(fail(Error::classified(
                Kind::ProtocolViolation,
                "dicom dimse: pending C-FIND response without an Identifier",
            )));
        }
        if let Some(e) = context_err(&ctx) {
            return Err(fail(e));
        }
        on_result(FindResult {
            response: rsp.clone(),
            identifier: id,
        });
        match category {
            CFindStatusCategory::Pending => continue,
            CFindStatusCategory::Success => return Ok(()),
            _ => return Err(c_find_status_error(&rsp).expect("final non-success")),
        }
    }
}
pub fn find(
    ctx: &Ctx,
    assoc: &Association,
    pc_id: u8,
    req: &CFindRequest,
    identifier: Option<&Object>,
    syntax: &Syntax,
    on_result: &mut dyn FnMut(FindResult),
) -> Result<()> {
    find_with_options(
        OperationOptions {
            context: Some(ctx.clone()),
            ..Default::default()
        },
        assoc,
        pc_id,
        req,
        identifier,
        syntax,
        on_result,
    )
}
pub struct FindClient<'a> {
    pub assoc: &'a Association,
    pub sop_class_uid: String,
    pub presentation_ctx_id: u8,
    pub identifier_syntax: Syntax,
}
impl<'a> FindClient<'a> {
    pub fn for_sop_class(assoc: &'a Association, sop_class_uid: &str) -> Result<FindClient<'a>> {
        let pc = accepted_context_for_sop_class(assoc, sop_class_uid)?;
        let syntax = transfer_syntax_for_accepted_context(&pc)?;
        Ok(FindClient {
            assoc,
            sop_class_uid: sop_class_uid.to_string(),
            presentation_ctx_id: pc.id,
            identifier_syntax: syntax,
        })
    }
    pub fn find_with_options(
        &self,
        options: OperationOptions,
        mut req: CFindRequest,
        identifier: Option<&Object>,
        on_result: &mut dyn FnMut(FindResult),
    ) -> Result<()> {
        if self.presentation_ctx_id == 0 {
            return Err(Error::new(
                "dicom dimse: find client presentation context ID is required",
            ));
        }
        let requested = object::transfer::normalize_uid(&req.affected_sop_class_uid).to_string();
        let client = object::transfer::normalize_uid(&self.sop_class_uid).to_string();
        if !requested.is_empty() && requested != client {
            return Err(Error::new(format!(
                "dicom dimse: C-FIND request AffectedSOPClassUID {} does not match find client SOPClassUID {}",
                crate::textfmt::quote(&requested),
                crate::textfmt::quote(&client)
            )));
        }
        req.affected_sop_class_uid = if requested.is_empty() {
            client
        } else {
            requested
        };
        find_with_options(
            options,
            self.assoc,
            self.presentation_ctx_id,
            &req,
            identifier,
            &self.identifier_syntax,
            on_result,
        )
    }
}
#[derive(Debug, Clone)]
pub struct CFindRequestContext {
    pub request: CFindRequest,
    pub identifier: Object,
    pub query_retrieve_level: String,
    pub presentation_context_id: u8,
    pub identifier_syntax: Syntax,
    pub extended_negotiation: CFindExtendedNegotiation,
}
pub type BeforeSend = Arc<dyn Fn(&Ctx) -> Result<()> + Send + Sync>;
#[derive(Clone)]
pub struct CFindMatch {
    pub identifier: Option<Object>,
    pub status: u16,
    pub before_send: Option<BeforeSend>,
}
pub type CFindHandler =
    Arc<dyn Fn(&Ctx, &CFindRequestContext) -> Result<Vec<CFindMatch>> + Send + Sync>;
pub fn send_c_find_response(
    ctx: Option<&Ctx>,
    assoc: &Association,
    pc_id: u8,
    mut rsp: CFindResponse,
    identifier: Option<&Object>,
    syntax: &Syntax,
) -> Result<()> {
    if rsp.affected_sop_class_uid.is_empty() {
        return Err(Error::new(
            "dicom dimse: C-FIND response AffectedSOPClassUID is required",
        ));
    }
    rsp.command_data_set_type = if identifier.is_none() {
        NO_DATA_SET
    } else {
        DATA_SET_PRESENT
    };
    send_message_data_set(ctx, assoc, pc_id, &rsp.command_set(), identifier, syntax)
}
pub fn receive_c_find_request(
    assoc: &Association,
    pc_id: u8,
    syntax: Syntax,
) -> Result<(CFindRequest, Object)> {
    let command = receive_command_set(None, assoc, pc_id)?;
    let req = parse_c_find_request(&command)?;
    let identifier = receive_identifier_data_set(None, assoc, pc_id, syntax)?;
    Ok((req, identifier))
}
pub fn new_c_find_scp_error(status: u16, comment: &str, err: Option<Error>) -> Error {
    new_scp_status_error("C-FIND", status, comment, err)
}
struct FindModel {
    name: &'static str,
    sop_class_uid: &'static str,
    patient_root: bool,
}
pub fn serve_study_root_c_find(
    ctx: Option<&Ctx>,
    assoc: &Association,
    pc_id: u8,
    handler: &CFindHandler,
) -> Result<()> {
    let command = receive_command_set(ctx, assoc, pc_id)?;
    serve_c_find_command(ctx, assoc, pc_id, &command, handler, false)
}
pub fn serve_patient_root_c_find(
    ctx: Option<&Ctx>,
    assoc: &Association,
    pc_id: u8,
    handler: &CFindHandler,
) -> Result<()> {
    let command = receive_command_set(ctx, assoc, pc_id)?;
    serve_c_find_command(ctx, assoc, pc_id, &command, handler, true)
}
pub(crate) fn c_find_level(identifier: Option<&Object>, patient_root: bool) -> Result<String> {
    let Some(identifier) = identifier else {
        return Err(Error::new("dicom dimse: missing C-FIND Identifier"));
    };
    let level = identifier
        .get_string(TAG_QUERY_RETRIEVE_LEVEL)
        .unwrap_or_default();
    if level.trim().is_empty() {
        return Err(Error::new("dicom dimse: missing QueryRetrieveLevel"));
    }
    let level = level.trim().to_uppercase();
    let ok = match level.as_str() {
        "STUDY" | "SERIES" | "IMAGE" => true,
        "PATIENT" => patient_root,
        _ => false,
    };
    if !ok {
        return Err(Error::new(format!(
            "dicom dimse: unsupported QueryRetrieveLevel {}",
            crate::textfmt::quote(&level)
        )));
    }
    Ok(level)
}
pub(crate) fn query_retrieve_level_error_comment(
    identifier: Option<&Object>,
    service: &str,
) -> String {
    let Some(identifier) = identifier else {
        return format!("Missing {service} Identifier");
    };
    match identifier.get_string(TAG_QUERY_RETRIEVE_LEVEL) {
        Some(l) if !l.trim().is_empty() => "Unsupported Query/Retrieve Level (0008,0052)".into(),
        _ => "Missing Query/Retrieve Level (0008,0052)".into(),
    }
}
pub(crate) fn is_c_find_failure_status(status: u16) -> bool {
    status == C_FIND_STATUS_OUT_OF_RESOURCES
        || status == C_FIND_STATUS_IDENTIFIER_DOES_NOT_MATCH_SOP_CLASS
        || (0xC000..=0xCFFF).contains(&status)
}
pub(crate) fn c_find_scp_status_for_error(err: &Error) -> (u16, String) {
    if let Some((status, comment)) = scp_status_of(err, "C-FIND") {
        let comment = if comment.is_empty() {
            let prefix = format!("dicom dimse: C-FIND SCP status 0x{status:04X}: ");
            err.text().strip_prefix(&prefix).unwrap_or("").to_string()
        } else {
            comment
        };
        return (status, comment);
    }
    if err.is_deadline() {
        return (C_FIND_STATUS_UNABLE_TO_PROCESS, err.text().to_string());
    }
    if err.is(Kind::CFindCanceled) || err.is_canceled() {
        return (C_FIND_STATUS_CANCEL, "C-FIND canceled".into());
    }
    (C_FIND_STATUS_UNABLE_TO_PROCESS, err.text().to_string())
}
#[allow(clippy::too_many_arguments)]
pub(crate) fn send_c_find_final(
    ctx: Option<&Ctx>,
    assoc: &Association,
    pc_id: u8,
    req: &CFindRequest,
    sop_class_uid: &str,
    status: u16,
    comment: &str,
    syntax: &Syntax,
) -> Result<()> {
    send_with_scp_response_context(ctx, Some(assoc), |c| {
        send_c_find_response(
            Some(c),
            assoc,
            pc_id,
            CFindResponse {
                affected_sop_class_uid: sop_class_uid.to_string(),
                message_id_being_responded_to: req.message_id,
                status,
                error_comment: comment.to_string(),
                ..Default::default()
            },
            None,
            syntax,
        )
    })
}
fn send_c_find_operation_error(
    ctx: Option<&Ctx>,
    assoc: &Association,
    pc_id: u8,
    req: &CFindRequest,
    sop_class_uid: &str,
    syntax: &Syntax,
    err: Error,
) -> Result<()> {
    if err.is(Kind::StoreAccessDenied) {
        let mut status = C_FIND_STATUS_UNABLE_TO_PROCESS;
        if let Some((s, _)) = scp_status_of(&err, "C-FIND")
            && is_c_find_failure_status(s)
        {
            status = s;
        }
        send_c_find_final(
            ctx,
            assoc,
            pc_id,
            req,
            sop_class_uid,
            status,
            "access denied",
            syntax,
        )?;
        return Err(new_c_find_scp_error(
            status,
            "access denied",
            Some(Error::sentinel(Kind::StoreAccessDenied)),
        ));
    }
    let (status, comment) = c_find_scp_status_for_error(&err);
    let _ = send_c_find_final(
        ctx,
        assoc,
        pc_id,
        req,
        sop_class_uid,
        status,
        &comment,
        syntax,
    );
    Err(err)
}
fn check_c_find_send_access(ctx: &Ctx, check: Option<&BeforeSend>) -> Result<()> {
    let Some(check) = check else { return Ok(()) };
    if ctx.err().is_some() {
        return Err(context_cause(ctx).expect("ended"));
    }
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| check(ctx)));
    if ctx.err().is_some() {
        return Err(context_cause(ctx).expect("ended"));
    }
    match r {
        Ok(Ok(())) => Ok(()),
        Ok(Err(e)) => {
            if let Some((s, _)) = scp_status_of(&e, "C-FIND")
                && is_c_find_failure_status(s)
            {
                return Err(new_c_find_scp_error(
                    s,
                    "access denied",
                    Some(Error::sentinel(Kind::StoreAccessDenied)),
                ));
            }
            Err(Error::sentinel(Kind::StoreAccessDenied))
        }
        Err(_) => Err(Error::sentinel(Kind::StoreAccessDenied)),
    }
}
pub(crate) fn serve_c_find_command(
    ctx: Option<&Ctx>,
    assoc: &Association,
    pc_id: u8,
    command: &Object,
    handler: &CFindHandler,
    patient_root: bool,
) -> Result<()> {
    let model = if patient_root {
        FindModel {
            name: "Patient Root FIND",
            sop_class_uid: PATIENT_ROOT_FIND_SOP_CLASS_UID,
            patient_root: true,
        }
    } else {
        FindModel {
            name: "Study Root FIND",
            sop_class_uid: STUDY_ROOT_FIND_SOP_CLASS_UID,
            patient_root: false,
        }
    };
    let syntax = accepted_scp_transfer_syntax(ctx, assoc, pc_id, model.sop_class_uid)?;
    let req = parse_c_find_request(command)?;
    let identifier = receive_identifier_data_set(ctx, assoc, pc_id, syntax)?;
    if req.affected_sop_class_uid != model.sop_class_uid {
        let err = Error::new(format!(
            "dicom dimse: C-FIND SCP SOP Class UID {}, want {} {}",
            crate::textfmt::quote(&req.affected_sop_class_uid),
            model.name,
            crate::textfmt::quote(model.sop_class_uid)
        ));
        let _ = send_c_find_final(
            ctx,
            assoc,
            pc_id,
            &req,
            model.sop_class_uid,
            C_FIND_STATUS_UNABLE_TO_PROCESS,
            err.text(),
            &syntax,
        );
        return Err(err);
    }
    let level = match c_find_level(Some(&identifier), model.patient_root) {
        Ok(l) => l,
        Err(err) => {
            return send_c_find_operation_error(
                ctx,
                assoc,
                pc_id,
                &req,
                model.sop_class_uid,
                &syntax,
                new_c_find_scp_error(
                    C_FIND_STATUS_IDENTIFIER_DOES_NOT_MATCH_SOP_CLASS,
                    &query_retrieve_level_error_comment(Some(&identifier), "C-FIND"),
                    Some(err),
                ),
            );
        }
    };
    let negotiated = accepted_c_find_extended_negotiation(assoc, model.sop_class_uid);
    if super::find_extras::c_find_identifier_check_enabled(ctx) {
        let model_name = if model.patient_root {
            "PATIENT_ROOT"
        } else {
            "STUDY_ROOT"
        };
        if let Err(err) = super::find_extras::validate_c_find_identifier(
            model_name,
            &level,
            Some(&identifier),
            negotiated.relational_queries,
        ) {
            let comment = err.text().to_string();
            return send_c_find_operation_error(
                ctx,
                assoc,
                pc_id,
                &req,
                model.sop_class_uid,
                &syntax,
                new_c_find_scp_error(
                    C_FIND_STATUS_IDENTIFIER_DOES_NOT_MATCH_SOP_CLASS,
                    &comment,
                    Some(err),
                ),
            );
        }
    }
    let grace = scp_controls(ctx).cancel_grace;
    let (err, op_ctx) = std::thread::scope(|s| {
        let monitor = ScpCancelMonitor::start(
            s,
            ctx,
            assoc,
            pc_id,
            req.message_id,
            Error::sentinel(Kind::CFindCanceled),
            false,
        );
        let op_ctx = monitor.context().clone();
        let request_context = CFindRequestContext {
            request: req.clone(),
            identifier: identifier.clone(),
            query_retrieve_level: level.clone(),
            presentation_context_id: pc_id,
            identifier_syntax: syntax,
            extended_negotiation: negotiated,
        };
        let r = run_scp_handler(&op_ctx, Some(assoc), grace, |handler_ctx| {
            let matches = handler(handler_ctx, &request_context)?;
            for m in &matches {
                if let Some(e) = monitor.operation_error() {
                    return Err(e);
                }
                let Some(identifier) = &m.identifier else {
                    return Err(Error::new("dicom dimse: C-FIND handler returned nil match"));
                };
                let status = if m.status == 0 {
                    STATUS_PENDING
                } else {
                    m.status
                };
                if status != STATUS_PENDING && status != STATUS_PENDING_WARNING {
                    return Err(Error::new(format!(
                        "dicom dimse: C-FIND handler returned invalid pending status 0x{status:04X}"
                    )));
                }
                send_with_scp_response_context(ctx, Some(assoc), |rc| {
                    check_c_find_send_access(handler_ctx, m.before_send.as_ref())?;
                    send_c_find_response(
                        Some(rc),
                        assoc,
                        pc_id,
                        CFindResponse {
                            affected_sop_class_uid: model.sop_class_uid.to_string(),
                            message_id_being_responded_to: req.message_id,
                            status,
                            ..Default::default()
                        },
                        Some(identifier),
                        &syntax,
                    )
                })?;
            }
            match monitor.operation_error() {
                Some(e) => Err(e),
                None => Ok(()),
            }
        })
        .err();
        let monitor_err = monitor.stop();
        (Error::join([r, monitor_err]), op_ctx)
    });
    if let Some(err) = err {
        return send_c_find_operation_error(
            Some(&op_ctx),
            assoc,
            pc_id,
            &req,
            model.sop_class_uid,
            &syntax,
            err,
        );
    }
    send_c_find_final(
        ctx,
        assoc,
        pc_id,
        &req,
        model.sop_class_uid,
        STATUS_SUCCESS,
        "",
        &syntax,
    )
}
