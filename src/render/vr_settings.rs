#![forbid(unsafe_code)]
use super::vr_camera::VrCamera;
use super::vr_clip::VrClip;
use super::vr_quality::VrQuality;
use super::vr_transfer::VrPreset;
use super::window::WindowLevel;
#[derive(Debug, Clone, Copy)]
pub struct VrRenderSettings<'a> {
    pub camera: &'a VrCamera,
    pub preset: &'a VrPreset,
    pub window: &'a WindowLevel,
    pub shading: bool,
    pub clip: Option<&'a VrClip>,
    pub quality: &'a VrQuality,
}
impl<'a> VrRenderSettings<'a> {
    pub fn new(
        camera: &'a VrCamera,
        preset: &'a VrPreset,
        window: &'a WindowLevel,
        quality: &'a VrQuality,
    ) -> Self {
        VrRenderSettings {
            camera,
            preset,
            window,
            shading: false,
            clip: None,
            quality,
        }
    }
    pub fn with_shading(mut self, shading: bool) -> Self {
        self.shading = shading;
        self
    }
    pub fn with_clip(mut self, clip: Option<&'a VrClip>) -> Self {
        self.clip = clip;
        self
    }
}
#[derive(Debug, Clone, Copy)]
pub struct VrSurfaceSettings<'a> {
    pub camera: &'a VrCamera,
    pub iso: f64,
    pub color: [u8; 4],
    pub shading: bool,
    pub clip: Option<&'a VrClip>,
    pub quality: &'a VrQuality,
}
impl<'a> VrSurfaceSettings<'a> {
    pub fn new(camera: &'a VrCamera, iso: f64, color: [u8; 4], quality: &'a VrQuality) -> Self {
        VrSurfaceSettings {
            camera,
            iso,
            color,
            shading: false,
            clip: None,
            quality,
        }
    }
    pub fn with_shading(mut self, shading: bool) -> Self {
        self.shading = shading;
        self
    }
    pub fn with_clip(mut self, clip: Option<&'a VrClip>) -> Self {
        self.clip = clip;
        self
    }
}
