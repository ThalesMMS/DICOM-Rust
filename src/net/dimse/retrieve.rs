#![forbid(unsafe_code)]
use super::command::*;
use super::controls::*;
use super::error::{Error, Kind, Result};
use super::find::{c_find_level, query_retrieve_level_error_comment};
use super::message::*;
use super::monitor::ScpCancelMonitor;
use super::retrieve_extras::{
    CMoveBatch, CMoveBatchHandler, batch_contract, validate_c_move_batch,
};
use super::store::{is_c_store_warning_status, new_scp_status_error, scp_status_of};
use crate::net::ul::{self, Association, Ctx};
use crate::object::{Object, Syntax};
use std::sync::Arc;
use std::time::Duration;
pub const STUDY_ROOT_MOVE_SOP_CLASS_UID: &str = "1.2.840.10008.5.1.4.1.2.2.2";
pub const PATIENT_ROOT_MOVE_SOP_CLASS_UID: &str = "1.2.840.10008.5.1.4.1.2.1.2";
pub const STUDY_ROOT_GET_SOP_CLASS_UID: &str = "1.2.840.10008.5.1.4.1.2.2.3";
pub const PATIENT_ROOT_GET_SOP_CLASS_UID: &str = "1.2.840.10008.5.1.4.1.2.1.3";
const PATIENT_STUDY_ONLY_MOVE: &str = "1.2.840.10008.5.1.4.1.2.3.2";
const PATIENT_STUDY_ONLY_GET: &str = "1.2.840.10008.5.1.4.1.2.3.3";
const MAX_C_MOVE_SUB_OPERATIONS: usize = (1 << 16) - 1;
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CRetrieveExtendedNegotiation {
    pub relational_retrieval: bool,
    pub enhanced_multi_frame_image_conversion: bool,
}
pub fn parse_c_retrieve_extended_negotiation(data: &[u8]) -> Result<CRetrieveExtendedNegotiation> {
    if data.is_empty() {
        return Err(Error::new(
            "dicom dimse: empty C-MOVE/C-GET extended negotiation",
        ));
    }
    Ok(CRetrieveExtendedNegotiation {
        relational_retrieval: data[0] == 1,
        enhanced_multi_frame_image_conversion: data.len() > 1 && data[1] == 1,
    })
}
pub(super) fn is_query_retrieve_retrieve_sop_class(uid: &str) -> bool {
    matches!(
        uid,
        PATIENT_ROOT_MOVE_SOP_CLASS_UID
            | STUDY_ROOT_MOVE_SOP_CLASS_UID
            | PATIENT_ROOT_GET_SOP_CLASS_UID
            | STUDY_ROOT_GET_SOP_CLASS_UID
            | PATIENT_STUDY_ONLY_MOVE
            | PATIENT_STUDY_ONLY_GET
    )
}
pub fn accepted_c_retrieve_extended_negotiation(
    assoc: &Association,
    sop_class_uid: &str,
) -> CRetrieveExtendedNegotiation {
    let data = assoc
        .accepted_extended_negotiation
        .iter()
        .find(|i| i.sop_class_uid == sop_class_uid)
        .map(|i| i.data.clone());
    match data {
        Some(d) if is_query_retrieve_retrieve_sop_class(sop_class_uid) => {
            parse_c_retrieve_extended_negotiation(&d).unwrap_or_default()
        }
        _ => CRetrieveExtendedNegotiation::default(),
    }
}
pub fn send_c_move_request(assoc: &Association, pc_id: u8, req: &CMoveRequest) -> Result<()> {
    if req.message_id == 0 {
        return Err(Error::new(
            "dicom dimse: C-MOVE request MessageID must be non-zero",
        ));
    }
    if req.affected_sop_class_uid.is_empty() {
        return Err(Error::new(
            "dicom dimse: C-MOVE request AffectedSOPClassUID is required",
        ));
    }
    if req.move_destination.is_empty() {
        return Err(Error::new(
            "dicom dimse: C-MOVE request MoveDestination is required",
        ));
    }
    send_command_set(None, assoc, pc_id, &req.command_set())
}
pub fn send_c_move_response(
    ctx: Option<&Ctx>,
    assoc: &Association,
    pc_id: u8,
    rsp: &CMoveResponse,
) -> Result<()> {
    if rsp.message_id_being_responded_to == 0 {
        return Err(Error::new(
            "dicom dimse: C-MOVE response MessageIDBeingRespondedTo must be non-zero",
        ));
    }
    if rsp.affected_sop_class_uid.is_empty() {
        return Err(Error::new(
            "dicom dimse: C-MOVE response AffectedSOPClassUID is required",
        ));
    }
    if rsp.identifier.is_some() && !retrieve_status_allows_identifier(rsp.status) {
        return Err(Error::new(format!(
            "dicom dimse: C-MOVE response identifier dataset not allowed for status 0x{:04X}",
            rsp.status
        )));
    }
    send_command_set(ctx, assoc, pc_id, &rsp.command_set(C_MOVE_RSP))?;
    let identifier = rsp.response_identifier();
    let Some(identifier) = identifier else {
        return Ok(());
    };
    if !retrieve_status_allows_identifier(rsp.status) {
        return Ok(());
    }
    let syntax = accepted_transfer_syntax(assoc, pc_id)?;
    send_data_set(ctx, assoc, pc_id, &identifier, &syntax)
}
pub fn receive_c_move_response(
    ctx: Option<&Ctx>,
    assoc: &Association,
    pc_id: u8,
) -> Result<CMoveResponse> {
    let command = receive_command_set(ctx, assoc, pc_id)?;
    let mut rsp = parse_c_move_response(&command)?;
    if rsp.identifier.is_none() {
        return Ok(rsp);
    }
    let syntax = accepted_transfer_syntax(assoc, pc_id)?;
    let identifier = receive_identifier_data_set(ctx, assoc, pc_id, syntax)?;
    if let Some(values) = identifier.get_strings(FAILED_SOP_INSTANCE_UID_LIST)
        && !values.is_empty()
    {
        rsp.failed_sop_instance_uid_list = Some(values);
    }
    rsp.identifier = Some(identifier);
    Ok(rsp)
}
#[derive(Debug, Clone)]
pub struct CMoveProgress {
    pub response: CMoveResponse,
    pub final_: bool,
    pub status_class: CMoveStatus,
}
fn cancel_c_move_at_provider(assoc: &Association, pc_id: u8, message_id: u16) {
    let ctx = Ctx::background().with_timeout(Duration::from_secs(1));
    let _ = send_command_set(
        Some(&ctx),
        assoc,
        pc_id,
        &CCancelRequest {
            message_id_being_responded_to: message_id,
        }
        .command_set(),
    );
}
pub fn send_c_move_with_options(
    options: OperationOptions,
    assoc: &Association,
    pc_id: u8,
    req: &CMoveRequest,
    identifier: Option<&Object>,
    syntax: &Syntax,
    on_progress: &mut dyn FnMut(&CMoveProgress),
) -> Result<CMoveResponse> {
    let options = operation_options_with_default_policy(options, OperationErrorPolicy::Abort);
    let ctx = operation_context(&options);
    let Some(identifier) = identifier else {
        return Err(Error::new(
            "dicom dimse: C-MOVE: identifier dataset is required",
        ));
    };
    if let Some(e) = context_err(&ctx) {
        return Err(new_operation_error("C-MOVE", e, false));
    }
    let _guard = begin_association_operation(Some(assoc))
        .map_err(|e| new_operation_error("C-MOVE", e, false))?;
    let policy = options.error_policy;
    let fail = |e: Error| {
        apply_operation_error_policy(Some(assoc), policy, new_operation_error("C-MOVE", e, true))
    };
    send_c_move_request(assoc, pc_id, req).map_err(fail)?;
    if let Some(e) = context_err(&ctx) {
        return Err(fail(e));
    }
    send_data_set(None, assoc, pc_id, identifier, syntax).map_err(fail)?;
    loop {
        if let Some(e) = context_err(&ctx) {
            cancel_c_move_at_provider(assoc, pc_id, req.message_id);
            return Err(fail(e));
        }
        let rctx = operation_response_context(&ctx, options.response_timeout);
        let mut r = receive_c_move_response(Some(&rctx), assoc, pc_id);
        if let Ok(rsp) = &r
            && rsp.message_id_being_responded_to != req.message_id
        {
            r = Err(Error::classified(
                Kind::ProtocolViolation,
                format!(
                    "dicom dimse: C-MOVE response message ID {}, want {}",
                    rsp.message_id_being_responded_to, req.message_id
                ),
            ));
        }
        let rsp = match r {
            Ok(rsp) => rsp,
            Err(e) => {
                if ctx.err().is_some() {
                    cancel_c_move_at_provider(assoc, pc_id, req.message_id);
                }
                return Err(fail(e));
            }
        };
        let cls = classify_c_move_status(rsp.status);
        let event = CMoveProgress {
            response: rsp.clone(),
            final_: cls != CMoveStatus::Pending,
            status_class: cls,
        };
        on_progress(&event);
        if event.final_ {
            return Ok(rsp);
        }
    }
}
#[derive(Debug, Clone)]
pub struct CMoveRequestContext {
    pub request: CMoveRequest,
    pub identifier: Object,
    pub query_retrieve_level: String,
    pub presentation_context_id: u8,
    pub identifier_syntax: Syntax,
    pub extended_negotiation: CRetrieveExtendedNegotiation,
}
#[derive(Debug, Clone, Default)]
pub struct CMoveSubOperationResult {
    pub status: u16,
    pub err: Option<Error>,
}
#[derive(Clone)]
pub struct CMoveSubOperation {
    pub affected_sop_class_uid: String,
    pub affected_sop_instance_uid: String,
    pub store: Option<SubOperationStore>,
}
pub(crate) type SubOperationStore = Arc<dyn Fn(&Ctx) -> CMoveSubOperationResult + Send + Sync>;
pub type CMoveHandler =
    Arc<dyn Fn(&Ctx, &CMoveRequestContext) -> Result<Vec<CMoveSubOperation>> + Send + Sync>;
