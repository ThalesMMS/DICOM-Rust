use super::attributes::{
    MAX_VERTEX_SCALAR_ATTRIBUTES, VertexScalarAttribute, validate_vertex_scalars,
};
use super::{ErrorKind, MeshError};
use crate::fmath;
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PointCloud {
    pub units: String,
    pub points: Vec<[f32; 3]>,
    pub normals: Option<Vec<[f32; 3]>>,
    pub colors: Option<Vec<[u8; 4]>>,
    pub scalars: Vec<VertexScalarAttribute>,
    pub protected: Option<Vec<bool>>,
}
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PointCloudOptions {
    pub neighbors: i64,
    pub sigma_factor: f64,
    pub remove_outliers: bool,
    pub min_distance_mm: f64,
    pub distance_tolerance_mm: f64,
    pub seed: u64,
    pub max_points: i64,
    pub max_bytes: i64,
    pub max_work: i64,
    pub max_neighbors: i64,
}
pub const POINT_KEPT: u8 = 0;
pub const POINT_STATISTICAL_OUTLIER: u8 = 1;
pub const POINT_DISTANCE_SUPPRESSED: u8 = 2;
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PointCloudReport {
    pub complete: bool,
    pub state: String,
    pub options: PointCloudOptions,
    pub input_points: i64,
    pub output_points: i64,
    pub candidates: i64,
    pub protected: i64,
    pub outliers_removed: i64,
    pub distance_removed: i64,
    pub mean_distance_mm: f64,
    pub standard_deviation_mm: f64,
    pub threshold_mm: f64,
    pub reserved_bytes: i64,
    pub work: i64,
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PointCloudResult {
    pub cloud: Option<PointCloud>,
    pub original_indices: Vec<u32>,
    pub input_to_output: Vec<i64>,
    pub removed: Vec<u8>,
    pub outlier_candidate: Vec<bool>,
    pub local_mean_mm: Vec<f64>,
    pub report: PointCloudReport,
}
fn limit_error(resource: &str, used: i64, limit: i64) -> MeshError {
    MeshError {
        kind: ErrorKind::BudgetExceeded,
        message: format!("mesh point cloud: {resource} limit ({used} > {limit})"),
        budget: None,
    }
}
fn is_limit(e: &MeshError) -> bool {
    e.message.starts_with("mesh point cloud: ")
}
fn point_cloud_options(mut o: PointCloudOptions) -> (PointCloudOptions, Option<MeshError>) {
    for f in [o.sigma_factor, o.min_distance_mm, o.distance_tolerance_mm] {
        if f.is_nan() || f.is_infinite() || f < 0.0 {
            return (
                o,
                Some(MeshError::invalid("invalid point cloud distance/factor")),
            );
        }
    }
    if o.neighbors < 0
        || o.max_neighbors < 0
        || (o.remove_outliers && o.neighbors == 0)
        || (o.distance_tolerance_mm > 0.0 && o.distance_tolerance_mm >= o.min_distance_mm)
    {
        return (
            o,
            Some(MeshError::invalid(
                "invalid point cloud neighborhood/tolerance",
            )),
        );
    }
    if o.max_neighbors == 0 {
        o.max_neighbors = 64;
    }
    if o.neighbors > o.max_neighbors {
        return (
            o,
            Some(limit_error("neighbors", o.neighbors, o.max_neighbors)),
        );
    }
    let defaults = [1_000_000, 256 << 20, 100_000_000];
    let mut negative = false;
    {
        let limits = [&mut o.max_points, &mut o.max_bytes, &mut o.max_work];
        for (p, d) in limits.into_iter().zip(defaults) {
            if *p < 0 {
                negative = true;
                break;
            }
            if *p == 0 {
                *p = d;
            }
        }
    }
    if negative {
        return (o, Some(MeshError::invalid("negative point cloud limit")));
    }
    (o, None)
}
fn step(r: &mut PointCloudReport) -> Result<(), MeshError> {
    if r.work >= r.options.max_work {
        return Err(limit_error("work", r.work + 1, r.options.max_work));
    }
    r.work += 1;
    Ok(())
}
#[derive(Debug, Clone, Copy, Default)]
struct Node {
    point: i32,
    left: i32,
    right: i32,
    axis: u8,
}
#[derive(Debug, Clone, Copy)]
struct Neighbor {
    id: i32,
    distance2: f64,
}
fn worse(a: Neighbor, b: Neighbor) -> bool {
    a.distance2 > b.distance2 || (a.distance2 == b.distance2 && a.id > b.id)
}
fn distance2(a: [f64; 3], b: [f32; 3]) -> f64 {
    let (x, y, z) = (
        a[0] - f64::from(b[0]),
        a[1] - f64::from(b[1]),
        a[2] - f64::from(b[2]),
    );
    x * x + y * y + z * z
}
fn point64(p: [f32; 3]) -> [f64; 3] {
    [f64::from(p[0]), f64::from(p[1]), f64::from(p[2])]
}
struct Tree<'a> {
    points: &'a [[f32; 3]],
    nodes: Vec<Node>,
    root: i32,
}
struct Builder<'a, 'b> {
    points: &'a [[f32; 3]],
    order: Vec<i32>,
    nodes: Vec<Node>,
    next: i32,
    report: &'b mut PointCloudReport,
}
impl Builder<'_, '_> {
    fn less(&self, a: i32, b: i32, axis: usize) -> bool {
        let (x, y) = (self.points[a as usize][axis], self.points[b as usize][axis]);
        x < y || (x == y && a < b)
    }
    fn select_median(
        &mut self,
        mut lo: i64,
        mut hi: i64,
        mid: i64,
        axis: usize,
    ) -> Result<(), MeshError> {
        while lo < hi {
            let pivot = self.order[((lo + hi) / 2) as usize];
            let (mut i, mut j) = (lo, hi);
            while i <= j {
                loop {
                    step(self.report)?;
                    if !self.less(self.order[i as usize], pivot, axis) {
                        break;
                    }
                    i += 1;
                }
                loop {
                    step(self.report)?;
                    if !self.less(pivot, self.order[j as usize], axis) {
                        break;
                    }
                    j -= 1;
                }
                if i <= j {
                    self.order.swap(i as usize, j as usize);
                    i += 1;
                    j -= 1;
                }
            }
            if mid <= j {
                hi = j;
            } else if mid >= i {
                lo = i;
            } else {
                break;
            }
        }
        Ok(())
    }
    fn build(&mut self, lo: i64, hi: i64, axis: usize) -> Result<i32, MeshError> {
        if lo >= hi {
            return Ok(-1);
        }
        step(self.report)?;
        let mid = lo + (hi - lo) / 2;
        self.select_median(lo, hi - 1, mid, axis)?;
        let id = self.next;
        self.next += 1;
        self.nodes[id as usize] = Node {
            point: self.order[mid as usize],
            left: 0,
            right: 0,
            axis: axis as u8,
        };
        let left = self.build(lo, mid, (axis + 1) % 3)?;
        let right = self.build(mid + 1, hi, (axis + 1) % 3)?;
        self.nodes[id as usize].left = left;
        self.nodes[id as usize].right = right;
        Ok(id)
    }
}
impl<'a> Tree<'a> {
    fn new(points: &'a [[f32; 3]], report: &mut PointCloudReport) -> Result<Tree<'a>, MeshError> {
        let mut order = Vec::with_capacity(points.len());
        for i in 0..points.len() {
            step(report)?;
            order.push(i as i32);
        }
        let mut builder = Builder {
            points,
            order,
            nodes: vec![Node::default(); points.len()],
            next: 0,
            report,
        };
        let root = builder.build(0, points.len() as i64, 0)?;
        Ok(Tree {
            points,
            nodes: builder.nodes,
            root,
        })
    }
    fn nearest(
        &self,
        id: i32,
        q: [f64; 3],
        skip: usize,
        k: usize,
        heap: &mut Vec<Neighbor>,
        report: &mut PointCloudReport,
    ) -> Result<(), MeshError> {
        if id < 0 {
            return Ok(());
        }
        step(report)?;
        let n = self.nodes[id as usize];
        let p = self.points[n.point as usize];
        if n.point as usize != skip {
            let v = Neighbor {
                id: n.point,
                distance2: distance2(q, p),
            };
            if heap.len() < k {
                heap.push(v);
                let mut i = heap.len() - 1;
                while i > 0 {
                    step(report)?;
                    let parent = (i - 1) / 2;
                    if !worse(heap[i], heap[parent]) {
                        break;
                    }
                    heap.swap(i, parent);
                    i = parent;
                }
            } else if worse(heap[0], v) {
                heap[0] = v;
                let mut i = 0;
                while 2 * i + 1 < heap.len() {
                    step(report)?;
                    let mut child = 2 * i + 1;
                    if child + 1 < heap.len() && worse(heap[child + 1], heap[child]) {
                        child += 1;
                    }
                    if !worse(heap[child], heap[i]) {
                        break;
                    }
                    heap.swap(i, child);
                    i = child;
                }
            }
        }
        let d = q[n.axis as usize] - f64::from(p[n.axis as usize]);
        let (near, far) = if d > 0.0 {
            (n.right, n.left)
        } else {
            (n.left, n.right)
        };
        self.nearest(near, q, skip, k, heap, report)?;
        if heap.len() < k || d * d <= heap[0].distance2 {
            return self.nearest(far, q, skip, k, heap, report);
        }
        Ok(())
    }
    fn within(
        &self,
        id: i32,
        q: [f64; 3],
        radius: f64,
        active: &[bool],
        report: &mut PointCloudReport,
    ) -> Result<bool, MeshError> {
        if id < 0 {
            return Ok(false);
        }
        step(report)?;
        let n = self.nodes[id as usize];
        let p = self.points[n.point as usize];
        if active[n.point as usize] && distance2(q, p).sqrt() < radius {
            return Ok(true);
        }
        let d = q[n.axis as usize] - f64::from(p[n.axis as usize]);
        let (near, far) = if d > 0.0 {
            (n.right, n.left)
        } else {
            (n.left, n.right)
        };
        if self.within(near, q, radius, active, report)? {
            return Ok(true);
        }
        if d.abs() < radius {
            return self.within(far, q, radius, active, report);
        }
        Ok(false)
    }
}
fn point_cloud_random(seed: &mut u64) -> u64 {
    *seed = seed.wrapping_add(0x9e37_79b9_7f4a_7c15);
    let mut x = *seed;
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    x ^ (x >> 31)
}
pub fn process_point_cloud(
    c: Option<&PointCloud>,
    opts: PointCloudOptions,
) -> (PointCloudResult, Result<(), MeshError>) {
    let (o, invalid) = point_cloud_options(opts);
    let mut result = PointCloudResult {
        report: PointCloudReport {
            options: o,
            state: "invalid".to_string(),
            ..PointCloudReport::default()
        },
        ..PointCloudResult::default()
    };
    let outcome = match invalid {
        Some(e) => Err(e),
        None => process(c, &o, &mut result),
    };
    if let Err(e) = &outcome {
        if is_limit(e) {
            result.report.state = "limited".to_string();
        }
        result = PointCloudResult {
            report: result.report,
            ..PointCloudResult::default()
        };
    }
    (result, outcome)
}
fn protected(c: &PointCloud, i: usize) -> bool {
    c.protected.as_ref().is_some_and(|p| p[i])
}
fn process(
    c: Option<&PointCloud>,
    o: &PointCloudOptions,
    result: &mut PointCloudResult,
) -> Result<(), MeshError> {
    let Some(c) = c.filter(|c| c.units == "mm") else {
        return Err(MeshError::invalid(
            "point cloud requires context, input and mm units",
        ));
    };
    let n = c.points.len();
    let r = &mut result.report;
    r.input_points = n as i64;
    if n as i64 > o.max_points {
        return Err(limit_error("points", n as i64, o.max_points));
    }
    if n as u64 > i32::MAX as u64 || (o.neighbors > 0 && o.neighbors >= n as i64) {
        return Err(MeshError::invalid("point cloud index/neighborhood range"));
    }
    if c.normals.as_ref().is_some_and(|v| v.len() != n)
        || c.colors.as_ref().is_some_and(|v| v.len() != n)
        || c.protected.as_ref().is_some_and(|v| v.len() != n)
        || c.scalars.len() > MAX_VERTEX_SCALAR_ATTRIBUTES
    {
        return Err(MeshError::invalid("point cloud attribute count"));
    }
    let mut per_point: i64 = 16 + 4 + 1 + 1 + 1 + 8 + 8 + 4 + 4 + 12;
    if c.normals.is_some() {
        per_point += 12;
    }
    if c.colors.is_some() {
        per_point += 4;
    }
    if c.protected.is_some() {
        per_point += 1;
    }
    per_point += c.scalars.len() as i64 * 8;
    let constant = o.neighbors * 16 + c.scalars.len() as i64 * 56;
    if n as i64 > (i64::MAX - constant) / per_point {
        return Err(limit_error("bytes", i64::MAX, o.max_bytes));
    }
    r.reserved_bytes = n as i64 * per_point + constant;
    if r.reserved_bytes > o.max_bytes {
        return Err(limit_error("bytes", r.reserved_bytes, o.max_bytes));
    }
    for (i, p) in c.points.iter().enumerate() {
        step(r)?;
        if p.iter().any(|f| !f.is_finite()) {
            return Err(MeshError::invalid(format!("nonfinite cloud point {i}")));
        }
        if let Some(normals) = &c.normals
            && normals[i].iter().any(|f| !f.is_finite())
        {
            return Err(MeshError::invalid(format!("nonfinite cloud normal {i}")));
        }
        if protected(c, i) {
            r.protected += 1;
        }
    }
    for (ai, a) in c.scalars.iter().enumerate() {
        step(r)?;
        if a.name.len() > 64 || a.units.len() > 64 {
            return Err(MeshError::invalid("point cloud scalar metadata length"));
        }
        let metadata = VertexScalarAttribute {
            name: a.name.clone(),
            units: a.units.clone(),
            values: Vec::new(),
        };
        validate_vertex_scalars(&[metadata], 0, false)?;
        if c.scalars[..ai].iter().any(|p| p.name == a.name) {
            return Err(MeshError::invalid("duplicate point cloud scalar"));
        }
        if a.values.len() != n {
            return Err(MeshError::invalid("point cloud scalar count"));
        }
        for f in &a.values {
            step(r)?;
            if !f.is_finite() {
                return Err(MeshError::invalid("nonfinite point cloud scalar"));
            }
        }
    }
    let tree = if o.neighbors > 0 || o.min_distance_mm > 0.0 {
        Some(Tree::new(&c.points, r)?)
    } else {
        None
    };
    result.outlier_candidate = vec![false; n];
    result.local_mean_mm = vec![0.0; n];
    result.removed = vec![POINT_KEPT; n];
    let mut active = vec![false; n];
    if let Some(tree) = tree.as_ref().filter(|_| o.neighbors > 0) {
        let k = o.neighbors as usize;
        let mut heap: Vec<Neighbor> = Vec::with_capacity(k);
        let (mut mean, mut m2) = (0.0, 0.0);
        for (i, p) in c.points.iter().enumerate() {
            heap.clear();
            tree.nearest(tree.root, point64(*p), i, k, &mut heap, r)?;
            let mut score = 0.0;
            for v in &heap {
                step(r)?;
                score += v.distance2.sqrt() / o.neighbors as f64;
            }
            result.local_mean_mm[i] = score;
            let delta = score - mean;
            mean += delta / (i + 1) as f64;
            m2 += delta * (score - mean);
        }
        r.mean_distance_mm = mean;
        r.standard_deviation_mm = fmath::max(0.0, m2 / n as f64).sqrt();
        r.threshold_mm = mean + o.sigma_factor * r.standard_deviation_mm;
        if r.threshold_mm.is_infinite() {
            return Err(MeshError::invalid("cloud threshold overflow"));
        }
        for i in 0..n {
            step(r)?;
            if result.local_mean_mm[i] > r.threshold_mm {
                result.outlier_candidate[i] = true;
                r.candidates += 1;
                if o.remove_outliers && !protected(c, i) {
                    result.removed[i] = POINT_STATISTICAL_OUTLIER;
                    r.outliers_removed += 1;
                }
            }
        }
    }
    if let Some(tree) = tree.as_ref().filter(|_| o.min_distance_mm > 0.0) {
        let separation = o.min_distance_mm - o.distance_tolerance_mm;
        for (i, p) in c.points.iter().enumerate() {
            step(r)?;
            if protected(c, i) {
                if tree.within(tree.root, point64(*p), separation, &active, r)? {
                    return Err(MeshError::invalid(format!(
                        "protected cloud point {i} conflicts with minimum distance"
                    )));
                }
                active[i] = true;
            }
        }
        let mut order: Vec<u32> = Vec::with_capacity(n);
        for i in 0..n {
            step(r)?;
            order.push(i as u32);
        }
        let mut seed = o.seed;
        for i in (1..n).rev() {
            step(r)?;
            let j = (point_cloud_random(&mut seed) % (i as u64 + 1)) as usize;
            order.swap(i, j);
        }
        for id in order {
            step(r)?;
            let id = id as usize;
            if protected(c, id) || result.removed[id] != POINT_KEPT {
                continue;
            }
            if tree.within(tree.root, point64(c.points[id]), separation, &active, r)? {
                result.removed[id] = POINT_DISTANCE_SUPPRESSED;
                r.distance_removed += 1;
            } else {
                active[id] = true;
            }
        }
    }
    let count = n - (r.outliers_removed + r.distance_removed) as usize;
    let mut out = PointCloud {
        units: "mm".to_string(),
        points: vec![[0.0; 3]; count],
        normals: c.normals.as_ref().map(|_| vec![[0.0; 3]; count]),
        colors: c.colors.as_ref().map(|_| vec![[0; 4]; count]),
        protected: c.protected.as_ref().map(|_| vec![false; count]),
        scalars: c
            .scalars
            .iter()
            .map(|a| VertexScalarAttribute {
                name: a.name.clone(),
                units: a.units.clone(),
                values: vec![0.0; count],
            })
            .collect(),
    };
    result.original_indices = vec![0; count];
    result.input_to_output = vec![0; n];
    let mut k = 0;
    for i in 0..n {
        step(r)?;
        result.input_to_output[i] = -1;
        if result.removed[i] != POINT_KEPT {
            continue;
        }
        result.input_to_output[i] = k as i64;
        result.original_indices[k] = i as u32;
        out.points[k] = c.points[i];
        if let (Some(to), Some(from)) = (&mut out.normals, &c.normals) {
            to[k] = from[i];
        }
        if let (Some(to), Some(from)) = (&mut out.colors, &c.colors) {
            to[k] = from[i];
        }
        if let (Some(to), Some(from)) = (&mut out.protected, &c.protected) {
            to[k] = from[i];
        }
        for (to, from) in out.scalars.iter_mut().zip(&c.scalars) {
            step(r)?;
            to.values[k] = from.values[i];
        }
        k += 1;
    }
    result.cloud = Some(out);
    r.output_points = count as i64;
    r.state = "complete".to_string();
    r.complete = true;
    Ok(())
}
