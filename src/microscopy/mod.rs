#![forbid(unsafe_code)]
use crate::derivedio;
use crate::fmath;
use crate::object::{File, Object, Tag, Value, Vr};
use std::fmt;
mod annotation_render;
mod annotations;
mod color;
mod concatenation;
mod image;
mod loader;
mod mosaic;
mod rect;
mod source;
mod tiles;
mod viewport;
mod webclient;
pub use annotation_render::{AnnotationRenderOptions, render_annotations};
pub use annotations::{
    AnnotationGroup, AnnotationMeasurement, AnnotationPoint, CodedConcept, Primitive,
    read_annotations,
};
pub use color::{
    COLOR_CONVERTED, COLOR_IDENTITY, COLOR_INVALID_PROFILE, COLOR_MISSING_PATH,
    COLOR_MISSING_PROFILE, COLOR_RESOURCE_LIMITED, COLOR_UNKNOWN_PATH, COLOR_UNSUPPORTED_PROFILE,
    ColorManagedTileSource, ColorResolver, ColorResult, MAX_COLOR_PROFILE_BYTES, MAX_COLOR_SOURCES,
};
use concatenation::{SlideConcatenation, SlideTileMember, read_slide_concatenation};
pub use image::{TileFormat, TileImage};
pub use loader::{CacheStats, Loader, RequestOutcome, TileResult};
pub use mosaic::{
    MosaicError, MosaicKind, MosaicLayout, MosaicLink, MosaicOptions, MosaicPlacement,
    MosaicRegion, MosaicRegionOptions, MosaicTile, compose_mosaic_region, mosaic_bounds,
    register_mosaic,
};
pub use rect::Rect;
pub use source::{
    DicomwebFrameClient, DicomwebTileSource, FileOpener, LocalTileSource, TileSource, TileSourceFn,
};
pub use viewport::{
    Channel, NrgbaImage, RgbaImage, ScaleBar, TileLayer, Viewport, compose_viewport,
    compose_viewport_to_size, rotate_viewport,
};
pub use webclient::DicomwebClientFrames;
pub const VL_WHOLE_SLIDE_MICROSCOPY_IMAGE_STORAGE: &str = "1.2.840.10008.5.1.4.1.1.77.1.6";
pub const CONFOCAL_MICROSCOPY_TILED_PYRAMIDAL_STORAGE: &str = "1.2.840.10008.5.1.4.1.1.77.1.9";
pub const MICROSCOPY_BULK_SIMPLE_ANNOTATIONS_STORAGE: &str = "1.2.840.10008.5.1.4.1.1.91.1";
pub const MAX_OPTICAL_PATHS: usize = 64;
pub const MAX_OPTICAL_PATH_PROFILE_BYTES: usize = 1 << 20;
pub const MAX_INSTANCE_PROFILE_BYTES: usize = 8 << 20;
const MAX_MICROSCOPY_FRAMES: i64 = 100_000_000;
const DEFAULT_OPTICAL_PATH_IDENTIFIER: &str = "1";
const fn tag(group: u16, element: u16) -> Tag {
    Tag { group, element }
}
const TAG_ICC_PROFILE: Tag = tag(0x0028, 0x2000);
const TAG_CONCATENATION_FRAME_OFFSET: Tag = tag(0x0020, 0x9228);
const TAG_DIMENSION_ORGANIZATION_TYPE: Tag = tag(0x0020, 0x9311);
const TAG_IMAGE_TYPE: Tag = tag(0x0008, 0x0008);
const TAG_SOP_CLASS_UID: Tag = tag(0x0008, 0x0016);
const TAG_SOP_INSTANCE_UID: Tag = tag(0x0008, 0x0018);
const TAG_STUDY_INSTANCE_UID: Tag = tag(0x0020, 0x000D);
const TAG_SERIES_INSTANCE_UID: Tag = tag(0x0020, 0x000E);
const TAG_SAMPLES_PER_PIXEL: Tag = tag(0x0028, 0x0002);
const TAG_PHOTOMETRIC_INTERPRETATION: Tag = tag(0x0028, 0x0004);
const TAG_ROWS: Tag = tag(0x0028, 0x0010);
const TAG_COLUMNS: Tag = tag(0x0028, 0x0011);
const TAG_NUMBER_OF_FRAMES: Tag = tag(0x0028, 0x0008);
const TAG_PIXEL_SPACING: Tag = tag(0x0028, 0x0030);
const TAG_SHARED_FUNCTIONAL_GROUPS: Tag = tag(0x5200, 0x9229);
const TAG_PER_FRAME_FUNCTIONAL_GROUPS: Tag = tag(0x5200, 0x9230);
const TAG_PIXEL_MEASURES_SEQUENCE: Tag = tag(0x0028, 0x9110);
const TAG_SPACING_BETWEEN_SLICES: Tag = tag(0x0018, 0x0088);
const TAG_TOTAL_PIXEL_MATRIX_COLUMNS: Tag = tag(0x0048, 0x0006);
const TAG_TOTAL_PIXEL_MATRIX_ROWS: Tag = tag(0x0048, 0x0007);
const TAG_TOTAL_PIXEL_MATRIX_ORIGIN_SEQUENCE: Tag = tag(0x0048, 0x0008);
const TAG_IMAGED_VOLUME_WIDTH: Tag = tag(0x0048, 0x0001);
const TAG_IMAGED_VOLUME_HEIGHT: Tag = tag(0x0048, 0x0002);
const TAG_IMAGED_VOLUME_DEPTH: Tag = tag(0x0048, 0x0003);
const TAG_IMAGE_ORIENTATION_SLIDE: Tag = tag(0x0048, 0x0102);
const TAG_OPTICAL_PATH_SEQUENCE: Tag = tag(0x0048, 0x0105);
const TAG_OPTICAL_PATH_IDENTIFIER: Tag = tag(0x0048, 0x0106);
const TAG_OPTICAL_PATH_DESCRIPTION: Tag = tag(0x0048, 0x0107);
const TAG_OPTICAL_PATH_IDENTIFICATION_SEQUENCE: Tag = tag(0x0048, 0x0207);
const TAG_PLANE_POSITION_SLIDE_SEQUENCE: Tag = tag(0x0048, 0x021A);
const TAG_COLUMN_POSITION_TOTAL_MATRIX: Tag = tag(0x0048, 0x021E);
const TAG_ROW_POSITION_TOTAL_MATRIX: Tag = tag(0x0048, 0x021F);
const TAG_NUMBER_OF_OPTICAL_PATHS: Tag = tag(0x0048, 0x0302);
const TAG_TOTAL_PIXEL_MATRIX_FOCAL_PLANES: Tag = tag(0x0048, 0x0303);
const TAG_X_OFFSET_SLIDE: Tag = tag(0x0040, 0x072A);
const TAG_Y_OFFSET_SLIDE: Tag = tag(0x0040, 0x073A);
const TAG_Z_OFFSET_SLIDE: Tag = tag(0x0040, 0x074A);
const TAG_CONTAINER_IDENTIFIER: Tag = tag(0x0040, 0x0512);
const TAG_SPECIMEN_IDENTIFIER: Tag = tag(0x0040, 0x0551);
const TAG_SPECIMEN_DESCRIPTION_SEQUENCE: Tag = tag(0x0040, 0x0560);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    NotTiledMicroscopy,
    InvalidSlide,
    UncalibratedSlide,
    ColorResourceLimit,
    MissingOpticalPath,
    UnknownOpticalPath,
    Other,
}
impl Kind {
    pub fn text(self) -> &'static str {
        match self {
            Kind::NotTiledMicroscopy => "dicom/microscopy: not a tiled microscopy image",
            Kind::InvalidSlide => "dicom/microscopy: invalid slide metadata",
            Kind::UncalibratedSlide => "dicom/microscopy: slide coordinates are not calibrated",
            Kind::ColorResourceLimit => "dicom/microscopy: color metadata resource limit",
            Kind::MissingOpticalPath => "dicom/microscopy: frame optical path unavailable",
            Kind::UnknownOpticalPath => "dicom/microscopy: unknown optical path",
            Kind::Other => "dicom/microscopy: error",
        }
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ColorDiagnostic {
    pub source: InstanceRef,
    pub optical_path: String,
    pub state: &'static str,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub kind: Kind,
    text: String,
    pub diagnostic: Option<Box<ColorDiagnostic>>,
    cause: Option<Cause>,
}
impl Error {
    pub(crate) fn bare(kind: Kind) -> Error {
        Error {
            kind,
            text: kind.text().to_string(),
            diagnostic: None,
            cause: None,
        }
    }
    pub(crate) fn invalid(detail: impl fmt::Display) -> Error {
        Error {
            kind: Kind::InvalidSlide,
            text: format!("{}: {detail}", Kind::InvalidSlide.text()),
            diagnostic: None,
            cause: None,
        }
    }
    pub(crate) fn other(text: impl Into<String>) -> Error {
        Error {
            kind: Kind::Other,
            text: text.into(),
            diagnostic: None,
            cause: None,
        }
    }
    pub(crate) fn prefixed(mut self, prefix: impl fmt::Display) -> Error {
        self.text = format!("{prefix}: {}", self.text);
        self
    }
    fn path(source: &InstanceRef, optical_path: &str, kind: Kind) -> Error {
        let state = if kind == Kind::UnknownOpticalPath {
            "unknown-path"
        } else {
            "missing-path"
        };
        Error {
            kind,
            text: format!(
                "optical path {}: {state}",
                crate::textfmt::quote(optical_path)
            ),
            diagnostic: Some(Box::new(ColorDiagnostic {
                source: source.clone(),
                optical_path: optical_path.to_string(),
                state,
            })),
            cause: None,
        }
    }
    pub fn is(&self, kind: Kind) -> bool {
        self.kind == kind
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}
impl std::error::Error for Error {}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cause {
    MissingOpticalPathProfile,
    LoaderClosed,
    UnsupportedFragmentLayout,
    Canceled,
    IccCacheResourceLimit,
    IccUnsupportedProfile,
    Icc,
    Pixel,
}
impl Error {
    pub fn message(text: impl Into<String>) -> Error {
        Error::other(text)
    }
    pub(crate) fn with_cause(kind: Kind, cause: Option<Cause>, text: impl Into<String>) -> Error {
        Error {
            kind,
            text: text.into(),
            diagnostic: None,
            cause,
        }
    }
    pub(crate) fn with_diagnostic(mut self, diagnostic: ColorDiagnostic) -> Error {
        self.diagnostic = Some(Box::new(diagnostic));
        self
    }
    pub fn cause(&self) -> Option<Cause> {
        self.cause
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct InstanceRef {
    pub study_instance_uid: String,
    pub series_instance_uid: String,
    pub sop_instance_uid: String,
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SlidePoint {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OpticalPath {
    pub icc_profile: Vec<u8>,
    pub identifier: String,
    pub description: String,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Specimen {
    pub identifier: String,
    pub container_identifier: String,
    pub description: String,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Tile {
    pub source: InstanceRef,
    pub frame_number: i64,
    pub row: i64,
    pub column: i64,
    pub width: i64,
    pub height: i64,
    pub optical_path: String,
    pub focal_plane: i64,
    pub z_offset_millimeter: f64,
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TileKey {
    pub source: InstanceRef,
    pub frame_number: i64,
    pub optical_path: String,
    pub focal_plane: i64,
}
impl Tile {
    pub fn bounds(&self) -> Rect {
        Rect::new(
            self.column,
            self.row,
            self.column.wrapping_add(self.width),
            self.row.wrapping_add(self.height),
        )
    }
    pub fn key(&self) -> TileKey {
        TileKey {
            source: self.source.clone(),
            frame_number: self.frame_number,
            optical_path: self.optical_path.clone(),
            focal_plane: self.focal_plane,
        }
    }
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Level {
    pub frame_offset: i64,
    pub id: String,
    pub matrix_width: i64,
    pub matrix_height: i64,
    pub tile_width: i64,
    pub tile_height: i64,
    pub pixel_spacing_x: f64,
    pub pixel_spacing_y: f64,
    pub origin: SlidePoint,
    pub orientation: [f64; 6],
    pub imaged_volume_depth: f64,
    pub tiles: Vec<Tile>,
    pub source: InstanceRef,
    pub frame_count: i64,
    pub implicit_tiling: bool,
    pub optical_path_ids: Vec<String>,
    pub focal_plane_offsets: Vec<f64>,
    concatenation: SlideConcatenation,
    members: Vec<SlideTileMember>,
}
fn finite_positive(value: f64) -> bool {
    value > 0.0 && value.is_finite()
}
fn ceil_div(value: i64, divisor: i64) -> i64 {
    let mut quotient = value / divisor;
    if value % divisor > 0 {
        quotient += 1;
    }
    quotient
}
fn squared_distance(bounds: Rect, x: i64, y: i64) -> i64 {
    let dx = bounds.min_x.wrapping_add(bounds.max_x) / 2 - x;
    let dy = bounds.min_y.wrapping_add(bounds.max_y) / 2 - y;
    dx.wrapping_mul(dx).wrapping_add(dy.wrapping_mul(dy))
}
impl Level {
    pub fn matrix_bounds(&self) -> Rect {
        Rect::new(0, 0, self.matrix_width, self.matrix_height)
    }
    pub fn calibrated(&self) -> bool {
        finite_positive(self.pixel_spacing_x) && finite_positive(self.pixel_spacing_y)
    }
    fn axes(&self) -> ([f64; 3], [f64; 3]) {
        let o = &self.orientation;
        let (row, column) = ([o[0], o[1], o[2]], [o[3], o[4], o[5]]);
        if row == [0.0; 3] || column == [0.0; 3] {
            return ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
        }
        (row, column)
    }
    pub fn slide_coordinate(
        &self,
        pixel_x: i64,
        pixel_y: i64,
        z_millimeter: f64,
    ) -> Result<SlidePoint, Error> {
        if !self.calibrated() {
            return Err(Error::bare(Kind::UncalibratedSlide));
        }
        let (row, column) = self.axes();
        let x_distance = pixel_x as f64 * self.pixel_spacing_x;
        let y_distance = pixel_y as f64 * self.pixel_spacing_y;
        Ok(SlidePoint {
            x: self.origin.x + row[0] * x_distance + column[0] * y_distance,
            y: self.origin.y + row[1] * x_distance + column[1] * y_distance,
            z: self.origin.z + row[2] * x_distance + column[2] * y_distance + z_millimeter,
        })
    }
    pub fn pixel_coordinate(&self, point: SlidePoint) -> Result<(f64, f64), Error> {
        if !self.calibrated() {
            return Err(Error::bare(Kind::UncalibratedSlide));
        }
        let (row, column) = self.axes();
        let delta = [
            point.x - self.origin.x,
            point.y - self.origin.y,
            point.z - self.origin.z,
        ];
        let x = (delta[0] * row[0] + delta[1] * row[1] + delta[2] * row[2]) / self.pixel_spacing_x;
        let y = (delta[0] * column[0] + delta[1] * column[1] + delta[2] * column[2])
            / self.pixel_spacing_y;
        Ok((x, y))
    }
    pub fn distance(&self, a: (i64, i64), b: (i64, i64)) -> Result<f64, Error> {
        let first = self.slide_coordinate(a.0, a.1, 0.0)?;
        let second = self.slide_coordinate(b.0, b.1, 0.0)?;
        Ok(((second.x - first.x) * (second.x - first.x)
            + (second.y - first.y) * (second.y - first.y)
            + (second.z - first.z) * (second.z - first.z))
            .sqrt())
    }
    pub fn tiles_for_viewport(
        &self,
        viewport: Rect,
        optical_path: &str,
        focal_plane: i64,
    ) -> Vec<Tile> {
        self.tiles_for_viewport_limit(viewport, optical_path, focal_plane, 0)
    }
    pub fn tiles_for_viewport_limit(
        &self,
        viewport: Rect,
        optical_path: &str,
        focal_plane: i64,
        limit: i64,
    ) -> Vec<Tile> {
        let matches = |path: &str| optical_path.is_empty() || path == optical_path;
        self.select_tiles(viewport, focal_plane, limit, &matches, false)
    }
    pub(crate) fn select_tiles(
        &self,
        viewport: Rect,
        focal_plane: i64,
        limit: i64,
        matches_path: &dyn Fn(&str) -> bool,
        deduplicate: bool,
    ) -> Vec<Tile> {
        let viewport = viewport.intersect(self.matrix_bounds());
        if viewport.is_empty() {
            return Vec::new();
        }
        if self.implicit_tiling {
            return self.implicit_tiles(viewport, focal_plane, limit, matches_path);
        }
        let mut out: Vec<Tile> = self
            .tiles
            .iter()
            .filter(|tile| {
                matches_path(&tile.optical_path)
                    && (focal_plane < 0 || tile.focal_plane == focal_plane)
                    && tile.bounds().overlaps(viewport)
            })
            .cloned()
            .collect();
        let (cx, cy) = viewport.center();
        out.sort_by_key(|tile| squared_distance(tile.bounds(), cx, cy));
        if deduplicate {
            return deduplicate_tiles(out, limit);
        }
        if limit > 0 && out.len() as i64 > limit {
            out.truncate(limit as usize);
        }
        out
    }
    fn implicit_tiles(
        &self,
        viewport: Rect,
        focal_plane: i64,
        limit: i64,
        matches_path: &dyn Fn(&str) -> bool,
    ) -> Vec<Tile> {
        if self.frame_count <= 0 || self.tile_width <= 0 || self.tile_height <= 0 {
            return Vec::new();
        }
        let tiles_across = ceil_div(self.matrix_width, self.tile_width);
        let tiles_down = ceil_div(self.matrix_height, self.tile_height);
        let tiles_per_plane_path = tiles_across * tiles_down;
        let default_paths = [DEFAULT_OPTICAL_PATH_IDENTIFIER.to_string()];
        let path_ids: &[String] = if self.optical_path_ids.is_empty() {
            &default_paths
        } else {
            &self.optical_path_ids
        };
        let path_indices: Vec<usize> = (0..path_ids.len())
            .filter(|&index| matches_path(&path_ids[index]))
            .collect();
        if path_indices.is_empty() {
            return Vec::new();
        }
        let plane_count = (self.focal_plane_offsets.len() as i64).max(1);
        let (mut first_plane, mut last_plane) = (0, plane_count - 1);
        if focal_plane >= 0 {
            if focal_plane >= plane_count {
                return Vec::new();
            }
            (first_plane, last_plane) = (focal_plane, focal_plane);
        }
        let first_column = (viewport.min_x / self.tile_width).max(0);
        let last_column = (tiles_across - 1).min((viewport.max_x - 1) / self.tile_width);
        let first_row = (viewport.min_y / self.tile_height).max(0);
        let last_row = (tiles_down - 1).min((viewport.max_y - 1) / self.tile_height);
        let mut out = Vec::new();
        let append = |out: &mut Vec<Tile>, row_index: i64, column_index: i64| -> bool {
            for plane in first_plane..=last_plane {
                for &path_index in &path_indices {
                    let ordinal = path_index as i64 * tiles_per_plane_path * plane_count
                        + plane * tiles_per_plane_path
                        + row_index * tiles_across
                        + column_index;
                    let Some((source, frame_number)) = self.locate_tile_frame(ordinal) else {
                        continue;
                    };
                    let column = column_index * self.tile_width;
                    let row = row_index * self.tile_height;
                    let z = self
                        .focal_plane_offsets
                        .get(plane as usize)
                        .copied()
                        .unwrap_or(0.0);
                    out.push(Tile {
                        source,
                        frame_number,
                        row,
                        column,
                        width: self.tile_width.min(self.matrix_width - column),
                        height: self.tile_height.min(self.matrix_height - row),
                        optical_path: path_ids[path_index].clone(),
                        focal_plane: plane,
                        z_offset_millimeter: z,
                    });
                    if limit > 0 && out.len() as i64 >= limit {
                        return true;
                    }
                }
            }
            false
        };
        let (cx, cy) = viewport.center();
        if limit > 0 {
            let center_column = last_column.min(first_column.max(cx / self.tile_width));
            let center_row = last_row.min(first_row.max(cy / self.tile_height));
            let max_radius = (center_column - first_column)
                .max(last_column - center_column)
                .max((center_row - first_row).max(last_row - center_row));
            for radius in 0..=max_radius {
                for row_index in center_row - radius..=center_row + radius {
                    if row_index < first_row || row_index > last_row {
                        continue;
                    }
                    for column_index in center_column - radius..=center_column + radius {
                        let inside_ring = radius > 0
                            && row_index != center_row - radius
                            && row_index != center_row + radius
                            && column_index != center_column - radius
                            && column_index != center_column + radius;
                        if column_index < first_column || column_index > last_column || inside_ring
                        {
                            continue;
                        }
                        if append(&mut out, row_index, column_index) {
                            return out;
                        }
                    }
                }
            }
            return out;
        }
        for row_index in first_row..=last_row {
            for column_index in first_column..=last_column {
                append(&mut out, row_index, column_index);
            }
        }
        out.sort_by_key(|tile| squared_distance(tile.bounds(), cx, cy));
        out
    }
}
pub(crate) fn deduplicate_tiles(tiles: Vec<Tile>, limit: i64) -> Vec<Tile> {
    let limit = if limit <= 0 || limit as usize > tiles.len() {
        tiles.len()
    } else {
        limit as usize
    };
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::with_capacity(limit);
    for tile in tiles {
        if tile.frame_number <= 0 || !seen.insert(tile.key()) {
            continue;
        }
        out.push(tile);
        if out.len() == limit {
            break;
        }
    }
    out
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Instance {
    pub color_pixels: bool,
    pub level: Level,
    pub optical_paths: Vec<OpticalPath>,
    pub focal_planes: Vec<f64>,
    pub specimen: Specimen,
    pub dimension_order: Vec<String>,
}
impl Instance {
    pub fn frame_optical_path(&self, frame: i64) -> Result<String, Error> {
        let l = &self.level;
        if l.tile_width <= 0 || l.tile_height <= 0 || l.matrix_width <= 0 || l.matrix_height <= 0 {
            return Err(Error::bare(Kind::InvalidSlide));
        }
        if frame < 1 || frame > l.frame_count {
            return Err(Error::invalid(format!("frame {frame}")));
        }
        let missing = || Error::bare(Kind::MissingOpticalPath);
        if !l.implicit_tiling {
            return match l.tiles.get(frame as usize - 1) {
                Some(tile) if !tile.optical_path.is_empty() => Ok(tile.optical_path.clone()),
                _ => Err(missing()),
            };
        }
        if l.optical_path_ids.is_empty() {
            return Err(missing());
        }
        let count = ceil_div(l.matrix_width, l.tile_width)
            .wrapping_mul(ceil_div(l.matrix_height, l.tile_height))
            .wrapping_mul((l.focal_plane_offsets.len() as i64).max(1));
        if count <= 0 {
            return Err(Error::bare(Kind::InvalidSlide));
        }
        let index = (frame - 1 + l.frame_offset) / count;
        match l.optical_path_ids.get(index as usize) {
            Some(path) if !path.is_empty() => Ok(path.clone()),
            _ => Err(missing()),
        }
    }
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pyramid {
    pub levels: Vec<Level>,
    pub optical_paths: Vec<OpticalPath>,
    pub focal_planes: Vec<f64>,
    pub specimens: Vec<Specimen>,
}
fn level_before(left: &Level, right: &Level) -> bool {
    if left.calibrated() && right.calibrated() && left.pixel_spacing_x != right.pixel_spacing_x {
        return left.pixel_spacing_x < right.pixel_spacing_x;
    }
    left.matrix_width.wrapping_mul(left.matrix_height)
        > right.matrix_width.wrapping_mul(right.matrix_height)
}
impl Pyramid {
    pub fn add(&mut self, instance: &Instance) -> Result<(), Error> {
        let level = &instance.level;
        if level.matrix_width <= 0
            || level.matrix_height <= 0
            || level.tile_width <= 0
            || level.tile_height <= 0
        {
            return Err(Error::invalid("invalid pyramid level"));
        }
        self.add_level(level.clone())?;
        for path in &instance.optical_paths {
            let identifier = path.identifier.trim();
            if identifier.is_empty()
                || self
                    .optical_paths
                    .iter()
                    .any(|p| p.identifier == identifier)
            {
                continue;
            }
            self.optical_paths.push(OpticalPath {
                identifier: identifier.to_string(),
                ..path.clone()
            });
        }
        for &value in &instance.focal_planes {
            if !self
                .focal_planes
                .iter()
                .any(|current| (current - value).abs() <= 1e-9)
            {
                self.focal_planes.push(value);
            }
        }
        let specimen = &instance.specimen;
        if *specimen != Specimen::default()
            && !self.specimens.iter().any(|current| {
                current.identifier == specimen.identifier
                    && current.container_identifier == specimen.container_identifier
            })
        {
            self.specimens.push(specimen.clone());
        }
        if self.levels.len() <= 20 {
            for i in 1..self.levels.len() {
                let mut j = i;
                while j > 0 && level_before(&self.levels[j], &self.levels[j - 1]) {
                    self.levels.swap(j, j - 1);
                    j -= 1;
                }
            }
        } else {
            self.levels.sort_by(|a, b| {
                if level_before(a, b) {
                    std::cmp::Ordering::Less
                } else if level_before(b, a) {
                    std::cmp::Ordering::Greater
                } else {
                    std::cmp::Ordering::Equal
                }
            });
        }
        self.focal_planes.sort_by(f64::total_cmp);
        Ok(())
    }
    pub fn level_for_scale(&self, total_matrix_pixels_per_screen_pixel: f64) -> Option<&Level> {
        let base = self.levels.first()?;
        let scale = total_matrix_pixels_per_screen_pixel;
        if !finite_positive(scale) {
            return Some(base);
        }
        let mut best = base;
        let mut best_error = f64::INFINITY;
        for level in &self.levels {
            let ratio = base.matrix_width as f64 / level.matrix_width as f64;
            if !finite_positive(ratio) {
                continue;
            }
            let err = fmath::log2(ratio / scale).abs();
            if err < best_error || (err == best_error && ratio < scale) {
                (best, best_error) = (level, err);
            }
        }
        Some(best)
    }
}
pub fn read(file: &File) -> Result<Instance, Error> {
    let root = &file.dataset;
    let sop_class = derivedio::clean_uid(root, TAG_SOP_CLASS_UID);
    if sop_class != VL_WHOLE_SLIDE_MICROSCOPY_IMAGE_STORAGE
        && sop_class != CONFOCAL_MICROSCOPY_TILED_PYRAMIDAL_STORAGE
    {
        return Err(Error::bare(Kind::NotTiledMicroscopy));
    }
    let matrix_width = derivedio::int(root, TAG_TOTAL_PIXEL_MATRIX_COLUMNS);
    let matrix_height = derivedio::int(root, TAG_TOTAL_PIXEL_MATRIX_ROWS);
    let tile_width = derivedio::int(root, TAG_COLUMNS);
    let tile_height = derivedio::int(root, TAG_ROWS);
    let frame_count = tiles::slide_count(root, TAG_NUMBER_OF_FRAMES, MAX_MICROSCOPY_FRAMES)?;
    if matrix_width <= 0 || matrix_height <= 0 || tile_width <= 0 || tile_height <= 0 {
        return Err(Error::invalid(format!(
            "matrix={matrix_width}x{matrix_height} tile={tile_width}x{tile_height} frames={frame_count}"
        )));
    }
    let source = InstanceRef {
        study_instance_uid: derivedio::clean_uid(root, TAG_STUDY_INSTANCE_UID),
        series_instance_uid: derivedio::clean_uid(root, TAG_SERIES_INSTANCE_UID),
        sop_instance_uid: derivedio::clean_uid(root, TAG_SOP_INSTANCE_UID),
    };
    let (pixel_spacing_x, pixel_spacing_y) = slide_pixel_spacing(root, matrix_width, matrix_height);
    let origin = slide_origin(root)?;
    let mut optical_paths = read_optical_paths(root)?;
    if optical_paths.is_empty() {
        optical_paths.push(OpticalPath {
            identifier: DEFAULT_OPTICAL_PATH_IDENTIFIER.to_string(),
            ..OpticalPath::default()
        });
    }
    let concatenation = read_slide_concatenation(root)?;
    if concatenation.offset > MAX_MICROSCOPY_FRAMES - frame_count {
        return Err(Error::invalid("concatenation frame offset"));
    }
    let mut orientation = [0.0; 6];
    if let Some(values) = derivedio::floats(Some(root), TAG_IMAGE_ORIENTATION_SLIDE)
        && values.len() == 6
    {
        orientation.copy_from_slice(&values);
    }
    let mut level = Level {
        id: format!("{}:{matrix_width}x{matrix_height}", source.sop_instance_uid),
        matrix_width,
        matrix_height,
        tile_width,
        tile_height,
        pixel_spacing_x,
        pixel_spacing_y,
        origin,
        orientation,
        imaged_volume_depth: first_float(root, TAG_IMAGED_VOLUME_DEPTH),
        source,
        frame_offset: concatenation.offset,
        frame_count,
        concatenation,
        optical_path_ids: optical_paths.iter().map(|p| p.identifier.clone()).collect(),
        ..Level::default()
    };
    tiles::place_slide_tiles(root, &mut level)?;
    let photometric = derivedio::clean_string(root, TAG_PHOTOMETRIC_INTERPRETATION);
    Ok(Instance {
        color_pixels: derivedio::int(root, TAG_SAMPLES_PER_PIXEL) >= 3
            || photometric.eq_ignore_ascii_case("PALETTE COLOR"),
        focal_planes: level.focal_plane_offsets.clone(),
        level,
        optical_paths,
        specimen: read_specimen(root),
        dimension_order: Vec::new(),
    })
}
fn slide_pixel_spacing(root: &Object, matrix_width: i64, matrix_height: i64) -> (f64, f64) {
    let shared = derivedio::sequence(root, TAG_SHARED_FUNCTIONAL_GROUPS);
    let measures = shared
        .first()
        .and_then(|group| find_nested_sequence_item(group, TAG_PIXEL_MEASURES_SEQUENCE, 0));
    for source in [measures.as_ref(), Some(root)].into_iter().flatten() {
        if let Ok((row, column, values)) = source.spacing_pair(TAG_PIXEL_SPACING)
            && values != crate::object::geometry::spacing_values::ABSENT
        {
            return (column, row);
        }
    }
    let width = first_float(root, TAG_IMAGED_VOLUME_WIDTH);
    let height = first_float(root, TAG_IMAGED_VOLUME_HEIGHT);
    if finite_positive(width) && finite_positive(height) {
        return (width / matrix_width as f64, height / matrix_height as f64);
    }
    (0.0, 0.0)
}
fn slide_origin(root: &Object) -> Result<SlidePoint, Error> {
    let items = derivedio::sequence(root, TAG_TOTAL_PIXEL_MATRIX_ORIGIN_SEQUENCE);
    let Some(item) = items.first() else {
        return Ok(SlidePoint::default());
    };
    let z = tiles::slide_z(item, 0.0)?;
    Ok(SlidePoint {
        x: first_float(item, TAG_X_OFFSET_SLIDE),
        y: first_float(item, TAG_Y_OFFSET_SLIDE),
        z,
    })
}
fn read_optical_paths(root: &Object) -> Result<Vec<OpticalPath>, Error> {
    let items = derivedio::sequence(root, TAG_OPTICAL_PATH_SEQUENCE);
    if items.len() > MAX_OPTICAL_PATHS {
        return Err(Error::bare(Kind::ColorResourceLimit));
    }
    let mut out: Vec<OpticalPath> = Vec::with_capacity(items.len());
    let mut bytes = 0;
    for item in &items {
        let identifier = derivedio::clean_string(item, TAG_OPTICAL_PATH_IDENTIFIER);
        if identifier.len() > 64 {
            return Err(Error::bare(Kind::ColorResourceLimit));
        }
        if identifier.is_empty() || out.iter().any(|p| p.identifier == identifier) {
            return Err(Error::invalid(
                "missing or duplicate optical path identifier",
            ));
        }
        let data: &[u8] = match item.get(TAG_ICC_PROFILE).map(|e| &e.value) {
            Some(Value::Raw(raw)) => raw,
            _ => &[],
        };
        if data.len() > MAX_OPTICAL_PATH_PROFILE_BYTES
            || data.len() > MAX_INSTANCE_PROFILE_BYTES - bytes
        {
            return Err(Error::bare(Kind::ColorResourceLimit));
        }
        bytes += data.len();
        out.push(OpticalPath {
            icc_profile: data.to_vec(),
            identifier,
            description: derivedio::clean_string(item, TAG_OPTICAL_PATH_DESCRIPTION),
        });
    }
    Ok(out)
}
fn read_specimen(root: &Object) -> Specimen {
    let mut specimen = Specimen {
        identifier: derivedio::clean_string(root, TAG_SPECIMEN_IDENTIFIER),
        container_identifier: derivedio::clean_string(root, TAG_CONTAINER_IDENTIFIER),
        description: String::new(),
    };
    if specimen.identifier.is_empty()
        && let Some(item) = derivedio::sequence(root, TAG_SPECIMEN_DESCRIPTION_SEQUENCE).first()
    {
        specimen.identifier = derivedio::clean_string(item, TAG_SPECIMEN_IDENTIFIER);
    }
    specimen
}
fn find_nested_sequence_item(root: &Object, target: Tag, depth: usize) -> Option<Object> {
    if depth > 5 {
        return None;
    }
    if let Some(item) = derivedio::sequence(root, target).into_iter().next() {
        return Some(item);
    }
    for element in root.elements() {
        if element.vr() != Vr::SQ {
            continue;
        }
        for item in root.get_sequence(element.tag()).iter().flatten() {
            if let Some(found) = find_nested_sequence_item(item, target, depth + 1) {
                return Some(found);
            }
        }
    }
    None
}
fn first_float(root: &Object, tag: Tag) -> f64 {
    match derivedio::floats(Some(root), tag).as_deref() {
        Some([first, ..]) if first.is_finite() => *first,
        _ => 0.0,
    }
}
