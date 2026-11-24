#![forbid(unsafe_code)]
pub use crate::render::parallel::Cancellation;
use std::fmt;
pub mod advanced_geometry;
pub mod autothreshold;
pub mod bidimensional;
pub mod binary_antialias;
pub(crate) mod binary_antialias_distance;
pub mod calcium;
pub mod chan_vese;
pub mod connected_components;
pub mod consensus;
pub mod contour_interpolation;
pub mod control_curve;
pub mod cpr_lumen;
pub mod distance3d;
pub mod edge_levelset;
pub mod grayscale_attribute;
pub mod grayscale_reconstruction;
pub mod grayscale_tophat;
pub mod growcut;
pub mod growcut_context;
pub mod intensity_visit;
pub mod levelset;
pub mod levelset_interface;
pub mod line_profile;
pub mod livewire;
pub mod livewire_canny;
pub mod local_texture;
pub mod marching_squares;
pub mod measure;
pub mod measure_reslice;
pub mod measure_subpixel;
pub mod medial_thickness;
pub mod pca_shape;
pub mod pca_shape_levelset;
pub(crate) mod pdqsort;
pub(crate) mod pdqsort_by;
pub mod polygon_feret;
pub mod polygon_raster;
pub mod probabilistic_classifier;
pub mod raster_contour;
pub mod region_growing;
pub mod resample_fraction;
pub mod resample_grid;
pub mod resample_labels;
pub mod roimask;
pub mod segment_ops;
pub mod segment_stats;
pub mod segmentation;
mod shape_crofton;
mod shape_feret;
pub mod shape_intensity;
pub mod shape_stats;
pub mod signed_distance;
pub mod simplify_physical;
mod simplify_physical_exact;
pub mod skeleton;
pub mod slic;
pub mod texture;
pub mod vector_confidence;
pub(crate) mod vector_confidence_stats;
pub mod vector_roi;
pub mod voi;
pub mod voi_peak;
pub mod voi_volume;
pub mod watershed;
pub mod watershed_split;
pub use advanced_geometry::Point2D;
pub use distance3d::{ERR_INVALID_VOLUME_MASK, VolumeMask};
pub use intensity_visit::{intensity_moments_visit, intensity_statistics_visit};
pub use marching_squares::{
    ERR_ISO_CONTOUR, IsoContour, LabelContours, SubpixelContourOptions, iso_contours,
    label_iso_contours,
};
pub use measure::{
    Stats, SummaryStats, angle_deg, cobb_angle_deg, deviation_mm, ellipse_area_mm2, length_mm,
    polygon_area_mm2, polygon_length_mm, rectangle_area_mm2, stats_2d, stats_2d_summary,
    stats_2d_with_histogram, stats_2d_with_histogram_bounds, stats_3d,
};
pub use measure_reslice::{
    MeasureSpacing, ellipse_area_mm2_with, length_mm_with, polygon_area_mm2_with,
    polygon_length_mm_with, rectangle_area_mm2_with,
};
pub use measure_subpixel::{
    closed_polygon_area, point_2ds_from_image, subpixel_angle_deg, subpixel_cobb_angle_deg,
    subpixel_deviation_mm, subpixel_ellipse_area_mm2, subpixel_length_mm,
    subpixel_polygon_area_mm2, subpixel_polygon_length_mm, subpixel_rectangle_area_mm2,
};
pub use polygon_raster::rasterize_polygon;
pub use raster_contour::simplify_closed_contour;
pub use roimask::{
    ERR_INVALID_RASTER_MASK_DIMENSIONS, ERR_RASTER_MASK_BUFFER_LENGTH,
    ERR_RASTER_MASK_DIMENSION_OVERFLOW, ERR_RASTER_MASK_TOO_LARGE, MaskRun,
    RASTER_MASK_PIXEL_LIMIT, RasterMask, raster_mask_pixel_count, validate_raster_mask_buffer,
};
pub use segment_stats::VolumeStats;
pub use segmentation::Segmentation3D;
pub use shape_intensity::{
    IntensityAccumulator, IntensityMoments, IntensityStatistics, new_intensity_moments,
    new_intensity_statistics,
};
pub use vector_roi::{RoiShape, VectorRoi};
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct ImagePoint {
    pub x: i64,
    pub y: i64,
}
impl ImagePoint {
    pub const fn new(x: i64, y: i64) -> ImagePoint {
        ImagePoint { x, y }
    }
}
impl std::ops::Sub for ImagePoint {
    type Output = ImagePoint;
    fn sub(self, other: ImagePoint) -> ImagePoint {
        ImagePoint::new(self.x.wrapping_sub(other.x), self.y.wrapping_sub(other.y))
    }
}
impl std::ops::Add for ImagePoint {
    type Output = ImagePoint;
    fn add(self, other: ImagePoint) -> ImagePoint {
        ImagePoint::new(self.x.wrapping_add(other.x), self.y.wrapping_add(other.y))
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct ImageRect {
    pub min: ImagePoint,
    pub max: ImagePoint,
}
pub const ERR_CONTEXT_CANCELED: &str = "context canceled";
#[doc(hidden)]
pub const ERR_CONTEXT_DEADLINE_EXCEEDED: &str = "context deadline exceeded";
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    sentinel: &'static str,
    cause: &'static str,
    message: String,
}
impl Error {
    pub fn sentinel(sentinel: &'static str) -> Error {
        Error {
            sentinel,
            cause: "",
            message: sentinel.to_string(),
        }
    }
    pub fn wrap(sentinel: &'static str, detail: impl fmt::Display) -> Error {
        Error {
            sentinel,
            cause: "",
            message: format!("{sentinel}: {detail}"),
        }
    }
    pub fn with_message(sentinel: &'static str, message: impl Into<String>) -> Error {
        Error {
            sentinel,
            cause: "",
            message: message.into(),
        }
    }
    pub fn wrap_err(sentinel: &'static str, cause: &Error) -> Error {
        Error {
            sentinel,
            cause: cause.sentinel,
            message: format!("{sentinel}: {cause}"),
        }
    }
    pub fn msg(message: impl Into<String>) -> Error {
        Error {
            sentinel: "",
            cause: "",
            message: message.into(),
        }
    }
    pub fn is(&self, sentinel: &str) -> bool {
        !sentinel.is_empty() && (self.sentinel == sentinel || self.cause == sentinel)
    }
    pub(crate) fn sentinel_text(&self) -> &'static str {
        self.sentinel
    }
    pub fn message(&self) -> &str {
        &self.message
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for Error {}
impl From<crate::render::RenderError> for Error {
    fn from(e: crate::render::RenderError) -> Error {
        use crate::render::ErrorKind;
        let sentinel = match e.kind() {
            ErrorKind::Canceled => ERR_CONTEXT_CANCELED,
            ErrorKind::DeadlineExceeded => ERR_CONTEXT_DEADLINE_EXCEEDED,
            _ => "",
        };
        Error {
            sentinel,
            cause: "",
            message: e.to_string(),
        }
    }
}
pub fn ctx_err(ctx: Option<&dyn Cancellation>) -> Option<Error> {
    ctx.and_then(|c| c.err()).map(Error::from)
}
