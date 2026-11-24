#![forbid(unsafe_code)]
#![allow(clippy::needless_range_loop)]
#![allow(clippy::too_many_arguments)]
mod motion;
mod phase_correlation;
mod plan;
mod shift;
use crate::object::{Object, Tag};
pub use motion::{
    BlockMatch, BlockMatchResult, BlockOptions, DisplacementField, Rigid, RigidOptions,
    RigidRegistration, apply_field, apply_rigid, gradient_difference, match_blocks, register_rigid,
};
pub use phase_correlation::{
    PeakInterpolation, PhaseCorrelationOptions, Registration, phase_correlate,
};
pub use plan::{Plan, PlanOptions, Recovery, ShiftSource};
pub use shift::{Shift, apply_shift, average_frames, subtract_modality};
use std::fmt;
pub const MASK_OPERATION_AVERAGE_SUBTRACTION: &str = "AVG_SUB";
pub const MASK_OPERATION_TID: &str = "TID";
pub const MASK_OPERATION_REVERSED_TID: &str = "REV_TID";
pub(crate) const DEFAULT_TID_OFFSET: i64 = 1;
pub const RECOMMENDED_VIEWING_SUBTRACTION: &str = "SUB";
pub const RECOMMENDED_VIEWING_NATIVE: &str = "NAT";
const TAG_NUMBER_OF_FRAMES: Tag = Tag::new(0x0028, 0x0008);
const TAG_MASK_SUBTRACTION_SEQUENCE: Tag = Tag::new(0x0028, 0x6100);
const TAG_MASK_OPERATION: Tag = Tag::new(0x0028, 0x6101);
const TAG_APPLICABLE_FRAME_RANGE: Tag = Tag::new(0x0028, 0x6102);
const TAG_MASK_FRAME_NUMBERS: Tag = Tag::new(0x0028, 0x6110);
const TAG_CONTRAST_FRAME_AVERAGING: Tag = Tag::new(0x0028, 0x6112);
const TAG_MASK_SUB_PIXEL_SHIFT: Tag = Tag::new(0x0028, 0x6114);
const TAG_TID_OFFSET: Tag = Tag::new(0x0028, 0x6120);
const TAG_MASK_OPERATION_EXPLANATION: Tag = Tag::new(0x0028, 0x6190);
const TAG_FRAME_PIXEL_SHIFT_SEQUENCE: Tag = Tag::new(0x0028, 0x9415);
const TAG_SUBTRACTION_ITEM_ID: Tag = Tag::new(0x0028, 0x9416);
const TAG_MASK_SELECTION_MODE: Tag = Tag::new(0x0028, 0x9454);
const TAG_RECOMMENDED_VIEWING_MODE: Tag = Tag::new(0x0028, 0x1090);
const TAG_SHARED_FUNCTIONAL_GROUPS: Tag = Tag::new(0x5200, 0x9229);
const TAG_PER_FRAME_FUNCTIONAL_GROUPS: Tag = Tag::new(0x5200, 0x9230);
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    NoMaskModule,
    InvalidNumberOfFrames,
    FrameOutOfRange,
    FrameNotApplicable,
    ItemOutOfRange,
    UnsupportedMaskOperation,
    InvalidFrameRange,
    MissingApplicableFrameRange,
    MissingMaskFrameNumbers,
    MaskFrameOutOfRange,
    MissingTidOffset,
    InvalidContrastFrameAveraging,
    ContrastAveragingOverrun,
    InvalidPixelShift,
    MissingSubtractionItemId,
    FrameSizeMismatch,
    NoFrames,
    Motion,
    FrameTooSmall,
    FunctionalGroupConflict,
    Optimize,
    Other,
}
impl ErrorKind {
    pub fn text(self) -> &'static str {
        match self {
            ErrorKind::NoMaskModule => "dsa: no Mask Subtraction Sequence items",
            ErrorKind::InvalidNumberOfFrames => "dsa: invalid number of frames",
            ErrorKind::FrameOutOfRange => "dsa: frame number out of range",
            ErrorKind::FrameNotApplicable => "dsa: no mask subtraction item applies to the frame",
            ErrorKind::ItemOutOfRange => "dsa: mask subtraction item index out of range",
            ErrorKind::UnsupportedMaskOperation => "dsa: unsupported mask operation",
            ErrorKind::InvalidFrameRange => "dsa: invalid applicable frame range",
            ErrorKind::MissingApplicableFrameRange => {
                "dsa: REV_TID item has no applicable frame range"
            }
            ErrorKind::MissingMaskFrameNumbers => "dsa: AVG_SUB item has no mask frame numbers",
            ErrorKind::MaskFrameOutOfRange => "dsa: mask frame number out of range",
            ErrorKind::MissingTidOffset => "dsa: TID item has no TID offset",
            ErrorKind::InvalidContrastFrameAveraging => "dsa: invalid contrast frame averaging",
            ErrorKind::ContrastAveragingOverrun => {
                "dsa: contrast frame averaging runs past the last frame"
            }
            ErrorKind::InvalidPixelShift => "dsa: invalid mask sub-pixel shift",
            ErrorKind::MissingSubtractionItemId => {
                "dsa: mask subtraction item has no subtraction item ID"
            }
            ErrorKind::FrameSizeMismatch => "dsa: frame size mismatch",
            ErrorKind::NoFrames => "dsa: no frames to average",
            ErrorKind::Motion => "dsa: mask motion cannot be estimated",
            ErrorKind::FrameTooSmall => "dsa: frame too small for phase correlation",
            ErrorKind::FunctionalGroupConflict => {
                crate::object::geometry::ERR_FUNCTIONAL_GROUP_CONFLICT
            }
            ErrorKind::Optimize | ErrorKind::Other => "",
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    kind: ErrorKind,
    also: Option<ErrorKind>,
    message: String,
}
impl Error {
    pub(crate) fn sentinel(kind: ErrorKind) -> Error {
        Error {
            kind,
            also: None,
            message: kind.text().to_string(),
        }
    }
    pub(crate) fn wrap(kind: ErrorKind, detail: impl fmt::Display) -> Error {
        Error {
            kind,
            also: None,
            message: format!("{}: {detail}", kind.text()),
        }
    }
    pub(crate) fn with_message(kind: ErrorKind, message: String) -> Error {
        Error {
            kind,
            also: None,
            message,
        }
    }
    pub(crate) fn prefixed(self, prefix: impl fmt::Display) -> Error {
        Error {
            message: format!("{prefix}: {}", self.message),
            ..self
        }
    }
    pub(crate) fn and(mut self, kind: ErrorKind) -> Error {
        self.also = Some(kind);
        self
    }
    pub fn kind(&self) -> ErrorKind {
        self.kind
    }
    pub fn is(&self, kind: ErrorKind) -> bool {
        self.kind == kind || self.also == Some(kind)
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for Error {}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MaskSubtractionItem {
    pub mask_operation: String,
    pub subtraction_item_id: Option<i64>,
    pub applicable_frame_range: Vec<i64>,
    pub mask_frame_numbers: Vec<i64>,
    pub contrast_frame_averaging: Option<i64>,
    pub mask_sub_pixel_shift: Option<Vec<f64>>,
    pub tid_offset: Option<i64>,
    pub tid_offset_defaulted: bool,
    pub explanation: String,
    pub mask_selection_mode: String,
}
impl MaskSubtractionItem {
    pub fn applies_to_frame(&self, frame_number: i64) -> bool {
        let r = &self.applicable_frame_range;
        if r.is_empty() {
            return true;
        }
        let mut i = 0;
        while i + 1 < r.len() {
            if frame_number >= r[i] && frame_number <= r[i + 1] {
                return true;
            }
            i += 2;
        }
        false
    }
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FramePixelShift {
    pub subtraction_item_id: Option<i64>,
    pub mask_sub_pixel_shift: Option<Vec<f64>>,
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Module {
    pub items: Vec<MaskSubtractionItem>,
    pub recommended_viewing_mode: String,
    pub number_of_frames: i64,
    pub shared_pixel_shifts: Vec<FramePixelShift>,
    pub per_frame_pixel_shifts: Vec<Vec<FramePixelShift>>,
}
pub fn read_module(obj: &Object) -> Option<Module> {
    let seq_items = obj.get_sequence(TAG_MASK_SUBTRACTION_SEQUENCE)?;
    let mut module = Module {
        recommended_viewing_mode: recommended_viewing_mode(obj),
        number_of_frames: number_of_frames(obj),
        ..Module::default()
    };
    for item in &seq_items {
        module.items.push(read_item(item));
    }
    if module.items.is_empty() {
        return None;
    }
    if let Some(shared) = obj.get_sequence(TAG_SHARED_FUNCTIONAL_GROUPS)
        && let Some(first) = shared.first()
    {
        module.shared_pixel_shifts = read_frame_pixel_shifts(first);
    }
    if let Some(frames) = obj.get_sequence(TAG_PER_FRAME_FUNCTIONAL_GROUPS) {
        module.per_frame_pixel_shifts = frames.iter().map(read_frame_pixel_shifts).collect();
    }
    Some(module)
}
pub fn read_mask_subtraction(obj: &Object) -> Option<Vec<MaskSubtractionItem>> {
    read_module(obj).map(|m| m.items)
}
pub fn recommended_viewing_mode(obj: &Object) -> String {
    obj.get_string(TAG_RECOMMENDED_VIEWING_MODE)
        .map(|v| v.trim().to_string())
        .unwrap_or_default()
}
fn read_item(seq_item: &Object) -> MaskSubtractionItem {
    let mut item = MaskSubtractionItem::default();
    if let Some(op) = seq_item.get_string(TAG_MASK_OPERATION) {
        item.mask_operation = op.trim().to_string();
    }
    if let Some(explanation) = seq_item.get_string(TAG_MASK_OPERATION_EXPLANATION) {
        item.explanation = explanation;
    }
    if let Some(mode) = seq_item.get_string(TAG_MASK_SELECTION_MODE) {
        item.mask_selection_mode = mode.trim().to_string();
    }
    if let Some(id) = get_uint16s(seq_item, TAG_SUBTRACTION_ITEM_ID) {
        item.subtraction_item_id = id.first().copied();
    }
    if let Some(range) = get_uint16s(seq_item, TAG_APPLICABLE_FRAME_RANGE) {
        item.applicable_frame_range = range;
    }
    if let Some(numbers) = get_uint16s(seq_item, TAG_MASK_FRAME_NUMBERS) {
        item.mask_frame_numbers = numbers;
    }
    if let Some(averaging) = get_uint16s(seq_item, TAG_CONTRAST_FRAME_AVERAGING) {
        item.contrast_frame_averaging = averaging.first().copied();
    }
    item.mask_sub_pixel_shift = get_float32s(seq_item, TAG_MASK_SUB_PIXEL_SHIFT);
    if seq_item.has(TAG_TID_OFFSET) {
        match get_uint16s(seq_item, TAG_TID_OFFSET).and_then(|v| v.first().copied()) {
            Some(offset) => item.tid_offset = Some(i64::from(offset as u16 as i16)),
            None => {
                item.tid_offset = Some(DEFAULT_TID_OFFSET);
                item.tid_offset_defaulted = true;
            }
        }
    }
    item
}
fn read_frame_pixel_shifts(group: &Object) -> Vec<FramePixelShift> {
    let Some(items) = group.get_sequence(TAG_FRAME_PIXEL_SHIFT_SEQUENCE) else {
        return Vec::new();
    };
    items
        .iter()
        .map(|item| FramePixelShift {
            mask_sub_pixel_shift: get_float32s(item, TAG_MASK_SUB_PIXEL_SHIFT),
            subtraction_item_id: get_uint16s(item, TAG_SUBTRACTION_ITEM_ID)
                .and_then(|id| id.first().copied()),
        })
        .collect()
}
fn number_of_frames(obj: &Object) -> i64 {
    if !obj.has(TAG_NUMBER_OF_FRAMES) {
        return 1;
    }
    let Some(value) = obj.get_string(TAG_NUMBER_OF_FRAMES) else {
        return 0;
    };
    match crate::numtext::parse_int(value.trim()) {
        Ok(n) if n >= 0 => n,
        _ => 0,
    }
}
fn get_uint16s(obj: &Object, tag: Tag) -> Option<Vec<i64>> {
    let raw = obj.get_raw(tag)?;
    if raw.len() < 2 {
        return None;
    }
    let big = obj.value_big_endian();
    Some(
        raw.as_chunks::<2>()
            .0
            .iter()
            .map(|c| {
                let b = [c[0], c[1]];
                i64::from(if big {
                    u16::from_be_bytes(b)
                } else {
                    u16::from_le_bytes(b)
                })
            })
            .collect(),
    )
}
fn get_float32s(obj: &Object, tag: Tag) -> Option<Vec<f64>> {
    let raw = obj.get_raw(tag)?;
    if raw.len() < 4 {
        return None;
    }
    let big = obj.value_big_endian();
    Some(
        raw.as_chunks::<4>()
            .0
            .iter()
            .map(|c| {
                let b = [c[0], c[1], c[2], c[3]];
                f64::from(if big {
                    f32::from_be_bytes(b)
                } else {
                    f32::from_le_bytes(b)
                })
            })
            .collect(),
    )
}
pub(crate) fn finite(value: f64) -> bool {
    !value.is_nan() && !value.is_infinite()
}
pub(crate) fn fmin(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else if a < b || (a == b && a.is_sign_negative()) {
        a
    } else {
        b
    }
}
pub(crate) fn fmax(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else if a > b || (a == b && a.is_sign_positive()) {
        a
    } else {
        b
    }
}
pub(crate) fn frame_size_matches(columns: i64, rows: i64, frames: &[&[f64]]) -> bool {
    if columns <= 0 || rows <= 0 || columns > i64::MAX / rows {
        return false;
    }
    let size = columns * rows;
    frames.iter().all(|f| f.len() as i64 == size)
}
