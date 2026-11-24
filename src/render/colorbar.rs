#![forbid(unsafe_code)]
use super::error::{ErrorKind, RenderError};
use super::window::{WindowLevel, windowed_gray};
use crate::display::VoiFunction;
use crate::fmath;
use crate::numtext::format_float;
use crate::textfmt::to_upper;
use std::fmt::Write;
use std::sync::Arc;
pub type Nrgba = [u8; 4];
pub type ColorFn = Arc<dyn Fn(f64) -> Nrgba + Send + Sync>;
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Kind {
    #[default]
    Continuous,
    Discrete,
}
pub const DEFAULT_MAX_TICKS: i64 = 6;
const MINIMUM_RASTER_HEIGHT: i64 = 2;
const MAX_DECIMALS: i64 = 6;
fn err_invalid_range() -> RenderError {
    RenderError::new(
        ErrorKind::ColorbarInvalidRange,
        "colorbar: invalid value range",
    )
}
fn err_non_affine() -> RenderError {
    RenderError::new(
        ErrorKind::ColorbarNonAffine,
        "colorbar: calibration must be an increasing affine transform",
    )
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Tick {
    pub value: f64,
    pub position: f64,
    pub label: String,
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Band {
    pub low: f64,
    pub high: f64,
    pub color: Nrgba,
    pub label: String,
}
#[derive(Clone, Default)]
pub struct Legend {
    pub title: String,
    pub unit: String,
    pub kind: Kind,
    pub min: f64,
    pub max: f64,
    pub ticks: Option<Vec<Tick>>,
    pub bands: Option<Vec<Band>>,
    color_at: Option<ColorFn>,
}
impl std::fmt::Debug for Legend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Legend")
            .field("title", &self.title)
            .field("unit", &self.unit)
            .field("kind", &self.kind)
            .field("min", &self.min)
            .field("max", &self.max)
            .field("ticks", &self.ticks)
            .field("bands", &self.bands)
            .finish()
    }
}
fn finite(v: f64) -> bool {
    v.is_finite()
}
fn clamp01(v: f64) -> f64 {
    fmath::max(0.0, fmath::min(1.0, v))
}
fn trim_space(s: &str) -> &str {
    s.trim_matches(char::is_whitespace)
}
impl Legend {
    pub fn ticks(&self) -> &[Tick] {
        self.ticks.as_deref().unwrap_or(&[])
    }
    pub fn bands(&self) -> &[Band] {
        self.bands.as_deref().unwrap_or(&[])
    }
    pub fn valid(&self) -> bool {
        if !finite(self.min) || !finite(self.max) || self.max <= self.min {
            return false;
        }
        if self.kind == Kind::Discrete {
            return !self.bands().is_empty();
        }
        self.color_at.is_some()
    }
    pub fn header(&self) -> String {
        let title = trim_space(&self.title);
        let unit = trim_space(&self.unit);
        match (title.is_empty(), unit.is_empty()) {
            (true, true) => String::new(),
            (true, false) => format!("[{unit}]"),
            (false, true) => title.to_string(),
            (false, false) => format!("{title} [{unit}]"),
        }
    }
    pub fn color_at(&self, value: f64) -> Option<Nrgba> {
        if value.is_nan() {
            return None;
        }
        if self.kind == Kind::Discrete {
            let index = self.band_index(value)?;
            return Some(self.bands()[index].color);
        }
        self.color_at.as_ref().map(|f| f(value))
    }
    pub fn position(&self, value: f64) -> f64 {
        if !self.valid() || value.is_nan() {
            return 0.0;
        }
        if self.kind == Kind::Discrete {
            let bands = self.bands();
            let n = bands.len();
            if value <= bands[0].low {
                return 0.0;
            }
            if value >= bands[n - 1].high {
                return 1.0;
            }
            for (index, band) in bands.iter().enumerate() {
                if value < band.high || index == n - 1 {
                    let width = band.high - band.low;
                    let fraction = if width > 0.0 {
                        (value - band.low) / width
                    } else {
                        0.0
                    };
                    return clamp01((index as f64 + clamp01(fraction)) / n as f64);
                }
            }
            return 1.0;
        }
        clamp01((value - self.min) / (self.max - self.min))
    }
    pub fn value_at_position(&self, position: f64) -> f64 {
        let position = clamp01(position);
        let bands = self.bands();
        if self.kind == Kind::Discrete && !bands.is_empty() {
            let band = &bands[band_slot(position, bands.len())];
            return (band.low + band.high) / 2.0;
        }
        self.min + position * (self.max - self.min)
    }
    pub fn raster(&self, width: i64, height: i64) -> Option<Vec<u8>> {
        if width <= 0 || height < MINIMUM_RASTER_HEIGHT || !self.valid() {
            return None;
        }
        let (w, h) = (width as usize, height as usize);
        let mut out = vec![0u8; w * h * 4];
        for y in 0..h {
            let position = 1.0 - (y as f64 + 0.5) / height as f64;
            let c = if self.kind == Kind::Discrete {
                let bands = self.bands();
                bands[band_slot(position, bands.len())].color
            } else {
                let f = self.color_at.as_ref()?;
                f(self.value_at_position(position))
            };
            for pixel in out[y * w * 4..(y + 1) * w * 4].as_chunks_mut::<4>().0 {
                pixel.copy_from_slice(&c);
            }
        }
        Some(out)
    }
    pub fn signature(&self) -> String {
        let mut b = String::new();
        let _ = write!(
            b,
            "{}|{}|{}|{}|{}",
            self.kind as i64,
            self.title,
            self.unit,
            format_float(self.min, 'g', -1),
            format_float(self.max, 'g', -1)
        );
        for tick in self.ticks() {
            let _ = write!(
                b,
                "|t{}@{}",
                tick.label,
                format_float(tick.position, 'f', 4)
            );
        }
        for band in self.bands() {
            let c = band.color;
            let _ = write!(
                b,
                "|b{}:{}:{:02x}{:02x}{:02x}{:02x}",
                format_float(band.low, 'g', -1),
                format_float(band.high, 'g', -1),
                c[0],
                c[1],
                c[2],
                c[3]
            );
        }
        if self.kind == Kind::Continuous
            && self.valid()
            && let Some(f) = &self.color_at
        {
            for step in 0..=16 {
                let c = f(self.value_at_position(step as f64 / 16.0));
                let _ = write!(b, "|c{:02x}{:02x}{:02x}{:02x}", c[0], c[1], c[2], c[3]);
            }
        }
        b
    }
    fn band_index(&self, value: f64) -> Option<usize> {
        let bands = self.bands();
        let n = bands.len();
        bands.iter().enumerate().position(|(index, band)| {
            value >= band.low && (value < band.high || (index == n - 1 && value <= band.high))
        })
    }
}
fn band_slot(position: f64, n: usize) -> usize {
    let index = fmath::f64_to_i64((position * n as f64).floor());
    if index >= n as i64 {
        n - 1
    } else {
        index as usize
    }
}
#[derive(Clone, Default)]
pub struct WindowSpec {
    pub title: String,
    pub unit: String,
    pub window: WindowLevel,
    pub palette: Option<Arc<dyn Fn(u8) -> Nrgba + Send + Sync>>,
    pub invert: bool,
    pub calibration: Option<Arc<dyn Fn(f64) -> Option<f64> + Send + Sync>>,
    pub max_ticks: i64,
}
pub fn window_range(window: &WindowLevel) -> (f64, f64, bool) {
    if let Some(lut) = &window.lut {
        let entries = lut.entries.len() as i64;
        if entries < 2 {
            return (0.0, 0.0, false);
        }
        let low = lut.first_mapped as f64;
        return (low, low + (entries - 1) as f64, true);
    }
    if !finite(window.center) || !finite(window.width) || window.width <= 0.0 {
        return (0.0, 0.0, false);
    }
    let (mut low, mut high);
    match window.function {
        VoiFunction::LinearExact | VoiFunction::Sigmoid => {
            low = window.center - window.width / 2.0;
            high = window.center + window.width / 2.0;
        }
        VoiFunction::Linear => {
            if window.width < 1.0 {
                return (0.0, 0.0, false);
            }
            low = window.center - 0.5 - (window.width - 1.0) / 2.0;
            high = window.center - 0.5 + (window.width - 1.0) / 2.0;
            if high <= low {
                low = window.center - 1.0;
                high = window.center;
            }
        }
    }
    (low, high, high > low)
}
pub fn for_window(spec: &WindowSpec) -> Result<Legend, RenderError> {
    let (low, high, ok) = window_range(&spec.window);
    if !ok {
        return Err(err_invalid_range());
    }
    let (mut display_low, mut display_high) = (low, high);
    if let Some(calibration) = &spec.calibration {
        let l = calibration(low);
        let h = calibration(high);
        let m = calibration((low + high) / 2.0);
        let (Some(l), Some(h), Some(mid)) = (l, h, m) else {
            return Err(err_non_affine());
        };
        if !finite(l) || !finite(h) || h <= l {
            return Err(err_non_affine());
        }
        display_low = l;
        display_high = h;
        let want = (display_low + display_high) / 2.0;
        if (mid - want).abs() > 1e-9 * fmath::max(1.0, (display_high - display_low).abs()) {
            return Err(err_non_affine());
        }
    }
    let window = spec.window.clone();
    let palette = spec.palette.clone();
    let invert = spec.invert;
    let calibrated = spec.calibration.is_some();
    let color_at: ColorFn = Arc::new(move |display: f64| {
        let raw = if calibrated {
            low + (display - display_low) / (display_high - display_low) * (high - low)
        } else {
            display
        };
        let mut gray = windowed_gray(raw, &window);
        if invert {
            gray = 255 - gray;
        }
        match &palette {
            None => [gray, gray, gray, 255],
            Some(p) => p(gray),
        }
    });
    Ok(Legend {
        title: spec.title.clone(),
        unit: spec.unit.clone(),
        kind: Kind::Continuous,
        min: display_low,
        max: display_high,
        ticks: Some(continuous_ticks(display_low, display_high, spec.max_ticks)),
        bands: None,
        color_at: Some(color_at),
    })
}
pub fn for_function(
    title: &str,
    unit: &str,
    min: f64,
    max: f64,
    color_at: Option<ColorFn>,
    max_ticks: i64,
) -> Result<Legend, RenderError> {
    if !finite(min) || !finite(max) || max <= min {
        return Err(err_invalid_range());
    }
    let Some(color_at) = color_at else {
        return Err(RenderError::other("colorbar: nil colour function"));
    };
    Ok(Legend {
        title: title.into(),
        unit: unit.into(),
        kind: Kind::Continuous,
        min,
        max,
        ticks: Some(continuous_ticks(min, max, max_ticks)),
        bands: None,
        color_at: Some(color_at),
    })
}
pub fn for_bands(title: &str, unit: &str, bands: &[Band]) -> Result<Legend, RenderError> {
    let no_bands = |detail: String| {
        RenderError::new(
            ErrorKind::ColorbarNoBands,
            format!("colorbar: no usable bands{detail}"),
        )
    };
    if bands.is_empty() {
        return Err(no_bands(String::new()));
    }
    let mut out = bands.to_vec();
    for index in 0..out.len() {
        let band = &out[index];
        if !finite(band.low) || !finite(band.high) || band.high <= band.low {
            return Err(no_bands(format!(
                ": band {index} [{}, {}]",
                format_float(band.low, 'g', -1),
                format_float(band.high, 'g', -1)
            )));
        }
        if index > 0 {
            let previous = out[index - 1].high;
            if (band.low - previous).abs() > 1e-9 * fmath::max(1.0, previous.abs()) {
                return Err(no_bands(format!(
                    ": band {index} does not start at {}",
                    format_float(previous, 'g', -1)
                )));
            }
            out[index].low = previous;
        }
    }
    let mut boundaries = Vec::with_capacity(out.len() + 1);
    boundaries.push(out[0].low);
    boundaries.extend(out.iter().map(|b| b.high));
    let decimals = boundary_decimals(&boundaries);
    let ticks = boundaries
        .iter()
        .enumerate()
        .map(|(index, value)| Tick {
            value: *value,
            position: index as f64 / out.len() as f64,
            label: format_decimals(*value, decimals),
        })
        .collect();
    Ok(Legend {
        title: title.into(),
        unit: unit.into(),
        kind: Kind::Discrete,
        min: out[0].low,
        max: out[out.len() - 1].high,
        ticks: Some(ticks),
        bands: Some(out),
        color_at: None,
    })
}
fn continuous_ticks(low: f64, high: f64, max_ticks: i64) -> Vec<Tick> {
    let (values, step) = nice_ticks(low, high, max_ticks);
    values
        .unwrap_or_default()
        .into_iter()
        .map(|value| Tick {
            value,
            position: clamp01((value - low) / (high - low)),
            label: format_value(value, step),
        })
        .collect()
}
fn boundary_decimals(boundaries: &[f64]) -> i64 {
    let (mut largest, mut closest) = (0.0f64, f64::INFINITY);
    for (index, value) in boundaries.iter().enumerate() {
        largest = fmath::max(largest, value.abs());
        if index > 0 {
            closest = fmath::min(closest, boundaries[index] - boundaries[index - 1]);
        }
    }
    let mut decimals = 0;
    if largest > 0.0 {
        decimals = 2 - fmath::f64_to_i64(fmath::log10(largest).floor());
    }
    if finite(closest) && closest > 0.0 {
        decimals = decimals.max(decimals_for_step(closest));
    }
    MAX_DECIMALS.min(decimals).max(0)
}
pub fn nice_ticks(low: f64, high: f64, max_ticks: i64) -> (Option<Vec<f64>>, f64) {
    if !finite(low) || !finite(high) || high <= low {
        return (None, 0.0);
    }
    let max_ticks = if max_ticks < 2 {
        DEFAULT_MAX_TICKS
    } else {
        max_ticks
    };
    let span = nice_number(high - low, false);
    let mut step = nice_number(span / (max_ticks - 1) as f64, true);
    let count = |step: f64| {
        let first = (low / step - 1e-9).ceil();
        let last = (high / step + 1e-9).floor();
        (first, last, fmath::f64_to_i64(last - first).wrapping_add(1))
    };
    let mut guard = 0;
    while guard < 64 && step > 0.0 {
        if count(step).2 <= max_ticks {
            break;
        }
        step = nice_number(step * 1.0001, false);
        guard += 1;
    }
    guard = 0;
    while guard < 64 && step > 0.0 {
        if count(step).2 >= 2 {
            break;
        }
        let finer = previous_nice(step);
        if count(finer).2 > max_ticks {
            break;
        }
        step = finer;
        guard += 1;
    }
    if step <= 0.0 {
        return (Some(vec![low, high]), high - low);
    }
    let (first, last, n) = count(step);
    if n < 2 || n > max_ticks {
        return (Some(vec![low, high]), high - low);
    }
    let mut values = Vec::with_capacity(n as usize);
    let mut k = first;
    while k <= last {
        let mut value = k * step;
        if value == 0.0 {
            value = 0.0;
        }
        values.push(value);
        if k + 1.0 == k {
            break;
        }
        k += 1.0;
    }
    (Some(values), step)
}
fn previous_nice(step: f64) -> f64 {
    let exponent = (fmath::log10(step) + 1e-9).floor();
    let scale = fmath::pow(10.0, exponent);
    let fraction = (step / scale).round();
    if fraction >= 5.0 {
        2.0 * scale
    } else if fraction >= 2.0 {
        scale
    } else {
        scale / 2.0
    }
}
pub fn format_value(value: f64, step: f64) -> String {
    format_decimals(value, decimals_for_step(step))
}
fn decimals_for_step(step: f64) -> i64 {
    if !finite(step) || step <= 0.0 {
        return 0;
    }
    let decimals = fmath::f64_to_i64(-(fmath::log10(step) + 1e-9).floor());
    MAX_DECIMALS.min(decimals).max(0)
}
fn format_decimals(value: f64, decimals: i64) -> String {
    let text = format_float(value, 'f', decimals as i32);
    if let Some(rest) = text.strip_prefix('-')
        && rest.trim_matches(|c| c == '0' || c == '.').is_empty()
    {
        return rest.to_string();
    }
    text
}
fn nice_number(x: f64, round: bool) -> f64 {
    if !finite(x) || x <= 0.0 {
        return 0.0;
    }
    let exponent = fmath::log10(x).floor();
    let scale = fmath::pow(10.0, exponent);
    let fraction = x / scale;
    let nice = if round {
        if fraction < 1.5 {
            1.0
        } else if fraction < 3.0 {
            2.0
        } else if fraction < 7.0 {
            5.0
        } else {
            10.0
        }
    } else if fraction <= 1.0 {
        1.0
    } else if fraction <= 2.0 {
        2.0
    } else if fraction <= 5.0 {
        5.0
    } else {
        10.0
    };
    nice * scale
}
pub fn pet_units_label(code: &str) -> &'static str {
    match to_upper(trim_space(code)).as_str() {
        "CNTS" => "counts",
        "CM2" => "cm²",
        "CM2ML" => "cm²/ml",
        "PCNT" => "%",
        "CPS" => "counts/s",
        "BQML" => "Bq/ml",
        "MGMINML" => "mg/min/ml",
        "UMOLMINML" => "µmol/min/ml",
        "MLMING" => "ml/min/g",
        "MLG" => "ml/g",
        "1CM" => "1/cm",
        "UMOLML" => "µmol/ml",
        "PROPCNTS" => "prop. counts",
        "PROPCPS" => "prop. counts/s",
        "MLMINML" => "ml/min/ml",
        "MLML" => "ml/ml",
        "GML" => "g/ml",
        "STDDEV" => "SD",
        _ => "",
    }
}
