#![forbid(unsafe_code)]
use super::command::*;
use super::controls::*;
use super::error::{Detail, Error, Kind, Result};
use super::find::query_retrieve_level_error_comment;
use super::message::*;
use super::monitor::ScpCancelMonitor;
use super::pdata::{PDataReader, drain_pdata_reader};
use super::retrieve::*;
use super::store::*;
use super::store_source::{OpenedStoreSource, close_store_source, normalize_store_descriptor};
use crate::net::ul::{self, Association, Ctx};
use crate::object::{self, File, Object, ReadFileOptions, Syntax, Tag};
use std::sync::Arc;
use std::time::Duration;
pub fn send_c_get_request(assoc: &Association, pc_id: u8, req: &CGetRequest) -> Result<()> {
    if req.message_id == 0 {
        return Err(Error::new(
            "dicom dimse: C-GET request MessageID must be non-zero",
        ));
    }
    if req.affected_sop_class_uid.is_empty() {
        return Err(Error::new(
            "dicom dimse: C-GET request AffectedSOPClassUID is required",
        ));
    }
    send_command_set(None, assoc, pc_id, &req.command_set())
}
pub fn send_c_get_response(
    ctx: Option<&Ctx>,
    assoc: &Association,
    pc_id: u8,
    rsp: &CGetResponse,
) -> Result<()> {
    if rsp.message_id_being_responded_to == 0 {
        return Err(Error::new(
            "dicom dimse: C-GET response MessageIDBeingRespondedTo must be non-zero",
        ));
    }
    if rsp.affected_sop_class_uid.is_empty() {
        return Err(Error::new(
            "dicom dimse: C-GET response AffectedSOPClassUID is required",
        ));
    }
    if rsp.identifier.is_some() && !retrieve_status_allows_identifier(rsp.status) {
        return Err(Error::new(format!(
            "dicom dimse: C-GET response identifier dataset not allowed for status 0x{:04X}",
            rsp.status
        )));
    }
    send_command_set(ctx, assoc, pc_id, &rsp.command_set(C_GET_RSP))?;
    let Some(identifier) = rsp.response_identifier() else {
        return Ok(());
    };
    if !retrieve_status_allows_identifier(rsp.status) {
        return Ok(());
    }
    let syntax = accepted_transfer_syntax(assoc, pc_id)?;
    send_data_set(ctx, assoc, pc_id, &identifier, &syntax)
}
#[derive(Debug, Clone)]
pub struct CGetProgress {
    pub response: CGetResponse,
    pub final_: bool,
    pub status_class: CMoveStatus,
}
#[derive(Debug, Clone)]
pub struct CGetStoreRequestContext {
    pub request: CStoreRequest,
    pub data_set: Object,
    pub presentation_context_id: u8,
    pub data_set_syntax: Syntax,
}
pub type CGetStoreHandler =
    Arc<dyn Fn(&Ctx, &CGetStoreRequestContext) -> (u16, Option<Error>) + Send + Sync>;
