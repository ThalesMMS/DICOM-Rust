#![forbid(unsafe_code)]
use super::coercion::*;
use super::command::*;
use super::controls::*;
use super::echo::serve_c_echo_command;
use super::error::{Detail, Error, Kind, Result};
use super::message::*;
use super::pdata::*;
use super::spool::{OnSpoolRemoveError, SpoolRelease, SpoolTarget, spool_data_set};
use super::stream::{CStoreStreamSink, serve_storage_c_store_stream};
use crate::dictionary::uid as uiddict;
use crate::net::ul::{AcceptedContext, Association, Ctx};
use crate::object::clone::CloneFileOptions;
use crate::object::{self, Object, ReadFileOptions, Syntax, Tag, Value};
use std::sync::Arc;
pub const STATUS_C_STORE_OUT_OF_RESOURCES: u16 = 0xA700;
pub const STATUS_C_STORE_DATA_SET_DOES_NOT_MATCH: u16 = 0xA900;
pub const STATUS_C_STORE_CANNOT_UNDERSTAND: u16 = 0xC000;
pub const STATUS_C_STORE_COERCION_WARNING: u16 = 0xB000;
const TAG_SOP_CLASS_UID: Tag = Tag::new(0x0008, 0x0016);
const TAG_SOP_INSTANCE_UID: Tag = Tag::new(0x0008, 0x0018);
pub fn new_scp_status_error(
    service: &'static str,
    status: u16,
    comment: &str,
    err: Option<Error>,
) -> Error {
    let text = match (&err, comment.is_empty()) {
        (Some(e), false) => format!(
            "dicom dimse: {service} SCP status 0x{status:04X}: {comment}: {}",
            e.text()
        ),
        (Some(e), true) => format!(
            "dicom dimse: {service} SCP status 0x{status:04X}: {}",
            e.text()
        ),
        (None, false) => format!("dicom dimse: {service} SCP status 0x{status:04X}: {comment}"),
        (None, true) => format!("dicom dimse: {service} SCP status 0x{status:04X}"),
    };
    let base = match err {
        Some(e) => e.relabel(text),
        None => Error::new(text),
    };
    base.with_detail(Detail::ScpStatus(service, status, comment.to_string()))
}
pub fn scp_status_of(err: &Error, service: &str) -> Option<(u16, String)> {
    match err.detail() {
        Detail::ScpStatus(s, status, comment) if *s == service => Some((*status, comment.clone())),
        _ => None,
    }
}
pub fn send_c_store_request(
    ctx: Option<&Ctx>,
    assoc: &Association,
    pc_id: u8,
    req: &CStoreRequest,
) -> Result<()> {
    if req.affected_sop_class_uid.is_empty() {
        return Err(Error::new(
            "dicom dimse: missing C-STORE Affected SOP Class UID",
        ));
    }
    if req.affected_sop_instance_uid.is_empty() {
        return Err(Error::new(
            "dicom dimse: missing C-STORE Affected SOP Instance UID",
        ));
    }
    send_command_set(ctx, assoc, pc_id, &req.command_set())
}
pub fn receive_c_store_request(assoc: &Association, pc_id: u8) -> Result<CStoreRequest> {
    parse_c_store_request(&receive_command_set(None, assoc, pc_id)?)
}
pub fn receive_c_store_request_any(assoc: &Association) -> Result<(u8, CStoreRequest)> {
    let (pc, command) = receive_command_set_any_raw(None, assoc)?;
    Ok((pc, parse_c_store_request(&command)?))
}
pub fn send_c_store_response(
    ctx: Option<&Ctx>,
    assoc: &Association,
    pc_id: u8,
    rsp: &CStoreResponse,
) -> Result<()> {
    if rsp.affected_sop_class_uid.is_empty() {
        return Err(Error::new(
            "dicom dimse: missing C-STORE response Affected SOP Class UID",
        ));
    }
    if rsp.affected_sop_instance_uid.is_empty() {
        return Err(Error::new(
            "dicom dimse: missing C-STORE response Affected SOP Instance UID",
        ));
    }
    send_command_set(ctx, assoc, pc_id, &rsp.command_set())
}
pub fn receive_c_store_response(
    ctx: Option<&Ctx>,
    assoc: &Association,
    pc_id: u8,
) -> Result<CStoreResponse> {
    parse_c_store_response(&receive_command_set(ctx, assoc, pc_id)?)
}
pub fn is_c_store_warning_status(status: u16) -> bool {
    matches!(status, 0x0001 | 0x0107 | 0x0116 | 0xB000..=0xBFFF)
}
pub fn check_c_store_status(response: &CStoreResponse) -> Result<()> {
    if response.status == STATUS_SUCCESS || is_c_store_warning_status(response.status) {
        return Ok(());
    }
    let text = if response.error_comment.is_empty() {
        format!(
            "dicom dimse: C-STORE failed with status 0x{:04X}",
            response.status
        )
    } else {
        format!(
            "dicom dimse: C-STORE failed with status 0x{:04X}: {}",
            response.status, response.error_comment
        )
    };
    Err(Error::new(text).with_detail(Detail::Status(response.status)))
}
#[derive(Debug, Clone, Default)]
pub struct CStoreOptions {
    pub affected_sop_class_uid: String,
    pub affected_sop_instance_uid: String,
    pub transfer_syntax_uids: Vec<String>,
    pub message_id: u16,
    pub priority: u16,
    pub move_originator_ae_title: String,
    pub move_originator_message_id: Option<u16>,
}
#[derive(Debug, Clone, Default)]
pub struct CStoreResult {
    pub request: CStoreRequest,
    pub response: Option<CStoreResponse>,
    pub presentation_context: AcceptedContext,
    pub transfer_syntax: Option<Syntax>,
}
pub(crate) type CStoreStreamWriter<'a> =
    dyn Fn(&Ctx, &mut dyn FnMut(&[u8]) -> Result<()>, &Syntax) -> Result<()> + 'a;
