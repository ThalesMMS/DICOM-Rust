use super::clean::compact_mesh;
use super::{Mesh, MeshError, cross3, dot3, sub3};
use crate::fmath;
use crate::textfmt::float_v;
use std::collections::HashMap;
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct DecimateOptions {
    pub target_reduction: f64,
    pub max_error: f64,
    pub limit_error: bool,
    pub preserve_boundary: bool,
}
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct DecimateReport {
    pub triangles_before: i64,
    pub triangles_after: i64,
    pub max_quadric_error: f64,
}
type Quadric = [f64; 10];
fn plane_quadric(n: [f64; 3], d: f64) -> Quadric {
    let (a, b, c) = (n[0], n[1], n[2]);
    [
        a * a,
        a * b,
        a * c,
        a * d,
        b * b,
        b * c,
        b * d,
        c * c,
        c * d,
        d * d,
    ]
}
fn q_add(mut q: Quadric, o: &Quadric) -> Quadric {
    for (x, y) in q.iter_mut().zip(o) {
        *x += y;
    }
    q
}
fn q_scale(mut q: Quadric, s: f64) -> Quadric {
    for x in q.iter_mut() {
        *x *= s;
    }
    q
}
fn q_error(q: &Quadric, p: [f64; 3]) -> f64 {
    let (x, y, z) = (p[0], p[1], p[2]);
    q[0] * x * x
        + 2.0 * q[1] * x * y
        + 2.0 * q[2] * x * z
        + 2.0 * q[3] * x
        + q[4] * y * y
        + 2.0 * q[5] * y * z
        + 2.0 * q[6] * y
        + q[7] * z * z
        + 2.0 * q[8] * z
        + q[9]
}
fn det3(m: &[[f64; 3]; 3]) -> f64 {
    m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
}
fn q_minimum(q: &Quadric) -> Option<[f64; 3]> {
    let a = [[q[0], q[1], q[2]], [q[1], q[4], q[5]], [q[2], q[5], q[7]]];
    let b = [-q[3], -q[6], -q[8]];
    let det = det3(&a);
    let scale = a[0][0].abs() + a[1][1].abs() + a[2][2].abs();
    if scale == 0.0 || det.abs() < 1e-10 * scale * scale * scale {
        return None;
    }
    let solve = |col: usize| -> f64 {
        let mut m = a;
        for (row, value) in m.iter_mut().zip(b) {
            row[col] = value;
        }
        det3(&m) / det
    };
    Some([solve(0), solve(1), solve(2)])
}
#[derive(Debug, Clone, Copy)]
struct Collapse {
    cost: f64,
    u: u32,
    v: u32,
    target: [f64; 3],
    stamp: [u32; 2],
}
#[derive(Default)]
struct CollapseHeap(Vec<Collapse>);
impl CollapseHeap {
    fn less(&self, i: usize, j: usize) -> bool {
        self.0[i].cost < self.0[j].cost
    }
    fn init(&mut self) {
        let n = self.0.len();
        for i in (0..n / 2).rev() {
            self.down(i, n);
        }
    }
    fn push(&mut self, c: Collapse) {
        self.0.push(c);
        let mut j = self.0.len() - 1;
        while j > 0 {
            let i = (j - 1) / 2;
            if !self.less(j, i) {
                break;
            }
            self.0.swap(i, j);
            j = i;
        }
    }
    fn pop(&mut self) -> Option<Collapse> {
        let n = self.0.len().checked_sub(1)?;
        self.0.swap(0, n);
        self.down(0, n);
        self.0.pop()
    }
    fn down(&mut self, i0: usize, n: usize) {
        let mut i = i0;
        loop {
            let j1 = 2 * i + 1;
            if j1 >= n {
                break;
            }
            let mut j = j1;
            if j1 + 1 < n && self.less(j1 + 1, j1) {
                j = j1 + 1;
            }
            if !self.less(j, i) {
                break;
            }
            self.0.swap(i, j);
            i = j;
        }
    }
}
struct Decimator {
    pos: Vec<[f64; 3]>,
    quadrics: Vec<Quadric>,
    tris: Vec<[u32; 3]>,
    alive: Vec<bool>,
    faces: Vec<Vec<i32>>,
    removed: Vec<bool>,
    boundary: Vec<bool>,
    stamp: Vec<u32>,
    neighbour_marks: [Vec<u32>; 2],
    neighbour_epoch: u32,
    heap: CollapseHeap,
    opts: DecimateOptions,
}
fn has_vertex(t: &[u32; 3], v: u32) -> bool {
    t[0] == v || t[1] == v || t[2] == v
}
pub fn decimate_quadric(
    m: &Mesh,
    opts: &DecimateOptions,
) -> Result<(Mesh, DecimateReport), MeshError> {
    m.validate()?;
    if !(opts.target_reduction >= 0.0 && opts.target_reduction < 1.0)
        || (opts.limit_error && (opts.max_error.is_nan() || opts.max_error < 0.0))
    {
        return Err(MeshError::invalid(format!(
            "target reduction {}, max error {}",
            float_v(opts.target_reduction),
            float_v(opts.max_error)
        )));
    }
    let mut report = DecimateReport {
        triangles_before: m.triangles.len() as i64,
        ..DecimateReport::default()
    };
    let mut d = Decimator::new(m, *opts);
    let target =
        fmath::f64_to_i64((m.triangles.len() as f64 * (1.0 - opts.target_reduction)).ceil());
    let mut count = m.triangles.len() as i64;
    while count > target {
        let Some(c) = d.heap.pop() else { break };
        let (u, v) = (c.u as usize, c.v as usize);
        if d.removed[u] || d.removed[v] || d.stamp[u] != c.stamp[0] || d.stamp[v] != c.stamp[1] {
            continue;
        }
        if opts.limit_error && c.cost > opts.max_error {
            break;
        }
        let Some(removed) = d.collapse(&c) else {
            continue;
        };
        count -= removed;
        report.max_quadric_error = fmath::max(report.max_quadric_error, c.cost);
    }
    let out = d.mesh(m.normals.is_some());
    report.triangles_after = out.triangles.len() as i64;
    Ok((out, report))
}
impl Decimator {
    fn new(m: &Mesh, opts: DecimateOptions) -> Decimator {
        let n = m.vertices.len();
        let mut d = Decimator {
            pos: m
                .vertices
                .iter()
                .map(|v| [f64::from(v[0]), f64::from(v[1]), f64::from(v[2])])
                .collect(),
            quadrics: vec![[0.0; 10]; n],
            tris: m.triangles.clone(),
            alive: vec![true; m.triangles.len()],
            faces: vec![Vec::new(); n],
            removed: vec![false; n],
            boundary: vec![false; n],
            stamp: vec![0; n],
            neighbour_marks: [vec![0; n], vec![0; n]],
            neighbour_epoch: 0,
            heap: CollapseHeap::default(),
            opts,
        };
        let mut edges: HashMap<[u32; 2], i64> = HashMap::with_capacity(3 * m.triangles.len() / 2);
        for t in &m.triangles {
            for i in 0..3 {
                let (a, b) = (t[i], t[(i + 1) % 3]);
                *edges.entry([a.min(b), a.max(b)]).or_insert(0) += 1;
            }
        }
        for i in 0..d.tris.len() {
            let t = d.tris[i];
            for v in t {
                d.faces[v as usize].push(i as i32);
            }
            let (normal, area) = d.normal(&t);
            if area == 0.0 {
                continue;
            }
            let q = plane_quadric(normal, -dot3(normal, d.pos[t[0] as usize]));
            for v in t {
                d.quadrics[v as usize] = q_add(d.quadrics[v as usize], &q);
            }
            for k in 0..3 {
                let (a, b) = (t[k], t[(k + 1) % 3]);
                if edges[&[a.min(b), a.max(b)]] != 1 {
                    continue;
                }
                let (a, b) = (a as usize, b as usize);
                d.boundary[a] = true;
                d.boundary[b] = true;
                let edge = sub3(d.pos[b], d.pos[a]);
                let side = cross3(edge, normal);
                let l = dot3(side, side).sqrt();
                if l > 0.0 {
                    let side = [side[0] / l, side[1] / l, side[2] / l];
                    let bq = q_scale(plane_quadric(side, -dot3(side, d.pos[a])), 1000.0);
                    d.quadrics[a] = q_add(d.quadrics[a], &bq);
                    d.quadrics[b] = q_add(d.quadrics[b], &bq);
                }
            }
        }
        let mut keys: Vec<[u32; 2]> = edges.keys().copied().collect();
        keys.sort_unstable();
        for e in keys {
            if let Some(c) = d.candidate(e[0], e[1]) {
                d.heap.0.push(c);
            }
        }
        d.heap.init();
        d
    }
    fn normal(&self, t: &[u32; 3]) -> ([f64; 3], f64) {
        let p = |i: usize| self.pos[t[i] as usize];
        let n = cross3(sub3(p(1), p(0)), sub3(p(2), p(0)));
        let l = dot3(n, n).sqrt();
        if l == 0.0 {
            return ([0.0; 3], 0.0);
        }
        ([n[0] / l, n[1] / l, n[2] / l], l / 2.0)
    }
    fn candidate(&self, u: u32, v: u32) -> Option<Collapse> {
        let (ui, vi) = (u as usize, v as usize);
        if self.opts.preserve_boundary && (self.boundary[ui] || self.boundary[vi]) {
            return None;
        }
        let q = q_add(self.quadrics[ui], &self.quadrics[vi]);
        let minimum = q_minimum(&q);
        let mut ok = minimum.is_some();
        let mut target = minimum.unwrap_or([0.0; 3]);
        let mut cost = if ok {
            q_error(&q, target)
        } else {
            f64::INFINITY
        };
        if !ok
            || (cost > q_error(&q, self.pos[ui]) + 1e-12
                && cost > q_error(&q, self.pos[vi]) + 1e-12)
        {
            let (pu, pv) = (self.pos[ui], self.pos[vi]);
            let mid = [
                (pu[0] + pv[0]) / 2.0,
                (pu[1] + pv[1]) / 2.0,
                (pu[2] + pv[2]) / 2.0,
            ];
            for p in [pu, pv, mid] {
                let e = q_error(&q, p);
                if e < cost || !ok {
                    cost = e;
                    target = p;
                    ok = true;
                }
            }
        }
        Some(Collapse {
            cost: fmath::max(cost, 0.0),
            u,
            v,
            target,
            stamp: [self.stamp[ui], self.stamp[vi]],
        })
    }
    fn neighbours(&mut self, v: u32, slot: usize) -> Vec<u32> {
        self.neighbour_epoch = self.neighbour_epoch.wrapping_add(1);
        if self.neighbour_epoch == 0 {
            self.neighbour_marks[0].fill(0);
            self.neighbour_marks[1].fill(0);
            self.neighbour_epoch = 1;
        }
        let epoch = self.neighbour_epoch;
        let mut out = Vec::new();
        for f in &self.faces[v as usize] {
            if !self.alive[*f as usize] {
                continue;
            }
            for w in self.tris[*f as usize] {
                if w != v && self.neighbour_marks[slot][w as usize] != epoch {
                    self.neighbour_marks[slot][w as usize] = epoch;
                    out.push(w);
                }
            }
        }
        out
    }
    fn collapse(&mut self, c: &Collapse) -> Option<i64> {
        let (u, v) = (c.u, c.v);
        let (ui, vi) = (u as usize, v as usize);
        let nu = self.neighbours(u, 0);
        if !nu.contains(&v) {
            return None;
        }
        self.neighbours(v, 1);
        let nv_epoch = self.neighbour_epoch;
        let edge_faces: Vec<i32> = self.faces[ui]
            .iter()
            .copied()
            .filter(|f| self.alive[*f as usize] && has_vertex(&self.tris[*f as usize], v))
            .collect();
        let shared = nu
            .iter()
            .filter(|w| self.neighbour_marks[1][**w as usize] == nv_epoch)
            .count();
        if shared != edge_faces.len() || edge_faces.is_empty() || edge_faces.len() > 2 {
            return None;
        }
        for vertex in [ui, vi] {
            for k in 0..self.faces[vertex].len() {
                let f = self.faces[vertex][k] as usize;
                let tri = self.tris[f];
                if !self.alive[f] || (has_vertex(&tri, u) && has_vertex(&tri, v)) {
                    continue;
                }
                let (before, _) = self.normal(&tri);
                let saved = self.pos[vertex];
                self.pos[vertex] = c.target;
                let (after, area) = self.normal(&tri);
                self.pos[vertex] = saved;
                if area == 0.0 || dot3(before, after) < 0.2 {
                    return None;
                }
            }
        }
        for f in &edge_faces {
            self.alive[*f as usize] = false;
        }
        let moved = std::mem::take(&mut self.faces[vi]);
        for f in moved {
            if !self.alive[f as usize] {
                continue;
            }
            for corner in self.tris[f as usize].iter_mut() {
                if *corner == v {
                    *corner = u;
                }
            }
            self.faces[ui].push(f);
        }
        self.removed[vi] = true;
        self.pos[ui] = c.target;
        self.quadrics[ui] = q_add(self.quadrics[ui], &self.quadrics[vi]);
        self.boundary[ui] = self.boundary[ui] || self.boundary[vi];
        self.stamp[ui] = self.stamp[ui].wrapping_add(1);
        let alive = &self.alive;
        self.faces[ui].retain(|f| alive[*f as usize]);
        for w in self.neighbours(u, 0) {
            if let Some(candidate) = self.candidate(u, w) {
                self.heap.push(candidate);
            }
        }
        Some(edge_faces.len() as i64)
    }
    fn mesh(&self, normals: bool) -> Mesh {
        let mut used = vec![false; self.pos.len()];
        let mut triangles = Vec::new();
        for (i, t) in self.tris.iter().enumerate() {
            if self.alive[i] {
                triangles.push(*t);
                for k in t {
                    used[*k as usize] = true;
                }
            }
        }
        let vertices: Vec<[f32; 3]> = self
            .pos
            .iter()
            .map(|p| [p[0] as f32, p[1] as f32, p[2] as f32])
            .collect();
        compact_mesh(&vertices, &triangles, &used, normals)
    }
}
