#![forbid(unsafe_code)]
use super::error::RenderError;
use crate::display::Lut;
use crate::display::{VoiByteMapper, VoiFunction, VoiLut};
use crate::fmath;
pub const DEFAULT_WINDOW_CENTER: f64 = 40.0;
pub const DEFAULT_WINDOW_WIDTH: f64 = 400.0;
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WindowLevel {
    pub center: f64,
    pub width: f64,
    pub function: VoiFunction,
    pub lut: Option<Lut>,
}
impl WindowLevel {
    pub fn new(center: f64, width: f64) -> WindowLevel {
        WindowLevel {
            center,
            width,
            ..Default::default()
        }
    }
    fn voi(&self) -> VoiLut {
        VoiLut {
            center: self.center,
            width: self.width,
            function: self.function,
            lut: self.lut.clone(),
            explanation: String::new(),
        }
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Rescale {
    pub slope: f64,
    pub intercept: f64,
}
pub fn valid_window_width(width: f64) -> bool {
    width > 0.0 && !width.is_nan() && !width.is_infinite()
}
pub fn normalize_window(window: &WindowLevel, fallback: &WindowLevel) -> WindowLevel {
    if window.lut.is_some() {
        return window.clone();
    }
    let window = if valid_window_width(window.width) {
        window
    } else {
        fallback
    };
    if !valid_window_width(window.width) {
        return WindowLevel::new(DEFAULT_WINDOW_CENTER, DEFAULT_WINDOW_WIDTH);
    }
    window.clone()
}
#[derive(Debug, Clone)]
pub(crate) struct PreparedVoi {
    mapper: VoiByteMapper,
    invalid: bool,
}
pub(crate) fn prepare_window(window: &WindowLevel) -> PreparedVoi {
    PreparedVoi {
        mapper: VoiByteMapper::new(window.voi()),
        invalid: window.center.is_nan() || window.width.is_nan(),
    }
}
impl PreparedVoi {
    pub(crate) fn gray(&self, value: f64) -> u8 {
        if value.is_nan() || self.invalid {
            return 0;
        }
        self.mapper.byte(value)
    }
}
pub fn windowed_gray(value: f64, window: &WindowLevel) -> u8 {
    prepare_window(window).gray(value)
}
pub(crate) fn windowed_unit(value: f64, window: &WindowLevel) -> f64 {
    if value.is_nan() || window.center.is_nan() || window.width.is_nan() {
        return 0.0;
    }
    VoiLut {
        center: window.center,
        width: window.width,
        function: window.function,
        ..Default::default()
    }
    .window_unit(value)
}
pub fn window_slider_ranges(window: &WindowLevel) -> (f64, f64, f64) {
    let center_span = fmath::max(2048.0, window.center.abs() * 2.0 + window.width * 2.0);
    (
        window.center - center_span,
        window.center + center_span,
        fmath::max(4096.0, window.width * 4.0),
    )
}
pub fn validate_window(window: &WindowLevel) -> Result<(), RenderError> {
    if window.lut.as_ref().is_some_and(|l| !l.entries.is_empty()) {
        return Ok(());
    }
    if window.center.is_nan() || window.center.is_infinite() {
        return Err(RenderError::other("render: window center must be finite"));
    }
    if window.width.is_nan() || window.width.is_infinite() || window.width <= 0.0 {
        return Err(RenderError::other(
            "render: window width must be positive and finite",
        ));
    }
    Ok(())
}
