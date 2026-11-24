use super::extract::{Options, check_output_budget, validate_field_dimensions};
use super::field::AffineFrame;
use super::{Mesh, MeshError, dims_v, face_normals, mesh_bytes};
use crate::fmath;
use crate::render::GeometryAffine;
use crate::textfmt::float_v;
use std::collections::BTreeSet;
pub trait LabelField {
    fn dims(&self) -> [i64; 3];
    fn index_to_patient(&self) -> GeometryAffine;
    fn read_labels(&self, z: i64, dst: &mut [u32]) -> Result<(), MeshError>;
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DenseLabelField {
    pub size: [i64; 3],
    pub affine: GeometryAffine,
    pub labels: Vec<u32>,
}
impl LabelField for DenseLabelField {
    fn dims(&self) -> [i64; 3] {
        self.size
    }
    fn index_to_patient(&self) -> GeometryAffine {
        self.affine
    }
    fn read_labels(&self, z: i64, dst: &mut [u32]) -> Result<(), MeshError> {
        let n = self.size[0] * self.size[1];
        if z < 0
            || z >= self.size[2]
            || (dst.len() as i64) < n
            || (self.labels.len() as i64) < (z + 1) * n
        {
            return Err(MeshError::invalid(format!(
                "label slice {z} of {}",
                dims_v(self.size)
            )));
        }
        let (n, z) = (n as usize, z as usize);
        dst[..n].copy_from_slice(&self.labels[z * n..(z + 1) * n]);
        Ok(())
    }
}
pub const DEFAULT_SURFACE_NETS_ITERATIONS: i64 = 16;
const TAUBIN_PASS_BAND: f64 = 0.1;
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SurfaceNetsOptions {
    pub labels: Vec<u32>,
    pub iterations: i64,
    pub relaxation: f64,
    pub max_triangles: i64,
    pub max_bytes: i64,
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LabelSurface {
    pub mesh: Mesh,
    pub pairs: Vec<[u32; 2]>,
    pub labels: Vec<u32>,
}
impl LabelSurface {
    pub fn label_mesh(&self, label: u32) -> Option<Mesh> {
        if label == 0 {
            return None;
        }
        let mut remap = vec![-1i32; self.mesh.vertices.len()];
        let mut out = Mesh::default();
        let mut vertex = |out: &mut Mesh, i: u32| -> u32 {
            let slot = &mut remap[i as usize];
            if *slot < 0 {
                *slot = out.vertices.len() as i32;
                out.vertices.push(self.mesh.vertices[i as usize]);
            }
            *slot as u32
        };
        for (pair, t) in self.pairs.iter().zip(&self.mesh.triangles) {
            if label == pair[1] {
                let tri = [
                    vertex(&mut out, t[0]),
                    vertex(&mut out, t[1]),
                    vertex(&mut out, t[2]),
                ];
                out.triangles.push(tri);
            } else if label == pair[0] {
                let tri = [
                    vertex(&mut out, t[0]),
                    vertex(&mut out, t[2]),
                    vertex(&mut out, t[1]),
                ];
                out.triangles.push(tri);
            }
        }
        if out.triangles.is_empty() {
            return None;
        }
        face_normals(&mut out, None);
        Some(out)
    }
}
pub fn extract_labels(
    field: &dyn LabelField,
    opts: &SurfaceNetsOptions,
) -> Result<LabelSurface, MeshError> {
    let dims = field.dims();
    validate_field_dimensions(dims)?;
    let frame = AffineFrame::new(field.index_to_patient())?;
    let relaxation = if opts.relaxation == 0.0 {
        0.5
    } else {
        opts.relaxation
    };
    if !(relaxation > 0.0 && relaxation <= 1.0) {
        return Err(MeshError::invalid(format!(
            "relaxation {}",
            float_v(opts.relaxation)
        )));
    }
    let iterations = if opts.iterations == 0 {
        DEFAULT_SURFACE_NETS_ITERATIONS
    } else {
        opts.iterations
    };
    let mut nets = SurfaceNets {
        nx: dims[0] as usize,
        ny: dims[1] as usize,
        nz: dims[2],
        keep: (!opts.labels.is_empty()).then(|| opts.labels.iter().copied().collect()),
        ..SurfaceNets::default()
    };
    nets.build(field, opts)?;
    nets.smooth(iterations.max(0), relaxation);
    let mut surface = LabelSurface {
        mesh: Mesh {
            vertices: nets
                .position
                .iter()
                .map(|p| frame.point(p[0], p[1], p[2]))
                .collect(),
            normals: None,
            triangles: nets.triangles,
        },
        pairs: nets.pairs,
        labels: nets.seen.into_iter().collect(),
    };
    if frame.det < 0.0 {
        for t in surface.mesh.triangles.iter_mut() {
            t.swap(1, 2);
        }
    }
    Ok(surface)
}
#[derive(Default)]
struct SurfaceNets {
    nx: usize,
    ny: usize,
    nz: i64,
    keep: Option<BTreeSet<u32>>,
    position: Vec<[f64; 3]>,
    cell: Vec<[i32; 3]>,
    kind: Vec<u8>,
    quads: Vec<[u32; 4]>,
    triangles: Vec<[u32; 3]>,
    pairs: Vec<[u32; 2]>,
    seen: BTreeSet<u32>,
}
fn distinct_labels(corners: [u32; 8]) -> usize {
    (0..8)
        .filter(|i| !corners[..*i].contains(&corners[*i]))
        .count()
}
impl SurfaceNets {
    fn build(
        &mut self,
        field: &dyn LabelField,
        opts: &SurfaceNetsOptions,
    ) -> Result<(), MeshError> {
        let (nx, ny) = (self.nx, self.ny);
        let (px, py) = (nx + 2, ny + 2);
        let mut labels = [vec![0u32; px * py], vec![0u32; px * py]];
        let mut raw = vec![0u32; nx * ny];
        let mut loaded = [-2i64; 2];
        let (cx, cy) = (nx + 1, ny + 1);
        let mut ids = [vec![0u32; cx * cy], vec![0u32; cx * cy]];
        const NONE: u32 = u32::MAX;
        let budget_options = Options {
            max_triangles: opts.max_triangles,
            max_bytes: opts.max_bytes,
            ..Options::default()
        };
        for k in -1..self.nz {
            for z in [k, k + 1] {
                let slot = (z & 1) as usize;
                if loaded[slot] == z {
                    continue;
                }
                let buffer = &mut labels[slot];
                buffer.fill(0);
                if z >= 0 && z < self.nz {
                    field.read_labels(z, &mut raw)?;
                    for y in 0..ny {
                        let row = &mut buffer[(y + 1) * px + 1..(y + 1) * px + 1 + nx];
                        for (x, label) in raw[y * nx..(y + 1) * nx].iter().enumerate() {
                            let mut label = *label;
                            if self
                                .keep
                                .as_ref()
                                .is_some_and(|keep| !keep.contains(&label))
                            {
                                label = 0;
                            }
                            row[x] = label;
                            if label != 0 {
                                self.seen.insert(label);
                            }
                        }
                    }
                }
                loaded[slot] = z;
            }
            let (lo, hi) = (&labels[(k & 1) as usize], &labels[((k + 1) & 1) as usize]);
            let (plane_slot, previous_slot) = (((k + 1) & 1) as usize, (k & 1) as usize);
            for j in 0..cy {
                for i in 0..cx {
                    let o = j * px + i;
                    let corners = [
                        lo[o],
                        lo[o + 1],
                        lo[o + px],
                        lo[o + px + 1],
                        hi[o],
                        hi[o + 1],
                        hi[o + px],
                        hi[o + px + 1],
                    ];
                    let distinct = distinct_labels(corners);
                    let mut id = NONE;
                    if distinct > 1 {
                        id = self.position.len() as u32;
                        self.position
                            .push([i as f64 - 0.5, j as f64 - 0.5, k as f64 + 0.5]);
                        self.cell.push([i as i32 - 1, j as i32 - 1, k as i32]);
                        self.kind.push(distinct as u8);
                    }
                    ids[plane_slot][j * cx + i] = id;
                }
            }
            let (plane, previous) = (&ids[plane_slot], &ids[previous_slot]);
            let at = |ids: &[u32], i: i64, j: i64| ids[((j + 1) as usize) * cx + (i + 1) as usize];
            let label =
                |buffer: &[u32], x: i64, y: i64| buffer[((y + 1) as usize) * px + (x + 1) as usize];
            let (wx, wy) = (nx as i64, ny as i64);
            for y in 0..wy {
                for x in 0..wx {
                    let (a, b) = (label(lo, x, y), label(hi, x, y));
                    if a != b {
                        self.quad(
                            a,
                            b,
                            [
                                at(plane, x - 1, y - 1),
                                at(plane, x, y - 1),
                                at(plane, x, y),
                                at(plane, x - 1, y),
                            ],
                        );
                    }
                }
            }
            if k >= 0 {
                for y in 0..wy {
                    for x in -1..wx {
                        let (a, b) = (label(lo, x, y), label(lo, x + 1, y));
                        if a != b {
                            self.quad(
                                a,
                                b,
                                [
                                    at(previous, x, y - 1),
                                    at(previous, x, y),
                                    at(plane, x, y),
                                    at(plane, x, y - 1),
                                ],
                            );
                        }
                    }
                }
                for y in -1..wy {
                    for x in 0..wx {
                        let (a, b) = (label(lo, x, y), label(lo, x, y + 1));
                        if a != b {
                            self.quad(
                                a,
                                b,
                                [
                                    at(previous, x - 1, y),
                                    at(plane, x - 1, y),
                                    at(plane, x, y),
                                    at(previous, x, y),
                                ],
                            );
                        }
                    }
                }
            }
            let (vertices, triangles) = (self.position.len() as i64, self.triangles.len() as i64);
            let bytes = mesh_bytes(vertices, triangles, false)
                + 8 * self.pairs.len() as i64
                + 4 * self.seen.len() as i64;
            check_output_budget(&budget_options, vertices, triangles, bytes)?;
        }
        Ok(())
    }
    fn quad(&mut self, a: u32, b: u32, mut v: [u32; 4]) {
        let pair = [a.min(b), a.max(b)];
        if a < b {
            v.swap(1, 3);
        }
        self.quads.push(v);
        self.triangles.push([v[0], v[1], v[2]]);
        self.triangles.push([v[0], v[2], v[3]]);
        self.pairs.push(pair);
        self.pairs.push(pair);
    }
    fn smooth(&mut self, iterations: i64, relaxation: f64) {
        if iterations == 0 || self.position.is_empty() {
            return;
        }
        let mut edges: Vec<u64> = Vec::with_capacity(4 * self.quads.len());
        for q in &self.quads {
            for i in 0..4 {
                let (a, b) = (q[i], q[(i + 1) % 4]);
                edges.push(u64::from(a.min(b)) << 32 | u64::from(a.max(b)));
            }
        }
        edges.sort_unstable();
        edges.dedup();
        let count = self.position.len();
        let mut start = vec![0usize; count + 1];
        for e in &edges {
            start[(e >> 32) as usize + 1] += 1;
            start[(*e as u32) as usize + 1] += 1;
        }
        for i in 1..start.len() {
            start[i] += start[i - 1];
        }
        let mut neighbours = vec![0u32; start[count]];
        let mut fill: Vec<usize> = start[..count].to_vec();
        for e in &edges {
            let (a, b) = ((e >> 32) as usize, (*e as u32) as usize);
            if self.kind[b] >= self.kind[a] {
                neighbours[fill[a]] = b as u32;
                fill[a] += 1;
            }
            if self.kind[a] >= self.kind[b] {
                neighbours[fill[b]] = a as u32;
                fill[b] += 1;
            }
        }
        let mut next = vec![[0.0f64; 3]; count];
        for step in 0..2 * iterations {
            let factor = if step % 2 == 1 {
                -relaxation - TAUBIN_PASS_BAND * relaxation * relaxation
            } else {
                relaxation
            };
            for v in 0..count {
                let p = self.position[v];
                let list = &neighbours[start[v]..fill[v]];
                if list.is_empty() {
                    next[v] = p;
                    continue;
                }
                let mut mean = [0.0f64; 3];
                for u in list {
                    let q = self.position[*u as usize];
                    mean[0] += q[0];
                    mean[1] += q[1];
                    mean[2] += q[2];
                }
                let c = self.cell[v];
                for axis in 0..3 {
                    let moved = p[axis] + factor * (mean[axis] / list.len() as f64 - p[axis]);
                    let low = f64::from(c[axis]);
                    next[v][axis] = fmath::min(fmath::max(moved, low), low + 1.0);
                }
            }
            std::mem::swap(&mut self.position, &mut next);
        }
    }
}
