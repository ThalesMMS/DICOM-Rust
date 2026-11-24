#![allow(
    clippy::neg_cmp_op_on_partial_ord,
    clippy::manual_range_contains,
    clippy::needless_range_loop,
    clippy::too_many_arguments,
    clippy::manual_clamp,
    clippy::map_entry
)]
use super::edge_levelset::{
    ShapeDetectionParams, ShapePrior, shape_detection_level_set, shape_detection_with_prior,
};
use super::levelset::{
    LevelSetGrid, LevelSetInitialization, LevelSetOptions, LevelSetResult, LevelSetVolume,
    finite_non_negative, level_set_context_error,
};
use super::levelset_interface::ShapeEvolutionProfile;
use super::pca_shape::{
    ERR_PCA_SHAPE_INVALID, ERR_PCA_SHAPE_LIMIT, PcaShapeGeometry, PcaShapeLimits, PcaShapeModel,
    PcaShapeOutsidePolicy, PcaShapePose,
};
use super::{Cancellation, Error};
use crate::fmath;
#[derive(Debug, Clone, Copy, Default)]
pub struct PcaShapePriorOptions<'m> {
    pub model: Option<&'m PcaShapeModel>,
    pub geometry: PcaShapeGeometry,
    pub coefficients_sd: &'m [f64],
    pub pose: PcaShapePose,
    pub outside: PcaShapeOutsidePolicy,
    pub weight: f64,
    pub max_coefficient_sd: f64,
    pub max_residual_mm: f64,
    pub limits: PcaShapeLimits,
}
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PcaShapePriorDiagnostics {
    pub model_id: String,
    pub provenance: String,
    pub weight: f64,
    pub coefficients_sd: Option<Vec<f64>>,
    pub pose: PcaShapePose,
    pub initial_field_rms_mm: f64,
    pub final_field_rms_mm: f64,
    pub initial_mean_potential: f64,
    pub final_mean_potential: f64,
    pub evaluated_voxels: i64,
    pub extrapolated_voxels: i64,
    pub outside_distribution: bool,
    pub limitation: String,
}
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PcaShapePriorResult {
    pub segmentation: LevelSetResult,
    pub prior: PcaShapePriorDiagnostics,
}
struct PcaShapeEvolution<'m> {
    model: &'m PcaShapeModel,
    options: PcaShapePriorOptions<'m>,
    target: Vec<f32>,
    diagnostics: PcaShapePriorDiagnostics,
}
fn invalid() -> Error {
    Error::sentinel(ERR_PCA_SHAPE_INVALID)
}
fn cancelled(ctx: Option<&dyn Cancellation>) -> Result<(), Error> {
    match level_set_context_error(ctx) {
        Some(e) => Err(e),
        None => Ok(()),
    }
}
pub fn shape_prior_detection_level_set(
    ctx: Option<&dyn Cancellation>,
    v: LevelSetVolume,
    init: &LevelSetInitialization,
    p: ShapeDetectionParams,
    o: LevelSetOptions,
    prior: PcaShapePriorOptions,
) -> Result<PcaShapePriorResult, Error> {
    let mut prior = prior;
    let mut o = o;
    cancelled(ctx)?;
    if !finite_non_negative(prior.weight) || prior.weight > 1e4 {
        return Err(invalid());
    }
    if !finite_non_negative(prior.max_coefficient_sd)
        || prior.max_coefficient_sd > 100.0
        || !finite_non_negative(prior.max_residual_mm)
        || prior.max_residual_mm > 1e6
        || prior.outside.0 > PcaShapeOutsidePolicy::NEAREST_BOUNDARY.0
    {
        return Err(invalid());
    }
    if prior.weight == 0.0 && prior.model.is_none() {
        if !prior.coefficients_sd.is_empty() {
            return Err(invalid());
        }
        let r = shape_detection_level_set(ctx, v, init, p, o)?;
        return Ok(PcaShapePriorResult {
            segmentation: r,
            prior: PcaShapePriorDiagnostics {
                limitation: "prior disabled; base solver result".to_string(),
                ..Default::default()
            },
        });
    }
    PcaShapeModel::parameters(
        prior.model,
        prior.coefficients_sd,
        &prior.pose,
        prior.outside,
    )?;
    let Some(model) = prior.model else {
        return Err(invalid());
    };
    let l = prior.limits.normalize()?;
    let n = prior.geometry.count(l)?;
    if prior.geometry != model.data_ref().geometry
        || prior.geometry.dimensions != [v.columns, v.rows, v.slices]
        || prior.geometry.spacing_mm != v.spacing_mm
    {
        return Err(Error::wrap(
            ERR_PCA_SHAPE_INVALID,
            "explicit grid/spacing mismatch",
        ));
    }
    let k = prior.coefficients_sd.len() as i64;
    if k > l.max_components || n * (k + 1) * 8 > l.max_work {
        return Err(Error::sentinel(ERR_PCA_SHAPE_LIMIT));
    }
    if prior.max_coefficient_sd == 0.0 {
        prior.max_coefficient_sd = 3.0;
    }
    if prior.max_residual_mm == 0.0 {
        prior.max_residual_mm = 1.0;
    }
    let mut d = PcaShapePriorDiagnostics {
        model_id: model.data_ref().id.clone(),
        provenance: model.data_ref().provenance.clone(),
        weight: prior.weight,
        coefficients_sd: (!prior.coefficients_sd.is_empty())
            .then(|| prior.coefficients_sd.to_vec()),
        pose: prior.pose,
        limitation: "fixed synthetic/research approximate SDF; clipped-field residual is not anatomical confidence; evolution differs from ITK's sparse-field/MAP optimizer".to_string(),
        ..Default::default()
    };
    for &c in prior.coefficients_sd {
        d.outside_distribution = d.outside_distribution || c.abs() > prior.max_coefficient_sd;
    }
    if prior.weight == 0.0 {
        let r = shape_detection_level_set(ctx, v, init, p, o)?;
        return Ok(PcaShapePriorResult {
            segmentation: r,
            prior: d,
        });
    }
    if p.evolution != ShapeEvolutionProfile::DENSE_BAND {
        return Err(Error::wrap(
            ERR_PCA_SHAPE_INVALID,
            "shape prior currently requires DenseBand",
        ));
    }
    if prior.outside == PcaShapeOutsidePolicy::REJECT_OUTSIDE {
        for x in [0, v.columns - 1] {
            for y in [0, v.rows - 1] {
                for z in [0, v.slices - 1] {
                    model.value(
                        prior.coefficients_sd,
                        &prior.pose,
                        prior.outside,
                        prior.geometry.point(x, y, z),
                    )?;
                }
            }
        }
    }
    let base = o.limits.normalize()?;
    let reserve = n * 4 + k * 16 + 8192;
    let budget = base.max_bytes.min(l.max_bytes);
    if reserve >= budget {
        return Err(Error::sentinel(ERR_PCA_SHAPE_LIMIT));
    }
    o.limits = base;
    o.limits.max_bytes = budget - reserve;
    let max_residual = prior.max_residual_mm;
    let mut state = PcaShapeEvolution {
        model,
        options: prior,
        target: Vec::new(),
        diagnostics: d,
    };
    let r = shape_detection_with_prior(ctx, v, init, p, o, Some(&mut state))?;
    cancelled(ctx)?;
    let mut diagnostics = state.diagnostics;
    diagnostics.outside_distribution =
        diagnostics.outside_distribution || diagnostics.initial_field_rms_mm > max_residual;
    Ok(PcaShapePriorResult {
        segmentation: r,
        prior: diagnostics,
    })
}
impl ShapePrior for PcaShapeEvolution<'_> {
    fn prepare(&mut self, g: &LevelSetGrid) -> Result<(), Error> {
        self.target = vec![0.0; g.n];
        let width = 3.0 * g.hmax;
        for i in 0..g.n {
            if i % 1024 == 0 {
                cancelled(g.ctx)?;
            }
            let (x, y, z) = g.coords(i);
            let point = self.options.geometry.point(
                x as i64 + g.origin[0],
                y as i64 + g.origin[1],
                z as i64 + g.origin[2],
            );
            let (v, out) = self.model.value(
                self.options.coefficients_sd,
                &self.options.pose,
                self.options.outside,
                point,
            )?;
            if out {
                self.diagnostics.extrapolated_voxels += 1;
            }
            self.target[i] = fmath::max(-width, fmath::min(width, v)) as f32;
        }
        self.diagnostics.evaluated_voxels = g.n as i64;
        cancelled(g.ctx)
    }
    fn target(&self) -> &[f32] {
        &self.target
    }
    fn weight(&self) -> f64 {
        self.options.weight
    }
    fn measure(
        &mut self,
        ctx: Option<&dyn Cancellation>,
        phi: &[f32],
        initial: bool,
    ) -> Result<(), Error> {
        let mut sum = 0.0f64;
        for (i, &v) in phi.iter().enumerate() {
            if i % 4096 == 0 {
                cancelled(ctx)?;
            }
            let delta = f64::from(v) - f64::from(self.target[i]);
            sum += delta * delta;
        }
        let mean = sum / phi.len() as f64;
        let (rms, energy) = (mean.sqrt(), self.options.weight * mean / 2.0);
        if initial {
            self.diagnostics.initial_field_rms_mm = rms;
            self.diagnostics.initial_mean_potential = energy;
        } else {
            self.diagnostics.final_field_rms_mm = rms;
            self.diagnostics.final_mean_potential = energy;
        }
        cancelled(ctx)
    }
}
