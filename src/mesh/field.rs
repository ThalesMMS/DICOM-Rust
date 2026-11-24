use super::{MeshError, VolumeMeshError, dims_v};
use crate::fmath;
use crate::render::vec3::Vec3;
use crate::render::volume_store::{VolumeSnapshot, validate_modality_f32_conversion};
use crate::render::{GeometryAffine, RenderError};
use std::sync::Mutex;
pub trait Field {
    fn dims(&self) -> [i64; 3];
    fn index_to_patient(&self) -> GeometryAffine;
    fn read_slice(&self, z: i64, dst: &mut [f32]) -> Result<(), MeshError>;
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DenseField {
    pub size: [i64; 3],
    pub affine: GeometryAffine,
    pub data: Vec<f32>,
}
impl Field for DenseField {
    fn dims(&self) -> [i64; 3] {
        self.size
    }
    fn index_to_patient(&self) -> GeometryAffine {
        self.affine
    }
    fn read_slice(&self, z: i64, dst: &mut [f32]) -> Result<(), MeshError> {
        let plane = self.size[0] * self.size[1];
        if z < 0 || z >= self.size[2] || (self.data.len() as i64) < (z + 1) * plane {
            return Err(MeshError::invalid(format!(
                "slice {z} of a {} field holding {} samples",
                dims_v(self.size),
                self.data.len()
            )));
        }
        let (plane, z) = (plane as usize, z as usize);
        dst[..plane].copy_from_slice(&self.data[z * plane..(z + 1) * plane]);
        Ok(())
    }
}
pub struct SnapshotField {
    snapshot: VolumeSnapshot,
    size: [i64; 3],
    affine: GeometryAffine,
    failure: Mutex<Option<RenderError>>,
}
pub fn snapshot_field(snapshot: VolumeSnapshot) -> Result<SnapshotField, RenderError> {
    let descriptor = snapshot.descriptor()?;
    validate_modality_f32_conversion(&descriptor)?;
    let d = descriptor.dimensions;
    Ok(SnapshotField {
        snapshot,
        size: [i64::from(d[0]), i64::from(d[1]), i64::from(d[2])],
        affine: descriptor.index_to_patient_lps,
        failure: Mutex::new(None),
    })
}
impl SnapshotField {
    #[doc(hidden)]
    pub fn read_modality_slice(&self, z: i64, dst: &mut [f32]) -> Result<(), VolumeMeshError> {
        if z < 0 || z >= self.size[2] {
            return Err(VolumeMeshError::Mesh(MeshError::invalid(format!(
                "slice {z} of {}",
                self.size[2]
            ))));
        }
        self.snapshot
            .read_modality_slice_f32(z as u32, dst)
            .map_err(VolumeMeshError::Render)
    }
    pub(crate) fn take_render_error(&self) -> Option<RenderError> {
        self.failure
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
    }
}
impl Field for SnapshotField {
    fn dims(&self) -> [i64; 3] {
        self.size
    }
    fn index_to_patient(&self) -> GeometryAffine {
        self.affine
    }
    fn read_slice(&self, z: i64, dst: &mut [f32]) -> Result<(), MeshError> {
        match self.read_modality_slice(z, dst) {
            Ok(()) => Ok(()),
            Err(VolumeMeshError::Mesh(e)) => Err(e),
            Err(VolumeMeshError::Render(e)) => {
                let text = e.to_string();
                *self
                    .failure
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(e);
                Err(MeshError::other(text))
            }
        }
    }
}
pub struct FuncField<F: Fn(i64, i64, i64) -> f64> {
    pub size: [i64; 3],
    pub affine: GeometryAffine,
    pub value: F,
}
impl<F: Fn(i64, i64, i64) -> f64> Field for FuncField<F> {
    fn dims(&self) -> [i64; 3] {
        self.size
    }
    fn index_to_patient(&self) -> GeometryAffine {
        self.affine
    }
    fn read_slice(&self, z: i64, dst: &mut [f32]) -> Result<(), MeshError> {
        if z < 0 || z >= self.size[2] {
            return Err(MeshError::invalid(format!(
                "slice {z} of a {} function field",
                dims_v(self.size)
            )));
        }
        let mut i = 0;
        for y in 0..self.size[1] {
            for x in 0..self.size[0] {
                dst[i] = (self.value)(x, y, z) as f32;
                i += 1;
            }
        }
        Ok(())
    }
}
pub fn grid_affine(origin: Vec3, axes: [Vec3; 3], spacing: [f64; 3]) -> GeometryAffine {
    let (x, y, z) = (
        axes[0].scale(spacing[0]),
        axes[1].scale(spacing[1]),
        axes[2].scale(spacing[2]),
    );
    GeometryAffine([
        x.x, y.x, z.x, origin.x, x.y, y.y, z.y, origin.y, x.z, y.z, z.z, origin.z, 0.0, 0.0, 0.0,
        1.0,
    ])
}
pub const IDENTITY_AXES: [Vec3; 3] = [
    Vec3 {
        x: 1.0,
        y: 0.0,
        z: 0.0,
    },
    Vec3 {
        x: 0.0,
        y: 1.0,
        z: 0.0,
    },
    Vec3 {
        x: 0.0,
        y: 0.0,
        z: 1.0,
    },
];
#[derive(Debug, Clone, Copy)]
pub(crate) struct AffineFrame {
    pub a: [f64; 16],
    pub det: f64,
    pub gradient: [[f64; 3]; 3],
}
impl AffineFrame {
    pub fn new(a: GeometryAffine) -> Result<AffineFrame, MeshError> {
        if !a.finite() {
            return Err(MeshError::invalid("non-finite index-to-patient affine"));
        }
        let a = a.0;
        let m = [[a[0], a[1], a[2]], [a[4], a[5], a[6]], [a[8], a[9], a[10]]];
        let det = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
            - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
            + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
        let mut scale = 0.0;
        for row in &m {
            for v in row {
                scale = fmath::max(scale, v.abs());
            }
        }
        if scale == 0.0 || det.abs() <= 1e-12 * scale * scale * scale {
            return Err(MeshError::invalid("singular index-to-patient affine"));
        }
        let mut g = [[0.0; 3]; 3];
        for (i, row) in g.iter_mut().enumerate() {
            for (j, cell) in row.iter_mut().enumerate() {
                let (r0, r1) = ((i + 1) % 3, (i + 2) % 3);
                let (c0, c1) = ((j + 1) % 3, (j + 2) % 3);
                *cell = (m[r0][c0] * m[r1][c1] - m[r0][c1] * m[r1][c0]) / det;
            }
        }
        Ok(AffineFrame {
            a,
            det,
            gradient: g,
        })
    }
    pub fn point(&self, x: f64, y: f64, z: f64) -> [f32; 3] {
        let a = &self.a;
        [
            (a[0] * x + a[1] * y + a[2] * z + a[3]) as f32,
            (a[4] * x + a[5] * y + a[6] * z + a[7]) as f32,
            (a[8] * x + a[9] * y + a[10] * z + a[11]) as f32,
        ]
    }
    pub(crate) fn patient_gradient(&self, g: [f64; 3]) -> [f64; 3] {
        let m = &self.gradient;
        [
            m[0][0] * g[0] + m[0][1] * g[1] + m[0][2] * g[2],
            m[1][0] * g[0] + m[1][1] * g[1] + m[1][2] * g[2],
            m[2][0] * g[0] + m[2][1] * g[1] + m[2][2] * g[2],
        ]
    }
}
