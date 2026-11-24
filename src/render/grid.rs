#![forbid(unsafe_code)]
use super::error::{ErrorKind, RenderError};
use super::vec3::Vec3;
use crate::fmath;
use crate::textfmt::float_v;
pub fn nearest_index(x: f64) -> i64 {
    fmath::f64_to_i64((x + 0.5).floor())
}
pub fn continuous_index_inside(x: f64, n: i64) -> bool {
    n > 0 && x >= -0.5 && x < n as f64 - 0.5
}
pub fn clamp_continuous_index(x: f64, n: i64, tolerance: f64) -> Option<f64> {
    if n <= 0 || !(x >= -0.5 - tolerance && x < n as f64 - 0.5 + tolerance) {
        return None;
    }
    Some(fmath::max(0.0, fmath::min((n - 1) as f64, x)))
}
pub fn nearest_index_inside(x: f64, n: i64) -> Option<i64> {
    if !continuous_index_inside(x, n) {
        return None;
    }
    Some(nearest_index(x).min(n - 1))
}
pub const DEFAULT_GRID_COORDINATE_TOLERANCE: f64 = 1e-6;
pub const DEFAULT_GRID_DIRECTION_TOLERANCE: f64 = 1e-6;
pub const DICOM_GRID_COORDINATE_TOLERANCE: f64 = 0.05;
pub const DICOM_GRID_DIRECTION_TOLERANCE: f64 = 1e-3;
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Grid {
    pub size: [i64; 3],
    pub origin: Vec3,
    pub spacing: [f64; 3],
    pub direction: [Vec3; 3],
}
fn finite(v: f64) -> bool {
    v.is_finite()
}
fn invalid(detail: String) -> RenderError {
    RenderError::new(
        ErrorKind::InvalidGrid,
        format!("render: invalid grid: {detail}"),
    )
}
impl Grid {
    pub fn new(
        size: [i64; 3],
        origin: Vec3,
        spacing: [f64; 3],
        direction: [Vec3; 3],
    ) -> (Grid, Result<(), RenderError>) {
        let g = Grid {
            size,
            origin,
            spacing,
            direction,
        };
        (g, g.validate())
    }
    pub fn validate(&self) -> Result<(), RenderError> {
        for axis in 0..3 {
            if self.size[axis] < 0 {
                return Err(invalid(format!(
                    "size [{} {} {}]",
                    self.size[0], self.size[1], self.size[2]
                )));
            }
            if !finite(self.spacing[axis]) || self.spacing[axis] <= 0.0 {
                return Err(invalid(format!(
                    "spacing [{} {} {}]",
                    float_v(self.spacing[0]),
                    float_v(self.spacing[1]),
                    float_v(self.spacing[2])
                )));
            }
            let d = self.direction[axis];
            if !d.finite() || d.length() == 0.0 {
                return Err(invalid(format!(
                    "direction [{} {} {}]",
                    self.direction[0].value_text(),
                    self.direction[1].value_text(),
                    self.direction[2].value_text()
                )));
            }
        }
        if !self.origin.finite() {
            return Err(invalid(format!("origin {}", self.origin.value_text())));
        }
        if self.index_matrix().is_none() {
            return Err(invalid("direction matrix is singular".into()));
        }
        Ok(())
    }
    pub fn index_to_point(&self, ci: Vec3) -> Vec3 {
        self.origin
            .add(self.direction[0].scale(self.spacing[0] * ci.x))
            .add(self.direction[1].scale(self.spacing[1] * ci.y))
            .add(self.direction[2].scale(self.spacing[2] * ci.z))
    }
    pub fn continuous_index(&self, p: Vec3) -> Option<Vec3> {
        let m = self.index_matrix()?;
        let d = p.sub(self.origin);
        Some(Vec3::new(
            m[0] * d.x + m[1] * d.y + m[2] * d.z,
            m[3] * d.x + m[4] * d.y + m[5] * d.z,
            m[6] * d.x + m[7] * d.y + m[8] * d.z,
        ))
    }
    pub fn index(&self, p: Vec3) -> ([i64; 3], bool) {
        let Some(ci) = self.continuous_index(p) else {
            return ([0; 3], false);
        };
        (
            [
                nearest_index(ci.x),
                nearest_index(ci.y),
                nearest_index(ci.z),
            ],
            self.is_inside_continuous_index(ci),
        )
    }
    pub fn is_inside(&self, p: Vec3) -> bool {
        self.continuous_index(p)
            .is_some_and(|ci| self.is_inside_continuous_index(ci))
    }
    pub fn is_inside_continuous_index(&self, ci: Vec3) -> bool {
        continuous_index_inside(ci.x, self.size[0])
            && continuous_index_inside(ci.y, self.size[1])
            && continuous_index_inside(ci.z, self.size[2])
    }
    pub fn congruent(
        &self,
        other: &Grid,
        coordinate_tolerance: f64,
        direction_tolerance: f64,
    ) -> bool {
        for axis in 0..3 {
            if !vec_within(
                self.direction[axis],
                other.direction[axis],
                direction_tolerance.abs(),
            ) {
                return false;
            }
        }
        let tolerance = coordinate_tolerance.abs()
            * fmath::min(
                self.spacing[0],
                fmath::min(self.spacing[1], self.spacing[2]),
            );
        if !(tolerance >= 0.0) {
            return false;
        }
        let mut last = [0f64; 3];
        for axis in 0..3 {
            last[axis] = (self.size[axis] - 1).max(0) as f64;
        }
        for corner in 0..8 {
            let mut ci = Vec3::ZERO;
            if corner & 1 != 0 {
                ci.x = last[0];
            }
            if corner & 2 != 0 {
                ci.y = last[1];
            }
            if corner & 4 != 0 {
                ci.z = last[2];
            }
            if self
                .index_to_point(ci)
                .sub(other.index_to_point(ci))
                .length()
                > tolerance
            {
                return false;
            }
        }
        true
    }
    pub fn same_geometry(
        &self,
        other: &Grid,
        coordinate_tolerance: f64,
        direction_tolerance: f64,
    ) -> bool {
        self.size == other.size && self.congruent(other, coordinate_tolerance, direction_tolerance)
    }
    pub(crate) fn index_matrix(&self) -> Option<[f64; 9]> {
        let c0 = self.direction[0].scale(self.spacing[0]);
        let c1 = self.direction[1].scale(self.spacing[1]);
        let c2 = self.direction[2].scale(self.spacing[2]);
        let (r0, r1, r2) = (c1.cross(c2), c2.cross(c0), c0.cross(c1));
        let det = c0.dot(r0);
        let scale = c0.length() * c1.length() * c2.length();
        if !finite(det) || scale == 0.0 || det.abs() <= 1e-12 * scale {
            return None;
        }
        Some([
            r0.x / det,
            r0.y / det,
            r0.z / det,
            r1.x / det,
            r1.y / det,
            r1.z / det,
            r2.x / det,
            r2.y / det,
            r2.z / det,
        ])
    }
}
fn vec_within(a: Vec3, b: Vec3, tolerance: f64) -> bool {
    (a.x - b.x).abs() <= tolerance
        && (a.y - b.y).abs() <= tolerance
        && (a.z - b.z).abs() <= tolerance
}
impl super::geometry::SliceGeometry {
    pub fn grid(&self, thickness: f64) -> Option<Grid> {
        let thickness = if thickness > 0.0 { thickness } else { 1.0 };
        let (row, column) = (self.row_dir.normalize(), self.col_dir.normalize());
        let g = Grid {
            size: [self.columns, self.rows, 1],
            origin: self.origin,
            spacing: [self.col_spacing, self.row_spacing, thickness],
            direction: [row, column, row.cross(column).normalize()],
        };
        g.validate().is_ok().then_some(g)
    }
}
