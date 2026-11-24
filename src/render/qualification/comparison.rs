use super::{
    Error, Generation, OPERATION_MPR, OPERATION_VR, Result, blank, json_float, json_string,
    write_text,
};
use crate::textfmt::quote;
use std::io::Write;
pub const COMPARISON_V1_SCHEMA: &str = "comparison-v1";
pub const COMPARISON_PASS: &str = "pass";
pub const COMPARISON_FAIL: &str = "fail";
pub const COMPARISON_TOLERANCE_FROZEN_V1: &str = "frozen-contract-v1";
pub const COMPARISON_TOLERANCE_SELF_CALIBRATED: &str = "self-calibrated";
pub const COMPARISON_TOLERANCE_MPR_F32_V1: &str = "mpr-f32-v1";
pub const COMPARISON_TOLERANCE_VR_RGBA8_V1: &str = "vr-rgba8-v1";
pub const COMPARISON_TOLERANCE_MPR_F32_EXACT_V1: &str = "mpr-f32-exact-v1";
pub const COMPARISON_TOLERANCE_VR_RGBA8_EXACT_V1: &str = "vr-rgba8-exact-v1";
pub const GEOMETRY_BASIS_CANONICAL_REQUEST_LANDMARKS_V1: &str =
    "canonical-request-patient-lps-landmarks-v1";
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ComparisonTolerancesV1 {
    pub origin: String,
    pub profile: String,
    pub max_landmark_error_pixels: f64,
    pub max_landmark_error_min_spacing_fraction: f64,
    pub frame_abs_component_tolerance: f64,
    pub max_frame_components_over_tolerance_fraction: f64,
    pub require_exact_geometry_sha256: bool,
    pub require_exact_frame_sha256: bool,
}
pub fn frozen_comparison_tolerances_v1(operation: &str) -> Result<ComparisonTolerancesV1> {
    let mut common = ComparisonTolerancesV1 {
        origin: COMPARISON_TOLERANCE_FROZEN_V1.to_string(),
        max_landmark_error_pixels: 0.5,
        max_landmark_error_min_spacing_fraction: 0.5,
        ..Default::default()
    };
    if operation == OPERATION_MPR {
        common.profile = COMPARISON_TOLERANCE_MPR_F32_V1.to_string();
        common.frame_abs_component_tolerance = 1e-5;
        common.max_frame_components_over_tolerance_fraction = 0.0;
    } else if operation == OPERATION_VR {
        common.profile = COMPARISON_TOLERANCE_VR_RGBA8_V1.to_string();
        common.frame_abs_component_tolerance = 32.0;
        common.max_frame_components_over_tolerance_fraction = 0.002;
    } else {
        return Err(Error::comparison(format!(
            "operation {} has no frozen comparison profile",
            quote(operation)
        )));
    }
    Ok(common)
}
pub fn frozen_exact_comparison_tolerances_v1(operation: &str) -> Result<ComparisonTolerancesV1> {
    let mut tolerances = frozen_comparison_tolerances_v1(operation)?;
    if operation == OPERATION_MPR {
        tolerances.profile = COMPARISON_TOLERANCE_MPR_F32_EXACT_V1.to_string();
    } else if operation == OPERATION_VR {
        tolerances.profile = COMPARISON_TOLERANCE_VR_RGBA8_EXACT_V1.to_string();
    }
    tolerances.require_exact_geometry_sha256 = true;
    tolerances.require_exact_frame_sha256 = true;
    Ok(tolerances)
}
#[derive(Debug, Clone, PartialEq, Default)]
pub struct GeometryComparisonV1 {
    pub basis: String,
    pub reference_sha256: String,
    pub candidate_sha256: String,
    pub hashes_match: bool,
    pub landmark_count: u64,
    pub minimum_source_spacing_mm: f64,
    pub max_landmark_error_pixels: f64,
    pub max_landmark_error_mm: f64,
}
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FrameComparisonV1 {
    pub reference_sha256: String,
    pub candidate_sha256: String,
    pub hashes_match: bool,
    pub compared_components: u64,
    pub components_over_tolerance: u64,
    pub max_abs_component_difference: f64,
}
impl FrameComparisonV1 {
    pub fn fraction_over_tolerance(&self) -> f64 {
        if self.compared_components == 0 {
            return f64::NAN;
        }
        self.components_over_tolerance as f64 / self.compared_components as f64
    }
}
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ComparisonV1 {
    pub schema: String,
    pub run_id: String,
    pub sequence: String,
    pub step_id: String,
    pub sequence_no: u64,
    pub operation: String,
    pub reference_backend: String,
    pub candidate_backend: String,
    pub generation: Generation,
    pub geometry: GeometryComparisonV1,
    pub frame: FrameComparisonV1,
    pub tolerances: ComparisonTolerancesV1,
    pub outcome: String,
}
impl ComparisonV1 {
    pub fn expected_outcome(&self) -> Result<&'static str> {
        self.validate_inputs()?;
        let t = &self.tolerances;
        let geometry_pass = (!t.require_exact_geometry_sha256 || self.geometry.hashes_match)
            && self.geometry.max_landmark_error_pixels <= t.max_landmark_error_pixels
            && self.geometry.max_landmark_error_mm
                <= t.max_landmark_error_min_spacing_fraction
                    * self.geometry.minimum_source_spacing_mm;
        let frame_pass = (!t.require_exact_frame_sha256 || self.frame.hashes_match)
            && self.frame.fraction_over_tolerance()
                <= t.max_frame_components_over_tolerance_fraction;
        Ok(if geometry_pass && frame_pass {
            COMPARISON_PASS
        } else {
            COMPARISON_FAIL
        })
    }
    pub fn validate(&self) -> Result<()> {
        let expected = self.expected_outcome()?;
        if self.outcome != expected {
            return Err(Error::comparison(format!(
                "outcome {} does not match derived outcome {}",
                quote(&self.outcome),
                quote(expected)
            )));
        }
        Ok(())
    }
    pub fn require_pass(&self) -> Result<()> {
        self.validate()?;
        if self.outcome != COMPARISON_PASS {
            return Err(Error::comparison(format!(
                "{}/{} comparison failed frozen tolerances",
                self.sequence, self.step_id
            )));
        }
        Ok(())
    }
    fn validate_inputs(&self) -> Result<()> {
        if self.schema != COMPARISON_V1_SCHEMA {
            return Err(Error::comparison(format!("schema {}", quote(&self.schema))));
        }
        for (field, value) in [
            ("run_id", &self.run_id),
            ("sequence", &self.sequence),
            ("step_id", &self.step_id),
            ("reference_backend", &self.reference_backend),
            ("candidate_backend", &self.candidate_backend),
        ] {
            if blank(value) {
                return Err(Error::comparison(format!("empty {field}")));
            }
        }
        if self.reference_backend == self.candidate_backend {
            return Err(Error::comparison(
                "reference and candidate backend are identical",
            ));
        }
        if self.sequence_no == 0 {
            return Err(Error::comparison("zero sequence number"));
        }
        if self.generation.volume == 0
            || self.generation.view == 0
            || self.generation.presentation == 0
        {
            return Err(Error::comparison("incomplete generation tuple"));
        }
        let frozen = frozen_comparison_tolerances_v1(&self.operation)?;
        if self.tolerances.origin == COMPARISON_TOLERANCE_SELF_CALIBRATED {
            return Err(Error::comparison("self-calibrated tolerance is forbidden"));
        }
        let exact = frozen_exact_comparison_tolerances_v1(&self.operation)?;
        if self.tolerances != frozen && self.tolerances != exact {
            return Err(Error::comparison(format!(
                "tolerance profile {} is not the frozen {} profile",
                quote(&self.tolerances.profile),
                quote(&frozen.profile)
            )));
        }
        validate_geometry(&self.geometry)?;
        validate_frame(&self.frame, self.tolerances.frame_abs_component_tolerance)
    }
    fn write_json(&self, out: &mut String) {
        let field = |out: &mut String, name: &str, value: &str| {
            out.push_str(name);
            json_string(out, value);
        };
        let number = |out: &mut String, name: &str, value: f64| {
            out.push_str(name);
            json_float(out, value);
        };
        field(out, "{\"schema\":", &self.schema);
        field(out, ",\"run_id\":", &self.run_id);
        field(out, ",\"sequence\":", &self.sequence);
        field(out, ",\"step_id\":", &self.step_id);
        out.push_str(&format!(",\"sequence_no\":{}", self.sequence_no));
        field(out, ",\"operation\":", &self.operation);
        field(out, ",\"reference_backend\":", &self.reference_backend);
        field(out, ",\"candidate_backend\":", &self.candidate_backend);
        out.push_str(",\"generation\":");
        self.generation.write_json(out);
        let g = &self.geometry;
        field(out, ",\"geometry\":{\"basis\":", &g.basis);
        field(out, ",\"reference_sha256\":", &g.reference_sha256);
        field(out, ",\"candidate_sha256\":", &g.candidate_sha256);
        out.push_str(&format!(
            ",\"hashes_match\":{},\"landmark_count\":{}",
            g.hashes_match, g.landmark_count
        ));
        number(
            out,
            ",\"minimum_source_spacing_mm\":",
            g.minimum_source_spacing_mm,
        );
        number(
            out,
            ",\"max_landmark_error_pixels\":",
            g.max_landmark_error_pixels,
        );
        number(out, ",\"max_landmark_error_mm\":", g.max_landmark_error_mm);
        let f = &self.frame;
        field(
            out,
            "},\"frame\":{\"reference_sha256\":",
            &f.reference_sha256,
        );
        field(out, ",\"candidate_sha256\":", &f.candidate_sha256);
        out.push_str(&format!(
            ",\"hashes_match\":{},\"compared_components\":{},\"components_over_tolerance\":{}",
            f.hashes_match, f.compared_components, f.components_over_tolerance
        ));
        number(
            out,
            ",\"max_abs_component_difference\":",
            f.max_abs_component_difference,
        );
        let t = &self.tolerances;
        field(out, "},\"tolerances\":{\"origin\":", &t.origin);
        field(out, ",\"profile\":", &t.profile);
        number(
            out,
            ",\"max_landmark_error_pixels\":",
            t.max_landmark_error_pixels,
        );
        number(
            out,
            ",\"max_landmark_error_min_spacing_fraction\":",
            t.max_landmark_error_min_spacing_fraction,
        );
        number(
            out,
            ",\"frame_abs_component_tolerance\":",
            t.frame_abs_component_tolerance,
        );
        number(
            out,
            ",\"max_frame_components_over_tolerance_fraction\":",
            t.max_frame_components_over_tolerance_fraction,
        );
        out.push_str(&format!(
            ",\"require_exact_geometry_sha256\":{},\"require_exact_frame_sha256\":{}",
            t.require_exact_geometry_sha256, t.require_exact_frame_sha256
        ));
        field(out, "},\"outcome\":", &self.outcome);
        out.push_str("}\n");
    }
}
fn validate_geometry(geometry: &GeometryComparisonV1) -> Result<()> {
    if geometry.basis != GEOMETRY_BASIS_CANONICAL_REQUEST_LANDMARKS_V1 {
        return Err(Error::comparison(format!(
            "unsupported geometry basis {}",
            quote(&geometry.basis)
        )));
    }
    validate_sha256("reference geometry", &geometry.reference_sha256)?;
    validate_sha256("candidate geometry", &geometry.candidate_sha256)?;
    if geometry.hashes_match != (geometry.reference_sha256 == geometry.candidate_sha256) {
        return Err(Error::comparison(
            "geometry hashes_match disagrees with hashes",
        ));
    }
    if geometry.landmark_count == 0 {
        return Err(Error::comparison("zero geometry landmark count"));
    }
    if !positive_finite(geometry.minimum_source_spacing_mm)
        || !non_negative_finite(geometry.max_landmark_error_pixels)
        || !non_negative_finite(geometry.max_landmark_error_mm)
    {
        return Err(Error::comparison(
            "invalid geometry spacing or landmark error",
        ));
    }
    Ok(())
}
fn validate_frame(frame: &FrameComparisonV1, abs_tolerance: f64) -> Result<()> {
    validate_sha256("reference frame", &frame.reference_sha256)?;
    validate_sha256("candidate frame", &frame.candidate_sha256)?;
    if frame.hashes_match != (frame.reference_sha256 == frame.candidate_sha256) {
        return Err(Error::comparison(
            "frame hashes_match disagrees with hashes",
        ));
    }
    if frame.compared_components == 0 || frame.components_over_tolerance > frame.compared_components
    {
        return Err(Error::comparison(
            "invalid compared/over-tolerance component counts",
        ));
    }
    if !non_negative_finite(frame.max_abs_component_difference) {
        return Err(Error::comparison(
            "invalid maximum frame component difference",
        ));
    }
    if frame.max_abs_component_difference <= abs_tolerance && frame.components_over_tolerance != 0 {
        return Err(Error::comparison(
            "components exceed tolerance but maximum difference does not",
        ));
    }
    if frame.max_abs_component_difference > abs_tolerance && frame.components_over_tolerance == 0 {
        return Err(Error::comparison(
            "maximum difference exceeds tolerance without an affected component",
        ));
    }
    Ok(())
}
fn validate_sha256(field: &str, value: &str) -> Result<()> {
    if value.len() != 64 || lower_text(value) != value {
        return Err(Error::comparison(format!(
            "{field} is not lowercase SHA-256"
        )));
    }
    if let Some(&byte) = value.as_bytes().iter().find(|b| !b.is_ascii_hexdigit()) {
        let rune = char::from(byte);
        let mut text = format!("U+{:04X}", u32::from(byte));
        if is_print_latin1(rune) {
            text.push_str(&format!(" '{rune}'"));
        }
        return Err(Error::comparison(format!(
            "{field} is not lowercase SHA-256: encoding/hex: invalid byte: {text}"
        )));
    }
    Ok(())
}
fn is_print_latin1(c: char) -> bool {
    let v = c as u32;
    (0x20..0x7f).contains(&v) || (0xa1..=0xff).contains(&v) && v != 0xad
}
fn lower_text(value: &str) -> String {
    if value.is_ascii() {
        return value.to_ascii_lowercase();
    }
    value.chars().flat_map(char::to_lowercase).collect()
}
fn positive_finite(value: f64) -> bool {
    value > 0.0 && value.is_finite()
}
fn non_negative_finite(value: f64) -> bool {
    value >= 0.0 && value.is_finite()
}
pub(crate) fn comparison_jsonl(comparisons: &[ComparisonV1]) -> Result<String> {
    if comparisons.is_empty() {
        return Err(Error::comparison("empty comparison stream"));
    }
    let mut previous = 0u64;
    let mut buffer = String::new();
    let first = &comparisons[0];
    for (index, comparison) in comparisons.iter().enumerate() {
        comparison
            .validate()
            .map_err(|e| e.prefixed(format!("comparison record {index}")))?;
        if comparison.sequence_no <= previous {
            return Err(Error::comparison(format!(
                "sequence_no {} is duplicate or out of order after {previous}",
                comparison.sequence_no
            )));
        }
        if comparison.run_id != first.run_id
            || comparison.reference_backend != first.reference_backend
            || comparison.candidate_backend != first.candidate_backend
        {
            return Err(Error::comparison(format!(
                "comparison stream identity changed at sequence_no {}",
                comparison.sequence_no
            )));
        }
        comparison.write_json(&mut buffer);
        previous = comparison.sequence_no;
    }
    Ok(buffer)
}
pub fn write_comparison_jsonl(dst: &mut dyn Write, comparisons: &[ComparisonV1]) -> Result<()> {
    let text = comparison_jsonl(comparisons)?;
    write_text(dst, &text)
}
