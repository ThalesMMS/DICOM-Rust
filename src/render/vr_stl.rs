#![forbid(unsafe_code)]
use super::error::RenderError;
use super::vec3::Vec3;
use super::volume::{Sampler, Volume, checked_voxel_count};
use crate::numtext::format_float;
use std::sync::Arc;
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct StlTriangle {
    pub a: Vec3,
    pub b: Vec3,
    pub c: Vec3,
}
impl Volume {
    pub fn iso_surface_triangles(
        self: &Arc<Self>,
        iso: f64,
    ) -> Result<Vec<StlTriangle>, RenderError> {
        let sampler = Sampler::new(self)?;
        let Some(voxel_count) = checked_voxel_count(self.cols, self.rows, self.depth) else {
            return Err(RenderError::other(
                "render: iso-surface dimensions overflow",
            ));
        };
        let (cols, rows, depth) = (self.cols, self.rows, self.depth);
        let resampled = !Arc::ptr_eq(&sampler.vol, self);
        let mut inside_voxels = vec![false; voxel_count];
        for z in 0..depth {
            for y in 0..rows {
                for x in 0..cols {
                    let mut point = Vec3::new(x as f64, y as f64, z as f64);
                    if resampled {
                        point = sampler.vol.patient_to_voxel(self.voxel_to_patient(point));
                    }
                    inside_voxels[((z * rows + y) * cols + x) as usize] = sampler
                        .trilinear_at(point)
                        .is_some_and(|value| value >= iso);
                }
            }
        }
        let inside = |x: i64, y: i64, z: i64| -> bool {
            if x < 0 || y < 0 || z < 0 || x >= cols || y >= rows || z >= depth {
                return false;
            }
            inside_voxels[((z * rows + y) * cols + x) as usize]
        };
        let mut tris = Vec::new();
        let mut add_quad = |p: [[f64; 3]; 4]| {
            let [a, b, c, d] = p.map(|v| self.voxel_to_patient(Vec3::new(v[0], v[1], v[2])));
            tris.push(StlTriangle { a, b, c });
            tris.push(StlTriangle { a, b: c, c: d });
        };
        for z in 0..depth {
            for y in 0..rows {
                for x in 0..cols {
                    if !inside(x, y, z) {
                        continue;
                    }
                    let (fx, fy, fz) = (x as f64, y as f64, z as f64);
                    let (x0, x1) = (fx - 0.5, fx + 0.5);
                    let (y0, y1) = (fy - 0.5, fy + 0.5);
                    let (z0, z1) = (fz - 0.5, fz + 0.5);
                    if !inside(x + 1, y, z) {
                        add_quad([[x1, y0, z0], [x1, y1, z0], [x1, y1, z1], [x1, y0, z1]]);
                    }
                    if !inside(x - 1, y, z) {
                        add_quad([[x0, y0, z0], [x0, y0, z1], [x0, y1, z1], [x0, y1, z0]]);
                    }
                    if !inside(x, y + 1, z) {
                        add_quad([[x0, y1, z0], [x0, y1, z1], [x1, y1, z1], [x1, y1, z0]]);
                    }
                    if !inside(x, y - 1, z) {
                        add_quad([[x0, y0, z0], [x1, y0, z0], [x1, y0, z1], [x0, y0, z1]]);
                    }
                    if !inside(x, y, z + 1) {
                        add_quad([[x0, y0, z1], [x1, y0, z1], [x1, y1, z1], [x0, y1, z1]]);
                    }
                    if !inside(x, y, z - 1) {
                        add_quad([[x0, y0, z0], [x0, y1, z0], [x1, y1, z0], [x1, y0, z0]]);
                    }
                }
            }
        }
        Ok(tris)
    }
}
fn tri_normal(a: Vec3, b: Vec3, c: Vec3) -> Vec3 {
    let n = b.sub(a).cross(c.sub(a));
    if n.length() < 1e-12 {
        return Vec3::ZERO;
    }
    let l = n.length();
    Vec3::new(n.x / l, n.y / l, n.z / l)
}
pub fn write_binary_stl(vol: Option<&Arc<Volume>>, iso: f64) -> Result<Vec<u8>, RenderError> {
    let tris = match vol {
        None => Vec::new(),
        Some(vol) => vol
            .iso_surface_triangles(iso)
            .map_err(|e| RenderError::new(e.kind(), format!("render: prepare iso-surface: {e}")))?,
    };
    if tris.is_empty() {
        return Err(RenderError::other(format!(
            "render: iso-surface at HU {} is empty",
            format_float(iso, 'f', 0)
        )));
    }
    let mut out = Vec::with_capacity(84 + 50 * tris.len());
    let mut header = [0u8; 80];
    let title = b"dcm-rust 3D iso-surface";
    header[..title.len()].copy_from_slice(title);
    out.extend_from_slice(&header);
    out.extend_from_slice(&(tris.len() as u32).to_le_bytes());
    for t in &tris {
        let n = tri_normal(t.a, t.b, t.c);
        for v in [n, t.a, t.b, t.c] {
            for component in [v.x, v.y, v.z] {
                out.extend_from_slice(&(component as f32).to_le_bytes());
            }
        }
        out.extend_from_slice(&0u16.to_le_bytes());
    }
    Ok(out)
}
