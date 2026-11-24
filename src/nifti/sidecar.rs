use crate::numtext::format_float;
pub const SIDECAR_SCHEMA: &str = "dcm-rust.nifti.sidecar/v1";
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SidecarError {
    InvalidLimit,
    TooLarge { actual_bytes: usize, max_bytes: i64 },
    Json(String),
}
impl std::fmt::Display for SidecarError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SidecarError::InvalidLimit => f.write_str("nifti: sidecar maxBytes must be positive"),
            SidecarError::TooLarge {
                actual_bytes,
                max_bytes,
            } => write!(
                f,
                "nifti: sidecar exceeds byte limit: encoded={actual_bytes} max={max_bytes}"
            ),
            SidecarError::Json(text) => f.write_str(text),
        }
    }
}
impl std::error::Error for SidecarError {}
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CodedUnits {
    pub code: String,
    pub scheme: String,
}
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Sidecar {
    pub dimensions: [i64; 4],
    pub datatype: String,
    pub scaling_policy: String,
    pub reordered: bool,
    pub resampled: bool,
    pub interpolation: String,
    pub units: CodedUnits,
    pub quantity: CodedUnits,
    pub temporal_offsets_seconds: Option<Vec<f64>>,
    pub temporal_durations_seconds: Option<Vec<f64>>,
    pub warnings: Option<Vec<String>>,
    pub slice_spacing_source: String,
    pub pixel_spacing_semantic: String,
}
pub(crate) fn json_string(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '<' | '>' | '&' => out.push_str(&format!("\\u{:04x}", c as u32)),
            '\u{2028}' | '\u{2029}' => out.push_str(&format!("\\u{:04x}", c as u32)),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}
pub(crate) fn json_float(out: &mut String, f: f64) -> Result<(), String> {
    if f.is_nan() || f.is_infinite() {
        return Err(format!(
            "json: unsupported value: {}",
            format_float(f, 'g', -1)
        ));
    }
    let abs = f.abs();
    let exponent = abs != 0.0 && (abs < 1e-6 || abs >= 1e21);
    let mut text = format_float(f, if exponent { 'e' } else { 'f' }, -1);
    if exponent {
        let b = text.as_bytes();
        let n = b.len();
        if n >= 4 && b[n - 4] == b'e' && (b[n - 3] == b'-' || b[n - 3] == b'+') && b[n - 2] == b'0'
        {
            text.remove(n - 2);
        }
    }
    out.push_str(&text);
    Ok(())
}
fn json_floats(out: &mut String, values: &Option<Vec<f64>>) -> Result<(), String> {
    let Some(values) = values else {
        out.push_str("null");
        return Ok(());
    };
    out.push('[');
    for (index, &value) in values.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        json_float(out, value)?;
    }
    out.push(']');
    Ok(())
}
fn json_units(out: &mut String, units: &CodedUnits) {
    out.push_str("{\"code\":");
    json_string(out, &units.code);
    out.push_str(",\"scheme\":");
    json_string(out, &units.scheme);
    out.push('}');
}
impl Sidecar {
    pub fn marshal(&self, max_bytes: i64) -> Result<Vec<u8>, SidecarError> {
        if max_bytes <= 0 {
            return Err(SidecarError::InvalidLimit);
        }
        let mut out = String::new();
        out.push_str("{\"schema\":");
        json_string(&mut out, SIDECAR_SCHEMA);
        out.push_str(&format!(
            ",\"dimensions\":[{},{},{},{}]",
            self.dimensions[0], self.dimensions[1], self.dimensions[2], self.dimensions[3]
        ));
        out.push_str(",\"datatype\":");
        json_string(&mut out, &self.datatype);
        out.push_str(",\"scaling_policy\":");
        json_string(&mut out, &self.scaling_policy);
        out.push_str(&format!(
            ",\"reordered\":{},\"resampled\":{}",
            self.reordered, self.resampled
        ));
        out.push_str(",\"interpolation\":");
        json_string(&mut out, &self.interpolation);
        out.push_str(",\"units\":");
        json_units(&mut out, &self.units);
        out.push_str(",\"quantity\":");
        json_units(&mut out, &self.quantity);
        out.push_str(",\"temporal_offsets_seconds\":");
        json_floats(&mut out, &self.temporal_offsets_seconds).map_err(SidecarError::Json)?;
        out.push_str(",\"temporal_durations_seconds\":");
        json_floats(&mut out, &self.temporal_durations_seconds).map_err(SidecarError::Json)?;
        out.push_str(",\"warnings\":");
        match &self.warnings {
            None => out.push_str("null"),
            Some(warnings) => {
                out.push('[');
                for (index, warning) in warnings.iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    json_string(&mut out, warning);
                }
                out.push(']');
            }
        }
        if !self.slice_spacing_source.is_empty() {
            out.push_str(",\"slice_spacing_source\":");
            json_string(&mut out, &self.slice_spacing_source);
        }
        if !self.pixel_spacing_semantic.is_empty() {
            out.push_str(",\"pixel_spacing_semantic\":");
            json_string(&mut out, &self.pixel_spacing_semantic);
        }
        out.push('}');
        if out.len() as i64 > max_bytes {
            return Err(SidecarError::TooLarge {
                actual_bytes: out.len(),
                max_bytes,
            });
        }
        Ok(out.into_bytes())
    }
}
