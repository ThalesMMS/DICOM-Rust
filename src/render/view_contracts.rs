#![forbid(unsafe_code)]
use super::affine::GeometryAffine;
use super::error::{ErrorKind, RenderError};
use super::parallel::{Cancellation, ctx_err};
use super::vec3::Vec3;
use super::view_state::{
    Interpolation, JsonObject, MAX_TRANSFER_LUT_SAMPLES, MAX_VIEW_OUTPUT_BYTES,
    MAX_VIEW_OUTPUT_DIMENSION, TransferLut, TransferSample, ViewKind, ViewState, finite, finite3,
    invalid_view_state, json_transfer_lut, mpr_presentation_rgba, transfer_sample_unit,
    validate_view_state,
};
use super::volume_store::{VolumeDescriptor, validate_descriptor};
type Ctx<'a> = Option<&'a dyn Cancellation>;
fn shortest32(x: f32) -> (Vec<u8>, i32) {
    let text = format!("{:e}", x.abs());
    let (mantissa, exponent) = text.split_once('e').unwrap_or((&text, "0"));
    let digits: Vec<u8> = mantissa.bytes().filter(u8::is_ascii_digit).collect();
    let exp: i32 = exponent.parse().unwrap_or(0);
    let mut digits = digits;
    while digits.len() > 1 && digits.last() == Some(&b'0') {
        digits.pop();
    }
    (digits, exp + 1)
}
fn push_e(out: &mut String, digits: &[u8], dp: i32) {
    out.push(digits[0] as char);
    if digits.len() > 1 {
        out.push('.');
        for d in &digits[1..] {
            out.push(*d as char);
        }
    }
    let exp = dp - 1;
    out.push('e');
    out.push(if exp < 0 { '-' } else { '+' });
    let e = exp.abs();
    if e < 10 {
        out.push('0');
    }
    out.push_str(&e.to_string());
}
fn push_f(out: &mut String, digits: &[u8], dp: i32) {
    if dp <= 0 {
        out.push('0');
        if !digits.is_empty() {
            out.push('.');
            for _ in 0..-dp {
                out.push('0');
            }
            for d in digits {
                out.push(*d as char);
            }
        }
        return;
    }
    let dp = dp as usize;
    for i in 0..dp {
        out.push(if i < digits.len() {
            digits[i] as char
        } else {
            '0'
        });
    }
    if digits.len() > dp {
        out.push('.');
        for d in &digits[dp..] {
            out.push(*d as char);
        }
    }
}
pub(crate) fn f32_text(x: f32) -> String {
    if x.is_nan() {
        return "NaN".into();
    }
    if x.is_infinite() {
        return if x > 0.0 { "+Inf" } else { "-Inf" }.into();
    }
    let mut out = String::new();
    if x.is_sign_negative() {
        out.push('-');
    }
    if x == 0.0 {
        out.push('0');
        return out;
    }
    let (digits, dp) = shortest32(x);
    let exp = dp - 1;
    if !(-4..6).contains(&exp) {
        push_e(&mut out, &digits, dp);
    } else {
        push_f(&mut out, &digits, dp);
    }
    out
}
pub(crate) fn json_f32(out: &mut String, x: f32) {
    if !x.is_finite() {
        out.push_str(&f32_text(x));
        return;
    }
    if x.is_sign_negative() {
        out.push('-');
    }
    if x == 0.0 {
        out.push('0');
        return;
    }
    let abs = x.abs();
    let (digits, dp) = shortest32(x);
    if abs < 1e-6 || abs >= 1e21 {
        let mut text = String::new();
        push_e(&mut text, &digits, dp);
        let b = text.as_bytes();
        let n = b.len();
        if n >= 4 && b[n - 4] == b'e' && (b[n - 3] == b'-' || b[n - 3] == b'+') && b[n - 2] == b'0'
        {
            text.remove(n - 2);
        }
        out.push_str(&text);
    } else {
        push_f(out, &digits, dp);
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StereoMode(pub u32);
impl StereoMode {
    pub const MONO: StereoMode = StereoMode(0);
    pub const SIDE_BY_SIDE: StereoMode = StereoMode(1);
    pub const RED_CYAN: StereoMode = StereoMode(2);
}
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct StereoPresentationState {
    pub mode: StereoMode,
    pub eye_separation_mm: f64,
    pub convergence_mm: f64,
}
impl StereoPresentationState {
    pub fn validate(&self) -> Result<(), RenderError> {
        let invalid = || invalid_view_state("invalid stereo presentation");
        if self.mode.0 > StereoMode::RED_CYAN.0
            || !finite(self.eye_separation_mm)
            || !finite(self.convergence_mm)
            || self.eye_separation_mm < 0.0
            || self.eye_separation_mm > 1000.0
            || self.convergence_mm < 0.0
            || self.convergence_mm > 1_000_000.0
        {
            return Err(invalid());
        }
        if self.mode != StereoMode::MONO
            && (self.convergence_mm == 0.0 || self.eye_separation_mm > self.convergence_mm)
        {
            return Err(invalid());
        }
        Ok(())
    }
    pub(crate) fn json(&self, out: &mut String) {
        let mut o = JsonObject::new(out);
        o.int("Mode", self.mode.0);
        o.f64("EyeSeparationMM", self.eye_separation_mm);
        o.f64("ConvergenceMM", self.convergence_mm);
        o.finish();
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct StereoEyeViews {
    pub left: ViewState,
    pub right: ViewState,
    pub convergence_lps: [f64; 3],
}
pub fn derive_stereo_eye_views(center: &ViewState) -> Result<StereoEyeViews, RenderError> {
    validate_view_state(center)?;
    if center.kind != ViewKind::VR || center.vr.stereo.mode == StereoMode::MONO {
        return Err(invalid_view_state(
            "stereo eye views require active VR stereo",
        ));
    }
    let config = center.vr.stereo;
    let v = |a: [f64; 3]| Vec3::new(a[0], a[1], a[2]);
    let pos = v(center.vr.position_lps);
    let focus = v(center.vr.focal_point_lps);
    let up = v(center.vr.view_up_lps);
    let forward = focus.sub(pos).normalize();
    let right = forward.cross(up).normalize();
    let convergence = pos.add(forward.scale(config.convergence_mm));
    let array = |p: Vec3| [p.x, p.y, p.z];
    let mut result = StereoEyeViews {
        left: center.clone(),
        right: center.clone(),
        convergence_lps: array(convergence),
    };
    if config.mode == StereoMode::SIDE_BY_SIDE {
        if center.output_width < 2 {
            return Err(invalid_view_state(
                "stereo surface must be at least two pixels wide",
            ));
        }
        result.left.output_width = center.output_width.div_ceil(2);
        result.right.output_width = result.left.output_width;
    }
    result.left.vr.stereo = StereoPresentationState::default();
    result.right.vr.stereo = StereoPresentationState::default();
    if config.eye_separation_mm != 0.0 {
        result.left.vr.position_lps = array(pos.add(right.scale(-config.eye_separation_mm / 2.0)));
        result.right.vr.position_lps = array(pos.add(right.scale(config.eye_separation_mm / 2.0)));
        result.left.vr.focal_point_lps = array(convergence);
        result.right.vr.focal_point_lps = array(convergence);
    }
    validate_view_state(&result.left)?;
    validate_view_state(&result.right)?;
    if !finite3(result.convergence_lps) {
        return Err(invalid_view_state("stereo convergence overflow"));
    }
    Ok(result)
}
pub const FRAME_OUTPUT_CONTRACT_VERSION: u32 = 1;
pub const FRAME_OUTPUT_HEADER_SIZE_V1: u32 = 96;
pub const MAX_FRAME_BACKEND_ID_BYTES: usize = 128;
pub const ERR_INVALID_FRAME_OUTPUT: &str = "render: invalid frame output";
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FrameFormat(pub u32);
impl FrameFormat {
    pub const UNKNOWN: FrameFormat = FrameFormat(0);
    pub const GRAY_F32_TOP_LEFT: FrameFormat = FrameFormat(1);
    pub const RGBA8_NRGBA_TOP_LEFT: FrameFormat = FrameFormat(2);
    pub const SCALAR_VALIDITY_F32_TOP_LEFT: FrameFormat = FrameFormat(3);
}
pub const FRAME_WARNING_HOST_RENDER_TIMING: u64 = 1;
pub const FRAME_WARNING_INTERACTIVE_LOD: u64 = 2;
pub const FRAME_WARNING_MPR_ANTIALIAS_2X: u64 = 4;
pub const FRAME_WARNING_VOLUME_FILTERED: u64 = 8;
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FrameOutput {
    pub contract_version: u32,
    pub struct_size: u32,
    pub format: FrameFormat,
    pub width: u32,
    pub height: u32,
    pub stride_bytes: u64,
    pub capacity_bytes: u64,
    pub written_bytes: u64,
    pub volume_generation: u64,
    pub view_generation: u64,
    pub presentation_generation: u64,
    pub render_time_ns: u64,
    pub readback_time_ns: u64,
    pub warning_bits: u64,
    pub backend_id: String,
    pub pixels: Vec<u8>,
}
fn frame_error(detail: impl AsRef<str>) -> RenderError {
    RenderError::new(
        ErrorKind::InvalidFrameOutput,
        format!("{ERR_INVALID_FRAME_OUTPUT}: {}", detail.as_ref()),
    )
}
pub fn validate_frame_output(
    output: &FrameOutput,
    expected: &ViewState,
) -> Result<(), RenderError> {
    if output.contract_version != FRAME_OUTPUT_CONTRACT_VERSION {
        return Err(frame_error(format!(
            "contract version {}",
            output.contract_version
        )));
    }
    if output.struct_size < FRAME_OUTPUT_HEADER_SIZE_V1 {
        return Err(frame_error(format!(
            "struct size {} below V1 minimum {FRAME_OUTPUT_HEADER_SIZE_V1}",
            output.struct_size
        )));
    }
    let bytes_per_pixel: u64 = match output.format {
        FrameFormat::GRAY_F32_TOP_LEFT | FrameFormat::RGBA8_NRGBA_TOP_LEFT => 4,
        FrameFormat::SCALAR_VALIDITY_F32_TOP_LEFT => 8,
        other => return Err(frame_error(format!("unsupported format {}", other.0))),
    };
    if output.width == 0
        || output.height == 0
        || output.width > MAX_VIEW_OUTPUT_DIMENSION
        || output.height > MAX_VIEW_OUTPUT_DIMENSION
    {
        return Err(frame_error(format!(
            "output dimensions {}x{} outside bound",
            output.width, output.height
        )));
    }
    let packed = u64::from(output.width).checked_mul(bytes_per_pixel);
    let Some(packed) = packed.filter(|p| output.stride_bytes >= *p) else {
        return Err(frame_error(format!(
            "stride {} below packed row {}",
            output.stride_bytes,
            packed.unwrap_or(0)
        )));
    };
    let Some(last_row) = u64::from(output.height - 1).checked_mul(output.stride_bytes) else {
        return Err(frame_error("frame byte size overflow"));
    };
    let addressed = last_row.checked_add(packed);
    if addressed.is_none_or(|a| output.capacity_bytes < a || output.written_bytes != a) {
        return Err(frame_error(format!(
            "capacity/written mismatch: capacity={} written={} addressed={}",
            output.capacity_bytes,
            output.written_bytes,
            addressed.unwrap_or(0)
        )));
    }
    if output.capacity_bytes > MAX_VIEW_OUTPUT_BYTES || output.written_bytes > MAX_VIEW_OUTPUT_BYTES
    {
        return Err(frame_error(format!(
            "frame bytes exceed {MAX_VIEW_OUTPUT_BYTES}"
        )));
    }
    if (output.pixels.len() as u64) < output.written_bytes {
        return Err(frame_error(format!(
            "pixel buffer length {} below written bytes {}",
            output.pixels.len(),
            output.written_bytes
        )));
    }
    if output.capacity_bytes > output.pixels.len() as u64 {
        return Err(frame_error(format!(
            "capacity {} exceeds pixel buffer length {}",
            output.capacity_bytes,
            output.pixels.len()
        )));
    }
    if output.backend_id.is_empty() || output.backend_id.len() > MAX_FRAME_BACKEND_ID_BYTES {
        return Err(frame_error(format!(
            "backend id length {} outside 1..{MAX_FRAME_BACKEND_ID_BYTES}",
            output.backend_id.len()
        )));
    }
    if output.volume_generation == 0 || output.view_generation == 0 {
        return Err(frame_error("zero generation"));
    }
    if expected.contract_version != 0
        && (output.width != expected.output_width
            || output.height != expected.output_height
            || output.volume_generation != expected.volume_generation
            || output.view_generation != expected.view_generation
            || output.presentation_generation != expected.presentation_generation)
    {
        return Err(frame_error(
            "output does not match requested dimensions/generations",
        ));
    }
    let mpr = expected.kind == ViewKind::MPR;
    if mpr
        && expected.mpr.scalar_validity
        && output.format != FrameFormat::SCALAR_VALIDITY_F32_TOP_LEFT
    {
        return Err(frame_error(
            "quantitative MPR output has no scalar validity",
        ));
    }
    let component_rgba = expected.components.mode != ComponentPresentationMode::SCALAR;
    let presented = mpr_presentation_rgba(&expected.mpr);
    if mpr
        && !expected.mpr.scalar_validity
        && !presented
        && !component_rgba
        && output.format != FrameFormat::GRAY_F32_TOP_LEFT
    {
        return Err(frame_error(format!(
            "MPR output format {} is not GRAY_F32",
            output.format.0
        )));
    }
    if mpr && (presented || component_rgba) && output.format != FrameFormat::RGBA8_NRGBA_TOP_LEFT {
        return Err(frame_error(format!(
            "presented MPR output format {} is not RGBA8_NRGBA",
            output.format.0
        )));
    }
    if expected.kind == ViewKind::VR && output.format != FrameFormat::RGBA8_NRGBA_TOP_LEFT {
        return Err(frame_error(format!(
            "VR output format {} is not RGBA8_NRGBA",
            output.format.0
        )));
    }
    Ok(())
}
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ScalarValiditySamples {
    pub values: Vec<f64>,
    pub valid: Vec<bool>,
}
pub fn decode_scalar_validity_frame(
    ctx: Ctx<'_>,
    frame: &FrameOutput,
) -> Result<ScalarValiditySamples, RenderError> {
    if let Some(e) = ctx_err(ctx) {
        return Err(e);
    }
    validate_frame_output(frame, &ViewState::default())?;
    if frame.format != FrameFormat::SCALAR_VALIDITY_F32_TOP_LEFT {
        return Err(frame_error("scalar validity format required"));
    }
    let (w, h) = (frame.width as usize, frame.height as usize);
    let mut result = ScalarValiditySamples {
        values: vec![0.0; w * h],
        valid: vec![false; w * h],
    };
    let one = 1.0f32.to_bits();
    for y in 0..h {
        if let Some(e) = ctx_err(ctx) {
            return Err(e);
        }
        for x in 0..w {
            let at = (y as u64 * frame.stride_bytes + x as u64 * 8) as usize;
            let word = |o: usize| {
                u32::from_le_bytes([
                    frame.pixels[o],
                    frame.pixels[o + 1],
                    frame.pixels[o + 2],
                    frame.pixels[o + 3],
                ])
            };
            let value = f64::from(f32::from_bits(word(at)));
            let flag = word(at + 4);
            if !finite(value) || (flag != 0 && flag != one) {
                return Err(frame_error(format!(
                    "invalid quantitative sample at {x},{y}"
                )));
            }
            let index = y * w + x;
            result.values[index] = value;
            result.valid[index] = flag == one;
        }
    }
    Ok(result)
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SurfaceProbeOperator(pub u32);
impl SurfaceProbeOperator {
    pub const POINT: SurfaceProbeOperator = SurfaceProbeOperator(0);
    pub const MINIMUM: SurfaceProbeOperator = SurfaceProbeOperator(1);
    pub const MAXIMUM: SurfaceProbeOperator = SurfaceProbeOperator(2);
    pub const MEAN: SurfaceProbeOperator = SurfaceProbeOperator(3);
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SurfaceProbeRegistration(pub u32);
impl SurfaceProbeRegistration {
    pub const SAME_FRAME: SurfaceProbeRegistration = SurfaceProbeRegistration(0);
    pub const EXPLICIT_AFFINE: SurfaceProbeRegistration = SurfaceProbeRegistration(1);
}
pub const MAX_SURFACE_PROBE_SAMPLES: u32 = 512;
pub const MAX_SURFACE_PROBE_WORK: u64 = 16_000_000;
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SurfaceProbeState {
    pub source_generation: u64,
    pub source_dimensions: [u32; 3],
    pub source_id: String,
    pub source_frame_of_reference: String,
    pub surface_frame_of_reference: String,
    pub registration: SurfaceProbeRegistration,
    pub surface_to_source_lps: GeometryAffine,
    pub patient_to_source_index: GeometryAffine,
    pub unit: String,
    pub scale: f64,
    pub offset: f64,
    pub operator: SurfaceProbeOperator,
    pub start_mm: f64,
    pub end_mm: f64,
    pub maximum_step_mm: f64,
    pub reverse_normals: bool,
    pub interpolation: Interpolation,
    pub transfer: TransferLut,
    pub invalid_color: [f64; 3],
}
impl SurfaceProbeState {
    pub fn enabled(&self) -> bool {
        self.source_generation != 0
    }
    pub fn sample_count(&self) -> Result<u32, RenderError> {
        if self.operator == SurfaceProbeOperator::POINT {
            if self.start_mm != 0.0 || self.end_mm != 0.0 || self.maximum_step_mm != 0.0 {
                return Err(invalid_view_state("point probe carries band offsets"));
            }
            return Ok(1);
        }
        if self.operator.0 > SurfaceProbeOperator::MEAN.0
            || !finite(self.start_mm)
            || !finite(self.end_mm)
            || !finite(self.maximum_step_mm)
            || self.end_mm <= self.start_mm
            || self.maximum_step_mm <= 0.0
        {
            return Err(invalid_view_state("invalid surface probe band"));
        }
        let intervals = ((self.end_mm - self.start_mm) / self.maximum_step_mm).ceil();
        if !finite(intervals)
            || intervals < 1.0
            || intervals >= f64::from(MAX_SURFACE_PROBE_SAMPLES)
        {
            return Err(invalid_view_state("surface probe sample limit"));
        }
        Ok(intervals as u32 + 1)
    }
    pub fn validate(&self) -> Result<(), RenderError> {
        let fail = |reason: &str| invalid_view_state(format!("surface probe: {reason}"));
        if !self.enabled() {
            let zero = SurfaceProbeState::default();
            let mut probe = self.clone();
            probe.source_generation = 0;
            if probe != zero {
                return Err(fail("parameters without source generation"));
            }
            return Ok(());
        }
        for value in [
            &self.source_id,
            &self.source_frame_of_reference,
            &self.surface_frame_of_reference,
            &self.unit,
        ] {
            if value.trim_matches(is_unicode_space).is_empty()
                || value.len() > 256
                || value.contains(['\r', '\n', '\0'])
            {
                return Err(fail("missing or invalid source identity, frame or unit"));
            }
        }
        if self.source_dimensions.contains(&0) {
            return Err(fail("empty source"));
        }
        if !finite(self.scale) || self.scale == 0.0 || !finite(self.offset) {
            return Err(fail("invalid quantitative scale"));
        }
        let identity = GeometryAffine([
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ]);
        if self.registration == SurfaceProbeRegistration::SAME_FRAME {
            if self.source_frame_of_reference != self.surface_frame_of_reference
                || self.surface_to_source_lps != identity
            {
                return Err(fail(
                    "same-frame registration requires matching frames and identity",
                ));
            }
        } else if self.registration != SurfaceProbeRegistration::EXPLICIT_AFFINE {
            return Err(fail("unknown registration"));
        }
        if !self.surface_to_source_lps.inverse().1 {
            return Err(fail("invalid registration affine"));
        }
        if !self.patient_to_source_index.inverse().1 {
            return Err(fail("invalid source index affine"));
        }
        if self.interpolation != Interpolation::NEAREST
            && self.interpolation != Interpolation::LINEAR
        {
            return Err(fail("unsupported interpolation"));
        }
        self.sample_count()?;
        let t = &self.transfer;
        if !finite(t.domain_min)
            || !finite(t.domain_max)
            || t.domain_min >= t.domain_max
            || t.samples.len() < 2
            || t.samples.len() > MAX_TRANSFER_LUT_SAMPLES
        {
            return Err(fail("invalid color transfer"));
        }
        for sample in &t.samples {
            if sample.a != 1.0 {
                return Err(fail("surface probe color table must be opaque"));
            }
            for value in [sample.r, sample.g, sample.b] {
                if !value.is_finite() || !(0.0..=1.0).contains(&value) {
                    return Err(fail("invalid color sample"));
                }
            }
        }
        if self
            .invalid_color
            .iter()
            .any(|v| !finite(*v) || !(0.0..=1.0).contains(v))
        {
            return Err(fail("invalid missing-sample color"));
        }
        Ok(())
    }
    pub(crate) fn json(&self, out: &mut String) {
        use crate::dicomjson::jsonwire::append_string;
        let mut o = JsonObject::new(out);
        o.int("SourceGeneration", self.source_generation);
        o.uints("SourceDimensions", &self.source_dimensions);
        append_string(o.key("SourceID"), &self.source_id);
        append_string(
            o.key("SourceFrameOfReference"),
            &self.source_frame_of_reference,
        );
        append_string(
            o.key("SurfaceFrameOfReference"),
            &self.surface_frame_of_reference,
        );
        o.int("Registration", self.registration.0);
        o.floats("SurfaceToSourceLPS", &self.surface_to_source_lps.0);
        o.floats("PatientToSourceIndex", &self.patient_to_source_index.0);
        append_string(o.key("Unit"), &self.unit);
        o.f64("Scale", self.scale);
        o.f64("Offset", self.offset);
        o.int("Operator", self.operator.0);
        o.f64("StartMM", self.start_mm);
        o.f64("EndMM", self.end_mm);
        o.f64("MaximumStepMM", self.maximum_step_mm);
        o.bool("ReverseNormals", self.reverse_normals);
        o.int("Interpolation", self.interpolation.0);
        json_transfer_lut(o.key("Transfer"), &self.transfer);
        o.floats("InvalidColor", &self.invalid_color);
        o.finish();
    }
}
fn is_unicode_space(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\u{b}' | '\u{c}' | '\r' | ' ' | '\u{85}' | '\u{a0}'
    ) || (c as u32 > 0xff && c.is_whitespace())
}
pub fn validate_surface_probe_source(
    s: &SurfaceProbeState,
    d: &VolumeDescriptor,
) -> Result<(), RenderError> {
    s.validate()?;
    validate_descriptor(d)?;
    if !s.enabled()
        || d.components != 1
        || d.volume_generation != s.source_generation
        || d.dimensions != s.source_dimensions
    {
        return Err(invalid_view_state(
            "surface probe source identity/layout mismatch",
        ));
    }
    for row in 0..4 {
        for col in 0..4 {
            let mut want = 0.0;
            for k in 0..4 {
                want +=
                    d.patient_lps_to_index.0[row * 4 + k] * s.surface_to_source_lps.0[k * 4 + col];
            }
            let got = s.patient_to_source_index.0[row * 4 + col];
            if !finite(want) || (got - want).abs() > 1e-10 * crate::fmath::max(1.0, want.abs()) {
                return Err(invalid_view_state(
                    "surface probe index transform does not match source and registration",
                ));
            }
        }
    }
    Ok(())
}
pub fn validate_surface_probe_normals(
    vertices: &[Vec3],
    normals: &[Vec3],
    triangles: &[[u32; 3]],
) -> Result<(), RenderError> {
    let fail = || invalid_view_state("surface probe requires complete consistent unit normals");
    if vertices.is_empty() || normals.len() != vertices.len() || triangles.is_empty() {
        return Err(fail());
    }
    for (i, n) in normals.iter().enumerate() {
        let v = vertices[i];
        if !finite(v.x)
            || !finite(v.y)
            || !finite(v.z)
            || !finite(n.x)
            || !finite(n.y)
            || !finite(n.z)
            || (n.dot(*n) - 1.0).abs() > 1e-3
        {
            return Err(fail());
        }
    }
    for triangle in triangles {
        if triangle.iter().any(|i| *i as usize >= vertices.len()) {
            return Err(fail());
        }
        let p = |k: usize| vertices[triangle[k] as usize];
        let face = p(1).sub(p(0)).cross(p(2).sub(p(0)));
        let length = face.length();
        if !finite(length) || length <= 1e-12 {
            return Err(fail());
        }
        let face = face.scale(1.0 / length);
        let sign = normals[triangle[0] as usize].dot(face);
        if sign.abs() < 1e-4 {
            return Err(fail());
        }
        for index in &triangle[1..] {
            let projection = normals[*index as usize].dot(face);
            if projection.abs() < 1e-4 || projection * sign <= 0.0 {
                return Err(fail());
            }
        }
    }
    Ok(())
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ComponentPresentationMode(pub u32);
impl ComponentPresentationMode {
    pub const SCALAR: ComponentPresentationMode = ComponentPresentationMode(0);
    pub const RGB: ComponentPresentationMode = ComponentPresentationMode(1);
    pub const INDEPENDENT: ComponentPresentationMode = ComponentPresentationMode(2);
}
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ComponentPresentation {
    pub mode: ComponentPresentationMode,
    pub rgb_maximum: f64,
    pub opacity_scale: f64,
    pub transfers: Vec<TransferLut>,
    pub weights: Vec<f64>,
}
impl ComponentPresentation {
    pub fn validate(&self) -> Result<(), RenderError> {
        let fail = |reason: &str| invalid_view_state(format!("component presentation: {reason}"));
        if self.mode == ComponentPresentationMode::SCALAR {
            if self.rgb_maximum != 0.0
                || self.opacity_scale != 0.0
                || !self.transfers.is_empty()
                || !self.weights.is_empty()
            {
                return Err(fail("scalar mode carries component parameters"));
            }
            return Ok(());
        }
        if !finite(self.opacity_scale) || self.opacity_scale <= 0.0 || self.opacity_scale > 16.0 {
            return Err(fail("opacity scale outside (0,16]"));
        }
        match self.mode {
            ComponentPresentationMode::RGB => {
                if !finite(self.rgb_maximum)
                    || self.rgb_maximum <= 0.0
                    || !self.transfers.is_empty()
                    || !self.weights.is_empty()
                {
                    return Err(fail("invalid RGB domain or independent transfers"));
                }
            }
            ComponentPresentationMode::INDEPENDENT => {
                if self.rgb_maximum != 0.0
                    || self.transfers.len() < 2
                    || self.transfers.len() > 4
                    || self.weights.len() != self.transfers.len()
                {
                    return Err(fail("two to four transfers and matching weights required"));
                }
                let mut count = 0;
                for (i, lut) in self.transfers.iter().enumerate() {
                    if !finite(lut.domain_min)
                        || !finite(lut.domain_max)
                        || lut.domain_min >= lut.domain_max
                        || lut.samples.len() < 2
                        || lut.samples.len() > MAX_TRANSFER_LUT_SAMPLES
                    {
                        return Err(fail("invalid component transfer domain or size"));
                    }
                    let w = self.weights[i];
                    if !finite(w) || !(0.0..=1.0).contains(&w) {
                        return Err(fail("weight outside [0,1]"));
                    }
                    count += lut.samples.len();
                    if lut.samples.iter().any(|s| !transfer_sample_unit(s)) {
                        return Err(fail("invalid transfer RGBA"));
                    }
                }
                if count > MAX_TRANSFER_LUT_SAMPLES {
                    return Err(fail("total transfer sample limit exceeded"));
                }
            }
            _ => return Err(fail("unknown component mode")),
        }
        Ok(())
    }
    pub(crate) fn json(&self, out: &mut String) {
        let mut o = JsonObject::new(out);
        o.int("Mode", self.mode.0);
        o.f64("RGBMaximum", self.rgb_maximum);
        o.f64("OpacityScale", self.opacity_scale);
        super::view_state::json_list(o.key("Transfers"), &self.transfers, json_transfer_lut);
        super::view_state::json_list(o.key("Weights"), &self.weights, |out, w| {
            super::view_state::json_f64(out, *w)
        });
        o.finish();
    }
}
pub fn validate_component_view_for_volume(
    descriptor: &VolumeDescriptor,
    state: &ViewState,
) -> Result<(), RenderError> {
    validate_descriptor(descriptor)?;
    validate_view_state(state)?;
    let p = &state.components;
    let c = descriptor.components;
    let valid = c == 1 && p.mode == ComponentPresentationMode::SCALAR
        || (c == 3 || c == 4) && p.mode == ComponentPresentationMode::RGB
        || c > 1
            && p.mode == ComponentPresentationMode::INDEPENDENT
            && p.transfers.len() == c as usize;
    if !valid {
        return Err(invalid_view_state(
            "component material does not match volume channels",
        ));
    }
    Ok(())
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct VolumeFilterMode(pub u32);
impl VolumeFilterMode {
    pub const NONE: VolumeFilterMode = VolumeFilterMode(0);
    pub const GAUSSIAN: VolumeFilterMode = VolumeFilterMode(1);
    pub const BASIC_SMOOTH_5X5: VolumeFilterMode = VolumeFilterMode(2);
    pub const MEDIAN_3X3X3: VolumeFilterMode = VolumeFilterMode(3);
    pub const VESSEL_ENHANCE: VolumeFilterMode = VolumeFilterMode(4);
}
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct VolumeFilterState {
    pub mode: VolumeFilterMode,
    pub sigma_mm: f64,
}
pub const MAX_VOLUME_GAUSSIAN_RADIUS: usize = 64;
impl VolumeFilterState {
    pub fn validate(&self) -> Result<(), RenderError> {
        if self.mode.0 > VolumeFilterMode::VESSEL_ENHANCE.0
            || !finite(self.sigma_mm)
            || (self.mode != VolumeFilterMode::GAUSSIAN && self.sigma_mm != 0.0)
            || (self.mode == VolumeFilterMode::GAUSSIAN
                && (self.sigma_mm <= 0.0 || self.sigma_mm > 100.0))
        {
            return Err(invalid_view_state("invalid volume filter"));
        }
        Ok(())
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct VolumeGaussianPlan {
    pub radius: [u32; 3],
    pub weights: [[f32; 2 * MAX_VOLUME_GAUSSIAN_RADIUS + 1]; 3],
}
impl Default for VolumeGaussianPlan {
    fn default() -> Self {
        VolumeGaussianPlan {
            radius: [0; 3],
            weights: [[0.0; 2 * MAX_VOLUME_GAUSSIAN_RADIUS + 1]; 3],
        }
    }
}
pub fn plan_volume_gaussian(
    d: &VolumeDescriptor,
    s: &VolumeFilterState,
) -> (VolumeGaussianPlan, Option<RenderError>) {
    let mut plan = VolumeGaussianPlan::default();
    if let Err(e) = s.validate() {
        return (plan, Some(e));
    }
    if s.mode != VolumeFilterMode::GAUSSIAN {
        return (
            plan,
            Some(invalid_view_state("Gaussian plan requires Gaussian mode")),
        );
    }
    if let Err(e) = validate_descriptor(d) {
        return (plan, Some(e));
    }
    for axis in 0..3 {
        let n = d.dimensions[axis];
        let a = &d.index_to_patient_lps.0;
        let spacing = crate::fmath::hypot(crate::fmath::hypot(a[axis], a[axis + 4]), a[axis + 8]);
        if spacing <= 0.0 || !finite(spacing) {
            return (plan, Some(invalid_view_state("Gaussian voxel spacing")));
        }
        let sigma = s.sigma_mm / spacing;
        let mut radius = (3.0 * sigma).floor();
        if n < 2 || sigma < 0.01 || radius < 1.0 {
            plan.weights[axis][0] = 1.0;
            continue;
        }
        radius = crate::fmath::min(radius, f64::from(n - 1));
        if radius > MAX_VOLUME_GAUSSIAN_RADIUS as f64 {
            return (
                plan,
                Some(invalid_view_state(format!(
                    "Gaussian radius exceeds {MAX_VOLUME_GAUSSIAN_RADIUS}"
                ))),
            );
        }
        let r = radius as i64;
        plan.radius[axis] = r as u32;
        let mut total = 0.0;
        for k in -r..=r {
            total += crate::fmath::exp(-((k * k) as f64) / (2.0 * sigma * sigma));
        }
        for k in -r..=r {
            plan.weights[axis][(k + r) as usize] =
                (crate::fmath::exp(-((k * k) as f64) / (2.0 * sigma * sigma)) / total) as f32;
        }
    }
    (plan, None)
}
pub fn volume_basic_smooth_kernel() -> [[f32; 5]; 5] {
    let mut kernel = [[0.0f32; 5]; 5];
    for (y, row) in super::vr_prepared::BASIC_SMOOTH_5X5_KERNEL
        .iter()
        .enumerate()
    {
        for (x, weight) in row.iter().enumerate() {
            kernel[y][x] = *weight as f32;
        }
    }
    kernel
}
#[derive(Debug, Clone, PartialEq, Default)]
pub struct VrFusionState {
    pub overlay_generation: u64,
    pub overlay_dimensions: [u32; 3],
    pub patient_to_overlay_index: GeometryAffine,
    pub domain_min: f64,
    pub domain_max: f64,
    pub opacity: f64,
    pub opacity_unit_distance_mm: f64,
    pub sample_count: u32,
    pub samples: Vec<TransferSample>,
}
impl VrFusionState {
    pub fn enabled(&self) -> bool {
        self.overlay_generation != 0
    }
    fn validate_metadata(&self) -> Result<(), RenderError> {
        let fail = |message: &str| invalid_view_state(format!("VR fusion {message}"));
        if !self.enabled() {
            if self.overlay_dimensions != [0; 3]
                || self.patient_to_overlay_index != GeometryAffine::default()
                || self.domain_min != 0.0
                || self.domain_max != 0.0
                || self.opacity != 0.0
                || self.opacity_unit_distance_mm != 0.0
                || self.sample_count != 0
            {
                return Err(fail("metadata without overlay"));
            }
            return Ok(());
        }
        if self.overlay_dimensions.contains(&0) {
            return Err(fail("empty overlay"));
        }
        let a = &self.patient_to_overlay_index.0;
        if a.iter().any(|v| !finite(*v)) {
            return Err(fail("non-finite registration"));
        }
        if a[12] != 0.0 || a[13] != 0.0 || a[14] != 0.0 || a[15] != 1.0 {
            return Err(fail("non-affine registration"));
        }
        let det = a[0] * (a[5] * a[10] - a[6] * a[9]) - a[1] * (a[4] * a[10] - a[6] * a[8])
            + a[2] * (a[4] * a[9] - a[5] * a[8]);
        if !finite(det) || det == 0.0 {
            return Err(fail("singular registration"));
        }
        if !finite(self.opacity)
            || self.opacity <= 0.0
            || self.opacity > 1.0
            || !finite(self.opacity_unit_distance_mm)
            || self.opacity_unit_distance_mm <= 0.0
        {
            return Err(fail("invalid opacity or physical unit"));
        }
        if !finite(self.domain_min)
            || !finite(self.domain_max)
            || self.domain_max <= self.domain_min
            || self.sample_count < 2
            || self.sample_count as usize > MAX_TRANSFER_LUT_SAMPLES
        {
            return Err(fail("invalid transfer domain or table"));
        }
        Ok(())
    }
    pub fn payload_bytes(&self) -> Result<u64, RenderError> {
        self.validate_metadata()?;
        Ok(u64::from(self.sample_count) * 16)
    }
    pub fn validate(&self) -> Result<(), RenderError> {
        self.validate_metadata()?;
        if self.samples.len() != self.sample_count as usize {
            return Err(invalid_view_state("incomplete VR fusion transfer table"));
        }
        if self.samples.iter().any(|s| !transfer_sample_unit(s)) {
            return Err(invalid_view_state("invalid VR fusion transfer channel"));
        }
        Ok(())
    }
    pub(crate) fn json(&self, out: &mut String) {
        let mut o = JsonObject::new(out);
        o.int("OverlayGeneration", self.overlay_generation);
        o.uints("OverlayDimensions", &self.overlay_dimensions);
        o.floats("PatientToOverlayIndex", &self.patient_to_overlay_index.0);
        o.f64("DomainMin", self.domain_min);
        o.f64("DomainMax", self.domain_max);
        o.f64("Opacity", self.opacity);
        o.f64("OpacityUnitDistanceMM", self.opacity_unit_distance_mm);
        o.int("SampleCount", self.sample_count);
        o.finish();
    }
}
pub const MAX_DEFORMATION_FIELD_NODES: u64 = 2_097_152;
pub const MAX_DEFORMATION_INSTANCES: u32 = 32_768;
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DeformationFieldDescriptor {
    pub generation: u64,
    pub registered_frame_uid: String,
    pub source_frame_uid: String,
    pub dimensions: [u32; 3],
    pub grid_to_patient_lps: GeometryAffine,
    pub pre: GeometryAffine,
    pub post: GeometryAffine,
}
impl DeformationFieldDescriptor {
    pub fn payload_bytes(&self) -> Result<u64, RenderError> {
        let mut n: u64 = 1;
        for size in self.dimensions {
            if !(1..=2048).contains(&size) {
                return Err(RenderError::other(
                    "deformation grid dimensions outside bounds",
                ));
            }
            n *= u64::from(size);
        }
        if n > MAX_DEFORMATION_FIELD_NODES {
            return Err(RenderError::other(format!(
                "deformation grid exceeds {MAX_DEFORMATION_FIELD_NODES} nodes"
            )));
        }
        Ok(n * 16)
    }
    pub fn validate(&self) -> Result<(), RenderError> {
        if self.generation == 0 {
            return Err(RenderError::other("deformation field generation required"));
        }
        for id in [&self.registered_frame_uid, &self.source_frame_uid] {
            if id.trim_matches(is_unicode_space).is_empty() || id.len() > 1024 {
                return Err(RenderError::other(
                    "deformation frame identity required and bounded",
                ));
            }
        }
        self.payload_bytes()?;
        for matrix in [&self.grid_to_patient_lps, &self.pre, &self.post] {
            if !matrix.finite() {
                return Err(RenderError::other("deformation affine must be finite"));
            }
            let m = &matrix.0;
            if m.iter().any(|v| v.abs() > 1e6) {
                return Err(RenderError::other(
                    "deformation affine exceeds coordinate bounds",
                ));
            }
            if m[12] != 0.0 || m[13] != 0.0 || m[14] != 0.0 || m[15] != 1.0 {
                return Err(RenderError::other("deformation matrix must be affine"));
            }
            let det = m[0] * (m[5] * m[10] - m[6] * m[9]) - m[1] * (m[4] * m[10] - m[6] * m[8])
                + m[2] * (m[4] * m[9] - m[5] * m[8]);
            if det.abs() < 1e-12 || det.abs() > 1e18 {
                return Err(RenderError::other(
                    "deformation affine determinant outside bounds",
                ));
            }
        }
        Ok(())
    }
    pub fn validate_payload(&self, data: &[u8]) -> Result<(), RenderError> {
        self.validate()?;
        let n = self.payload_bytes()?;
        if data.len() as u64 != n {
            return Err(RenderError::other("deformation payload length mismatch"));
        }
        let word = |o: usize| {
            f32::from_bits(u32::from_le_bytes([
                data[o],
                data[o + 1],
                data[o + 2],
                data[o + 3],
            ]))
        };
        for i in (0..data.len()).step_by(16) {
            let valid = word(i + 12);
            if valid != 0.0 && valid != 1.0 {
                return Err(RenderError::other("invalid deformation node validity"));
            }
            for j in (0..12).step_by(4) {
                let v = word(i + j);
                let f = f64::from(v);
                if !finite(f)
                    || f.abs() > 1e6
                    || (v != 0.0 && f.abs() < f64::from(f32::MIN_POSITIVE))
                    || (valid == 0.0 && v != 0.0)
                {
                    return Err(RenderError::other(format!(
                        "invalid deformation node at {}",
                        i / 16
                    )));
                }
            }
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct DeformationInspection {
    pub min: [u32; 3],
    pub max: [u32; 3],
    pub stride: u32,
    pub maximum_instances: u32,
    pub scale: f64,
    pub maximum_length_mm: f64,
    pub sampling_offset: [f64; 3],
    pub show_arrows: bool,
    pub show_grid: bool,
    pub show_reference: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DeformationInspectionPlan {
    pub stride: u32,
    pub counts: [u32; 3],
    pub instances: u32,
}
fn deformation_finite_positive(v: f64, maximum: f64) -> bool {
    v > 0.0 && v <= maximum && finite(v)
}
impl DeformationInspection {
    pub fn plan(
        &self,
        d: &DeformationFieldDescriptor,
    ) -> Result<DeformationInspectionPlan, RenderError> {
        d.validate()?;
        if self.stride < 1
            || self.stride > 2048
            || self.maximum_instances < 1
            || self.maximum_instances > MAX_DEFORMATION_INSTANCES
            || !deformation_finite_positive(self.scale, 100.0)
            || self.scale < 1e-6
            || !deformation_finite_positive(self.maximum_length_mm, 1e4)
            || self.maximum_length_mm < 1e-4
            || !(self.show_arrows || self.show_grid || self.show_reference)
        {
            return Err(RenderError::other("deformation display limits invalid"));
        }
        for a in 0..3 {
            let o = self.sampling_offset[a];
            if self.min[a] >= self.max[a]
                || self.max[a] > d.dimensions[a]
                || o.is_nan()
                || o < 0.0
                || o >= 1.0
            {
                return Err(RenderError::other(
                    "deformation region or sample offset invalid",
                ));
            }
        }
        for stride in self.stride..=2048 {
            let mut p = DeformationInspectionPlan {
                stride,
                ..Default::default()
            };
            let mut n: u64 = 1;
            for a in 0..3 {
                p.counts[a] = (self.max[a] - self.min[a]).div_ceil(stride);
                n *= u64::from(p.counts[a]);
            }
            if n <= u64::from(self.maximum_instances) {
                p.instances = n as u32;
                return Ok(p);
            }
        }
        Err(RenderError::other(
            "deformation instance budget cannot be met",
        ))
    }
}
#[cfg(test)]
mod tests {
    use super::super::view_state::ERR_INVALID_VIEW_STATE;
    use super::*;
    #[test]
    fn float32_like_go() {
        assert_eq!(f32_text(0.1), "0.1");
        assert_eq!(f32_text(1e-5), "1e-05");
        assert_eq!(f32_text(1234567.0), "1.234567e+06");
        assert_eq!(f32_text(-0.5), "-0.5");
        let mut s = String::new();
        json_f32(&mut s, 1e-7);
        assert_eq!(s, "1e-7");
        s.clear();
        json_f32(&mut s, 123456.0);
        assert_eq!(s, "123456");
    }
    #[test]
    fn error_prefix() {
        assert!(
            invalid_view_state("x")
                .to_string()
                .starts_with(ERR_INVALID_VIEW_STATE)
        );
    }
}
