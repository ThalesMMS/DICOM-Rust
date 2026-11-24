mod codestream;
mod decode;
mod dwt;
mod geometry;
mod mq;
mod packet;
mod par;
mod tier1;
pub use codestream::{
    Codestream, CodingStyle, Component, ProgressionChange, Quantization, TilePart, parse_codestream,
};
pub use decode::{DecodeOptions, Image, decode, with_decode_threads};
pub use geometry::Limits;
use std::fmt;
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoreError {
    Plain(String),
    Unsupported(String),
    Limit {
        resource: &'static str,
        value: i64,
        limit: i64,
    },
}
impl fmt::Display for CoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CoreError::Plain(text) => f.write_str(text),
            CoreError::Unsupported(feature) => write!(f, "jpeg2000: unsupported: {feature}"),
            CoreError::Limit {
                resource,
                value,
                limit,
            } => {
                write!(f, "jpeg2000: {resource} {value} exceeds the limit {limit}")
            }
        }
    }
}
impl std::error::Error for CoreError {}
pub(crate) type Result<T> = std::result::Result<T, CoreError>;
fn errorf(text: impl fmt::Display) -> CoreError {
    CoreError::Plain(format!("jpeg2000: {text}"))
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Rect {
    pub min_x: i64,
    pub min_y: i64,
    pub max_x: i64,
    pub max_y: i64,
}
impl Rect {
    pub fn new(x0: i64, y0: i64, x1: i64, y1: i64) -> Rect {
        let (min_x, max_x) = if x0 > x1 { (x1, x0) } else { (x0, x1) };
        let (min_y, max_y) = if y0 > y1 { (y1, y0) } else { (y0, y1) };
        Rect {
            min_x,
            min_y,
            max_x,
            max_y,
        }
    }
    pub fn dx(&self) -> i64 {
        self.max_x.saturating_sub(self.min_x)
    }
    pub fn dy(&self) -> i64 {
        self.max_y.saturating_sub(self.min_y)
    }
    pub fn is_empty(&self) -> bool {
        self.min_x >= self.max_x || self.min_y >= self.max_y
    }
    pub fn is_in(&self, s: &Rect) -> bool {
        if self.is_empty() {
            return true;
        }
        s.min_x <= self.min_x
            && self.max_x <= s.max_x
            && s.min_y <= self.min_y
            && self.max_y <= s.max_y
    }
    pub fn overlaps(&self, s: &Rect) -> bool {
        !self.is_empty()
            && !s.is_empty()
            && self.min_x < s.max_x
            && s.min_x < self.max_x
            && self.min_y < s.max_y
            && s.min_y < self.max_y
    }
}
impl fmt::Display for Rect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "({},{})-({},{})",
            self.min_x, self.min_y, self.max_x, self.max_y
        )
    }
}
