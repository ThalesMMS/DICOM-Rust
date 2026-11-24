use super::bspline::{
    BSpline, BSplineJacobianBound, abs_difference_upper, determinant_lower_from_norm,
    gradient_norm_upper, matrix_determinant_lower, multiply_lower,
};
use super::composite::Composite;
use super::displacement::{DisplacementField, DisplacementFieldTransform};
use super::error::{RegError, Sentinel};
use super::image::{Image, ints_text};
use super::matrix::{finite, finite_vec};
use super::mattes::MattesMutualInformation;
use super::method::{
    Level, LevelResult, Method, Progress, ProgressObjective, Sampling, ctx_check, shrink_grid,
    validate_sampling_percentage,
};
use super::metric::{Metric, MetricDomain};
use super::resample::Grid;
use super::scales::maximum_step_size;
use super::transform::{MatrixTransform, Transform};
use super::{Affine, Euler3D, ScaleVersor3D, Similarity3D, Translation3D, VersorRigid3D};
use crate::fmath::rand::Rand;
use crate::optimize::{Context, Lbfgs, Lbfgsb, OptimizeResult};
use crate::render::Vec3;
use crate::textfmt::float_v;
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct FfdLevel {
    pub level: Level,
    pub refine_mesh: bool,
}
pub fn default_ffd_levels() -> Vec<FfdLevel> {
    let l = |shrink, sigma| Level {
        shrink,
        smoothing_sigma_mm: sigma,
        sampling_percentage: 0.0,
    };
    vec![
        FfdLevel {
            level: l(4, 2.0),
            refine_mesh: false,
        },
        FfdLevel {
            level: l(2, 1.0),
            refine_mesh: true,
        },
        FfdLevel {
            level: l(1, 0.0),
            refine_mesh: true,
        },
    ]
}
#[derive(Debug, Clone, Default)]
pub struct FfdBoundedOptions {
    pub lower_mm: [f64; 3],
    pub upper_mm: [f64; 3],
    pub optimizer: Lbfgsb,
    pub maximum_displacement_gradient: f64,
    pub max_levels: i64,
    pub max_control_point_bytes: i64,
}
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct FfdJacobianBound {
    pub bspline: BSplineJacobianBound,
    pub initial_determinant_lower: f64,
    pub transform_determinant_lower: f64,
}
#[derive(Default)]
pub struct FreeFormDeformation<'a> {
    pub fixed: Option<&'a Image>,
    pub moving: Option<&'a Image>,
    pub fixed_mask: Option<&'a Image>,
    pub moving_mask: Option<&'a Image>,
    pub initial: Option<&'a dyn Transform>,
    pub mesh_size: [i64; 3],
    pub levels: Option<Vec<FfdLevel>>,
    pub bins: i64,
    pub sampling: Sampling,
    pub seed: i64,
    pub regularization: f64,
    pub optimizer: Lbfgs,
    pub bounded_optimizer: Option<FfdBoundedOptions>,
    pub progress: Option<Box<dyn FnMut(Progress) + 'a>>,
    pub workers: i64,
}
#[derive(Default)]
pub struct FfdResult {
    pub bspline: Option<BSpline>,
    pub transform: Option<Box<dyn Transform>>,
    pub value: f64,
    pub levels: Vec<LevelResult>,
    pub jacobian_bound: Option<FfdJacobianBound>,
}
fn ffd(detail: impl AsRef<str>) -> RegError {
    RegError::new(Sentinel::Ffd, detail)
}
fn ffd_limit() -> RegError {
    RegError::bare(Sentinel::FfdResourceLimit)
}
fn bspline_of(t: &dyn Transform) -> Option<&BSpline> {
    if let Some(b) = t.as_any().downcast_ref::<BSpline>() {
        return Some(b);
    }
    t.as_any()
        .downcast_ref::<Composite>()
        .filter(|c| !c.is_empty())
        .and_then(|c| c.transform(0).as_any().downcast_ref::<BSpline>())
}
struct Regularized<'m> {
    metric: &'m dyn Metric,
    weight: f64,
}
impl Metric for Regularized<'_> {
    fn value(&self, t: &dyn Transform) -> Result<f64, RegError> {
        let v = match self.metric.value(t) {
            Err(e) if e.is(Sentinel::TooFewSamples) => return Ok(f64::INFINITY),
            r => r?,
        };
        if self.weight == 0.0 {
            return Ok(v);
        }
        let energy = bspline_of(t).map_or(0.0, |b| b.membrane_energy(false).0);
        Ok(v + self.weight * energy)
    }
    fn value_and_derivative(&self, t: &dyn Transform) -> Result<(f64, Vec<f64>), RegError> {
        let (v, mut gradient) = match self.metric.value_and_derivative(t) {
            Err(e) if e.is(Sentinel::TooFewSamples) => return Ok((f64::INFINITY, Vec::new())),
            r => r?,
        };
        if self.weight == 0.0 {
            return Ok((v, gradient));
        }
        let (energy, energy_gradient) = match bspline_of(t) {
            Some(b) => b.membrane_energy(true),
            None => (0.0, None),
        };
        for (g, e) in gradient.iter_mut().zip(energy_gradient.unwrap_or_default()) {
            *g += self.weight * e;
        }
        Ok((v + self.weight * energy, gradient))
    }
}
impl FreeFormDeformation<'_> {
    pub fn run(&mut self, ctx: Option<Context<'_>>) -> (FfdResult, Option<RegError>) {
        let mut result = FfdResult::default();
        match self.run_inner(ctx, &mut result) {
            Ok(()) => (result, None),
            Err(e) => {
                result.bspline = None;
                result.transform = None;
                (result, Some(e))
            }
        }
    }
    fn run_inner(
        &mut self,
        ctx: Option<Context<'_>>,
        result: &mut FfdResult,
    ) -> Result<(), RegError> {
        let (Some(fixed_image), Some(moving_image)) = (self.fixed, self.moving) else {
            return Err(ffd("fixed and moving images are required"));
        };
        if self.regularization < 0.0 {
            return Err(ffd("negative regularization"));
        }
        let levels = self.levels.clone().unwrap_or_else(default_ffd_levels);
        if levels.is_empty() {
            return Err(ffd("no level"));
        }
        if let Some(b) = &self.bounded_optimizer {
            let maximum = b.max_levels;
            if maximum < 0 {
                return Err(ffd("bounded FFD level limit"));
            }
            let maximum = if maximum == 0 { 16 } else { maximum };
            if levels.len() as i64 > maximum {
                return Err(ffd_limit());
            }
            let Some(c) = ctx else {
                return Err(ffd("bounded FFD nil context"));
            };
            ctx_check(c)?;
        }
        let ctx = ctx.unwrap_or_default();
        for level in &levels {
            let l = &level.level;
            if l.shrink < 1 || l.smoothing_sigma_mm < 0.0 {
                return Err(ffd(format!(
                    "level {{Shrink:{} SmoothingSigmaMM:{} SamplingPercentage:{}}}: shrink must be at least 1 and σ not negative",
                    l.shrink,
                    float_v(l.smoothing_sigma_mm),
                    float_v(l.sampling_percentage)
                )));
            }
            if l.sampling_percentage != 0.0 {
                validate_sampling_percentage(l.sampling_percentage)?;
            }
        }
        let size = fixed_image.size();
        let mut mesh = self.mesh_size;
        let mut refine_axes = [false; 3];
        for axis in 0..3 {
            refine_axes[axis] = size[axis] > 1;
            if mesh[axis] == 0 {
                mesh[axis] = if refine_axes[axis] { 4 } else { 1 };
            }
            if mesh[axis] < 1 {
                return Err(ffd(format!("mesh size {}", ints_text(&self.mesh_size))));
            }
        }
        let bins = if self.bins == 0 { 32 } else { self.bins };
        let bounded = match &self.bounded_optimizer {
            Some(_) => Some(self.prepare_bounded(ctx, &levels, mesh, refine_axes)?),
            None => None,
        };
        let bspline = BSpline::for_image(Some(fixed_image), mesh)?;
        let initial = self.initial;
        let assemble = |b: BSpline| -> Result<Box<dyn Transform>, RegError> {
            let Some(initial) = initial else {
                return Ok(Box::new(b));
            };
            let mut c = Composite::new(vec![Box::new(b), initial.clone_box()]);
            c.set_optimized(1, false)?;
            Ok(Box::new(c))
        };
        let mut transform = assemble(bspline)?;
        let helper = Method {
            fixed: self.fixed,
            moving: self.moving,
            fixed_mask: self.fixed_mask,
            moving_mask: self.moving_mask,
            sampling: self.sampling,
            workers: self.workers,
            ..Default::default()
        };
        let adapter = super::method::CtxAdapter(ctx);
        let mut random = Rand::new(self.seed);
        for (index, level) in levels.iter().enumerate() {
            ctx_check(ctx)?;
            if index > 0 && level.refine_mesh {
                let current = bspline_of(transform.as_ref()).cloned().unwrap_or_default();
                let refined = current.refine(refine_axes)?;
                transform = assemble(refined)?;
            }
            if let Some(b) = &bounded {
                b.verify(ctx, transform.as_ref())?;
            }
            let (fixed, moving) = helper.smooth(&adapter, level.level.smoothing_sigma_mm)?;
            let fixed_ref = fixed.as_ref().unwrap_or(fixed_image);
            let moving_ref = moving.as_ref().unwrap_or(moving_image);
            let shrunk = shrink_grid(fixed_image, level.level.shrink);
            let virtual_image = shrunk.as_ref().unwrap_or(fixed_image);
            let points =
                helper.sample_points(virtual_image, shrunk.is_none(), &level.level, &mut random)?;
            let count = points.as_ref().map_or(fixed_ref.data.len(), |p| p.len()) as i64;
            let domain = MetricDomain {
                fixed_mask: self.fixed_mask,
                moving_mask: self.moving_mask,
                points,
                workers: self.workers,
            };
            let mattes =
                MattesMutualInformation::new(Some(fixed_ref), Some(moving_ref), domain, bins)
                    .map_err(|e| e.prefixed(format!("registration: level {index}")))?;
            let metric = Regularized {
                metric: &mattes,
                weight: self.regularization,
            };
            let start = transform.parameters();
            let mut report = self.progress.as_deref_mut();
            let mut objective = ProgressObjective {
                ctx,
                metric: &metric,
                transform: transform.as_mut(),
                level: index as i64,
                evaluations: 0,
                report: report.take(),
            };
            let run: Result<OptimizeResult, crate::optimize::Error> = match &bounded {
                None => {
                    let mut o = self.optimizer.clone();
                    if o.maximum_iterations == 0 {
                        o.maximum_iterations = 100;
                    }
                    if o.gradient_tolerance == 0.0 {
                        o.gradient_tolerance = 1e-9;
                    }
                    if o.initial_step_length == 0.0 {
                        o.initial_step_length = maximum_step_size(virtual_image);
                    }
                    o.minimize(&mut objective, &start)
                }
                Some(b) => {
                    let n = start.len();
                    match b.optimizer(ctx, n, virtual_image) {
                        Ok(o) => o.minimize_context(ctx, &mut objective, &start),
                        Err(e) => Err(e.into()),
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
            if let Some(b) = &bounded {
                result.jacobian_bound = b.verify(ctx, transform.as_ref())?;
            }
            result.levels.push(LevelResult {
                level: level.level,
                points: count,
                iterations: run.iterations,
                evaluations,
                value: run.value,
                stop: run.stop,
                parameters: run.x.clone(),
            });
            result.value = run.value;
        }
        result.bspline = bspline_of(transform.as_ref()).cloned();
        result.transform = Some(transform);
        Ok(())
    }
    fn prepare_bounded(
        &self,
        ctx: Context<'_>,
        levels: &[FfdLevel],
        mesh: [i64; 3],
        refine: [bool; 3],
    ) -> Result<BoundedRun, RegError> {
        ctx_check(ctx)?;
        let Some(o) = self.bounded_optimizer.clone() else {
            return Err(ffd("bounded FFD options"));
        };
        let g = o.maximum_displacement_gradient;
        if o.max_levels < 0
            || o.max_control_point_bytes < 0
            || o.optimizer.lower_bounds.is_some()
            || o.optimizer.upper_bounds.is_some()
            || !g.is_finite()
            || g < 0.0
            || g >= 1.0
            || !self.regularization.is_finite()
        {
            return Err(ffd("bounded FFD options"));
        }
        let maximum_levels = if o.max_levels == 0 { 16 } else { o.max_levels };
        if levels.len() as i64 > maximum_levels {
            return Err(ffd_limit());
        }
        for d in 0..3 {
            let (lo, hi) = (o.lower_mm[d], o.upper_mm[d]);
            if !lo.is_finite() || !hi.is_finite() || lo > 0.0 || hi < 0.0 || lo > hi {
                return Err(ffd("bounded FFD component box"));
            }
        }
        let initial_bytes = initial_clone_bytes(ctx, self.initial)?;
        let mut initial_lower = 1.0;
        if g > 0.0 {
            initial_lower = initial_determinant_lower(self.initial)?;
        }
        let maximum_bytes = if o.max_control_point_bytes == 0 {
            256 << 20
        } else {
            o.max_control_point_bytes
        };
        let mut mesh = mesh;
        let (mut total, mut largest) = (0i64, 0i64);
        let fixed = self.fixed.unwrap_or_else(|| unreachable_image());
        for (index, level) in levels.iter().enumerate() {
            if !finite(level.level.smoothing_sigma_mm) {
                return Err(ffd("non-finite bounded FFD smoothing"));
            }
            ctx_check(ctx)?;
            if index > 0 && level.refine_mesh {
                for a in 0..3 {
                    if refine[a] {
                        if mesh[a] > i64::MAX / 2 {
                            return Err(ffd_limit());
                        }
                        mesh[a] *= 2;
                    }
                }
            }
            let mut n = 3i64;
            for &v in &mesh {
                if !(1..=i64::MAX - 3).contains(&v) {
                    return Err(ffd_limit());
                }
                let size = v + 3;
                if n > i64::MAX / size {
                    return Err(ffd_limit());
                }
                n *= size;
            }
            if n > 3 * (1 << 26) {
                return Err(ffd_limit());
            }
            o.optimizer.workspace_bytes(n)?;
            if total > i64::MAX - n {
                return Err(ffd_limit());
            }
            total += n;
            largest = largest.max(n);
            if g > 0.0 {
                let q = box_gradient_norm(fixed, mesh, o.lower_mm, o.upper_mm)?;
                if !(q <= g) || multiply_lower(initial_lower, determinant_lower_from_norm(q)) == 0.0
                {
                    return Err(ffd(format!("component box cannot certify level {index}")));
                }
            }
        }
        if initial_bytes > (i64::MAX - 8192) / 2 {
            return Err(ffd_limit());
        }
        let mut overhead = 8192 + 2 * initial_bytes;
        if levels.len() as i64 > (i64::MAX - overhead) / 256 {
            return Err(ffd_limit());
        }
        overhead += 256 * levels.len() as i64;
        if largest > (i64::MAX - overhead) / 80 || total > (i64::MAX - overhead - 80 * largest) / 8
        {
            return Err(ffd_limit());
        }
        let bytes = 8 * total + 80 * largest + overhead;
        if bytes > maximum_bytes {
            return Err(ffd_limit());
        }
        initial_finite(ctx, self.initial)?;
        Ok(BoundedRun { options: o })
    }
}
fn unreachable_image() -> &'static Image {
    static EMPTY: std::sync::OnceLock<Image> = std::sync::OnceLock::new();
    EMPTY.get_or_init(Image::default)
}
fn box_gradient_norm(
    im: &Image,
    mesh: [i64; 3],
    lo: [f64; 3],
    hi: [f64; 3],
) -> Result<f64, RegError> {
    const MARGIN: f64 = 1.0 / 8.0;
    let origin = im.point(Vec3::new(-0.5 - MARGIN, -0.5 - MARGIN, -0.5 - MARGIN));
    let sp = Vec3::new(
        (im.size[0] as f64 + 2.0 * MARGIN) * im.spacing.x / mesh[0] as f64,
        (im.size[1] as f64 + 2.0 * MARGIN) * im.spacing.y / mesh[1] as f64,
        (im.size[2] as f64 + 2.0 * MARGIN) * im.spacing.z / mesh[2] as f64,
    );
    let grid_origin = origin.add(im.direction.mul_vec(sp.scale(-1.0)));
    let geometry = Image::geometry([1, 1, 1], grid_origin, sp, im.direction)?;
    let mut delta = [[0f64; 3]; 3];
    for d in 0..3 {
        for a in 0..3 {
            delta[d][a] = abs_difference_upper(hi[d], lo[d]);
        }
    }
    Ok(gradient_norm_upper(&delta, &geometry.physical_to_index))
}
struct BoundedRun {
    options: FfdBoundedOptions,
}
impl BoundedRun {
    fn optimizer(
        &self,
        ctx: Context<'_>,
        n: usize,
        virtual_image: &Image,
    ) -> Result<Lbfgsb, RegError> {
        let mut o = self.options.optimizer.clone();
        o.workspace_bytes(n as i64)?;
        ctx_check(ctx)?;
        let count = n / 3;
        let mut lower = vec![0.0; n];
        let mut upper = vec![0.0; n];
        for d in 0..3 {
            for i in d * count..(d + 1) * count {
                if i & 1023 == 0 {
                    ctx_check(ctx)?;
                }
                lower[i] = self.options.lower_mm[d];
                upper[i] = self.options.upper_mm[d];
            }
        }
        o.lower_bounds = Some(lower);
        o.upper_bounds = Some(upper);
        if o.projected_gradient_tolerance == 0.0 {
            o.projected_gradient_tolerance = 1e-9;
        }
        if o.initial_step_length == 0.0 {
            o.initial_step_length = maximum_step_size(virtual_image);
        }
        Ok(o)
    }
    fn verify(
        &self,
        ctx: Context<'_>,
        t: &dyn Transform,
    ) -> Result<Option<FfdJacobianBound>, RegError> {
        let Some(b) = bspline_of(t) else {
            return Err(ffd("bounded composite shape"));
        };
        let coefficients = b.parameters();
        let count = b.count();
        for (i, &v) in coefficients.iter().enumerate() {
            if i & 1023 == 0 {
                ctx_check(ctx)?;
            }
            let d = i / count;
            if v < self.options.lower_mm[d] || v > self.options.upper_mm[d] || !v.is_finite() {
                return Err(ffd("refined/accepted coefficient outside box"));
            }
        }
        if self.options.maximum_displacement_gradient == 0.0 {
            return Ok(None);
        }
        let bound = b.jacobian_bound(Some(ctx))?;
        if !bound.positive_jacobian_certified
            || bound.displacement_gradient_norm_upper > self.options.maximum_displacement_gradient
        {
            return Err(ffd("uncertified accepted spline"));
        }
        let mut initial = None;
        if let Some(c) = t.as_any().downcast_ref::<Composite>() {
            if c.len() != 2 {
                return Err(ffd("bounded composite shape"));
            }
            initial = Some(c.transform(1));
        }
        let lower = initial_determinant_lower(initial)?;
        let combined = multiply_lower(lower, bound.determinant_lower);
        if !(combined > 0.0) || combined.is_infinite() {
            return Err(ffd("unrepresentable composition bound"));
        }
        Ok(Some(FfdJacobianBound {
            bspline: bound,
            initial_determinant_lower: lower,
            transform_determinant_lower: combined,
        }))
    }
}
fn matrix_of(t: &dyn Transform) -> Option<&dyn MatrixTransform> {
    let any = t.as_any();
    let matrix = any.is::<Affine>()
        || any.is::<Euler3D>()
        || any.is::<VersorRigid3D>()
        || any.is::<Similarity3D>()
        || any.is::<Translation3D>()
        || any.is::<ScaleVersor3D>();
    if matrix {
        t.as_matrix_transform()
    } else {
        None
    }
}
fn initial_determinant_lower(t: Option<&dyn Transform>) -> Result<f64, RegError> {
    let Some(t) = t else {
        return Ok(1.0);
    };
    let Some(m) = matrix_of(t) else {
        return Err(ffd("guaranteed bounded FFD requires matrix Initial"));
    };
    if !finite_vec(m.offset()) {
        return Err(ffd("non-finite Initial offset"));
    }
    let lower = matrix_determinant_lower(&m.matrix());
    if !(lower > 0.0) || lower.is_infinite() {
        return Err(ffd("Initial orientation is not certified positive"));
    }
    Ok(lower)
}
fn initial_clone_bytes(ctx: Context<'_>, t: Option<&dyn Transform>) -> Result<i64, RegError> {
    let Some(t) = t else {
        return Ok(0);
    };
    let mut nodes = 0i64;
    let mut bytes = 0i64;
    count_initial(ctx, t, 1, &mut nodes, &mut bytes)?;
    Ok(bytes)
}
fn add_bytes(bytes: &mut i64, n: i64) -> Result<(), RegError> {
    if n < 0 || *bytes > i64::MAX - n {
        return Err(ffd_limit());
    }
    *bytes += n;
    Ok(())
}
fn count_initial(
    ctx: Context<'_>,
    t: &dyn Transform,
    depth: i64,
    nodes: &mut i64,
    bytes: &mut i64,
) -> Result<(), RegError> {
    ctx_check(ctx)?;
    if depth > 16 || *nodes == 256 {
        return Err(ffd("Initial depth/node limit"));
    }
    *nodes += 1;
    add_bytes(bytes, 4096)?;
    let mut numeric = 0i64;
    let any = t.as_any();
    if let Some(b) = any.downcast_ref::<BSpline>() {
        numeric = b.number_of_parameters() as i64;
    } else if let Some(d) = any.downcast_ref::<DisplacementFieldTransform>() {
        numeric = d.number_of_parameters() as i64;
    } else if let Some(c) = any.downcast_ref::<Composite>() {
        if c.len() as i64 > 256 - *nodes {
            return Err(ffd("Initial composite size"));
        }
        add_bytes(bytes, 32 * c.len() as i64)?;
        for i in 0..c.len() {
            count_initial(ctx, c.transform(i), depth + 1, nodes, bytes)?;
        }
    } else if matrix_of(t).is_none() {
        return Err(ffd(format!("unaccountable Initial type {}", t.kind())));
    }
    if numeric > i64::MAX / 16 {
        return Err(ffd_limit());
    }
    add_bytes(bytes, 16 * numeric)
}
fn initial_finite(ctx: Context<'_>, t: Option<&dyn Transform>) -> Result<(), RegError> {
    ctx_check(ctx)?;
    let Some(t) = t else {
        return Ok(());
    };
    let any = t.as_any();
    let invalid = if let Some(b) = any.downcast_ref::<BSpline>() {
        let mut bad = false;
        for (i, c) in b.parameters().iter().enumerate() {
            if i & 1023 == 0 {
                ctx_check(ctx)?;
            }
            if !finite(*c) {
                bad = true;
                break;
            }
        }
        bad
    } else if let Some(d) = any.downcast_ref::<DisplacementFieldTransform>() {
        let mut bad = false;
        for (i, v) in d.field().vectors.iter().enumerate() {
            if i & 1023 == 0 {
                ctx_check(ctx)?;
            }
            if !finite_vec(*v) {
                bad = true;
                break;
            }
        }
        bad
    } else if let Some(c) = any.downcast_ref::<Composite>() {
        for i in 0..c.len() {
            initial_finite(ctx, Some(c.transform(i)))?;
        }
        false
    } else {
        match t.as_matrix_transform() {
            Some(m) => !m.matrix().finite() || !finite_vec(m.offset()),
            None => true,
        }
    };
    if invalid {
        return Err(ffd("non-finite Initial"));
    }
    Ok(())
}
impl FfdResult {
    pub fn displacement_field(
        &self,
        ctx: Context<'_>,
        grid: &Grid,
    ) -> Result<DisplacementField, RegError> {
        super::bspline::transform_displacement_field(ctx, self.transform.as_deref(), grid)
    }
}
