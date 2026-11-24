#![forbid(unsafe_code)]
use super::affine::finite_positive_spacing;
use super::frame::Frame;
use super::geometry::spacing_within_tolerance;
use super::geometry::{GeometryDisposition, GeometryTolerances, SliceGeometry};
use super::vec3::Vec3;
use super::volume::{
    GeometryAssessment, SliceGeometryFailure, Stack, collect_slice_geometries, frame_pixel_spacing,
    geometry_timeline, series_pixel_spacing, slice_geometry,
};
use crate::numtext::format_float;
use crate::object::geometry::{IMAGE_ORIENTATION_TOLERANCE, image_orientation_deviation};
use crate::object::orientation::classify_image_plane;
use std::sync::Arc;
pub type VolumeCandidateRestriction = Box<dyn Fn(&Frame) -> String + Send + Sync>;
#[derive(Default)]
pub struct VolumeCandidateOptions {
    pub tolerances: GeometryTolerances,
    pub split_by_echo: bool,
    pub split_by_temporal_position: bool,
    pub split_by_acquisition_number: bool,
    pub split_by_slice_thickness: bool,
    pub restrictions: Vec<Option<VolumeCandidateRestriction>>,
}
pub fn default_volume_candidate_options() -> VolumeCandidateOptions {
    VolumeCandidateOptions {
        tolerances: GeometryTolerances::default_tolerances(),
        split_by_echo: true,
        split_by_temporal_position: true,
        ..VolumeCandidateOptions::default()
    }
}
#[derive(Debug, Clone, Default)]
pub struct VolumeCandidate {
    pub frame_indices: Vec<i64>,
    pub frames: Vec<Arc<Frame>>,
    pub plane: &'static str,
    pub row_dir: Vec3,
    pub col_dir: Vec3,
    pub normal: Vec3,
    pub rows: i64,
    pub columns: i64,
    pub row_spacing: f64,
    pub column_spacing: f64,
    pub slice_thickness: f64,
    pub echo_number: i64,
    pub has_echo_number: bool,
    pub acquisition_number: i64,
    pub has_acquisition_number: bool,
    pub temporal_position: i64,
    pub has_temporal_position: bool,
    pub stack_id: String,
    pub description: String,
    pub assessment: GeometryAssessment,
}
impl VolumeCandidate {
    pub fn slice_count(&self) -> i64 {
        self.frame_indices.len() as i64
    }
    pub fn usable(&self) -> bool {
        self.assessment.disposition != GeometryDisposition::Unsupported
            && self.frame_indices.len() >= 2
    }
}
#[derive(Debug, Clone, Default)]
struct Temporal {
    position: i64,
    has_position: bool,
    stack_id: String,
}
struct Partition {
    reference: SliceGeometry,
    source: u8,
    thickness: f64,
    echo: i64,
    has_echo: bool,
    acq: i64,
    has_acq: bool,
    restricted: Vec<String>,
    indices: Vec<i64>,
}
fn same_slice_thickness(reference: f64, value: f64, tol: &GeometryTolerances) -> bool {
    let (reference_ok, value_ok) = (
        finite_positive_spacing(reference),
        finite_positive_spacing(value),
    );
    if !reference_ok || !value_ok {
        return reference_ok == value_ok;
    }
    spacing_within_tolerance(value, reference, tol)
}
impl Stack {
    pub fn volume_candidates(&self) -> Vec<VolumeCandidate> {
        self.volume_candidates_with_options(&default_volume_candidate_options())
    }
    pub fn volume_candidates_with_options(
        &self,
        opts: &VolumeCandidateOptions,
    ) -> Vec<VolumeCandidate> {
        let tol = opts.tolerances.normalized();
        let (mut fallback_row, mut fallback_column) = series_pixel_spacing(self);
        if !finite_positive_spacing(fallback_row) || !finite_positive_spacing(fallback_column) {
            (fallback_row, fallback_column) = (1.0, 1.0);
        }
        let mut partitions: Vec<Partition> = Vec::new();
        for (index, frame) in self.frames.iter().enumerate() {
            let Some(frame) = frame else { continue };
            if frame.decode_err.is_some() || frame.metadata.rows == 0 || frame.metadata.columns == 0
            {
                continue;
            }
            match image_orientation_deviation(&frame.image_orientation) {
                Some(d) if d <= IMAGE_ORIENTATION_TOLERANCE => {}
                _ => continue,
            }
            let Some(geometry) = slice_geometry(frame, Vec3::ZERO, fallback_row, fallback_column)
            else {
                continue;
            };
            let source = if frame.image_position.len() >= 3 {
                1
            } else {
                2
            };
            let restricted: Vec<String> = opts
                .restrictions
                .iter()
                .map(|r| r.as_ref().map(|r| r(frame)).unwrap_or_default())
                .collect();
            let found = partitions.iter().position(|c| {
                let r = &c.reference;
                if r.rows != geometry.rows
                    || r.columns != geometry.columns
                    || c.source != source
                    || geometry.row_dir.dot(r.row_dir) < tol.orientation_cos
                    || geometry.col_dir.dot(r.col_dir) < tol.orientation_cos
                    || !spacing_within_tolerance(geometry.row_spacing, r.row_spacing, &tol)
                    || !spacing_within_tolerance(geometry.col_spacing, r.col_spacing, &tol)
                {
                    return false;
                }
                if opts.split_by_echo
                    && (c.has_echo != frame.has_echo_number || c.echo != frame.echo_number)
                {
                    return false;
                }
                if opts.split_by_acquisition_number
                    && (c.has_acq != frame.has_acquisition_number
                        || c.acq != frame.acquisition_number)
                {
                    return false;
                }
                if opts.split_by_slice_thickness
                    && !same_slice_thickness(c.thickness, frame.slice_thickness, &tol)
                {
                    return false;
                }
                c.restricted == restricted
            });
            let at = match found {
                Some(at) => at,
                None => {
                    partitions.push(Partition {
                        reference: geometry,
                        source,
                        thickness: frame.slice_thickness,
                        echo: frame.echo_number,
                        has_echo: frame.has_echo_number,
                        acq: frame.acquisition_number,
                        has_acq: frame.has_acquisition_number,
                        restricted,
                        indices: Vec::new(),
                    });
                    partitions.len() - 1
                }
            };
            partitions[at].indices.push(index as i64);
        }
        let mut candidates = Vec::with_capacity(partitions.len());
        for part in &partitions {
            let r = part.reference;
            let (groups, temporal) = if opts.split_by_temporal_position && part.indices.len() > 1 {
                self.split_volume_candidate_temporal(
                    &part.indices,
                    &r,
                    fallback_row,
                    fallback_column,
                )
            } else {
                (vec![part.indices.clone()], vec![Temporal::default()])
            };
            for (g, indices) in groups.into_iter().enumerate() {
                let frames: Vec<Arc<Frame>> = indices
                    .iter()
                    .filter_map(|&i| self.frames[i as usize].clone())
                    .collect();
                let first = &frames[0];
                let mut candidate = VolumeCandidate {
                    plane: classify_image_plane(&first.image_orientation),
                    row_dir: r.row_dir,
                    col_dir: r.col_dir,
                    normal: r.normal,
                    rows: r.rows,
                    columns: r.columns,
                    row_spacing: r.row_spacing,
                    column_spacing: r.col_spacing,
                    slice_thickness: first.slice_thickness,
                    echo_number: first.echo_number,
                    has_echo_number: first.has_echo_number,
                    acquisition_number: first.acquisition_number,
                    has_acquisition_number: first.has_acquisition_number,
                    temporal_position: temporal[g].position,
                    has_temporal_position: temporal[g].has_position,
                    stack_id: temporal[g].stack_id.clone(),
                    frame_indices: indices,
                    frames,
                    ..VolumeCandidate::default()
                };
                candidate.assessment = self
                    .volume_candidate_stack(&candidate)
                    .geometry_assessment();
                candidates.push(candidate);
            }
        }
        candidates.sort_by(|a, b| {
            b.usable()
                .cmp(&a.usable())
                .then(b.frame_indices.len().cmp(&a.frame_indices.len()))
                .then(a.frame_indices[0].cmp(&b.frame_indices[0]))
        });
        describe_volume_candidates(&mut candidates);
        candidates
    }
    pub fn volume_candidate_stack(&self, candidate: &VolumeCandidate) -> Stack {
        let mut sub = Stack::new(candidate.frames.iter().cloned().map(Some).collect());
        sub.uid = self.uid.clone();
        sub.study_uid = self.study_uid.clone();
        sub.modality = self.modality.clone();
        sub.body_part = self.body_part.clone();
        sub.default_window = self.default_window.clone();
        sub.pixel_spacing = self.pixel_spacing.clone();
        sub.slice_thickness = self.slice_thickness;
        sub.spacing_between_slices = self.spacing_between_slices;
        if let Some(first) = candidate.frames.first() {
            if frame_pixel_spacing(&first.pixel_spacing).is_some() {
                sub.pixel_spacing = first.pixel_spacing.clone();
            }
            if finite_positive_spacing(first.slice_thickness) {
                sub.slice_thickness = first.slice_thickness;
            }
            if first.spacing_between_slices != 0.0 && first.spacing_between_slices.is_finite() {
                sub.spacing_between_slices = first.spacing_between_slices;
            }
        }
        sub
    }
    fn split_volume_candidate_temporal(
        &self,
        indices: &[i64],
        reference: &SliceGeometry,
        row_spacing: f64,
        column_spacing: f64,
    ) -> (Vec<Vec<i64>>, Vec<Temporal>) {
        let whole = (vec![indices.to_vec()], vec![Temporal::default()]);
        let frames: Vec<Option<Arc<Frame>>> = indices
            .iter()
            .map(|&i| self.frames[i as usize].clone())
            .collect();
        let (collected, failure, _) = collect_slice_geometries(
            &frames,
            reference.normal,
            row_spacing,
            column_spacing,
            reference.rows,
            reference.columns,
        );
        if failure != SliceGeometryFailure::Valid || collected.len() != frames.len() {
            return whole;
        }
        let timeline = geometry_timeline(&collected);
        if timeline.used_occurrence_fallback || (!timeline.dynamic && !timeline.multiple_stacks) {
            return whole;
        }
        let mut groups = Vec::new();
        let mut temporal = Vec::new();
        for point in timeline.points() {
            for stack in &point.stacks {
                let mut group: Vec<i64> = stack
                    .frame_indices
                    .iter()
                    .filter(|&&local| local >= 0 && (local as usize) < collected.len())
                    .map(|&local| indices[(collected[local as usize].frame_index - 1) as usize])
                    .collect();
                if group.is_empty() {
                    continue;
                }
                group.sort_unstable();
                groups.push(group);
                temporal.push(Temporal {
                    position: point.temporal_position,
                    has_position: point.has_position && timeline.dynamic,
                    stack_id: stack.id.clone(),
                });
            }
        }
        if groups.is_empty() {
            return whole;
        }
        (groups, temporal)
    }
}
fn describe_volume_candidates(candidates: &mut [VolumeCandidate]) {
    if candidates.is_empty() {
        return;
    }
    let spacing = |c: &VolumeCandidate| {
        format!(
            "{}×{} mm",
            format_float(c.column_spacing, 'g', 4),
            format_float(c.row_spacing, 'g', 4)
        )
    };
    let thickness = |c: &VolumeCandidate| {
        if !finite_positive_spacing(c.slice_thickness) {
            return String::new();
        }
        format!("{} mm thick", format_float(c.slice_thickness, 'g', 4))
    };
    let echo = |c: &VolumeCandidate| {
        if c.has_echo_number {
            format!("echo {}", c.echo_number)
        } else {
            String::new()
        }
    };
    let acquisition = |c: &VolumeCandidate| {
        if c.has_acquisition_number {
            format!("acquisition {}", c.acquisition_number)
        } else {
            String::new()
        }
    };
    let time_point = |c: &VolumeCandidate| {
        if c.has_temporal_position {
            format!("time point {}", c.temporal_position)
        } else {
            String::new()
        }
    };
    let stack_id = |c: &VolumeCandidate| {
        if c.stack_id.is_empty() {
            String::new()
        } else {
            format!("stack {}", c.stack_id)
        }
    };
    let details: [&dyn Fn(&VolumeCandidate) -> String; 6] = [
        &spacing,
        &thickness,
        &echo,
        &acquisition,
        &time_point,
        &stack_id,
    ];
    let varying: Vec<bool> = details
        .iter()
        .map(|d| {
            let first = d(&candidates[0]);
            candidates[1..].iter().any(|c| d(c) != first)
        })
        .collect();
    for c in candidates.iter_mut() {
        let mut plane = c.plane.to_lowercase();
        if plane.is_empty() {
            plane = "unknown plane".into();
        }
        let noun = if c.frame_indices.len() == 1 {
            "slice"
        } else {
            "slices"
        };
        let mut parts = vec![
            format!("{plane} {}×{}", c.columns, c.rows),
            format!("{} {noun}", c.frame_indices.len()),
        ];
        for (d, detail) in details.iter().enumerate() {
            if varying[d] {
                let value = detail(c);
                if !value.is_empty() {
                    parts.push(value);
                }
            }
        }
        c.description = parts.join(", ");
    }
}
