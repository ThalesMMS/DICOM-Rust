#![forbid(unsafe_code)]
use super::vec3::Vec3;
use super::volume::Volume;
use super::vr_transfer::clamp_float;
use crate::fmath;
pub(crate) const VR_DEFAULT_FOV: f64 = std::f64::consts::FRAC_PI_4;
const VR_FIT_FILL: f64 = 0.92;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct VrCameraProjection(pub i64);
impl VrCameraProjection {
    pub const PERSPECTIVE: VrCameraProjection = VrCameraProjection(0);
    pub const PARALLEL: VrCameraProjection = VrCameraProjection(1);
    pub const ENDOSCOPY: VrCameraProjection = VrCameraProjection(2);
    pub fn normalized(self) -> VrCameraProjection {
        match self {
            VrCameraProjection::PARALLEL | VrCameraProjection::ENDOSCOPY => self,
            _ => VrCameraProjection::PERSPECTIVE,
        }
    }
    pub fn label(self) -> &'static str {
        match self.normalized() {
            VrCameraProjection::PARALLEL => "Parallel",
            VrCameraProjection::ENDOSCOPY => "Endoscopy",
            _ => "Perspective",
        }
    }
    pub fn labels() -> Vec<&'static str> {
        vec!["Perspective", "Parallel", "Endoscopy"]
    }
    pub fn parse(label: &str) -> Option<VrCameraProjection> {
        let lower: String = label
            .trim()
            .chars()
            .map(|c| {
                let mut mapped = c.to_lowercase();
                match (mapped.next(), mapped.next()) {
                    (Some(l), None) => l,
                    _ if c == '\u{130}' => 'i',
                    _ => c,
                }
            })
            .collect();
        match lower.as_str() {
            "perspective" | "orbit" => Some(VrCameraProjection::PERSPECTIVE),
            "parallel" | "orthographic" | "ortho" => Some(VrCameraProjection::PARALLEL),
            "endoscopy" | "endoscopic" => Some(VrCameraProjection::ENDOSCOPY),
            _ => None,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct VrCamera {
    pub target: Vec3,
    pub yaw: f64,
    pub pitch: f64,
    pub roll: f64,
    pub distance: f64,
    pub fov_y: f64,
    pub radius: f64,
    pub projection: VrCameraProjection,
}
impl VrCamera {
    pub fn new(radius: f64) -> VrCamera {
        let mut c = VrCamera {
            fov_y: VR_DEFAULT_FOV,
            pitch: 0.3,
            ..Default::default()
        };
        c.fit(radius);
        c
    }
    fn dir(&self) -> Vec3 {
        let cp = fmath::cos(self.pitch);
        Vec3::new(
            cp * fmath::sin(self.yaw),
            cp * fmath::cos(self.yaw),
            fmath::sin(self.pitch),
        )
    }
    pub fn forward(&self) -> Vec3 {
        self.dir().normalize()
    }
    pub fn position(&self) -> Vec3 {
        self.target.sub(self.forward().scale(self.distance))
    }
    pub fn basis(&self) -> (Vec3, Vec3, Vec3) {
        let forward = self.forward();
        let mut right = Vec3::new(fmath::cos(self.yaw), -fmath::sin(self.yaw), 0.0);
        let mut up = right.cross(forward).normalize();
        if self.roll != 0.0 {
            right = right.rotate(forward, self.roll);
            up = up.rotate(forward, self.roll);
        }
        (right, up, forward)
    }
    pub fn look_along(&mut self, forward: Vec3, up: Vec3) {
        let forward = forward.normalize();
        self.pitch = fmath::asin(clamp_float(forward.z, -1.0, 1.0));
        if fmath::hypot(forward.x, forward.y) > 1e-9 {
            self.yaw = fmath::atan2(forward.x, forward.y);
        } else if forward.z < 0.0 {
            self.yaw = fmath::atan2(up.x, up.y);
        } else {
            self.yaw = fmath::atan2(-up.x, -up.y);
        }
        self.roll = 0.0;
        let (right, screen_up, _) = self.basis();
        self.roll = fmath::atan2(right.dot(up), screen_up.dot(up));
    }
    pub fn set_basis(&mut self, right: Vec3, _up: Vec3, forward: Vec3) {
        let forward = forward.normalize();
        let right = right.sub(forward.scale(right.dot(forward))).normalize();
        if forward.length() == 0.0 || right.length() == 0.0 {
            return;
        }
        self.pitch = fmath::asin(clamp_float(forward.z, -1.0, 1.0));
        if fmath::hypot(forward.x, forward.y) > 1e-9 {
            self.yaw = fmath::atan2(forward.x, forward.y);
        } else {
            self.yaw = fmath::atan2(-right.y, right.x);
        }
        let horizontal = Vec3::new(fmath::cos(self.yaw), -fmath::sin(self.yaw), 0.0);
        self.roll = fmath::atan2(horizontal.cross(right).dot(forward), horizontal.dot(right));
    }
    pub fn right(&self) -> Vec3 {
        self.basis().0
    }
    pub fn up(&self) -> Vec3 {
        self.basis().1
    }
    fn ray_ndc(&self, px: i64, py: i64, w: i64, h: i64) -> (f64, f64) {
        let mut aspect = 1.0;
        if h > 0 {
            aspect = w as f64 / h as f64;
        }
        let tan = fmath::tan(self.fov_y / 2.0);
        let ndcx = (2.0 * (px as f64 + 0.5) / w as f64 - 1.0) * aspect * tan;
        let ndcy = (1.0 - 2.0 * (py as f64 + 0.5) / h as f64) * tan;
        (ndcx, ndcy)
    }
    pub fn ray(&self, px: i64, py: i64, w: i64, h: i64) -> (Vec3, Vec3) {
        let (right, up, forward) = self.basis();
        let (ndcx, ndcy) = self.ray_ndc(px, py, w, h);
        match self.projection.normalized() {
            VrCameraProjection::PARALLEL => {
                let origin = self
                    .position()
                    .add(right.scale(ndcx * self.distance))
                    .add(up.scale(ndcy * self.distance));
                (origin, forward)
            }
            VrCameraProjection::ENDOSCOPY => {
                let dir = forward
                    .add(right.scale(ndcx))
                    .add(up.scale(ndcy))
                    .normalize();
                (self.target, dir)
            }
            _ => {
                let dir = forward
                    .add(right.scale(ndcx))
                    .add(up.scale(ndcy))
                    .normalize();
                (self.position(), dir)
            }
        }
    }
    pub fn rotate(&mut self, dx: f64, dy: f64) {
        const K: f64 = 0.008;
        let (mut right, mut up, mut forward) = self.basis();
        if dx != 0.0 {
            forward = forward.rotate(up, dx * K);
            right = right.rotate(up, dx * K);
        }
        if dy != 0.0 {
            forward = forward.rotate(right, -dy * K);
            up = up.rotate(right, -dy * K);
        }
        self.set_basis(right, up, forward);
    }
    pub fn roll_by(&mut self, theta: f64) {
        self.roll += theta;
    }
    pub fn zoom(&mut self, scale: f64) {
        if scale <= 0.0 {
            return;
        }
        if self.projection.normalized() == VrCameraProjection::ENDOSCOPY {
            let mut step_base = self.radius;
            if step_base <= 0.0 {
                step_base = self.distance;
            }
            if step_base <= 0.0 {
                step_base = 1.0;
            }
            self.target = self
                .target
                .add(self.forward().scale(fmath::log(scale) * step_base * 0.25));
            return;
        }
        self.distance /= scale;
        if self.radius > 0.0 {
            self.distance = clamp_float(self.distance, self.radius * 0.25, self.radius * 12.0);
        }
    }
    pub fn pan(&mut self, dx: f64, dy: f64) {
        let (right, up, _) = self.basis();
        let scale = self.distance * 0.0015;
        self.target = self
            .target
            .add(right.scale(-dx * scale))
            .add(up.scale(dy * scale));
    }
    pub fn fit(&mut self, radius: f64) {
        let radius = if radius <= 0.0 { 1.0 } else { radius };
        self.radius = radius;
        self.target = Vec3::ZERO;
        let mut s = fmath::sin(self.fov_y / 2.0);
        if s < 1e-3 {
            s = 1e-3;
        }
        self.distance = radius / s + radius;
        self.roll = 0.0;
    }
    pub fn fit_extent(&mut self, bounding_radius: f64, frame_extent: f64) {
        let bounding_radius = if bounding_radius <= 0.0 {
            1.0
        } else {
            bounding_radius
        };
        self.radius = bounding_radius;
        self.target = Vec3::ZERO;
        let frame_extent = if frame_extent <= 0.0 {
            bounding_radius
        } else {
            frame_extent
        };
        let mut t = fmath::tan(self.fov_y / 2.0);
        if t < 1e-3 {
            t = 1e-3;
        }
        self.distance = frame_extent / (t * VR_FIT_FILL);
        self.roll = 0.0;
    }
    pub fn fit_volume(&mut self, v: Option<&Volume>) {
        match v {
            None => self.fit(1.0),
            Some(v) => self.fit_extent(v.bounding_radius_mm(), v.frame_radius_mm()),
        }
    }
    pub fn set_orientation(&mut self, normal: Vec3) {
        let f = normal.scale(-1.0).normalize();
        if f.length() == 0.0 {
            return;
        }
        self.pitch = fmath::asin(clamp_float(f.z, -1.0, 1.0));
        self.yaw = fmath::atan2(f.x, f.y);
        self.roll = 0.0;
    }
}
