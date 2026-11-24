#![forbid(unsafe_code)]
use super::grid::nearest_index;
use super::vec3::Vec3;
use super::volume::Volume;
use super::vr_camera::{VrCamera, VrCameraProjection};
use crate::fmath;
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct VrCutMask {
    cols: i64,
    rows: i64,
    depth: i64,
    bits: Vec<u64>,
}
impl VrCutMask {
    pub fn new(vol: Option<&Volume>) -> VrCutMask {
        let Some(vol) = vol else {
            return VrCutMask::default();
        };
        let n = (vol.cols * vol.rows * vol.depth).max(0) as usize;
        VrCutMask {
            cols: vol.cols,
            rows: vol.rows,
            depth: vol.depth,
            bits: vec![0; n.div_ceil(64)],
        }
    }
    pub(crate) fn matches_volume(&self, vol: &Volume) -> bool {
        if self.cols != vol.cols || self.rows != vol.rows || self.depth != vol.depth {
            return false;
        }
        let total = vol.cols * vol.rows * vol.depth;
        total >= 0 && self.bits.len() == (total as usize).div_ceil(64)
    }
    fn inside(&self, x: i64, y: i64, z: i64) -> bool {
        x >= 0 && y >= 0 && z >= 0 && x < self.cols && y < self.rows && z < self.depth
    }
    fn idx(&self, x: i64, y: i64, z: i64) -> usize {
        ((z * self.rows + y) * self.cols + x) as usize
    }
    pub fn is_cut(&self, x: i64, y: i64, z: i64) -> bool {
        if !self.inside(x, y, z) {
            return false;
        }
        let i = self.idx(x, y, z);
        self.bits[i >> 6] & (1u64 << (i & 63)) != 0
    }
    pub fn set(&mut self, x: i64, y: i64, z: i64, cut: bool) {
        if !self.inside(x, y, z) {
            return;
        }
        let i = self.idx(x, y, z);
        if cut {
            self.bits[i >> 6] |= 1u64 << (i & 63);
        } else {
            self.bits[i >> 6] &= !(1u64 << (i & 63));
        }
    }
    pub fn cut_count(&self) -> u64 {
        self.bits.iter().map(|w| u64::from(w.count_ones())).sum()
    }
    pub(crate) fn cut(&self, tex: Vec3) -> bool {
        if self.cols < 2 {
            return false;
        }
        let x = nearest_index(tex.x * (self.cols - 1) as f64);
        let y = nearest_index(tex.y * (self.rows - 1) as f64);
        let z = nearest_index(tex.z * (self.depth - 1) as f64);
        self.is_cut(x, y, z)
    }
}
pub(crate) struct Projector {
    eye: Vec3,
    right: Vec3,
    up: Vec3,
    forward: Vec3,
    projection: VrCameraProjection,
    aspect_tan: f64,
    tan_half_fov: f64,
    distance: f64,
    width: f64,
    height: f64,
}
impl Projector {
    pub(crate) fn new(cam: &VrCamera, w: i64, h: i64) -> Projector {
        let (right, up, forward) = cam.basis();
        let aspect = if h > 0 { w as f64 / h as f64 } else { 1.0 };
        let mut tan = fmath::tan(cam.fov_y / 2.0);
        if tan == 0.0 {
            tan = 1e-6;
        }
        let projection = cam.projection.normalized();
        let eye = if projection == VrCameraProjection::ENDOSCOPY {
            cam.target
        } else {
            cam.position()
        };
        Projector {
            eye,
            right,
            up,
            forward,
            projection,
            aspect_tan: aspect * tan,
            tan_half_fov: tan,
            distance: cam.distance,
            width: w as f64,
            height: h as f64,
        }
    }
    pub(crate) fn project(&self, point: Vec3) -> Option<(f64, f64)> {
        let rel = point.sub(self.eye);
        let depth = rel.dot(self.forward);
        if depth <= 1e-6 {
            return None;
        }
        let (ndcx, ndcy);
        if self.projection == VrCameraProjection::PARALLEL {
            let x_scale = self.aspect_tan * self.distance;
            let y_scale = self.tan_half_fov * self.distance;
            if x_scale.abs() < 1e-6 || y_scale.abs() < 1e-6 {
                return None;
            }
            ndcx = rel.dot(self.right) / x_scale;
            ndcy = rel.dot(self.up) / y_scale;
        } else {
            ndcx = (rel.dot(self.right) / depth) / self.aspect_tan;
            ndcy = (rel.dot(self.up) / depth) / self.tan_half_fov;
        }
        Some((
            (ndcx + 1.0) / 2.0 * self.width,
            (1.0 - ndcy) / 2.0 * self.height,
        ))
    }
}
impl VrCamera {
    pub fn project(&self, p: Vec3, w: i64, h: i64) -> Option<(f64, f64)> {
        Projector::new(self, w, h).project(p)
    }
}
fn point_in_polygon(px: f64, py: f64, poly: &[[f64; 2]]) -> bool {
    let mut inside = false;
    let n = poly.len();
    let mut j = n.wrapping_sub(1);
    for i in 0..n {
        let (xi, yi) = (poly[i][0], poly[i][1]);
        let (xj, yj) = (poly[j][0], poly[j][1]);
        if (yi > py) != (yj > py) && px < (xj - xi) * (py - yi) / (yj - yi) + xi {
            inside = !inside;
        }
        j = i;
    }
    inside
}
pub fn carve_vr(
    vol: Option<&Volume>,
    cam: &VrCamera,
    prev: Option<&VrCutMask>,
    lasso: &[[f64; 2]],
    w: i64,
    h: i64,
    cut_inside: bool,
) -> VrCutMask {
    let mut mask = match (prev, vol) {
        (Some(prev), Some(vol)) if prev.matches_volume(vol) => prev.clone(),
        _ => VrCutMask::new(vol),
    };
    let Some(vol) = vol else {
        return mask;
    };
    if lasso.len() < 3 {
        return mask;
    }
    let projector = Projector::new(cam, w, h);
    let size = vol.physical_size();
    let box_min = size.scale(-0.5);
    let cxf = fmath::max(1.0, (vol.cols - 1) as f64);
    let cyf = fmath::max(1.0, (vol.rows - 1) as f64);
    let czf = fmath::max(1.0, (vol.depth - 1) as f64);
    let row_cols = vol.rows * vol.cols;
    let total = vol.depth * row_cols;
    for i in 0..total.max(0) {
        let z = i / row_cols;
        let rem = i - z * row_cols;
        let y = rem / vol.cols;
        let x = rem - y * vol.cols;
        let point = Vec3::new(
            box_min.x + x as f64 / cxf * size.x,
            box_min.y + y as f64 / cyf * size.y,
            box_min.z + z as f64 / czf * size.z,
        );
        let inside = projector
            .project(point)
            .is_some_and(|(px, py)| point_in_polygon(px, py, lasso));
        if cut_inside == inside {
            let i = i as usize;
            mask.bits[i >> 6] |= 1u64 << (i & 63);
        }
    }
    mask
}
impl VrCutMask {
    pub fn dimensions(&self) -> (i64, i64, i64) {
        (self.cols, self.rows, self.depth)
    }
}
