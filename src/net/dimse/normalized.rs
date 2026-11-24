#![forbid(unsafe_code)]
use super::command::*;
use super::controls::*;
use super::error::{Detail, Error, Kind, Result};
use super::message::*;
use super::pdata::*;
use super::scp::{CommandHandler, Dispatcher};
use crate::net::ul::{AcceptedContext, Association, Ctx};
use crate::object::parser::source::MemFile;
use crate::object::{self, Element, Object, ReadFileOptions, Syntax, Value};
use crate::textfmt::quote;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
pub const MAX_NORMALIZED_DATA_SET_BYTES: i64 = 4 << 20;
pub const MAX_NORMALIZED_DATA_SET_ELEMENTS: i64 = 16_384;
pub const MAX_NORMALIZED_DATA_SET_DEPTH: i64 = 32;
pub const DEFAULT_MAX_INTERLEAVED_EVENT_REPORTS: i64 = 32;
pub const STATUS_NO_SUCH_SOP_CLASS: u16 = 0x0118;
pub const STATUS_NO_SUCH_SOP_INSTANCE: u16 = 0x0112;
pub const STATUS_NO_SUCH_EVENT_TYPE: u16 = 0x0113;
pub const STATUS_NO_SUCH_ACTION_TYPE: u16 = 0x0123;
pub const STATUS_UNRECOGNIZED_OPERATION: u16 = 0x0211;
pub fn normalized_presentation_context_error(
    pc_id: u8,
    abstract_syntax_uid: &str,
    command_sop_class_uid: &str,
) -> Error {
    Error::classified(
        Kind::NormalizedPresentationContext,
        format!(
            "{}: context {pc_id} abstract syntax {}, command SOP Class {}",
            Kind::NormalizedPresentationContext.text(),
            quote(abstract_syntax_uid),
            quote(command_sop_class_uid)
        ),
    )
    .with_detail(Detail::NormalizedPresentationContext(
        pc_id,
        abstract_syntax_uid.to_string(),
        command_sop_class_uid.to_string(),
    ))
}
pub(crate) fn normalized_handler_error(service: &str, err: Option<Error>) -> Option<Error> {
    let err = err?;
    let text = format!("dicom dimse: {service} handler failed: {}", err.text());
    Some(
        err.relabel(text)
            .with_detail(Detail::NormalizedHandler(service.to_string())),
    )
}
fn normalized_scp_status_error(err: Error) -> Error {
    err.also(Kind::NormalizedScpStatusHandled)
}
pub(crate) fn is_handled_normalized_scp_error(err: &Error) -> bool {
    matches!(err.detail(), Detail::NormalizedHandler(_)) || err.is(Kind::NormalizedScpStatusHandled)
}
#[derive(Debug, Clone)]
pub struct NormalizedRequestInfo {
    pub presentation_context: AcceptedContext,
    pub transfer_syntax: Syntax,
}
pub fn context_with_normalized_request_info(ctx: Option<&Ctx>, info: NormalizedRequestInfo) -> Ctx {
    ctx.cloned().unwrap_or_default().with_value(info)
}
pub fn normalized_request_info_from_context(ctx: Option<&Ctx>) -> Option<NormalizedRequestInfo> {
    ctx?.value::<NormalizedRequestInfo>().cloned()
}
pub type NormalizedPresentationContextPolicy =
    Arc<dyn Fn(&AcceptedContext, &str) -> Result<()> + Send + Sync>;
#[derive(Debug, Clone, Default)]
pub struct NormalizedScpResult {
    pub response: NormalizedResponse,
    pub data_set: Option<Object>,
}
pub type NormalizedHandler = Arc<
    dyn Fn(&Ctx, &NormalizedRequest, Option<&Object>) -> (NormalizedScpResult, Option<Error>)
        + Send
        + Sync,