pub(crate) type ResponseReceiver<'a> = dyn Fn(&Ctx, u8) -> Result<CStoreResponse> + 'a;
pub struct StoreClient<'a> {
    pub assoc: &'a Association,
    message_id: u16,
}
impl<'a> StoreClient<'a> {
    pub fn new(assoc: &'a Association) -> StoreClient<'a> {
        StoreClient {
            assoc,
            message_id: 0,
        }
    }
    pub fn store(&mut self, ctx: Option<&Ctx>, dataset: &Object) -> Result<Option<CStoreResponse>> {
        match self.store_with_options(ctx, dataset, CStoreOptions::default()) {
            Ok(r) => Ok(r.response),
            Err((r, e)) => {
                if matches!(e.detail(), Detail::Status(_)) {
                    Ok(r.response)
                } else {
                    Err(e)
                }
            }
        }
    }
    #[allow(clippy::result_large_err)]
    pub fn store_with_options(
        &mut self,
        ctx: Option<&Ctx>,
        dataset: &Object,
        opts: CStoreOptions,
    ) -> std::result::Result<CStoreResult, (CStoreResult, Error)> {
        self.store_object(ctx, dataset, opts, true, None)
    }
    #[allow(clippy::result_large_err)]
    pub(crate) fn store_object(
        &mut self,
        ctx: Option<&Ctx>,
        dataset: &Object,
        mut opts: CStoreOptions,
        acquire: bool,
        receive: Option<&ResponseReceiver<'_>>,
    ) -> std::result::Result<CStoreResult, (CStoreResult, Error)> {
        let fail = |e: Error| (CStoreResult::default(), e);
        if opts.priority > 2 {
            return Err(fail(Error::sentinel(Kind::StoreInvalidOptions)));
        }
        let ctx = ctx.cloned().unwrap_or_default();
        if let Some(e) = context_err(&ctx) {
            return Err(fail(e));
        }
        if opts.affected_sop_class_uid.trim().is_empty() {
            match dataset.get_uid(TAG_SOP_CLASS_UID) {
                Some(u) if !u.is_empty() => opts.affected_sop_class_uid = u,
                _ => {
                    return Err(fail(Error::new(
                        "dicom dimse: dataset missing SOP Class UID (0008,0016)",
                    )));
                }
            }
        }
        if opts.affected_sop_instance_uid.trim().is_empty() {
            match dataset.get_uid(TAG_SOP_INSTANCE_UID) {
                Some(u) if !u.is_empty() => opts.affected_sop_instance_uid = u,
                _ => {
                    return Err(fail(Error::new(
                        "dicom dimse: dataset missing SOP Instance UID (0008,0018)",
                    )));
                }
            }
        }
        self.store_encoded(
            Some(&ctx),
            &|syntax: &Syntax| {
                object::writer::write_data_set(dataset, syntax.uid).map_err(Error::from)
            },
            opts,
            acquire,
            receive,
        )
    }
    #[allow(clippy::result_large_err)]
    pub(crate) fn store_encoded(
        &mut self,
        ctx: Option<&Ctx>,
        encode: &dyn Fn(&Syntax) -> Result<Vec<u8>>,
        opts: CStoreOptions,
        acquire: bool,
        receive: Option<&ResponseReceiver<'_>>,
    ) -> std::result::Result<CStoreResult, (CStoreResult, Error)> {
        self.store_streamed(
            ctx,
            Some(
                &|_ctx: &Ctx, sink: &mut dyn FnMut(&[u8]) -> Result<()>, syntax: &Syntax| {
                    let bytes = encode(syntax)?;
                    sink(&bytes)
                },
            ),
            opts,
            acquire,
            receive,
        )
    }
    #[allow(clippy::result_large_err)]
    pub(crate) fn store_streamed(
        &mut self,
        ctx: Option<&Ctx>,
        write: Option<&CStoreStreamWriter<'_>>,
        opts: CStoreOptions,
        acquire: bool,
        receive: Option<&ResponseReceiver<'_>>,
    ) -> std::result::Result<CStoreResult, (CStoreResult, Error)> {
        let fail = |e: Error| (CStoreResult::default(), e);
        if opts.priority > 2 {
            return Err(fail(Error::sentinel(Kind::StoreInvalidOptions)));
        }
        let Some(write) = write else {
            return Err(fail(Error::new(
                "dicom dimse: C-STORE dataset writer is nil",
            )));
        };
        let ctx = ctx.cloned().unwrap_or_default();
        if let Some(e) = context_err(&ctx) {
            return Err(fail(e));
        }
        let class = opts.affected_sop_class_uid.trim().to_string();
        if class.is_empty() {
            return Err(fail(Error::new(
                "dicom dimse: missing C-STORE Affected SOP Class UID",
            )));
        }
        let instance = opts.affected_sop_instance_uid.trim().to_string();
        if instance.is_empty() {
            return Err(fail(Error::new(
                "dicom dimse: missing C-STORE Affected SOP Instance UID",
            )));
        }
        let _guard = if acquire {
            Some(begin_association_operation(Some(self.assoc)).map_err(fail)?)
        } else {
            None
        };
        let pc = if opts.transfer_syntax_uids.is_empty() {
            accepted_context_for_sop_class(self.assoc, &class)
        } else {
            accepted_context_for_sop_class_transfer_syntaxes(
                self.assoc,
                &class,
                &opts.transfer_syntax_uids,
            )
        }
        .map_err(fail)?;
        let Some(syntax) = object::transfer::lookup(&pc.transfer_syntax_uid) else {
            let r = CStoreResult {
                presentation_context: pc.clone(),
                ..Default::default()
            };
            return Err((
                r,
                Error::wrap(
                    Kind::UnknownTransferSyntax,
                    crate::textfmt::quote(&pc.transfer_syntax_uid),
                ),
            ));
        };
        let message_id = if opts.message_id == 0 {
            self.message_id = self.message_id.wrapping_add(1);
            if self.message_id == 0 {
                self.message_id = 1;
            }
            self.message_id
        } else {
            self.message_id = opts.message_id;
            opts.message_id
        };
        let req = CStoreRequest {
            affected_sop_class_uid: class,
            message_id,
            priority: opts.priority,
            affected_sop_instance_uid: instance,
            move_originator_ae_title: opts.move_originator_ae_title.clone(),
            move_originator_message_id: opts.move_originator_message_id,
        };
        let mut result = CStoreResult {
            request: req.clone(),
            response: None,
            presentation_context: pc.clone(),
            transfer_syntax: Some(syntax),
        };
        if let Err(e) = send_message(
            Some(&ctx),
            self.assoc,
            pc.id,
            &req.command_set(),
            &syntax,
            |w| {
                write(
                    &ctx,
                    &mut |chunk: &[u8]| w.write(chunk).map(|_| ()),
                    &syntax,
                )
            },
        ) {
            return Err((result, new_operation_error("C-STORE", e, true)));
        }
        let received = match receive {
            Some(r) => r(&ctx, pc.id),
            None => receive_c_store_response(Some(&ctx), self.assoc, pc.id),
        };
        let response = match received {
            Ok(r) => r,
            Err(e) => return Err((result, new_operation_error("C-STORE", e, true))),
        };
        result.response = Some(response.clone());
        if response.message_id_being_responded_to != req.message_id {
            return Err((
                result,
                new_operation_error("C-STORE", Error::new("response message ID mismatch"), true),
            ));
        }
        if response.affected_sop_instance_uid != req.affected_sop_instance_uid {
            return Err((
                result,
                new_operation_error(
                    "C-STORE",
                    Error::new("response SOP Instance UID mismatch"),
                    true,
                ),
            ));
        }
        if let Err(e) = check_c_store_status(&response) {
            return Err((result, e));
        }
        Ok(result)
    }
}
#[derive(Debug, Clone)]
pub struct CStoreRequestContext {
    pub request: CStoreRequest,
    pub presentation_context_id: u8,
    pub presentation_context: AcceptedContext,
    pub data_set: Option<Object>,
    pub data_set_syntax: Option<Syntax>,
}
pub type CStoreHandler =
    Arc<dyn Fn(&Ctx, &CStoreRequestContext) -> (u16, Option<Error>) + Send + Sync>;
pub type OnCStoreResponse = Arc<dyn Fn(&Ctx, &CStoreRequestContext, u16) + Send + Sync>;
#[derive(Clone, Default)]
pub struct StorageScpOptions {
    pub store_handler: Option<CStoreHandler>,
    pub attribute_coercion: Option<AttributeCoercion>,
    pub max_data_set_bytes: i64,
    pub max_element_bytes: i64,
    pub max_elements: i64,
    pub max_sequence_depth: i64,
    pub max_pixel_data_bytes: i64,
    pub max_pixel_data_fragments: i64,
    pub on_c_store_response: Option<OnCStoreResponse>,
    pub stream_sink: Option<Arc<dyn CStoreStreamSink>>,
    pub spool_dir: String,
    pub spool_inline_bytes: i64,
    pub on_spool_remove_error: Option<OnSpoolRemoveError>,
}
pub(crate) fn storage_coercion_options(
    opts: &StorageScpOptions,
) -> (StoreCoercionOptions, Option<Error>) {
    normalize_store_coercion_options(StoreCoercionOptions {
        coercion: opts.attribute_coercion.clone(),
        clone_options: CloneFileOptions {
            max_total_bytes: opts.max_data_set_bytes,
            max_element_bytes: opts.max_element_bytes,
            max_elements: opts.max_elements,
            max_sequence_depth: opts.max_sequence_depth,
            max_pixel_data_bytes: opts.max_pixel_data_bytes,
            max_fragments: opts.max_pixel_data_fragments,
            max_items: 0,
        },
        ..Default::default()
    })
}
pub fn serve_storage_association(
    ctx: Option<&Ctx>,
    assoc: &Association,
    opts: &StorageScpOptions,
) -> Result<()> {
    let ctx = ctx.cloned().unwrap_or_default();
    if opts.store_handler.is_none() && opts.stream_sink.is_none() {
        return Err(Error::new("dicom dimse: missing C-STORE handler"));
    }
    if opts.attribute_coercion.is_some() {
        if opts.stream_sink.is_some() {
            return Err(Error::sentinel(Kind::StoreInvalidOptions));
        }
        if let (_, Some(e)) = storage_coercion_options(opts) {
            return Err(e);
        }
    }
    loop {
        let Some((pc_id, command)) = receive_storage_command_or_release(Some(&ctx), assoc)? else {
            return Ok(());
        };
        let field = command_uint16(Some(&command), COMMAND_FIELD)?;
        match field {
            C_ECHO_RQ => serve_c_echo_command(Some(&ctx), assoc, pc_id, &command)?,
            C_STORE_RQ => serve_storage_c_store(Some(&ctx), assoc, pc_id, &command, opts)?,
            _ => {
                return Err(Error::new(format!(
                    "dicom dimse: unsupported Storage SCP command field 0x{field:04X}"
                )));
            }
        }
    }
}
pub(crate) fn c_store_context_matches(affected_sop_class_uid: &str, pc: &AcceptedContext) -> bool {
    let class = object::transfer::normalize_uid(affected_sop_class_uid);
    if class != object::transfer::normalize_uid(&pc.abstract_syntax_uid) {
        return false;
    }
    if uiddict::is_storage(class) {
        return true;
    }
    uiddict::by_uid(class).is_none()
}
pub(crate) fn validate_c_store_image_payload(
    class_uid: &str,
    syntax_uid: &str,
    dataset: &Object,
) -> Result<()> {
    const CT: &str = "1.2.840.10008.5.1.4.1.1.2";
    if class_uid != CT {
        return Ok(());
    }
    if matches!(
        object::transfer::normalize_uid(syntax_uid),
        "1.2.840.10008.1.2.4.94"
            | "1.2.840.10008.1.2.4.95"
            | "1.2.840.10008.1.2.4.204"
            | "1.2.840.10008.1.2.4.205"
    ) {
        return Ok(());
    }
    let pixels = dataset.get(Tag::PIXEL_DATA);
    if pixels.is_none()
        && (!dataset.has(Tag::new(0x0028, 0x0010)) || !dataset.has(Tag::new(0x0028, 0x0011)))
    {
        return Ok(());
    }
    if let Some(p) = pixels {
        match &p.value {
            Value::Raw(v) if !v.is_empty() => return Ok(()),
            Value::Fragments { fragments, .. } if fragments.iter().any(|f| !f.is_empty()) => {
                return Ok(());
            }
            _ => {}
        }
    }
    Err(Error::new("CT Image Storage requires non-empty Pixel Data"))
}
pub fn validate_c_store_data_set(
    affected_sop_class_uid: &str,
    affected_sop_instance_uid: &str,
    pc: &AcceptedContext,
    dataset: Option<&Object>,
) -> Result<()> {
    let Some(dataset) = dataset else {
        return Err(Error::new("missing dataset"));
    };
    if affected_sop_class_uid.is_empty() {
        return Err(Error::new("missing affected SOP Class UID"));
    }
    if affected_sop_instance_uid.is_empty() {
        return Err(Error::new("missing affected SOP Instance UID"));
    }
    if !c_store_context_matches(affected_sop_class_uid, pc) {
        return Err(Error::new(format!(
            "presentation context SOP Class UID {} does not support C-STORE for {}",
            crate::textfmt::quote(&pc.abstract_syntax_uid),
            crate::textfmt::quote(affected_sop_class_uid)
        )));
    }
    if dataset.get_uid(TAG_SOP_CLASS_UID).as_deref() != Some(affected_sop_class_uid) {
        return Err(Error::new("dataset SOP Class UID does not match request"));
    }
    if dataset.get_uid(TAG_SOP_INSTANCE_UID).as_deref() != Some(affected_sop_instance_uid) {
        return Err(Error::new(
            "dataset SOP Instance UID does not match request",
        ));
    }
    validate_c_store_image_payload(affected_sop_class_uid, &pc.transfer_syntax_uid, dataset)
}
pub(crate) fn storage_data_set_error_status(err: &Error) -> u16 {
    let t = err.text();
    let limit = [
        "dicom: maximum element value size exceeded",
        "dicom: maximum total bytes exceeded",
        "dicom: maximum element count exceeded",
        "dicom: maximum sequence nesting depth exceeded",
        "dicom: maximum Pixel Data size exceeded",
        "dicom: maximum fragment count exceeded",
    ];
    if err.is_obj(object::Kind::MaxTotalBytes)
        || err.is(Kind::DataSetSpool)
        || err.is(Kind::ParserResourceLimit)
        || limit.iter().any(|l| t.contains(l))
    {
        return STATUS_C_STORE_OUT_OF_RESOURCES;
    }
    STATUS_C_STORE_CANNOT_UNDERSTAND
}
pub(crate) fn normalize_store_status(mut status: u16, err: Option<&Error>) -> (u16, String) {
    let mut comment = String::new();
    if let Some((s, c)) = err.and_then(|e| scp_status_of(e, "C-STORE")) {
        if s != 0 {
            status = s;
        }
        comment = c;
    }
    if err.is_some() && status == STATUS_SUCCESS {
        status = STATUS_C_STORE_CANNOT_UNDERSTAND;
    }
    (status, comment)
}
fn send_storage_c_store_response(
    ctx: Option<&Ctx>,
    assoc: &Association,
    pc_id: u8,
    req: &CStoreRequest,
    status: u16,
    comment: &str,
) -> Result<()> {
    send_with_scp_response_context(ctx, Some(assoc), |c| {
        send_command_set(
            Some(c),
            assoc,
            pc_id,
            &CStoreResponse {
                affected_sop_class_uid: req.affected_sop_class_uid.clone(),
                message_id_being_responded_to: req.message_id,
                affected_sop_instance_uid: req.affected_sop_instance_uid.clone(),
                status,
                error_comment: comment.to_string(),
            }
            .command_set(),
        )
    })
}
fn receive_storage_data_set(
    ctx: Option<&Ctx>,
    assoc: &Association,
    pc_id: u8,
    syntax: Syntax,
    opts: &StorageScpOptions,
) -> (Result<Object>, SpoolRelease) {
    let rctx = data_set_read_context(ctx, Some(assoc));
    let mut reader = PDataReader::typed(Some(rctx), Some(assoc), pc_id, false);
    let max_element_bytes = if opts.max_element_bytes == 0 {
        opts.max_data_set_bytes
    } else {
        opts.max_element_bytes
    };
    let max_pixel = if opts.max_pixel_data_bytes == 0 {
        opts.max_data_set_bytes
    } else {
        opts.max_pixel_data_bytes
    };
    let read = ReadFileOptions {
        max_element_bytes,
        max_total_bytes: opts.max_data_set_bytes,
        max_elements: opts.max_elements,
        max_sequence_depth: opts.max_sequence_depth,
        max_pixel_data_bytes: max_pixel,
        max_fragments: opts.max_pixel_data_fragments,
        ..Default::default()
    };
    let (result, release) = if opts.spool_dir.is_empty() {
        (
            read_pdata_data_set(&mut reader, syntax, read),
            SpoolRelease::default(),
        )
    } else {
        spool_data_set(
            &mut reader,
            syntax,
            &SpoolTarget {
                dir: opts.spool_dir.clone(),
                inline: opts.spool_inline_bytes,
                on_remove_error: opts.on_spool_remove_error.clone(),
            },
            read,
        )
    };
    match result {
        Ok(o) => (Ok(o), release),
        Err(err) => {
            if should_drain_on_error(&err)
                && let Err(drain) = drain_pdata_reader(&mut reader)
            {
                return (Err(drain), release);
            }
            (Err(err), release)
        }
    }
}
pub(crate) fn serve_storage_c_store(
    ctx: Option<&Ctx>,
    assoc: &Association,
    pc_id: u8,
    command: &Object,
    opts: &StorageScpOptions,
) -> Result<()> {
    let req = parse_c_store_request(command)?;
    let pc = accepted_context_by_id(assoc, pc_id)?;
    let ctx_v = ctx.cloned().unwrap_or_default();
    let mut req_ctx = CStoreRequestContext {
        request: req.clone(),
        presentation_context_id: pc_id,
        presentation_context: pc.clone(),
        data_set: None,
        data_set_syntax: None,
    };
    let notify = |rc: &CStoreRequestContext, status: u16| {
        if let Some(cb) = &opts.on_c_store_response {
            cb(&ctx_v, rc, status);
        }
    };
    let syntax = match transfer_syntax_for_accepted_context(&pc) {
        Ok(s) => s,
        Err(_) => {
            notify(&req_ctx, STATUS_C_STORE_CANNOT_UNDERSTAND);
            return send_storage_c_store_response(
                ctx,
                assoc,
                pc_id,
                &req,
                STATUS_C_STORE_CANNOT_UNDERSTAND,
                "",
            );
        }
    };
    req_ctx.data_set_syntax = Some(syntax);
    if !c_store_context_matches(&req.affected_sop_class_uid, &pc) {
        let mut reader = PDataReader::typed(
            Some(data_set_read_context(ctx, Some(assoc))),
            Some(assoc),
            pc_id,
            false,
        );
        drain_pdata_reader(&mut reader)?;
        notify(&req_ctx, STATUS_C_STORE_DATA_SET_DOES_NOT_MATCH);
        return send_storage_c_store_response(
            ctx,
            assoc,
            pc_id,
            &req,
            STATUS_C_STORE_DATA_SET_DOES_NOT_MATCH,
            "",
        );
    }
    if let Some(sink) = &opts.stream_sink {
        if let Some((status, comment)) =
            serve_storage_c_store_stream(ctx, assoc, pc_id, &req, &req_ctx, opts, sink.as_ref())?
        {
            notify(&req_ctx, status);
            return send_storage_c_store_response(ctx, assoc, pc_id, &req, status, &comment);
        }
        if opts.store_handler.is_none() {
            let mut reader = PDataReader::typed(
                Some(data_set_read_context(ctx, Some(assoc))),
                Some(assoc),
                pc_id,
                false,
            );
            drain_pdata_reader(&mut reader)?;
            return send_storage_c_store_response(
                ctx,
                assoc,
                pc_id,
                &req,
                STATUS_C_STORE_OUT_OF_RESOURCES,
                "",
            );
        }
    }
    let mut comment = String::new();
    let (received, release) = receive_storage_data_set(ctx, assoc, pc_id, syntax, opts);
    let status = match received {
        Err(err) => {
            if !should_drain_on_error(&err) {
                return Err(err);
            }
            storage_data_set_error_status(&err)
        }
        Ok(dataset) => {
            if validate_c_store_data_set(
                &req.affected_sop_class_uid,
                &req.affected_sop_instance_uid,
                &pc,
                Some(&dataset),
            )
            .is_err()
            {
                req_ctx.data_set = Some(dataset);
                STATUS_C_STORE_DATA_SET_DOES_NOT_MATCH
            } else if dataset.check_pixel_data_encoding(&syntax).is_err() {
                req_ctx.data_set = Some(dataset);
                STATUS_C_STORE_CANNOT_UNDERSTAND
            } else {
                let mut changed = false;
                let mut coercion_err = None;
                if opts.attribute_coercion.is_some() {
                    let (coercion_options, _) = storage_coercion_options(opts);
                    let encode = |_: &Ctx, target: &Syntax| {
                        object::writer::write_data_set(&dataset, target.uid).map_err(Error::from)
                    };
                    let prepared =
                        prepare_store_coercion(&ctx_v, &encode, syntax, &coercion_options);
                    match prepared {
                        Ok((file, c)) => {
                            changed = c;
                            if validate_c_store_data_set(
                                &req.affected_sop_class_uid,
                                &req.affected_sop_instance_uid,
                                &pc,
                                Some(&file.dataset),
                            )
                            .is_err()
                            {
                                coercion_err = Some(Error::sentinel(Kind::StoreCoercionIdentity));
                                req_ctx.data_set = Some(dataset);
                            } else {
                                req_ctx.data_set = Some(file.dataset);
                            }
                        }
                        Err(e) => {
                            coercion_err = Some(e);
                            req_ctx.data_set = Some(dataset);
                        }
                    }
                } else {
                    req_ctx.data_set = Some(dataset);
                }
                if let Some(err) = coercion_err {
                    comment = "attribute coercion failed".to_string();
                    storage_coercion_error_status(&err)
                } else {
                    let (status, err) = match opts.store_handler.as_ref() {
                        Some(handler) => handler(&ctx_v, &req_ctx),
                        None => (
                            STATUS_C_STORE_CANNOT_UNDERSTAND,
                            Some(Error::new("dicom dimse: nil C-STORE handler")),
                        ),
                    };
                    let (status, c) = normalize_store_status(status, err.as_ref());
                    comment = c;
                    if changed && err.is_none() && status == STATUS_SUCCESS {
                        STATUS_C_STORE_COERCION_WARNING
                    } else {
                        status
                    }
                }
            }
        }
    };
    notify(&req_ctx, status);
    let sent = send_storage_c_store_response(ctx, assoc, pc_id, &req, status, &comment);
    drop(req_ctx);
    drop(release);
    sent
}
#[doc(hidden)]
pub fn accepted_store_syntax(pc: &AcceptedContext) -> Result<Syntax> {
    transfer_syntax_for_accepted_context(pc)
}
