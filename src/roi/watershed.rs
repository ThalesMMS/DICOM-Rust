use super::connected_components::{CONNECTIVITY_6, Connectivity3D, RunVolume};
use super::distance3d::VolumeMask;
use super::grayscale_reconstruction::{
    ERR_INVALID_MORPHOLOGY, GrayscaleOptions, Neighbourhood, check_morphology_volume, dims_v, max32,
};
use super::resample_grid::Grid;
use super::resample_labels::LabelVolume;
use super::roimask::RasterMask;
use super::{Cancellation, Error, ctx_err};
use crate::imagefilter::Volume;
#[derive(Debug, Clone, Copy, Default)]
pub struct WatershedOptions<'a> {
    pub connectivity: Connectivity3D,
    pub mark_watershed_line: bool,
    pub mask: Option<&'a VolumeMask>,
    pub max_voxels: i64,
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WatershedResult {
    pub labels: LabelVolume,
    pub line_voxels: i64,
    pub unreached: i64,
}
pub fn watershed_from_markers(
    ctx: Option<&dyn Cancellation>,
    relief: &Volume,
    markers: &LabelVolume,
    opts: WatershedOptions<'_>,
) -> Result<WatershedResult, Error> {
    let gray_opts = GrayscaleOptions {
        connectivity: opts.connectivity,
        max_voxels: opts.max_voxels,
    }
    .normalize()?;
    check_morphology_volume(relief, gray_opts.max_voxels, "relief")?;
    let dims = relief.dims;
    if markers.grid.columns != dims[0]
        || markers.grid.rows != dims[1]
        || markers.grid.slices != dims[2]
        || markers.labels.len() != relief.data.len()
    {
        return Err(Error::wrap(
            ERR_INVALID_MORPHOLOGY,
            format_args!(
                "markers do not match the relief dimensions {}",
                dims_v(dims)
            ),
        ));
    }
    let mut blocked: Option<Vec<bool>> = None;
    if let Some(mask) = opts.mask {
        if let Err(e) = mask.validate() {
            return Err(Error {
                sentinel: ERR_INVALID_MORPHOLOGY,
                cause: e.sentinel,
                message: format!("{ERR_INVALID_MORPHOLOGY}: mask: {e}"),
            });
        }
        if mask.columns != dims[0] || mask.rows != dims[1] || mask.slices != dims[2] {
            return Err(Error::wrap(
                ERR_INVALID_MORPHOLOGY,
                format_args!("mask dimensions differ from the relief {}", dims_v(dims)),
            ));
        }
        let mut b = vec![false; relief.data.len()];
        for (i, &inside) in mask.voxels.iter().enumerate() {
            if !inside {
                if markers.labels[i] != 0 {
                    return Err(Error::wrap(
                        ERR_INVALID_MORPHOLOGY,
                        format_args!("marker voxel {i} lies outside the mask"),
                    ));
                }
                b[i] = true;
            }
        }
        blocked = Some(b);
    }
    let mut w = Watershed {
        nb: Neighbourhood::new(dims, gray_opts.connectivity),
        level: &relief.data,
        blocked,
        labels: markers.labels.clone(),
        status: Vec::new(),
        queue: FloodQueue::default(),
    };
    let mut line_voxels = 0i64;
    let mut unreached = 0i64;
    if opts.mark_watershed_line {
        w.meyer(ctx, &markers.labels)?;
        for i in 0..w.status.len() {
            if w.is_blocked(i) {
            } else if !w.status[i] {
                unreached += 1;
            } else if w.labels[i] == 0 {
                line_voxels += 1;
            }
        }
    } else {
        w.beucher(ctx)?;
        for i in 0..w.labels.len() {
            if w.labels[i] == 0 && !w.is_blocked(i) {
                unreached += 1;
            }
        }
    }
    Ok(WatershedResult {
        labels: LabelVolume {
            grid: markers.grid.clone(),
            labels: w.labels,
        },
        line_voxels,
        unreached,
    })
}
struct Watershed<'a> {
    nb: Neighbourhood,
    level: &'a [f32],
    blocked: Option<Vec<bool>>,
    labels: Vec<u16>,
    status: Vec<bool>,
    queue: FloodQueue,
}
#[inline]
fn at(i: usize, step: i64) -> usize {
    (i as i64 + step) as usize
}
impl Watershed<'_> {
    fn is_blocked(&self, i: usize) -> bool {
        self.blocked.as_ref().is_some_and(|b| b[i])
    }
    fn beucher(&mut self, ctx: Option<&dyn Cancellation>) -> Result<(), Error> {
        for i in 0..self.labels.len() {
            if self.labels[i] == 0 {
                continue;
            }
            let (x, y, z) = self.nb.coords(i);
            for k in 0..self.nb.all.len() {
                let o = self.nb.all[k];
                if !self.nb.inside(x, y, z, o) {
                    continue;
                }
                let q = at(i, o.step);
                if self.labels[q] == 0 && !self.is_blocked(q) {
                    self.queue.push(self.level[i], i);
                    break;
                }
            }
        }
        let mut steps = 0u64;
        while !self.queue.items.is_empty() {
            if steps & 0xffff == 0
                && let Some(e) = ctx_err(ctx)
            {
                return Err(e);
            }
            let (current, p) = self.queue.pop();
            let label = self.labels[p];
            let (x, y, z) = self.nb.coords(p);
            for k in 0..self.nb.all.len() {
                let o = self.nb.all[k];
                if !self.nb.inside(x, y, z, o) {
                    continue;
                }
                let q = at(p, o.step);
                if self.labels[q] != 0 || self.is_blocked(q) {
                    continue;
                }
                self.labels[q] = label;
                self.queue.push(max32(self.level[q], current), q);
            }
            steps += 1;
        }
        Ok(())
    }
    fn meyer(&mut self, ctx: Option<&dyn Cancellation>, markers: &[u16]) -> Result<(), Error> {
        self.status = vec![false; self.labels.len()];
        for i in 0..self.status.len() {
            if self.is_blocked(i) || markers[i] != 0 {
                self.status[i] = true;
            }
        }
        for (i, &l) in markers.iter().enumerate() {
            if l == 0 {
                continue;
            }
            let (x, y, z) = self.nb.coords(i);
            for k in 0..self.nb.all.len() {
                let o = self.nb.all[k];
                if !self.nb.inside(x, y, z, o) {
                    continue;
                }
                let q = at(i, o.step);
                if !self.status[q] {
                    self.status[q] = true;
                    self.queue.push(self.level[q], q);
                }
            }
        }
        let mut steps = 0u64;
        while !self.queue.items.is_empty() {
            if steps & 0xffff == 0
                && let Some(e) = ctx_err(ctx)
            {
                return Err(e);
            }
            steps += 1;
            let (current, p) = self.queue.pop();
            let (x, y, z) = self.nb.coords(p);
            let mut label = 0u16;
            let mut collision = false;
            for k in 0..self.nb.all.len() {
                let o = self.nb.all[k];
                if !self.nb.inside(x, y, z, o) {
                    continue;
                }
                let l = self.labels[at(p, o.step)];
                if l != 0 {
                    if label != 0 && l != label {
                        collision = true;
                        break;
                    }
                    label = l;
                }
            }
            if collision || label == 0 {
                continue;
            }
            self.labels[p] = label;
            for k in 0..self.nb.all.len() {
                let o = self.nb.all[k];
                if !self.nb.inside(x, y, z, o) {
                    continue;
                }
                let q = at(p, o.step);
                if !self.status[q] {
                    self.status[q] = true;
                    self.queue.push(max32(self.level[q], current), q);
                }
            }
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Copy)]
struct FloodItem {
    level: f32,
    seq: u32,
    index: i32,
}
impl FloodItem {
    fn before(self, b: FloodItem) -> bool {
        self.level < b.level || self.level == b.level && self.seq < b.seq
    }
}
#[derive(Debug, Default)]
struct FloodQueue {
    items: Vec<FloodItem>,
    seq: u32,
}
impl FloodQueue {
    fn push(&mut self, level: f32, index: usize) {
        let item = FloodItem {
            level,
            seq: self.seq,
            index: index as i32,
        };
        self.seq = self.seq.wrapping_add(1);
        self.items.push(item);
        let mut i = self.items.len() - 1;
        while i > 0 {
            let parent = (i - 1) / 2;
            if !item.before(self.items[parent]) {
                break;
            }
            self.items[i] = self.items[parent];
            i = parent;
        }
        self.items[i] = item;
    }
    fn pop(&mut self) -> (f32, usize) {
        let top = self.items[0];
        let last = self.items.pop().unwrap_or(top);
        let n = self.items.len();
        let mut i = 0usize;
        loop {
            let mut child = 2 * i + 1;
            if child >= n {
                break;
            }
            let right = child + 1;
            if right < n && self.items[right].before(self.items[child]) {
                child = right;
            }
            if !self.items[child].before(last) {
                break;
            }
            self.items[i] = self.items[child];
            i = child;
        }
        if n > 0 {
            self.items[i] = last;
        }
        (top.level, top.index as usize)
    }
}
pub const ERR_TOO_MANY_LABELS: &str =
    "roi: invalid grayscale morphology request: more than 65535 components";
