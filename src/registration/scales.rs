use super::error::{RegError, Sentinel};
use super::image::Image;
use super::mattes::EPSILON;
use super::transform::Transform;
use crate::fmath;
use crate::render::Vec3;
pub struct PhysicalShiftScales<'a> {
    pub transform: &'a dyn Transform,
    pub points: Vec<Vec3>,
    pub small_parameter_variation: f64,
}
impl PhysicalShiftScales<'_> {
    fn variation(&self) -> f64 {
        if self.small_parameter_variation > 0.0 {
            self.small_parameter_variation
        } else {
            0.01
        }
    }
    fn max_shift(&self, delta: &[f64]) -> Result<f64, RegError> {
        if self.points.is_empty() {
            return Err(RegError::bare(Sentinel::NoSamplePoints));
        }
        let mut moved = self.transform.clone_box();
        moved.update_parameters(delta, 1.0)?;
        let mut out = 0.0;
        for &point in &self.points {
            out = fmath::max(
                out,
                moved
                    .transform_point(point)
                    .sub(self.transform.transform_point(point))
                    .length(),
            );
        }
        Ok(out)
    }
    pub fn scales(&self) -> Result<Vec<f64>, RegError> {
        let n = self.transform.number_of_parameters();
        let v = self.variation();
        let mut shifts = vec![0.0; n];
        let mut smallest = f64::INFINITY;
        for i in 0..n {
            let mut delta = vec![0.0; n];
            delta[i] = v;
            let shift = self.max_shift(&delta)?;
            shifts[i] = shift;
            if shift > EPSILON {
                smallest = fmath::min(smallest, shift);
            }
        }
        Ok(shifts
            .iter()
            .map(|&shift| {
                if smallest == f64::INFINITY {
                    1.0
                } else if shift <= EPSILON {
                    smallest * smallest / (v * v)
                } else {
                    shift * shift / (v * v)
                }
            })
            .collect())
    }
    pub fn step_scale(&self, step: &[f64]) -> Result<f64, RegError> {
        let mut largest = 0.0;
        for s in step {
            largest = fmath::max(largest, s.abs());
        }
        if largest <= EPSILON {
            return Ok(0.0);
        }
        let factor = self.variation() / largest;
        let reduced: Vec<f64> = step.iter().map(|s| s * factor).collect();
        Ok(self.max_shift(&reduced)? / factor)
    }
}
pub fn corner_points(im: &Image) -> Vec<Vec3> {
    let mut out = Vec::new();
    let mut seen: Vec<[usize; 3]> = Vec::new();
    for corner in 0..8 {
        let mut index = [0usize; 3];
        for (axis, slot) in index.iter_mut().enumerate() {
            if corner >> axis & 1 == 1 {
                *slot = im.size[axis] - 1;
            }
        }
        if seen.contains(&index) {
            continue;
        }
        seen.push(index);
        out.push(im.point(Vec3::new(index[0] as f64, index[1] as f64, index[2] as f64)));
    }
    out
}
pub fn maximum_step_size(im: &Image) -> f64 {
    fmath::min(im.spacing.x, fmath::min(im.spacing.y, im.spacing.z))
}
