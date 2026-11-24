use super::{Mesh, cross3, dot3, sub3};
use crate::fmath;
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct BvhNode {
    pub lo: [f64; 3],
    pub hi: [f64; 3],
    pub first: i32,
    pub count: i32,
    pub axis: i8,
}
#[derive(Debug, Clone, Default)]
pub(crate) struct TriangleBvh {
    pub nodes: Vec<BvhNode>,
    pub order: Vec<u32>,
}
const BVH_LEAF_SIZE: usize = 4;
pub(crate) fn scale3(a: [f64; 3], s: f64) -> [f64; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
}
impl TriangleBvh {
    pub fn new(m: &Mesh) -> TriangleBvh {
        let mut order: Vec<u32> = (0..m.triangles.len() as u32).collect();
        let centroids: Vec<[f64; 3]> = m
            .triangles
            .iter()
            .map(|t| {
                let (a, b, c) = (m.point(t[0]), m.point(t[1]), m.point(t[2]));
                [
                    (a[0] + b[0] + c[0]) / 3.0,
                    (a[1] + b[1] + c[1]) / 3.0,
                    (a[2] + b[2] + c[2]) / 3.0,
                ]
            })
            .collect();
        let bounds = |run: &[u32]| {
            let mut lo = [f64::INFINITY; 3];
            let mut hi = [f64::NEG_INFINITY; 3];
            let (mut clo, mut chi) = (lo, hi);
            for i in run {
                for v in m.triangles[*i as usize] {
                    let p = m.point(v);
                    for k in 0..3 {
                        lo[k] = fmath::min(lo[k], p[k]);
                        hi[k] = fmath::max(hi[k], p[k]);
                    }
                }
                let c = centroids[*i as usize];
                for k in 0..3 {
                    clo[k] = fmath::min(clo[k], c[k]);
                    chi[k] = fmath::max(chi[k], c[k]);
                }
            }
            (lo, hi, clo, chi)
        };
        let mut nodes = vec![BvhNode::default()];
        let mut stack = vec![(0usize, 0usize, m.triangles.len())];
        while let Some((index, first, count)) = stack.pop() {
            let (lo, hi, clo, chi) = bounds(&order[first..first + count]);
            let mut node = BvhNode {
                lo,
                hi,
                first: first as i32,
                count: count as i32,
                axis: 0,
            };
            if count > BVH_LEAF_SIZE {
                let mut axis = 0;
                for k in 1..3 {
                    if chi[k] - clo[k] > chi[axis] - clo[axis] {
                        axis = k;
                    }
                }
                let mid = (clo[axis] + chi[axis]) / 2.0;
                let run = &mut order[first..first + count];
                let mut left = 0;
                for i in 0..run.len() {
                    if centroids[run[i] as usize][axis] < mid {
                        run.swap(i, left);
                        left += 1;
                    }
                }
                if left == 0 || left == run.len() {
                    left = run.len() / 2;
                }
                let child = nodes.len();
                nodes.push(BvhNode::default());
                nodes.push(BvhNode::default());
                node.first = child as i32;
                node.count = 0;
                node.axis = axis as i8;
                stack.push((child + 1, first + left, count - left));
                stack.push((child, first, left));
            }
            nodes[index] = node;
        }
        TriangleBvh { nodes, order }
    }
    pub(crate) fn closest_hit(
        &self,
        m: &Mesh,
        o: [f64; 3],
        d: [f64; 3],
        max_t: f64,
        skip: u32,
        tolerance: f64,
    ) -> Option<(f64, u32)> {
        let inverse = [1.0 / d[0], 1.0 / d[1], 1.0 / d[2]];
        let mut best = max_t;
        let mut found: Option<u32> = None;
        let mut stack: Vec<i32> = Vec::with_capacity(64);
        stack.push(0);
        while let Some(index) = stack.pop() {
            let node = &self.nodes[index as usize];
            if !ray_hits_box(o, inverse, node.lo, node.hi, best) {
                continue;
            }
            if node.count == 0 {
                if d[node.axis as usize] < 0.0 {
                    stack.push(node.first);
                    stack.push(node.first + 1);
                } else {
                    stack.push(node.first + 1);
                    stack.push(node.first);
                }
                continue;
            }
            let run = &self.order[node.first as usize..(node.first + node.count) as usize];
            for ti in run {
                let t = m.triangles[*ti as usize];
                if t[0] == skip || t[1] == skip || t[2] == skip {
                    continue;
                }
                if let Some(distance) =
                    ray_triangle(o, d, m.point(t[0]), m.point(t[1]), m.point(t[2]), tolerance)
                    && distance > 1e-9
                    && distance < best
                {
                    best = distance;
                    found = Some(*ti);
                }
            }
        }
        found.map(|ti| (best, ti))
    }
}
fn ray_hits_box(o: [f64; 3], inverse: [f64; 3], lo: [f64; 3], hi: [f64; 3], max_t: f64) -> bool {
    let (mut near, mut far) = (0.0, max_t);
    for k in 0..3 {
        if inverse[k].is_infinite() {
            if o[k] < lo[k] || o[k] > hi[k] {
                return false;
            }
            continue;
        }
        let (mut t0, mut t1) = ((lo[k] - o[k]) * inverse[k], (hi[k] - o[k]) * inverse[k]);
        if t0 > t1 {
            std::mem::swap(&mut t0, &mut t1);
        }
        if t0 > near {
            near = t0;
        }
        if t1 < far {
            far = t1;
        }
        if near > far {
            return false;
        }
    }
    true
}
fn ray_triangle(
    o: [f64; 3],
    d: [f64; 3],
    a: [f64; 3],
    b: [f64; 3],
    c: [f64; 3],
    tolerance: f64,
) -> Option<f64> {
    let (e1, e2) = (sub3(b, a), sub3(c, a));
    let p = cross3(d, e2);
    let det = dot3(e1, p);
    if det.abs() < 1e-18 {
        return None;
    }
    let inv = 1.0 / det;
    let s = sub3(o, a);
    let u = dot3(s, p) * inv;
    if u < -tolerance || u > 1.0 + tolerance {
        return None;
    }
    let q = cross3(s, e1);
    let v = dot3(d, q) * inv;
    if v < -tolerance || u + v > 1.0 + tolerance {
        return None;
    }
    Some(dot3(e2, q) * inv)
}
