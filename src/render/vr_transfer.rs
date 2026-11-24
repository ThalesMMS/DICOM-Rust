#![forbid(unsafe_code)]
use super::vr_gradient_opacity::VrGradientOpacity;
use super::vr_projection::VrValueRange;
use super::vr_scattering::VrScattering;
use super::vr_transfer_2d::VrTransfer2d;
use super::window::{WindowLevel, windowed_unit};
use crate::fmath;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
pub(crate) fn finite(value: f64) -> bool {
    !value.is_nan() && !value.is_infinite()
}
pub(crate) fn clamp_unit(value: f64) -> f64 {
    if value < 0.0 || value.is_nan() {
        return 0.0;
    }
    if value > 1.0 || value.is_infinite() {
        return 1.0;
    }
    value
}
pub(crate) fn clamp_float(value: f64, min_value: f64, max_value: f64) -> f64 {
    fmath::max(min_value, fmath::min(max_value, value))
}
pub(crate) fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rgba {
    pub r: f64,
    pub g: f64,
    pub b: f64,
    pub a: f64,
}
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct TfColorStop {
    pub hu: f64,
    pub r: f64,
    pub g: f64,
    pub b: f64,
}
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct TfAlphaStop {
    pub hu: f64,
    pub a: f64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct VrMode(pub i64);
impl VrMode {
    pub const DVR: VrMode = VrMode(0);
    pub const MIP: VrMode = VrMode(1);
    pub const MIN_IP: VrMode = VrMode(2);
    pub const AVERAGE: VrMode = VrMode(3);
    pub const RAY_SUM: VrMode = VrMode(5);
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VrTransferDomain {
    #[default]
    Hu,
    Normalized,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VrColorMapping {
    #[default]
    ControlPoints,
    CtMuscle,
    LungAdvancedRgba,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VrOpacityMapping {
    #[default]
    ControlPoints,
    Linear,
    LogarithmicInverse,
    EmbeddedRgba,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VrColorSpace {
    #[default]
    Srgb,
    Linear,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct VrPrefilterId(pub u8);
impl VrPrefilterId {
    pub const NONE: VrPrefilterId = VrPrefilterId(0);
    pub const BASIC_SMOOTH_5X5: VrPrefilterId = VrPrefilterId(1);
    pub const GAUSSIAN_1MM: VrPrefilterId = VrPrefilterId(2);
    pub const MEDIAN_3X3X3: VrPrefilterId = VrPrefilterId(3);
    pub const VESSEL_ENHANCE: VrPrefilterId = VrPrefilterId(4);
    pub fn as_str(self) -> &'static str {
        match self.0 {
            0 => "none",
            1 => "basic-smooth-5x5",
            2 => "gaussian-1mm",
            3 => "median-3x3x3",
            4 => "vessel-enhance",
            _ => "unknown",
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct VrLightingMaterial {
    pub ambient: f64,
    pub diffuse: f64,
    pub specular: f64,
    pub specular_power: f64,
}
impl VrLightingMaterial {
    pub fn basic_clinical() -> VrLightingMaterial {
        VrLightingMaterial {
            ambient: 0.15,
            diffuse: 0.90,
            specular: 0.30,
            specular_power: 15.0,
        }
    }
    pub fn glossy_vascular() -> VrLightingMaterial {
        VrLightingMaterial {
            ambient: 0.15,
            diffuse: 0.28,
            specular: 1.42,
            specular_power: 50.0,
        }
    }
    pub fn legacy() -> VrLightingMaterial {
        VrLightingMaterial {
            ambient: 0.3,
            diffuse: 0.7,
            specular: 0.3,
            specular_power: 20.0,
        }
    }
    pub fn valid(&self) -> bool {
        [
            self.ambient,
            self.diffuse,
            self.specular,
            self.specular_power,
        ]
        .iter()
        .all(|v| finite(*v) && *v >= 0.0)
    }
}
pub const DEFAULT_VR_OPACITY_UNIT_DISTANCE_MM: f64 = 1.0;
pub const CLINICAL_VR_LUT_SIZE: i64 = 4096;
type LutKey = (u64, u64, i64);
#[derive(Debug, Clone)]
pub struct VrTransferFunction {
    domain: VrTransferDomain,
    color_mapping: VrColorMapping,
    opacity_mapping: VrOpacityMapping,
    color_space: VrColorSpace,
    opacity_unit_distance_mm: f64,
    opacity_scale: f64,
    opacity_scale_set: bool,
    color: Vec<TfColorStop>,
    alpha: Vec<TfAlphaStop>,
    cache: Option<Arc<Mutex<HashMap<LutKey, VrLut>>>>,
}
impl Default for VrTransferFunction {
    fn default() -> Self {
        VrTransferFunction {
            domain: VrTransferDomain::Hu,
            color_mapping: VrColorMapping::ControlPoints,
            opacity_mapping: VrOpacityMapping::ControlPoints,
            color_space: VrColorSpace::Srgb,
            opacity_unit_distance_mm: 0.0,
            opacity_scale: 0.0,
            opacity_scale_set: false,
            color: Vec::new(),
            alpha: Vec::new(),
            cache: None,
        }
    }
}
fn new_cache() -> Option<Arc<Mutex<HashMap<LutKey, VrLut>>>> {
    Some(Arc::new(Mutex::new(HashMap::new())))
}
fn sort_color_stops(stops: &mut [TfColorStop]) {
    for i in 1..stops.len() {
        let mut j = i;
        while j > 0 && stops[j - 1].hu > stops[j].hu {
            stops.swap(j - 1, j);
            j -= 1;
        }
    }
}
fn sort_alpha_stops(stops: &mut [TfAlphaStop]) {
    for i in 1..stops.len() {
        let mut j = i;
        while j > 0 && stops[j - 1].hu > stops[j].hu {
            stops.swap(j - 1, j);
            j -= 1;
        }
    }
}
fn color_at_stops(stops: &[TfColorStop], value: f64) -> (f64, f64, f64) {
    let n = stops.len();
    if n == 0 {
        return (1.0, 1.0, 1.0);
    }
    if value <= stops[0].hu {
        return (stops[0].r, stops[0].g, stops[0].b);
    }
    if value >= stops[n - 1].hu {
        return (stops[n - 1].r, stops[n - 1].g, stops[n - 1].b);
    }
    for i in 1..n {
        if value <= stops[i].hu {
            let (lower, upper) = (stops[i - 1], stops[i]);
            let fraction = (value - lower.hu) / (upper.hu - lower.hu);
            return (
                lerp(lower.r, upper.r, fraction),
                lerp(lower.g, upper.g, fraction),
                lerp(lower.b, upper.b, fraction),
            );
        }
    }
    (stops[n - 1].r, stops[n - 1].g, stops[n - 1].b)
}
pub fn vr_linear_opacity(t: f64) -> f64 {
    clamp_unit(t)
}
pub fn vr_logarithmic_inverse_opacity(t: f64) -> f64 {
    let t = clamp_unit(t);
    clamp_unit(1.0 - fmath::log10(10.0 - 9.0 * t))
}
pub(crate) fn srgb_to_linear(value: f64) -> f64 {
    let value = clamp_unit(value);
    if value <= 0.04045 {
        return value / 12.92;
    }
    fmath::pow((value + 0.055) / 1.055, 2.4)
}
pub(crate) fn linear_to_srgb(value: f64) -> f64 {
    let value = clamp_unit(value);
    if value <= 0.0031308 {
        return 12.92 * value;
    }
    1.055 * fmath::pow(value, 0.4166666666666667) - 0.055
}
impl VrTransferFunction {
    pub fn new(color: &[TfColorStop], alpha: &[TfAlphaStop]) -> VrTransferFunction {
        VrTransferFunction::build(
            VrTransferDomain::Hu,
            VrColorMapping::ControlPoints,
            VrOpacityMapping::ControlPoints,
            VrColorSpace::Srgb,
            color,
            alpha,
            DEFAULT_VR_OPACITY_UNIT_DISTANCE_MM,
        )
    }
    pub fn new_normalized(
        color: &[TfColorStop],
        alpha: &[TfAlphaStop],
        opacity_unit_distance_mm: f64,
    ) -> VrTransferFunction {
        VrTransferFunction::build(
            VrTransferDomain::Normalized,
            VrColorMapping::ControlPoints,
            VrOpacityMapping::ControlPoints,
            VrColorSpace::Srgb,
            color,
            alpha,
            opacity_unit_distance_mm,
        )
    }
    pub fn new_ct_muscle(opacity: VrOpacityMapping) -> VrTransferFunction {
        let opacity = match opacity {
            VrOpacityMapping::Linear | VrOpacityMapping::LogarithmicInverse => opacity,
            _ => VrOpacityMapping::LogarithmicInverse,
        };
        VrTransferFunction::build(
            VrTransferDomain::Normalized,
            VrColorMapping::CtMuscle,
            opacity,
            VrColorSpace::Srgb,
            SLICER_CT_MUSCLE.color,
            &[],
            DEFAULT_VR_OPACITY_UNIT_DISTANCE_MM,
        )
    }
    pub fn new_lung_advanced() -> VrTransferFunction {
        let (low, high) = LUNG_WINDOW_HU;
        let position = |hu: f64| (hu - low) / (high - low);
        let colors: Vec<TfColorStop> = LUNG_COLOR_POINTS_HU
            .iter()
            .map(|&(hu, r, g, b)| TfColorStop {
                hu: position(hu),
                r,
                g,
                b,
            })
            .collect();
        let alpha: Vec<TfAlphaStop> = LUNG_OPACITY_POINTS_HU
            .iter()
            .map(|&(hu, a)| TfAlphaStop {
                hu: position(hu),
                a,
            })
            .collect();
        VrTransferFunction::build(
            VrTransferDomain::Normalized,
            VrColorMapping::LungAdvancedRgba,
            VrOpacityMapping::EmbeddedRgba,
            VrColorSpace::Srgb,
            &colors,
            &alpha,
            DEFAULT_VR_OPACITY_UNIT_DISTANCE_MM,
        )
    }
    fn build(
        domain: VrTransferDomain,
        color_mapping: VrColorMapping,
        opacity_mapping: VrOpacityMapping,
        color_space: VrColorSpace,
        color: &[TfColorStop],
        alpha: &[TfAlphaStop],
        opacity_unit_distance_mm: f64,
    ) -> VrTransferFunction {
        let mut c = color.to_vec();
        let mut a = alpha.to_vec();
        sort_color_stops(&mut c);
        sort_alpha_stops(&mut a);
        let distance = if !finite(opacity_unit_distance_mm) || opacity_unit_distance_mm <= 0.0 {
            DEFAULT_VR_OPACITY_UNIT_DISTANCE_MM
        } else {
            opacity_unit_distance_mm
        };
        VrTransferFunction {
            domain,
            color_mapping,
            opacity_mapping,
            color_space,
            opacity_unit_distance_mm: distance,
            opacity_scale: 0.0,
            opacity_scale_set: false,
            color: c,
            alpha: a,
            cache: new_cache(),
        }
    }
    pub fn cloned(&self) -> VrTransferFunction {
        let mut tf = self.clone();
        if tf.cache.is_none() {
            tf.cache = new_cache();
        }
        tf
    }
    pub fn domain(&self) -> VrTransferDomain {
        self.domain
    }
    pub fn color_mapping(&self) -> VrColorMapping {
        self.color_mapping
    }
    pub fn opacity_mapping(&self) -> VrOpacityMapping {
        self.opacity_mapping
    }
    pub fn color_space(&self) -> VrColorSpace {
        self.color_space
    }
    pub fn opacity_unit_distance_mm(&self) -> f64 {
        if !finite(self.opacity_unit_distance_mm) || self.opacity_unit_distance_mm <= 0.0 {
            return DEFAULT_VR_OPACITY_UNIT_DISTANCE_MM;
        }
        self.opacity_unit_distance_mm
    }
    pub fn opacity_scale(&self) -> f64 {
        if !self.opacity_scale_set {
            return 1.0;
        }
        self.opacity_scale
    }
    pub fn with_opacity_scale(&self, scale: f64) -> VrTransferFunction {
        let mut tf = self.clone();
        tf.opacity_scale = if !finite(scale) || scale < 0.0 {
            1.0
        } else {
            scale
        };
        tf.opacity_scale_set = true;
        tf.cache = new_cache();
        tf
    }
    pub fn with_opacity_unit_distance_mm(&self, distance: f64) -> VrTransferFunction {
        let mut tf = self.clone();
        tf.opacity_unit_distance_mm = if !finite(distance) || distance <= 0.0 {
            DEFAULT_VR_OPACITY_UNIT_DISTANCE_MM
        } else {
            distance
        };
        tf.cache = new_cache();
        tf
    }
    pub fn with_control_points(
        &self,
        color: &[TfColorStop],
        alpha: &[TfAlphaStop],
    ) -> VrTransferFunction {
        let result = VrTransferFunction::build(
            self.domain,
            VrColorMapping::ControlPoints,
            VrOpacityMapping::ControlPoints,
            self.color_space,
            color,
            alpha,
            self.opacity_unit_distance_mm(),
        );
        if self.opacity_scale_set {
            return result.with_opacity_scale(self.opacity_scale);
        }
        result
    }
    pub fn with_color_stops(&self, color: &[TfColorStop]) -> VrTransferFunction {
        let mut tf = self.clone();
        tf.color = color.to_vec();
        sort_color_stops(&mut tf.color);
        tf.color_mapping = VrColorMapping::ControlPoints;
        tf.cache = new_cache();
        tf
    }
    pub fn color_stops(&self) -> Vec<TfColorStop> {
        self.color.clone()
    }
    pub fn alpha_stops(&self) -> Vec<TfAlphaStop> {
        match self.opacity_mapping {
            VrOpacityMapping::Linear => vec![
                TfAlphaStop { hu: 0.0, a: 0.0 },
                TfAlphaStop { hu: 1.0, a: 1.0 },
            ],
            VrOpacityMapping::LogarithmicInverse => (0..8)
                .map(|index| {
                    let t = index as f64 / 7.0;
                    TfAlphaStop {
                        hu: t,
                        a: vr_logarithmic_inverse_opacity(t),
                    }
                })
                .collect(),
            _ => self.alpha.clone(),
        }
    }
    pub fn color_at(&self, value: f64) -> (f64, f64, f64) {
        color_at_stops(&self.color, value)
    }
    pub fn alpha_at(&self, value: f64) -> f64 {
        match self.opacity_mapping {
            VrOpacityMapping::Linear => return vr_linear_opacity(value),
            VrOpacityMapping::LogarithmicInverse => return vr_logarithmic_inverse_opacity(value),
            _ => {}
        }
        let alpha = &self.alpha;
        let n = alpha.len();
        if n == 0 {
            return 0.0;
        }
        if value <= alpha[0].hu {
            return alpha[0].a;
        }
        if value >= alpha[n - 1].hu {
            return alpha[n - 1].a;
        }
        for i in 1..n {
            if value <= alpha[i].hu {
                let (lower, upper) = (alpha[i - 1], alpha[i]);
                let fraction = (value - lower.hu) / (upper.hu - lower.hu);
                return lerp(lower.a, upper.a, fraction);
            }
        }
        alpha[n - 1].a
    }
    pub fn bake_lut(&self, domain_min: f64, domain_max: f64, count: i64) -> VrLut {
        let count = count.max(2);
        let domain_min = if finite(domain_min) { domain_min } else { 0.0 };
        let domain_max = if !finite(domain_max) || domain_max <= domain_min {
            domain_min + 1.0
        } else {
            domain_max
        };
        let bits = |v: f64| if v == 0.0 { 0u64 } else { v.to_bits() };
        let key = (bits(domain_min), bits(domain_max), count);
        if let Some(cache) = &self.cache
            && let Ok(entries) = cache.lock()
            && let Some(cached) = entries.get(&key)
        {
            return cached.clone();
        }
        let mut linear_color = self.color.clone();
        for stop in &mut linear_color {
            if self.color_space == VrColorSpace::Srgb {
                stop.r = srgb_to_linear(stop.r);
                stop.g = srgb_to_linear(stop.g);
                stop.b = srgb_to_linear(stop.b);
            } else {
                stop.r = clamp_unit(stop.r);
                stop.g = clamp_unit(stop.g);
                stop.b = clamp_unit(stop.b);
            }
        }
        let entries: Vec<Rgba> = (0..count)
            .map(|index| {
                let value =
                    domain_min + index as f64 / (count - 1) as f64 * (domain_max - domain_min);
                let (r, g, b) = color_at_stops(&linear_color, value);
                Rgba {
                    r,
                    g,
                    b,
                    a: clamp_unit(self.alpha_at(value) * self.opacity_scale()),
                }
            })
            .collect();
        let lut = VrLut {
            entries: Arc::new(entries),
            min: domain_min,
            max: domain_max,
        };
        if let Some(cache) = &self.cache
            && let Ok(mut entries) = cache.lock()
        {
            return entries.entry(key).or_insert(lut).clone();
        }
        lut
    }
    pub fn bake_window_lut(&self, window: &WindowLevel, count: i64) -> VrLut {
        if self.domain == VrTransferDomain::Normalized {
            return self.bake_lut(0.0, 1.0, count);
        }
        let (lower, upper) = vr_window_bounds(window);
        self.bake_lut(lower, upper, count)
    }
    pub fn preferred_lut_size(&self) -> i64 {
        if self.domain == VrTransferDomain::Normalized {
            return CLINICAL_VR_LUT_SIZE;
        }
        1024
    }
    pub(crate) fn window_coordinate(&self, value: f64, window: &WindowLevel) -> f64 {
        if self.domain == VrTransferDomain::Normalized {
            return vr_window_coordinate(value, window);
        }
        windowed_unit(value, window)
    }
}
#[derive(Debug, Clone, Default)]
pub struct VrLut {
    entries: Arc<Vec<Rgba>>,
    min: f64,
    max: f64,
}
impl VrLut {
    pub fn lookup(&self, t: f64) -> Rgba {
        let entries = &self.entries;
        let n = entries.len();
        if n == 0 {
            return Rgba::default();
        }
        if !finite(t) || t <= 0.0 {
            return entries[0];
        }
        if t >= 1.0 {
            return entries[n - 1];
        }
        let position = t * (n - 1) as f64;
        let lower = position.floor() as usize;
        let upper = (lower + 1).min(n - 1);
        let fraction = position - lower as f64;
        let (a, b) = (entries[lower], entries[upper]);
        Rgba {
            r: lerp(a.r, b.r, fraction),
            g: lerp(a.g, b.g, fraction),
            b: lerp(a.b, b.b, fraction),
            a: lerp(a.a, b.a, fraction),
        }
    }
    pub fn at(&self, index: i64) -> Rgba {
        if index < 0 || index as usize >= self.entries.len() {
            return Rgba::default();
        }
        self.entries[index as usize]
    }
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    pub fn domain(&self) -> (f64, f64) {
        (self.min, self.max)
    }
    pub fn same_table(&self, other: &VrLut) -> bool {
        if self.entries.len() != other.entries.len()
            || self.min != other.min
            || self.max != other.max
        {
            return false;
        }
        self.entries.is_empty() || Arc::ptr_eq(&self.entries, &other.entries)
    }
}
pub fn vr_window_bounds(window: &WindowLevel) -> (f64, f64) {
    if !finite(window.center) || !finite(window.width) || window.width <= 0.0 {
        return (0.0, 1.0);
    }
    let lower = window.center - window.width / 2.0;
    let upper = window.center + window.width / 2.0;
    if !finite(lower) || !finite(upper) || upper <= lower {
        return (0.0, 1.0);
    }
    (lower, upper)
}
pub fn vr_window_coordinate(value: f64, window: &WindowLevel) -> f64 {
    let (lower, upper) = vr_window_bounds(window);
    clamp_unit((value - lower) / (upper - lower))
}
pub fn correct_vr_alpha(
    sample_alpha: f64,
    opacity_scale: f64,
    sample_distance_mm: f64,
    opacity_unit_distance_mm: f64,
) -> f64 {
    let opacity_scale = if !finite(opacity_scale) || opacity_scale < 0.0 {
        1.0
    } else {
        opacity_scale
    };
    if !finite(sample_distance_mm) || sample_distance_mm <= 0.0 {
        return 0.0;
    }
    let unit = if !finite(opacity_unit_distance_mm) || opacity_unit_distance_mm <= 0.0 {
        DEFAULT_VR_OPACITY_UNIT_DISTANCE_MM
    } else {
        opacity_unit_distance_mm
    };
    let base = clamp_unit(sample_alpha * opacity_scale);
    if base <= 0.0 || base >= 1.0 {
        return base;
    }
    clamp_unit(1.0 - fmath::pow(1.0 - base, sample_distance_mm / unit))
}
pub fn vr_mip_color(lut: &VrLut, modality: f64, inverse: bool) -> Rgba {
    let (low, high) = lut.domain();
    let t = if high > low {
        (modality - low) / (high - low)
    } else {
        0.0
    };
    let mut mapped = lut.lookup(t);
    if inverse {
        mapped.r = srgb_to_linear(1.0 - linear_to_srgb(mapped.r));
        mapped.g = srgb_to_linear(1.0 - linear_to_srgb(mapped.g));
        mapped.b = srgb_to_linear(1.0 - linear_to_srgb(mapped.b));
    }
    mapped.a = 1.0;
    mapped
}
#[derive(Debug, Clone, Default)]
pub struct VrPreset {
    pub name: String,
    pub tf: VrTransferFunction,
    pub mode: VrMode,
    pub shading_default: bool,
    pub inverse: bool,
    pub lighting_material: VrLightingMaterial,
    pub lighting_material_set: bool,
    pub opacity_normals: bool,
    pub prefilter: VrPrefilterId,
    pub background: Rgba,
    pub gradient_opacity_scale: f64,
    pub gradient_opacity: VrGradientOpacity,
    pub scattering: VrScattering,
    pub projection_range: VrValueRange,
    pub colored_mip: bool,
    pub transfer_2d: Option<Arc<VrTransfer2d>>,
}
impl VrPreset {
    pub fn effective_lighting_material(&self) -> VrLightingMaterial {
        if self.lighting_material_set && self.lighting_material.valid() {
            return self.lighting_material;
        }
        VrLightingMaterial::legacy()
    }
    pub fn effective_background(&self) -> Rgba {
        let bg = self.background;
        let unit = |v: f64| finite(v) && (0.0..=1.0).contains(&v);
        if unit(bg.r) && unit(bg.g) && unit(bg.b) && bg.a == 1.0 {
            return bg;
        }
        Rgba {
            a: 1.0,
            ..Default::default()
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SlicerVrPreset {
    pub name: &'static str,
    pub window_hu: (f64, f64),
    pub color: &'static [TfColorStop],
    pub alpha: &'static [TfAlphaStop],
    pub lighting: VrLightingMaterial,
}
impl SlicerVrPreset {
    pub fn transfer_function(&self) -> VrTransferFunction {
        VrTransferFunction::new_normalized(
            self.color,
            self.alpha,
            DEFAULT_VR_OPACITY_UNIT_DISTANCE_MM,
        )
    }
    pub fn window(&self) -> (f64, f64) {
        let (low, high) = self.window_hu;
        ((low + high) / 2.0, high - low)
    }
}
pub const SLICER_CT_CHEST_CONTRAST_ENHANCED: SlicerVrPreset = SlicerVrPreset {
    name: "CT-Chest-Contrast-Enhanced",
    window_hu: (67.0106, 3071.0),
    color: &[
        TfColorStop {
            hu: 0.0,
            r: 0.54902,
            g: 0.25098,
            b: 0.14902,
        },
        TfColorStop {
            hu: 0.061283305,
            r: 0.882353,
            g: 0.603922,
            b: 0.290196,
        },
        TfColorStop {
            hu: 0.123928666,
            r: 1.0,
            g: 0.937033,
            b: 0.954531,
        },
        TfColorStop {
            hu: 1.0,
            r: 0.827451,
            g: 0.658824,
            b: 1.0,
        },
    ],
    alpha: &[
        TfAlphaStop { hu: 0.0, a: 0.0 },
        TfAlphaStop {
            hu: 0.061283305,
            a: 0.446429,
        },
        TfAlphaStop {
            hu: 0.123928666,
            a: 0.625,
        },
        TfAlphaStop {
            hu: 1.0,
            a: 0.616071,
        },
    ],
    lighting: VrLightingMaterial {
        ambient: 0.1,
        diffuse: 0.9,
        specular: 0.2,
        specular_power: 10.0,
    },
};
pub const SLICER_CT_CORONARY_ARTERIES_3: SlicerVrPreset = SlicerVrPreset {
    name: "CT-Coronary-Arteries-3",
    window_hu: (128.643, 3661.0),
    color: &[
        TfColorStop {
            hu: 0.0,
            r: 0.0,
            g: 0.0,
            b: 0.0,
        },
        TfColorStop {
            hu: 0.000379067,
            r: 0.615686,
            g: 0.0,
            b: 0.0156863,
        },
        TfColorStop {
            hu: 0.012737388,
            r: 0.909804,
            g: 0.454902,
            b: 0.0,
        },
        TfColorStop {
            hu: 0.036021557,
            r: 0.886275,
            g: 0.886275,
            b: 0.886275,
        },
        TfColorStop {
            hu: 0.129158802,
            r: 0.968627,
            g: 0.968627,
            b: 0.968627,
        },
        TfColorStop {
            hu: 1.0,
            r: 1.0,
            g: 1.0,
            b: 1.0,
        },
    ],
    alpha: &[
        TfAlphaStop { hu: 0.0, a: 0.0 },
        TfAlphaStop {
            hu: 0.000379067,
            a: 0.0982143,
        },
        TfAlphaStop {
            hu: 0.012737388,
            a: 0.669643,
        },
        TfAlphaStop {
            hu: 0.036021557,
            a: 0.857143,
        },
        TfAlphaStop {
            hu: 0.129158802,
            a: 0.866071,
        },
        TfAlphaStop { hu: 1.0, a: 1.0 },
    ],
    lighting: VrLightingMaterial {
        ambient: 0.1,
        diffuse: 0.9,
        specular: 0.2,
        specular_power: 10.0,
    },
};
pub const SLICER_CT_MUSCLE: SlicerVrPreset = SlicerVrPreset {
    name: "CT-Muscle",
    window_hu: (-155.407, 3071.0),
    color: &[
        TfColorStop {
            hu: 0.0,
            r: 0.54902,
            g: 0.25098,
            b: 0.14902,
        },
        TfColorStop {
            hu: 0.115623354,
            r: 0.882353,
            g: 0.603922,
            b: 0.290196,
        },
        TfColorStop {
            hu: 0.178261143,
            r: 1.0,
            g: 0.937033,
            b: 0.954531,
        },
        TfColorStop {
            hu: 1.0,
            r: 0.827451,
            g: 0.658824,
            b: 1.0,
        },
    ],
    alpha: &[
        TfAlphaStop { hu: 0.0, a: 0.0 },
        TfAlphaStop {
            hu: 0.115623354,
            a: 0.676471,
        },
        TfAlphaStop {
            hu: 0.178261143,
            a: 0.833333,
        },
        TfAlphaStop {
            hu: 1.0,
            a: 0.803922,
        },
    ],
    lighting: VrLightingMaterial {
        ambient: 0.1,
        diffuse: 0.9,
        specular: 0.2,
        specular_power: 10.0,
    },
};
pub fn slicer_vr_presets() -> [SlicerVrPreset; 3] {
    [
        SLICER_CT_CHEST_CONTRAST_ENHANCED,
        SLICER_CT_CORONARY_ARTERIES_3,
        SLICER_CT_MUSCLE,
    ]
}
pub const LUNG_WINDOW_HU: (f64, f64) = (-1024.0, -200.0);
const LUNG_COLOR_POINTS_HU: [(f64, f64, f64, f64); 5] = [
    (-1024.0, 0.05, 0.25, 0.75),
    (-900.0, 0.10, 0.55, 0.95),
    (-780.0, 0.30, 0.85, 1.00),
    (-600.0, 0.85, 0.97, 1.00),
    (-200.0, 1.00, 1.00, 1.00),
];
const LUNG_OPACITY_POINTS_HU: [(f64, f64); 7] = [
    (-1024.0, 0.0),
    (-940.0, 0.0),
    (-860.0, 0.12),
    (-760.0, 0.25),
    (-620.0, 0.12),
    (-480.0, 0.0),
    (-200.0, 0.0),
];
pub const PRESET_ANGIO: &str = "Angio";
pub const PRESET_AIRWAYS: &str = "Airways";
pub const PRESET_BONES_SKIN_1: &str = "Bones and skin 1";
pub const PRESET_BONES_SKIN_2: &str = "Bones and skin 2";
pub const PRESET_BONES_SKIN_3: &str = "Bones and skin 3";
pub const PRESET_BONES_BW: &str = "Bones B/W";
pub const PRESET_SKIN_BW: &str = "Skin B/W";
pub const PRESET_MIP_BW: &str = "MIP B/W";
pub const PRESET_MIP_BW_INVERSE: &str = "MIP B/W Inverse";
fn cs(hu: f64, r: f64, g: f64, b: f64) -> TfColorStop {
    TfColorStop { hu, r, g, b }
}
fn als(hu: f64, a: f64) -> TfAlphaStop {
    TfAlphaStop { hu, a }
}
fn bone_color() -> [TfColorStop; 4] {
    [
        cs(-1000.0, 0.0, 0.0, 0.0),
        cs(300.0, 0.72, 0.48, 0.38),
        cs(700.0, 0.92, 0.82, 0.72),
        cs(1800.0, 1.0, 0.98, 0.92),
    ]
}
fn angio_preset() -> VrPreset {
    VrPreset {
        name: PRESET_ANGIO.into(),
        tf: VrTransferFunction::new(
            &[
                cs(100.0, 0.5, 0.0, 0.0),
                cs(250.0, 0.9, 0.3, 0.1),
                cs(450.0, 1.0, 0.7, 0.4),
                cs(700.0, 1.0, 1.0, 1.0),
            ],
            &[als(150.0, 0.0), als(250.0, 0.25), als(500.0, 0.85)],
        ),
        mode: VrMode::DVR,
        shading_default: true,
        ..Default::default()
    }
}
fn airways_preset() -> VrPreset {
    VrPreset {
        name: PRESET_AIRWAYS.into(),
        tf: VrTransferFunction::new(
            &[
                cs(-1000.0, 0.0, 0.0, 0.0),
                cs(-600.0, 0.4, 0.6, 0.9),
                cs(350.0, 1.0, 0.96, 0.88),
            ],
            &[
                als(-900.0, 0.05),
                als(-600.0, 0.18),
                als(-200.0, 0.05),
                als(350.0, 0.3),
            ],
        ),
        mode: VrMode::DVR,
        ..Default::default()
    }
}
fn bones_skin_preset(name: &str, skin_alpha: f64) -> VrPreset {
    let mut color = vec![cs(-200.0, 0.85, 0.7, 0.6)];
    color.extend_from_slice(&bone_color());
    let alpha = [
        als(-250.0, 0.0),
        als(-150.0, skin_alpha),
        als(100.0, skin_alpha * 0.6),
        als(150.0, 0.05),
        als(300.0, 0.4),
        als(700.0, 0.75),
        als(1800.0, 0.95),
    ];
    VrPreset {
        name: name.into(),
        tf: VrTransferFunction::new(&color, &alpha),
        mode: VrMode::DVR,
        shading_default: true,
        gradient_opacity_scale: 220.0,
        ..Default::default()
    }
}
fn bones_bw_preset() -> VrPreset {
    VrPreset {
        name: PRESET_BONES_BW.into(),
        tf: VrTransferFunction::new(
            &[
                cs(150.0, 0.2, 0.2, 0.2),
                cs(700.0, 0.8, 0.8, 0.8),
                cs(1800.0, 1.0, 1.0, 1.0),
            ],
            &[
                als(150.0, 0.0),
                als(300.0, 0.2),
                als(700.0, 0.65),
                als(1800.0, 0.95),
            ],
        ),
        mode: VrMode::DVR,
        shading_default: true,
        ..Default::default()
    }
}
fn skin_bw_preset() -> VrPreset {
    VrPreset {
        name: PRESET_SKIN_BW.into(),
        tf: VrTransferFunction::new(
            &[
                cs(-200.0, 0.3, 0.3, 0.3),
                cs(100.0, 0.8, 0.8, 0.8),
                cs(400.0, 1.0, 1.0, 1.0),
            ],
            &[
                als(-250.0, 0.0),
                als(-150.0, 0.15),
                als(100.0, 0.35),
                als(400.0, 0.6),
            ],
        ),
        mode: VrMode::DVR,
        gradient_opacity_scale: 180.0,
        ..Default::default()
    }
}
fn mip_bw_preset(name: &str, inverse: bool) -> VrPreset {
    VrPreset {
        name: name.into(),
        tf: VrTransferFunction::new(
            &[cs(0.0, 1.0, 1.0, 1.0), cs(1.0, 1.0, 1.0, 1.0)],
            &[als(0.0, 1.0), als(1.0, 1.0)],
        ),
        mode: VrMode::MIP,
        inverse,
        ..Default::default()
    }
}
pub fn vr_presets() -> Vec<VrPreset> {
    vec![
        angio_preset(),
        airways_preset(),
        bones_skin_preset(PRESET_BONES_SKIN_1, 0.05),
        bones_skin_preset(PRESET_BONES_SKIN_2, 0.12),
        bones_skin_preset(PRESET_BONES_SKIN_3, 0.22),
        bones_bw_preset(),
        skin_bw_preset(),
        mip_bw_preset(PRESET_MIP_BW, false),
        mip_bw_preset(PRESET_MIP_BW_INVERSE, true),
    ]
}
pub fn vr_preset_by_name(name: &str) -> Option<VrPreset> {
    vr_presets().into_iter().find(|p| p.name == name)
}
pub fn default_vr_preset() -> VrPreset {
    bones_skin_preset(PRESET_BONES_SKIN_1, 0.05)
}
pub fn source_rgb_vr_preset() -> VrPreset {
    let mut p = default_vr_preset();
    p.name = "Source RGB".into();
    p.mode = VrMode::DVR;
    p.tf = VrTransferFunction::new_normalized(
        &[cs(0.0, 0.0, 0.0, 0.0), cs(1.0, 1.0, 1.0, 1.0)],
        &[als(0.0, 0.0), als(1.0, 1.0)],
        1.0,
    );
    p.gradient_opacity_scale = 0.0;
    p.gradient_opacity = VrGradientOpacity::default();
    p.scattering = VrScattering::default();
    p.opacity_normals = true;
    p
}
