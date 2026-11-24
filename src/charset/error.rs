use crate::textfmt::spaced_hex;
use std::fmt;
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CharsetError {
    Unsupported(String),
    InvalidDeclaration(String),
    Unrepresentable(String),
    Decode {
        charset: String,
        offset: usize,
        byte: u8,
    },
    CodeExtension {
        offset: usize,
        sequence: Vec<u8>,
        reason: String,
    },
    PersonNameGroup {
        encode: bool,
        group: usize,
        codec: String,
        source: Box<CharsetError>,
    },
    Other(String),
}
impl CharsetError {
    pub fn is_unsupported(&self) -> bool {
        match self {
            CharsetError::Unsupported(_) => true,
            CharsetError::PersonNameGroup { source, .. } => source.is_unsupported(),
            _ => false,
        }
    }
    pub fn is_unrepresentable(&self) -> bool {
        match self {
            CharsetError::Unrepresentable(_) => true,
            CharsetError::PersonNameGroup { source, .. } => source.is_unrepresentable(),
            _ => false,
        }
    }
    pub(crate) fn decode_offset(&self) -> Option<usize> {
        match self {
            CharsetError::Decode { offset, .. } => Some(*offset),
            _ => None,
        }
    }
}
impl fmt::Display for CharsetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CharsetError::Unsupported(detail) => {
                write!(f, "dicom: unsupported specific character set: {detail}")
            }
            CharsetError::InvalidDeclaration(detail) => write!(
                f,
                "dicom: invalid specific character set declaration: {detail}"
            ),
            CharsetError::Unrepresentable(detail) => write!(
                f,
                "dicom: character cannot be represented in the selected character set: {detail}"
            ),
            CharsetError::Decode {
                charset,
                offset,
                byte,
            } => write!(
                f,
                "dicom: decode {charset} at byte {offset}: invalid byte 0x{byte:02X}"
            ),
            CharsetError::CodeExtension {
                offset,
                sequence,
                reason,
            } => {
                let reason = if reason.is_empty() {
                    "invalid sequence"
                } else {
                    reason
                };
                write!(
                    f,
                    "dicom: invalid ISO 2022 code extension at byte {offset} ({}): {reason}",
                    spaced_hex(sequence)
                )
            }
            CharsetError::PersonNameGroup {
                encode,
                group,
                codec,
                source,
            } => write!(
                f,
                "dicom: {} PN component group {group} with {codec}: {source}",
                if *encode { "encode" } else { "decode" }
            ),
            CharsetError::Other(message) => f.write_str(message),
        }
    }
}
impl std::error::Error for CharsetError {}
