use super::{add3, cross3, dot3, sub3};
use std::collections::HashMap;
use std::sync::OnceLock;
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct CubeEdge {
    pub base: u8,
    pub axis: u8,
}
pub(crate) struct CaseTable {
    pub edges: [CubeEdge; 12],
    start: [u16; 257],
    tris: Vec<u8>,
    pub count: [u8; 256],
}
impl CaseTable {
    pub fn triangles(&self, c: usize) -> &[u8] {
        &self.tris[self.start[c] as usize..self.start[c + 1] as usize]
    }
}
pub(crate) fn mc_table() -> &'static CaseTable {
    static TABLE: OnceLock<CaseTable> = OnceLock::new();
    TABLE.get_or_init(build_case_table)
}
fn build_case_table() -> CaseTable {
    let mut edges = [CubeEdge::default(); 12];
    let mut edge_id = [[0usize; 8]; 8];
    let mut n = 0;
    for a in 0..8usize {
        for axis in 0..3 {
            if (a >> axis) & 1 == 0 {
                let b = a | (1 << axis);
                edges[n] = CubeEdge {
                    base: a as u8,
                    axis: axis as u8,
                };
                edge_id[a][b] = n;
                edge_id[b][a] = n;
                n += 1;
            }
        }
    }
    let faces = cube_faces();
    let mut edge_faces = [0u8; 12];
    for (f, face) in faces.iter().enumerate() {
        for i in 0..4 {
            edge_faces[edge_id[face[i]][face[(i + 1) % 4]]] |= 1 << f;
        }
    }
    let loops_of = |c: usize| -> Vec<Vec<usize>> {
        let mut next = [usize::MAX; 12];
        for face in &faces {
            let mut inside = [false; 4];
            for (i, corner) in face.iter().enumerate() {
                inside[i] = (c >> corner) & 1 == 1;
            }
            for i in 0..4 {
                let prev = (i + 3) % 4;
                if !inside[i] || inside[prev] {
                    continue;
                }
                let mut j = i;
                while inside[(j + 1) % 4] {
                    j = (j + 1) % 4;
                }
                let from = edge_id[face[prev]][face[i]];
                let to = edge_id[face[j]][face[(j + 1) % 4]];
                next[from] = to;
            }
        }
        let mut loops = Vec::new();
        let mut seen = [false; 12];
        for e in 0..12 {
            if next[e] == usize::MAX || seen[e] {
                continue;
            }
            let mut lp = Vec::new();
            let mut cur = e;
            while !seen[cur] {
                seen[cur] = true;
                lp.push(cur);
                cur = next[cur];
            }
            loops.push(lp);
        }
        loops
    };
    let reverse = {
        let lp = &loops_of(1)[0];
        let p = |e: usize| edge_midpoint(edges[e]);
        let (a, b, c) = (p(lp[0]), p(lp[1]), p(lp[2]));
        let normal = cross3(sub3(b, a), sub3(c, a));
        dot3(normal, add3(add3(a, b), c)) < 0.0
    };
    let mut start = [0u16; 257];
    let mut tris: Vec<u8> = Vec::new();
    for (c, slot) in start.iter_mut().enumerate().take(256) {
        *slot = tris.len() as u16;
        for mut lp in loops_of(c) {
            if reverse {
                lp.reverse();
            }
            let fan = triangulate_loop(&lp, &edge_faces)
                .expect("mesh: no face-safe triangulation for a marching cubes loop");
            for tri in fan {
                tris.extend(tri.iter().map(|e| *e as u8));
            }
        }
    }
    start[256] = tris.len() as u16;
    let mut count = [0u8; 256];
    for (c, slot) in count.iter_mut().enumerate() {
        *slot = ((start[c + 1] - start[c]) / 3) as u8;
    }
    CaseTable {
        edges,
        start,
        tris,
        count,
    }
}
fn triangulate_loop(lp: &[usize], edge_faces: &[u8; 12]) -> Option<Vec<[usize; 3]>> {
    struct Search<'a> {
        lp: &'a [usize],
        edge_faces: &'a [u8; 12],
        memo: HashMap<(usize, usize), bool>,
        tris: Vec<[usize; 3]>,
    }
    impl Search<'_> {
        fn chord(&self, i: usize, j: usize) -> bool {
            if j - i == 1 || (i == 0 && j == self.lp.len() - 1) {
                return true;
            }
            self.edge_faces[self.lp[i]] & self.edge_faces[self.lp[j]] == 0
        }
        fn possible(&mut self, i: usize, j: usize) -> bool {
            if j - i < 2 {
                return true;
            }
            if let Some(v) = self.memo.get(&(i, j)) {
                return *v;
            }
            let mut ok = false;
            let mut k = j - 1;
            while k > i && !ok {
                ok = self.chord(i, k)
                    && self.chord(k, j)
                    && self.possible(i, k)
                    && self.possible(k, j);
                k -= 1;
            }
            self.memo.insert((i, j), ok);
            ok
        }
        fn build(&mut self, i: usize, j: usize) {
            if j - i < 2 {
                return;
            }
            let mut k = j - 1;
            while k > i {
                if self.chord(i, k)
                    && self.chord(k, j)
                    && self.possible(i, k)
                    && self.possible(k, j)
                {
                    self.tris.push([self.lp[i], self.lp[k], self.lp[j]]);
                    self.build(i, k);
                    self.build(k, j);
                    return;
                }
                k -= 1;
            }
        }
    }
    let n = lp.len();
    let mut s = Search {
        lp,
        edge_faces,
        memo: HashMap::new(),
        tris: Vec::new(),
    };
    if !s.possible(0, n - 1) {
        return None;
    }
    s.build(0, n - 1);
    Some(s.tris)
}
fn cube_faces() -> [[usize; 4]; 6] {
    let mut faces = [[0usize; 4]; 6];
    let mut n = 0;
    for axis in 0..3 {
        let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
        for side in 0..2usize {
            let order: [[usize; 2]; 4] = if side == 0 {
                [[0, 0], [0, 1], [1, 1], [1, 0]]
            } else {
                [[0, 0], [1, 0], [1, 1], [0, 1]]
            };
            for (i, uv) in order.iter().enumerate() {
                faces[n][i] = (side << axis) | (uv[0] << u) | (uv[1] << v);
            }
            n += 1;
        }
    }
    faces
}
fn edge_midpoint(e: CubeEdge) -> [f64; 3] {
    let mut p = [
        f64::from(e.base & 1),
        f64::from((e.base >> 1) & 1),
        f64::from((e.base >> 2) & 1),
    ];
    p[e.axis as usize] += 0.5;
    p
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn table_is_complete() {
        let t = mc_table();
        assert!(t.triangles(0).is_empty());
        assert!(t.triangles(255).is_empty());
        assert_eq!(t.triangles(1).len(), 3);
        assert_eq!(t.count[1], 1);
    }
}