pub fn new_c_move_scp_error(status: u16, comment: &str, err: Option<Error>) -> Error {
    new_scp_status_error("C-MOVE", status, comment, err)
}
#[derive(Debug, Clone, Default)]
pub(crate) struct SubOperationTally {
    pub remaining: u16,
    pub completed: u16,
    pub failed: u16,
    pub warning: u16,
    pub failed_sop_instance_uids: Vec<String>,
}
pub(crate) fn count_sub_operation_result(
    counts: &mut SubOperationTally,
    result: &CMoveSubOperationResult,
    sop_instance_uid: &str,
) {
    if result.err.is_some() {
        counts.failed += 1;
        counts
            .failed_sop_instance_uids
            .push(sop_instance_uid.to_string());
        return;
    }
    if result.status == STATUS_SUCCESS {
        counts.completed += 1;
    } else if is_c_store_warning_status(result.status) {
        counts.warning += 1;
    } else {
        counts.failed += 1;
        counts
            .failed_sop_instance_uids
            .push(sop_instance_uid.to_string());
    }
}
pub(crate) fn validate_c_move_sub_operations(ops: &[CMoveSubOperation]) -> Result<()> {
    if ops.len() > MAX_C_MOVE_SUB_OPERATIONS {
        return Err(Error::new(format!(
            "dicom dimse: too many C-MOVE sub-operations: {} exceeds {MAX_C_MOVE_SUB_OPERATIONS}",
            ops.len()
        )));
    }
    for (i, op) in ops.iter().enumerate() {
        if op.affected_sop_class_uid.is_empty() {
            return Err(Error::new(format!(
                "dicom dimse: C-MOVE sub-operation {i} missing Affected SOP Class UID"
            )));
        }
        if op.affected_sop_instance_uid.is_empty() {
            return Err(Error::new(format!(
                "dicom dimse: C-MOVE sub-operation {i} missing Affected SOP Instance UID"
            )));
        }
        if op.store.is_none() {
            return Err(Error::new(format!(
                "dicom dimse: C-MOVE sub-operation {i} missing Store callback"
            )));
        }
    }
    Ok(())
}
pub(crate) fn c_move_scp_status_for_error(err: &Error) -> (u16, String) {
    if let Some((status, comment)) = scp_status_of(err, "C-MOVE") {
        let comment = if comment.is_empty() {
            let prefix = format!("dicom dimse: C-MOVE SCP status 0x{status:04X}: ");
            err.text().strip_prefix(&prefix).unwrap_or("").to_string()
        } else {
            comment
        };
        return (status, comment);
    }
    if err.is_deadline() {
        return (STATUS_C_MOVE_UNABLE_TO_PROCESS, err.text().to_string());
    }
    if err.is(Kind::CMoveCanceled) || err.is_canceled() {
        return (STATUS_C_MOVE_CANCEL, "C-MOVE canceled".into());
    }
    (STATUS_C_MOVE_UNABLE_TO_PROCESS, err.text().to_string())
}
#[allow(clippy::too_many_arguments)]
fn send_c_move_scp_response(
    ctx: Option<&Ctx>,
    assoc: &Association,
    pc_id: u8,
    req: &CMoveRequest,
    sop_class_uid: &str,
    status: u16,
    comment: &str,
    counts: &SubOperationTally,
) -> Result<()> {
    let remaining = if status == STATUS_PENDING {
        Some(counts.remaining)
    } else {
        None
    };
    send_with_scp_response_context(ctx, Some(assoc), |c| {
        send_c_move_response(
            Some(c),
            assoc,
            pc_id,
            &RetrieveResponse {
                affected_sop_class_uid: sop_class_uid.to_string(),
                message_id_being_responded_to: req.message_id,
                status,
                error_comment: comment.to_string(),
                identifier: None,
                counts: SubOperationCounts {
                    remaining,
                    completed: Some(counts.completed),
                    failed: Some(counts.failed),
                    warning: Some(counts.warning),
                },
                failed_sop_instance_uid_list: Some(counts.failed_sop_instance_uids.clone()),
            },
        )
    })
}
#[allow(clippy::too_many_arguments)]
fn send_c_move_operation_error(
    ctx: Option<&Ctx>,
    assoc: &Association,
    pc_id: u8,
    req: &CMoveRequest,
    sop_class_uid: &str,
    counts: &SubOperationTally,
    err: Error,
) -> Result<()> {
    if err.is(Kind::StoreAccessDenied) {
        send_c_move_scp_response(
            ctx,
            assoc,
            pc_id,
            req,
            sop_class_uid,
            STATUS_C_MOVE_UNABLE_TO_PROCESS,
            "access denied",
            counts,
        )?;
        return Err(new_c_move_scp_error(
            STATUS_C_MOVE_UNABLE_TO_PROCESS,
            "access denied",
            Some(Error::sentinel(Kind::StoreAccessDenied)),
        ));
    }
    let (status, comment) = c_move_scp_status_for_error(&err);
    let _ = send_c_move_scp_response(
        ctx,
        assoc,
        pc_id,
        req,
        sop_class_uid,
        status,
        &comment,
        counts,
    );
    Err(err)
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RetrieveScpModel {
    StudyRoot,
    PatientRoot,
    NonPatient(super::nonpatient::NonPatientModel),
}
impl RetrieveScpModel {
    pub(crate) fn move_name_sop(self) -> (&'static str, &'static str) {
        match self {
            RetrieveScpModel::StudyRoot => ("Study Root MOVE", STUDY_ROOT_MOVE_SOP_CLASS_UID),
            RetrieveScpModel::PatientRoot => ("Patient Root MOVE", PATIENT_ROOT_MOVE_SOP_CLASS_UID),
            RetrieveScpModel::NonPatient(m) => (
                "non-patient MOVE",
                m.sop_classes().map(|c| c.2).unwrap_or(""),
            ),
        }
    }
    pub(crate) fn get_name_sop(self) -> (&'static str, &'static str) {
        match self {
            RetrieveScpModel::StudyRoot => ("Study Root GET", STUDY_ROOT_GET_SOP_CLASS_UID),
            RetrieveScpModel::PatientRoot => ("Patient Root GET", PATIENT_ROOT_GET_SOP_CLASS_UID),
            RetrieveScpModel::NonPatient(m) => (
                "non-patient GET",
                m.sop_classes().map(|c| c.3).unwrap_or(""),
            ),
        }
    }
    pub(crate) fn qr_model(self) -> &'static str {
        match self {
            RetrieveScpModel::StudyRoot => "STUDY_ROOT",
            RetrieveScpModel::PatientRoot => "PATIENT_ROOT",
            RetrieveScpModel::NonPatient(_) => "",
        }
    }
    pub(crate) fn level(self, identifier: &Object) -> Result<String> {
        match self {
            RetrieveScpModel::StudyRoot => c_find_level(Some(identifier), false),
            RetrieveScpModel::PatientRoot => c_find_level(Some(identifier), true),
            RetrieveScpModel::NonPatient(_) => Ok(String::new()),
        }
    }
}
#[doc(hidden)]
pub fn serve_c_move_command(
    ctx: Option<&Ctx>,
    assoc: &Association,
    pc_id: u8,
    command: &Object,
    handler: &CMoveHandler,
    patient_root: bool,
) -> Result<()> {
    let model = if patient_root {
        RetrieveScpModel::PatientRoot
    } else {
        RetrieveScpModel::StudyRoot
    };
    serve_c_move_command_model(ctx, assoc, pc_id, command, Some(handler), None, model)
}
pub(crate) fn serve_c_move_command_model(
    ctx: Option<&Ctx>,
    assoc: &Association,
    pc_id: u8,
    command: &Object,
    handler: Option<&CMoveHandler>,
    batch_handler: Option<&CMoveBatchHandler>,
    model: RetrieveScpModel,
) -> Result<()> {
    let (name, sop) = model.move_name_sop();
    let syntax = accepted_scp_transfer_syntax(ctx, assoc, pc_id, sop)?;
    let req = parse_c_move_request(command)?;
    let identifier = receive_identifier_data_set(ctx, assoc, pc_id, syntax)?;
    let empty = SubOperationTally::default();
    if req.affected_sop_class_uid != sop {
        let err = Error::new(format!(
            "dicom dimse: C-MOVE SCP SOP Class UID {}, want {name} {}",
            crate::textfmt::quote(&req.affected_sop_class_uid),
            crate::textfmt::quote(sop)
        ));
        let _ = send_c_move_scp_response(
            ctx,
            assoc,
            pc_id,
            &req,
            sop,
            STATUS_C_MOVE_UNABLE_TO_PROCESS,
            err.text(),
            &empty,
        );
        return Err(err);
    }
    let level = match model.level(&identifier) {
        Ok(l) => l,
        Err(err) => {
            let err = Error::new(err.text().replace("C-FIND", "C-MOVE"));
            let comment = query_retrieve_level_error_comment(Some(&identifier), "C-MOVE");
            send_c_move_scp_response(
                ctx,
                assoc,
                pc_id,
                &req,
                sop,
                STATUS_C_MOVE_IDENTIFIER_DOES_NOT_MATCH_SOP_CLASS,
                &comment,
                &empty,
            )?;
            return Err(new_c_move_scp_error(
                STATUS_C_MOVE_IDENTIFIER_DOES_NOT_MATCH_SOP_CLASS,
                &comment,
                Some(err),
            ));
        }
    };
    if req.move_destination.trim().is_empty() {
        let err = Error::new("dicom dimse: C-MOVE request MoveDestination is required");
        let _ = send_c_move_scp_response(
            ctx,
            assoc,
            pc_id,
            &req,
            sop,
            STATUS_C_MOVE_MOVE_DESTINATION_UNKNOWN,
            err.text(),
            &empty,
        );
        return Err(err);
    }
    if let Err(invalid) = ul::validate_ae_title(&req.move_destination) {
        let err = Error::from(invalid).prefixed("dicom dimse: C-MOVE request MoveDestination");
        let _ = send_c_move_scp_response(
            ctx,
            assoc,
            pc_id,
            &req,
            sop,
            STATUS_C_MOVE_MOVE_DESTINATION_UNKNOWN,
            "Move Destination is not a valid AE title",
            &empty,
        );
        return Err(err);
    }
    let grace = scp_controls(ctx).cancel_grace;
    let request_context = CMoveRequestContext {
        request: req.clone(),
        identifier,
        query_retrieve_level: level,
        presentation_context_id: pc_id,
        identifier_syntax: syntax,
        extended_negotiation: accepted_c_retrieve_extended_negotiation(assoc, sop),
    };
    let (result, op_ctx) = std::thread::scope(|s| {
        let monitor = ScpCancelMonitor::start(
            s,
            ctx,
            assoc,
            pc_id,
            req.message_id,
            Error::sentinel(Kind::CMoveCanceled),
            false,
        );
        let op_ctx = monitor.context().clone();
        let mut counts = SubOperationTally::default();
        let r = run_scp_handler(&op_ctx, Some(assoc), grace, |hctx| {
            if let Some(batch_handler) = batch_handler {
                let batch = batch_handler(hctx, &request_context)?;
                return run_c_move_batch(
                    hctx,
                    ctx,
                    assoc,
                    pc_id,
                    &req,
                    sop,
                    &monitor,
                    batch,
                    &mut counts,
                );
            }
            let Some(handler) = handler else {
                return Err(Error::new("dicom dimse: missing C-MOVE handler"));
            };
            let ops = handler(hctx, &request_context)?;
            validate_c_move_sub_operations(&ops)?;
            counts.remaining = ops.len() as u16;
            for (i, op) in ops.iter().enumerate() {
                if let Some(e) = monitor.operation_error() {
                    return Err(e);
                }
                let store = op.store.as_ref().expect("validated");
                let result = store(hctx);
                if let Some(e) = &result.err {
                    if e.is(Kind::StoreAccessDenied) {
                        return Err(Error::sentinel(Kind::StoreAccessDenied));
                    }
                    if e.is(Kind::CMoveCanceled) || e.is_canceled() {
                        return Err(e.clone());
                    }
                }
                counts.remaining -= 1;
                count_sub_operation_result(&mut counts, &result, &op.affected_sop_instance_uid);
                if let Some(e) = monitor.operation_error() {
                    return Err(e);
                }
                if i < ops.len() - 1 {
                    send_c_move_scp_response(
                        ctx,
                        assoc,
                        pc_id,
                        &req,
                        sop,
                        STATUS_PENDING,
                        "",
                        &counts,
                    )?;
                }
            }
            match monitor.operation_error() {
                Some(e) => Err(e),
                None => Ok(()),
            }
        })
        .err();
        let monitor_err = monitor.stop();
        ((Error::join([r, monitor_err]), counts), op_ctx)
    });
    let (err, counts) = result;
    if let Some(err) = err {
        return send_c_move_operation_error(Some(&op_ctx), assoc, pc_id, &req, sop, &counts, err);
    }
    let status = if counts.failed > 0 || counts.warning > 0 {
        STATUS_C_MOVE_SUB_OPERATIONS_COMPLETE_ONE_OR_MORE_FAILURES
    } else {
        STATUS_SUCCESS
    };
    send_c_move_scp_response(ctx, assoc, pc_id, &req, sop, status, "", &counts)
}
#[allow(clippy::too_many_arguments)]
fn run_c_move_batch(
    hctx: &Ctx,
    response_ctx: Option<&Ctx>,
    assoc: &Association,
    pc_id: u8,
    req: &CMoveRequest,
    sop: &str,
    monitor: &ScpCancelMonitor<'_>,
    batch: CMoveBatch,
    counts: &mut SubOperationTally,
) -> Result<()> {
    validate_c_move_batch(&batch)?;
    let descriptors = batch.sub_operations.clone();
    let n = descriptors.len();
    counts.remaining = n as u16;
    let mut yielded = 0usize;
    let mut yield_err: Option<Error> = None;
    let interrupting = |e: &Error| {
        e.is(Kind::CMoveCanceled)
            || e.is_canceled()
            || e.is_deadline()
            || e.is(Kind::StoreAccessDenied)
    };
    let run = batch.run.clone().expect("validated");
    let run_err = {
        let mut yield_fn = |result: CMoveSubOperationResult| -> Result<()> {
            if let Some(e) = &yield_err {
                return Err(e.clone());
            }
            if yielded >= n {
                let e = batch_contract(format!("yielded more than {n} sub-operations"));
                yield_err = Some(e.clone());
                return Err(e);
            }
            if let Some(e) = result.err.as_ref().filter(|e| interrupting(e)) {
                yield_err = Some(e.clone());
                return Err(e.clone());
            }
            counts.remaining -= 1;
            count_sub_operation_result(
                counts,
                &result,
                &descriptors[yielded].affected_sop_instance_uid,
            );
            yielded += 1;
            if let Some(e) = monitor.operation_error() {
                yield_err = Some(e.clone());
                return Err(e);
            }
            if yielded < n
                && let Err(e) = send_c_move_scp_response(
                    response_ctx,
                    assoc,
                    pc_id,
                    req,
                    sop,
                    STATUS_PENDING,
                    "",
                    counts,
                )
            {
                yield_err = Some(e.clone());
                return Err(e);
            }
            Ok(())
        };
        run(hctx, &mut yield_fn).err()
    };
    let operation_err = monitor.operation_error();
    let had_yield_err = yield_err.is_some();
    let mut err = Error::join([run_err, yield_err, operation_err]);
    let interrupted = err.as_ref().is_some_and(interrupting);
    if !interrupted && !had_yield_err && yielded != n {
        err = Error::join([
            err,
            Some(batch_contract(format!(
                "Run yielded {yielded} of {n} sub-operations"
            ))),
        ]);
    }
    match err {
        Some(e) => Err(e),
        None => Ok(()),
    }
}
