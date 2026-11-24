use super::error::{RegError, Sentinel};
use super::image::Image;
use super::matrix::Matrix3;
use super::mattes::MattesMutualInformation;
use super::metric::{Metric, MetricDomain};
use super::scales::{PhysicalShiftScales, corner_points, maximum_step_size};
use super::transform::Transform;
use super::{Affine, Euler3D, ScaleVersor3D, Similarity3D, VersorRigid3D};
use crate::fmath::rand::Rand;
use crate::imagefilter::{
    RecursiveGaussianOptions, Volume, isotropic_sigma, smooth_recursive_gaussian,
};
use crate::optimize::{
    self, Context, Function, GradientFunction, OptimizeResult, RegularStepGradientDescent,
    StepScaleEstimator, StopReason,
};
use crate::render::Vec3;
use crate::render::error::{ErrorKind as RenderKind, RenderError};
use crate::render::parallel::Cancellation;
use crate::textfmt::float_v;
#[derive(Clone, Copy)]
pub(crate) struct CtxAdapter<'a>(pub Context<'a>);
impl Cancellation for CtxAdapter<'_> {
    fn err(&self) -> Option<RenderError> {
        self.0
            .err()
            .err()
            .map(|_| RenderError::new(RenderKind::Canceled, "context canceled"))
    }
}
pub(crate) fn ctx_check(ctx: Context<'_>) -> Result<(), RegError> {
    ctx.err().map_err(|_| RegError::canceled())
}
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Level {
    pub shrink: i64,
    pub smoothing_sigma_mm: f64,
    pub sampling_percentage: f64,
}
pub fn default_levels() -> Vec<Level> {
    vec![
        Level {
            shrink: 2,
            smoothing_sigma_mm: 2.0,
            sampling_percentage: 0.0,
        },
        Level {
            shrink: 1,
            smoothing_sigma_mm: 1.0,
            sampling_percentage: 0.0,
        },
        Level {
            shrink: 1,
            smoothing_sigma_mm: 0.0,
            sampling_percentage: 0.0,
        },
    ]
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Sampling(pub i64);
impl Sampling {
    pub const ALL: Sampling = Sampling(0);
    pub const REGULAR: Sampling = Sampling(1);
    #[doc(hidden)]
    pub const RANDOM: Sampling = Sampling(2);
}
pub fn validate_sampling_percentage(p: f64) -> Result<(), RegError> {
    if !(p > 0.0 && p <= 1.0) {
        return Err(RegError::new(Sentinel::SamplingPercentage, float_v(p)));
    }
    Ok(())
}
pub type NewMetric<'a> = Box<
    dyn for<'x> Fn(&'x Image, &'x Image, MetricDomain<'x>) -> Result<Box<dyn Metric + 'x>, RegError>
        + 'a,
>;
pub type LevelMinimizer<'a> = Box<
    dyn FnMut(
            i64,
            &mut dyn GradientFunction,
            &[f64],
            &[f64],
        ) -> Result<OptimizeResult, optimize::Error>
        + 'a,
