use super::dicom::{
    TRANSFORM_AFFINE, TRANSFORM_RIGID, TRANSFORM_RIGID_SCALE, transform_from_linear,
};
use super::error::{RegError, Sentinel};
use super::matrix::{Matrix3, finite_vec};
use super::method::ctx_check;
use super::transform::{DEFAULT_ORTHOGONALITY_TOLERANCE, Transform};
use super::{Affine, Similarity3D};
use crate::fmath;
use crate::numeric::linalg::{symmetric_eigen3, symmetric_eigen4};
use crate::optimize::Context;
use crate::render::Vec3;
use crate::roi::pdqsort_by::sort_slice;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LandmarkMode(pub i64);
impl LandmarkMode {
    pub const RIGID: LandmarkMode = LandmarkMode(0);
    pub const SIMILARITY: LandmarkMode = LandmarkMode(1);
    pub const AFFINE: LandmarkMode = LandmarkMode(2);
}
impl std::fmt::Display for LandmarkMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.0 {
            0 => f.write_str("rigid"),
            1 => f.write_str("similarity"),
            2 => f.write_str("affine"),
            n => write!(f, "LandmarkMode({n})"),
        }
    }
}
pub struct LandmarkResult {
    pub transform: Box<dyn Transform>,
    pub fre: f64,
    pub residuals: Vec<f64>,
}
fn landmarks(detail: impl AsRef<str>) -> RegError {
    RegError::new(Sentinel::Landmarks, detail)
}
pub fn register_landmarks(
    fixed: &[Vec3],
    moving: &[Vec3],
    mode: LandmarkMode,
) -> Result<LandmarkResult, RegError> {
    if fixed.len() != moving.len() {
        return Err(landmarks(format!(
            "{} fixed and {} moving points",
            fixed.len(),
            moving.len()
        )));
    }
    let need = if mode == LandmarkMode::AFFINE { 4 } else { 3 };
    if !(0..=2).contains(&mode.0) {
        return Err(landmarks(format!("unknown mode {}", mode.0)));
    }
    if fixed.len() < need {
        return Err(landmarks(format!(
            "{mode} fit needs at least {need} pairs, got {}",
            fixed.len()
        )));
    }
    for i in 0..fixed.len() {
        if !finite_vec(fixed[i]) || !finite_vec(moving[i]) {
            return Err(landmarks(format!("pair {} is not finite", i + 1)));
        }
    }
    let (fc, mc) = (centroid(fixed), centroid(moving));
    let mut cov = [[0f64; 3]; 3];
    for p in fixed {
        let d = p.sub(fc);
        let v = [d.x, d.y, d.z];
        for r in 0..3 {
            for c in 0..3 {
                cov[r][c] += v[r] * v[c];
            }
        }
    }
    let (spread, _) = symmetric_eigen3(cov).map_err(|e| landmarks(e.to_string()))?;
    let largest = fmath::max(spread[0], fmath::max(spread[1], spread[2]));
    if !(largest > 0.0) {
        return Err(landmarks("the fixed points coincide"));
    }
    let sorted = sort3(spread);
    if sorted[1] < 1e-10 * largest {
        return Err(landmarks("the fixed points are collinear"));
    }
    if mode == LandmarkMode::AFFINE && sorted[0] < 1e-10 * largest {
        return Err(landmarks("an affine fit needs non-coplanar fixed points"));
    }
    let mut kind = TRANSFORM_RIGID;
    let m = if mode == LandmarkMode::AFFINE {
        kind = TRANSFORM_AFFINE;
        least_squares_linear(fixed, moving, fc, mc)?
    } else {
        let mut m = horn_rotation(fixed, moving, fc, mc)?;
        if mode == LandmarkMode::SIMILARITY {
            let (mut num, mut den) = (0.0, 0.0);
            for i in 0..fixed.len() {
                let f = fixed[i].sub(fc);
                num += moving[i].sub(mc).dot(m.mul_vec(f));
                den += f.dot(f);
            }
            let scale = num / den;
            if !(scale > 0.0) {
                return Err(landmarks("the similarity scale is not positive"));
            }
            m = m.scale(scale);
            kind = TRANSFORM_RIGID_SCALE;
        }
        m
    };
    let offset = mc.sub(m.mul_vec(fc));
    let transform = transform_from_linear(&m, offset, kind, fc)
        .map_err(|e| RegError::wrap_text(Sentinel::Landmarks, &e))?;
    let mut residuals = vec![0.0; fixed.len()];
    let mut sum = 0.0;
    for i in 0..fixed.len() {
        let r = transform.transform_point(fixed[i]).sub(moving[i]).length();
        residuals[i] = r;
        sum += r * r;
    }
    Ok(LandmarkResult {
        transform,
        fre: (sum / fixed.len() as f64).sqrt(),
        residuals,
    })
}
pub(crate) fn centroid(points: &[Vec3]) -> Vec3 {
    let mut sum = Vec3::ZERO;
    for p in points {
        sum = sum.add(*p);
    }
    sum.scale(1.0 / points.len() as f64)
}
fn sort3(mut v: [f64; 3]) -> [f64; 3] {
    if v[0] > v[1] {
        v.swap(0, 1);
    }
    if v[1] > v[2] {
        v.swap(1, 2);
    }
    if v[0] > v[1] {
        v.swap(0, 1);
    }
    v
}
fn horn_rotation(fixed: &[Vec3], moving: &[Vec3], fc: Vec3, mc: Vec3) -> Result<Matrix3, RegError> {
    let mut s = [[0f64; 3]; 3];
    for i in 0..fixed.len() {
        let f = fixed[i].sub(fc);
        let g = moving[i].sub(mc);
        let fv = [f.x, f.y, f.z];
        let gv = [g.x, g.y, g.z];
        for a in 0..3 {
            for b in 0..3 {
                s[a][b] += fv[a] * gv[b];
            }
        }
    }
    let (sxx, sxy, sxz) = (s[0][0], s[0][1], s[0][2]);
    let (syx, syy, syz) = (s[1][0], s[1][1], s[1][2]);
    let (szx, szy, szz) = (s[2][0], s[2][1], s[2][2]);
    let n = [
        [sxx + syy + szz, syz - szy, szx - sxz, sxy - syx],
        [syz - szy, sxx - syy - szz, sxy + syx, szx + sxz],
        [szx - sxz, sxy + syx, -sxx + syy - szz, syz + szy],
        [sxy - syx, szx + sxz, syz + szy, -sxx - syy + szz],
    ];
    let (values, vectors) = symmetric_eigen4(n).map_err(|e| landmarks(e.to_string()))?;
    let mut best = 0;
    for i in 1..4 {
        if values[i] > values[best] {
            best = i;
        }
    }
    let (w, x, y, z) = (
        vectors[0][best],
        vectors[1][best],
        vectors[2][best],
        vectors[3][best],
    );
    let norm = (w * w + x * x + y * y + z * z).sqrt();
    if !(norm > 0.0) {
        return Err(landmarks("no rotation"));
    }
    let (w, x, y, z) = (w / norm, x / norm, y / norm, z / norm);
    Ok(Matrix3([
        [
            w * w + x * x - y * y - z * z,
            2.0 * (x * y - w * z),
            2.0 * (x * z + w * y),
        ],
        [
            2.0 * (x * y + w * z),
            w * w - x * x + y * y - z * z,
            2.0 * (y * z - w * x),
        ],
        [
            2.0 * (x * z - w * y),
            2.0 * (y * z + w * x),
            w * w - x * x - y * y + z * z,
        ],
    ]))
}
fn least_squares_linear(
    fixed: &[Vec3],
    moving: &[Vec3],
    fc: Vec3,
    mc: Vec3,
) -> Result<Matrix3, RegError> {
    let mut mf = [[0f64; 3]; 3];
    let mut ff = [[0f64; 3]; 3];
    for i in 0..fixed.len() {
        let f = fixed[i].sub(fc);
        let g = moving[i].sub(mc);
        let fv = [f.x, f.y, f.z];
        let gv = [g.x, g.y, g.z];
        for r in 0..3 {
            for c in 0..3 {
                mf[r][c] += gv[r] * fv[c];
                ff[r][c] += fv[r] * fv[c];
            }
        }
    }
    let inv = Matrix3(ff)
        .inverse()
        .map_err(|e| RegError::new(Sentinel::Landmarks, e.to_string()))?;
    Ok(Matrix3(mf).mul(&inv))
}
#[derive(Default)]
pub struct IcpOptions<'a> {
    pub mode: LandmarkMode,
    pub max_iterations: i64,
    pub tolerance: f64,
    pub max_fixed_points: i64,
    pub initial: Option<&'a dyn Transform>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IcpStopReason {
    #[default]
    None,
    InitiallyAligned,
    Tolerance,
    Worsened,
    MaximumIterations,
}
impl IcpStopReason {
    pub fn as_str(self) -> &'static str {
        match self {
            IcpStopReason::None => "",
            IcpStopReason::InitiallyAligned => "initially-aligned",
            IcpStopReason::Tolerance => "tolerance",
            IcpStopReason::Worsened => "worsened",
            IcpStopReason::MaximumIterations => "maximum-iterations",
        }
    }
}
#[derive(Default)]
pub struct IcpResult {
    pub transform: Option<Box<dyn Transform>>,
    pub mean_distance: f64,
    pub rms: f64,
    pub iterations: i64,
    pub converged: bool,
    pub stop_reason: IcpStopReason,
}
pub fn iterative_closest_point(
    ctx: Context<'_>,
    fixed: &[Vec3],
    moving: &[Vec3],
    opts: &IcpOptions,
) -> Result<IcpResult, RegError> {
    let max_iterations = if opts.max_iterations == 0 {
        50
    } else {
        opts.max_iterations
    };
    let tolerance = if opts.tolerance == 0.0 {
        1e-4
    } else {
        opts.tolerance
    };
    let max_fixed = if opts.max_fixed_points == 0 {
        20000
    } else {
        opts.max_fixed_points
    };
    if max_iterations < 0 || tolerance < 0.0 || !tolerance.is_finite() || max_fixed < 4 {
        return Err(landmarks("invalid ICP options"));
    }
    if fixed.len() < 4 || moving.len() < 4 {
        return Err(landmarks("ICP needs at least 4 points per cloud"));
    }
    ctx_check(ctx)?;
    for cloud in [fixed, moving] {
        for (i, p) in cloud.iter().enumerate() {
            if i % 256 == 0 {
                ctx_check(ctx)?;
            }
            if !finite_vec(*p) {
                return Err(landmarks(format!("point {} is not finite", i + 1)));
            }
        }
    }
    let max_fixed = max_fixed as usize;
    let sample: Vec<Vec3> = if fixed.len() > max_fixed {
        (0..max_fixed)
            .map(|i| fixed[i * fixed.len() / max_fixed])
            .collect()
    } else {
        fixed.to_vec()
    };
    let tree = KdTree::new(moving);
    let shift;
    let current: &dyn Transform = match opts.initial {
        Some(t) => t,
        None => {
            let mut a = Affine::new();
            a.set_offset(centroid(moving).sub(centroid(fixed)))?;
            shift = a;
            &shift
        }
    };
    let mut targets = vec![Vec3::ZERO; sample.len()];
    let (mut previous, _) = icp_distances(ctx, &tree, &sample, current, Some(&mut targets))?;
    let mut result = IcpResult::default();
    for iteration in 0..max_iterations {
        ctx_check(ctx)?;
        let fit = register_landmarks(&sample, &targets, opts.mode)?;
        if iteration == 0
            && previous == 0.0
            && let Some(clone) = clone_aligned_initial(current, opts.mode)
        {
            let (mean, rms) = icp_distances(ctx, &tree, &sample, clone.as_ref(), None)?;
            if mean == 0.0 && rms == 0.0 {
                return Ok(IcpResult {
                    transform: Some(clone),
                    mean_distance: mean,
                    rms,
                    iterations: 0,
                    converged: true,
                    stop_reason: IcpStopReason::InitiallyAligned,
                });
            }
        }
        let (mean, rms) = icp_distances(
            ctx,
            &tree,
            &sample,
            fit.transform.as_ref(),
            Some(&mut targets),
        )?;
        result = IcpResult {
            transform: Some(fit.transform),
            mean_distance: mean,
            rms,
            iterations: iteration + 1,
            converged: false,
            stop_reason: IcpStopReason::MaximumIterations,
        };
        let improvement = previous - mean;
        if improvement < 0.0 {
            result.stop_reason = IcpStopReason::Worsened;
            break;
        }
        if improvement < tolerance {
            result.stop_reason = IcpStopReason::Tolerance;
            result.converged = true;
            break;
        }
        previous = mean;
    }
    Ok(result)
}
fn clone_aligned_initial(
    initial: &dyn Transform,
    mode: LandmarkMode,
) -> Option<Box<dyn Transform>> {
    let m = initial.as_matrix_transform()?.matrix();
    match mode {
        LandmarkMode::RIGID => {
            if !m.finite()
                || !m.orthogonal(DEFAULT_ORTHOGONALITY_TOLERANCE)
                || m.determinant() <= 0.0
            {
                return None;
            }
        }
        LandmarkMode::SIMILARITY => {
            Similarity3D::new()
                .set_matrix_with_tolerance(&m, DEFAULT_ORTHOGONALITY_TOLERANCE)
                .ok()?;
        }
        LandmarkMode::AFFINE => {
            if !m.finite() {
                return None;
            }
        }
        _ => return None,
    }
    Some(initial.clone_box())
}
fn icp_distances(
    ctx: Context<'_>,
    tree: &KdTree,
    sample: &[Vec3],
    transform: &dyn Transform,
    mut targets: Option<&mut Vec<Vec3>>,
) -> Result<(f64, f64), RegError> {
    let (mut sum, mut sum2) = (0.0, 0.0);
    for (i, &p) in sample.iter().enumerate() {
        if i % 256 == 0 {
            ctx_check(ctx)?;
        }
        let image = transform.transform_point(p);
        if !finite_vec(image) {
            return Err(landmarks(format!(
                "ICP mapped point {} is not finite",
                i + 1
            )));
        }
        let (q, d2) = tree.nearest(image);
        if !d2.is_finite() {
            return Err(landmarks(format!("ICP distance {} is not finite", i + 1)));
        }
        if let Some(t) = targets.as_mut() {
            t[i] = q;
        }
        sum += d2.sqrt();
        sum2 += d2;
    }
    if sum.is_infinite() || sum2.is_infinite() {
        return Err(landmarks("ICP distance sum is not finite"));
    }
    ctx_check(ctx)?;
    let n = sample.len() as f64;
    Ok((sum / n, (sum2 / n).sqrt()))
}
#[derive(Debug, Clone, Copy)]
pub(crate) struct KdNode {
    pub point: usize,
    pub axis: usize,
    pub left: i64,
    pub right: i64,
}
#[derive(Debug, Clone)]
pub(crate) struct KdTree {
    pub points: Vec<Vec3>,
    pub nodes: Vec<KdNode>,
}
pub(crate) fn coordinate(p: Vec3, axis: usize) -> f64 {
    match axis {
        0 => p.x,
        1 => p.y,
        _ => p.z,
    }
}
impl KdTree {
    fn new(points: &[Vec3]) -> KdTree {
        let mut t = KdTree {
            points: points.to_vec(),
            nodes: Vec::with_capacity(points.len()),
        };
        let mut indices: Vec<usize> = (0..points.len()).collect();
        t.build(&mut indices, 0);
        t
    }
    fn build(&mut self, indices: &mut [usize], depth: usize) -> i64 {
        if indices.is_empty() {
            return -1;
        }
        let axis = depth % 3;
        let points = &self.points;
        sort_slice(indices, &|a: &usize, b: &usize| {
            coordinate(points[*a], axis) < coordinate(points[*b], axis)
        });
        let mid = indices.len() / 2;
        let node = self.nodes.len();
        self.nodes.push(KdNode {
            point: indices[mid],
            axis,
            left: -1,
            right: -1,
        });
        let (lower, upper) = indices.split_at_mut(mid);
        let left = self.build(lower, depth + 1);
        let right = self.build(&mut upper[1..], depth + 1);
        self.nodes[node].left = left;
        self.nodes[node].right = right;
        node as i64
    }
    fn nearest(&self, q: Vec3) -> (Vec3, f64) {
        let mut best: i64 = -1;
        let mut best_d2 = f64::INFINITY;
        self.search(0, q, &mut best, &mut best_d2);
        if best < 0 {
            return (Vec3::ZERO, f64::INFINITY);
        }
        (self.points[best as usize], best_d2)
    }
    fn search(&self, node: i64, q: Vec3, best: &mut i64, best_d2: &mut f64) {
        if node < 0 {
            return;
        }
        let n = self.nodes[node as usize];
        let p = self.points[n.point];
        let d2 = p.sub(q).dot(p.sub(q));
        if d2 < *best_d2 {
            *best = n.point as i64;
            *best_d2 = d2;
        }
        let diff = coordinate(q, n.axis) - coordinate(p, n.axis);
        let (mut near, mut far) = (n.left, n.right);
        if diff > 0.0 {
            std::mem::swap(&mut near, &mut far);
        }
        self.search(near, q, best, best_d2);
        if diff * diff < *best_d2 {
            self.search(far, q, best, best_d2);
        }
    }
}
