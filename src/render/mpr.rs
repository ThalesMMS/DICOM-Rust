#![forbid(unsafe_code)]
use super::vec3::Vec3;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MprPlane {
    Axial,
    Coronal,
    Sagittal,
}
impl MprPlane {
    pub fn as_str(self) -> &'static str {
        match self {
            MprPlane::Axial => "axial",
            MprPlane::Coronal => "coronal",
            MprPlane::Sagittal => "sagittal",
        }
    }
    pub fn parse(s: &str) -> Option<MprPlane> {
        match s {
            "axial" => Some(MprPlane::Axial),
            "coronal" => Some(MprPlane::Coronal),
            "sagittal" => Some(MprPlane::Sagittal),
            _ => None,
        }
    }
}
pub fn mpr_plane_image_axes(plane: MprPlane, axis_x: Vec3, axis_y: Vec3) -> (Vec3, Vec3, bool) {
    let (axis_x, axis_y) = (axis_x.normalize(), axis_y.normalize());
    let normal = axis_x.cross(axis_y).normalize();
    if axis_x.length() == 0.0 || axis_y.length() == 0.0 || normal.length() == 0.0 {
        return (Vec3::ZERO, Vec3::ZERO, false);
    }
    match plane {
        MprPlane::Axial => (axis_x, axis_y, true),
        MprPlane::Coronal => (axis_x, normal.scale(-1.0), true),
        MprPlane::Sagittal => (axis_y, normal.scale(-1.0), true),
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct MeasureSpacing {
    pub x: f64,
    pub y: f64,
}
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Plane {
    pub origin: Vec3,
    pub u: Vec3,
    pub v: Vec3,
}
impl Plane {
    pub fn at(&self, s: f64, t: f64) -> Vec3 {
        self.origin.add(self.u.scale(s)).add(self.v.scale(t))
    }
    pub fn pixel_spacing_mm(&self, out_w: i64, out_h: i64) -> MeasureSpacing {
        let mut dw = (out_w - 1) as f64;
        let mut dh = (out_h - 1) as f64;
        if dw <= 0.0 {
            dw = 1.0;
        }
        if dh <= 0.0 {
            dh = 1.0;
        }
        MeasureSpacing {
            x: self.u.length() / dw,
            y: self.v.length() / dh,
        }
    }
}