pub fn label_components(
    ctx: Option<&dyn Cancellation>,
    grid: &Grid,
    mask: &VolumeMask,
    connectivity: Connectivity3D,
) -> Result<(LabelVolume, i64), Error> {
    let connectivity = if connectivity.0 == 0 {
        CONNECTIVITY_6
    } else {
        connectivity
    };
    connectivity.validate()?;
    mask.validate()?;
    if grid.columns != mask.columns || grid.rows != mask.rows || grid.slices != mask.slices {
        return Err(Error::wrap(
            ERR_INVALID_MORPHOLOGY,
            format_args!(
                "grid {}x{}x{}, mask {}x{}x{}",
                grid.columns, grid.rows, grid.slices, mask.columns, mask.rows, mask.slices
            ),
        ));
    }
    grid.validate()?;
    let (nx, ny) = (mask.columns, mask.rows);
    let w = nx as usize;
    let mut z = Vec::with_capacity(mask.slices as usize);
    let mut masks: Vec<RasterMask> = Vec::with_capacity(mask.slices as usize);
    for s in 0..mask.slices {
        if let Some(e) = ctx_err(ctx) {
            return Err(e);
        }
        z.push(s);
        let mut m = RasterMask::new(nx, ny);
        for y in 0..ny {
            let base = ((s * ny + y) * nx) as usize;
            let row = &mask.voxels[base..base + w];
            let mut start = -1i64;
            for x in 0..=nx {
                let on = x < nx && row[x as usize];
                if on && start < 0 {
                    start = x;
                } else if !on && start >= 0 {
                    m.set_run(y, start, x);
                    start = -1;
                }
            }
        }
        masks.push(m);
    }
    let runs = RunVolume::new(z, masks.iter().collect());
    let (labels, count) = runs.label(connectivity.adjacency());
    if count > u16::MAX as usize {
        return Err(Error::with_message(
            ERR_INVALID_MORPHOLOGY,
            ERR_TOO_MANY_LABELS,
        ));
    }
    let mut out = LabelVolume {
        grid: grid.clone(),
        labels: vec![0u16; mask.voxels.len()],
    };
    for s in 0..masks.len() {
        for y in 0..ny {
            let (row, base) = runs.row(s, y);
            for (i, run) in row.iter().enumerate() {
                let l = (labels[base + i] + 1) as u16;
                let line = (s as i64 * ny + y) as usize * w;
                for x in run.start..run.end {
                    out.labels[line + x as usize] = l;
                }
            }
        }
    }
    if let Some(e) = ctx_err(ctx) {
        return Err(e);
    }
    Ok((out, count as i64))
}
