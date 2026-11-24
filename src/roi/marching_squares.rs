use super::Error;
use super::advanced_geometry::Point2D;
use super::roimask::RasterMask;
use crate::fmath;
use crate::textfmt::float_v;
use std::collections::{BTreeSet, HashMap};
pub const ERR_ISO_CONTOUR: &str = "roi: invalid iso-contour input";
#[derive(Debug, Clone, Default, PartialEq)]
pub struct IsoContour {
    pub points: Vec<Point2D>,
}
pub(crate) const MAX_ISO_CONTOUR_SAMPLES: i64 = 1 << 28;
pub(crate) fn valid_iso_contour_grid(width: i64, height: i64) -> bool {
    width > 0 && height > 0 && width <= MAX_ISO_CONTOUR_SAMPLES / height
}
pub fn iso_contours(
    width: i64,
    height: i64,
    mut value: impl FnMut(i64, i64) -> f64,
    level: f64,
) -> Result<Vec<IsoContour>, Error> {
    if !valid_iso_contour_grid(width, height) || !level.is_finite() {
        return Err(iso_contour_field_error(width, height, level));
    }
    let (pw, ph) = (width as usize + 2, height as usize + 2);
    let mut sample = vec![f64::NEG_INFINITY; pw * ph];
    for y in 0..height {
        for x in 0..width {
            let v = value(x, y);
            if !v.is_nan() {
                sample[(y as usize + 1) * pw + x as usize + 1] = v;
            }
        }
    }
    Ok(trace_iso_contours(&sample, pw, ph, level))
}
pub(crate) fn iso_contour_field_error(width: i64, height: i64, level: f64) -> Error {
    Error::wrap(
        ERR_ISO_CONTOUR,
        format_args!("{width}×{height} field at level {}", float_v(level)),
    )
}
struct IsoSegment {
    from: usize,
    to: usize,
    start: Point2D,
}
fn trace_iso_contours(sample: &[f64], pw: usize, ph: usize, level: f64) -> Vec<IsoContour> {
    let at = |i: usize, j: usize| sample[j * pw + i];
    let crossing = |ax: usize, ay: usize, bx: usize, by: usize| -> Point2D {
        let (va, vb) = (at(ax, ay), at(bx, by));
        let mut t = 0.5;
        if !va.is_infinite() && !vb.is_infinite() && va != vb {
            t = (level - va) / (vb - va);
        }
        Point2D::new(
            ax as f64 + t * (bx as f64 - ax as f64) - 1.0,
            ay as f64 + t * (by as f64 - ay as f64) - 1.0,
        )
    };
    let mut segments: Vec<IsoSegment> = Vec::new();
    for j in 0..ph.saturating_sub(1) {
        for i in 0..pw.saturating_sub(1) {
            let cx = [i, i + 1, i + 1, i];
            let cy = [j, j, j + 1, j + 1];
            let mut inside = [false; 4];
            let mut count = 0;
            for k in 0..4 {
                inside[k] = at(cx[k], cy[k]) >= level;
                if inside[k] {
                    count += 1;
                }
            }
            if count == 0 || count == 4 {
                continue;
            }
            let edge_key = [
                2 * (j * pw + i),
                2 * (j * pw + i + 1) + 1,
                2 * ((j + 1) * pw + i),
                2 * (j * pw + i) + 1,
            ];
            struct Cross {
                key: usize,
                p: Point2D,
                exit: bool,
            }
            let mut crosses: Vec<Cross> = Vec::with_capacity(4);
            for k in 0..4 {
                let n = (k + 1) % 4;
                if inside[k] == inside[n] {
                    continue;
                }
                crosses.push(Cross {
                    key: edge_key[k],
                    p: crossing(cx[k], cy[k], cx[n], cy[n]),
                    exit: inside[k],
                });
            }
            let mut connected = true;
            if crosses.len() == 4 {
                let mut centre = 0.0;
                for k in 0..4 {
                    centre += at(cx[k], cy[k]);
                }
                connected = centre / 4.0 >= level;
            }
            let len = crosses.len() as isize;
            for (c, x) in crosses.iter().enumerate() {
                if !x.exit {
                    continue;
                }
                let step: isize = if connected { 1 } else { -1 };
                let mut e = ((c as isize + step + len) % len) as usize;
                while crosses[e].exit {
                    e = ((e as isize + step + len) % len) as usize;
                }
                segments.push(IsoSegment {
                    from: x.key,
                    to: crosses[e].key,
                    start: x.p,
                });
            }
        }
    }
    let mut by_start: HashMap<usize, usize> = HashMap::with_capacity(segments.len());
    for (s, segment) in segments.iter().enumerate() {
        by_start.insert(segment.from, s);
    }
    let mut used = vec![false; segments.len()];
    let mut contours = Vec::new();
    for s in 0..segments.len() {
        if used[s] {
            continue;
        }
        let mut points: Vec<Point2D> = Vec::new();
        let mut cur = s;
        while !used[cur] {
            used[cur] = true;
            append_distinct(&mut points, segments[cur].start);
            match by_start.get(&segments[cur].to) {
                Some(&next) => cur = next,
                None => break,
            }
        }
        if points.len() > 1 && points[0] == points[points.len() - 1] {
            points.pop();
        }
        if points.len() >= 3 {
            contours.push(IsoContour { points });
        }
    }
    contours
}
fn append_distinct(points: &mut Vec<Point2D>, p: Point2D) {
    if points.last() == Some(&p) {
        return;
    }
    points.push(p);
}
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SubpixelContourOptions {
    pub smooth_sigma: f64,
}
impl RasterMask {
    pub fn subpixel_contours(
        &self,
        opts: SubpixelContourOptions,
    ) -> Result<Vec<IsoContour>, Error> {
        if !valid_iso_contour_grid(self.columns, self.rows) {
            return Err(Error::wrap(
                ERR_ISO_CONTOUR,
                format_args!("{}×{} mask", self.columns, self.rows),
            ));
        }
        if opts.smooth_sigma < 0.0 || opts.smooth_sigma.is_nan() || opts.smooth_sigma > 64.0 {
            return Err(Error::wrap(
                ERR_ISO_CONTOUR,
                format_args!("smoothing σ {}", float_v(opts.smooth_sigma)),
            ));
        }
        if self.is_empty() {
            return Ok(Vec::new());
        }
        let (w, h) = (self.columns as usize, self.rows as usize);
        let mut field = vec![0.0f64; w * h];
        for y in 0..h {
            for run in &self.runs[y] {
                for x in run.start..run.end {
                    field[y * w + x as usize] = 1.0;
                }
            }
        }
        if opts.smooth_sigma > 0.0 {
            field = gaussian_blur_2d(&field, w, h, opts.smooth_sigma);
        }
        iso_contours(
            self.columns,
            self.rows,
            |x, y| field[y as usize * w + x as usize],
            0.5,
        )
    }
}
pub(crate) fn gaussian_blur_2d(field: &[f64], w: usize, h: usize, sigma: f64) -> Vec<f64> {
    let radius = fmath::f64_to_i64((3.0 * sigma).ceil());
    let mut kernel = vec![0.0f64; (2 * radius + 1) as usize];
    let mut sum = 0.0;
    for (i, k) in kernel.iter_mut().enumerate() {
        let d = (i as i64 - radius) as f64;
        *k = fmath::exp(-d * d / (2.0 * sigma * sigma));
        sum += *k;
    }
    for k in kernel.iter_mut() {
        *k /= sum;
    }
    let (wi, hi) = (w as i64, h as i64);
    let mut tmp = vec![0.0f64; field.len()];
    for y in 0..h {
        for x in 0..w {
            let mut v = 0.0;
            for (k, weight) in kernel.iter().enumerate() {
                let sx = x as i64 + k as i64 - radius;
                if sx >= 0 && sx < wi {
                    v += weight * field[y * w + sx as usize];
                }
            }
            tmp[y * w + x] = v;
        }
    }
    let mut out = vec![0.0f64; field.len()];
    for y in 0..h {
        for x in 0..w {
            let mut v = 0.0;
            for (k, weight) in kernel.iter().enumerate() {
                let sy = y as i64 + k as i64 - radius;
                if sy >= 0 && sy < hi {
                    v += weight * tmp[sy as usize * w + x];
                }
            }
            out[y * w + x] = v;
        }
    }
    out
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LabelContours {
    pub label: i64,
    pub contours: Vec<IsoContour>,
}
pub fn label_iso_contours(
    width: i64,
    height: i64,
    mut label: impl FnMut(i64, i64) -> i64,
) -> Result<Vec<LabelContours>, Error> {
    if !valid_iso_contour_grid(width, height) {
        return Err(label_iso_contour_field_error(width, height));
    }
    let (w, h) = (width as usize, height as usize);
    let mut labels = vec![0i64; w * h];
    let mut seen: BTreeSet<i64> = BTreeSet::new();
    for y in 0..h {
        for x in 0..w {
            let value = label(x as i64, y as i64);
            labels[y * w + x] = value;
            if value != 0 {
                seen.insert(value);
            }
        }
    }
    let mut out = Vec::with_capacity(seen.len());
    for value in seen {
        let contours = iso_contours(
            width,
            height,
            |x, y| {
                if labels[y as usize * w + x as usize] == value {
                    1.0
                } else {
                    0.0
                }
            },
            0.5,
        )?;
        out.push(LabelContours {
            label: value,
            contours,
        });
    }
    Ok(out)
}
pub(crate) fn label_iso_contour_field_error(width: i64, height: i64) -> Error {
    Error::wrap(
        ERR_ISO_CONTOUR,
        format_args!("{width}×{height} label field"),
    )
}
