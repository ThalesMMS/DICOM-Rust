use super::bvh::scale3;
use super::curvature::edge_id;
use super::locator::{SurfacePoint, vertex_point};
use super::{ErrorKind, Mesh, MeshError, add3, dot3, sub3};
use std::collections::{HashMap, HashSet};
pub const DEFAULT_GEODESIC_STEINER_POINTS: i64 = 3;
pub const MAX_GEODESIC_STEINER_POINTS: i64 = 16;
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GeodesicOptions {
    pub steiner_points: i64,
    pub edges_only: bool,
}
pub struct GeodesicGraph<'a> {
    m: &'a Mesh,
    steiner: u32,
    vertices: u32,
    nodes: u32,
    edges: Vec<[u32; 2]>,
    tri_edges: Vec<[u32; 3]>,
    vertex_start: Vec<u32>,
    edge_start: Vec<u32>,
    vertex_tris: Vec<u32>,
    edge_tris: Vec<u32>,
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GeodesicPath {
    pub points: Vec<[f64; 3]>,
    pub length: f64,
}
fn distance3(a: [f64; 3], b: [f64; 3]) -> f64 {
    let d = sub3(a, b);
    dot3(d, d).sqrt()
}
fn no_path() -> MeshError {
    MeshError {
        kind: ErrorKind::NoPath,
        message: "mesh: no path on the surface between the points".to_string(),
        budget: None,
    }
}
const START: i32 = -2;
const UNSET: i32 = -1;
#[derive(Clone, Copy)]
struct Item {
    f: f64,
    g: f64,
    node: u32,
}
struct Search {
    dist: Vec<f64>,
    prev: Vec<i32>,
    queue: Vec<Item>,
}
impl Search {
    fn new(nodes: u32) -> Search {
        Search {
            dist: vec![f64::INFINITY; nodes as usize],
            prev: vec![0; nodes as usize],
            queue: Vec::new(),
        }
    }
    fn push(&mut self, n: u32, g: f64, prev: i32, h: f64) {
        if g >= self.dist[n as usize] {
            return;
        }
        self.dist[n as usize] = g;
        self.prev[n as usize] = prev;
        let item = Item {
            f: g + h,
            g,
            node: n,
        };
        let heap = &mut self.queue;
        heap.push(item);
        let mut i = heap.len() - 1;
        while i > 0 {
            let parent = (i - 1) / 2;
            if heap[parent].f <= item.f {
                break;
            }
            heap[i] = heap[parent];
            i = parent;
        }
        heap[i] = item;
    }
    fn pop(&mut self) -> Option<Item> {
        let heap = &mut self.queue;
        let last = heap.pop()?;
        let n = heap.len();
        if n == 0 {
            return Some(last);
        }
        let top = heap[0];
        let mut i = 0;
        loop {
            let mut child = 2 * i + 1;
            if child >= n {
                break;
            }
            if child + 1 < n && heap[child + 1].f < heap[child].f {
                child += 1;
            }
            if last.f <= heap[child].f {
                break;
            }
            heap[i] = heap[child];
            i = child;
        }
        heap[i] = last;
        Some(top)
    }
}
impl<'a> GeodesicGraph<'a> {
    pub fn new(m: &'a Mesh, opts: &GeodesicOptions) -> Result<GeodesicGraph<'a>, MeshError> {
        m.validate()?;
        let mut steiner = if opts.steiner_points == 0 {
            DEFAULT_GEODESIC_STEINER_POINTS
        } else {
            opts.steiner_points
        };
        if opts.edges_only {
            steiner = 0;
        }
        if !(0..=MAX_GEODESIC_STEINER_POINTS).contains(&steiner) {
            return Err(MeshError::invalid(format!(
                "{} Steiner points per edge (0..{MAX_GEODESIC_STEINER_POINTS})",
                opts.steiner_points
            )));
        }
        let vertices = m.vertices.len();
        let mut edges: Vec<[u32; 2]> = Vec::new();
        let mut tri_edges = vec![[0u32; 3]; m.triangles.len()];
        let mut ids: HashMap<u64, u32> = HashMap::with_capacity(3 * m.triangles.len() / 2);
        let mut vertex_start = vec![0u32; vertices + 1];
        for (ti, t) in m.triangles.iter().enumerate() {
            for k in 0..3 {
                let key = edge_id(t[k], t[(k + 1) % 3]);
                let e = *ids.entry(key).or_insert_with(|| {
                    edges.push([(key >> 32) as u32, key as u32]);
                    edges.len() as u32 - 1
                });
                tri_edges[ti][k] = e;
                vertex_start[t[k] as usize + 1] += 1;
            }
        }
        let nodes = vertices as u64 + edges.len() as u64 * steiner as u64;
        if nodes >= i32::MAX as u64 {
            return Err(MeshError {
                kind: ErrorKind::BudgetExceeded,
                message: format!("mesh: surface exceeds budget: geodesic graph of {nodes} nodes"),
                budget: None,
            });
        }
        for v in 1..vertex_start.len() {
            vertex_start[v] += vertex_start[v - 1];
        }
        let mut vertex_tris = vec![0u32; vertex_start[vertices] as usize];
        let mut fill: Vec<u32> = vertex_start[..vertices].to_vec();
        let mut edge_start = vec![0u32; edges.len() + 1];
        for (ti, t) in m.triangles.iter().enumerate() {
            for k in 0..3 {
                let v = t[k] as usize;
                vertex_tris[fill[v] as usize] = ti as u32;
                fill[v] += 1;
                edge_start[tri_edges[ti][k] as usize + 1] += 1;
            }
        }
        for e in 1..edge_start.len() {
            edge_start[e] += edge_start[e - 1];
        }
        let mut edge_tris = vec![0u32; edge_start[edges.len()] as usize];
        let mut fill: Vec<u32> = edge_start[..edges.len()].to_vec();
        for (ti, corners) in tri_edges.iter().enumerate() {
            for e in corners {
                edge_tris[fill[*e as usize] as usize] = ti as u32;
                fill[*e as usize] += 1;
            }
        }
        Ok(GeodesicGraph {
            m,
            steiner: steiner as u32,
            vertices: vertices as u32,
            nodes: nodes as u32,
            edges,
            tri_edges,
            vertex_start,
            edge_start,
            vertex_tris,
            edge_tris,
        })
    }
    pub fn mesh(&self) -> &'a Mesh {
        self.m
    }
    fn position(&self, n: u32) -> [f64; 3] {
        if n < self.vertices {
            return self.m.point(n);
        }
        let (e, j) = (
            (n - self.vertices) / self.steiner,
            (n - self.vertices) % self.steiner,
        );
        let edge = self.edges[e as usize];
        let (a, b) = (self.m.point(edge[0]), self.m.point(edge[1]));
        let s = f64::from(j + 1) / f64::from(self.steiner + 1);
        add3(a, scale3(sub3(b, a), s))
    }
    fn triangles(&self, n: u32) -> &[u32] {
        if n < self.vertices {
            let n = n as usize;
            return &self.vertex_tris
                [self.vertex_start[n] as usize..self.vertex_start[n + 1] as usize];
        }
        let e = ((n - self.vertices) / self.steiner) as usize;
        &self.edge_tris[self.edge_start[e] as usize..self.edge_start[e + 1] as usize]
    }
    fn triangle_nodes(&self, ti: u32, buf: &mut Vec<u32>) {
        buf.clear();
        buf.extend_from_slice(&self.m.triangles[ti as usize]);
        for e in self.tri_edges[ti as usize] {
            let base = self.vertices + e * self.steiner;
            buf.extend(base..base + self.steiner);
        }
    }
    pub fn shortest_path(&self, from: u32, to: u32) -> Result<GeodesicPath, MeshError> {
        match (vertex_point(self.m, from), vertex_point(self.m, to)) {
            (Some(a), Some(b)) => self.shortest_path_between(&a, &b),
            _ => Err(MeshError::invalid(format!(
                "geodesic between vertices {from} and {to} of a mesh of {} vertices (both must be used by a triangle)",
                self.m.vertices.len()
            ))),
        }
    }
    pub fn shortest_path_between(
        &self,
        from: &SurfacePoint,
        to: &SurfacePoint,
    ) -> Result<GeodesicPath, MeshError> {
        let count = self.m.triangles.len();
        if from.triangle as usize >= count || to.triangle as usize >= count {
            return Err(MeshError::invalid("surface point on a missing triangle"));
        }
        let (p, q) = (self.surface_point(from), self.surface_point(to));
        let mut buf: Vec<u32> = Vec::with_capacity(3 + 3 * self.steiner as usize);
        self.triangle_nodes(to.triangle, &mut buf);
        let target_nodes: HashSet<u32> = buf.iter().copied().collect();
        let (mut best, mut best_prev) = (f64::INFINITY, UNSET);
        if from.triangle == to.triangle {
            best = distance3(p, q);
            best_prev = START;
        }
        let mut s = Search::new(self.nodes);
        self.triangle_nodes(from.triangle, &mut buf);
        for n in &buf {
            let at = self.position(*n);
            s.push(*n, distance3(p, at), START, distance3(at, q));
        }
        while let Some(item) = s.pop() {
            if item.f >= best {
                break;
            }
            if item.g > s.dist[item.node as usize] {
                continue;
            }
            let (n, at) = (item.node, self.position(item.node));
            if target_nodes.contains(&n) {
                let d = item.g + distance3(at, q);
                if d < best {
                    best = d;
                    best_prev = n as i32;
                }
            }
            for ti in self.triangles(n) {
                self.triangle_nodes(*ti, &mut buf);
                for next in &buf {
                    if *next == n {
                        continue;
                    }
                    let there = self.position(*next);
                    s.push(
                        *next,
                        item.g + distance3(at, there),
                        n as i32,
                        distance3(there, q),
                    );
                }
            }
        }
        if best_prev == UNSET {
            return Err(no_path());
        }
        let mut points = vec![q];
        let mut n = best_prev;
        while n != START {
            points.push(self.position(n as u32));
            n = s.prev[n as usize];
        }
        points.push(p);
        let mut path = GeodesicPath {
            points: Vec::with_capacity(points.len()),
            length: 0.0,
        };
        for point in points.iter().rev() {
            if let Some(last) = path.points.last() {
                if point == last {
                    continue;
                }
                path.length += distance3(*last, *point);
            }
            path.points.push(*point);
        }
        Ok(path)
    }
    pub fn distances(&self, sources: &[u32], max_distance: f64) -> Result<Vec<f64>, MeshError> {
        if sources.is_empty() {
            return Err(MeshError::invalid("geodesic distances without a source"));
        }
        let max_distance = if max_distance > 0.0 {
            max_distance
        } else {
            f64::INFINITY
        };
        let mut s = Search::new(self.nodes);
        for v in sources {
            if *v >= self.vertices {
                return Err(MeshError::invalid(format!(
                    "source vertex {v} of a mesh of {} vertices",
                    self.vertices
                )));
            }
            s.push(*v, 0.0, START, 0.0);
        }
        let mut buf: Vec<u32> = Vec::with_capacity(3 + 3 * self.steiner as usize);
        while let Some(item) = s.pop() {
            if item.g > s.dist[item.node as usize] {
                continue;
            }
            if item.g > max_distance {
                break;
            }
            let (n, at) = (item.node, self.position(item.node));
            for ti in self.triangles(n) {
                self.triangle_nodes(*ti, &mut buf);
                for next in &buf {
                    if *next != n {
                        s.push(
                            *next,
                            item.g + distance3(at, self.position(*next)),
                            n as i32,
                            0.0,
                        );
                    }
                }
            }
        }
        Ok(s.dist[..self.vertices as usize]
            .iter()
            .map(|d| if *d > max_distance { f64::INFINITY } else { *d })
            .collect())
    }
    fn surface_point(&self, s: &SurfacePoint) -> [f64; 3] {
        let t = self.m.triangles[s.triangle as usize];
        let mut p = [0.0; 3];
        for k in 0..3 {
            p = add3(p, scale3(self.m.point(t[k]), s.barycentric[k]));
        }
        p
    }
}
