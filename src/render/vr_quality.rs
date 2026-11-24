#![forbid(unsafe_code)]
use super::vr_transfer::{Rgba, clamp_unit, finite};
use crate::fmath;
use crate::numtext::format_float;
use std::fmt;
use std::sync::Mutex;
pub const DEFAULT_VR_MAX_STEPS: i64 = 4096;
pub const DEFAULT_VR_SPACING_FRACTION: f64 = 0.5;
pub const DEFAULT_VR_ADAPTIVE_STEP_THRESHOLD: f64 = 0.02;
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct VrAdaptiveStep {
    pub max: f64,
    pub min: f64,
    pub threshold: f64,
}
impl VrAdaptiveStep {
    pub fn default_settings() -> VrAdaptiveStep {
        VrAdaptiveStep {
            max: 4.0,
            min: 1.0,
            threshold: DEFAULT_VR_ADAPTIVE_STEP_THRESHOLD,
        }
    }
    pub fn enabled(&self) -> bool {
        finite(self.max) && self.max > 1.0
    }
    pub fn normalized(&self) -> VrAdaptiveStep {
        let mut a = *self;
        if !finite(a.max) || a.max < 1.0 {
            a.max = 1.0;
        }
        a.max = fmath::min(a.max, 16.0);
        if !finite(a.min) || a.min <= 0.0 || a.min > 1.0 {
            a.min = 1.0;
        }
        if !finite(a.threshold) || a.threshold <= 0.0 {
            a.threshold = DEFAULT_VR_ADAPTIVE_STEP_THRESHOLD;
        }
        a
    }
}
#[derive(Debug, Clone, Default)]
pub(crate) struct VrAdaptiveStepper {
    pub(crate) enabled: bool,
    pub(crate) samples: i64,
    base: f64,
    min_step: f64,
    max_step: f64,
    threshold: f64,
    previous: Rgba,
    t_previous: f64,
    has_previous: bool,
    refine_until: f64,
}
fn vr_classified_change(a: Rgba, b: Rgba) -> f64 {
    let weight = fmath::max(a.a, b.a);
    let mut change = (a.a - b.a).abs();
    change = fmath::max(change, weight * (a.r - b.r).abs());
    change = fmath::max(change, weight * (a.g - b.g).abs());
    fmath::max(change, weight * (a.b - b.b).abs())
}
#[allow(dead_code)]
impl VrAdaptiveStepper {
    pub(crate) fn new(settings: VrAdaptiveStep, base: f64) -> VrAdaptiveStepper {
        if !settings.enabled() || !(base > 0.0) {
            return VrAdaptiveStepper {
                base,
                ..Default::default()
            };
        }
        let settings = settings.normalized();
        VrAdaptiveStepper {
            enabled: true,
            base,
            min_step: base * settings.min,
            max_step: base * settings.max,
            threshold: settings.threshold,
            ..Default::default()
        }
    }
    pub(crate) fn begin_ray(&mut self) {
        self.has_previous = false;
        self.refine_until = f64::NEG_INFINITY;
    }
    pub(crate) fn reset(&mut self) {
        self.has_previous = false;
    }
    pub(crate) fn observe(&mut self, t: f64, rgba: Rgba) -> (bool, f64, f64, f64) {
        self.samples += 1;
        if !self.enabled {
            return (true, self.base, self.base, t);
        }
        if !self.has_previous {
            self.previous = rgba;
            self.t_previous = t;
            self.has_previous = true;
            return (true, self.base, self.base, t);
        }
        let mut taken = t - self.t_previous;
        if !(taken > 0.0) {
            taken = self.base;
        }
        let rate = vr_classified_change(self.previous, rgba) * self.base / taken;
        let fraction = clamp_unit(rate / self.threshold);
        let mut next = self.max_step + (self.min_step - self.max_step) * fraction;
        if taken > self.base * (1.0 + 1e-9) && next <= self.base {
            self.refine_until = t;
            return (false, 0.0, self.min_step, self.t_previous);
        }
        if t < self.refine_until {
            next = self.min_step;
        }
        self.previous = rgba;
        self.t_previous = t;
        (true, taken, next, t)
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct VrQuality {
    pub width: i64,
    pub height: i64,
    pub max_steps: i64,
    pub spacing_fraction: f64,
    pub jitter: bool,
    pub scattering: bool,
    pub adaptive_step: VrAdaptiveStep,
}
impl VrQuality {
    pub fn default_quality(w: i64, h: i64) -> VrQuality {
        VrQuality {
            width: w,
            height: h,
            max_steps: DEFAULT_VR_MAX_STEPS,
            spacing_fraction: DEFAULT_VR_SPACING_FRACTION,
            jitter: true,
            scattering: true,
            adaptive_step: VrAdaptiveStep::default(),
        }
    }
    pub fn preview(w: i64, h: i64) -> VrQuality {
        VrQuality {
            width: w,
            height: h,
            max_steps: DEFAULT_VR_MAX_STEPS,
            spacing_fraction: 2.0,
            ..Default::default()
        }
    }
    pub fn adaptive_preview(w: i64, h: i64) -> VrQuality {
        let mut q = VrQuality::preview(w, h);
        q.spacing_fraction /= 2.0;
        q.adaptive_step = VrAdaptiveStep::default_settings();
        q
    }
}
#[derive(Debug, Clone, PartialEq)]
pub enum VrSamplingError {
    Invalid,
    Budget {
        required_steps: i64,
        max_steps: i64,
        sample_distance_mm: f64,
    },
}
impl fmt::Display for VrSamplingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VrSamplingError::Invalid => f.write_str("render: invalid VR sampling"),
            VrSamplingError::Budget {
                required_steps,
                max_steps,
                sample_distance_mm,
            } => write!(
                f,
                "render: VR ray sample budget exceeded: requires up to {} iterations at {} mm, limit {}",
                required_steps,
                format_float(*sample_distance_mm, 'g', 6),
                max_steps
            ),
        }
    }
}
impl std::error::Error for VrSamplingError {}
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct VrRaySamplingPlan {
    pub sample_distance_mm: f64,
    pub required_steps: i64,
    pub max_steps: i64,
}
pub fn resolve_vr_ray_sampling(
    spacing_mm: [f64; 3],
    diagonal_mm: f64,
    quality: &VrQuality,
) -> Result<VrRaySamplingPlan, VrSamplingError> {
    let fraction = if quality.spacing_fraction == 0.0 {
        DEFAULT_VR_SPACING_FRACTION
    } else {
        quality.spacing_fraction
    };
    let cap = if quality.max_steps == 0 {
        DEFAULT_VR_MAX_STEPS
    } else {
        quality.max_steps
    };
    if !finite(fraction) || fraction <= 0.0 || cap < 1 || !finite(diagonal_mm) || diagonal_mm <= 0.0
    {
        return Err(VrSamplingError::Invalid);
    }
    let mut minimum = f64::INFINITY;
    for spacing in spacing_mm {
        if !finite(spacing) || spacing <= 0.0 {
            return Err(VrSamplingError::Invalid);
        }
        minimum = fmath::min(minimum, spacing);
    }
    let step = minimum * fraction;
    let (mut minimum_step, mut refinement) = (step, 1.0);
    if quality.adaptive_step.enabled() {
        minimum_step *= quality.adaptive_step.normalized().min;
        refinement = 2.0;
    }
    if !finite(step) || step <= 0.0 || !finite(minimum_step) || minimum_step <= 0.0 {
        return Err(VrSamplingError::Invalid);
    }
    let required = (diagonal_mm / minimum_step).ceil() * refinement + 1.0;
    const MAX_INT_F: f64 = 9_223_372_036_854_775_808.0;
    if !finite(required) || required >= MAX_INT_F || required > cap as f64 {
        let count = if finite(required) && required < MAX_INT_F {
            fmath::f64_to_i64(required)
        } else {
            i64::MAX
        };
        return Err(VrSamplingError::Budget {
            required_steps: count,
            max_steps: cap,
            sample_distance_mm: step,
        });
    }
    Ok(VrRaySamplingPlan {
        sample_distance_mm: step,
        required_steps: fmath::f64_to_i64(required),
        max_steps: cap,
    })
}
#[derive(Debug, Clone, Default)]
#[allow(dead_code)]
pub(crate) struct VrRayGuard {
    pub(crate) plan: VrRaySamplingPlan,
    pub(crate) iterations: i64,
    pub(crate) err: Option<VrSamplingError>,
}
#[allow(dead_code)]
impl VrRayGuard {
    pub(crate) fn take(&mut self) -> bool {
        if self.iterations >= self.plan.max_steps {
            self.err = Some(VrSamplingError::Budget {
                required_steps: self.iterations + 1,
                max_steps: self.plan.max_steps,
                sample_distance_mm: self.plan.sample_distance_mm,
            });
            return false;
        }
        self.iterations += 1;
        true
    }
}
pub fn vr_ray_jitter(x: i64, y: i64) -> f64 {
    let mut h = (x as u32).wrapping_mul(73_856_093) ^ (y as u32).wrapping_mul(19_349_663);
    h = (h ^ (h >> 16)).wrapping_mul(0x7feb_352d);
    h = (h ^ (h >> 15)).wrapping_mul(0x846c_a68b);
    h ^= h >> 16;
    f64::from(h >> 8) / 16_777_216.0
}
pub const MIN_VR_INTERACTIVE_SCALE: f64 = 1.0 / 16.0;
const VR_LOW_RES_ENTER: f64 = 0.25;
const VR_LOW_RES_RESUME: f64 = 0.5;
const VR_LOD_MAX_STEP: f64 = 2.0;
const MIN_VR_INTERACTIVE_SIDE: i64 = 32;
pub const DEFAULT_VR_INTERACTIVE_TARGET_NS: i64 = 50_000_000;
#[derive(Debug)]
pub struct VrInteractiveLod {
    target_ns: i64,
    state: Mutex<(f64, bool)>,
}
impl VrInteractiveLod {
    pub fn new(target_ns: i64) -> VrInteractiveLod {
        VrInteractiveLod {
            target_ns: if target_ns <= 0 {
                DEFAULT_VR_INTERACTIVE_TARGET_NS
            } else {
                target_ns
            },
            state: Mutex::new((1.0, false)),
        }
    }
    fn get(&self) -> (f64, bool) {
        match self.state.lock() {
            Ok(g) => *g,
            Err(p) => *p.into_inner(),
        }
    }
    #[doc(hidden)]
    pub fn target_ns(&self) -> i64 {
        self.target_ns
    }
    pub fn scale(&self) -> f64 {
        self.get().0
    }
    pub fn low_resolution(&self) -> bool {
        self.get().1
    }
    pub fn quality(&self, base: VrQuality) -> VrQuality {
        let factor = fmath::cbrt(self.scale());
        let scale = |n: i64, floor: i64| {
            if n <= floor {
                return n;
            }
            floor.max(fmath::f64_to_i64((n as f64 * factor).round()))
        };
        let mut q = base;
        q.width = scale(q.width, MIN_VR_INTERACTIVE_SIDE);
        q.height = scale(q.height, MIN_VR_INTERACTIVE_SIDE);
        if q.spacing_fraction == 0.0 {
            q.spacing_fraction = DEFAULT_VR_SPACING_FRACTION;
        }
        q.spacing_fraction /= factor;
        q
    }
    pub fn observe(&self, elapsed_ns: i64) {
        if elapsed_ns < 0 {
            return;
        }
        let elapsed = elapsed_ns.max(1);
        let mut guard = match self.state.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        let (scale, low_res) = &mut *guard;
        let mut ratio = self.target_ns as f64 / elapsed as f64;
        ratio = fmath::max(1.0 / VR_LOD_MAX_STEP, fmath::min(VR_LOD_MAX_STEP, ratio));
        if *low_res && *scale == 1.0 && ratio >= VR_LOD_MAX_STEP {
            *low_res = false;
            *scale = VR_LOW_RES_RESUME;
            return;
        }
        *scale = fmath::max(MIN_VR_INTERACTIVE_SCALE, fmath::min(1.0, *scale * ratio));
        if !*low_res && *scale < VR_LOW_RES_ENTER {
            *low_res = true;
        }
    }
    pub fn reset(&self) {
        let mut guard = match self.state.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        *guard = (1.0, false);
    }
}
