use super::error::{RegError, Sentinel};
use super::euler::Euler3D;
use super::image::Image;
use super::matrix::finite;
use super::mattes::MattesMutualInformation;
use super::method::{CtxAdapter, Level, LevelResult, Method, MethodResult, Sampling, ctx_check};
use super::metric::{Correlation, MeanSquares, Metric, MetricDomain};
use super::resample::{ResampleOptions, Resampled, resample, transform_mapping};
use super::scales::corner_points;
use super::transform::Transform;
use crate::fmath;
use crate::optimize::Context;
use crate::render::Vec3;
use std::cell::{Cell, RefCell};
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MotionMetric(pub i64);
impl MotionMetric {
    #[doc(hidden)]
    pub const MATTES: MotionMetric = MotionMetric(0);
    pub const MEAN_SQUARES: MotionMetric = MotionMetric(1);
    pub const CORRELATION: MotionMetric = MotionMetric(2);
}
pub const DEFAULT_MOTION_SAMPLES: i64 = 96;
pub const DEFAULT_MOTION_POINTS: i64 = 60000;
pub const DEFAULT_MOTION_ITERATIONS: i64 = 150;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MotionStage(pub i64);
impl MotionStage {
    pub const LOAD: MotionStage = MotionStage(0);
    pub const REGISTER: MotionStage = MotionStage(1);
    pub const RESAMPLE: MotionStage = MotionStage(2);
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MotionProgress {
    pub phase: i64,
    pub completed: i64,
    pub phases: i64,
    pub stage: MotionStage,
    pub level: i64,
    pub evaluation: i64,
    pub done: i64,
    pub total: i64,
}
#[derive(Default)]
pub struct MotionOptions<'a> {
    pub reference: i64,
    pub metric: MotionMetric,
    pub levels: Option<Vec<Level>>,
    pub max_samples: i64,
    pub max_points: i64,
    pub max_iterations: i64,
    pub seed: i64,
    pub padding: f32,
    pub pad_with_minimum: bool,
    pub workers: i64,
    pub progress: Option<Box<dyn FnMut(MotionProgress) + 'a>>,
}
#[derive(Debug, Clone)]
pub struct PhaseMotion {
    pub index: i64,
    pub reference: bool,
    pub transform: Option<Euler3D>,
    pub rotation_degrees: [f64; 3],
    pub translation: Vec3,
    pub max_displacement_mm: f64,
    pub levels: Vec<LevelResult>,
    pub value: f64,
}
#[derive(Debug, Clone)]
pub struct PhaseCorrection {
    pub motion: PhaseMotion,
    pub corrected: Resampled,
}
pub type MotionSource<'a> = dyn FnMut(Context<'_>, i64) -> Result<Option<Image>, RegError> + 'a;
pub type MotionSink<'a> = dyn FnMut(PhaseCorrection) -> Result<(), RegError> + 'a;
pub fn correct_motion(
    ctx: Context<'_>,
    phases: i64,
    source: Option<&mut MotionSource<'_>>,
    opts: MotionOptions<'_>,
    sink: Option<&mut MotionSink<'_>>,
) -> Result<Vec<PhaseMotion>, RegError> {
    let (Some(source), Some(sink)) = (source, sink) else {
        return Err(RegError::new(
            Sentinel::Motion,
            format!("{phases} phases, a source and a sink are required"),
        ));
    };
    if phases < 2 {
        return Err(RegError::new(
            Sentinel::Motion,
            format!("{phases} phases, a source and a sink are required"),
        ));
    }
    if opts.reference < 0 || opts.reference >= phases {
        return Err(RegError::new(
            Sentinel::Motion,
            format!("reference {} outside 0…{}", opts.reference, phases - 1),
        ));
    }
    if opts.max_samples < 0
        || opts.max_points < 0
        || opts.max_iterations < 0
        || !finite(f64::from(opts.padding))
    {
        return Err(RegError::new(
            Sentinel::Motion,
            "negative limits or non-finite padding",
        ));
    }
    if !(0..=2).contains(&opts.metric.0) {
        return Err(RegError::new(
            Sentinel::Motion,
            format!("unknown metric {}", opts.metric.0),
        ));
    }
    let levels = opts.levels.clone().unwrap_or_else(|| {
        vec![
            Level {
                shrink: 2,
                smoothing_sigma_mm: 4.0,
                sampling_percentage: 0.0,
            },
            Level {
                shrink: 1,
                smoothing_sigma_mm: 2.0,
                sampling_percentage: 0.0,
            },
            Level {
                shrink: 1,
                smoothing_sigma_mm: 0.0,
                sampling_percentage: 0.0,
            },
        ]
    });
    let max_samples = if opts.max_samples == 0 {
        DEFAULT_MOTION_SAMPLES
    } else {
        opts.max_samples
    };
    let max_points = if opts.max_points == 0 {
        DEFAULT_MOTION_POINTS
    } else {
        opts.max_points
    };
    let iterations = if opts.max_iterations == 0 {
        DEFAULT_MOTION_ITERATIONS
    } else {
        opts.max_iterations
    };
    let completed = Cell::new(0i64);
    let progress = RefCell::new(opts.progress);
    let report = |mut p: MotionProgress| {
        if let Some(f) = progress.borrow_mut().as_mut() {
            p.completed = completed.get();
            p.phases = phases;
            f(p);
        }
    };
    let mut load = |index: i64| -> Result<Image, RegError> {
        report(MotionProgress {
            phase: index,
            stage: MotionStage::LOAD,
            ..Default::default()
        });
        match source(ctx, index) {
            Err(e) => {
                ctx_check(ctx)?;
                Err(e.prefixed(format!("registration: phase {index}")))
            }
            Ok(None) => Err(RegError::new(
                Sentinel::Motion,
                format!("phase {index} is missing"),
            )),
            Ok(Some(im)) => Ok(im),
        }
    };
    let reference = load(opts.reference)?;
    let grid = reference.grid();
    let fixed = reference.downsample(motion_factors(reference.size(), max_samples))?;
    let centre = fixed.point(Vec3::new(
        (fixed.size[0] - 1) as f64 / 2.0,
        (fixed.size[1] - 1) as f64 / 2.0,
        (fixed.size[2] - 1) as f64 / 2.0,
    ));
    let corners = corner_points(&reference);
    let mut motions: Vec<Option<PhaseMotion>> = vec![None; phases as usize];
    let adapter = CtxAdapter(ctx);
    let mut correct = |index: i64, moving: &Image, motion: PhaseMotion| -> Result<(), RegError> {
        let padding = if opts.pad_with_minimum {
            lowest(moving)
        } else {
            opts.padding
        };
        let transform = motion.transform.as_ref().map(|t| t as &dyn Transform);
        let mapping = transform_mapping(transform)?;
        report(MotionProgress {
            phase: index,
            stage: MotionStage::RESAMPLE,
            total: grid.size[2],
            ..Default::default()
        });
        let resampled = resample(
            Some(&adapter),
            Some(moving),
            &grid,
            Some(&mapping),
            ResampleOptions {
                padding,
                progress: Some(Box::new(|done, total| {
                    report(MotionProgress {
                        phase: index,
                        stage: MotionStage::RESAMPLE,
                        done,
                        total,
                        ..Default::default()
                    })
                })),
                ..Default::default()
            },
        )?;
        motions[index as usize] = Some(motion.clone());
        sink(PhaseCorrection {
            motion,
            corrected: resampled,
        })?;
        completed.set(completed.get() + 1);
        Ok(())
    };
    let mut identity = Euler3D::new();
    identity.set_center(centre)?;
    correct(
        opts.reference,
        &reference,
        describe_motion(
            opts.reference,
            true,
            &identity,
            &corners,
            &MethodResult::default(),
        ),
    )?;
    for direction in [1i64, -1] {
        let mut previous = identity.clone();
        let mut index = opts.reference + direction;
        while index >= 0 && index < phases {
            ctx_check(ctx)?;
            let phase = load(index)?;
            let moving = phase.downsample(motion_factors(phase.size(), max_samples))?;
            let mut phase_levels = levels.clone();
            for l in phase_levels.iter_mut() {
                if l.sampling_percentage == 0.0 {
                    l.sampling_percentage = motion_sampling(&fixed, l.shrink.max(1), max_points);
                }
            }
            let metric = opts.metric;
            let mut method = Method {
                fixed: Some(&fixed),
                moving: Some(&moving),
                transform: Some(&previous),
                levels: Some(phase_levels),
                sampling: Sampling::REGULAR,
                seed: opts.seed,
                workers: opts.workers,
                new_metric: Some(Box::new(move |f, m, d| motion_metric(metric, f, m, d))),
                progress: Some(Box::new(|p: super::method::Progress| {
                    report(MotionProgress {
                        phase: index,
                        stage: MotionStage::REGISTER,
                        level: p.level,
                        evaluation: p.evaluation,
                        ..Default::default()
                    })
                })),
                ..Default::default()
            };
            method.optimizer.maximum_iterations = iterations;
            report(MotionProgress {
                phase: index,
                stage: MotionStage::REGISTER,
                ..Default::default()
            });
            let (result, err) = method.run(ctx);
            drop(method);
            if let Some(e) = err {
                ctx_check(ctx)?;
                return Err(e.prefixed(format!("registration: phase {index}")));
            }
            let euler = match result
                .transform
                .as_ref()
                .and_then(|t| t.as_any().downcast_ref::<Euler3D>())
            {
                Some(e) => e.clone(),
                None => {
                    let kind = result.transform.as_ref().map(|t| t.kind().to_string());
                    return Err(RegError::new(
                        Sentinel::Motion,
                        format!("unexpected transform {}", kind.unwrap_or_default()),
                    ));
                }
            };
            correct(
                index,
                &phase,
                describe_motion(index, false, &euler, &corners, &result),
            )?;
            previous = euler;
            index += direction;
        }
    }
    Ok(motions
        .into_iter()
        .map(|m| m.unwrap_or_else(empty_motion))
        .collect())
}
fn empty_motion() -> PhaseMotion {
    PhaseMotion {
        index: 0,
        reference: false,
        transform: None,
        rotation_degrees: [0.0; 3],
        translation: Vec3::ZERO,
        max_displacement_mm: 0.0,
        levels: Vec::new(),
        value: 0.0,
    }
}
fn describe_motion(
    index: i64,
    reference: bool,
    transform: &Euler3D,
    corners: &[Vec3],
    result: &MethodResult,
) -> PhaseMotion {
    let clone = transform.clone();
    let p = clone.parameters();
    let pi = std::f64::consts::PI;
    let mut motion = PhaseMotion {
        index,
        reference,
        transform: None,
        rotation_degrees: [p[0] * 180.0 / pi, p[1] * 180.0 / pi, p[2] * 180.0 / pi],
        translation: Vec3::new(p[3], p[4], p[5]),
        max_displacement_mm: 0.0,
        levels: result.levels.clone(),
        value: result.value,
    };
    for &q in corners {
        motion.max_displacement_mm = fmath::max(
            motion.max_displacement_mm,
            clone.transform_point(q).sub(q).length(),
        );
    }
    motion.transform = Some(clone);
    motion
}
fn motion_factors(size: [usize; 3], limit: i64) -> [i64; 3] {
    size.map(|n| ((n as i64 + limit - 1) / limit).max(1))
}
fn motion_sampling(im: &Image, shrink: i64, points: i64) -> f64 {
    let s = shrink as usize;
    let size = im.size;
    let total = ((size[0] / s).max(1) * (size[1] / s).max(1) * (size[2] / s).max(1)) as f64;
    fmath::min(1.0, points as f64 / total)
}
fn motion_metric<'x>(
    metric: MotionMetric,
    f: &'x Image,
    m: &'x Image,
    d: MetricDomain<'x>,
) -> Result<Box<dyn Metric + 'x>, RegError> {
    match metric {
        MotionMetric::MEAN_SQUARES => {
            MeanSquares::new(Some(f), Some(m), d).map(|v| Box::new(v) as Box<dyn Metric>)
        }
        MotionMetric::CORRELATION => {
            Correlation::new(Some(f), Some(m), d).map(|v| Box::new(v) as Box<dyn Metric>)
        }
        _ => MattesMutualInformation::new(Some(f), Some(m), d, 32)
            .map(|v| Box::new(v) as Box<dyn Metric>),
    }
}
fn lowest(im: &Image) -> f32 {
    let mut out = f32::INFINITY;
    for &v in &im.data {
        if finite(f64::from(v)) && v < out {
            out = v;
        }
    }
    if out.is_infinite() {
        return 0.0;
    }
    out
}
