#![forbid(unsafe_code)]
use super::affine::GeometryAffine;
use super::error::{ErrorKind, RenderError};
use super::view_state::{Interpolation, Projection, SLAB_NONE, ViewKind, ViewState};
use super::volume_store::VolumeDescriptor;
use crate::fmath;
pub const DEFAULT_VOLUME_BRICK_EXTENT: u32 = 128;
pub const ERR_INVALID_VOLUME_BRICKS: &str = "render: invalid volume bricks";
pub const ERR_VOLUME_BRICK_BUDGET: &str =
    "render: required volume bricks exceed the brick capacity";
pub(crate) fn invalid_bricks(detail: impl std::fmt::Display) -> RenderError {
    RenderError::new(
        ErrorKind::InvalidVolumeBricks,
        format!("{ERR_INVALID_VOLUME_BRICKS}: {detail}"),
    )
}
fn budget_err(detail: impl std::fmt::Display) -> RenderError {
    RenderError::new(
        ErrorKind::VolumeBrickBudget,
        format!("{ERR_VOLUME_BRICK_BUDGET}: {detail}"),
    )
}
pub(crate) fn dims_v(d: [u32; 3]) -> String {
    format!("[{} {} {}]", d[0], d[1], d[2])
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct VolumeBrickGrid {
    pub dimensions: [u32; 3],
    pub brick_size: [u32; 3],
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct VolumeBrick {
    pub index: i64,
    pub origin: [u32; 3],
    pub size: [u32; 3],
}
impl VolumeBrick {
    pub fn voxels(&self) -> u64 {
        u64::from(self.size[0]) * u64::from(self.size[1]) * u64::from(self.size[2])
    }
}
fn brick_ceil_div(value: u32, divisor: u32) -> u32 {
    ((u64::from(value) + u64::from(divisor) - 1) / u64::from(divisor)) as u32
}
fn brick_ceil_div_int(value: i64, divisor: i64) -> i64 {
    (value + divisor - 1) / divisor
}
pub fn plan_volume_brick_grid(
    dimensions: [u32; 3],
    max_extent: u32,
) -> (VolumeBrickGrid, Result<(), RenderError>) {
    if max_extent == 0 {
        return (
            VolumeBrickGrid::default(),
            Err(invalid_bricks("zero brick extent")),
        );
    }
    let mut grid = VolumeBrickGrid {
        dimensions,
        brick_size: [0; 3],
    };
    for (axis, &dimension) in dimensions.iter().enumerate() {
        if dimension == 0 {
            return (
                VolumeBrickGrid::default(),
                Err(invalid_bricks(format!("empty axis {axis}"))),
            );
        }
        let count = brick_ceil_div(dimension, max_extent);
        grid.brick_size[axis] = brick_ceil_div(dimension, count);
    }
    (grid, grid.validate())
}
impl VolumeBrickGrid {
    pub fn validate(&self) -> Result<(), RenderError> {
        let mut bricks: u64 = 1;
        for axis in 0..3 {
            let (d, b) = (self.dimensions[axis], self.brick_size[axis]);
            if d == 0 || b == 0 || b > d {
                return Err(invalid_bricks(format!(
                    "axis {axis} brick {b} of {d} voxels"
                )));
            }
            bricks = bricks.wrapping_mul(u64::from(brick_ceil_div(d, b)));
        }
        if bricks > i32::MAX as u64 {
            return Err(invalid_bricks(format!("{bricks} bricks")));
        }
        Ok(())
    }
    pub fn counts(&self) -> [u32; 3] {
        let mut counts = [0u32; 3];
        for axis in 0..3 {
            if self.brick_size[axis] != 0 {
                counts[axis] = brick_ceil_div(self.dimensions[axis], self.brick_size[axis]);
            }
        }
        counts
    }
    pub fn len(&self) -> i64 {
        let c = self.counts();
        (c[0] as i64)
            .wrapping_mul(c[1] as i64)
            .wrapping_mul(c[2] as i64)
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    pub fn brick(&self, index: i64) -> Result<VolumeBrick, RenderError> {
        self.validate()?;
        if index < 0 || index >= self.len() {
            return Err(invalid_bricks(format!("brick {index} of {}", self.len())));
        }
        let c = self.counts();
        let (c0, c1) = (c[0] as i64, c[1] as i64);
        let coordinate = [
            (index % c0) as u32,
            (index / c0 % c1) as u32,
            (index / (c0 * c1)) as u32,
        ];
        let mut brick = VolumeBrick {
            index,
            ..VolumeBrick::default()
        };
        for axis in 0..3 {
            brick.origin[axis] = coordinate[axis].wrapping_mul(self.brick_size[axis]);
            brick.size[axis] =
                self.brick_size[axis].min(self.dimensions[axis].wrapping_sub(brick.origin[axis]));
        }
        Ok(brick)
    }
    pub fn bricks_in_box(&self, lo: [f64; 3], hi: [f64; 3]) -> Option<Vec<i64>> {
        self.validate().ok()?;
        let counts = self.counts();
        let mut first = [0i64; 3];
        let mut last = [0i64; 3];
        for axis in 0..3 {
            let top = f64::from(self.dimensions[axis] - 1);
            #[allow(clippy::neg_cmp_op_on_partial_ord)]
            if !(lo[axis] <= hi[axis]) || hi[axis] < 0.0 || lo[axis] > top {
                return Some(Vec::new());
            }
            let low = fmath::max(0.0, lo[axis].floor());
            let high = fmath::min(top, hi[axis].floor());
            let b = i64::from(self.brick_size[axis]);
            first[axis] = fmath::f64_to_i64(low) / b;
            last[axis] = (fmath::f64_to_i64(high) / b).min(i64::from(counts[axis]) - 1);
        }
        let mut bricks = Vec::new();
        let (c0, c1) = (i64::from(counts[0]), i64::from(counts[1]));
        for z in first[2]..=last[2] {
            for y in first[1]..=last[1] {
                for x in first[0]..=last[0] {
                    bricks.push(x + c0 * (y + c1 * z));
                }
            }
        }
        Some(bricks)
    }
}
fn volume_brick_stencil_margin(reach: f64) -> f64 {
    reach + 1.0
}
pub fn required_volume_bricks(
    grid: &VolumeBrickGrid,
    descriptor: &VolumeDescriptor,
    view: &ViewState,
) -> Result<Vec<i64>, RenderError> {
    grid.validate()?;
    if grid.dimensions != descriptor.dimensions {
        return Err(invalid_bricks(format!(
            "grid of {} voxels for a {} volume",
            dims_v(grid.dimensions),
            dims_v(descriptor.dimensions)
        )));
    }
    let bricks = match view.kind {
        ViewKind::MPR => required_mpr_bricks(grid, descriptor, view),
        ViewKind::VR => required_vr_bricks(grid, descriptor, view),
        _ => {
            return Err(RenderError::new(
                ErrorKind::InvalidViewState,
                format!("render: invalid view state: view kind {}", view.kind.0),
            ));
        }
    };
    Ok(bricks.unwrap_or_else(|| (0..grid.len()).collect()))
}
fn finite(v: f64) -> bool {
    v.is_finite()
}
fn required_mpr_bricks(
    grid: &VolumeBrickGrid,
    descriptor: &VolumeDescriptor,
    view: &ViewState,
) -> Option<Vec<i64>> {
    let mpr = &view.mpr;
    let to_index = &descriptor.patient_lps_to_index;
    let (origin, ok_origin) = brick_affine_point(to_index, mpr.pixel_origin_lps);
    let (u, ok_u) = brick_affine_vector(to_index, mpr.pixel_step_u_lps);
    let (v, ok_v) = brick_affine_vector(to_index, mpr.pixel_step_v_lps);
    if !ok_origin || !ok_u || !ok_v || view.output_width == 0 || view.output_height == 0 {
        return None;
    }
    let u = brick_scale(u, f64::from(view.output_width - 1));
    let v = brick_scale(v, f64::from(view.output_height - 1));
    let mut a = [0.0; 3];
    let mut extent = 0.0;
    if mpr.slab_mode != SLAB_NONE {
        let (axis, ok) = brick_affine_vector(to_index, mpr.slab_axis_lps);
        let samples = (mpr.slab_thickness_mm / mpr.slab_sample_spacing_mm).floor() + 1.0;
        if !ok || !finite(samples) || samples < 1.0 {
            return None;
        }
        a = axis;
        extent = 0.5 * (samples - 1.0) * mpr.slab_sample_spacing_mm;
        if !finite(extent) {
            return None;
        }
    }
    let reach = if mpr.interpolation == Interpolation::CUBIC {
        2.0
    } else {
        1.0
    };
    let margin = volume_brick_stencil_margin(reach);
    let start = brick_add(origin, brick_scale(a, -extent));
    let a = brick_scale(a, 2.0 * extent);
    if !brick_finite(start) || !brick_finite(a) {
        return None;
    }
    let corners = brick_parallelepiped(start, u, v, a);
    let (mut lo, mut hi) = brick_corner_bounds(&corners);
    for axis in 0..3 {
        lo[axis] -= margin;
        hi[axis] += margin;
    }
    let candidates = grid.bricks_in_box(lo, hi).unwrap_or_default();
    let n = brick_cross(u, v);
    let normals: [[f64; 3]; 3] = if brick_dot(a, a) == 0.0 {
        [n, brick_cross(n, u), brick_cross(n, v)]
    } else {
        [brick_cross(v, a), brick_cross(a, u), n]
    };
    let mut required = Vec::with_capacity(candidates.len());
    for index in candidates {
        let Ok(brick) = grid.brick(index) else {
            continue;
        };
        let bx = brick_dilated_corners(&brick, margin);
        let mut separated = false;
        for normal in &normals {
            if brick_dot(*normal, *normal) == 0.0 {
                continue;
            }
            let (p_lo, p_hi) = brick_projection(&corners, *normal);
            let (b_lo, b_hi) = brick_projection(&bx, *normal);
            if p_hi < b_lo || b_hi < p_lo {
                separated = true;
                break;
            }
        }
        if !separated {
            required.push(index);
        }
    }
    Some(required)
}
fn required_vr_bricks(
    grid: &VolumeBrickGrid,
    descriptor: &VolumeDescriptor,
    view: &ViewState,
) -> Option<Vec<i64>> {
    let vr = &view.vr;
    if vr.scattering.blend > 0.0 || view.output_width == 0 || view.output_height == 0 {
        return None;
    }
    let forward = brick_normalize(brick_sub(vr.focal_point_lps, vr.position_lps));
    let view_up = brick_normalize(vr.view_up_lps);
    let mut right_raw = brick_cross(forward, view_up);
    if brick_dot(right_raw, right_raw) == 0.0 {
        let alternate = if forward[2].abs() > forward[0].abs() {
            [1.0, 0.0, 0.0]
        } else {
            [0.0, 0.0, 1.0]
        };
        right_raw = brick_cross(forward, alternate);
    }
    let right = brick_normalize(right_raw);
    let up = brick_normalize(brick_cross(right, forward));
    if brick_dot(forward, forward) == 0.0
        || brick_dot(right, right) == 0.0
        || brick_dot(up, up) == 0.0
    {
        return None;
    }
    let aspect = f64::from(view.output_width) / f64::from(view.output_height);
    let mut planes: Vec<([f64; 3], f64)> = match vr.projection {
        Projection::PERSPECTIVE | Projection::ENDOSCOPY => {
            let tangent = fmath::tan(0.5 * vr.vertical_fov_radians);
            if !finite(tangent) || tangent <= 0.0 {
                return None;
            }
            let (horizontal, vertical) = (tangent * aspect, tangent);
            vec![
                (brick_sub(right, brick_scale(forward, horizontal)), 0.0),
                (
                    brick_sub(brick_scale(right, -1.0), brick_scale(forward, horizontal)),
                    0.0,
                ),
                (brick_sub(up, brick_scale(forward, vertical)), 0.0),
                (
                    brick_sub(brick_scale(up, -1.0), brick_scale(forward, vertical)),
                    0.0,
                ),
            ]
        }
        Projection::PARALLEL => {
            let half_height = vr.parallel_scale_mm;
            if !finite(half_height) || half_height <= 0.0 {
                return None;
            }
            vec![
                (right, half_height * aspect),
                (brick_scale(right, -1.0), half_height * aspect),
                (up, half_height),
                (brick_scale(up, -1.0), half_height),
            ]
        }
        _ => return None,
    };
    if finite(vr.far_mm) && vr.far_mm > 0.0 {
        planes.push((forward, vr.far_mm));
    }
    let to_patient = &descriptor.index_to_patient_lps;
    let margin = volume_brick_stencil_margin(2.0);
    let mut required = Vec::new();
    for index in 0..grid.len() {
        let Ok(brick) = grid.brick(index) else {
            continue;
        };
        let bx = brick_dilated_corners(&brick, margin);
        let mut corners = [[0.0; 3]; 8];
        for (corner, point) in bx.iter().enumerate() {
            let (patient, ok) = brick_affine_point(to_patient, *point);
            if !ok {
                return None;
            }
            corners[corner] = brick_sub(patient, vr.position_lps);
        }
        let outside = planes.iter().any(|(normal, offset)| {
            corners.iter().all(|corner| {
                let value = brick_dot(*normal, *corner);
                #[allow(clippy::neg_cmp_op_on_partial_ord)]
                let inside = value <= offset + 1e-9 * (value.abs() + offset.abs() + 1.0);
                !inside
            })
        });
        if !outside {
            required.push(index);
        }
    }
    Some(required)
}
fn brick_dilated_corners(brick: &VolumeBrick, margin: f64) -> [[f64; 3]; 8] {
    let mut lo = [0.0; 3];
    let mut hi = [0.0; 3];
    for axis in 0..3 {
        lo[axis] = f64::from(brick.origin[axis]) - margin;
        hi[axis] = f64::from(
            brick.origin[axis]
                .wrapping_add(brick.size[axis])
                .wrapping_sub(1),
        ) + margin;
    }
    let mut corners = [[0.0; 3]; 8];
    for (corner, c) in corners.iter_mut().enumerate() {
        for axis in 0..3 {
            c[axis] = if corner & (1 << axis) != 0 {
                hi[axis]
            } else {
                lo[axis]
            };
        }
    }
    corners
}
fn brick_parallelepiped(origin: [f64; 3], u: [f64; 3], v: [f64; 3], a: [f64; 3]) -> [[f64; 3]; 8] {
    let mut corners = [[0.0; 3]; 8];
    for (corner, c) in corners.iter_mut().enumerate() {
        let mut point = origin;
        if corner & 1 != 0 {
            point = brick_add(point, u);
        }
        if corner & 2 != 0 {
            point = brick_add(point, v);
        }
        if corner & 4 != 0 {
            point = brick_add(point, a);
        }
        *c = point;
    }
    corners
}
fn brick_corner_bounds(corners: &[[f64; 3]]) -> ([f64; 3], [f64; 3]) {
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for corner in corners {
        for axis in 0..3 {
            lo[axis] = fmath::min(lo[axis], corner[axis]);
            hi[axis] = fmath::max(hi[axis], corner[axis]);
        }
    }
    (lo, hi)
}
fn brick_projection(corners: &[[f64; 3]], normal: [f64; 3]) -> (f64, f64) {
    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
    for corner in corners {
        let value = brick_dot(normal, *corner);
        lo = fmath::min(lo, value);
        hi = fmath::max(hi, value);
    }
    let slack = 1e-9 * (lo.abs() + hi.abs() + 1.0);
    (lo - slack, hi + slack)
}
fn brick_affine_point(m: &GeometryAffine, p: [f64; 3]) -> ([f64; 3], bool) {
    let m = &m.0;
    let mut r = [0.0; 3];
    for (row, out) in r.iter_mut().enumerate() {
        *out = m[row * 4] * p[0] + m[row * 4 + 1] * p[1] + m[row * 4 + 2] * p[2] + m[row * 4 + 3];
    }
    (r, brick_finite(r))
}
fn brick_affine_vector(m: &GeometryAffine, p: [f64; 3]) -> ([f64; 3], bool) {
    let m = &m.0;
    let mut r = [0.0; 3];
    for (row, out) in r.iter_mut().enumerate() {
        *out = m[row * 4] * p[0] + m[row * 4 + 1] * p[1] + m[row * 4 + 2] * p[2];
    }
    (r, brick_finite(r))
}
fn brick_add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn brick_sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn brick_scale(a: [f64; 3], s: f64) -> [f64; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
}
fn brick_dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn brick_cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn brick_normalize(a: [f64; 3]) -> [f64; 3] {
    let length = brick_dot(a, a).sqrt();
    if length == 0.0 || !finite(length) {
        return [0.0; 3];
    }
    brick_scale(a, 1.0 / length)
}
fn brick_finite(a: [f64; 3]) -> bool {
    finite(a[0]) && finite(a[1]) && finite(a[2])
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct VolumeBrickAtlas {
    pub slots: [u32; 3],
}
pub fn plan_volume_brick_atlas(
    grid: &VolumeBrickGrid,
    capacity: i64,
    max_extent: u32,
) -> Result<VolumeBrickAtlas, RenderError> {
    grid.validate()?;
    let capacity = capacity.min(grid.len());
    if capacity <= 0 {
        return Err(invalid_bricks(format!("atlas capacity {capacity}")));
    }
    let mut limit = [0i64; 3];
    for axis in 0..3 {
        limit[axis] = i64::from(max_extent / grid.brick_size[axis]);
        if limit[axis] == 0 {
            return Err(invalid_bricks(format!(
                "brick of {} voxels exceeds the {max_extent}-voxel atlas extent",
                grid.brick_size[axis]
            )));
        }
    }
    let (mut best, mut best_slots, mut best_extent) =
        (VolumeBrickAtlas::default(), i64::MAX, u32::MAX);
    for x in 1..=capacity.min(limit[0]) {
        for y in 1..=brick_ceil_div_int(capacity, x).min(limit[1]) {
            let z = brick_ceil_div_int(capacity, x * y);
            if z > limit[2] {
                continue;
            }
            let candidate = VolumeBrickAtlas {
                slots: [x as u32, y as u32, z as u32],
            };
            let d = candidate.dimensions(grid);
            let extent = d[0].max(d[1]).max(d[2]);
            let slots = x * y * z;
            if slots < best_slots || slots == best_slots && extent < best_extent {
                best = candidate;
                best_slots = slots;
                best_extent = extent;
            }
        }
    }
    if best_slots == i64::MAX {
        return Err(invalid_bricks(format!(
            "{capacity} bricks exceed an atlas of [{} {} {}] slots",
            limit[0], limit[1], limit[2]
        )));
    }
    Ok(best)
}
pub fn max_volume_brick_slots(grid: &VolumeBrickGrid, max_extent: u32) -> i64 {
    if grid.validate().is_err() {
        return 0;
    }
    let mut slots: i64 = 1;
    for axis in 0..3 {
        slots = slots.wrapping_mul(i64::from(max_extent / grid.brick_size[axis]));
        if slots == 0 || slots >= grid.len() {
            return slots.min(grid.len());
        }
    }
    slots
}
impl VolumeBrickAtlas {
    pub fn capacity(&self) -> i64 {
        (self.slots[0] as i64) * (self.slots[1] as i64) * (self.slots[2] as i64)
    }
    pub fn dimensions(&self, grid: &VolumeBrickGrid) -> [u32; 3] {
        [
            self.slots[0].wrapping_mul(grid.brick_size[0]),
            self.slots[1].wrapping_mul(grid.brick_size[1]),
            self.slots[2].wrapping_mul(grid.brick_size[2]),
        ]
    }
    pub fn slot_origin(&self, grid: &VolumeBrickGrid, slot: i64) -> Option<[u32; 3]> {
        let s = slot as u32;
        let x = s.checked_rem(self.slots[0])?;
        let y = (s / self.slots[0]).checked_rem(self.slots[1])?;
        let z = s.checked_div(self.slots[0].wrapping_mul(self.slots[1]))?;
        Some([
            x.wrapping_mul(grid.brick_size[0]),
            y.wrapping_mul(grid.brick_size[1]),
            z.wrapping_mul(grid.brick_size[2]),
        ])
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct VolumeBrickLoad {
    pub brick: i64,
    pub slot: i64,
}
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct VolumeBrickPlan {
    pub evict: Vec<i64>,
    pub load: Vec<VolumeBrickLoad>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VolumeBrickPageTable {
    grid: VolumeBrickGrid,
    slot_of: Vec<i32>,
    brick_of: Vec<i32>,
    used: Vec<u64>,
    tick: u64,
}
pub fn new_volume_brick_page_table(
    grid: &VolumeBrickGrid,
    capacity: i64,
) -> Result<VolumeBrickPageTable, RenderError> {
    grid.validate()?;
    if capacity <= 0 || capacity > grid.len() {
        return Err(invalid_bricks(format!(
            "capacity {capacity} for {} bricks",
            grid.len()
        )));
    }
    Ok(VolumeBrickPageTable {
        grid: *grid,
        slot_of: vec![-1; grid.len() as usize],
        brick_of: vec![-1; capacity as usize],
        used: vec![0; capacity as usize],
        tick: 0,
    })
}
pub fn initial_volume_brick_plan(grid: &VolumeBrickGrid, capacity: i64) -> VolumeBrickPlan {
    VolumeBrickPlan {
        evict: Vec::new(),
        load: (0..capacity.min(grid.len()))
            .map(|index| VolumeBrickLoad {
                brick: index,
                slot: index,
            })
            .collect(),
    }
}
impl VolumeBrickPageTable {
    #[doc(hidden)]
    pub fn clone_table(&self) -> VolumeBrickPageTable {
        self.clone()
    }
    pub fn grid(&self) -> VolumeBrickGrid {
        self.grid
    }
    pub fn capacity(&self) -> i64 {
        self.brick_of.len() as i64
    }
    pub fn slot(&self, brick: i64) -> Option<i64> {
        if brick < 0 || brick >= self.slot_of.len() as i64 || self.slot_of[brick as usize] < 0 {
            return None;
        }
        Some(i64::from(self.slot_of[brick as usize]))
    }
    pub fn resident(&self) -> Vec<i64> {
        (0..self.slot_of.len())
            .filter(|&b| self.slot_of[b] >= 0)
            .map(|b| b as i64)
            .collect()
    }
    pub fn missing(&self, required: &[i64]) -> Vec<i64> {
        required
            .iter()
            .copied()
            .filter(|&b| self.slot(b).is_none())
            .collect()
    }
    pub fn plan(&self, required: &[i64]) -> Result<VolumeBrickPlan, RenderError> {
        let wanted = self.normalize(required)?;
        if wanted.len() > self.brick_of.len() {
            return Err(budget_err(format!(
                "{} bricks, capacity {}",
                wanted.len(),
                self.brick_of.len()
            )));
        }
        let missing = self.missing(&wanted);
        if missing.is_empty() {
            return Ok(VolumeBrickPlan::default());
        }
        let mut free = Vec::new();
        let mut victims = Vec::new();
        for (slot, &brick) in self.brick_of.iter().enumerate() {
            if brick < 0 {
                free.push(slot);
            } else if wanted.binary_search(&i64::from(brick)).is_err() {
                victims.push(slot);
            }
        }
        victims.sort_by(|&a, &b| {
            self.used[a]
                .cmp(&self.used[b])
                .then(self.brick_of[a].cmp(&self.brick_of[b]))
        });
        let mut plan = VolumeBrickPlan::default();
        let (mut free, mut victims) = (free.into_iter(), victims.into_iter());
        for brick in missing {
            let slot = match free.next() {
                Some(slot) => slot,
                None => {
                    let Some(slot) = victims.next() else {
                        break;
                    };
                    plan.evict.push(i64::from(self.brick_of[slot]));
                    slot
                }
            };
            plan.load.push(VolumeBrickLoad {
                brick,
                slot: slot as i64,
            });
        }
        Ok(plan)
    }
    pub fn apply(&mut self, plan: &VolumeBrickPlan) -> Result<(), RenderError> {
        use std::collections::HashSet;
        let mut freed = HashSet::new();
        for &brick in &plan.evict {
            match self.slot(brick) {
                Some(slot) if !freed.contains(&slot) => {
                    freed.insert(slot);
                }
                _ => {
                    return Err(invalid_bricks(format!(
                        "evicting brick {brick} that is not resident"
                    )));
                }
            }
        }
        let mut filled = HashSet::new();
        let mut loaded = HashSet::new();
        for load in &plan.load {
            if load.brick < 0
                || load.brick >= self.slot_of.len() as i64
                || load.slot < 0
                || load.slot >= self.brick_of.len() as i64
                || filled.contains(&load.slot)
                || loaded.contains(&load.brick)
                || (self.brick_of[load.slot as usize] >= 0 && !freed.contains(&load.slot))
            {
                return Err(invalid_bricks(format!(
                    "loading brick {} into slot {}",
                    load.brick, load.slot
                )));
            }
            if let Some(slot) = self.slot(load.brick)
                && !freed.contains(&slot)
            {
                return Err(invalid_bricks(format!(
                    "brick {} is already resident",
                    load.brick
                )));
            }
            filled.insert(load.slot);
            loaded.insert(load.brick);
        }
        self.tick += 1;
        for &brick in &plan.evict {
            let slot = self.slot_of[brick as usize];
            self.slot_of[brick as usize] = -1;
            self.brick_of[slot as usize] = -1;
        }
        for load in &plan.load {
            self.slot_of[load.brick as usize] = load.slot as i32;
            self.brick_of[load.slot as usize] = load.brick as i32;
            self.used[load.slot as usize] = self.tick;
        }
        Ok(())
    }
    pub fn touch(&mut self, bricks: &[i64]) {
        self.tick += 1;
        for &brick in bricks {
            if let Some(slot) = self.slot(brick) {
                self.used[slot as usize] = self.tick;
            }
        }
    }
    fn normalize(&self, bricks: &[i64]) -> Result<Vec<i64>, RenderError> {
        let mut sorted = bricks.to_vec();
        sorted.sort_unstable();
        let mut unique: Vec<i64> = Vec::with_capacity(sorted.len());
        for &brick in &sorted {
            if brick < 0 || brick >= self.slot_of.len() as i64 {
                return Err(invalid_bricks(format!(
                    "brick {brick} of {}",
                    self.slot_of.len()
                )));
            }
            if unique.last() != Some(&brick) {
                unique.push(brick);
            }
        }
        Ok(unique)
    }
}
pub fn volume_brick_sample_stencil(
    grid: &VolumeBrickGrid,
    index: [f32; 3],
    gradient: bool,
) -> Result<Vec<i64>, RenderError> {
    grid.validate()?;
    let margin: i64 = i64::from(gradient);
    let mut low = [0u32; 3];
    let mut high = [0u32; 3];
    for axis in 0..3 {
        let value = f64::from(index[axis]);
        if !value.is_finite() {
            return Err(invalid_bricks("non-finite sample coordinate"));
        }
        let dim = grid.dimensions[axis];
        let base =
            fmath::f64_to_i64(fmath::min(fmath::max(value, -2.0), f64::from(dim) + 1.0).floor());
        let upper = i64::from(dim) - 1;
        low[axis] = ((base - margin).max(0).min(upper) as u32) / grid.brick_size[axis];
        high[axis] = ((base + 1 + margin).max(0).min(upper) as u32) / grid.brick_size[axis];
    }
    let counts = grid.counts();
    let mut result = Vec::new();
    for z in low[2]..=high[2] {
        for y in low[1]..=high[1] {
            for x in low[0]..=high[0] {
                let i = z
                    .wrapping_mul(counts[1])
                    .wrapping_add(y)
                    .wrapping_mul(counts[0])
                    .wrapping_add(x);
                result.push(i64::from(i));
            }
        }
    }
    Ok(result)
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct VolumeBrickBatchBudget {
    pub max_passes: u32,
    pub max_upload_bytes: u64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct VolumeBrickBatchUsage {
    pub passes: u32,
    pub upload_bytes: u64,
}
impl VolumeBrickBatchUsage {
    pub fn charge(
        &mut self,
        budget: VolumeBrickBatchBudget,
        upload_bytes: u64,
    ) -> Result<(), RenderError> {
        if self.passes >= budget.max_passes
            || self.upload_bytes > budget.max_upload_bytes
            || upload_bytes > budget.max_upload_bytes - self.upload_bytes
        {
            return Err(budget_err("streamed frame pass/upload budget"));
        }
        self.passes += 1;
        self.upload_bytes += upload_bytes;
        Ok(())
    }
}
