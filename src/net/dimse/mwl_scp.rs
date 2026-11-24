#![forbid(unsafe_code)]
use super::command::*;
use super::controls::*;
use super::error::{Detail, Error, Kind, Result};
use super::find::{
    C_FIND_STATUS_IDENTIFIER_DOES_NOT_MATCH_SOP_CLASS, MODALITY_WORKLIST_FIND_SOP_CLASS_UID,
    send_c_find_final, send_c_find_response,
};
use super::message::*;
use super::monitor::ScpCancelMonitor;
use super::mwl::*;
use super::pdata::{PDataReader, read_pdata_data_set};
use crate::net::ul::{Association, Ctx, Nanos};
use crate::object::{Object, ReadFileOptions, Syntax};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
#[derive(Debug, Clone)]
pub struct ModalityWorklistRequest {
    pub request: CFindRequest,
    pub identifier: ParsedModalityWorklistIdentifier,
    pub raw_identifier: Object,
    pub presentation_context_id: u8,
    pub identifier_syntax: Syntax,
    pub calling_ae_title: String,
    pub called_ae_title: String,
}
pub type ModalityWorklistYield<'a> = dyn FnMut(&Object) -> Result<()> + 'a;
pub trait ModalityWorklistHandler: Send + Sync {
    fn find(
        &self,
        ctx: &Ctx,
        request: &ModalityWorklistRequest,
        yield_: &mut ModalityWorklistYield<'_>,
    ) -> Result<()>;
}
pub struct ModalityWorklistHandlerFunc<F>(pub F);
impl<F> ModalityWorklistHandler for ModalityWorklistHandlerFunc<F>
where
    F: Fn(&Ctx, &ModalityWorklistRequest, &mut ModalityWorklistYield<'_>) -> Result<()>
        + Send
        + Sync,
{
    fn find(
        &self,
        ctx: &Ctx,
        request: &ModalityWorklistRequest,
        yield_: &mut ModalityWorklistYield<'_>,
    ) -> Result<()> {
        (self.0)(ctx, request, yield_)
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ModalityWorklistScpOptions {
    pub parse: ModalityWorklistParseOptions,
    pub allow_universal_query: bool,
    pub max_matches: i64,
    pub max_identifier_bytes: i64,
    pub max_identifier_elements: i64,
    pub max_identifier_depth: i64,
    pub max_response_bytes: i64,
    pub max_response_elements: i64,
    pub max_response_depth: i64,
}
impl ModalityWorklistScpOptions {
    pub(crate) fn normalized(mut self) -> Result<ModalityWorklistScpOptions> {
        if self.max_matches < 0
            || self.max_identifier_bytes < 0
            || self.max_identifier_elements < 0
            || self.max_identifier_depth < 0
            || self.max_response_bytes < 0
            || self.max_response_elements < 0
            || self.max_response_depth < 0
        {
            return Err(Error::new("dicom dimse: MWL limits must not be negative"));
        }
        if self.max_matches == 0 {
            self.max_matches = DEFAULT_MWL_MAX_MATCHES;
        }
        if self.max_identifier_bytes == 0 {
            self.max_identifier_bytes = MAX_IDENTIFIER_BYTES;
        }
        if self.max_identifier_elements == 0 {
            self.max_identifier_elements = DEFAULT_MWL_IDENTIFIER_ELEMENTS;
        }
        if self.max_identifier_depth == 0 {
            self.max_identifier_depth = DEFAULT_MWL_IDENTIFIER_DEPTH;
        }
        if self.max_response_bytes == 0 {
            self.max_response_bytes = MAX_IDENTIFIER_BYTES;
        }
        if self.max_response_elements == 0 {
            self.max_response_elements = DEFAULT_MWL_RESPONSE_ELEMENTS;
        }
        if self.max_response_depth == 0 {
            self.max_response_depth = DEFAULT_MWL_RESPONSE_DEPTH;
        }
        Ok(self)
    }
}
pub fn serve_modality_worklist_c_find(
    ctx: Option<&Ctx>,
    assoc: &Association,
    pc_id: u8,
    handler: Option<&dyn ModalityWorklistHandler>,
) -> Result<()> {
    serve_modality_worklist_c_find_with_options(
        ctx,
        assoc,
        pc_id,
        handler,
        ModalityWorklistScpOptions::default(),
    )
}
pub fn serve_modality_worklist_c_find_with_options(
    ctx: Option<&Ctx>,
    assoc: &Association,
    pc_id: u8,
    handler: Option<&dyn ModalityWorklistHandler>,
    options: ModalityWorklistScpOptions,
) -> Result<()> {
    let Some(handler) = handler else {
        return Err(Error::sentinel(Kind::ModalityWorklistProvider));
    };
    let options = options.normalized()?;
    let command = receive_command_set(ctx, assoc, pc_id)?;
    serve_modality_worklist_c_find_command(ctx, assoc, pc_id, &command, handler, options)
}
fn send_final(
    ctx: Option<&Ctx>,
    assoc: &Association,
    pc_id: u8,
    request: &CFindRequest,
    status: u16,
    comment: &str,
    syntax: &Syntax,
) -> Result<()> {
    send_c_find_final(
        ctx,
        assoc,
        pc_id,
        request,
        MODALITY_WORKLIST_FIND_SOP_CLASS_UID,
        status,
        comment,
        syntax,
    )
}
pub(crate) fn serve_modality_worklist_c_find_command(
    ctx: Option<&Ctx>,
    assoc: &Association,
    pc_id: u8,
    command: &Object,
    handler: &dyn ModalityWorklistHandler,
    options: ModalityWorklistScpOptions,
) -> Result<()> {
    let syntax =
        accepted_scp_transfer_syntax(ctx, assoc, pc_id, MODALITY_WORKLIST_FIND_SOP_CLASS_UID)?;
    let request = parse_c_find_request(command)?;
    if request.affected_sop_class_uid != MODALITY_WORKLIST_FIND_SOP_CLASS_UID {
        send_final(
            ctx,
            assoc,
            pc_id,
            &request,
            C_FIND_STATUS_IDENTIFIER_DOES_NOT_MATCH_SOP_CLASS,
            "Identifier does not match SOP Class",
            &syntax,
        )?;
        return Err(Error::sentinel(Kind::ModalityWorklistIdentifier));
    }
    if request.priority > PRIORITY_LOW {
        send_final(
            ctx,
            assoc,
            pc_id,
            &request,
            C_FIND_STATUS_UNABLE_TO_PROCESS,
            "Unable to process",
            &syntax,
        )?;
        return Err(Error::sentinel(Kind::ModalityWorklistIdentifier));
    }
    let out_of_resources = |err: Error| -> Result<()> {
        send_final(
            ctx,
            assoc,
            pc_id,
            &request,
            C_FIND_STATUS_OUT_OF_RESOURCES,
            "Out of resources",
            &syntax,
        )?;
        Err(err)
    };
    let raw_identifier = match receive_limited_data_set(
        ctx,
        assoc,
        pc_id,
        syntax,
        options.max_identifier_bytes,
        options.max_identifier_elements,
        options.max_identifier_depth,
    ) {
        Ok(o) => o,
        Err(err) => {
            if is_modality_worklist_limit_error(&err) {
                return out_of_resources(Error::sentinel(Kind::ModalityWorklistResourceLimit));
            }
            return Err(err.prefixed("dicom dimse: receive MWL Identifier"));
        }
    };
    if let Err(err) = validate_modality_worklist_object_limits(
        Some(&raw_identifier),
        options.max_identifier_elements,
        options.max_identifier_depth,
    ) {
        return out_of_resources(err);
    }
    let identifier =
        match parse_modality_worklist_identifier_with_options(Some(&raw_identifier), options.parse)
        {
            Ok(p) => p,
            Err(err) => {
                if is_modality_worklist_limit_error(&err) {
                    return out_of_resources(Error::sentinel(Kind::ModalityWorklistResourceLimit));
                }
                send_final(
                    ctx,
                    assoc,
                    pc_id,
                    &request,
                    C_FIND_STATUS_IDENTIFIER_DOES_NOT_MATCH_SOP_CLASS,
                    "Identifier does not match SOP Class",
                    &syntax,
                )?;
                return Err(err);
            }
        };
    if !options.allow_universal_query && !modality_worklist_has_matching_key(&identifier.query) {
        return send_final(ctx, assoc, pc_id, &request, STATUS_SUCCESS, "", &syntax);
    }
    let grace = scp_controls(ctx).cancel_grace;
    let (handler_err, op_ctx) = std::thread::scope(|s| {
        let monitor = ScpCancelMonitor::start(
            s,
            ctx,
            assoc,
            pc_id,
            request.message_id,
            Error::sentinel(Kind::CFindCanceled),
            false,
        );
        let op_ctx = monitor.context().clone();
        let mwl_request = ModalityWorklistRequest {
            request: request.clone(),
            identifier: identifier.clone(),
            raw_identifier: raw_identifier.clone(),
            presentation_context_id: pc_id,
            identifier_syntax: syntax,
            calling_ae_title: assoc.calling_ae_title.clone(),
            called_ae_title: assoc.called_ae_title.clone(),
        };
        let r = run_scp_handler(&op_ctx, Some(assoc), grace, |handler_ctx| {
            let mut matches = 0i64;
            let mut emit = |candidate: &Object| -> Result<()> {
                if let Some(e) = monitor.operation_error() {
                    return Err(e);
                }
                if matches >= options.max_matches {
                    return Err(Error::sentinel(Kind::ModalityWorklistResourceLimit));
                }
                let pending = modality_worklist_pending_status(&identifier, Some(candidate));
                let projected = project_modality_worklist_result_with_limits(
                    &identifier,
                    Some(candidate),
                    options.max_response_elements,
                    options.max_response_bytes,
                    options.max_response_depth,
                )?;
                preflight_modality_worklist_response(
                    &projected,
                    &syntax,
                    options.max_response_elements,
                    options.max_response_bytes,
                    options.max_response_depth,
                )?;
                send_with_scp_response_context(ctx, Some(assoc), |rc| {
                    send_c_find_response(
                        Some(rc),
                        assoc,
                        pc_id,
                        CFindResponse {
                            affected_sop_class_uid: MODALITY_WORKLIST_FIND_SOP_CLASS_UID
                                .to_string(),
                            message_id_being_responded_to: request.message_id,
                            status: pending,
                            ..Default::default()
                        },
                        Some(&projected),
                        &syntax,
                    )
                })?;
                matches += 1;
                Ok(())
            };
            handler.find(handler_ctx, &mwl_request, &mut emit)
        })
        .err();
        let monitor_err = monitor.stop();
        (Error::join([r, monitor_err]), op_ctx)
    });
    if let Some(err) = handler_err {
        let (status, comment) = modality_worklist_scp_status(&err);
        send_final(
            Some(&op_ctx),
            assoc,
            pc_id,
            &request,
            status,
            comment,
            &syntax,
        )?;
        if err.is(Kind::CFindCanceled) || err.is(Kind::ModalityWorklistResourceLimit) {
            return Err(err);
        }
        return Err(Error::sentinel(Kind::ModalityWorklistProvider));
    }
    send_final(ctx, assoc, pc_id, &request, STATUS_SUCCESS, "", &syntax)
}
pub(crate) fn receive_limited_data_set(
    ctx: Option<&Ctx>,
    assoc: &Association,
    pc_id: u8,
    syntax: Syntax,
    max_bytes: i64,
    max_elements: i64,
    max_depth: i64,
) -> Result<Object> {
    let mut reader = PDataReader::typed(
        Some(data_set_read_context(ctx, Some(assoc))),
        Some(assoc),
        pc_id,
        false,
    );
    read_pdata_data_set(
        &mut reader,
        syntax,
        limit_options(max_bytes, max_elements, max_depth),
    )
}
pub(crate) fn limit_options(max_bytes: i64, max_elements: i64, max_depth: i64) -> ReadFileOptions {
    ReadFileOptions {
        max_element_bytes: max_bytes,
        max_total_bytes: max_bytes,
        max_sequence_depth: max_depth,
        max_elements,
        ..Default::default()
    }
}
pub(crate) fn is_modality_worklist_limit_error(err: &Error) -> bool {
    let t = err.text();
    err.is(Kind::ModalityWorklistResourceLimit)
        || err.is_obj(crate::object::Kind::MaxTotalBytes)
        || [
            "dicom: maximum element value size exceeded",
            "dicom: maximum total bytes exceeded",
            "dicom: maximum element count exceeded",
            "dicom: maximum sequence nesting depth exceeded",
        ]
        .iter()
        .any(|l| t.contains(l))
}
pub(crate) fn preflight_modality_worklist_response(
    value: &Object,
    syntax: &Syntax,
    max_elements: i64,
    max_bytes: i64,
    max_depth: i64,
) -> Result<()> {
    validate_modality_worklist_object_limits(Some(value), max_elements, max_depth)?;
    if max_bytes <= 0 {
        return Err(Error::sentinel(Kind::ModalityWorklistResourceLimit));
    }
    match crate::object::writer::write_data_set(value, syntax.uid) {
        Ok(bytes) if bytes.len() as i64 > max_bytes => {
            Err(Error::sentinel(Kind::ModalityWorklistResourceLimit))
        }
        Ok(_) => Ok(()),
        Err(_) => Err(Error::sentinel(Kind::ModalityWorklistProvider)),
    }
}
fn modality_worklist_scp_status(err: &Error) -> (u16, &'static str) {
    if err.is(Kind::CFindCanceled) || err.is_canceled() {
        return (C_FIND_STATUS_CANCEL, "C-FIND canceled");
    }
    if err.is(Kind::ModalityWorklistResourceLimit) {
        return (C_FIND_STATUS_OUT_OF_RESOURCES, "Out of resources");
    }
    (C_FIND_STATUS_UNABLE_TO_PROCESS, "Unable to process")
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct InMemoryModalityWorklistOptions {
    pub universal_query_matches_all: bool,
}
#[derive(Debug, Clone)]
pub struct InMemoryModalityWorklistHandler {
    records: Vec<Object>,
    options: InMemoryModalityWorklistOptions,
}
pub fn new_in_memory_modality_worklist_handler(
    records: &[Option<Object>],
) -> Result<InMemoryModalityWorklistHandler> {
    new_in_memory_modality_worklist_handler_with_options(
        records,
        InMemoryModalityWorklistOptions::default(),
    )
}
pub fn new_in_memory_modality_worklist_handler_with_options(
    records: &[Option<Object>],
    options: InMemoryModalityWorklistOptions,
) -> Result<InMemoryModalityWorklistHandler> {
    let mut cloned = Vec::with_capacity(records.len());
    for (i, record) in records.iter().enumerate() {
        let Some(record) = record else {
            return Err(Error::new(format!(
                "dicom dimse: nil MWL record at index {i}"
            )));
        };
        cloned.push(clone_mwl_object(record));
    }
    Ok(InMemoryModalityWorklistHandler {
        records: cloned,
        options,
    })
}
impl InMemoryModalityWorklistHandler {
    pub(crate) fn from_records(
        records: Vec<Object>,
        options: InMemoryModalityWorklistOptions,
    ) -> Self {
        InMemoryModalityWorklistHandler { records, options }
    }
}
impl ModalityWorklistHandler for InMemoryModalityWorklistHandler {
    fn find(
        &self,
        ctx: &Ctx,
        request: &ModalityWorklistRequest,
        yield_: &mut ModalityWorklistYield<'_>,
    ) -> Result<()> {
        if !self.options.universal_query_matches_all
            && !modality_worklist_has_matching_key(&request.identifier.query)
        {
            return Ok(());
        }
        for record in &self.records {
            if let Some(e) = context_err(ctx) {
                return Err(e);
            }
            let matched = match_modality_worklist(&request.identifier.query, Some(record))
                .map_err(|_| Error::sentinel(Kind::ModalityWorklistProvider))?;
            if !matched {
                continue;
            }
            yield_(&clone_mwl_object(record))?;
        }
        Ok(())
    }
}
pub(crate) fn clone_mwl_object(source: &Object) -> Object {
    Object::from_elements(source.elements().to_vec())
}
#[derive(Clone)]
pub(crate) struct ModalityWorklistCFindRoute {
    pub handler: Arc<dyn ModalityWorklistHandler>,
    pub options: ModalityWorklistScpOptions,
}
#[derive(Debug, Clone, Default)]
pub struct ModalityWorklistFindResult {
    pub final_response: Option<CFindResponse>,
    pub match_count: i64,
    pub cancel_sent: bool,
}
#[derive(Debug, Clone, Default)]
pub struct ModalityWorklistFindOptions {
    pub operation: OperationOptions,
    pub max_matches: i64,
    pub max_response_bytes: i64,
    pub max_response_elements: i64,
    pub max_response_depth: i64,
    pub cancel_drain_timeout: Nanos,
    pub priority: u16,
}
const DEFAULT_MWL_CANCEL_DRAIN_TIMEOUT_MS: i64 = 1000;
impl ModalityWorklistFindOptions {
    fn normalized(mut self) -> Result<ModalityWorklistFindOptions> {
        if self.max_matches < 0
            || self.max_response_bytes < 0
            || self.max_response_elements < 0
            || self.max_response_depth < 0
            || self.cancel_drain_timeout.0 < 0
        {
            return Err(Error::new(
                "dicom dimse: MWL client limits must not be negative",
            ));
        }
        if self.priority > PRIORITY_LOW {
            return Err(Error::new("dicom dimse: MWL priority must be 0, 1, or 2"));
        }
        if self.max_matches == 0 {
            self.max_matches = DEFAULT_MWL_MAX_MATCHES;
        }
        if self.max_response_bytes == 0 {
            self.max_response_bytes = MAX_IDENTIFIER_BYTES;
        }
        if self.max_response_elements == 0 {
            self.max_response_elements = DEFAULT_MWL_RESPONSE_ELEMENTS;
        }
        if self.max_response_depth == 0 {
            self.max_response_depth = DEFAULT_MWL_RESPONSE_DEPTH;
        }
        if self.cancel_drain_timeout.0 == 0 {
            self.cancel_drain_timeout = Nanos::from_millis(DEFAULT_MWL_CANCEL_DRAIN_TIMEOUT_MS);
        }
        self.operation =
            operation_options_with_default_policy(self.operation, OperationErrorPolicy::Abort);
        Ok(self)
    }
}
pub fn modality_worklist_status_error(status: u16, comment: &str) -> Error {
    Error::classified(
        Kind::ModalityWorklistStatus,
        format!("{} 0x{status:04X}", Kind::ModalityWorklistStatus.text()),
    )
    .with_detail(Detail::ScpStatus("MWL C-FIND", status, comment.to_string()))
}
pub(crate) fn c_find_invalid_status_error(status: u16) -> Error {
    Error::classified(
        Kind::CFindInvalidStatus,
        format!("{} 0x{status:04X}", Kind::CFindInvalidStatus.text()),
    )
}
pub struct ModalityWorklistClient<'a> {
    assoc: &'a Association,
    pc_id: u8,
    syntax: Syntax,
    next_id: AtomicU32,
}
pub type ModalityWorklistMatchCallback<'a> = dyn FnMut(&Object) -> Result<()> + 'a;
const MWL_OPERATION: &str = "MWL C-FIND";
impl<'a> ModalityWorklistClient<'a> {
    pub fn new(assoc: &'a Association) -> Result<ModalityWorklistClient<'a>> {
        let pc = accepted_context_for_sop_class(assoc, MODALITY_WORKLIST_FIND_SOP_CLASS_UID)?;
        let syntax = transfer_syntax_for_accepted_context(&pc)?;
        Ok(ModalityWorklistClient {
            assoc,
            pc_id: pc.id,
            syntax,
            next_id: AtomicU32::new(0),
        })
    }
    fn next_message_id(&self) -> u16 {
        loop {
            let id = self.next_id.fetch_add(1, Ordering::SeqCst).wrapping_add(1) as u16;
            if id != 0 {
                return id;
            }
        }
    }
    pub fn find(
        &self,
        ctx: &Ctx,
        query: &ModalityWorklistQuery,
        yield_: Option<&mut ModalityWorklistMatchCallback<'_>>,
    ) -> (ModalityWorklistFindResult, Result<()>) {
        self.find_with_options(
            ModalityWorklistFindOptions {
                operation: OperationOptions {
                    context: Some(ctx.clone()),
                    ..Default::default()
                },
                ..Default::default()
            },
            query,
            yield_,
        )
    }
    fn fail(&self, policy: OperationErrorPolicy, err: Error) -> Error {
        apply_operation_error_policy(
            Some(self.assoc),
            policy,
            new_operation_error(MWL_OPERATION, err, true),
        )
    }
    pub fn find_with_options(
        &self,
        options: ModalityWorklistFindOptions,
        query: &ModalityWorklistQuery,
        yield_: Option<&mut ModalityWorklistMatchCallback<'_>>,
    ) -> (ModalityWorklistFindResult, Result<()>) {
        let mut result = ModalityWorklistFindResult::default();
        let Some(yield_) = yield_ else {
            return (
                result,
                Err(Error::new("dicom dimse: nil MWL match callback")),
            );
        };
        let options = match options.normalized() {
            Ok(o) => o,
            Err(e) => return (result, Err(e)),
        };
        let identifier = match build_modality_worklist_identifier(query) {
            Ok(i) => i,
            Err(e) => return (result, Err(e)),
        };
        let ctx = operation_context(&options.operation);
        if let Some(e) = context_err(&ctx) {
            return (result, Err(new_operation_error(MWL_OPERATION, e, false)));
        }
        let _guard = match begin_association_operation(Some(self.assoc)) {
            Ok(g) => g,
            Err(e) => return (result, Err(new_operation_error(MWL_OPERATION, e, false))),
        };
        let policy = options.operation.error_policy;
        let request = CFindRequest {
            affected_sop_class_uid: MODALITY_WORKLIST_FIND_SOP_CLASS_UID.to_string(),
            message_id: self.next_message_id(),
            priority: options.priority,
        };
        if let Err(e) = send_command_set(Some(&ctx), self.assoc, self.pc_id, &request.command_set())
        {
            return (result, Err(self.fail(policy, e)));
        }
        if let Err(e) = send_data_set(
            Some(&ctx),
            self.assoc,
            self.pc_id,
            &identifier,
            &self.syntax,
        ) {
            return (result, Err(self.fail(policy, e)));
        }
        loop {
            if let Some(e) = context_err(&ctx) {
                return self.cancel_and_drain(result, &request, &options, e);
            }
            let response_ctx = operation_response_context(&ctx, options.operation.response_timeout);
            let received = receive_modality_worklist_response(
                &response_ctx,
                self.assoc,
                self.pc_id,
                self.syntax,
                &options,
            );
            let (response, matched) = match received {
                Ok(v) => v,
                Err(e) => {
                    if let Some(c) = context_err(&ctx) {
                        return self.cancel_and_drain(result, &request, &options, c);
                    }
                    return (result, Err(self.fail(policy, e)));
                }
            };
            if let Err(e) = validate_modality_worklist_response(&response, request.message_id) {
                return (result, Err(self.fail(policy, e)));
            }
            match categorize_c_find_status(response.status) {
                CFindStatusCategory::Pending => {
                    let Some(matched) = matched else {
                        return (
                            result,
                            Err(self
                                .fail(policy, Error::new("pending response missing Identifier"))),
                        );
                    };
                    if result.match_count >= options.max_matches {
                        return self.cancel_and_drain(
                            result,
                            &request,
                            &options,
                            Error::sentinel(Kind::ModalityWorklistResourceLimit),
                        );
                    }
                    result.match_count += 1;
                    if let Err(e) = call_callback(yield_, &matched) {
                        return self.cancel_and_drain(result, &request, &options, e);
                    }
                    if let Some(e) = context_err(&ctx) {
                        return self.cancel_and_drain(result, &request, &options, e);
                    }
                }
                CFindStatusCategory::Success => {
                    if matched.is_some() {
                        return (
                            result,
                            Err(self.fail(policy, Error::new("final response included Identifier"))),
                        );
                    }
                    result.final_response = Some(response);
                    return (result, Ok(()));
                }
                CFindStatusCategory::Failure => {
                    if matched.is_some() {
                        return (
                            result,
                            Err(self.fail(policy, Error::new("final response included Identifier"))),
                        );
                    }
                    let err =
                        modality_worklist_status_error(response.status, &response.error_comment);
                    result.final_response = Some(response);
                    return (result, Err(err));
                }
                CFindStatusCategory::Invalid => {
                    return (
                        result,
                        Err(self.fail(policy, c_find_invalid_status_error(response.status))),
                    );
                }
            }
        }
    }
    fn cancel_and_drain(
        &self,
        mut result: ModalityWorklistFindResult,
        request: &CFindRequest,
        options: &ModalityWorklistFindOptions,
        cause: Error,
    ) -> (ModalityWorklistFindResult, Result<()>) {
        let policy = options.operation.error_policy;
        let drain = Ctx::background().with_optional_timeout(options.cancel_drain_timeout);
        let joined = |e: Error| Error::join([Some(cause.clone()), Some(e)]).expect("two errors");
        let cancel = CCancelRequest {
            message_id_being_responded_to: request.message_id,
        };
        if let Err(e) =
            send_command_set(Some(&drain), self.assoc, self.pc_id, &cancel.command_set())
        {
            return (result, Err(self.fail(policy, joined(e))));
        }
        result.cancel_sent = true;
        loop {
            let (response, matched) = match receive_modality_worklist_response(
                &drain,
                self.assoc,
                self.pc_id,
                self.syntax,
                options,
            ) {
                Ok(v) => v,
                Err(e) => return (result, Err(self.fail(policy, joined(e)))),
            };
            if let Err(e) = validate_modality_worklist_response(&response, request.message_id) {
                return (result, Err(self.fail(policy, joined(e))));
            }
            let category = categorize_c_find_status(response.status);
            if let Err(e) = validate_response_identifier(category, matched.is_some()) {
                return (result, Err(self.fail(policy, joined(e))));
            }
            match category {
                CFindStatusCategory::Pending => continue,
                CFindStatusCategory::Invalid => {
                    return (
                        result,
                        Err(self.fail(policy, joined(c_find_invalid_status_error(response.status)))),
                    );
                }
                _ => {
                    result.final_response = Some(response);
                    return (
                        result,
                        Err(new_operation_error(MWL_OPERATION, cause, false)),
                    );
                }
            }
        }
    }
}
fn call_callback(callback: &mut ModalityWorklistMatchCallback<'_>, matched: &Object) -> Result<()> {
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| callback(matched)));
    match r {
        Ok(Ok(())) => Ok(()),
        Ok(Err(e)) => Err(e
            .relabel(Kind::ModalityWorklistCallback.text())
            .also(Kind::ModalityWorklistCallback)),
        Err(_) => Err(Error::sentinel(Kind::ModalityWorklistCallback)),
    }
}
pub(crate) fn validate_response_identifier(
    category: CFindStatusCategory,
    has_identifier: bool,
) -> Result<()> {
    if category == CFindStatusCategory::Pending && !has_identifier {
        return Err(Error::new(
            "dicom dimse: pending MWL C-FIND response is missing Identifier",
        ));
    }
    if category != CFindStatusCategory::Pending && has_identifier {
        return Err(Error::new(
            "dicom dimse: final MWL C-FIND response included Identifier",
        ));
    }
    Ok(())
}
fn receive_modality_worklist_response(
    ctx: &Ctx,
    assoc: &Association,
    pc_id: u8,
    syntax: Syntax,
    options: &ModalityWorklistFindOptions,
) -> Result<(CFindResponse, Option<Object>)> {
    let command = receive_command_set(Some(ctx), assoc, pc_id)?;
    let response = parse_c_find_response(&command)?;
    if response.command_data_set_type == NO_DATA_SET {
        return Ok((response, None));
    }
    let matched = match receive_limited_data_set(
        Some(ctx),
        assoc,
        pc_id,
        syntax,
        options.max_response_bytes,
        options.max_response_elements,
        options.max_response_depth,
    ) {
        Ok(m) => m,
        Err(err) => {
            if is_modality_worklist_limit_error(&err) {
                return Err(Error::sentinel(Kind::ModalityWorklistResourceLimit));
            }
            return Err(err.prefixed("dicom dimse: receive MWL response Identifier"));
        }
    };
    validate_modality_worklist_object_limits(
        Some(&matched),
        options.max_response_elements,
        options.max_response_depth,
    )?;
    Ok((response, Some(matched)))
}
fn validate_modality_worklist_response(response: &CFindResponse, message_id: u16) -> Result<()> {
    if response.message_id_being_responded_to != message_id {
        return Err(Error::new("dicom dimse: MWL response message ID mismatch"));
    }
    if !response.affected_sop_class_uid.is_empty()
        && response.affected_sop_class_uid != MODALITY_WORKLIST_FIND_SOP_CLASS_UID
    {
        return Err(Error::new("dicom dimse: MWL response SOP Class mismatch"));
    }
    Ok(())
}
