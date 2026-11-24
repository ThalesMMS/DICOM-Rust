#![allow(clippy::neg_cmp_op_on_partial_ord)]
use super::advanced_geometry::Point2D;
use super::pdqsort_by::sort_slice;
use super::{Cancellation, Error, ctx_err};
use crate::fmath;
pub const ERR_POLYGON_FERET: &str = "roi: invalid polygon for Feret measurement";
fn cross(a: Point2D, b: Point2D, c: Point2D) -> f64 {
    (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)
}
pub fn polygon_feret_diameters_context(
    ctx: Option<&dyn Cancellation>,
    points: &[Point2D],
) -> Result<(f64, f64), Error> {
    if let Some(err) = ctx_err(ctx) {
        return Err(err);
    }
    if points.len() < 3 || points.len() > 1 << 20 {
        return Err(Error::sentinel(ERR_POLYGON_FERET));
    }
    let mut ordered = points.to_vec();
    if ordered.iter().any(|p| !p.x.is_finite() || !p.y.is_finite()) {
        return Err(Error::sentinel(ERR_POLYGON_FERET));
    }
    sort_slice(&mut ordered, &|a: &Point2D, b: &Point2D| {
        a.x < b.x || (a.x == b.x && a.y < b.y)
    });
    let mut hull: Vec<Point2D> = Vec::with_capacity(2 * ordered.len());
    for &p in &ordered {
        while hull.len() >= 2 && cross(hull[hull.len() - 2], hull[hull.len() - 1], p) <= 0.0 {
            hull.pop();
        }
        hull.push(p);
    }
    let upper_start = hull.len() + 1;
    for &p in ordered[..ordered.len() - 1].iter().rev() {
        while hull.len() >= upper_start
            && cross(hull[hull.len() - 2], hull[hull.len() - 1], p) <= 0.0
        {
            hull.pop();
        }
        hull.push(p);
    }
    hull.pop();
    if hull.len() < 3 {
        return Err(Error::sentinel(ERR_POLYGON_FERET));
    }
    let n = hull.len();
    let mut minimum = f64::INFINITY;
    let mut maximum = 0.0;
    let mut j = 1;
    for i in 0..n {
        if i & 255 == 0
            && let Some(err) = ctx_err(ctx)
        {
            return Err(err);
        }
        let a = hull[i];
        let b = hull[(i + 1) % n];
        let area = |k: usize| cross(a, b, hull[k]).abs();
        while area((j + 1) % n) > area(j) {
            j = (j + 1) % n;
        }
        let width = area(j) / fmath::hypot(b.x - a.x, b.y - a.y);
        minimum = fmath::min(minimum, width);
        for p in [a, b] {
            maximum = fmath::max(maximum, fmath::hypot(p.x - hull[j].x, p.y - hull[j].y));
            if area((j + 1) % n) == area(j) {
                let q = hull[(j + 1) % n];
                maximum = fmath::max(maximum, fmath::hypot(p.x - q.x, p.y - q.y));
            }
        }
    }
    if !(minimum > 0.0) || !minimum.is_finite() || !maximum.is_finite() {
        return Err(Error::sentinel(ERR_POLYGON_FERET));
    }
    Ok((minimum, maximum))
}
