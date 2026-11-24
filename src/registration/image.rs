use super::error::{RegError, Sentinel};
use super::matrix::{Matrix3, finite_vec};
use crate::fmath;
use crate::render::Vec3;
pub(crate) fn ints_text(v: &[i64]) -> String {
    let parts: Vec<String> = v.iter().map(|n| n.to_string()).collect();
    format!("[{}]", parts.join(" "))
}
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Image {
    pub(crate) size: [usize; 3],
    pub(crate) data: Vec<f32>,
    pub(crate) origin: Vec3,
    pub(crate) spacing: Vec3,
    pub(crate) direction: Matrix3,
    pub(crate) index_to_physical: Matrix3,
    pub(crate) physical_to_index: Matrix3,
}
pub fn image_from_snapshot(
    snapshot: &crate::render::volume_store::VolumeSnapshot,
) -> Result<Image, RegError> {
    let descriptor = snapshot.descriptor()?;
    if descriptor.components != 1 {
        return Err(RegError::new(
            Sentinel::Image,
            format!("{} components", descriptor.components),
        ));
    }
    let d = descriptor.dimensions;
    let size = [i64::from(d[0]), i64::from(d[1]), i64::from(d[2])];
    let slice = d[0] as usize * d[1] as usize;
    let mut data = vec![0f32; slice * d[2] as usize];
    for z in 0..d[2] {
        let at = z as usize * slice;
        snapshot.read_modality_slice_f32(z, &mut data[at..at + slice])?;
    }
    let a = descriptor.index_to_patient_lps.0;
    let s = descriptor.spacing_mm;
    let spacing = Vec3 {
        x: s[0],
        y: s[1],
        z: s[2],
    };
    let mut direction = Matrix3::default();
    for row in 0..3 {
        direction.0[row] = [
            a[row * 4] / spacing.x,
            a[row * 4 + 1] / spacing.y,
            a[row * 4 + 2] / spacing.z,
        ];
    }
    let origin = Vec3 {
        x: a[3],
        y: a[7],
        z: a[11],
    };
    Image::new(size, data, origin, spacing, direction)
}
impl Image {
    pub fn new(
        size: [i64; 3],
        data: Vec<f32>,
        origin: Vec3,
        spacing: Vec3,
        direction: Matrix3,
    ) -> Result<Image, RegError> {
        let mut im = Image::geometry(size, origin, spacing, direction)?;
        let count = size[0].wrapping_mul(size[1]).wrapping_mul(size[2]);
        if count != data.len() as i64 || count / size[0] / size[1] != size[2] {
            return Err(RegError::new(
                Sentinel::Image,
                format!("{} samples for size {}", data.len(), ints_text(&size)),
            ));
        }
        im.data = data;
        Ok(im)
    }
    pub(crate) fn geometry(
        size: [i64; 3],
        origin: Vec3,
        spacing: Vec3,
        direction: Matrix3,
    ) -> Result<Image, RegError> {
        if size.iter().any(|&n| n < 1) {
            return Err(RegError::new(
                Sentinel::Image,
                format!("size {}", ints_text(&size)),
            ));
        }
        for s in [spacing.x, spacing.y, spacing.z] {
            if !(s > 0.0) || s.is_infinite() {
                return Err(RegError::new(
                    Sentinel::Image,
                    format!("spacing {}", spacing.value_text()),
                ));
            }
        }
        let scale = Matrix3([
            [spacing.x, 0.0, 0.0],
            [0.0, spacing.y, 0.0],
            [0.0, 0.0, spacing.z],
        ]);
        let index_to_physical = direction.mul(&scale);
        let physical_to_index = match index_to_physical.inverse() {
            Ok(m) if finite_vec(origin) => m,
            _ => {
                return Err(RegError::new(
                    Sentinel::Image,
                    "singular direction or non-finite origin",
                ));
            }
        };
        Ok(Image {
            size: size.map(|n| n as usize),
            data: Vec::new(),
            origin,
            spacing,
            direction,
            index_to_physical,
            physical_to_index,
        })
    }
    pub fn downsample(&self, factors: [i64; 3]) -> Result<Image, RegError> {
        let mut size = [0usize; 3];
        for axis in 0..3 {
            let f = factors[axis];
            if f < 1 {
                return Err(RegError::new(
                    Sentinel::Image,
                    format!("downsampling factors {}", ints_text(&factors)),
                ));
            }
            size[axis] = 1 + (self.size[axis] - 1) / f as usize;
        }
        if factors == [1, 1, 1] {
            return Ok(self.clone());
        }
        let f = factors.map(|v| v as usize);
        let mut data = vec![0f32; size[0] * size[1] * size[2]];
        for z in 0..size[2] {
            for y in 0..size[1] {
                for x in 0..size[0] {
                    let mut sum = 0.0f64;
                    let mut count = 0i64;
                    for k in z * f[2]..((z + 1) * f[2]).min(self.size[2]) {
                        for j in y * f[1]..((y + 1) * f[1]).min(self.size[1]) {
                            let row = (k * self.size[1] + j) * self.size[0];
                            for i in x * f[0]..((x + 1) * f[0]).min(self.size[0]) {
                                sum += f64::from(self.data[row + i]);
                                count += 1;
                            }
                        }
                    }
                    data[(z * size[1] + y) * size[0] + x] = (sum / count as f64) as f32;
                }
            }
        }
        let origin = self.point(Vec3::new(
            (factors[0] - 1) as f64 / 2.0,
            (factors[1] - 1) as f64 / 2.0,
            (factors[2] - 1) as f64 / 2.0,
        ));
        let spacing = Vec3::new(
            self.spacing.x * factors[0] as f64,
            self.spacing.y * factors[1] as f64,
            self.spacing.z * factors[2] as f64,
        );
        Image::new(
            size.map(|n| n as i64),
            data,
            origin,
            spacing,
            self.direction,
        )
    }
    pub fn size(&self) -> [usize; 3] {
        self.size
    }
    pub fn data(&self) -> &[f32] {
        &self.data
    }
    pub fn spacing(&self) -> Vec3 {
        self.spacing
    }
    pub fn origin(&self) -> Vec3 {
        self.origin
    }
    pub fn direction(&self) -> Matrix3 {
        self.direction
    }
    pub fn at(&self, x: usize, y: usize, z: usize) -> f32 {
        self.data[(z * self.size[1] + y) * self.size[0] + x]
    }
    pub fn point(&self, index: Vec3) -> Vec3 {
        self.origin.add(self.index_to_physical.mul_vec(index))
    }
    pub fn continuous_index(&self, point: Vec3) -> Vec3 {
        self.physical_to_index.mul_vec(point.sub(self.origin))
    }
    pub(crate) fn inside(&self, index: Vec3) -> bool {
        index.x >= -0.5
            && index.x < self.size[0] as f64 - 0.5
            && index.y >= -0.5
            && index.y < self.size[1] as f64 - 0.5
            && index.z >= -0.5
            && index.z < self.size[2] as f64 - 0.5
    }
    pub fn interpolate(&self, point: Vec3) -> Option<f64> {
        let index = self.continuous_index(point);
        if !self.inside(index) {
            return None;
        }
        Some(self.linear_at(index))
    }
    pub(crate) fn linear_at(&self, index: Vec3) -> f64 {
        self.linear_at_with_slices(index).0
    }
    pub(crate) fn linear_at_with_slices(&self, index: Vec3) -> (f64, usize, usize) {
        let (x, fx, nx) = linear_axis(index.x, self.size[0]);
        let (y, fy, ny) = linear_axis(index.y, self.size[1]);
        let (z, fz, nz) = linear_axis(index.z, self.size[2]);
        if nx == 1 && ny == 1 && nz == 1 {
            return (0.0 + f64::from(self.at(x, y, z)), z, z);
        }
        let xw = [1.0 - fx, fx];
        let yw = [1.0 - fy, fy];
        let zw = [1.0 - fz, fz];
        let (width, height) = (self.size[0], self.size[1]);
        let mut value = 0.0;
        for k in 0..nz {
            for j in 0..ny {
                let row = ((z + k) * height + y + j) * width + x;
                for i in 0..nx {
                    let weight = xw[i] * yw[j] * zw[k];
                    if weight != 0.0 {
                        value += weight * f64::from(self.data[row + i]);
                    }
                }
            }
        }
        (value, z, z + nz - 1)
    }
    pub(crate) fn clamp_index(&self, index: Vec3) -> Vec3 {
        Vec3::new(
            fmath::min(fmath::max(index.x, 0.0), (self.size[0] - 1) as f64),
            fmath::min(fmath::max(index.y, 0.0), (self.size[1] - 1) as f64),
            fmath::min(fmath::max(index.z, 0.0), (self.size[2] - 1) as f64),
        )
    }
    pub(crate) fn nearest(&self, index: Vec3) -> [usize; 3] {
        let mut at = [0usize; 3];
        for (axis, value) in [index.x, index.y, index.z].into_iter().enumerate() {
            let v = fmath::f64_to_i64((value + 0.5).floor());
            at[axis] = v.max(0).min(self.size[axis] as i64 - 1) as usize;
        }
        at
    }
}
fn linear_axis(index: f64, size: usize) -> (usize, f64, usize) {
    let index = fmath::min(fmath::max(index, 0.0), (size - 1) as f64);
    let base = index as usize;
    let fraction = index - base as f64;
    if fraction == 0.0 {
        return (base, 0.0, 1);
    }
    (base, fraction, 2)
}
