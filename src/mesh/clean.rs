use super::{Mesh, dot3, face_normals};
use crate::fmath;
use std::collections::{HashMap, HashSet};
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CleanReport {
    pub merged_vertices: i64,
    pub degenerate_triangles: i64,
    pub duplicate_triangles: i64,
    pub unused_vertices: i64,
}
pub fn clean(m: &Mesh, tolerance: f64) -> (Mesh, CleanReport) {
    let mut report = CleanReport::default();
    let representative = merge_vertices(&m.vertices, fmath::max(tolerance, 0.0));
    for (i, r) in representative.iter().enumerate() {
        if *r != i as u32 {
            report.merged_vertices += 1;
        }
    }
    let mut seen: HashSet<[u32; 3]> = HashSet::with_capacity(m.triangles.len());
    let mut used = vec![false; m.vertices.len()];
    let mut triangles = Vec::new();
    for t in &m.triangles {
        let t = [
            representative[t[0] as usize],
            representative[t[1] as usize],
            representative[t[2] as usize],
        ];
        if t[0] == t[1] || t[1] == t[2] || t[0] == t[2] {
            report.degenerate_triangles += 1;
            continue;
        }
        if !seen.insert(canonical_triangle(t)) {
            report.duplicate_triangles += 1;
            continue;
        }
        for k in t {
            used[k as usize] = true;
        }
        triangles.push(t);
    }
    let out = compact_mesh(&m.vertices, &triangles, &used, m.normals.is_some());
    report.unused_vertices =
        m.vertices.len() as i64 - report.merged_vertices - out.vertices.len() as i64;
    (out, report)
}
pub(crate) fn canonical_triangle(t: [u32; 3]) -> [u32; 3] {
    if t[1] < t[0] && t[1] < t[2] {
        [t[1], t[2], t[0]]
    } else if t[2] < t[0] && t[2] < t[1] {
        [t[2], t[0], t[1]]
    } else {
        t
    }
}
fn position_key(v: [f32; 3]) -> [u32; 3] {
    v.map(|c| if c == 0.0 { 0 } else { c.to_bits() })
}
pub(crate) fn merge_vertices(vertices: &[[f32; 3]], tolerance: f64) -> Vec<u32> {
    let mut representative = vec![0u32; vertices.len()];
    if tolerance == 0.0 {
        let mut first: HashMap<[u32; 3], u32> = HashMap::with_capacity(vertices.len());
        for (i, v) in vertices.iter().enumerate() {
            representative[i] = i as u32;
            if v.iter().any(|c| c.is_nan()) {
                continue;
            }
            match first.get(&position_key(*v)) {
                Some(j) => representative[i] = *j,
                None => {
                    first.insert(position_key(*v), i as u32);
                }
            }
        }
        return representative;
    }
    let cell = |v: [f32; 3]| -> [i64; 3] {
        [
            fmath::f64_to_i64((f64::from(v[0]) / tolerance).floor()),
            fmath::f64_to_i64((f64::from(v[1]) / tolerance).floor()),
            fmath::f64_to_i64((f64::from(v[2]) / tolerance).floor()),
        ]
    };
    let mut grid: HashMap<[i64; 3], Vec<u32>> = HashMap::with_capacity(vertices.len());
    let limit = tolerance * tolerance;
    for (i, v) in vertices.iter().enumerate() {
        let c = cell(*v);
        representative[i] = i as u32;
        let mut found = false;
        'search: for dz in -1i64..=1 {
            for dy in -1i64..=1 {
                for dx in -1i64..=1 {
                    let key = [
                        c[0].wrapping_add(dx),
                        c[1].wrapping_add(dy),
                        c[2].wrapping_add(dz),
                    ];
                    let Some(bucket) = grid.get(&key) else {
                        continue;
                    };
                    for j in bucket {
                        let w = vertices[*j as usize];
                        let (x, y, z) = (
                            f64::from(v[0] - w[0]),
                            f64::from(v[1] - w[1]),
                            f64::from(v[2] - w[2]),
                        );
                        if x * x + y * y + z * z <= limit {
                            representative[i] = *j;
                            found = true;
                            break 'search;
                        }
                    }
                }
            }
        }
        if !found {
            grid.entry(c).or_default().push(i as u32);
        }
    }
    representative
}
pub(crate) fn compact_mesh(
    vertices: &[[f32; 3]],
    triangles: &[[u32; 3]],
    used: &[bool],
    normals: bool,
) -> Mesh {
    let mut index = vec![0u32; vertices.len()];
    let mut out = Mesh::default();
    for (i, u) in used.iter().enumerate() {
        if *u {
            index[i] = out.vertices.len() as u32;
            out.vertices.push(vertices[i]);
        }
    }
    out.triangles = triangles
        .iter()
        .map(|t| {
            [
                index[t[0] as usize],
                index[t[1] as usize],
                index[t[2] as usize],
            ]
        })
        .collect();
    if normals {
        face_normals(&mut out, None);
    }
    out
}
pub fn connected_components(m: &Mesh) -> (Vec<i64>, i64) {
    let mut parent: Vec<i32> = (0..m.vertices.len() as i32).collect();
    fn find(parent: &mut [i32], mut x: i32) -> i32 {
        while parent[x as usize] != x {
            parent[x as usize] = parent[parent[x as usize] as usize];
            x = parent[x as usize];
        }
        x
    }
    for t in &m.triangles {
        let a = find(&mut parent, t[0] as i32);
        let b = find(&mut parent, t[1] as i32);
        let c = find(&mut parent, t[2] as i32);
        parent[b as usize] = a;
        let rc = find(&mut parent, c);
        parent[rc as usize] = a;
    }
    let mut roots: HashMap<i32, usize> = HashMap::new();
    let mut sizes: Vec<i64> = Vec::new();
    let mut raw = vec![0usize; m.triangles.len()];
    for (i, t) in m.triangles.iter().enumerate() {
        let root = find(&mut parent, t[0] as i32);
        let id = *roots.entry(root).or_insert_with(|| {
            sizes.push(0);
            sizes.len() - 1
        });
        sizes[id] += 1;
        raw[i] = id;
    }
    let mut order: Vec<usize> = (0..sizes.len()).collect();
    order.sort_by(|a, b| sizes[*b].cmp(&sizes[*a]));
    let mut rank = vec![0i64; sizes.len()];
    for (r, id) in order.iter().enumerate() {
        rank[*id] = r as i64;
    }
    (raw.iter().map(|id| rank[*id]).collect(), sizes.len() as i64)
}
pub fn largest_component(m: &Mesh) -> Mesh {
    let (component, count) = connected_components(m);
    if count == 0 {
        return Mesh::default();
    }
    let mut used = vec![false; m.vertices.len()];
    let mut triangles = Vec::new();
    for (i, t) in m.triangles.iter().enumerate() {
        if component[i] == 0 {
            triangles.push(*t);
            for k in t {
                used[*k as usize] = true;
            }
        }
    }
    compact_mesh(&m.vertices, &triangles, &used, m.normals.is_some())
}
pub fn fill_holes(m: &Mesh, max_radius: f64) -> (Mesh, i64) {
    let mut directed: HashSet<[u32; 2]> = HashSet::with_capacity(3 * m.triangles.len());
    let mut ordered: Vec<[u32; 2]> = Vec::new();
    for t in &m.triangles {
        for i in 0..3 {
            let e = [t[i], t[(i + 1) % 3]];
            if directed.insert(e) {
                ordered.push(e);
            }
        }
    }
    let mut next: HashMap<u32, u32> = HashMap::new();
    let mut branching: HashSet<u32> = HashSet::new();
    for e in &ordered {
        if directed.contains(&[e[1], e[0]]) {
            continue;
        }
        if next.insert(e[0], e[1]).is_some() {
            branching.insert(e[0]);
        }
    }
    let mut out = Mesh {
        vertices: m.vertices.clone(),
        normals: None,
        triangles: m.triangles.clone(),
    };
    let mut visited: HashSet<u32> = HashSet::new();
    let mut starts: Vec<u32> = next.keys().copied().collect();
    starts.sort_unstable();
    let mut filled = 0;
    for first in starts {
        if visited.contains(&first) {
            continue;
        }
        let mut lp = vec![first];
        let (mut closed, mut simple) = (false, !branching.contains(&first));
        visited.insert(first);
        let mut current = next[&first];
        loop {
            if current == first {
                closed = true;
                break;
            }
            if visited.contains(&current) {
                break;
            }
            let Some(following) = next.get(&current) else {
                break;
            };
            if branching.contains(&current) {
                simple = false;
            }
            visited.insert(current);
            lp.push(current);
            current = *following;
        }
        if !closed || !simple || lp.len() < 3 {
            continue;
        }
        let mut centre = [0.0f64; 3];
        for v in &lp {
            let p = m.vertices[*v as usize];
            centre[0] += f64::from(p[0]);
            centre[1] += f64::from(p[1]);
            centre[2] += f64::from(p[2]);
        }
        for axis in centre.iter_mut() {
            *axis /= lp.len() as f64;
        }
        let mut radius = 0.0;
        for v in &lp {
            let p = m.vertices[*v as usize];
            let d = [
                f64::from(p[0]) - centre[0],
                f64::from(p[1]) - centre[1],
                f64::from(p[2]) - centre[2],
            ];
            radius = fmath::max(radius, dot3(d, d).sqrt());
        }
        if radius > max_radius {
            continue;
        }
        if lp.len() == 3 {
            out.triangles.push([lp[0], lp[2], lp[1]]);
        } else {
            let c = out.vertices.len() as u32;
            out.vertices
                .push([centre[0] as f32, centre[1] as f32, centre[2] as f32]);
            for (i, a) in lp.iter().enumerate() {
                let b = lp[(i + 1) % lp.len()];
                out.triangles.push([b, *a, c]);
            }
        }
        filled += 1;
    }
    if m.normals.is_some() {
        face_normals(&mut out, None);
    }
    (out, filled)
}
