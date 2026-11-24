#![forbid(unsafe_code)]
use super::command::*;
use super::controls::*;
use super::error::{Detail, Error, Kind, Result};
use super::pdata::*;
use crate::net::telemetry::{CommandObservation, Direction};
use crate::net::ul::{self, AcceptedContext, Association, Ctx, Pdu};
use crate::object::{self, Element, Object, ReadFileOptions, Syntax};
pub(crate) fn observe_command(
    assoc: &Association,
    direction: Direction,
    pc_id: u8,
    command: &Object,
    duration: std::time::Duration,
) {
    if !assoc.observed() {
        return;
    }
    let Ok(field) = command_uint16(Some(command), COMMAND_FIELD) else {
        return;
    };
    let mut observation = CommandObservation {
        direction,
        presentation_context_id: pc_id,
        command_field: field,
        duration,
        ..Default::default()
    };
    if let Ok(Some(kind)) = optional_command_uint16(command, COMMAND_DATA_SET_TYPE) {
        observation.data_set_present = kind != NO_DATA_SET;
    }
    if let Ok(Some(status)) = optional_command_uint16(command, STATUS) {
        observation.status = status;
        observation.status_set = true;
        observation.status_category = command_status_category(status).to_string();
    }
    assoc.record_command_observation(observation);
}
fn observed_timer(assoc: &Association) -> Option<std::time::Instant> {
    assoc.observed().then(std::time::Instant::now)
}
fn observe_outbound(
    assoc: &Association,
    pc_id: u8,
    elements: &[Element],
    timer: Option<std::time::Instant>,
) {
    let Some(started) = timer else {
        return;
    };
    let elapsed = started.elapsed().max(std::time::Duration::from_nanos(1));
    let command = Object::from_elements(elements.to_vec());
    observe_command(assoc, Direction::Outbound, pc_id, &command, elapsed);
}
pub fn send_command_set(
    ctx: Option<&Ctx>,
    assoc: &Association,
    pc_id: u8,
    elements: &[Element],
) -> Result<()> {
    let ctx = command_write_context(ctx);
    let timer = observed_timer(assoc);
    let data = encode_command_set(elements)?;
    let mut writer = PDataWriter::new(
        ctx,
        Some(assoc),
        pc_id,
        true,
        peer_max_pdu_with_header(assoc),
    );
    let r = writer.write(&data).and_then(|_| writer.finish());
    writer.discard();
    if r.is_ok() {
        observe_outbound(assoc, pc_id, elements, timer);
    }
    r
}
pub fn receive_command_set(ctx: Option<&Ctx>, assoc: &Association, pc_id: u8) -> Result<Object> {
    let ctx = command_read_context(ctx);
    let mut reader = PDataReader::typed(ctx, Some(assoc), pc_id, true);
    let data = reader.read_to_end()?;
    let command = decode_command_set(&data)?;
    observe_command(
        assoc,
        Direction::Inbound,
        pc_id,
        &command,
        std::time::Duration::ZERO,
    );
    Ok(command)
}
fn assemble_command(
    ctx: Option<Ctx>,
    assoc: &Association,
    expected: &str,
    mut first: impl FnMut(Option<&Ctx>) -> Result<Option<Vec<ul::PDataValue>>>,
) -> Result<Option<(u8, Object)>> {
    let mut ctx = ctx;
    let mut pc_id = 0u8;
    let mut have = false;
    let mut buf = Vec::new();
    loop {
        let Some(values) = first(ctx.as_ref())? else {
            return Ok(None);
        };
        let mut it = values.into_iter();
        while let Some(value) = it.next() {
            if !value.is_command {
                return Err(Error::new(expected.to_string()));
            }
            if !have {
                pc_id = value.presentation_context_id;
                have = true;
            } else if value.presentation_context_id != pc_id {
                return Err(Error::wrap(
                    Kind::PresentationContextMismatch,
                    format!("got {}, want {pc_id}", value.presentation_context_id),
                ));
            }
            append_command_set_fragment(&mut buf, &value.data)?;
            if value.is_last {
                store_carryover(ctx.as_ref(), Some(assoc), it.collect())?;
                let command = decode_command_set(&buf)?;
                observe_command(
                    assoc,
                    Direction::Inbound,
                    pc_id,
                    &command,
                    std::time::Duration::ZERO,
                );
                return Ok(Some((pc_id, command)));
            }
        }
        let base = ctx.clone().unwrap_or_else(|| assoc.context.clone());
        ctx = Some(base.with_active_transfer());
    }
}
pub(crate) fn receive_command_set_any_raw(
    ctx: Option<&Ctx>,
    assoc: &Association,
) -> Result<(u8, Object)> {
    assemble_command(
        ctx.cloned(),
        assoc,
        "dicom dimse: C-GET expected command P-DATA",
        |c| receive_pdata_values(c, Some(assoc)).map(Some),
    )
    .and_then(|o| {
        o.ok_or_else(|| Error::new("dicom dimse: C-GET expected command P-DATA".to_string()))
    })
}
pub fn receive_command_set_any(ctx: Option<&Ctx>, assoc: &Association) -> Result<(u8, Object)> {
    let ctx = command_read_context(ctx);
    receive_command_set_any_raw(ctx.as_ref(), assoc)
}
pub(crate) fn receive_storage_command_or_release(
    ctx: Option<&Ctx>,
    assoc: &Association,
) -> Result<Option<(u8, Object)>> {
    assemble_command(
        ctx.cloned(),
        assoc,
        "dicom dimse: Storage SCP expected command P-DATA",
        |c| {
            let values = assoc.take_pdata_carryover(c)?;
            if !values.is_empty() {
                return Ok(Some(values));
            }
            let pdu = assoc.receive(c)?;
            match pdu {
                Pdu::ReleaseRq => {
                    assoc.write_pdu(&Pdu::ReleaseRp)?;
                    Ok(None)
                }
                Pdu::PData(p) => Ok(Some(p.values)),
                other => Err(ul::unexpected_pdu_error(
                    &other,
                    "waiting for P-DATA-TF or A-RELEASE-RQ",
                )
                .into()),
            }
        },
    )
}
#[allow(clippy::large_enum_variant)]
pub(crate) enum CommandOrControl {
    Command(u8, Object, i64),
    Control(Pdu),
}
pub(crate) fn receive_dimse_command_or_control(
    ctx: &Ctx,
    assoc: &Association,
) -> Result<CommandOrControl> {
    let mut ctx = ctx.clone();
    let mut pc_id = 0u8;
    let mut have = false;
    let mut buf: Vec<u8> = Vec::new();
    loop {
        let carry = assoc.take_pdata_carryover(Some(&ctx))?;
        let values = if !carry.is_empty() {
            carry
        } else {
            match assoc.receive(Some(&ctx))? {
                Pdu::PData(p) => p.values,
                other => {
                    if have || !buf.is_empty() {
                        return Err(ul::unexpected_pdu_error(
                            &other,
                            "receiving a fragmented DIMSE command",
                        )
                        .into());
                    }
                    return Ok(CommandOrControl::Control(other));
                }
            }
        };
        let mut it = values.into_iter();
        while let Some(value) = it.next() {
            if !value.is_command {
                return Err(Error::new(
                    "dicom dimse: command assembler expected command P-DATA",
                ));
            }
            if !have {
                pc_id = value.presentation_context_id;
                have = true;
            } else if value.presentation_context_id != pc_id {
                return Err(Error::wrap(
                    Kind::PresentationContextMismatch,
                    format!("got {}, want {pc_id}", value.presentation_context_id),
                ));
            }
            append_command_set_fragment(&mut buf, &value.data)?;
            if value.is_last {
                store_carryover(Some(&ctx), Some(assoc), it.collect())?;
                let command = decode_command_set(&buf)?;
                observe_command(
                    assoc,
                    Direction::Inbound,
                    pc_id,
                    &command,
                    std::time::Duration::ZERO,
                );
                return Ok(CommandOrControl::Command(pc_id, command, buf.len() as i64));
            }
        }
        ctx = ctx.with_active_transfer();
    }
}
pub fn send_data_set(
    ctx: Option<&Ctx>,
    assoc: &Association,
    pc_id: u8,
    dataset: &Object,
    syntax: &Syntax,
) -> Result<()> {
    let ctx = data_set_write_context(ctx);
    let mut writer = PDataWriter::new(
        ctx,
        Some(assoc),
        pc_id,
        false,
        peer_max_pdu_with_header(assoc),
    );
    let bytes = object::writer::write_data_set(dataset, syntax.uid)
        .map_err(|e| Error::from(e).prefixed("dicom dimse: send dataset"))?;
    writer
        .write(&bytes)
        .map_err(|e| e.prefixed("dicom dimse: send dataset"))?;
    writer
        .finish_data_set(syntax)
        .map_err(|e| e.prefixed("dicom dimse: finish dataset P-DATA"))
}
pub(crate) fn send_message_data_set(
    ctx: Option<&Ctx>,
    assoc: &Association,
    pc_id: u8,
    command: &[Element],
    dataset: Option<&Object>,
    syntax: &Syntax,
) -> Result<()> {
    let Some(dataset) = dataset else {
        return send_command_set(ctx, assoc, pc_id, command);
    };
    if dataset.is_empty() && !syntax.deflated {
        send_command_set(ctx, assoc, pc_id, command)?;
        return send_data_set(ctx, assoc, pc_id, dataset, syntax);
    }
    send_message(ctx, assoc, pc_id, command, syntax, |w| {
        let bytes = object::writer::write_data_set(dataset, syntax.uid).map_err(Error::from)?;
        w.write(&bytes).map(|_| ())
    })
}
pub(crate) fn send_message(
    ctx: Option<&Ctx>,
    assoc: &Association,
    pc_id: u8,
    elements: &[Element],
    syntax: &Syntax,
    produce: impl FnOnce(&mut PDataWriter<'_>) -> Result<()>,
) -> Result<()> {
    let ctx = ctx.cloned().unwrap_or_else(|| assoc.context.clone());
    if let Some(e) = context_err(&ctx) {
        return Err(e);
    }
    let timer = observed_timer(assoc);
    let data = encode_command_set(elements)?;
    let max = peer_max_pdu_with_header(assoc);
    let mut command = PDataWriter::new(
        command_write_context(Some(&ctx)),
        Some(assoc),
        pc_id,
        true,
        max,
    );
    command.write(&data)?;
    let mut writer = PDataWriter::new(
        data_set_write_context(Some(&ctx)),
        Some(assoc),
        pc_id,
        false,
        max,
    );
    let room =
        assoc.send_pdu_limit() as i64 - 2 * ul::PDV_HEADER_SIZE as i64 - command.buf.len() as i64;
    let mixed = room >= 2;
    if mixed {
        writer.command_prefix = Some(std::mem::take(&mut command.buf));
        writer.first_context = Some(mixed_message_write_context(&ctx));
    } else {
        command.finish()?;
        observe_outbound(assoc, pc_id, elements, timer);
    }
    command.discard();
    let r = (|| {
        produce(&mut writer).map_err(|e| e.prefixed("dicom dimse: send dataset"))?;
        if writer.written == 0 {
            return Err(Error::new("dicom dimse: dataset producer emitted no bytes"));
        }
        if !syntax.deflated && writer.written % 2 != 0 {
            return Err(Error::new(
                "dicom dimse: dataset producer emitted an odd byte count",
            ));
        }
        writer
            .finish_data_set(syntax)
            .map_err(|e| e.prefixed("dicom dimse: finish dataset P-DATA"))
    })();
    writer.discard();
    if mixed && r.is_ok() {
        observe_outbound(assoc, pc_id, elements, timer);
    }
    r
}
pub fn receive_data_set(
    ctx: Option<&Ctx>,
    assoc: &Association,
    pc_id: u8,
    syntax: Syntax,
) -> Result<Object> {
    receive_data_set_limits(ctx, assoc, pc_id, syntax, 0, 0, 0).map(|(o, _)| o)
}
pub(crate) fn receive_identifier_data_set(
    ctx: Option<&Ctx>,
    assoc: &Association,
    pc_id: u8,
    syntax: Syntax,
) -> Result<Object> {
    receive_data_set_limits(ctx, assoc, pc_id, syntax, MAX_IDENTIFIER_BYTES, 0, 0).map(|(o, _)| o)
}
pub(crate) fn should_drain_on_error(err: &Error) -> bool {
    !(err.is_ul(ul::Kind::Closed)
        || err.is_ul(ul::Kind::AssociationAborted)
        || err.is_ul(ul::Kind::AssociationTimeout)
        || err.is_canceled()
        || err.is_deadline())
}
pub(crate) fn receive_data_set_limits(
    ctx: Option<&Ctx>,
    assoc: &Association,
    pc_id: u8,
    syntax: Syntax,
    max_bytes: i64,
    max_elements: i64,
    max_depth: i64,
) -> Result<(Object, i64)> {
    let ctx = data_set_read_context(ctx, Some(assoc));
    let mut reader = PDataReader::typed(Some(ctx), Some(assoc), pc_id, false);
    let opts = ReadFileOptions {
        max_element_bytes: max_bytes,
        max_total_bytes: max_bytes,
        max_elements,
        max_sequence_depth: max_depth,
        ..Default::default()
    };
    match read_pdata_data_set(&mut reader, syntax, opts) {
        Ok(o) => Ok((o, reader.received_bytes() as i64)),
        Err(err) => {
            let size = reader.received_bytes() as i64;
            if should_drain_on_error(&err)
                && let Err(d) = drain_pdata_reader(&mut reader)
            {
                return Err(d.prefixed("dicom dimse: drain rejected dataset"));
            }
            let _ = size;
            Err(err.prefixed("dicom dimse: receive dataset"))
        }
    }
}
pub fn accepted_verification_context(assoc: &Association) -> Option<AcceptedContext> {
    assoc
        .accepted_contexts
        .iter()
        .find(|pc| pc.abstract_syntax_uid == VERIFICATION_SOP_CLASS_UID)
        .cloned()
}
pub fn missing_presentation_context_text(
    sop_class_uid: &str,
    outcomes: &[ul::PresentationContextOutcome],
) -> String {
    let q = crate::textfmt::quote(sop_class_uid);
    if outcomes.is_empty() {
        return format!("dicom dimse: no accepted presentation context for SOP Class UID {q}");
    }
    let parts: Vec<String> = outcomes.iter().map(|o| o.explain()).collect();
    format!(
        "dicom dimse: no accepted presentation context for SOP Class UID {q}: {}",
        parts.join("; ")
    )
}
pub fn format_presentation_context_diagnostic(err: Option<&Error>) -> String {
    let Some(err) = err else {
        return String::new();
    };
    let mut detail = err.detail();
    loop {
        match detail {
            Detail::MissingPresentationContext(sop, outcomes) => {
                return missing_presentation_context_text(sop, outcomes);
            }
            Detail::Store(_, _, _, inner) | Detail::StorageCommitmentDelivery(_, _, _, inner) => {
                detail = inner.as_ref();
            }
            _ => break,
        }
    }
    if let ul::Detail::NoAccepted(outcomes) = err.ul_detail() {
        return ul::Error::no_accepted(outcomes.clone()).text().to_string();
    }
    String::new()
}
pub fn send_c_cancel_request(
    ctx: Option<&Ctx>,
    assoc: &Association,
    pc_id: u8,
    request: &super::command::CCancelRequest,
) -> Result<()> {
    send_command_set(ctx, assoc, pc_id, &request.command_set())
}
pub fn explain_missing_presentation_context(assoc: &Association, sop_class_uid: &str) -> Error {
    let matching: Vec<ul::PresentationContextOutcome> = assoc
        .presentation_context_outcomes()
        .into_iter()
        .filter(|o| o.abstract_syntax_uid == sop_class_uid && !o.accepted())
        .collect();
    Error::new(missing_presentation_context_text(sop_class_uid, &matching)).with_detail(
        Detail::MissingPresentationContext(sop_class_uid.to_string(), matching),
    )
}
pub fn accepted_context_by_id(assoc: &Association, pc_id: u8) -> Result<AcceptedContext> {
    assoc
        .accepted_contexts
        .iter()
        .find(|pc| pc.id == pc_id)
        .cloned()
        .ok_or_else(|| {
            Error::new(format!(
                "dicom dimse: no accepted presentation context {pc_id}"
            ))
        })
}
pub fn transfer_syntax_for_accepted_context(pc: &AcceptedContext) -> Result<Syntax> {
    object::transfer::lookup(&pc.transfer_syntax_uid).ok_or_else(|| {
        Error::wrap(
            Kind::UnknownTransferSyntax,
            format!(
                "{} for presentation context {}",
                crate::textfmt::quote(&pc.transfer_syntax_uid),
                pc.id
            ),
        )
    })
}
pub fn accepted_transfer_syntax(assoc: &Association, pc_id: u8) -> Result<Syntax> {
    transfer_syntax_for_accepted_context(&accepted_context_by_id(assoc, pc_id)?)
}
pub(crate) fn require_scp_presentation_context(
    ctx: Option<&Ctx>,
    assoc: &Association,
    pc_id: u8,
    sop_class_uid: &str,
) -> Result<AcceptedContext> {
    let pc = accepted_context_by_id(assoc, pc_id)?;
    if pc.abstract_syntax_uid != sop_class_uid {
        let err = Error::wrap(
            Kind::PresentationContextMismatch,
            format!(
                "context {pc_id} is for {}, want {}",
                crate::textfmt::quote(&pc.abstract_syntax_uid),
                crate::textfmt::quote(sop_class_uid)
            ),
        );
        let abort = send_with_scp_response_context(ctx, Some(assoc), |c| {
            assoc
                .abort_with_context(Some(c), ul::ABORT_REASON_NOT_SPECIFIED)
                .map_err(Error::from)
        })
        .err();
        return Err(Error::join([Some(err), abort]).unwrap());
    }
    Ok(pc)
}
pub(crate) fn accepted_scp_transfer_syntax(
    ctx: Option<&Ctx>,
    assoc: &Association,
    pc_id: u8,
    sop_class_uid: &str,
) -> Result<Syntax> {
    let pc = require_scp_presentation_context(ctx, assoc, pc_id, sop_class_uid)?;
    transfer_syntax_for_accepted_context(&pc)
}
pub fn accepted_context_for_sop_class(
    assoc: &Association,
    sop_class_uid: &str,
) -> Result<AcceptedContext> {
    assoc
        .accepted_contexts
        .iter()
        .find(|pc| pc.abstract_syntax_uid == sop_class_uid)
        .cloned()
        .ok_or_else(|| explain_missing_presentation_context(assoc, sop_class_uid))
}
pub fn normalized_transfer_syntax_uids(uids: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for uid in uids {
        let uid = object::transfer::normalize_uid(uid);
        if uid.is_empty() || out.iter().any(|u| u == uid) {
            continue;
        }
        out.push(uid.to_string());
    }
    out
}
pub fn accepted_context_for_sop_class_transfer_syntaxes(
    assoc: &Association,
    sop_class_uid: &str,
    transfer_syntax_uids: &[String],
) -> Result<AcceptedContext> {
    let wanted = normalized_transfer_syntax_uids(transfer_syntax_uids);
    if wanted.is_empty() {
        return Err(Error::new(format!(
            "dicom dimse: no requested transfer syntax UID for SOP Class UID {}",
            crate::textfmt::quote(sop_class_uid)
        )));
    }
    for ts in &wanted {
        if let Some(pc) = assoc.accepted_contexts.iter().find(|pc| {
            pc.abstract_syntax_uid == sop_class_uid
                && object::transfer::normalize_uid(&pc.transfer_syntax_uid) == ts
        }) {
            return Ok(pc.clone());
        }
    }
    let base = explain_missing_presentation_context(assoc, sop_class_uid);
    let text = format!(
        "{}: transfer syntax UIDs [{}]",
        base.text(),
        wanted.join(" ")
    );
    Err(base.relabel(text))
}
pub fn accepted_storage_scp_role(assoc: &Association, sop_class_uid: &str) -> bool {
    let uid = sop_class_uid.trim();
    assoc
        .accepted_role_selections
        .iter()
        .any(|r| r.sop_class_uid == uid && r.scp_role)
}
