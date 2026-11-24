use super::extract::{Options, budget, finite32, prefix};
use super::field::{AffineFrame, Field};
use super::{Mesh, MeshError, face_normals};
pub(crate) struct VoxelExtractor<'a> {
    field: &'a dyn Field,
    frame: AffineFrame,
    opts: Options,
    iso: f64,
    nx: i64,
    ny: i64,
    nz: i64,
    cx: i64,
    cy: i64,
    flip: bool,
    used_count: Vec<i64>,
    face_count: Vec<i64>,
    used_off: Vec<i64>,
    face_off: Vec<i64>,
    mesh: Mesh,
    z: [i64; 4],
    inside: [Vec<u8>; 4],
    vals: Vec<f32>,
    outside: Vec<u8>,
    id: [Vec<u32>; 2],
}
#[derive(Clone, Copy)]
enum Layer {
    Outside,
    Slot(usize),
}
impl<'a> VoxelExtractor<'a> {
    pub fn new(field: &'a dyn Field, frame: AffineFrame, opts: Options) -> Self {
        let d = field.dims();
        VoxelExtractor {
            field,
            frame,
            opts,
            iso: opts.iso,
            nx: d[0],
            ny: d[1],
            nz: d[2],
            cx: d[0] + 1,
            cy: d[1] + 1,
            flip: frame.det < 0.0,
            used_count: Vec::new(),
            face_count: Vec::new(),
            used_off: Vec::new(),
            face_off: Vec::new(),
            mesh: Mesh::default(),
            z: [-1; 4],
            inside: Default::default(),
            vals: Vec::new(),
            outside: Vec::new(),
            id: Default::default(),
        }
    }
    pub fn run(mut self) -> Result<Mesh, MeshError> {
        let nz = self.nz as usize;
        self.used_count = vec![0; nz + 1];
        self.face_count = vec![0; nz];
        let plane = (self.nx * self.ny) as usize;
        for slot in self.inside.iter_mut() {
            *slot = vec![0; plane];
        }
        self.vals = vec![0.0; plane];
        self.outside = vec![0; plane];
        self.count()?;
        self.used_off = self.used_count.clone();
        let vertices = prefix(&mut self.used_off);
        self.face_off = self.face_count.clone();
        let faces = prefix(&mut self.face_off);
        let normals = !self.opts.skip_normals;
        budget(&self.opts, vertices, 2 * faces, normals)?;
        self.mesh = Mesh {
            vertices: vec![[0.0; 3]; vertices as usize],
            normals: None,
            triangles: vec![[0; 3]; 2 * faces as usize],
        };
        for id in self.id.iter_mut() {
            *id = vec![0; (self.cx * self.cy) as usize];
        }
        self.z = [-1; 4];
        self.generate()?;
        if normals {
            self.mesh.normals = Some(vec![[0.0; 3]; vertices as usize]);
            face_normals(&mut self.mesh, None);
        }
        Ok(self.mesh)
    }
    fn layer(&mut self, z: i64) -> Result<Layer, MeshError> {
        if z < 0 || z >= self.nz {
            return Ok(Layer::Outside);
        }
        let slot = (z & 3) as usize;
        if self.z[slot] == z {
            return Ok(Layer::Slot(slot));
        }
        self.z[slot] = -1;
        self.field
            .read_slice(z, &mut self.vals)
            .map_err(|e| e.wrap(&format!("mesh: read slice {z}: ")))?;
        let iso = self.iso;
        for (flag, v) in self.inside[slot].iter_mut().zip(&self.vals) {
            *flag = u8::from(finite32(*v) && f64::from(*v) >= iso);
        }
        self.z[slot] = z;
        Ok(Layer::Slot(slot))
    }
    fn flags(&self, layer: Layer) -> &[u8] {
        match layer {
            Layer::Outside => &self.outside,
            Layer::Slot(slot) => &self.inside[slot],
        }
    }
    fn at(&self, inside: &[u8], x: i64, y: i64) -> u8 {
        if x < 0 || y < 0 || x >= self.nx || y >= self.ny {
            return 0;
        }
        inside[(y * self.nx + x) as usize]
    }
    fn corner_used(&self, below: &[u8], above: &[u8], i: i64, j: i64) -> bool {
        let s = self.at(below, i - 1, j - 1)
            + self.at(below, i, j - 1)
            + self.at(below, i - 1, j)
            + self.at(below, i, j)
            + self.at(above, i - 1, j - 1)
            + self.at(above, i, j - 1)
            + self.at(above, i - 1, j)
            + self.at(above, i, j);
        s > 0 && s < 8
    }
    fn faces(&self, below: &[u8], cur: &[u8], above: &[u8], z: i64) -> Vec<[[i64; 3]; 4]> {
        let mut out = Vec::new();
        let mut visit = |mut q: [[i64; 3]; 4], b: u8| {
            if b == 1 {
                q.swap(1, 3);
            }
            out.push(q);
        };
        for y in 0..self.ny {
            for x in 0..=self.nx {
                let (a, b) = (self.at(cur, x - 1, y), self.at(cur, x, y));
                if a != b {
                    visit([[x, y, 0], [x, y + 1, 0], [x, y + 1, 1], [x, y, 1]], b);
                }
            }
        }
        for y in 0..=self.ny {
            for x in 0..self.nx {
                let (a, b) = (self.at(cur, x, y - 1), self.at(cur, x, y));
                if a != b {
                    visit([[x, y, 0], [x, y, 1], [x + 1, y, 1], [x + 1, y, 0]], b);
                }
            }
        }
        let mut z_faces = |lo: &[u8], hi: &[u8], dk: i64| {
            for y in 0..self.ny {
                for x in 0..self.nx {
                    let k = (y * self.nx + x) as usize;
                    let (a, b) = (lo[k], hi[k]);
                    if a != b {
                        visit(
                            [
                                [x, y, dk],
                                [x + 1, y, dk],
                                [x + 1, y + 1, dk],
                                [x, y + 1, dk],
                            ],
                            b,
                        );
                    }
                }
            }
        };
        z_faces(below, cur, 0);
        if z == self.nz - 1 {
            z_faces(cur, above, 1);
        }
        out
    }
    fn count(&mut self) -> Result<(), MeshError> {
        for k in 0..=self.nz {
            let below = self.layer(k - 1)?;
            let cur = self.layer(k)?;
            let mut used = 0;
            for j in 0..self.cy {
                for i in 0..self.cx {
                    if self.corner_used(self.flags(below), self.flags(cur), i, j) {
                        used += 1;
                    }
                }
            }
            self.used_count[k as usize] = used;
            if k < self.nz {
                let above = self.layer(k + 1)?;
                let faces = self.faces(self.flags(below), self.flags(cur), self.flags(above), k);
                self.face_count[k as usize] = faces.len() as i64;
            }
        }
        Ok(())
    }
    fn number(&mut self, k: i64) -> Result<(), MeshError> {
        let below = self.layer(k - 1)?;
        let cur = self.layer(k)?;
        let mut id = self.used_off[k as usize];
        for j in 0..self.cy {
            for i in 0..self.cx {
                if !self.corner_used(self.flags(below), self.flags(cur), i, j) {
                    continue;
                }
                self.id[(k & 1) as usize][(j * self.cx + i) as usize] = id as u32;
                self.mesh.vertices[id as usize] =
                    self.frame
                        .point(i as f64 - 0.5, j as f64 - 0.5, k as f64 - 0.5);
                id += 1;
            }
        }
        if id - self.used_off[k as usize] != self.used_count[k as usize] {
            return Err(MeshError::other(format!(
                "mesh: corner plane {k} has {} vertices, counted {}",
                id - self.used_off[k as usize],
                self.used_count[k as usize]
            )));
        }
        Ok(())
    }
    fn generate(&mut self) -> Result<(), MeshError> {
        self.number(0)?;
        for z in 0..self.nz {
            self.number(z + 1)?;
            let below = self.layer(z - 1)?;
            let cur = self.layer(z)?;
            let above = self.layer(z + 1)?;
            let mut k = 2 * self.face_off[z as usize] as usize;
            let faces = self.faces(self.flags(below), self.flags(cur), self.flags(above), z);
            for q in faces {
                let mut v = [0u32; 4];
                for (i, c) in q.iter().enumerate() {
                    v[i] = self.id[((z + c[2]) & 1) as usize][(c[1] * self.cx + c[0]) as usize];
                }
                if self.flip {
                    v.swap(1, 3);
                }
                self.mesh.triangles[k] = [v[0], v[1], v[2]];
                self.mesh.triangles[k + 1] = [v[0], v[2], v[3]];
                k += 2;
            }
            let want = 2 * (self.face_off[z as usize] + self.face_count[z as usize]) as usize;
            if k != want {
                return Err(MeshError::other(format!(
                    "mesh: layer {z} has {} faces, counted {}",
                    k as i64 / 2 - self.face_off[z as usize],
                    self.face_count[z as usize]
                )));
            }
        }
        Ok(())
    }
}
