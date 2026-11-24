use super::inverse::{Matrix3, determinant3, linear_part};
use super::{Context, Error, Grid, Kind, Registration, finite};
use crate::fmath;
use crate::render::vec3::Vec3;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GridRegion {
    pub min: [i64; 3],
    pub max: [i64; 3],
}
impl GridRegion {
    pub fn size(&self) -> [i64; 3] {
        let mut size = [0i64; 3];
        for axis in 0..3 {
            size[axis] = self.max[axis].wrapping_sub(self.min[axis]).max(0);
        }
        size
    }
    pub fn empty(&self) -> bool {
        self.size().contains(&0)
    }
    pub fn contains(&self, x: i64, y: i64, z: i64) -> bool {
        let at = [x, y, z];
        (0..3).all(|axis| at[axis] >= self.min[axis] && at[axis] < self.max[axis])
    }
    pub fn intersect(&self, other: &GridRegion) -> GridRegion {
        let mut out = GridRegion::default();
        for axis in 0..3 {
            out.min[axis] = self.min[axis].max(other.min[axis]);
            out.max[axis] = out.min[axis].max(self.max[axis].min(other.max[axis]));
        }
        out
    }
}
impl Grid {
    pub fn whole_region(&self) -> GridRegion {
        GridRegion {
            min: [0; 3],
            max: self.dimensions.map(i64::from),
        }
    }
    pub fn continuous_index(&self, point: Vec3) -> Vec3 {
        let delta = point.sub(self.origin);
        Vec3::new(
            delta.dot(self.row_direction) / self.spacing.x,
            delta.dot(self.column_direction) / self.spacing.y,
            delta.dot(self.normal_direction) / self.spacing.z,
        )
    }
    pub fn node_point(&self, x: i64, y: i64, z: i64) -> Vec3 {
        self.origin
            .add(self.row_direction.scale(x as f64 * self.spacing.x))
            .add(self.column_direction.scale(y as f64 * self.spacing.y))
            .add(self.normal_direction.scale(z as f64 * self.spacing.z))
    }
    fn node_determinant(&self, a: &Matrix3, n: [i64; 3], at: [i64; 3]) -> Option<f64> {
        let directions = [
            self.row_direction,
            self.column_direction,
            self.normal_direction,
        ];
        let spacing = [self.spacing.x, self.spacing.y, self.spacing.z];
        let mut m = *a;
        for axis in 0..3 {
            if n[axis] < 2 {
                continue;
            }
            let (mut low, mut high) = (at, at);
            low[axis] = (at[axis] - 1).max(0);
            high[axis] = (at[axis] + 1).min(n[axis] - 1);
            let low_v = self.vector_at(low[0], low[1], low[2]);
            let high_v = self.vector_at(high[0], high[1], high[2]);
            let (Some(low_v), Some(high_v)) = (low_v, high_v) else {
                return None;
            };
            let derivative = high_v
                .sub(low_v)
                .scale(1.0 / (high[axis] - low[axis]) as f64);
            let d = directions[axis].scale(1.0 / spacing[axis]);
            for (i, component) in [derivative.x, derivative.y, derivative.z]
                .into_iter()
                .enumerate()
            {
                m[i][0] += component * d.x;
                m[i][1] += component * d.y;
                m[i][2] += component * d.z;
            }
        }
        Some(determinant3(&m))
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct VolumeChange {
    pub dimensions: [u32; 3],
    pub region: GridRegion,
    pub values: Option<Vec<f64>>,
    pub min: f64,
    pub max: f64,
    pub min_node: [i64; 3],
    pub folded: i64,
    pub folded_bounds: GridRegion,
    pub undefined: i64,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct JacobianOptions {
    pub region: Option<GridRegion>,
    pub summary_only: bool,
    pub workers: i64,
}
struct Summary {
    min: f64,
    max: f64,
    min_node: [i64; 3],
    folded: i64,
    undefined: i64,
    folded_bounds: GridRegion,
}
impl Summary {
    fn new() -> Summary {
        Summary {
            min: f64::INFINITY,
            max: f64::NEG_INFINITY,
            min_node: [0; 3],
            folded: 0,
            undefined: 0,
            folded_bounds: GridRegion {
                min: [i64::MAX; 3],
                max: [i64::MIN; 3],
            },
        }
    }
    fn add(&mut self, value: f64, node: [i64; 3]) {
        if value < self.min {
            self.min = value;
            self.min_node = node;
        }
        self.max = fmath::max(self.max, value);
        if !(value > 0.0) {
            self.folded += 1;
            for axis in 0..3 {
                self.folded_bounds.min[axis] = self.folded_bounds.min[axis].min(node[axis]);
                self.folded_bounds.max[axis] = self.folded_bounds.max[axis].max(node[axis] + 1);
            }
        }
    }
    fn merge(&mut self, o: &Summary) {
        if o.min < self.min {
            self.min = o.min;
            self.min_node = o.min_node;
        }
        self.max = fmath::max(self.max, o.max);
        self.folded += o.folded;
        self.undefined += o.undefined;
        for axis in 0..3 {
            self.folded_bounds.min[axis] =
                self.folded_bounds.min[axis].min(o.folded_bounds.min[axis]);
            self.folded_bounds.max[axis] =
                self.folded_bounds.max[axis].max(o.folded_bounds.max[axis]);
        }
    }
}
impl Registration {
    pub fn volume_change(&self) -> Result<VolumeChange, Error> {
        self.jacobian(&Context::background(), JacobianOptions::default())
    }
    pub fn jacobian(&self, ctx: &Context, opts: JacobianOptions) -> Result<VolumeChange, Error> {
        let Some(g) = &self.grid else {
            return Err(Error::new(Kind::InvalidObject, "no deformation grid"));
        };
        let a = linear_part(&self.pre.values);
        let base = determinant3(&a);
        if base == 0.0 || !finite(base) {
            return Err(Error::new(Kind::InvalidObject, "singular pre-matrix"));
        }
        let mut region = g.whole_region();
        if let Some(r) = &opts.region {
            region = r.intersect(&region);
        }
        let size = region.size();
        let mut out = VolumeChange {
            dimensions: [size[0] as u32, size[1] as u32, size[2] as u32],
            region,
            values: None,
            min: f64::INFINITY,
            max: f64::NEG_INFINITY,
            min_node: [0; 3],
            folded: 0,
            folded_bounds: GridRegion::default(),
            undefined: 0,
        };
        if region.empty() {
            ctx.err()?;
            return Ok(out);
        }
        let mut values = if opts.summary_only {
            None
        } else {
            Some(vec![0f64; (size[0] * size[1] * size[2]) as usize])
        };
        let n = g.dimensions.map(i64::from);
        let mut summary = Summary::new();
        for plane in 0..size[2] {
            ctx.err()?;
            let z = region.min[2] + plane;
            let mut partial = Summary::new();
            for y in region.min[1]..region.max[1] {
                for x in region.min[0]..region.max[0] {
                    let at = [x, y, z];
                    let determinant = g.node_determinant(&a, n, at);
                    let value = match determinant {
                        Some(v) => v / base,
                        None => f64::NAN,
                    };
                    if let Some(values) = values.as_mut() {
                        let index = ((z - region.min[2]) * size[1] + (y - region.min[1])) * size[0]
                            + (x - region.min[0]);
                        values[index as usize] = value;
                    }
                    if determinant.is_none() {
                        partial.undefined += 1;
                        continue;
                    }
                    partial.add(value, at);
                }
            }
            summary.merge(&partial);
        }
        ctx.err()?;
        out.values = values;
        out.min = summary.min;
        out.max = summary.max;
        out.folded = summary.folded;
        out.undefined = summary.undefined;
        out.min_node = summary.min_node;
        out.folded_bounds = if summary.folded == 0 {
            GridRegion::default()
        } else {
            summary.folded_bounds
        };
        Ok(out)
    }
    pub fn volume_change_at(&self, point: Vec3) -> Result<f64, Error> {
        self.jacobian_determinant(point)
    }
}