#[derive(Debug, Clone, Copy)]
pub(crate) struct CGetMaxDataSetBytes(pub i64);
pub fn with_c_get_max_data_set_bytes(ctx: &Ctx, max: i64) -> Ctx {
    if max <= 0 {
        return ctx.clone();
    }
    ctx.with_value(CGetMaxDataSetBytes(max))
}
pub(crate) fn has_accepted_storage_scp_role(assoc: &Association) -> bool {
    let accepted: Vec<&str> = assoc
        .accepted_contexts
        .iter()
        .filter(|c| c.abstract_syntax_uid != STUDY_ROOT_GET_SOP_CLASS_UID)
        .map(|c| c.abstract_syntax_uid.as_str())
        .collect();
    assoc
        .accepted_role_selections
        .iter()
        .any(|r| r.scp_role && accepted.contains(&r.sop_class_uid.as_str()))
}
fn abort_c_get_association(assoc: &Association) {
    let ctx = Ctx::background().with_timeout(Duration::from_secs(1));
    let _ = assoc.abort_with_context(Some(&ctx), ul::ABORT_REASON_NOT_SPECIFIED);
}
fn c_get_store_response_context(ctx: &Ctx, assoc: &Association) -> Ctx {
    let respond = ctx.without_cancel();
    if ctx.err().is_some() || assoc.context.err().is_some() {
        return respond.with_timeout(Duration::from_secs(1));
    }
    respond
}
fn receive_spooled_c_get_data_set(
    ctx: &Ctx,
    assoc: &Association,
    pc_id: u8,
    syntax: Syntax,
    dir: &str,
    max_bytes: i64,
) -> (Result<Object>, super::spool::SpoolRelease) {
    let rctx = data_set_read_context(Some(ctx), Some(assoc));
    let mut reader = PDataReader::typed(Some(rctx), Some(assoc), pc_id, false);
    let (result, release) = super::spool::spool_data_set(
        &mut reader,
        syntax,
        &super::spool::SpoolTarget {
            dir: dir.to_string(),
            inline: 0,
            on_remove_error: super::spool::c_get_spool_remove_error(ctx),
        },
        ReadFileOptions {
            max_element_bytes: max_bytes,
            max_total_bytes: max_bytes,
            ..Default::default()
        },
    );
    match result {
        Ok(o) => (Ok(o), release),
        Err(err) => {
            if should_drain_on_error(&err)
                && let Err(d) = drain_pdata_reader(&mut reader)
            {
                return (
                    Err(d.prefixed("dicom dimse: drain rejected dataset")),
                    release,
                );
            }
            (Err(err.prefixed("dicom dimse: receive dataset")), release)
        }
    }
}
fn handle_c_get_store_request(
    ctx: &Ctx,
    assoc: &Association,
    pc_id: u8,
    command: &Object,
    handler: &CGetStoreHandler,
) -> Result<()> {
    let req = parse_c_store_request(command)?;
    let pc = accepted_context_by_id(assoc, pc_id)?;
    let syntax = transfer_syntax_for_accepted_context(&pc)?;
    let max = ctx.value::<CGetMaxDataSetBytes>().map_or(0, |m| m.0);
    let spool_dir = super::spool::c_get_spool_dir(ctx);
    let mut _release = super::spool::SpoolRelease::default();
    let received = if spool_dir.is_empty() {
        receive_data_set_limits(Some(ctx), assoc, pc_id, syntax, max, 0, 0)
    } else {
        let (r, release) =
            receive_spooled_c_get_data_set(ctx, assoc, pc_id, syntax, &spool_dir, max);
        _release = release;
        r.map(|o| (o, 0))
    };
    if let Err(e) = &received
        && storage_data_set_error_status(e) != STATUS_C_STORE_OUT_OF_RESOURCES
    {
        return Err(e.clone());
    }
    let mut handler_err = None;
    let mut status = match received {
        Err(_) => STATUS_C_STORE_OUT_OF_RESOURCES,
        Ok((dataset, _)) => {
            if validate_c_store_data_set(
                &req.affected_sop_class_uid,
                &req.affected_sop_instance_uid,
                &pc,
                Some(&dataset),
            )
            .is_err()
            {
                STATUS_C_STORE_DATA_SET_DOES_NOT_MATCH
            } else if let Err(e) = dataset.check_pixel_data_encoding(&syntax) {
                handler_err = Some(Error::from(e));
                STATUS_C_STORE_CANNOT_UNDERSTAND
            } else {
                let (s, e) = handler(
                    &ctx.without_cancel(),
                    &CGetStoreRequestContext {
                        request: req.clone(),
                        data_set: dataset,
                        presentation_context_id: pc_id,
                        data_set_syntax: syntax,
                    },
                );
                handler_err = e;
                s
            }
        }
    };
    if handler_err.is_some() && status == STATUS_SUCCESS {
        status = STATUS_C_GET_UNABLE_TO_PROCESS;
    }
    let respond = c_get_store_response_context(ctx, assoc);
    send_c_store_response(
        Some(&respond),
        assoc,
        pc_id,
        &CStoreResponse {
            affected_sop_class_uid: req.affected_sop_class_uid.clone(),
            message_id_being_responded_to: req.message_id,
            affected_sop_instance_uid: req.affected_sop_instance_uid.clone(),
            status,
            error_comment: String::new(),
        },
    )
}
fn parse_c_get_response_with_identifier(
    ctx: &Ctx,
    assoc: &Association,
    pc_id: u8,
    command: &Object,
) -> Result<CGetResponse> {
    let mut rsp = parse_c_get_response(command)?;
    if rsp.identifier.is_none() {
        return Ok(rsp);
    }
    let syntax = accepted_transfer_syntax(assoc, pc_id)?;
    let identifier = receive_identifier_data_set(Some(ctx), assoc, pc_id, syntax)?;
    if let Some(v) = identifier.get_strings(FAILED_SOP_INSTANCE_UID_LIST)
        && !v.is_empty()
    {
        rsp.failed_sop_instance_uid_list = Some(v);
    }
    rsp.identifier = Some(identifier);
    Ok(rsp)
}
pub struct CGetHandlers<'a> {
    pub store: Option<&'a CGetStoreHandler>,
    pub on_progress: &'a mut dyn FnMut(&CGetProgress),
}
pub fn send_c_get_with_progress(
    ctx: Option<&Ctx>,
    assoc: &Association,
    pc_id: u8,
    req: &CGetRequest,
    identifier: Option<&Object>,
    syntax: &Syntax,
    handlers: CGetHandlers<'_>,
) -> Result<CGetResponse> {
    let CGetHandlers {
        store: store_handler,
        on_progress,
    } = handlers;
    let ctx = ctx.cloned().unwrap_or_default();
    let Some(identifier) = identifier else {
        return Err(Error::new(
            "dicom dimse: C-GET: identifier dataset is required",
        ));
    };
    let Some(store_handler) = store_handler else {
        return Err(Error::new("dicom dimse: nil C-GET store handler"));
    };
    if !has_accepted_storage_scp_role(assoc) {
        return Err(Error::sentinel(Kind::CGetStorageRoleNotAccepted));
    }
    if let Some(e) = context_err(&ctx) {
        return Err(e);
    }
    let _guard = begin_association_operation(Some(assoc))?;
    send_c_get_request(assoc, pc_id, req)?;
    send_data_set(None, assoc, pc_id, identifier, syntax)?;
    loop {
        if let Some(e) = context_err(&ctx) {
            abort_c_get_association(assoc);
            return Err(e);
        }
        let (command_pc, command) = match receive_command_set_any_raw(Some(&ctx), assoc) {
            Ok(v) => v,
            Err(e) => {
                if ctx.err().is_some() {
                    abort_c_get_association(assoc);
                }
                return Err(e);
            }
        };
        let field = command_uint16(Some(&command), COMMAND_FIELD)?;
        match field {
            C_STORE_RQ => {
                if let Err(e) =
                    handle_c_get_store_request(&ctx, assoc, command_pc, &command, store_handler)
                {
                    if ctx.err().is_some() {
                        abort_c_get_association(assoc);
                    }
                    return Err(e);
                }
            }
            C_GET_RSP => {
                let rsp =
                    match parse_c_get_response_with_identifier(&ctx, assoc, command_pc, &command) {
                        Ok(r) => r,
                        Err(e) => {
                            if ctx.err().is_some() {
                                abort_c_get_association(assoc);
                            }
                            return Err(e);
                        }
                    };
                let cls = classify_c_move_status(rsp.status);
                let event = CGetProgress {
                    response: rsp.clone(),
                    final_: cls != CMoveStatus::Pending,
                    status_class: cls,
                };
                on_progress(&event);
                if event.final_ {
                    return Ok(rsp);
                }
            }
            _ => {
                return Err(Error::classified(
                    Kind::ProtocolViolation,
                    format!("dicom dimse: C-GET received command field 0x{field:04X}"),
                ));
            }
        }
    }
}
#[derive(Debug, Clone)]
pub struct CGetRequestContext {
    pub request: CGetRequest,
    pub identifier: Object,
    pub query_retrieve_level: String,
    pub presentation_context_id: u8,
    pub identifier_syntax: Syntax,
    pub extended_negotiation: CRetrieveExtendedNegotiation,
}
pub type LoadDataSet = Arc<dyn Fn(&Ctx) -> Result<Object> + Send + Sync>;
pub type LoadFile = Arc<dyn Fn(&Ctx) -> (Option<Arc<File>>, Option<Error>) + Send + Sync>;
pub type LoadStoreSource = Arc<dyn Fn(&Ctx) -> (OpenedStoreSource, Option<Error>) + Send + Sync>;
#[derive(Clone, Default)]
pub struct CGetSubOperation {
    pub affected_sop_class_uid: String,
    pub affected_sop_instance_uid: String,
    pub transfer_syntax_uids: Vec<String>,
    pub message_id: u16,
    pub load_data_set: Option<LoadDataSet>,
    pub on_response: Option<OnSubOperationResponse>,
    pub load_file: Option<LoadFile>,
    pub load_store_source: Option<LoadStoreSource>,
}
pub type OnSubOperationResponse = Arc<dyn Fn(&Ctx, u16) + Send + Sync>;
pub type CGetHandler =
    Arc<dyn Fn(&Ctx, &CGetRequestContext) -> Result<Vec<CGetSubOperation>> + Send + Sync>;
