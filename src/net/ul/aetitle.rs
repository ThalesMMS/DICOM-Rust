#![forbid(unsafe_code)]
use super::error::{Error, Kind, Result};
use super::pdu::MAX_AE_TITLE_LENGTH;
use crate::net::telemetry::escape_peer_string;
pub fn normalize_ae_title(title: &str) -> &str {
    title.trim_end_matches([' ', '\0']).trim_start_matches(' ')
}
pub fn validate_ae_title(title: &str) -> Result<()> {
    let normalized = normalize_ae_title(title);
    if normalized.is_empty() {
        return Err(Error::wrap(
            Kind::InvalidAeTitle,
            "empty or all-space value",
        ));
    }
    if normalized.len() > MAX_AE_TITLE_LENGTH {
        return Err(Error::wrap(
            Kind::InvalidAeTitle,
            format!("length {} exceeds {MAX_AE_TITLE_LENGTH}", normalized.len()),
        ));
    }
    for &c in normalized.as_bytes() {
        if !(0x20..=0x7e).contains(&c) || c == b'\\' {
            return Err(Error::wrap(
                Kind::InvalidAeTitle,
                format!(
                    "\"{}\" contains byte 0x{c:02X} outside the AE repertoire",
                    escape_peer_string(normalized)
                ),
            ));
        }
    }
    Ok(())
}
pub fn equal_ae_titles(a: &str, b: &str) -> bool {
    normalize_ae_title(a) == normalize_ae_title(b)
}
