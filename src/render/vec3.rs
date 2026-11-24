#![forbid(unsafe_code)]
use crate::fmath;
use std::f64::consts::PI;
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Vec3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}
impl Vec3 {
    pub const ZERO: Vec3 = Vec3 {
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };
    pub const fn new(x: f64, y: f64, z: f64) -> Vec3 {
        Vec3 { x, y, z }
    }
    pub fn add(self, b: Vec3) -> Vec3 {
        Vec3::new(self.x + b.x, self.y + b.y, self.z + b.z)
    }
    pub fn sub(self, b: Vec3) -> Vec3 {
        Vec3::new(self.x - b.x, self.y - b.y, self.z - b.z)
    }
    pub fn scale(self, s: f64) -> Vec3 {
        Vec3::new(self.x * s, self.y * s, self.z * s)
    }
    pub fn dot(self, b: Vec3) -> f64 {
        self.x * b.x + self.y * b.y + self.z * b.z
    }
    pub fn cross(self, b: Vec3) -> Vec3 {
        Vec3::new(
            self.y * b.z - self.z * b.y,
            self.z * b.x - self.x * b.z,
            self.x * b.y - self.y * b.x,
        )
    }
    pub fn length(self) -> f64 {
        self.dot(self).sqrt()
    }
    pub fn normalize(self) -> Vec3 {
        let l = self.length();
        if l == 0.0 || l.is_nan() || l.is_infinite() {
            return Vec3::ZERO;
        }
        self.scale(1.0 / l)
    }
    pub fn rotate(self, axis: Vec3, theta: f64) -> Vec3 {
        let k = axis.normalize();
        if k == Vec3::ZERO {
            return self;
        }
        let (cos, sin) = (fmath::cos(theta), fmath::sin(theta));
        self.scale(cos)
            .add(k.cross(self).scale(sin))
            .add(k.scale(k.dot(self) * (1.0 - cos)))
    }
    pub fn is_zero(self) -> bool {
        self == Vec3::ZERO
    }
    pub fn finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.z.is_finite()
    }
    #[doc(hidden)]
    pub fn value_text(self) -> String {
        use crate::textfmt::float_v;
        format!(
            "{{{} {} {}}}",
            float_v(self.x),
            float_v(self.y),
            float_v(self.z)
        )
    }
}
pub type WorldPoint = Vec3;
pub fn world_distance(a: WorldPoint, b: WorldPoint) -> f64 {
    Vec3::new(b.x - a.x, b.y - a.y, b.z - a.z).length()
}
pub fn world_angle(a: WorldPoint, vertex: WorldPoint, b: WorldPoint, acute: bool) -> Option<f64> {
    world_segment_angle(vertex, a, vertex, b, acute)
}
pub fn world_segment_angle(
    a: WorldPoint,
    b: WorldPoint,
    c: WorldPoint,
    d: WorldPoint,
    acute: bool,
) -> Option<f64> {
    let u = Vec3::new(b.x - a.x, b.y - a.y, b.z - a.z);
    let v = Vec3::new(d.x - c.x, d.y - c.y, d.z - c.z);
    let denominator = u.length() * v.length();
    if denominator == 0.0 || denominator.is_nan() || denominator.is_infinite() {
        return None;
    }
    let mut cosine = u.dot(v) / denominator;
    if acute {
        cosine = cosine.abs();
    }
    cosine = fmath::max(-1.0, fmath::min(1.0, cosine));
    Some(fmath::acos(cosine) * 180.0 / PI)
}
