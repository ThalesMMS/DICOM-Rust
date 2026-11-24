use super::region_growing::RegionGrowSeed;
use super::roimask::{MaskRun, RasterMask, add_run, clamp_span, raster_mask_pixel_count};
use super::segment_stats::{slice_in_plane_spacing, slice_thickness};
use super::segmentation::Segmentation3D;
use super::{Cancellation, Error, ctx_err};
use std::collections::BTreeMap;
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Connectivity3D(pub i64);
pub const CONNECTIVITY_6: Connectivity3D = Connectivity3D(6);
pub const CONNECTIVITY_18: Connectivity3D = Connectivity3D(18);
pub const CONNECTIVITY_26: Connectivity3D = Connectivity3D(26);
pub const ERR_INVALID_CONNECTIVITY: &str = "roi: connectivity must be 6, 18 or 26";
pub const ERR_INVALID_FILL_VOLUME: &str = "roi: invalid flood fill volume";
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct RunAdjacency {
    in_plane: i64,
    through_row: i64,
    diagonal: bool,
    diagonal_ext: i64,
}
impl Connectivity3D {
    pub(crate) fn validate(self) -> Result<(), Error> {
        match self {
            CONNECTIVITY_6 | CONNECTIVITY_18 | CONNECTIVITY_26 => Ok(()),
            _ => Err(Error::wrap(
                ERR_INVALID_CONNECTIVITY,
                format_args!("got {}", self.0),
            )),
        }
    }
    pub(crate) fn adjacency(self) -> RunAdjacency {
        match self {
            CONNECTIVITY_18 => RunAdjacency {
                in_plane: 1,
                through_row: 1,
                diagonal: true,
                diagonal_ext: 0,
            },
            CONNECTIVITY_26 => RunAdjacency {
                in_plane: 1,
                through_row: 1,
                diagonal: true,
                diagonal_ext: 1,
            },
            _ => RunAdjacency::default(),
        }
    }
}
pub(crate) struct RunVolume<'a> {
    z: Vec<i64>,
    masks: Vec<&'a RasterMask>,
    row_base: Vec<usize>,
    row_off: Vec<usize>,
}
enum Sink<'a> {
    Masks(&'a mut [RasterMask]),
    Segmentations(&'a mut [Segmentation3D]),
}
impl Sink<'_> {
    fn mask(&mut self, t: usize, z: i64) -> &mut RasterMask {
        match self {
            Sink::Masks(masks) => &mut masks[t],
            Sink::Segmentations(segs) => segs[t].mask(z),
        }
    }
}
impl<'a> RunVolume<'a> {
    pub(crate) fn new(z: Vec<i64>, masks: Vec<&'a RasterMask>) -> RunVolume<'a> {
        let mut row_base = vec![0usize; masks.len()];
        let mut total_rows = 0usize;
        for (s, mask) in masks.iter().enumerate() {
            row_base[s] = total_rows;
            total_rows += mask.runs.len();
        }
        let mut row_off = vec![0usize; total_rows + 1];
        let mut n = 0usize;
        for (s, mask) in masks.iter().enumerate() {
            let base = row_base[s];
            for (y, runs) in mask.runs.iter().enumerate() {
                row_off[base + y] = n;
                n += runs.len();
            }
        }
        row_off[total_rows] = n;
        RunVolume {
            z,
            masks,
            row_base,
            row_off,
        }
    }
    fn of_segmentation(seg: &'a Segmentation3D) -> RunVolume<'a> {
        let z = seg.slices();
        let masks = z.iter().map(|slice| &seg.masks[slice]).collect();
        RunVolume::new(z, masks)
    }
    fn run_count(&self) -> usize {
        self.row_off[self.row_off.len() - 1]
    }
    pub(crate) fn row(&self, s: usize, y: i64) -> (&'a [MaskRun], usize) {
        let mask = self.masks[s];
        if y < 0 || y >= mask.runs.len() as i64 {
            return (&[], 0);
        }
        (
            &mask.runs[y as usize],
            self.row_off[self.row_base[s] + y as usize],
        )
    }
    pub(crate) fn label(&self, adj: RunAdjacency) -> (Vec<usize>, usize) {
        let mut u: Vec<usize> = (0..self.run_count()).collect();
        for s in 0..self.masks.len() {
            let prev = (s > 0 && self.z[s - 1] == self.z[s] - 1).then(|| s - 1);
            for y in 0..self.masks[s].runs.len() as i64 {
                let (cur, base) = self.row(s, y);
                if cur.is_empty() {
                    continue;
                }
                for i in 1..cur.len() {
                    if cur[i].start <= cur[i - 1].end {
                        union(&mut u, base + i - 1, base + i);
                    }
                }
                let (above, above_base) = self.row(s, y - 1);
                union_overlapping(&mut u, cur, base, above, above_base, adj.in_plane);
                let Some(prev) = prev else {
                    continue;
                };
                let (back, back_base) = self.row(prev, y);
                union_overlapping(&mut u, cur, base, back, back_base, adj.through_row);
                if adj.diagonal {
                    let (back, back_base) = self.row(prev, y - 1);
                    union_overlapping(&mut u, cur, base, back, back_base, adj.diagonal_ext);
                    let (back, back_base) = self.row(prev, y + 1);
                    union_overlapping(&mut u, cur, base, back, back_base, adj.diagonal_ext);
                }
            }
        }
        let mut count = 0usize;
        for r in 0..u.len() {
            let p = u[r];
            if p == r {
                u[r] = count;
                count += 1;
            } else {
                u[r] = u[p];
            }
        }
        (u, count)
    }
    fn emit(
        &self,
        labels: &[usize],
        target: Option<&[i64]>,
        outputs: usize,
        columns: i64,
        rows: i64,
        sink: &mut Sink<'_>,
    ) {
        let mut current_slice = vec![usize::MAX; outputs];
        let mut row_key = vec![usize::MAX; outputs];
        for (s, mask) in self.masks.iter().enumerate() {
            for y in 0..mask.runs.len() {
                let (runs, base) = self.row(s, y as i64);
                let key = self.row_base[s] + y;
                for (i, run) in runs.iter().enumerate() {
                    let mut t = labels[base + i] as i64;
                    if let Some(target) = target {
                        t = target[t as usize];
                    }
                    if t < 0 {
                        continue;
                    }
                    let t = t as usize;
                    if current_slice[t] != s {
                        sink.mask(t, self.z[s]);
                        current_slice[t] = s;
                    }
                    if y as i64 >= rows {
                        continue;
                    }
                    let (start, end) = clamp_span(run.start, run.end, columns);
                    if start >= end {
                        continue;
                    }
                    let row = &mut sink.mask(t, self.z[s]).runs[y];
                    if row_key[t] != key {
                        row_key[t] = key;
                        row.clear();
                    }
                    match row.last_mut() {
                        Some(last) if start <= last.end => last.end = last.end.max(end),
                        _ => row.push(MaskRun { start, end }),
                    }
                }
            }
        }
    }
    fn segmentations(
        &self,
        seg: &Segmentation3D,
        labels: &[usize],
        target: Option<&[i64]>,
        outputs: usize,
    ) -> Vec<Segmentation3D> {
        let mut out: Vec<Segmentation3D> = (0..outputs)
            .map(|_| Segmentation3D::new(seg.geometry.clone(), seg.columns, seg.rows))
            .collect();
        self.emit(
            labels,
            target,
            outputs,
            seg.columns,
            seg.rows,
            &mut Sink::Segmentations(&mut out),
        );
        out
    }
    fn component_measure(
        &self,
        seg: &Segmentation3D,
        labels: &[usize],
        count: usize,
        per_voxel: impl Fn(i64) -> f64,
    ) -> Vec<f64> {
        let mut out = vec![0.0f64; count];
        for (s, mask) in self.masks.iter().enumerate() {
            let weight = per_voxel(self.z[s]);
            for y in 0..mask.runs.len() as i64 {
                if y >= seg.rows {
                    break;
                }
                let (runs, base) = self.row(s, y);
                for (i, run) in runs.iter().enumerate() {
                    let (start, end) = clamp_span(run.start, run.end, seg.columns);
                    if start < end {
                        out[labels[base + i]] += (end - start) as f64 * weight;
                    }
                }
            }
        }
        out
    }
    fn select_components(
        &self,
        seg: &Segmentation3D,
        labels: &[usize],
        count: usize,
        keep: impl Fn(usize) -> bool,
    ) -> Segmentation3D {
        let target: Vec<i64> = (0..count)
            .map(|label| if keep(label) { 0 } else { -1 })
            .collect();
        let mut out = self
            .segmentations(seg, labels, Some(&target), 1)
            .pop()
            .expect("one output");
        out.prune();
        out
    }
}
fn find(u: &mut [usize], mut i: usize) -> usize {
    while u[i] != i {
        u[i] = u[u[i]];
        i = u[i];
    }
    i
}
fn union(u: &mut [usize], a: usize, b: usize) {
    let (a, b) = (find(u, a), find(u, b));
    if a < b {
        u[b] = a;
    } else if b < a {
        u[a] = b;
    }
}
fn union_overlapping(
    u: &mut [usize],
    cur: &[MaskRun],
    cur_base: usize,
    other: &[MaskRun],
    other_base: usize,
    ext: i64,
) {
    let (mut i, mut j) = (0, 0);
    while i < cur.len() && j < other.len() {
        if cur[i].start < other[j].end.wrapping_add(ext)
            && other[j].start < cur[i].end.wrapping_add(ext)
        {
            union(u, cur_base + i, other_base + j);
        }
        if cur[i].end < other[j].end {
            i += 1;
        } else {
            j += 1;
        }
    }
}
fn label_segmentation(
    seg: &Segmentation3D,
    connectivity: Connectivity3D,
) -> Result<(RunVolume<'_>, Vec<usize>, usize), Error> {
    connectivity.validate()?;
    let v = RunVolume::of_segmentation(seg);
    let (labels, count) = v.label(connectivity.adjacency());
    Ok((v, labels, count))
}
pub fn connected_components(mask: &RasterMask, eight_connected: bool) -> Vec<RasterMask> {
    let v = RunVolume::new(vec![0], vec![mask]);
    let adj = RunAdjacency {
        in_plane: i64::from(eight_connected),
        ..RunAdjacency::default()
    };
    let (labels, count) = v.label(adj);
    if count == 0 {
        return Vec::new();
    }
    let mut out: Vec<RasterMask> = (0..count)
        .map(|_| RasterMask::new(mask.columns, mask.rows))
        .collect();
    v.emit(
        &labels,
        None,
        count,
        mask.columns,
        mask.rows,
        &mut Sink::Masks(&mut out),
    );
    out
}
pub fn connected_components_3d(seg: &Segmentation3D) -> Vec<Segmentation3D> {
    connected_components_3d_with_connectivity(seg, CONNECTIVITY_6).unwrap_or_default()
}
pub fn connected_components_3d_with_connectivity(
    seg: &Segmentation3D,
    connectivity: Connectivity3D,
) -> Result<Vec<Segmentation3D>, Error> {
    let (v, labels, count) = label_segmentation(seg, connectivity)?;
    if count == 0 {
        return Ok(Vec::new());
    }
    Ok(v.segmentations(seg, &labels, None, count))
}
pub fn connected_component_3d_at(
    seg: &Segmentation3D,
    x: i64,
    y: i64,
    slice: i64,
    connectivity: Connectivity3D,
) -> Result<Option<Segmentation3D>, Error> {
    let (v, labels, count) = label_segmentation(seg, connectivity)?;
    if count == 0 {
        return Ok(None);
    }
    let s = v.z.partition_point(|&z| z < slice);
    if s == v.z.len() || v.z[s] != slice {
        return Ok(None);
    }
    let (runs, base) = v.row(s, y);
    let i = runs.partition_point(|r| r.end <= x);
    if i == runs.len() || runs[i].start > x || x < 0 || x >= seg.columns || y >= seg.rows {
        return Ok(None);
    }
    let wanted = labels[base + i];
    Ok(Some(v.select_components(seg, &labels, count, |label| {
        label == wanted
    })))
}
pub fn largest_connected_components_3d(
    seg: &Segmentation3D,
    n: i64,
    connectivity: Connectivity3D,
) -> Result<Segmentation3D, Error> {
    let (v, labels, count) = label_segmentation(seg, connectivity)?;
    let sizes = v.component_measure(seg, &labels, count, |_| 1.0);
    let mut order: Vec<usize> = (0..count).collect();
    order.sort_by(|&a, &b| {
        if sizes[a] > sizes[b] {
            std::cmp::Ordering::Less
        } else if sizes[b] > sizes[a] {
            std::cmp::Ordering::Greater
        } else {
            std::cmp::Ordering::Equal
        }
    });
    let mut keep = vec![false; count];
    for &label in order.iter().take(n.max(0).min(count as i64) as usize) {
        keep[label] = true;
    }
    Ok(v.select_components(seg, &labels, count, |label| keep[label]))
}
pub fn remove_small_connected_components_3d(
    seg: &Segmentation3D,
    min_volume_mm3: f64,
    connectivity: Connectivity3D,
) -> Result<Segmentation3D, Error> {
    let (v, labels, count) = label_segmentation(seg, connectivity)?;
    let volumes = v.component_measure(seg, &labels, count, |slice| {
        let (col_sp, row_sp) = slice_in_plane_spacing(&seg.geometry, slice);
        col_sp * row_sp * slice_thickness(&seg.geometry, slice)
    });
    Ok(v.select_components(seg, &labels, count, |label| {
        volumes[label] >= min_volume_mm3
    }))
}
pub fn flood_fill(
    columns: i64,
    rows: i64,
    seed_x: i64,
    seed_y: i64,
    inside: Option<&mut dyn FnMut(i64, i64) -> bool>,
) -> RasterMask {
    let mask = RasterMask::new(columns, rows);
    let Some(inside) = inside else {
        return mask;
    };
    if seed_x < 0 || seed_y < 0 || seed_x >= columns || seed_y >= rows || !inside(seed_x, seed_y) {
        return mask;
    }
    let mut masks = BTreeMap::new();
    masks.insert(0, mask);
    let mut inside3 = |x: i64, y: i64, _z: i64| inside(x, y);
    let mut f = SpanFiller::new(
        columns,
        rows,
        1,
        &mut inside3,
        FOUR_NEIGHBOUR_ROWS.to_vec(),
        &mut masks,
    );
    f.fill(seed_x, seed_y, 0);
    masks.remove(&0).expect("the seed mask")
}
pub fn flood_fill_3d(
    columns: i64,
    rows: i64,
    slices: i64,
    seed: RegionGrowSeed,
    connectivity: Connectivity3D,
    inside: Option<&mut dyn FnMut(i64, i64, i64) -> bool>,
) -> Result<BTreeMap<i64, RasterMask>, Error> {
    let RegionGrowSeed {
        x: seed_x,
        y: seed_y,
        slice: seed_slice,
    } = seed;
    connectivity.validate()?;
    if let Err(err) = raster_mask_pixel_count(columns, rows) {
        return Err(Error::wrap(ERR_INVALID_FILL_VOLUME, err));
    }
    if slices <= 0 {
        return Err(Error::wrap(
            ERR_INVALID_FILL_VOLUME,
            format_args!("{slices} slices"),
        ));
    }
    let mut out = BTreeMap::new();
    let Some(inside) = inside else {
        return Ok(out);
    };
    if seed_x < 0
        || seed_y < 0
        || seed_slice < 0
        || seed_x >= columns
        || seed_y >= rows
        || seed_slice >= slices
        || !inside(seed_x, seed_y, seed_slice)
    {
        return Ok(out);
    }
    let mut f = SpanFiller::new(
        columns,
        rows,
        slices,
        inside,
        fill_neighbour_rows(connectivity),
        &mut out,
    );
    f.fill(seed_x, seed_y, seed_slice);
    Ok(out)
}
#[derive(Debug, Clone, Copy)]
pub(crate) struct FillNeighbour {
    pub(crate) dz: i64,
    pub(crate) dy: i64,
    pub(crate) ext: i64,
}
pub(crate) const FOUR_NEIGHBOUR_ROWS: [FillNeighbour; 2] = [
    FillNeighbour {
        dz: 0,
        dy: -1,
        ext: 0,
    },
    FillNeighbour {
        dz: 0,
        dy: 1,
        ext: 0,
    },
];
pub(crate) fn fill_neighbour_rows(connectivity: Connectivity3D) -> Vec<FillNeighbour> {
    let adj = connectivity.adjacency();
    let n = |dz, dy, ext| FillNeighbour { dz, dy, ext };
    let mut out = vec![
        n(0, -1, adj.in_plane),
        n(0, 1, adj.in_plane),
        n(-1, 0, adj.through_row),
        n(1, 0, adj.through_row),
    ];
    if adj.diagonal {
        for dz in [-1, 1] {
            for dy in [-1, 1] {
                out.push(n(dz, dy, adj.diagonal_ext));
            }
        }
    }
    out
}
#[derive(Debug, Clone, Copy)]
struct FillCandidate {
    z: i64,
    y: i64,
    x0: i64,
    x1: i64,
}
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) enum SpanFillError {
    Cancelled(Error),
    ResourceLimit(i64),
}
const FILL_CANCEL_INTERVAL: i64 = 1024;
pub(crate) type SpanStop<'a> = &'a mut dyn FnMut(i64, i64, i64, i64) -> bool;
pub(crate) struct SpanFiller<'a> {
    pub(crate) columns: i64,
    pub(crate) rows: i64,
    pub(crate) slices: i64,
    pub(crate) inside: &'a mut dyn FnMut(i64, i64, i64) -> bool,
    pub(crate) neighbours: Vec<FillNeighbour>,
    pub(crate) masks: &'a mut BTreeMap<i64, RasterMask>,
    stack: Vec<FillCandidate>,
    pub(crate) ctx: Option<&'a dyn Cancellation>,
    pub(crate) max_voxels: i64,
    pub(crate) stop: Option<SpanStop<'a>>,
    pub(crate) filled: i64,
    pub(crate) polls: i64,
    pub(crate) stopped: bool,
    pub(crate) err: Option<SpanFillError>,
}
impl<'a> SpanFiller<'a> {
    pub(crate) fn new(
        columns: i64,
        rows: i64,
        slices: i64,
        inside: &'a mut dyn FnMut(i64, i64, i64) -> bool,
        neighbours: Vec<FillNeighbour>,
        masks: &'a mut BTreeMap<i64, RasterMask>,
    ) -> SpanFiller<'a> {
        SpanFiller {
            columns,
            rows,
            slices,
            inside,
            neighbours,
            masks,
            stack: Vec::new(),
            ctx: None,
            max_voxels: 0,
            stop: None,
            filled: 0,
            polls: 0,
            stopped: false,
            err: None,
        }
    }
    pub(crate) fn fill(&mut self, seed_x: i64, seed_y: i64, seed_z: i64) {
        if self.err.is_some() || self.stopped {
            return;
        }
        self.stack.clear();
        self.stack.push(FillCandidate {
            z: seed_z,
            y: seed_y,
            x0: seed_x,
            x1: seed_x + 1,
        });
        while !self.stack.is_empty() {
            if let Some(ctx) = self.ctx {
                self.polls += 1;
                if self.polls % FILL_CANCEL_INTERVAL == 0
                    && let Some(err) = ctx_err(Some(ctx))
                {
                    self.err = Some(SpanFillError::Cancelled(err));
                    return;
                }
            }
            let c = self.stack.pop().expect("a candidate");
            let row = c.y as usize;
            let mut x = c.x0;
            while x < c.x1 {
                if let Some(mask) = self.masks.get(&c.z) {
                    let runs = &mask.runs[row];
                    let i = runs.partition_point(|r| r.end <= x);
                    if i < runs.len() && runs[i].start <= x {
                        x = runs[i].end;
                        continue;
                    }
                }
                if !(self.inside)(x, c.y, c.z) {
                    x += 1;
                    continue;
                }
                let (mut left, mut right) = (x, x + 1);
                while left > 0 && (self.inside)(left - 1, c.y, c.z) {
                    left -= 1;
                }
                while right < self.columns && (self.inside)(right, c.y, c.z) {
                    right += 1;
                }
                let (columns, rows) = (self.columns, self.rows);
                let mask = self
                    .masks
                    .entry(c.z)
                    .or_insert_with(|| RasterMask::new(columns, rows));
                insert_run(&mut mask.runs[row], left, right);
                self.filled += right - left;
                if self.max_voxels > 0 && self.filled > self.max_voxels {
                    self.err = Some(SpanFillError::ResourceLimit(self.max_voxels));
                    return;
                }
                if let Some(stop) = self.stop.as_mut()
                    && stop(c.z, c.y, left, right)
                {
                    self.stopped = true;
                    return;
                }
                for n in &self.neighbours {
                    let (z, y) = (c.z + n.dz, c.y + n.dy);
                    if z < 0 || z >= self.slices || y < 0 || y >= self.rows {
                        continue;
                    }
                    self.stack.push(FillCandidate {
                        z,
                        y,
                        x0: (left - n.ext).max(0),
                        x1: (right + n.ext).min(self.columns),
                    });
                }
                x = right + 1;
            }
        }
    }
}
fn insert_run(runs: &mut Vec<MaskRun>, x0: i64, x1: i64) {
    let i = runs.partition_point(|r| r.end < x0);
    if i < runs.len() && runs[i].start <= x1 {
        *runs = add_run(runs, x0, x1);
        return;
    }
    runs.insert(i, MaskRun { start: x0, end: x1 });
}
