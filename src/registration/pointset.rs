use super::error::{RegError, Sentinel};
use super::image_component::{ComponentInfo, MetricLimits, multi};
use super::landmark::{KdNode, KdTree, coordinate};
use super::matrix::{Jacobian, finite, finite_slice, finite_vec};
use super::metric::Metric;
use super::transform::Transform;
use crate::fmath;
use crate::fmath::rand::Rand;
use crate::render::Vec3;
use crate::render::parallel::{CancelToken, Cancellation, ctx_err};
use std::collections::HashMap;
fn check(ctx: &dyn Cancellation) -> Result<(), RegError> {
    match ctx_err(Some(ctx)) {
        Some(e) => Err(e.into()),
        None => Ok(()),
    }
}
#[derive(Debug, Clone)]
pub struct PointSetMetric {
    fixed: Vec<Vec3>,
    moving: Vec<Vec3>,
    pub(crate) info: ComponentInfo,
}
impl PointSetMetric {
    pub fn new(
        ctx: Option<&dyn Cancellation>,
        fixed: &[Vec3],
        moving: &[Vec3],
        frame: &str,
        limits: MetricLimits,
    ) -> Result<PointSetMetric, RegError> {
        let l = limits.checked()?;
        let base = 4096u64 + l.parameters as u64 * 128;
        if base > l.bytes || fixed.len() as u64 > (l.bytes - base) / 48 {
            return Err(multi());
        }
        let bytes = base + fixed.len() as u64 * 48;
        let Some(ctx) = ctx else {
            return Err(multi());
        };
        if frame.is_empty()
            || frame.len() > 1024
            || fixed.is_empty()
            || fixed.len() != moving.len()
            || fixed.len() as i64 > l.points
            || bytes > l.bytes
        {
            return Err(multi());
        }
        check(ctx)?;
        for i in 0..fixed.len() {
            if i % 64 == 0 {
                check(ctx)?;
            }
            if !finite_vec(fixed[i]) || !finite_vec(moving[i]) {
                return Err(multi());
            }
        }
        check(ctx)?;
        Ok(PointSetMetric {
            fixed: fixed.to_vec(),
            moving: moving.to_vec(),
            info: ComponentInfo {
                frame: frame.to_string(),
                units: "mm²".to_string(),
                points: fixed.len() as i64,
                channels: 0,
                parameters: l.parameters,
                bytes,
            },
        })
    }
    pub(crate) fn evaluate_component(
        &self,
        ctx: Option<&dyn Cancellation>,
        t: &dyn Transform,
        derivatives: bool,
    ) -> Result<(f64, Vec<f64>, i64), (RegError, i64)> {
        let p = t.number_of_parameters();
        let Some(ctx) = ctx else {
            return Err((multi(), 0));
        };
        if self.fixed.is_empty() || p < 1 || p as i64 > self.info.parameters {
            return Err((multi(), 0));
        }
        let mut g = vec![0.0; p];
        let mut j = Jacobian::default();
        let mut value = 0.0;
        for (i, &f) in self.fixed.iter().enumerate() {
            if i % 64 == 0 {
                check(ctx).map_err(|e| (e, i as i64))?;
            }
            let d = t.transform_point(f).sub(self.moving[i]);
            if !finite_vec(d) {
                return Err((multi(), i as i64));
            }
            value += d.x * d.x + d.y * d.y + d.z * d.z;
            if derivatives {
                t.jacobian_with_respect_to_parameters(f, &mut j);
                for (k, gk) in g.iter_mut().enumerate() {
                    *gk += 2.0 * (d.x * j.at(0, k) + d.y * j.at(1, k) + d.z * j.at(2, k));
                }
            }
        }
        let count = self.fixed.len() as i64;
        check(ctx).map_err(|e| (e, count))?;
        let n = self.fixed.len() as f64;
        value /= n;
        for gk in g.iter_mut() {
            *gk /= n;
        }
        if !finite(value) || !finite_slice(&g) {
            return Err((multi(), count));
        }
        if !derivatives {
            g = Vec::new();
        }
        Ok((value, g, count))
    }
    pub fn value_context(
        &self,
        ctx: Option<&dyn Cancellation>,
        t: &dyn Transform,
    ) -> Result<f64, RegError> {
        self.evaluate_component(ctx, t, false)
            .map(|r| r.0)
            .map_err(|e| e.0)
    }
    pub fn value_and_derivative_context(
        &self,
        ctx: Option<&dyn Cancellation>,
        t: &dyn Transform,
    ) -> Result<(f64, Vec<f64>), RegError> {
        self.evaluate_component(ctx, t, true)
            .map(|r| (r.0, r.1))
            .map_err(|e| e.0)
    }
}
#[derive(Debug, Clone, PartialEq, Default)]
pub struct GaussianPointSetOptions {
    pub sigma_mm: f64,
    pub neighbors: i64,
    pub max_neighbors: i64,
    pub sample_count: i64,
    pub workers: i64,
    pub seed: i64,
    pub frame_of_reference: String,
    pub fixed_labels: Option<Vec<u32>>,
    pub moving_labels: Option<Vec<u32>>,
    pub limits: MetricLimits,
    pub max_node_visits: u64,
}
#[derive(Debug, Clone)]
pub struct GaussianPointSetMetric {
    fixed: Vec<Vec3>,
    samples: Vec<usize>,
    fixed_labels: Option<Vec<u32>>,
    moving_labels: Option<Vec<u32>>,
    tree: KdTree,
    sigma2: f64,
    neighbors: usize,
    max_visits: u64,
    pub(crate) info: ComponentInfo,
}
fn gaussian() -> RegError {
    RegError::bare(Sentinel::GaussianPointSet)
}
impl GaussianPointSetMetric {
    pub fn new(
        ctx: Option<&dyn Cancellation>,
        fixed: &[Vec3],
        moving: &[Vec3],
        o: &GaussianPointSetOptions,
    ) -> Result<GaussianPointSetMetric, RegError> {
        let l = o.limits.checked()?;
        let neighbors = if o.neighbors == 0 { 5 } else { o.neighbors };
        let max_neighbors = if o.max_neighbors == 0 {
            1024
        } else {
            o.max_neighbors
        };
        let max_node_visits = if o.max_node_visits == 0 {
            1 << 26
        } else {
            o.max_node_visits
        };
        let sigma2 = o.sigma_mm * o.sigma_mm;
        let Some(ctx) = ctx else {
            return Err(gaussian());
        };
        let (nf, nm) = (fixed.len() as i64, moving.len() as i64);
        if o.frame_of_reference.is_empty()
            || o.frame_of_reference.len() > 1024
            || o.sigma_mm <= 0.0
            || !finite_slice(&[o.sigma_mm, sigma2, 1.0 / sigma2])
            || sigma2 <= 0.0
            || neighbors < 1
            || !(1..=1024).contains(&max_neighbors)
            || neighbors > max_neighbors
            || neighbors > nm
            || o.sample_count < 0
            || o.workers < 0
            || o.workers > l.workers
            || nf == 0
            || nm == 0
            || nf > l.points
            || nm > l.points - nf
            || o.sample_count > l.points
        {
            return Err(gaussian());
        }
        let labels = o.fixed_labels.is_some() || o.moving_labels.is_some();
        if labels
            && (o.fixed_labels.as_ref().map_or(0, Vec::len) != fixed.len()
                || o.moving_labels.as_ref().map_or(0, Vec::len) != moving.len())
        {
            return Err(gaussian());
        }
        let samples = if o.sample_count > 0 {
            o.sample_count
        } else {
            nf
        };
        if max_node_visits < samples as u64 {
            return Err(RegError::bare(Sentinel::PointSetWorkLimit));
        }
        let mut bytes = 8192u64 + l.parameters as u64 * 128 + neighbors as u64 * 64;
        for (n, unit) in [(nf, 32u64), (nm, 160), (samples, 8)] {
            if bytes > l.bytes || n as u64 > (l.bytes - bytes) / unit {
                return Err(gaussian());
            }
            bytes += n as u64 * unit;
        }
        check(ctx)?;
        if labels {
            let mut counts: HashMap<u32, i64> = HashMap::new();
            for (i, label) in o.moving_labels.iter().flatten().enumerate() {
                if i % 64 == 0 {
                    check(ctx)?;
                }
                *counts.entry(*label).or_default() += 1;
            }
            for (i, label) in o.fixed_labels.iter().flatten().enumerate() {
                if i % 64 == 0 {
                    check(ctx)?;
                }
                if counts.get(label).copied().unwrap_or(0) < neighbors {
                    return Err(gaussian());
                }
            }
        }
        for cloud in [fixed, moving] {
            for (i, v) in cloud.iter().enumerate() {
                if i % 64 == 0 {
                    check(ctx)?;
                }
                if !finite_vec(*v) {
                    return Err(gaussian());
                }
            }
        }
        let (mut fixed_labels, mut moving_labels) = (None, None);
        if labels {
            for (i, _) in o.fixed_labels.iter().flatten().enumerate() {
                if i % 64 == 0 {
                    check(ctx)?;
                }
            }
            for (i, _) in o.moving_labels.iter().flatten().enumerate() {
                if i % 64 == 0 {
                    check(ctx)?;
                }
            }
            fixed_labels = Some(o.fixed_labels.clone().unwrap_or_default());
            moving_labels = Some(o.moving_labels.clone().unwrap_or_default());
        }
        let mut rng = Rand::new(o.seed);
        let mut sample_indices = Vec::with_capacity(samples as usize);
        for i in 0..samples as usize {
            if i % 64 == 0 {
                check(ctx)?;
            }
            sample_indices.push(if o.sample_count > 0 {
                rng.intn(nf).unwrap_or(0) as usize
            } else {
                i
            });
        }
        let tree = gaussian_kd_tree(ctx, moving)?;
        check(ctx)?;
        Ok(GaussianPointSetMetric {
            fixed: fixed.to_vec(),
            samples: sample_indices,
            fixed_labels,
            moving_labels,
            tree,
            sigma2,
            neighbors: neighbors as usize,
            max_visits: max_node_visits,
            info: ComponentInfo {
                frame: o.frame_of_reference.clone(),
                units: "dimensionless negative log density".to_string(),
                points: samples,
                channels: 0,
                parameters: l.parameters,
                bytes,
            },
        })
    }
    pub(crate) fn evaluate_component(
        &self,
        ctx: Option<&dyn Cancellation>,
        t: &dyn Transform,
        derivatives: bool,
    ) -> Result<(f64, Vec<f64>, i64), (RegError, i64)> {
        let np = t.number_of_parameters();
        let Some(ctx) = ctx else {
            return Err((gaussian(), 0));
        };
        if self.samples.is_empty() || np < 1 || np as i64 > self.info.parameters {
            return Err((gaussian(), 0));
        }
        check(ctx).map_err(|e| (e, 0))?;
        let params = t.parameters();
        let fixed_params = t.fixed_parameters();
        if params.len() != np
            || !finite_slice(&params)
            || fixed_params.len() > 4096
            || !finite_slice(&fixed_params)
        {
            return Err((gaussian(), 0));
        }
        let mut g = vec![0.0; np];
        let mut neighbors: Vec<(usize, f64)> = Vec::with_capacity(self.neighbors);
        let mut j = Jacobian::default();
        let mut sum = 0.0;
        let mut visits = 0u64;
        let kf = self.neighbors as f64;
        for (at, &index) in self.samples.iter().enumerate() {
            let at = at as i64;
            check(ctx).map_err(|e| (e, at))?;
            let point = self.fixed[index];
            let mapped = t.transform_point(point);
            if !finite_vec(mapped) {
                return Err((gaussian(), at));
            }
            let label = self.fixed_labels.as_ref().map_or(0, |l| l[index]);
            neighbors.clear();
            self.tree
                .gaussian_nearest(
                    ctx,
                    mapped,
                    self.neighbors,
                    self.moving_labels.as_deref(),
                    label,
                    &mut neighbors,
                    &mut visits,
                    self.max_visits,
                )
                .map_err(|e| (e, at))?;
            let base = neighbors[0].1 / self.sigma2 / 2.0;
            if !finite(base) {
                return Err((gaussian(), at));
            }
            let mut weights = 0.0;
            for (k, n) in neighbors.iter().enumerate() {
                if k % 32 == 0 {
                    check(ctx).map_err(|e| (e, at))?;
                }
                let delta = (n.1 - neighbors[0].1) / self.sigma2 / 2.0;
                if !finite(delta) {
                    return Err((gaussian(), at));
                }
                weights += fmath::exp(-delta);
            }
            let value = base - fmath::log(weights) + fmath::log(kf);
            if !finite(value) {
                return Err((gaussian(), at));
            }
            sum += value;
            if derivatives {
                t.jacobian_with_respect_to_parameters(point, &mut j);
                for (k, n) in neighbors.iter().enumerate() {
                    if k % 32 == 0 {
                        check(ctx).map_err(|e| (e, at))?;
                    }
                    let delta = (n.1 - neighbors[0].1) / self.sigma2 / 2.0;
                    let w = fmath::exp(-delta) / weights;
                    let d = mapped.sub(self.tree.points[n.0]).scale(w / self.sigma2);
                    if !finite_vec(d) {
                        return Err((gaussian(), at));
                    }
                    for (p, gp) in g.iter_mut().enumerate() {
                        *gp += d.x * j.at(0, p) + d.y * j.at(1, p) + d.z * j.at(2, p);
                    }
                }
            }
        }
        let count = self.samples.len() as i64;
        check(ctx).map_err(|e| (e, count))?;
        sum /= self.samples.len() as f64;
        for gp in g.iter_mut() {
            *gp /= self.samples.len() as f64;
        }
        if !finite(sum) || !finite_slice(&g) {
            return Err((
                RegError::new(Sentinel::GaussianPointSet, "nonfinite density/gradient"),
                count,
            ));
        }
        if !derivatives {
            g = Vec::new();
        }
        Ok((sum, g, count))
    }
    pub fn value_context(
        &self,
        ctx: Option<&dyn Cancellation>,
        t: &dyn Transform,
    ) -> Result<f64, RegError> {
        self.evaluate_component(ctx, t, false)
            .map(|r| r.0)
            .map_err(|e| e.0)
    }
    pub fn value_and_derivative_context(
        &self,
        ctx: Option<&dyn Cancellation>,
        t: &dyn Transform,
    ) -> Result<(f64, Vec<f64>), RegError> {
        self.evaluate_component(ctx, t, true)
            .map(|r| (r.0, r.1))
            .map_err(|e| e.0)
    }
}
fn gaussian_kd_tree(ctx: &dyn Cancellation, points: &[Vec3]) -> Result<KdTree, RegError> {
    let mut tree = KdTree {
        points: points.to_vec(),
        nodes: Vec::with_capacity(points.len()),
    };
    let mut indices = Vec::with_capacity(points.len());
    for i in 0..points.len() {
        if i % 64 == 0 {
            check(ctx)?;
        }
        indices.push(i);
    }
    let mut scratch = vec![0usize; points.len()];
    build(ctx, &mut tree, &mut indices, &mut scratch, 0)?;
    Ok(tree)
}
fn build(
    ctx: &dyn Cancellation,
    tree: &mut KdTree,
    ids: &mut [usize],
    scratch: &mut [usize],
    depth: usize,
) -> Result<i64, RegError> {
    check(ctx)?;
    if ids.is_empty() {
        return Ok(-1);
    }
    let axis = depth % 3;
    sort_indices(ctx, &tree.points, ids, &mut scratch[..ids.len()], axis)?;
    let middle = ids.len() / 2;
    let node = tree.nodes.len();
    tree.nodes.push(KdNode {
        point: ids[middle],
        axis,
        left: -1,
        right: -1,
    });
    let (lower, upper) = ids.split_at_mut(middle);
    let left = build(ctx, tree, lower, scratch, depth + 1)?;
    let right = build(ctx, tree, &mut upper[1..], scratch, depth + 1)?;
    tree.nodes[node].left = left;
    tree.nodes[node].right = right;
    Ok(node as i64)
}
fn sort_indices(
    ctx: &dyn Cancellation,
    points: &[Vec3],
    ids: &mut [usize],
    scratch: &mut [usize],
    axis: usize,
) -> Result<(), RegError> {
    let less = |a: usize, b: usize| {
        let (av, bv) = (coordinate(points[a], axis), coordinate(points[b], axis));
        av < bv || av == bv && a < b
    };
    let n = ids.len();
    let mut width = 1;
    while width < n {
        let mut start = 0;
        while start < n {
            let (mid, end) = ((start + width).min(n), (start + 2 * width).min(n));
            let (mut a, mut b) = (start, mid);
            for at in start..end {
                if at % 64 == 0 {
                    check(ctx)?;
                }
                if a < mid && (b >= end || less(ids[a], ids[b])) {
                    scratch[at] = ids[a];
                    a += 1;
                } else {
                    scratch[at] = ids[b];
                    b += 1;
                }
            }
            start += 2 * width;
        }
        for i in 0..n {
            if i % 64 == 0 {
                check(ctx)?;
            }
            ids[i] = scratch[i];
        }
        width *= 2;
    }
    check(ctx)
}
impl KdTree {
    #[allow(clippy::too_many_arguments)]
    fn gaussian_nearest(
        &self,
        ctx: &dyn Cancellation,
        q: Vec3,
        k: usize,
        labels: Option<&[u32]>,
        label: u32,
        out: &mut Vec<(usize, f64)>,
        visits: &mut u64,
        limit: u64,
    ) -> Result<(), RegError> {
        self.gaussian_search(ctx, 0, q, k, labels, label, out, visits, limit)?;
        if out.len() != k {
            return Err(gaussian());
        }
        check(ctx)
    }
    #[allow(clippy::too_many_arguments)]
    fn gaussian_search(
        &self,
        ctx: &dyn Cancellation,
        node: i64,
        q: Vec3,
        k: usize,
        labels: Option<&[u32]>,
        label: u32,
        out: &mut Vec<(usize, f64)>,
        visits: &mut u64,
        limit: u64,
    ) -> Result<(), RegError> {
        if node < 0 {
            return Ok(());
        }
        if *visits >= limit {
            return Err(RegError::bare(Sentinel::PointSetWorkLimit));
        }
        if (*visits).is_multiple_of(32) {
            check(ctx)?;
        }
        *visits += 1;
        let n = self.nodes[node as usize];
        let point = self.points[n.point];
        let d = q.sub(point);
        if !finite_vec(d) {
            return Err(gaussian());
        }
        let d2 = d.dot(d);
        if !finite(d2) {
            return Err(gaussian());
        }
        if labels.is_none_or(|l| l[n.point] == label) {
            let mut at = out.len();
            while at > 0 && (d2 < out[at - 1].1 || d2 == out[at - 1].1 && n.point < out[at - 1].0) {
                at -= 1;
            }
            if at < k {
                if out.len() < k {
                    out.push((0, 0.0));
                }
                let len = out.len();
                out.copy_within(at..len - 1, at + 1);
                out[at] = (n.point, d2);
            }
        }
        let diff = coordinate(q, n.axis) - coordinate(point, n.axis);
        let plane = diff * diff;
        if !finite(plane) {
            return Err(gaussian());
        }
        let (mut near, mut far) = (n.left, n.right);
        if diff > 0.0 {
            std::mem::swap(&mut near, &mut far);
        }
        self.gaussian_search(ctx, near, q, k, labels, label, out, visits, limit)?;
        if out.len() < k || plane <= out[out.len() - 1].1 {
            return self.gaussian_search(ctx, far, q, k, labels, label, out, visits, limit);
        }
        Ok(())
    }
}
macro_rules! component_metric {
    ($ty:ty) => {
        impl Metric for $ty {
            fn value(&self, t: &dyn Transform) -> Result<f64, RegError> {
                let token = CancelToken::new();
                self.value_context(Some(&token), t)
            }
            fn value_and_derivative(&self, t: &dyn Transform) -> Result<(f64, Vec<f64>), RegError> {
                let token = CancelToken::new();
                self.value_and_derivative_context(Some(&token), t)
            }
            fn context_value(
                &self,
                ctx: Option<&dyn Cancellation>,
                t: &dyn Transform,
            ) -> Option<Result<f64, RegError>> {
                Some(self.value_context(ctx, t))
            }
            fn context_value_and_derivative(
                &self,
                ctx: Option<&dyn Cancellation>,
                t: &dyn Transform,
            ) -> Option<Result<(f64, Vec<f64>), RegError>> {
                Some(self.value_and_derivative_context(ctx, t))
            }
        }
    };
}
component_metric!(PointSetMetric);
component_metric!(GaussianPointSetMetric);
