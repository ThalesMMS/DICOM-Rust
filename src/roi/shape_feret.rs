use super::pdqsort_by::sort_slice;
use super::shape_stats::{ShapeGrid, ShapeRun};
use crate::fmath;
const FERET_LEAF_SIZE: usize = 8;
#[derive(Debug, Clone, Copy)]
struct FeretNode {
    lo: [f64; 3],
    hi: [f64; 3],
    start: usize,
    end: usize,
    left: i64,
    right: i64,
}
struct FeretTree {
    pts: Vec<[f64; 3]>,
    idx: Vec<usize>,
    nodes: Vec<FeretNode>,
    best: f64,
    a: usize,
    b: usize,
}
impl ShapeGrid {
    pub(crate) fn feret_diameter(
        &self,
        runs: &[ShapeRun],
        lo: [i64; 3],
        hi: [i64; 3],
    ) -> (f64, [f64; 3], [f64; 3]) {
        let pts = self.feret_candidates(runs, lo, hi);
        if pts.len() == 1 {
            return (0.0, pts[0], pts[0]);
        }
        let n = pts.len();
        let mut t = FeretTree {
            pts,
            idx: (0..n).collect(),
            nodes: Vec::new(),
            best: 0.0,
            a: 0,
            b: 0,
        };
        t.seed();
        t.build(0, n);
        for i in 0..n {
            t.search(0, i);
        }
        (t.best.sqrt(), t.pts[t.a], t.pts[t.b])
    }
    fn feret_candidates(&self, runs: &[ShapeRun], lo: [i64; 3], hi: [i64; 3]) -> Vec<[f64; 3]> {
        let g = self;
        let (nx, ny, nz) = (hi[0] - lo[0] + 1, hi[1] - lo[1] + 1, hi[2] - lo[2] + 1);
        let mut y_min = vec![i32::MAX; (nx * nz) as usize];
        let mut y_max = vec![-1i32; (nx * nz) as usize];
        let mut z_min = vec![i32::MAX; (nx * ny) as usize];
        let mut z_max = vec![-1i32; (nx * ny) as usize];
        for r in runs {
            let (y, z) = (r.y as i32, r.z as i32);
            for x in r.x0..=r.x1 {
                let iy = ((r.z - lo[2]) * nx + (x - lo[0])) as usize;
                y_min[iy] = y_min[iy].min(y);
                y_max[iy] = y_max[iy].max(y);
                let iz = ((r.y - lo[1]) * nx + (x - lo[0])) as usize;
                z_min[iz] = z_min[iz].min(z);
                z_max[iz] = z_max[iz].max(z);
            }
        }
        let extreme = |x: i64, y: i64, z: i64| -> bool {
            let iy = ((z - lo[2]) * nx + (x - lo[0])) as usize;
            if y as i32 != y_min[iy] && y as i32 != y_max[iy] {
                return false;
            }
            if g.dim == 2 {
                return true;
            }
            let iz = ((y - lo[1]) * nx + (x - lo[0])) as usize;
            z as i32 == z_min[iz] || z as i32 == z_max[iz]
        };
        let mut pts: Vec<[f64; 3]> = Vec::new();
        let mut add = |x: i64, y: i64, z: i64| {
            if extreme(x, y, z) {
                let mut p = [
                    g.coord[0][x as usize],
                    g.coord[1][y as usize],
                    g.coord[2][z as usize],
                ];
                if g.dim == 2 {
                    p[2] = 0.0;
                }
                pts.push(p);
            }
        };
        let mut i = 0usize;
        while i < runs.len() {
            let mut j = i + 1;
            while j < runs.len() && runs[j].y == runs[i].y && runs[j].z == runs[i].z {
                j += 1;
            }
            let (first, last) = (runs[i].x0, runs[j - 1].x1);
            add(first, runs[i].y, runs[i].z);
            if last != first {
                add(last, runs[i].y, runs[i].z);
            }
            i = j;
        }
        pts
    }
}
fn dist2(p: [f64; 3], q: [f64; 3]) -> f64 {
    let (dx, dy, dz) = (p[0] - q[0], p[1] - q[1], p[2] - q[2]);
    dx * dx + dy * dy + dz * dz
}
impl FeretNode {
    fn farthest2(&self, p: [f64; 3]) -> f64 {
        let mut s = 0.0f64;
        for k in 0..3 {
            let d = fmath::max((p[k] - self.lo[k]).abs(), (p[k] - self.hi[k]).abs());
            s += d * d;
        }
        s
    }
}
impl FeretTree {
    fn seed(&mut self) {
        let mut ext: Vec<usize> = Vec::new();
        for dx in -1..=1i64 {
            for dy in -1..=1i64 {
                for dz in -1..=1i64 {
                    if dx < 0 || dx == 0 && (dy < 0 || dy == 0 && dz <= 0) {
                        continue;
                    }
                    let (fx, fy, fz) = (dx as f64, dy as f64, dz as f64);
                    let (mut lo, mut hi) = (0usize, 0usize);
                    for (i, p) in self.pts.iter().enumerate() {
                        let v = fx * p[0] + fy * p[1] + fz * p[2];
                        let l = self.pts[lo];
                        if v < fx * l[0] + fy * l[1] + fz * l[2] {
                            lo = i;
                        }
                        let h = self.pts[hi];
                        if v > fx * h[0] + fy * h[1] + fz * h[2] {
                            hi = i;
                        }
                    }
                    ext.push(lo);
                    ext.push(hi);
                }
            }
        }
        self.best = -1.0;
        self.a = ext[0];
        self.b = ext[0];
        for (i, &p) in ext.iter().enumerate() {
            for &q in &ext[i + 1..] {
                let d = dist2(self.pts[p], self.pts[q]);
                if d > self.best {
                    self.best = d;
                    self.a = p;
                    self.b = q;
                }
            }
        }
    }
    fn build(&mut self, start: usize, end: usize) -> i64 {
        let first = self.pts[self.idx[start]];
        let mut node = FeretNode {
            lo: first,
            hi: first,
            start,
            end,
            left: -1,
            right: -1,
        };
        for &i in &self.idx[start + 1..end] {
            for k in 0..3 {
                node.lo[k] = fmath::min(node.lo[k], self.pts[i][k]);
                node.hi[k] = fmath::max(node.hi[k], self.pts[i][k]);
            }
        }
        let pos = self.nodes.len();
        self.nodes.push(node);
        if end - start > FERET_LEAF_SIZE {
            let mut axis = 0usize;
            for k in 1..3 {
                if node.hi[k] - node.lo[k] > node.hi[axis] - node.lo[axis] {
                    axis = k;
                }
            }
            let pts = &self.pts;
            sort_slice(&mut self.idx[start..end], &|a: &usize, b: &usize| {
                pts[*a][axis] < pts[*b][axis]
            });
            let mid = start + (end - start) / 2;
            let left = self.build(start, mid);
            let right = self.build(mid, end);
            self.nodes[pos].left = left;
            self.nodes[pos].right = right;
        }
        pos as i64
    }
    fn search(&mut self, node: usize, i: usize) {
        let n = self.nodes[node];
        let p = self.pts[i];
        if n.farthest2(p) <= self.best {
            return;
        }
        if n.left < 0 {
            for at in n.start..n.end {
                let j = self.idx[at];
                let d = dist2(p, self.pts[j]);
                if d > self.best {
                    self.best = d;
                    self.a = i.min(j);
                    self.b = i.max(j);
                }
            }
            return;
        }
        let (mut first, mut second) = (n.left as usize, n.right as usize);
        if self.nodes[second].farthest2(p) > self.nodes[first].farthest2(p) {
            std::mem::swap(&mut first, &mut second);
        }
        self.search(first, i);
        self.search(second, i);
    }
}