pub fn new_c_get_scp_error(status: u16, comment: &str, err: Option<Error>) -> Error {
    new_scp_status_error("C-GET", status, comment, err)
}
pub fn new_c_get_scp_error_with_failed(
    status: u16,
    comment: &str,
    failed: u16,
    err: Option<Error>,
) -> Error {
    new_scp_status_error("C-GET", status, comment, err).with_failed_suboperations(failed)
}
pub(crate) fn c_get_scp_failed_suboperations(err: &Error) -> Option<u16> {
    match err.detail() {
        Detail::ScpStatus("C-GET", ..) => Some(err.failed_suboperations()),
        _ => None,
    }
}
pub(crate) fn validate_c_get_sub_operations(ops: &[CGetSubOperation]) -> Result<()> {
    if ops.len() > (1 << 16) - 1 {
        return Err(Error::new(format!(
            "dicom dimse: too many C-GET sub-operations: {} exceeds {}",
            ops.len(),
            (1 << 16) - 1
        )));
    }
    for (i, op) in ops.iter().enumerate() {
        if op.affected_sop_class_uid.trim().is_empty()
            && (op.load_file.is_none() || op.load_store_source.is_some())
        {
            return Err(Error::new(format!(
                "dicom dimse: C-GET sub-operation {i} missing Affected SOP Class UID"
            )));
        }
        if op.affected_sop_instance_uid.trim().is_empty() {
            return Err(Error::new(format!(
                "dicom dimse: C-GET sub-operation {i} missing Affected SOP Instance UID"
            )));
        }
        if op.load_data_set.is_none() && op.load_file.is_none() && op.load_store_source.is_none() {
            return Err(Error::new(format!(
                "dicom dimse: C-GET sub-operation {i} missing LoadDataSet callback"
            )));
        }
    }
    Ok(())
}
fn check_c_get_storage_context(assoc: &Association, sop: &str) -> Option<CMoveSubOperationResult> {
    if let Err(e) = accepted_context_for_sop_class(assoc, sop) {
        return Some(CMoveSubOperationResult {
            status: STATUS_C_GET_UNABLE_TO_PROCESS,
            err: Some(e),
        });
    }
    if !accepted_storage_scp_role(assoc, sop) {
        return Some(CMoveSubOperationResult {
            status: STATUS_C_GET_UNABLE_TO_PROCESS,
            err: Some(Error::new(format!(
                "dicom dimse: C-GET storage SCP role not accepted for SOP Class UID {}",
                crate::textfmt::quote(sop)
            ))),
        });
    }
    None
}
fn run_c_get_sub_operation(
    load_ctx: &Ctx,
    response_ctx: Option<&Ctx>,
    client: &mut StoreClient<'_>,
    monitor: &ScpCancelMonitor<'_>,
    op: &CGetSubOperation,
    message_id: u16,
    priority: u16,
) -> CMoveSubOperationResult {
    let unable = |e: Error| CMoveSubOperationResult {
        status: STATUS_C_GET_UNABLE_TO_PROCESS,
        err: Some(e),
    };
    let mut sop = op.affected_sop_class_uid.trim().to_string();
    if !sop.is_empty()
        && let Some(r) = check_c_get_storage_context(client.assoc, &sop)
    {
        return r;
    }
    if op.load_store_source.is_some() {
        return run_c_get_store_source(
            load_ctx,
            response_ctx,
            client,
            monitor,
            op,
            message_id,
            priority,
        );
    }
    let mut syntaxes = op.transfer_syntax_uids.clone();
    let file: Option<Arc<File>>;
    let loaded: Option<Object>;
    if let Some(load_file) = &op.load_file {
        let (opened, err) = load_file(load_ctx);
        if let Some(e) = err {
            return unable(e);
        }
        let Some(opened) = opened else {
            return unable(Error::new("dicom dimse: C-GET sub-operation file is nil"));
        };
        if sop.is_empty() {
            sop = opened
                .dataset
                .get_uid(Tag::new(0x0008, 0x0016))
                .unwrap_or_default();
            if !object::is_valid_uid(sop.as_bytes()) {
                return unable(Error::new(
                    "dicom dimse: C-GET sub-operation data set lacks SOP Class UID",
                ));
            }
            if let Some(r) = check_c_get_storage_context(client.assoc, &sop) {
                return r;
            }
        }
        syntaxes = super::retrieve_proxy::store_transfer_syntax_uids(
            opened.transfer_syntax.uid,
            &op.transfer_syntax_uids,
        );
        if syntaxes.is_empty() {
            return unable(Error::sentinel(Kind::StoreTransferSyntax));
        }
        file = Some(opened);
        loaded = None;
    } else {
        let Some(load) = op.load_data_set.as_ref() else {
            return unable(Error::new(
                "dicom dimse: C-GET sub-operation dataset is nil",
            ));
        };
        match load(load_ctx) {
            Ok(d) => loaded = Some(d),
            Err(e) => return unable(e),
        }
        file = None;
    }
    if let Some(e) = context_cause(load_ctx) {
        return unable(e);
    }
    let dataset: &Object = match (&file, &loaded) {
        (Some(f), _) => &f.dataset,
        (None, Some(d)) => d,
        (None, None) => {
            return unable(Error::new(
                "dicom dimse: C-GET sub-operation dataset is nil",
            ));
        }
    };
    let message_id = if op.message_id != 0 {
        op.message_id
    } else {
        message_id
    };
    let receive = |c: &Ctx, pc: u8| monitor.receive_c_store_response(c, pc);
    let rctx = response_ctx.cloned().unwrap_or_default();
    let r = client.store_object(
        Some(&rctx),
        dataset,
        CStoreOptions {
            affected_sop_class_uid: sop,
            affected_sop_instance_uid: op.affected_sop_instance_uid.clone(),
            transfer_syntax_uids: syntaxes,
            message_id,
            priority,
            ..Default::default()
        },
        false,
        Some(&receive),
    );
    let (result, err) = match r {
        Ok(r) => (r, None),
        Err((r, e)) => (r, Some(e)),
    };
    let mut status = STATUS_SUCCESS;
    if let Some(resp) = &result.response {
        status = resp.status;
        observe_c_get_store_response(&op.on_response, &rctx, status);
    }
    CMoveSubOperationResult { status, err }
}
fn observe_c_get_store_response(observer: &Option<OnSubOperationResponse>, ctx: &Ctx, status: u16) {
    if let Some(observer) = observer {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| observer(ctx, status)));
    }
}
fn run_c_get_store_source(
    load_ctx: &Ctx,
    response_ctx: Option<&Ctx>,
    client: &mut StoreClient<'_>,
    monitor: &ScpCancelMonitor<'_>,
    op: &CGetSubOperation,
    message_id: u16,
    priority: u16,
) -> CMoveSubOperationResult {
    let unable = |e: Error| CMoveSubOperationResult {
        status: STATUS_C_GET_UNABLE_TO_PROCESS,
        err: Some(e),
    };
    let pc = match accepted_context_for_sop_class_transfer_syntaxes(
        client.assoc,
        &op.affected_sop_class_uid,
        &op.transfer_syntax_uids,
    ) {
        Ok(pc) => pc,
        Err(e) => return unable(e),
    };
    let Some(load) = &op.load_store_source else {
        return unable(Error::sentinel(Kind::StoreInvalidSource));
    };
    let (mut opened, load_err) =
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| load(load_ctx))) {
            Ok(r) => r,
            Err(_) => (
                OpenedStoreSource::default(),
                Some(Error::sentinel(Kind::StoreInvalidSource)),
            ),
        };
    let close = opened.close.take();
    let had_close = close.is_some();
    let mut result = (|| {
        if let Some(e) = load_err {
            return unable(e);
        }
        if let Some(e) = context_cause(load_ctx) {
            return unable(e);
        }
        let descriptor = normalize_store_descriptor(opened.descriptor.clone());
        let (Ok(descriptor), true, Some(write)) =
            (descriptor, had_close, opened.write_data_set.clone())
        else {
            return unable(Error::sentinel(Kind::StoreInvalidSource));
        };
        if descriptor.sop_class_uid != op.affected_sop_class_uid
            || descriptor.sop_instance_uid != op.affected_sop_instance_uid
        {
            return unable(Error::sentinel(Kind::StoreSourceChanged));
        }
        if !descriptor
            .writable_transfer_syntax_uids
            .contains(&pc.transfer_syntax_uid)
        {
            return unable(Error::sentinel(Kind::StoreTransferSyntax));
        }
        let message_id = if op.message_id != 0 {
            op.message_id
        } else {
            message_id
        };
        if let Err(e) = check_store_access(load_ctx, opened.before_store.as_deref()) {
            return unable(e);
        }
        let receive = |c: &Ctx, pc: u8| monitor.receive_c_store_response(c, pc);
        let rctx = response_ctx.cloned().unwrap_or_default();
        let stored = client.store_streamed(
            Some(&rctx),
            Some(
                &|ctx: &Ctx, sink: &mut dyn FnMut(&[u8]) -> Result<()>, syntax: &Syntax| {
                    write(ctx, sink, syntax)
                },
            ),
            CStoreOptions {
                affected_sop_class_uid: op.affected_sop_class_uid.clone(),
                affected_sop_instance_uid: op.affected_sop_instance_uid.clone(),
                transfer_syntax_uids: vec![pc.transfer_syntax_uid.clone()],
                message_id,
                priority,
                ..Default::default()
            },
            false,
            Some(&receive),
        );
        let (stored, err) = match stored {
            Ok(r) => (r, None),
            Err((r, e)) => (r, Some(e)),
        };
        let mut status = STATUS_SUCCESS;
        if let Some(response) = &stored.response {
            status = response.status;
            observe_c_get_store_response(&op.on_response, &rctx, status);
        }
        CMoveSubOperationResult { status, err }
    })();
    if had_close && close_store_source(close).is_err() {
        result.err = Error::join([
            result.err.take(),
            Some(Error::sentinel(Kind::StoreInvalidSource)),
        ]);
    }
    result
}
pub(crate) fn c_get_scp_status_for_error(err: &Error) -> (u16, String) {
    if let Some((status, comment)) = scp_status_of(err, "C-GET") {
        let comment = if comment.is_empty() {
            let prefix = format!("dicom dimse: C-GET SCP status 0x{status:04X}: ");
            err.text().strip_prefix(&prefix).unwrap_or("").to_string()
        } else {
            comment
        };
        return (status, comment);
    }
    if err.is_deadline() {
        return (STATUS_C_GET_UNABLE_TO_PROCESS, err.text().to_string());
    }
    if err.is(Kind::CGetCanceled) || err.is_canceled() {
        return (STATUS_C_GET_CANCEL, "C-GET canceled".into());
    }
    (STATUS_C_GET_UNABLE_TO_PROCESS, err.text().to_string())
}
#[allow(clippy::too_many_arguments)]
fn send_c_get_scp_response(
    ctx: Option<&Ctx>,
    assoc: &Association,
    pc_id: u8,
    req: &CGetRequest,
    sop: &str,
    status: u16,
    comment: &str,
    counts: &SubOperationTally,
) -> Result<()> {
    let remaining =
        (status == STATUS_PENDING || status == STATUS_C_GET_CANCEL).then_some(counts.remaining);
    send_with_scp_response_context(ctx, Some(assoc), |c| {
        send_c_get_response(
            Some(c),
            assoc,
            pc_id,
            &RetrieveResponse {
                affected_sop_class_uid: sop.to_string(),
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
#[doc(hidden)]
pub fn serve_c_get_command(
    ctx: Option<&Ctx>,
    assoc: &Association,
    pc_id: u8,
    command: &Object,
    handler: &CGetHandler,
    patient_root: bool,
) -> Result<()> {
    let model = if patient_root {
        RetrieveScpModel::PatientRoot
    } else {
        RetrieveScpModel::StudyRoot
    };
    serve_c_get_command_model(ctx, assoc, pc_id, command, handler, model)
}
pub(crate) fn serve_c_get_command_model(
    ctx: Option<&Ctx>,
    assoc: &Association,
    pc_id: u8,
    command: &Object,
    handler: &CGetHandler,
    model: RetrieveScpModel,
) -> Result<()> {
    let (name, sop) = model.get_name_sop();
    let syntax = accepted_scp_transfer_syntax(ctx, assoc, pc_id, sop)?;
    let req = parse_c_get_request(command)?;
    let identifier = receive_identifier_data_set(ctx, assoc, pc_id, syntax)?;
    let empty = SubOperationTally::default();
    if req.affected_sop_class_uid != sop {
        let err = Error::new(format!(
            "dicom dimse: C-GET SCP SOP Class UID {}, want {name} {}",
            crate::textfmt::quote(&req.affected_sop_class_uid),
            crate::textfmt::quote(sop)
        ));
        let _ = send_c_get_scp_response(
            ctx,
            assoc,
            pc_id,
            &req,
            sop,
            STATUS_C_GET_UNABLE_TO_PROCESS,
            err.text(),
            &empty,
        );
        return Err(err);
    }
    let negotiated = accepted_c_retrieve_extended_negotiation(assoc, sop);
    let checked = if !model.qr_model().is_empty()
        && super::retrieve_extras::c_get_identifier_check_enabled(ctx)
    {
        match super::retrieve_extras::parse_c_get_identifier_keys(
            model.qr_model(),
            Some(&identifier),
            &negotiated,
        ) {
            Ok(keys) => Some(keys.level),
            Err(err) => {
                let status = STATUS_C_GET_IDENTIFIER_DOES_NOT_MATCH_SOP_CLASS;
                let comment = super::retrieve_extras::c_get_identifier_error_comment(Some(&err));
                send_c_get_scp_response(ctx, assoc, pc_id, &req, sop, status, &comment, &empty)?;
                return Err(new_c_get_scp_error(status, &comment, Some(err)));
            }
        }
    } else {
        None
    };
    let level = match checked.map_or_else(|| model.level(&identifier), Ok) {
        Ok(l) => l,
        Err(err) => {
            let err = Error::new(err.text().replace("C-FIND", "C-GET"));
            let comment = query_retrieve_level_error_comment(Some(&identifier), "C-GET");
            send_c_get_scp_response(
                ctx,
                assoc,
                pc_id,
                &req,
                sop,
                STATUS_C_GET_IDENTIFIER_DOES_NOT_MATCH_SOP_CLASS,
                &comment,
                &empty,
            )?;
            return Err(new_c_get_scp_error(
                STATUS_C_GET_IDENTIFIER_DOES_NOT_MATCH_SOP_CLASS,
                &comment,
                Some(err),
            ));
        }
    };
    let grace = scp_controls(ctx).cancel_grace;
    let request_context = CGetRequestContext {
        request: req.clone(),
        identifier,
        query_retrieve_level: level,
        presentation_context_id: pc_id,
        identifier_syntax: syntax,
        extended_negotiation: negotiated,
    };
    let ((err, counts, final_comment), op_ctx) = std::thread::scope(|s| {
        let monitor = ScpCancelMonitor::start(
            s,
            ctx,
            assoc,
            pc_id,
            req.message_id,
            Error::sentinel(Kind::CGetCanceled),
            true,
        );
        let op_ctx = monitor.context().clone();
        let mut counts = SubOperationTally::default();
        let mut final_comment = String::new();
        let r = run_scp_handler(&op_ctx, Some(assoc), grace, |hctx| {
            let ops = handler(hctx, &request_context).inspect_err(|e| {
                if let Some(failed) = c_get_scp_failed_suboperations(e) {
                    counts.failed = failed;
                }
            })?;
            validate_c_get_sub_operations(&ops)?;
            counts.remaining = ops.len() as u16;
            let mut client = StoreClient::new(assoc);
            for (i, op) in ops.iter().enumerate() {
                if let Some(e) = monitor.operation_error() {
                    return Err(e);
                }
                let result = run_c_get_sub_operation(
                    hctx,
                    ctx,
                    &mut client,
                    &monitor,
                    op,
                    (i + 1) as u16,
                    req.priority,
                );
                if let Some(e) = &result.err {
                    if e.is(Kind::StoreAccessDenied) {
                        return Err(Error::sentinel(Kind::StoreAccessDenied));
                    }
                    if e.is(Kind::CGetCanceled) || e.is_canceled() {
                        return Err(e.clone());
                    }
                }
                if let Some(e) = monitor.operation_error() {
                    return Err(e);
                }
                if let Some(e) = &result.err {
                    final_comment = e.text().to_string();
                }
                counts.remaining -= 1;
                count_sub_operation_result(&mut counts, &result, &op.affected_sop_instance_uid);
                if let Some(e) = monitor.operation_error() {
                    return Err(e);
                }
                if i < ops.len() - 1 {
                    send_c_get_scp_response(
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
        (
            (Error::join([r, monitor_err]), counts, final_comment),
            op_ctx,
        )
    });
    if let Some(err) = err {
        if err.is(Kind::StoreAccessDenied) {
            send_c_get_scp_response(
                Some(&op_ctx),
                assoc,
                pc_id,
                &req,
                sop,
                STATUS_C_GET_UNABLE_TO_PROCESS,
                "access denied",
                &counts,
            )?;
            return Err(new_c_get_scp_error(
                STATUS_C_GET_UNABLE_TO_PROCESS,
                "access denied",
                Some(Error::sentinel(Kind::StoreAccessDenied)),
            ));
        }
        let (status, comment) = c_get_scp_status_for_error(&err);
        let _ = send_c_get_scp_response(
            Some(&op_ctx),
            assoc,
            pc_id,
            &req,
            sop,
            status,
            &comment,
            &counts,
        );
        return Err(err);
    }
    let status = if counts.failed > 0 || counts.warning > 0 {
        STATUS_C_GET_SUB_OPERATIONS_COMPLETE_ONE_OR_MORE_FAILURES
    } else {
        STATUS_SUCCESS
    };
    send_c_get_scp_response(
        ctx,
        assoc,
        pc_id,
        &req,
        sop,
        status,
        &final_comment,
        &counts,
    )
}
