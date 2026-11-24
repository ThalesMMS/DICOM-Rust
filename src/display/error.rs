use super::FrameFault;
use std::fmt;
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DisplayError {
    Frame {
        field: &'static str,
        fault: FrameFault,
    },
    InvalidLutDescriptor(String),
    InvalidLutData(String),
    LegacyLut {
        data: bool,
        detail: String,
    },
}
impl DisplayError {
    pub(super) fn frame(field: &'static str, fault: FrameFault) -> DisplayError {
        DisplayError::Frame { field, fault }
    }
}
const INVALID_DESCRIPTOR: &str = "dicom/display: invalid LUT descriptor";
const INVALID_DATA: &str = "dicom/display: LUT data does not match descriptor";
const LEGACY: &str = "dicom/display: legacy LUT encoding (read with LUTOptions.Legacy)";
impl fmt::Display for FrameFault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            FrameFault::Invalid => "dicom/display: invalid frame metadata",
            FrameFault::UnsupportedBitsAllocated => "dicom/display: unsupported BitsAllocated",
            FrameFault::PixelDataTooShort => "dicom/display: pixel data too short for frame",
            FrameFault::DestinationTooShort => "dicom/display: modality destination too short",
            FrameFault::SizeOverflow => "dicom/display: frame size overflow",
        })
    }
}
impl fmt::Display for DisplayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DisplayError::Frame { field, fault } => write!(f, "{fault}: {field}"),
            DisplayError::InvalidLutDescriptor(detail) => {
                write!(f, "{INVALID_DESCRIPTOR}: {detail}")
            }
            DisplayError::InvalidLutData(detail) => write!(f, "{INVALID_DATA}: {detail}"),
            DisplayError::LegacyLut { data, detail } => {
                let base = if *data {
                    INVALID_DATA
                } else {
                    INVALID_DESCRIPTOR
                };
                write!(f, "{base}: {LEGACY}: {detail}")
            }
        }
    }
}
impl std::error::Error for DisplayError {}
