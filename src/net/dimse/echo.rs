#![forbid(unsafe_code)]
use super::command::*;
use super::controls::*;
use super::error::{Error, Kind, Result};
use super::message::*;
use crate::net::ul::{self, AcceptedContext, Association, Ctx, Nanos};
use crate::object::Object;
use std::time::{Duration, SystemTime};
fn protocol_violation(text: String) -> Error {
    Error::classified(Kind::ProtocolViolation, text)
}
pub fn send_c_echo(
    ctx: Option<&Ctx>,
    assoc: &Association,
    pc_id: u8,
    message_id: u16,
) -> Result<CEchoResponse> {
    let req = CEchoRequest { message_id };
    send_command_set(ctx, assoc, pc_id, &req.command_set())?;
    let obj = receive_command_set(ctx, assoc, pc_id)?;
    let resp = parse_c_echo_response(&obj)?;
    if resp.message_id_being_responded_to != message_id {
        return Err(protocol_violation(format!(
            "dicom dimse: response message ID {}, want {message_id}",
            resp.message_id_being_responded_to
        )));
    }
    if resp.status != STATUS_SUCCESS {
        return Err(Error::classified(
            Kind::CEchoStatus,
            format!(
                "dicom dimse: C-ECHO failed with status 0x{:04X}",
                resp.status
            ),
        ));
    }
    Ok(resp)
}
pub fn send_c_echo_request(assoc: &Association, pc_id: u8, message_id: u16) -> Result<()> {
    send_command_set(
        None,
        assoc,
        pc_id,
        &CEchoRequest { message_id }.command_set(),
    )
}
pub fn receive_c_echo_response(assoc: &Association, pc_id: u8, message_id: u16) -> Result<u16> {
    let command = receive_command_set(None, assoc, pc_id)?;
    let resp = parse_c_echo_response(&command)?;
    if resp.message_id_being_responded_to != message_id {
        return Err(protocol_violation(format!(
            "dicom dimse: response message ID {}, want {message_id}",
            resp.message_id_being_responded_to
        )));
    }
    Ok(resp.status)
}
pub fn receive_c_echo(assoc: &Association, pc_id: u8) -> Result<u16> {
    let command = receive_command_set(None, assoc, pc_id)?;
    validate_c_echo_request(None, assoc, pc_id, &command)
}
pub fn serve_c_echo_command(
    ctx: Option<&Ctx>,
    assoc: &Association,
    pc_id: u8,
    command: &Object,
) -> Result<()> {
    let message_id = validate_c_echo_request(ctx, assoc, pc_id, command)?;
    send_with_scp_response_context(ctx, Some(assoc), |c| {
        send_c_echo_response(Some(c), assoc, pc_id, message_id, STATUS_SUCCESS)
    })
}
pub(crate) fn validate_c_echo_request(
    ctx: Option<&Ctx>,
    assoc: &Association,
    pc_id: u8,
    command: &Object,
) -> Result<u16> {
    require_scp_presentation_context(ctx, assoc, pc_id, VERIFICATION_SOP_CLASS_UID)?;
    let field = command_uint16(Some(command), COMMAND_FIELD)?;
    if field != C_ECHO_RQ {
        return Err(Error::new(format!(
            "dicom dimse: command field 0x{field:04X}, want C-ECHO-RQ 0x{C_ECHO_RQ:04X}"
        )));
    }
    let class_uid = command_uid(command, AFFECTED_SOP_CLASS_UID)?;
    if class_uid != VERIFICATION_SOP_CLASS_UID {
        return Err(Error::wrap(
            Kind::PresentationContextMismatch,
            format!(
                "C-ECHO affected SOP Class UID {}, want Verification",
                crate::textfmt::quote(&class_uid)
            ),
        ));
    }
    let dst = command_uint16(Some(command), COMMAND_DATA_SET_TYPE)?;
    if dst != NO_DATA_SET {
        return Err(Error::new(format!(
            "dicom dimse: C-ECHO request dataset type 0x{dst:04X}, want no dataset 0x{NO_DATA_SET:04X}"
        )));
    }
    command_uint16(Some(command), MESSAGE_ID)
}
pub fn send_c_echo_response(
    ctx: Option<&Ctx>,
    assoc: &Association,
    pc_id: u8,
    message_id: u16,
    status: u16,
) -> Result<()> {
    let resp = CEchoResponse {
        message_id_being_responded_to: message_id,
        status,
    };
    send_command_set(ctx, assoc, pc_id, &resp.command_set())
}
pub fn handle_c_echo(assoc: &Association) -> Result<()> {
    let Some(pc) = accepted_verification_context(assoc) else {
        return Err(Error::new(
            "dicom dimse: no accepted Verification SOP Class presentation context",
        ));
    };
    let message_id = receive_c_echo(assoc, pc.id)?;
    send_c_echo_response(None, assoc, pc.id, message_id, STATUS_SUCCESS)
}
#[derive(Debug, Clone, Default)]
pub struct CEchoVerificationOptions {
    pub transport: ul::TransportOptions,
    #[cfg(feature = "tls")]
    pub tls_config: Option<ul::TlsClientConfig>,
    pub address: String,
    pub called_ae_title: String,
    pub calling_ae_title: String,
    pub transfer_syntax_uids: Vec<String>,
    pub user_identity: Option<ul::UserIdentityRequest>,
    pub dial_timeout: Nanos,
    pub release_timeout: Nanos,
    pub message_id: u16,
}
#[derive(Debug, Clone)]
pub struct CEchoVerificationResult {
    pub status: u16,
    pub message_id: u16,
    pub presentation_context: AcceptedContext,
    pub duration: Duration,
    pub started_at: SystemTime,
}
#[allow(clippy::result_large_err)]
pub fn verify_c_echo(
    ctx: Option<&Ctx>,
    opts: CEchoVerificationOptions,
) -> std::result::Result<CEchoVerificationResult, (CEchoVerificationResult, Error)> {
    let ctx = ctx.cloned().unwrap_or_default();
    let timer = ElapsedTimer::start();
    let mut result = CEchoVerificationResult {
        status: 0,
        message_id: 0,
        presentation_context: AcceptedContext::default(),
        duration: Duration::ZERO,
        started_at: SystemTime::now(),
    };
    if opts.address.is_empty() {
        return Err((
            result,
            Error::wrap(Kind::CEchoAssociation, "address is required"),
        ));
    }
    let message_id = if opts.message_id == 0 {
        1
    } else {
        opts.message_id
    };
    result.message_id = message_id;
    let dial_ctx = ctx.with_optional_timeout(opts.dial_timeout);
    let syntaxes: Vec<&str> = if opts.transfer_syntax_uids.is_empty() {
        vec![ul::IMPLICIT_VR_LITTLE_ENDIAN]
    } else {
        opts.transfer_syntax_uids
            .iter()
            .map(String::as_str)
            .collect()
    };
    let dial = ul::DialOptions {
        called_ae_title: opts.called_ae_title.clone(),
        calling_ae_title: opts.calling_ae_title.clone(),
        user_identity: opts.user_identity.clone(),
        transport: opts.transport.clone(),
        #[cfg(feature = "tls")]
        tls_config: opts.tls_config.clone(),
        contexts: vec![ul::PresentationContext::new(
            VERIFICATION_SOP_CLASS_UID,
            &syntaxes,
        )],
        ..Default::default()
    };
    let assoc = match ul::dial_context(&dial_ctx, &opts.address, dial) {
        Ok(a) => a,
        Err(err) => {
            let err = Error::from(err);
            if err.is_ul(ul::Kind::NoAcceptedPresentationContexts) {
                let e = Error::wrap_err(
                    Kind::CEchoAssociation,
                    Error::wrap_err(Kind::CEchoNoAcceptedPresentationContext, err),
                );
                return Err((result, e));
            }
            return Err((
                result,
                Error::wrap_msg_err(
                    Kind::CEchoAssociation,
                    format!("associate with {}", opts.address),
                    err,
                ),
            ));
        }
    };
    let Some(pc) = accepted_verification_context(&assoc) else {
        let e = Error::wrap_err(
            Kind::CEchoNoAcceptedPresentationContext,
            explain_missing_presentation_context(&assoc, VERIFICATION_SOP_CLASS_UID),
        );
        assoc.close();
        return Err((result, e));
    };
    result.presentation_context = pc.clone();
    let response = match send_c_echo(Some(&ctx), &assoc, pc.id, message_id) {
        Ok(r) => r,
        Err(err) => {
            assoc.close();
            return Err((
                result,
                Error::wrap_msg_err(Kind::CEchoRequest, format!("send to {}", opts.address), err),
            ));
        }
    };
    result.status = response.status;
    let release_ctx = ctx.with_optional_timeout(opts.release_timeout);
    if let Err(err) = assoc.release(Some(&release_ctx)) {
        assoc.close();
        return Err((
            result,
            Error::wrap_msg_err(
                Kind::CEchoRelease,
                format!("release association with {}", opts.address),
                err.into(),
            ),
        ));
    }
    result.duration = timer.elapsed();
    Ok(result)
}
