#![allow(clippy::neg_cmp_op_on_partial_ord)]
use super::resample_grid::Grid;
use super::roimask::{RasterMask, raster_mask_pixel_count};
use super::segment_ops::interpolate_mask;
use super::{Cancellation, Error, ctx_err};
use crate::fmath;
use std::collections::{BTreeMap, HashMap};
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ContourInterpolationMethod(pub String);
pub const CONTOUR_SIGNED_DISTANCE: &str = "signed-distance";
pub const CONTOUR_MORPHOLOGICAL: &str = "morphological-geodesic";
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ContourEndPolicy(pub String);
pub const CONTOUR_REPORT_UNMATCHED: &str = "report-unmatched";
pub const CONTOUR_CLOSE_UNMATCHED: &str = "close-unmatched";
pub const ERR_CONTOUR_INTERPOLATION: &str = "roi: invalid contour interpolation";
pub const ERR_CONTOUR_LIMIT: &str = "roi: contour interpolation resource limit";
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ContourInterpolationRequest {
    pub method: ContourInterpolationMethod,
    pub label: u16,
    pub axis: i64,
    pub geometry: Grid,
    pub manual: BTreeMap<i64, RasterMask>,
    pub max_translation_mm: f64,
    pub end_policy: ContourEndPolicy,
    pub end_length_mm: f64,
    pub max_end_slices: i64,
    pub max_slices: i64,
    pub max_components: i64,
    pub max_voxels: u64,
    pub max_bytes: u64,
    pub max_work: u64,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ContourRelation {
    pub first_slice: i64,
    pub last_slice: i64,
    pub first_components: Vec<i64>,
    pub last_components: Vec<i64>,
    pub kind: &'static str,
    pub translation: [i64; 2],
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ContourInterpolationResult {
    pub label: u16,
    pub axis: i64,
    pub method: ContourInterpolationMethod,
    pub manual: BTreeMap<i64, RasterMask>,
    pub generated: BTreeMap<i64, RasterMask>,
    pub relations: Vec<ContourRelation>,
    pub unresolved: Vec<i64>,
    pub work: u64,
}
struct ContourBudget<'a> {
    ctx: Option<&'a dyn Cancellation>,
    used: u64,
    limit: u64,
}
impl ContourBudget<'_> {
    fn step(&mut self, n: u64) -> Result<(), Error> {
        if let Some(err) = ctx_err(self.ctx) {
            return Err(err);
        }
        if n > self.limit - self.used {
            return Err(Error::sentinel(ERR_CONTOUR_LIMIT));
        }
        self.used += n;
        Ok(())
    }
}
fn bad() -> Error {
    Error::sentinel(ERR_CONTOUR_INTERPOLATION)
}
fn limit() -> Error {
    Error::sentinel(ERR_CONTOUR_LIMIT)
}
impl ContourInterpolationRequest {
    fn admitted(&self) -> Result<(ContourInterpolationRequest, i64, Vec<i64>), Error> {
        let mut r = self.clone();
        if r.label == 0
            || r.axis != 2
            || (r.method.0 != CONTOUR_SIGNED_DISTANCE && r.method.0 != CONTOUR_MORPHOLOGICAL)
            || r.manual.len() < 2
            || !r.max_translation_mm.is_finite()
            || r.max_translation_mm < 0.0
        {
            return Err(bad());
        }
        if r.end_policy.0.is_empty() {
            r.end_policy = ContourEndPolicy(CONTOUR_REPORT_UNMATCHED.to_string());
        }
        if r.end_policy.0 != CONTOUR_REPORT_UNMATCHED && r.end_policy.0 != CONTOUR_CLOSE_UNMATCHED {
            return Err(bad());
        }
        if r.end_policy.0 == CONTOUR_CLOSE_UNMATCHED
            && (r.method.0 != CONTOUR_MORPHOLOGICAL
                || !(r.end_length_mm > 0.0)
                || r.end_length_mm.is_infinite()
                || r.max_end_slices < 1
                || r.max_end_slices > 4096)
        {
            return Err(bad());
        }
        if r.geometry.validate().is_err() {
            return Err(bad());
        }
        let g = &r.geometry;
        let (row, col, normal) = (
            g.row_dir.normalize(),
            g.col_dir.normalize(),
            g.slice_dir.normalize(),
        );
        if row.dot(col).abs() > 1e-6 || row.cross(col).dot(normal) < 0.999999 {
            return Err(bad());
        }
        let Ok(pixels) = raster_mask_pixel_count(g.columns, g.rows) else {
            return Err(bad());
        };
        if r.max_slices == 0 {
            r.max_slices = 512;
        }
        if r.max_components == 0 {
            r.max_components = 64;
        }
        if r.max_voxels == 0 {
            r.max_voxels = 16 << 20;
        }
        if r.max_bytes == 0 {
            r.max_bytes = 256 << 20;
        }
        if r.max_work == 0 {
            r.max_work = 128 << 20;
        }
        if r.max_slices < 2
            || r.max_slices > 4096
            || r.max_components < 1
            || r.max_components > 256
            || r.manual.len() as i64 > r.max_slices
        {
            return Err(limit());
        }
        let g = &r.geometry;
        let mut indices = Vec::with_capacity(r.manual.len());
        for (&z, m) in &r.manual {
            if z < 0 || z >= g.slices || m.columns != g.columns || m.rows != g.rows {
                return Err(bad());
            }
            indices.push(z);
        }
        let (first, last) = (indices[0], indices[indices.len() - 1]);
        let span = (last - first + 1) as u64;
        let pixels_u = pixels as u64;
        if span as i64 > r.max_slices
            || pixels_u > r.max_voxels / span
            || pixels_u > r.max_bytes / (span.wrapping_mul(32).wrapping_add(256))
        {
            return Err(limit());
        }
        if (pixels as f64 * fmath::max(g.col_spacing, g.row_spacing)).is_infinite() {
            return Err(bad());
        }
        let first_position = g.slice_offset(first);
        let last_position = g.slice_offset(last);
        if first_position.is_infinite()
            || last_position.is_infinite()
            || (last_position - first_position).is_infinite()
            || !(last_position > first_position)
        {
            return Err(bad());
        }
        let components = r.max_components as u64;
        let metadata = span
            .wrapping_mul(components)
            .wrapping_mul(256)
            .wrapping_add(components.wrapping_mul(components).wrapping_mul(16));
        let storage = pixels_u.wrapping_mul(span.wrapping_mul(32).wrapping_add(256));
        if metadata > r.max_bytes || storage > r.max_bytes - metadata {
            return Err(limit());
        }
        Ok((r, pixels, indices))
    }
}
pub fn interpolate_contours(
    ctx: Option<&dyn Cancellation>,
    request: &ContourInterpolationRequest,
) -> Result<ContourInterpolationResult, Error> {
    if let Some(err) = ctx_err(ctx) {
        return Err(err);
    }
    let (r, pixels, indices) = request.admitted()?;
    let mut b = ContourBudget {
        ctx,
        used: 0,
        limit: r.max_work,
    };
    let mut out = ContourInterpolationResult {
        label: r.label,
        axis: r.axis,
        method: r.method.clone(),
        ..ContourInterpolationResult::default()
    };
    for &z in &indices {
        b.step(pixels as u64)?;
        out.manual.insert(z, r.manual[&z].clone());
    }
    for gap in 1..indices.len() {
        let (first, last) = (indices[gap - 1], indices[gap]);
        if last - first < 2 {
            continue;
        }
        if r.method.0 == CONTOUR_SIGNED_DISTANCE {
            for z in first + 1..last {
                b.step((pixels as u64).wrapping_mul(32))?;
                let t = (r.geometry.slice_offset(z) - r.geometry.slice_offset(first))
                    / (r.geometry.slice_offset(last) - r.geometry.slice_offset(first));
                if let Some(mask) = interpolate_mask(&r.manual[&first], &r.manual[&last], t) {
                    out.generated.insert(z, mask);
                }
            }
            continue;
        }
        let (generated, relations, ok) = morphological_gap(&mut b, &r, first, last, pixels)?;
        out.relations.extend(relations);
        if !ok {
            out.unresolved.push(first);
            continue;
        }
        out.generated.extend(generated);
    }
    if let Some(err) = ctx_err(ctx) {
        return Err(err);
    }
    out.work = b.used;
    Ok(out)
}
#[derive(Default)]
struct ContourComponent {
    pixels: Vec<i64>,
    x: f64,
    y: f64,
}
fn contour_components(
    b: &mut ContourBudget<'_>,
    m: &RasterMask,
    max_components: i64,
    pixels: i64,
) -> Result<(Vec<ContourComponent>, Vec<i64>), Error> {
    let mut labels = vec![-1i64; pixels as usize];
    let mut out: Vec<ContourComponent> = Vec::new();
    for start in 0..pixels {
        if start % 1024 == 0 {
            b.step(1024)?;
        }
        if labels[start as usize] >= 0 || !m.get(start % m.columns, start / m.columns) {
            continue;
        }
        if out.len() as i64 >= max_components {
            return Err(limit());
        }
        let label = out.len() as i64;
        let mut c = ContourComponent {
            pixels: vec![start],
            ..ContourComponent::default()
        };
        labels[start as usize] = label;
        let mut head = 0;
        while head < c.pixels.len() {
            if head % 1024 == 0 {
                b.step(1024)?;
            }
            let p = c.pixels[head];
            let (x, y) = (p % m.columns, p / m.columns);
            c.x += x as f64;
            c.y += y as f64;
            for q in contour_neighbours(p, m.columns, m.rows) {
                if q >= 0 && labels[q as usize] < 0 && m.get(q % m.columns, q / m.columns) {
                    labels[q as usize] = label;
                    c.pixels.push(q);
                }
            }
            head += 1;
        }
        c.x /= c.pixels.len() as f64;
        c.y /= c.pixels.len() as f64;
        out.push(c);
    }
    Ok((out, labels))
}
fn contour_neighbours(p: i64, columns: i64, rows: i64) -> [i64; 4] {
    let mut n = [-1i64; 4];
    let (x, y) = (p % columns, p / columns);
    if x > 0 {
        n[0] = p - 1;
    }
    if x + 1 < columns {
        n[1] = p + 1;
    }
    if y > 0 {
        n[2] = p - columns;
    }
    if y + 1 < rows {
        n[3] = p + columns;
    }
    n
}
struct GapWorkspace {
    left: Vec<bool>,
    right: Vec<bool>,
    ld: Vec<f64>,
    rd: Vec<f64>,
    positions: Vec<i64>,
    nodes: Vec<i64>,
}
type GapResult = (BTreeMap<i64, RasterMask>, Vec<ContourRelation>, bool);
fn morphological_gap(
    b: &mut ContourBudget<'_>,
    r: &ContourInterpolationRequest,
    first: i64,
    last: i64,
    pixels: i64,
) -> Result<GapResult, Error> {
    let g = &r.geometry;
    let (a, al) = contour_components(b, &r.manual[&first], r.max_components, pixels)?;
    let (c, cl) = contour_components(b, &r.manual[&last], r.max_components, pixels)?;
    let mut edges = vec![vec![false; c.len()]; a.len()];
    for p in 0..pixels as usize {
        if p % 1024 == 0 {
            b.step(1024)?;
        }
        if al[p] >= 0 && cl[p] >= 0 {
            edges[al[p] as usize][cl[p] as usize] = true;
        }
    }
    let mut shifts: HashMap<(usize, usize), [i64; 2]> = HashMap::new();
    b.step(
        (a.len() as u64)
            .wrapping_mul(c.len() as u64)
            .wrapping_mul(4),
    )?;
    let mut linked_a = vec![false; a.len()];
    let mut linked_c = vec![false; c.len()];
    for i in 0..a.len() {
        for (j, &yes) in edges[i].iter().enumerate() {
            if yes {
                linked_a[i] = true;
                linked_c[j] = true;
            }
        }
    }
    let nearest = |from: &[ContourComponent],
                   to: &[ContourComponent],
                   i: usize,
                   linked: &[bool]|
     -> Option<usize> {
        let (mut best, mut distance, mut tie) = (None, f64::INFINITY, false);
        for j in 0..to.len() {
            if linked[j] {
                continue;
            }
            let d = fmath::hypot(
                (from[i].x - to[j].x) * g.col_spacing,
                (from[i].y - to[j].y) * g.row_spacing,
            );
            if d < distance - 1e-9 {
                (best, distance, tie) = (Some(j), d, false);
            } else if (d - distance).abs() <= 1e-9 {
                tie = true;
            }
        }
        if tie || distance > r.max_translation_mm {
            return None;
        }
        best
    };
    let near_a: Vec<Option<usize>> = (0..a.len())
        .map(|i| nearest(&a, &c, i, &linked_c))
        .collect();
    let near_c: Vec<Option<usize>> = (0..c.len())
        .map(|j| nearest(&c, &a, j, &linked_a))
        .collect();
    for i in 0..a.len() {
        if linked_a[i] || r.max_translation_mm == 0.0 {
            continue;
        }
        let Some(j) = near_a[i] else {
            continue;
        };
        if near_c[j] != Some(i) {
            continue;
        }
        edges[i][j] = true;
        shifts.insert(
            (i, j),
            [
                fmath::f64_to_i64((c[j].x - a[i].x).round()),
                fmath::f64_to_i64((c[j].y - a[i].y).round()),
            ],
        );
    }
    let n = pixels as usize;
    let mut w = GapWorkspace {
        left: vec![false; n],
        right: vec![false; n],
        ld: vec![0.0; n],
        rd: vec![0.0; n],
        positions: vec![0; n],
        nodes: Vec::with_capacity(n),
    };
    let mut seen_a = vec![false; a.len()];
    let mut seen_c = vec![false; c.len()];
    let mut relations: Vec<ContourRelation> = Vec::new();
    let mut complete = true;
    let mut generated: BTreeMap<i64, RasterMask> = BTreeMap::new();
    for z in first + 1..last {
        generated.insert(z, RasterMask::new(g.columns, g.rows));
    }
    let close_unmatched = r.end_policy.0 == CONTOUR_CLOSE_UNMATCHED;
    for i in 0..a.len() {
        if seen_a[i] {
            continue;
        }
        let mut aa: Vec<usize> = vec![i];
        let mut cc: Vec<usize> = Vec::new();
        seen_a[i] = true;
        let mut head = 0;
        while head < aa.len() + cc.len() {
            b.step(
                (aa.len() as u64)
                    .wrapping_mul(c.len() as u64)
                    .wrapping_add((cc.len() as u64).wrapping_mul(a.len() as u64)),
            )?;
            let mut changed = false;
            for &k in &aa {
                for (j, &yes) in edges[k].iter().enumerate() {
                    if yes && !seen_c[j] {
                        seen_c[j] = true;
                        cc.push(j);
                        changed = true;
                    }
                }
            }
            for &j in &cc {
                for k in 0..a.len() {
                    if edges[k][j] && !seen_a[k] {
                        seen_a[k] = true;
                        aa.push(k);
                        changed = true;
                    }
                }
            }
            if !changed {
                break;
            }
            head += 1;
        }
        let mut relation = ContourRelation {
            first_slice: first,
            last_slice: last,
            first_components: aa.iter().map(|&k| k as i64).collect(),
            last_components: cc.iter().map(|&j| j as i64).collect(),
            kind: "one-to-one",
            translation: [0, 0],
        };
        if cc.is_empty() {
            relation.kind = "unmatched";
            if close_unmatched {
                relation.kind = "closing";
            } else {
                complete = false;
            }
        } else if aa.len() > 1 && cc.len() > 1 {
            relation.kind = "complex";
        } else if aa.len() > 1 {
            relation.kind = "merge";
        } else if cc.len() > 1 {
            relation.kind = "branch";
        }
        if aa.len() == 1 && cc.len() == 1 {
            relation.translation = shifts.get(&(aa[0], cc[0])).copied().unwrap_or([0, 0]);
        }
        let translation = relation.translation;
        relations.push(relation);
        if cc.is_empty() {
            if close_unmatched {
                close_contour_end(b, r, &a[aa[0]], first, last, &mut generated, &mut w)?;
            }
            continue;
        }
        w.left.fill(false);
        w.right.fill(false);
        let (dx, dy) = (translation[0], translation[1]);
        for &k in &aa {
            for &p in &a[k].pixels {
                let (x, y) = (p % g.columns + dx, p / g.columns + dy);
                if x >= 0 && x < g.columns && y >= 0 && y < g.rows {
                    w.left[(y * g.columns + x) as usize] = true;
                } else {
                    complete = false;
                }
            }
        }
        for &j in &cc {
            for &p in &c[j].pixels {
                w.right[p as usize] = true;
            }
        }
        let lmax = contour_dilation_distance_into(
            b,
            &w.left,
            &w.right,
            g,
            &mut w.ld,
            &mut w.positions,
            &mut w.nodes,
        )?;
        let rmax = contour_dilation_distance_into(
            b,
            &w.right,
            &w.left,
            g,
            &mut w.rd,
            &mut w.positions,
            &mut w.nodes,
        )?;
        if lmax == f64::INFINITY || rmax == f64::INFINITY {
            complete = false;
            continue;
        }
        for z in first + 1..last {
            let t = (g.slice_offset(z) - g.slice_offset(first))
                / (g.slice_offset(last) - g.slice_offset(first));
            b.step(pixels as u64)?;
            let shift_x = fmath::f64_to_i64(((1.0 - t) * dx as f64).round());
            let shift_y = fmath::f64_to_i64(((1.0 - t) * dy as f64).round());
            let (left, right, ld, rd) = (&w.left, &w.right, &w.ld, &w.rd);
            if let Some(mask) = generated.get_mut(&z) {
                contour_emit_runs(mask, shift_x, shift_y, |p| {
                    (left[p] && right[p])
                        || (left[p] && ld[p] <= (1.0 - t) * lmax)
                        || (right[p] && rd[p] <= t * rmax)
                });
            }
        }
    }
    for j in 0..c.len() {
        if !seen_c[j] {
            let mut kind = "unmatched";
            if close_unmatched {
                kind = "closing";
                close_contour_end(b, r, &c[j], last, first, &mut generated, &mut w)?;
            } else {
                complete = false;
            }
            relations.push(ContourRelation {
                first_slice: first,
                last_slice: last,
                first_components: Vec::new(),
                last_components: vec![j as i64],
                kind,
                translation: [0, 0],
            });
        }
    }
    Ok((generated, relations, complete))
}
fn close_contour_end(
    b: &mut ContourBudget<'_>,
    r: &ContourInterpolationRequest,
    c: &ContourComponent,
    from: i64,
    to: i64,
    generated: &mut BTreeMap<i64, RasterMask>,
    w: &mut GapWorkspace,
) -> Result<(), Error> {
    let g = &r.geometry;
    w.left.fill(false);
    w.right.fill(false);
    for &p in &c.pixels {
        w.left[p as usize] = true;
    }
    for &p in &c.pixels {
        for q in contour_neighbours(p, g.columns, g.rows) {
            if q < 0 || !w.left[q as usize] {
                w.right[p as usize] = true;
            }
        }
    }
    let maximum = contour_dilation_distance_into(
        b,
        &w.left,
        &w.right,
        g,
        &mut w.ld,
        &mut w.positions,
        &mut w.nodes,
    )?;
    let direction = if from > to { -1 } else { 1 };
    let mut z = from + direction;
    while z != to {
        let delta = fmath::f64_to_i64(((z - from) as f64).abs());
        let physical = (g.slice_offset(z) - g.slice_offset(from)).abs();
        if delta > r.max_end_slices || physical >= r.end_length_mm {
            z += direction;
            continue;
        }
        b.step(w.left.len() as u64)?;
        let threshold =
            (maximum + fmath::min(g.row_spacing, g.col_spacing)) * physical / r.end_length_mm;
        let (mask, d) = (&w.left, &w.ld);
        if let Some(out) = generated.get_mut(&z) {
            contour_emit_runs(out, 0, 0, |p| mask[p] && d[p] >= threshold);
        }
        z += direction;
    }
    Ok(())
}
fn contour_emit_runs(
    mask: &mut RasterMask,
    shift_x: i64,
    shift_y: i64,
    on: impl Fn(usize) -> bool,
) {
    let (columns, rows) = (mask.columns, mask.rows);
    for y in 0..rows {
        let mut x = 0;
        while x < columns {
            if !on((y * columns + x) as usize) {
                x += 1;
                continue;
            }
            let start = x;
            x += 1;
            while x < columns && on((y * columns + x) as usize) {
                x += 1;
            }
            mask.set_run(y - shift_y, start - shift_x, x - shift_x);
        }
    }
}
struct ContourHeap<'a> {
    nodes: &'a mut Vec<i64>,
    positions: &'a mut [i64],
}
impl ContourHeap<'_> {
    fn swap(&mut self, a: usize, b: usize) {
        self.nodes.swap(a, b);
        self.positions[self.nodes[a] as usize] = a as i64;
        self.positions[self.nodes[b] as usize] = b as i64;
    }
    fn up(&mut self, distance: &[f64], mut i: usize) {
        while i > 0 {
            let p = (i - 1) / 2;
            if distance[self.nodes[p] as usize] <= distance[self.nodes[i] as usize] {
                break;
            }
            self.swap(i, p);
            i = p;
        }
    }
    fn update(&mut self, distance: &[f64], p: usize) {
        let mut i = self.positions[p];
        if i < 0 {
            i = self.nodes.len() as i64;
            self.nodes.push(p as i64);
            self.positions[p] = i;
        }
        self.up(distance, i as usize);
    }
    fn pop(&mut self, distance: &[f64]) -> usize {
        let p = self.nodes[0] as usize;
        let last = self.nodes.len() - 1;
        self.swap(0, last);
        self.nodes.truncate(last);
        self.positions[p] = -1;
        let mut i = 0;
        while 2 * i + 1 < self.nodes.len() {
            let mut j = 2 * i + 1;
            if j + 1 < self.nodes.len()
                && distance[self.nodes[j + 1] as usize] < distance[self.nodes[j] as usize]
            {
                j += 1;
            }
            if distance[self.nodes[i] as usize] <= distance[self.nodes[j] as usize] {
                break;
            }
            self.swap(i, j);
            i = j;
        }
        p
    }
}
fn contour_dilation_distance_into(
    b: &mut ContourBudget<'_>,
    mask: &[bool],
    seed: &[bool],
    g: &Grid,
    d: &mut [f64],
    positions: &mut [i64],
    nodes: &mut Vec<i64>,
) -> Result<f64, Error> {
    nodes.clear();
    let mut h = ContourHeap { nodes, positions };
    for p in 0..d.len() {
        if p % 1024 == 0 {
            b.step(1024)?;
        }
        d[p] = f64::INFINITY;
        h.positions[p] = -1;
        if mask[p] && seed[p] {
            d[p] = 0.0;
            h.update(d, p);
        }
    }
    let mut count = 0u64;
    while !h.nodes.is_empty() {
        if count.is_multiple_of(256) {
            b.step(256 * 4)?;
        }
        count += 1;
        let p = h.pop(d);
        for (edge, q) in contour_neighbours(p as i64, g.columns, g.rows)
            .into_iter()
            .enumerate()
        {
            if q < 0 || !mask[q as usize] {
                continue;
            }
            let cost = if edge >= 2 {
                g.row_spacing
            } else {
                g.col_spacing
            };
            let candidate = d[p] + cost;
            if candidate < d[q as usize] {
                d[q as usize] = candidate;
                h.update(d, q as usize);
            }
        }
    }
    let mut maximum = 0.0;
    for (p, &on) in mask.iter().enumerate() {
        if p % 1024 == 0 {
            b.step(1024)?;
        }
        if on {
            maximum = fmath::max(maximum, d[p]);
        }
    }
    Ok(maximum)
}
impl ContourInterpolationResult {
    pub fn validate_complete(&self) -> Result<(), Error> {
        if !self.unresolved.is_empty() {
            return Err(Error::wrap(
                ERR_CONTOUR_INTERPOLATION,
                format!("{} unresolved contour gaps", self.unresolved.len()),
            ));
        }
        Ok(())
    }
}
