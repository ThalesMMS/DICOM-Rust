use super::lut::Lut;
use crate::fmath;
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ModalityLut {
    pub slope: f64,
    pub intercept: f64,
    pub lut: Option<Lut>,
}
impl ModalityLut {
    pub fn apply(&self, stored: i64) -> f64 {
        match &self.lut {
            Some(lut) => f64::from(lut.lookup(stored)),
            None => stored as f64 * self.effective_slope() + self.intercept,
        }
    }
    pub(super) fn effective_slope(&self) -> f64 {
        if self.slope == 0.0 { 1.0 } else { self.slope }
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum VoiFunction {
    #[default]
    Linear,
    LinearExact,
    Sigmoid,
}
impl VoiFunction {
    pub fn from_index(value: i64) -> VoiFunction {
        match value {
            1 => VoiFunction::LinearExact,
            2 => VoiFunction::Sigmoid,
            _ => VoiFunction::Linear,
        }
    }
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct VoiLut {
    pub center: f64,
    pub width: f64,
    pub function: VoiFunction,
    pub lut: Option<Lut>,
    pub explanation: String,
}
impl VoiLut {
    pub(super) fn has_window(&self) -> bool {
        self.lut.is_none() && self.width > 0.0
    }
    pub fn window_byte(&self, value: f64) -> u8 {
        fmath::f64_to_u8((self.window_unit(value) * 255.0).round())
    }
    pub fn window_unit(&self, value: f64) -> f64 {
        match self.function {
            VoiFunction::Sigmoid => sigmoid_window_unit(value, self.center, self.width),
            VoiFunction::LinearExact => linear_exact_window_unit(value, self.center, self.width),
            VoiFunction::Linear => linear_window_unit(value, self.center, self.width),
        }
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct VoiByteMapper {
    pub(super) voi: VoiLut,
    pub(super) range_min: f64,
    pub(super) range_span: f64,
}
impl VoiByteMapper {
    pub fn new(voi: VoiLut) -> VoiByteMapper {
        let range_span = voi.lut.as_ref().map_or(0.0, |l| f64::from(l.output_max()));
        VoiByteMapper {
            voi,
            range_min: 0.0,
            range_span,
        }
    }
    pub(super) fn range(range_min: f64, range_span: f64) -> VoiByteMapper {
        VoiByteMapper {
            voi: VoiLut::default(),
            range_min,
            range_span,
        }
    }
    pub fn byte(&self, mut value: f64) -> u8 {
        if let Some(lut) = &self.voi.lut {
            value = f64::from(lut.lookup(nearest(value)));
        }
        if self.range_span > 0.0 {
            return scale_to_byte(value, self.range_min, self.range_span);
        }
        self.voi.window_byte(value)
    }
}
pub(crate) fn nearest(x: f64) -> i64 {
    fmath::f64_to_i64((x + 0.5).floor())
}
pub(super) fn scale_to_byte(value: f64, min_value: f64, span: f64) -> u8 {
    if span <= 0.0 {
        return 0;
    }
    let scaled = (value - min_value) / span * 255.0;
    fmath::f64_to_u8(clamp(scaled, 0.0, 255.0).round())
}
pub(super) fn clamp(value: f64, min_value: f64, max_value: f64) -> f64 {
    fmath::max(min_value, fmath::min(max_value, value))
}
fn linear_window_unit(value: f64, center: f64, width: f64) -> f64 {
    let width = fmath::max(width, 1.0);
    let low = center - 0.5 - (width - 1.0) / 2.0;
    let high = center - 0.5 + (width - 1.0) / 2.0;
    if value <= low {
        0.0
    } else if value > high {
        1.0
    } else {
        clamp((value - (center - 0.5)) / (width - 1.0) + 0.5, 0.0, 1.0)
    }
}
fn linear_exact_window_unit(value: f64, center: f64, width: f64) -> f64 {
    if width <= 0.0 {
        return if value > center { 1.0 } else { 0.0 };
    }
    let low = center - width / 2.0;
    let high = center + width / 2.0;
    if value <= low {
        0.0
    } else if value >= high {
        1.0
    } else {
        clamp((value - low) / width, 0.0, 1.0)
    }
}
fn sigmoid_window_unit(value: f64, center: f64, width: f64) -> f64 {
    if width == 0.0 {
        return if value > center { 1.0 } else { 0.0 };
    }
    let y = 1.0 / (1.0 + fmath::exp(-4.0 * (value - center) / width));
    clamp(y, 0.0, 1.0)
}
pub fn valid_window_width(function: VoiFunction, width: f64) -> bool {
    if function == VoiFunction::Linear {
        width >= 1.0
    } else {
        width > 0.0
    }
}
pub(super) fn finite(v: f64) -> bool {
    v.is_finite()
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WindowDomain {
    pub min: f64,
    pub max: f64,
    pub quantum: f64,
    pub unit: String,
    pub known: bool,
}
impl WindowDomain {
    pub fn rescaled(
        bits: i64,
        signed: bool,
        slope: f64,
        intercept: f64,
        unit: &str,
    ) -> WindowDomain {
        if !(1..=32).contains(&bits) || !finite(slope) || !finite(intercept) {
            return WindowDomain::default();
        }
        let slope = if slope == 0.0 { 1.0 } else { slope };
        let (mut lo, mut hi) = (0.0, 2f64.powi(bits as i32) - 1.0);
        if signed {
            lo = -(2f64.powi(bits as i32 - 1));
            hi = 2f64.powi(bits as i32 - 1) - 1.0;
        }
        lo = lo * slope + intercept;
        hi = hi * slope + intercept;
        if lo > hi {
            std::mem::swap(&mut lo, &mut hi);
        }
        if !finite(lo) || !finite(hi) || !finite(hi - lo) {
            return WindowDomain::default();
        }
        WindowDomain {
            min: lo,
            max: hi,
            quantum: slope.abs(),
            unit: unit.to_owned(),
            known: true,
        }
    }
    pub fn measured(values: &[f64], unit: &str) -> WindowDomain {
        let mut d = WindowDomain {
            min: f64::INFINITY,
            max: f64::NEG_INFINITY,
            unit: unit.to_owned(),
            ..WindowDomain::default()
        };
        for &v in values.iter().filter(|v| finite(**v)) {
            d.min = fmath::min(d.min, v);
            d.max = fmath::max(d.max, v);
            d.known = true;
        }
        if !d.known || !finite(d.max - d.min) {
            return WindowDomain::default();
        }
        d
    }
}
pub fn manual_window_minimum(function: VoiFunction, domain: &WindowDomain) -> f64 {
    if function != VoiFunction::LinearExact && function != VoiFunction::Sigmoid {
        return 1.0;
    }
    let magnitude = fmath::max(domain.min.abs(), domain.max.abs());
    let mut ulp = fmath::next_up(magnitude) - magnitude;
    if !finite(ulp) || ulp <= 0.0 {
        ulp = f64::from_bits(1);
    }
    let span = domain.max - domain.min;
    let mut floor = fmath::max(ulp * 16.0, f64::from_bits(1) * 1024.0);
    if domain.known && finite(span) && span > 0.0 {
        floor = fmath::max(floor, span / 1e6);
    }
    floor
}
