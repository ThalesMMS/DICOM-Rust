use super::volume::{Ctx, ErrorKind, FilterError, Volume, ctx_err, voxel_count};
use crate::fmath;
use std::f64::consts::PI;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rect {
    pub min_x: i64,
    pub min_y: i64,
    pub max_x: i64,
    pub max_y: i64,
}
impl Rect {
    fn empty(&self) -> bool {
        self.min_x >= self.max_x || self.min_y >= self.max_y
    }
    fn within(&self, s: &Rect) -> bool {
        self.empty()
            || (s.min_x <= self.min_x
                && self.max_x <= s.max_x
                && s.min_y <= self.min_y
                && self.max_y <= s.max_y)
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct HoughLimits {
    pub max_pixels: i64,
    pub max_bytes: i64,
    pub max_operations: u64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Hough2DOptions {
    pub region: Rect,
    pub limits: HoughLimits,
    pub min_votes: u32,
    pub max_candidates: i64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct HoughReport {
    pub working_bytes: i64,
    pub reserved_operations: u64,
    pub operations: u64,
    pub edge_pixels: i64,
}
fn resource() -> FilterError {
    FilterError::new(
        ErrorKind::HoughResourceLimit,
        "imagefilter: Hough 2D resource limit",
    )
}
fn hough_invalid(what: &str) -> FilterError {
    FilterError::new(
        ErrorKind::InvalidHough2D,
        format!("imagefilter: invalid Hough 2D request: {what}"),
    )
}
struct HoughWork<'a> {
    ctx: Ctx<'a>,
    report: HoughReport,
    limit: u64,
}
impl HoughWork<'_> {
    fn tick(&mut self) -> Result<(), FilterError> {
        self.report.operations = self.report.operations.wrapping_add(1);
        if self.report.operations > self.limit {
            return Err(resource());
        }
        if self.report.operations & 1023 == 0 {
            return ctx_err(self.ctx);
        }
        Ok(())
    }
}
fn finite(v: f64) -> bool {
    v.is_finite()
}
fn hough_grid(lo: f64, hi: f64, step: f64, exclusive: bool) -> Result<usize, FilterError> {
    if !finite(lo)
        || !finite(hi)
        || !(step > 0.0)
        || !finite(step)
        || hi < lo
        || (exclusive && hi == lo)
    {
        return Err(hough_invalid("grid interval/step"));
    }
    let q = (hi - lo) / step;
    if !finite(q) || q > (1u64 << 24) as f64 {
        return Err(resource());
    }
    let rounded = q.round();
    if (q - rounded).abs() > 1e-8 * fmath::max(1.0, fmath::min(10.0, q.abs())) {
        return Err(hough_invalid("grid endpoints must align with step"));
    }
    let mut n = rounded as i64 + 1;
    if exclusive {
        n -= 1;
    }
    if n < 1 {
        return Err(hough_invalid("empty grid"));
    }
    Ok(n as usize)
}
fn hough_mul(a: u64, b: u64) -> Result<u64, FilterError> {
    if b != 0 && a > u64::MAX / b {
        return Err(resource());
    }
    Ok(a * b)
}
fn hough_sum(v: &[u64]) -> Result<u64, FilterError> {
    let mut sum: u64 = 0;
    for x in v {
        if sum > u64::MAX - x {
            return Err(resource());
        }
        sum += x;
    }
    Ok(sum)
}
#[allow(clippy::too_many_arguments)]
fn hough_setup<'a>(
    ctx: Ctx<'a>,
    src: &Volume,
    o: &Hough2DOptions,
    cells: u64,
    scratch: u64,
    angles: i64,
    ops: u64,
) -> Result<HoughWork<'a>, FilterError> {
    ctx_err(ctx)?;
    if src.dims[2] != 1 {
        return Err(hough_invalid("requires one 2D slice"));
    }
    let n = match voxel_count(src.dims) {
        Some(n) if src.data.len() as i64 == n => n,
        _ => return Err(hough_invalid("source dimensions/data")),
    };
    let mut l = o.limits;
    if l.max_pixels < 0
        || l.max_pixels > 1 << 20
        || l.max_bytes < 0
        || l.max_bytes > 64 << 20
        || l.max_operations > 100_000_000
    {
        return Err(hough_invalid("limits may only lower finite ceilings"));
    }
    if l.max_pixels == 0 {
        l.max_pixels = 1 << 20;
    }
    if l.max_bytes == 0 {
        l.max_bytes = 64 << 20;
    }
    if l.max_operations == 0 {
        l.max_operations = 100_000_000;
    }
    if n > l.max_pixels {
        return Err(resource());
    }
    for a in 0..2 {
        let s = src.spacing_mm[a];
        if !(s > 0.0) || !finite(s) || !finite((src.dims[a] - 1) as f64 * s) {
            return Err(hough_invalid("physical spacing/extent"));
        }
    }
    let bounds = Rect {
        min_x: 0,
        min_y: 0,
        max_x: src.dims[0],
        max_y: src.dims[1],
    };
    if o.region.empty() || !o.region.within(&bounds) {
        return Err(hough_invalid("nonempty contained region required"));
    }
    if o.max_candidates < 1 || o.max_candidates > 64 || o.min_votes == 0 {
        return Err(hough_invalid("candidate cap/minimum votes"));
    }
    if cells < 1 || !(1..=4096).contains(&angles) {
        return Err(resource());
    }
    let bytes = hough_sum(&[
        cells.wrapping_mul(4),
        scratch.wrapping_mul(4),
        (angles as u64) * 16,
        (o.max_candidates as u64) * 64,
        4096,
    ]);
    let bytes = match bytes {
        Ok(b) if b <= l.max_bytes as u64 && ops <= l.max_operations => b,
        _ => return Err(resource()),
    };
    let mut w = HoughWork {
        ctx,
        limit: ops,
        report: HoughReport {
            working_bytes: bytes as i64,
            reserved_operations: ops,
            ..Default::default()
        },
    };
    for v in &src.data {
        w.tick()?;
        if *v != 0.0 && *v != 1.0 {
            return Err(hough_invalid("binary edge mask required"));
        }
    }
    ctx_err(ctx)?;
    Ok(w)
}
fn hough_dimensions(src: &Volume, o: &Hough2DOptions) -> Result<(u64, u64), FilterError> {
    if src.dims[2] != 1 {
        return Err(hough_invalid("requires one 2D slice"));
    }
    let n = match voxel_count(src.dims) {
        Some(n) if src.data.len() as i64 == n => n,
        _ => return Err(hough_invalid("dimensions/data")),
    };
    if n > 1 << 20 {
        return Err(resource());
    }
    let bounds = Rect {
        min_x: 0,
        min_y: 0,
        max_x: src.dims[0],
        max_y: src.dims[1],
    };
    if o.max_candidates < 1
        || o.max_candidates > 64
        || o.region.empty()
        || !o.region.within(&bounds)
    {
        return Err(hough_invalid("region/candidate cap"));
    }
    let r = o.region;
    Ok((
        n as u64,
        ((r.max_x - r.min_x) as u64) * ((r.max_y - r.min_y) as u64),
    ))
}
fn hough_peak(w: &mut HoughWork<'_>, acc: &[u32], min: u32) -> Result<Option<usize>, FilterError> {
    let mut best = None;
    let mut votes = 0u32;
    for (i, v) in acc.iter().copied().enumerate() {
        w.tick()?;
        if v >= min && v > votes {
            best = Some(i);
            votes = v;
        }
    }
    Ok(best)
}
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct HoughLineOptions {
    pub base: Hough2DOptions,
    pub rho_min_mm: f64,
    pub rho_max_mm: f64,
    pub rho_step_mm: f64,
    pub theta_min: f64,
    pub theta_max: f64,
    pub theta_step: f64,
    pub suppress_rho_mm: f64,
    pub suppress_theta: f64,
}
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct HoughLine {
    pub rho_mm: f64,
    pub theta: f64,
    pub votes: u32,
}
#[derive(Debug, Clone, PartialEq, Default)]
pub struct HoughLineResult {
    pub candidates: Vec<HoughLine>,
    pub report: HoughReport,
}
pub fn hough_lines_2d(
    ctx: Ctx<'_>,
    edges: Option<&Volume>,
    o: &HoughLineOptions,
) -> Result<HoughLineResult, FilterError> {
    let Some(edges) = edges else {
        return Err(hough_invalid("requires one 2D slice"));
    };
    let (n, area) = hough_dimensions(edges, &o.base)?;
    if !finite(o.theta_min)
        || !finite(o.theta_max)
        || o.theta_min < 0.0
        || o.theta_max > PI
        || !(o.suppress_rho_mm > 0.0)
        || !finite(o.suppress_rho_mm)
        || !(o.suppress_theta > 0.0)
        || !finite(o.suppress_theta)
        || o.suppress_theta > PI / 2.0
    {
        return Err(hough_invalid("line angle/suppression"));
    }
    let nr = hough_grid(o.rho_min_mm, o.rho_max_mm, o.rho_step_mm, false)?;
    let nt = hough_grid(o.theta_min, o.theta_max, o.theta_step, true)?;
    let cells = match hough_mul(nr as u64, nt as u64) {
        Ok(c) if c <= 16 << 20 => c,
        _ => return Err(resource()),
    };
    let votes = hough_mul(area, nt as u64)?;
    let scans = hough_mul(cells, (1 + 2 * o.base.max_candidates) as u64)?;
    let ops = hough_sum(&[n, votes, scans, nt as u64])?;
    let mut w = hough_setup(ctx, edges, &o.base, cells, 0, nt as i64, ops)?;
    let mut acc = vec![0u32; cells as usize];
    let mut trig = vec![[0f64; 2]; nt];
    for (a, t) in trig.iter_mut().enumerate() {
        w.tick()?;
        let theta = o.theta_min + a as f64 * o.theta_step;
        *t = [fmath::cos(theta), fmath::sin(theta)];
    }
    let r = o.base.region;
    for y in r.min_y..r.max_y {
        for x in r.min_x..r.max_x {
            let on = edges.at(x, y, 0) == 1.0;
            if on {
                w.report.edge_pixels += 1;
            }
            let (px, py) = (
                x as f64 * edges.spacing_mm[0],
                y as f64 * edges.spacing_mm[1],
            );
            for (a, t) in trig.iter().enumerate() {
                w.tick()?;
                if !on {
                    continue;
                }
                let rho = px * t[0] + py * t[1];
                if rho < o.rho_min_mm || rho > o.rho_max_mm {
                    continue;
                }
                let ri = fmath::f64_to_i64(((rho - o.rho_min_mm) / o.rho_step_mm + 0.5).floor());
                if ri >= 0 && (ri as usize) < nr {
                    acc[a * nr + ri as usize] += 1;
                }
            }
        }
    }
    let mut out = Vec::new();
    while (out.len() as i64) < o.base.max_candidates {
        let Some(idx) = hough_peak(&mut w, &acc, o.base.min_votes)? else {
            break;
        };
        let p = HoughLine {
            rho_mm: o.rho_min_mm + (idx % nr) as f64 * o.rho_step_mm,
            theta: o.theta_min + (idx / nr) as f64 * o.theta_step,
            votes: acc[idx],
        };
        out.push(p);
        for i in 0..acc.len() {
            w.tick()?;
            let rho = o.rho_min_mm + (i % nr) as f64 * o.rho_step_mm;
            let theta = o.theta_min + (i / nr) as f64 * o.theta_step;
            let mut da = (theta - p.theta).abs();
            let mut dr = (rho - p.rho_mm).abs();
            if da > PI / 2.0 {
                da = PI - da;
                dr = (rho + p.rho_mm).abs();
            }
            if da <= o.suppress_theta && dr <= o.suppress_rho_mm {
                acc[i] = 0;
            }
        }
    }
    ctx_err(ctx)?;
    Ok(HoughLineResult {
        candidates: out,
        report: w.report,
    })
}
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct HoughCircleOptions {
    pub base: Hough2DOptions,
    pub center_min_mm: [f64; 2],
    pub center_max_mm: [f64; 2],
    pub center_step_mm: f64,
    pub radius_min_mm: f64,
    pub radius_max_mm: f64,
    pub radius_step_mm: f64,
    pub angle_bins: i64,
    pub suppress_center_mm: f64,
    pub suppress_radius_mm: f64,
}
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct HoughCircle {
    pub center_mm: [f64; 2],
    pub radius_mm: f64,
    pub votes: u32,
}
#[derive(Debug, Clone, PartialEq, Default)]
pub struct HoughCircleResult {
    pub candidates: Vec<HoughCircle>,
    pub report: HoughReport,
}
pub fn hough_circles_2d(
    ctx: Ctx<'_>,
    edges: Option<&Volume>,
    o: &HoughCircleOptions,
) -> Result<HoughCircleResult, FilterError> {
    let Some(edges) = edges else {
        return Err(hough_invalid("requires one 2D slice"));
    };
    let (n, area) = hough_dimensions(edges, &o.base)?;
    if o.angle_bins < 8
        || o.angle_bins > 4096
        || !(o.radius_min_mm > 0.0)
        || !(o.suppress_center_mm > 0.0)
        || !finite(o.suppress_center_mm)
        || !(o.suppress_radius_mm > 0.0)
        || !finite(o.suppress_radius_mm)
    {
        return Err(hough_invalid("circle angle/radius/suppression"));
    }
    let r = o.base.region;
    for a in 0..2 {
        let lo = [r.min_x, r.min_y][a] as f64 * edges.spacing_mm[a];
        let hi = [r.max_x - 1, r.max_y - 1][a] as f64 * edges.spacing_mm[a];
        if !finite(lo) || !finite(hi) || o.center_min_mm[a] < lo || o.center_max_mm[a] > hi {
            return Err(hough_invalid("center grid outside physical region"));
        }
    }
    let nx = hough_grid(
        o.center_min_mm[0],
        o.center_max_mm[0],
        o.center_step_mm,
        false,
    )?;
    let ny = hough_grid(
        o.center_min_mm[1],
        o.center_max_mm[1],
        o.center_step_mm,
        false,
    )?;
    let nr = hough_grid(o.radius_min_mm, o.radius_max_mm, o.radius_step_mm, false)?;
    let plane = hough_mul(nx as u64, ny as u64)?;
    let cells = match hough_mul(plane, nr as u64) {
        Ok(c) if c <= 16 << 20 => c,
        _ => return Err(resource()),
    };
    let voting = hough_mul(area, (o.angle_bins + 1) as u64)?;
    let voting = hough_sum(&[voting, plane])?;
    let voting = hough_mul(voting, nr as u64)?;
    let scans = hough_mul(cells, (1 + 2 * o.base.max_candidates) as u64)?;
    let ops = hough_sum(&[n, voting, scans, o.angle_bins as u64])?;
    let mut w = hough_setup(ctx, edges, &o.base, cells, plane, o.angle_bins, ops)?;
    let plane = plane as usize;
    let mut acc = vec![0u32; cells as usize];
    let mut stamp = vec![0u32; plane];
    let mut trig = vec![[0f64; 2]; o.angle_bins as usize];
    for (a, t) in trig.iter_mut().enumerate() {
        w.tick()?;
        let angle = 2.0 * PI * a as f64 / o.angle_bins as f64;
        *t = [fmath::cos(angle), fmath::sin(angle)];
    }
    for ri in 0..nr {
        for s in stamp.iter_mut() {
            w.tick()?;
            *s = 0;
        }
        let radius = o.radius_min_mm + ri as f64 * o.radius_step_mm;
        for y in r.min_y..r.max_y {
            for x in r.min_x..r.max_x {
                w.tick()?;
                if edges.at(x, y, 0) != 1.0 {
                    continue;
                }
                if ri == 0 {
                    w.report.edge_pixels += 1;
                }
                let id = (edges.index(x, y, 0) + 1) as u32;
                let (px, py) = (
                    x as f64 * edges.spacing_mm[0],
                    y as f64 * edges.spacing_mm[1],
                );
                for t in &trig {
                    w.tick()?;
                    let (cx, cy) = (px - radius * t[0], py - radius * t[1]);
                    if cx < o.center_min_mm[0]
                        || cx > o.center_max_mm[0]
                        || cy < o.center_min_mm[1]
                        || cy > o.center_max_mm[1]
                    {
                        continue;
                    }
                    let ix = fmath::f64_to_i64(
                        ((cx - o.center_min_mm[0]) / o.center_step_mm + 0.5).floor(),
                    );
                    let iy = fmath::f64_to_i64(
                        ((cy - o.center_min_mm[1]) / o.center_step_mm + 0.5).floor(),
                    );
                    if ix < 0 || ix as usize >= nx || iy < 0 || iy as usize >= ny {
                        continue;
                    }
                    let i = iy as usize * nx + ix as usize;
                    if stamp[i] != id {
                        stamp[i] = id;
                        acc[ri * plane + i] += 1;
                    }
                }
            }
        }
    }
    let mut out = Vec::new();
    while (out.len() as i64) < o.base.max_candidates {
        let Some(idx) = hough_peak(&mut w, &acc, o.base.min_votes)? else {
            break;
        };
        let p = HoughCircle {
            center_mm: [
                o.center_min_mm[0] + (idx % nx) as f64 * o.center_step_mm,
                o.center_min_mm[1] + ((idx / nx) % ny) as f64 * o.center_step_mm,
            ],
            radius_mm: o.radius_min_mm + (idx / plane) as f64 * o.radius_step_mm,
            votes: acc[idx],
        };
        out.push(p);
        for i in 0..acc.len() {
            w.tick()?;
            let cx = o.center_min_mm[0] + (i % nx) as f64 * o.center_step_mm;
            let cy = o.center_min_mm[1] + ((i / nx) % ny) as f64 * o.center_step_mm;
            let radius = o.radius_min_mm + (i / plane) as f64 * o.radius_step_mm;
            if fmath::hypot(cx - p.center_mm[0], cy - p.center_mm[1]) <= o.suppress_center_mm
                && (radius - p.radius_mm).abs() <= o.suppress_radius_mm
            {
                acc[i] = 0;
            }
        }
    }
    ctx_err(ctx)?;
    Ok(HoughCircleResult {
        candidates: out,
        report: w.report,
    })
}
