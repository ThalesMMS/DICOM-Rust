#![forbid(unsafe_code)]
use super::affine::{GeometryAffine, finite_positive_spacing, geometry_affine_pair};
use super::stablesort::slice_stable;
use super::vec3::Vec3;
use crate::fmath;
use std::f64::consts::PI;
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SliceGeometry {
    pub origin: Vec3,
    pub row_dir: Vec3,
    pub col_dir: Vec3,
    pub normal: Vec3,
    pub row_spacing: f64,
    pub col_spacing: f64,
    pub rows: i64,
    pub columns: i64,
}
impl SliceGeometry {
    pub fn position_along(&self, normal: Vec3) -> f64 {
        self.origin.dot(normal)
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum VolumeClassification {
    #[default]
    Regular,
    SingleSlice,
    IrregularSpacing,
    MissingSlices,
    GantryTilted,
    UnstableOrientation,
    DuplicatePositions,
    MixedOrientation,
    InconsistentNormals,
    TemporalInterleaved,
}
impl VolumeClassification {
    pub fn code(self) -> i64 {
        self as i64
    }
    pub fn as_str(self) -> &'static str {
        match self {
            VolumeClassification::Regular => "regular",
            VolumeClassification::SingleSlice => "single-slice",
            VolumeClassification::IrregularSpacing => "irregular-spacing",
            VolumeClassification::MissingSlices => "missing-slices",
            VolumeClassification::GantryTilted => "gantry-tilted",
            VolumeClassification::UnstableOrientation => "unstable-orientation",
            VolumeClassification::DuplicatePositions => "duplicate-positions",
            VolumeClassification::MixedOrientation => "mixed-orientation",
            VolumeClassification::InconsistentNormals => "inconsistent-normals",
            VolumeClassification::TemporalInterleaved => "temporal-interleaving",
        }
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum GeometryDisposition {
    #[default]
    Unsupported,
    RegularFastPath,
    Regularizable,
}
impl GeometryDisposition {
    pub fn code(self) -> i64 {
        self as i64
    }
    pub fn as_str(self) -> &'static str {
        match self {
            GeometryDisposition::RegularFastPath => "regular-fast-path",
            GeometryDisposition::Regularizable => "regularizable",
            GeometryDisposition::Unsupported => "unsupported",
        }
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum GeometryIssue {
    #[default]
    None,
    SingleSlice,
    ReversedStack,
    DuplicatePositions,
    SliceGaps,
    IrregularSpacing,
    GantryTilt,
    InconsistentGantryShear,
    MixedOrientation,
    InconsistentNormals,
    InconsistentPixelSpacing,
    TemporalInterleaving,
    DifferentPixelGrid,
    InvalidGeometry,
    MixedPositionSource,
    MissingPixelSpacing,
    NonOrthonormalOrientation,
}
impl GeometryIssue {
    pub fn as_str(self) -> &'static str {
        match self {
            GeometryIssue::None => "",
            GeometryIssue::SingleSlice => "single-slice",
            GeometryIssue::ReversedStack => "reversed-stack",
            GeometryIssue::DuplicatePositions => "duplicate-positions",
            GeometryIssue::SliceGaps => "slice-gaps",
            GeometryIssue::IrregularSpacing => "irregular-spacing",
            GeometryIssue::GantryTilt => "gantry-tilt",
            GeometryIssue::InconsistentGantryShear => "inconsistent-gantry-shear",
            GeometryIssue::MixedOrientation => "mixed-orientation",
            GeometryIssue::InconsistentNormals => "inconsistent-normals",
            GeometryIssue::InconsistentPixelSpacing => "inconsistent-pixel-spacing",
            GeometryIssue::TemporalInterleaving => "temporal-interleaving",
            GeometryIssue::DifferentPixelGrid => "different-pixel-grid",
            GeometryIssue::InvalidGeometry => "invalid-geometry",
            GeometryIssue::MixedPositionSource => "mixed-position-source",
            GeometryIssue::MissingPixelSpacing => "missing-pixel-spacing",
            GeometryIssue::NonOrthonormalOrientation => "non-orthonormal-orientation",
        }
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct GeometryTolerances {
    pub spacing_rel: f64,
    pub spacing_abs: f64,
    pub orientation_cos: f64,
    pub tilt_cos: f64,
    pub position_abs: f64,
    pub shear_abs: f64,
    pub affine_round_trip: f64,
}
impl GeometryTolerances {
    pub fn default_tolerances() -> GeometryTolerances {
        GeometryTolerances {
            spacing_rel: 0.05,
            spacing_abs: 0.01,
            orientation_cos: fmath::cos(PI / 180.0),
            tilt_cos: fmath::cos(PI / 180.0),
            position_abs: 1e-6,
            shear_abs: fmath::tan(PI / 180.0),
            affine_round_trip: 1e-9,
        }
    }
    pub(crate) fn normalized(mut self) -> GeometryTolerances {
        let d = GeometryTolerances::default_tolerances();
        if !finite_positive_spacing(self.spacing_rel) {
            self.spacing_rel = d.spacing_rel;
        }
        if !finite_positive_spacing(self.spacing_abs) {
            self.spacing_abs = d.spacing_abs;
        }
        if self.orientation_cos <= 0.0
            || self.orientation_cos > 1.0
            || self.orientation_cos.is_nan()
        {
            self.orientation_cos = d.orientation_cos;
        }
        if self.tilt_cos <= 0.0 || self.tilt_cos > 1.0 || self.tilt_cos.is_nan() {
            self.tilt_cos = d.tilt_cos;
        }
        if !finite_positive_spacing(self.position_abs) {
            self.position_abs = d.position_abs;
        }
        if !finite_positive_spacing(self.shear_abs) {
            self.shear_abs = d.shear_abs;
        }
        if !finite_positive_spacing(self.affine_round_trip) {
            self.affine_round_trip = d.affine_round_trip;
        }
        self
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct GeometryMetrics {
    pub slice_count: i64,
    pub minimum_spacing_mm: f64,
    pub maximum_spacing_mm: f64,
    pub mean_spacing_mm: f64,
    pub maximum_spacing_deviation_mm: f64,
    pub maximum_orientation_angle_deg: f64,
    pub maximum_normal_angle_deg: f64,
    pub maximum_affine_residual_mm: f64,
    pub maximum_shear_deviation: f64,
    pub maximum_orientation_deviation: f64,
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct VolumeGeometry {
    pub slices: Option<Vec<SliceGeometry>>,
    pub row_dir: Vec3,
    pub col_dir: Vec3,
    pub normal: Vec3,
    pub positions: Option<Vec<f64>>,
    pub spacings: Option<Vec<f64>>,
    pub mean_spacing: f64,
    pub regular: bool,
    pub orientation_stable: bool,
    pub gantry_tilted: bool,
    pub gantry_tilt_offset: Vec3,
    pub gantry_tilt_shear: Vec3,
    pub gantry_tilt_angle_degrees: f64,
    pub missing_slices: bool,
    pub classification: VolumeClassification,
    pub disposition: GeometryDisposition,
    pub primary_issue: GeometryIssue,
    pub issues: Option<Vec<GeometryIssue>>,
    pub reversed_input: bool,
    pub input_reordered: bool,
    pub duplicate_positions: bool,
    pub temporal_interleaved: bool,
    pub requires_resampling: bool,
    pub source_affine: bool,
    pub regularized_depth: i64,
    pub voxel_to_patient_affine: GeometryAffine,
    pub patient_to_voxel_affine: GeometryAffine,
    pub metrics: GeometryMetrics,
}
impl VolumeGeometry {
    pub fn slices(&self) -> &[SliceGeometry] {
        self.slices.as_deref().unwrap_or(&[])
    }
    pub fn positions(&self) -> &[f64] {
        self.positions.as_deref().unwrap_or(&[])
    }
    pub fn spacings(&self) -> &[f64] {
        self.spacings.as_deref().unwrap_or(&[])
    }
    pub fn issues(&self) -> &[GeometryIssue] {
        self.issues.as_deref().unwrap_or(&[])
    }
    pub(crate) fn has_issue(&self, issue: GeometryIssue) -> bool {
        self.issues().contains(&issue)
    }
    pub(crate) fn add_issue(&mut self, issue: GeometryIssue) {
        if issue == GeometryIssue::None || self.has_issue(issue) {
            return;
        }
        self.issues.get_or_insert_with(Vec::new).push(issue);
        if self.primary_issue == GeometryIssue::None {
            self.primary_issue = issue;
        }
    }
    pub(crate) fn finalize_disposition(&mut self) {
        use GeometryIssue as I;
        let unsupported = self.issues().iter().any(|issue| {
            matches!(
                issue,
                I::DuplicatePositions
                    | I::InconsistentGantryShear
                    | I::MixedOrientation
                    | I::InconsistentNormals
                    | I::InconsistentPixelSpacing
                    | I::TemporalInterleaving
                    | I::DifferentPixelGrid
                    | I::InvalidGeometry
                    | I::MixedPositionSource
                    | I::NonOrthonormalOrientation
            )
        });
        self.disposition = if unsupported {
            GeometryDisposition::Unsupported
        } else if self.requires_resampling || self.reversed_input {
            GeometryDisposition::Regularizable
        } else {
            GeometryDisposition::RegularFastPath
        };
        const PRIORITY: [GeometryIssue; 15] = [
            I::TemporalInterleaving,
            I::DuplicatePositions,
            I::InconsistentGantryShear,
            I::InconsistentNormals,
            I::MixedOrientation,
            I::InconsistentPixelSpacing,
            I::DifferentPixelGrid,
            I::MixedPositionSource,
            I::NonOrthonormalOrientation,
            I::InvalidGeometry,
            I::SingleSlice,
            I::SliceGaps,
            I::IrregularSpacing,
            I::GantryTilt,
            I::ReversedStack,
        ];
        self.primary_issue = PRIORITY
            .iter()
            .copied()
            .find(|issue| self.has_issue(*issue))
            .unwrap_or(I::None);
    }
    fn build_affines(
        &mut self,
        origin: Vec3,
        normal: Vec3,
        col_spacing: f64,
        row_spacing: f64,
        slice_spacing: f64,
    ) {
        let (forward, inverse, _) = geometry_affine_pair(
            origin,
            self.row_dir.scale(col_spacing),
            self.col_dir.scale(row_spacing),
            normal.scale(slice_spacing),
        );
        self.voxel_to_patient_affine = forward;
        self.patient_to_voxel_affine = inverse;
    }
    fn affines_conform(&self, tol: &GeometryTolerances) -> bool {
        if !self.voxel_to_patient_affine.finite()
            || !self.patient_to_voxel_affine.finite()
            || self.slices().is_empty()
        {
            return false;
        }
        let columns = self.slices()[0].columns;
        let rows = self.slices()[0].rows;
        let depth = self.regularized_depth;
        if columns <= 0 || rows <= 0 || depth <= 0 {
            return false;
        }
        let points = [
            Vec3::ZERO,
            Vec3::new((columns - 1) as f64, 0.0, 0.0),
            Vec3::new(0.0, (rows - 1) as f64, 0.0),
            Vec3::new(0.0, 0.0, (depth - 1) as f64),
            Vec3::new(
                (columns - 1) as f64 / 2.0,
                (rows - 1) as f64 / 2.0,
                (depth - 1) as f64 / 2.0,
            ),
        ];
        points.iter().all(|point| {
            let round_trip = self
                .patient_to_voxel_affine
                .transform_point(self.voxel_to_patient_affine.transform_point(*point));
            (round_trip.x - point.x).abs() <= tol.affine_round_trip
                && (round_trip.y - point.y).abs() <= tol.affine_round_trip
                && (round_trip.z - point.z).abs() <= tol.affine_round_trip
        })
    }
}
pub fn build_volume_geometry(slices: &[SliceGeometry], tol: GeometryTolerances) -> VolumeGeometry {
    let tol = tol.normalized();
    let mut out = VolumeGeometry {
        orientation_stable: true,
        regular: true,
        disposition: GeometryDisposition::RegularFastPath,
        ..Default::default()
    };
    if slices.is_empty() {
        out.classification = VolumeClassification::SingleSlice;
        out.disposition = GeometryDisposition::Unsupported;
        out.primary_issue = GeometryIssue::InvalidGeometry;
        out.issues = Some(vec![GeometryIssue::InvalidGeometry]);
        return out;
    }
    let reference = slices[0];
    out.row_dir = reference.row_dir;
    out.col_dir = reference.col_dir;
    out.normal = reference.normal;
    out.metrics.slice_count = slices.len() as i64;
    let mut indexed: Vec<(usize, SliceGeometry, f64)> = slices
        .iter()
        .enumerate()
        .map(|(i, g)| (i, *g, g.position_along(reference.normal)))
        .collect();
    let input_positions: Vec<f64> = indexed.iter().map(|item| item.2).collect();
    slice_stable(&mut indexed, |a, b| a.2 < b.2);
    let sorted: Vec<SliceGeometry> = indexed.iter().map(|item| item.1).collect();
    out.input_reordered = indexed.iter().enumerate().any(|(i, item)| item.0 != i);
    out.slices = Some(sorted.clone());
    out.reversed_input = reversed_geometry_input(&input_positions, tol.position_abs);
    if out.reversed_input {
        out.add_issue(GeometryIssue::ReversedStack);
    }
    out.positions = Some(
        sorted
            .iter()
            .map(|g| g.position_along(reference.normal))
            .collect(),
    );
    let mut mixed_orientation = false;
    let mut inconsistent_normals = false;
    let mut inconsistent_pixel_spacing = false;
    for g in &sorted {
        let row_angle = vector_angle_degrees(g.row_dir, reference.row_dir);
        let col_angle = vector_angle_degrees(g.col_dir, reference.col_dir);
        let normal_angle = vector_angle_degrees(g.normal, reference.normal);
        out.metrics.maximum_orientation_angle_deg = fmath::max(
            out.metrics.maximum_orientation_angle_deg,
            fmath::max(row_angle, col_angle),
        );
        out.metrics.maximum_normal_angle_deg =
            fmath::max(out.metrics.maximum_normal_angle_deg, normal_angle);
        if g.normal.dot(reference.normal) < tol.orientation_cos {
            inconsistent_normals = true;
        } else if g.row_dir.dot(reference.row_dir) < tol.orientation_cos
            || g.col_dir.dot(reference.col_dir) < tol.orientation_cos
        {
            mixed_orientation = true;
        }
        if !spacing_within_tolerance(g.row_spacing, reference.row_spacing, &tol)
            || !spacing_within_tolerance(g.col_spacing, reference.col_spacing, &tol)
        {
            inconsistent_pixel_spacing = true;
        }
    }
    out.orientation_stable = !mixed_orientation && !inconsistent_normals;
    if inconsistent_normals {
        out.add_issue(GeometryIssue::InconsistentNormals);
    }
    if mixed_orientation {
        out.add_issue(GeometryIssue::MixedOrientation);
    }
    if inconsistent_pixel_spacing {
        out.add_issue(GeometryIssue::InconsistentPixelSpacing);
    }
    if sorted.len() < 2 {
        out.classification = VolumeClassification::SingleSlice;
        out.regularized_depth = 1;
        out.source_affine = true;
        out.add_issue(GeometryIssue::SingleSlice);
        out.build_affines(
            reference.origin,
            reference.normal,
            reference.col_spacing,
            reference.row_spacing,
            1.0,
        );
        if !out.affines_conform(&tol) {
            out.add_issue(GeometryIssue::InvalidGeometry);
        }
        out.finalize_disposition();
        return out;
    }
    let positions = out.positions().to_vec();
    let mut spacings = Vec::with_capacity(sorted.len() - 1);
    let mut spacing_sum = 0.0;
    for i in 1..sorted.len() {
        let s = positions[i] - positions[i - 1];
        spacings.push(s);
        spacing_sum += s;
        if s <= tol.position_abs {
            out.duplicate_positions = true;
        }
    }
    out.mean_spacing = spacing_sum / spacings.len() as f64;
    out.metrics.mean_spacing_mm = out.mean_spacing;
    let (minimum, maximum) = spacing_range(&spacings);
    out.metrics.minimum_spacing_mm = minimum;
    out.metrics.maximum_spacing_mm = maximum;
    for spacing in &spacings {
        out.metrics.maximum_spacing_deviation_mm = fmath::max(
            out.metrics.maximum_spacing_deviation_mm,
            (spacing - out.mean_spacing).abs(),
        );
    }
    if out.duplicate_positions {
        out.add_issue(GeometryIssue::DuplicatePositions);
    }
    let (regular, missing) = classify_spacing(&spacings, &tol);
    out.regular = regular;
    out.missing_slices = missing;
    out.gantry_tilted = detect_gantry_tilt(&sorted, reference.normal, &tol);
    let (offset, shear, angle) = gantry_tilt_metrics(&sorted, reference.normal);
    out.gantry_tilt_offset = offset;
    out.gantry_tilt_shear = shear;
    out.gantry_tilt_angle_degrees = angle;
    if out.gantry_tilted {
        out.metrics.maximum_shear_deviation =
            maximum_shear_deviation(&sorted, reference.normal, out.gantry_tilt_shear);
    }
    if out.missing_slices {
        out.add_issue(GeometryIssue::SliceGaps);
    } else if !out.regular && !out.duplicate_positions {
        out.add_issue(GeometryIssue::IrregularSpacing);
    }
    if out.gantry_tilted {
        out.add_issue(GeometryIssue::GantryTilt);
        if out.metrics.maximum_shear_deviation > tol.shear_abs {
            out.add_issue(GeometryIssue::InconsistentGantryShear);
        }
    }
    let last = sorted.len() - 1;
    let z_step = sorted[last]
        .origin
        .sub(sorted[0].origin)
        .scale(1.0 / last as f64);
    out.metrics.maximum_affine_residual_mm =
        maximum_affine_residual(&sorted, sorted[0].origin, z_step);
    let affine_residual_limit = fmath::max(
        tol.position_abs,
        fmath::max(tol.spacing_abs, tol.spacing_rel * out.mean_spacing.abs()),
    );
    out.source_affine = out.orientation_stable
        && !inconsistent_pixel_spacing
        && !out.duplicate_positions
        && out.metrics.maximum_affine_residual_mm <= affine_residual_limit;
    let mut target_spacing = regularized_slice_spacing(&spacings, out.missing_slices);
    let span = positions[positions.len() - 1] - positions[0];
    out.regularized_depth = regularized_depth(span, target_spacing, sorted.len() as i64);
    if out.regularized_depth > 1 && span > 0.0 {
        target_spacing = span / (out.regularized_depth - 1) as f64;
    }
    out.spacings = Some(spacings);
    out.build_affines(
        sorted[0].origin,
        reference.normal,
        reference.col_spacing,
        reference.row_spacing,
        target_spacing,
    );
    if !out.affines_conform(&tol) {
        out.add_issue(GeometryIssue::InvalidGeometry);
    }
    out.classification = classify_volume(&out);
    out.requires_resampling = out.missing_slices
        || (!out.regular && !out.duplicate_positions)
        || out.gantry_tilted
        || !out.source_affine;
    out.finalize_disposition();
    out
}
fn reversed_geometry_input(positions: &[f64], tolerance: f64) -> bool {
    if positions.len() < 2 {
        return false;
    }
    let mut has_decrease = false;
    for i in 1..positions.len() {
        let delta = positions[i] - positions[i - 1];
        if delta > tolerance {
            return false;
        }
        if delta < -tolerance {
            has_decrease = true;
        }
    }
    has_decrease
}
pub(crate) fn vector_angle_degrees(a: Vec3, b: Vec3) -> f64 {
    let denominator = a.length() * b.length();
    if !finite_positive_spacing(denominator) {
        return 180.0;
    }
    let cosine = fmath::max(-1.0, fmath::min(1.0, a.dot(b) / denominator));
    fmath::acos(cosine) * 180.0 / PI
}
pub(crate) fn spacing_within_tolerance(
    value: f64,
    reference: f64,
    tol: &GeometryTolerances,
) -> bool {
    if !finite_positive_spacing(value) || !finite_positive_spacing(reference) {
        return false;
    }
    let limit = fmath::max(tol.spacing_abs, tol.spacing_rel * reference);
    (value - reference).abs() <= limit
}
fn spacing_range(spacings: &[f64]) -> (f64, f64) {
    let mut minimum = f64::INFINITY;
    let mut maximum = 0.0;
    for &spacing in spacings {
        if spacing < minimum {
            minimum = spacing;
        }
        if spacing > maximum {
            maximum = spacing;
        }
    }
    if minimum == f64::INFINITY {
        minimum = 0.0;
    }
    (minimum, maximum)
}
fn maximum_affine_residual(slices: &[SliceGeometry], origin: Vec3, z_step: Vec3) -> f64 {
    let mut maximum = 0.0;
    for (index, slice) in slices.iter().enumerate() {
        let expected = origin.add(z_step.scale(index as f64));
        maximum = fmath::max(maximum, slice.origin.sub(expected).length());
    }
    maximum
}
pub(crate) fn regularized_slice_spacing(spacings: &[f64], missing: bool) -> f64 {
    let mut positive: Vec<f64> = spacings
        .iter()
        .copied()
        .filter(|s| finite_positive_spacing(*s))
        .collect();
    if positive.is_empty() {
        return 1.0;
    }
    positive.sort_by(|a, b| a.total_cmp(b));
    if missing {
        return positive[0];
    }
    let middle = positive.len() / 2;
    if positive.len() % 2 == 1 {
        return positive[middle];
    }
    (positive[middle - 1] + positive[middle]) / 2.0
}
pub(crate) fn regularized_depth(span: f64, spacing: f64, fallback: i64) -> i64 {
    if !finite_positive_spacing(span) || !finite_positive_spacing(spacing) {
        return fallback.max(1);
    }
    let depth = fmath::f64_to_i64((span / spacing - 1e-12).ceil()).wrapping_add(1);
    depth.max(2)
}
fn classify_spacing(spacings: &[f64], tol: &GeometryTolerances) -> (bool, bool) {
    if spacings.is_empty() {
        return (true, false);
    }
    let mut base = f64::INFINITY;
    for &s in spacings {
        if s > 0.0 && s < base {
            base = s;
        }
    }
    if base == f64::INFINITY || base <= 0.0 {
        return (false, false);
    }
    let limit = fmath::max(tol.spacing_abs, tol.spacing_rel * base);
    let mut regular = true;
    let mut all_integer_multiples = true;
    let mut max_multiple = 1i64;
    for &s in spacings {
        if (s - base).abs() > limit {
            regular = false;
        }
        let mut k = fmath::f64_to_i64((s / base).round());
        if k < 1 {
            k = 1;
        }
        if (s - k as f64 * base).abs() > limit {
            all_integer_multiples = false;
        }
        if k > max_multiple {
            max_multiple = k;
        }
    }
    let missing = !regular && all_integer_multiples && max_multiple >= 2;
    (regular, missing)
}
fn detect_gantry_tilt(sorted: &[SliceGeometry], normal: Vec3, tol: &GeometryTolerances) -> bool {
    if sorted.len() < 2 {
        return false;
    }
    let stack = sorted[sorted.len() - 1]
        .origin
        .sub(sorted[0].origin)
        .normalize();
    if stack.is_zero() {
        return false;
    }
    stack.dot(normal).abs() < tol.tilt_cos
}
fn gantry_tilt_metrics(sorted: &[SliceGeometry], normal: Vec3) -> (Vec3, Vec3, f64) {
    if sorted.len() < 2 {
        return (Vec3::ZERO, Vec3::ZERO, 0.0);
    }
    let delta = sorted[sorted.len() - 1].origin.sub(sorted[0].origin);
    let normal_distance = delta.dot(normal);
    let offset = delta.sub(normal.scale(normal_distance));
    let span = normal_distance.abs();
    if span <= 0.0 || span.is_nan() || span.is_infinite() {
        return (Vec3::ZERO, Vec3::ZERO, 0.0);
    }
    let shear = offset.scale(1.0 / span);
    let angle = fmath::atan(shear.length()) * 180.0 / PI;
    (offset, shear, angle)
}
fn maximum_shear_deviation(sorted: &[SliceGeometry], normal: Vec3, reference_shear: Vec3) -> f64 {
    let mut maximum = 0.0;
    for index in 1..sorted.len() {
        let delta = sorted[index].origin.sub(sorted[index - 1].origin);
        let normal_span = delta.dot(normal);
        if !finite_positive_spacing(normal_span) {
            return f64::INFINITY;
        }
        let offset = delta.sub(normal.scale(normal_span));
        let interval_shear = offset.scale(1.0 / normal_span);
        maximum = fmath::max(maximum, interval_shear.sub(reference_shear).length());
    }
    maximum
}
pub(crate) fn classify_volume(g: &VolumeGeometry) -> VolumeClassification {
    use VolumeClassification as C;
    if g.slices().len() < 2 {
        C::SingleSlice
    } else if g.temporal_interleaved {
        C::TemporalInterleaved
    } else if g.duplicate_positions {
        C::DuplicatePositions
    } else if g.has_issue(GeometryIssue::InconsistentNormals) {
        C::InconsistentNormals
    } else if g.has_issue(GeometryIssue::MixedOrientation) {
        C::MixedOrientation
    } else if !g.orientation_stable {
        C::UnstableOrientation
    } else if g.gantry_tilted {
        C::GantryTilted
    } else if g.missing_slices {
        C::MissingSlices
    } else if !g.regular {
        C::IrregularSpacing
    } else {
        C::Regular
    }
}
pub(crate) fn classification_for_geometry_issue(
    reason: GeometryIssue,
) -> (VolumeClassification, bool) {
    use GeometryIssue as I;
    use VolumeClassification as C;
    match reason {
        I::SingleSlice => (C::SingleSlice, true),
        I::DuplicatePositions => (C::DuplicatePositions, true),
        I::SliceGaps => (C::MissingSlices, true),
        I::IrregularSpacing => (C::IrregularSpacing, true),
        I::GantryTilt | I::InconsistentGantryShear => (C::GantryTilted, true),
        I::MixedOrientation => (C::MixedOrientation, true),
        I::InconsistentNormals => (C::InconsistentNormals, true),
        I::TemporalInterleaving => (C::TemporalInterleaved, true),
        _ => (C::Regular, false),
    }
}
