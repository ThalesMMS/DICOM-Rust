#![forbid(unsafe_code)]
use super::mpr::{MprPlane, Plane, mpr_plane_image_axes};
use super::vec3::Vec3;
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PatientAxis {
    #[default]
    X,
    Y,
    Z,
}
impl PatientAxis {
    pub fn from_index(i: usize) -> PatientAxis {
        match i {
            0 => PatientAxis::X,
            1 => PatientAxis::Y,
            _ => PatientAxis::Z,
        }
    }
    pub fn index(self) -> usize {
        self as usize
    }
    pub fn unit(self) -> Vec3 {
        match self {
            PatientAxis::X => Vec3::new(1.0, 0.0, 0.0),
            PatientAxis::Y => Vec3::new(0.0, 1.0, 0.0),
            PatientAxis::Z => Vec3::new(0.0, 0.0, 1.0),
        }
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AnatomicalAxis {
    pub axis: PatientAxis,
    pub negative: bool,
}
impl AnatomicalAxis {
    pub fn direction(self) -> Vec3 {
        if self.negative {
            self.axis.unit().scale(-1.0)
        } else {
            self.axis.unit()
        }
    }
}
const ANATOMICAL_AXES_DEGENERATE_DETERMINANT: f64 = 1e-6;
pub fn nearest_anatomical_axes(axes: [Vec3; 3]) -> Option<[AnatomicalAxis; 3]> {
    let mut unit = [[0f64; 3]; 3];
    for (index, axis) in axes.iter().enumerate() {
        let n = finite_unit(*axis)?;
        unit[index] = [n.x, n.y, n.z];
    }
    let determinant = unit[0][0] * (unit[1][1] * unit[2][2] - unit[1][2] * unit[2][1])
        - unit[0][1] * (unit[1][0] * unit[2][2] - unit[1][2] * unit[2][0])
        + unit[0][2] * (unit[1][0] * unit[2][1] - unit[1][1] * unit[2][0]);
    if determinant.abs() <= ANATOMICAL_AXES_DEGENERATE_DETERMINANT {
        return None;
    }
    let mut result = [AnatomicalAxis::default(); 3];
    let mut image_used = [false; 3];
    let mut patient_used = [false; 3];
    for _ in 0..3 {
        let (mut best_image, mut best_patient, mut best) = (usize::MAX, usize::MAX, -1.0);
        for image_axis in 0..3 {
            if image_used[image_axis] {
                continue;
            }
            for patient in 0..3 {
                if patient_used[patient] {
                    continue;
                }
                let magnitude = unit[image_axis][patient].abs();
                if magnitude > best {
                    best_image = image_axis;
                    best_patient = patient;
                    best = magnitude;
                }
            }
        }
        image_used[best_image] = true;
        patient_used[best_patient] = true;
        result[best_image] = AnatomicalAxis {
            axis: PatientAxis::from_index(best_patient),
            negative: unit[best_image][best_patient] < 0.0,
        };
    }
    Some(result)
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AxisReorientation {
    pub order: [usize; 3],
    pub flip: [bool; 3],
}
pub fn reoriented_axes(axes: [Vec3; 3]) -> Option<AxisReorientation> {
    let assignment = nearest_anatomical_axes(axes)?;
    let mut r = AxisReorientation::default();
    for (image_axis, anatomical) in assignment.iter().enumerate() {
        r.order[anatomical.axis.index()] = image_axis;
        r.flip[anatomical.axis.index()] = anatomical.negative;
    }
    Some(r)
}
impl AxisReorientation {
    pub fn axes(&self, axes: [Vec3; 3]) -> [Vec3; 3] {
        let mut out = [Vec3::ZERO; 3];
        for (output, slot) in out.iter_mut().enumerate() {
            let axis = axes[self.order[output]];
            *slot = if self.flip[output] {
                axis.scale(-1.0)
            } else {
                axis
            };
        }
        out
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ImageOrientation {
    pub transpose: bool,
    pub flip_horizontal: bool,
    pub flip_vertical: bool,
}
impl ImageOrientation {
    pub fn is_identity(&self) -> bool {
        *self == ImageOrientation::default()
    }
    pub fn inverse(&self) -> ImageOrientation {
        if !self.transpose {
            return *self;
        }
        ImageOrientation {
            transpose: true,
            flip_horizontal: self.flip_vertical,
            flip_vertical: self.flip_horizontal,
        }
    }
    pub fn size(&self, w: i64, h: i64) -> (i64, i64) {
        if self.transpose { (h, w) } else { (w, h) }
    }
    pub fn point(&self, mut x: f64, mut y: f64, w: i64, h: i64) -> (f64, f64) {
        if self.transpose {
            std::mem::swap(&mut x, &mut y);
        }
        let (out_w, out_h) = self.size(w, h);
        if self.flip_horizontal {
            x = (out_w - 1) as f64 - x;
        }
        if self.flip_vertical {
            y = (out_h - 1) as f64 - y;
        }
        (x, y)
    }
    pub fn source_point(&self, mut x: f64, mut y: f64, w: i64, h: i64) -> (f64, f64) {
        let (out_w, out_h) = self.size(w, h);
        if self.flip_horizontal {
            x = (out_w - 1) as f64 - x;
        }
        if self.flip_vertical {
            y = (out_h - 1) as f64 - y;
        }
        if self.transpose {
            std::mem::swap(&mut x, &mut y);
        }
        (x, y)
    }
    pub fn fraction(&self, mut fx: f64, mut fy: f64) -> (f64, f64) {
        if self.transpose {
            std::mem::swap(&mut fx, &mut fy);
        }
        if self.flip_horizontal {
            fx = 1.0 - fx;
        }
        if self.flip_vertical {
            fy = 1.0 - fy;
        }
        (fx, fy)
    }
    pub fn source_fraction(&self, mut fx: f64, mut fy: f64) -> (f64, f64) {
        if self.flip_horizontal {
            fx = 1.0 - fx;
        }
        if self.flip_vertical {
            fy = 1.0 - fy;
        }
        if self.transpose {
            std::mem::swap(&mut fx, &mut fy);
        }
        (fx, fy)
    }
    pub fn axes(&self, mut right: Vec3, mut down: Vec3) -> (Vec3, Vec3) {
        if self.transpose {
            std::mem::swap(&mut right, &mut down);
        }
        if self.flip_horizontal {
            right = right.scale(-1.0);
        }
        if self.flip_vertical {
            down = down.scale(-1.0);
        }
        (right, down)
    }
    pub fn plane(&self, mut plane: Plane) -> Plane {
        if self.transpose {
            std::mem::swap(&mut plane.u, &mut plane.v);
        }
        if self.flip_horizontal {
            plane.origin = plane.origin.add(plane.u);
            plane.u = plane.u.scale(-1.0);
        }
        if self.flip_vertical {
            plane.origin = plane.origin.add(plane.v);
            plane.v = plane.v.scale(-1.0);
        }
        plane
    }
    #[doc(hidden)]
    pub fn permute_pixels(&self, pixels: &[u8], w: i64, h: i64, channels: usize) -> Vec<u8> {
        if self.is_identity() {
            return pixels.to_vec();
        }
        let (out_w, out_h) = self.size(w, h);
        let mut out = vec![0u8; (out_w.max(0) * out_h.max(0)) as usize * channels];
        for y in 0..out_h {
            for x in 0..out_w {
                let (sx, sy) = self.source_point(x as f64, y as f64, w, h);
                let src = ((sy as i64) * w + sx as i64) as usize * channels;
                let dst = (y * out_w + x) as usize * channels;
                out[dst..dst + channels].copy_from_slice(&pixels[src..src + channels]);
            }
        }
        out
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnatomicalPlane {
    Axial,
    Coronal,
    Sagittal,
}
impl AnatomicalPlane {
    pub fn as_str(self) -> &'static str {
        match self {
            AnatomicalPlane::Axial => "axial",
            AnatomicalPlane::Coronal => "coronal",
            AnatomicalPlane::Sagittal => "sagittal",
        }
    }
    pub fn parse(s: &str) -> Option<AnatomicalPlane> {
        match s {
            "axial" => Some(AnatomicalPlane::Axial),
            "coronal" => Some(AnatomicalPlane::Coronal),
            "sagittal" => Some(AnatomicalPlane::Sagittal),
            _ => None,
        }
    }
    pub fn normal_axis(self) -> PatientAxis {
        match self {
            AnatomicalPlane::Axial => PatientAxis::Z,
            AnatomicalPlane::Coronal => PatientAxis::Y,
            AnatomicalPlane::Sagittal => PatientAxis::X,
        }
    }
}
pub fn anatomical_plane_order() -> [AnatomicalPlane; 3] {
    [
        AnatomicalPlane::Axial,
        AnatomicalPlane::Coronal,
        AnatomicalPlane::Sagittal,
    ]
}
pub fn radiological_image_axes(p: AnatomicalPlane) -> (Vec3, Vec3) {
    match p {
        AnatomicalPlane::Axial => (Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0)),
        AnatomicalPlane::Coronal => (Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 0.0, -1.0)),
        AnatomicalPlane::Sagittal => (Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, 0.0, -1.0)),
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnatomicalMprView {
    pub anatomical: AnatomicalPlane,
    pub plane: MprPlane,
    pub orientation: ImageOrientation,
}
pub fn index_mpr_views() -> [AnatomicalMprView; 3] {
    let view = |anatomical, plane| AnatomicalMprView {
        anatomical,
        plane,
        orientation: ImageOrientation::default(),
    };
    [
        view(AnatomicalPlane::Axial, MprPlane::Axial),
        view(AnatomicalPlane::Coronal, MprPlane::Coronal),
        view(AnatomicalPlane::Sagittal, MprPlane::Sagittal),
    ]
}
pub fn anatomical_mpr_views(axis_x: Vec3, axis_y: Vec3) -> ([AnatomicalMprView; 3], bool) {
    let (axis_x, axis_y) = (axis_x.normalize(), axis_y.normalize());
    let normal = axis_x.cross(axis_y).normalize();
    let Some(assignment) = nearest_anatomical_axes([axis_x, axis_y, normal]) else {
        return (index_mpr_views(), false);
    };
    let index_planes = [MprPlane::Sagittal, MprPlane::Coronal, MprPlane::Axial];
    let mut views = index_mpr_views();
    for (view_index, anatomical) in anatomical_plane_order().into_iter().enumerate() {
        let normal_axis = anatomical.normal_axis();
        let mut volume_axis = 0;
        for (axis, assigned) in assignment.iter().enumerate() {
            if assigned.axis == normal_axis {
                volume_axis = axis;
            }
        }
        let plane = index_planes[volume_axis];
        let (right, down, _) = mpr_plane_image_axes(plane, axis_x, axis_y);
        views[view_index] = AnatomicalMprView {
            anatomical,
            plane,
            orientation: radiological_orientation(
                anatomical,
                right,
                down,
                &assignment,
                [axis_x, axis_y, normal],
            ),
        };
    }
    (views, true)
}
fn radiological_orientation(
    anatomical: AnatomicalPlane,
    right: Vec3,
    down: Vec3,
    assignment: &[AnatomicalAxis; 3],
    volume_axes: [Vec3; 3],
) -> ImageOrientation {
    let (target_right, target_down) = radiological_image_axes(anatomical);
    let patient_direction = |vector: Vec3| {
        let (mut best, mut best_dot) = (0usize, 0.0f64);
        for (axis, volume_axis) in volume_axes.iter().enumerate() {
            let dot = vector.dot(*volume_axis);
            if dot.abs() > best_dot.abs() {
                best = axis;
                best_dot = dot;
            }
        }
        let direction = assignment[best].direction();
        if best_dot < 0.0 {
            direction.scale(-1.0)
        } else {
            direction
        }
    };
    let (mut right, mut down) = (patient_direction(right), patient_direction(down));
    let mut orientation = ImageOrientation::default();
    if right.dot(target_right) == 0.0 {
        orientation.transpose = true;
        std::mem::swap(&mut right, &mut down);
    }
    orientation.flip_horizontal = right.dot(target_right) < 0.0;
    orientation.flip_vertical = down.dot(target_down) < 0.0;
    orientation
}
pub fn mpr_view_for_plane(
    views: &[AnatomicalMprView; 3],
    plane: MprPlane,
) -> Option<AnatomicalMprView> {
    views.iter().copied().find(|v| v.plane == plane)
}
pub fn mpr_view_for_anatomical(
    views: &[AnatomicalMprView; 3],
    anatomical: AnatomicalPlane,
) -> Option<AnatomicalMprView> {
    views.iter().copied().find(|v| v.anatomical == anatomical)
}
fn finite_unit(v: Vec3) -> Option<Vec3> {
    if !v.finite() {
        return None;
    }
    let unit = v.normalize();
    if unit.is_zero() { None } else { Some(unit) }
}
