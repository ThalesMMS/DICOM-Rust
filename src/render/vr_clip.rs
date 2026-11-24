#![forbid(unsafe_code)]
use super::vec3::Vec3;
use super::volume::Volume;
use super::vr_geometry::VrGeometry;
use super::vr_labels::VrLabelMap;
use super::vr_scalpel::VrCutMask;
use super::vr_transfer::{clamp_unit, finite};
use super::window::WindowLevel;
use crate::fmath;
use std::sync::Arc;
pub const VR_CROP_ALL_REGIONS: u32 = (1 << 27) - 1;
pub const VR_CROP_SUBVOLUME: u32 = 1 << 13;
pub const VR_CROP_FENCE: u32 = 0x2eb_feba;
pub const VR_CROP_CROSS: u32 = 0x041_7410;
pub const VR_CROP_INVERTED_FENCE: u32 = VR_CROP_ALL_REGIONS ^ VR_CROP_FENCE;
pub const VR_CROP_INVERTED_CROSS: u32 = VR_CROP_ALL_REGIONS ^ VR_CROP_CROSS;
pub fn crop_region_keeps(flags: u32, point: Vec3, min: Vec3, max: Vec3) -> bool {
    if flags & !VR_CROP_ALL_REGIONS != 0 {
        return false;
    }
    let flags = if flags == 0 { VR_CROP_SUBVOLUME } else { flags };
    let axis = |value: f64, low: f64, high: f64| -> u32 {
        if value < low {
            0
        } else if value > high {
            2
        } else {
            1
        }
    };
    let region = axis(point.x, min.x, max.x)
        + 3 * axis(point.y, min.y, max.y)
        + 9 * axis(point.z, min.z, max.z);
    flags & (1 << region) != 0
}
#[derive(Debug, Clone, Copy, PartialEq)]
struct ClipPlane {
    normal: Vec3,
    d: f64,
    cut: bool,
}
const RAY_INTERVAL_SLACK: f64 = 1e-9;
#[derive(Debug, Clone, Default, PartialEq)]
pub struct VrClip {
    has_crop: bool,
    crop_min: Vec3,
    crop_max: Vec3,
    crop_region_flags: u32,
    planes: Vec<ClipPlane>,
    cap_opacity: f64,
    cap_window: WindowLevel,
    cut: Option<Arc<VrCutMask>>,
    labels: Option<Arc<VrLabelMap>>,
    geometry: Option<Arc<VrGeometry>>,
    pub(crate) full_rays: bool,
}
impl VrClip {
    pub fn new() -> VrClip {
        VrClip::default()
    }
    pub fn keep(&self, tex: Vec3) -> bool {
        if self.has_crop
            && !crop_region_keeps(self.crop_region_flags, tex, self.crop_min, self.crop_max)
        {
            return false;
        }
        for p in &self.planes {
            if tex.dot(p.normal) - p.d > 0.0 {
                return false;
            }
        }
        !self.cut.as_ref().is_some_and(|m| m.cut(tex))
    }
    pub fn set_cut(&mut self, mask: Option<Arc<VrCutMask>>) {
        self.cut = mask;
    }
    pub fn cut_mask(&self) -> Option<&Arc<VrCutMask>> {
        self.cut.as_ref()
    }
    pub fn set_labels(&mut self, labels: Option<Arc<VrLabelMap>>) {
        self.labels = labels;
    }
    pub fn labels(&self) -> Option<&Arc<VrLabelMap>> {
        self.labels.as_ref()
    }
    pub fn set_geometry(&mut self, geometry: Option<Arc<VrGeometry>>) {
        self.geometry = geometry;
    }
    pub fn geometry(&self) -> Option<&Arc<VrGeometry>> {
        self.geometry.as_ref()
    }
    pub(crate) fn active_labels(&self, vol: &Volume) -> Option<&VrLabelMap> {
        self.labels.as_deref().filter(|m| m.matches_volume(vol))
    }
    fn ray_interval(&self, o: Vec3, d: Vec3, box_min: Vec3, size: Vec3) -> (f64, f64, bool) {
        let (mut lo, mut hi) = (f64::NEG_INFINITY, f64::INFINITY);
        if self.full_rays {
            return (lo, hi, true);
        }
        let a = Vec3::new(
            (o.x - box_min.x) / size.x,
            (o.y - box_min.y) / size.y,
            (o.z - box_min.z) / size.z,
        );
        let b = Vec3::new(d.x / size.x, d.y / size.y, d.z / size.z);
        let mut limit = |base: f64, slope: f64| -> bool {
            if slope == 0.0 {
                return base <= RAY_INTERVAL_SLACK;
            }
            let t = (RAY_INTERVAL_SLACK - base) / slope;
            if slope > 0.0 {
                hi = fmath::min(hi, t);
            } else {
                lo = fmath::max(lo, t);
            }
            true
        };
        if self.has_crop && self.crop_region_flags() == VR_CROP_SUBVOLUME {
            for axis in [
                [a.x, b.x, self.crop_min.x, self.crop_max.x],
                [a.y, b.y, self.crop_min.y, self.crop_max.y],
                [a.z, b.z, self.crop_min.z, self.crop_max.z],
            ] {
                if axis[1] == 0.0 {
                    if axis[0] < axis[2] - RAY_INTERVAL_SLACK
                        || axis[0] > axis[3] + RAY_INTERVAL_SLACK
                    {
                        return (0.0, 0.0, false);
                    }
                    continue;
                }
                limit(axis[2] - axis[0], -axis[1]);
                limit(axis[0] - axis[3], axis[1]);
            }
        }
        for p in &self.planes {
            if !limit(a.dot(p.normal) - p.d, b.dot(p.normal)) {
                return (0.0, 0.0, false);
            }
        }
        (lo, hi, lo <= hi)
    }
    pub fn set_crop_box(&mut self, min: Vec3, max: Vec3) {
        self.crop_min = min;
        self.crop_max = max;
        self.has_crop = true;
    }
    pub fn clear_crop_box(&mut self) {
        self.has_crop = false;
    }
    pub fn set_crop_region_flags(&mut self, flags: u32) -> bool {
        if flags & !VR_CROP_ALL_REGIONS != 0 {
            return false;
        }
        self.crop_region_flags = flags;
        true
    }
    pub fn crop_region_flags(&self) -> u32 {
        if self.crop_region_flags == 0 {
            return VR_CROP_SUBVOLUME;
        }
        self.crop_region_flags
    }
    pub fn add_clip_plane(&mut self, normal: Vec3, d: f64) {
        if self.planes.len() >= 3 {
            return;
        }
        self.planes.push(ClipPlane {
            normal,
            d,
            cut: false,
        });
    }
    pub fn add_cut_plane(&mut self, normal: Vec3, d: f64) {
        if self.planes.len() >= 3 {
            return;
        }
        self.planes.push(ClipPlane {
            normal,
            d,
            cut: true,
        });
    }
    pub fn set_cap_opacity(&mut self, opacity: f64) {
        let opacity = if finite(opacity) { opacity } else { 0.0 };
        self.cap_opacity = clamp_unit(opacity);
    }
    pub fn set_cap_window(&mut self, window: WindowLevel) {
        self.cap_window = window;
    }
    pub(crate) fn cap_window_or(&self, window: &WindowLevel) -> WindowLevel {
        if self.cap_window.width > 0.0
            && finite(self.cap_window.width)
            && finite(self.cap_window.center)
        {
            return self.cap_window.clone();
        }
        window.clone()
    }
    pub fn cap_opacity(&self) -> f64 {
        self.cap_opacity
    }
    pub(crate) fn has_cut_planes(&self) -> bool {
        self.planes.iter().any(|p| p.cut)
    }
    pub(crate) fn cut_away(&self, tex: Vec3) -> bool {
        self.planes
            .iter()
            .any(|p| p.cut && tex.dot(p.normal) - p.d > 0.0)
    }
    pub(crate) fn caps_at(&self, previous: Vec3) -> bool {
        if self.cap_opacity <= 0.0 {
            return false;
        }
        if previous.x < 0.0
            || previous.y < 0.0
            || previous.z < 0.0
            || previous.x > 1.0
            || previous.y > 1.0
            || previous.z > 1.0
        {
            return false;
        }
        if self.has_crop
            && !crop_region_keeps(
                self.crop_region_flags,
                previous,
                self.crop_min,
                self.crop_max,
            )
        {
            return false;
        }
        self.cut_away(previous)
    }
    pub fn clear_planes(&mut self) {
        self.planes.clear();
    }
    pub fn empty(&self) -> bool {
        !self.has_crop && self.planes.is_empty() && self.cut.is_none()
    }
}
pub(crate) fn clip_keep(clip: Option<&VrClip>, tex: Vec3) -> bool {
    clip.is_none_or(|c| c.keep(tex))
}
#[allow(clippy::too_many_arguments)]
pub(crate) fn clip_ray(
    clip: Option<&VrClip>,
    o: Vec3,
    d: Vec3,
    box_min: Vec3,
    size: Vec3,
    t0: f64,
    t1: f64,
    step: f64,
) -> Option<(f64, f64)> {
    let (lo, hi, visible) = match clip {
        None => (f64::NEG_INFINITY, f64::INFINITY, true),
        Some(c) => c.ray_interval(o, d, box_min, size),
    };
    if !visible || lo > t1 || hi < t0 {
        return None;
    }
    let (mut start, end) = (t0, fmath::min(t1, hi));
    while start < lo {
        start += step;
    }
    if start <= end {
        Some((start, end))
    } else {
        None
    }
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VrClipPlaneView {
    pub normal: Vec3,
    pub d: f64,
    pub cut: bool,
}
impl VrClip {
    pub fn crop_box(&self) -> Option<(Vec3, Vec3)> {
        self.has_crop.then_some((self.crop_min, self.crop_max))
    }
    pub fn plane_views(&self) -> Vec<VrClipPlaneView> {
        self.planes
            .iter()
            .map(|p| VrClipPlaneView {
                normal: p.normal,
                d: p.d,
                cut: p.cut,
            })
            .collect()
    }
    pub fn effective_cap_window(&self, window: &WindowLevel) -> WindowLevel {
        self.cap_window_or(window)
    }
    pub fn full_rays(&self) -> bool {
        self.full_rays
    }
}
