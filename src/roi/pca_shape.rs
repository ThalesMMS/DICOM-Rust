#![allow(
    clippy::neg_cmp_op_on_partial_ord,
    clippy::manual_range_contains,
    clippy::needless_range_loop,
    clippy::too_many_arguments,
    clippy::manual_clamp,
    clippy::map_entry
)]
use super::{Cancellation, Error, ctx_err};
use crate::fmath;
pub const ERR_PCA_SHAPE_INVALID: &str = "roi: invalid research PCA shape model";
pub const ERR_PCA_SHAPE_LIMIT: &str = "roi: research PCA shape resource limit";
pub const ERR_PCA_SHAPE_OUTSIDE: &str = "roi: PCA shape point outside model domain";
pub const PCA_SHAPE_CONVENTION: &str = "negative-inside-mm";
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PcaShapeGeometry {
    pub dimensions: [i64; 3],
    pub spacing_mm: [f64; 3],
    pub origin_mm: [f64; 3],
    pub direction: [f64; 9],
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PcaShapeLimits {
    pub max_voxels: i64,
    pub max_components: i64,
    pub max_samples: i64,
    pub max_bytes: i64,
    pub max_work: i64,
}
fn invalid() -> Error {
    Error::sentinel(ERR_PCA_SHAPE_INVALID)
}
fn limit() -> Error {
    Error::sentinel(ERR_PCA_SHAPE_LIMIT)
}
fn poll(ctx: Option<&dyn Cancellation>) -> Result<(), Error> {
    match ctx_err(ctx) {
        Some(e) => Err(e),
        None => Ok(()),
    }
}
impl PcaShapeLimits {
    pub(crate) fn normalize(self) -> Result<PcaShapeLimits, Error> {
        let mut l = self;
        if l.max_voxels < 0
            || l.max_components < 0
            || l.max_samples < 0
            || l.max_bytes < 0
            || l.max_work < 0
        {
            return Err(invalid());
        }
        if l.max_voxels == 0 {
            l.max_voxels = 1 << 20;
        }
        if l.max_components == 0 {
            l.max_components = 8;
        }
        if l.max_samples == 0 {
            l.max_samples = 16;
        }
        if l.max_bytes == 0 {
            l.max_bytes = 256 << 20;
        }
        if l.max_work == 0 {
            l.max_work = 1 << 28;
        }
        if l.max_voxels > 4 << 20
            || l.max_components > 16
            || l.max_samples > 32
            || l.max_bytes > 512 << 20
            || l.max_work > 1 << 30
        {
            return Err(limit());
        }
        Ok(l)
    }
}
fn finite(v: f64) -> bool {
    v.is_finite()
}
fn rotation(r: &[f64; 9]) -> bool {
    for &v in r {
        if !finite(v) || v.abs() > 1.0 + 1e-10 {
            return false;
        }
    }
    for a in 0..3 {
        for b in 0..3 {
            let mut dot = 0.0;
            for j in 0..3 {
                dot += r[j * 3 + a] * r[j * 3 + b];
            }
            let want = if a == b { 1.0 } else { 0.0 };
            if (dot - want).abs() > 1e-10 {
                return false;
            }
        }
    }
    let det = r[0] * (r[4] * r[8] - r[5] * r[7]) - r[1] * (r[3] * r[8] - r[5] * r[6])
        + r[2] * (r[3] * r[7] - r[4] * r[6]);
    (det - 1.0).abs() < 1e-10
}
impl PcaShapeGeometry {
    pub(crate) fn count(&self, l: PcaShapeLimits) -> Result<i64, Error> {
        let mut n: i64 = 1;
        if !rotation(&self.direction) {
            return Err(Error::wrap(
                ERR_PCA_SHAPE_INVALID,
                "orthonormal right-handed direction required",
            ));
        }
        for (a, &d) in self.dimensions.iter().enumerate() {
            if d <= 0 {
                return Err(invalid());
            }
            if d > l.max_voxels / n {
                return Err(limit());
            }
            n *= d;
            let sp = self.spacing_mm[a];
            let o = self.origin_mm[a];
            if !finite(sp) || sp < 1e-6 || sp > 1e6 || !finite(o) || o.abs() + d as f64 * sp > 1e9 {
                return Err(Error::wrap(ERR_PCA_SHAPE_INVALID, "physical grid"));
            }
        }
        for a in 0..3 {
            let mut extent = self.origin_mm[a].abs();
            for b in 0..3 {
                extent += self.direction[3 * a + b].abs()
                    * (self.dimensions[b] - 1) as f64
                    * self.spacing_mm[b];
            }
            if extent > 1e9 {
                return Err(invalid());
            }
        }
        Ok(n)
    }
    pub fn point(&self, x: i64, y: i64, z: i64) -> [f64; 3] {
        let i = [
            x as f64 * self.spacing_mm[0],
            y as f64 * self.spacing_mm[1],
            z as f64 * self.spacing_mm[2],
        ];
        let mut p = self.origin_mm;
        for a in 0..3 {
            for b in 0..3 {
                p[a] += self.direction[3 * a + b] * i[b];
            }
        }
        p
    }
}
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PcaShapeModelData {
    pub version: i64,
    pub id: String,
    pub provenance: String,
    pub convention: String,
    pub geometry: PcaShapeGeometry,
    pub training_samples: i64,
    pub mean_mm: Vec<f64>,
    pub components: Vec<Vec<f64>>,
    pub standard_deviations_mm: Vec<f64>,
}
#[derive(Debug, Clone, PartialEq)]
pub struct PcaShapeModel {
    data: PcaShapeModelData,
}
fn check_field(
    ctx: Option<&dyn Cancellation>,
    field: &[f64],
    n: i64,
    signs: bool,
) -> Result<(), Error> {
    if field.len() as i64 != n {
        return Err(Error::wrap(ERR_PCA_SHAPE_INVALID, "field length"));
    }
    let (mut negative, mut positive) = (false, false);
    for (i, &v) in field.iter().enumerate() {
        if i % 4096 == 0 {
            poll(ctx)?;
        }
        if !finite(v) || v.abs() > 1e6 {
            return Err(Error::wrap(ERR_PCA_SHAPE_INVALID, "finite mm field"));
        }
        negative = negative || v < 0.0;
        positive = positive || v > 0.0;
    }
    if signs && (!negative || !positive) {
        return Err(Error::wrap(
            ERR_PCA_SHAPE_INVALID,
            "shape requires inside and outside samples",
        ));
    }
    Ok(())
}
impl PcaShapeModel {
    pub fn new(
        ctx: Option<&dyn Cancellation>,
        d: &PcaShapeModelData,
        limits: PcaShapeLimits,
    ) -> Result<PcaShapeModel, Error> {
        poll(ctx)?;
        let l = limits.normalize()?;
        let n = d.geometry.count(l)?;
        let k = d.components.len() as i64;
        if d.version != 1
            || d.convention != PCA_SHAPE_CONVENTION
            || d.id.is_empty()
            || d.provenance.is_empty()
            || d.id.len() > 1024
            || d.provenance.len() > 4096
            || d.training_samples < 2
            || k >= d.training_samples
        {
            return Err(invalid());
        }
        if k > l.max_components
            || d.training_samples > l.max_samples
            || n * (k + 1) * 8 + 8192 > l.max_bytes
            || n * (k * k + k + 1) > l.max_work
        {
            return Err(limit());
        }
        if d.standard_deviations_mm.len() as i64 != k {
            return Err(invalid());
        }
        check_field(ctx, &d.mean_mm, n, true)?;
        for (a, component) in d.components.iter().enumerate() {
            let sd = d.standard_deviations_mm[a];
            if !finite(sd) || sd <= 0.0 || sd > 1e9 {
                return Err(invalid());
            }
            check_field(ctx, component, n, false)?;
            for b in 0..=a {
                let mut dot = 0.0;
                for (i, &v) in component.iter().enumerate() {
                    if i % 4096 == 0 {
                        poll(ctx)?;
                    }
                    dot += v * d.components[b][i];
                }
                let want = if a == b { 1.0 } else { 0.0 };
                if (dot - want).abs() > 1e-8 {
                    return Err(Error::wrap(
                        ERR_PCA_SHAPE_INVALID,
                        "components not orthonormal",
                    ));
                }
            }
        }
        let data = d.clone();
        poll(ctx)?;
        Ok(PcaShapeModel { data })
    }
    pub fn data(
        model: Option<&PcaShapeModel>,
        ctx: Option<&dyn Cancellation>,
        l: PcaShapeLimits,
    ) -> Result<PcaShapeModelData, Error> {
        let Some(m) = model else {
            return Err(invalid());
        };
        Ok(PcaShapeModel::new(ctx, &m.data, l)?.data)
    }
    pub(crate) fn parameters(
        model: Option<&PcaShapeModel>,
        coeff: &[f64],
        pose: &PcaShapePose,
        policy: PcaShapeOutsidePolicy,
    ) -> Result<(), Error> {
        let Some(m) = model else {
            return Err(invalid());
        };
        if coeff.len() != m.data.components.len()
            || policy.0 > PcaShapeOutsidePolicy::NEAREST_BOUNDARY.0
        {
            return Err(invalid());
        }
        pose.validate()?;
        for &v in coeff {
            if !finite(v) || v.abs() > 100.0 {
                return Err(invalid());
            }
        }
        Ok(())
    }
    pub(crate) fn value(
        &self,
        coeff: &[f64],
        pose: &PcaShapePose,
        policy: PcaShapeOutsidePolicy,
        point: [f64; 3],
    ) -> Result<(f64, bool), Error> {
        let mut p = [0.0; 3];
        let mut q = [0.0; 3];
        for a in 0..3 {
            if !finite(point[a]) || point[a].abs() > 1e9 {
                return Err(invalid());
            }
            p[a] = pose.translation_mm[a];
            for b in 0..3 {
                p[a] += pose.rotation[3 * a + b] * point[b];
            }
        }
        let g = &self.data.geometry;
        let mut outside = false;
        let mut lo = [0i64; 3];
        let mut hi = [0i64; 3];
        let mut f = [0.0; 3];
        for a in 0..3 {
            for b in 0..3 {
                q[a] += g.direction[3 * b + a] * (p[b] - g.origin_mm[b]);
            }
            q[a] /= g.spacing_mm[a];
            let last = (g.dimensions[a] - 1) as f64;
            if q[a] < -1e-8 || q[a] > last + 1e-8 {
                outside = true;
            }
            q[a] = fmath::max(0.0, fmath::min(last, q[a]));
            lo[a] = q[a].floor() as i64;
            hi[a] = (lo[a] + 1).min(g.dimensions[a] - 1);
            f[a] = q[a] - lo[a] as f64;
        }
        if outside && policy == PcaShapeOutsidePolicy::REJECT_OUTSIDE {
            return Err(Error::sentinel(ERR_PCA_SHAPE_OUTSIDE));
        }
        let mut value = 0.0;
        for z in 0..2 {
            for y in 0..2 {
                for x in 0..2 {
                    let mut pos = lo;
                    let mut w = 1.0;
                    for (a, b) in [x, y, z].into_iter().enumerate() {
                        if b == 1 {
                            pos[a] = hi[a];
                            w *= f[a];
                        } else {
                            w *= 1.0 - f[a];
                        }
                    }
                    let i =
                        ((pos[2] * g.dimensions[1] + pos[1]) * g.dimensions[0] + pos[0]) as usize;
                    let mut v = self.data.mean_mm[i];
                    for (a, &c) in coeff.iter().enumerate() {
                        v += c * self.data.standard_deviations_mm[a] * self.data.components[a][i];
                    }
                    value += w * v;
                }
            }
        }
        if !finite(value) || value.abs() > 1e6 {
            return Err(invalid());
        }
        Ok((value, outside))
    }
    pub fn evaluate(
        model: Option<&PcaShapeModel>,
        ctx: Option<&dyn Cancellation>,
        coeff: &[f64],
        pose: &PcaShapePose,
        points: &[[f64; 3]],
        policy: PcaShapeOutsidePolicy,
        limits: PcaShapeLimits,
    ) -> Result<PcaShapeEvaluation, Error> {
        poll(ctx)?;
        PcaShapeModel::parameters(model, coeff, pose, policy)?;
        let Some(m) = model else {
            return Err(invalid());
        };
        let l = limits.normalize()?;
        let np = points.len() as i64;
        if coeff.len() as i64 > l.max_components
            || np > l.max_voxels
            || np * 8 + 8192 > l.max_bytes
            || np * (coeff.len() as i64 + 1) * 8 > l.max_work
        {
            return Err(limit());
        }
        let mut r = PcaShapeEvaluation {
            field_mm: vec![0.0; points.len()],
            outside_samples: 0,
        };
        for (i, &p) in points.iter().enumerate() {
            if i % 1024 == 0 {
                poll(ctx)?;
            }
            let (v, out) = m.value(coeff, pose, policy, p)?;
            r.field_mm[i] = v;
            if out {
                r.outside_samples += 1;
            }
        }
        poll(ctx)?;
        Ok(r)
    }
    pub fn fit(
        model: Option<&PcaShapeModel>,
        ctx: Option<&dyn Cancellation>,
        geometry: &PcaShapeGeometry,
        field: &[f64],
        max_coefficient_sd: f64,
        max_residual_mm: f64,
        limits: PcaShapeLimits,
    ) -> Result<PcaShapeFit, Error> {
        poll(ctx)?;
        let Some(m) = model else {
            return Err(invalid());
        };
        if *geometry != m.data.geometry
            || !(max_coefficient_sd > 0.0 && max_coefficient_sd <= 100.0)
            || !(max_residual_mm > 0.0 && max_residual_mm <= 1e6)
            || !finite(max_coefficient_sd)
            || !finite(max_residual_mm)
        {
            return Err(invalid());
        }
        let l = limits.normalize()?;
        let n = geometry.count(l)?;
        let k = m.data.components.len() as i64;
        if k > l.max_components || n * (k + 1) * 2 > l.max_work || k * 8 + 8192 > l.max_bytes {
            return Err(limit());
        }
        check_field(ctx, field, n, true)?;
        let d = &m.data;
        let mut r = PcaShapeFit {
            model_id: d.id.clone(),
            coefficients_sd: vec![0.0; k as usize],
            residual_rms_mm: 0.0,
            outside_distribution: false,
        };
        for (a, u) in d.components.iter().enumerate() {
            let mut dot = 0.0;
            for (i, &v) in u.iter().enumerate() {
                if i % 4096 == 0 {
                    poll(ctx)?;
                }
                dot += (field[i] - d.mean_mm[i]) * v;
            }
            r.coefficients_sd[a] = dot / d.standard_deviations_mm[a];
            if r.coefficients_sd[a].abs() > max_coefficient_sd {
                r.outside_distribution = true;
            }
        }
        let mut sum = 0.0;
        for (i, &v) in field.iter().enumerate() {
            if i % 4096 == 0 {
                poll(ctx)?;
            }
            let mut recon = d.mean_mm[i];
            for (a, &c) in r.coefficients_sd.iter().enumerate() {
                recon += c * d.standard_deviations_mm[a] * d.components[a][i];
            }
            let delta = v - recon;
            sum += delta * delta;
        }
        r.residual_rms_mm = (sum / n as f64).sqrt();
        r.outside_distribution = r.outside_distribution || r.residual_rms_mm > max_residual_mm;
        if !finite(r.residual_rms_mm) {
            return Err(invalid());
        }
        poll(ctx)?;
        Ok(r)
    }
    pub fn data_ref(&self) -> &PcaShapeModelData {
        &self.data
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PcaShapePose {
    pub rotation: [f64; 9],
    pub translation_mm: [f64; 3],
}
pub fn identity_pca_shape_pose() -> PcaShapePose {
    PcaShapePose {
        rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
        translation_mm: [0.0; 3],
    }
}
impl PcaShapePose {
    fn validate(&self) -> Result<(), Error> {
        if !rotation(&self.rotation) {
            return Err(invalid());
        }
        for &v in &self.translation_mm {
            if !finite(v) || v.abs() > 1e9 {
                return Err(invalid());
            }
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PcaShapeOutsidePolicy(pub u8);
impl PcaShapeOutsidePolicy {
    pub const REJECT_OUTSIDE: PcaShapeOutsidePolicy = PcaShapeOutsidePolicy(0);
    pub const NEAREST_BOUNDARY: PcaShapeOutsidePolicy = PcaShapeOutsidePolicy(1);
}
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PcaShapeEvaluation {
    pub field_mm: Vec<f64>,
    pub outside_samples: i64,
}
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PcaShapeFit {
    pub model_id: String,
    pub coefficients_sd: Vec<f64>,
    pub residual_rms_mm: f64,
    pub outside_distribution: bool,
}
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PcaShapeTrainingSample {
    pub id: String,
    pub geometry: PcaShapeGeometry,
    pub field_mm: Vec<f64>,
}
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PcaShapeTrainOptions {
    pub id: String,
    pub provenance: String,
    pub components: i64,
    pub limits: PcaShapeLimits,
}
pub fn train_pca_shape_model(
    ctx: Option<&dyn Cancellation>,
    samples: &[PcaShapeTrainingSample],
    o: &PcaShapeTrainOptions,
) -> Result<PcaShapeModel, Error> {
    poll(ctx)?;
    let l = o.limits.normalize()?;
    let m = samples.len();
    if m < 2
        || o.id.is_empty()
        || o.provenance.is_empty()
        || o.id.len() > 1024
        || o.provenance.len() > 4096
        || o.components < 0
    {
        return Err(invalid());
    }
    if m as i64 > l.max_samples || o.components > l.max_components {
        return Err(limit());
    }
    let mut k = o.components;
    if k == 0 {
        k = l.max_components;
    }
    let k = k.min(m as i64 - 1);
    let g = samples[0].geometry;
    let n = g.count(l)?;
    let mi = m as i64;
    let bytes = n * (k + 1) * 16 + mi * mi * 16 + mi * 64 + 16384;
    let work = n * (mi * mi + mi * k + k * k + 2 * mi + 2 * k + 4) + mi * mi * mi * 64;
    if bytes > l.max_bytes || work > l.max_work {
        return Err(limit());
    }
    let mut ids: Vec<&str> = Vec::new();
    for s in samples {
        if s.id.is_empty() || s.id.len() > 1024 || ids.contains(&s.id.as_str()) || s.geometry != g {
            return Err(Error::wrap(
                ERR_PCA_SHAPE_INVALID,
                "sample identity/geometry",
            ));
        }
        ids.push(&s.id);
        check_field(ctx, &s.field_mm, n, true)?;
    }
    let n = n as usize;
    let mut mean = vec![0.0; n];
    for (i, mu) in mean.iter_mut().enumerate() {
        if i % 4096 == 0 {
            poll(ctx)?;
        }
        for s in samples {
            *mu += s.field_mm[i] / m as f64;
        }
    }
    let mut gram = vec![0.0; m * m];
    for a in 0..m {
        for b in 0..=a {
            let mut sum = 0.0;
            for (i, &mu) in mean.iter().enumerate() {
                if i % 4096 == 0 {
                    poll(ctx)?;
                }
                sum += (samples[a].field_mm[i] - mu) * (samples[b].field_mm[i] - mu);
            }
            gram[a * m + b] = sum / (m - 1) as f64;
            gram[b * m + a] = gram[a * m + b];
        }
    }
    let (values, vectors) = jacobi(ctx, &mut gram, m)?;
    let mut order: Vec<usize> = (0..m).collect();
    order.sort_by(|&a, &b| {
        if values[a] > values[b] {
            std::cmp::Ordering::Less
        } else if values[b] > values[a] {
            std::cmp::Ordering::Greater
        } else {
            std::cmp::Ordering::Equal
        }
    });
    let mut d = PcaShapeModelData {
        version: 1,
        id: o.id.clone(),
        provenance: o.provenance.clone(),
        convention: PCA_SHAPE_CONVENTION.into(),
        geometry: g,
        training_samples: mi,
        mean_mm: mean,
        components: Vec::new(),
        standard_deviations_mm: Vec::new(),
    };
    let maximum = fmath::max(0.0, values[order[0]]);
    for &a in &order {
        let lambda = values[a];
        if d.components.len() as i64 >= k || lambda <= fmath::max(1e-18, maximum * 1e-12) {
            break;
        }
        let mut u = vec![0.0; n];
        let denom = ((m - 1) as f64 * lambda).sqrt();
        let mut norm = 0.0;
        let mut largest = 0;
        for i in 0..n {
            if i % 4096 == 0 {
                poll(ctx)?;
            }
            let mu = d.mean_mm[i];
            for (j, s) in samples.iter().enumerate() {
                u[i] += (s.field_mm[i] - mu) * vectors[j * m + a] / denom;
            }
            norm += u[i] * u[i];
            if u[i].abs() > u[largest].abs() {
                largest = i;
            }
        }
        if !(norm > 0.0) || !finite(norm) {
            return Err(invalid());
        }
        let mut scale = 1.0 / norm.sqrt();
        if u[largest] < 0.0 {
            scale = -scale;
        }
        for (i, x) in u.iter_mut().enumerate() {
            if i % 4096 == 0 {
                poll(ctx)?;
            }
            *x *= scale;
        }
        d.components.push(u);
        d.standard_deviations_mm.push(lambda.sqrt());
    }
    PcaShapeModel::new(ctx, &d, l)
}
fn jacobi(
    ctx: Option<&dyn Cancellation>,
    a: &mut [f64],
    n: usize,
) -> Result<(Vec<f64>, Vec<f64>), Error> {
    let mut v = vec![0.0; n * n];
    for i in 0..n {
        v[i * n + i] = 1.0;
    }
    for _sweep in 0..64 {
        poll(ctx)?;
        let (mut off, mut diag): (f64, f64) = (0.0, 0.0);
        for i in 0..n {
            diag = fmath::max(diag, a[i * n + i].abs());
            for j in i + 1..n {
                off = fmath::max(off, a[i * n + j].abs());
            }
        }
        if off <= fmath::max(1e-20, diag * 1e-13) {
            let values = (0..n).map(|i| a[i * n + i]).collect();
            return Ok((values, v));
        }
        for p in 0..n {
            for q in p + 1..n {
                poll(ctx)?;
                let apq = a[p * n + q];
                if apq.abs() <= fmath::max(1e-22, diag * 1e-15) {
                    continue;
                }
                let tau = (a[q * n + q] - a[p * n + p]) / (2.0 * apq);
                let t = (1.0 / (tau.abs() + fmath::hypot(1.0, tau))).copysign(tau);
                let c = 1.0 / fmath::hypot(1.0, t);
                let s = t * c;
                let (app, aqq) = (a[p * n + p], a[q * n + q]);
                a[p * n + p] = app - t * apq;
                a[q * n + q] = aqq + t * apq;
                a[p * n + q] = 0.0;
                a[q * n + p] = 0.0;
                for k in 0..n {
                    if k != p && k != q {
                        let (akp, akq) = (a[k * n + p], a[k * n + q]);
                        a[k * n + p] = c * akp - s * akq;
                        a[p * n + k] = a[k * n + p];
                        a[k * n + q] = s * akp + c * akq;
                        a[q * n + k] = a[k * n + q];
                    }
                    let (vkp, vkq) = (v[k * n + p], v[k * n + q]);
                    v[k * n + p] = c * vkp - s * vkq;
                    v[k * n + q] = s * vkp + c * vkq;
                }
            }
        }
    }
    Err(Error::wrap(
        ERR_PCA_SHAPE_LIMIT,
        "PCA eigensolve did not converge in 64 sweeps",
    ))
}
