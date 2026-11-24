use super::{
    CONTOUR_CLOSED_PLANAR, CONTOUR_CLOSED_PLANAR_XOR, Contour, Error, Kind, Point3D,
    finite_positive, finite_vec_components, finite_vector, g6,
};
use crate::render::{SliceGeometry, Vec3};
use crate::roi::{Point2D, RasterMask, rasterize_polygon};
use crate::textfmt::quote;
#[derive(Debug, Clone, Default)]
pub struct SlicePreviewOptions {
    pub sop_instance_uid: String,
    pub geometry: SliceGeometry,
    pub plane_tolerance_mm: f64,
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SlicePreview {
    pub contours: Vec<Vec<Point2D>>,
    pub mask: Option<RasterMask>,
    pub partial_geometry: bool,
    pub missing_geometry: bool,
}
pub fn preview_slice(contours: &[Contour], opts: &SlicePreviewOptions) -> SlicePreview {
    let (fill, partial) = preview_contour_fill(contours);
    let mut result = SlicePreview {
        partial_geometry: partial,
        ..Default::default()
    };
    if fill == Fill::Unsupported {
        return result;
    }
    let geometry = &opts.geometry;
    let has_geometry = valid_projection_geometry(geometry);
    for contour in contours {
        if contour_fill_for(&contour.geometric_type) != fill || contour.points.len() < 3 {
            continue;
        }
        let mut tolerance = opts.plane_tolerance_mm;
        if !contour.referenced_images.is_empty() {
            if !references_sop_instance(contour, &opts.sop_instance_uid) {
                continue;
            }
            tolerance = f64::INFINITY;
        } else if !has_geometry || tolerance < 0.0 || !tolerance.is_finite() {
            continue;
        }
        let points = if has_geometry {
            match project_contour(&contour.points, geometry, tolerance) {
                Ok(p) => p,
                Err(_) => continue,
            }
        } else {
            let p = preview_xy(&contour.points);
            if p.is_empty() {
                continue;
            }
            result.missing_geometry = true;
            p
        };
        if geometry.columns > 0 && geometry.rows > 0 {
            let mask = result
                .mask
                .get_or_insert_with(|| RasterMask::new(geometry.columns, geometry.rows));
            fill.combine(mask, &points);
        }
        result.contours.push(points);
    }
    if result.mask.as_ref().is_some_and(RasterMask::is_empty) {
        result.mask = None;
    }
    result
}
fn references_sop_instance(contour: &Contour, sop_instance_uid: &str) -> bool {
    if sop_instance_uid.is_empty() {
        return false;
    }
    contour
        .referenced_images
        .iter()
        .any(|r| r.sop_instance_uid == sop_instance_uid)
}
fn preview_xy(points: &[Point3D]) -> Vec<Point2D> {
    let mut out = Vec::with_capacity(points.len());
    for p in points {
        if !p.x.is_finite() || !p.y.is_finite() {
            return Vec::new();
        }
        out.push(Point2D::new(p.x, p.y));
    }
    out
}
pub(super) fn valid_projection_geometry(g: &SliceGeometry) -> bool {
    finite_positive(g.row_spacing)
        && finite_positive(g.col_spacing)
        && finite_vec_components(g.origin)
        && finite_vector(g.row_dir)
        && finite_vector(g.col_dir)
        && finite_vector(g.normal)
}
pub(super) fn project_contour(
    points: &[Point3D],
    g: &SliceGeometry,
    plane_tolerance_mm: f64,
) -> Result<Vec<Point2D>, Error> {
    let row_dir = g.row_dir.normalize();
    let col_dir = g.col_dir.normalize();
    let normal = g.normal.normalize();
    let mut out = Vec::with_capacity(points.len());
    for p in points {
        let delta = Vec3::new(p.x, p.y, p.z).sub(g.origin);
        let distance = delta.dot(normal).abs();
        if !distance.is_finite() || distance > plane_tolerance_mm {
            return Err(Error::new(
                Kind::GeometryMismatch,
                format!(
                    "contour point is {} mm from plane (tolerance {} mm)",
                    g6(distance),
                    g6(plane_tolerance_mm)
                ),
            ));
        }
        let x = delta.dot(row_dir) / g.col_spacing;
        let y = delta.dot(col_dir) / g.row_spacing;
        if !x.is_finite() || !y.is_finite() {
            return Err(Error::new(
                Kind::GeometryMismatch,
                "projected contour point is not finite",
            ));
        }
        out.push(Point2D::new(x, y));
    }
    Ok(out)
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Fill {
    Unsupported,
    Union,
    Xor,
}
fn contour_fill_for(geometric_type: &str) -> Fill {
    match geometric_type {
        "" | CONTOUR_CLOSED_PLANAR => Fill::Union,
        CONTOUR_CLOSED_PLANAR_XOR => Fill::Xor,
        _ => Fill::Unsupported,
    }
}
pub(super) fn strict_contour_fill(contours: &[Contour]) -> Result<Fill, Error> {
    let mut fill = Fill::Union;
    for c in contours {
        match contour_fill_for(&c.geometric_type) {
            Fill::Xor => fill = Fill::Xor,
            Fill::Unsupported => {
                return Err(Error::new(Kind::UnsupportedContourType, &c.geometric_type));
            }
            Fill::Union => {}
        }
    }
    if fill == Fill::Xor {
        for c in contours {
            if contour_fill_for(&c.geometric_type) != Fill::Xor {
                return Err(Error::new(
                    Kind::GeometryMismatch,
                    format!(
                        "CLOSEDPLANAR_XOR cannot be mixed with {}",
                        quote(&c.geometric_type)
                    ),
                ));
            }
        }
    }
    Ok(fill)
}
fn preview_contour_fill(contours: &[Contour]) -> (Fill, bool) {
    let mut fill = Fill::Unsupported;
    let mut partial = false;
    for c in contours {
        let mode = contour_fill_for(&c.geometric_type);
        if mode == Fill::Unsupported {
            partial = true;
        } else if fill == Fill::Unsupported {
            fill = mode;
        } else if fill != mode {
            partial = true;
        }
    }
    (fill, partial)
}
impl Fill {
    pub(super) fn combine(self, mask: &mut RasterMask, points: &[Point2D]) {
        let contour = rasterize_polygon(points, mask.columns, mask.rows);
        if self == Fill::Xor {
            mask.xor(&contour);
        } else {
            mask.union(&contour);
        }
    }
}