>;
#[derive(Clone, Default)]
pub struct NormalizedScpOptions {
    pub event_report_handler: Option<NormalizedHandler>,
    pub get_handler: Option<NormalizedHandler>,
    pub set_handler: Option<NormalizedHandler>,
    pub action_handler: Option<NormalizedHandler>,
    pub create_handler: Option<NormalizedHandler>,
    pub delete_handler: Option<NormalizedHandler>,
    pub max_data_set_bytes: i64,
    pub presentation_context_policy: Option<NormalizedPresentationContextPolicy>,
}
impl NormalizedScpOptions {
    fn handler(&self, field: u16) -> Option<&NormalizedHandler> {
        match field {
            N_EVENT_REPORT_RQ => self.event_report_handler.as_ref(),
            N_GET_RQ => self.get_handler.as_ref(),
            N_SET_RQ => self.set_handler.as_ref(),
            N_ACTION_RQ => self.action_handler.as_ref(),
            N_CREATE_RQ => self.create_handler.as_ref(),
            N_DELETE_RQ => self.delete_handler.as_ref(),
            _ => None,
        }
    }
}
pub fn register_normalized_handlers(dispatcher: &Dispatcher<'_>, options: NormalizedScpOptions) {
    for field in [
        N_EVENT_REPORT_RQ,
        N_GET_RQ,
        N_SET_RQ,
        N_ACTION_RQ,
        N_CREATE_RQ,
        N_DELETE_RQ,
    ] {
        let options = options.clone();
        let handler: CommandHandler = Arc::new(move |ctx, assoc, pc_id, command| {
            match serve_normalized_command(Some(ctx), assoc, pc_id, command, &options) {
                Err(e) if is_handled_normalized_scp_error(&e) => Ok(()),
                r => r,
            }
        });
        dispatcher.handle(field, Some(handler));
    }
}
fn service_of(field: u16) -> &'static str {
    match field {
        N_EVENT_REPORT_RQ => "N-EVENT-REPORT",
        N_GET_RQ => "N-GET",
        N_SET_RQ => "N-SET",
        N_ACTION_RQ => "N-ACTION",
        N_CREATE_RQ => "N-CREATE",
        N_DELETE_RQ => "N-DELETE",
        _ => "",
    }
}
pub fn serve_normalized_command(
    ctx: Option<&Ctx>,
    assoc: &Association,
    pc_id: u8,
    command: &Object,
    options: &NormalizedScpOptions,
) -> Result<()> {
    let ctx = ctx.cloned().unwrap_or_default();
    let pc = accepted_context_by_id(assoc, pc_id)?;
    let syntax = transfer_syntax_for_accepted_context(&pc)?;
    let field = command_uint16(Some(command), COMMAND_FIELD)?;
    match field {
        N_EVENT_REPORT_RQ | N_GET_RQ | N_SET_RQ | N_ACTION_RQ | N_CREATE_RQ | N_DELETE_RQ => {
            serve_normalized(&ctx, assoc, &pc, syntax, command, field, options)
        }
        _ => Err(super::scp::unexpected_command_error(field, 0)),
    }
}
fn serve_normalized(
    ctx: &Ctx,
    assoc: &Association,
    pc: &AcceptedContext,
    syntax: Syntax,
    command: &Object,
    field: u16,
    options: &NormalizedScpOptions,
) -> Result<()> {
    let service = service_of(field);
    let request = parse_normalized_request(command, field)?;
    let rsp_field = field | 0x8000;
    let data_set = match field {
        N_GET_RQ | N_DELETE_RQ => None,
        N_SET_RQ => receive_normalized_request_data_set(
            ctx,
            assoc,
            pc.id,
            syntax,
            DATA_SET_PRESENT,
            options,
        )?,
        _ => receive_normalized_request_data_set(
            ctx,
            assoc,
            pc.id,
            syntax,
            request.command_data_set_type,
            options,
        )?,
    };
    let bare = |status: u16| NormalizedResponse {
        command_field: rsp_field,
        message_id_being_responded_to: request.message_id,
        command_data_set_type: NO_DATA_SET,
        status,
        ..Default::default()
    };
    if let Err(e) = validate_normalized_scp_presentation_context(
        pc,
        &request.sop_class_uid,
        options.presentation_context_policy.as_ref(),
    ) {
        let response = bare(STATUS_NO_SUCH_SOP_CLASS);
        return send_normalized_scp_response(
            ctx,
            assoc,
            pc.id,
            syntax,
            &response.command_set(),
            None,
            Some(normalized_scp_status_error(e)),
        );
    }
    let Some(handler) = options.handler(field) else {
        let response = bare(STATUS_UNRECOGNIZED_OPERATION);
        return send_normalized_scp_response(
            ctx,
            assoc,
            pc.id,
            syntax,
            &response.command_set(),
            None,
            None,
        );
    };
    let handler_ctx = ctx.with_value(NormalizedRequestInfo {
        presentation_context: pc.clone(),
        transfer_syntax: syntax,
    });
    let (mut result, mut handler_err) = handler(&handler_ctx, &request, data_set.as_ref());
    let mut response_hook = None;
    if field == N_ACTION_RQ
        && let Some(e) = &handler_err
        && let Detail::NormalizedActionResponseHook(hook) = e.detail()
    {
        response_hook = Some(hook.clone());
        handler_err = None;
    }
    let mut response = std::mem::take(&mut result.response);
    response.command_field = rsp_field;
    let typed = matches!(field, N_EVENT_REPORT_RQ | N_ACTION_RQ);
    let correlation_err = Error::join([
        correlate_normalized_response(
            &mut response.affected_sop_class_uid,
            &mut response.affected_sop_instance_uid,
            &mut response.message_id_being_responded_to,
            &request.sop_class_uid,
            &request.sop_instance_uid,
            request.message_id,
            field == N_CREATE_RQ,
        ),
        if typed {
            correlate_normalized_type_id(
                &mut response.type_id,
                request.type_id,
                if field == N_ACTION_RQ {
                    "Action Type ID"
                } else {
                    "Event Type ID"
                },
            )
        } else {
            response.type_id = None;
            None
        },
    ]);
    if let Some(c) = correlation_err {
        response.status = STATUS_PROCESSING_FAILURE;
        result.data_set = None;
        handler_err = Error::join([handler_err, Some(c)]);
    }
    if field == N_DELETE_RQ {
        result.data_set = None;
    }
    if result.data_set.is_some() {
        response.command_data_set_type = DATA_SET_PRESENT;
        if typed && response.status == STATUS_SUCCESS && response.type_id.is_none() {
            response.type_id = Some(request.type_id);
        }
    } else {
        response.command_data_set_type = NO_DATA_SET;
        if field == N_GET_RQ && response.status == STATUS_SUCCESS && handler_err.is_none() {
            handler_err = Some(Error::new(
                "N-GET success requires an Attribute List dataset",
            ));
        }
    }
    if field == N_CREATE_RQ
        && response.status == STATUS_SUCCESS
        && response.affected_sop_instance_uid.is_empty()
        && handler_err.is_none()
    {
        handler_err = Some(Error::new(
            "N-CREATE success requires an Affected SOP Instance UID",
        ));
    }
    normalize_handler_status(&mut response.status, handler_err.as_ref());
    let sent = send_normalized_scp_response(
        ctx,
        assoc,
        pc.id,
        syntax,
        &response.command_set(),
        result.data_set.as_ref(),
        normalized_handler_error(service, handler_err),
    );
    let Some(hook) = response_hook else {
        return sent;
    };
    let response_sent = match &sent {
        Ok(()) => true,
        Err(e) => is_handled_normalized_scp_error(e),
    };
    let observer_err = normalized_handler_error(service, (hook.0)(ctx, &response, response_sent));
    match Error::join([sent.err(), observer_err]) {
        Some(e) => Err(e),
        None => Ok(()),
    }
}
fn receive_normalized_request_data_set(
    ctx: &Ctx,
    assoc: &Association,
    pc_id: u8,
    syntax: Syntax,
    data_set_type: u16,
    options: &NormalizedScpOptions,
) -> Result<Option<Object>> {
    if data_set_type == NO_DATA_SET {
        return Ok(None);
    }
    let max = if options.max_data_set_bytes == 0 {
        MAX_NORMALIZED_DATA_SET_BYTES
    } else {
        options.max_data_set_bytes
    };
    receive_data_set_limits(
        Some(ctx),
        assoc,
        pc_id,
        syntax,
        max,
        MAX_NORMALIZED_DATA_SET_ELEMENTS,
        MAX_NORMALIZED_DATA_SET_DEPTH,
    )
    .map(|(o, _)| Some(o))
}
fn send_normalized_scp_response(
    ctx: &Ctx,
    assoc: &Association,
    pc_id: u8,
    syntax: Syntax,
    command_set: &[Element],
    data_set: Option<&Object>,
    result_err: Option<Error>,
) -> Result<()> {
    let response_ctx = scp_response_context(Some(ctx), Some(assoc));
    send_message_data_set(
        Some(&response_ctx),
        assoc,
        pc_id,
        command_set,
        data_set,
        &syntax,
    )?;
    match result_err {
        Some(e) => Err(e),
        None => Ok(()),
    }
}
pub(crate) fn validate_normalized_scp_presentation_context(
    pc: &AcceptedContext,
    command_sop_class_uid: &str,
    policy: Option<&NormalizedPresentationContextPolicy>,
) -> Result<()> {
    if let Some(p) = policy {
        return p(pc, command_sop_class_uid);
    }
    if pc.abstract_syntax_uid != command_sop_class_uid {
        return Err(normalized_presentation_context_error(
            pc.id,
            &pc.abstract_syntax_uid,
            command_sop_class_uid,
        ));
    }
    Ok(())
}
fn correlate_normalized_response(
    class_uid: &mut String,
    instance_uid: &mut String,
    message_id: &mut u16,
    request_class_uid: &str,
    request_instance_uid: &str,
    request_message_id: u16,
    allow_assigned_instance: bool,
) -> Option<Error> {
    let mut err = None;
    if !class_uid.is_empty() && class_uid != request_class_uid {
        err = Error::join([
            err,
            Some(Error::new(format!(
                "handler response SOP Class UID {} does not match request {}",
                quote(class_uid),
                quote(request_class_uid)
            ))),
        ]);
    }
    *class_uid = request_class_uid.to_string();
    if !request_instance_uid.is_empty() {
        if !instance_uid.is_empty() && instance_uid != request_instance_uid {
            err = Error::join([
                err,
                Some(Error::new(format!(
                    "handler response SOP Instance UID {} does not match request {}",
                    quote(instance_uid),
                    quote(request_instance_uid)
                ))),
            ]);
        }
        *instance_uid = request_instance_uid.to_string();
    } else if !allow_assigned_instance && !instance_uid.is_empty() {
        err = Error::join([
            err,
            Some(Error::new(format!(
                "handler response SOP Instance UID {} has no corresponding request value",
                quote(instance_uid)
            ))),
        ]);
        instance_uid.clear();
    }
    *message_id = request_message_id;
    err
}
fn correlate_normalized_type_id(
    response_type_id: &mut Option<u16>,
    request_type_id: u16,
    name: &str,
) -> Option<Error> {
    let value = (*response_type_id)?;
    if value == request_type_id {
        return None;
    }
    *response_type_id = Some(request_type_id);
    Some(Error::new(format!(
        "handler response {name} {value} does not match request {request_type_id}"
    )))
}
fn normalize_handler_status(status: &mut u16, handler_err: Option<&Error>) {
    if handler_err.is_some() && *status == STATUS_SUCCESS {
        *status = STATUS_PROCESSING_FAILURE;
    }
}
#[derive(Clone, Default)]
pub struct NormalizedScpRoute {
    pub sop_class_uid: String,
    pub options: NormalizedScpOptions,
}
pub fn combine_normalized_scp_options(
    routes: &[NormalizedScpRoute],
) -> Result<NormalizedScpOptions> {
    if routes.is_empty() {
        return Err(Error::new("dicom dimse: no normalized SCP routes"));
    }
    let mut by_class: HashMap<String, NormalizedScpOptions> = HashMap::new();
    let mut combined = NormalizedScpOptions::default();
    let mut has = [false; 6];
    let fields = [
        N_EVENT_REPORT_RQ,
        N_GET_RQ,
        N_SET_RQ,
        N_ACTION_RQ,
        N_CREATE_RQ,
        N_DELETE_RQ,
    ];
    for route in routes {
        let uid = route.sop_class_uid.trim();
        if uid.is_empty() {
            return Err(Error::new(
                "dicom dimse: normalized SCP route SOP Class UID is required",
            ));
        }
        if by_class.contains_key(uid) {
            return Err(Error::new("dicom dimse: duplicate normalized SCP route"));
        }
        by_class.insert(uid.to_string(), route.options.clone());
        if route.options.max_data_set_bytes > combined.max_data_set_bytes {
            combined.max_data_set_bytes = route.options.max_data_set_bytes;
        }
        for (i, f) in fields.iter().enumerate() {
            has[i] |= route.options.handler(*f).is_some();
        }
    }
    let by_class = Arc::new(by_class);
    {
        let by_class = by_class.clone();
        combined.presentation_context_policy =
            Some(Arc::new(move |pc: &AcceptedContext, class: &str| {
                let Some(options) = by_class.get(class) else {
                    return Err(normalized_presentation_context_error(
                        pc.id,
                        &pc.abstract_syntax_uid,
                        class,
                    ));
                };
                if let Some(p) = &options.presentation_context_policy {
                    return p(pc, class);
                }
                if pc.abstract_syntax_uid != class {
                    return Err(normalized_presentation_context_error(
                        pc.id,
                        &pc.abstract_syntax_uid,
                        class,
                    ));
                }
                Ok(())
            }));
    }
    for (i, field) in fields.into_iter().enumerate() {
        if !has[i] {
            continue;
        }
        let by_class = by_class.clone();
        let routed: NormalizedHandler = Arc::new(move |ctx, request, data_set| {
            match by_class
                .get(&request.sop_class_uid)
                .and_then(|o| o.handler(field))
            {
                Some(h) => h(ctx, request, data_set),
                None => (
                    NormalizedScpResult {
                        response: NormalizedResponse {
                            status: STATUS_UNRECOGNIZED_OPERATION,
                            ..Default::default()
                        },
                        data_set: None,
                    },
                    None,
                ),
            }
        });
        match field {
            N_EVENT_REPORT_RQ => combined.event_report_handler = Some(routed),
            N_GET_RQ => combined.get_handler = Some(routed),
            N_SET_RQ => combined.set_handler = Some(routed),
            N_ACTION_RQ => combined.action_handler = Some(routed),
            N_CREATE_RQ => combined.create_handler = Some(routed),
            _ => combined.delete_handler = Some(routed),
        }
    }
    Ok(combined)
}
#[derive(Debug, Clone, Default)]
pub struct NormalizedEventReportSubscription {
    pub sop_class_uid: String,
    pub event_type_ids: Vec<u16>,
}
#[derive(Clone, Default)]
pub struct NormalizedEventReportOptions {
    pub handler: Option<NormalizedHandler>,
    pub subscriptions: Vec<NormalizedEventReportSubscription>,
    pub presentation_context_policy: Option<NormalizedPresentationContextPolicy>,
    pub max_events: i64,
    pub max_data_set_bytes: i64,
}
#[derive(Debug, Clone, Default)]
pub struct NormalizedEventReportOutcome {
    pub message_id: u16,
    pub status: u16,
    pub err: Option<Error>,
}
#[derive(Clone, Default)]
pub struct NormalizedOperationOptions {
    pub operation: OperationOptions,
    pub presentation_context_id: u8,
    pub presentation_context_abstract_syntax_uid: String,
    pub max_response_data_set_bytes: i64,
    pub event_reports: Option<NormalizedEventReportOptions>,
}
#[derive(Debug, Clone, Default)]
pub struct NormalizedResult {
    pub presentation_context: AcceptedContext,
    pub transfer_syntax: Option<Syntax>,
    pub data_set: Option<Object>,
    pub event_reports: Vec<NormalizedEventReportOutcome>,
    pub response: Option<NormalizedResponse>,
}
pub struct NormalizedClient<'a> {
    pub assoc: &'a Association,
    message_id: Mutex<u16>,
}
struct ExchangeRequest<'r> {
    service: &'static str,
    request: &'r NormalizedRequest,
    data_set: Option<&'r Object>,
    response_field: u16,
    require_type_id_with_response: bool,
    require_data_set_on_success: bool,
    require_assigned_instance_on_success: bool,
}
fn require_normalized_uid(service: &str, name: &str, uid: &str) -> Result<()> {
    if uid.trim().is_empty() {
        return Err(Error::new(format!(
            "dicom dimse: {service} {name} is required"
        )));
    }
    Ok(())
}
fn validate_normalized_requested_uids(service: &str, class: &str, instance: &str) -> Result<()> {
    require_normalized_uid(service, "RequestedSOPClassUID", class)?;
    require_normalized_uid(service, "RequestedSOPInstanceUID", instance)
}
fn validate_normalized_data_set_argument(
    service: &str,
    data_set_type: u16,
    data_set: Option<&Object>,
) -> Result<()> {
    let has = data_set_type != NO_DATA_SET;
    if has && data_set.is_none() {
        return Err(Error::new(format!(
            "dicom dimse: {service} CommandDataSetType 0x{data_set_type:04X} requires a dataset"
        )));
    }
    if !has && data_set.is_some() {
        return Err(Error::new(format!(
            "dicom dimse: {service} CommandDataSetType 0x{data_set_type:04X} forbids a dataset"
        )));
    }
    Ok(())
}
impl<'a> NormalizedClient<'a> {
    pub fn new(assoc: &'a Association) -> NormalizedClient<'a> {
        NormalizedClient {
            assoc,
            message_id: Mutex::new(0),
        }
    }
    fn assign_message_id(&self, message_id: u16) -> u16 {
        if message_id != 0 {
            return message_id;
        }
        let mut m = self.message_id.lock().unwrap();
        *m = m.wrapping_add(1);
        if *m == 0 {
            *m = 1;
        }
        *m
    }
    pub fn event_report(
        &self,
        options: &NormalizedOperationOptions,
        mut req: NormalizedRequest,
        event_information: Option<&Object>,
    ) -> (NormalizedResult, Result<()>) {
        const SERVICE: &str = "N-EVENT-REPORT";
        let pre = require_normalized_uid(SERVICE, "AffectedSOPClassUID", &req.sop_class_uid)
            .and_then(|_| {
                require_normalized_uid(SERVICE, "AffectedSOPInstanceUID", &req.sop_instance_uid)
            })
            .and_then(|_| {
                validate_normalized_data_set_argument(
                    SERVICE,
                    req.command_data_set_type,
                    event_information,
                )
            });
        if let Err(e) = pre {
            return (NormalizedResult::default(), Err(e));
        }
        req.command_field = N_EVENT_REPORT_RQ;
        req.message_id = self.assign_message_id(req.message_id);
        self.exchange(
            options,
            ExchangeRequest {
                service: SERVICE,
                request: &req,
                data_set: event_information,
                response_field: N_EVENT_REPORT_RSP,
                require_type_id_with_response: true,
                require_data_set_on_success: false,
                require_assigned_instance_on_success: false,
            },
        )
    }
    pub fn get(
        &self,
        options: &NormalizedOperationOptions,
        mut req: NormalizedRequest,
    ) -> (NormalizedResult, Result<()>) {
        const SERVICE: &str = "N-GET";
        if let Err(e) =
            validate_normalized_requested_uids(SERVICE, &req.sop_class_uid, &req.sop_instance_uid)
        {
            return (NormalizedResult::default(), Err(e));
        }
        req.command_field = N_GET_RQ;
        req.message_id = self.assign_message_id(req.message_id);
        self.exchange(
            options,
            ExchangeRequest {
                service: SERVICE,
                request: &req,
                data_set: None,
                response_field: N_GET_RSP,
                require_type_id_with_response: false,
                require_data_set_on_success: true,
                require_assigned_instance_on_success: false,
            },
        )
    }
    pub fn set(
        &self,
        options: &NormalizedOperationOptions,
        mut req: NormalizedRequest,
        modification_list: Option<&Object>,
    ) -> (NormalizedResult, Result<()>) {
        const SERVICE: &str = "N-SET";
        if let Err(e) =
            validate_normalized_requested_uids(SERVICE, &req.sop_class_uid, &req.sop_instance_uid)
        {
            return (NormalizedResult::default(), Err(e));
        }
        if modification_list.is_none() {
            return (
                NormalizedResult::default(),
                Err(Error::new(format!(
                    "dicom dimse: {SERVICE} Modification List dataset is required"
                ))),
            );
        }
        req.command_field = N_SET_RQ;
        req.message_id = self.assign_message_id(req.message_id);
        self.exchange(
            options,
            ExchangeRequest {
                service: SERVICE,
                request: &req,
                data_set: modification_list,
                response_field: N_SET_RSP,
                require_type_id_with_response: false,
                require_data_set_on_success: false,
                require_assigned_instance_on_success: false,
            },
        )
    }
    pub fn action(
        &self,
        options: &NormalizedOperationOptions,
        mut req: NormalizedRequest,
        action_information: Option<&Object>,
    ) -> (NormalizedResult, Result<()>) {
        const SERVICE: &str = "N-ACTION";
        let pre =
            validate_normalized_requested_uids(SERVICE, &req.sop_class_uid, &req.sop_instance_uid)
                .and_then(|_| {
                    validate_normalized_data_set_argument(
                        SERVICE,
                        req.command_data_set_type,
                        action_information,
                    )
                });
        if let Err(e) = pre {
            return (NormalizedResult::default(), Err(e));
        }
        req.command_field = N_ACTION_RQ;
        req.message_id = self.assign_message_id(req.message_id);
        self.exchange(
            options,
            ExchangeRequest {
                service: SERVICE,
                request: &req,
                data_set: action_information,
                response_field: N_ACTION_RSP,
                require_type_id_with_response: true,
                require_data_set_on_success: false,
                require_assigned_instance_on_success: false,
            },
        )
    }
    pub fn create(
        &self,
        options: &NormalizedOperationOptions,
        mut req: NormalizedRequest,
        attribute_list: Option<&Object>,
    ) -> (NormalizedResult, Result<()>) {
        const SERVICE: &str = "N-CREATE";
        let pre = require_normalized_uid(SERVICE, "AffectedSOPClassUID", &req.sop_class_uid)
            .and_then(|_| {
                validate_normalized_data_set_argument(
                    SERVICE,
                    req.command_data_set_type,
                    attribute_list,
                )
            });
        if let Err(e) = pre {
            return (NormalizedResult::default(), Err(e));
        }
        req.command_field = N_CREATE_RQ;
        req.message_id = self.assign_message_id(req.message_id);
        let assigned = req.sop_instance_uid.is_empty();
        self.exchange(
            options,
            ExchangeRequest {
                service: SERVICE,
                request: &req,
                data_set: attribute_list,
                response_field: N_CREATE_RSP,
                require_type_id_with_response: false,
                require_data_set_on_success: false,
                require_assigned_instance_on_success: assigned,
            },
        )
    }
    pub fn delete(
        &self,
        options: &NormalizedOperationOptions,
        mut req: NormalizedRequest,
    ) -> (NormalizedResult, Result<()>) {
        const SERVICE: &str = "N-DELETE";
        if let Err(e) =
            validate_normalized_requested_uids(SERVICE, &req.sop_class_uid, &req.sop_instance_uid)
        {
            return (NormalizedResult::default(), Err(e));
        }
        req.command_field = N_DELETE_RQ;
        req.message_id = self.assign_message_id(req.message_id);
        self.exchange(
            options,
            ExchangeRequest {
                service: SERVICE,
                request: &req,
                data_set: None,
                response_field: N_DELETE_RSP,
                require_type_id_with_response: false,
                require_data_set_on_success: false,
                require_assigned_instance_on_success: false,
            },
        )
    }
    fn exchange(
        &self,
        options: &NormalizedOperationOptions,
        x: ExchangeRequest<'_>,
    ) -> (NormalizedResult, Result<()>) {
        let mut result = NormalizedResult::default();
        let r = self.exchange_inner(options, &x, &mut result);
        (result, r)
    }
    fn exchange_inner(
        &self,
        options: &NormalizedOperationOptions,
        x: &ExchangeRequest<'_>,
        result: &mut NormalizedResult,
    ) -> Result<()> {
        let service = x.service;
        let assoc = self.assoc;
        let operation = operation_options_with_default_policy(
            options.operation.clone(),
            OperationErrorPolicy::Abort,
        );
        let ctx = operation_context(&operation);
        if let Some(e) = context_err(&ctx) {
            return Err(new_operation_error(service, e, false));
        }
        let mut events = new_normalized_event_report_state(options.event_reports.as_ref())
            .map_err(|e| new_operation_error(service, e, false))?;
        let _guard = begin_association_operation(Some(assoc))
            .map_err(|e| new_operation_error(service, e, false))?;
        let (pc, syntax) =
            normalized_client_presentation_context(assoc, &x.request.sop_class_uid, options)?;
        result.presentation_context = pc.clone();
        result.transfer_syntax = Some(syntax);
        let policy = operation.error_policy;
        let fail = |e: Error| {
            apply_operation_error_policy(Some(assoc), policy, new_operation_error(service, e, true))
        };
        send_message_data_set(
            Some(&ctx),
            assoc,
            pc.id,
            &x.request.command_set(),
            x.data_set,
            &syntax,
        )
        .map_err(fail)?;
        let response_ctx = operation_response_context(&ctx, operation.response_timeout);
        let (command, field) = loop {
            let (response_pc, next) =
                receive_command_set_any(Some(&response_ctx), assoc).map_err(fail)?;
            let field = command_uint16(Some(&next), COMMAND_FIELD).map_err(fail)?;
            if field == N_EVENT_REPORT_RQ
                && let Some(state) = events.as_mut()
            {
                let outcome = state
                    .receive_and_respond(&response_ctx, assoc, response_pc, &next)
                    .map_err(fail)?;
                result.event_reports.push(outcome);
                continue;
            }
            if response_pc != pc.id {
                return Err(fail(Error::sentinel(Kind::PresentationContextMismatch)));
            }
            break (next, field);
        };
        if field != x.response_field {
            return Err(fail(Error::new(format!(
                "dicom dimse: {service} response command field 0x{field:04X}, want 0x{:04X}",
                x.response_field
            ))));
        }
        let response = parse_normalized_response(&command, x.response_field).map_err(fail)?;
        result.response = Some(response.clone());
        let dst = if x.response_field == N_DELETE_RSP {
            NO_DATA_SET
        } else {
            response.command_data_set_type
        };
        validate_normalized_response_correlation(x, &response).map_err(fail)?;
        if dst != NO_DATA_SET {
            let limit = if options.max_response_data_set_bytes == 0 {
                MAX_NORMALIZED_DATA_SET_BYTES
            } else {
                options.max_response_data_set_bytes
            };
            let (data_set, _) =
                receive_data_set_limits(Some(&response_ctx), assoc, pc.id, syntax, limit, 0, 0)
                    .map_err(fail)?;
            result.data_set = Some(data_set);
        }
        if x.require_data_set_on_success
            && response.status == STATUS_SUCCESS
            && result.data_set.is_none()
        {
            return Err(fail(Error::new(format!(
                "dicom dimse: {service} success response requires an Attribute List dataset"
            ))));
        }
        if x.require_type_id_with_response {
            if result.data_set.is_some()
                && response.status == STATUS_SUCCESS
                && response.type_id.is_none()
            {
                return Err(fail(Error::new(format!(
                    "dicom dimse: {service} response dataset requires the conditional type ID"
                ))));
            }
            if let Some(t) = response.type_id
                && t != x.request.type_id
            {
                return Err(fail(Error::new(format!(
                    "dicom dimse: {service} response type ID {t}, want {}",
                    x.request.type_id
                ))));
            }
        }
        if x.require_assigned_instance_on_success
            && response.status == STATUS_SUCCESS
            && response.affected_sop_instance_uid.is_empty()
        {
            return Err(fail(Error::new(format!(
                "dicom dimse: {service} success response must return the SCP-assigned SOP Instance UID"
            ))));
        }
        if let Some(e) =
            check_normalized_status(service, response.status, response.status_fields.as_ref())
        {
            return Err(e);
        }
        Ok(())
    }
}
fn normalized_client_presentation_context(
    assoc: &Association,
    command_sop_class_uid: &str,
    options: &NormalizedOperationOptions,
) -> Result<(AcceptedContext, Syntax)> {
    let explicit = options.presentation_context_abstract_syntax_uid.as_str();
    let lookup = if explicit.is_empty() {
        command_sop_class_uid
    } else {
        explicit
    };
    let pc = if options.presentation_context_id != 0 {
        accepted_context_by_id(assoc, options.presentation_context_id)?
    } else {
        accepted_context_for_sop_class(assoc, lookup)?
    };
    if !explicit.is_empty() && pc.abstract_syntax_uid != explicit {
        return Err(normalized_presentation_context_error(
            pc.id,
            &pc.abstract_syntax_uid,
            explicit,
        ));
    }
    if explicit.is_empty() && pc.abstract_syntax_uid != command_sop_class_uid {
        return Err(normalized_presentation_context_error(
            pc.id,
            &pc.abstract_syntax_uid,
            command_sop_class_uid,
        ));
    }
    let syntax = transfer_syntax_for_accepted_context(&pc)?;
    Ok((pc, syntax))
}
fn validate_normalized_response_correlation(
    x: &ExchangeRequest<'_>,
    response: &NormalizedResponse,
) -> Result<()> {
    let service = x.service;
    let req = x.request;
    if response.message_id_being_responded_to != req.message_id {
        return Err(Error::new(format!(
            "dicom dimse: {service} response MessageIDBeingRespondedTo {}, want {}",
            response.message_id_being_responded_to, req.message_id
        )));
    }
    if !response.affected_sop_class_uid.is_empty()
        && response.affected_sop_class_uid != req.sop_class_uid
    {
        return Err(Error::new(format!(
            "dicom dimse: {service} response AffectedSOPClassUID {}, want {}",
            quote(&response.affected_sop_class_uid),
            quote(&req.sop_class_uid)
        )));
    }
    if !req.sop_instance_uid.is_empty()
        && !response.affected_sop_instance_uid.is_empty()
        && response.affected_sop_instance_uid != req.sop_instance_uid
    {
        return Err(Error::new(format!(
            "dicom dimse: {service} response AffectedSOPInstanceUID {}, want {}",
            quote(&response.affected_sop_instance_uid),
            quote(&req.sop_instance_uid)
        )));
    }
    Ok(())
}
struct EventReportState {
    options: NormalizedEventReportOptions,
    allowed: HashMap<String, HashSet<u16>>,
    count: i64,
}
fn new_normalized_event_report_state(
    options: Option<&NormalizedEventReportOptions>,
) -> Result<Option<EventReportState>> {
    let Some(options) = options else {
        return Ok(None);
    };
    let mut s = EventReportState {
        options: NormalizedEventReportOptions {
            subscriptions: Vec::new(),
            ..options.clone()
        },
        allowed: HashMap::new(),
        count: 0,
    };
    if s.options.max_events < 0 || s.options.max_data_set_bytes < 0 {
        return Err(Error::new("dicom dimse: invalid interleaved event limits"));
    }
    if s.options.max_events == 0 {
        s.options.max_events = DEFAULT_MAX_INTERLEAVED_EVENT_REPORTS;
    }
    if s.options.max_data_set_bytes == 0 {
        s.options.max_data_set_bytes = MAX_NORMALIZED_DATA_SET_BYTES;
    }
    for sub in &options.subscriptions {
        if !object::is_valid_uid(sub.sop_class_uid.as_bytes()) {
            return Err(Error::new(
                "dicom dimse: invalid interleaved event subscription SOP Class UID",
            ));
        }
        if s.allowed.contains_key(&sub.sop_class_uid) {
            return Err(Error::new(
                "dicom dimse: duplicate interleaved event subscription",
            ));
        }
        let mut types = HashSet::new();
        for t in &sub.event_type_ids {
            if !types.insert(*t) {
                return Err(Error::new("dicom dimse: duplicate interleaved event type"));
            }
        }
        s.allowed.insert(sub.sop_class_uid.clone(), types);
    }
    if s.options.handler.is_some() && s.allowed.is_empty() {
        return Err(Error::new(
            "dicom dimse: interleaved event handler requires subscriptions",
        ));
    }
    Ok(Some(s))
}
impl EventReportState {
    fn receive_and_respond(
        &mut self,
        ctx: &Ctx,
        assoc: &Association,
        pc_id: u8,
        command: &Object,
    ) -> Result<NormalizedEventReportOutcome> {
        let mut outcome = NormalizedEventReportOutcome::default();
        if let Some(e) = context_err(ctx) {
            return Err(e);
        }
        if self.count >= self.options.max_events {
            return Err(Error::new("dicom dimse: interleaved event count exceeded"));
        }
        self.count += 1;
        let request = parse_normalized_request(command, N_EVENT_REPORT_RQ)?;
        if !object::is_valid_uid(request.sop_class_uid.as_bytes())
            || !object::is_valid_uid(request.sop_instance_uid.as_bytes())
        {
            return Err(Error::new("dicom dimse: invalid interleaved event SOP UID"));
        }
        let pc = accepted_context_by_id(assoc, pc_id)?;
        let syntax = transfer_syntax_for_accepted_context(&pc)?;
        let mut data_set = None;
        if request.command_data_set_type != NO_DATA_SET {
            data_set = Some(receive_interleaved_event_data_set(
                ctx,
                assoc,
                pc.id,
                syntax,
                self.options.max_data_set_bytes,
            )?);
        }
        let mut response = NormalizedResponse {
            command_field: N_EVENT_REPORT_RSP,
            status: STATUS_SUCCESS,
            ..Default::default()
        };
        if let Err(e) = validate_interleaved_event_context(
            &pc,
            &request.sop_class_uid,
            self.options.presentation_context_policy.as_ref(),
        ) {
            response.status = STATUS_NO_SUCH_SOP_CLASS;
            outcome.err = Some(e);
        } else if self.options.handler.is_none() {
            response.status = STATUS_UNRECOGNIZED_OPERATION;
        } else if !self.allowed.contains_key(&request.sop_class_uid) {
            response.status = STATUS_NO_SUCH_SOP_CLASS;
        } else if !self.allowed[&request.sop_class_uid].contains(&request.type_id) {
            response.status = STATUS_NO_SUCH_EVENT_TYPE;
        } else {
            let handler_ctx = ctx.with_value(NormalizedRequestInfo {
                presentation_context: pc.clone(),
                transfer_syntax: syntax,
            });
            let handler = self.options.handler.clone().expect("checked");
            let (result, handler_err) =
                match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    handler(&handler_ctx, &request, data_set.as_ref())
                })) {
                    Ok(r) => r,
                    Err(_) => (
                        NormalizedScpResult::default(),
                        Some(Error::new(
                            "dicom dimse: interleaved event handler panicked",
                        )),
                    ),
                };
            response = result.response.clone();
            response.command_field = N_EVENT_REPORT_RSP;
            let mut correlation_err = Error::join([
                correlate_normalized_response(
                    &mut response.affected_sop_class_uid,
                    &mut response.affected_sop_instance_uid,
                    &mut response.message_id_being_responded_to,
                    &request.sop_class_uid,
                    &request.sop_instance_uid,
                    request.message_id,
                    false,
                ),
                correlate_normalized_type_id(
                    &mut response.type_id,
                    request.type_id,
                    "Event Type ID",
                ),
            ]);
            if result.response.message_id_being_responded_to != 0
                && result.response.message_id_being_responded_to != request.message_id
            {
                correlation_err = Error::join([
                    correlation_err,
                    Some(Error::new(
                        "dicom dimse: interleaved handler response message ID mismatch",
                    )),
                ]);
            }
            if correlation_err.is_some() {
                response.status = STATUS_PROCESSING_FAILURE;
            }
            outcome.err = Error::join([handler_err, correlation_err]);
            normalize_handler_status(&mut response.status, outcome.err.as_ref());
            let mut reply = None;
            if outcome.err.is_none()
                && let Some(ds) = &result.data_set
            {
                match encode_interleaved_event_reply(
                    ctx,
                    ds,
                    syntax,
                    self.options.max_data_set_bytes,
                ) {
                    Ok(b) => reply = Some(b),
                    Err(e) => {
                        outcome.err = Some(e);
                        response.status = STATUS_PROCESSING_FAILURE;
                    }
                }
            }
            if let Some(e) = context_err(ctx) {
                return Err(e);
            }
            return send_interleaved_event_response(
                ctx, assoc, &pc, syntax, &request, response, reply, outcome,
            );
        }
        send_interleaved_event_response(ctx, assoc, &pc, syntax, &request, response, None, outcome)
    }
}
fn receive_interleaved_event_data_set(
    ctx: &Ctx,
    assoc: &Association,
    pc_id: u8,
    syntax: Syntax,
    limit: i64,
) -> Result<Object> {
    let read_ctx = data_set_read_context(Some(ctx), Some(assoc));
    let mut reader = PDataReader::typed(Some(read_ctx), Some(assoc), pc_id, false);
    let opts = ReadFileOptions {
        max_element_bytes: limit,
        max_total_bytes: limit,
        max_elements: MAX_NORMALIZED_DATA_SET_ELEMENTS,
        max_sequence_depth: MAX_NORMALIZED_DATA_SET_DEPTH,
        max_fragments: MAX_NORMALIZED_DATA_SET_ELEMENTS,
        ..Default::default()
    };
    let data_set = read_pdata_data_set_wire_limited(&mut reader, syntax, opts, limit)?;
    validate_interleaved_event_graph(ctx, &data_set)?;
    Ok(data_set)
}
fn validate_interleaved_event_context(
    pc: &AcceptedContext,
    class: &str,
    policy: Option<&NormalizedPresentationContextPolicy>,
) -> Result<()> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        validate_normalized_scp_presentation_context(pc, class, policy)
    })) {
        Ok(r) => r,
        Err(_) => Err(Error::new(
            "dicom dimse: interleaved event context policy panicked",
        )),
    }
}
fn validate_interleaved_event_graph(ctx: &Ctx, data_set: &Object) -> Result<()> {
    if data_set.len() as i64 > MAX_NORMALIZED_DATA_SET_ELEMENTS {
        return Err(Error::new(
            "dicom dimse: interleaved event graph node limit exceeded",
        ));
    }
    let mut remaining = MAX_NORMALIZED_DATA_SET_ELEMENTS;
    fn visit(ctx: &Ctx, elements: &[Element], depth: i64, remaining: &mut i64) -> Result<()> {
        if let Some(e) = context_err(ctx) {
            return Err(e);
        }
        if depth > MAX_NORMALIZED_DATA_SET_DEPTH {
            return Err(Error::new(
                "dicom dimse: interleaved event graph depth limit exceeded",
            ));
        }
        if elements.len() as i64 > *remaining {
            return Err(Error::new(
                "dicom dimse: interleaved event graph node limit exceeded",
            ));
        }
        *remaining -= elements.len() as i64;
        for element in elements {
            match &element.value {
                Value::Sequence(items) => {
                    if depth + 1 > MAX_NORMALIZED_DATA_SET_DEPTH || items.len() as i64 > *remaining
                    {
                        return Err(Error::new(
                            "dicom dimse: interleaved event sequence graph limit exceeded",
                        ));
                    }
                    *remaining -= items.len() as i64;
                    for item in items {
                        visit(ctx, &item.elements, depth + 2, remaining)?;
                    }
                }
                Value::Fragments { fragments, .. } => {
                    if fragments.len() as i64 > *remaining {
                        return Err(Error::new(
                            "dicom dimse: interleaved event fragment graph limit exceeded",
                        ));
                    }
                    *remaining -= fragments.len() as i64;
                }
                _ => {}
            }
        }
        Ok(())
    }
    visit(ctx, data_set.elements(), 0, &mut remaining)
}
fn encode_interleaved_event_reply(
    ctx: &Ctx,
    data_set: &Object,
    syntax: Syntax,
    limit: i64,
) -> Result<Vec<u8>> {
    validate_interleaved_event_graph(ctx, data_set)?;
    let logical = if syntax.deflated {
        object::transfer::EXPLICIT_VR_LITTLE_ENDIAN
    } else {
        syntax
    };
    let bytes = object::writer::write_data_set(data_set, logical.uid).map_err(Error::from)?;
    if let Some(e) = context_err(ctx) {
        return Err(e);
    }
    if bytes.len() as i64 > limit {
        return Err(Error::new(
            "dicom dimse: interleaved event reply exceeds dataset limit",
        ));
    }
    if bytes.is_empty() {
        return Err(Error::new(
            "dicom dimse: interleaved event reply dataset is empty",
        ));
    }
    let validated = object::read_data_set_with_options(
        Box::new(MemFile::new(std::rc::Rc::from(bytes.as_slice()))),
        logical,
        ReadFileOptions {
            max_element_bytes: limit,
            max_total_bytes: limit,
            max_elements: MAX_NORMALIZED_DATA_SET_ELEMENTS,
            max_sequence_depth: MAX_NORMALIZED_DATA_SET_DEPTH,
            ..Default::default()
        },
    )
    .map_err(Error::from)?;
    if let Some(e) = context_err(ctx) {
        return Err(e);
    }
    if syntax.deflated {
        let wire = object::writer::write_data_set(&validated, syntax.uid).map_err(Error::from)?;
        if wire.len() as i64 > limit {
            return Err(Error::new(
                "dicom dimse: interleaved event reply exceeds dataset limit",
            ));
        }
        return Ok(wire);
    }
    Ok(bytes)
}
#[allow(clippy::too_many_arguments)]
fn send_interleaved_event_response(
    ctx: &Ctx,
    assoc: &Association,
    pc: &AcceptedContext,
    syntax: Syntax,
    request: &NormalizedRequest,
    mut response: NormalizedResponse,
    reply: Option<Vec<u8>>,
    mut outcome: NormalizedEventReportOutcome,
) -> Result<NormalizedEventReportOutcome> {
    response.command_field = N_EVENT_REPORT_RSP;
    response.affected_sop_class_uid = request.sop_class_uid.clone();
    response.affected_sop_instance_uid = request.sop_instance_uid.clone();
    response.message_id_being_responded_to = request.message_id;
    response.type_id = Some(request.type_id);
    response.command_data_set_type = NO_DATA_SET;
    let r = match reply {
        Some(reply) => {
            response.command_data_set_type = DATA_SET_PRESENT;
            send_message(
                Some(ctx),
                assoc,
                pc.id,
                &response.command_set(),
                &syntax,
                |w| w.write(&reply).map(|_| ()),
            )
        }
        None => send_command_set(Some(ctx), assoc, pc.id, &response.command_set()),
    };
    outcome.message_id = request.message_id;
    outcome.status = response.status;
    r.map(|_| outcome)
}