>;
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Progress {
    pub level: i64,
    pub evaluation: i64,
    pub value: f64,
}
#[derive(Debug, Clone, PartialEq)]
pub struct LevelResult {
    pub level: Level,
    pub points: i64,
    pub iterations: i64,
    pub evaluations: i64,
    pub value: f64,
    pub stop: StopReason,
    pub parameters: Vec<f64>,
}
#[derive(Default)]
pub struct MethodResult {
    pub transform: Option<Box<dyn Transform>>,
    pub value: f64,
    pub levels: Vec<LevelResult>,
}
#[derive(Default)]
pub struct Method<'a> {
    pub fixed: Option<&'a Image>,
    pub moving: Option<&'a Image>,
    pub fixed_mask: Option<&'a Image>,
    pub moving_mask: Option<&'a Image>,
    pub transform: Option<&'a dyn Transform>,
    pub new_metric: Option<NewMetric<'a>>,
    pub levels: Option<Vec<Level>>,
    pub sampling: Sampling,
    pub seed: i64,
    pub optimizer: RegularStepGradientDescent,
    pub minimize: Option<LevelMinimizer<'a>>,
    pub progress: Option<Box<dyn FnMut(Progress) + 'a>>,
    pub workers: i64,
}
fn level_v(l: &Level) -> String {
    format!(
        "{{Shrink:{} SmoothingSigmaMM:{} SamplingPercentage:{}}}",
        l.shrink,
        float_v(l.smoothing_sigma_mm),
        float_v(l.sampling_percentage)
    )
}
struct ShiftEstimator {
    transform: Box<dyn Transform>,
    points: Vec<Vec3>,
}
impl StepScaleEstimator for ShiftEstimator {
    fn step_scale(&self, step: &[f64]) -> Result<f64, optimize::Error> {
        PhysicalShiftScales {
            transform: self.transform.as_ref(),
            points: self.points.clone(),
            small_parameter_variation: 0.0,
        }
        .step_scale(step)
        .map_err(optimize::Error::from)
    }
}
pub(crate) struct ProgressObjective<'o, 'p> {
    pub ctx: Context<'o>,
    pub metric: &'o dyn Metric,
    pub transform: &'o mut dyn Transform,
    pub level: i64,
    pub evaluations: i64,
    pub report: Option<&'o mut (dyn FnMut(Progress) + 'p)>,
}
impl ProgressObjective<'_, '_> {
    fn note(&mut self, value: f64, ok: bool) {
        self.evaluations += 1;
        if ok && let Some(report) = self.report.as_mut() {
            report(Progress {
                level: self.level,
                evaluation: self.evaluations,
                value,
            });
        }
    }
}
impl Function for ProgressObjective<'_, '_> {
    fn value(&mut self, x: &[f64]) -> Result<f64, optimize::Error> {
        self.ctx.err()?;
        let adapter = CtxAdapter(self.ctx);
        let r = match self.transform.set_parameters(x) {
            Err(e) => Err(RegError::from(e)),
            Ok(()) => match self.metric.context_value(Some(&adapter), &*self.transform) {
                Some(r) => r,
                None => self.metric.value(&*self.transform),
            },
        };
        self.note(*r.as_ref().unwrap_or(&0.0), r.is_ok());
        r.map_err(optimize::Error::from)
    }
    fn as_gradient(&mut self) -> Option<&mut dyn GradientFunction> {
        Some(self)
    }
}
impl GradientFunction for ProgressObjective<'_, '_> {
    fn value_and_gradient(&mut self, x: &[f64]) -> Result<(f64, Vec<f64>), optimize::Error> {
        self.ctx.err()?;
        let adapter = CtxAdapter(self.ctx);
        let r = match self.transform.set_parameters(x) {
            Err(e) => Err(RegError::from(e)),
            Ok(()) => match self
                .metric
                .context_value_and_derivative(Some(&adapter), &*self.transform)
            {
                Some(r) => r,
                None => self.metric.value_and_derivative(&*self.transform),
            },
        };
        let value = r.as_ref().map_or(0.0, |v| v.0);
        self.note(value, r.is_ok());
        r.map_err(optimize::Error::from)
    }
}
impl Method<'_> {
    pub fn run(&mut self, ctx: Context<'_>) -> (MethodResult, Option<RegError>) {
        let mut result = MethodResult::default();
        match self.run_inner(ctx, &mut result) {
            Ok(()) => (result, None),
            Err(e) => {
                result.transform = None;
                (result, Some(e))
            }
        }
    }
    fn run_inner(&mut self, ctx: Context<'_>, result: &mut MethodResult) -> Result<(), RegError> {
        let (Some(m_fixed), Some(m_moving), Some(initial)) =
            (self.fixed, self.moving, self.transform)
        else {
            return Err(RegError::new(
                Sentinel::Image,
                "fixed image, moving image and transform are required",
            ));
        };
        let levels = self.levels.clone().unwrap_or_else(default_levels);
        for level in &levels {
            if level.shrink < 1 || level.smoothing_sigma_mm < 0.0 {
                return Err(RegError::with_text(
                    Vec::new(),
                    format!(
                        "registration: level {}: shrink must be at least 1 and σ not negative",
                        level_v(level)
                    ),
                ));
            }
            if level.sampling_percentage != 0.0 {
                validate_sampling_percentage(level.sampling_percentage)?;
            }
        }
        let mut transform = initial.clone_box();
        let mut random = Rand::new(self.seed);
        let mut maximum_step = 0.0;
        let adapter = CtxAdapter(ctx);
        for (index, level) in levels.iter().enumerate() {
            ctx_check(ctx)?;
            let (fixed, moving) = self.smooth(&adapter, level.smoothing_sigma_mm)?;
            let fixed_ref = fixed.as_ref().unwrap_or(m_fixed);
            let moving_ref = moving.as_ref().unwrap_or(m_moving);
            let shrunk = shrink_grid(m_fixed, level.shrink);
            let virtual_image = shrunk.as_ref().unwrap_or(m_fixed);
            let points = self.sample_points(virtual_image, shrunk.is_none(), level, &mut random)?;
            let count = points.as_ref().map_or(fixed_ref.data.len(), |p| p.len()) as i64;
            let domain = MetricDomain {
                fixed_mask: self.fixed_mask,
                moving_mask: self.moving_mask,
                points,
                workers: self.workers,
            };
            let metric: Box<dyn Metric + '_> = match &self.new_metric {
                Some(f) => f(fixed_ref, moving_ref, domain),
                None => MattesMutualInformation::new(Some(fixed_ref), Some(moving_ref), domain, 20)
                    .map(|m| Box::new(m) as Box<dyn Metric>),
            }
            .map_err(|e| e.prefixed(format!("registration: level {index}")))?;
            let corners = corner_points(virtual_image);
            let scales = match &self.optimizer.scales {
                Some(s) => s.clone(),
                None => PhysicalShiftScales {
                    transform: transform.as_ref(),
                    points: corners.clone(),
                    small_parameter_variation: 0.0,
                }
                .scales()?,
            };
            if maximum_step == 0.0 {
                maximum_step = maximum_step_size(virtual_image);
            }
            let start = transform.parameters();
            let estimator = ShiftEstimator {
                transform: transform.clone_box(),
                points: corners,
            };
            let mut report = self.progress.as_deref_mut();
            let mut objective = ProgressObjective {
                ctx,
                metric: metric.as_ref(),
                transform: transform.as_mut(),
                level: index as i64,
                evaluations: 0,
                report: report.take(),
            };
            let run = match self.minimize.as_mut() {
                Some(minimize) => minimize(index as i64, &mut objective, &start, &scales),
                None => {
                    let o = &self.optimizer;
                    let mut level_optimizer = RegularStepGradientDescent {
                        learning_rate: o.learning_rate,
                        relaxation_factor: o.relaxation_factor,
                        minimum_step_length: o.minimum_step_length,
                        gradient_magnitude_tolerance: o.gradient_magnitude_tolerance,
                        maximum_iterations: o.maximum_iterations,
                        scales: Some(scales.clone()),
                        step_scale: None,
                        maximum_step_size: o.maximum_step_size,
                        estimate_learning_rate_each_iteration: o
                            .estimate_learning_rate_each_iteration,
                        convergence_window: o.convergence_window,
                        minimum_convergence_value: o.minimum_convergence_value,
                    };
                    if o.step_scale.is_none() && o.maximum_step_size == 0.0 {
                        level_optimizer.step_scale = Some(Box::new(estimator));
                        level_optimizer.maximum_step_size = maximum_step;
                    }
                    if level_optimizer.maximum_iterations == 0 {
                        level_optimizer.maximum_iterations = 500;
                    }
                    if level_optimizer.minimum_step_length == 0.0 {
                        level_optimizer.minimum_step_length = 1e-6;
                    }
                    if level_optimizer.gradient_magnitude_tolerance == 0.0 {
                        level_optimizer.gradient_magnitude_tolerance = 1e-12;
                    }
                    if level_optimizer.step_scale.is_none() {
                        level_optimizer.step_scale = self.optimizer.step_scale.take();
                        let r = level_optimizer.minimize(&mut objective, &start);
                        self.optimizer.step_scale = level_optimizer.step_scale.take();
                        r
                    } else {
                        level_optimizer.minimize(&mut objective, &start)
                    }
                }
            };
            let evaluations = objective.evaluations;
            let run = match run {
                Ok(r) => r,
                Err(e) => {
                    ctx_check(ctx)?;
                    return Err(RegError::from(e).prefixed(format!("registration: level {index}")));
                }
            };
            transform.set_parameters(&run.x)?;
            result.levels.push(LevelResult {
                level: *level,
                points: count,
                iterations: run.iterations,
                evaluations,
                value: run.value,
                stop: run.stop,
                parameters: run.x.clone(),
            });
            result.value = run.value;
        }
        ctx_check(ctx)?;
        result.transform = Some(transform);
        Ok(())
    }
    pub(crate) fn smooth(
        &self,
        ctx: &CtxAdapter<'_>,
        sigma: f64,
    ) -> Result<(Option<Image>, Option<Image>), RegError> {
        if sigma == 0.0 {
            return Ok((None, None));
        }
        let mut out = Vec::with_capacity(2);
        for im in [self.fixed, self.moving].into_iter().flatten() {
            let volume = Volume {
                dims: im.size.map(|n| n as i64),
                spacing_mm: [im.spacing.x, im.spacing.y, im.spacing.z],
                data: im.data.clone(),
            };
            let smoothed = smooth_recursive_gaussian(
                Some(ctx),
                &volume,
                RecursiveGaussianOptions {
                    sigma_mm: isotropic_sigma(sigma),
                    normalize_across_scale: false,
                    workers: self.workers,
                },
            )?;
            out.push(Image::new(
                im.size.map(|n| n as i64),
                smoothed.data,
                im.origin,
                im.spacing,
                im.direction,
            )?);
        }
        let moving = out.pop();
        Ok((out.pop(), moving))
    }
    pub(crate) fn sample_points(
        &self,
        virtual_image: &Image,
        is_fixed: bool,
        level: &Level,
        random: &mut Rand,
    ) -> Result<Option<Vec<Vec3>>, RegError> {
        let percentage = if level.sampling_percentage == 0.0 {
            1.0
        } else {
            level.sampling_percentage
        };
        let v = virtual_image;
        let total = v.data.len();
        let point = |i: usize| {
            v.point(Vec3::new(
                (i % v.size[0]) as f64,
                (i / v.size[0] % v.size[1]) as f64,
                (i / (v.size[0] * v.size[1])) as f64,
            ))
        };
        let jitter = |p: Vec3, random: &mut Rand| {
            let x = random.norm_float64() * v.spacing.x / 3.0;
            let y = random.norm_float64() * v.spacing.y / 3.0;
            let mut z = random.norm_float64() * v.spacing.z / 3.0;
            if v.size[2] == 1 {
                z = 0.0;
            }
            p.add(v.direction.mul_vec(Vec3::new(x, y, z)))
        };
        match self.sampling {
            Sampling::ALL => {
                if is_fixed {
                    return Ok(None);
                }
                Ok(Some((0..total).map(point).collect()))
            }
            Sampling::REGULAR => {
                let step = (1.0 / percentage).ceil() as usize;
                let mut points = Vec::with_capacity(total / step + 1);
                let mut i = 0;
                while i < total {
                    points.push(jitter(point(i), random));
                    i += step;
                }
                Ok(Some(points))
            }
            Sampling::RANDOM => {
                let count = (total as f64 * percentage) as usize;
                let mut points = Vec::with_capacity(count);
                for _ in 0..count {
                    let i = random.intn(total as i64).unwrap_or(0) as usize;
                    points.push(jitter(point(i), random));
                }
                Ok(Some(points))
            }
            Sampling(n) => Err(RegError::with_text(
                Vec::new(),
                format!("registration: unknown sampling {n}"),
            )),
        }
    }
}
pub(crate) fn shrink_grid(im: &Image, factor: i64) -> Option<Image> {
    if factor == 1 {
        return None;
    }
    let f = factor as usize;
    let size = im.size.map(|n| (n / f).max(1));
    let spacing = im.spacing.scale(factor as f64);
    let scale = Matrix3([
        [spacing.x, 0.0, 0.0],
        [0.0, spacing.y, 0.0],
        [0.0, 0.0, spacing.z],
    ]);
    let centre_in = im.point(Vec3::new(
        (im.size[0] - 1) as f64 / 2.0,
        (im.size[1] - 1) as f64 / 2.0,
        (im.size[2] - 1) as f64 / 2.0,
    ));
    let centre_out = im.direction.mul(&scale).mul_vec(Vec3::new(
        (size[0] - 1) as f64 / 2.0,
        (size[1] - 1) as f64 / 2.0,
        (size[2] - 1) as f64 / 2.0,
    ));
    Image::new(
        size.map(|n| n as i64),
        vec![0.0; size[0] * size[1] * size[2]],
        centre_in.sub(centre_out),
        spacing,
        im.direction,
    )
    .ok()
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CenteringMode(pub i64);
impl CenteringMode {
    pub const GEOMETRY: CenteringMode = CenteringMode(0);
    pub const MOMENTS: CenteringMode = CenteringMode(1);
}
pub fn center_transform(
    transform: &mut dyn Transform,
    fixed: &Image,
    moving: &Image,
    mode: CenteringMode,
) -> Result<(), RegError> {
    let kind = transform.kind();
    let any = transform.as_any_mut();
    let centered = any.is::<Affine>()
        || any.is::<Euler3D>()
        || any.is::<Similarity3D>()
        || any.is::<VersorRigid3D>()
        || any.is::<ScaleVersor3D>();
    if !centered {
        return Err(RegError::with_text(
            Vec::new(),
            format!("registration: {kind} has no centre to initialize"),
        ));
    }
    let fixed_centre = image_centre(fixed, mode)?;
    let moving_centre = image_centre(moving, mode)?;
    let translation = moving_centre.sub(fixed_centre);
    macro_rules! set {
        ($($ty:ty),+) => {
            $(if let Some(t) = any.downcast_mut::<$ty>() {
                t.set_center(fixed_centre)?;
                t.set_translation(translation)?;
                return Ok(());
            })+
        };
    }
    set!(Affine, Euler3D, Similarity3D, VersorRigid3D, ScaleVersor3D);
    Ok(())
}
pub(crate) fn image_centre(im: &Image, mode: CenteringMode) -> Result<Vec3, RegError> {
    if mode == CenteringMode::GEOMETRY {
        return Ok(im.point(Vec3::new(
            (im.size[0] - 1) as f64 / 2.0,
            (im.size[1] - 1) as f64 / 2.0,
            (im.size[2] - 1) as f64 / 2.0,
        )));
    }
    let (mut mass, mut sx, mut sy, mut sz) = (0.0, 0.0, 0.0, 0.0);
    for (i, &v) in im.data.iter().enumerate() {
        if v == 0.0 {
            continue;
        }
        let w = f64::from(v);
        let (x, y, z) = (
            i % im.size[0],
            i / im.size[0] % im.size[1],
            i / (im.size[0] * im.size[1]),
        );
        mass += w;
        sx += w * x as f64;
        sy += w * y as f64;
        sz += w * z as f64;
    }
    if mass == 0.0 || !mass.is_finite() {
        return Err(RegError::bare(Sentinel::NoMass));
    }
    Ok(im.point(Vec3::new(sx / mass, sy / mass, sz / mass)))
}
