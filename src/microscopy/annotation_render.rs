use super::{AnnotationGroup, Error, Level, Primitive, Rect, RgbaImage, SlidePoint};
use crate::fmath;
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AnnotationRenderOptions {
    pub optical_path: String,
    pub focal_z: f64,
    pub z_tolerance: f64,
    pub color: [u8; 4],
}
type Point = (i64, i64);
fn nearest(x: f64) -> i64 {
    fmath::f64_to_i64((x + 0.5).floor())
}
pub fn render_annotations(
    target: &mut RgbaImage,
    viewport: Rect,
    level: &Level,
    groups: &[AnnotationGroup],
    options: &AnnotationRenderOptions,
) -> Result<(), Error> {
    let bounds = Rect::new(0, 0, target.width as i64, target.height as i64);
    if bounds.is_empty() || viewport.is_empty() {
        return Err(Error::other(
            "dicom/microscopy: annotation target and viewport are required",
        ));
    }
    let mut ink = options.color;
    if ink[3] == 0 {
        ink = [255, 214, 64, 255];
    }
    let ink = premultiply(ink);
    for group in groups {
        if !path_visible(group, &options.optical_path)
            || !plane_visible(group, options.focal_z, options.z_tolerance)
        {
            continue;
        }
        for primitive in matrix_primitives(group, level)? {
            let points: Vec<Point> = primitive
                .points
                .iter()
                .map(|p| target_point((nearest(p.x), nearest(p.y)), viewport, bounds))
                .collect();
            draw_primitive(target, bounds, &points, &group.graphic_type, ink);
        }
    }
    Ok(())
}
fn matrix_primitives(group: &AnnotationGroup, level: &Level) -> Result<Vec<Primitive>, Error> {
    let mut out = group.primitives.clone();
    if group.coordinate_type == "2D" {
        if group.pixel_origin != "VOLUME" {
            return Err(Error::other(
                "dicom/microscopy: FRAME-relative annotations require a frame transform",
            ));
        }
        return Ok(out);
    }
    for primitive in &mut out {
        for point in &mut primitive.points {
            let (x, y) = level.pixel_coordinate(SlidePoint {
                x: point.x,
                y: point.y,
                z: point.z,
            })?;
            point.x = x;
            point.y = y;
        }
    }
    Ok(out)
}
fn path_visible(group: &AnnotationGroup, selected: &str) -> bool {
    selected.is_empty()
        || group.applies_all_optical_paths
        || group.optical_paths.iter().any(|id| id == selected)
}
fn plane_visible(group: &AnnotationGroup, focal_z: f64, tolerance: f64) -> bool {
    if group.applies_all_z_planes || group.common_z_coordinates.is_empty() {
        return true;
    }
    let tolerance = if tolerance < 0.0 { 0.0 } else { tolerance };
    group
        .common_z_coordinates
        .iter()
        .any(|z| (z - focal_z).abs() <= tolerance)
}
fn target_point(matrix: Point, viewport: Rect, target: Rect) -> Point {
    let x = target.min_x.wrapping_add(nearest(
        matrix.0.wrapping_sub(viewport.min_x) as f64 * target.dx() as f64 / viewport.dx() as f64,
    ));
    let y = target.min_y.wrapping_add(nearest(
        matrix.1.wrapping_sub(viewport.min_y) as f64 * target.dy() as f64 / viewport.dy() as f64,
    ));
    (x, y)
}
fn premultiply(p: [u8; 4]) -> [u8; 4] {
    let channel = |v: u8| ((u32::from(v) * 0x101 * u32::from(p[3]) / 0xff) >> 8) as u8;
    [channel(p[0]), channel(p[1]), channel(p[2]), p[3]]
}
fn set_pixel(target: &mut RgbaImage, bounds: Rect, x: i64, y: i64, ink: [u8; 4]) {
    if x >= bounds.min_x && x < bounds.max_x && y >= bounds.min_y && y < bounds.max_y {
        let i = (y as usize * target.width + x as usize) * 4;
        target.pix[i..i + 4].copy_from_slice(&ink);
    }
}
fn draw_primitive(
    target: &mut RgbaImage,
    bounds: Rect,
    points: &[Point],
    graphic_type: &str,
    ink: [u8; 4],
) {
    let Some(&first) = points.first() else {
        return;
    };
    if graphic_type == "POINT" || points.len() == 1 {
        for delta in -2..=2i64 {
            set_pixel(target, bounds, first.0.wrapping_add(delta), first.1, ink);
            set_pixel(target, bounds, first.0, first.1.wrapping_add(delta), ink);
        }
        return;
    }
    if graphic_type == "ELLIPSE" && points.len() == 4 && draw_ellipse(target, bounds, points, ink) {
        return;
    }
    for pair in points.windows(2) {
        draw_line(target, bounds, pair[0], pair[1], ink);
    }
    if matches!(graphic_type, "POLYGON" | "RECTANGLE" | "ELLIPSE") {
        draw_line(target, bounds, points[points.len() - 1], first, ink);
    }
}
fn draw_line(target: &mut RgbaImage, bounds: Rect, from: Point, to: Point, ink: [u8; 4]) {
    let Some((mut from, to)) = clip_line(bounds, from, to) else {
        return;
    };
    let dx = (to.0 - from.0).abs();
    let dy = -(to.1 - from.1).abs();
    let step_x = if from.0 < to.0 { 1 } else { -1 };
    let step_y = if from.1 < to.1 { 1 } else { -1 };
    let mut err = dx + dy;
    loop {
        set_pixel(target, bounds, from.0, from.1, ink);
        if from == to {
            return;
        }
        let twice = 2 * err;
        if twice >= dy {
            err += dy;
            from.0 += step_x;
        }
        if twice <= dx {
            err += dx;
            from.1 += step_y;
        }
    }
}
fn draw_ellipse(target: &mut RgbaImage, bounds: Rect, p: &[Point], ink: [u8; 4]) -> bool {
    let f = |v: i64| v as f64;
    let major_x = (f(p[1].0) - f(p[0].0)) / 2.0;
    let major_y = (f(p[1].1) - f(p[0].1)) / 2.0;
    let minor_x = (f(p[3].0) - f(p[2].0)) / 2.0;
    let minor_y = (f(p[3].1) - f(p[2].1)) / 2.0;
    let major = fmath::hypot(major_x, major_y);
    let minor = fmath::hypot(minor_x, minor_y);
    if major == 0.0 || minor == 0.0 {
        return false;
    }
    let center_x = (f(p[0].0) + f(p[1].0) + f(p[2].0) + f(p[3].0)) / 4.0;
    let center_y = (f(p[0].1) + f(p[1].1) + f(p[2].1) + f(p[3].1)) / 4.0;
    let max_steps = 32.max(4 * (bounds.dx() + bounds.dy()));
    let estimated = (2.0 * std::f64::consts::PI * fmath::max(major, minor)).ceil();
    let steps = if estimated < max_steps as f64 {
        32.max(fmath::f64_to_i64(estimated))
    } else {
        max_steps
    };
    let point_at = |angle: f64| {
        let (cosine, sine) = (fmath::cos(angle), fmath::sin(angle));
        (
            nearest(center_x + major_x * cosine + minor_x * sine),
            nearest(center_y + major_y * cosine + minor_y * sine),
        )
    };
    let mut previous = point_at(0.0);
    for step in 1..=steps {
        let current = point_at(2.0 * std::f64::consts::PI * step as f64 / steps as f64);
        draw_line(target, bounds, previous, current, ink);
        previous = current;
    }
    true
}
fn clip_line(bounds: Rect, from: Point, to: Point) -> Option<(Point, Point)> {
    if bounds.is_empty() {
        return None;
    }
    let (x0, y0) = (from.0 as f64, from.1 as f64);
    let dx = to.0 as f64 - x0;
    let dy = to.1 as f64 - y0;
    let (min_x, min_y) = (bounds.min_x as f64, bounds.min_y as f64);
    let (max_x, max_y) = ((bounds.max_x - 1) as f64, (bounds.max_y - 1) as f64);
    let (mut start, mut end) = (0.0f64, 1.0f64);
    for (delta, distance) in [
        (-dx, x0 - min_x),
        (dx, max_x - x0),
        (-dy, y0 - min_y),
        (dy, max_y - y0),
    ] {
        if delta == 0.0 {
            if distance < 0.0 {
                return None;
            }
            continue;
        }
        let ratio = distance / delta;
        if delta < 0.0 {
            if ratio > end {
                return None;
            }
            start = fmath::max(start, ratio);
        } else {
            if ratio < start {
                return None;
            }
            end = fmath::min(end, ratio);
        }
    }
    let clipped = |ratio: f64| {
        (
            (bounds.max_x - 1).min(bounds.min_x.max(nearest(x0 + ratio * dx))),
            (bounds.max_y - 1).min(bounds.min_y.max(nearest(y0 + ratio * dy))),
        )
    };
    Some((clipped(start), clipped(end)))
}
