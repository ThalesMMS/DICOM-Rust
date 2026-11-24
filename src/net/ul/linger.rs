#![forbid(unsafe_code)]
use super::conn::{Conn, Ctx, Nanos};
use super::error::{Error, Kind};
use super::pdu::*;
use std::time::{Duration, Instant};
pub const DEFAULT_ARTIM_TIMEOUT: Duration = Duration::from_secs(2);
pub(crate) const MAX_DRAIN_BYTES: u64 = 64 << 20;
pub fn unexpected_pdu_error(pdu: &Pdu, while_: &str) -> Error {
    if matches!(pdu, Pdu::Unknown(_)) {
        return Error::with_kinds(
            format!(
                "{} ({}): got {} while {while_}",
                Kind::UnexpectedPdu.text(),
                Kind::UnrecognizedPdu.text(),
                pdu.kind_label()
            ),
            vec![Kind::UnexpectedPdu, Kind::UnrecognizedPdu],
        );
    }
    Error::wrap(
        Kind::UnexpectedPdu,
        format!("got {} while {while_}", pdu.kind_label()),
    )
}
pub fn provider_abort_reason(err: &Error) -> Option<u8> {
    let is = |k| err.is(k);
    if is(Kind::UnrecognizedPdu) || is(Kind::UnsupportedPdu) {
        return Some(ABORT_REASON_UNRECOGNIZED_PDU);
    }
    if is(Kind::UnexpectedPdu) {
        return Some(ABORT_REASON_UNEXPECTED_PDU);
    }
    if is(Kind::InvalidPduItem) || is(Kind::InvalidUserItem) {
        return Some(ABORT_REASON_UNRECOGNIZED_PDU_PARAMETER);
    }
    if [
        Kind::PduTooLarge,
        Kind::LengthOverflow,
        Kind::InvalidPduField,
        Kind::MissingPduField,
        Kind::InvalidAeTitle,
        Kind::InvalidPcid,
        Kind::InvalidPduSize,
        Kind::InvalidPdu,
    ]
    .into_iter()
    .any(is)
    {
        return Some(ABORT_REASON_INVALID_PDU_PARAMETER);
    }
    None
}
pub(crate) fn artim_timeout(configured: Nanos) -> Duration {
    if configured.is_zero() {
        DEFAULT_ARTIM_TIMEOUT
    } else {
        configured.positive().unwrap_or(Duration::ZERO)
    }
}
pub(crate) fn context_limit_reached(ctx: &Ctx) -> bool {
    ctx.err().is_some()
}
pub(crate) fn linger_close(conn: &Conn, artim: Duration) {
    if !artim.is_zero() {
        conn.close_write();
        conn.drain(Instant::now() + artim, MAX_DRAIN_BYTES);
    }
    conn.close();
}
