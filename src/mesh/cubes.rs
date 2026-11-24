use super::cases::mc_table;
use super::extract::{Options, budget, finite32, prefix};
use super::field::{AffineFrame, Field};
use super::{Mesh, MeshError, face_normals, unit32};
use crate::fmath;
#[derive(Debug, Clone, Copy, Default)]
struct Rect {
    min_x: usize,
    min_y: usize,
    max_x: usize,
    max_y: usize,
}
impl Rect {
    fn empty(&self) -> bool {
        self.min_x >= self.max_x || self.min_y >= self.max_y
    }
    fn union(&self, s: &Rect) -> Rect {
        if self.empty() {
            return *s;
        }
        if s.empty() {
            return *self;
        }
        Rect {
            min_x: self.min_x.min(s.min_x),
            min_y: self.min_y.min(s.min_y),
            max_x: self.max_x.max(s.max_x),
            max_y: self.max_y.max(s.max_y),
        }
    }
}
fn inside_bounds(inside: &[u8], width: usize) -> Rect {
    let mut bounds = Rect {
        min_x: width,
        min_y: inside.len() / width,
        max_x: 0,
        max_y: 0,
    };
    for (y, row) in inside.chunks_exact(width).enumerate() {
        let Some(first) = row.iter().position(|v| *v == 1) else {
            continue;
        };
        let last = row.iter().rposition(|v| *v == 1).unwrap_or(first);
        bounds.min_x = bounds.min_x.min(first);
        bounds.min_y = bounds.min_y.min(y);
        bounds.max_x = bounds.max_x.max(last + 1);
        bounds.max_y = y + 1;
    }
    bounds
}
fn preceding_bases(mut bounds: Rect) -> Rect {
    if bounds.empty() {
        return Rect::default();
    }
    bounds.min_x = bounds.min_x.saturating_sub(1);
    bounds.min_y = bounds.min_y.saturating_sub(1);
    bounds
}
fn cell_case(in0: &[u8], in1: &[u8], i: usize, nx: usize) -> usize {
    usize::from(in0[i])
        | usize::from(in0[i + 1]) << 1
        | usize::from(in0[i + nx]) << 2
        | usize::from(in0[i + nx + 1]) << 3
        | usize::from(in1[i]) << 4
        | usize::from(in1[i + 1]) << 5
        | usize::from(in1[i + nx]) << 6
        | usize::from(in1[i + nx + 1]) << 7
}
fn difference(c: f32, m: f32, p: f32, hm: bool, hp: bool) -> f64 {
    if hm && hp {
        (f64::from(p) - f64::from(m)) / 2.0
    } else if hp {
        f64::from(p) - f64::from(c)
    } else if hm {
        f64::from(c) - f64::from(m)
    } else {
        0.0
    }
}
pub(crate) struct CubesExtractor<'a> {
    field: &'a dyn Field,
    frame: AffineFrame,
    opts: Options,
    iso: f64,
    nx: usize,
    ny: usize,
    pad: usize,
    ex: usize,
    ey: usize,
    ez: usize,
    plane: usize,
    flip: bool,
    xy_count: Vec<i64>,
    z_count: Vec<i64>,
    tri_count: Vec<i64>,
    xy_offset: Vec<i64>,
    z_offset: Vec<i64>,
    tri_off: Vec<i64>,
    mesh: Mesh,
    normals: bool,
    need: Vec<bool>,
    z: [i64; 4],
    vals: [Vec<f32>; 4],
    inside: [Vec<u8>; 4],
    bounds: [Rect; 4],
    scratch: Vec<f32>,
    x_id: [Vec<u32>; 2],
    y_id: [Vec<u32>; 2],
    z_id: Vec<u32>,
    edge_offset: [usize; 12],
}
impl<'a> CubesExtractor<'a> {
    pub fn new(field: &'a dyn Field, frame: AffineFrame, opts: Options) -> Self {
        let d = field.dims();
        let (nx, ny, nz) = (d[0] as usize, d[1] as usize, d[2] as usize);
        let pad = usize::from(!opts.open_boundary);
        let (ex, ey, ez) = (nx + 2 * pad, ny + 2 * pad, nz + 2 * pad);
        let plane = ex * ey;
        let mut edge_offset = [0usize; 12];
        for (i, edge) in mc_table().edges.iter().enumerate() {
            let (dx, dy) = (
                usize::from(edge.base & 1),
                usize::from((edge.base >> 1) & 1),
            );
            edge_offset[i] = dy * ex + dx;
        }
        CubesExtractor {
            field,
            frame,
            opts,
            iso: opts.iso,
            nx,
            ny,
            pad,
            ex,
            ey,
            ez,
            plane,
            flip: frame.det < 0.0,
            xy_count: Vec::new(),
            z_count: Vec::new(),
            tri_count: Vec::new(),
            xy_offset: Vec::new(),
            z_offset: Vec::new(),
            tri_off: Vec::new(),
            mesh: Mesh::default(),
            normals: false,
            need: Vec::new(),
            z: [-1; 4],
            vals: Default::default(),
            inside: Default::default(),
            bounds: [Rect::default(); 4],
            scratch: Vec::new(),
            x_id: Default::default(),
            y_id: Default::default(),
            z_id: Vec::new(),
            edge_offset,
        }
    }
    pub fn run(mut self) -> Result<Mesh, MeshError> {
        if self.ex < 2 || self.ey < 2 || self.ez < 2 {
            return Ok(Mesh::default());
        }
        let layers = self.ez - 1;
        self.xy_count = vec![0; self.ez];
        self.z_count = vec![0; layers];
        self.tri_count = vec![0; layers];
        for i in 0..4 {
            self.vals[i] = vec![0.0; self.plane];
            self.inside[i] = vec![0; self.plane];
        }
        if self.pad == 1 {
            self.scratch = vec![0.0; self.nx * self.ny];
        }
        self.count(layers)?;
        self.xy_offset = vec![0; self.ez];
        self.z_offset = vec![0; layers];
        let mut vertices: i64 = 0;
        for z in 0..self.ez {
            self.xy_offset[z] = vertices;
            vertices += self.xy_count[z];
            if z < layers {
                self.z_offset[z] = vertices;
                vertices += self.z_count[z];
            }
        }
        self.tri_off = self.tri_count.clone();
        let triangles = prefix(&mut self.tri_off);
        self.normals = !self.opts.skip_normals;
        budget(&self.opts, vertices, triangles, self.normals)?;
        self.mesh = Mesh {
            vertices: vec![[0.0; 3]; vertices as usize],
            normals: None,
            triangles: vec![[0; 3]; triangles as usize],
        };
        if self.normals {
            self.mesh.normals = Some(vec![[0.0; 3]; vertices as usize]);
            self.need = vec![false; vertices as usize];
        }
        for i in 0..2 {
            self.x_id[i] = vec![0; self.plane];
            self.y_id[i] = vec![0; self.plane];
        }
        self.z_id = vec![0; self.plane];
        self.z = [-1; 4];
        self.generate(layers)?;
        if self.normals && self.need.iter().any(|n| *n) {
            let need = std::mem::take(&mut self.need);
            face_normals(&mut self.mesh, Some(&need));
        }
        Ok(self.mesh)
    }
    fn plane_at(&mut self, z: usize) -> Result<usize, MeshError> {
        let slot = z & 3;
        if self.z[slot] == z as i64 {
            return Ok(slot);
        }
        self.z[slot] = -1;
        let neg_inf = f32::NEG_INFINITY;
        let (ex, plane) = (self.ex, self.plane);
        if self.pad == 1 && (z == 0 || z == self.ez - 1) {
            self.vals[slot].fill(neg_inf);
        } else if self.pad == 0 {
            self.field
                .read_slice(z as i64, &mut self.vals[slot])
                .map_err(|e| e.wrap(&format!("mesh: read slice {z}: ")))?;
        } else {
            self.field
                .read_slice(z as i64 - 1, &mut self.scratch)
                .map_err(|e| e.wrap(&format!("mesh: read slice {}: ", z - 1)))?;
            let vals = &mut self.vals[slot];
            for i in 0..ex {
                vals[i] = neg_inf;
                vals[plane - 1 - i] = neg_inf;
            }
            for y in 0..self.ny {
                let row = &mut vals[(y + 1) * ex..(y + 2) * ex];
                row[0] = neg_inf;
                row[ex - 1] = neg_inf;
                row[1..ex - 1].copy_from_slice(&self.scratch[y * self.nx..(y + 1) * self.nx]);
            }
        }
        let iso = self.iso;
        let inside = &mut self.inside[slot];
        for (i, v) in self.vals[slot].iter_mut().enumerate() {
            if !finite32(*v) {
                *v = neg_inf;
                inside[i] = 0;
            } else {
                inside[i] = u8::from(f64::from(*v) >= iso);
            }
        }
        self.bounds[slot] = inside_bounds(inside, ex);
        self.z[slot] = z as i64;
        Ok(slot)
    }
    fn cell_bounds(&self, inside: Rect) -> Rect {
        let mut bounds = preceding_bases(inside);
        bounds.max_x = bounds.max_x.min(self.ex - 1);
        bounds.max_y = bounds.max_y.min(self.ey - 1);
        bounds
    }
    fn count(&mut self, layers: usize) -> Result<(), MeshError> {
        let ex = self.ex;
        for z in 0..=layers {
            let slot = self.plane_at(z)?;
            self.xy_count[z] = self.count_xy(slot, preceding_bases(self.bounds[slot]));
            if z > 0 {
                let prev_slot = (z - 1) & 3;
                let (mut edges, mut tris) = (0i64, 0i64);
                let inside = self.bounds[prev_slot].union(&self.bounds[slot]);
                let (cur, prev) = (&self.inside[slot], &self.inside[prev_slot]);
                for y in inside.min_y..inside.max_y {
                    let row = y * ex;
                    for i in row + inside.min_x..row + inside.max_x {
                        if cur[i] != prev[i] {
                            edges += 1;
                        }
                    }
                }
                let cells = self.cell_bounds(inside);
                let table = mc_table();
                for y in cells.min_y..cells.max_y {
                    let row = y * ex;
                    for i in row + cells.min_x..row + cells.max_x {
                        tris += i64::from(table.count[cell_case(prev, cur, i, ex)]);
                    }
                }
                self.z_count[z - 1] = edges;
                self.tri_count[z - 1] = tris;
            }
        }
        Ok(())
    }
    fn count_xy(&self, slot: usize, bounds: Rect) -> i64 {
        let inside = &self.inside[slot];
        let mut n = 0;
        for y in bounds.min_y..bounds.max_y {
            for x in bounds.min_x..bounds.max_x {
                let i = y * self.ex + x;
                if x + 1 < self.ex && inside[i] != inside[i + 1] {
                    n += 1;
                }
                if y + 1 < self.ey && inside[i] != inside[i + self.ex] {
                    n += 1;
                }
            }
        }
        n
    }
    fn generate(&mut self, layers: usize) -> Result<(), MeshError> {
        self.number_xy(0)?;
        for z in 0..layers {
            self.number_xy(z + 1)?;
            self.number_z(z)?;
            self.triangles(z)?;
        }
        Ok(())
    }
    fn number_xy(&mut self, z: usize) -> Result<(), MeshError> {
        let slot = self.plane_at(z)?;
        let ex = self.ex;
        let mut id = self.xy_offset[z];
        let bounds = preceding_bases(self.bounds[slot]);
        for y in bounds.min_y..bounds.max_y {
            for x in bounds.min_x..bounds.max_x {
                let i = y * ex + x;
                if x + 1 < ex && self.inside[slot][i] != self.inside[slot][i + 1] {
                    self.x_id[z & 1][i] = id as u32;
                    let (v0, v1) = (self.vals[slot][i], self.vals[slot][i + 1]);
                    self.vertex(id, x, y, z, 0, v0, v1)?;
                    id += 1;
                }
                if y + 1 < self.ey && self.inside[slot][i] != self.inside[slot][i + ex] {
                    self.y_id[z & 1][i] = id as u32;
                    let (v0, v1) = (self.vals[slot][i], self.vals[slot][i + ex]);
                    self.vertex(id, x, y, z, 1, v0, v1)?;
                    id += 1;
                }
            }
        }
        if id - self.xy_offset[z] != self.xy_count[z] {
            return Err(MeshError::other(format!(
                "mesh: plane {z} has {} vertices, counted {}",
                id - self.xy_offset[z],
                self.xy_count[z]
            )));
        }
        Ok(())
    }
    fn number_z(&mut self, z: usize) -> Result<(), MeshError> {
        let s0 = self.plane_at(z)?;
        let s1 = self.plane_at(z + 1)?;
        let mut id = self.z_offset[z];
        let bounds = self.bounds[s0].union(&self.bounds[s1]);
        for y in bounds.min_y..bounds.max_y {
            for x in bounds.min_x..bounds.max_x {
                let i = y * self.ex + x;
                if self.inside[s0][i] != self.inside[s1][i] {
                    self.z_id[i] = id as u32;
                    let (v0, v1) = (self.vals[s0][i], self.vals[s1][i]);
                    self.vertex(id, x, y, z, 2, v0, v1)?;
                    id += 1;
                }
            }
        }
        if id - self.z_offset[z] != self.z_count[z] {
            return Err(MeshError::other(format!(
                "mesh: layer {z} has {} z-edge vertices, counted {}",
                id - self.z_offset[z],
                self.z_count[z]
            )));
        }
        Ok(())
    }
    fn triangles(&mut self, z: usize) -> Result<(), MeshError> {
        let s0 = self.plane_at(z)?;
        let s1 = self.plane_at(z + 1)?;
        let table = mc_table();
        let ex = self.ex;
        let mut k = self.tri_off[z] as usize;
        let bounds = self.cell_bounds(self.bounds[s0].union(&self.bounds[s1]));
        let (in0, in1) = (&self.inside[s0], &self.inside[s1]);
        for y in bounds.min_y..bounds.max_y {
            let row = y * ex;
            for i in row + bounds.min_x..row + bounds.max_x {
                let c = cell_case(in0, in1, i, ex);
                if c == 0 || c == 255 {
                    continue;
                }
                let id = |e: u8| -> u32 {
                    let edge = table.edges[e as usize];
                    let top = usize::from((edge.base >> 2) & 1);
                    let at = self.edge_offset[e as usize] + i;
                    match edge.axis {
                        0 => self.x_id[(z + top) & 1][at],
                        1 => self.y_id[(z + top) & 1][at],
                        _ => self.z_id[at],
                    }
                };
                for tri in table.triangles(c).as_chunks::<3>().0 {
                    let (a, mut b, mut cc) = (id(tri[0]), id(tri[1]), id(tri[2]));
                    if self.flip {
                        std::mem::swap(&mut b, &mut cc);
                    }
                    self.mesh.triangles[k] = [a, b, cc];
                    k += 1;
                }
            }
        }
        let want = (self.tri_off[z] + self.tri_count[z]) as usize;
        if k != want {
            return Err(MeshError::other(format!(
                "mesh: layer {z} has {} triangles, counted {}",
                k as i64 - self.tri_off[z],
                self.tri_count[z]
            )));
        }
        Ok(())
    }
    #[allow(clippy::too_many_arguments)]
    fn vertex(
        &mut self,
        id: i64,
        x: usize,
        y: usize,
        z: usize,
        axis: usize,
        v0: f32,
        v1: f32,
    ) -> Result<(), MeshError> {
        let id = id as usize;
        let boundary = !finite32(v0) || !finite32(v1);
        let mut t = 0.5;
        if !boundary {
            t = (self.iso - f64::from(v0)) / (f64::from(v1) - f64::from(v0));
            t = fmath::max(0.0, fmath::min(1.0, t));
        }
        let pad = self.pad as f64;
        let mut p = [x as f64 - pad, y as f64 - pad, z as f64 - pad];
        p[axis] += t;
        self.mesh.vertices[id] = self.frame.point(p[0], p[1], p[2]);
        if !self.normals {
            return Ok(());
        }
        let n = if boundary {
            let mut dir = [0.0; 3];
            dir[axis] = if !finite32(v1) { 1.0 } else { -1.0 };
            self.frame.patient_gradient(dir)
        } else {
            let g0 = self.gradient(x, y, z)?;
            let mut q = [x, y, z];
            q[axis] += 1;
            let g1 = self.gradient(q[0], q[1], q[2])?;
            let g = [
                g0[0] + t * (g1[0] - g0[0]),
                g0[1] + t * (g1[1] - g0[1]),
                g0[2] + t * (g1[2] - g0[2]),
            ];
            let pg = self.frame.patient_gradient(g);
            [-pg[0], -pg[1], -pg[2]]
        };
        let unit = unit32(n);
        if let Some(normals) = &mut self.mesh.normals {
            normals[id] = unit;
        }
        if unit == [0.0; 3] {
            self.need[id] = true;
        }
        Ok(())
    }
    fn gradient(&mut self, x: usize, y: usize, z: usize) -> Result<[f64; 3], MeshError> {
        let slot = self.plane_at(z)?;
        let ex = self.ex;
        let i = y * ex + x;
        let vals = &self.vals[slot];
        let c = vals[i];
        let mut g = [0.0; 3];
        let sample = |v: f32| (v, finite32(v));
        let (mut m, mut p) = (0f32, 0f32);
        let (mut hm, mut hp) = (false, false);
        if x > 0 {
            (m, hm) = sample(vals[i - 1]);
        }
        if x + 1 < ex {
            (p, hp) = sample(vals[i + 1]);
        }
        g[0] = difference(c, m, p, hm, hp);
        (hm, hp) = (false, false);
        if y > 0 {
            (m, hm) = sample(vals[i - ex]);
        }
        if y + 1 < self.ey {
            (p, hp) = sample(vals[i + ex]);
        }
        g[1] = difference(c, m, p, hm, hp);
        (hm, hp) = (false, false);
        if z > 0 {
            let below = self.plane_at(z - 1)?;
            (m, hm) = sample(self.vals[below][i]);
        }
        if z + 1 < self.ez {
            let above = self.plane_at(z + 1)?;
            (p, hp) = sample(self.vals[above][i]);
        }
        g[2] = difference(c, m, p, hm, hp);
        Ok(g)
    }
}
