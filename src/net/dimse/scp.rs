#![forbid(unsafe_code)]
use super::command::*;
use super::controls::*;
use super::echo::serve_c_echo_command;
use super::error::{Detail, Error, Kind, Result};
use super::find::*;
use super::find_extras::CFindRouter;
use super::get::*;
use super::message::*;
use super::nonpatient::{
    non_patient_c_get_handler, non_patient_c_move_batch_handler, non_patient_c_move_handler,
    non_patient_retrieve_model,
};
use super::normalized::*;
use super::retrieve::*;
use super::retrieve_extras::CMoveBatchHandler;
use super::store::*;
use crate::net::ul::{self, Association, Ctx, Pdu};
use crate::object::Object;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;
#[derive(Clone, Default)]
pub struct AssociationScpOptions {
    pub storage: StorageScpOptions,
    pub controls: ScpControls,
    pub c_find_handler: Option<CFindHandler>,
    pub c_move_handler: Option<CMoveHandler>,
    pub c_get_handler: Option<CGetHandler>,
    pub normalized_scp: Option<NormalizedScpOptions>,
    pub c_find_router: Option<CFindRouter>,
    pub check_c_find_identifier: bool,
    pub c_move_batch_handler: Option<CMoveBatchHandler>,
    pub check_c_get_identifier: bool,
}
pub(crate) fn is_handled_scp_status_error(err: &Error) -> bool {
    if [
        Kind::CFindCanceled,
        Kind::CMoveCanceled,
        Kind::CGetCanceled,
        Kind::ModalityWorklistIdentifier,
        Kind::ModalityWorklistProvider,
        Kind::ModalityWorklistResourceLimit,
        Kind::StreamingCFindIdentifier,
        Kind::StreamingCFindProvider,
        Kind::StreamingCFindResourceLimit,
    ]
    .iter()
    .any(|k| err.is(*k))
    {
        return true;
    }
    matches!(
        err.detail(),
        Detail::ScpStatus("C-FIND", ..)
            | Detail::ScpStatus("C-MOVE", ..)
            | Detail::ScpStatus("C-GET", ..)
    )
}
fn association_operation_error_class(err: Option<&Error>) -> &'static str {
    use crate::net::telemetry as t;
    let Some(err) = err else {
        return "";
    };
    if err.is_deadline() || err.is_ul(ul::Kind::AssociationTimeout) {
        t::ERROR_CLASS_TIMEOUT
    } else if err.is_canceled()
        || err.is(Kind::CFindCanceled)
        || err.is(Kind::CMoveCanceled)
        || err.is(Kind::CGetCanceled)
    {
        t::ERROR_CLASS_CANCELED
    } else if is_handled_scp_status_error(err) || is_handled_normalized_scp_error(err) {
        t::ERROR_CLASS_DIMSE_STATUS
    } else if err.is_ul(ul::Kind::UnexpectedPdu)
        || err.is(Kind::PresentationContextMismatch)
        || err.is(Kind::CommandSetTooLarge)
    {
        t::ERROR_CLASS_PROTOCOL
    } else {
        t::ERROR_CLASS_HANDLER_OR_TRANSPORT
    }
}
pub fn serve_association(
    ctx: Option<&Ctx>,
    assoc: &Association,
    opts: &AssociationScpOptions,
) -> Result<()> {
    let ctx = ctx.cloned().unwrap_or_default();
    let _guard = begin_association_operation(Some(assoc))?;
    opts.controls.validate()?;
    let mut ctx = with_scp_controls(&ctx, opts.controls);
    if opts.check_c_find_identifier {
        ctx = super::find_extras::with_c_find_identifier_check(&ctx);
    }
    if opts.check_c_get_identifier {
        ctx = super::retrieve_extras::with_c_get_identifier_check(&ctx);
    }
    loop {
        let read = command_read_context(Some(&ctx));
        let Some((pc_id, command)) = receive_storage_command_or_release(read.as_ref(), assoc)?
        else {
            return Ok(());
        };
        let field = command_uint16(Some(&command), COMMAND_FIELD)?;
        let timer = assoc.observed().then(std::time::Instant::now);
        if timer.is_some() {
            assoc.record_operation_observation(crate::net::telemetry::OperationObservation {
                presentation_context_id: pc_id,
                command_field: field,
                ..Default::default()
            });
        }
        let op_ctx = ctx.with_optional_timeout(opts.controls.operation_timeout);
        let op_ctx = with_scp_response_context(&op_ctx, &ctx);
        let r = run_scp_handler(&op_ctx, Some(assoc), opts.controls.cancel_grace, |hctx| {
            serve_association_command(hctx, assoc, pc_id, &command, field, opts)
        });
        if let Some(started) = timer {
            assoc.record_operation_observation(crate::net::telemetry::OperationObservation {
                completed: true,
                presentation_context_id: pc_id,
                command_field: field,
                duration: started.elapsed().max(std::time::Duration::from_nanos(1)),
                error_class: association_operation_error_class(r.as_ref().err()).to_string(),
            });
        }
        match r {
            Ok(()) => continue,
            Err(e) if is_handled_scp_status_error(&e) || is_handled_normalized_scp_error(&e) => {
                continue;
            }
            Err(e) => return Err(e),
        }
    }
}
fn serve_association_command(
    ctx: &Ctx,
    assoc: &Association,
    pc_id: u8,
    command: &Object,
    field: u16,
    opts: &AssociationScpOptions,
) -> Result<()> {
    match field {
        C_ECHO_RQ => serve_c_echo_command(Some(ctx), assoc, pc_id, command),
        C_STORE_RQ => {
            if opts.storage.store_handler.is_none() && opts.storage.stream_sink.is_none() {
                return Err(Error::new("dicom dimse: missing C-STORE handler"));
            }
            serve_storage_c_store(Some(ctx), assoc, pc_id, command, &opts.storage)
        }
        C_FIND_RQ => {
            let fallback;
            let router = match (&opts.c_find_router, &opts.c_find_handler) {
                (Some(router), _) => router,
                (None, Some(handler)) => {
                    fallback = CFindRouter::from_handler(handler.clone());
                    &fallback
                }
                (None, None) => return Err(Error::new("dicom dimse: missing C-FIND handler")),
            };
            let sop = command_uid(command, AFFECTED_SOP_CLASS_UID)?;
            match sop.as_str() {
                PATIENT_ROOT_FIND_SOP_CLASS_UID => serve_c_find_command(
                    Some(ctx),
                    assoc,
                    pc_id,
                    command,
                    router.hierarchical(),
                    true,
                ),
                STUDY_ROOT_FIND_SOP_CLASS_UID => serve_c_find_command(
                    Some(ctx),
                    assoc,
                    pc_id,
                    command,
                    router.hierarchical(),
                    false,
                ),
                MODALITY_WORKLIST_FIND_SOP_CLASS_UID => {
                    router.serve_modality_worklist(ctx, assoc, pc_id, command)
                }
                other => {
                    let (handled, r) = router.serve_streaming(ctx, assoc, pc_id, command, other);
                    if handled {
                        return r;
                    }
                    Err(Error::new("dicom dimse: unsupported C-FIND SOP Class"))
                }
            }
        }
        C_MOVE_RQ => {
            if opts.c_move_handler.is_none() && opts.c_move_batch_handler.is_none() {
                return Err(Error::new("dicom dimse: missing C-MOVE handler"));
            }
            let sop = command_uid(command, AFFECTED_SOP_CLASS_UID)?;
            let (model, handler, batch) = match non_patient_retrieve_model(&sop) {
                Some(m) => (
                    RetrieveScpModel::NonPatient(m),
                    opts.c_move_handler
                        .clone()
                        .map(|h| non_patient_c_move_handler(m, h)),
                    opts.c_move_batch_handler
                        .clone()
                        .map(|b| non_patient_c_move_batch_handler(m, b)),
                ),
                None => (
                    if sop == PATIENT_ROOT_MOVE_SOP_CLASS_UID {
                        RetrieveScpModel::PatientRoot
                    } else {
                        RetrieveScpModel::StudyRoot
                    },
                    opts.c_move_handler.clone(),
                    opts.c_move_batch_handler.clone(),
                ),
            };
            serve_c_move_command_model(
                Some(ctx),
                assoc,
                pc_id,
                command,
                handler.as_ref(),
                batch.as_ref(),
                model,
            )
        }
        C_GET_RQ => {
            let Some(handler) = &opts.c_get_handler else {
                return Err(Error::new("dicom dimse: missing C-GET handler"));
            };
            let sop = command_uid(command, AFFECTED_SOP_CLASS_UID)?;
            let (model, handler) = match non_patient_retrieve_model(&sop) {
                Some(m) => (
                    RetrieveScpModel::NonPatient(m),
                    non_patient_c_get_handler(m, handler.clone()),
                ),
                None => (
                    if sop == PATIENT_ROOT_GET_SOP_CLASS_UID {
                        RetrieveScpModel::PatientRoot
                    } else {
                        RetrieveScpModel::StudyRoot
                    },
                    handler.clone(),
                ),
            };
            serve_c_get_command_model(Some(ctx), assoc, pc_id, command, &handler, model)
        }
        C_CANCEL_RQ => parse_c_cancel_request(command).map(|_| ()),
        N_EVENT_REPORT_RQ | N_GET_RQ | N_SET_RQ | N_ACTION_RQ | N_CREATE_RQ | N_DELETE_RQ => {
            let default = NormalizedScpOptions::default();
            let options = opts.normalized_scp.as_ref().unwrap_or(&default);
            serve_normalized_command(Some(ctx), assoc, pc_id, command, options)
        }
        _ => Err(Error::new(format!(
            "dicom dimse: unsupported DIMSE SCP command field 0x{field:04X}"
        ))),
    }
}
pub type CommandHandler = Arc<dyn Fn(&Ctx, &Association, u8, &Object) -> Result<()> + Send + Sync>;
pub struct Dispatcher<'a> {
    assoc: &'a Association,
    handlers: Mutex<HashMap<u16, CommandHandler>>,
    once: Mutex<HashMap<(u16, u16), CommandHandler>>,
}
pub fn unexpected_command_error(field: u16, message_id: u16) -> Error {
    Error::classified(
        Kind::UnexpectedCommand,
        format!(
            "{} field=0x{field:04X} messageID={message_id}",
            Kind::UnexpectedCommand.text()
        ),
    )
    .with_detail(Detail::UnexpectedCommand(field, message_id))
}
impl<'a> Dispatcher<'a> {
    pub fn new(assoc: &'a Association) -> Dispatcher<'a> {
        Dispatcher {
            assoc,
            handlers: Mutex::new(HashMap::new()),
            once: Mutex::new(HashMap::new()),
        }
    }
    pub fn handle(&self, field: u16, handler: Option<CommandHandler>) {
        let mut h = self.handlers.lock().unwrap();
        match handler {
            Some(f) => {
                h.insert(field, f);
            }
            None => {
                h.remove(&field);
            }
        }
    }
    pub fn handle_once(&self, field: u16, message_id: u16, handler: Option<CommandHandler>) {
        let mut h = self.once.lock().unwrap();
        match handler {
            Some(f) => {
                h.insert((field, message_id), f);
            }
            None => {
                h.remove(&(field, message_id));
            }
        }
    }
    pub fn remove(&self, field: u16) {
        self.handlers.lock().unwrap().remove(&field);
    }
    pub fn remove_once(&self, field: u16, message_id: u16) {
        self.once.lock().unwrap().remove(&(field, message_id));
    }
    fn take_handler(&self, field: u16, message_id: u16) -> Option<CommandHandler> {
        if let Some(h) = self.once.lock().unwrap().remove(&(field, message_id)) {
            return Some(h);
        }
        self.handlers.lock().unwrap().get(&field).cloned()
    }
    pub fn next(&self, ctx: Option<&Ctx>) -> Result<()> {
        let _guard = begin_association_operation(Some(self.assoc))?;
        self.next_inner(ctx)
    }
    fn next_inner(&self, ctx: Option<&Ctx>) -> Result<()> {
        let ctx = ctx.cloned().unwrap_or_default();
        if let Some(e) = context_err(&ctx) {
            return Err(classify_dispatcher_receive_error(e));
        }
        let (pc_id, command) = match receive_dispatcher_command_or_release(&ctx, self.assoc) {
            Ok(Some(v)) => v,
            Ok(None) => return Err(Error::sentinel(Kind::AssociationReleased)),
            Err((true, e)) => {
                return Err(Error::wrap_msg_err(
                    Kind::AssociationReleased,
                    "release response failed",
                    e,
                ));
            }
            Err((false, e)) => return Err(classify_dispatcher_receive_error(e)),
        };
        let field = command_uint16(Some(&command), COMMAND_FIELD)?;
        let message_id = dispatcher_message_id(&command)?;
        let Some(handler) = self.take_handler(field, message_id) else {
            return Err(unexpected_command_error(field, message_id));
        };
        handler(&ctx, self.assoc, pc_id, &command)
    }
    pub fn run(&self, ctx: Option<&Ctx>) -> Result<()> {
        let _guard = begin_association_operation(Some(self.assoc))?;
        loop {
            match self.next_inner(ctx) {
                Ok(()) => continue,
                Err(e) if e.text() == Kind::AssociationReleased.text() => return Ok(()),
                Err(e) => return Err(e),
            }
        }
    }
}
pub(crate) fn dispatcher_message_id(command: &Object) -> Result<u16> {
    if command.get(MESSAGE_ID_BEING_RESPONDED_TO).is_some() {
        return command_uint16(Some(command), MESSAGE_ID_BEING_RESPONDED_TO);
    }
    if command.get(MESSAGE_ID).is_some() {
        return command_uint16(Some(command), MESSAGE_ID);
    }
    Err(Error::new(
        "dicom dimse: command set has no Message ID correlation element",
    ))
}
fn receive_dispatcher_command_or_release(
    ctx: &Ctx,
    assoc: &Association,
) -> std::result::Result<Option<(u8, Object)>, (bool, Error)> {
    match receive_dimse_command_or_control(ctx, assoc).map_err(|e| (false, e))? {
        CommandOrControl::Command(pc, command, _) => Ok(Some((pc, command))),
        CommandOrControl::Control(Pdu::ReleaseRq) => {
            let r = assoc.send(Some(ctx), &Pdu::ReleaseRp);
            match r {
                Ok(()) => Ok(None),
                Err(e) => {
                    let mut err = Error::from(e);
                    let mut ctx_err = context_err(ctx);
                    if ctx_err.is_none()
                        && err.is_ul(ul::Kind::AssociationTimeout)
                        && ctx.deadline().is_some_and(|d| Instant::now() >= d)
                    {
                        ctx_err =
                            Some(Error::from(ul::Error::sentinel(ul::Kind::DeadlineExceeded)));
                    }
                    if let Some(c) = ctx_err {
                        let same = c.causes().iter().all(|k| err.causes().contains(k));
                        if !same {
                            err = Error::join([Some(err), Some(c)]).expect("two errors");
                        }
                    }
                    Err((true, err))
                }
            }
        }
        CommandOrControl::Control(other) => Err((
            false,
            ul::unexpected_pdu_error(&other, "waiting for P-DATA-TF or A-RELEASE-RQ").into(),
        )),
    }
}
pub(crate) fn classify_dispatcher_receive_error(err: Error) -> Error {
    if err.is_canceled() {
        return Error::wrap_err(Kind::OperationCanceled, err);
    }
    if err.is_deadline() || err.is_ul(ul::Kind::AssociationTimeout) {
        return Error::wrap_err(Kind::OperationTimeout, err);
    }
    err
}
